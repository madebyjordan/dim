// FIXME: We have a shim in dim/utils but we cant depend on dim because itd be a circular dep.
#![deny(warnings)]

use crate::utils::ffpath;

use std::str::FromStr;
use std::sync::atomic::AtomicBool;
use std::sync::atomic::Ordering;
use std::time::Duration;

use sqlx::sqlite::{SqliteConnectOptions, SqliteJournalMode, SqliteSynchronous};
use sqlx::{ConnectOptions, Row};
use tracing::{info, instrument};

use once_cell::sync::OnceCell;

pub mod asset;
pub mod compact_mediafile;
pub mod episode;
pub mod error;
pub mod genre;
pub mod ingestion;
pub mod library;
pub mod media;
pub mod mediafile;
pub mod movie;
pub mod progress;
pub mod query_ext;
pub mod rw_pool;
pub mod season;
pub mod tv;
pub mod user;
pub mod utils;

#[cfg(test)]
pub mod tests;

pub use crate::error::DatabaseError;
/// Ugly hack because of a shitty deadlock in `Pool`
pub use crate::rw_pool::write_tx;
pub use dim_auth::generate_key;
pub use dim_auth::set_key;

pub type DbConnection = rw_pool::SqlitePool;
pub type Transaction<'tx> = sqlx::Transaction<'tx, sqlx::Sqlite>;

lazy_static::lazy_static! {
    static ref MIGRATIONS_FLAG: AtomicBool = AtomicBool::new(false);
}

static __GLOBAL: OnceCell<DbConnection> = OnceCell::new();
const MIGRATOR: sqlx::migrate::Migrator = sqlx::migrate!("./migrations/");

/// How long SQLite waits for another connection to release a lock before returning `BUSY`.
pub const SQLITE_BUSY_TIMEOUT: Duration = Duration::from_secs(5);

/// Dim uses WAL with `synchronous=NORMAL` on file-backed databases. In WAL mode this preserves
/// database consistency and normally survives application/process crashes, while accepting that
/// the most recent committed transaction can be lost after an operating-system or power failure.
fn connection_options(path: &str, read_only: bool) -> SqliteConnectOptions {
    SqliteConnectOptions::new()
        .filename(path)
        .read_only(read_only)
        .create_if_missing(!read_only)
        .foreign_keys(true)
        .journal_mode(SqliteJournalMode::Wal)
        .synchronous(SqliteSynchronous::Normal)
        .busy_timeout(SQLITE_BUSY_TIMEOUT)
}

async fn open_file_pool(path: &str) -> sqlx::Result<DbConnection> {
    // Open the writer first so it can create the database and establish WAL before read-only
    // connections are admitted to the pool.
    let writer = connection_options(path, false).connect().await?;
    let reader = sqlx::pool::PoolOptions::new()
        .connect_with(connection_options(path, true))
        .await?;

    Ok(rw_pool::SqlitePool::new(writer, reader))
}

/// Open and validate an explicitly owned file-backed database. Runtime code should prefer this
/// over the legacy process-global accessor.
pub async fn open_at(path: impl AsRef<std::path::Path>) -> sqlx::Result<DbConnection> {
    let path_ref = path.as_ref();
    let existed = path_ref.exists();
    let path = path_ref.to_string_lossy();
    let pool = open_file_pool(&path).await?;
    #[cfg(unix)]
    if !existed {
        use std::os::unix::fs::PermissionsExt;
        let mut permissions = std::fs::metadata(path_ref)?.permissions();
        permissions.set_mode(0o600);
        std::fs::set_permissions(path_ref, permissions)?;
    }
    prepare_connection(&pool).await?;
    Ok(pool)
}

/// Function runs all migrations embedded to make sure the database works as expected.
///
/// # Arguments
/// * `conn` - diesel connection
async fn run_migrations(conn: &crate::DbConnection) -> Result<(), sqlx::migrate::MigrateError> {
    let mut lock = conn.writer().lock_owned().await;
    MIGRATOR.run(&mut *lock).await
}

