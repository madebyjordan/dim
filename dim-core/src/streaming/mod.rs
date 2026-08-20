pub mod codec;
pub mod ffprobe;
pub mod planner;

use cfg_if::cfg_if;

use std::collections::HashMap;
use std::sync::Arc;
use std::sync::RwLock;

use crate::utils::ffpath;

lazy_static::lazy_static! {
    pub static ref STREAMING_SESSION: Arc<RwLock<HashMap<String, HashMap<String, String>>>> = Arc::new(RwLock::new(HashMap::new()));
    pub static ref FFMPEG_BIN: &'static str = Box::leak(ffpath(if cfg!(windows) { "utils/ffmpeg.exe" } else { "utils/ffmpeg" }).into_boxed_str());
    pub static ref FFPROBE_BIN: &'static str = {
        cfg_if! {
            if #[cfg(test)] {
                "ffprobe"
            } else {
                Box::leak(ffpath(if cfg!(windows) { "utils/ffprobe.exe" } else { "utils/ffprobe" }).into_boxed_str())
            }
        }
    };
}

use std::process::Command;

/// Check that Eclipse's provisioned FFmpeg tools execute and implement the
/// supported FFmpeg 9-or-newer command model.
///
/// This runs `ffmpeg -version` and `ffprobe -version`, returning each tool's
/// stdout on success or an actionable execution/identity/version diagnostic.
///
/// # Example
///
/// ```ignore
/// use streaming::ffcheck;
///
/// for result in ffcheck() {
///     match result {
///         Ok(stdout) => println!("{:?}", stdout),
///         Err(diagnostic) => eprintln!("Media tool validation failed: {diagnostic}"),
///     }
/// }
/// ```
pub fn ffcheck() -> Vec<Result<Box<str>, Box<str>>> {
    let mut results = vec![];

    for (program, identity) in [(*FFMPEG_BIN, "ffmpeg"), (*FFPROBE_BIN, "ffprobe")] {
        let output = match Command::new(program).arg("-version").output() {
            Ok(output) => output,
            Err(error) => {
                results.push(Err(format!(
                    "{program}: could not execute -version: {error}"
                )
                .into()));
                continue;
            }
        };
        let stdout = String::from_utf8_lossy(&output.stdout);
        let stderr = String::from_utf8_lossy(&output.stderr);
        if !output.status.success() {
            let diagnostic = stderr.lines().next().unwrap_or("no diagnostic output");
            results.push(Err(format!(
                "{program}: -version exited with {}: {diagnostic}",
                output.status
            )
            .into()));
            continue;
        }
        match supported_media_tool_major(&stdout, identity) {
            Ok(_) => results.push(Ok(stdout.into_owned().into_boxed_str())),
            Err(reason) => results.push(Err(format!("{program}: {reason}").into())),
        }
    }

    results
}

fn supported_media_tool_major(output: &str, identity: &str) -> Result<u32, String> {
    let first_line = output.lines().next().unwrap_or_default();
    let prefix = format!("{identity} version ");
    let version = first_line
        .strip_prefix(&prefix)
        .ok_or_else(|| format!("did not identify itself as {identity}"))?;
    let version = version.strip_prefix('n').unwrap_or(version);
    let major = version
        .split(|character: char| !character.is_ascii_digit())
        .next()
        .and_then(|value| value.parse::<u32>().ok())
        .ok_or_else(|| format!("did not report a numeric {identity} major version"))?;
    if major < 9 {
        return Err(format!(
            "major version {major} is unsupported; Eclipse requires FFmpeg 9 or newer"
        ));
    }
    Ok(major)
}

#[derive(Clone, Copy, Debug, Eq, PartialEq, serde::Serialize)]
pub struct Quality {
    pub height: u64,
    pub bitrate: u64,
}

pub fn get_qualities(height: u64, bitrate: u64) -> Vec<Quality> {
    planner::source_bounded_qualities(height, bitrate)
}

pub const VIDEO_QUALITIES: [Quality; 3] = [
    Quality {
        height: 1080,
        bitrate: 10_000_000,
    },
    Quality {
        height: 720,
        bitrate: 5_000_000,
    },
    Quality {
        height: 480,
        bitrate: 1_000_000,
    },
];

#[derive(Clone)]
pub struct Avc1Level {
    pub level: u64,
    pub macro_blocks_rate: u64,
    pub max_frame_size: u64,
    pub max_bitrate: u64,
}

impl ToString for Avc1Level {
    fn to_string(&self) -> String {
        format!("avc1.6400{:x}", self.level)
    }
}

