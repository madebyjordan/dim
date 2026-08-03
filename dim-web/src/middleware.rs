use axum::extract::FromRequestParts;
use axum::extract::State;
use axum::http::request::Parts;
use dim_core::errors::DimError;
use dim_database::user::User;
use dim_database::DbConnection;

use crate::DimErrorWrapper;

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
