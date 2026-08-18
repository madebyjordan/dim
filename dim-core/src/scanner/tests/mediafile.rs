use super::super::mediafile::Error as CreatorError;
use super::super::mediafile::InsertBatch;
use super::super::mediafile::MediafileCreator;
use super::super::parse_filenames;

use dim_database::library::InsertableLibrary;
use dim_database::library::MediaType;
use dim_database::mediafile::InsertableMediaFile;
use dim_database::mediafile::MediaFile;

use futures::stream::FuturesUnordered;
use futures::StreamExt;
use itertools::Itertools;

use std::future::Future;

use core::pin::Pin;

use futures::FutureExt;

use xtra::spawn::Tokio;
use xtra::Actor;

pub(crate) async fn create_library(conn: &mut dim_database::DbConnection) -> i64 {
    let mut lock = conn.writer().lock_owned().await;
    let mut tx = dim_database::write_tx(&mut lock).await.unwrap();

    let id = InsertableLibrary {
        name: "Tests".to_string(),
        locations: vec![],
        media_type: MediaType::Movie,
    }
    .insert(&mut tx)
    .await
    .expect("Failed to create test library.");

    tx.commit().await.expect("Failed to commit test library.");

    id
}

#[tokio::test(flavor = "multi_thread")]
async fn test_construct_mediafile() {
    let files = (0..512)
        .map(|i| format!("Movie{i}.mkv"))
        .collect::<Vec<String>>();
    let (tempdir, files) = super::temp_dir_symlink(files.into_iter(), super::TEST_MP4_PATH);

    let mut conn = dim_database::get_conn_memory()
        .await
        .expect("Failed to obtain a in-memory db pool.");
    let library = create_library(&mut conn).await;

    let mut instance = MediafileCreator::new(conn.clone(), library).await;

    let parsed = parse_filenames(files.iter());

    assert_eq!(parsed.len(), files.len());

    let insertable_futures =
        parsed
            .into_iter()
            .map(|(path, meta)| instance.construct_mediafile(path, meta[0].clone()).boxed())
            .chunks(5)
            .into_iter()
            .map(|chunk| chunk.collect())
            .collect::<Vec<
                Vec<
                    Pin<Box<dyn Future<Output = Result<InsertableMediaFile, CreatorError>> + Send>>,
                >,
            >>();

    let mut insertables = vec![];

    for chunk in insertable_futures.into_iter() {
        let results: Vec<Result<InsertableMediaFile, CreatorError>> =
            futures::future::join_all(chunk).await;

        for result in results {
            insertables.push(result.expect("Failed to create insertable."));
        }
    }

    let mut mediafiles = vec![];

    for chunk in insertables.chunks(128) {
        mediafiles.append(
            &mut instance
                .insert_batch(chunk.iter())
                .await
                .expect("Failed to insert batch."),
        );
    }

    // We should have inserted all the files as they dont exist in the database.
    assert_eq!(mediafiles.len(), files.len());

    // All the files in `insertables` should already exist in the database, thus this should return
    // `0`.
    for chunk in insertables.chunks(128) {
        let files = instance
            .insert_batch(chunk.iter())
            .await
            .expect("Failed to insert batch.");

        assert_eq!(files.len(), 0);
    }

    // At this point we should have 512 files in the database.
    let mut tx = conn.read().begin().await.unwrap();
    let files_in_db = MediaFile::get_by_lib_null_media(&mut tx, library)
        .await
        .expect("Failed to get mediafiles.");
    assert_eq!(files_in_db.len(), files.len());

    let rescan_work =
        super::super::insert_mediafiles(&mut conn, library, vec![tempdir.path().to_path_buf()])
            .await
            .expect("Rescanning existing files should not fail.");
    assert_eq!(rescan_work.len(), files.len());
    assert!(rescan_work.iter().all(|unit| unit.0.media_id.is_none()));
}

#[tokio::test(flavor = "multi_thread")]
async fn rescan_keeps_metadata_aligned_after_existing_files_are_filtered() {
    let names = ["Already Here (1999).mp4", "New Arrival (2024).mp4"];
    let (tempdir, files) = super::temp_dir_symlink(names.into_iter(), super::TEST_MP4_PATH);
    let mut conn = dim_database::get_conn_memory()
        .await
        .expect("Failed to obtain an in-memory db pool.");
    let library = create_library(&mut conn).await;

    let existing = InsertableMediaFile {
        library_id: library,
        target_file: files[0].to_string_lossy().into_owned(),
        raw_name: "Already Here".into(),
        raw_year: Some(1999),
        ..Default::default()
    };
    let mut lock = conn.writer().lock_owned().await;
    let mut tx = dim_database::write_tx(&mut lock).await.unwrap();
    existing.insert(&mut tx).await.unwrap();
    tx.commit().await.unwrap();
    drop(lock);

    let work =
        super::super::insert_mediafiles(&mut conn, library, vec![tempdir.path().to_path_buf()])
            .await
            .unwrap();

    assert_eq!(work.len(), 2);
    assert_eq!(work[0].0.target_file, files[0].to_string_lossy());
    assert_eq!(work[0].1[0].name, "Already Here");
    assert_eq!(work[0].1[0].year, Some(1999));
    assert_eq!(work[1].0.target_file, files[1].to_string_lossy());
    assert_eq!(work[1].1[0].name, "New Arrival");
    assert_eq!(work[1].1[0].year, Some(2024));
}

