use super::{ProfileContext, ProfileType, StreamType, TranscodingProfile};
use crate::{NightfallError, Result};
use std::ops::Deref;
use std::path::{Path, PathBuf};

const NANOS_PER_SECOND: u64 = 1_000_000_000;
const MICROS_PER_SECOND: u64 = 1_000_000;
const MAX_AUDIO_CHANNELS: u64 = 64;

/// The container and delivery contract produced by one FFmpeg invocation.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum OutputContainer {
    FragmentedMp4Hls,
    RawVideo,
    WebVtt,
    Ass,
}

/// Whether FFmpeg copies the source codec or invokes an encoder.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum CodecContract {
    Copy { codec: String },
    Encode { codec: String },
}

/// The fallback promise attached to a generated command.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum FallbackSemantics {
    Software,
    Hardware { software_profile_tag: &'static str },
    NotApplicable,
}

/// Frame/timescale alignment retained alongside the rendered FFmpeg arguments.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum FrameAlignment {
    Passthrough {
        nominal_frames_per_segment: u64,
        frame_rate_nanos: u64,
    },
    Constant {
        frames_per_segment: u64,
        frame_rate_nanos: u64,
    },
    Audio,
    NotApplicable,
}

/// Validated output paths for an fMP4 HLS representation.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct HlsOutputPaths {
    pub directory: PathBuf,
    pub playlist: PathBuf,
    pub segment_pattern: PathBuf,
    pub init_filename: String,
}

/// A validated representation timeline. Integer time units prevent later lossy or overflowing
/// float-to-duration casts in the session and command layers.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Fmp4HlsRepresentation {
    pub stream_type: StreamType,
    pub codec: CodecContract,
    pub fallback: FallbackSemantics,
    pub start_number: u32,
    pub seek_micros: u64,
    pub window_micros: Option<u64>,
    pub segment_duration_nanos: u64,
    pub frame_alignment: FrameAlignment,
    pub paths: HlsOutputPaths,
}

impl Fmp4HlsRepresentation {
    pub fn seek_seconds(&self) -> String {
        format_scaled(self.seek_micros, MICROS_PER_SECOND, 6)
    }

    pub fn segment_duration_seconds(&self) -> String {
        format_scaled(self.segment_duration_nanos, NANOS_PER_SECOND, 9)
    }

    pub fn window_seconds(&self) -> Option<String> {
        self.window_micros
            .map(|window| format_scaled(window, MICROS_PER_SECOND, 6))
    }

    pub fn discontinuity_movflags(&self) -> &'static str {
        if self.start_number > 0 {
            "movflags=frag_custom+dash+delay_moov+frag_discont"
        } else {
            "movflags=frag_custom+dash+delay_moov"
        }
    }
}

/// The typed representation associated with a generated FFmpeg command.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Representation {
    Fmp4Hls(Fmp4HlsRepresentation),
    Stdout {
        stream_type: StreamType,
        container: OutputContainer,
        codec: CodecContract,
    },
}

/// A fully validated command. Construction is crate-private so callers cannot detach raw
/// arguments from the representation invariants that produced them.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct FfmpegCommand {
    executable: String,
    args: Vec<String>,
    representation: Representation,
}

impl FfmpegCommand {
    pub(crate) fn new(
        executable: String,
        args: Vec<String>,
        representation: Representation,
    ) -> Result<Self> {
        validate_text(&executable, "FFmpeg executable")?;
        if args.is_empty() {
            return Err(invalid("FFmpeg command has no arguments"));
        }
        if args.iter().any(|argument| argument.contains('\0')) {
            return Err(invalid("FFmpeg argument contains a NUL byte"));
        }
        Ok(Self {
            executable,
            args,
            representation,
        })
    }

    pub fn executable(&self) -> &str {
        &self.executable
    }

    pub fn args(&self) -> &[String] {
        &self.args
    }

    pub fn representation(&self) -> &Representation {
        &self.representation
    }
}

impl Deref for FfmpegCommand {
    type Target = [String];

    fn deref(&self) -> &Self::Target {
        self.args()
    }
}

impl IntoIterator for FfmpegCommand {
    type Item = String;
    type IntoIter = std::vec::IntoIter<String>;

    fn into_iter(self) -> Self::IntoIter {
        self.args.into_iter()
    }
}

impl<'a> IntoIterator for &'a FfmpegCommand {
    type Item = &'a String;
    type IntoIter = std::slice::Iter<'a, String>;

