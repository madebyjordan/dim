use serde_derive::{Deserialize, Serialize};
use std::collections::VecDeque;
use std::process::Stdio;
use std::time::Duration;
use tokio::process::Command;
use tracing::error;
use tracing::trace;

#[derive(Clone, Debug, displaydoc::Display, thiserror::Error)]
pub enum Error {
    /// ffprobe could not be started or read: {0}
    Io(String),
    /// ffprobe exceeded its configured deadline.
    Timeout,
    /// ffprobe rejected the input: {0}
    InvalidMedia(String),
    /// ffprobe returned malformed JSON: {0}
    InvalidOutput(String),
}

impl Error {
    pub fn class(&self) -> &'static str {
        match self {
            Self::Io(_) => "probe_transient",
            Self::Timeout => "probe_timeout",
            Self::InvalidMedia(_) | Self::InvalidOutput(_) => "corrupt_media",
        }
    }

    pub fn retryable(&self) -> bool {
        matches!(self, Self::Io(_) | Self::Timeout)
    }
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct FFPStream {
    streams: Vec<Stream>,
    format: Format,
    #[serde(default)]
    corrupt: bool,
}

impl Default for FFPStream {
    fn default() -> Self {
        Self {
            corrupt: true,
            streams: Default::default(),
            format: Default::default(),
        }
    }
}

#[derive(Default, Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Stream {
    pub index: i64,
    // ffprobe omits codec_name for some non-media streams, such as Matroska text attachments.
    // Keep those streams observable without rejecting otherwise valid video/audio metadata.
    #[serde(default)]
    pub codec_name: String,
    pub profile: Option<String>,
    pub codec_type: String,
    pub codec_time_base: Option<String>,
    pub r_frame_rate: Option<String>,
    pub avg_frame_rate: Option<String>,
    pub width: Option<i64>,
    pub height: Option<i64>,
    pub coded_width: Option<i64>,
    pub coded_height: Option<i64>,
    pub display_aspect_ratio: Option<String>,
    pub is_avc: Option<String>,
    pub has_b_frames: Option<u64>,
    pub pix_fmt: Option<String>,
    pub level: Option<i64>,
    pub tags: Option<Tags>,
    pub sample_rate: Option<String>,
    pub channels: Option<i64>,
    pub channel_layout: Option<String>,
    pub bit_rate: Option<String>,
    pub duration_ts: Option<i64>,
    pub duration: Option<String>,
    pub color_range: Option<String>,
    pub color_space: Option<String>,
    pub color_transfer: Option<String>,
    pub color_primaries: Option<String>,
    pub chroma_location: Option<String>,
    pub disposition: Option<Disposition>,
}

impl Stream {
    pub fn get_bitrate(&self) -> Option<u64> {
        self.bit_rate
            .as_deref()
            .and_then(|value| value.parse::<u64>().ok())
            .or_else(|| self.tags.as_ref()?.bps_eng.as_deref()?.parse::<u64>().ok())
            .filter(|bitrate| *bitrate > 0)
    }

    pub fn get_codec(&self) -> &str {
        &self.codec_name
    }

    pub fn get_language(&self) -> Option<String> {
        self.tags.as_ref()?.language.clone()
    }

    pub fn get_title(&self) -> Option<String> {
        self.tags.as_ref()?.title.clone()
    }

    pub fn frame_rate(&self) -> Option<u64> {
        self.avg_frame_rate
            .as_deref()
            .or(self.r_frame_rate.as_deref())
            .and_then(|rate| {
                let (numerator, denominator) = rate.split_once('/')?;
                let numerator = numerator.parse::<f64>().ok()?;
                let denominator = denominator.parse::<f64>().ok()?;
                (denominator > 0.0).then_some((numerator / denominator).round() as u64)
            })
            .filter(|rate| *rate > 0)
    }
}

impl From<Stream> for nightfall::profiles::InputCtx {
    fn from(stream: Stream) -> nightfall::profiles::InputCtx {
        nightfall::profiles::InputCtx {
            stream: stream.index as usize,
            codec: stream.codec_name,
            pix_fmt: stream.pix_fmt.unwrap_or_default(),
            profile: stream.profile.unwrap_or_default(),
            bitrate: stream
                .tags
                .and_then(|x| x.bps_eng?.parse::<u64>().ok())
                .unwrap_or_default(),
            bframes: stream.has_b_frames,
            audio_channels: stream.channels.unwrap_or(2) as u64,
            ..Default::default()
        }
    }
}

