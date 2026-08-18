//! Module contains all the code for the new generation media scanner.

pub mod daemon;
pub mod error;
mod mediafile;
pub mod movie;
#[cfg(test)]
mod tests;
pub mod tv_show;

use self::mediafile::Error as CreatorError;
use self::mediafile::MediafileCreator;
use crate::core::EventTx;

use async_trait::async_trait;

use dim_database::library::Library;
use dim_database::library::MediaType;
use dim_database::mediafile::InsertableMediaFile;
use dim_database::mediafile::MediaFile;

use dim_extern_api::filename::Anitomy;
use dim_extern_api::filename::CombinedExtractor;
use dim_extern_api::filename::FilenameMetadata;
use dim_extern_api::filename::Metadata;
use dim_extern_api::filename::TorrentMetadata;
use dim_extern_api::ExternalQueryIntoShow;

use futures::stream::FuturesUnordered;
use futures::StreamExt;
use ignore::WalkBuilder;
use itertools::Itertools;

use std::collections::{HashMap, HashSet};
use std::ffi::OsStr;
use std::path::Component;
use std::path::Path;
use std::path::PathBuf;
use std::sync::Arc;
use std::time::Instant;

use tracing::error;
use tracing::info;
use tracing::instrument;
use tracing::warn;

use once_cell::sync::Lazy;
use parking_lot::Mutex as ParkingMutex;
use tokio::sync::mpsc;
use tokio::sync::Mutex as AsyncMutex;

pub use error::Error;

pub(super) static SUPPORTED_EXTS: &[&str] = &[
    "001", "3g2", "3gp", "amv", "asf", "asx", "avi", "bin", "bivx", "divx", "dv", "dvr-ms", "f4v",
    "fli", "flv", "ifo", "img", "iso", "m2t", "m2ts", "m2v", "m4v", "mkv", "mk3d", "mov", "mp4",
    "mpe", "mpeg", "mpg", "mts", "mxf", "nrg", "nsv", "nuv", "ogg", "ogm", "ogv", "pva", "qt",
    "rec", "rm", "rmvb", "strm", "svq3", "tp", "ts", "ty", "viv", "vob", "vp3", "webm", "wmv",
    "wtv", "xvid",
];

static LIBRARY_SCAN_LOCKS: Lazy<ParkingMutex<HashMap<i64, Arc<AsyncMutex<()>>>>> =
    Lazy::new(|| ParkingMutex::new(HashMap::new()));

// The blocking walker may never get more than this far ahead of async filename parsing and file
// assessment. In particular, this bounds memory on slow/network filesystems and makes dropping a
// scan close the channel promptly instead of leaving a complete tree queued in memory.
const DISCOVERY_QUEUE_CAPACITY: usize = 64;
const ASSESSMENT_CONCURRENCY: usize = 4;
#[cfg(not(test))]
const HEARTBEAT_INTERVAL_SECONDS: u64 = 15;
#[cfg(test)]
const HEARTBEAT_INTERVAL_SECONDS: u64 = 1;

/// A single task owns periodic heartbeat persistence for a scan. Stage transitions are persisted
/// synchronously, while unchanged stages write at most once every 15 seconds. Aggregate counter
/// updates advance the same timestamp in their existing transactions, so active high-volume
/// discovery does not add writes.
struct ScanHeartbeat {
    conn: dim_database::DbConnection,
    scan_id: i64,
    stage: Arc<ParkingMutex<&'static str>>,
    handle: tokio::task::JoinHandle<()>,
}

impl ScanHeartbeat {
    fn start(conn: dim_database::DbConnection, scan_id: i64) -> Self {
        let stage = Arc::new(ParkingMutex::new("starting"));
        let task_stage = Arc::clone(&stage);
        let task_conn = conn.clone();
        let handle = tokio::spawn(async move {
            let mut interval =
                tokio::time::interval(std::time::Duration::from_secs(HEARTBEAT_INTERVAL_SECONDS));
            interval.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Skip);
            interval.tick().await;
            loop {
                interval.tick().await;
                let current_stage = *task_stage.lock();
                let mut lock = task_conn.writer().lock_owned().await;
                let result = async {
                    let mut tx = dim_database::write_tx(&mut lock).await?;
                    dim_database::ingestion::ScanRun::touch(
                        &mut tx,
                        scan_id,
                        current_stage,
                        HEARTBEAT_INTERVAL_SECONDS as i64,
                    )
                    .await?;
                    tx.commit().await?;
                    Ok::<_, dim_database::DatabaseError>(())
                }
                .await;
                if let Err(error) = result {
                    warn!(?error, scan_id, "Could not persist scan heartbeat");
                }
            }
        });
        Self {
            conn,
            scan_id,
            stage,
            handle,
        }
    }

    async fn stage(&self, stage: &'static str) -> Result<(), Error> {
        let changed = {
            let mut current = self.stage.lock();
            if *current == stage {
                false
            } else {
                *current = stage;
                true
            }
        };
        if !changed {
            return Ok(());
        }

        let mut lock = self.conn.writer().lock_owned().await;
        let mut tx = dim_database::write_tx(&mut lock).await?;
        dim_database::ingestion::ScanRun::touch(&mut tx, self.scan_id, stage, 0).await?;
        tx.commit().await?;
        Ok(())
    }
}

impl Drop for ScanHeartbeat {
    fn drop(&mut self) {
        self.handle.abort();
    }
}

