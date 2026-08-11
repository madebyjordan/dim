use crate::AppState;
use axum::extract::Json;
use axum::extract::State;
use axum::response::IntoResponse;
use axum::response::Response;
use axum::Extension;

use dim_core::settings::GlobalSettings;
use dim_database::user::UpdateableUser;
use dim_database::user::User;
use dim_database::user::UserSettings;
use dim_database::DatabaseError;
use serde::{Deserialize, Serialize};

use super::auth::AuthError;
use crate::middleware::Owner;

const REDACTED_PATH: &str = "<redacted>";

fn public_setting_path(path: String) -> String {
    if std::path::Path::new(&path).is_absolute() {
        REDACTED_PATH.into()
    } else {
        path
    }
}

#[derive(Clone, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct HostSettings {
    pub bind_address: String,
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
    pub https_reverse_proxy: bool,
    pub trust_proxy_headers: bool,
    pub session_ttl_seconds: u64,
    pub login_attempts_per_minute: u32,
    #[serde(default)]
    pub restart_required: bool,
}

impl From<GlobalSettings> for HostSettings {
    fn from(settings: GlobalSettings) -> Self {
        Self {
            bind_address: settings.bind_address,
            enable_ssl: settings.enable_ssl,
            port: settings.port,
            priv_key: settings.priv_key,
            ssl_cert: settings.ssl_cert,
            cache_dir: public_setting_path(settings.cache_dir),
            metadata_dir: public_setting_path(settings.metadata_dir),
            quiet_boot: settings.quiet_boot,
            disable_auth: settings.disable_auth,
            verbose: settings.verbose,
            enable_hwaccel: settings.enable_hwaccel,
            version: settings.version,
            https_reverse_proxy: settings.https_reverse_proxy,
            trust_proxy_headers: settings.trust_proxy_headers,
            session_ttl_seconds: settings.session_ttl_seconds,
            login_attempts_per_minute: settings.login_attempts_per_minute,
            restart_required: false,
        }
    }
}

impl HostSettings {
    fn into_global_settings(self, current: &GlobalSettings) -> GlobalSettings {
        GlobalSettings {
            bind_address: self.bind_address,
            enable_ssl: self.enable_ssl,
            port: self.port,
            priv_key: self.priv_key,
            ssl_cert: self.ssl_cert,
            cache_dir: if self.cache_dir == REDACTED_PATH {
                current.cache_dir.clone()
            } else {
                self.cache_dir
            },
            metadata_dir: if self.metadata_dir == REDACTED_PATH {
                current.metadata_dir.clone()
            } else {
                self.metadata_dir
            },
            quiet_boot: self.quiet_boot,
            disable_auth: self.disable_auth,
            verbose: self.verbose,
            secret_key: current.secret_key,
            enable_hwaccel: self.enable_hwaccel,
            version: self.version,
            https_reverse_proxy: self.https_reverse_proxy,
            trust_proxy_headers: self.trust_proxy_headers,
            session_ttl_seconds: self.session_ttl_seconds,
            login_attempts_per_minute: self.login_attempts_per_minute,
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

fn get_host_settings(settings: &dim_core::settings::SettingsStore) -> Result<HostSettings, String> {
    let mut global_settings = settings.persisted().map_err(|error| error.to_string())?;
    let restart_required = global_settings.restart_required(settings.running());
    let git_tag = String::from(env!("GIT_TAG")).to_owned();
    let mut git_sha = String::from(env!("GIT_SHA_256")).to_owned();
    git_sha.truncate(8);
    let version = git_tag + " " + git_sha.as_str();
    global_settings.version = version;
    let mut host: HostSettings = global_settings.into();
    host.restart_required = restart_required;
    Ok(host)
}

pub async fn http_get_global_settings(
    _owner: Owner,
    State(AppState { settings, .. }): State<AppState>,
) -> Result<Response, AuthError> {
    Ok(
        axum::response::Json(&get_host_settings(&settings).map_err(AuthError::BadRequest)?)
            .into_response(),
    )
}

pub async fn http_set_global_settings(
    _owner: Owner,
    State(AppState { settings, .. }): State<AppState>,
    Json(new_settings): Json<HostSettings>,
) -> Result<Response, AuthError> {
    let current = settings
        .persisted()
        .map_err(|error| AuthError::BadRequest(error.to_string()))?;
    settings
        .save_for_restart(&new_settings.into_global_settings(&current))
        .map_err(|error| AuthError::BadRequest(error.to_string()))?;

    Ok(Json(&get_host_settings(&settings).map_err(AuthError::BadRequest)?).into_response())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn absolute_host_paths_are_redacted_and_round_trip_without_overwrite() {
        let mut current = GlobalSettings::default();
        current.cache_dir = "/private/cache".into();
        current.metadata_dir = "/private/metadata".into();
        let public = HostSettings::from(current.clone());
        assert_eq!(public.cache_dir, REDACTED_PATH);
        assert_eq!(public.metadata_dir, REDACTED_PATH);
        let restored = public.into_global_settings(&current);
        assert_eq!(restored.cache_dir, "/private/cache");
        assert_eq!(restored.metadata_dir, "/private/metadata");
    }
}
