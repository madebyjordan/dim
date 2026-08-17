use std::net::SocketAddr;
use std::path::PathBuf;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;

use axum::body::Body;
use axum::http::header::{AUTHORIZATION, CONTENT_TYPE};
use axum::http::{Method, Request, StatusCode};
use dim_core::stream_tracking::StreamTracking;
use dim_core::workers::{LibraryWorkerKind, LibraryWorkers};
use dim_database::user::{InsertableUser, Roles, UserSettings};
use dim_web::routes::websocket::CtrlEvent;
use dim_web::{build_router, AppState};
use hyper::body::to_bytes;
use percent_encoding::{utf8_percent_encode, NON_ALPHANUMERIC};
use serde_json::Value;
use tokio::sync::mpsc::Receiver;
use tower::ServiceExt;
use xtra::spawn::Tokio;

struct TestApp {
    router: axum::Router,
    conn: dim_database::DbConnection,
    event_rx: Receiver<String>,
    root: PathBuf,
    workers: LibraryWorkers,
}

async fn test_app() -> TestApp {
    dim_database::set_key([9; 32]);

    let root = std::env::temp_dir().join(format!("dim-library-flow-{}", uuid::Uuid::new_v4()));
    let metadata = root.join("metadata");
    let cache = root.join("cache");
    std::fs::create_dir_all(&metadata).unwrap();
    std::fs::create_dir_all(&cache).unwrap();

    let config_path = root.join("config.toml");
    dim_core::settings::init_global_settings(Some(config_path.to_string_lossy().into_owned()))
        .unwrap();
    dim_core::core::METADATA_PATH
        .set(metadata.to_string_lossy().into_owned())
        .unwrap();

    let db_path = root.join("dim.db");
    let conn = dim_database::get_conn_file(db_path.to_str().unwrap())
        .await
        .unwrap();
    let mut lock = conn.writer().lock_owned().await;
    let mut tx = dim_database::write_tx(&mut lock).await.unwrap();
    let invite = dim_database::user::Login::new_invite(&mut tx)
        .await
        .unwrap();
    InsertableUser {
        username: "owner".to_owned(),
        password: "password".to_owned(),
        roles: Roles(vec!["owner".to_owned()]),
        prefs: UserSettings::default(),
        claimed_invite: invite,
    }
    .insert(&mut tx)
    .await
    .unwrap();
    tx.commit().await.unwrap();
    drop(lock);

    let (socket_tx, _socket_rx) = tokio::sync::mpsc::channel::<CtrlEvent<SocketAddr, String>>(16);
    let (event_tx, event_rx) = tokio::sync::mpsc::channel(128);
    let state = nightfall::StateManager::new(
        &mut Tokio::Global,
        cache.to_string_lossy().into_owned(),
        "/bin/false".to_owned(),
    );
    let workers = LibraryWorkers::default();
    let settings = dim_core::settings::SettingsStore::load(&config_path).unwrap();
    let app = AppState::new(
        conn.clone(),
        socket_tx,
        event_tx,
        state,
        StreamTracking::default(),
        settings,
    )
    .with_library_workers(workers.clone());

    TestApp {
        router: build_router(app),
        conn,
        event_rx,
        root,
        workers,
    }
}

fn request(method: Method, uri: &str, token: Option<&str>, body: &str) -> Request<Body> {
    let mut builder = Request::builder().method(method).uri(uri);
    if let Some(token) = token {
        builder = builder.header(AUTHORIZATION, token);
    }
    if !body.is_empty() {
        builder = builder.header(CONTENT_TYPE, "application/json");
    }
    builder.body(Body::from(body.to_owned())).unwrap()
}

async fn json_body(response: axum::response::Response) -> Value {
    let body = to_bytes(response.into_body()).await.unwrap();
    serde_json::from_slice(&body).unwrap()
}

