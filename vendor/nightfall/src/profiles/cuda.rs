use super::ProfileContext;
use super::ProfileType;
use super::Representation;
use super::StreamType;
use super::TranscodingProfile;

use crate::NightfallError;

/// Cuda(NVENC/NVDEC) transcoding profiles.
/// This is a nvidia exclusive transcoding profile that leverages cuda. This profile will
/// automatically be enabled if any of your GPUs support encoding and decoding h264 with the
/// profiles `Main`, `High` and `ConstrainedBaseline`. This profile will only transcode h264 input
/// streams.
#[cfg(target_os = "linux")]
#[derive(Debug)]
pub struct CudaTranscodeProfile;

#[cfg(target_os = "linux")]
impl TranscodingProfile for CudaTranscodeProfile {
    fn profile_type(&self) -> ProfileType {
        ProfileType::HardwareTranscode
    }

    fn stream_type(&self) -> StreamType {
        StreamType::Video
    }

    fn name(&self) -> &str {
        "CudaTranscodeProfile"
    }

    fn is_enabled(&self) -> Result<(), NightfallError> {
        // TODO: Add runtime profile support detection.
        Ok(())
    }

    fn build_args(&self, ctx: &ProfileContext, representation: &Representation) -> Vec<String> {
        let Representation::Fmp4Hls(hls) = representation else {
            unreachable!("CUDA transcode must produce fMP4 HLS")
        };
        let stream = format!("0:{}", ctx.input_ctx.stream);

        // ffmpeg -hwaccel cuda -hwaccel_output_format cuda -i input -c:v h264_nvenc -preset slow output
        let mut args = vec![
            "-hwaccel".into(),
            "cuda".into(),
            "-hwaccel_output_format".into(),
            "cuda".into(),
            "-y".into(),
            "-ss".into(),
            hls.seek_seconds(),
            "-i".into(),
            ctx.file.clone(),
            "-copyts".into(),
            "-map".into(),
            stream,
            "-c:0".into(),
            "h264_nvenc".into(),
            "-bf".into(),
            "0".into(),
        ];

        if let Some(height) = ctx.output_ctx.height {
            let width = ctx.output_ctx.width.unwrap_or(-2); // defaults to scaling by 2
            args.push("-vf".into());
            args.push(format!("scale_cuda={}:{}", width, height));
        }

        super::video::append_h264_output_signalling(&mut args, &ctx);

        if let Some(bitrate) = ctx.output_ctx.bitrate {
            args.push("-b:v".into());
            args.push(bitrate.to_string());
        }

        let gop_frames = match hls.frame_alignment {
            super::FrameAlignment::Passthrough {
                nominal_frames_per_segment,
                ..
            }
            | super::FrameAlignment::Constant {
                frames_per_segment: nominal_frames_per_segment,
                ..
            } => nominal_frames_per_segment,
            _ => unreachable!("video command has video frame alignment"),
        };
        args.append(&mut vec![
            "-start_at_zero".into(),
            "-avoid_negative_ts".into(),
            "disabled".into(),
            "-max_muxing_queue_size".into(),
            "2048".into(),
            "-keyint_min".into(),
            gop_frames.to_string(),
            "-g".into(),
            gop_frames.to_string(),
            "-frag_duration".into(),
            (hls.segment_duration_nanos / 1_000).to_string(),
        ]);
        super::video::append_video_fps_mode(&mut args, ctx.output_ctx.force_cfr);
        super::command::append_fmp4_hls_output(
            &mut args,
            representation,
            super::command::HlsMuxOptions {
                force_key_frames: true,
                independent_segments: true,
                disable_scene_change: false,
            },
        );
        args
    }

    /// This profile technically could work on any codec since the codec is just `copy` here, but
    /// the container doesnt support it, so we will be constricting it down.
    fn supports(&self, ctx: &ProfileContext) -> Result<(), NightfallError> {
        // TODO: At runtime check which file formats are supported by the current gpu for enc/dec.
        super::video::hardware_h264_contract_supported(ctx)
    }

    fn tag(&self) -> &str {
        "h264_cuda"
    }
}