fn library_scan_lock(library_id: i64) -> Arc<AsyncMutex<()>> {
    LIBRARY_SCAN_LOCKS
        .lock()
        .entry(library_id)
        .or_insert_with(|| Arc::new(AsyncMutex::new(())))
        .clone()
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct DiscoveredFile {
    pub path: PathBuf,
    pub supported: bool,
}

#[derive(Clone, Copy)]
enum ScanScope {
    Full,
    Incremental,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum RootAuthority {
    Authoritative,
    Missing,
}

#[derive(Debug)]
struct RootOutcome {
    root: PathBuf,
    normalized: Option<PathBuf>,
    authority: Option<RootAuthority>,
}

fn normalize_absolute(path: &Path) -> Option<PathBuf> {
    if !path.is_absolute() {
        return None;
    }
    let mut normalized = PathBuf::new();
    for component in path.components() {
        match component {
            Component::Prefix(prefix) => normalized.push(prefix.as_os_str()),
            Component::RootDir => normalized.push(component.as_os_str()),
            Component::CurDir => {}
            Component::ParentDir => {
                if !normalized.pop() {
                    return None;
                }
            }
            Component::Normal(part) => normalized.push(part),
        }
    }
    Some(normalized)
}

fn crosses_uncertain_symlink_boundary(root: &Path, path: &Path) -> bool {
    let Ok(relative) = path.strip_prefix(root) else {
        return true;
    };
    let Some(parent) = relative.parent() else {
        return true;
    };
    let mut cursor = root.to_path_buf();
    for component in parent.components() {
        cursor.push(component.as_os_str());
        match std::fs::symlink_metadata(&cursor) {
            Ok(metadata) if metadata.file_type().is_symlink() => return true,
            Ok(metadata) if metadata.is_dir() => {}
            _ => return true,
        }
    }
    false
}

fn authoritative_owner<'a>(path: &Path, roots: &'a [RootOutcome]) -> Option<&'a RootOutcome> {
    let normalized = normalize_absolute(path)?;
    if normalized != path {
        return None;
    }
    let mut owners = roots.iter().filter(|root| {
        root.normalized
            .as_deref()
            .is_some_and(|candidate| normalized != candidate && normalized.starts_with(candidate))
    });
    let owner = owners.next()?;
    if owners.next().is_some() || owner.normalized.as_deref() != Some(owner.root.as_path()) {
        return None;
    }
    match owner.authority? {
        RootAuthority::Missing => Some(owner),
        RootAuthority::Authoritative => {
            let root = owner.normalized.as_deref()?;
            if std::fs::symlink_metadata(root)
                .map(|metadata| metadata.file_type().is_symlink())
                .unwrap_or(true)
                || crosses_uncertain_symlink_boundary(root, &normalized)
            {
                None
            } else {
                Some(owner)
            }
        }
    }
}

fn walk_files_checked(
    paths: impl Iterator<Item = impl AsRef<Path>>,
    scope: ScanScope,
    emit: &mut dyn FnMut(DiscoveredFile) -> bool,
) -> Result<usize, Error> {
    let mut files = 0;
    for path in paths {
        let root = path.as_ref();
        let metadata = match std::fs::metadata(root) {
            Ok(metadata) => metadata,
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => {
                info!(path = ?root, "Library scan root no longer exists; treating it as authoritatively empty");
                continue;
            }
            Err(error) => {
                return Err(Error::FilesystemTraversal {
                    path: root.to_string_lossy().into_owned(),
                    message: error.to_string(),
                })
            }
        };
        if matches!(scope, ScanScope::Full) && !metadata.is_dir() {
            return Err(Error::FilesystemTraversal {
                path: root.to_string_lossy().into_owned(),
                message: "configured library root is not a directory".into(),
            });
        }

        for entry in WalkBuilder::new(root)
            .follow_links(true)
            .add_custom_ignore_filename(".plexignore")
            .build()
        {
            let entry = entry.map_err(|error| Error::FilesystemTraversal {
                path: root.to_string_lossy().into_owned(),
                message: error.to_string(),
            })?;
            if !entry.file_type().map_or(false, |kind| kind.is_file())
                || entry.path().iter().any(|part| {
                    part.to_str()
                        .map(|part| part.starts_with('.'))
                        .unwrap_or(false)
                })
            {
                continue;
            }
            let discovered = DiscoveredFile {
                supported: supported_path(entry.path()),
                path: entry.into_path(),
            };
            if !emit(discovered) {
                return Ok(files);
            }
            files += 1;
        }
    }
    Ok(files)
}

#[derive(Debug)]
struct DiscoveryStats {
    files: usize,
    elapsed: std::time::Duration,
}

fn spawn_discovery_worker<F>(
    walk: F,
) -> (
    mpsc::Receiver<DiscoveredFile>,
    tokio::task::JoinHandle<Result<DiscoveryStats, Error>>,
)
where
    F: FnOnce(&mut dyn FnMut(DiscoveredFile) -> bool) -> Result<usize, Error> + Send + 'static,
{
    let (sender, receiver) = mpsc::channel(DISCOVERY_QUEUE_CAPACITY);
    let worker = tokio::task::spawn_blocking(move || {
        let started = Instant::now();
        let mut emit = |file| sender.blocking_send(file).is_ok();
        let files = walk(&mut emit)?;
        Ok(DiscoveryStats {
            files,
            elapsed: started.elapsed(),
        })
    });
    (receiver, worker)
}

fn spawn_checked_discovery<T>(
    paths: Vec<T>,
    scope: ScanScope,
) -> (
    mpsc::Receiver<DiscoveredFile>,
    tokio::task::JoinHandle<Result<DiscoveryStats, Error>>,
)
where
    T: AsRef<Path> + Send + 'static,
{
    spawn_discovery_worker(move |emit| walk_files_checked(paths.into_iter(), scope, emit))
}

fn finish_discovery_worker(
    result: Result<Result<DiscoveryStats, Error>, tokio::task::JoinError>,
) -> Result<DiscoveryStats, Error> {
    result.map_err(|error| Error::FilesystemTraversal {
        path: "scanner worker".into(),
        message: error.to_string(),
    })?
}

fn supported_path(path: &Path) -> bool {
    path.extension()
        .and_then(|extension| extension.to_str())
        .map(|extension| extension.to_ascii_lowercase())
        .map_or(false, |extension| {
            SUPPORTED_EXTS.contains(&extension.as_str())
        })
}

