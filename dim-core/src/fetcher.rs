use crate::core::METADATA_PATH;

use once_cell::sync::{Lazy, OnceCell};
use reqwest::header::CONTENT_TYPE;
use std::collections::hash_map::DefaultHasher;
use std::hash::{Hash, Hasher};
use std::path::Path;
use std::time::Duration;
use tokio::sync::mpsc::{self, Receiver, Sender};
use tracing::{debug, error, instrument};

const PARTITIONS: usize = 5;
const QUEUE_CAPACITY: usize = 128;
const MAX_ARTWORK_BYTES: usize = 20 * 1024 * 1024;

static CLIENT: Lazy<reqwest::Client> = Lazy::new(|| {
    reqwest::Client::builder()
        .connect_timeout(Duration::from_secs(5))
        .timeout(Duration::from_secs(20))
        .build()
        .expect("valid shared artwork HTTP client")
});
static SENDER_PARTITIONS: OnceCell<[Sender<(String, String)>; PARTITIONS]> = OnceCell::new();

#[instrument]
pub async fn insert_into_queue(poster: String, outfile: String, immediate: bool) {
    let partition = if immediate {
        PARTITIONS - 1
    } else {
        let mut hasher = DefaultHasher::new();
        poster.hash(&mut hasher);
        (hasher.finish() % (PARTITIONS as u64 - 1)) as usize
    };
    let partitions = SENDER_PARTITIONS.get_or_init(|| {
        [(); PARTITIONS].map(|_| {
            let (tx, rx) = mpsc::channel(QUEUE_CAPACITY);
            tokio::spawn(process_queue(rx));
            tx
        })
    });
    // Bounded backpressure prevents metadata providers from creating unbounded resident work.
    if let Err(send_error) = partitions[partition].send((poster, outfile)).await {
        error!(?send_error, "Artwork queue stopped");
    }
}

#[instrument(skip_all)]
async fn process_queue(mut rx: Receiver<(String, String)>) {
    while let Some((url, outfile)) = rx.recv().await {
        if let Err(download_error) = download_artwork(&url, &outfile).await {
            error!(?download_error, %url, "Failed to cache artwork");
            persist_download_state(&url, "failed", Some(&download_error)).await;
        } else {
            persist_download_state(&url, "complete", None).await;
        }
    }
}

async fn persist_download_state(url: &str, status: &str, message: Option<&str>) {
    let Some(conn) = dim_database::try_get_conn() else {
        return;
    };
    let mut lock = conn.writer().lock_owned().await;
    let Ok(mut tx) = dim_database::write_tx(&mut lock).await else {
        return;
    };
    let _ = sqlx::query("UPDATE assets SET download_status = ?, download_error = ?, downloaded_at = CASE WHEN ? = 'complete' THEN CURRENT_TIMESTAMP ELSE downloaded_at END WHERE remote_url = ?")
        .bind(status).bind(message).bind(status).bind(url).execute(&mut tx).await;
    let _ = tx.commit().await;
}

async fn download_artwork(url: &str, outfile: &str) -> Result<(), String> {
    let response = CLIENT
        .get(url)
        .send()
        .await
        .map_err(|error| error.to_string())?;
    if !response.status().is_success() {
        return Err(format!("HTTP status {}", response.status()));
    }
    let content_type = response
        .headers()
        .get(CONTENT_TYPE)
        .and_then(|value| value.to_str().ok())
        .unwrap_or_default();
    if !content_type.starts_with("image/") {
        return Err(format!("unexpected content type {content_type:?}"));
    }
    if response
        .content_length()
        .map_or(false, |size| size > MAX_ARTWORK_BYTES as u64)
    {
        return Err("artwork exceeds size limit".into());
    }
    let bytes = response.bytes().await.map_err(|error| error.to_string())?;
    if bytes.len() > MAX_ARTWORK_BYTES {
        return Err("artwork exceeds size limit".into());
    }
    image::guess_format(&bytes).map_err(|error| format!("invalid image: {error}"))?;

    let metadata_path = METADATA_PATH
        .get()
        .ok_or_else(|| "metadata path is not initialized".to_owned())?;
    let out_path = crate::utils::safe_metadata_path(metadata_path, outfile)
        .map_err(|error| error.to_string())?;
    atomic_write(&out_path, &bytes).await
}

async fn atomic_write(out_path: &Path, bytes: &[u8]) -> Result<(), String> {
    let parent = out_path
        .parent()
        .ok_or_else(|| "artwork has no parent directory".to_owned())?;
    tokio::fs::create_dir_all(parent)
        .await
        .map_err(|error| error.to_string())?;
    let filename = out_path
        .file_name()
        .and_then(|name| name.to_str())
        .ok_or_else(|| "invalid artwork filename".to_owned())?;
    let temporary = parent.join(format!(".{filename}.{}.part", uuid::Uuid::new_v4()));
    if let Err(error) = tokio::fs::write(&temporary, bytes).await {
        return Err(error.to_string());
    }
    if let Err(error) = tokio::fs::rename(&temporary, out_path).await {
        let _ = tokio::fs::remove_file(&temporary).await;
        return Err(error.to_string());
    }
    debug!(path = ?out_path, "Artwork cached atomically");
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn atomic_artwork_replace_leaves_no_partial_file() {
        let directory = tempfile::tempdir().unwrap();
        let target = directory.path().join("poster.jpg");
        tokio::fs::write(&target, b"old").await.unwrap();
        atomic_write(&target, b"new image bytes").await.unwrap();
        assert_eq!(tokio::fs::read(&target).await.unwrap(), b"new image bytes");
        let entries = std::fs::read_dir(directory.path()).unwrap().count();
        assert_eq!(entries, 1);
    }
}