pub fn level_to_tag(level: i64) -> Option<Avc1Level> {
    let level = level as u64;
    AVC1_LEVELS.iter().find(|&x| x.level == level).cloned()
}

pub fn get_avc1_tag(width: u64, height: u64, bitrate: u64, framerate: u64) -> Avc1Level {
    let macro_blocks = width.div_ceil(16) * height.div_ceil(16);
    let blocks_per_sec = macro_blocks.saturating_mul(framerate);

    let mut avc1_levels = AVC1_LEVELS.iter().filter(|&x| {
        x.max_bitrate >= bitrate
            && macro_blocks <= x.max_frame_size
            && blocks_per_sec <= x.macro_blocks_rate
    });

    avc1_levels
        .next()
        .cloned()
        .unwrap_or_else(|| AVC1_LEVELS.last().expect("AVC1 levels are defined").clone())
}

pub const AVC1_LEVELS: [Avc1Level; 20] = [
    Avc1Level {
        level: 9,
        macro_blocks_rate: 1_485,
        max_frame_size: 99,
        max_bitrate: 128_000,
    },
    Avc1Level {
        level: 10,
        macro_blocks_rate: 1_485,
        max_frame_size: 99,
        max_bitrate: 64_000,
    },
    Avc1Level {
        level: 11,
        macro_blocks_rate: 3_000,
        max_frame_size: 396,
        max_bitrate: 192_000,
    },
    Avc1Level {
        level: 12,
        macro_blocks_rate: 6_000,
        max_frame_size: 396,
        max_bitrate: 384_000,
    },
    Avc1Level {
        level: 13,
        macro_blocks_rate: 11_880,
        max_frame_size: 396,
        max_bitrate: 768_000,
    },
    Avc1Level {
        level: 20,
        macro_blocks_rate: 11_880,
        max_frame_size: 396,
        max_bitrate: 2_000_000,
    },
    Avc1Level {
        level: 21,
        macro_blocks_rate: 19_800,
        max_frame_size: 792,
        max_bitrate: 4_000_000,
    },
    Avc1Level {
        level: 22,
        macro_blocks_rate: 20_250,
        max_frame_size: 1_620,
        max_bitrate: 4_000_000,
    },
    Avc1Level {
        level: 30,
        macro_blocks_rate: 40_500,
        max_frame_size: 1_620,
        max_bitrate: 10_000_000,
    },
    Avc1Level {
        level: 31,
        macro_blocks_rate: 108_000,
        max_frame_size: 3600,
        max_bitrate: 14_000_000,
    },
    Avc1Level {
        level: 32,
        macro_blocks_rate: 216_000,
        max_frame_size: 5_120,
        max_bitrate: 20_000_000,
    },
    Avc1Level {
        level: 40,
        macro_blocks_rate: 245_760,
        max_frame_size: 8_192,
        max_bitrate: 20_000_000,
    },
    Avc1Level {
        level: 41,
        macro_blocks_rate: 245_760,
        max_frame_size: 8_192,
        max_bitrate: 50_000_000,
    },
    Avc1Level {
        level: 42,
        macro_blocks_rate: 522_240,
        max_frame_size: 8_704,
        max_bitrate: 50_000_000,
    },
    Avc1Level {
        level: 50,
        macro_blocks_rate: 589_824,
        max_frame_size: 22_080,
        max_bitrate: 135_000_000,
    },
    Avc1Level {
        level: 51,
        macro_blocks_rate: 983_040,
        max_frame_size: 36_864,
        max_bitrate: 240_000_000,
    },
    Avc1Level {
        level: 52,
        macro_blocks_rate: 2_073_600,
        max_frame_size: 36_864,
        max_bitrate: 240_000_000,
    },
    Avc1Level {
        level: 60,
        macro_blocks_rate: 4_177_920,
        max_frame_size: 139_264,
        max_bitrate: 240_000_000,
    },
    Avc1Level {
        level: 61,
        macro_blocks_rate: 8_355_840,
        max_frame_size: 139_264,
        max_bitrate: 480_000_000,
    },
    Avc1Level {
        level: 62,
        macro_blocks_rate: 16_711_680,
        max_frame_size: 139_264,
        max_bitrate: 800_000_000,
    },
];

