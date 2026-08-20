use std::env;
use std::fs;
use std::io;
use std::path::{Path as FsPath, PathBuf};

use axum::extract::Query;
use axum::response::{IntoResponse, Response};
use axum::Json;
use http::StatusCode;
use serde::{Deserialize, Serialize};
use sysinfo::Disks;
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

#[derive(Clone, Copy, Debug, Serialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum StorageRootKind {
    Fixed,
    Removable,
    Network,
}

#[derive(Debug, Serialize, PartialEq, Eq)]
pub struct StorageRoot {
    display_name: String,
    path: String,
    available_bytes: u64,
    kind: StorageRootKind,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Platform {
    Windows,
    MacOs,
    Linux,
    Other,
}

#[derive(Debug)]
struct StorageRootCandidate {
    name: String,
    path: PathBuf,
    file_system: String,
    available_bytes: u64,
    removable: bool,
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

fn current_platform() -> Platform {
    if cfg!(target_os = "windows") {
        Platform::Windows
    } else if cfg!(target_os = "macos") {
        Platform::MacOs
    } else if cfg!(target_os = "linux") {
        Platform::Linux
    } else {
        Platform::Other
    }
}

fn is_network_file_system(file_system: &str) -> bool {
    matches!(
        file_system.to_ascii_lowercase().as_str(),
        "9p" | "afpfs" | "cifs" | "davfs" | "nfs" | "nfs4" | "smbfs" | "sshfs"
    )
}

fn is_pseudo_file_system(file_system: &str) -> bool {
    matches!(
        file_system.to_ascii_lowercase().as_str(),
        "autofs"
            | "binfmt_misc"
            | "cgroup"
            | "cgroup2"
            | "configfs"
            | "debugfs"
            | "devpts"
            | "devtmpfs"
            | "fusectl"
            | "hugetlbfs"
            | "mqueue"
            | "proc"
            | "pstore"
            | "rpc_pipefs"
            | "securityfs"
            | "squashfs"
            | "sysfs"
            | "tmpfs"
            | "tracefs"
    )
}

fn has_path_prefix(path: &FsPath, prefix: &str) -> bool {
    path == FsPath::new(prefix) || path.starts_with(format!("{prefix}/"))
}

fn is_usable_mount(candidate: &StorageRootCandidate, platform: Platform) -> bool {
    match platform {
        Platform::Windows | Platform::Other => true,
        Platform::MacOs => {
            candidate.path == FsPath::new("/")
                || (candidate.path.starts_with("/Volumes")
                    && candidate.path.components().count() == 3)
        }
        Platform::Linux => {
            candidate.path == FsPath::new("/")
                || (!is_pseudo_file_system(&candidate.file_system)
                    && !["/boot", "/dev", "/proc", "/run", "/snap", "/sys"]
                        .iter()
                        .any(|prefix| has_path_prefix(&candidate.path, prefix)))
        }
    }
}

fn fallback_name(path: &FsPath, platform: Platform) -> String {
    match platform {
        Platform::Windows => path
            .to_string_lossy()
            .trim_end_matches(['\\', '/'])
            .to_owned(),
        Platform::MacOs if path == FsPath::new("/") => "System Volume".to_owned(),
        Platform::Linux if path == FsPath::new("/") => "System".to_owned(),
        _ => path
            .file_name()
            .filter(|name| !name.is_empty())
            .map(|name| name.to_string_lossy().into_owned())
            .unwrap_or_else(|| path.to_string_lossy().into_owned()),
    }
}

fn display_name(candidate: &StorageRootCandidate, platform: Platform) -> String {
    let name = candidate.name.trim();
    let device_like = name.is_empty()
        || name.starts_with("/dev/")
        || name.starts_with("\\\\?\\")
        || name.starts_with("disk");

    if device_like {
        fallback_name(&candidate.path, platform)
    } else {
        name.to_owned()
    }
}

fn normalize_storage_roots(
    candidates: Vec<StorageRootCandidate>,
    platform: Platform,
) -> Vec<StorageRoot> {
    let mut roots = candidates
        .into_iter()
        .filter(|candidate| is_usable_mount(candidate, platform))
        .map(|candidate| StorageRoot {
            display_name: display_name(&candidate, platform),
            path: path_string(&candidate.path),
            available_bytes: candidate.available_bytes,
            kind: if is_network_file_system(&candidate.file_system) {
                StorageRootKind::Network
            } else if candidate.removable {
                StorageRootKind::Removable
            } else {
                StorageRootKind::Fixed
            },
        })
        .collect::<Vec<_>>();

    roots.sort_by(|a, b| a.path.to_lowercase().cmp(&b.path.to_lowercase()));
    roots.dedup_by(|a, b| a.path.eq_ignore_ascii_case(&b.path));
    roots
}

pub fn discover_storage_roots() -> Vec<StorageRoot> {
    let candidates = Disks::new_with_refreshed_list()
        .iter()
        .map(|disk| StorageRootCandidate {
            name: disk.name().to_string_lossy().into_owned(),
            path: disk.mount_point().to_owned(),
            file_system: disk.file_system().to_string_lossy().into_owned(),
            available_bytes: disk.available_space(),
            removable: disk.is_removable(),
        })
        .filter(|candidate| is_readable_directory(&candidate.path))
        .collect();

    normalize_storage_roots(candidates, current_platform())
}

pub async fn get_storage_roots(_owner: Owner) -> Result<Json<Vec<StorageRoot>>, FileBrowserError> {
    let roots = spawn_blocking(discover_storage_roots)
        .await
        .map_err(|_| FileBrowserError::Io)?;
    Ok(Json(roots))
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

    fn candidate(
        name: &str,
        path: PathBuf,
        file_system: &str,
        available_bytes: u64,
        removable: bool,
    ) -> StorageRootCandidate {
        StorageRootCandidate {
            name: name.to_owned(),
            path,
            file_system: file_system.to_owned(),
            available_bytes,
            removable,
        }
    }

    #[test]
    fn normalizes_windows_drive_roots() {
        let roots = normalize_storage_roots(
            vec![
                candidate("Local Disk", PathBuf::from(r"C:\"), "NTFS", 100, false),
                candidate("USB", PathBuf::from(r"D:\"), "exFAT", 50, true),
            ],
            Platform::Windows,
        );

        assert_eq!(roots.len(), 2);
        assert_eq!(roots[0].display_name, "Local Disk");
        assert_eq!(roots[0].path, r"C:\");
        assert_eq!(roots[1].kind, StorageRootKind::Removable);
    }

    #[test]
    fn linux_filters_pseudo_mounts_and_keeps_real_volumes() {
        let media = PathBuf::from("/media/archive");
        let roots = normalize_storage_roots(
            vec![
                candidate("/dev/root", PathBuf::from("/"), "ext4", 100, false),
                candidate("proc", PathBuf::from("/proc"), "proc", 0, false),
                candidate("Media", media.clone(), "ext4", 80, false),
            ],
            Platform::Linux,
        );

        assert!(roots.iter().any(|root| root.path == "/"));
        assert!(roots.iter().any(|root| root.path == path_string(&media)));
        assert!(!roots.iter().any(|root| root.path == "/proc"));
    }

    #[test]
    fn macos_keeps_system_and_top_level_external_volumes_only() {
        let roots = normalize_storage_roots(
            vec![
                candidate("Macintosh HD", PathBuf::from("/"), "apfs", 100, false),
                candidate(
                    "Archive",
                    PathBuf::from("/Volumes/Archive"),
                    "apfs",
                    80,
                    true,
                ),
                candidate(
                    "Data",
                    PathBuf::from("/System/Volumes/Data"),
                    "apfs",
                    60,
                    false,
                ),
            ],
            Platform::MacOs,
        );

        assert_eq!(roots.len(), 2);
        assert!(roots.iter().any(|root| root.display_name == "Macintosh HD"));
        assert!(roots.iter().any(|root| root.display_name == "Archive"));
    }

    #[test]
    fn discovery_returns_readable_native_roots() {
        let roots = discover_storage_roots();

        assert!(!roots.is_empty());
        for root in roots {
            assert!(!root.display_name.is_empty());
            let path = PathBuf::from(&root.path);
            assert!(path.is_absolute());
            assert!(is_readable_directory(&path));
        }
    }
}