    fn into_iter(self) -> Self::IntoIter {
        self.args.iter()
    }
}

pub(crate) fn validate_command_context(
    profile: &(impl TranscodingProfile + ?Sized),
    ctx: &ProfileContext,
) -> Result<Representation> {
    validate_text(&ctx.file, "input path")?;
    validate_text(&ctx.ffmpeg_bin, "FFmpeg executable")?;
    validate_optional_positive(ctx.output_ctx.media_duration, "media duration")?;
    if let Some(peak) = ctx.output_ctx.hdr_peak_nits {
        if !peak.is_finite() || !(100.0..=10_000.0).contains(&peak) {
            return Err(invalid(
                "HDR peak luminance must be finite and within 100..=10000 nits",
            ));
        }
    }
    if let Some(bitrate) = ctx.output_ctx.bitrate {
        if bitrate == 0 || bitrate > u64::MAX / 3 {
            return Err(invalid(
                "output bitrate is zero or overflows rate-control arithmetic",
            ));
        }
    }
    if let Some(sample_rate) = ctx.output_ctx.audio_sample_rate {
        if sample_rate == 0 || sample_rate > u32::MAX.into() {
            return Err(invalid(
                "audio sample rate is outside FFmpeg's supported range",
            ));
        }
    }
    if ctx.output_ctx.max_to_transcode == Some(0) {
        return Err(invalid("maximum transcode window must be positive"));
    }

    let container = profile.output_container(ctx);
    let codec = match profile.profile_type() {
        ProfileType::Transmux => CodecContract::Copy {
            codec: ctx.output_ctx.codec.clone(),
        },
        ProfileType::Transcode | ProfileType::HardwareTranscode => CodecContract::Encode {
            codec: ctx.output_ctx.codec.clone(),
        },
    };
    validate_text(&ctx.output_ctx.codec, "output codec")?;
    for (name, value) in [
        (
            "audio channel layout",
            ctx.output_ctx.audio_channel_layout.as_deref(),
        ),
        ("audio filter", ctx.output_ctx.audio_filter.as_deref()),
        ("video profile", ctx.output_ctx.video_profile.as_deref()),
        ("pixel format", ctx.output_ctx.pixel_format.as_deref()),
        ("color range", ctx.output_ctx.color_range.as_deref()),
        ("color space", ctx.output_ctx.color_space.as_deref()),
        ("color transfer", ctx.output_ctx.color_transfer.as_deref()),
        ("color primaries", ctx.output_ctx.color_primaries.as_deref()),
        ("HDR transfer", ctx.output_ctx.hdr_transfer.as_deref()),
    ] {
        if let Some(value) = value {
            validate_text(value, name)?;
        }
    }
    if ctx.output_ctx.video_level.is_some_and(|level| level > 62) {
        return Err(invalid("H.264 output level exceeds 6.2"));
    }

    if container != OutputContainer::FragmentedMp4Hls {
        validate_dimensions(ctx, profile.stream_type())?;
        return Ok(Representation::Stdout {
            stream_type: profile.stream_type(),
            container,
            codec,
        });
    }

    let outdir = Path::new(&ctx.output_ctx.outdir);
    validate_text(&ctx.output_ctx.outdir, "output directory")?;
    if outdir.as_os_str().is_empty() {
        return Err(invalid("output directory is empty"));
    }
    validate_dimensions(ctx, profile.stream_type())?;
    if ctx.output_ctx.target_gop == 0 {
        return Err(invalid("GOP/segment cadence must be positive"));
    }
    if profile.stream_type() == StreamType::Audio
        && !(1..=MAX_AUDIO_CHANNELS).contains(&ctx.output_ctx.audio_channels)
    {
        return Err(invalid(format!(
            "audio channel count {} is outside 1..={MAX_AUDIO_CHANNELS}",
            ctx.output_ctx.audio_channels
        )));
    }

    let segment_duration = ctx.output_ctx.segment_duration();
    let segment_duration_nanos = scaled_time(
        segment_duration,
        NANOS_PER_SECOND,
        "HLS segment duration",
        false,
    )?;
    if segment_duration_nanos < 1_000 {
        return Err(invalid(
            "HLS segment duration is lost at FFmpeg's microsecond muxer timescale",
        ));
    }
    let seek_micros = validated_seek_micros(ctx)?;
    let window_micros = if let Some(duration) = ctx.output_ctx.media_duration {
        let duration_micros = scaled_time(duration, MICROS_PER_SECOND, "media duration", false)?;
        if seek_micros >= duration_micros {
            return Err(invalid(format!(
                "seek time {} is outside media duration {}",
                format_scaled(seek_micros, MICROS_PER_SECOND, 6),
                format_scaled(duration_micros, MICROS_PER_SECOND, 6)
            )));
        }
        Some(duration_micros - seek_micros)
    } else {
        None
    };

    let frame_alignment = match profile.stream_type() {
        StreamType::Video => {
            let fps = ctx.input_ctx.fps;
            if !fps.is_finite() || fps <= 0.0 {
                return Err(invalid("video frame rate must be finite and positive"));
            }
            let frame_rate_nanos = scaled_time(fps, NANOS_PER_SECOND, "video frame rate", false)?;
            let frames = segment_duration * fps;
            if !frames.is_finite() || frames <= 0.0 || frames > i32::MAX as f64 {
                return Err(invalid("video segment frame cadence overflows"));
            }
            let rounded = frames.round();
            if ctx.output_ctx.force_cfr {
                let tolerance = fps.abs().max(1.0) * 1e-9;
                if (frames - rounded).abs() > tolerance {
                    return Err(invalid(format!(
                        "segment duration {segment_duration:.9}s does not align to whole CFR frames at {fps:.12} fps"
                    )));
                }
                FrameAlignment::Constant {
                    frames_per_segment: rounded as u64,
                    frame_rate_nanos,
                }
            } else {
                FrameAlignment::Passthrough {
                    nominal_frames_per_segment: rounded as u64,
                    frame_rate_nanos,
                }
            }
        }
        StreamType::Audio => FrameAlignment::Audio,
        StreamType::Subtitle => {
            return Err(invalid("subtitle output cannot use fragmented MP4 HLS"));
        }
    };

    let init_filename = format!("{}_init.mp4", ctx.output_ctx.start_num);
    let paths = HlsOutputPaths {
        directory: outdir.to_path_buf(),
        playlist: outdir.join("playlist.m3u8"),
        segment_pattern: outdir.join("%d.m4s"),
        init_filename,
    };
    Ok(Representation::Fmp4Hls(Fmp4HlsRepresentation {
        stream_type: profile.stream_type(),
        codec,
        fallback: profile.fallback_semantics(),
        start_number: ctx.output_ctx.start_num,
        seek_micros,
        window_micros,
        segment_duration_nanos,
        frame_alignment,
        paths,
    }))
}

