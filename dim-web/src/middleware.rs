use axum::extract::FromRequestParts;
use axum::extract::State;
use axum::http::request::Parts;
use dim_core::errors::DimError;
use dim_database::user::Session;
use dim_database::user::User;
use dim_database::DbConnection;

use crate::error::REQUEST_ID_HEADER;
use crate::DimErrorWrapper;
use dim_core::settings::SettingsStore;

pub async fn request_id<B>(
    req: axum::http::Request<B>,
    next: axum::middleware::Next<B>,
) -> axum::response::Response {
    let mut response = next.run(req).await;
    if response.status().is_client_error() || response.status().is_server_error() {
        let is_structured = response
            .headers()
            .get(axum::http::header::CONTENT_TYPE)
            .and_then(|value| value.to_str().ok())
            .map(|value| value.starts_with("application/json"))
            .unwrap_or(false);
        if !is_structured {
            let status = response.status();
            let (code, message) = match status {
                axum::http::StatusCode::BAD_REQUEST => {
                    ("invalid_request", "The request is invalid.")
                }
                axum::http::StatusCode::UNPROCESSABLE_ENTITY => {
                    ("invalid_request", "The request is invalid.")
                }
                axum::http::StatusCode::UNAUTHORIZED => ("session_expired", "Sign in to continue."),
                axum::http::StatusCode::FORBIDDEN => (
                    "forbidden",
                    "You do not have permission to perform this action.",
                ),
                axum::http::StatusCode::NOT_FOUND => {
                    ("not_found", "The requested resource was not found.")
                }
                axum::http::StatusCode::CONFLICT => {
                    ("conflict", "The request conflicts with the current state.")
                }
                axum::http::StatusCode::SERVICE_UNAVAILABLE => {
                    ("server_unavailable", "Dim is temporarily unavailable.")
                }
                _ => ("internal_error", "Dim could not complete the request."),
            };
            if status.is_server_error() {
                tracing::error!(%status, "Legacy API error was mapped to the safe envelope");
            }
            response = crate::error::api_error(status, code, message);
        }
    }
    if !response.headers().contains_key(&REQUEST_ID_HEADER) {
        response.headers_mut().insert(
            REQUEST_ID_HEADER,
            uuid::Uuid::new_v4()
                .to_string()
                .parse()
                .expect("UUID is a valid header value"),
        );
    }
    response
}

