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
    let media_id =
        sqlx::query("INSERT INTO _tblmedia (library_id, name, media_type) VALUES (?, ?, 'movie')")
            .bind(library_id)
            .bind(path)
            .execute(&mut tx)
            .await
            .unwrap()
            .last_insert_rowid();
    sqlx::query(
        "INSERT INTO mediafile (media_id, library_id, target_file, raw_name) VALUES (?, ?, ?, ?)",
    )
    .bind(media_id)
    .bind(library_id)
    .bind(path)
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

async fn path_count(conn: &dim_database::DbConnection, library_id: i64, path: &str) -> i64 {
    sqlx::query_scalar("SELECT COUNT(*) FROM mediafile WHERE library_id = ? AND target_file = ?")
        .bind(library_id)
        .bind(path)
        .fetch_one(conn.read_ref())
        .await
        .unwrap()
}

async fn latest_root_statuses(
    conn: &dim_database::DbConnection,
    library_id: i64,
) -> Vec<(String, String, Option<String>)> {
    sqlx::query_as(
        "SELECT root, status, error FROM ingestion_scan_root
         WHERE scan_id = (SELECT id FROM ingestion_scan WHERE library_id = ? ORDER BY id DESC LIMIT 1)
         ORDER BY ordinal",
    )
    .bind(library_id)
    .fetch_all(conn.read_ref())
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
    let root = tempfile::Builder::new()
        .prefix("dim-scan")
        .tempdir()
        .unwrap();
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
async fn two_successful_roots_reconcile_independently() {
    let first = tempfile::Builder::new()
        .prefix("dim-first-root")
        .tempdir()
        .unwrap();
    let second = tempfile::Builder::new()
        .prefix("dim-second-root")
        .tempdir()
        .unwrap();
    let first_stale = first.path().join("First (2024).mkv");
    let second_stale = second.path().join("Second (2025).mkv");
    std::fs::write(first.path().join("first.txt"), b"sidecar").unwrap();
    std::fs::write(second.path().join("second.txt"), b"sidecar").unwrap();
    let mut conn = dim_database::get_conn_memory().await.unwrap();
    let library_id = library_with_location(&conn, first.path().to_str().unwrap()).await;
    insert_catalogued_movie(&conn, library_id, first_stale.to_str().unwrap()).await;
    insert_catalogued_movie(&conn, library_id, second_stale.to_str().unwrap()).await;
    let (events, _receiver) = tokio::sync::mpsc::channel(8);

    super::super::start_custom(
        &mut conn,
        library_id,
        vec![first.path().to_path_buf(), second.path().to_path_buf()],
        events,
        MediaType::Movie,
        Arc::new(UnusedProvider),
    )
    .await
    .unwrap();

    assert_eq!(counts(&conn, library_id).await, (0, 0));
    assert_eq!(latest_status(&conn, library_id).await, "complete");
    assert!(latest_root_statuses(&conn, library_id)
        .await
        .iter()
        .all(|(_, status, error)| status == "authoritative" && error.is_none()));
    let attributed: i64 = sqlx::query_scalar(
        "SELECT COUNT(*) FROM ingestion_item WHERE scan_id = (SELECT id FROM ingestion_scan WHERE library_id = ? ORDER BY id DESC LIMIT 1) AND root_id IS NOT NULL",
    )
    .bind(library_id)
    .fetch_one(conn.read_ref())
    .await
    .unwrap();
    assert_eq!(attributed, 2);
}

#[tokio::test(flavor = "multi_thread")]
async fn incremental_scan_never_performs_authoritative_reconciliation() {
    let root = tempfile::tempdir().unwrap();
    let stale = root.path().join("Watcher Preserves (2025).mkv");
    let mut conn = dim_database::get_conn_memory().await.unwrap();
    let library_id = library_with_location(&conn, root.path().to_str().unwrap()).await;
    insert_catalogued_movie(&conn, library_id, stale.to_str().unwrap()).await;
    let (events, _receiver) = tokio::sync::mpsc::channel(8);

    super::super::start_incremental_custom(
        &mut conn,
        library_id,
        vec![root.path().to_path_buf()],
        events,
        MediaType::Movie,
        Arc::new(UnusedProvider),
    )
    .await
    .unwrap();

    assert_eq!(
        path_count(&conn, library_id, stale.to_str().unwrap()).await,
        1
    );
    let kind: String = sqlx::query_scalar(
        "SELECT kind FROM ingestion_scan WHERE library_id = ? ORDER BY id DESC LIMIT 1",
    )
    .bind(library_id)
    .fetch_one(conn.read_ref())
    .await
    .unwrap();
    assert_eq!(kind, "watcher");
}

#[tokio::test(flavor = "multi_thread")]
async fn missing_root_and_healthy_root_both_reconcile() {
    let missing = tempfile::tempdir().unwrap();
    let missing_path = missing.path().to_path_buf();
    let healthy = tempfile::tempdir().unwrap();
    let missing_stale = missing_path.join("Missing (2024).mkv");
    let healthy_stale = healthy.path().join("Healthy (2025).mkv");
    let mut conn = dim_database::get_conn_memory().await.unwrap();
    let library_id = library_with_location(&conn, healthy.path().to_str().unwrap()).await;
    insert_catalogued_movie(&conn, library_id, missing_stale.to_str().unwrap()).await;
    insert_catalogued_movie(&conn, library_id, healthy_stale.to_str().unwrap()).await;
    drop(missing);
    let (events, _receiver) = tokio::sync::mpsc::channel(8);

    super::super::start_custom(
        &mut conn,
        library_id,
        vec![missing_path, healthy.path().to_path_buf()],
        events,
        MediaType::Movie,
        Arc::new(UnusedProvider),
    )
    .await
    .unwrap();

    assert_eq!(counts(&conn, library_id).await, (0, 0));
    let statuses = latest_root_statuses(&conn, library_id).await;
    assert_eq!(statuses[0].1, "missing");
    assert_eq!(statuses[1].1, "authoritative");
}

#[tokio::test(flavor = "multi_thread")]
async fn overlapping_roots_and_legacy_paths_are_never_deleted() {
    let root = tempfile::Builder::new()
        .prefix("dim-partial-scan")
        .tempdir()
        .unwrap();
    let nested = root.path().join("nested");
    std::fs::create_dir(&nested).unwrap();
    let ambiguous = nested.join("Ambiguous (2025).mkv");
    let legacy = tempfile::tempdir()
        .unwrap()
        .path()
        .join("Legacy (2020).mkv");
    let mut conn = dim_database::get_conn_memory().await.unwrap();
    let library_id = library_with_location(&conn, root.path().to_str().unwrap()).await;
    insert_catalogued_movie(&conn, library_id, ambiguous.to_str().unwrap()).await;
    insert_catalogued_movie(&conn, library_id, legacy.to_str().unwrap()).await;
    let (events, _receiver) = tokio::sync::mpsc::channel(8);

    super::super::start_custom(
        &mut conn,
        library_id,
        vec![root.path().to_path_buf(), nested],
        events,
        MediaType::Movie,
        Arc::new(UnusedProvider),
    )
    .await
    .unwrap();

    assert_eq!(
        path_count(&conn, library_id, ambiguous.to_str().unwrap()).await,
        1
    );
    assert_eq!(
        path_count(&conn, library_id, legacy.to_str().unwrap()).await,
        1
    );
}

#[cfg(unix)]
#[tokio::test(flavor = "multi_thread")]
async fn normalized_and_symlink_boundary_ownership_is_ambiguous() {
    use std::os::unix::fs::symlink;

    let root = tempfile::tempdir().unwrap();
    let external = tempfile::tempdir().unwrap();
    let link = root.path().join("external");
    symlink(external.path(), &link).unwrap();
    let through_symlink = link.join("Symlinked (2025).mkv");
    let normalized_edge = root.path().join("Normalized (2024).mkv");
    let lexical_root = root.path().join("marker").join("..");
    std::fs::create_dir(root.path().join("marker")).unwrap();
    let mut conn = dim_database::get_conn_memory().await.unwrap();
    let library_id = library_with_location(&conn, root.path().to_str().unwrap()).await;
    insert_catalogued_movie(&conn, library_id, through_symlink.to_str().unwrap()).await;
    insert_catalogued_movie(&conn, library_id, normalized_edge.to_str().unwrap()).await;
    let (events, _receiver) = tokio::sync::mpsc::channel(8);

    super::super::start_custom(
        &mut conn,
        library_id,
        vec![lexical_root],
        events,
        MediaType::Movie,
        Arc::new(UnusedProvider),
    )
    .await
    .unwrap();

    assert_eq!(
        path_count(&conn, library_id, through_symlink.to_str().unwrap()).await,
        1
    );
    assert_eq!(
        path_count(&conn, library_id, normalized_edge.to_str().unwrap()).await,
        1
    );
}

#[tokio::test(flavor = "multi_thread")]
async fn large_successful_scan_streams_and_accounts_for_every_discovery() {
    const FILES: usize = 512;
    let root = tempfile::Builder::new()
        .prefix("dim-large-scan")
        .tempdir()
        .unwrap();
    for index in 0..FILES {
        std::fs::write(
            root.path().join(format!("sidecar-{index}.txt")),
            b"metadata",
        )
        .unwrap();
    }
    let stale_path = root.path().join("Removed Movie (2024).mp4");
    let mut conn = dim_database::get_conn_memory().await.unwrap();
    let library_id = library_with_location(&conn, root.path().to_str().unwrap()).await;
    insert_catalogued_movie(&conn, library_id, stale_path.to_str().unwrap()).await;
    let (events, _receiver) = tokio::sync::mpsc::channel(8);

    super::super::start(&mut conn, library_id, events, Arc::new(UnusedProvider))
        .await
        .unwrap();

    assert_eq!(counts(&conn, library_id).await, (0, 0));
    let scan = sqlx::query_as::<_, (String, String, i64, i64, i64)>(
        "SELECT kind, status, discovered, skipped,
                (SELECT COUNT(*) FROM ingestion_item WHERE scan_id = ingestion_scan.id)
         FROM ingestion_scan WHERE library_id = ? ORDER BY id DESC LIMIT 1",
    )
    .bind(library_id)
    .fetch_one(conn.read_ref())
    .await
    .unwrap();
    assert_eq!(
        scan,
        (
            "full".into(),
            "complete".into(),
            FILES as i64,
            FILES as i64,
            FILES as i64,
        )
    );
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

#[cfg(unix)]
#[tokio::test(flavor = "multi_thread")]
async fn failed_root_preserves_itself_but_healthy_root_still_reconciles() {
    use std::os::unix::fs::symlink;

    let good_root = tempfile::Builder::new()
        .prefix("dim-good-root")
        .tempdir()
        .unwrap();
    std::fs::write(
        good_root.path().join("Streamed Movie (2026).mkv"),
        b"seen before traversal fails",
    )
    .unwrap();
    let bad_root = tempfile::Builder::new()
        .prefix("dim-bad-root")
        .tempdir()
        .unwrap();
    let loop_dir = bad_root.path().join("loop");
    std::fs::create_dir(&loop_dir).unwrap();
    symlink(&loop_dir, loop_dir.join("back")).unwrap();

    let stale_path = good_root.path().join("Existing Movie (2025).mp4");
    let failed_stale = bad_root.path().join("Preserved Movie (2024).mp4");
    let mut conn = dim_database::get_conn_memory().await.unwrap();
    let library_id = library_with_location(&conn, good_root.path().to_str().unwrap()).await;
    insert_catalogued_movie(&conn, library_id, stale_path.to_str().unwrap()).await;
    insert_catalogued_movie(&conn, library_id, failed_stale.to_str().unwrap()).await;
    let (events, _receiver) = tokio::sync::mpsc::channel(8);

    let error = super::super::start_custom(
        &mut conn,
        library_id,
        vec![
            good_root.path().to_path_buf(),
            bad_root.path().to_path_buf(),
        ],
        events,
        MediaType::Movie,
        Arc::new(UnusedProvider),
    )
    .await
    .unwrap_err();

    assert!(matches!(
        error,
        super::super::Error::FilesystemTraversal { .. }
    ));
    assert_eq!(
        counts(&conn, library_id).await,
        (1, 1),
        "the authoritative healthy root must reconcile despite another root failing"
    );
    assert_eq!(
        path_count(&conn, library_id, stale_path.to_str().unwrap()).await,
        0
    );
    assert_eq!(
        path_count(&conn, library_id, failed_stale.to_str().unwrap()).await,
        1
    );
    assert_eq!(latest_status(&conn, library_id).await, "failed");
    let discovered: i64 = sqlx::query_scalar(
        "SELECT discovered FROM ingestion_scan WHERE library_id = ? ORDER BY id DESC LIMIT 1",
    )
    .bind(library_id)
    .fetch_one(conn.read_ref())
    .await
    .unwrap();
    assert!(
        discovered > 0,
        "failure did not occur after streaming began"
    );
    let roots = latest_root_statuses(&conn, library_id).await;
    assert_eq!(roots[0].1, "authoritative");
    assert_eq!(roots[1].1, "failed");
    assert!(roots[1].2.as_deref().unwrap().contains("loop"));
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
async fn cancellation_during_partial_traversal_preserves_catalogue_and_cancels_root() {
    const FILES: usize = 1024;
    let root = tempfile::Builder::new()
        .prefix("dim-partial-scan")
        .tempdir()
        .unwrap();
    for index in 0..FILES {
        std::fs::write(
            root.path().join(format!("sidecar-{index}.txt")),
            b"metadata",
        )
        .unwrap();
    }
    let stale_path = root.path().join("Existing Movie (2025).mp4");
    let conn = dim_database::get_conn_memory().await.unwrap();
    let library_id = library_with_location(&conn, root.path().to_str().unwrap()).await;
    insert_catalogued_movie(&conn, library_id, stale_path.to_str().unwrap()).await;
    let (events, mut receiver) = tokio::sync::mpsc::channel(8);
    let mut scan_conn = conn.clone();

    let scan = tokio::spawn(async move {
        super::super::start(&mut scan_conn, library_id, events, Arc::new(UnusedProvider)).await
    });
    assert!(receiver
        .recv()
        .await
        .unwrap()
        .contains("EventStartedScanning"));
    tokio::time::timeout(std::time::Duration::from_secs(5), async {
        loop {
            let state = sqlx::query_as::<_, (String, i64)>(
                "SELECT status, discovered FROM ingestion_scan_root ORDER BY id DESC LIMIT 1",
            )
            .fetch_optional(conn.read_ref())
            .await
            .unwrap();
            if matches!(state, Some((ref status, discovered)) if status == "running" && discovered > 0 && discovered < FILES as i64)
            {
                break;
            }
            tokio::task::yield_now().await;
        }
    })
    .await
    .expect("scan did not expose a partially traversed running root");
    scan.abort();
    let _ = scan.await;

    tokio::time::timeout(std::time::Duration::from_secs(15), async {
        while latest_status(&conn, library_id).await != "cancelled" {
            tokio::task::yield_now().await;
        }
    })
    .await
    .expect("cancelled scan did not become terminal");
    assert_eq!(
        path_count(&conn, library_id, stale_path.to_str().unwrap()).await,
        1
    );
    assert_eq!(
        latest_root_statuses(&conn, library_id).await[0].1,
        "cancelled"
    );
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
    {
        let mut lock = conn.writer().lock_owned().await;
        sqlx::query("UPDATE ingestion_scan SET last_progress_at = '2000-01-01 00:00:00' WHERE library_id = ? AND status = 'running'")
            .bind(library_id)
            .execute(&mut *lock)
            .await
            .unwrap();
    }
    tokio::time::timeout(std::time::Duration::from_secs(3), async {
        loop {
            let progress: (String, String) = sqlx::query_as(
                "SELECT stage, last_progress_at FROM ingestion_scan WHERE library_id = ? AND status = 'running'",
            )
            .bind(library_id)
            .fetch_one(conn.read_ref())
            .await
            .unwrap();
            if progress.0 == "matching" && progress.1 != "2000-01-01 00:00:00" {
                break;
            }
            tokio::task::yield_now().await;
        }
    })
    .await
    .expect("healthy metadata wait did not advance its durable heartbeat");
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
