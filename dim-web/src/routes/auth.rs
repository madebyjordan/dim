//! This module contains all docs and APIs related to authentication and user creation.
//!
//! # Request Authentication and Authorization
//! Most API endpoints require a valid server-side session token. If no such token is supplied, the
//! API will return [`AuthError`]. Authentication tokens can be obtained by logging in with
//! the [`login`] method. Authentication tokens must be passed to the server through a
//! `Authroization` header.
//!
//! ## Example of an authenticated call
//! ```text
//! curl -X POST http://127.0.0.1:8000/api/v1/auth/whoami -H "Content-type: application/json" -H
//! "Authorization: eyJhb....."
//! ```
//!
//! # Token expiration
//! Sessions expire after the configured finite lifetime (seven days by default). Renewal requires
//! logging in again, and password changes revoke all sessions for the account.
//!
//! [`AuthError`]
//! [`login`]: fn@login

use crate::AppState;
use axum::extract::ConnectInfo;
use axum::extract::Json;
use axum::extract::Path;
use axum::extract::State;
use axum::response::IntoResponse;
use axum::response::Response;
use axum::Extension;
use std::net::SocketAddr;

use dim_database::asset::Asset;
use dim_database::progress::Progress;
use dim_database::user::InsertableUser;
use dim_database::user::Login;
use dim_database::user::Session;
use dim_database::user::User;
use dim_database::DatabaseError;

use displaydoc::Display;
use http::StatusCode;
use serde_json::json;
use thiserror::Error;

use crate::middleware::Owner;