/// Refuse to start on referential or cross-library inconsistencies. This assessment deliberately
/// does not modify rows: an existing user's data must be repaired explicitly rather than silently
/// discarded by a migration.
pub async fn validate_integrity(conn: &crate::DbConnection) -> sqlx::Result<()> {
    validate_foreign_keys(conn).await?;
    let mut lock = conn.writer().lock_owned().await;

    let quick_check: String = sqlx::query_scalar("PRAGMA quick_check")
        .fetch_one(&mut *lock)
        .await?;
    if quick_check != "ok" {
        return Err(sqlx::Error::Protocol(format!(
            "SQLite quick_check failed: {quick_check}; refusing to modify data"
        )));
    }

    let mismatched_mediafiles: i64 = sqlx::query_scalar(
        "SELECT COUNT(*) FROM mediafile mf JOIN _tblmedia m ON m.id = mf.media_id \
         WHERE mf.library_id <> m.library_id",
    )
    .fetch_one(&mut *lock)
    .await?;
    if mismatched_mediafiles != 0 {
        return Err(sqlx::Error::Protocol(format!(
            "database contains {mismatched_mediafiles} mediafile record(s) linked to media in a different library; refusing to modify data"
        )));
    }

    let semantic_checks = [
        (
            "library record(s) with an unsupported media type",
            "SELECT COUNT(*) FROM library WHERE media_type NOT IN ('movie', 'tv')",
        ),
        (
            "media record(s) with an unsupported media type",
            "SELECT COUNT(*) FROM _tblmedia WHERE media_type NOT IN ('movie', 'tv', 'episode')",
        ),
        (
            "season record(s) linked to non-TV media",
            "SELECT COUNT(*) FROM _tblseason s JOIN _tblmedia m ON m.id = s.tvshowid WHERE m.media_type <> 'tv'",
        ),
        (
            "episode record(s) whose base media is not an episode",
            "SELECT COUNT(*) FROM episode e JOIN _tblmedia m ON m.id = e.id WHERE m.media_type <> 'episode'",
        ),
        (
            "episode record(s) assigned across library boundaries",
            "SELECT COUNT(*) FROM episode e JOIN _tblmedia em ON em.id = e.id JOIN _tblseason s ON s.id = e.seasonid JOIN _tblmedia tm ON tm.id = s.tvshowid WHERE em.library_id <> tm.library_id",
        ),
    ];
    for (description, query) in semantic_checks {
        let count: i64 = sqlx::query_scalar(query).fetch_one(&mut *lock).await?;
        if count != 0 {
            return Err(sqlx::Error::Protocol(format!(
                "database contains {count} {description}; refusing to modify data"
            )));
        }
    }

    Ok(())
}

async fn validate_foreign_keys(conn: &crate::DbConnection) -> sqlx::Result<()> {
    let mut lock = conn.writer().lock_owned().await;
    let foreign_key_violations = sqlx::query("PRAGMA foreign_key_check")
        .fetch_all(&mut *lock)
        .await?;
    if !foreign_key_violations.is_empty() {
        let samples = foreign_key_violations
            .iter()
            .take(5)
            .map(|row| {
                let table: String = row.try_get(0).unwrap_or_else(|_| "unknown".into());
                let rowid: Option<i64> = row.try_get(1).ok();
                format!("{table}(rowid={rowid:?})")
            })
            .collect::<Vec<_>>()
            .join(", ");
        return Err(sqlx::Error::Protocol(format!(
            "database contains {} foreign-key violation(s); refusing to modify data. Examples: {}",
            foreign_key_violations.len(),
            samples
        )));
    }

    Ok(())
}

async fn prepare_connection(conn: &crate::DbConnection) -> sqlx::Result<()> {
    // Catch legacy orphans before a historical table-copy migration has a chance to encounter
    // them. `foreign_key_check` is valid even for an empty, brand-new database.
    validate_foreign_keys(conn).await?;
    run_migrations(conn)
        .await
        .map_err(|error| sqlx::Error::Protocol(format!("database migration failed: {error}")))?;
    validate_integrity(conn).await
}