#[derive(Default, Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Tags {
    pub language: Option<String>,
    pub title: Option<String>,
    #[serde(rename = "BPS-eng")]
    pub bps_eng: Option<String>,
    #[serde(rename = "DURATION-eng")]
    duration_eng: Option<String>,
    #[serde(rename = "NUMBER_OF_FRAMES-eng")]
    number_of_frames_eng: Option<String>,
    #[serde(rename = "_STATISTICS_WRITING_APP-eng")]
    statistics_writing_app_eng: Option<String>,
    #[serde(rename = "_STATISTICS_WRITING_DATE_UTC-eng")]
    statistics_writing_date_utc_eng: Option<String>,
    #[serde(rename = "_STATISTICS_TAGS-eng")]
    statistics_tags_eng: Option<String>,
    filename: Option<String>,
    mimetype: Option<String>,
}

#[derive(Default, Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(default)]
struct Format {
    pub filename: String,
    pub nb_streams: i64,
    pub nb_programs: i64,
    pub format_name: String,
    pub format_long_name: String,
    pub start_time: String,
    pub duration: String,
    pub size: String,
    pub bit_rate: String,
}

pub struct FFProbeCtx {
    ffprobe_bin: String,
}

impl FFProbeCtx {
    pub fn new(ffprobe_bin: &'static str) -> Self {
        Self {
            ffprobe_bin: ffprobe_bin.to_owned(),
        }
    }

    #[tracing::instrument(skip(self, file))]
    pub async fn get_meta(&self, file: impl ToString) -> Result<FFPStream, Error> {
        self.get_meta_with_timeout(file, Duration::from_secs(30))
            .await
    }

    pub async fn get_meta_with_timeout(
        &self,
        file: impl ToString,
        deadline: Duration,
    ) -> Result<FFPStream, Error> {
        let mut probe = Command::new(self.ffprobe_bin.clone());

        probe
            .kill_on_drop(true)
            .arg(file.to_string())
            .arg("-v")
            .arg("error")
            .arg("-print_format")
            .arg("json")
            .arg("-show_streams")
            .arg("-show_format")
            .stdout(Stdio::piped())
            .stderr(Stdio::piped());

        trace!(
            binary = self.ffprobe_bin.as_str(),
            args = %probe.as_std().get_args().filter_map(|x| x.to_str()).collect::<Vec<_>>().join(" "),
            "Spawning ffprobe."
        );

        let child = probe
            .spawn()
            .map_err(|error| Error::Io(error.to_string()))?;
        let output = tokio::time::timeout(deadline, child.wait_with_output())
            .await
            .map_err(|_| Error::Timeout)?
            .map_err(|error| Error::Io(error.to_string()))?;

        if !output.status.success() {
            let stderr = String::from_utf8_lossy(output.stderr.as_slice());
            error!(status = ?output.status, stderr = %stderr, "ffprobe exited with an error status.");
            return Err(Error::InvalidMedia(stderr.chars().take(512).collect()));
        }

        let json = String::from_utf8_lossy(output.stdout.as_slice());

        let de =
            serde_json::from_str(&json).map_err(|error| Error::InvalidOutput(error.to_string()))?;

        Ok(de)
    }
}

impl FFPStream {
    pub fn get_container(&self) -> String {
        self.format.format_name.clone()
    }

    pub fn get_primary_channels(&self) -> Option<i64> {
        self.get_primary("audio")?.channels
    }

    pub fn get_audio_lang(&self) -> Option<String> {
        self.get_primary("audio")?.get_language()
    }

    pub fn get_video_lang(&self) -> Option<String> {
        self.get_primary("video")?.get_language()
    }

    pub fn get_container_bitrate(&self) -> Option<u64> {
        self.format.bit_rate.parse::<u64>().ok()
    }

    pub fn get_video_codec(&self) -> Option<String> {
        Some(self.find_by_type("video").first()?.codec_name.clone())
    }

    pub fn get_video_profile(&self) -> Option<String> {
        self.find_by_type("video").first()?.profile.clone()
    }

    pub fn get_height(&self) -> Option<i64> {
        self.find_by_type("video").first()?.height
    }

    pub fn get_width(&self) -> Option<i64> {
        self.find_by_type("video").first()?.width
    }

