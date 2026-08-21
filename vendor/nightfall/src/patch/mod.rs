use crate::NightfallError;
use crate::Result;
use std::fs;
use std::fs::File;
use std::fs::OpenOptions;
use std::io::Read;
use std::io::Write;
use std::path::Path;
use std::path::PathBuf;

mod engine;
pub mod init_segment;
pub mod segment;

pub(crate) fn replacement_path(destination: &Path) -> PathBuf {
    let file_name = destination
        .file_name()
        .and_then(|name| name.to_str())
        .unwrap_or("segment");
    destination.with_file_name(format!(
        ".{file_name}.{}.nightfall-replacement",
        uuid::Uuid::new_v4().hyphenated()
    ))
}

pub(crate) fn write_new_atomically(
    destination: &Path,
    write: impl FnOnce(&mut File) -> Result<()>,
) -> Result<bool> {
    let parent = destination.parent().ok_or_else(|| {
        NightfallError::SegmentPatchError("Published segment has no parent directory".into())
    })?;
    fs::create_dir_all(parent)?;
    let temporary = replacement_path(destination);
    let result = (|| {
        let mut file = OpenOptions::new()
            .write(true)
            .create_new(true)
            .open(&temporary)?;
        write(&mut file)?;
        file.flush()?;
        file.sync_all()?;
        drop(file);

        // Linking a fully-synced sibling file is an atomic create-if-absent operation on every
        // supported target. Unlike rename, it cannot replace an immutable generation artifact if
        // two publishers race. An identical pre-existing artifact makes retry idempotent; a
        // different one is treated as corruption rather than silently accepted.
        match fs::hard_link(&temporary, destination) {
            Ok(()) => Ok(true),
            Err(error) if error.kind() == std::io::ErrorKind::AlreadyExists => {
                if files_equal(&temporary, destination)? {
                    Ok(false)
                } else {
                    Err(NightfallError::SegmentPatchError(format!(
                        "Refusing to replace non-identical published artifact {}",
                        destination.display()
                    )))
                }
            }
            Err(error) => Err(error.into()),
        }
    })();

    if result.is_err() || temporary.exists() {
        let _ = fs::remove_file(&temporary);
    }
    result
}

fn files_equal(left: &Path, right: &Path) -> Result<bool> {
    if fs::metadata(left)?.len() != fs::metadata(right)?.len() {
        return Ok(false);
    }

    let mut left = File::open(left)?;
    let mut right = File::open(right)?;
    let mut left_buffer = [0_u8; 64 * 1024];
    let mut right_buffer = [0_u8; 64 * 1024];
    loop {
        let left_count = left.read(&mut left_buffer)?;
        let right_count = right.read(&mut right_buffer)?;
        if left_count != right_count || left_buffer[..left_count] != right_buffer[..right_count] {
            return Ok(false);
        }
        if left_count == 0 {
            return Ok(true);
        }
    }
}

pub(crate) fn replace_atomically(source: &Path, destination: &Path) -> std::io::Result<()> {
    #[cfg(windows)]
    {
        use std::os::windows::ffi::OsStrExt;
        use windows_sys::Win32::Storage::FileSystem::{
            MoveFileExW, MOVEFILE_REPLACE_EXISTING, MOVEFILE_WRITE_THROUGH,
        };

        let source: Vec<u16> = source
            .as_os_str()
            .encode_wide()
            .chain(std::iter::once(0))
            .collect();
        let destination: Vec<u16> = destination
            .as_os_str()
            .encode_wide()
            .chain(std::iter::once(0))
            .collect();
        let result = unsafe {
            MoveFileExW(
                source.as_ptr(),
                destination.as_ptr(),
                MOVEFILE_REPLACE_EXISTING | MOVEFILE_WRITE_THROUGH,
            )
        };
        if result == 0 {
            return Err(std::io::Error::last_os_error());
        }
        Ok(())
    }

    #[cfg(not(windows))]
    {
        fs::rename(source, destination)
    }
}

pub(crate) async fn publish_copy(
    source: impl AsRef<Path> + Send + 'static,
    destination: impl AsRef<Path> + Send + 'static,
) -> Result<()> {
    let source = source.as_ref().to_path_buf();
    let destination = destination.as_ref().to_path_buf();
    tokio::task::spawn_blocking(move || {
        let created = write_new_atomically(&destination, |output| {
            let mut input = File::open(source)?;
            std::io::copy(&mut input, output)?;
            Ok(())
        })?;
        if created || destination.is_file() {
            Ok(())
        } else {
            Err(NightfallError::SegmentPatchError(format!(
                "Failed to publish {}",
                destination.display()
            )))
        }
    })
    .await
    .map_err(|error| NightfallError::SegmentPatchError(error.to_string()))?
}
