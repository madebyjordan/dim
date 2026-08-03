pub use dim_utils::json;
pub use dim_utils::*;

use std::io;
use std::path::{Component, Path, PathBuf};

pub fn safe_relative_path(path: impl AsRef<Path>) -> io::Result<PathBuf> {
    if path.as_ref().as_os_str().to_string_lossy().contains('\\') {
        return Err(io::Error::new(
            io::ErrorKind::InvalidInput,
            "metadata path contains a platform-specific separator",
        ));
    }

    let mut relative = PathBuf::new();

    for component in path.as_ref().components() {
        match component {
            Component::Normal(component) => relative.push(component),
            Component::CurDir => {}
            Component::ParentDir | Component::RootDir | Component::Prefix(_) => {
                return Err(io::Error::new(
                    io::ErrorKind::InvalidInput,
                    "path must remain relative to the metadata directory",
                ));
            }
        }
    }

    if relative.as_os_str().is_empty() {
        return Err(io::Error::new(
            io::ErrorKind::InvalidInput,
            "metadata path must not be empty",
        ));
    }

    Ok(relative)
}

pub fn safe_metadata_path(
    root: impl AsRef<Path>,
    relative: impl AsRef<Path>,
) -> io::Result<PathBuf> {
    let relative = safe_relative_path(relative)?;
    let root = std::fs::canonicalize(root)?;
    let target = root.join(relative);

    let mut existing = target.as_path();
    while !existing.exists() {
        existing = existing.parent().ok_or_else(|| {
            io::Error::new(
                io::ErrorKind::InvalidInput,
                "metadata path has no existing parent",
            )
        })?;
    }

    if !std::fs::canonicalize(existing)?.starts_with(&root) {
        return Err(io::Error::new(
            io::ErrorKind::PermissionDenied,
            "metadata path escapes the metadata directory",
        ));
    }

    Ok(target)
}

#[cfg(test)]
mod path_tests {
    use super::safe_relative_path;

    #[test]
    fn accepts_nested_relative_paths() {
        assert_eq!(
            safe_relative_path("posters/movie.jpg").unwrap(),
            std::path::PathBuf::from("posters/movie.jpg")
        );
    }

    #[test]
    fn rejects_escaping_paths() {
        assert!(safe_relative_path("../secret").is_err());
        assert!(safe_relative_path("/etc/passwd").is_err());
        assert!(safe_relative_path(r"C:\Windows\system.ini").is_err());
    }

    #[cfg(unix)]
    #[test]
    fn rejects_symlinks_that_escape_the_metadata_directory() {
        use super::safe_metadata_path;
        use std::os::unix::fs::symlink;

        let root = std::env::temp_dir().join(format!("dim-safe-path-{}", uuid::Uuid::new_v4()));
        let metadata = root.join("metadata");
        let outside = root.join("outside");
        std::fs::create_dir_all(&metadata).unwrap();
        std::fs::create_dir_all(&outside).unwrap();
        symlink(&outside, metadata.join("escape")).unwrap();

        assert!(safe_metadata_path(&metadata, "escape/file.jpg").is_err());

        std::fs::remove_dir_all(root).unwrap();
    }
}