    pub fn get_primary(&self, codec_type: &str) -> Option<&Stream> {
        let mut streams: VecDeque<_> = self.find_by_type(codec_type).into();

        if streams.is_empty() {
            return None;
        }

        if streams.len() == 1 {
            return streams.pop_front();
        }

        let primary_stream = streams.iter().find_map(|x| {
            if x.disposition.as_ref()?.default == 1 {
                Some(*x)
            } else {
                None
            }
        });

        primary_stream.or_else(|| streams.pop_front())
    }

    pub fn get_primary_codec(&self, codec_type: &str) -> Option<&str> {
        Some(&self.get_primary(codec_type)?.codec_name)
    }

    pub fn get_duration(&self) -> Option<i32> {
        Some(self.format.duration.parse::<f64>().ok()? as i32)
    }

    pub fn get_ms(&self) -> Option<u128> {
        self.format
            .duration
            .parse::<f64>()
            .map(|x| (x.trunc() * 1_000_000.0) as u128)
            .ok()
    }

    pub fn is_corrupt(&self) -> bool {
        self.corrupt
    }

    pub fn is_codec_type(&self, codec_type: &str) -> Option<bool> {
        Some(!self.find_by_type(codec_type).is_empty())
    }

    pub fn find_by_type(&self, codec_type: &str) -> Vec<&Stream> {
        self.streams
            .iter()
            .filter(|x| x.codec_type == *codec_type)
            .collect()
    }
}

#[derive(Debug, Deserialize, Clone, PartialEq, Eq, Serialize)]
pub struct Disposition {
    pub default: i64,
    pub dub: i64,
    pub original: i64,
    pub comment: i64,
    pub lyrics: i64,
    pub karaoke: i64,
    pub forced: i64,
    pub hearing_impaired: i64,
    pub visual_impaired: i64,
}

#[cfg(all(test, unix))]
mod tests {
    use super::*;
    use std::os::unix::fs::PermissionsExt;

    fn script(body: &str) -> tempfile::TempPath {
        let file = tempfile::NamedTempFile::new().unwrap();
        std::fs::write(file.path(), format!("#!/bin/sh\n{body}\n")).unwrap();
        let mut permissions = std::fs::metadata(file.path()).unwrap().permissions();
        permissions.set_mode(0o755);
        std::fs::set_permissions(file.path(), permissions).unwrap();
        file.into_temp_path()
    }

    #[test]
    fn reads_standard_stream_level_bitrate() {
        let stream = Stream {
            bit_rate: Some("6277855".into()),
            ..Default::default()
        };
        assert_eq!(stream.get_bitrate(), Some(6_277_855));
    }

    #[test]
    fn falls_back_to_matroska_stream_statistics_bitrate() {
        let stream = Stream {
            tags: Some(Tags {
                bps_eng: Some("5000000".into()),
                ..Default::default()
            }),
            ..Default::default()
        };
        assert_eq!(stream.get_bitrate(), Some(5_000_000));
    }

    #[test]
    fn does_not_invent_a_stream_bitrate() {
        assert_eq!(Stream::default().get_bitrate(), None);
    }

    #[test]
    fn accepts_attachment_without_codec_name() {
        let probe: FFPStream = serde_json::from_str(
            r#"{
                "streams": [
                    {"index": 0, "codec_name": "av1", "codec_type": "video"},
                    {
                        "index": 1,
                        "codec_type": "attachment",
                        "tags": {"filename": "encode.txt", "mimetype": "text/plain"}
                    }
                ],
                "format": {}
            }"#,
        )
        .expect("an attachment does not need to identify a media codec");

        assert_eq!(probe.get_video_codec().as_deref(), Some("av1"));
        assert_eq!(probe.find_by_type("attachment")[0].codec_name, "");
    }

    #[tokio::test]
    async fn classifies_probe_timeout_as_retryable() {
        let probe = script("sleep 2");
        let context = FFProbeCtx {
            ffprobe_bin: probe.to_string_lossy().into_owned(),
        };
        let error = context
            .get_meta_with_timeout("file.mkv", Duration::from_millis(20))
            .await
            .unwrap_err();
        assert!(matches!(error, Error::Timeout));
        assert!(error.retryable());
    }

    #[tokio::test]
    async fn classifies_rejected_input_as_corrupt_not_transient() {
        let probe = script("echo 'Invalid data found' >&2; exit 1");
        let context = FFProbeCtx {
            ffprobe_bin: probe.to_string_lossy().into_owned(),
        };
        let error = context.get_meta("file.mkv").await.unwrap_err();
        assert!(matches!(error, Error::InvalidMedia(_)));
        assert!(!error.retryable());
    }
}
