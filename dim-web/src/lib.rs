#![deny(warnings)]

use std::collections::{HashMap, VecDeque};
use std::future::IntoFuture;
use std::net::IpAddr;
use std::net::SocketAddr;
use std::sync::{Arc, Mutex};
use std::time::Instant;

pub mod routes;
pub mod tree;

pub use axum;
use axum::extract::{ConnectInfo, State};
use axum::http::StatusCode;
use axum::response::Response;
use axum::routing::{delete, get, patch, post};
use axum::Router;

use dim_core::core::EventTx;
use dim_core::settings::SettingsStore;
use dim_core::stream_tracking::StreamTracking;
use dim_core::workers::LibraryWorkers;
use dim_database::DbConnection;

use futures::{Future, SinkExt, StreamExt};
use nightfall::StateManager;
use tokio::sync::mpsc::Receiver;

pub mod error;
pub use error::DimErrorWrapper;

pub mod middleware;
pub use middleware::verify_cookie_token;

#[derive(Debug, Clone)]
pub struct AppState {
    conn: DbConnection,
    socket_tx: routes::websocket::EventSocketTx,
    event_tx: EventTx,
    state: StateManager,
    stream_tracking: StreamTracking,
    library_workers: LibraryWorkers,
    settings: SettingsStore,
    login_failures: Arc<Mutex<HashMap<IpAddr, VecDeque<Instant>>>>,
}

impl AppState {
    pub fn new(
        conn: DbConnection,
        socket_tx: routes::websocket::EventSocketTx,
        event_tx: EventTx,
        state: StateManager,
        stream_tracking: StreamTracking,
        settings: SettingsStore,
    ) -> Self {
        Self {
            conn,
            socket_tx,
            event_tx,
            state,
            stream_tracking,
            library_workers: LibraryWorkers::default(),
            settings,
            login_failures: Arc::new(Mutex::new(HashMap::new())),
        }
    }

    pub fn login_allowed(&self, remote: IpAddr) -> bool {
        if self
            .settings
            .running()
            .bind_address
            .parse::<IpAddr>()
            .map(|address| address.is_loopback())
            .unwrap_or(true)
        {
            return true;
        }
        let now = Instant::now();
        let mut failures = self
            .login_failures
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner());
        let attempts = failures.entry(remote).or_default();
        while attempts
            .front()
            .map(|then| now.duration_since(*then).as_secs() >= 60)
            .unwrap_or(false)
        {
            attempts.pop_front();
        }
        attempts.len() < self.settings.running().login_attempts_per_minute as usize
    }

    pub fn record_login_failure(&self, remote: IpAddr) {
        self.login_failures
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner())
            .entry(remote)
            .or_default()
            .push_back(Instant::now());
    }

    pub fn clear_login_failures(&self, remote: IpAddr) {
        self.login_failures
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner())
            .remove(&remote);
    }

    pub fn with_library_workers(mut self, library_workers: LibraryWorkers) -> Self {
        self.library_workers = library_workers;
        self
    }
}

fn library_routes(_app: AppState) -> Router<AppState> {
    Router::new()
        .route(
            "/api/v1/library",
            post(routes::library::library_post).get(routes::library::library_get_all),
        )
        .route(
            "/api/v1/library/:id/media",
            get(routes::library::library_get_media),
        )
        .route(
            "/api/v1/library/:id",
            get(routes::library::library_get_one).delete(routes::library::library_delete),
        )
        .route(
            "/api/v1/library/:id/unmatched",
            get(routes::library::library_get_unmatched),
        )
        .route(
            "/api/v1/library/:id/scan",
            get(routes::library::library_scan_status).post(routes::library::library_scan_retry),
        )
}

fn auth_routes(AppState { .. }: AppState) -> Router<AppState> {
    Router::new()
        .route("/api/v1/auth/login", post(routes::auth::login))
        .route("/api/v1/auth/register", post(routes::auth::register))
        .route("/api/v1/auth/admin_exists", get(routes::auth::admin_exists))
}

