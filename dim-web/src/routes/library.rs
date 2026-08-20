#![warn(warnings)]

use std::collections::HashMap;
use std::collections::HashSet;
use std::fs;
use std::io;
use std::path::PathBuf;
use std::sync::Arc;

use axum::extract::{Path, Query, State};
use axum::response::{IntoResponse, Response};
use axum::Json;

use dim_core::errors::DimError;
use dim_core::scanner::daemon::FsWatcher;
use dim_core::workers::LibraryWorkerKind;
use dim_database::compact_mediafile::CompactMediafile;
use dim_database::library::{InsertableLibrary, Library, MediaType};
use dim_database::media::Media;
use dim_database::mediafile::MediaFile;

use dim_extern_api::tmdb::TMDBMetadataProvider;

use fuzzy_matcher::skim::SkimMatcherV2;
use fuzzy_matcher::FuzzyMatcher;
use http::StatusCode;
use serde::Deserialize;
use serde::Serialize;
use tokio::task::spawn_blocking;

use crate::error::DimErrorWrapper;
use crate::middleware::Owner;
use crate::AppState;

#[derive(Copy, Clone, Debug, Serialize, PartialEq, Eq)]
#[serde(rename_all = "lowercase")]
enum LibraryScanStatus {
    Scanning,
    Complete,
    Failed,
    Cancelled,
}

#[derive(Debug, Serialize)]
struct LibraryScanProgress {
    status: LibraryScanStatus,
    stage: String,
    discovered: i64,
    processed: i64,
    committed: i64,
    skipped: i64,
    failed: i64,
    requested_at: Option<String>,
    started_at: Option<String>,
    finished_at: Option<String>,
    last_progress_at: Option<String>,
    elapsed_seconds: i64,
    seconds_since_progress: Option<i64>,
    error_summary: Option<String>,
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct UpdateLibrary {
    auto_scan: bool,
}

fn safe_scan_error(error: Option<&str>) -> Option<String> {
    let error = error?.trim();
    if error.is_empty() {
        return None;
    }
    Some(
        error
            .chars()
            .filter(|character| !character.is_control() || character.is_whitespace())
            .take(500)
            .collect(),
    )
}

fn metadata_provider(media_type: MediaType) -> Arc<dyn dim_extern_api::ExternalQueryIntoShow> {
    const TMDB_KEY: &str = "38c372f5bc572c8aadde7a802638534e";
    let provider = TMDBMetadataProvider::new(TMDB_KEY);

    match media_type {
        MediaType::Movie => Arc::new(provider.movies()),
        MediaType::Tv => Arc::new(provider.tv_shows()),
        _ => unreachable!(),
    }
}

async fn spawn_library_scan(
    state: &AppState,
    id: i64,
    provider: Arc<dyn dim_extern_api::ExternalQueryIntoShow>,
) -> Result<(), &'static str> {
    let scan_id = {
        let mut lock = state.conn.writer().lock_owned().await;
        let mut tx = dim_database::write_tx(&mut lock)
            .await
            .map_err(|_| "failed to persist scan request")?;
        let scan_id = dim_database::ingestion::ScanRun::queue(&mut tx, id, "full")
            .await
            .map_err(|_| "failed to persist scan request")?;
        tx.commit()
            .await
            .map_err(|_| "failed to persist scan request")?;
        scan_id
    };
    let mut conn = state.conn.clone();
    let scan_tx = state.event_tx.clone();
    let spawn_result = state
        .library_workers
        .spawn(id, LibraryWorkerKind::Scanner, async move {
            let terminal_tx = scan_tx.clone();
            if let Err(error) = dim_core::scanner::start(&mut conn, id, scan_tx, provider).await {
                tracing::error!(?error, library_id = id, "Library scan failed");
                let mut lock = conn.writer().lock_owned().await;
                if let Ok(mut tx) = dim_database::write_tx(&mut lock).await {
                    let newly_failed = dim_database::ingestion::ScanRun::finish_active(
                        &mut tx,
                        scan_id,
                        "failed",
                        Some(&error.to_string()),
                    )
                    .await
                    .unwrap_or(false);
                    if tx.commit().await.is_ok() && newly_failed {
                        let _ = terminal_tx.try_send(
                            dim_events::Message {
                                id,
                                event_type: dim_events::PushEventType::EventScanFailed,
                            }
                            .to_string(),
                        );
                    }
                };
            }
        })
        .await;
    if spawn_result.is_err() {
        let mut lock = state.conn.writer().lock_owned().await;
        if let Ok(mut tx) = dim_database::write_tx(&mut lock).await {
            let _ = dim_database::ingestion::ScanRun::finish(
                &mut tx,
                scan_id,
                "failed",
                Some("scanner worker was not started"),
            )
            .await;
            let _ = tx.commit().await;
        };
    }
    spawn_result
}