#[derive(Debug, Display, Error)]
pub enum AuthError {
    /// Not logged in.
    InvalidCredentials,
    /// Bad request: {0}
    BadRequest(String),
    /// database: {0}
    Database(#[from] DatabaseError),
}

impl IntoResponse for AuthError {
    fn into_response(self) -> Response {
        match self {
            Self::InvalidCredentials => crate::error::api_error(
                StatusCode::UNAUTHORIZED,
                "session_expired",
                "Sign in to continue.",
            ),
            Self::BadRequest(_) => crate::error::api_error(
                StatusCode::BAD_REQUEST,
                "invalid_request",
                "The request is invalid.",
            ),
            Self::Database(error) => {
                tracing::error!(?error, "Authentication API database failure");
                crate::error::api_error(
                    StatusCode::INTERNAL_SERVER_ERROR,
                    "internal_error",
                    "Eclipse could not complete the request.",
                )
            }
        }
    }
}

/// # GET `/api/v1/auth/invites`
/// Method will retrieve and return all invite tokens in the database.
///
/// # Authorization
/// This route requires a valid authentication token to be supplied. The token must have `owner`
/// permissions.
///
/// # Request
/// ## Example
/// ```text
/// curl -X GET http://127.0.0.1:8000/api/v1/auth/invites -H "Authorization: ...."
/// ```
///
/// # Response
/// The route will return a response with the following schema
/// ```no_compile
/// [
///   {
///     "id": String,
///     "created": i64,
///     "claimed_by": Option<String>,
///   },
///   ...
/// ]
/// ```
///
/// ## Example
/// ```no_compile
/// [
///   {
///     "id": "079a38b4-d39f-4a9e-9a18-964f225b75d3",
///     "created": 1638708402,
///     "claimed_by": "admin"
///   },
///   {
///     "id": "844caa7b-f54f-a9ea-4444-555555555555",
///     "created": 1640000000,
///   }
/// ]
/// ```
///
/// # Errors
/// * [`AuthError`] - Returned if the authentication token lacks `owner` permissions
///
/// [`AuthError`]
pub async fn get_all_invites(
    _owner: Owner,
    State(AppState { conn, .. }): State<AppState>,
) -> Result<axum::response::Response, AuthError> {
    let mut tx = conn.read().begin().await.map_err(DatabaseError::from)?;
    #[derive(serde::Serialize)]
    struct Row {
        id: String,
        created: i64,
        claimed_by: Option<String>,
    }

    // FIXME: LEFT JOINs cause sqlx::query! to panic, thus we must get tokens in two queries.
    // TODO: Move these into database.
    // TODO: We silently drop db errors here, we should probably change this.
    let mut row = sqlx::query_as!(
        Row,
        r#"SELECT invites.id, invites.date_added as created, NULL as "claimed_by: _"
                FROM invites
                WHERE invites.id NOT IN (SELECT users.claimed_invite FROM users)
                ORDER BY created ASC"#
    )
    .fetch_all(&mut tx)
    .await
    .unwrap_or_default();

    row.append(
        &mut sqlx::query_as!(
            Row,
            r#"SELECT invites.id, invites.date_added as created, users.username as "claimed_by: Option<String>"
            FROM  invites
            INNER JOIN users ON users.claimed_invite = invites.id"#
        )
        .fetch_all(&mut tx)
        .await
        .unwrap_or_default(),
    );

    Ok(axum::response::Json(json!(&row)).into_response())
}

/// # POST `/api/v1/auth/invites`
/// Method will generate and return a new invite token.
///
/// # Authorization
/// This route requires a valid authentication token to be supplied. The token must have `owner`
/// permissions.
///
/// # Request
/// ## Example
/// ```text
/// curl -X POST http://127.0.0.1:8000/api/v1/auth/invites -H "Authorization: ...."
/// ```
///
/// # Response
/// The route will return a response with the following schema
/// ```no_compile
/// {
///   "token": String,
/// }
/// ```
///
/// ## Example
/// ```no_compile
/// {
///   "token": "844caa7b-f54f-a9ea-4444-555555555555",
/// }
/// ```
///
/// # Errors
/// * [`AuthError`] - Returned if the authentication token lacks `owner` permissions
///
/// [`AuthError`]
pub async fn generate_invite(
    _owner: Owner,
    State(AppState { conn, .. }): State<AppState>,
) -> Result<axum::response::Response, AuthError> {
    let mut lock = conn.writer().lock_owned().await;
    let mut tx = dim_database::write_tx(&mut lock)
        .await
        .map_err(DatabaseError::from)?;

    let token = Login::new_invite(&mut tx).await?;

    tx.commit().await.map_err(DatabaseError::from)?;

    Ok(axum::response::Json(json!({ "token": token })).into_response())
}

/// # DELETE `/api/v1/auth/token/:token`
/// Method will revoke the supplied token.
///
/// # Authorization
/// This route requires a valid authentication token to be supplied. The token must have `owner`
/// permissions.
///
/// # Request
/// This request takes in a route parameter which is the token we want to delete.
/// ## Example
/// ```text
/// curl -X DELETE http://127.0.0.1:8000/api/v1/auth/token/844caa7b-f54f-a9ea-4444-555555555555 -H "Authorization: ...."
/// ```
///
/// # Response
/// If the token was successfully deleted, this route will return `200 0K`.
///
/// # Errors
/// * [`AuthError`] - Returned if the authentication token lacks `owner` permissions
///
/// [`AuthError`]
pub async fn delete_token(
    _owner: Owner,
    State(AppState { conn, .. }): State<AppState>,
    Path(token): Path<String>,
) -> Result<impl IntoResponse, AuthError> {
    let mut lock = conn.writer().lock_owned().await;
    let mut tx = dim_database::write_tx(&mut lock)
        .await
        .map_err(DatabaseError::from)?;
    Login::delete_token(&mut tx, token).await?;
    tx.commit().await.map_err(DatabaseError::from)?;

    Ok(StatusCode::OK)
}

/// # GET `/api/v1/user`
/// Method returns metadata about the currently logged in user.
///
/// # Request
/// This method takes in no additional parameters or data.
///
/// ## Authorization
/// This method requires a valid authentication token.
///
/// ## Example
/// ```text
/// curl -X GET http://127.0.0.1:8000/api/v1/user -H "Authorization: ..."
/// ```
///
/// # Response
/// This method will return a JSON payload with the following schema:
/// ```no_compile
/// {
///   "picture": Option<String>,
///   "spentWatching": i64,
///   "username": String,
///   "roles": [String]
/// }
/// ```
///
/// ## Example
/// ```no_compile
/// {
///   "picture": "/images/avatar.jpg",
///   "spentWatching": 12,
///   "username": "admin",
///   "roles": ["owner"],
/// }
/// ```
#[axum::debug_handler]
pub async fn whoami(
    Extension(user): Extension<User>,
    State(AppState { conn, .. }): State<AppState>,
) -> Result<Response, AuthError> {
    let mut tx = conn.read().begin().await.map_err(DatabaseError::from)?;

    Ok(axum::response::Json(json!({
        "picture": Asset::get_of_user(&mut tx, user.id).await.ok().map(|x| format!("/images/{}", x.local_path)),
        "spentWatching": Progress::get_total_time_spent_watching(&mut tx, user.id)
            .await
            .unwrap_or(0) / 3600,
        "username": user.username,
        "roles": user.roles()
    }))
    .into_response())
}

#[derive(Debug, Display, Error)]
pub enum LoginError {
    /// The provided username or password is incorrect.
    InvalidCredentials,
    /// Too many failed attempts.
    Throttled,
    /// Invalid credential input.
    InvalidInput,
    /// database: {0}
    Database(#[from] DatabaseError),
}

impl IntoResponse for LoginError {
    fn into_response(self) -> Response {
        match self {
            Self::InvalidCredentials => crate::error::api_error(
                StatusCode::UNAUTHORIZED,
                "invalid_credentials",
                "The username or password is incorrect.",
            ),
            Self::Throttled => crate::error::api_error(
                StatusCode::TOO_MANY_REQUESTS,
                "login_throttled",
                "Too many failed sign-in attempts. Try again shortly.",
            ),
            Self::InvalidInput => crate::error::api_error(
                StatusCode::BAD_REQUEST,
                "invalid_credentials_input",
                "Username and password must meet the documented length limits.",
            ),
            Self::Database(error) => {
                tracing::error!(?error, "Login database failure");
                crate::error::api_error(
                    StatusCode::INTERNAL_SERVER_ERROR,
                    "internal_error",
                    "Eclipse could not complete the request.",
                )
            }
        }
    }
}

/// # POST `/api/v1/auth/login`
/// Method will log a user in and return a authentication token that can be used to authenticate other
/// requests.
///
/// # Request
/// This method accepts a JSON body that deserializes into [`Login`].
///
/// ## Example
/// ```text
/// curl -X POST http://127.0.0.1:8000/api/v1/auth/login -H "Content-type: application/json" -d
/// '{"username": "testuser", "password": "testpassword"}'
/// ```
///
/// # Response
/// If authentication is successful, this method will return status `200 0K` as well as a
/// authentication token.
/// ```no_compile
/// {
///   "token": "...."
/// }
/// ```
///
/// # Errors
/// * [`LoginError`] - The provided username or password is incorrect.
///
/// [`LoginError`]
/// [`Login`]: dim_database::user::Login
#[axum::debug_handler]
pub async fn login(
    State(app): State<AppState>,
    remote: Option<ConnectInfo<SocketAddr>>,
    Json(new_login): Json<Login>,
) -> Result<Response, LoginError> {
    validate_login_credentials(&new_login.username, &new_login.password)
        .map_err(|_| LoginError::InvalidInput)?;
    let remote_ip = remote
        .map(|value| value.0.ip())
        .unwrap_or_else(|| "127.0.0.1".parse().unwrap());
    if !app.login_allowed(remote_ip) {
        return Err(LoginError::Throttled);
    }
    let mut lock = app.conn.writer().lock_owned().await;
    let mut tx = dim_database::write_tx(&mut lock)
        .await
        .map_err(DatabaseError::from)?;
    let user = match User::authenticate(&mut tx, new_login.username, new_login.password).await {
        Ok(user) => user,
        Err(_) => {
            app.record_login_failure(remote_ip);
            return Err(LoginError::InvalidCredentials);
        }
    };
    let (_, token) =
        Session::create(&mut tx, user.id, app.settings.running().session_ttl_seconds).await?;
    tx.commit().await.map_err(DatabaseError::from)?;
    app.clear_login_failures(remote_ip);
    Ok(session_response(
        json!({ "token": token }),
        &token,
        app.settings.running(),
    ))
}

pub async fn logout(
    Extension(session): Extension<Session>,
    State(AppState { conn, settings, .. }): State<AppState>,
) -> Result<Response, AuthError> {
    let mut lock = conn.writer().lock_owned().await;
    let mut tx = dim_database::write_tx(&mut lock)
        .await
        .map_err(DatabaseError::from)?;
    Session::revoke(&mut tx, &session.id).await?;
    tx.commit().await.map_err(DatabaseError::from)?;
    let mut response = StatusCode::NO_CONTENT.into_response();
    response.headers_mut().insert(
        http::header::SET_COOKIE,
        expired_session_cookie(settings.running())
            .parse()
            .expect("static cookie is valid"),
    );
    Ok(response)
}

pub async fn admin_exists(
    State(AppState { conn, .. }): State<AppState>,
) -> Result<Response, LoginError> {
    let mut tx = conn.read().begin().await.map_err(DatabaseError::from)?;
    let exists = User::get_all(&mut tx)
        .await
        .map_err(LoginError::Database)?
        .is_empty();
    let value = json!({
        "exists": !exists
    });
    Ok(axum::response::Json(value).into_response())
}

#[derive(Debug, Display, Error)]
pub enum RegisterError {
    /// the request does not contain a valid invite token
    NoToken,
    /// Invalid credential input.
    InvalidInput,
    /// database: {0}
    Database(#[from] DatabaseError),
}

impl IntoResponse for RegisterError {
    fn into_response(self) -> Response {
        match self {
            RegisterError::NoToken => crate::error::api_error(
                StatusCode::UNAUTHORIZED,
                "invite_required",
                "A valid invite token is required.",
            ),
            RegisterError::InvalidInput => crate::error::api_error(
                StatusCode::BAD_REQUEST,
                "invalid_credentials_input",
                "Use a username of 1-64 characters and a password of 8-1024 characters.",
            ),
            RegisterError::Database(error) => {
                tracing::error!(?error, "Registration database failure");
                crate::error::api_error(
                    StatusCode::INTERNAL_SERVER_ERROR,
                    "internal_error",
                    "Eclipse could not complete the request.",
                )
            }
        }
    }
}

/// # POST `/api/v1/auth/register`
/// Method will create a new user and return it a authentication token if a user has been
/// successfuly created.
///
/// # Request
/// This method accepts a JSON body that deserializes into [`Login`]. If there are no other users
/// in the database, this route will give the new user `owner` permissions. Additionally this route
/// will not require an invite token.
///
/// If there is a user in the database, this request will require an invite token and the user will
/// be given only `user` permissions.
///
/// ## Example
/// ```text
/// curl -X POST http://127.0.0.1:8000/api/v1/auth/login -H "Content-type: application/json" -d
/// '{"username": "testuser", "password": "testpassword", "invite_token":
/// "72390330-b8af-4413-8305-5f8cae1c8f88"}'
/// ```
///
/// # Response
/// If a user is successfully created, this method will return status `200 0K` as well as the
/// create user's username.
/// ```no_compile
/// {
///   "username": "...."
/// }
/// ```
///
/// # Errors
/// * [`RegisterError`] - Either the request doesnt contain an invite token, or the invite token is
/// invalid.
///
/// [`RegisterError`]
/// [`Login`]: dim_database::user::Login
#[axum::debug_handler]
pub async fn register(
    State(app): State<AppState>,
    Json(new_user): Json<Login>,
) -> Result<Response, RegisterError> {
    validate_credentials(&new_user.username, &new_user.password)
        .map_err(|_| RegisterError::InvalidInput)?;
    // FIXME: Return INTERNAL SERVER ERROR maybe with a traceback?
    let mut lock = app.conn.writer().lock_owned().await;
    let mut tx = dim_database::write_tx(&mut lock)
        .await
        .map_err(DatabaseError::from)?;
    // NOTE: I doubt this method can faily all the time, we should map server error here too.
    let users_empty = User::get_all(&mut tx).await?.is_empty();

    if !users_empty
        && (new_user.invite_token.is_none() || !new_user.invite_token_valid(&mut tx).await?)
    {
        return Err(RegisterError::NoToken);
    }

    let roles = dim_database::user::Roles(if !users_empty {
        vec!["user".to_string()]
    } else {
        vec!["owner".to_string()]
    });

    let claimed_invite = if users_empty {
        // NOTE: Double check what we are returning here.
        Login::new_invite(&mut tx).await?
    } else {
        new_user.invite_token.ok_or(RegisterError::NoToken)?
    };

    let res = InsertableUser {
        username: new_user.username.clone(),
        password: new_user.password.clone(),
        roles,
        claimed_invite,
        prefs: Default::default(),
    }
    .insert(&mut tx)
    .await?;

    // FIXME: Return internal server error.
    let (_, token) =
        Session::create(&mut tx, res.id, app.settings.running().session_ttl_seconds).await?;
    tx.commit().await.map_err(DatabaseError::from)?;
    Ok(session_response(
        json!({ "username": res.username, "token": token }),
        &token,
        app.settings.running(),
    ))
}

fn validate_login_credentials(username: &str, password: &str) -> Result<(), ()> {
    if !(1..=64).contains(&username.chars().count())
        || username.chars().any(char::is_control)
        || !(1..=1024).contains(&password.chars().count())
    {
        return Err(());
    }
    Ok(())
}

fn validate_credentials(username: &str, password: &str) -> Result<(), ()> {
    validate_login_credentials(username, password)?;
    if password.chars().count() < 8 {
        return Err(());
    }
    Ok(())
}

fn session_response(
    body: serde_json::Value,
    token: &str,
    settings: &dim_core::settings::GlobalSettings,
) -> Response {
    let mut response = axum::response::Json(body).into_response();
    response.headers_mut().insert(
        http::header::SET_COOKIE,
        session_cookie(token, settings)
            .parse()
            .expect("session token is a valid cookie value"),
    );
    response
}

fn session_cookie(token: &str, settings: &dim_core::settings::GlobalSettings) -> String {
    format!(
        "dim_session={token}; Path=/; HttpOnly; SameSite=Lax; Max-Age={}{}",
        settings.session_ttl_seconds,
        if settings.https_reverse_proxy {
            "; Secure"
        } else {
            ""
        }
    )
}

fn expired_session_cookie(settings: &dim_core::settings::GlobalSettings) -> String {
    format!(
        "dim_session=; Path=/; HttpOnly; SameSite=Lax; Max-Age=0{}",
        if settings.https_reverse_proxy {
            "; Secure"
        } else {
            ""
        }
    )
}

#[cfg(test)]
mod session_cookie_tests {
    use super::*;

    #[test]
    fn cookie_attributes_follow_deployment_mode() {
        let mut settings = dim_core::settings::GlobalSettings::default();
        let local = session_cookie("token", &settings);
        assert!(local.contains("HttpOnly"));
        assert!(local.contains("SameSite=Lax"));
        assert!(!local.contains("Secure"));

        settings.https_reverse_proxy = true;
        settings.trust_proxy_headers = true;
        assert!(session_cookie("token", &settings).contains("; Secure"));
    }
}
