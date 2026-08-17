use crate::error::NightfallError;

use super::ProfileContext;
use super::ProfileType;
use super::StreamType;
use super::TranscodingProfile;

#[derive(Debug)]
pub struct WebvttTranscodeProfile;

impl TranscodingProfile for WebvttTranscodeProfile {
    fn profile_type(&self) -> ProfileType {
        ProfileType::Transcode
    }

    fn stream_type(&self) -> StreamType {
        StreamType::Subtitle
    }

    fn name(&self) -> &str {
        "WebvttTranscodeProfile"
    }

    fn build(&self, ctx: ProfileContext) -> Option<Vec<String>> {
        let args = vec![
            "-y".into(),
            "-i".into(),
            ctx.file,
            "-map".into(),
            format!("0:{}", ctx.input_ctx.stream),
            "-f".into(),
            "webvtt".into(),
            "-".into(),
        ];

        Some(args)
    }

    fn supports(&self, ctx: &ProfileContext) -> Result<(), NightfallError> {
        if ["srt", "ass", "ssa", "subrip", "webvtt", "vtt"]
            .contains(&ctx.input_ctx.codec.as_str())
            && ctx.output_ctx.codec == "webvtt"
        {
            return Ok(());
        }

        Err(NightfallError::ProfileNotSupported(format!(
            "Codec {} not supported.",
            ctx.input_ctx.codec.as_str()
        )))
    }

    fn tag(&self) -> &str {
        "webvtt"
    }

    fn is_stdio_stream(&self) -> bool {
        true
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::profiles::{InputCtx, OutputCtx};

    fn context(codec: &str) -> ProfileContext {
        ProfileContext {
            input_ctx: InputCtx {
                codec: codec.into(),
                ..Default::default()
            },
            output_ctx: OutputCtx {
                codec: "webvtt".into(),
                ..Default::default()
            },
            ..Default::default()
        }
    }

    #[test]
    fn accepts_all_advertised_text_subtitle_codecs() {
        let profile = WebvttTranscodeProfile;
        for codec in ["srt", "subrip", "ass", "ssa", "webvtt", "vtt"] {
            assert!(profile.supports(&context(codec)).is_ok(), "{codec}");
        }
    }
}

#[cfg(feature = "ssa_transmux")]
#[derive(Debug)]
pub struct AssExtractProfile;

#[cfg(feature = "ssa_transmux")]
impl TranscodingProfile for AssExtractProfile {
    fn profile_type(&self) -> ProfileType {
        ProfileType::Transmux
    }

    fn stream_type(&self) -> StreamType {
        StreamType::Subtitle
    }

    fn name(&self) -> &str {
        "AssExtractProfile"
    }

    fn build(&self, ctx: ProfileContext) -> Option<Vec<String>> {
        let args = vec![
            "-y".into(),
            "-i".into(),
            ctx.file,
            "-map".into(),
            format!("0:{}", ctx.input_ctx.stream),
            "-f".into(),
            "ass".into(),
            "-".into(),
        ];

        Some(args)
    }

    fn supports(&self, ctx: &ProfileContext) -> Result<(), NightfallError> {
        if ["ass", "ssa"].contains(&ctx.input_ctx.codec.as_str()) && ctx.output_ctx.codec.as_str() == "ass" {
            return Ok(());
        }

        Err(NightfallError::ProfileNotSupported(
            "Profile only supports extracting ass subtitles.".into(),
        ))
    }

    fn tag(&self) -> &str {
        "ass"
    }

    fn is_stdio_stream(&self) -> bool {
        true
    }
}