async fn spawn_library_watcher(state: &AppState, library: &Library) -> Result<(), &'static str> {
    let library_id = library.id;
    let mut watcher = FsWatcher::new(
        state.conn.clone(),
        library_id,
        library.media_type,
        state.event_tx.clone(),
        metadata_provider(library.media_type),
    );
    state
        .library_workers
        .spawn(library_id, LibraryWorkerKind::Watcher, async move {
            if let Err(error) = watcher.start_daemon().await {
                tracing::error!(?error, library_id, "Filesystem watcher failed");
            }
        })
        .await
}

#[derive(Copy, Clone, Debug)]
enum CreateLibraryError {
    InvalidName,
    MissingLocations,
    InvalidLocation,
    LocationNotFound,
    LocationNotDirectory,
    PermissionDenied,
    InvalidMediaType,
    Conflict,
    Internal,
}

impl From<io::Error> for CreateLibraryError {
    fn from(error: io::Error) -> Self {
        match error.kind() {
            io::ErrorKind::NotFound => Self::LocationNotFound,
            io::ErrorKind::PermissionDenied => Self::PermissionDenied,
            _ => Self::Internal,
        }
    }
}

impl IntoResponse for CreateLibraryError {
    fn into_response(self) -> Response {
        let (status, message) = match self {
            Self::InvalidName => (StatusCode::BAD_REQUEST, "Enter a library name."),
            Self::MissingLocations => (
                StatusCode::BAD_REQUEST,
                "Select at least one folder for this library.",
            ),
            Self::InvalidLocation => (
                StatusCode::BAD_REQUEST,
                "Library folders must use valid absolute paths.",
            ),
            Self::LocationNotFound => (
                StatusCode::NOT_FOUND,
                "One or more selected folders no longer exist.",
            ),
            Self::LocationNotDirectory => (
                StatusCode::BAD_REQUEST,
                "One or more selected paths are not folders.",
            ),
            Self::PermissionDenied => (
                StatusCode::FORBIDDEN,
                "Eclipse does not have permission to read one or more selected folders.",
            ),
            Self::InvalidMediaType => (
                StatusCode::BAD_REQUEST,
                "Choose either Movies or Shows for this library.",
            ),
            Self::Conflict => (
                StatusCode::CONFLICT,
                "A library already uses that name or folder.",
            ),
            Self::Internal => (
                StatusCode::INTERNAL_SERVER_ERROR,
                "Eclipse could not create the library.",
            ),
        };

        let code = match self {
            Self::InvalidName => "invalid_library_name",
            Self::MissingLocations => "missing_library_locations",
            Self::InvalidLocation => "invalid_library_location",
            Self::LocationNotFound => "library_location_not_found",
            Self::LocationNotDirectory => "library_location_not_directory",
            Self::PermissionDenied => "library_location_forbidden",
            Self::InvalidMediaType => "invalid_media_type",
            Self::Conflict => "library_conflict",
            Self::Internal => "internal_error",
        };
        crate::error::api_error(status, code, message)
    }
}

