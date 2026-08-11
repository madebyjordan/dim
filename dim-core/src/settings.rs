use crate::utils::ffpath;

use std::error::Error;
use std::fmt;
use std::fs::{self, File, OpenOptions};
use std::io::{Read, Write};
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Arc, Mutex};

use once_cell::sync::{Lazy, OnceCell};
use serde::{Deserialize, Serialize};

#[derive(Debug)]
pub enum SettingsError {
    Io { path: PathBuf, source: std::io::Error },
    Parse { path: PathBuf, source: toml::de::Error },
    Serialize(toml::ser::Error),
    Invalid(String),
}

impl fmt::Display for SettingsError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Io { path, source } => write!(formatter, "could not access settings file {}: {source}", path.display()),
            Self::Parse { path, source } => write!(formatter, "settings file {} is malformed: {source}", path.display()),
            Self::Serialize(source) => write!(formatter, "could not serialize settings: {source}"),
            Self::Invalid(message) => write!(formatter, "invalid settings: {message}"),
        }
    }
}

impl Error for SettingsError {
    fn source(&self) -> Option<&(dyn Error + 'static)> {
        match self {
            Self::Io { source, .. } => Some(source),
            Self::Parse { source, .. } => Some(source),
            Self::Serialize(source) => Some(source),
            Self::Invalid(_) => None,
        }
    }
}

#[derive(Serialize, Deserialize, Clone, Debug, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
pub struct GlobalSettings {
    pub enable_ssl: bool,
    pub port: u16,
    pub priv_key: Option<String>,
    pub ssl_cert: Option<String>,
    pub cache_dir: String,
    pub metadata_dir: String,
    pub quiet_boot: bool,
    pub disable_auth: bool,
    pub verbose: bool,
    pub secret_key: Option<[u8; 32]>,
    pub enable_hwaccel: bool,
    pub version: String,
}

impl Default for GlobalSettings {
    fn default() -> Self {
        Self {
            enable_ssl: false,
            port: 8000,
            priv_key: None,
            ssl_cert: None,
            cache_dir: "streaming_cache".into(),
            metadata_dir: "metadata".into(),
            quiet_boot: false,
            disable_auth: false,
            verbose: false,
            secret_key: None,
            enable_hwaccel: false,
            version: String::new(),
        }
    }
}

impl GlobalSettings {
    pub fn validate(&self) -> Result<(), SettingsError> {
        if self.port == 0 {
            return Err(SettingsError::Invalid("port must be between 1 and 65535".into()));
        }
        if self.cache_dir.trim().is_empty() {
            return Err(SettingsError::Invalid("cache_dir must not be empty".into()));
        }
        if self.metadata_dir.trim().is_empty() {
            return Err(SettingsError::Invalid("metadata_dir must not be empty".into()));
        }
        if self.enable_ssl || self.priv_key.is_some() || self.ssl_cert.is_some() {
            return Err(SettingsError::Invalid(
                "TLS settings are not implemented; terminate TLS at a reverse proxy and keep enable_ssl=false, priv_key unset, and ssl_cert unset".into(),
            ));
        }
        if self.disable_auth {
            return Err(SettingsError::Invalid(
                "disable_auth is not implemented and must remain false".into(),
            ));
        }
        Ok(())
    }

    /// Host settings are startup-only in Milestone 4. Persisting changes never mutates running
    /// components, so callers can accurately report that a restart is required.
    pub fn restart_required(&self, running: &Self) -> bool {
        self != running
    }
}

#[derive(Clone, Debug)]
pub struct SettingsStore {
    path: Arc<PathBuf>,
    running: Arc<GlobalSettings>,
}

impl SettingsStore {
    pub fn load(path: impl Into<PathBuf>) -> Result<Self, SettingsError> {
        let path = path.into();
        let settings = if path.exists() {
            let mut content = String::new();
            File::open(&path)
                .map_err(|source| SettingsError::Io { path: path.clone(), source })?
                .read_to_string(&mut content)
                .map_err(|source| SettingsError::Io { path: path.clone(), source })?;
            toml::from_str::<GlobalSettings>(&content)
                .map_err(|source| SettingsError::Parse { path: path.clone(), source })?
        } else {
            let settings = GlobalSettings::default();
            persist_atomic(&path, &settings)?;
            settings
        };
        settings.validate()?;
        Ok(Self { path: Arc::new(path), running: Arc::new(settings) })
    }

    pub fn path(&self) -> &Path { self.path.as_path() }
    pub fn running(&self) -> &GlobalSettings { self.running.as_ref() }

    pub fn persisted(&self) -> Result<GlobalSettings, SettingsError> {
        let mut content = String::new();
        File::open(self.path())
            .map_err(|source| SettingsError::Io { path: self.path().to_owned(), source })?
            .read_to_string(&mut content)
            .map_err(|source| SettingsError::Io { path: self.path().to_owned(), source })?;
        let settings = toml::from_str::<GlobalSettings>(&content)
            .map_err(|source| SettingsError::Parse { path: self.path().to_owned(), source })?;
        settings.validate()?;
        Ok(settings)
    }