#[cfg(test)]
mod compatibility_tests {
    use super::{get_avc1_tag, supported_media_tool_major};
    #[cfg(windows)]
    use nightfall::profiles::AmfTranscodeProfile;
    use nightfall::profiles::{
        AacTranscodeProfile, AudioTransmuxProfile, H264TranscodeProfile, H264TransmuxProfile,
        ProfileContext, TranscodingProfile, WebvttTranscodeProfile,
    };
    use std::path::{Path, PathBuf};
    use std::process::Command;

    #[test]
    fn runtime_rejects_legacy_ffmpeg_and_accepts_the_supported_baseline() {
        assert_eq!(
            supported_media_tool_major("ffmpeg version 9.0.1\n", "ffmpeg"),
            Ok(9)
        );
        assert!(
            supported_media_tool_major("ffprobe version 8.1\n", "ffprobe")
                .unwrap_err()
                .contains("major version 8 is unsupported")
        );
        assert!(supported_media_tool_major("not ffmpeg\n", "ffmpeg").is_err());
    }

    #[test]
    fn generated_profiles_use_ffmpeg_9_output_scoping() {
        let video_profiles = [
            H264TransmuxProfile
                .build(ProfileContext::default())
                .unwrap(),
            H264TranscodeProfile
                .build(ProfileContext::default())
                .unwrap(),
        ];
        for args in video_profiles {
            assert!(!args.iter().any(|arg| arg == "-vsync"));
            assert!(args.iter().any(|arg| arg == "-fps_mode:v:0"));
            assert!(args
                .windows(2)
                .any(|pair| pair == ["-hls_segment_type", "fmp4"]));
        }

        let audio_profiles = [
            AudioTransmuxProfile
                .build(ProfileContext::default())
                .unwrap(),
            AacTranscodeProfile
                .build(ProfileContext::default())
                .unwrap(),
        ];
        for args in audio_profiles {
            assert!(!args.iter().any(|arg| arg == "-vsync"));
            assert!(!args.iter().any(|arg| arg.starts_with("-fps_mode")));
            assert!(args
                .windows(2)
                .any(|pair| pair == ["-hls_segment_type", "fmp4"]));
        }
    }

    #[cfg(windows)]
    #[test]
    fn windows_amf_profile_uses_ffmpeg_9_output_scoping() {
        let args = AmfTranscodeProfile
            .build(ProfileContext::default())
            .unwrap();
        assert!(!args.iter().any(|arg| arg == "-vsync"));
        assert!(args
            .windows(2)
            .any(|pair| pair == ["-fps_mode:v:0", "passthrough"]));
    }

    fn acceptance_ffmpeg() -> PathBuf {
        Path::new(env!("CARGO_MANIFEST_DIR"))
            .parent()
            .unwrap()
            .join(if cfg!(windows) {
                "utils/ffmpeg.exe"
            } else {
                "utils/ffmpeg"
            })
    }

    fn run_ffmpeg(args: &[String]) {
        let output = Command::new(acceptance_ffmpeg())
            .args(args)
            .output()
            .expect("FFmpeg 9 acceptance process should start");
        assert!(
            output.status.success(),
            "FFmpeg failed:\n{}",
            String::from_utf8_lossy(&output.stderr)
        );
    }

    fn assert_fmp4_output(path: &Path) {
        let playlist = std::fs::read_to_string(path.join("playlist.m3u8")).unwrap();
        assert!(playlist.contains("#EXTM3U"));
        assert!(playlist.contains("#EXT-X-MAP"));
        assert!(playlist.contains(".m4s"));
        let entries = std::fs::read_dir(path)
            .unwrap()
            .filter_map(Result::ok)
            .collect::<Vec<_>>();
        assert!(entries.iter().any(|entry| {
            entry.file_name().to_string_lossy().ends_with("_init.mp4")
                && entry.metadata().unwrap().len() > 0
        }));
        assert!(entries.iter().any(|entry| {
            entry.path().extension().and_then(|ext| ext.to_str()) == Some("m4s")
                && entry.metadata().unwrap().len() > 0
        }));
    }

