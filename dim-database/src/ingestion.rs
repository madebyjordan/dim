use crate::{DatabaseError, Transaction};
use serde::Serialize;

#[derive(Clone, Debug, Serialize, sqlx::FromRow)]
pub struct ScanRun {
    pub id: i64,
    pub library_id: i64,
    pub kind: String,
    pub status: String,
    pub requested_at: String,
    pub started_at: Option<String>,
    pub finished_at: Option<String>,
    pub stage: String,
    pub last_progress_at: String,
    pub discovered: i64,
    pub processed: i64,
    pub committed: i64,
    pub failed: i64,
    pub skipped: i64,
    pub error: Option<String>,
}

pub async fn begin_root(
    tx: &mut Transaction<'_>,
    scan_id: i64,
    ordinal: i64,
    root: &str,
    normalized_root: Option<&str>,
) -> Result<i64, DatabaseError> {
    Ok(sqlx::query(
        "INSERT INTO ingestion_scan_root (scan_id, ordinal, root, normalized_root) VALUES (?, ?, ?, ?)",
    )
    .bind(scan_id)
    .bind(ordinal)
    .bind(root)
    .bind(normalized_root)
    .execute(&mut *tx)
    .await?
    .last_insert_rowid())
}

pub async fn finish_root(
    tx: &mut Transaction<'_>,
    root_id: i64,
    status: &str,
    error: Option<&str>,
) -> Result<(), DatabaseError> {
    sqlx::query(
        "UPDATE ingestion_scan_root SET status = ?, error = ?, finished_at = CURRENT_TIMESTAMP WHERE id = ?",
    )
    .bind(status)
    .bind(error)
    .bind(root_id)
    .execute(&mut *tx)
    .await?;
    Ok(())
}

pub async fn cancel_active_roots(
    tx: &mut Transaction<'_>,
    scan_id: i64,
) -> Result<(), DatabaseError> {
    sqlx::query("UPDATE ingestion_scan_root SET status = 'cancelled', error = COALESCE(error, 'scan task was cancelled before traversal became authoritative'), finished_at = CURRENT_TIMESTAMP WHERE scan_id = ? AND status = 'running'")
        .bind(scan_id)
        .execute(&mut *tx)
        .await?;
    Ok(())
}

impl ScanRun {
    /// Mark work that belonged to a previous process as terminal. This is intentionally a
    /// process-boundary lease rather than a wall-clock timeout: a quiet network mount or metadata
    /// provider is not evidence that the current process abandoned its scan.
    pub async fn recover_abandoned(tx: &mut Transaction<'_>) -> Result<(u64, u64), DatabaseError> {
        sqlx::query("UPDATE ingestion_scan_root SET status = 'cancelled', finished_at = CURRENT_TIMESTAMP, error = COALESCE(error, 'scanner process restarted before traversal became authoritative') WHERE status = 'running' AND scan_id IN (SELECT id FROM ingestion_scan WHERE status IN ('queued', 'running'))")
            .execute(&mut *tx)
            .await?;
        let running = sqlx::query("UPDATE ingestion_scan SET status = 'failed', stage = 'failed', finished_at = CURRENT_TIMESTAMP, last_progress_at = CURRENT_TIMESTAMP, error = COALESCE(error, 'Eclipse restarted before this scan finished; retry the scan') WHERE status = 'running'")
            .execute(&mut *tx)
            .await?
            .rows_affected();
        let queued = sqlx::query("UPDATE ingestion_scan SET status = 'cancelled', stage = 'cancelled', finished_at = CURRENT_TIMESTAMP, last_progress_at = CURRENT_TIMESTAMP, error = COALESCE(error, 'Eclipse restarted before the scanner worker began; retry the scan') WHERE status = 'queued'")
            .execute(&mut *tx)
            .await?
            .rows_affected();
        Ok((running, queued))
    }

    pub async fn queue(
        tx: &mut Transaction<'_>,
        library_id: i64,
        kind: &str,
    ) -> Result<i64, DatabaseError> {
        Ok(sqlx::query(
            "INSERT INTO ingestion_scan (library_id, kind, status, last_progress_at) VALUES (?, ?, 'queued', CURRENT_TIMESTAMP)",
        )
        .bind(library_id)
        .bind(kind)
        .execute(&mut *tx)
        .await?
        .last_insert_rowid())
    }

    pub async fn begin(
        tx: &mut Transaction<'_>,
        library_id: i64,
        kind: &str,
    ) -> Result<i64, DatabaseError> {
        // The per-library ownership gate permits only one live scanner. Preserve any prior active
        // row as an explicit failure before the replacement takes ownership.
        sqlx::query("UPDATE ingestion_scan_root SET status = 'cancelled', finished_at = CURRENT_TIMESTAMP, error = COALESCE(error, 'interrupted before replacement traversal became authoritative') WHERE status = 'running' AND scan_id IN (SELECT id FROM ingestion_scan WHERE library_id = ? AND status = 'running')")
            .bind(library_id)
            .execute(&mut *tx)
            .await?;
        sqlx::query("UPDATE ingestion_scan SET status = 'failed', stage = 'failed', finished_at = CURRENT_TIMESTAMP, last_progress_at = CURRENT_TIMESTAMP, error = COALESCE(error, 'interrupted before a replacement scan began') WHERE library_id = ? AND status = 'running'")
            .bind(library_id)
            .execute(&mut *tx)
            .await?;
        if let Some(id) = sqlx::query_scalar::<_, i64>("SELECT id FROM ingestion_scan WHERE library_id = ? AND status = 'queued' ORDER BY id DESC LIMIT 1")
            .bind(library_id).fetch_optional(&mut *tx).await?
        {
            sqlx::query("UPDATE ingestion_scan SET status = 'running', stage = 'starting', started_at = CURRENT_TIMESTAMP, last_progress_at = CURRENT_TIMESTAMP WHERE id = ?")
                .bind(id).execute(&mut *tx).await?;
            return Ok(id);
        }
        let id = sqlx::query("INSERT INTO ingestion_scan (library_id, kind, status, stage, started_at, last_progress_at) VALUES (?, ?, 'running', 'starting', CURRENT_TIMESTAMP, CURRENT_TIMESTAMP)")
            .bind(library_id)
            .bind(kind)
            .execute(&mut *tx)
            .await?
            .last_insert_rowid();
        Ok(id)
    }

