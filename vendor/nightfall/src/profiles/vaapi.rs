use super::ProfileContext;
use super::ProfileType;
use super::Representation;
use super::StreamType;
use super::TranscodingProfile;

use crate::NightfallError;

use std::fs;
use std::path::PathBuf;

/// Vaapi transcoding profiles.
/// This is a unix exclusive transcoding profile that leverages vaapi. This profile will
/// automatically be enabled if any of your GPUs support encoding and decoding h264 with the
/// profiles `Main`, `High` and `ConstrainedBaseline`. This profile will only transcode h264 input
/// streams.
#[cfg(target_os = "linux")]
#[derive(Debug)]
pub struct VaapiTranscodeProfile {
    profiles: Vec<rusty_vainfo::Profile>,
    vendor: String,
    dri: PathBuf,
}

impl VaapiTranscodeProfile {
    const H264_ENCODE_PROFILES: [&'static str; 4] = [
        "VAProfileH264ConstrainedBaseline",
        "VAProfileH264Baseline",
        "VAProfileH264Main",
        "VAProfileH264High",
    ];
    const ENCODE_ENTRYPOINTS: [&'static str; 2] =
        ["VAEntrypointEncSlice", "VAEntrypointEncSliceLP"];

    pub fn new() -> Option<Self> {
        let hw_targets = fs::read_dir("/dev/dri")
            .ok()?
            .filter_map(Result::ok)
            .filter(|x| x.file_name().to_string_lossy().contains("render"))
            .map(|x| x.path())
            .collect::<Vec<_>>();

        for target in hw_targets {
            if let Ok(x) = rusty_vainfo::VaInstance::with_drm(&target) {
                return Some(Self {
                    profiles: x.profiles().unwrap_or_default(),
                    vendor: x.vendor_string(),
                    dri: target,
                });
            }
        }

        Some(Self {
            profiles: Vec::new(),
            vendor: "<null_device>".into(),
            dri: PathBuf::new(),
        })
    }

    fn hw_scaling_supported(&self) -> bool {
        self.supports_h264_encoding(None)
    }

    fn has_entrypoint(&self, profile: &str, entrypoints: &[&str]) -> bool {
        self.profiles.iter().any(|candidate| {
            candidate.name == profile
                && candidate
                    .entrypoints
                    .iter()
                    .any(|entrypoint| entrypoints.contains(&entrypoint.as_str()))
        })
    }

    fn supports_h264_encoding(&self, requested_profile: Option<&str>) -> bool {
        let candidates: &[&str] = match requested_profile.map(str::to_ascii_lowercase).as_deref() {
            Some("baseline" | "constrained baseline") => &Self::H264_ENCODE_PROFILES[..2],
            Some("main") => &Self::H264_ENCODE_PROFILES[2..3],
            Some("high") => &Self::H264_ENCODE_PROFILES[3..],
            Some(_) => return false,
            None => &Self::H264_ENCODE_PROFILES,
        };
        candidates
            .iter()
            .any(|profile| self.has_entrypoint(profile, &Self::ENCODE_ENTRYPOINTS))
    }
}

#[cfg(target_os = "linux")]
impl TranscodingProfile for VaapiTranscodeProfile {
    fn profile_type(&self) -> ProfileType {
        ProfileType::HardwareTranscode
    }

    fn stream_type(&self) -> StreamType {
        StreamType::Video
    }

    fn name(&self) -> &str {
        "VaapiTranscodeProfile"
    }

    fn is_enabled(&self) -> Result<(), NightfallError> {
        if !self.supports_h264_encoding(None) {
            return Err(NightfallError::ProfileNotSupported(format!(
                "Device {} has no H.264 slice-encoding entrypoint.",
                self.vendor
            )));
        }
        if !self.profiles.iter().any(|profile| {
            profile
                .entrypoints
                .iter()
                .any(|entrypoint| entrypoint == "VAEntrypointVLD")
        }) {
            return Err(NightfallError::ProfileNotSupported(format!(
                "Device {} has no video decode entrypoint.",
                self.vendor
            )));
        }
        Ok(())
    }