pub async fn deployment_guard<B>(
    State(settings): State<SettingsStore>,
    req: axum::http::Request<B>,
    next: axum::middleware::Next<B>,
) -> axum::response::Response {
    let headers = req.headers();
    let has_forwarded = headers.contains_key("forwarded")
        || headers.contains_key("x-forwarded-for")
        || headers.contains_key("x-forwarded-host")
        || headers.contains_key("x-forwarded-proto");
    if has_forwarded && !settings.running().trust_proxy_headers {
        return crate::error::api_error(
            axum::http::StatusCode::BAD_REQUEST,
            "untrusted_proxy_headers",
            "Forwarded headers are not accepted in this deployment mode.",
        );
    }
    if has_forwarded && settings.running().trust_proxy_headers {
        let trusted_peer = req
            .extensions()
            .get::<axum::extract::ConnectInfo<std::net::SocketAddr>>()
            .map(|connect| connect.0.ip().is_loopback())
            .unwrap_or(false);
        if !trusted_peer {
            return crate::error::api_error(
                axum::http::StatusCode::BAD_REQUEST,
                "untrusted_proxy",
                "Proxy headers are accepted only from a loopback proxy.",
            );
        }
    }

    let configured_bind = settings
        .running()
        .bind_address
        .parse::<std::net::IpAddr>()
        .ok();
    if !settings.running().https_reverse_proxy
        && configured_bind
            .map(|address| address.is_loopback())
            .unwrap_or(true)
    {
        if let Some(host) = headers
            .get(axum::http::header::HOST)
            .and_then(|value| value.to_str().ok())
        {
            let authority = format!("http://{host}")
                .parse::<axum::http::Uri>()
                .ok()
                .and_then(|uri| uri.authority().cloned());
            let allowed = authority
                .as_ref()
                .map(|authority| {
                    authority.host().eq_ignore_ascii_case("localhost")
                        || authority
                            .host()
                            .parse::<std::net::IpAddr>()
                            .map(|address| address.is_loopback())
                            .unwrap_or(false)
                })
                .unwrap_or(false);
            if !allowed {
                return crate::error::api_error(
                    axum::http::StatusCode::BAD_REQUEST,
                    "host_not_allowed",
                    "The request host is not allowed for the configured listener.",
                );
            }
        }
    }

    if let Some(origin) = headers
        .get(axum::http::header::ORIGIN)
        .and_then(|value| value.to_str().ok())
    {
        let parsed = origin.parse::<axum::http::Uri>();
        let host = headers
            .get(axum::http::header::HOST)
            .and_then(|value| value.to_str().ok());
        let expected_scheme = if settings.running().https_reverse_proxy {
            "https"
        } else {
            "http"
        };
        let valid = parsed
            .ok()
            .map(|uri| {
                uri.scheme_str() == Some(expected_scheme)
                    && uri.authority().map(|authority| authority.as_str()) == host
            })
            .unwrap_or(false);
        if !valid {
            return crate::error::api_error(
                axum::http::StatusCode::FORBIDDEN,
                "origin_not_allowed",
                "The request origin is not allowed.",
            );
        }
    }
    next.run(req).await
}

/// Extractor for routes that require the authenticated user to have the owner role.
pub struct Owner;

#[axum::async_trait]
impl<S> FromRequestParts<S> for Owner
where
    S: Send + Sync,
{
    type Rejection = DimErrorWrapper;

    async fn from_request_parts(parts: &mut Parts, _state: &S) -> Result<Self, Self::Rejection> {
        let user = parts
            .extensions
            .get::<User>()
            .ok_or(DimErrorWrapper(DimError::Unauthenticated))?;

        if user.has_role("owner") {
            Ok(Self)
        } else {
            Err(DimErrorWrapper(DimError::Unauthorized))
        }
    }
}

pub async fn verify_cookie_token<B>(
    State(conn): State<DbConnection>,
    mut req: axum::http::Request<B>,
    next: axum::middleware::Next<B>,
) -> Result<axum::response::Response, DimErrorWrapper> {
    let bearer = req
        .headers()
        .get(axum::http::header::AUTHORIZATION)
        .and_then(|value| value.to_str().ok())
        .map(str::trim)
        .filter(|value| !value.is_empty());
    let cookie = req
        .headers()
        .get(axum::http::header::COOKIE)
        .and_then(|value| value.to_str().ok())
        .and_then(|cookies| {
            cookies.split(';').find_map(|cookie| {
                let (name, value) = cookie.trim().split_once('=')?;
                (name == "dim_session").then_some(value)
            })
        });
    let token = bearer
        .or(cookie)
        .ok_or(DimErrorWrapper(DimError::Unauthenticated))?;

    if token.is_empty() {
        return Err(DimErrorWrapper(DimError::InvalidCredentials));
    }

    let mut tx = conn.read().begin().await.map_err(|_| {
        DimErrorWrapper(DimError::DatabaseError {
            description: String::from("Failed to start transaction"),
        })
    })?;
    let session = Session::verify(&mut tx, token)
        .await
        .map_err(|_| DimErrorWrapper(DimError::InvalidCredentials))?;

    let current_user = dim_database::user::User::get_by_id(&mut tx, session.user_id)
        .await
        .map_err(|_| DimErrorWrapper(DimError::InvalidCredentials))?;

    req.extensions_mut().insert(current_user);
    req.extensions_mut().insert(session);
    Ok(next.run(req).await)
}