    pub async fn latest(
        tx: &mut Transaction<'_>,
        library_id: i64,
    ) -> Result<Option<Self>, DatabaseError> {
        Ok(sqlx::query_as::<_, Self>(
            "SELECT * FROM ingestion_scan WHERE library_id = ? ORDER BY id DESC LIMIT 1",
        )
        .bind(library_id)
        .fetch_optional(&mut *tx)
        .await?)
    }

    pub async fn finish(
        tx: &mut Transaction<'_>,
        id: i64,
        status: &str,
        error: Option<&str>,
    ) -> Result<(), DatabaseError> {
        sqlx::query("UPDATE ingestion_scan SET status = ?, stage = ?, error = ?, finished_at = CURRENT_TIMESTAMP, last_progress_at = CURRENT_TIMESTAMP WHERE id = ?")
            .bind(status)
            .bind(status)
            .bind(error)
            .bind(id)
            .execute(&mut *tx)
            .await?;
        Ok(())
    }

    pub async fn finish_active(
        tx: &mut Transaction<'_>,
        id: i64,
        status: &str,
        error: Option<&str>,
    ) -> Result<bool, DatabaseError> {
        let result = sqlx::query("UPDATE ingestion_scan SET status = ?, stage = ?, error = ?, finished_at = CURRENT_TIMESTAMP, last_progress_at = CURRENT_TIMESTAMP WHERE id = ? AND status IN ('queued', 'running')")
            .bind(status)
            .bind(status)
            .bind(error)
            .bind(id)
            .execute(&mut *tx)
            .await?;
        Ok(result.rows_affected() == 1)
    }

    pub async fn count(
        tx: &mut Transaction<'_>,
        id: i64,
        column: &str,
    ) -> Result<(), DatabaseError> {
        let query = match column {
            "discovered" => "UPDATE ingestion_scan SET discovered = discovered + 1, last_progress_at = CURRENT_TIMESTAMP WHERE id = ?",
            "processed" => "UPDATE ingestion_scan SET processed = processed + 1, last_progress_at = CURRENT_TIMESTAMP WHERE id = ?",
            "committed" => "UPDATE ingestion_scan SET committed = committed + 1, last_progress_at = CURRENT_TIMESTAMP WHERE id = ?",
            "failed" => "UPDATE ingestion_scan SET failed = failed + 1, last_progress_at = CURRENT_TIMESTAMP WHERE id = ?",
            "skipped" => "UPDATE ingestion_scan SET skipped = skipped + 1, last_progress_at = CURRENT_TIMESTAMP WHERE id = ?",
            _ => return Ok(()),
        };
        sqlx::query(query).bind(id).execute(&mut *tx).await?;
        Ok(())
    }

    /// Persist a stage transition immediately, or coalesce repeated heartbeats to at most one
    /// update per interval. Count mutations also advance `last_progress_at` in their existing
    /// transaction, so busy scans do not generate a second stream of writes.
    pub async fn touch(
        tx: &mut Transaction<'_>,
        id: i64,
        stage: &str,
        minimum_interval_seconds: i64,
    ) -> Result<bool, DatabaseError> {
        let result = sqlx::query("UPDATE ingestion_scan SET stage = ?, last_progress_at = CURRENT_TIMESTAMP WHERE id = ? AND status = 'running' AND (stage != ? OR last_progress_at <= datetime('now', '-' || ? || ' seconds'))")
            .bind(stage)
            .bind(id)
            .bind(stage)
            .bind(minimum_interval_seconds)
            .execute(&mut *tx)
            .await?;
        Ok(result.rows_affected() == 1)
    }
}

pub async fn upsert_item(
    tx: &mut Transaction<'_>,
    scan_id: i64,
    library_id: i64,
    root_id: Option<i64>,
    path: &str,
    fingerprint: Option<&str>,
    stage: &str,
    status: &str,
    error_class: Option<&str>,
    error_message: Option<&str>,
) -> Result<(), DatabaseError> {
    sqlx::query("INSERT INTO ingestion_item (scan_id, library_id, root_id, path, fingerprint, stage, status, attempts, error_class, error_message) VALUES (?, ?, ?, ?, ?, ?, ?, 1, ?, ?) ON CONFLICT(scan_id, path) DO UPDATE SET root_id = CASE WHEN ingestion_item.root_id = excluded.root_id OR excluded.root_id IS NULL THEN ingestion_item.root_id ELSE NULL END, fingerprint = COALESCE(excluded.fingerprint, fingerprint), stage = excluded.stage, status = excluded.status, attempts = attempts + 1, error_class = excluded.error_class, error_message = excluded.error_message, updated_at = CURRENT_TIMESTAMP")
        .bind(scan_id)
        .bind(library_id)
        .bind(root_id)
        .bind(path)
        .bind(fingerprint)
        .bind(stage)
        .bind(status)
        .bind(error_class)
        .bind(error_message)
        .execute(&mut *tx)
        .await?;
    Ok(())
}
