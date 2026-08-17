use crate::scanner;
use crate::workers::{LibraryWorkerKind, LibraryWorkers};

use dim_database::library::MediaType;

use dim_extern_api::tmdb::TMDBMetadataProvider;

use once_cell::sync::OnceCell;

use tokio::sync::mpsc::Sender;
use tracing::{info, instrument};

use std::sync::Arc;

pub type StateManager = nightfall::StateManager;
pub type DbConnection = dim_database::DbConnection;
pub type EventTx = Sender<String>;

/// Path to where metadata is stored and should be fetched to.
pub static METADATA_PATH: OnceCell<String> = OnceCell::new();

/// Function dumps a list of all libraries in the database and starts a scanner for each which
/// monitors for new files using fsnotify. It also scans all orphans on boot.
///
/// # Arguments
/// * `log` - Logger to which to log shit
/// * `tx` - this is the websocket channel to which we can send websocket events to which get
/// dispatched to clients.
#[instrument(skip_all)]
pub async fn run_scanners(conn: DbConnection, tx: EventTx, workers: &LibraryWorkers) {
    if let Ok(mut db_tx) = conn.read().begin().await {
        let mut libs = dim_database::library::Library::get_all(&mut db_tx).await;

        for lib in libs.drain(..) {
            if !lib.auto_scan {
                info!(
                    "Automatic scanning is disabled for {} with id: {}",
                    lib.name, lib.id
                );
                continue;
            }
            info!("Starting scanner for {} with id: {}", lib.name, lib.id);

            let library_id = lib.id;
            let tx_clone = tx.clone();
            let media_type = lib.media_type;

            let provider = TMDBMetadataProvider::new("38c372f5bc572c8aadde7a802638534e");

            let provider = match media_type {
                MediaType::Movie => Arc::new(provider.movies()) as Arc<_>,
                MediaType::Tv => Arc::new(provider.tv_shows()) as Arc<_>,
                _ => unreachable!(),
            };

            let conn_clone = conn.clone();
            let scanner_provider = Arc::clone(&provider);
            let scanner_tx = tx_clone.clone();

            if let Err(error) = workers
                .spawn(library_id, LibraryWorkerKind::Scanner, async move {
                    let mut conn = conn_clone;
                    if let Err(error) =
                        scanner::start(&mut conn, library_id, scanner_tx, scanner_provider).await
                    {
                        tracing::error!(?error, library_id, "Library scan failed");
                    }
                })
                .await
            {
                tracing::warn!(?error, library_id, "Scanner was not started");
            }

            let mut watcher = scanner::daemon::FsWatcher::new(
                conn.clone(),
                library_id,
                media_type,
                tx_clone,
                Arc::clone(&provider),
            );
            if let Err(error) = workers
                .spawn(library_id, LibraryWorkerKind::Watcher, async move {
                    if let Err(error) = watcher.start_daemon().await {
                        tracing::error!(?error, library_id, "Filesystem watcher failed");
                    }
                })
                .await
            {
                tracing::warn!(?error, library_id, "Filesystem watcher was not started");
            }
        }
    }
}
