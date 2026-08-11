use crate::settings::GlobalSettings;
use crate::utils::ffpath;

use std::fs;
use std::io;
use std::path::{Path, PathBuf};

/// All writable runtime locations, resolved once at startup. The legacy relative metadata/cache
/// paths and `config/dim.db` database location are intentionally preserved.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct RuntimePaths {
    pub config: PathBuf,
    pub database: PathBuf,
    pub metadata: PathBuf,
    pub cache: PathBuf,
    pub logs: PathBuf,
    pub temporary: PathBuf,
}

impl RuntimePaths {
    pub fn from_settings(config: impl Into<PathBuf>, settings: &GlobalSettings) -> Self {
        Self {
            config: config.into(),
            database: PathBuf::from(ffpath("config/dim.db")),
            metadata: PathBuf::from(&settings.metadata_dir),
            cache: PathBuf::from(&settings.cache_dir),
            logs: PathBuf::from("logs"),
            temporary: PathBuf::from(&settings.cache_dir).join("tmp"),
        }
    }

    pub fn prepare(&self) -> io::Result<()> {
        if let Some(parent) = self
            .config
            .parent()
            .filter(|path| !path.as_os_str().is_empty())
        {
            create_private_dir(parent)?;
        }
        if let Some(parent) = self
            .database
            .parent()
            .filter(|path| !path.as_os_str().is_empty())
        {
            create_private_dir(parent)?;
        }
        for directory in [&self.metadata, &self.cache, &self.logs, &self.temporary] {
            create_private_dir(directory)?;
            let probe = directory.join(".dim-write-probe");
            fs::write(&probe, b"")?;
            fs::remove_file(probe)?;
        }
        Ok(())
    }
}

fn create_private_dir(path: &Path) -> io::Result<()> {
    let existed = path.exists();
    fs::create_dir_all(path)?;
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        // Existing installations may intentionally share media/cache directories with a group.
        // Restrict only directories created by Dim; never rewrite an existing operator's mode.
        if !existed {
            let mut permissions = fs::metadata(path)?.permissions();
            permissions.set_mode(0o700);
            fs::set_permissions(path, permissions)?;
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn preserves_legacy_default_locations() {
        let paths = RuntimePaths::from_settings("config/config.toml", &GlobalSettings::default());
        assert_eq!(paths.database, PathBuf::from(ffpath("config/dim.db")));
        assert_eq!(paths.metadata, PathBuf::from("metadata"));
        assert_eq!(paths.cache, PathBuf::from("streaming_cache"));
        assert_eq!(paths.temporary, PathBuf::from("streaming_cache/tmp"));
    }

    #[test]
    fn prepare_is_non_destructive() {
        let directory = tempfile::tempdir().unwrap();
        let mut settings = GlobalSettings::default();
        settings.metadata_dir = directory
            .path()
            .join("metadata")
            .to_string_lossy()
            .into_owned();
        settings.cache_dir = directory
            .path()
            .join("cache")
            .to_string_lossy()
            .into_owned();
        let mut paths =
            RuntimePaths::from_settings(directory.path().join("config/config.toml"), &settings);
        paths.database = directory.path().join("config/dim.db");
        paths.logs = directory.path().join("logs");
        fs::create_dir_all(&paths.metadata).unwrap();
        fs::write(paths.metadata.join("keep.jpg"), b"user data").unwrap();
        paths.prepare().unwrap();
        assert_eq!(
            fs::read(paths.metadata.join("keep.jpg")).unwrap(),
            b"user data"
        );
    }
}
