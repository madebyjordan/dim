use super::ProfileContext;
use super::ProfileType;
use super::Representation;
use super::StreamType;
use super::TranscodingProfile;

use crate::error::NightfallError;

#[derive(Debug)]
pub struct AudioTransmuxProfile;

impl TranscodingProfile for AudioTransmuxProfile {
    fn profile_type(&self) -> ProfileType {
        ProfileType::Transmux
    }

    fn stream_type(&self) -> StreamType {
        StreamType::Audio
    }

    fn name(&self) -> &str {
        "AudioTransmuxProfile"
    }

    fn build_args(&self, ctx: &ProfileContext, representation: &Representation) -> Vec<String> {
        let Representation::Fmp4Hls(hls) = representation else {
            unreachable!("audio transmux must produce fMP4 HLS")
        };
        let stream = format!("0:{}", ctx.input_ctx.stream);
        let mut args = vec![
            "-y".into(),
            "-ss".into(),
            hls.seek_seconds(),
            "-i".into(),
            ctx.file.clone(),
            "-copyts".into(),
            "-map".into(),
            stream,
            "-c:0".into(),
            "copy".into(),
            "-start_at_zero".into(),
            "-avoid_negative_ts".into(),
            "disabled".into(),
        ];
        super::command::append_fmp4_hls_output(
            &mut args,
            representation,
            super::command::HlsMuxOptions::default(),
        );
        args
    }

    fn supports(&self, ctx: &ProfileContext) -> Result<(), NightfallError> {
        if ctx.input_ctx.codec == ctx.output_ctx.codec
            && matches!(ctx.input_ctx.codec.as_str(), "aac" | "eac3" | "ac3")
            && ctx.output_ctx.bitrate.is_none()
            && ctx.output_ctx.audio_filter.is_none()
        {
            return Ok(());
        }
        Err(NightfallError::ProfileNotSupported(
            "Profile only supports unmodified MP4-compatible audio streams.".into(),
        ))
    }

    fn tag(&self) -> &str {
        "audio_copy"
    }
}

#[derive(Debug)]
pub struct AacTranscodeProfile;

impl TranscodingProfile for AacTranscodeProfile {
    fn profile_type(&self) -> ProfileType {
        ProfileType::Transcode
    }

    fn stream_type(&self) -> StreamType {
        StreamType::Audio
    }

    fn name(&self) -> &str {
        "AacTranscodeProfile"
    }

    fn build_args(&self, ctx: &ProfileContext, representation: &Representation) -> Vec<String> {
        let Representation::Fmp4Hls(hls) = representation else {
            unreachable!("AAC transcode must produce fMP4 HLS")
        };
        let stream = format!("0:{}", ctx.input_ctx.stream);

        // NOTE: might need flags -fflages +genpts if seeking breaks.
        let mut args = vec![
            "-y".into(),
            "-ss".into(),
            hls.seek_seconds(),
            "-i".into(),
            ctx.file.clone(),
            "-copyts".into(),
            "-map".into(),
            stream,
            "-c:0".into(),
            "aac".into(),
        ];

        if let Some(duration) = hls.window_seconds() {
            args.push("-t".into());
            args.push(duration);
        }

        if let Some(filter) = ctx.output_ctx.audio_filter.as_ref() {
            args.push("-af".into());
            args.push(filter.clone());
        }

        args.push("-ac".into());
        args.push(ctx.output_ctx.audio_channels.to_string());
        if let Some(sample_rate) = ctx.output_ctx.audio_sample_rate {
            args.push("-ar".into());
            args.push(sample_rate.to_string());
        }
        if let Some(layout) = ctx.output_ctx.audio_channel_layout.as_ref() {
            args.push("-ch_layout:a:0".into());
            args.push(layout.clone());
        }

        let ab = ctx.output_ctx.bitrate.unwrap_or(120_000).to_string();
        args.push("-b:a:0".into());
        args.push(ab);

        args.append(&mut vec![
            "-start_at_zero".into(),
            "-avoid_negative_ts".into(),
            "make_non_negative".into(),
        ]);

        super::command::append_fmp4_hls_output(
            &mut args,
            representation,
            super::command::HlsMuxOptions::default(),
        );
        args
    }

    fn supports(&self, ctx: &ProfileContext) -> Result<(), NightfallError> {
        if ctx.output_ctx.codec == "aac" {
            return Ok(());
        }

        Err(NightfallError::ProfileNotSupported(
            "Profile not supported.".into(),
        ))
    }

    fn tag(&self) -> &str {
        "aac"
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn context(input_codec: &str, output_codec: &str) -> ProfileContext {
        ProfileContext {
            file: "source.mkv".into(),
            input_ctx: super::super::InputCtx {
                codec: input_codec.into(),
                ..Default::default()
            },
            output_ctx: super::super::OutputCtx {
                codec: output_codec.into(),
                outdir: "output".into(),
                ..Default::default()
            },
            ..Default::default()
        }
    }

    fn value_after<'a>(args: &'a [String], flag: &str) -> Option<&'a str> {
        args.windows(2)
            .find(|pair| pair[0] == flag)
            .map(|pair| pair[1].as_str())
    }

    #[test]
    fn verified_eac3_is_copied_into_fragmented_mp4() {
        let ctx = context("eac3", "eac3");
        let args = AudioTransmuxProfile.build(ctx).unwrap();
        assert_eq!(value_after(&args, "-c:0"), Some("copy"));
        assert_eq!(value_after(&args, "-hls_segment_type"), Some("fmp4"));
    }

    #[test]
    fn normalized_surround_contract_is_explicit_in_ffmpeg_arguments() {
        let mut ctx = context("aac", "aac");
        ctx.output_ctx.audio_channels = 6;
        ctx.output_ctx.audio_channel_layout = Some("5.1".into());
        ctx.output_ctx.audio_filter = Some("pan=5.1|FL=FL|FR=FR|FC=FC|LFE=LFE|BL=SL|BR=SR".into());
        ctx.output_ctx.bitrate = Some(576_000);

        let args = AacTranscodeProfile.build(ctx).unwrap();
        assert_eq!(value_after(&args, "-ac"), Some("6"));
        assert_eq!(value_after(&args, "-ch_layout:a:0"), Some("5.1"));
        assert_eq!(value_after(&args, "-b:a:0"), Some("576000"));
        assert_eq!(
            value_after(&args, "-af"),
            Some("pan=5.1|FL=FL|FR=FR|FC=FC|LFE=LFE|BL=SL|BR=SR")
        );
    }

    #[test]
    fn standard_seven_one_contract_is_preserved_without_remapping() {
        let mut ctx = context("aac", "aac");
        ctx.output_ctx.audio_channels = 8;
        ctx.output_ctx.audio_channel_layout = Some("7.1".into());
        ctx.output_ctx.bitrate = Some(512_000);

        let args = AacTranscodeProfile.build(ctx).unwrap();
        assert_eq!(value_after(&args, "-ac"), Some("8"));
        assert_eq!(value_after(&args, "-ch_layout:a:0"), Some("7.1"));
        assert_eq!(value_after(&args, "-b:a:0"), Some("512000"));
        assert_eq!(value_after(&args, "-af"), None);
    }
}
