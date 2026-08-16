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
    pub discovered: i64,
    pub committed: i64,
    pub failed: i64,
    pub skipped: i64,
    pub error: Option<String>,
}

impl ScanRun {
    pub async fn queue(
        tx: &mut Transaction<'_>,
        library_id: i64,
        kind: &str,
    ) -> Result<i64, DatabaseError> {
        Ok(sqlx::query(
            "INSERT INTO ingestion_scan (library_id, kind, status) VALUES (?, ?, 'queued')",
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
        // A process that disappeared cannot still own useful in-memory work. Preserve the old
        // run as failed so status endpoints never mistake missing memory for completion.
        sqlx::query("UPDATE ingestion_scan SET status = 'failed', finished_at = CURRENT_TIMESTAMP, error = COALESCE(error, 'interrupted by process restart') WHERE library_id = ? AND status = 'running'")
            .bind(library_id)
            .execute(&mut *tx)
            .await?;
        if let Some(id) = sqlx::query_scalar::<_, i64>("SELECT id FROM ingestion_scan WHERE library_id = ? AND status = 'queued' ORDER BY id DESC LIMIT 1")
            .bind(library_id).fetch_optional(&mut *tx).await?
        {
            sqlx::query("UPDATE ingestion_scan SET status = 'running', started_at = CURRENT_TIMESTAMP WHERE id = ?")
                .bind(id).execute(&mut *tx).await?;
            return Ok(id);
        }
        let id = sqlx::query("INSERT INTO ingestion_scan (library_id, kind, status, started_at) VALUES (?, ?, 'running', CURRENT_TIMESTAMP)")
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
        sqlx::query("UPDATE ingestion_scan SET status = ?, error = ?, finished_at = CURRENT_TIMESTAMP WHERE id = ?")
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
        let result = sqlx::query("UPDATE ingestion_scan SET status = ?, error = ?, finished_at = CURRENT_TIMESTAMP WHERE id = ? AND status IN ('queued', 'running')")
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
            "discovered" => "UPDATE ingestion_scan SET discovered = discovered + 1 WHERE id = ?",
            "committed" => "UPDATE ingestion_scan SET committed = committed + 1 WHERE id = ?",
            "failed" => "UPDATE ingestion_scan SET failed = failed + 1 WHERE id = ?",
            "skipped" => "UPDATE ingestion_scan SET skipped = skipped + 1 WHERE id = ?",
            _ => return Ok(()),
        };
        sqlx::query(query).bind(id).execute(&mut *tx).await?;
        Ok(())
    }
}

pub async fn upsert_item(
    tx: &mut Transaction<'_>,
    scan_id: i64,
    library_id: i64,
    path: &str,
    fingerprint: Option<&str>,
    stage: &str,
    status: &str,
    error_class: Option<&str>,
    error_message: Option<&str>,
) -> Result<(), DatabaseError> {
    sqlx::query("INSERT INTO ingestion_item (scan_id, library_id, path, fingerprint, stage, status, attempts, error_class, error_message) VALUES (?, ?, ?, ?, ?, ?, 1, ?, ?) ON CONFLICT(scan_id, path) DO UPDATE SET fingerprint = COALESCE(excluded.fingerprint, fingerprint), stage = excluded.stage, status = excluded.status, attempts = attempts + 1, error_class = excluded.error_class, error_message = excluded.error_message, updated_at = CURRENT_TIMESTAMP")
        .bind(scan_id)
        .bind(library_id)
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