#[tokio::test]
async fn owner_can_browse_create_and_complete_an_initial_scan() {
    let mut test = test_app().await;
    let media = test.root.join("Media").join("Movies & Family");
    std::fs::create_dir_all(&media).unwrap();
    // An unsupported file exercises a complete scan without an external metadata request.
    std::fs::write(media.join("README.txt"), b"media goes here").unwrap();

    let response = test
        .router
        .clone()
        .oneshot(request(
            Method::POST,
            "/api/v1/auth/login",
            None,
            r#"{"username":"owner","password":"password"}"#,
        ))
        .await
        .unwrap();
    assert_eq!(response.status(), StatusCode::OK);
    let login = json_body(response).await;
    let owner_token = login["token"].as_str().unwrap().to_owned();

    let response = test
        .router
        .clone()
        .oneshot(request(
            Method::GET,
            "/api/v1/filebrowser",
            Some(&owner_token),
            "",
        ))
        .await
        .unwrap();
    assert_eq!(response.status(), StatusCode::OK);
    let default_listing = json_body(response).await;
    if let Some(home) = std::env::var_os("HOME").map(PathBuf::from) {
        if home.is_dir() && std::fs::read_dir(&home).is_ok() {
            assert_eq!(
                default_listing["current"],
                std::fs::canonicalize(home)
                    .unwrap()
                    .to_string_lossy()
                    .as_ref()
            );
        }
    }

    let encoded = utf8_percent_encode(&media.to_string_lossy(), NON_ALPHANUMERIC).to_string();
    let response = test
        .router
        .clone()
        .oneshot(request(
            Method::GET,
            &format!("/api/v1/filebrowser?path={encoded}"),
            Some(&owner_token),
            "",
        ))
        .await
        .unwrap();
    assert_eq!(response.status(), StatusCode::OK);
    let listing = json_body(response).await;
    assert_eq!(
        listing["current"],
        std::fs::canonicalize(&media)
            .unwrap()
            .to_string_lossy()
            .as_ref()
    );

    let relative_response = test
        .router
        .clone()
        .oneshot(request(
            Method::GET,
            "/api/v1/filebrowser?path=relative",
            Some(&owner_token),
            "",
        ))
        .await
        .unwrap();
    assert_eq!(relative_response.status(), StatusCode::BAD_REQUEST);

    let body = serde_json::json!({
        "name": "Family Movies",
        "locations": [media.to_string_lossy()],
        "media_type": "movie"
    })
    .to_string();
    let response = test
        .router
        .clone()
        .oneshot(request(
            Method::POST,
            "/api/v1/library",
            Some(&owner_token),
            &body,
        ))
        .await
        .unwrap();
    assert_eq!(response.status(), StatusCode::OK);
    let created = json_body(response).await;
    let id = created["id"].as_i64().unwrap();
    assert_eq!(created["scan_status"], "scanning");

    let mut scan_started = false;
    let mut scan_stopped = false;
    while !scan_stopped {
        let message = tokio::time::timeout(std::time::Duration::from_secs(5), test.event_rx.recv())
            .await
            .expect("initial scan timed out")
            .expect("scanner event channel closed");
        let event: Value = serde_json::from_str(&message).unwrap();
        if event["id"] != id {
            continue;
        }
        match event["type"].as_str() {
            Some("EventStartedScanning") => scan_started = true,
            Some("EventStoppedScanning") => scan_stopped = true,
            _ => {}
        }
    }
    assert!(scan_started);

    let mut status = String::new();
    let mut durable_status = Value::Null;
    for _ in 0..20 {
        let response = test
            .router
            .clone()
            .oneshot(request(
                Method::GET,
                &format!("/api/v1/library/{id}/scan"),
                Some(&owner_token),
                "",
            ))
            .await
            .unwrap();
        assert_eq!(response.status(), StatusCode::OK);
        durable_status = json_body(response).await;
        status = durable_status["status"].as_str().unwrap().to_owned();
        if status == "complete" {
            break;
        }
        tokio::task::yield_now().await;
    }
    assert_eq!(status, "complete");
    assert_eq!(durable_status["stage"], "complete");
    assert_eq!(durable_status["discovered"], 1);
    assert_eq!(durable_status["processed"], 1);
    assert_eq!(durable_status["skipped"], 1);
    assert!(durable_status["elapsed_seconds"].is_i64());
    assert!(durable_status["last_progress_at"].is_string());
    assert!(durable_status["error_summary"].is_null());

    {
        let mut lock = test.conn.writer().lock_owned().await;
        let mut tx = dim_database::write_tx(&mut lock).await.unwrap();
        let failed = dim_database::ingestion::ScanRun::begin(&mut tx, id, "full")
            .await
            .unwrap();
        dim_database::ingestion::ScanRun::finish(
            &mut tx,
            failed,
            "failed",
            Some("network share became unreadable; reconnect it and retry"),
        )
        .await
        .unwrap();
        tx.commit().await.unwrap();
    }
    let response = test
        .router
        .clone()
        .oneshot(request(
            Method::GET,
            &format!("/api/v1/library/{id}/scan"),
            Some(&owner_token),
            "",
        ))
        .await
        .unwrap();
    let failed_status = json_body(response).await;
    assert_eq!(failed_status["status"], "failed");
    assert_eq!(failed_status["stage"], "failed");
    assert_eq!(
        failed_status["error_summary"],
        "network share became unreadable; reconnect it and retry"
    );

    let response = test
        .router
        .clone()
        .oneshot(request(
            Method::GET,
            &format!("/api/v1/library/{id}/media"),
            Some(&owner_token),
            "",
        ))
        .await
        .unwrap();
    assert_eq!(response.status(), StatusCode::NOT_FOUND);

    let response = test
        .router
        .clone()
        .oneshot(request(
            Method::POST,
            &format!("/api/v1/library/{id}/scan"),
            Some(&owner_token),
            "",
        ))
        .await
        .unwrap();
    assert_eq!(response.status(), StatusCode::OK);
    assert_eq!(json_body(response).await["status"], "scanning");

    let mut retry_started = false;
    let mut retry_stopped = false;
    while !retry_stopped {
        let message = tokio::time::timeout(std::time::Duration::from_secs(5), test.event_rx.recv())
            .await
            .expect("retry scan timed out")
            .expect("scanner event channel closed");
        let event: Value = serde_json::from_str(&message).unwrap();
        if event["id"] != id {
            continue;
        }
        match event["type"].as_str() {
            Some("EventStartedScanning") => retry_started = true,
            Some("EventStoppedScanning") => retry_stopped = true,
            _ => {}
        }
    }
    assert!(retry_started);

    for _ in 0..20 {
        let response = test
            .router
            .clone()
            .oneshot(request(
                Method::GET,
                &format!("/api/v1/library/{id}/scan"),
                Some(&owner_token),
                "",
            ))
            .await
            .unwrap();
        status = json_body(response).await["status"]
            .as_str()
            .unwrap()
            .to_owned();
        if status == "complete" {
            break;
        }
        tokio::task::yield_now().await;
    }
    assert_eq!(status, "complete");

    // Losing websocket listeners must not prevent the scanner from doing its work.
    test.event_rx.close();
    let response = test
        .router
        .clone()
        .oneshot(request(
            Method::POST,
            &format!("/api/v1/library/{id}/scan"),
            Some(&owner_token),
            "",
        ))
        .await
        .unwrap();
    assert_eq!(response.status(), StatusCode::OK);

    for _ in 0..20 {
        let response = test
            .router
            .clone()
            .oneshot(request(
                Method::GET,
                &format!("/api/v1/library/{id}/scan"),
                Some(&owner_token),
                "",
            ))
            .await
            .unwrap();
        status = json_body(response).await["status"]
            .as_str()
            .unwrap()
            .to_owned();
        if status == "complete" {
            break;
        }
        tokio::task::yield_now().await;
    }
    assert_eq!(status, "complete");

    let response = test
        .router
        .clone()
        .oneshot(request(
            Method::GET,
            &format!("/api/v1/library/{id}"),
            Some(&owner_token),
            "",
        ))
        .await
        .unwrap();
    assert_eq!(response.status(), StatusCode::OK);
    let library = json_body(response).await;
    assert_eq!(library["name"], "Family Movies");
    assert_eq!(library["media_type"], "movie");
    assert_eq!(library["auto_scan"], true);
    assert!(
        library.get("locations").is_none(),
        "normal library reads redact server paths"
    );

    let response = test
        .router
        .clone()
        .oneshot(request(
            Method::PATCH,
            &format!("/api/v1/library/{id}"),
            Some(&owner_token),
            r#"{"auto_scan":false}"#,
        ))
        .await
        .unwrap();
    assert_eq!(response.status(), StatusCode::OK);
    assert_eq!(json_body(response).await["auto_scan"], false);

    let response = test
        .router
        .clone()
        .oneshot(request(
            Method::PATCH,
            &format!("/api/v1/library/{id}"),
            Some(&owner_token),
            r#"{"auto_scan":true}"#,
        ))
        .await
        .unwrap();
    assert_eq!(response.status(), StatusCode::OK);
    assert_eq!(json_body(response).await["auto_scan"], true);

    struct Dropped(Arc<AtomicBool>);
    impl Drop for Dropped {
        fn drop(&mut self) {
            self.0.store(true, Ordering::SeqCst);
        }
    }

    // Replace the completed scanner with a deterministic in-flight worker. The DELETE response
    // must wait until this future has been cancelled and dropped before removing database rows.
    let started = Arc::new(tokio::sync::Notify::new());
    let dropped = Arc::new(AtomicBool::new(false));
    let started_task = started.clone();
    let dropped_task = dropped.clone();
    test.workers
        .spawn(id, LibraryWorkerKind::Scanner, async move {
            let _guard = Dropped(dropped_task);
            started_task.notify_one();
            std::future::pending::<()>().await;
        })
        .await
        .unwrap();
    started.notified().await;

    let response = test
        .router
        .clone()
        .oneshot(request(
            Method::DELETE,
            &format!("/api/v1/library/{id}"),
            Some(&owner_token),
            "",
        ))
        .await
        .unwrap();
    assert_eq!(response.status(), StatusCode::NO_CONTENT);
    assert!(dropped.load(Ordering::SeqCst));
    assert!(test
        .workers
        .spawn(id, LibraryWorkerKind::Scanner, async {})
        .await
        .is_err());

    let response = test
        .router
        .clone()
        .oneshot(request(
            Method::GET,
            "/api/v1/library",
            Some(&owner_token),
            "",
        ))
        .await
        .unwrap();
    assert_eq!(response.status(), StatusCode::OK);
    assert_eq!(json_body(response).await, serde_json::json!([]));

    std::fs::remove_dir_all(&test.root).unwrap();
}
