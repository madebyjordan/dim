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
    pub static ref FFMPEG_BIN: &'static str = Box::leak(ffpath("utils/ffmpeg").into_boxed_str());
    pub static ref FFPROBE_BIN: &'static str = {
        cfg_if! {
            if #[cfg(test)] {
                "ffprobe"
            } else {
                Box::leak(ffpath("utils/ffprobe").into_boxed_str())
            }
        }
    };
}

use std::process::Command;

/// ffcheck - Check if "ffmpeg" and "ffprobe" are accessable through `std::process::Command`.
///
/// This will run `ffmpeg -version` and `ffprobe -version` and return a vec of the stdout
/// output if successfull or the binaries name if not.
///
/// # Example
///
/// ```ignore
/// use streaming::ffcheck;
///
/// for result in ffcheck() {
///     match result {
///         Ok(stdout) => println!("{:?}", stdout),
///         Err(program) => eprintln!("Failed to get the `-version` output of {:?}", program),
///     }
/// }
/// ```
pub fn ffcheck() -> Vec<Result<Box<str>, &'static str>> {
    let mut results = vec![];

    for program in [*FFMPEG_BIN, *FFPROBE_BIN].iter() {
        if let Ok(output) = Command::new(program).arg("-version").output() {
            let stdout = String::from_utf8(output.stdout)
                .expect("Failed to decode subprocess stdout.")
                .into_boxed_str();

            results.push(Ok(stdout));
        } else {
            results.push(Err(*program));
        }
    }

    results
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
    use super::get_avc1_tag;
    use nightfall::profiles::{
        AacTranscodeProfile, H264TranscodeProfile, H264TransmuxProfile, ProfileContext,
        TranscodingProfile,
    };

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