fn validate_dimensions(ctx: &ProfileContext, stream_type: StreamType) -> Result<()> {
    let width = ctx.output_ctx.width;
    let height = ctx.output_ctx.height;
    if stream_type != StreamType::Video && (width.is_some() || height.is_some()) {
        return Err(invalid(
            "non-video representation contains video dimensions",
        ));
    }
    if width.is_some() && height.is_none() {
        return Err(invalid(
            "output width cannot be applied without output height",
        ));
    }
    for (name, value) in [("width", width), ("height", height)] {
        let Some(value) = value else { continue };
        if name == "width" && value == -2 {
            continue;
        }
        if value <= 0 || value > i32::MAX as i64 {
            return Err(invalid(format!(
                "output {name} {value} is negative, zero, or exceeds FFmpeg's supported range"
            )));
        }
        if ctx.output_ctx.codec == "h264" && value % 2 != 0 {
            return Err(invalid(format!(
                "H.264 output {name} {value} is not aligned to a 4:2:0 pixel pair"
            )));
        }
    }
    Ok(())
}

fn validated_seek_micros(ctx: &ProfileContext) -> Result<u64> {
    if let Some(durations) = ctx.output_ctx.segment_durations.as_deref() {
        if durations.is_empty() {
            return Err(invalid("segment duration timeline is empty"));
        }
        if ctx.output_ctx.start_num as usize >= durations.len() {
            return Err(invalid(format!(
                "start segment {} is outside a {}-segment timeline",
                ctx.output_ctx.start_num,
                durations.len()
            )));
        }
        let expected_nanos = scaled_time(
            ctx.output_ctx.segment_duration(),
            NANOS_PER_SECOND,
            "HLS segment duration",
            false,
        )?;
        let mut seek_nanos = 0_u64;
        let mut timeline_nanos = 0_u64;
        for (index, duration) in durations.iter().enumerate() {
            let duration_nanos = scaled_time(
                *duration,
                NANOS_PER_SECOND,
                &format!("segment duration at index {index}"),
                false,
            )?;
            if index + 1 < durations.len() && duration_nanos != expected_nanos {
                return Err(invalid(format!(
                    "segment duration at index {index} does not match the representation cadence"
                )));
            }
            if index < ctx.output_ctx.start_num as usize {
                seek_nanos = seek_nanos
                    .checked_add(duration_nanos)
                    .ok_or_else(|| invalid("segment timeline seek arithmetic overflowed"))?;
            }
            timeline_nanos = timeline_nanos
                .checked_add(duration_nanos)
                .ok_or_else(|| invalid("segment timeline duration overflowed"))?;
        }
        let _ = timeline_nanos;
        return seek_nanos
            .checked_add(500)
            .map(|rounded| rounded / 1_000)
            .ok_or_else(|| invalid("segment timeline seek rounding overflowed"));
    }

    u64::from(ctx.output_ctx.start_num)
        .checked_mul(u64::from(ctx.output_ctx.target_gop))
        .and_then(|seconds| seconds.checked_mul(MICROS_PER_SECOND))
        .ok_or_else(|| invalid("start segment seek arithmetic overflowed"))
}