fn media_routes(AppState { .. }: AppState) -> Router<AppState> {
    Router::new()
        .route(
            "/api/v1/media/:id",
            get(routes::media::get_media_by_id)
                .patch(routes::media::update_media_by_id)
                .delete(routes::media::delete_media_by_id),
        )
        .route(
            "/api/v1/media/:id/files",
            get(routes::media::get_media_files),
        )
        .route(
            "/api/v1/media/:id/tree",
            get(routes::media::get_mediafile_tree),
        )
        .route(
            "/api/v1/media/:id/progress",
            post(routes::media::map_progress),
        )
        .route("/api/v1/media/tmdb_search", get(routes::media::tmdb_search))
        .route(
            "/api/v1/media/:id/rematch",
            post(routes::media::rematch_media_by_id),
        )
}

fn stream_routes(
    AppState {
        conn: _conn,
        state: _state,
        stream_tracking: _stream_tracking,
        ..
    }: AppState,
) -> Router<AppState> {
    Router::new()
        .route(
            "/api/v1/stream/:id/capabilities",
            get(routes::stream::return_playback_capabilities),
        )
        .route(
            "/api/v1/stream/:id/manifest",
            get(routes::stream::return_virtual_manifest),
        )
        .route(
            "/api/v1/stream/:gid/manifest.mpd",
            get(routes::stream::return_manifest),
        )
        .route(
            "/api/v1/stream/:id/data/init.mp4",
            get(routes::stream::get_init),
        )
        .route(
            "/api/v1/stream/:id/data/*chunk",
            get(routes::stream::get_chunk),
        )
        .route(
            "/api/v1/stream/:id/data/stream.vtt",
            get(routes::stream::get_subtitle),
        )
        .route(
            "/api/v1/stream/:id/data/stream.ass",
            get(routes::stream::get_subtitle_ass),
        )
        .route(
            "/api/v1/stream/:gid/state/should_hard_seek/:chunk_num",
            get(routes::stream::should_client_hard_seek),
        )
        .route(
            "/api/v1/stream/:gid/state/get_stderr",
            get(routes::stream::session_get_stderr),
        )
        .route(
            "/api/v1/stream/:gid/state/kill",
            delete(routes::stream::kill_session),
        )
}

fn remote_stream_routes() -> Router<AppState> {
    Router::new()
        .route(
            "/api/v1/remote/:gid/master.m3u8",
            get(routes::stream::return_remote_master),
        )
        .route(
            "/api/v1/remote/:gid/:track/index.m3u8",
            get(routes::stream::return_remote_media_playlist),
        )
        .route(
            "/api/v1/remote/:gid/:track/init.mp4",
            get(routes::stream::get_remote_init),
        )
        .route(
            "/api/v1/remote/:gid/:track/:chunk",
            get(routes::stream::get_remote_chunk),
        )
}

fn season_routes(_app: AppState) -> Router<AppState> {
    Router::new()
        .route(
            "/api/v1/season/:id",
            get(routes::tv::get_season_by_id)
                .patch(routes::tv::patch_season_by_id)
                .delete(routes::tv::delete_season_by_id),
        )
        .route(
            "/api/v1/season/:id/episodes",
            get(routes::tv::get_season_episodes),
        )
        .route(
            "/api/v1/season/:id/episode/:episode_id",
            patch(routes::tv::patch_episode_by_id).delete(routes::tv::delete_episode_by_id),
        )
}

fn settings_routes(AppState { .. }: AppState) -> Router<AppState> {
    Router::new()
        .route(
            "/api/v1/user/settings",
            get(routes::settings::get_user_settings),
        )
        .route(
            "/api/v1/user/settings",
            post(routes::settings::post_user_settings),
        )
}