    pub fn save_for_restart(&self, settings: &GlobalSettings) -> Result<bool, SettingsError> {
        settings.validate()?;
        persist_atomic(self.path(), settings)?;
        Ok(settings.restart_required(self.running()))
    }
}

static TEMP_SEQUENCE: AtomicU64 = AtomicU64::new(0);

fn persist_atomic(path: &Path, settings: &GlobalSettings) -> Result<(), SettingsError> {
    settings.validate()?;
    let parent = path.parent().filter(|parent| !parent.as_os_str().is_empty()).unwrap_or_else(|| Path::new("."));
    fs::create_dir_all(parent).map_err(|source| SettingsError::Io { path: parent.to_owned(), source })?;
    let name = path.file_name().and_then(|name| name.to_str()).unwrap_or("config.toml");
    let sequence = TEMP_SEQUENCE.fetch_add(1, Ordering::Relaxed);
    let temp = parent.join(format!(".{name}.tmp-{}-{sequence}", std::process::id()));

    let result = (|| {
        let mut options = OpenOptions::new();
        options.write(true).create_new(true);
        #[cfg(unix)]
        {
            use std::os::unix::fs::OpenOptionsExt;
            options.mode(0o600);
        }
        let mut file = options.open(&temp).map_err(|source| SettingsError::Io { path: temp.clone(), source })?;
        let encoded = toml::to_string_pretty(settings).map_err(SettingsError::Serialize)?;
        file.write_all(encoded.as_bytes()).map_err(|source| SettingsError::Io { path: temp.clone(), source })?;
        file.sync_all().map_err(|source| SettingsError::Io { path: temp.clone(), source })?;
        drop(file);
        fs::rename(&temp, path).map_err(|source| SettingsError::Io { path: path.to_owned(), source })?;
        #[cfg(unix)]
        File::open(parent).and_then(|directory| directory.sync_all())
            .map_err(|source| SettingsError::Io { path: parent.to_owned(), source })?;
        Ok(())
    })();
    if result.is_err() { let _ = fs::remove_file(&temp); }
    result
}

// Compatibility accessors for older scanner/streaming call sites. New runtime code owns a
// SettingsStore and injects it explicitly.
static GLOBAL_SETTINGS: Lazy<Mutex<GlobalSettings>> = Lazy::new(|| Mutex::new(GlobalSettings::default()));
static SETTINGS_PATH: OnceCell<String> = OnceCell::new();

pub fn get_global_settings() -> GlobalSettings { GLOBAL_SETTINGS.lock().unwrap().clone() }

pub fn init_global_settings(path: Option<String>) -> Result<(), Box<dyn Error>> {
    let path = path.unwrap_or(ffpath("config/config.toml"));
    let store = SettingsStore::load(&path)?;
    let _ = SETTINGS_PATH.set(path);
    *GLOBAL_SETTINGS.lock().unwrap() = store.running().clone();
    Ok(())
}

pub fn set_global_settings(settings: GlobalSettings) -> Result<(), Box<dyn Error>> {
    let path = SETTINGS_PATH.get().cloned().unwrap_or(ffpath("config/config.toml"));
    persist_atomic(Path::new(&path), &settings)?;
    *GLOBAL_SETTINGS.lock().unwrap() = settings;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn malformed_existing_config_is_not_replaced() {
        let directory = tempfile::tempdir().unwrap();
        let path = directory.path().join("config.toml");
        fs::write(&path, "port = definitely-not-a-number").unwrap();
        let before = fs::read(&path).unwrap();
        assert!(matches!(SettingsStore::load(&path), Err(SettingsError::Parse { .. })));
        assert_eq!(fs::read(&path).unwrap(), before);
    }

    #[test]
    fn atomic_save_leaves_no_temporary_file_and_marks_restart() {
        let directory = tempfile::tempdir().unwrap();
        let path = directory.path().join("config.toml");
        let store = SettingsStore::load(&path).unwrap();
        let mut changed = store.running().clone();
        changed.port = 9000;
        assert!(store.save_for_restart(&changed).unwrap());
        assert_eq!(store.persisted().unwrap().port, 9000);
        assert_eq!(store.running().port, 8000);
        assert_eq!(fs::read_dir(directory.path()).unwrap().count(), 1);
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            assert_eq!(fs::metadata(path).unwrap().permissions().mode() & 0o777, 0o600);
        }
    }

    #[test]
    fn rejects_unimplemented_security_switches() {
        let mut settings = GlobalSettings::default();
        settings.disable_auth = true;
        assert!(settings.validate().unwrap_err().to_string().contains("not implemented"));
        settings.disable_auth = false;
        settings.enable_ssl = true;
        assert!(settings.validate().unwrap_err().to_string().contains("TLS"));
    }
}
