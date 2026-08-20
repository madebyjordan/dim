use super::ProfileContext;
use super::ProfileType;
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

    fn build(&self, ctx: ProfileContext) -> Option<Vec<String>> {
        let start_num = ctx.output_ctx.start_num.to_string();
        let stream = format!("0:{}", ctx.input_ctx.stream);
        let init_seg = format!("{}_init.mp4", &start_num);
        let seg_name = format!("{}/%d.m4s", ctx.output_ctx.outdir);
        let outdir = format!("{}/playlist.m3u8", ctx.output_ctx.outdir);
        let mut args = vec![
            "-y".into(),
            "-ss".into(),
            format!("{:.6}", ctx.output_ctx.start_time()),
            "-i".into(),
            ctx.file,
            "-copyts".into(),
            "-map".into(),
            stream,
            "-c:0".into(),
            "copy".into(),
            "-start_at_zero".into(),
            "-avoid_negative_ts".into(),
            "disabled".into(),
            "-f".into(),
            "hls".into(),
            "-start_number".into(),
            start_num,
            "-hls_flags".into(),
            "temp_file".into(),
            "-max_delay".into(),
            "5000000".into(),
        ];
        args.push("-hls_segment_options".into());
        args.push(if ctx.output_ctx.start_num > 0 {
            "movflags=frag_custom+dash+delay_moov+frag_discont".into()
        } else {
            "movflags=frag_custom+dash+delay_moov".into()
        });
        args.extend([
            "-hls_fmp4_init_filename".into(),
            init_seg,
            "-hls_time".into(),
            ctx.output_ctx.target_gop.to_string(),
            "-hls_segment_type".into(),
            "fmp4".into(),
            "-loglevel".into(),
            "info".into(),
            "-progress".into(),
            "pipe:1".into(),
            "-hls_segment_filename".into(),
            seg_name,
            outdir,
        ]);
        Some(args)
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

    fn build(&self, ctx: ProfileContext) -> Option<Vec<String>> {
        let start_num = ctx.output_ctx.start_num.to_string();
        let stream = format!("0:{}", ctx.input_ctx.stream);
        let init_seg = format!("{}_init.mp4", &start_num);
        let seg_name = format!("{}/%d.m4s", ctx.output_ctx.outdir);
        let outdir = format!("{}/playlist.m3u8", ctx.output_ctx.outdir);

        // NOTE: might need flags -fflages +genpts if seeking breaks.
        let mut args = vec![
            "-y".into(),
            "-ss".into(),
            format!("{:.6}", ctx.output_ctx.start_time()),
            "-i".into(),
            ctx.file,
            "-copyts".into(),
            "-map".into(),
            stream,
            "-c:0".into(),
            "aac".into(),
        ];

        if let Some(duration) = ctx.output_ctx.media_duration {
            args.push("-t".into());
            args.push(format!("{duration:.6}"));
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

        args.append(&mut vec![
            "-f".into(),
            "hls".into(),
            "-start_number".into(),
            start_num,
        ]);

        // needed so that in progress segments are named `tmp` and then renamed after the data is
        // on disk.
        // This in theory practically prevents the web server from returning a segment that is
        // in progress.
        args.append(&mut vec![
            "-hls_flags".into(),
            "temp_file".into(),
            "-max_delay".into(),
            "5000000".into(),
        ]);

        // these args are needed if we start a new stream in the middle of a old one, such as when
        // seeking. These args will reset the base decode ts to equal the earliest presentation
        // timestamp.
        if ctx.output_ctx.start_num > 0 {
            args.append(&mut vec![
                "-hls_segment_options".into(),
                "movflags=frag_custom+dash+delay_moov+frag_discont".into(),
            ]);
        } else {
            args.append(&mut vec![
                "-hls_segment_options".into(),
                "movflags=frag_custom+dash+delay_moov".into(),
            ]);
        }

        // args needed so we can distinguish between init fragments for new streams.
        // Basically on the web seeking works by reloading the entire video because of
        // discontinuity issues that browsers seem to not ignore like mpv.
        args.append(&mut vec!["-hls_fmp4_init_filename".into(), init_seg]);

        args.append(&mut vec![
            "-hls_time".into(),
            format!("{:.9}", ctx.output_ctx.segment_duration()),
        ]);

        args.append(&mut vec!["-hls_segment_type".into(), "fmp4".into()]);
        args.append(&mut vec![
            "-loglevel".into(),
            "info".into(),
            "-progress".into(),
            "pipe:1".into(),
        ]);
        args.append(&mut vec!["-hls_segment_filename".into(), seg_name]);
        args.append(&mut vec![outdir]);

        Some(args)
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

    fn value_after<'a>(args: &'a [String], flag: &str) -> Option<&'a str> {
        args.windows(2)
            .find(|pair| pair[0] == flag)
            .map(|pair| pair[1].as_str())
    }

    #[test]
    fn verified_eac3_is_copied_into_fragmented_mp4() {
        let mut ctx = ProfileContext::default();
        ctx.input_ctx.codec = "eac3".into();
        ctx.output_ctx.codec = "eac3".into();
        let args = AudioTransmuxProfile.build(ctx).unwrap();
        assert_eq!(value_after(&args, "-c:0"), Some("copy"));
        assert_eq!(value_after(&args, "-hls_segment_type"), Some("fmp4"));
    }

    #[test]
    fn normalized_surround_contract_is_explicit_in_ffmpeg_arguments() {
        let mut ctx = ProfileContext::default();
        ctx.output_ctx.codec = "aac".into();
        ctx.output_ctx.audio_channels = 6;
        ctx.output_ctx.audio_channel_layout = Some("5.1".into());
        ctx.output_ctx.audio_filter =
            Some("pan=5.1|FL=FL|FR=FR|FC=FC|LFE=LFE|BL=SL|BR=SR".into());
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
        let mut ctx = ProfileContext::default();
        ctx.output_ctx.codec = "aac".into();
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
