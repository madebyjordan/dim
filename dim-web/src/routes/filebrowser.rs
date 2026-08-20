use std::env;
use std::fs;
use std::io;
use std::path::{Path as FsPath, PathBuf};

use axum::extract::Query;
use axum::response::{IntoResponse, Response};
use axum::Json;
use http::StatusCode;
use serde::{Deserialize, Serialize};
use tokio::task::spawn_blocking;

use crate::middleware::Owner;

#[derive(Debug)]
pub enum FileBrowserError {
    InvalidPath,
    NotFound,
    NotDirectory,
    PermissionDenied,
    Io,
}

impl From<io::Error> for FileBrowserError {
    fn from(error: io::Error) -> Self {
        match error.kind() {
            io::ErrorKind::NotFound => Self::NotFound,
            io::ErrorKind::PermissionDenied => Self::PermissionDenied,
            _ => Self::Io,
        }
    }
}

impl IntoResponse for FileBrowserError {
    fn into_response(self) -> Response {
        let (status, message) = match self {
            Self::InvalidPath => (
                StatusCode::BAD_REQUEST,
                "Enter a valid absolute folder path.",
            ),
            Self::NotFound => (StatusCode::NOT_FOUND, "That folder no longer exists."),
            Self::NotDirectory => (StatusCode::BAD_REQUEST, "That path is not a folder."),
            Self::PermissionDenied => (
                StatusCode::FORBIDDEN,
                "Eclipse does not have permission to read that folder.",
            ),
            Self::Io => (
                StatusCode::INTERNAL_SERVER_ERROR,
                "Eclipse could not read that folder.",
            ),
        };

        (status, message).into_response()
    }
}

#[derive(Debug, Deserialize)]
pub struct DirectoryQuery {
    path: Option<String>,
}

#[derive(Debug, Serialize, PartialEq, Eq)]
pub struct DirectoryEntry {
    name: String,
    path: String,
}

#[derive(Debug, Serialize, PartialEq, Eq)]
pub struct DirectoryListing {
    current: String,
    parent: Option<String>,
    directories: Vec<DirectoryEntry>,
}

fn path_string(path: &FsPath) -> String {
    path.to_string_lossy().into_owned()
}

fn is_readable_directory(path: &FsPath) -> bool {
    path.is_absolute() && path.is_dir() && fs::read_dir(path).is_ok()
}

fn default_directory() -> PathBuf {
    if let Some(home) = env::var_os("HOME").map(PathBuf::from) {
        if is_readable_directory(&home) {
            return home;
        }
    }

    PathBuf::from(FsPath::new(std::path::MAIN_SEPARATOR_STR))
}

pub fn enumerate_directory(path: PathBuf) -> Result<DirectoryListing, FileBrowserError> {
    if !path.is_absolute() {
        return Err(FileBrowserError::InvalidPath);
    }

    let path = fs::canonicalize(path)?;
    if !path.is_dir() {
        return Err(FileBrowserError::NotDirectory);
    }

    let mut directories = fs::read_dir(&path)?
        .filter_map(Result::ok)
        .filter_map(|entry| {
            let name = entry.file_name().to_string_lossy().into_owned();
            if name.starts_with('.') || !entry.path().is_dir() {
                return None;
            }

            Some(DirectoryEntry {
                name,
                path: path_string(&entry.path()),
            })
        })
        .collect::<Vec<_>>();

    directories.sort_by(|a, b| a.name.to_lowercase().cmp(&b.name.to_lowercase()));

    Ok(DirectoryListing {
        current: path_string(&path),
        parent: path.parent().map(path_string),
        directories,
    })
}

pub async fn get_directory_structure(
    _owner: Owner,
    Query(query): Query<DirectoryQuery>,
) -> Result<Json<DirectoryListing>, FileBrowserError> {
    let path = match query.path {
        Some(path) if path.trim().is_empty() => return Err(FileBrowserError::InvalidPath),
        Some(path) => PathBuf::from(path),
        None => default_directory(),
    };

    let listing = spawn_blocking(move || enumerate_directory(path))
        .await
        .map_err(|_| FileBrowserError::Io)??;

    Ok(Json(listing))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn lists_only_visible_directories_with_navigation_metadata() {
        let root = env::temp_dir().join(format!("dim-filebrowser-{}", uuid::Uuid::new_v4()));
        fs::create_dir_all(root.join("Movies")).unwrap();
        fs::create_dir_all(root.join("TV Shows")).unwrap();
        fs::create_dir_all(root.join(".private")).unwrap();
        fs::write(root.join("notes.txt"), b"not a directory").unwrap();

        let listing = enumerate_directory(root.clone()).unwrap();

        assert_eq!(
            listing.current,
            path_string(&fs::canonicalize(&root).unwrap())
        );
        assert_eq!(
            listing
                .directories
                .iter()
                .map(|entry| entry.name.as_str())
                .collect::<Vec<_>>(),
            vec!["Movies", "TV Shows"]
        );
        assert_eq!(
            listing.parent,
            fs::canonicalize(&root).unwrap().parent().map(path_string)
        );

        fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn rejects_relative_and_non_directory_paths() {
        assert!(matches!(
            enumerate_directory(PathBuf::from("relative/path")),
            Err(FileBrowserError::InvalidPath)
        ));

        let file = env::temp_dir().join(format!("dim-filebrowser-{}", uuid::Uuid::new_v4()));
        fs::write(&file, b"file").unwrap();
        assert!(matches!(
            enumerate_directory(file.clone()),
            Err(FileBrowserError::NotDirectory)
        ));
        fs::remove_file(file).unwrap();
    }
}