    fn build_args(&self, ctx: &ProfileContext, representation: &Representation) -> Vec<String> {
        let Representation::Fmp4Hls(hls) = representation else {
            unreachable!("VAAPI transcode must produce fMP4 HLS")
        };
        let stream = format!("0:{}", ctx.input_ctx.stream);

        let mut args = vec![
            "-hwaccel".into(),
            "vaapi".into(),
            "-vaapi_device".into(),
            self.dri.to_string_lossy().into(),
            "-hwaccel_output_format".into(),
            "vaapi".into(),
            "-y".into(),
            "-ss".into(),
            hls.seek_seconds(),
            "-i".into(),
            ctx.file.clone(),
            "-copyts".into(),
            "-map".into(),
            stream,
            "-c:0".into(),
            "h264_vaapi".into(),
            "-bf".into(),
            "0".into(),
        ];

        args.push("-vf".into());

        if let Some(height) = ctx.output_ctx.height {
            let mut vfilter = Vec::new();
            let width = ctx.output_ctx.width.unwrap_or(-2); // defaults to scaling by 2

            if self.hw_scaling_supported() {
                vfilter.push(format!("scale_vaapi={}:{}", width, height));
            }

            vfilter.push("hwdownload".into());

            // TODO: Detect if input file is 10-bit with a less hacky way.
            if ctx.input_ctx.profile.as_str() == "Main 10" {
                vfilter.push("format=p010le".into());
            }

            vfilter.push("format=nv12".into());

            if !self.hw_scaling_supported() {
                vfilter.push(format!("scale={}:{}", width, height));
            }

            vfilter.push("hwupload".into());

            args.push(vfilter.join(","));
        } else {
            args.push("hwdownload,format=nv12,hwupload".into());
        }

        if let Some(bitrate) = ctx.output_ctx.bitrate {
            // NOTE: it seems that when the non-free qsv driver is not installed then we cant use
            // -b:v. This might be a way to detect whether we can use -b:v flag but im not too
            // sure.
            if !self.hw_scaling_supported() {
                args.push("-maxrate".into());
                args.push(bitrate.to_string());
            } else {
                args.push("-b:v".into());
                args.push(bitrate.to_string());
            }
        }

        super::video::append_h264_output_signalling(&mut args, &ctx);

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
                disable_scene_change: true,
            },
        );
        args
    }

    /// This profile technically could work on any codec since the codec is just `copy` here, but
    /// the container doesnt support it, so we will be constricting it down.
    fn supports(&self, ctx: &ProfileContext) -> Result<(), NightfallError> {
        super::video::hardware_h264_contract_supported(ctx)?;
        if !["h264", "hevc"].contains(&ctx.input_ctx.codec.as_str()) {
            return Err(NightfallError::ProfileNotSupported(
                "Profile only supports decoding h264 or h265 video streams.".into(),
            ));
        }

        let decode_profiles: &[&str] =
            match [ctx.input_ctx.codec.as_str(), ctx.input_ctx.profile.as_str()] {
                ["h264", "High"] => &["VAProfileH264High"],
                ["h264", "Main"] => &["VAProfileH264Main"],
                ["h264", "Baseline" | "Constrained Baseline"] => {
                    &["VAProfileH264Baseline", "VAProfileH264ConstrainedBaseline"]
                }
                ["hevc", "Main"] => &["VAProfileHEVCMain"],
                ["hevc", "Main 10"] => &["VAProfileHEVCMain10"],
                [codec, profile] => {
                    return Err(NightfallError::ProfileNotSupported(format!(
                        "Profile {} for {} not supported by device.",
                        profile, codec
                    )))
                }
            };

        if !decode_profiles
            .iter()
            .any(|profile| self.has_entrypoint(profile, &["VAEntrypointVLD"]))
        {
            return Err(NightfallError::ProfileNotSupported(format!(
                "Device does not expose a decode entrypoint for {} {}.",
                ctx.input_ctx.codec, ctx.input_ctx.profile
            )));
        }

        if !self.supports_h264_encoding(ctx.output_ctx.video_profile.as_deref()) {
            return Err(NightfallError::ProfileNotSupported(
                "Device does not expose the required H.264 encoding entrypoint.".into(),
            ));
        }

        Ok(())
    }

    fn tag(&self) -> &str {
        "h264_vaapi"
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use rusty_vainfo::Profile;

    fn profile(name: &str, entrypoints: &[&str]) -> Profile {
        Profile {
            name: name.into(),
            entrypoints: entrypoints.iter().map(ToString::to_string).collect(),
        }
    }

    fn context() -> ProfileContext {
        let mut context = ProfileContext::default();
        context.file = "source.mp4".into();
        context.input_ctx.codec = "h264".into();
        context.input_ctx.profile = "High".into();
        context.input_ctx.pix_fmt = "yuv420p".into();
        context.input_ctx.fps = 24.0;
        context.output_ctx.codec = "h264".into();
        context.output_ctx.outdir = "output".into();
        context.output_ctx.width = Some(1280);
        context.output_ctx.height = Some(720);
        context
    }

    fn vaapi(profiles: Vec<Profile>) -> VaapiTranscodeProfile {
        VaapiTranscodeProfile {
            profiles,
            vendor: "test".into(),
            dri: "/dev/dri/renderD128".into(),
        }
    }

    #[test]
    fn enablement_requires_real_encode_and_decode_entrypoints() {
        let encode_only = vaapi(vec![profile(
            "VAProfileH264High",
            &["VAEntrypointEncSlice"],
        )]);
        assert!(encode_only.is_enabled().is_err());

        let decode_only = vaapi(vec![profile("VAProfileH264High", &["VAEntrypointVLD"])]);
        assert!(decode_only.is_enabled().is_err());
    }

    #[test]
    fn support_requires_decode_entrypoint_for_the_input_profile() {
        let profile_without_decode = vaapi(vec![
            profile("VAProfileH264High", &["VAEntrypointEncSlice"]),
            profile("VAProfileH264Main", &["VAEntrypointVLD"]),
        ]);
        assert!(profile_without_decode.supports(&context()).is_err());
    }

    #[test]
    fn scale_filter_preserves_width_height_order() {
        let profile = vaapi(vec![profile(
            "VAProfileH264High",
            &["VAEntrypointVLD", "VAEntrypointEncSlice"],
        )]);
        let args = profile.build(context()).unwrap();
        let filter = args
            .windows(2)
            .find(|pair| pair[0] == "-vf")
            .map(|pair| pair[1].as_str())
            .unwrap();
        assert!(filter.contains("scale_vaapi=1280:720"));
    }

    #[test]
    fn low_power_slice_encoding_is_a_valid_capability() {
        let profile = vaapi(vec![profile(
            "VAProfileH264High",
            &["VAEntrypointVLD", "VAEntrypointEncSliceLP"],
        )]);
        assert!(profile.is_enabled().is_ok());
        assert!(profile.supports(&context()).is_ok());
    }
}