fn validate_optional_positive(value: Option<f64>, name: &str) -> Result<()> {
    if let Some(value) = value {
        if !value.is_finite() || value <= 0.0 {
            return Err(invalid(format!("{name} must be finite and positive")));
        }
    }
    Ok(())
}

fn validate_text(value: &str, name: &str) -> Result<()> {
    if value.trim().is_empty() || value.contains('\0') {
        return Err(invalid(format!("{name} is empty or contains a NUL byte")));
    }
    Ok(())
}

fn scaled_time(value: f64, scale: u64, name: &str, allow_zero: bool) -> Result<u64> {
    if !value.is_finite() || value < 0.0 || (!allow_zero && value == 0.0) {
        return Err(invalid(format!("{name} must be finite and positive")));
    }
    let scaled = value * scale as f64;
    if !scaled.is_finite() || scaled > u64::MAX as f64 {
        return Err(invalid(format!("{name} exceeds the supported timescale")));
    }
    let rounded = scaled.round();
    if !allow_zero && rounded < 1.0 {
        return Err(invalid(format!(
            "{name} is lost at the supported timescale"
        )));
    }
    Ok(rounded as u64)
}

fn format_scaled(value: u64, scale: u64, precision: usize) -> String {
    format!(
        "{}.{:0precision$}",
        value / scale,
        value % scale,
        precision = precision
    )
}

fn invalid(message: impl Into<String>) -> NightfallError {
    NightfallError::InvalidContext(message.into())
}

#[derive(Clone, Copy, Debug, Default)]
pub(crate) struct HlsMuxOptions {
    pub force_key_frames: bool,
    pub independent_segments: bool,
    pub disable_scene_change: bool,
}