fn validate_new_library(
    mut library: InsertableLibrary,
) -> Result<InsertableLibrary, CreateLibraryError> {
    library.name = library.name.trim().to_owned();
    if library.name.is_empty() {
        return Err(CreateLibraryError::InvalidName);
    }
    if library.locations.is_empty() {
        return Err(CreateLibraryError::MissingLocations);
    }
    if !matches!(library.media_type, MediaType::Movie | MediaType::Tv) {
        return Err(CreateLibraryError::InvalidMediaType);
    }

    let mut seen = HashSet::new();
    let mut locations = Vec::with_capacity(library.locations.len());
    for location in library.locations {
        let path = PathBuf::from(location);
        if !path.is_absolute() {
            return Err(CreateLibraryError::InvalidLocation);
        }

        let path = fs::canonicalize(path)?;
        if !path.is_dir() {
            return Err(CreateLibraryError::LocationNotDirectory);
        }

        // Opening the directory verifies that the scanner can at least begin walking it.
        fs::read_dir(&path)?;
        if seen.insert(path.clone()) {
            locations.push(path.to_string_lossy().into_owned());
        }
    }

    library.locations = locations;
    Ok(library)
}

#[cfg(test)]
mod library_creation_tests {
    use super::*;

    #[test]
    fn validates_movie_and_tv_directories_and_rejects_relative_paths() {
        let directory = std::env::temp_dir().join(format!("dim-library-{}", uuid::Uuid::new_v4()));
        fs::create_dir_all(&directory).unwrap();

        for media_type in [MediaType::Movie, MediaType::Tv] {
            let library = validate_new_library(InsertableLibrary {
                name: "  Media  ".to_owned(),
                locations: vec![directory.to_string_lossy().into_owned()],
                media_type,
            })
            .unwrap();

            assert_eq!(library.name, "Media");
            assert_eq!(library.media_type, media_type);
            assert_eq!(
                library.locations,
                vec![fs::canonicalize(&directory)
                    .unwrap()
                    .to_string_lossy()
                    .into_owned()]
            );
        }

        assert!(matches!(
            validate_new_library(InsertableLibrary {
                name: "Media".to_owned(),
                locations: vec!["relative/media".to_owned()],
                media_type: MediaType::Movie,
            }),
            Err(CreateLibraryError::InvalidLocation)
        ));

        fs::remove_dir_all(directory).unwrap();
    }
}

/// Method maps to `POST /api/v1/library`, it adds a new library to the database, starts a new
/// scanner for it, then dispatches a event to all clients notifying them that a new library has
/// been created. This method can only be accessed by authenticated users. Method returns 200 OK
///
pub async fn library_post(
    _owner: Owner,
    State(state): State<AppState>,
    Json(new_library): Json<InsertableLibrary>,
) -> Response {
    let new_library = match spawn_blocking(move || validate_new_library(new_library)).await {
        Ok(Ok(library)) => library,
        Ok(Err(error)) => return error.into_response(),
        Err(error) => {
            tracing::error!(?error, "Library path validation task failed");
            return CreateLibraryError::Internal.into_response();
        }
    };

    let mut lock = state.conn.writer().lock_owned().await;

    let mut tx = match dim_database::write_tx(&mut lock).await {
        Ok(tx) => tx,
        Err(err) => {
            tracing::error!(?err, "Error getting connection");
            return CreateLibraryError::Internal.into_response();
        }
    };

    // A client may retry after losing the response to a successful create. Treat the exact same
    // library as idempotent instead of trapping the user behind the unique name/path constraints.
    match Library::get_by_name(&mut tx, &new_library.name).await {
        Ok(Some(existing)) => {
            let mut existing_locations = existing.locations;
            let mut requested_locations = new_library.locations.clone();
            existing_locations.sort();
            requested_locations.sort();
            if !existing.hidden
                && existing.media_type == new_library.media_type
                && existing_locations == requested_locations
            {
                return Json(serde_json::json!({
                    "id": existing.id,
                    "scan_status": "scanning"
                }))
                .into_response();
            }
            return CreateLibraryError::Conflict.into_response();
        }
        Ok(None) => {}
        Err(err) => {
            tracing::error!(?err, "Error checking for an existing library");
            return CreateLibraryError::Internal.into_response();
        }
    }

    let id = match new_library.insert(&mut tx).await {
        Ok(id) => id,
        Err(err) => {
            tracing::error!(?err, "Error inserting library");
            return if err.is_unique_violation() {
                CreateLibraryError::Conflict
            } else {
                CreateLibraryError::Internal
            }
            .into_response();
        }
    };

    match tx.commit().await {
        Ok(_) => (),
        Err(err) => {
            tracing::error!(?err, "Error committing transaction");
            return CreateLibraryError::Internal.into_response();
        }
    }
    drop(lock);

    let provider = metadata_provider(new_library.media_type);

    let scan_status =
        if let Err(error) = spawn_library_scan(&state, id, Arc::clone(&provider)).await {
            tracing::error!(?error, library_id = id, "Library scanner was not started");
            "failed"
        } else {
            "scanning"
        };

    let library = Library {
        id,
        name: new_library.name,
        locations: new_library.locations,
        media_type: new_library.media_type,
        hidden: false,
        auto_scan: true,
    };
    if let Err(error) = spawn_library_watcher(&state, &library).await {
        tracing::error!(
            ?error,
            library_id = id,
            "Filesystem watcher was not started"
        );
    }

    // The durable library is the contract of this endpoint. Background worker startup is
    // recoverable (manual scan/restart), so it must not turn a committed create into a false 500.
    Json(serde_json::json!({ "id": id, "scan_status": scan_status })).into_response()
}

