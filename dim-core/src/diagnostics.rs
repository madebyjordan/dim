use crate::runtime_paths::RuntimePaths;
use dim_database::DbConnection;
use sqlx::Row;
use std::path::{Path, PathBuf};

#[derive(Clone, Debug, Default, Eq, PartialEq)]
pub struct ReconciliationReport {
    pub missing_media_files: usize,
    pub missing_metadata_files: usize,
    pub samples: Vec<PathBuf>,
}

/// Compare durable references with the filesystem without changing either side. Missing or
/// ambiguous paths are reported for operators; user media and metadata are never deleted.
pub async fn reconcile(
    conn: &DbConnection,
    paths: &RuntimePaths,
) -> sqlx::Result<ReconciliationReport> {
    let mut report = ReconciliationReport::default();
    let media = sqlx::query("SELECT target_file FROM mediafile")
        .fetch_all(conn.read_ref())
        .await?;
    for row in media {
        let path: String = row.try_get(0)?;
        if !Path::new(&path).is_file() {
            report.missing_media_files += 1;
            if report.samples.len() < 10 {
                report.samples.push(PathBuf::from(path));
            }
        }
    }
    let assets = sqlx::query("SELECT local_path FROM assets")
        .fetch_all(conn.read_ref())
        .await?;
    for row in assets {
        let relative: String = row.try_get(0)?;
        let candidate = paths.metadata.join(relative);
        if !candidate.is_file() {
            report.missing_metadata_files += 1;
            if report.samples.len() < 10 {
                report.samples.push(candidate);
            }
        }
    }
    Ok(report)
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct DiskDiagnostic {
    pub path: PathBuf,
    pub available_bytes: u64,
}

#[cfg(unix)]
pub fn disk_diagnostics(paths: &RuntimePaths) -> std::io::Result<Vec<DiskDiagnostic>> {
    [
        &paths.database,
        &paths.metadata,
        &paths.cache,
        &paths.temporary,
    ]
    .iter()
    .map(|path| {
        let probe = if path.is_dir() {
            path.as_path()
        } else {
            path.parent().unwrap_or_else(|| Path::new("."))
        };
        let stats = nix::sys::statvfs::statvfs(probe)
            .map_err(|error| std::io::Error::new(std::io::ErrorKind::Other, error))?;
        Ok(DiskDiagnostic {
            path: (*path).clone(),
            available_bytes: u64::from(stats.blocks_available()) * stats.fragment_size(),
        })
    })
    .collect()
}

#[cfg(not(unix))]
pub fn disk_diagnostics(_paths: &RuntimePaths) -> std::io::Result<Vec<DiskDiagnostic>> {
    Ok(Vec::new())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::settings::GlobalSettings;

    #[tokio::test]
    async fn reconciliation_reports_but_does_not_delete_ambiguous_rows() {
        let pool = dim_database::get_conn_memory().await.unwrap();
        let directory = tempfile::tempdir().unwrap();
        let mut settings = GlobalSettings::default();
        settings.metadata_dir = directory
            .path()
            .join("metadata")
            .to_string_lossy()
            .into_owned();
        let paths = RuntimePaths::from_settings(directory.path().join("config.toml"), &settings);
        std::fs::create_dir_all(&paths.metadata).unwrap();
        let mut lock = pool.writer().lock_owned().await;
        sqlx::query("INSERT INTO library(id, name, media_type) VALUES (1, 'library', 'movie')")
            .execute(&mut *lock)
            .await
            .unwrap();
        sqlx::query("INSERT INTO mediafile(id, library_id, target_file, raw_name) VALUES (1, 1, '/missing/user/movie.mkv', 'movie')").execute(&mut *lock).await.unwrap();
        sqlx::query(
            "INSERT INTO assets(id, local_path, file_ext) VALUES (1, 'missing.jpg', 'jpg')",
        )
        .execute(&mut *lock)
        .await
        .unwrap();
        drop(lock);
        let report = reconcile(&pool, &paths).await.unwrap();
        assert_eq!(report.missing_media_files, 1);
        assert_eq!(report.missing_metadata_files, 1);
        assert_eq!(
            sqlx::query_scalar::<_, i64>("SELECT COUNT(*) FROM mediafile")
                .fetch_one(pool.read_ref())
                .await
                .unwrap(),
            1
        );
        assert_eq!(
            sqlx::query_scalar::<_, i64>("SELECT COUNT(*) FROM assets")
                .fetch_one(pool.read_ref())
                .await
                .unwrap(),
            1
        );
    }
}