    #[test]
    #[ignore = "real FFmpeg 9 acceptance; run explicitly on provisioned release hosts"]
    fn ffmpeg9_profiles_produce_playable_fmp4_hls_and_subtitles() {
        let temp = tempfile::tempdir().unwrap();
        let source = temp.path().join("source.mp4");
        let fixture_args = vec![
            "-y".into(),
            "-f".into(),
            "lavfi".into(),
            "-i".into(),
            "testsrc2=size=640x360:rate=24".into(),
            "-f".into(),
            "lavfi".into(),
            "-i".into(),
            "sine=frequency=1000:sample_rate=48000".into(),
            "-t".into(),
            "12".into(),
            "-c:v".into(),
            "libx264".into(),
            "-pix_fmt".into(),
            "yuv420p".into(),
            "-g".into(),
            "120".into(),
            "-c:a".into(),
            "aac".into(),
            source.to_string_lossy().into_owned(),
        ];
        run_ffmpeg(&fixture_args);

        for (name, profile, stream, codec, output_codec, force_cfr, start_num) in [
            (
                "video-copy",
                &H264TransmuxProfile as &dyn TranscodingProfile,
                0,
                "h264",
                "h264",
                false,
                0,
            ),
            (
                "video-transcode",
                &H264TranscodeProfile as &dyn TranscodingProfile,
                0,
                "h264",
                "h264",
                true,
                0,
            ),
            (
                "video-seek",
                &H264TranscodeProfile as &dyn TranscodingProfile,
                0,
                "h264",
                "h264",
                false,
                1,
            ),
            (
                "audio-copy",
                &AudioTransmuxProfile as &dyn TranscodingProfile,
                1,
                "aac",
                "aac",
                false,
                0,
            ),
            (
                "audio-aac",
                &AacTranscodeProfile as &dyn TranscodingProfile,
                1,
                "aac",
                "aac",
                false,
                0,
            ),
        ] {
            let outdir = temp.path().join(name);
            std::fs::create_dir(&outdir).unwrap();
            let mut ctx = ProfileContext::default();
            ctx.file = source.to_string_lossy().into_owned();
            ctx.input_ctx.stream = stream;
            ctx.input_ctx.codec = codec.into();
            ctx.input_ctx.pix_fmt = "yuv420p".into();
            ctx.input_ctx.fps = 24.0;
            ctx.output_ctx.codec = output_codec.into();
            ctx.output_ctx.outdir = outdir.to_string_lossy().into_owned();
            ctx.output_ctx.start_num = start_num;
            ctx.output_ctx.force_cfr = force_cfr;
            ctx.output_ctx.media_duration = Some(6.0);
            ctx.output_ctx.audio_channels = 2;
            ctx.output_ctx.audio_sample_rate = Some(48_000);
            ctx.output_ctx.audio_channel_layout = Some("stereo".into());
            run_ffmpeg(&profile.build(ctx).unwrap());
            assert_fmp4_output(&outdir);
        }

        // Exercise Nightfall's existing HDR-to-SDR FFmpeg filter chain with
        // small deterministic LUTs; Session normally creates the production
        // 4096-entry LUTs before spawning this same generated command.
        let hdr_outdir = temp.path().join("hdr-to-sdr");
        std::fs::create_dir(&hdr_outdir).unwrap();
        let identity_lut =
            "TITLE \"identity\"\nLUT_1D_SIZE 2\nDOMAIN_MIN 0 0 0\nDOMAIN_MAX 1 1 1\n0 0 0\n1 1 1\n";
        std::fs::write(hdr_outdir.join("hdr_eotf.cube"), identity_lut).unwrap();
        std::fs::write(hdr_outdir.join("bt709_oetf.cube"), identity_lut).unwrap();
        let mut hdr_ctx = ProfileContext::default();
        hdr_ctx.file = source.to_string_lossy().into_owned();
        hdr_ctx.input_ctx.codec = "h264".into();
        hdr_ctx.input_ctx.pix_fmt = "yuv420p".into();
        hdr_ctx.input_ctx.fps = 24.0;
        hdr_ctx.output_ctx.codec = "h264".into();
        hdr_ctx.output_ctx.outdir = hdr_outdir.to_string_lossy().into_owned();
        hdr_ctx.output_ctx.hdr_transfer = Some("smpte2084".into());
        hdr_ctx.output_ctx.hdr_peak_nits = Some(1_000.0);
        hdr_ctx.output_ctx.pixel_format = Some("yuv420p".into());
        hdr_ctx.output_ctx.color_space = Some("bt709".into());
        hdr_ctx.output_ctx.color_transfer = Some("bt709".into());
        hdr_ctx.output_ctx.color_primaries = Some("bt709".into());
        hdr_ctx.output_ctx.media_duration = Some(3.0);
        run_ffmpeg(&H264TranscodeProfile.build(hdr_ctx).unwrap());
        assert_fmp4_output(&hdr_outdir);

        let subtitle = temp.path().join("subtitle.srt");
        std::fs::write(&subtitle, "1\n00:00:00,000 --> 00:00:01,000\nFFmpeg 9\n").unwrap();
        let mut subtitle_ctx = ProfileContext::default();
        subtitle_ctx.file = subtitle.to_string_lossy().into_owned();
        subtitle_ctx.input_ctx.codec = "subrip".into();
        subtitle_ctx.output_ctx.codec = "webvtt".into();
        let output = Command::new(acceptance_ffmpeg())
            .args(WebvttTranscodeProfile.build(subtitle_ctx).unwrap())
            .output()
            .unwrap();
        assert!(output.status.success());
        assert!(String::from_utf8_lossy(&output.stdout).contains("WEBVTT"));
    }