/// Discovery does not discard unsupported files. Keeping them as classified results makes mixed
/// scans observable while only supported candidates advance to probing.
pub fn discover_files(paths: impl Iterator<Item = impl AsRef<Path>>) -> Vec<DiscoveredFile> {
    let mut files = Vec::with_capacity(2048);
    for path in paths {
        files.extend(
            WalkBuilder::new(path)
                .follow_links(true)
                .add_custom_ignore_filename(".plexignore")
                .build()
                .filter_map(Result::ok)
                .filter(|entry| entry.file_type().map_or(false, |kind| kind.is_file()))
                .filter(|entry| {
                    !entry.path().iter().any(|part| {
                        part.to_str()
                            .map(|part| part.starts_with('.'))
                            .unwrap_or(false)
                    })
                })
                .map(|entry| DiscoveredFile {
                    supported: supported_path(entry.path()),
                    path: entry.into_path(),
                }),
        );
    }
    files
}

fn parse_filename(file: &Path) -> Option<Vec<Metadata>> {
    let filename = match file.file_stem().and_then(OsStr::to_str) {
        Some(filename) => filename,
        None => {
            warn!(file = ?file, "Received a filename that is not unicode");
            return None;
        }
    };

    let metadata = IntoIterator::into_iter([
        TorrentMetadata::from_str(filename),
        Anitomy::from_str(filename),
        CombinedExtractor::from_str(filename),
    ])
    .filter_map(|metadata| metadata)
    .collect::<Vec<_>>();

    if metadata.is_empty() {
        warn!(file = ?file, "Failed to parse the filename and extract metadata.");
        None
    } else {
        Some(metadata)
    }
}

/// Function recursively walks the paths passed and returns all files in those directories.
/// FIXME: THIS IS NOT ASYNC-SAFE!!!
/// NOTE: I've noticed that walking a directory mounted over ssh is very slow, 80 files in like 300
/// seconds. Doubt theres a way to fix this but we could alliviate the UX-degradation by sending
/// the files over a channel instead of returning them at once.
pub fn get_subfiles(paths: impl Iterator<Item = impl AsRef<Path>>) -> Vec<PathBuf> {
    discover_files(paths)
        .into_iter()
        .filter(|file| file.supported)
        .map(|file| file.path)
        .collect()
}

pub fn parse_filenames(
    files: impl Iterator<Item = impl AsRef<Path>>,
) -> Vec<(PathBuf, Vec<Metadata>)> {
    let mut metadata = Vec::new();

    for file in files {
        if let Some(parsed) = parse_filename(file.as_ref()) {
            metadata.push((file.as_ref().into(), parsed));
        }
    }

    metadata
}

pub struct WorkUnit(pub MediaFile, pub Vec<Metadata>);

/// Trait that must be implemented by a media matcher. Matchers are responsible for fetching their
/// own external metadata but it is provided a metadata provider at initialization time.
#[async_trait]
pub trait MediaMatcher: Send + Sync {
    async fn batch_match(
        &self,
        tx: &mut dim_database::Transaction<'_>,
        provider: Arc<dyn ExternalQueryIntoShow>,
        work: Vec<WorkUnit>,
    ) -> Result<(), Error>;

    /// Scanner-specific matching keeps remote provider waits outside the single SQLite writer
    /// ownership window, allowing durable heartbeats to continue while metadata is slow.
    async fn batch_match_durable(
        &self,
        conn: &dim_database::DbConnection,
        provider: Arc<dyn ExternalQueryIntoShow>,
        work: Vec<WorkUnit>,
    ) -> Result<(), Error>;

    /// Match a WorkUnit to a specific external id.
    async fn match_to_id(
        &self,
        tx: &mut dim_database::Transaction<'_>,
        provider: Arc<dyn ExternalQueryIntoShow>,
        work: WorkUnit,
        external_id: &str,
    ) -> Result<(), Error>;
}

pub async fn insert_mediafiles(
    conn: &mut dim_database::DbConnection,
    library_id: i64,
    dirs: Vec<impl AsRef<Path> + Send + 'static>,
) -> Result<Vec<WorkUnit>, Error> {
    insert_mediafiles_for_scan(
        conn,
        library_id,
        dirs,
        None,
        None,
        ScanScope::Incremental,
        false,
    )
    .await
}

async fn update_item(
    conn: &dim_database::DbConnection,
    scan_id: i64,
    library_id: i64,
    root_id: Option<i64>,
    path: &Path,
    fingerprint: Option<&str>,
    stage: &str,
    status: &str,
    error_class: Option<&str>,
    error_message: Option<&str>,
) -> Result<(), Error> {
    let Some(path) = path.to_str() else {
        return Ok(());
    };
    let mut lock = conn.writer().lock_owned().await;
    let mut tx = dim_database::write_tx(&mut lock).await?;
    dim_database::ingestion::upsert_item(
        &mut tx,
        scan_id,
        library_id,
        root_id,
        path,
        fingerprint,
        stage,
        status,
        error_class,
        error_message,
    )
    .await?;
    tx.commit().await?;
    Ok(())
}