pub async fn library_patch(
    _owner: Owner,
    State(state): State<AppState>,
    Path(id): Path<i64>,
    Json(update): Json<UpdateLibrary>,
) -> Response {
    let mut lock = state.conn.writer().lock_owned().await;
    let mut tx = match dim_database::write_tx(&mut lock).await {
        Ok(tx) => tx,
        Err(error) => {
            tracing::error!(?error, "Error getting connection");
            return crate::error::api_error(
                StatusCode::INTERNAL_SERVER_ERROR,
                "internal_error",
                "Eclipse could not complete the request.",
            );
        }
    };
    let mut library = match Library::get_one(&mut tx, id).await {
        Ok(library) if !library.hidden => library,
        _ => {
            return crate::error::api_error(
                StatusCode::NOT_FOUND,
                "library_not_found",
                "The requested library was not found.",
            )
        }
    };

    if library.auto_scan != update.auto_scan {
        if let Err(error) = Library::set_auto_scan(&mut tx, id, update.auto_scan).await {
            tracing::error!(?error, library_id = id, "Failed to update library settings");
            return crate::error::api_error(
                StatusCode::INTERNAL_SERVER_ERROR,
                "internal_error",
                "Eclipse could not update the library.",
            );
        }
        if let Err(error) = tx.commit().await {
            tracing::error!(?error, library_id = id, "Failed to commit library settings");
            return crate::error::api_error(
                StatusCode::INTERNAL_SERVER_ERROR,
                "internal_error",
                "Eclipse could not update the library.",
            );
        }
        drop(lock);

        if update.auto_scan {
            if let Err(error) = spawn_library_watcher(&state, &library).await {
                tracing::error!(
                    ?error,
                    library_id = id,
                    "Filesystem watcher was not started"
                );
                return crate::error::api_error(
                    StatusCode::CONFLICT,
                    "library_stopping",
                    "This library is stopping.",
                );
            }
        } else {
            state
                .library_workers
                .stop(id, LibraryWorkerKind::Watcher)
                .await;
        }
        library.auto_scan = update.auto_scan;
    }

    library.locations.clear();
    Json(library).into_response()
}

