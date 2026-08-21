use std::convert::TryInto;
use std::fs::File;
use std::io::prelude::*;
use std::io::Seek;
use std::io::SeekFrom;
use std::path::Path;

use crate::NightfallError;
use crate::Result;

use tokio::task::spawn_blocking;

use mp4::mp4box::*;
use tracing::debug;

/// Struct represents an individual segment from a stream.
#[derive(Clone, Default, Debug)]
pub struct Segment {
    /// styp box is needed if the segment is written to a separate file.
    /// in our case we just clone it from the parent init segment.
    pub styp: Option<FtypBox>,
    /// segment index box contains the index of the segment.
    pub sidx: Option<SidxBox>,
    /// Original bytes for the segment index box. The current mp4 crate truncates FFmpeg 8's
    /// version-1 SIDX boxes when it serializes them.
    pub sidx_raw: Option<Vec<u8>>,
    /// Moof box contains metadata about the segment like the PTS and DTS.
    pub moof: Option<MoofBox>,
    /// Contains audio-visual data.
    pub mdat: Option<MdatBox>,
}

impl Segment {
    pub fn debug(&self) {
        println!("styp: {:?}", self.styp);
        println!("sidx: {:?}", self.sidx);
        println!("moof: {:?}", self.moof);
        println!(
            "mdat: {:?}",
            self.mdat.as_ref().map(|x| x.data.len()).unwrap_or(0)
        );
    }

    pub fn from_reader(mut reader: impl BufRead + Seek, size: u64) -> Result<(Self, u64)> {
        let start = reader.stream_position()?;

        let mut current = start;
        let mut segment = Self::default();

        while current < size {
            let header = BoxHeader::read(&mut reader)?;
            let BoxHeader { name, size: s } = header;

            match name {
                BoxType::SidxBox => {
                    let start = reader.stream_position()? - 8;
                    reader.seek(SeekFrom::Start(start))?;

                    let mut raw = vec![0; s as usize];
                    reader.read_exact(&mut raw)?;
                    segment.sidx_raw = Some(raw);
                }
                BoxType::MoofBox => {
                    segment.moof = Some(MoofBox::read_box(&mut reader, s)?);
                }
                BoxType::MdatBox => {
                    segment.mdat = Some(MdatBox::read_box(&mut reader, s)?);

                    // Since mdat would be the last box in the segment, we just return the segment
                    // here as well as the leftover bytes.
                    let leftover_bytes = reader.stream_position()?;
                    return Ok((segment, leftover_bytes));
                }
                BoxType::StypBox => {
                    let mut styp = FtypBox::read_box(&mut reader, s)?;
                    styp.box_type = BoxType::StypBox;
                    segment.styp = Some(styp);
                }
                b => {
                    debug!(box_type = %b, "Got a weird box type.");
                    skip_box(&mut reader, s)?;
                }
            }

            current = reader.stream_position()?;
        }

        // NOTE: In some cases, we could get here without a complete segment existing.
        Ok((segment, size))
    }

    /// Sometimes ffmpeg will output a bare initial segment.
    /// This method allows us to detect such segments and apply a fix if we want accurate seeks.
    /// An empty segment consists of a segment with only a `styp` box.
    pub fn is_empty_segment(&self) -> bool {
        self.styp.is_some()
            && self.sidx.is_none()
            && self.sidx_raw.is_none()
            && self.moof.is_none()
            && self.mdat.is_none()
    }

    /// Method will create a styp box for this segment if it doesnt exist
    pub fn gen_styp(mut self) -> Self {
        if self.styp.is_none() {
            let styp = FtypBox {
                box_type: BoxType::StypBox,
                ..Default::default()
            };

            self.styp = Some(styp);
        }

        self
    }

    pub fn set_styp(mut self) -> Self {
        if let Some(styp) = self.styp.as_mut() {
            styp.box_type = BoxType::StypBox;
        }

        self
    }

    pub fn set_segno(mut self, seq: u32) -> Self {
        if let Some(moof) = self.moof.as_mut() {
            moof.mfhd.sequence_number = seq;
        }
        self
    }

    pub fn normalize_dts(mut self) -> Self {
        let earliest_presentation_time = self
            .sidx_raw
            .as_deref()
            .and_then(sidx_earliest_presentation_time)
            .or_else(|| {
                self.sidx
                    .as_ref()
                    .map(|sidx| sidx.earliest_presentation_time)
            });

        // NOTE: Sometimes the first segment after init.mp4 can be blank, in cases like that we
        // just ignore that moof is empty.
        if let Some(tfdt) = self
            .moof
            .as_mut()
            .and_then(|x| x.trafs.get_mut(0).and_then(|x| x.tfdt.as_mut()))
        {
            if let Some(earliest_presentation_time) = earliest_presentation_time {
                tfdt.base_media_decode_time = earliest_presentation_time;
            }
        }

        self
    }

