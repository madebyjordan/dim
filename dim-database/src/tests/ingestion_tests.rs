use crate::ingestion::ScanRun;
use crate::library::{InsertableLibrary, MediaType};

#[tokio::test]
async fn starting_after_restart_preserves_interrupted_failure() {
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
    assert!(old_status.1.unwrap().contains("restart"));
    assert_eq!(
        ScanRun::latest(&mut tx, library_id)
            .await
            .unwrap()
            .unwrap()
            .id,
        second
    );
}