async fn handle_assessment(
    conn: &dim_database::DbConnection,
    library_id: i64,
    scan_id: Option<i64>,
    root_id: Option<i64>,
    insertables: &mut Vec<(InsertableMediaFile, Vec<Metadata>)>,
    existing_work: &mut Vec<WorkUnit>,
    reprocess_existing: bool,
    (path, result, metadata): (
        PathBuf,
        Result<InsertableMediaFile, CreatorError>,
        Vec<Metadata>,
    ),
) -> Result<(), Error> {
    match result {
        Ok(insertable) => insertables.push((insertable, metadata)),
        Err(CreatorError::FileExists) => {
            let existing = {
                let mut tx = conn.read().begin().await?;
                MediaFile::get_by_file(&mut tx, path.to_string_lossy().as_ref()).await?
            };
            {
                let mut lock = conn.writer().lock_owned().await;
                let mut tx = dim_database::write_tx(&mut lock).await?;
                sqlx::query("UPDATE mediafile SET missing_since = NULL WHERE library_id = ? AND target_file = ?")
                    .bind(library_id)
                    .bind(path.to_string_lossy().into_owned())
                    .execute(&mut tx)
                    .await?;
                tx.commit().await?;
            }
            // Discovery and probing are intentionally skipped for catalogue rows, but an earlier
            // metadata failure may have left the row unattached. Feed those rows back through the
            // matcher so a rescan can recover every file instead of permanently skipping it.
            if existing.media_id.is_none() || reprocess_existing {
                existing_work.push(WorkUnit(existing, metadata));
                return Ok(());
            }
            if let Some(scan_id) = scan_id {
                update_item(
                    conn,
                    scan_id,
                    library_id,
                    root_id,
                    &path,
                    None,
                    "commit",
                    "skipped",
                    Some("already_catalogued"),
                    None,
                )
                .await?;
                let mut lock = conn.writer().lock_owned().await;
                let mut tx = dim_database::write_tx(&mut lock).await?;
                dim_database::ingestion::ScanRun::count(&mut tx, scan_id, "skipped").await?;
                dim_database::ingestion::ScanRun::count(&mut tx, scan_id, "processed").await?;
                tx.commit().await?;
            }
        }
        Err(error) => {
            if let Some(scan_id) = scan_id {
                let retryable = error.retryable();
                let status = if retryable { "retryable" } else { "failed" };
                let stage = if matches!(
                    &error,
                    CreatorError::FileUnstable | CreatorError::FileMissing
                ) {
                    "stability"
                } else {
                    "probing"
                };
                update_item(
                    conn,
                    scan_id,
                    library_id,
                    root_id,
                    &path,
                    None,
                    stage,
                    status,
                    Some(error.class()),
                    Some(&error.to_string()),
                )
                .await?;
                let mut lock = conn.writer().lock_owned().await;
                let mut tx = dim_database::write_tx(&mut lock).await?;
                dim_database::ingestion::ScanRun::count(&mut tx, scan_id, "processed").await?;
                if !retryable {
                    dim_database::ingestion::ScanRun::count(&mut tx, scan_id, "failed").await?;
                }
                tx.commit().await?;
            }
            warn!(
                ?error,
                "Skipping media candidate after classified assessment failure"
            );
        }
    }
    Ok(())
}

async fn record_discovery(
    conn: dim_database::DbConnection,
    scan_id: i64,
    library_id: i64,
    root_id: Option<i64>,
    path: Option<String>,
    supported: bool,
) -> Result<(), Error> {
    // This task is intentionally detached from cancellation of the scan future. It owns the
    // writer transaction, so an abort cannot strand the SQLite connection before the terminal
    // scan guard records cancellation.
    tokio::spawn(async move {
        let mut lock = conn.writer().lock_owned().await;
        let mut tx = dim_database::write_tx(&mut lock).await?;
        let (status, class) = if supported {
            ("complete", None)
        } else {
            ("skipped", Some("unsupported_format"))
        };
        if let Some(path) = path {
            dim_database::ingestion::upsert_item(
                &mut tx,
                scan_id,
                library_id,
                root_id,
                &path,
                None,
                "discovery",
                status,
                class,
                None,
            )
            .await?;
        }
        dim_database::ingestion::ScanRun::count(&mut tx, scan_id, "discovered").await?;
        if let Some(root_id) = root_id {
            sqlx::query("UPDATE ingestion_scan_root SET discovered = discovered + 1 WHERE id = ?")
                .bind(root_id)
                .execute(&mut tx)
                .await?;
        }
        if !supported {
            dim_database::ingestion::ScanRun::count(&mut tx, scan_id, "skipped").await?;
            dim_database::ingestion::ScanRun::count(&mut tx, scan_id, "processed").await?;
        }
        tx.commit().await?;
        Ok::<_, Error>(())
    })
    .await
    .map_err(|error| Error::FilesystemTraversal {
        path: "discovery state worker".into(),
        message: error.to_string(),
    })?
}

