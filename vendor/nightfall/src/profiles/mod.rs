#[cfg(windows)]
pub mod amf;
pub mod audio;
mod command;
#[cfg(all(target_os = "linux", feature = "cuda"))]
pub mod cuda;
pub mod subtitle;
#[cfg(all(target_os = "linux", feature = "vaapi"))]
pub mod vaapi;
pub mod video;

#[cfg(windows)]
pub use amf::AmfTranscodeProfile;
pub use audio::{AacTranscodeProfile, AudioTransmuxProfile};
pub use command::{
    CodecContract, FallbackSemantics, FfmpegCommand, Fmp4HlsRepresentation, FrameAlignment,
    HlsOutputPaths, OutputContainer, Representation,
};
#[cfg(all(target_os = "linux", feature = "cuda"))]
pub use cuda::CudaTranscodeProfile;
#[cfg(feature = "ssa_transmux")]
pub use subtitle::AssExtractProfile;
pub use subtitle::WebvttTranscodeProfile;
use tracing::debug;
use tracing::info;
use tracing::warn;
#[cfg(all(target_os = "linux", feature = "vaapi"))]
pub use vaapi::VaapiTranscodeProfile;
pub use video::H264TranscodeProfile;
pub use video::H264TransmuxProfile;
pub use video::RawVideoTranscodeProfile;

use crate::NightfallError;
use std::fmt::Debug;
use std::sync::{Arc, OnceLock};

static PROFILES: OnceLock<Vec<Box<dyn TranscodingProfile>>> = OnceLock::new();

pub fn profiles_init() {
    let profiles: Vec<Option<Box<dyn TranscodingProfile>>> = vec![
        Some(Box::new(AacTranscodeProfile)),
        Some(Box::new(AudioTransmuxProfile)),
        Some(Box::new(H264TranscodeProfile)),
        Some(Box::new(H264TransmuxProfile)),
        Some(Box::new(RawVideoTranscodeProfile)),
        Some(Box::new(WebvttTranscodeProfile)),
        #[cfg(feature = "ssa_transmux")]
        Some(Box::new(AssExtractProfile)),
        #[cfg(all(target_os = "linux", feature = "cuda"))]
        Some(Box::new(CudaTranscodeProfile)),
        #[cfg(all(target_os = "linux", feature = "vaapi"))]
        VaapiTranscodeProfile::new().map(|x| Box::new(x) as _),
        #[cfg(windows)]
        Some(Box::new(AmfTranscodeProfile)),
    ];

    let profiles = profiles.into_iter().flatten().collect::<Vec<_>>();

    let _ = PROFILES.set(
        profiles
            .into_iter()
            .filter(|x| {
                if let Err(e) = x.is_enabled() {
                    warn!(
                        profile = x.name(),
                        reason = %e,
                        "Disabling profile"
                    );

                    false
                } else {
                    info!(profile = x.name(), "Enabling profile");

                    true
                }
            })
            .collect(),
    );
}

pub fn get_active_profiles() -> Vec<&'static dyn TranscodingProfile> {
    PROFILES
        .get()
        .expect("nightfall::PROFILES not initialized.")
        .iter()
        .map(AsRef::as_ref)
        .collect()
}

pub fn get_profile_for(
    stream_type: StreamType,
    ctx: &ProfileContext,
) -> Vec<&'static dyn TranscodingProfile> {
    let mut profiles: Vec<_> = PROFILES
        .get()
        .expect("nightfall::PROFILES not initialized.")
        .iter()
        .filter(|x| {
            x.stream_type() == stream_type
                && if let Err(e) = x.supports(ctx) {
                    debug!(
                        profile = x.name(),
                        reason = %e,
                        "Profile not supported for ctx"
                    );

                    false
                } else {
                    true
                }
        })
        .map(AsRef::as_ref)
        .collect();

    profiles.sort_by_key(|x| x.profile_type());

    profiles
}

pub fn get_profile_for_with_type(
    stream_type: StreamType,
    profile_type: ProfileType,
    ctx: &ProfileContext,
) -> Vec<&'static dyn TranscodingProfile> {
    let mut profiles: Vec<_> = PROFILES
        .get()
        .expect("nightfall::PROFILES not initialized.")
        .iter()
        .filter(|x| {
            x.profile_type() == profile_type
                && x.stream_type() == stream_type
                && if let Err(e) = x.supports(ctx) {
                    debug!(
                        profile = x.name(),
                        reason = %e,
                        "Profile not supported for ctx"
                    );

                    false
                } else {
                    true
                }
        })
        .map(AsRef::as_ref)
        .collect();

    profiles.sort_by_key(|x| x.profile_type());

    profiles
}