/// Function which returns a Result<T, E> where T is a new connection session or E is a connection
/// error.
pub async fn get_conn() -> sqlx::Result<crate::DbConnection> {
    let conn = if let Some(conn) = __GLOBAL.get() {
        conn
    } else {
        let conn = internal_get_conn().await?;
        let _ = __GLOBAL.set(conn);
        __GLOBAL.get().unwrap()
    };

    if !MIGRATIONS_FLAG.load(Ordering::SeqCst) {
        prepare_connection(conn).await?;
        MIGRATIONS_FLAG.store(true, Ordering::SeqCst);
    }

    Ok(conn.clone())
}

#[doc(hidden)]
pub fn set_conn(conn: crate::DbConnection) {
    __GLOBAL.set(conn).unwrap();
}

pub fn try_get_conn() -> Option<&'static crate::DbConnection> {
    __GLOBAL.get()
}

pub async fn get_conn_memory() -> sqlx::Result<crate::DbConnection> {
    // SQLite cannot use WAL for an in-memory database. All other policy settings still apply, and
    // SQLx gives the pool a unique shared-cache URI so its reader connections see the writer data.
    let options = SqliteConnectOptions::from_str(":memory:")?
        .foreign_keys(true)
        .synchronous(SqliteSynchronous::Normal)
        .busy_timeout(SQLITE_BUSY_TIMEOUT);
    let pool = sqlx::pool::PoolOptions::new()
        .max_connections(16)
        .connect_with(options)
        .await?;
    let connection: sqlx::pool::PoolConnection<sqlx::Sqlite> = pool.acquire().await?;
    let rw = connection.detach();
    let pool = rw_pool::SqlitePool::new(rw, pool);
    prepare_connection(&pool).await?;

    Ok(pool)
}

/// Function returns a connection to the development table of dim. This is mainly used for unit
/// tests.
#[doc(hidden)]
pub async fn get_conn_devel() -> sqlx::Result<crate::DbConnection> {
    let pool = open_file_pool("./dim_dev.db").await?;
    prepare_connection(&pool).await?;

    Ok(pool)
}

/// Function which returns a Result<T, E> where T is a new connection session or E is a connection
/// error. It takes in a logger instance.
///
/// # Arguments
/// * `log` - a Slog logger instance
#[instrument]
pub async fn get_conn_logged() -> sqlx::Result<DbConnection> {
    // This is the URL for the database inside a docker container
    let conn = if let Some(conn) = __GLOBAL.get() {
        conn
    } else {
        let conn = internal_get_conn().await?;
        let _ = __GLOBAL.set(conn);
        __GLOBAL.get().unwrap()
    };

    info!("Creating new database connection");

    if !MIGRATIONS_FLAG.load(Ordering::SeqCst) {
        prepare_connection(&conn).await?;
        MIGRATIONS_FLAG.store(true, Ordering::SeqCst);
    }

    Ok(conn.clone())
}

async fn internal_get_conn() -> sqlx::Result<DbConnection> {
    open_file_pool(&ffpath("config/dim.db")).await
}

#[doc(hidden)]
pub async fn get_conn_file(file: &str) -> sqlx::Result<crate::DbConnection> {
    let pool = open_file_pool(file).await?;
    prepare_connection(&pool).await?;

    Ok(pool)
}

#[cfg(all(test, unix))]
mod permission_tests {
    use std::os::unix::fs::PermissionsExt;

    #[tokio::test]
    async fn newly_created_runtime_database_is_private() {
        let directory = tempfile::tempdir().unwrap();
        let path = directory.path().join("dim.db");
        let _connection = super::open_at(&path).await.unwrap();
        assert_eq!(
            std::fs::metadata(path).unwrap().permissions().mode() & 0o777,
            0o600
        );
    }
}
