use super::ProfileContext;
use super::ProfileType;
use super::Representation;
use super::StreamType;
use super::TranscodingProfile;

use crate::NightfallError;

#[derive(Debug)]
pub struct AmfTranscodeProfile;

impl TranscodingProfile for AmfTranscodeProfile {
    fn profile_type(&self) -> ProfileType {
        ProfileType::HardwareTranscode
    }

    fn stream_type(&self) -> StreamType {
        StreamType::Video
    }

    fn name(&self) -> &str {
        "AmfTranscodeProfile"
    }

    fn build_args(&self, ctx: &ProfileContext, representation: &Representation) -> Vec<String> {
        let Representation::Fmp4Hls(hls) = representation else {
            unreachable!("AMF transcode must produce fMP4 HLS")
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
            "h264_amf".into(),
        ];

        args.extend(["-pix_fmt".into(), "yuv420p".into()]);
        super::video::append_h264_output_signalling(&mut args, &ctx);

        if let Some(bitrate) = ctx.output_ctx.bitrate {
            args.extend(["-b:v".into(), bitrate.to_string()]);
        }

        args.append(&mut vec![
            "-start_at_zero".into(),
            "-avoid_negative_ts".into(),
            "disabled".into(),
            "-max_muxing_queue_size".into(),
            "2048".into(),
        ]);
        super::video::append_video_fps_mode(&mut args, ctx.output_ctx.force_cfr);

        super::command::append_fmp4_hls_output(
            &mut args,
            representation,
            super::command::HlsMuxOptions {
                force_key_frames: true,
                ..Default::default()
            },
        );
        args
    }

    /// This profile technically could work on any codec since the codec is just `copy` here, but
    /// the container doesnt support it, so we will be constricting it down.
    fn supports(&self, ctx: &ProfileContext) -> Result<(), NightfallError> {
        super::video::hardware_h264_contract_supported(ctx)
    }

    fn tag(&self) -> &str {
        "h264_amf"
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn amf_is_classified_as_hardware_transcoding() {
        assert_eq!(
            AmfTranscodeProfile.profile_type(),
            ProfileType::HardwareTranscode
        );
    }
}