pub trait TranscodingProfile: Debug + Send + Sync + 'static {
    /// Function must return what kind of profile it is.
    fn profile_type(&self) -> ProfileType;

    /// Function will return what type of stream this profile is for.
    fn stream_type(&self) -> StreamType;

    /// This function gets called at run-time to check whether this profile is enabled.
    /// By default this function is auto-implemented to return `true`, however for complex
    /// profiles such as VAAPI we may want at run-time to check whether ffmpeg will actually
    /// transcode the given file.
    fn is_enabled(&self) -> Result<(), NightfallError> {
        Ok(())
    }

    /// Build a typed FFmpeg command only after the profile and representation contracts have
    /// accepted the complete context.
    fn build(&self, ctx: ProfileContext) -> crate::Result<FfmpegCommand> {
        self.supports(&ctx)?;
        let representation = command::validate_command_context(self, &ctx)?;
        let args = self.build_args(&ctx, &representation);
        FfmpegCommand::new(ctx.ffmpeg_bin.clone(), args, representation)
    }

    /// Render the profile-specific portion of an already validated command.
    fn build_args(&self, ctx: &ProfileContext, representation: &Representation) -> Vec<String>;

    /// Function will return whether the conversion to `codec_out` is possible. Some
    /// implementations of this function (HWAccelerated profiles) will also check whether
    /// a direct conversion betwen`codec_in` and `codec_out` is possible.
    fn supports(&self, ctx: &ProfileContext) -> Result<(), NightfallError>;

    /// Return tag of this profile.
    fn tag(&self) -> &str;

    /// Return name of this profile.
    fn name(&self) -> &str;

    /// Function will return whether this profile emit data over stdout instead of progress information.
    fn is_stdio_stream(&self) -> bool {
        false
    }

    /// Declare the output container before raw FFmpeg arguments are assembled.
    fn output_container(&self, ctx: &ProfileContext) -> OutputContainer {
        match self.stream_type() {
            StreamType::Video | StreamType::Audio => OutputContainer::FragmentedMp4Hls,
            StreamType::Subtitle if ctx.output_ctx.codec == "ass" => OutputContainer::Ass,
            StreamType::Subtitle => OutputContainer::WebVtt,
        }
    }

    /// Hardware commands must name the verified software profile that remains in their fallback
    /// chain. Software and copy profiles carry explicit non-hardware semantics.
    fn fallback_semantics(&self) -> FallbackSemantics {
        match self.profile_type() {
            ProfileType::HardwareTranscode => FallbackSemantics::Hardware {
                software_profile_tag: "h264",
            },
            ProfileType::Transcode => FallbackSemantics::Software,
            ProfileType::Transmux => FallbackSemantics::NotApplicable,
        }
    }
}

/// A context which contains information we may need when building the ffmpeg arguments.
#[derive(Clone, Debug)]
pub struct ProfileContext {
    pub file: String,
    pub input_ctx: InputCtx,
    pub output_ctx: OutputCtx,
    pub ffmpeg_bin: String,
}

#[derive(Clone, Debug)]
pub struct InputCtx {
    pub stream: usize,
    pub audio_channels: u64,
    pub codec: String,
    pub pix_fmt: String,
    pub profile: String,
    pub bframes: Option<u64>,
    pub fps: f64,
    pub bitrate: u64,
    pub seek: Option<i64>,
}

impl Default for InputCtx {
    fn default() -> Self {
        Self {
            stream: 0,
            codec: String::new(),
            audio_channels: 2,
            pix_fmt: String::new(),
            profile: String::new(),
            bframes: None,
            fps: 0.0,
            bitrate: 0,
            seek: None,
        }
    }
}

#[derive(Clone, Debug)]
pub struct OutputCtx {
    pub codec: String,
    pub start_num: u32,
    pub outdir: String,
    pub max_to_transcode: Option<u64>,
    pub bitrate: Option<u64>,
    pub height: Option<i64>,
    pub width: Option<i64>,
    pub audio_channels: u64,
    pub audio_sample_rate: Option<u64>,
    pub audio_channel_layout: Option<String>,
    pub audio_filter: Option<String>,
    pub target_gop: u32,
    pub video_profile: Option<String>,
    pub video_level: Option<u64>,
    pub pixel_format: Option<String>,
    pub color_range: Option<String>,
    pub color_space: Option<String>,
    pub color_transfer: Option<String>,
    pub color_primaries: Option<String>,
    pub hdr_transfer: Option<String>,
    pub hdr_peak_nits: Option<f64>,
    /// Exact source duration used to bound representation output. Remote HLS manifests depend on
    /// this contract; integer container duration is not sufficiently precise.
    pub media_duration: Option<f64>,
    pub force_cfr: bool,
    pub segment_durations: Option<Arc<Vec<f64>>>,
    pub hls_segment_duration: Option<f64>,
}

impl Default for OutputCtx {
    fn default() -> Self {
        Self {
            codec: String::new(),
            start_num: 0,
            outdir: String::new(),
            max_to_transcode: None,
            bitrate: None,
            height: None,
            width: None,
            audio_channels: 2,
            audio_sample_rate: None,
            audio_channel_layout: None,
            audio_filter: None,
            target_gop: 5,
            video_profile: None,
            video_level: None,
            pixel_format: None,
            color_range: None,
            color_space: None,
            color_transfer: None,
            color_primaries: None,
            hdr_transfer: None,
            hdr_peak_nits: None,
            media_duration: None,
            force_cfr: false,
            segment_durations: None,
            hls_segment_duration: None,
        }
    }
}

impl OutputCtx {
    pub fn segment_duration(&self) -> f64 {
        self.hls_segment_duration.unwrap_or(self.target_gop as f64)
    }
}

impl Default for ProfileContext {
    fn default() -> Self {
        Self {
            file: String::new(),
            input_ctx: Default::default(),
            output_ctx: Default::default(),
            ffmpeg_bin: "ffmpeg".into(),
        }
    }
}

#[repr(u32)]
#[derive(Clone, Copy, Debug, PartialEq, Eq, Ord, PartialOrd)]
pub enum ProfileType {
    Transcode,
    Transmux,
    HardwareTranscode,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum StreamType {
    Video,
    Audio,
    Subtitle,
}

#[cfg(all(test, target_os = "macos", feature = "cuda"))]
mod macos_profile_tests {
    use super::*;

    #[test]
    fn enabling_the_cuda_feature_does_not_register_cuda_on_macos() {
        profiles_init();
        assert!(get_active_profiles()
            .iter()
            .all(|profile| profile.name() != "CudaTranscodeProfile"));
    }
}
