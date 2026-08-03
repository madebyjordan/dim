use crate::AppState;
use axum::extract::Json;
use axum::extract::State;
use axum::response::IntoResponse;
use axum::response::Response;
use axum::Extension;

use dim_core::settings;
use dim_core::settings::{set_global_settings, GlobalSettings};
use dim_database::user::UpdateableUser;
use dim_database::user::User;
use dim_database::user::UserSettings;
use dim_database::DatabaseError;
use serde::{Deserialize, Serialize};

use super::auth::AuthError;
use crate::middleware::Owner;

#[derive(Clone, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct HostSettings {
    pub enable_ssl: bool,
    pub port: u16,
    pub priv_key: Option<String>,
    pub ssl_cert: Option<String>,
    pub cache_dir: String,
    pub metadata_dir: String,
    pub quiet_boot: bool,
    pub disable_auth: bool,
    pub verbose: bool,
    pub enable_hwaccel: bool,
    pub version: String,
}

impl From<GlobalSettings> for HostSettings {
    fn from(settings: GlobalSettings) -> Self {
        Self {
            enable_ssl: settings.enable_ssl,
            port: settings.port,
            priv_key: settings.priv_key,
            ssl_cert: settings.ssl_cert,
            cache_dir: settings.cache_dir,
            metadata_dir: settings.metadata_dir,
            quiet_boot: settings.quiet_boot,
            disable_auth: settings.disable_auth,
            verbose: settings.verbose,
            enable_hwaccel: settings.enable_hwaccel,
            version: settings.version,
        }
    }
}

impl HostSettings {
    fn into_global_settings(self, secret_key: Option<[u8; 32]>) -> GlobalSettings {
        GlobalSettings {
            enable_ssl: self.enable_ssl,
            port: self.port,
            priv_key: self.priv_key,
            ssl_cert: self.ssl_cert,
            cache_dir: self.cache_dir,
            metadata_dir: self.metadata_dir,
            quiet_boot: self.quiet_boot,
            disable_auth: self.disable_auth,
            verbose: self.verbose,
            secret_key,
            enable_hwaccel: self.enable_hwaccel,
            version: self.version,
        }
    }
}

pub async fn get_user_settings(
    Extension(user): Extension<User>,
    State(AppState { conn, .. }): State<AppState>,
) -> Result<Response, AuthError> {
    let mut tx = conn.read().begin().await.map_err(DatabaseError::from)?;
    Ok(axum::response::Json(&User::get_by_id(&mut tx, user.id).await?.prefs).into_response())
}

pub async fn post_user_settings(
    Extension(user): Extension<User>,
    State(AppState { conn, .. }): State<AppState>,
    Json(new_settings): Json<UserSettings>,
) -> Result<Response, AuthError> {
    let mut lock = conn.writer().lock_owned().await;
    let mut tx = dim_database::write_tx(&mut lock)
        .await
        .map_err(DatabaseError::from)?;
    let update_user = UpdateableUser {
        prefs: Some(new_settings.clone()),
    };

    update_user.update(&mut tx, user.id).await?;

    tx.commit().await.map_err(DatabaseError::from)?;
    drop(lock);

    Ok(axum::response::Json(&new_settings).into_response())
}

fn get_host_settings() -> HostSettings {
    let mut global_settings: GlobalSettings = settings::get_global_settings();
    let git_tag = String::from(env!("GIT_TAG")).to_owned();
    let mut git_sha = String::from(env!("GIT_SHA_256")).to_owned();
    git_sha.truncate(8);
    let version = git_tag + " " + git_sha.as_str();
    global_settings.version = version;
    global_settings.into()
}

pub async fn http_get_global_settings(_owner: Owner) -> Result<Response, AuthError> {
    Ok(axum::response::Json(&get_host_settings()).into_response())
}

pub async fn http_set_global_settings(
    _owner: Owner,
    Json(new_settings): Json<HostSettings>,
) -> Result<Response, AuthError> {
    let secret_key = settings::get_global_settings().secret_key;
    set_global_settings(new_settings.into_global_settings(secret_key))
        .map_err(|error| AuthError::BadRequest(error.to_string()))?;

    Ok(Json(&get_host_settings()).into_response())
}
