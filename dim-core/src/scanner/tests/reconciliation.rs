use async_trait::async_trait;
use dim_database::library::{InsertableLibrary, MediaType};
use dim_extern_api::{
    ExternalActor, ExternalMedia, ExternalQuery, ExternalQueryIntoShow, IntoQueryShow,
};
use std::sync::Arc;

#[derive(Debug)]
struct UnusedProvider;

impl IntoQueryShow for UnusedProvider {}
impl ExternalQueryIntoShow for UnusedProvider {}

#[async_trait]
impl ExternalQuery for UnusedProvider {
    async fn search(
        &self,
        _title: &str,
        _year: Option<i32>,
    ) -> dim_extern_api::Result<Vec<ExternalMedia>> {
        panic!("an empty scan must not query metadata")
    }

    async fn search_by_id(&self, _external_id: &str) -> dim_extern_api::Result<ExternalMedia> {
        panic!("an empty scan must not query metadata")
    }

    async fn cast(&self, _external_id: &str) -> dim_extern_api::Result<Vec<ExternalActor>> {
        panic!("an empty scan must not query metadata")
    }
}

#[derive(Debug)]
struct BlockingProvider {
    entered: Arc<tokio::sync::Notify>,
}

impl IntoQueryShow for BlockingProvider {}
impl ExternalQueryIntoShow for BlockingProvider {}

#[async_trait]
impl ExternalQuery for BlockingProvider {
    async fn search(
        &self,
        _title: &str,
        _year: Option<i32>,
    ) -> dim_extern_api::Result<Vec<ExternalMedia>> {
        self.entered.notify_one();
        std::future::pending().await
    }

    async fn search_by_id(&self, _external_id: &str) -> dim_extern_api::Result<ExternalMedia> {
        std::future::pending().await
    }

    async fn cast(&self, _external_id: &str) -> dim_extern_api::Result<Vec<ExternalActor>> {
        std::future::pending().await
    }
}

async fn library_with_location(conn: &dim_database::DbConnection, location: &str) -> i64 {
    let mut lock = conn.writer().lock_owned().await;
    let mut tx = dim_database::write_tx(&mut lock).await.unwrap();
    let id = InsertableLibrary {
        name: format!("Library at {location}"),
        locations: vec![location.into()],
        media_type: MediaType::Movie,
    }
    .insert(&mut tx)
    .await
    .unwrap();
    tx.commit().await.unwrap();
    id
}

async fn insert_catalogued_movie(conn: &dim_database::DbConnection, library_id: i64, path: &str) {
    let mut lock = conn.writer().lock_owned().await;
    let mut tx = dim_database::write_tx(&mut lock).await.unwrap();
    let media_id = sqlx::query(
        "INSERT INTO _tblmedia (library_id, name, media_type) VALUES (?, 'Friday', 'movie')",
    )
    .bind(library_id)
    .execute(&mut tx)
    .await
    .unwrap()
    .last_insert_rowid();
    sqlx::query("INSERT INTO mediafile (media_id, library_id, target_file, raw_name) VALUES (?, ?, ?, 'Friday')")
        .bind(media_id)
        .bind(library_id)
        .bind(path)
        .execute(&mut tx)
        .await
        .unwrap();
    tx.commit().await.unwrap();
}

async fn counts(conn: &dim_database::DbConnection, library_id: i64) -> (i64, i64) {
    let mediafiles = sqlx::query_scalar("SELECT COUNT(*) FROM mediafile WHERE library_id = ?")
        .bind(library_id)
        .fetch_one(conn.read_ref())
        .await
        .unwrap();
    let media = sqlx::query_scalar("SELECT COUNT(*) FROM _tblmedia WHERE library_id = ?")
        .bind(library_id)
        .fetch_one(conn.read_ref())
        .await
        .unwrap();
    (mediafiles, media)
}

async fn latest_status(conn: &dim_database::DbConnection, library_id: i64) -> String {
    sqlx::query_scalar(
        "SELECT status FROM ingestion_scan WHERE library_id = ? ORDER BY id DESC LIMIT 1",
    )
    .bind(library_id)
    .fetch_one(conn.read_ref())
    .await
    .unwrap()
}

#[tokio::test(flavor = "multi_thread")]
async fn full_scan_of_deleted_library_root_reconciles_catalogue_to_empty() {
    let deleted_root = tempfile::tempdir().unwrap();
    let root_path = deleted_root.path().to_path_buf();
    let stale_path = root_path.join("Friday (1995).mkv");

    let mut conn = dim_database::get_conn_memory().await.unwrap();
    let library_id = library_with_location(&conn, root_path.to_str().unwrap()).await;
    insert_catalogued_movie(&conn, library_id, stale_path.to_str().unwrap()).await;
    drop(deleted_root);

    let (event_tx, mut events) = tokio::sync::mpsc::channel(8);
    super::super::start(&mut conn, library_id, event_tx, Arc::new(UnusedProvider))
        .await
        .unwrap();

    assert_eq!(counts(&conn, library_id).await, (0, 0));
    assert_eq!(latest_status(&conn, library_id).await, "complete");

    let first = events.recv().await.unwrap();
    let terminal = events.recv().await.unwrap();
    assert!(first.contains("EventStartedScanning"));
    assert!(terminal.contains("EventStoppedScanning"));
}

#[tokio::test(flavor = "multi_thread")]
async fn full_scan_of_empty_library_root_removes_stale_catalogue() {
    let root = tempfile::tempdir().unwrap();
    let stale_path = root.path().join("Friday (1995).mkv");
    let mut conn = dim_database::get_conn_memory().await.unwrap();
    let library_id = library_with_location(&conn, root.path().to_str().unwrap()).await;
    insert_catalogued_movie(&conn, library_id, stale_path.to_str().unwrap()).await;

    let (events, _receiver) = tokio::sync::mpsc::channel(8);
    super::super::start(&mut conn, library_id, events, Arc::new(UnusedProvider))
        .await
        .unwrap();

    assert_eq!(counts(&conn, library_id).await, (0, 0));
    assert_eq!(latest_status(&conn, library_id).await, "complete");
}