async fn ws_handler(
    ws: axum::extract::WebSocketUpgrade,
    ConnectInfo(remote_address): ConnectInfo<SocketAddr>,
    State(AppState {
        conn, socket_tx, ..
    }): State<AppState>,
) -> Response {
    ws.on_upgrade(move |websocket| async move {
        let (ws_tx, ws_rx) = websocket.split();

        routes::websocket::handle_websocket_session(
            ws_tx.sink_err_into::<routes::websocket::WsMessageError>(),
            ws_rx.filter_map(|m| async move { m.ok() }),
            Some(remote_address),
            conn,
            socket_tx,
        )
        .await;
    })
}

pub fn build_router(app: AppState) -> Router {
    let protected = axum::Router::new()
        .route("/api/v1/auth/whoami", get(routes::auth::whoami))
        .route("/api/v1/auth/logout", post(routes::auth::logout))
        .merge(library_routes(app.clone()))
        .route("/api/v1/dashboard", get(routes::dashboard::dashboard))
        .route("/api/v1/dashboard/banner", get(routes::dashboard::banners))
        .route("/api/v1/search", get(routes::search::search))
        .route(
            "/api/v1/filebrowser",
            get(routes::filebrowser::get_directory_structure),
        )
        .merge(media_routes(app.clone()))
        .merge(stream_routes(app.clone()))
        .route(
            "/api/v1/mediafile/:id",
            get(routes::mediafile::get_mediafile_info),
        )
        .route(
            "/api/v1/mediafile/match",
            patch(routes::mediafile::rematch_mediafile),
        )
        .route("/api/v1/tv/:id/season", get(routes::tv::get_tv_seasons))
        .merge(season_routes(app.clone()))
        .route(
            "/api/v1/episode/:id",
            patch(routes::tv::patch_episode_by_id).delete(routes::tv::delete_episode_by_id),
        )
        .merge(settings_routes(app.clone()))
        .route(
            "/api/v1/host/settings",
            get(routes::settings::http_get_global_settings)
                .post(routes::settings::http_set_global_settings),
        )
        .route(
            "/api/v1/user/password",
            patch(routes::user::change_password),
        )
        .route("/api/v1/user", delete(routes::user::delete))
        .route(
            "/api/v1/user/username",
            patch(routes::user::change_username),
        )
        .route(
            "/api/v1/user/avatar",
            post(routes::user::upload_avatar).delete(routes::user::delete_avatar),
        )
        .route(
            "/api/v1/auth/invites",
            get(routes::auth::get_all_invites).post(routes::auth::generate_invite),
        )
        .route(
            "/api/v1/auth/token/:token",
            delete(routes::auth::delete_token),
        )
        .route_layer(axum::middleware::from_fn_with_state(
            app.conn.clone(),
            verify_cookie_token,
        ));

    let deployment_settings = app.settings.clone();
    axum::Router::new()
        .route("/health/live", get(|| async { StatusCode::OK }))
        .route("/health/ready", get(readiness))
        .merge(auth_routes(app.clone()))
        .merge(remote_stream_routes())
        .route("/images/*path", get(routes::statik::get_image))
        .route("/", get(routes::statik::react_routes))
        .route("/*path", get(routes::statik::react_routes))
        .route("/static/*path", get(routes::statik::dist_static))
        .route("/ws", get(ws_handler))
        .merge(protected)
        .with_state(app)
        .layer(axum::middleware::from_fn_with_state(
            deployment_settings,
            middleware::deployment_guard,
        ))
        .layer(axum::middleware::from_fn(middleware::request_id))
        .layer(tower_http::trace::TraceLayer::new_for_http())
}

async fn readiness(State(AppState { conn, .. }): State<AppState>) -> StatusCode {
    match sqlx::query("SELECT 1").execute(conn.read_ref()).await {
        Ok(_) => StatusCode::OK,
        Err(error) => {
            tracing::warn!(?error, "Readiness database probe failed");
            StatusCode::SERVICE_UNAVAILABLE
        }
    }
}