async fn insert_mediafiles_for_scan(
    conn: &mut dim_database::DbConnection,
    library_id: i64,
    dirs: Vec<impl AsRef<Path> + Send + 'static>,
    scan_id: Option<i64>,
    root_id: Option<i64>,
    scope: ScanScope,
    reprocess_existing: bool,
) -> Result<Vec<WorkUnit>, Error> {
    let (mut discovered, mut discovery_worker) = spawn_checked_discovery(dirs, scope);
    let mut discovery_stats = None;
    let mut instance = MediafileCreator::new(conn.clone(), library_id).await;
    let mut assessments = FuturesUnordered::new();
    let mut insertables = Vec::new();
    let mut existing_work = Vec::new();
    let mut max_in_flight = 0;

    loop {
        let file = if discovery_stats.is_some() {
            discovered.recv().await
        } else {
            tokio::select! {
                biased;
                result = &mut discovery_worker => {
                    match finish_discovery_worker(result) {
                        Ok(stats) => {
                            discovery_stats = Some(stats);
                            continue;
                        }
                        Err(error) => return Err(error),
                    }
                }
                file = discovered.recv() => file,
            }
        };
        let Some(file) = file else {
            break;
        };

        if let Some(scan_id) = scan_id {
            record_discovery(
                conn.clone(),
                scan_id,
                library_id,
                root_id,
                file.path.to_str().map(str::to_owned),
                file.supported,
            )
            .await?;
        }

        if !file.supported {
            continue;
        }

        let Some(metadata) = parse_filename(&file.path) else {
            if let Some(scan_id) = scan_id {
                update_item(
                    conn,
                    scan_id,
                    library_id,
                    root_id,
                    &file.path,
                    None,
                    "matching",
                    "failed",
                    Some("filename_unparseable"),
                    None,
                )
                .await?;
                let mut lock = conn.writer().lock_owned().await;
                let mut tx = dim_database::write_tx(&mut lock).await?;
                dim_database::ingestion::ScanRun::count(&mut tx, scan_id, "failed").await?;
                dim_database::ingestion::ScanRun::count(&mut tx, scan_id, "processed").await?;
                tx.commit().await?;
            }
            continue;
        };

        let primary = metadata[0].clone();
        let path = file.path;
        let assessed_path = path.clone();
        assessments.push(async {
            (
                assessed_path,
                instance.construct_mediafile(path, primary).await,
                metadata,
            )
        });
        max_in_flight = max_in_flight.max(assessments.len());

        if assessments.len() >= ASSESSMENT_CONCURRENCY {
            let outcome = if discovery_stats.is_some() {
                assessments
                    .next()
                    .await
                    .expect("a full assessment set cannot be empty")
            } else {
                tokio::select! {
                    biased;
                    result = &mut discovery_worker => {
                        match finish_discovery_worker(result) {
                            Ok(stats) => {
                                discovery_stats = Some(stats);
                                assessments
                                    .next()
                                    .await
                                    .expect("a full assessment set cannot be empty")
                            }
                            Err(error) => return Err(error),
                        }
                    }
                    outcome = assessments.next() => {
                        outcome.expect("a full assessment set cannot be empty")
                    }
                }
            };
            handle_assessment(
                conn,
                library_id,
                scan_id,
                root_id,
                &mut insertables,
                &mut existing_work,
                reprocess_existing,
                outcome,
            )
            .await?;
        }
    }

    // A closed channel only means that the walker stopped. Its result is the authority signal:
    // never insert newly assessed rows (and therefore never reconcile) until it confirms that all
    // roots completed without a traversal error.
    let discovery = match discovery_stats {
        Some(stats) => stats,
        None => finish_discovery_worker(discovery_worker.await)?,
    };

    while let Some(outcome) = assessments.next().await {
        handle_assessment(
            conn,
            library_id,
            scan_id,
            root_id,
            &mut insertables,
            &mut existing_work,
            reprocess_existing,
            outcome,
        )
        .await?;
    }
    drop(assessments);

    info!(
        elapsed_ms = discovery.elapsed.as_millis(),
        files = discovery.files,
        queue_capacity = DISCOVERY_QUEUE_CAPACITY,
        max_in_flight,
        assessment_concurrency = ASSESSMENT_CONCURRENCY,
        "Streamed all target directories."
    );

    let mut mediafiles = vec![];

    for chunk in insertables.chunks(256) {
        mediafiles.append(
            &mut instance
                .insert_batch(chunk.iter().map(|(file, _)| file))
                .await?,
        );
    }

    let mut metadata_by_path = insertables
        .into_iter()
        .map(|(file, metadata)| (file.target_file, metadata))
        .collect::<HashMap<_, _>>();

    let mut work = mediafiles
        .into_iter()
        .map(|mfile| {
            let metadata = metadata_by_path
                .remove(&mfile.target_file)
                .expect("inserted mediafile must retain its parsed metadata");
            WorkUnit(mfile, metadata)
        })
        .collect::<Vec<_>>();
    work.append(&mut existing_work);
    work.sort_by(|left, right| left.0.target_file.cmp(&right.0.target_file));

    if let Some(scan_id) = scan_id {
        let mut lock = conn.writer().lock_owned().await;
        let mut tx = dim_database::write_tx(&mut lock).await?;
        for unit in &work {
            let fingerprint = match (unit.0.file_size, unit.0.modified_ns) {
                (Some(size), Some(modified)) => Some(format!("{size}:{modified}")),
                _ => None,
            };
            dim_database::ingestion::upsert_item(
                &mut tx,
                scan_id,
                library_id,
                root_id,
                &unit.0.target_file,
                fingerprint.as_deref(),
                "commit",
                "complete",
                None,
                None,
            )
            .await?;
        }
        tx.commit().await?;
    }
    Ok(work)
}

fn tv_metadata_candidates(
    path: &Path,
    roots: &[RootOutcome],
    parsed: &[Metadata],
) -> Vec<Metadata> {
    let path = normalize_absolute(path).unwrap_or_else(|| path.to_path_buf());
    let root = roots
        .iter()
        .filter_map(|root| root.normalized.as_deref())
        .filter(|root| path.starts_with(root))
        .max_by_key(|root| root.components().count());
    let Some(root) = root else {
        return parsed.to_vec();
    };
    let Some(show_directory) = path
        .strip_prefix(root)
        .ok()
        .and_then(|relative| relative.components().next())
        .map(|component| component.as_os_str())
    else {
        return parsed.to_vec();
    };
    let Some(folder_metadata) = parse_filename(Path::new(show_directory)) else {
        return parsed.to_vec();
    };

    let mut candidates = Vec::new();
    let mut seen = HashSet::new();
    let folder_name = show_directory.to_string_lossy();
    let mut name_parts = folder_name.split_whitespace().collect::<Vec<_>>();
    if name_parts.last().is_some_and(|part| {
        part.strip_prefix('S')
            .or_else(|| part.strip_prefix('s'))
            .is_some_and(|season| !season.is_empty() && season.chars().all(|c| c.is_ascii_digit()))
    }) {
        name_parts.pop();
    }
    let normalized_folder_name = name_parts.join(" ");
    if !normalized_folder_name.is_empty() {
        for episode in parsed {
            let candidate = Metadata {
                name: normalized_folder_name.clone(),
                year: folder_metadata
                    .first()
                    .and_then(|folder| folder.year)
                    .or(episode.year),
                season: episode.season,
                episode: episode.episode,
            };
            if candidate.episode.is_some() && seen.insert(candidate.clone()) {
                candidates.push(candidate);
            }
        }
    }
    for folder in folder_metadata {
        for episode in parsed {
            let candidate = Metadata {
                name: folder.name.clone(),
                year: folder.year.or(episode.year),
                season: episode.season.or(folder.season),
                episode: episode.episode,
            };
            if candidate.episode.is_some() && seen.insert(candidate.clone()) {
                candidates.push(candidate);
            }
        }
    }
    for candidate in parsed {
        if seen.insert(candidate.clone()) {
            candidates.push(candidate.clone());
        }
    }
    candidates
}