/// Append the one canonical fMP4/HLS muxing contract shared by all audio, software-video, and
/// hardware-video profiles.
pub(crate) fn append_fmp4_hls_output(
    args: &mut Vec<String>,
    representation: &Representation,
    options: HlsMuxOptions,
) {
    let Representation::Fmp4Hls(hls) = representation else {
        unreachable!("fMP4/HLS arguments require an HLS representation")
    };
    args.extend([
        "-f".into(),
        "hls".into(),
        "-start_number".into(),
        hls.start_number.to_string(),
    ]);
    if options.independent_segments {
        args.extend(["-hls_flags".into(), "independent_segments".into()]);
    }
    args.extend([
        "-hls_flags".into(),
        "temp_file".into(),
        "-max_delay".into(),
        "5000000".into(),
        "-hls_segment_options".into(),
        hls.discontinuity_movflags().into(),
        "-hls_fmp4_init_filename".into(),
        hls.paths.init_filename.clone(),
        "-hls_time".into(),
        hls.segment_duration_seconds(),
    ]);
    if options.force_key_frames {
        args.extend([
            "-force_key_frames".into(),
            format!("expr:gte(t,n_forced*{})", hls.segment_duration_seconds()),
        ]);
    }
    if options.disable_scene_change {
        args.extend(["-sc_threshold:v:0".into(), "0".into()]);
    }
    args.extend([
        "-hls_segment_type".into(),
        "fmp4".into(),
        "-loglevel".into(),
        "info".into(),
        "-progress".into(),
        "pipe:1".into(),
        "-hls_segment_filename".into(),
        hls.paths.segment_pattern.to_string_lossy().into_owned(),
        hls.paths.playlist.to_string_lossy().into_owned(),
    ]);
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::profiles::{H264TranscodeProfile, InputCtx, OutputCtx};
    use std::sync::Arc;

    fn context() -> ProfileContext {
        ProfileContext {
            file: "source.mp4".into(),
            input_ctx: InputCtx {
                codec: "h264".into(),
                pix_fmt: "yuv420p".into(),
                fps: 24.0,
                ..Default::default()
            },
            output_ctx: OutputCtx {
                codec: "h264".into(),
                outdir: "output".into(),
                width: Some(1280),
                height: Some(720),
                ..Default::default()
            },
            ..Default::default()
        }
    }

    #[test]
    fn representation_retains_command_contract_and_paths() {
        let command = H264TranscodeProfile.build(context()).unwrap();
        let Representation::Fmp4Hls(hls) = command.representation() else {
            panic!("expected HLS representation")
        };
        assert_eq!(hls.seek_micros, 0);
        assert_eq!(hls.window_micros, None);
        assert_eq!(hls.segment_duration_nanos, 5_000_000_000);
        assert_eq!(hls.paths.playlist, Path::new("output/playlist.m3u8"));
        assert_eq!(
            hls.codec,
            CodecContract::Encode {
                codec: "h264".into()
            }
        );
        assert_eq!(hls.fallback, FallbackSemantics::Software);
    }

    #[test]
    fn rejects_invalid_or_overflowing_context_before_rendering_arguments() {
        for invalid_fps in [f64::NAN, f64::INFINITY, -1.0, 0.0] {
            let mut ctx = context();
            ctx.input_ctx.fps = invalid_fps;
            assert!(matches!(
                H264TranscodeProfile.build(ctx),
                Err(NightfallError::InvalidContext(_))
            ));
        }

        let mut ctx = context();
        ctx.output_ctx.width = Some(i64::MAX);
        assert!(H264TranscodeProfile.build(ctx).is_err());

        let mut ctx = context();
        ctx.output_ctx.segment_durations = Some(Arc::new(vec![5.0]));
        ctx.output_ctx.start_num = 1;
        assert!(H264TranscodeProfile.build(ctx).is_err());

        let mut ctx = context();
        ctx.output_ctx.segment_durations = Some(Arc::new(vec![5.0, f64::NAN]));
        assert!(H264TranscodeProfile.build(ctx).is_err());

        let mut ctx = context();
        ctx.output_ctx.start_num = u32::MAX;
        ctx.output_ctx.target_gop = u32::MAX;
        assert!(H264TranscodeProfile.build(ctx).is_err());
    }

    #[test]
    fn rejects_non_frame_aligned_cfr_cadence() {
        let mut ctx = context();
        ctx.output_ctx.force_cfr = true;
        ctx.output_ctx.hls_segment_duration = Some(5.01);
        assert!(matches!(
            H264TranscodeProfile.build(ctx),
            Err(NightfallError::InvalidContext(message)) if message.contains("whole CFR frames")
        ));
    }

    #[test]
    fn seek_and_remaining_window_use_checked_representation_time() {
        let mut ctx = context();
        ctx.output_ctx.start_num = 2;
        ctx.output_ctx.segment_durations = Some(Arc::new(vec![5.005, 5.005, 1.99]));
        ctx.output_ctx.hls_segment_duration = Some(5.005);
        ctx.output_ctx.media_duration = Some(12.0);
        let command = H264TranscodeProfile.build(ctx).unwrap();
        let Representation::Fmp4Hls(hls) = command.representation() else {
            panic!("expected HLS representation")
        };
        assert_eq!(hls.seek_seconds(), "10.010000");
        assert_eq!(hls.window_seconds().as_deref(), Some("1.990000"));
    }

    #[test]
    fn retired_adapter_dependencies_do_not_return() {
        let manifest = include_str!("../../Cargo.toml");
        for retired in [
            "err-derive",
            "serde_derive",
            "xtra =",
            "xtra_proc",
            "async-trait",
            "tokio-stream",
            "psutil",
            "ntapi",
            "winapi",
        ] {
            assert!(
                !manifest.contains(retired),
                "retired Nightfall dependency returned: {retired}"
            );
        }
        assert!(manifest.contains("edition = \"2021\""));
        assert!(manifest.contains("thiserror = \"2\""));
        assert!(manifest.contains("windows-sys"));
    }
}
