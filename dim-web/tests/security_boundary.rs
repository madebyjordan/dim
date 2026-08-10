use std::net::SocketAddr;
use std::path::PathBuf;

use axum::body::Body;
use axum::http::header::{AUTHORIZATION, CONTENT_TYPE, ORIGIN};
use axum::http::{HeaderValue, Method, Request, StatusCode};
use dim_core::stream_tracking::StreamTracking;
use dim_database::user::{InsertableUser, Roles, UserSettings};
use dim_web::routes::websocket::CtrlEvent;
use dim_web::{build_router, AppState};
use hyper::body::to_bytes;
use tower::ServiceExt;
use xtra::spawn::Tokio;

struct TestApp {
    router: axum::Router,
    owner_token: String,
    user_token: String,
    root: PathBuf,
}

async fn insert_user(
    conn: &dim_database::DbConnection,
    username: &str,
    role: &str,
) -> dim_database::user::User {
    let mut lock = conn.writer().lock_owned().await;
    let mut tx = dim_database::write_tx(&mut lock).await.unwrap();
    let invite = dim_database::user::Login::new_invite(&mut tx)
        .await
        .unwrap();
    let user = InsertableUser {
        username: username.to_owned(),
        password: "password".to_owned(),
        roles: Roles(vec![role.to_owned()]),
        prefs: UserSettings::default(),
        claimed_invite: invite,
    }
    .insert(&mut tx)
    .await
    .unwrap();
    tx.commit().await.unwrap();
    user
}

