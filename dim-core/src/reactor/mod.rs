//! Durable, transactionally safe application-domain event delivery.
//!
//! SQLite triggers append to `runtime_event_outbox` in the same transaction as domain writes.
//! This dispatcher reads committed rows in sequence order and deletes each row only after the
//! consumer succeeds. A crash or consumer error therefore retries from the first undelivered row.

pub mod handler;
mod types;

use async_trait::async_trait;
use dim_database::DbConnection;
use sqlx::Row;
use std::time::Duration;
use tokio::sync::watch;
use tracing::{error, warn};
use types::{Event, EventType, Table};

#[async_trait]
pub trait Reactor {
    type Error: ::std::error::Error;
    async fn react(&mut self, event: Event) -> Result<(), Self::Error>;
}

pub struct OutboxDispatcher<R> {
    pool: DbConnection,
    reactor: R,
    poll_interval: Duration,
}

impl<R> OutboxDispatcher<R>
where
    R: Reactor,
{
    pub fn new(pool: DbConnection, reactor: R) -> Self {
        Self {
            pool,
            reactor,
            poll_interval: Duration::from_millis(250),
        }
    }

    #[cfg(test)]
    fn with_poll_interval(mut self, poll_interval: Duration) -> Self {
        self.poll_interval = poll_interval;
        self
    }

    pub async fn run(mut self, mut shutdown: watch::Receiver<bool>) {
        loop {
            if *shutdown.borrow() {
                break;
            }
            match self.dispatch_batch().await {
                Ok(0) => {
                    tokio::select! {
                        _ = tokio::time::sleep(self.poll_interval) => {}
                        changed = shutdown.changed() => if changed.is_err() || *shutdown.borrow() { break; }
                    }
                }
                Ok(_) => {}
                Err(error) => {
                    error!(%error, "Outbox dispatch paused; the event remains durable for retry");
                    tokio::select! {
                        _ = tokio::time::sleep(Duration::from_secs(1)) => {}
                        changed = shutdown.changed() => if changed.is_err() || *shutdown.borrow() { break; }
                    }
                }
            }
        }
    }

    async fn dispatch_batch(&mut self) -> Result<usize, DispatchError<R::Error>> {
        let rows = sqlx::query(
            "SELECT sequence, source_table, row_id, event_type FROM runtime_event_outbox ORDER BY sequence LIMIT 128",
        )
        .fetch_all(self.pool.read_ref())
        .await
        .map_err(DispatchError::Database)?;

        let count = rows.len();
        for row in rows {
            let sequence: i64 = row.try_get("sequence").map_err(DispatchError::Database)?;
            let source_table: String = row
                .try_get("source_table")
                .map_err(DispatchError::Database)?;
            let event_type: String = row.try_get("event_type").map_err(DispatchError::Database)?;
            let event = Event {
                id: row.try_get("row_id").map_err(DispatchError::Database)?,
                table: Table::try_from(source_table.as_str())
                    .map_err(|_| DispatchError::Invalid(sequence))?,
                event_type: EventType::try_from(event_type.as_str())
                    .map_err(|_| DispatchError::Invalid(sequence))?,
            };
            self.reactor
                .react(event)
                .await
                .map_err(DispatchError::Consumer)?;

            let mut lock = self.pool.writer().lock_owned().await;
            let result = sqlx::query("DELETE FROM runtime_event_outbox WHERE sequence = ?")
                .bind(sequence)
                .execute(&mut *lock)
                .await
                .map_err(DispatchError::Database)?;
            if result.rows_affected() != 1 {
                warn!(sequence, "Outbox row was already acknowledged");
            }
        }
        Ok(count)
    }
}

#[derive(Debug)]
enum DispatchError<E> {
    Database(sqlx::Error),
    Consumer(E),
    Invalid(i64),
}

