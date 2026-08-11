//! Source-aware playback planning.  This module deliberately contains no HTTP or process
//! management so decisions can be inspected without starting FFmpeg.

use super::{Quality, VIDEO_QUALITIES};
use serde::{Deserialize, Serialize};

#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum PlaybackStrategy {
    DirectPlay,
    Transcode,
}

#[derive(Clone, Debug, Default, Eq, PartialEq)]
pub struct BrowserCapabilities {
    pub video: Option<BrowserVideoCapability>,
}

#[derive(Clone, Debug, Deserialize, Serialize, Eq, PartialEq)]
pub struct BrowserVideoCapability {
    pub content_type: String,
    pub can_play_type: bool,
    pub media_source: bool,
    pub supported: bool,
    pub smooth: bool,
    pub power_efficient: Option<bool>,
    pub hdr_display: Option<bool>,
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
    pub codec_descriptor: Option<String>,
    pub remux_supported: bool,
    pub hdr: bool,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize)]
pub struct PlaybackPlan {
    pub preferred_strategy: PlaybackStrategy,
    pub direct_play_supported: bool,
    pub decision_reason: &'static str,
    pub renditions: Vec<Quality>,
}

pub fn plan_video(source: &VideoSource, capabilities: &BrowserCapabilities) -> PlaybackPlan {
    let expected_content_type = source
        .codec_descriptor
        .as_ref()
        .map(|codec| format!("video/mp4; codecs=\"{codec}\""));
    let capability = capabilities.video.as_ref();
    let exact_source_match = capability
        .zip(expected_content_type.as_ref())
        .is_some_and(|(capability, expected)| capability.content_type == *expected);
    let browser_decode_supported = capability.is_some_and(|capability| {
        capability.can_play_type
            && capability.media_source
            && capability.supported
            && capability.smooth
    });
    let hdr_output_supported =
        !source.hdr || capability.is_some_and(|capability| capability.hdr_display == Some(true));
    let direct_play_supported = source.remux_supported
        && exact_source_match
        && browser_decode_supported
        && hdr_output_supported;
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
        decision_reason: if direct_play_supported {
            "client_verified_source_and_fmp4_remux"
        } else if !source.remux_supported || source.codec_descriptor.is_none() {
            "source_not_fmp4_remux_eligible"
        } else if capability.is_none() {
            "client_capability_unavailable"
        } else if !exact_source_match {
            "client_capability_does_not_match_source"
        } else if !browser_decode_supported {
            "client_rejected_source_configuration"
        } else if !hdr_output_supported {
            "client_hdr_output_unavailable"
        } else {
            "source_not_verified_for_direct_play"
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

    fn av1_source(hdr: bool) -> VideoSource {
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
            codec_descriptor: Some("av01.0.08M.10.0.111.01.01.01.0".into()),
            remux_supported: true,
            hdr,
        }
    }

    fn supported(source: &VideoSource) -> BrowserCapabilities {
        BrowserCapabilities {
            video: Some(BrowserVideoCapability {
                content_type: format!(
                    "video/mp4; codecs=\"{}\"",
                    source.codec_descriptor.as_deref().unwrap()
                ),
                can_play_type: true,
                media_source: true,
                supported: true,
                smooth: true,
                power_efficient: Some(true),
                hdr_display: Some(true),
            }),
        }
    }

    #[test]
    fn av1_requires_explicit_client_evidence() {
        let plan = plan_video(&av1_source(false), &BrowserCapabilities::default());
        assert_eq!(plan.preferred_strategy, PlaybackStrategy::Transcode);
        assert_eq!(plan.decision_reason, "client_capability_unavailable");
    }

    #[test]
    fn exact_sdr_av1_source_can_be_remuxed() {
        let source = av1_source(false);
        let plan = plan_video(&source, &supported(&source));
        assert_eq!(plan.preferred_strategy, PlaybackStrategy::DirectPlay);
        assert_eq!(
            plan.decision_reason,
            "client_verified_source_and_fmp4_remux"
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
        let mut source = av1_source(false);
        source.codec = "hevc".into();
        source.codec_descriptor = None;
        source.remux_supported = false;
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
    fn exact_4k_hdr_av1_source_direct_plays_when_supported() {
        let source = VideoSource {
            width: 3840,
            height: 1600,
            bitrate: 11_618_576,
            level: Some(12),
            color_space: Some("bt2020nc".into()),
            color_transfer: Some("smpte2084".into()),
            color_primaries: Some("bt2020".into()),
            chroma_location: None,
            codec_descriptor: Some("av01.0.12M.10.0.110.09.16.09.0".into()),
            hdr: true,
            ..av1_source(true)
        };
        assert!(plan_video(&source, &supported(&source)).direct_play_supported);
    }

    #[test]
    fn unsupported_or_inconclusive_source_capability_falls_back() {
        let source = av1_source(false);
        let mut capabilities = supported(&source);
        capabilities.video.as_mut().unwrap().smooth = false;
        let plan = plan_video(&source, &capabilities);
        assert!(!plan.direct_play_supported);
        assert_eq!(plan.decision_reason, "client_rejected_source_configuration");
    }

    #[test]
    fn hdr_direct_play_requires_hdr_output_evidence() {
        let source = av1_source(true);
        let mut capabilities = supported(&source);
        capabilities.video.as_mut().unwrap().hdr_display = Some(false);
        let plan = plan_video(&source, &capabilities);
        assert!(!plan.direct_play_supported);
        assert_eq!(plan.decision_reason, "client_hdr_output_unavailable");
    }

    #[test]
    fn stale_capability_evidence_cannot_enable_another_source() {
        let source = av1_source(false);
        let mut capabilities = supported(&source);
        capabilities.video.as_mut().unwrap().content_type =
            "video/mp4; codecs=\"av01.0.12M.10\"".into();
        let plan = plan_video(&source, &capabilities);
        assert!(!plan.direct_play_supported);
        assert_eq!(
            plan.decision_reason,
            "client_capability_does_not_match_source"
        );
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
