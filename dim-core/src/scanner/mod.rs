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

use futures::FutureExt;
use ignore::WalkBuilder;
use itertools::Itertools;

use std::collections::HashMap;
use std::ffi::OsStr;
use std::future::Future;
use std::path::Path;
use std::path::PathBuf;
use std::pin::Pin;
use std::sync::Arc;
use std::time::Instant;

use tracing::error;
use tracing::info;
use tracing::instrument;
use tracing::warn;

use once_cell::sync::Lazy;
use parking_lot::Mutex as ParkingMutex;
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
        let filename = match file.as_ref().file_stem().and_then(OsStr::to_str) {
            Some(x) => x,
            None => {
                warn!(file = ?file.as_ref(), "Received a filename that is not unicode");
                continue;
            }
        };

        let metas = IntoIterator::into_iter([
            TorrentMetadata::from_str(&filename),
            Anitomy::from_str(&filename),
            CombinedExtractor::from_str(&filename),
        ])
        .filter_map(|x| x)
        .collect::<Vec<_>>();

        if metas.is_empty() {
            warn!(file = ?file.as_ref(), "Failed to parse the filename and extract metadata.");
            continue;
        }

        metadata.push((file.as_ref().into(), metas));
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
    insert_mediafiles_for_scan(conn, library_id, dirs, None).await
}

