//! This module contains all docs and APIs related to users and user metadata.
use crate::AppState;
use axum::extract::multipart::Field;
use axum::extract::Json;
use axum::extract::Multipart;
use axum::extract::State;
use axum::response::IntoResponse;
use axum::response::Response;
use axum::Extension;

use dim_database::asset::Asset;
use dim_database::asset::InsertableAsset;
use dim_database::user::Session;
use dim_database::user::User;
use dim_database::DatabaseError;

use displaydoc::Display;
use http::StatusCode;
use serde::Deserialize;
use thiserror::Error;
use uuid::Uuid;

#[derive(Debug, Display, Error)]
pub enum AuthError {
    /// Username not available.
    UsernameNotAvailable,
    /// Upload failed.
    UploadFailed,
    /// Unsupported file.
    UnsupportedFile,
    /// Not logged in.
    InvalidCredentials,
    /// Invalid credential input.
    InvalidInput,
    /// database: {0}
    Database(#[from] DatabaseError),
}

impl IntoResponse for AuthError {
    fn into_response(self) -> Response {
        match self {
            Self::UsernameNotAvailable => crate::error::api_error(
                StatusCode::CONFLICT,
                "username_unavailable",
                "That username is not available.",
            ),
            Self::UnsupportedFile => crate::error::api_error(
                StatusCode::UNSUPPORTED_MEDIA_TYPE,
                "unsupported_file",
                "This file type is not supported.",
            ),
            Self::InvalidCredentials => crate::error::api_error(
                StatusCode::UNAUTHORIZED,
                "invalid_credentials",
                "The supplied credentials are incorrect.",
            ),
            Self::InvalidInput => crate::error::api_error(
                StatusCode::BAD_REQUEST,
                "invalid_credentials_input",
                "Use a username of 1-64 characters and a password of 8-1024 characters.",
            ),
            Self::UploadFailed => crate::error::api_error(
                StatusCode::INTERNAL_SERVER_ERROR,
                "upload_failed",
                "The upload could not be completed.",
            ),
            Self::Database(error) => {
                tracing::error!(?error, "Account API database failure");
                crate::error::api_error(
                    StatusCode::INTERNAL_SERVER_ERROR,
                    "internal_error",
                    "Eclipse could not complete the request.",
                )
            }
        }
    }
}

#[derive(Deserialize)]
pub struct ChangePasswordParams {
    old_password: String,
    new_password: String,
}

/// # PATCH `/api/v1/user/password`
/// Method changes the password for a logged in account.
///
/// # Request
/// This method accepts a JSON body with the following schema:
/// ```no_compile
/// {
///   "old_password": String,
///   "new_password": String,
/// }
/// ```
/// The `old_password` field in the JSON payload must be the currently registered password for this
/// user. The `new_password` field is the new password that we want to set.
///
/// ## Example
/// ```text
/// curl -X PATCH http://127.0.0.1:8000/api/v1/user/password -H "Content-type: application/json"
/// -H "Authroization: ..." -d '{"old_password": "testPass", "new_password": "newTestPass"}'
/// ```
///
/// # Response
/// If the password is successfully changed, the method will simply return `200 0K`.
///
/// # Errors
/// * [`InvalidCredentials`] - The provided `old_password` is incorrect or the authentication token
/// is invalid.
///
/// [`InvalidCredentials`]: AuthError::InvalidCredentials
pub async fn change_password(
    Extension(user): Extension<User>,
    State(AppState { conn, .. }): State<AppState>,
    Json(params): Json<ChangePasswordParams>,
) -> Result<impl IntoResponse, AuthError> {
    if !(8..=1024).contains(&params.new_password.chars().count()) {
        return Err(AuthError::InvalidInput);
    }
    let mut lock = conn.writer().lock_owned().await;
    let mut tx = dim_database::write_tx(&mut lock)
        .await
        .map_err(DatabaseError::from)?;

    let user = User::authenticate(&mut tx, user.username, params.old_password)
        .await
        .map_err(|_| AuthError::InvalidCredentials)?;

    user.set_password(&mut tx, params.new_password).await?;
    Session::revoke_user(&mut tx, user.id).await?;

    tx.commit().await.map_err(DatabaseError::from)?;

    Ok(StatusCode::OK)
}

#[derive(Deserialize)]
pub struct DeleteParams {
    password: String,
}

/// # DELETE `/api/v1/user`
/// Method deletes the currently logged in account.
///
/// # Request
/// This method accepts a JSON body with the following schema:
/// ```no_compile
/// {
///   "password": String,
/// }
/// ```
/// The `password` field in the JSON payload must be the currently registered password for this
/// user. This is required as a safety mechanism to avoid accidental account deletion.
///
/// ## Example
/// ```text
/// curl -X DELETE http://127.0.0.1:8000/api/v1/user -H "Content-type: application/json" -H "Authroization: ..."
/// -d '{"password": "testPass"}'
/// ```
///
/// # Response
/// If the account is successfully deleted, the method will simply return `200 0K`.
///
/// # SAFETY and caveats
/// Deleting the account cascades to its server-side sessions, so outstanding tokens are revoked.
///
/// # Errors
/// * [`InvalidCredentials`] - The provided `old_password` is incorrect or the authentication token
/// is invalid.
///
/// [`InvalidCredentials`]: AuthError::InvalidCredentials
pub async fn delete(
    Extension(user): Extension<User>,
    State(AppState { conn, .. }): State<AppState>,
    Json(params): Json<DeleteParams>,
) -> Result<impl IntoResponse, AuthError> {
    let mut lock = conn.writer().lock_owned().await;
    let mut tx = dim_database::write_tx(&mut lock)
        .await
        .map_err(DatabaseError::from)?;

    let user = User::authenticate(&mut tx, user.username, params.password)
        .await
        .map_err(|_| AuthError::InvalidCredentials)?;

    User::delete(&mut tx, user.id).await?;

    tx.commit().await.map_err(DatabaseError::from)?;

    Ok(StatusCode::OK)
}

#[derive(Deserialize)]
pub struct ChangeUsernameParams {
    new_username: String,
}

/// # PATCH `/api/v1/user/username`
/// Method changes the username of the current account.
///
/// # Request
/// This method accepts a JSON payload with the following schema:
/// ```no_compile
/// {
///   "new_username": String
/// }
/// ```
///
/// ## Example
/// ```text
/// curl -X PATCH http://127.0.0.1:8000/api/v1/user/username -H "Content-type: application/json" -H
/// "Authorization: ..." -d '{"new_username": "testUsername"}'
/// ```
///
/// # Response
/// If the username is successfully changed this method will simply return `200 OK`.
///
/// # Errors
/// * [`UsernameNotAvailable`] - THe provided username has already been claimed by another user.
///
/// [`UsernameNotAvailable`]: AuthError::UsernameNotAvailable
pub async fn change_username(
    Extension(user): Extension<User>,
    State(AppState { conn, .. }): State<AppState>,
    Json(params): Json<ChangeUsernameParams>,
) -> Result<impl IntoResponse, AuthError> {
    if !(1..=64).contains(&params.new_username.chars().count())
        || params.new_username.chars().any(char::is_control)
    {
        return Err(AuthError::InvalidInput);
    }
    let mut lock = conn.writer().lock_owned().await;
    let mut tx = dim_database::write_tx(&mut lock)
        .await
        .map_err(DatabaseError::from)?;
    if User::get(&mut tx, &params.new_username).await.is_ok() {
        return Err(AuthError::UsernameNotAvailable);
    }

    User::set_username(&mut tx, user.id, params.new_username).await?;
    tx.commit().await.map_err(DatabaseError::from)?;

    Ok(StatusCode::OK)
}

/// # POST `/api/v1/user/avatar`
/// This method can be used to set a new avatar for a user.
///
/// # Request
/// This method accepts a multipart file upload. Only `jpg` and `png` files are supported.
///
/// ## Example
/// ```text
/// curl -X POST http://127.0.0.1:8000/api/v1/user/avatar -H "Authorization: ..." --form
/// file='@newAvatar.png'
/// ```
///
/// # Response
/// If the avatar is successfully uploaded, this route will return `200 OK`.
///
/// # Errors
/// * [`UploadFailed`] - No file has been uploaded correctly or the `file` form field has not been
/// * [`UnsupportedFile`] - The file uploaded is not supported.
/// found.
///
/// [`UploadFailed`]: AuthError::UploadFailed
/// [`UnsupportedFile`]: AuthError::UnsupportedFile
pub async fn upload_avatar(
    Extension(user): Extension<User>,
    State(AppState { conn, .. }): State<AppState>,
    mut multipart: Multipart,
) -> Result<impl IntoResponse, AuthError> {
    let mut lock = conn.writer().lock_owned().await;
    let mut tx = dim_database::write_tx(&mut lock)
        .await
        .map_err(DatabaseError::from)?;

    let mut asset: Option<Asset> = None;

    while let Some(field) = multipart.next_field().await.unwrap_or(None) {
        let name = field.name().unwrap().to_string();
        if name == "file" {
            asset = Some(process_part(&mut tx, field).await?)
        }
    }

    match asset {
        Some(asset) => {
            User::set_picture(&mut tx, user.id, asset.id).await?;
            tx.commit().await.map_err(DatabaseError::from)?;

            Ok(StatusCode::OK)
        }
        None => Err(AuthError::UploadFailed),
    }
}

/// Remove the avatar associated with the logged-in user.
pub async fn delete_avatar(
    Extension(user): Extension<User>,
    State(AppState { conn, .. }): State<AppState>,
) -> Result<impl IntoResponse, AuthError> {
    let mut lock = conn.writer().lock_owned().await;
    let mut tx = dim_database::write_tx(&mut lock)
        .await
        .map_err(DatabaseError::from)?;

    let asset = match user.picture {
        Some(asset_id) => Some(Asset::get_by_id(&mut tx, asset_id).await?),
        None => None,
    };
    User::clear_picture(&mut tx, user.id).await?;
    if let Some(asset) = &asset {
        Asset::delete(&mut tx, asset.id).await?;
    }
    tx.commit().await.map_err(DatabaseError::from)?;

    if let (Some(asset), Some(metadata_path)) = (asset, dim_core::core::METADATA_PATH.get()) {
        let path = format!("{metadata_path}/{}", asset.local_path);
        if let Err(error) = tokio::fs::remove_file(path).await {
            if error.kind() != std::io::ErrorKind::NotFound {
                tracing::warn!(?error, "Failed to remove deleted avatar from disk");
            }
        }
    }

    Ok(StatusCode::OK)
}

#[doc(hidden)]
pub async fn process_part(
    conn: &mut dim_database::Transaction<'_>,
    p: Field<'_>,
) -> Result<Asset, AuthError> {
    if p.name().unwrap() != "file" {
        return Err(AuthError::UploadFailed);
    }

    let file_ext = match p.content_type() {
        Some("image/jpeg" | "image/jpg") => "jpg",
        Some("image/png") => "png",
        _ => return Err(AuthError::UnsupportedFile),
    };

    let contents = p.bytes().await.map_err(|_| AuthError::UploadFailed)?;

    let local_file = format!("{}.{}", Uuid::new_v4().to_string(), file_ext);
    let local_path = format!(
        "{}/{}",
        dim_core::core::METADATA_PATH.get().unwrap(),
        &local_file
    );

    tokio::fs::write(&local_path, contents)
        .await
        .map_err(|_| AuthError::UploadFailed)?;

    Ok(InsertableAsset {
        local_path: local_file,
        file_ext: file_ext.into(),
        ..Default::default()
    }
    .insert_local_asset(conn)
    .await?)
}