pub async fn library_scan_status(State(state): State<AppState>, Path(id): Path<i64>) -> Response {
    let mut tx = match state.conn.read().begin().await {
        Ok(tx) => tx,
        Err(error) => {
            tracing::error!(?error, "Error getting connection");
            return StatusCode::INTERNAL_SERVER_ERROR.into_response();
        }
    };

    if Library::get_one(&mut tx, id).await.is_err() {
        return (StatusCode::NOT_FOUND, "Library not found.").into_response();
    }

    let run = match dim_database::ingestion::ScanRun::latest(&mut tx, id).await {
        Ok(run) => run,
        Err(error) => {
            tracing::error!(
                ?error,
                library_id = id,
                "Failed to read durable scan status"
            );
            return StatusCode::INTERNAL_SERVER_ERROR.into_response();
        }
    };
    let Some(run) = run else {
        return Json(LibraryScanProgress {
            status: LibraryScanStatus::Complete,
            stage: "complete".into(),
            discovered: 0,
            processed: 0,
            committed: 0,
            skipped: 0,
            failed: 0,
            requested_at: None,
            started_at: None,
            finished_at: None,
            last_progress_at: None,
            elapsed_seconds: 0,
            seconds_since_progress: None,
            error_summary: None,
        })
        .into_response();
    };
    let timing = sqlx::query_as::<_, (i64, i64)>(
        "SELECT MAX(0, unixepoch(COALESCE(finished_at, CURRENT_TIMESTAMP)) - unixepoch(COALESCE(started_at, requested_at))), MAX(0, unixepoch(CURRENT_TIMESTAMP) - unixepoch(last_progress_at)) FROM ingestion_scan WHERE id = ?",
    )
    .bind(run.id)
    .fetch_one(&mut tx)
    .await
    .unwrap_or((0, 0));
    let status = match run.status.as_str() {
        "queued" | "running" => LibraryScanStatus::Scanning,
        "failed" => LibraryScanStatus::Failed,
        "cancelled" => LibraryScanStatus::Cancelled,
        _ => LibraryScanStatus::Complete,
    };
    Json(LibraryScanProgress {
        status,
        stage: run.stage,
        discovered: run.discovered,
        processed: run.processed,
        committed: run.committed,
        skipped: run.skipped,
        failed: run.failed,
        requested_at: Some(run.requested_at),
        started_at: run.started_at,
        finished_at: run.finished_at,
        last_progress_at: Some(run.last_progress_at),
        elapsed_seconds: timing.0,
        seconds_since_progress: Some(timing.1),
        error_summary: safe_scan_error(run.error.as_deref()),
    })
    .into_response()
}

pub async fn library_scan_retry(
    _owner: Owner,
    State(state): State<AppState>,
    Path(id): Path<i64>,
) -> Response {
    let mut tx = match state.conn.read().begin().await {
        Ok(tx) => tx,
        Err(error) => {
            tracing::error!(?error, "Error getting connection");
            return StatusCode::INTERNAL_SERVER_ERROR.into_response();
        }
    };

    let library = match Library::get_one(&mut tx, id).await {
        Ok(library) => library,
        Err(_) => return (StatusCode::NOT_FOUND, "Library not found.").into_response(),
    };

    if matches!(dim_database::ingestion::ScanRun::latest(&mut tx, id).await, Ok(Some(run)) if matches!(run.status.as_str(), "queued" | "running"))
    {
        return (
            StatusCode::CONFLICT,
            "This library is already being scanned.",
        )
            .into_response();
    }

    if spawn_library_scan(&state, id, metadata_provider(library.media_type))
        .await
        .is_err()
    {
        return (StatusCode::CONFLICT, "This library is stopping.").into_response();
    }
    Json(serde_json::json!({ "status": LibraryScanStatus::Scanning })).into_response()
}