#[tokio::test(flavor = "multi_thread")]
async fn durable_rescan_of_existing_file_releases_writer_for_item_update() {
    let (tempdir, files) = super::temp_dir_symlink(
        ["Already Here (1999).mp4"].into_iter(),
        super::TEST_MP4_PATH,
    );
    let mut conn = dim_database::get_conn_memory()
        .await
        .expect("Failed to obtain an in-memory db pool.");
    let library = create_library(&mut conn).await;

    let existing = InsertableMediaFile {
        library_id: library,
        target_file: files[0].to_string_lossy().into_owned(),
        raw_name: "Already Here".into(),
        raw_year: Some(1999),
        ..Default::default()
    };
    let scan_id = {
        let mut lock = conn.writer().lock_owned().await;
        let mut tx = dim_database::write_tx(&mut lock).await.unwrap();
        existing.insert(&mut tx).await.unwrap();
        let scan_id = dim_database::ingestion::ScanRun::begin(&mut tx, library, "full")
            .await
            .unwrap();
        tx.commit().await.unwrap();
        scan_id
    };

    let work = tokio::time::timeout(
        std::time::Duration::from_secs(2),
        super::super::insert_mediafiles_for_scan(
            &mut conn,
            library,
            vec![tempdir.path().to_path_buf()],
            Some(scan_id),
            None,
            super::super::ScanScope::Full,
            false,
        ),
    )
    .await
    .expect("existing-file rescan deadlocked on the SQLite writer")
    .unwrap();
    assert_eq!(work.len(), 1);
    assert_eq!(work[0].0.target_file, files[0].to_string_lossy());

    let item = sqlx::query_as::<_, (String, String, Option<String>)>(
        "SELECT stage, status, error_class FROM ingestion_item WHERE scan_id = ?",
    )
    .bind(scan_id)
    .fetch_one(conn.read_ref())
    .await
    .unwrap();
    assert_eq!(item, ("commit".into(), "complete".into(), None));
}

#[tokio::test(flavor = "multi_thread")]
async fn durable_scan_counts_probe_failures() {
    let directory = super::temp_dir(["Unreadable By Ffprobe (2026).mkv"]);
    let path = directory.path().join("Unreadable By Ffprobe (2026).mkv");
    std::fs::write(&path, b"not a media container").unwrap();
    let mut conn = dim_database::get_conn_memory()
        .await
        .expect("Failed to obtain an in-memory db pool.");
    let library = create_library(&mut conn).await;
    let scan_id = {
        let mut lock = conn.writer().lock_owned().await;
        let mut tx = dim_database::write_tx(&mut lock).await.unwrap();
        let scan_id = dim_database::ingestion::ScanRun::begin(&mut tx, library, "full")
            .await
            .unwrap();
        tx.commit().await.unwrap();
        scan_id
    };

    let work = super::super::insert_mediafiles_for_scan(
        &mut conn,
        library,
        vec![directory.path().to_path_buf()],
        Some(scan_id),
        None,
        super::super::ScanScope::Full,
        false,
    )
    .await
    .unwrap();
    assert!(work.is_empty());

    let aggregate: i64 = sqlx::query_scalar("SELECT failed FROM ingestion_scan WHERE id = ?")
        .bind(scan_id)
        .fetch_one(conn.read_ref())
        .await
        .unwrap();
    assert_eq!(aggregate, 1);

    let item: (String, String, Option<String>) = sqlx::query_as(
        "SELECT stage, status, error_class FROM ingestion_item WHERE scan_id = ? AND path = ?",
    )
    .bind(scan_id)
    .bind(path.to_string_lossy().as_ref())
    .fetch_one(conn.read_ref())
    .await
    .unwrap();
    assert_eq!(item.0, "probing");
    assert_eq!(item.1, "failed");
    assert_eq!(item.2.as_deref(), Some("corrupt_media"));
}

#[tokio::test(flavor = "multi_thread")]
async fn test_multiple_instances() {
    let files = (0..1024)
        .map(|i| format!("Movie{i}.mkv"))
        .collect::<Vec<String>>();
    let (_tempdir, files) = super::temp_dir_symlink(files.into_iter(), super::TEST_MP4_PATH);

    let mut conn = dim_database::get_conn_memory()
        .await
        .expect("Failed to obtain a in-memory db pool.");
    let library = create_library(&mut conn).await;

    let instance = MediafileCreator::new(conn.clone(), library).await;

    let parsed = parse_filenames(files.iter());

    assert_eq!(parsed.len(), files.len());

    let mut insertables = vec![];

    for mut chunk in parsed
        .into_iter()
        .map(|(path, meta)| instance.construct_mediafile(path, meta[0].clone()))
        .chunks(16)
        .into_iter()
        .map(|ch| ch.collect::<FuturesUnordered<_>>())
    {
        while let Some(res) = chunk.next().await {
            insertables.push(res.expect("Failed to create insertable."));
        }
    }

    let mut instances = vec![];

    for _ in 0..8 {
        let addr = MediafileCreator::new(conn.clone(), library)
            .await
            .create(None)
            .spawn(&mut Tokio::Global);
        instances.push(addr);
    }

    let mut insert_futures = vec![];

    for (chunk, addr) in insertables.chunks(128).zip(instances.iter().cycle()) {
        let addr = addr.clone();
        insert_futures.push(async move {
            let chunk_len = chunk.len();
            let result = addr
                .send(InsertBatch(chunk.into_iter().cloned().collect()))
                .await
                .expect("Addr got dropped")
                .expect("Failed to insert batch");

            assert_eq!(result.len(), chunk_len);

            result
        });
    }

    let mediafiles = futures::future::join_all(insert_futures)
        .await
        .into_iter()
        .flatten()
        .collect::<Vec<_>>();

    assert_eq!(mediafiles.len(), files.len());
}
