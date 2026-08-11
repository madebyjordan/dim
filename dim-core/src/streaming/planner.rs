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
    pub av1_main10_bt709_1080p24_6_3mbps_fmp4: bool,
}

impl Default for BrowserCapabilities {
    fn default() -> Self {
        Self {
            h264: true,
            aac: true,
            av1_main10_bt709_1080p24_6_3mbps_fmp4: false,
        }
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct VideoSource {
    pub codec: String,
    pub profile: Option<String>,
    pub pixel_format: Option<String>,
    pub level: Option<i64>,
    pub color_range: Option<String>,
    pub color_space: Option<String>,
    pub color_transfer: Option<String>,
    pub color_primaries: Option<String>,
    pub chroma_location: Option<String>,
    pub width: u64,
    pub height: u64,
    pub bitrate: u64,
    pub frame_rate: u64,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize)]
pub struct PlaybackPlan {
    pub preferred_strategy: PlaybackStrategy,
    pub direct_play_supported: bool,
    pub decision_reason: &'static str,
    pub renditions: Vec<Quality>,
}

pub fn plan_video(source: &VideoSource, capabilities: &BrowserCapabilities) -> PlaybackPlan {
    let h264_supported = capabilities.h264 && matches!(source.codec.as_str(), "h264" | "avc1");
    let verified_av1_supported = capabilities.av1_main10_bt709_1080p24_6_3mbps_fmp4
        && source.codec == "av1"
        && source.profile.as_deref() == Some("Main")
        && source.pixel_format.as_deref() == Some("yuv420p10le")
        && source.level == Some(8)
        && source.color_range.as_deref() == Some("tv")
        && source.color_space.as_deref() == Some("bt709")
        && source.color_transfer.as_deref() == Some("bt709")
        && source.color_primaries.as_deref() == Some("bt709")
        && source.chroma_location.as_deref() == Some("left")
        && source.width <= 1920
        && source.height <= 1080
        && source.bitrate <= 6_300_000
        && source.frame_rate <= 24;
    let direct_play_supported = h264_supported || verified_av1_supported;
    let renditions = source_bounded_qualities(source.height, source.bitrate)
        .into_iter()
        .filter(|quality| !direct_play_supported || quality.height < source.height)
        .collect();
    PlaybackPlan {
        preferred_strategy: if direct_play_supported {
            PlaybackStrategy::DirectPlay
        } else {
            PlaybackStrategy::Transcode
        },
        direct_play_supported,
        decision_reason: if h264_supported {
            "h264_browser_default"
        } else if verified_av1_supported {
            "client_verified_av1_main10_bt709_1080p24_fmp4"
        } else {
            "source_codec_not_verified_for_client"
        },
        renditions,
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
                profile: None,
                pixel_format: None,
                level: None,
                color_range: None,
                color_space: None,
                color_transfer: None,
                color_primaries: None,
                chroma_location: None,
                width: 1920,
                height: 1080,
                bitrate: 8_000_000,
                frame_rate: 30,
            },
            &BrowserCapabilities::default(),
        );
        assert_eq!(plan.preferred_strategy, PlaybackStrategy::DirectPlay);
        assert!(plan.direct_play_supported);
        assert_eq!(
            plan.renditions
                .iter()
                .map(|quality| quality.height)
                .collect::<Vec<_>>(),
            vec![720, 480]
        );
    }

    fn verified_av1_source() -> VideoSource {
        VideoSource {
            codec: "av1".into(),
            profile: Some("Main".into()),
            pixel_format: Some("yuv420p10le".into()),
            level: Some(8),
            color_range: Some("tv".into()),
            color_space: Some("bt709".into()),
            color_transfer: Some("bt709".into()),
            color_primaries: Some("bt709".into()),
            chroma_location: Some("left".into()),
            width: 1920,
            height: 1080,
            bitrate: 6_277_855,
            frame_rate: 24,
        }
    }

    #[test]
    fn av1_requires_explicit_client_evidence() {
        let plan = plan_video(&verified_av1_source(), &BrowserCapabilities::default());
        assert_eq!(plan.preferred_strategy, PlaybackStrategy::Transcode);
        assert_eq!(plan.decision_reason, "source_codec_not_verified_for_client");
    }

    #[test]
    fn exact_verified_av1_envelope_can_be_remuxed() {
        let mut capabilities = BrowserCapabilities::default();
        capabilities.av1_main10_bt709_1080p24_6_3mbps_fmp4 = true;
        let plan = plan_video(&verified_av1_source(), &capabilities);
        assert_eq!(plan.preferred_strategy, PlaybackStrategy::DirectPlay);
        assert_eq!(
            plan.decision_reason,
            "client_verified_av1_main10_bt709_1080p24_fmp4"
        );
        assert_eq!(
            plan.renditions
                .iter()
                .map(|quality| quality.height)
                .collect::<Vec<_>>(),
            vec![720, 480]
        );
    }

    #[test]
    fn keeps_source_resolution_transcode_when_direct_play_is_unavailable() {
        let mut source = verified_av1_source();
        source.codec = "hevc".into();
        let plan = plan_video(&source, &BrowserCapabilities::default());
        assert!(!plan.direct_play_supported);
        assert_eq!(
            plan.renditions
                .iter()
                .map(|quality| quality.height)
                .collect::<Vec<_>>(),
            vec![1080, 720, 480]
        );
    }

    #[test]
    fn verified_av1_capability_does_not_cover_hdr_or_higher_rate_sources() {
        let mut capabilities = BrowserCapabilities::default();
        capabilities.av1_main10_bt709_1080p24_6_3mbps_fmp4 = true;
        for source in [
            VideoSource {
                color_transfer: Some("smpte2084".into()),
                ..verified_av1_source()
            },
            VideoSource {
                frame_rate: 60,
                ..verified_av1_source()
            },
            VideoSource {
                bitrate: 6_300_001,
                ..verified_av1_source()
            },
        ] {
            assert!(!plan_video(&source, &capabilities).direct_play_supported);
        }
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
