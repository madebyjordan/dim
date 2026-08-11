use dim_core::core::EventTx;
use dim_core::runtime_paths::RuntimePaths;
use dim_core::settings::SettingsStore;
use dim_core::workers::LibraryWorkers;
use dim_database::DbConnection;
use std::path::PathBuf;
use tokio::sync::{mpsc, watch};

const EVENT_QUEUE_CAPACITY: usize = 1024;

/// Explicit owner for process-wide configuration, database access, worker supervision, event
/// delivery and shutdown notification. Legacy globals remain only behind compatibility call sites.
pub struct ApplicationContext {
    pub settings: SettingsStore,
    pub paths: RuntimePaths,
    pub database: DbConnection,
    pub library_workers: LibraryWorkers,
    pub event_tx: EventTx,
    event_rx: Option<mpsc::Receiver<String>>,
    shutdown_tx: watch::Sender<bool>,
}

impl ApplicationContext {
    pub async fn build(config_path: PathBuf) -> Result<Self, Box<dyn std::error::Error>> {
        let mut settings = SettingsStore::load(&config_path)?;
        if settings.running().secret_key.is_none() {
            let mut with_key = settings.running().clone();
            with_key.secret_key = Some(dim_database::generate_key());
            settings.save_for_restart(&with_key)?;
            settings = SettingsStore::load(&config_path)?;
        }
        let paths = RuntimePaths::from_settings(&config_path, settings.running());
        paths.prepare()?;
        let database = dim_database::open_at(&paths.database)
            .await
            .map_err(|error| {
                std::io::Error::other(format!(
                    "failed to open and validate database at '{}': {error}",
                    paths.database.display()
                ))
            })?;
        let (event_tx, event_rx) = mpsc::channel(EVENT_QUEUE_CAPACITY);
        let (shutdown_tx, _) = watch::channel(false);
        Ok(Self {
            settings,
            paths,
            database,
            library_workers: LibraryWorkers::default(),
            event_tx,
            event_rx: Some(event_rx),
            shutdown_tx,
        })
    }

    pub fn take_event_rx(&mut self) -> mpsc::Receiver<String> {
        self.event_rx
            .take()
            .expect("event receiver can only be owned by one web runtime")
    }

    pub fn shutdown_receiver(&self) -> watch::Receiver<bool> {
        self.shutdown_tx.subscribe()
    }
    pub fn shutdown_sender(&self) -> watch::Sender<bool> {
        self.shutdown_tx.clone()
    }
    pub fn request_shutdown(&self) {
        let _ = self.shutdown_tx.send(true);
    }

    pub async fn shutdown(&self) {
        self.library_workers.shutdown().await;
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn context_constructs_owned_state_and_tears_workers_down() {
        let directory = tempfile::tempdir().unwrap();
        let old_directory = std::env::current_dir().unwrap();
        std::env::set_current_dir(directory.path()).unwrap();
        let context = ApplicationContext::build(directory.path().join("config/config.toml"))
            .await
            .unwrap();
        context
            .library_workers
            .spawn(
                1,
                dim_core::workers::LibraryWorkerKind::Scanner,
                std::future::pending(),
            )
            .await
            .unwrap();
        context.shutdown().await;
        assert!(context
            .library_workers
            .spawn(2, dim_core::workers::LibraryWorkerKind::Scanner, async {})
            .await
            .is_err());
        std::env::set_current_dir(old_directory).unwrap();
    }
}
