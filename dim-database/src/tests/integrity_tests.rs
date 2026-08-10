use crate::{get_conn_file, validate_integrity, SQLITE_BUSY_TIMEOUT};
use sqlx::Row;

#[tokio::test]
async fn file_connections_apply_sqlite_policy() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("policy.db");
    let pool = get_conn_file(path.to_str().unwrap()).await.unwrap();

    let mut writer = pool.writer().lock_owned().await;
    assert_eq!(
        sqlx::query_scalar::<_, i64>("PRAGMA foreign_keys")
            .fetch_one(&mut *writer)
            .await
            .unwrap(),
        1
    );
    assert_eq!(
        sqlx::query_scalar::<_, String>("PRAGMA journal_mode")
            .fetch_one(&mut *writer)
            .await
            .unwrap(),
        "wal"
    );
    assert_eq!(
        sqlx::query_scalar::<_, i64>("PRAGMA synchronous")
            .fetch_one(&mut *writer)
            .await
            .unwrap(),
        1
    );
    assert_eq!(
        sqlx::query_scalar::<_, i64>("PRAGMA busy_timeout")
            .fetch_one(&mut *writer)
            .await
            .unwrap(),
        SQLITE_BUSY_TIMEOUT.as_millis() as i64
    );
    drop(writer);

    let readers = pool.read();
    let mut reader = readers.acquire().await.unwrap();
    assert_eq!(
        sqlx::query_scalar::<_, i64>("PRAGMA foreign_keys")
            .fetch_one(&mut reader)
            .await
            .unwrap(),
        1
    );
    assert_eq!(
        sqlx::query_scalar::<_, String>("PRAGMA journal_mode")
            .fetch_one(&mut reader)
            .await
            .unwrap(),
        "wal"
    );
    assert_eq!(
        sqlx::query_scalar::<_, i64>("PRAGMA busy_timeout")
            .fetch_one(&mut reader)
            .await
            .unwrap(),
        SQLITE_BUSY_TIMEOUT.as_millis() as i64
    );
}

#[tokio::test]
async fn integrity_validation_reports_and_preserves_orphans() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("orphan.db");
    let pool = get_conn_file(path.to_str().unwrap()).await.unwrap();

    let mut writer = pool.writer().lock_owned().await;
    sqlx::query("PRAGMA foreign_keys = OFF")
        .execute(&mut *writer)
        .await
        .unwrap();
    sqlx::query("INSERT INTO indexed_paths(location, library_id) VALUES ('/preserve/me', 4242)")
        .execute(&mut *writer)
        .await
        .unwrap();
    sqlx::query("PRAGMA foreign_keys = ON")
        .execute(&mut *writer)
        .await
        .unwrap();
    drop(writer);

    let error = validate_integrity(&pool).await.unwrap_err().to_string();
    assert!(error.contains("foreign-key violation"), "{error}");

    let mut writer = pool.writer().lock_owned().await;
    let retained: i64 =
        sqlx::query_scalar("SELECT COUNT(*) FROM indexed_paths WHERE location = '/preserve/me'")
            .fetch_one(&mut *writer)
            .await
            .unwrap();
    assert_eq!(retained, 1, "validation must not silently delete user data");
}

#[tokio::test]
async fn migration_indexes_serve_representative_query_paths() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("indexes.db");
    let pool = get_conn_file(path.to_str().unwrap()).await.unwrap();
    let mut writer = pool.writer().lock_owned().await;

    let cases = [
        ("EXPLAIN QUERY PLAN SELECT * FROM mediafile WHERE library_id = 1", "idx_mediafile_library_media"),
        ("EXPLAIN QUERY PLAN SELECT * FROM mediafile WHERE media_id = 1 ORDER BY duration DESC", "idx_mediafile_media_duration"),
        ("EXPLAIN QUERY PLAN SELECT location FROM indexed_paths WHERE library_id = 1", "idx_indexed_paths_library"),
        ("EXPLAIN QUERY PLAN SELECT id FROM _tblmedia WHERE library_id = 1 AND media_type = 'movie' AND name = 'x'", "idx_media_library_type_name"),
        ("EXPLAIN QUERY PLAN SELECT * FROM progress WHERE media_id = 1", "idx_progress_media"),
    ];

    for (query, expected_index) in cases {
        let plan = sqlx::query(query).fetch_all(&mut *writer).await.unwrap();
        let details = plan
            .iter()
            .filter_map(|row| row.try_get::<String, _>(3).ok())
            .collect::<Vec<_>>()
            .join(" | ");
        assert!(
            details.contains(expected_index),
            "{query}: expected {expected_index}, got {details}"
        );
    }
}