/// Method mapped to `DELETE /api/v1/library/<id>` deletes the library with the supplied id from the path.
pub async fn library_delete(
    _owner: Owner,
    State(AppState {
        conn,
        library_workers,
        ..
    }): State<AppState>,
    Path(id): Path<i64>,
) -> Result<StatusCode, DimErrorWrapper> {
    // First we mark the library as scheduled for deletion which will make the library and all its
    // content hidden. This is necessary because huge libraries take a long time to delete.
    {
        let mut lock = conn.writer().lock_owned().await;
        let mut tx = dim_database::write_tx(&mut lock).await.map_err(|err| {
            DimErrorWrapper(DimError::DatabaseError {
                description: err.to_string(),
            })
        })?;
        if Library::mark_hidden(&mut tx, id).await.map_err(|err| {
            DimErrorWrapper(DimError::DatabaseError {
                description: err.to_string(),
            })
        })? < 1
        {
            return Err(DimError::LibraryNotFound.into());
        }
        tx.commit().await.map_err(|err| {
            DimErrorWrapper(DimError::DatabaseError {
                description: err.to_string(),
            })
        })?;
    }

    // The tombstone is installed before awaiting tasks, so a retry which raced the hidden update
    // cannot register a new scanner after this point.
    library_workers.stop_library(id).await;

    let mut lock = conn.writer().lock_owned().await;
    let mut tx = dim_database::write_tx(&mut lock).await.map_err(|err| {
        DimErrorWrapper(DimError::DatabaseError {
            description: err.to_string(),
        })
    })?;
    MediaFile::delete_by_lib_id(&mut tx, id).await?;
    Media::delete_by_lib_id(&mut tx, id).await?;
    Library::delete(&mut tx, id).await?;
    tx.commit().await.map_err(|err| {
        DimErrorWrapper(DimError::DatabaseError {
            description: err.to_string(),
        })
    })?;
    tracing::info!(library_id = id, "Deleted library");

    Ok(StatusCode::NO_CONTENT)
}

/// Method mapped to `GET /api/v1/library` returns a list of all libraries in the database
pub async fn library_get_all(State(state): State<AppState>) -> Response {
    let mut tx = match state.conn.read().begin().await {
        Ok(tx) => tx,
        Err(err) => {
            tracing::error!(?err, "Error getting connection");
            return crate::error::api_error(
                StatusCode::INTERNAL_SERVER_ERROR,
                "internal_error",
                "Eclipse could not complete the request.",
            );
        }
    };

    let libraries = Library::get_all(&mut tx).await;

    Json(libraries).into_response()
}

/// Method mapped to `GET /api/v1/library/<id>` returns info about the library with the supplied
/// id. Method can only be accessed by authenticated users.
///
pub async fn library_get_one(State(state): State<AppState>, Path(id): Path<i64>) -> Response {
    let mut tx = match state.conn.read().begin().await {
        Ok(tx) => tx,
        Err(err) => {
            tracing::error!(?err, "Error getting connection");
            return crate::error::api_error(
                StatusCode::INTERNAL_SERVER_ERROR,
                "internal_error",
                "Eclipse could not complete the request.",
            );
        }
    };

    let mut lib = match Library::get_one(&mut tx, id).await {
        Ok(library) => library,
        Err(err) => {
            tracing::error!(?err, "Error getting library");
            return crate::error::api_error(
                StatusCode::NOT_FOUND,
                "library_not_found",
                "The requested library was not found.",
            );
        }
    };
    // Filesystem locations are owner-only input to creation and browsing, not ordinary library
    // metadata. Never return absolute host paths from the normal authenticated read endpoint.
    lib.locations.clear();
    Json(lib).into_response()
}

