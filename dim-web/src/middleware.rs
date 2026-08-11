use axum::extract::FromRequestParts;
use axum::extract::State;
use axum::http::request::Parts;
use dim_core::errors::DimError;
use dim_database::user::User;
use dim_database::DbConnection;

use crate::error::REQUEST_ID_HEADER;
use crate::DimErrorWrapper;

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
    let token = req
        .headers()
        .get(axum::http::header::AUTHORIZATION)
        .ok_or(DimErrorWrapper(DimError::Unauthenticated))?
        .to_str()
        .map_err(|_| DimErrorWrapper(DimError::InvalidCredentials))?;

    if token.is_empty() {
        return Err(DimErrorWrapper(DimError::InvalidCredentials));
    }

    let mut tx = conn.read().begin().await.map_err(|_| {
        DimErrorWrapper(DimError::DatabaseError {
            description: String::from("Failed to start transaction"),
        })
    })?;
    let id = dim_database::user::Login::verify_cookie(token.to_owned())
        .map_err(|_| DimErrorWrapper(DimError::InvalidCredentials))?;

    let current_user = dim_database::user::User::get_by_id(&mut tx, id)
        .await
        .map_err(|_| DimErrorWrapper(DimError::InvalidCredentials))?;

    req.extensions_mut().insert(current_user);
    Ok(next.run(req).await)
}