#[cfg(test)]
mod tv_metadata_candidate_tests {
    use super::*;

    #[test]
    fn uses_show_folder_identity_with_episode_numbers_from_filename() {
        let root = PathBuf::from("/media/shows");
        let roots = vec![RootOutcome {
            root: root.clone(),
            normalized: Some(root),
            authority: Some(RootAuthority::Authoritative),
        }];
        let parsed = parse_filename(Path::new("S01E02 - Cat's in the Bag....mkv")).unwrap();
        let candidates = tv_metadata_candidates(
            Path::new("/media/shows/Breaking Bad S01/S01E02 - Cat's in the Bag....mkv"),
            &roots,
            &parsed,
        );

        assert_eq!(candidates[0].name, "Breaking Bad");
        assert_eq!(candidates[0].season, Some(1));
        assert_eq!(candidates[0].episode, Some(2));
        assert!(candidates
            .iter()
            .any(|candidate| candidate.name.contains("Cat")));
    }
}

async fn reconcile_library(
    conn: &dim_database::DbConnection,
    library_id: i64,
    scan_id: i64,
    roots: &[RootOutcome],
) -> Result<(), Error> {
    let mut lock = conn.writer().lock_owned().await;
    let mut tx = dim_database::write_tx(&mut lock).await?;

    let discovered = sqlx::query_scalar::<_, String>(
        "SELECT path FROM ingestion_item WHERE scan_id = ? AND library_id = ? AND COALESCE(error_class, '') != 'unsupported_format'",
    )
    .bind(scan_id)
    .bind(library_id)
    .fetch_all(&mut *tx)
    .await?
    .into_iter()
    .collect::<HashSet<_>>();
    let catalogue = sqlx::query_as::<_, (i64, String)>(
        "SELECT id, target_file FROM mediafile WHERE library_id = ?",
    )
    .bind(library_id)
    .fetch_all(&mut *tx)
    .await?;
    let mut removed_files = 0;
    for (id, path) in catalogue {
        if discovered.contains(&path) || authoritative_owner(Path::new(&path), roots).is_none() {
            continue;
        }
        removed_files += sqlx::query("DELETE FROM mediafile WHERE id = ?")
            .bind(id)
            .execute(&mut *tx)
            .await?
            .rows_affected();
    }

    // Remove catalogue parents only after their filesystem children have been reconciled. The
    // ordering preserves valid TV hierarchies while cleaning childless episodes, seasons, shows,
    // and movies in the same transaction as the stale mediafile deletion.
    sqlx::query(
        "DELETE FROM _tblmedia
         WHERE library_id = ? AND media_type = 'episode'
           AND NOT EXISTS (SELECT 1 FROM mediafile WHERE mediafile.media_id = _tblmedia.id)",
    )
    .bind(library_id)
    .execute(&mut tx)
    .await?;
    sqlx::query(
        "DELETE FROM _tblseason
         WHERE NOT EXISTS (SELECT 1 FROM episode WHERE episode.seasonid = _tblseason.id)
           AND tvshowid IN (SELECT id FROM _tblmedia WHERE library_id = ?)",
    )
    .bind(library_id)
    .execute(&mut tx)
    .await?;
    let removed_media = sqlx::query(
        "DELETE FROM _tblmedia
         WHERE library_id = ? AND media_type != 'episode'
           AND NOT EXISTS (SELECT 1 FROM mediafile WHERE mediafile.media_id = _tblmedia.id)
           AND NOT EXISTS (SELECT 1 FROM _tblseason WHERE _tblseason.tvshowid = _tblmedia.id)",
    )
    .bind(library_id)
    .execute(&mut tx)
    .await?
    .rows_affected();

    tx.commit().await?;
    info!(
        library_id,
        scan_id,
        removed_files,
        removed_media,
        "Reconciled library catalogue with authoritative filesystem scan"
    );
    Ok(())
}

struct ScanRunGuard {
    conn: dim_database::DbConnection,
    events: EventTx,
    library_id: i64,
    scan_id: i64,
    terminal: bool,
}

impl ScanRunGuard {
    fn new(
        conn: dim_database::DbConnection,
        events: EventTx,
        library_id: i64,
        scan_id: i64,
    ) -> Self {
        Self {
            conn,
            events,
            library_id,
            scan_id,
            terminal: false,
        }
    }

    async fn finish(&mut self, status: &str, error: Option<&str>) -> Result<(), Error> {
        let mut lock = self.conn.writer().lock_owned().await;
        let mut tx = dim_database::write_tx(&mut lock).await?;
        if status != "complete" {
            dim_database::ingestion::cancel_active_roots(&mut tx, self.scan_id).await?;
        }
        dim_database::ingestion::ScanRun::finish(&mut tx, self.scan_id, status, error).await?;
        tx.commit().await?;
        self.terminal = true;

        let event_type = match status {
            "complete" => dim_events::PushEventType::EventStoppedScanning,
            "cancelled" => dim_events::PushEventType::EventScanCancelled,
            _ => dim_events::PushEventType::EventScanFailed,
        };
        if let Err(event_error) = self.events.try_send(
            dim_events::Message {
                id: self.library_id,
                event_type,
            }
            .to_string(),
        ) {
            warn!(
                ?event_error,
                library_id = self.library_id,
                scan_id = self.scan_id,
                "Could not publish terminal scan event"
            );
        }
        Ok(())
    }
}