impl<E: std::fmt::Display> std::fmt::Display for DispatchError<E> {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Database(error) => write!(formatter, "database error: {error}"),
            Self::Consumer(error) => write!(formatter, "consumer error: {error}"),
            Self::Invalid(sequence) => {
                write!(formatter, "invalid outbox row at sequence {sequence}")
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::{Arc, Mutex};

    struct RecordingReactor {
        seen: Arc<Mutex<Vec<i64>>>,
        fail_once: bool,
    }

    #[async_trait]
    impl Reactor for RecordingReactor {
        type Error = std::io::Error;
        async fn react(&mut self, event: Event) -> Result<(), Self::Error> {
            if self.fail_once {
                self.fail_once = false;
                return Err(std::io::Error::new(
                    std::io::ErrorKind::Other,
                    "backpressure",
                ));
            }
            self.seen.lock().unwrap().push(event.id);
            Ok(())
        }
    }

    #[tokio::test]
    async fn retains_order_and_retries_after_consumer_error() {
        let pool = dim_database::get_conn_memory().await.unwrap();
        let mut lock = pool.writer().lock_owned().await;
        sqlx::query("INSERT INTO library(id, name, media_type) VALUES (41, 'one', 'movie'), (42, 'two', 'movie')")
            .execute(&mut *lock).await.unwrap();
        drop(lock);
        let seen = Arc::new(Mutex::new(Vec::new()));
        let reactor = RecordingReactor {
            seen: seen.clone(),
            fail_once: true,
        };
        let mut dispatcher = OutboxDispatcher::new(pool.clone(), reactor)
            .with_poll_interval(Duration::from_millis(1));
        assert!(dispatcher.dispatch_batch().await.is_err());
        assert_eq!(
            sqlx::query_scalar::<_, i64>("SELECT COUNT(*) FROM runtime_event_outbox")
                .fetch_one(pool.read_ref())
                .await
                .unwrap(),
            2
        );
        assert_eq!(dispatcher.dispatch_batch().await.unwrap(), 2);
        assert_eq!(*seen.lock().unwrap(), vec![41, 42]);
    }

    #[tokio::test]
    async fn rolled_back_changes_never_enter_outbox() {
        let pool = dim_database::get_conn_memory().await.unwrap();
        let mut lock = pool.writer().lock_owned().await;
        let mut tx = dim_database::write_tx(&mut lock).await.unwrap();
        sqlx::query("INSERT INTO library(id, name, media_type) VALUES (99, 'rollback', 'movie')")
            .execute(&mut tx)
            .await
            .unwrap();
        tx.rollback().await.unwrap();
        assert_eq!(
            sqlx::query_scalar::<_, i64>("SELECT COUNT(*) FROM runtime_event_outbox")
                .fetch_one(pool.read_ref())
                .await
                .unwrap(),
            0
        );
    }

    #[tokio::test]
    async fn restart_dispatch_skips_stale_insert_without_blocking_later_delete() {
        let pool = dim_database::get_conn_memory().await.unwrap();
        let mut lock = pool.writer().lock_owned().await;
        sqlx::query(
            "INSERT INTO library(id, name, media_type) VALUES (77, 'short-lived', 'movie')",
        )
        .execute(&mut *lock)
        .await
        .unwrap();
        sqlx::query("DELETE FROM library WHERE id = 77")
            .execute(&mut *lock)
            .await
            .unwrap();
        drop(lock);
        let (event_tx, mut event_rx) = tokio::sync::mpsc::channel(1);
        let reactor = handler::EventReactor::new(pool.clone()).with_websocket(event_tx);
        let mut dispatcher = OutboxDispatcher::new(pool.clone(), reactor);
        assert_eq!(dispatcher.dispatch_batch().await.unwrap(), 2);
        assert!(event_rx
            .recv()
            .await
            .unwrap()
            .contains("EventRemoveLibrary"));
        assert_eq!(
            sqlx::query_scalar::<_, i64>("SELECT COUNT(*) FROM runtime_event_outbox")
                .fetch_one(pool.read_ref())
                .await
                .unwrap(),
            0
        );
    }
}