    #[cfg(windows)]
    #[test]
    #[ignore = "requires a Windows AMD GPU with an available AMF encoder"]
    fn ffmpeg9_amf_profile_produces_fmp4_hls() {
        let temp = tempfile::tempdir().unwrap();
        let source = temp.path().join("source.mp4");
        run_ffmpeg(&[
            "-y".into(),
            "-f".into(),
            "lavfi".into(),
            "-i".into(),
            "testsrc2=size=640x360:rate=24".into(),
            "-t".into(),
            "6".into(),
            "-c:v".into(),
            "libx264".into(),
            "-pix_fmt".into(),
            "yuv420p".into(),
            source.to_string_lossy().into_owned(),
        ]);
        let outdir = temp.path().join("amf");
        std::fs::create_dir(&outdir).unwrap();
        let mut ctx = ProfileContext::default();
        ctx.file = source.to_string_lossy().into_owned();
        ctx.input_ctx.codec = "h264".into();
        ctx.input_ctx.pix_fmt = "yuv420p".into();
        ctx.input_ctx.fps = 24.0;
        ctx.output_ctx.codec = "h264".into();
        ctx.output_ctx.outdir = outdir.to_string_lossy().into_owned();
        ctx.output_ctx.media_duration = Some(5.0);
        run_ffmpeg(&AmfTranscodeProfile.build(ctx).unwrap());
        assert_fmp4_output(&outdir);
    }

    #[test]
    fn nightfall_uses_the_supported_hls_segment_option() {
        for args in [
            H264TranscodeProfile
                .build(ProfileContext::default())
                .expect("H264 profile should generate FFmpeg arguments"),
            AacTranscodeProfile
                .build(ProfileContext::default())
                .expect("AAC profile should generate FFmpeg arguments"),
        ] {
            assert!(!args.iter().any(|arg| arg == "-hls_ts_options"));
            assert!(args.iter().any(|arg| arg == "-hls_segment_options"));
            assert!(args
                .iter()
                .any(|arg| arg == "movflags=frag_custom+dash+delay_moov"));
        }
    }

    #[test]
    fn preserves_discontinuity_flags_used_for_seeking() {
        let mut ctx = ProfileContext::default();
        ctx.output_ctx.start_num = 4;

        let args = H264TranscodeProfile
            .build(ctx)
            .expect("H264 seek profile should generate FFmpeg arguments");

        assert!(args
            .iter()
            .any(|arg| arg == "movflags=frag_custom+dash+delay_moov+frag_discont"));
    }

    #[test]
    fn h264_scale_filter_uses_width_then_height() {
        let mut ctx = ProfileContext::default();
        ctx.output_ctx.codec = "h264".into();
        ctx.output_ctx.width = Some(1920);
        ctx.output_ctx.height = Some(1080);

        let args = H264TranscodeProfile
            .build(ctx)
            .expect("H264 profile should generate FFmpeg arguments");

        assert!(args
            .windows(2)
            .any(|args| args[0] == "-vf" && args[1].starts_with("scale=1920:1080,")));
    }

    #[test]
    fn verified_av1_can_use_the_existing_fragmented_mp4_remux_profile() {
        let mut ctx = ProfileContext::default();
        ctx.input_ctx.codec = "av1".into();
        ctx.output_ctx.codec = "av1".into();

        assert!(H264TransmuxProfile.supports(&ctx).is_ok());
        let args = H264TransmuxProfile
            .build(ctx)
            .expect("AV1 remux profile should generate FFmpeg arguments");
        assert!(args.windows(2).any(|args| args == ["-c:0", "copy"]));
        assert!(args.iter().any(|arg| arg == "-hls_segment_type"));
    }

    #[test]
    fn h264_manifest_level_is_derived_from_the_target_rendition() {
        let level = get_avc1_tag(1920, 1080, 10_000_000, 24);
        assert_eq!(level.level, 40);
        assert_eq!(level.to_string(), "avc1.640028");
    }
}