async fn test_app() -> TestApp {
    dim_database::set_key([7; 32]);

    let root = std::env::temp_dir().join(format!("dim-security-boundary-{}", uuid::Uuid::new_v4()));
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
    let owner = insert_user(&conn, "owner", "owner").await;
    let user = insert_user(&conn, "viewer", "user").await;
    std::fs::write(metadata.join("avatar.png"), b"avatar").unwrap();
    let mut lock = conn.writer().lock_owned().await;
    let mut tx = dim_database::write_tx(&mut lock).await.unwrap();
    let avatar = dim_database::asset::InsertableAsset {
        local_path: "avatar.png".into(),
        file_ext: "png".into(),
        ..Default::default()
    }
    .insert_local_asset(&mut tx)
    .await
    .unwrap();
    dim_database::user::User::set_picture(&mut tx, user.id, avatar.id)
        .await
        .unwrap();
    tx.commit().await.unwrap();
    drop(lock);

    let owner_token = dim_database::user::Login::create_cookie(owner.id);
    let user_token = dim_database::user::Login::create_cookie(user.id);

    let (socket_tx, _socket_rx) = tokio::sync::mpsc::channel::<CtrlEvent<SocketAddr, String>>(16);
    let (event_tx, _event_rx) = tokio::sync::mpsc::unbounded_channel();
    let state = nightfall::StateManager::new(
        &mut Tokio::Global,
        cache.to_string_lossy().into_owned(),
        "/bin/false".to_owned(),
    );
    let app = AppState::new(conn, socket_tx, event_tx, state, StreamTracking::default());

    TestApp {
        router: build_router(app),
        owner_token,
        user_token,
        root,
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

async fn response_body(response: axum::response::Response) -> Vec<u8> {
    to_bytes(response.into_body()).await.unwrap().to_vec()
}

#[tokio::test]
async fn enforces_the_application_security_boundary() {
    let test = test_app().await;

    let response = test
        .router
        .clone()
        .oneshot(request(Method::GET, "/api/v1/auth/admin_exists", None, ""))
        .await
        .unwrap();
    assert_eq!(response.status(), StatusCode::OK);

    for uri in [
        "/api/v1/library",
        "/api/v1/library/1/scan",
        "/api/v1/auth/whoami",
        "/api/v1/dashboard",
        "/api/v1/search?query=movie",
        "/api/v1/media/1/files",
        "/api/v1/mediafile/1",
        "/api/v1/season/1",
        "/api/v1/user/settings",
        "/api/v1/host/settings",
        "/api/v1/auth/invites",
        "/api/v1/filebrowser?path=/tmp",
        "/api/v1/stream/missing/state/get_stderr",
    ] {
        let response = test
            .router
            .clone()
            .oneshot(request(Method::GET, uri, None, ""))
            .await
            .unwrap();
        assert_eq!(response.status(), StatusCode::UNAUTHORIZED, "{uri}");
    }

    let response = test
        .router
        .clone()
        .oneshot(request(
            Method::GET,
            "/api/v1/library",
            Some(&test.user_token),
            "",
        ))
        .await
        .unwrap();
    assert_eq!(response.status(), StatusCode::OK);

    let response = test
        .router
        .clone()
        .oneshot(request(
            Method::GET,
            "/api/v1/user/settings",
            Some(&test.user_token),
            "",
        ))
        .await
        .unwrap();
    assert_eq!(response.status(), StatusCode::OK);

    for (method, uri, body) in [
        (Method::GET, "/api/v1/filebrowser?path=/tmp", ""),
        (Method::GET, "/api/v1/host/settings", ""),
        (Method::POST, "/api/v1/host/settings", "{}"),
        (Method::GET, "/api/v1/auth/invites", ""),
        (Method::POST, "/api/v1/auth/invites", ""),
        (Method::DELETE, "/api/v1/auth/token/invite", ""),
        (Method::POST, "/api/v1/library", "{}"),
        (Method::POST, "/api/v1/library/1/scan", ""),
        (Method::DELETE, "/api/v1/library/1", ""),
        (Method::PATCH, "/api/v1/media/1", "{}"),
        (Method::DELETE, "/api/v1/media/1", ""),
        (Method::POST, "/api/v1/media/1/rematch", "{}"),
        (Method::PATCH, "/api/v1/mediafile/match", "{}"),
        (Method::PATCH, "/api/v1/season/1", "{}"),
        (Method::DELETE, "/api/v1/season/1", ""),
        (Method::PATCH, "/api/v1/episode/1", "{}"),
        (Method::DELETE, "/api/v1/episode/1", ""),
    ] {
        let response = test
            .router
            .clone()
            .oneshot(request(method, uri, Some(&test.user_token), body))
            .await
            .unwrap();
        assert_eq!(response.status(), StatusCode::FORBIDDEN, "{uri}");
    }

    let response = test
        .router
        .clone()
        .oneshot(request(
            Method::GET,
            "/api/v1/filebrowser?path=/tmp",
            Some(&test.owner_token),
            "",
        ))
        .await
        .unwrap();
    assert_eq!(response.status(), StatusCode::OK);

    let response = test
        .router
        .clone()
        .oneshot(request(
            Method::POST,
            "/api/v1/auth/invites",
            Some(&test.owner_token),
            "",
        ))
        .await
        .unwrap();
    assert_eq!(response.status(), StatusCode::OK);

    let response = test
        .router
        .clone()
        .oneshot(request(
            Method::GET,
            "/api/v1/host/settings",
            Some(&test.owner_token),
            "",
        ))
        .await
        .unwrap();
    assert_eq!(response.status(), StatusCode::OK);
    let body = response_body(response).await;
    assert!(!String::from_utf8(body).unwrap().contains("secret_key"));

    let response = test
        .router
        .clone()
        .oneshot(request(
            Method::POST,
            "/api/v1/host/settings",
            Some(&test.owner_token),
            r#"{"secret_key":[0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0]}"#,
        ))
        .await
        .unwrap();
    assert_eq!(response.status(), StatusCode::UNPROCESSABLE_ENTITY);

    let mut malformed = request(Method::GET, "/api/v1/library", None, "");
    malformed
        .headers_mut()
        .insert(AUTHORIZATION, HeaderValue::from_bytes(&[0xff]).unwrap());
    let response = test.router.clone().oneshot(malformed).await.unwrap();
    assert_eq!(response.status(), StatusCode::UNAUTHORIZED);

    let response = test
        .router
        .clone()
        .oneshot(request(Method::GET, "/api/v1/library", Some(""), ""))
        .await
        .unwrap();
    assert_eq!(response.status(), StatusCode::UNAUTHORIZED);

    let response = test
        .router
        .clone()
        .oneshot(request(
            Method::PATCH,
            "/api/v1/user/username",
            Some(&test.user_token),
            r#"{"new_username":"renamed-viewer"}"#,
        ))
        .await
        .unwrap();
    assert_eq!(response.status(), StatusCode::OK);

    let response = test
        .router
        .clone()
        .oneshot(request(
            Method::PATCH,
            "/api/v1/user/password",
            Some(&test.user_token),
            r#"{"old_password":"password","new_password":"new-password"}"#,
        ))
        .await
        .unwrap();
    assert_eq!(response.status(), StatusCode::OK);

    let response = test
        .router
        .clone()
        .oneshot(request(
            Method::POST,
            "/api/v1/auth/login",
            None,
            r#"{"username":"renamed-viewer","password":"new-password"}"#,
        ))
        .await
        .unwrap();
    assert_eq!(response.status(), StatusCode::OK);

    let response = test
        .router
        .clone()
        .oneshot(request(
            Method::DELETE,
            "/api/v1/user/avatar",
            Some(&test.user_token),
            "",
        ))
        .await
        .unwrap();
    assert_eq!(response.status(), StatusCode::OK);
    assert!(!test.root.join("metadata/avatar.png").exists());

    let response = test
        .router
        .clone()
        .oneshot(request(
            Method::DELETE,
            "/api/v1/user",
            Some(&test.user_token),
            r#"{"password":"new-password"}"#,
        ))
        .await
        .unwrap();
    assert_eq!(response.status(), StatusCode::OK);

    let metadata = test.root.join("metadata");
    std::fs::write(metadata.join("poster.jpg"), b"poster").unwrap();
    std::fs::write(test.root.join("outside.txt"), b"private").unwrap();

    let response = test
        .router
        .clone()
        .oneshot(request(Method::GET, "/images/poster.jpg", None, ""))
        .await
        .unwrap();
    assert_eq!(response.status(), StatusCode::OK);

    for uri in [
        "/images/../outside.txt",
        "/images/%2e%2e/outside.txt",
        "/images/%2Fetc%2Fpasswd",
    ] {
        let response = test
            .router
            .clone()
            .oneshot(request(Method::GET, uri, None, ""))
            .await
            .unwrap();
        assert_ne!(response.status(), StatusCode::OK, "{uri}");
    }

    let mut cors_request = request(Method::GET, "/api/v1/auth/admin_exists", None, "");
    cors_request
        .headers_mut()
        .insert(ORIGIN, HeaderValue::from_static("https://example.com"));
    let response = test.router.clone().oneshot(cors_request).await.unwrap();
    assert!(response
        .headers()
        .get("access-control-allow-origin")
        .is_none());

    std::fs::remove_dir_all(&test.root).unwrap();
}