async fn update_item(
    conn: &dim_database::DbConnection,
    scan_id: i64,
    library_id: i64,
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

async fn insert_mediafiles_for_scan(
    conn: &mut dim_database::DbConnection,
    library_id: i64,
    dirs: Vec<impl AsRef<Path> + Send + 'static>,
    scan_id: Option<i64>,
) -> Result<Vec<WorkUnit>, Error> {
    let now = Instant::now();
    let discovered = tokio::task::spawn_blocking(|| discover_files(dirs.into_iter()))
        .await
        .unwrap();
    let subfiles = discovered
        .iter()
        .filter(|file| file.supported)
        .map(|file| file.path.clone())
        .collect::<Vec<_>>();
    let elapsed = now.elapsed();

    info!(
        elapsed_ms = elapsed.as_millis(),
        files = discovered.len(),
        "Walked all target directories."
    );

    if let Some(scan_id) = scan_id {
        let mut lock = conn.writer().lock_owned().await;
        let mut tx = dim_database::write_tx(&mut lock).await?;
        for file in &discovered {
            let (status, class) = if file.supported {
                ("complete", None)
            } else {
                ("skipped", Some("unsupported_format"))
            };
            if let Some(path) = file.path.to_str() {
                dim_database::ingestion::upsert_item(
                    &mut tx,
                    scan_id,
                    library_id,
                    path,
                    None,
                    "discovery",
                    status,
                    class,
                    None,
                )
                .await?;
            }
            dim_database::ingestion::ScanRun::count(&mut tx, scan_id, "discovered").await?;
            if !file.supported {
                dim_database::ingestion::ScanRun::count(&mut tx, scan_id, "skipped").await?;
            }
        }
        tx.commit().await?;
    }

    let parsed = parse_filenames(subfiles.iter());
    if let Some(scan_id) = scan_id {
        let parsed_paths = parsed
            .iter()
            .map(|(path, _)| path.as_path())
            .collect::<std::collections::HashSet<_>>();
        for path in subfiles
            .iter()
            .filter(|path| !parsed_paths.contains(path.as_path()))
        {
            update_item(
                conn,
                scan_id,
                library_id,
                path,
                None,
                "matching",
                "failed",
                Some("filename_unparseable"),
                None,
            )
            .await?;
        }
    }

    let mut instance = MediafileCreator::new(conn.clone(), library_id).await;

    let insertable_futures = parsed
        .clone()
        .into_iter()
        .map(|(path, metadata)| {
            let primary = metadata[0].clone();
            let assessed_path = path.clone();
            async {
                (
                    assessed_path,
                    instance.construct_mediafile(path, primary).await,
                    metadata,
                )
            }
            .boxed()
        })
        .chunks(4)
        .into_iter()
        .map(|chunk| chunk.collect())
        .collect::<Vec<
            Vec<
                Pin<
                    Box<
                        dyn Future<
                                Output = (
                                    PathBuf,
                                    Result<InsertableMediaFile, CreatorError>,
                                    Vec<Metadata>,
                                ),
                            > + Send,
                    >,
                >,
            >,
        >>();

    let mut insertables = vec![];

    for chunk in insertable_futures.into_iter() {
        let results = futures::future::join_all(chunk).await;

        for (path, result, metadata) in results {
            match result {
                Ok(insertable) => insertables.push((insertable, metadata)),
                Err(CreatorError::FileExists) => {
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
                    if let Some(scan_id) = scan_id {
                        update_item(
                            conn,
                            scan_id,
                            library_id,
                            &path,
                            None,
                            "commit",
                            "skipped",
                            Some("already_catalogued"),
                            None,
                        )
                        .await?;
                    }
                    continue;
                }
                Err(error) => {
                    if let Some(scan_id) = scan_id {
                        let status = if error.retryable() {
                            "retryable"
                        } else {
                            "failed"
                        };
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
                            &path,
                            None,
                            stage,
                            status,
                            Some(error.class()),
                            Some(&error.to_string()),
                        )
                        .await?;
                    }
                    warn!(
                        ?error,
                        "Skipping media candidate after classified assessment failure"
                    );
                    continue;
                }
            }
        }
    }

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

    let work = mediafiles
        .into_iter()
        .map(|mfile| {
            let metadata = metadata_by_path
                .remove(&mfile.target_file)
                .expect("inserted mediafile must retain its parsed metadata");
            WorkUnit(mfile, metadata)
        })
        .collect::<Vec<_>>();

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

#[instrument(skip(conn, dirs, tx))]
pub async fn start_custom(
    conn: &mut dim_database::DbConnection,
    library_id: i64,
    dirs: Vec<impl AsRef<Path> + Send + 'static>,
    tx: EventTx,
    media_type: MediaType,
    provider: Arc<dyn ExternalQueryIntoShow>,
) -> Result<(), Error> {
    // Watcher reconciliation and user-requested scans share the same per-library ownership gate.
    // Cancellation drops this guard; the next owner then records the abandoned durable run.
    let _scan_owner = library_scan_lock(library_id).lock_owned().await;
    let scan_id = {
        let mut lock = conn.writer().lock_owned().await;
        let mut db_tx = dim_database::write_tx(&mut lock).await?;
        let id = dim_database::ingestion::ScanRun::begin(&mut db_tx, library_id, "full").await?;
        db_tx.commit().await?;
        id
    };

    let result = run_scan_custom(conn, library_id, dirs, tx, media_type, provider, scan_id).await;

    let error_message = result.as_ref().err().map(ToString::to_string);
    let mut lock = conn.writer().lock_owned().await;
    let mut db_tx = dim_database::write_tx(&mut lock).await?;
    dim_database::ingestion::ScanRun::finish(
        &mut db_tx,
        scan_id,
        if result.is_ok() { "complete" } else { "failed" },
        error_message.as_deref(),
    )
    .await?;
    db_tx.commit().await?;
    result
}

async fn run_scan_custom(
    conn: &mut dim_database::DbConnection,
    library_id: i64,
    dirs: Vec<impl AsRef<Path> + Send + 'static>,
    tx: EventTx,
    media_type: MediaType,
    provider: Arc<dyn ExternalQueryIntoShow>,
    scan_id: i64,
) -> Result<(), Error> {
    info!(library_id, "Scanning library");

    if let Err(error) = tx
        .send(
            dim_events::Message {
                id: library_id,
                event_type: dim_events::PushEventType::EventStartedScanning,
            }
            .to_string(),
        )
        .await
    {
        warn!(?error, library_id, "Could not publish scan start event");
    }

    let matcher = match media_type {
        MediaType::Movie => Arc::new(movie::MovieMatcher) as Arc<dyn MediaMatcher>,
        MediaType::Tv => Arc::new(tv_show::TvMatcher) as Arc<dyn MediaMatcher>,
        _ => unimplemented!(),
    };

    let now = Instant::now();
    let workunits = insert_mediafiles_for_scan(conn, library_id, dirs, Some(scan_id)).await?;
    let workunits_size = workunits.len();

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
        let mut lock = conn.writer().lock_owned().await;
        let mut tx = dim_database::write_tx(&mut lock)
            .await
            .map_err(|e| Error::DatabaseError(e.into()))?;

        let match_failure = match matcher.batch_match(&mut tx, provider.clone(), unit).await {
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

        tx.commit()
            .await
            .map_err(|e| Error::DatabaseError(e.into()))?;

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
        }
        count_tx.commit().await?;
    }

    info!(
        library_id,
        units = workunits_size,
        elapsed_ms = now.elapsed().as_millis(),
        "Finished scanning library."
    );

    if let Err(error) = tx
        .send(
            dim_events::Message {
                id: library_id,
                event_type: dim_events::PushEventType::EventStoppedScanning,
            }
            .to_string(),
        )
        .await
    {
        warn!(
            ?error,
            library_id, "Could not publish scan completion event"
        );
    }

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