/// Method mapped to `GET /api/v1/library/<id>/media` returns all the movies/tv shows that belong
/// to the library with the id supplied. Method can only be accessed by authenticated users.
///
pub async fn library_get_media(
    State(AppState { conn, .. }): State<AppState>,
    Path(id): Path<i64>,
) -> Response {
    let mut result = HashMap::new();
    let mut tx = match conn.read().begin().await {
        Ok(tx) => tx,
        Err(err) => {
            tracing::error!(?err, "Error getting connection");
            return crate::error::api_error(
                StatusCode::INTERNAL_SERVER_ERROR,
                "internal_error",
                "Eclipse could not complete the request.",
            );
        }
    };

    let lib = match Library::get_one(&mut tx, id).await {
        Ok(library) => library,
        Err(err) => {
            tracing::error!(?err, "Error getting library");
            return crate::error::api_error(
                StatusCode::NOT_FOUND,
                "library_not_found",
                "The requested library was not found.",
            );
        }
    };

    #[derive(Serialize)]
    struct Record {
        id: i64,
        name: String,
        poster_path: Option<String>,
    }

    let mut data = match sqlx::query_as!(
        Record,
        r#"SELECT _tblmedia.id, name, assets.local_path as poster_path FROM _tblmedia
        LEFT JOIN assets ON _tblmedia.poster = assets.id
        WHERE library_id = ? AND NOT media_type = "episode""#,
        id
    )
    .fetch_all(&mut tx)
    .await
    {
        Ok(res) => res,
        Err(err) => {
            tracing::error!(?err, "Library media query failed");
            return crate::error::api_error(
                StatusCode::INTERNAL_SERVER_ERROR,
                "internal_error",
                "Eclipse could not complete the request.",
            );
        }
    };

    if data.is_empty() {
        return (StatusCode::NOT_FOUND, "No media found".to_string()).into_response();
    }

    data.sort_by(|a, b| a.name.cmp(&b.name));

    result.insert(lib.name, data);

    Json(result).into_response()
}

#[derive(Deserialize)]
pub struct UnmatchedArgs {
    search: Option<String>,
}

/// Method mapped to `GET /api/v1/library/<id>/unmatched` returns a list of all unmatched medias
/// to be displayed in the library pages.
///
pub async fn library_get_unmatched(
    State(AppState { conn, .. }): State<AppState>,
    Path(id): Path<i64>,
    Query(params): Query<UnmatchedArgs>,
) -> Response {
    let mut tx = match conn.read().begin().await {
        Ok(tx) => tx,
        Err(err) => {
            tracing::error!(?err, "Error getting connection");
            return crate::error::api_error(
                StatusCode::INTERNAL_SERVER_ERROR,
                "internal_error",
                "Eclipse could not complete the request.",
            );
        }
    };

    // let mut files = CompactMediafile::unmatched_for_library(&mut tx, id)
    //     .await
    //     .map_err(|_| errors::DimError::NotFoundError)?;

    let mut files = match CompactMediafile::unmatched_for_library(&mut tx, id).await {
        Ok(r) => r,
        Err(err) => {
            tracing::error!(?err, "Error getting unmatched files");
            return crate::error::api_error(
                StatusCode::INTERNAL_SERVER_ERROR,
                "internal_error",
                "Eclipse could not complete the request.",
            );
        }
    };

    // we want to pre-sort to ensure our tree is somewhat ordered.
    files.sort_by(|a, b| a.target_file.cmp(&b.target_file));

    if let Some(search) = params.search {
        let matcher = SkimMatcherV2::default();

        let mut matched_files = files
            .into_iter()
            .filter_map(|x| {
                let file_string = x.target_file.to_string_lossy();

                matcher
                    .fuzzy_match(&file_string, &search)
                    .map(|score| (x, score))
            })
            .collect::<Vec<_>>();

        matched_files.sort_by(|(_, a), (_, b)| b.cmp(&a));

        files = matched_files.into_iter().map(|(file, _)| file).collect();
    }

    let count = files.len();

    #[derive(Serialize)]
    struct Record {
        id: i64,
        name: String,
        duration: Option<i64>,
        file: String,
    }

    let entry = crate::tree::Entry::build_with(
        files,
        |x| {
            x.target_file
                .file_name()
                .into_iter()
                .map(|name| name.to_string_lossy().to_string())
                .collect()
        },
        |k, v| Record {
            id: v.id,
            name: v.name,
            duration: v.duration,
            file: k.to_string(),
        },
    );

    #[derive(Serialize)]
    struct Response {
        count: usize,
        files: Vec<crate::tree::Entry<Record>>,
    }

    let entries = match entry {
        crate::tree::Entry::Directory { files, .. } => files,
        _ => unreachable!(),
    };

    Json(Response {
        files: entries,
        count,
    })
    .into_response()
}