    pub fn write(self, file: &mut File) -> Result<()> {
        if let Some(styp) = self.styp {
            styp.write_box(file)?;
        }

        if let Some(raw) = self.sidx_raw {
            file.write_all(&raw)?;
        } else if let Some(sidx) = self.sidx {
            sidx.write_box(file)?;
        }

        if let Some(moof) = self.moof {
            moof.write_box(file)?;
        }

        if let Some(mdat) = self.mdat {
            mdat.write_box(file)?;
        }

        Ok(())
    }
}

fn sidx_earliest_presentation_time(raw: &[u8]) -> Option<u64> {
    const VERSION_OFFSET: usize = 8;
    const PRESENTATION_TIME_OFFSET: usize = 20;

    match *raw.get(VERSION_OFFSET)? {
        0 => raw
            .get(PRESENTATION_TIME_OFFSET..PRESENTATION_TIME_OFFSET + 4)?
            .try_into()
            .ok()
            .map(u32::from_be_bytes)
            .map(u64::from),
        1 => raw
            .get(PRESENTATION_TIME_OFFSET..PRESENTATION_TIME_OFFSET + 8)?
            .try_into()
            .ok()
            .map(u64::from_be_bytes),
        _ => None,
    }
}

#[cfg(test)]
mod tests {
    use super::{sidx_earliest_presentation_time, Segment};
    use std::fs;
    use std::fs::File;
    use std::io::Cursor;

    #[test]
    fn preserves_ffmpeg_8_version_one_sidx_bytes() {
        let sidx = vec![
            0, 0, 0, 52, 115, 105, 100, 120, 1, 0, 0, 0, 0, 0, 0, 1, 0, 0, 48, 0, 0, 0, 0, 0, 0, 0,
            0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 1, 0, 13, 166, 51, 0, 1, 244, 0, 128, 0, 0, 0,
        ];
        let (segment, position) =
            Segment::from_reader(Cursor::new(sidx.as_slice()), sidx.len() as u64)
                .expect("FFmpeg SIDX should parse");
        assert_eq!(position, sidx.len() as u64);

        let path = std::env::temp_dir().join(format!(
            "nightfall-sidx-{}.m4s",
            uuid::Uuid::new_v4().hyphenated()
        ));
        segment
            .write(&mut File::create(&path).expect("temporary segment should be created"))
            .expect("segment should be written");

        assert_eq!(fs::read(&path).unwrap(), sidx);
        fs::remove_file(path).unwrap();
    }

    #[test]
    fn reads_ffmpeg_8_version_one_sidx_presentation_time() {
        let mut sidx = vec![0; 52];
        sidx[8] = 1;
        sidx[20..28].copy_from_slice(&42_u64.to_be_bytes());

        assert_eq!(sidx_earliest_presentation_time(&sidx), Some(42));
    }
}

/// Function reads a segment file and patches it so that it is consistent
///
/// # Arguments
/// * `log` - logger instance for debugging
/// * `file` - target input/output file.
/// * `seq` - starting sequence number.
///
/// # Returns
/// This function will return the index of the current segment.
pub async fn patch_segment(file: impl AsRef<Path> + Send + 'static, seq: u32) -> Result<u32> {
    let file = file.as_ref().to_path_buf();
    let replacement = super::replacement_path(&file);
    let result = patch_segment_to(file.clone(), replacement.clone(), seq).await;
    match result {
        Ok(next_sequence) => {
            let replace_result = spawn_blocking(move || {
                let result = super::replace_atomically(&replacement, &file);
                if result.is_err() {
                    let _ = std::fs::remove_file(replacement);
                }
                result
            })
            .await
            .map_err(|error| NightfallError::SegmentPatchError(error.to_string()))?;
            replace_result?;
            Ok(next_sequence)
        }
        Err(error) => {
            let _ = std::fs::remove_file(replacement);
            Err(error)
        }
    }
}

pub async fn patch_segment_to(
    input: impl AsRef<Path> + Send + 'static,
    output: impl AsRef<Path> + Send + 'static,
    seq: u32,
) -> Result<u32> {
    let input = input.as_ref().to_path_buf();
    let output = output.as_ref().to_path_buf();
    spawn_blocking(move || super::engine::patch_media(&input, &output, seq))
        .await
        .map_err(|error| NightfallError::SegmentPatchError(error.to_string()))?
}
