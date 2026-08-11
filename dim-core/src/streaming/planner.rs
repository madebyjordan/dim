//! Source-aware playback planning.  This module deliberately contains no HTTP or process
//! management so decisions can be inspected without starting FFmpeg.

use super::{Quality, VIDEO_QUALITIES};
use serde::Serialize;

#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum PlaybackStrategy {
    DirectPlay,
    Transcode,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct BrowserCapabilities {
    pub h264: bool,
    pub aac: bool,
}

impl Default for BrowserCapabilities {
    fn default() -> Self {
        Self {
            h264: true,
            aac: true,
        }
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct VideoSource {
    pub codec: String,
    pub height: u64,
    pub bitrate: u64,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize)]
pub struct PlaybackPlan {
    pub preferred_strategy: PlaybackStrategy,
    pub direct_play_supported: bool,
    pub renditions: Vec<Quality>,
}

pub fn plan_video(source: &VideoSource, capabilities: &BrowserCapabilities) -> PlaybackPlan {
    let direct_play_supported =
        capabilities.h264 && matches!(source.codec.as_str(), "h264" | "avc1");
    PlaybackPlan {
        preferred_strategy: if direct_play_supported {
            PlaybackStrategy::DirectPlay
        } else {
            PlaybackStrategy::Transcode
        },
        direct_play_supported,
        renditions: source_bounded_qualities(source.height, source.bitrate),
    }
}

pub fn source_bounded_qualities(height: u64, bitrate: u64) -> Vec<Quality> {
    let mut qualities = VIDEO_QUALITIES
        .iter()
        .filter(|quality| quality.height <= height)
        .map(|quality| Quality {
            height: quality.height,
            bitrate: quality.bitrate.min(bitrate),
        })
        .collect::<Vec<_>>();

    // Very small sources still need one usable transcode choice.  It is always source-sized.
    if qualities.is_empty() && height > 0 {
        qualities.push(Quality { height, bitrate });
    }
    qualities
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn prefers_direct_play_for_browser_compatible_h264() {
        let plan = plan_video(
            &VideoSource {
                codec: "h264".into(),
                height: 1080,
                bitrate: 8_000_000,
            },
            &BrowserCapabilities::default(),
        );
        assert_eq!(plan.preferred_strategy, PlaybackStrategy::DirectPlay);
        assert!(plan.direct_play_supported);
    }

    #[test]
    fn never_advertises_an_upscaled_rendition() {
        let qualities = source_bounded_qualities(576, 2_000_000);
        assert_eq!(
            qualities.iter().map(|q| q.height).collect::<Vec<_>>(),
            vec![480]
        );
        assert!(qualities
            .iter()
            .all(|q| q.height <= 576 && q.bitrate <= 2_000_000));
    }

    #[test]
    fn preserves_a_sub_480p_source_without_upscaling() {
        assert_eq!(source_bounded_qualities(360, 700_000)[0].height, 360);
    }
}
