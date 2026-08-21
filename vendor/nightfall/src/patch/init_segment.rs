use std::collections::VecDeque;
use std::fs::File;
use std::io::prelude::*;
use std::io::Seek;
use std::io::SeekFrom;
use std::path::Path;

use super::segment::Segment;
use crate::Result;

use tokio::task::spawn_blocking;

use mp4::mp4box::*;
use tracing::warn;

#[derive(Default)]
pub struct InitSegment {
    pub ftyp: Option<FtypBox>,
    // FIXME: implement parsers for mvex and mdta boxes.
    // For now we just save and dump the bytes since we dont modify them ever.
    pub moov: Vec<u8>,
    pub segments: VecDeque<Segment>,
}

impl InitSegment {
    pub fn from_reader(mut reader: impl BufRead + Seek, size: u64) -> Result<Self> {
        let mut segment = Self::default();
        let start = reader.stream_position()?;

        let mut current = start;
        let mut current_segment = Segment::default();

        while current < size {
            let header = BoxHeader::read(&mut reader)?;
            let BoxHeader { name, size: s } = header;

            match name {
                BoxType::SidxBox => {
                    let start = reader.stream_position()? - 8;
                    reader.seek(SeekFrom::Start(start))?;

                    let mut raw = vec![0; s as usize];
                    reader.read_exact(&mut raw)?;
                    current_segment.sidx_raw = Some(raw);
                }
                BoxType::MoofBox => {
                    current_segment.moof = Some(MoofBox::read_box(&mut reader, s)?);
                }
                BoxType::MdatBox => {
                    current_segment.mdat = Some(MdatBox::read_box(&mut reader, s)?);
                    // segments packed in the init segments dont come with a styp box
                    // so we clone the ftyp box of the init segment and change its type.
                    if current_segment.styp.is_none() {
                        current_segment.styp = segment.ftyp.clone();
                    }
                    // mdat is the last atom in a segment so we break the segment here and append it to
                    // our list.
                    segment.segments.push_back(current_segment);
                    current_segment = Segment::default();
                }
                BoxType::FtypBox => {
                    segment.ftyp = Some(FtypBox::read_box(&mut reader, s)?);
                }
                BoxType::MoovBox => {
                    segment.moov = vec![0; (s - 8) as usize];
                    reader.read_exact(segment.moov.as_mut_slice())?;
                }
                BoxType::StypBox => {
                    current_segment.styp = Some(FtypBox::read_box(&mut reader, s)?);
                }
                b => {
                    warn!(box_type = %b, "Got a weird box type.");
                    BoxHeader { name: b, size: s }.write(&mut segment.moov)?;
                    let mut boks = vec![0; (s - 8) as usize];
                    reader.read_exact(boks.as_mut_slice())?;
                    segment.moov.append(&mut boks);
                }
            }

            current = reader.stream_position()?;
        }

        Ok(segment)
    }

    /// Method will check if this init segment contains any real segments.
    pub fn contains_segments(&self) -> bool {
        !self.segments.is_empty()
    }

    pub fn normalize_and_dump(self, file: &mut File) -> Result<()> {
        if let Some(ftyp) = self.ftyp {
            ftyp.write_box(file)?;
        }

        BoxHeader {
            name: BoxType::MoovBox,
            size: self.moov.len() as u64 + 8,
        }
        .write(file)?;
        file.write_all(&self.moov)?;

        Ok(())
    }
}

/// Function reads a init segment and moves audio-visual data over from the init segment into
/// `segment`.
///
/// # Arguments
/// * `log` - logger instance for debugging
/// * `init` - Path to the initialization segment.
/// * `segment` - Path to the segment
/// * `seq` - starting sequence number
pub async fn patch_init_segment(
    init: impl AsRef<Path> + Send + 'static,
    segment_path: impl AsRef<Path> + Send + 'static,
    seq: u32,
) -> Result<u32> {
    let init = init.as_ref().to_path_buf();
    let segment_path = segment_path.as_ref().to_path_buf();
    let normalized_replacement = super::replacement_path(&init);
    let segment_replacement = super::replacement_path(&segment_path);
    let result = patch_init_segment_to(
        init.clone(),
        segment_replacement.clone(),
        normalized_replacement.clone(),
        seq,
    )
    .await;
    match result {
        Ok(next_sequence) => {
            let replace_result = spawn_blocking(move || {
                super::replace_atomically(&normalized_replacement, &init)?;
                if let Err(error) = super::replace_atomically(&segment_replacement, &segment_path) {
                    let _ = std::fs::remove_file(segment_replacement);
                    return Err(error);
                }
                Ok::<_, std::io::Error>(())
            })
            .await
            .map_err(|error| crate::NightfallError::SegmentPatchError(error.to_string()))?;
            replace_result?;
            Ok(next_sequence)
        }
        Err(error) => {
            let _ = std::fs::remove_file(normalized_replacement);
            let _ = std::fs::remove_file(segment_replacement);
            Err(error)
        }
    }
}

pub async fn patch_init_segment_to(
    init: impl AsRef<Path> + Send + 'static,
    segment_path: impl AsRef<Path> + Send + 'static,
    normalized_init_path: impl AsRef<Path> + Send + 'static,
    seq: u32,
) -> Result<u32> {
    let init = init.as_ref().to_path_buf();
    let segment_path = segment_path.as_ref().to_path_buf();
    let normalized_init_path = normalized_init_path.as_ref().to_path_buf();
    spawn_blocking(move || {
        super::engine::patch_init(&init, &segment_path, &normalized_init_path, seq)
    })
    .await
    .map_err(|error| crate::NightfallError::SegmentPatchError(error.to_string()))?
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn missing_embedded_media_is_not_published_as_an_empty_segment() {
        let temp = tempfile::tempdir().unwrap();
        let init = temp.path().join("0_init.mp4");
        InitSegment::default()
            .normalize_and_dump(&mut File::create(&init).unwrap())
            .unwrap();
        let media = temp.path().join("published/0.m4s");
        let normalized = temp.path().join("published/normalized_init.mp4");

        assert!(matches!(
            patch_init_segment_to(init, media.clone(), normalized.clone(), 0).await,
            Err(crate::NightfallError::MissingSegmentBox)
        ));
        assert!(!media.exists());
        assert!(!normalized.exists());
    }
}
