use std::collections::{HashMap, HashSet};
use std::future::Future;
use std::sync::Arc;

use tokio::sync::{oneshot, Mutex};
use tokio::task::JoinHandle;

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum LibraryWorkerKind {
    Scanner,
    Watcher,
}

struct Worker {
    cancel: oneshot::Sender<()>,
    handle: JoinHandle<()>,
}

#[derive(Default)]
struct WorkerSet {
    scanner: Option<Worker>,
    watcher: Option<Worker>,
}

impl WorkerSet {
    fn slot(&mut self, kind: LibraryWorkerKind) -> &mut Option<Worker> {
        match kind {
            LibraryWorkerKind::Scanner => &mut self.scanner,
            LibraryWorkerKind::Watcher => &mut self.watcher,
        }
    }

    fn into_workers(self) -> impl Iterator<Item = Worker> {
        self.scanner.into_iter().chain(self.watcher)
    }
}

#[derive(Default)]
struct State {
    workers: HashMap<i64, WorkerSet>,
    stopping_libraries: HashSet<i64>,
    shutting_down: bool,
}

/// Owns every long-running scanner and filesystem watcher.
///
/// Stopping a library first installs a permanent in-process tombstone, closing the race where a
/// retry can register after deletion has begun. Dropping a scanner future is transaction-safe;
/// an already-running blocking directory walk may finish, but its result can no longer reach the
/// database.
#[derive(Clone, Default)]
pub struct LibraryWorkers {
    state: Arc<Mutex<State>>,
}

impl std::fmt::Debug for LibraryWorkers {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter
            .debug_struct("LibraryWorkers")
            .finish_non_exhaustive()
    }
}

impl LibraryWorkers {
    pub async fn spawn<F>(
        &self,
        library_id: i64,
        kind: LibraryWorkerKind,
        future: F,
    ) -> Result<(), &'static str>
    where
        F: Future<Output = ()> + Send + 'static,
    {
        let mut state = self.state.lock().await;
        if state.shutting_down || state.stopping_libraries.contains(&library_id) {
            return Err("library workers are stopping");
        }

        let (cancel, cancelled) = oneshot::channel();
        let handle = tokio::spawn(async move {
            tokio::select! {
                _ = future => {}
                _ = cancelled => {}
            }
        });
        let worker = Worker { cancel, handle };
        let old = state
            .workers
            .entry(library_id)
            .or_default()
            .slot(kind)
            .replace(worker);
        drop(state);

        if let Some(old) = old {
            stop_worker(old).await;
        }
        Ok(())
    }

    /// Prevent new workers for `library_id`, cancel current workers, and await their exit.
    pub async fn stop_library(&self, library_id: i64) {
        let workers = {
            let mut state = self.state.lock().await;
            state.stopping_libraries.insert(library_id);
            state.workers.remove(&library_id)
        };

        if let Some(workers) = workers {
            for worker in workers.into_workers() {
                stop_worker(worker).await;
            }
        }
    }

    /// Cancel and await all managed tasks and permanently reject new work.
    pub async fn shutdown(&self) {
        let workers = {
            let mut state = self.state.lock().await;
            state.shutting_down = true;
            state
                .workers
                .drain()
                .map(|(_, workers)| workers)
                .collect::<Vec<_>>()
        };

        for workers in workers {
            for worker in workers.into_workers() {
                stop_worker(worker).await;
            }
        }
    }

    #[cfg(test)]
    async fn contains(&self, library_id: i64) -> bool {
        self.state.lock().await.workers.contains_key(&library_id)
    }
}

async fn stop_worker(worker: Worker) {
    let _ = worker.cancel.send(());
    let _ = worker.handle.await;
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::atomic::{AtomicBool, Ordering};
    use tokio::sync::Notify;

    struct Dropped(Arc<AtomicBool>);

    impl Drop for Dropped {
        fn drop(&mut self) {
            self.0.store(true, Ordering::SeqCst);
        }
    }

    #[tokio::test]
    async fn stop_cancels_awaits_and_rejects_late_registration() {
        let workers = LibraryWorkers::default();
        let started = Arc::new(Notify::new());
        let dropped = Arc::new(AtomicBool::new(false));
        let started_task = started.clone();
        let dropped_task = dropped.clone();

        workers
            .spawn(7, LibraryWorkerKind::Scanner, async move {
                let _guard = Dropped(dropped_task);
                started_task.notify_one();
                std::future::pending::<()>().await;
            })
            .await
            .unwrap();
        started.notified().await;
        assert!(workers.contains(7).await);

        workers.stop_library(7).await;
        assert!(dropped.load(Ordering::SeqCst));
        assert!(!workers.contains(7).await);
        assert!(workers
            .spawn(7, LibraryWorkerKind::Watcher, async {})
            .await
            .is_err());
    }

    #[tokio::test]
    async fn shutdown_drains_all_workers() {
        let workers = LibraryWorkers::default();
        workers
            .spawn(1, LibraryWorkerKind::Scanner, std::future::pending())
            .await
            .unwrap();
        workers
            .spawn(2, LibraryWorkerKind::Watcher, std::future::pending())
            .await
            .unwrap();

        workers.shutdown().await;
        assert!(!workers.contains(1).await);
        assert!(!workers.contains(2).await);
        assert!(workers
            .spawn(3, LibraryWorkerKind::Scanner, async {})
            .await
            .is_err());
    }
}