impl Drop for ScanRunGuard {
    fn drop(&mut self) {
        if self.terminal {
            return;
        }
        let conn = self.conn.clone();
        let events = self.events.clone();
        let library_id = self.library_id;
        let scan_id = self.scan_id;
        if let Ok(runtime) = tokio::runtime::Handle::try_current() {
            runtime.spawn(async move {
                let mut lock = conn.writer().lock_owned().await;
                match dim_database::write_tx(&mut lock).await {
                    Ok(mut tx) => {
                        if let Err(error) = dim_database::ingestion::ScanRun::finish(
                            &mut tx,
                            scan_id,
                            "cancelled",
                            Some("scan task was cancelled before reaching a terminal state"),
                        )
                        .await
                        {
                            error!(
                                ?error,
                                library_id, scan_id, "Failed to mark cancelled scan terminal"
                            );
                            return;
                        }
                        if let Err(error) =
                            dim_database::ingestion::cancel_active_roots(&mut tx, scan_id).await
                        {
                            error!(
                                ?error,
                                library_id, scan_id, "Failed to mark cancelled scan roots terminal"
                            );
                            return;
                        }
                        if let Err(error) = tx.commit().await {
                            error!(
                                ?error,
                                library_id, scan_id, "Failed to commit cancelled scan state"
                            );
                            return;
                        }
                        let _ = events.try_send(
                            dim_events::Message {
                                id: library_id,
                                event_type: dim_events::PushEventType::EventScanCancelled,
                            }
                            .to_string(),
                        );
                    }
                    Err(error) => error!(
                        ?error,
                        library_id, scan_id, "Failed to open cancelled scan transaction"
                    ),
                };
            });
        }
    }
}

#[instrument(skip(conn, dirs, tx))]
pub async fn start_custom(
    conn: &mut dim_database::DbConnection,
    library_id: i64,
    dirs: Vec<impl AsRef<Path> + Send + 'static>,
    tx: EventTx,
    media_type: MediaType,
    provider: Arc<dyn ExternalQueryIntoShow>,
) -> Result<(), Error> {
    start_scoped_custom(
        conn,
        library_id,
        dirs,
        tx,
        media_type,
        provider,
        ScanScope::Full,
    )
    .await
}

async fn start_incremental_custom(
    conn: &mut dim_database::DbConnection,
    library_id: i64,
    dirs: Vec<impl AsRef<Path> + Send + 'static>,
    tx: EventTx,
    media_type: MediaType,
    provider: Arc<dyn ExternalQueryIntoShow>,
) -> Result<(), Error> {
    start_scoped_custom(
        conn,
        library_id,
        dirs,
        tx,
        media_type,
        provider,
        ScanScope::Incremental,
    )
    .await
}

async fn start_scoped_custom(
    conn: &mut dim_database::DbConnection,
    library_id: i64,
    dirs: Vec<impl AsRef<Path> + Send + 'static>,
    tx: EventTx,
    media_type: MediaType,
    provider: Arc<dyn ExternalQueryIntoShow>,
    scope: ScanScope,
) -> Result<(), Error> {
    // Watcher reconciliation and user-requested scans share the same per-library ownership gate.
    // Cancellation drops this guard; the next owner then records the abandoned durable run.
    let _scan_owner = library_scan_lock(library_id).lock_owned().await;
    let dirs = dirs
        .into_iter()
        .map(|path| path.as_ref().to_path_buf())
        .collect::<Vec<_>>();
    let (scan_id, root_ids) = {
        let mut lock = conn.writer().lock_owned().await;
        let mut db_tx = dim_database::write_tx(&mut lock).await?;
        let kind = if matches!(scope, ScanScope::Full) {
            "full"
        } else {
            "watcher"
        };
        let id = dim_database::ingestion::ScanRun::begin(&mut db_tx, library_id, kind).await?;
        let mut root_ids = Vec::with_capacity(dirs.len());
        for (ordinal, root) in dirs.iter().enumerate() {
            let normalized = normalize_absolute(root);
            root_ids.push(
                dim_database::ingestion::begin_root(
                    &mut db_tx,
                    id,
                    ordinal as i64,
                    &root.to_string_lossy(),
                    normalized.as_ref().and_then(|path| path.to_str()),
                )
                .await?,
            );
        }
        db_tx.commit().await?;
        (id, root_ids)
    };
    let mut guard = ScanRunGuard::new(conn.clone(), tx.clone(), library_id, scan_id);
    let heartbeat = ScanHeartbeat::start(conn.clone(), scan_id);

    let result = run_scan_custom(
        conn, library_id, dirs, root_ids, tx, media_type, provider, scan_id, scope, &heartbeat,
    )
    .await;
    drop(heartbeat);

    let error_message = result.as_ref().err().map(ToString::to_string);
    guard
        .finish(
            if result.is_ok() { "complete" } else { "failed" },
            error_message.as_deref(),
        )
        .await?;
    result
}

