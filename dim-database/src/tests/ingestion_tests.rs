use crate::ingestion::ScanRun;
use crate::library::{InsertableLibrary, MediaType};

async fn library(conn: &crate::DbConnection, name: &str) -> i64 {
    let mut lock = conn.writer().lock_owned().await;
    let mut tx = crate::write_tx(&mut lock).await.unwrap();
    let id = InsertableLibrary {
        name: name.into(),
        locations: vec![format!("/media/{name}")],
        media_type: MediaType::Movie,
    }
    .insert(&mut tx)
    .await
    .unwrap();
    tx.commit().await.unwrap();
    id
}

#[tokio::test]
async fn replacement_scan_preserves_interrupted_failure() {
    let conn = crate::get_conn_memory().await.unwrap();
    let library_id = {
        let mut lock = conn.writer().lock_owned().await;
        let mut tx = crate::write_tx(&mut lock).await.unwrap();
        let id = InsertableLibrary {
            name: "restart recovery".into(),
            locations: vec!["/media".into()],
            media_type: MediaType::Movie,
        }
        .insert(&mut tx)
        .await
        .unwrap();
        tx.commit().await.unwrap();
        id
    };
    let (first, second) = {
        let mut lock = conn.writer().lock_owned().await;
        let mut tx = crate::write_tx(&mut lock).await.unwrap();
        let first = ScanRun::begin(&mut tx, library_id, "full").await.unwrap();
        crate::ingestion::begin_root(&mut tx, first, 0, "/media", Some("/media"))
            .await
            .unwrap();
        let second = ScanRun::begin(&mut tx, library_id, "recovery")
            .await
            .unwrap();
        tx.commit().await.unwrap();
        (first, second)
    };
    let mut tx = conn.read().begin().await.unwrap();
    let old_status: (String, Option<String>) =
        sqlx::query_as("SELECT status, error FROM ingestion_scan WHERE id = ?")
            .bind(first)
            .fetch_one(&mut tx)
            .await
            .unwrap();
    assert_eq!(old_status.0, "failed");
    assert!(old_status.1.unwrap().contains("interrupted"));
    let root_status: (String, Option<String>) =
        sqlx::query_as("SELECT status, error FROM ingestion_scan_root WHERE scan_id = ?")
            .bind(first)
            .fetch_one(&mut tx)
            .await
            .unwrap();
    assert_eq!(root_status.0, "cancelled");
    assert!(root_status.1.unwrap().contains("interrupted"));
    assert_eq!(
        ScanRun::latest(&mut tx, library_id)
            .await
            .unwrap()
            .unwrap()
            .id,
        second
    );
}

#[tokio::test]
async fn heartbeat_advances_an_old_but_healthy_running_scan() {
    let conn = crate::get_conn_memory().await.unwrap();
    let library_id = library(&conn, "healthy heartbeat").await;
    let scan_id = {
        let mut lock = conn.writer().lock_owned().await;
        let mut tx = crate::write_tx(&mut lock).await.unwrap();
        let id = ScanRun::begin(&mut tx, library_id, "full").await.unwrap();
        sqlx::query(
            "UPDATE ingestion_scan SET last_progress_at = '2000-01-01 00:00:00' WHERE id = ?",
        )
        .bind(id)
        .execute(&mut tx)
        .await
        .unwrap();
        assert!(ScanRun::touch(&mut tx, id, "matching", 15).await.unwrap());
        tx.commit().await.unwrap();
        id
    };

    let mut tx = conn.read().begin().await.unwrap();
    let run = ScanRun::latest(&mut tx, library_id).await.unwrap().unwrap();
    assert_eq!(run.id, scan_id);
    assert_eq!(run.status, "running");
    assert_eq!(run.stage, "matching");
    assert_ne!(run.last_progress_at, "2000-01-01 00:00:00");
}

#[tokio::test]
async fn startup_recovery_terminals_stale_running_and_queued_scans() {
    let conn = crate::get_conn_memory().await.unwrap();
    let running_library = library(&conn, "stale running").await;
    let queued_library = library(&conn, "stale queued").await;
    let (running_id, queued_id) = {
        let mut lock = conn.writer().lock_owned().await;
        let mut tx = crate::write_tx(&mut lock).await.unwrap();
        let running = ScanRun::begin(&mut tx, running_library, "full")
            .await
            .unwrap();
        let queued = ScanRun::queue(&mut tx, queued_library, "full")
            .await
            .unwrap();
        assert_eq!(ScanRun::recover_abandoned(&mut tx).await.unwrap(), (1, 1));
        tx.commit().await.unwrap();
        (running, queued)
    };

    let mut tx = conn.read().begin().await.unwrap();
    let running: (String, String, Option<String>) =
        sqlx::query_as("SELECT status, stage, error FROM ingestion_scan WHERE id = ?")
            .bind(running_id)
            .fetch_one(&mut tx)
            .await
            .unwrap();
    assert_eq!(
        (&running.0, &running.1),
        (&"failed".into(), &"failed".into())
    );
    assert!(running.2.unwrap().contains("retry"));

    let queued: (String, String, Option<String>) =
        sqlx::query_as("SELECT status, stage, error FROM ingestion_scan WHERE id = ?")
            .bind(queued_id)
            .fetch_one(&mut tx)
            .await
            .unwrap();
    assert_eq!(
        (&queued.0, &queued.1),
        (&"cancelled".into(), &"cancelled".into())
    );
    assert!(queued.2.unwrap().contains("retry"));
}

#[tokio::test]
async fn terminal_scans_keep_explicit_diagnostics_and_are_not_recovered_again() {
    let conn = crate::get_conn_memory().await.unwrap();
    let library_id = library(&conn, "terminal diagnostics").await;
    let scan_id = {
        let mut lock = conn.writer().lock_owned().await;
        let mut tx = crate::write_tx(&mut lock).await.unwrap();
        let id = ScanRun::begin(&mut tx, library_id, "full").await.unwrap();
        ScanRun::finish(&mut tx, id, "failed", Some("metadata provider timed out"))
            .await
            .unwrap();
        assert_eq!(ScanRun::recover_abandoned(&mut tx).await.unwrap(), (0, 0));
        tx.commit().await.unwrap();
        id
    };

    let mut tx = conn.read().begin().await.unwrap();
    let run = ScanRun::latest(&mut tx, library_id).await.unwrap().unwrap();
    assert_eq!(run.id, scan_id);
    assert_eq!(run.status, "failed");
    assert_eq!(run.stage, "failed");
    assert_eq!(run.error.as_deref(), Some("metadata provider timed out"));
}