#[tokio::test(flavor = "multi_thread")]
async fn event_backpressure_cannot_leave_a_scan_running() {
    let root = tempfile::tempdir().unwrap();
    let mut conn = dim_database::get_conn_memory().await.unwrap();
    let library_id = library_with_location(&conn, root.path().to_str().unwrap()).await;
    let (events, _receiver) = tokio::sync::mpsc::channel(1);
    events.try_send("occupied event queue".into()).unwrap();

    tokio::time::timeout(
        std::time::Duration::from_secs(2),
        super::super::start(&mut conn, library_id, events, Arc::new(UnusedProvider)),
    )
    .await
    .expect("scan lifecycle blocked on a full event queue")
    .unwrap();

    assert_eq!(latest_status(&conn, library_id).await, "complete");
}

#[tokio::test(flavor = "multi_thread")]
async fn failed_full_traversal_preserves_existing_catalogue_and_reports_failure() {
    let malformed_root = tempfile::NamedTempFile::new().unwrap();
    let stale_path = format!("{}/Friday (1995).mkv", malformed_root.path().display());
    let mut conn = dim_database::get_conn_memory().await.unwrap();
    let library_id = library_with_location(&conn, malformed_root.path().to_str().unwrap()).await;
    insert_catalogued_movie(&conn, library_id, &stale_path).await;
    let (events, mut receiver) = tokio::sync::mpsc::channel(8);

    let error = super::super::start(&mut conn, library_id, events, Arc::new(UnusedProvider))
        .await
        .unwrap_err();

    assert!(error.to_string().contains("not a directory"));
    assert_eq!(counts(&conn, library_id).await, (1, 1));
    assert_eq!(latest_status(&conn, library_id).await, "failed");
    assert!(receiver
        .recv()
        .await
        .unwrap()
        .contains("EventStartedScanning"));
    assert!(receiver.recv().await.unwrap().contains("EventScanFailed"));
}

#[tokio::test(flavor = "multi_thread")]
async fn successful_full_scan_preserves_present_media() {
    let (root, files) =
        super::temp_dir_symlink(["Friday (1995).mp4"].into_iter(), super::TEST_MP4_PATH);
    let mut conn = dim_database::get_conn_memory().await.unwrap();
    let library_id = library_with_location(&conn, root.path().to_str().unwrap()).await;
    insert_catalogued_movie(&conn, library_id, files[0].to_str().unwrap()).await;
    {
        let mut lock = conn.writer().lock_owned().await;
        let mut tx = dim_database::write_tx(&mut lock).await.unwrap();
        sqlx::query("UPDATE mediafile SET missing_since = CURRENT_TIMESTAMP WHERE library_id = ?")
            .bind(library_id)
            .execute(&mut tx)
            .await
            .unwrap();
        tx.commit().await.unwrap();
    }
    let (events, _receiver) = tokio::sync::mpsc::channel(8);

    super::super::start(&mut conn, library_id, events, Arc::new(UnusedProvider))
        .await
        .unwrap();

    assert_eq!(counts(&conn, library_id).await, (1, 1));
    let missing: Option<String> =
        sqlx::query_scalar("SELECT missing_since FROM mediafile WHERE library_id = ?")
            .bind(library_id)
            .fetch_one(conn.read_ref())
            .await
            .unwrap();
    assert!(missing.is_none());
}

#[tokio::test(flavor = "multi_thread")]
async fn cancelled_scan_becomes_terminal_without_destructive_reconciliation() {
    let (root, _files) =
        super::temp_dir_symlink(["New Movie (2026).mp4"].into_iter(), super::TEST_MP4_PATH);
    let stale_path = root.path().join("Existing Movie (2025).mp4");
    let conn = dim_database::get_conn_memory().await.unwrap();
    let library_id = library_with_location(&conn, root.path().to_str().unwrap()).await;
    insert_catalogued_movie(&conn, library_id, stale_path.to_str().unwrap()).await;
    let entered = Arc::new(tokio::sync::Notify::new());
    let provider = Arc::new(BlockingProvider {
        entered: entered.clone(),
    });
    let (events, mut receiver) = tokio::sync::mpsc::channel(8);
    let mut scan_conn = conn.clone();

    let scan = tokio::spawn(async move {
        super::super::start(&mut scan_conn, library_id, events, provider).await
    });
    assert!(receiver
        .recv()
        .await
        .unwrap()
        .contains("EventStartedScanning"));
    tokio::time::timeout(std::time::Duration::from_secs(5), entered.notified())
        .await
        .expect("scan never reached its deliberately blocked matching stage");
    scan.abort();
    let _ = scan.await;

    let terminal = tokio::time::timeout(std::time::Duration::from_secs(5), async {
        loop {
            if latest_status(&conn, library_id).await == "cancelled" {
                break;
            }
            tokio::task::yield_now().await;
        }
    })
    .await;
    assert!(
        terminal.is_ok(),
        "cancelled scan remained marked as running"
    );
    assert!(receiver
        .recv()
        .await
        .unwrap()
        .contains("EventScanCancelled"));

    let stale_still_present: i64 = sqlx::query_scalar(
        "SELECT COUNT(*) FROM mediafile WHERE library_id = ? AND target_file = ?",
    )
    .bind(library_id)
    .bind(stale_path.to_string_lossy().as_ref())
    .fetch_one(conn.read_ref())
    .await
    .unwrap();
    assert_eq!(stale_still_present, 1);
}