async fn run_scan_custom(
    conn: &mut dim_database::DbConnection,
    library_id: i64,
    dirs: Vec<PathBuf>,
    root_ids: Vec<i64>,
    tx: EventTx,
    media_type: MediaType,
    provider: Arc<dyn ExternalQueryIntoShow>,
    scan_id: i64,
    scope: ScanScope,
    heartbeat: &ScanHeartbeat,
) -> Result<(), Error> {
    info!(library_id, "Scanning library");

    if let Err(error) = tx.try_send(
        dim_events::Message {
            id: library_id,
            event_type: dim_events::PushEventType::EventStartedScanning,
        }
        .to_string(),
    ) {
        warn!(?error, library_id, "Could not publish scan start event");
    }

    let matcher = match media_type {
        MediaType::Movie => Arc::new(movie::MovieMatcher) as Arc<dyn MediaMatcher>,
        MediaType::Tv => Arc::new(tv_show::TvMatcher) as Arc<dyn MediaMatcher>,
        _ => unimplemented!(),
    };

    let now = Instant::now();
    let mut workunits = Vec::new();
    let mut roots = Vec::with_capacity(dirs.len());
    let mut failures = Vec::new();
    heartbeat.stage("traversal").await?;
    for (root, root_id) in dirs.into_iter().zip(root_ids) {
        let normalized = normalize_absolute(&root);
        let was_missing = matches!(
            std::fs::metadata(&root),
            Err(error) if error.kind() == std::io::ErrorKind::NotFound
        );
        match insert_mediafiles_for_scan(
            conn,
            library_id,
            vec![root.clone()],
            Some(scan_id),
            Some(root_id),
            scope,
            matches!(scope, ScanScope::Full) && media_type == MediaType::Tv,
        )
        .await
        {
            Ok(mut root_work) => {
                let authority = if was_missing {
                    RootAuthority::Missing
                } else {
                    RootAuthority::Authoritative
                };
                let status = if was_missing {
                    "missing"
                } else {
                    "authoritative"
                };
                let mut lock = conn.writer().lock_owned().await;
                let mut root_tx = dim_database::write_tx(&mut lock).await?;
                dim_database::ingestion::finish_root(&mut root_tx, root_id, status, None).await?;
                root_tx.commit().await?;
                roots.push(RootOutcome {
                    root,
                    normalized,
                    authority: Some(authority),
                });
                workunits.append(&mut root_work);
            }
            Err(Error::FilesystemTraversal { path, message }) => {
                let diagnostic = format!("{path}: {message}");
                let mut lock = conn.writer().lock_owned().await;
                let mut root_tx = dim_database::write_tx(&mut lock).await?;
                dim_database::ingestion::finish_root(
                    &mut root_tx,
                    root_id,
                    "failed",
                    Some(&diagnostic),
                )
                .await?;
                root_tx.commit().await?;
                failures.push(diagnostic);
                roots.push(RootOutcome {
                    root,
                    normalized,
                    authority: None,
                });
            }
            Err(error) => return Err(error),
        }
    }
    let workunits_size = workunits.len();

    if media_type == MediaType::Tv {
        for work in &mut workunits {
            work.1 = tv_metadata_candidates(Path::new(&work.0.target_file), &roots, &work.1);
        }
    }

    info!(
        library_id,
        units = workunits_size,
        elapsed_ms = now.elapsed().as_millis(),
        "Walked and inserted mediafiles."
    );

    // NOTE: itertools::GroupBy is used across an await point and thus must also be Sync. This
    // breaks some of our higher-level logic where we spawn this task. Thus we collect it before
    // we proceed consuming it.
    let chunk_iter = workunits
        .into_iter()
        .chunks(128)
        .into_iter()
        .map(|x| x.collect::<Vec<_>>())
        .collect::<Vec<_>>();

    // TODO: We can receive work over a channel so that we can in parallel create new mediafiles
    // and match objects.
    heartbeat.stage("matching").await?;
    for unit in chunk_iter.into_iter() {
        let identities = unit
            .iter()
            .map(|work| (work.0.id, work.0.target_file.clone()))
            .collect::<Vec<_>>();
        {
            let mut state_lock = conn.writer().lock_owned().await;
            let mut state_tx = dim_database::write_tx(&mut state_lock).await?;
            for (_, path) in &identities {
                dim_database::ingestion::upsert_item(
                    &mut state_tx,
                    scan_id,
                    library_id,
                    None,
                    path,
                    None,
                    "matching",
                    "running",
                    None,
                    None,
                )
                .await?;
            }
            state_tx.commit().await?;
        }
        let match_failure = match matcher
            .batch_match_durable(conn, provider.clone(), unit)
            .await
        {
            Ok(()) => None,
            Err(e) => {
                let class = if matches!(e, Error::MetadataProviderFailure) {
                    "metadata_provider_failure"
                } else {
                    "metadata_commit_failure"
                };
                error!(error = ?e, class, "Failed to match batch of mediafiles.");
                Some(class)
            }
        };

        let matched = {
            let mut read_tx = conn.read().begin().await?;
            let mut matched = Vec::with_capacity(identities.len());
            for (id, path) in &identities {
                matched.push((
                    path.clone(),
                    MediaFile::get_one(&mut read_tx, *id)
                        .await?
                        .media_id
                        .is_some(),
                ));
            }
            matched
        };
        let mut count_lock = conn.writer().lock_owned().await;
        let mut count_tx = dim_database::write_tx(&mut count_lock).await?;
        for (path, did_match) in matched {
            let (status, class) = if did_match {
                ("complete", None)
            } else {
                ("failed", Some(match_failure.unwrap_or("metadata_no_match")))
            };
            dim_database::ingestion::upsert_item(
                &mut count_tx,
                scan_id,
                library_id,
                None,
                &path,
                None,
                "matching",
                status,
                class,
                None,
            )
            .await?;
            dim_database::ingestion::ScanRun::count(
                &mut count_tx,
                scan_id,
                if did_match { "committed" } else { "failed" },
            )
            .await?;
            dim_database::ingestion::ScanRun::count(&mut count_tx, scan_id, "processed").await?;
        }
        count_tx.commit().await?;
    }

    if matches!(scope, ScanScope::Full) {
        heartbeat.stage("reconciliation").await?;
        reconcile_library(conn, library_id, scan_id, &roots).await?;
    }

    if !failures.is_empty() {
        return Err(Error::FilesystemTraversal {
            path: "one or more library roots".into(),
            message: failures.join("; "),
        });
    }

    info!(
        library_id,
        units = workunits_size,
        elapsed_ms = now.elapsed().as_millis(),
        "Finished scanning library."
    );

    Ok(())
}

pub async fn start(
    conn: &mut dim_database::DbConnection,
    library_id: i64,
    tx: EventTx,
    provider: Arc<dyn ExternalQueryIntoShow>,
) -> Result<(), Error> {
    let mut tx_ = conn
        .read()
        .begin()
        .await
        .map_err(|e| Error::DatabaseError(e.into()))?;

    let lib = Library::get_one(&mut tx_, library_id)
        .await
        .map_err(|e| Error::LibraryNotFound(e))?;

    start_custom(
        conn,
        library_id,
        lib.locations,
        tx,
        lib.media_type,
        provider,
    )
    .await
}

/// Function formats the path where assets are stored.
pub fn format_path(x: Option<String>) -> String {
    x.map(|x| format!("images/{}", x.trim_start_matches('/')))
        .unwrap_or_default()
}
