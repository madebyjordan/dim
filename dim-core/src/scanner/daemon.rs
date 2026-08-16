use crate::core::EventTx;
use dim_extern_api::filename::{Anitomy, CombinedExtractor, FilenameMetadata, TorrentMetadata};
use dim_extern_api::ExternalQueryIntoShow;

use super::{movie, tv_show, MediaMatcher, WorkUnit};

use std::collections::{HashMap, HashSet};
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;
use std::time::Duration;

use dim_database::library::{Library, MediaType};
use dim_database::mediafile::MediaFile;
use dim_database::DbConnection;
use notify::event::{ModifyKind, RenameMode};
use notify::{Config, Event, EventKind, RecommendedWatcher, RecursiveMode, Watcher};
use tokio::sync::mpsc::{self, Receiver};
use tracing::{error, warn};

use displaydoc::Display;
use thiserror::Error;

const WATCH_QUEUE_CAPACITY: usize = 512;
const COALESCE_WINDOW: Duration = Duration::from_millis(350);

#[derive(Display, Debug, Error)]
pub enum FsWatcherError {
    /// A database error has occurred: {0:?}
    DatabaseError(#[from] dim_database::DatabaseError),
    /// A database connection or transaction error has occurred: {0:?}
    SqlxError(#[from] sqlx::Error),
    /// An error with notify has occurred: {0:?}
    NotifyError(#[from] notify::Error),
}

pub struct WatchReceiver {
    rx: Receiver<notify::Result<Event>>,
    overflowed: Arc<AtomicBool>,
}

impl WatchReceiver {
    async fn recv(&mut self) -> Option<notify::Result<Event>> {
        self.rx.recv().await
    }

    fn take_overflow(&self) -> bool {
        self.overflowed.swap(false, Ordering::AcqRel)
    }
}

#[derive(Default)]
struct EventBatch {
    creates: HashSet<PathBuf>,
    removes: HashSet<PathBuf>,
    renames: HashMap<PathBuf, PathBuf>,
    rename_from: HashMap<usize, PathBuf>,
    rename_to: HashMap<usize, PathBuf>,
    reconcile: bool,
}

impl EventBatch {
    fn push(&mut self, event: notify::Result<Event>) {
        let mut event = match event {
            Ok(event) => event,
            Err(error) => {
                warn!(
                    ?error,
                    "Filesystem event stream reported lost or invalid state"
                );
                self.reconcile = true;
                return;
            }
        };
        if event.need_rescan() {
            self.reconcile = true;
        }
        let tracker = event.tracker();
        match event.kind {
            EventKind::Create(_) => self.creates.extend(event.paths),
            EventKind::Remove(_) => self.removes.extend(event.paths),
            EventKind::Modify(ModifyKind::Name(RenameMode::Both)) if event.paths.len() == 2 => {
                let to = event.paths.pop().expect("rename destination");
                let from = event.paths.pop().expect("rename source");
                self.renames.insert(from, to);
            }
            EventKind::Modify(ModifyKind::Name(RenameMode::From)) => {
                if let Some(tracker) = tracker {
                    if let Some(path) = event.paths.pop() {
                        self.rename_from.insert(tracker, path);
                    }
                } else {
                    self.removes.extend(event.paths);
                }
            }
            EventKind::Modify(ModifyKind::Name(RenameMode::To)) => {
                if let Some(tracker) = tracker {
                    if let Some(path) = event.paths.pop() {
                        self.rename_to.insert(tracker, path);
                    }
                } else {
                    self.creates.extend(event.paths);
                }
            }
            // Modify events are common while files are copied. Coalesce them into one stability
            // assessment rather than probing on every write notification.
            EventKind::Modify(_) => self.creates.extend(event.paths),
            EventKind::Any | EventKind::Other => self.reconcile = true,
            _ => {}
        }
    }
}

pub struct FsWatcher {
    media_type: MediaType,
    library_id: i64,
    tx: EventTx,
    conn: DbConnection,
    matcher: Arc<dyn MediaMatcher>,
    provider: Arc<dyn ExternalQueryIntoShow>,
}

impl FsWatcher {
    pub fn new(
        conn: DbConnection,
        library_id: i64,
        media_type: MediaType,
        tx: EventTx,
        provider: Arc<dyn ExternalQueryIntoShow>,
    ) -> Self {
        let matcher = match media_type {
            MediaType::Movie => Arc::new(movie::MovieMatcher) as Arc<dyn MediaMatcher>,
            MediaType::Tv => Arc::new(tv_show::TvMatcher) as Arc<dyn MediaMatcher>,
            _ => unimplemented!(),
        };
        Self {
            media_type,
            library_id,
            tx,
            conn,
            matcher,
            provider,
        }
    }

    pub async fn start_daemon(&mut self) -> Result<(), FsWatcherError> {
        let library = {
            let mut tx = self.conn.read().begin().await?;
            Library::get_one(&mut tx, self.library_id).await?
        };
        let (mut receiver, _watcher) = spawn_file_watcher(&library.locations)?;

        while let Some(first) = receiver.recv().await {
            let mut batch = EventBatch::default();
            batch.push(first);
            let deadline = tokio::time::sleep(COALESCE_WINDOW);
            tokio::pin!(deadline);
            loop {
                tokio::select! {
                    _ = &mut deadline => break,
                    event = receiver.recv() => match event {
                        Some(event) => batch.push(event),
                        None => break,
                    },
                }
            }
            batch.reconcile |= receiver.take_overflow();
            self.process_batch(batch, &library.locations).await;
        }

        warn!(
            library_id = self.library_id,
            "Filesystem watcher finished; requesting reconciliation"
        );
        self.reconcile(&library.locations).await;
        Ok(())
    }

    async fn process_batch(&mut self, mut batch: EventBatch, locations: &[String]) {
        if batch.reconcile {
            self.reconcile(locations).await;
            return;
        }
        for (tracker, from) in batch.rename_from.drain() {
            if let Some(to) = batch.rename_to.remove(&tracker) {
                batch.renames.insert(from, to);
            } else {
                batch.removes.insert(from);
            }
        }
        batch.creates.extend(batch.rename_to.into_values());
        for (from, to) in batch.renames.drain() {
            batch.creates.remove(&to);
            batch.removes.remove(&from);
            self.handle_rename(&from, &to).await;
        }
        for path in batch.removes {
            self.mark_missing(&path).await;
        }
        // A directory event is reconciled as a subtree by the normal scanner.
        for path in batch.creates {
            self.handle_create(path).await;
        }
    }

    async fn handle_create(&mut self, path: PathBuf) {
        let candidate = if path.is_dir() {
            path
        } else if super::supported_path(&path) {
            path
        } else {
            return;
        };
        if let Err(error) = super::start_incremental_custom(
            &mut self.conn,
            self.library_id,
            vec![candidate],
            self.tx.clone(),
            self.media_type,
            self.provider.clone(),
        )
        .await
        {
            warn!(
                ?error,
                library_id = self.library_id,
                "Watcher ingestion request failed"
            );
        }
    }

    async fn mark_missing(&mut self, path: &Path) {
        let Some(path) = path.to_str() else {
            return;
        };
        let mut lock = self.conn.writer().lock_owned().await;
        match dim_database::write_tx(&mut lock).await {
            Ok(mut tx) => {
                let update = sqlx::query("UPDATE mediafile SET missing_since = COALESCE(missing_since, CURRENT_TIMESTAMP) WHERE library_id = ? AND (target_file = ? OR target_file LIKE ?)")
                    .bind(self.library_id)
                    .bind(path)
                    .bind(format!("{path}/%"))
                    .execute(&mut tx)
                    .await;
                if let Err(error) = update {
                    error!(?error, ?path, "Failed to persist missing media state");
                } else if let Err(error) = tx.commit().await {
                    error!(?error, ?path, "Failed to commit missing media state");
                }
            }
            Err(error) => error!(?error, "Failed to open missing-media transaction"),
        };
    }

    async fn handle_rename(&mut self, from: &Path, to: &Path) {
        let (Some(from), Some(to)) = (from.to_str(), to.to_str()) else {
            return;
        };
        let records = {
            let mut tx = match self.conn.read().begin().await {
                Ok(tx) => tx,
                Err(error) => {
                    error!(?error, "Failed to read renamed paths");
                    return;
                }
            };
            sqlx::query_as::<_, MediaFile>("SELECT * FROM mediafile WHERE library_id = ? AND (target_file = ? OR target_file LIKE ?)")
                .bind(self.library_id)
                .bind(from)
                .bind(format!("{from}/%"))
                .fetch_all(&mut tx)
                .await
                .unwrap_or_default()
        };

        for record in records {
            let suffix = record.target_file.strip_prefix(from).unwrap_or_default();
            let destination = format!("{to}{suffix}");
            let metadata = parse_path(Path::new(&destination));
            let mut lock = self.conn.writer().lock_owned().await;
            let mut tx = match dim_database::write_tx(&mut lock).await {
                Ok(tx) => tx,
                Err(error) => {
                    error!(?error, "Failed to update renamed path");
                    continue;
                }
            };
            let Some(metadata) = metadata else {
                warn!(
                    path = destination,
                    "Rename produced an unparseable media path"
                );
                continue;
            };
            let primary = &metadata[0];
            let query = if record.manual_override {
                "UPDATE mediafile SET target_file = ?, raw_name = ?, raw_year = ?, season = ?, episode = ?, missing_since = NULL WHERE id = ?"
            } else {
                "UPDATE mediafile SET target_file = ?, raw_name = ?, raw_year = ?, season = ?, episode = ?, media_id = NULL, missing_since = NULL WHERE id = ?"
            };
            if sqlx::query(query)
                .bind(&destination)
                .bind(&primary.name)
                .bind(primary.year)
                .bind(primary.season)
                .bind(primary.episode)
                .bind(record.id)
                .execute(&mut tx)
                .await
                .is_err()
                || tx.commit().await.is_err()
            {
                error!(from, to = destination, "Failed to commit media rename");
                continue;
            }
            if !record.manual_override {
                let mut read = match self.conn.read().begin().await {
                    Ok(tx) => tx,
                    Err(_) => continue,
                };
                let Ok(mut updated) = MediaFile::get_one(&mut read, record.id).await else {
                    continue;
                };
                // Retain the previous parent only in this stable work unit so matcher cleanup can
                // remove a now-childless automatic match after the new path has been matched.
                updated.media_id = record.media_id;
                drop(read);
                let mut lock = self.conn.writer().lock_owned().await;
                let Ok(mut tx) = dim_database::write_tx(&mut lock).await else {
                    continue;
                };
                if let Err(error) = self
                    .matcher
                    .batch_match(
                        &mut tx,
                        self.provider.clone(),
                        vec![WorkUnit(updated, metadata)],
                    )
                    .await
                {
                    error!(?error, "Failed to rematch renamed media");
                } else if let Err(error) = tx.commit().await {
                    error!(?error, "Failed to commit renamed media match");
                }
            }
        }
    }

    async fn reconcile(&mut self, locations: &[String]) {
        warn!(
            library_id = self.library_id,
            "Running full library reconciliation"
        );
        if let Err(error) = super::start_custom(
            &mut self.conn,
            self.library_id,
            locations.to_vec(),
            self.tx.clone(),
            self.media_type,
            self.provider.clone(),
        )
        .await
        {
            error!(?error, "Full reconciliation scan failed");
            return;
        }
        let mut lock = self.conn.writer().lock_owned().await;
        let Ok(mut tx) = dim_database::write_tx(&mut lock).await else {
            return;
        };
        // Mark only unreferenced remote assets; do not delete ambiguous files or user assets.
        let _ = sqlx::query("UPDATE assets SET orphaned_at = NULL WHERE id IN (SELECT poster FROM _tblmedia WHERE poster IS NOT NULL UNION SELECT backdrop FROM _tblmedia WHERE backdrop IS NOT NULL UNION SELECT poster FROM _tblseason WHERE poster IS NOT NULL UNION SELECT asset_id FROM media_posters UNION SELECT asset_id FROM media_backdrops)").execute(&mut tx).await;
        let _ = sqlx::query("UPDATE assets SET orphaned_at = COALESCE(orphaned_at, CURRENT_TIMESTAMP) WHERE remote_url IS NOT NULL AND id NOT IN (SELECT poster FROM _tblmedia WHERE poster IS NOT NULL UNION SELECT backdrop FROM _tblmedia WHERE backdrop IS NOT NULL UNION SELECT poster FROM _tblseason WHERE poster IS NOT NULL UNION SELECT asset_id FROM media_posters UNION SELECT asset_id FROM media_backdrops)").execute(&mut tx).await;
        let _ = tx.commit().await;
    }
}

fn parse_path(path: &Path) -> Option<Vec<dim_extern_api::filename::Metadata>> {
    let filename = path.file_stem()?.to_str()?;
    let metadata = [
        TorrentMetadata::from_str(filename),
        Anitomy::from_str(filename),
        CombinedExtractor::from_str(filename),
    ]
    .into_iter()
    .flatten()
    .collect::<Vec<_>>();
    (!metadata.is_empty()).then_some(metadata)
}

pub fn spawn_file_watcher<S>(
    paths: &[S],
) -> Result<(WatchReceiver, RecommendedWatcher), FsWatcherError>
where
    S: AsRef<str>,
{
    let (tx, rx) = mpsc::channel(WATCH_QUEUE_CAPACITY);
    let overflowed = Arc::new(AtomicBool::new(false));
    let callback_overflow = overflowed.clone();
    let mut watcher = RecommendedWatcher::new(
        move |event| {
            if tx.try_send(event).is_err() {
                callback_overflow.store(true, Ordering::Release);
            }
        },
        Config::default(),
    )?;
    for path in paths {
        watcher.watch(Path::new(path.as_ref()), RecursiveMode::Recursive)?;
    }
    Ok((WatchReceiver { rx, overflowed }, watcher))
}

#[cfg(test)]
mod tests {
    use super::*;
    use notify::event::{CreateKind, RemoveKind};

    #[test]
    fn coalesces_bursts_and_preserves_rename_identity() {
        let path = PathBuf::from("/media/a.mkv");
        let renamed = PathBuf::from("/media/b.mkv");
        let mut batch = EventBatch::default();
        for _ in 0..20 {
            batch.push(Ok(
                Event::new(EventKind::Create(CreateKind::File)).add_path(path.clone())
            ));
        }
        batch.push(Ok(Event::new(EventKind::Modify(ModifyKind::Name(
            RenameMode::Both,
        )))
        .add_path(path.clone())
        .add_path(renamed.clone())));
        batch.push(Ok(
            Event::new(EventKind::Remove(RemoveKind::File)).add_path(path.clone())
        ));
        assert_eq!(batch.creates.len(), 1);
        assert_eq!(batch.renames.get(&path), Some(&renamed));
        assert_eq!(batch.removes.len(), 1);
    }

    #[test]
    fn watcher_error_requests_reconciliation() {
        let mut batch = EventBatch::default();
        batch.push(Err(notify::Error::generic("overflow")));
        assert!(batch.reconcile);
    }

    #[test]
    fn pairs_split_rename_events_by_stable_tracker() {
        let from = PathBuf::from("/media/old.mkv");
        let to = PathBuf::from("/media/new.mkv");
        let mut batch = EventBatch::default();
        batch.push(Ok(Event::new(EventKind::Modify(ModifyKind::Name(
            RenameMode::From,
        )))
        .set_tracker(42)
        .add_path(from.clone())));
        batch.push(Ok(Event::new(EventKind::Modify(ModifyKind::Name(
            RenameMode::To,
        )))
        .set_tracker(42)
        .add_path(to.clone())));
        assert_eq!(batch.rename_from.get(&42), Some(&from));
        assert_eq!(batch.rename_to.get(&42), Some(&to));
    }
}
