use std::net::SocketAddr;

use axum::body::Body;
use axum::http::header::{AUTHORIZATION, CONTENT_TYPE, SET_COOKIE};
use axum::http::{Method, Request, StatusCode};
use dim_core::stream_tracking::StreamTracking;
use dim_web::routes::websocket::CtrlEvent;
use dim_web::{build_router, AppState};
use hyper::body::to_bytes;
use serde_json::{json, Value};
use tower::ServiceExt;
use xtra::spawn::Tokio;

fn request(method: Method, uri: &str, token: Option<&str>, body: Value) -> Request<Body> {
    let mut builder = Request::builder()
        .method(method)
        .uri(uri)
        .header(CONTENT_TYPE, "application/json");
    if let Some(token) = token {
        builder = builder.header(AUTHORIZATION, token);
    }
    builder.body(Body::from(body.to_string())).unwrap()
}

async fn body(response: axum::response::Response) -> Value {
    serde_json::from_slice(&to_bytes(response.into_body()).await.unwrap()).unwrap()
}

#[tokio::test]
async fn clean_database_bootstraps_exactly_one_owner_and_session() {
    dim_database::set_key([11; 32]);
    let directory = tempfile::tempdir().unwrap();
    let cache = directory.path().join("cache");
    std::fs::create_dir_all(&cache).unwrap();
    let config = directory.path().join("config.toml");
    let settings = dim_core::settings::SettingsStore::load(&config).unwrap();
    let conn = dim_database::get_conn_memory().await.unwrap();
    let (socket_tx, _socket_rx) = tokio::sync::mpsc::channel::<CtrlEvent<SocketAddr, String>>(4);
    let (event_tx, _event_rx) = tokio::sync::mpsc::channel(4);
    let manager = nightfall::StateManager::new(
        &mut Tokio::Global,
        cache.to_string_lossy().into_owned(),
        "/bin/false".to_owned(),
    );
    let router = build_router(AppState::new(
        conn,
        socket_tx,
        event_tx,
        manager,
        StreamTracking::default(),
        settings,
    ));

    let response = router
        .clone()
        .oneshot(request(
            Method::GET,
            "/api/v1/auth/admin_exists",
            None,
            json!(null),
        ))
        .await
        .unwrap();
    assert_eq!(response.status(), StatusCode::OK);
    assert_eq!(body(response).await, json!({ "exists": false }));

    let response = router
        .clone()
        .oneshot(request(
            Method::POST,
            "/api/v1/auth/register",
            None,
            json!({ "username": "first-owner", "password": "password123" }),
        ))
        .await
        .unwrap();
    assert_eq!(response.status(), StatusCode::OK);
    assert!(response
        .headers()
        .get(SET_COOKIE)
        .unwrap()
        .to_str()
        .unwrap()
        .starts_with("dim_session="));
    let registration = body(response).await;
    assert_eq!(registration["username"], "first-owner");
    let token = registration["token"].as_str().unwrap();

    let response = router
        .clone()
        .oneshot(request(
            Method::GET,
            "/api/v1/auth/whoami",
            Some(token),
            json!(null),
        ))
        .await
        .unwrap();
    assert_eq!(response.status(), StatusCode::OK);
    let user = body(response).await;
    assert_eq!(user["username"], "first-owner");
    assert_eq!(user["roles"], json!(["owner"]));

    let response = router
        .clone()
        .oneshot(request(
            Method::GET,
            "/api/v1/auth/admin_exists",
            None,
            json!(null),
        ))
        .await
        .unwrap();
    assert_eq!(body(response).await, json!({ "exists": true }));

    let response = router
        .oneshot(request(
            Method::POST,
            "/api/v1/auth/register",
            None,
            json!({ "username": "second-owner", "password": "password123" }),
        ))
        .await
        .unwrap();
    assert_eq!(response.status(), StatusCode::UNAUTHORIZED);
    assert_eq!(body(response).await["error"]["code"], "invite_required");
}