pub async fn start_webserver(
    address: SocketAddr,
    conn: DbConnection,
    settings: SettingsStore,
    event_tx: EventTx,
    stream_manager: StateManager,
    event_rx: Receiver<String>,
    library_workers: LibraryWorkers,
    shutdown_fut: impl Future<Output = ()> + Send + 'static,
) {
    let state = stream_manager;
    let stream_tracking = StreamTracking::default();
    let event_repeater = routes::websocket::event_repeater(
        tokio_stream::wrappers::ReceiverStream::new(event_rx),
        1024,
    );

    let socket_tx = event_repeater.sender();

    let event_repeater_handle = tokio::spawn(event_repeater.into_future());

    let cleanup_tracking = stream_tracking.clone();
    let cleanup_state = state.clone();
    let cleanup_handle = tokio::spawn(async move {
        let mut interval = tokio::time::interval(std::time::Duration::from_secs(30));
        interval.tick().await;
        loop {
            interval.tick().await;
            let cleaned = cleanup_tracking.cleanup_expired(&cleanup_state).await;
            if cleaned > 0 {
                tracing::info!(cleaned, "Expired inactive playback sessions");
            }
        }
    });

    let shutdown_tracking = stream_tracking.clone();
    let shutdown_state = state.clone();
    let app = AppState::new(conn, socket_tx, event_tx, state, stream_tracking, settings)
        .with_library_workers(library_workers);
    let router = build_router(app);

    tracing::info!(%address, "webserver is listening");

    let result = serve_router(address, router, shutdown_fut).await;
    if let Err(error) = result {
        tracing::error!(?error, "HTTP server stopped with an error");
    }

    cleanup_handle.abort();
    let _ = cleanup_handle.await;
    shutdown_tracking.shutdown(&shutdown_state).await;

    event_repeater_handle.abort();
    let _ = event_repeater_handle.await;
}

async fn serve_router(
    address: SocketAddr,
    router: Router,
    shutdown_fut: impl Future<Output = ()> + Send + 'static,
) -> Result<(), hyper::Error> {
    axum::Server::bind(&address)
        .serve(router.into_make_service_with_connect_info::<SocketAddr>())
        .with_graceful_shutdown(shutdown_fut)
        .await
}

#[cfg(test)]
mod shutdown_tests {
    use super::*;

    #[tokio::test]
    async fn http_server_accepts_owned_shutdown_signal() {
        let address = SocketAddr::from(([127, 0, 0, 1], 0));
        tokio::time::timeout(
            std::time::Duration::from_secs(2),
            serve_router(address, Router::new(), async {}),
        )
        .await
        .expect("server did not shut down")
        .unwrap();
    }

    #[tokio::test]
    async fn lan_login_throttle_is_conservative_and_loopback_scoped_by_configuration() {
        let directory = tempfile::tempdir().unwrap();
        let config = directory.path().join("config.toml");
        let store = SettingsStore::load(&config).unwrap();
        let mut lan = store.running().clone();
        lan.bind_address = "0.0.0.0".into();
        lan.login_attempts_per_minute = 2;
        store.save_for_restart(&lan).unwrap();
        let store = SettingsStore::load(&config).unwrap();
        let conn = dim_database::get_conn_memory().await.unwrap();
        let (socket_tx, _socket_rx) = tokio::sync::mpsc::channel(4);
        let (event_tx, _event_rx) = tokio::sync::mpsc::channel(4);
        let manager = nightfall::StateManager::new(
            &mut xtra::spawn::Tokio::Global,
            directory.path().to_string_lossy().into_owned(),
            "/bin/false".into(),
        );
        let app = AppState::new(
            conn,
            socket_tx,
            event_tx,
            manager,
            StreamTracking::default(),
            store,
        );
        let remote = "192.0.2.10".parse().unwrap();
        assert!(app.login_allowed(remote));
        app.record_login_failure(remote);
        assert!(app.login_allowed(remote));
        app.record_login_failure(remote);
        assert!(!app.login_allowed(remote));
        app.clear_login_failures(remote);
        assert!(app.login_allowed(remote));
    }
}
