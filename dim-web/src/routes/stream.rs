use crate::{error::DimErrorWrapper, AppState};
use axum::body::Body;
use axum::extract::{Path, Query, State};
use axum::response::{IntoResponse, Json, Response};
use axum::Extension;
use dim_core::core::StateManager;
use dim_core::stream_tracking::{
    ContentType, PlannedProfile, PlannedTrack, RemotePlaybackState, StreamTracking, VirtualManifest,
};
use dim_core::streaming::codec::{
    audio_capability_request, audio_codec_descriptor, audio_remux_supported, capability_request,
    codec_descriptor, has_exact_codec_configuration, hdr_peak_nits, is_hdr, remux_supported,
};
use dim_core::streaming::ffprobe::{FFPStream, FFProbeCtx};
use dim_core::streaming::get_avc1_tag;
use dim_core::streaming::planner::{
    plan_audio_for_target, plan_video_for_target, AudioAction, AudioSource, BrowserCapabilities,
    BrowserVideoCapability, PlaybackPlan, PlaybackTargetKind, VideoSource,
};
use dim_core::utils::{bitrate_to_label, quality_to_label};
use dim_database::mediafile::MediaFile;
use dim_database::user::{DefaultVideoQuality, User, UserSettings};
use futures::stream;
use futures::StreamExt;
use http::{header, HeaderMap, StatusCode};
use nightfall::error::NightfallError;
use nightfall::profiles::{OutputCtx, ProfileContext};
use serde::Deserialize;
use serde_json::json;
use std::future::Future;
use std::path::{self, PathBuf};
use std::time::{Duration, UNIX_EPOCH};
use tokio::fs::File;
use tokio::io::{AsyncReadExt, AsyncSeekExt};
use tokio_util::io::ReaderStream;
use uuid::Uuid;

#[derive(Deserialize)]
pub struct VirtualManifestParams {
    gid: Option<String>,
    #[serde(default)]
    force_ass: bool,
    #[serde(default)]
    video_capability: Option<String>,
    #[serde(default)]
    capabilities: Option<String>,
    #[serde(default)]
    target: PlaybackTargetKind,
}

pub async fn return_virtual_manifest(
    State(AppState {
        conn,
        stream_tracking,
        ..
    }): State<AppState>,
    Path(id): Path<i64>,
    Query(params): Query<VirtualManifestParams>,
    Extension(user): Extension<User>,
) -> Result<impl IntoResponse, DimErrorWrapper> {
    let owner = user.id.get();
    if let Some(gid) = params.gid.and_then(|value| Uuid::parse_str(&value).ok()) {
        return Ok(Json(json!({ "tracks": stream_tracking.inspect(&gid, owner).await?, "gid": gid.to_string() })).into_response());
    }

    let mut tx = conn.read().begin().await?;
    let media = MediaFile::get_one(&mut tx, id)
        .await
        .map_err(|error| dim_core::errors::StreamingErrors::NoMediaFileFound(error.to_string()))?;
    if !path::Path::new(&media.target_file).exists() {
        return Err(dim_core::errors::StreamingErrors::FileDoesNotExist.into());
    }

    let (info, reused_probe) = load_probe(&media).await?;
    let capabilities = params
        .capabilities
        .as_deref()
        .and_then(|value| serde_json::from_str::<BrowserCapabilities>(value).ok())
        .unwrap_or_else(|| BrowserCapabilities {
            video: params
                .video_capability
                .as_deref()
                .and_then(|value| serde_json::from_str::<BrowserVideoCapability>(value).ok()),
            audio: Vec::new(),
        });
    let target = params.target;
    let (tracks, plan) = build_tracks(
        &info,
        &media,
        &user.prefs,
        params.force_ass,
        &capabilities,
        target,
    )?;
    let gid = Uuid::new_v4();
    stream_tracking.create_session(gid, owner, tracks).await;
    let remote = if target == PlaybackTargetKind::Airplay {
        let token = stream_tracking.enable_remote_access(&gid, owner).await?;
        Some(json!({
            "kind": "airplay",
            "url": format!("/api/v1/remote/{gid}/master.m3u8?token={token}"),
        }))
    } else {
        None
    };
    Ok(Json(json!({
        "tracks": stream_tracking.inspect(&gid, owner).await?,
        "gid": gid.to_string(),
        "playback_plan": plan,
        "probe_source": if reused_probe { "ingestion" } else { "fallback" },
        "remote": remote,
    }))
    .into_response())
}

pub async fn return_playback_capabilities(
    State(AppState { conn, .. }): State<AppState>,
    Path(id): Path<i64>,
) -> Result<impl IntoResponse, DimErrorWrapper> {
    let mut tx = conn.read().begin().await?;
    let media = MediaFile::get_one(&mut tx, id)
        .await
        .map_err(|error| dim_core::errors::StreamingErrors::NoMediaFileFound(error.to_string()))?;
    if !path::Path::new(&media.target_file).exists() {
        return Err(dim_core::errors::StreamingErrors::FileDoesNotExist.into());
    }
    let (info, reused_probe) = load_probe(&media).await?;
    let video = info
        .get_primary("video")
        .ok_or(dim_core::errors::StreamingErrors::FileIsCorrupt)?;
    let width = positive_metadata(video.width, "video width")?;
    let height = positive_metadata(video.height, "video height")?;
    let bitrate = video
        .get_bitrate()
        .or_else(|| info.get_container_bitrate())
        .filter(|value| *value > 0)
        .ok_or_else(|| {
            dim_core::errors::StreamingErrors::InvalidMetadata("video bitrate is missing".into())
        })?;
    let frame_rate = video.precise_frame_rate().ok_or_else(|| {
        dim_core::errors::StreamingErrors::InvalidMetadata("video frame rate is missing".into())
    })?;
    let request = capability_request(video, width, height, bitrate, frame_rate);
    let audio = info
        .find_by_type("audio")
        .into_iter()
        .filter_map(|stream| {
            let channels = u64::try_from(stream.channels?)
                .ok()
                .filter(|value| *value > 0)?;
            let bitrate = stream
                .get_bitrate()
                .unwrap_or_else(|| fallback_audio_bitrate(channels));
            let sample_rate = stream.sample_rate.as_deref()?.parse::<u64>().ok()?;
            audio_capability_request(stream, channels, bitrate, sample_rate)
        })
        .collect::<Vec<_>>();

    Ok(Json(json!({
        "video": request,
        "audio": audio,
        "server_remux_supported": remux_supported(video),
        "probe_source": if reused_probe { "ingestion" } else { "fallback" },
    }))
    .into_response())
}

fn positive_metadata(value: Option<i64>, name: &str) -> Result<u64, DimErrorWrapper> {
    value
        .and_then(|value| u64::try_from(value).ok())
        .filter(|value| *value > 0)
        .ok_or_else(|| {
            dim_core::errors::StreamingErrors::InvalidMetadata(format!("{name} is missing")).into()
        })
}

async fn load_probe(media: &MediaFile) -> Result<(FFPStream, bool), DimErrorWrapper> {
    let metadata = tokio::fs::metadata(&media.target_file).await.ok();
    let fingerprint_matches = metadata
        .as_ref()
        .map(|metadata| {
            let size_matches = media
                .file_size
                .map(|size| size >= 0 && metadata.len() == size as u64)
                .unwrap_or(false);
            let modified_matches = media
                .modified_ns
                .zip(metadata.modified().ok())
                .and_then(|(saved, modified)| {
                    modified
                        .duration_since(UNIX_EPOCH)
                        .ok()
                        .map(|duration| saved >= 0 && duration.as_nanos() == saved as u128)
                })
                .unwrap_or(false);
            size_matches && modified_matches
        })
        .unwrap_or(false);
    if fingerprint_matches {
        if let Some(info) = media
            .probe_metadata
            .as_deref()
            .and_then(|json| serde_json::from_str::<FFPStream>(json).ok())
        {
            if info
                .get_primary("video")
                .is_some_and(has_exact_codec_configuration)
                && info.get_duration().is_some_and(|duration| duration > 0)
            {
                return Ok((info, true));
            }
        }
    }
    let info = FFProbeCtx::new(dim_core::streaming::FFPROBE_BIN.as_ref())
        .get_meta(&media.target_file)
        .await
        .map_err(|_| dim_core::errors::StreamingErrors::FFProbeCtxFailed)?;
    Ok((info, false))
}

fn build_tracks(
    info: &FFPStream,
    media: &MediaFile,
    prefs: &UserSettings,
    force_ass: bool,
    capabilities: &BrowserCapabilities,
    target: PlaybackTargetKind,
) -> Result<(Vec<PlannedTrack>, PlaybackPlan), DimErrorWrapper> {
    let video = info
        .get_primary("video")
        .cloned()
        .ok_or(dim_core::errors::StreamingErrors::FileIsCorrupt)?;
    let height = positive_metadata(video.height, "video height")?;
    let width = positive_metadata(video.width, "video width")?;
    let stream_bitrate = video.get_bitrate();
    let bitrate = stream_bitrate
        .or_else(|| info.get_container_bitrate())
        .filter(|value| *value > 0)
        .ok_or_else(|| {
            dim_core::errors::StreamingErrors::InvalidMetadata("video bitrate is missing".into())
        })?;
    let duration = info
        .get_duration()
        .filter(|value| *value > 0)
        .ok_or_else(|| {
            dim_core::errors::StreamingErrors::InvalidMetadata("duration is missing".into())
        })?;
    let precise_duration = video
        .duration
        .as_deref()
        .and_then(|value| value.parse::<f64>().ok())
        .filter(|value| value.is_finite() && *value > 0.0)
        .or_else(|| info.get_precise_duration())
        .ok_or_else(|| {
            dim_core::errors::StreamingErrors::InvalidMetadata(
                "precise video duration is missing".into(),
            )
        })?;
    let precise_frame_rate = video.precise_frame_rate().unwrap_or(30.0);
    let frame_rate = precise_frame_rate.round().max(1.0) as u64;
    let source_codec_descriptor = codec_descriptor(&video);
    let source_is_hdr = is_hdr(&video);
    let mut plan = plan_video_for_target(
        &VideoSource {
            codec: video.codec_name.clone(),
            profile: video.profile.clone(),
            pixel_format: video.pix_fmt.clone(),
            level: video.level,
            color_range: video.color_range.clone(),
            color_space: video.color_space.clone(),
            color_transfer: video.color_transfer.clone(),
            color_primaries: video.color_primaries.clone(),
            chroma_location: video.chroma_location.clone(),
            width,
            height,
            bitrate,
            frame_rate,
            codec_descriptor: source_codec_descriptor.clone(),
            remux_supported: remux_supported(&video),
            hdr: source_is_hdr,
        },
        capabilities,
        target,
    );
    let mut tracks = Vec::new();
    let direct_is_default = direct_play_is_default(
        &prefs.default_video_quality,
        height,
        plan.direct_play_supported,
    );

    if plan.direct_play_supported {
        let id = Uuid::new_v4().to_string();
        let output_codec = if video.codec_name == "av1" {
            "av1"
        } else {
            "h264"
        };
        let codec_tag = source_codec_descriptor.clone().ok_or_else(|| {
            dim_core::errors::StreamingErrors::InvalidMetadata(
                "direct-play codec descriptor is missing".into(),
            )
        })?;
        let mut input_ctx: nightfall::profiles::InputCtx = video.clone().into();
        input_ctx.fps = frame_rate as f64;
        let context = ProfileContext {
            file: media.target_file.clone(),
            input_ctx,
            output_ctx: OutputCtx {
                codec: output_codec.into(),
                start_num: 0,
                target_gop: 10,
                ..Default::default()
            },
            ..Default::default()
        };
        tracks.push(PlannedTrack {
            manifest: VirtualManifest::new(id, ContentType::Video)
                .set_direct()
                .set_mime("video/mp4")
                .set_duration(Some(duration))
                .set_codecs(codec_tag)
                .set_bandwidth(bitrate)
                .set_args([("width", width), ("height", height)])
                .set_is_default(direct_is_default)
                .set_target_duration(10)
                .set_frame_rate(Some(precise_frame_rate))
                .set_video_range(Some(match video.color_transfer.as_deref() {
                    Some("smpte2084") => "PQ",
                    Some("arib-std-b67") => "HLG",
                    _ => "SDR",
                }))
                .set_label(direct_play_label(height, stream_bitrate)),
            context,
            profile: PlannedProfile::DirectVideo,
        });
    }

    let mut assigned_default = direct_is_default;
    let preferred_resolution_available = match prefs.default_video_quality {
        DefaultVideoQuality::Resolution(resolution, _) => plan
            .renditions
            .iter()
            .any(|quality| quality.height == resolution),
        DefaultVideoQuality::DirectPlay => plan.direct_play_supported,
    };
    for quality in plan.renditions.iter().copied() {
        let target_width =
            ((width as f64 * quality.height as f64 / height as f64).round() as u64).max(2) & !1;
        let is_preferred_resolution = matches!(prefs.default_video_quality, DefaultVideoQuality::Resolution(res, _) if res == quality.height);
        let first_transcode = tracks.iter().all(|track| track.manifest.is_direct);
        let is_default = !assigned_default
            && (is_preferred_resolution || first_transcode && !preferred_resolution_available);
        assigned_default |= is_default;
        let mut input_ctx: nightfall::profiles::InputCtx = video.clone().into();
        input_ctx.fps = precise_frame_rate;
        let avc_level = get_avc1_tag(target_width, quality.height, quality.bitrate, frame_rate);
        let segment_durations = if target == PlaybackTargetKind::Airplay {
            cfr_video_segment_durations(precise_duration, precise_frame_rate, 5.0)?
        } else {
            Vec::new()
        };
        let context = ProfileContext {
            file: media.target_file.clone(),
            input_ctx,
            output_ctx: OutputCtx {
                codec: "h264".into(),
                start_num: 0,
                bitrate: Some(quality.bitrate),
                width: Some(target_width as i64),
                height: Some(quality.height as i64),
                video_profile: Some("high".into()),
                video_level: Some(avc_level.level),
                pixel_format: Some("yuv420p".into()),
                color_range: Some("tv".into()),
                color_space: Some("bt709".into()),
                color_transfer: Some("bt709".into()),
                color_primaries: Some("bt709".into()),
                hdr_transfer: source_is_hdr
                    .then(|| video.color_transfer.clone())
                    .flatten(),
                hdr_peak_nits: source_is_hdr.then(|| hdr_peak_nits(&video)).flatten(),
                media_duration: (target == PlaybackTargetKind::Airplay).then_some(precise_duration),
                force_cfr: target == PlaybackTargetKind::Airplay,
                segment_durations: (!segment_durations.is_empty())
                    .then(|| std::sync::Arc::new(segment_durations.clone())),
                hls_segment_duration: segment_durations.first().copied(),
                ..Default::default()
            },
            ..Default::default()
        };
        tracks.push(PlannedTrack {
            manifest: VirtualManifest::new(Uuid::new_v4().to_string(), ContentType::Video)
                .set_mime("video/mp4")
                .set_duration(Some(duration))
                .set_codecs(avc_level.to_string())
                .set_bandwidth(hls_transcode_peak_bandwidth(quality.bitrate))
                // The master is intentionally available before lazy encoding. An exact
                // average is not known yet, and AVERAGE-BANDWIDTH is optional in HLS.
                .set_average_bandwidth(0)
                .set_args([("width", target_width), ("height", quality.height)])
                .set_is_default(is_default)
                .set_frame_rate(Some(precise_frame_rate))
                .set_video_range(Some("SDR"))
                .set_segment_durations(segment_durations)
                .set_label(quality_to_label(
                    quality.bitrate,
                    quality.height,
                    Some(quality.bitrate),
                )),
            context,
            profile: PlannedProfile::Video,
        });
    }

    for audio in info.find_by_type("audio") {
        let source_channels = audio
            .channels
            .and_then(|value| u64::try_from(value).ok())
            .filter(|value| *value > 0)
            .unwrap_or(2);
        let source_bitrate = audio
            .bit_rate
            .as_deref()
            .and_then(|value| value.parse::<u64>().ok())
            .or_else(|| audio.get_bitrate())
            .unwrap_or_else(|| fallback_audio_bitrate(source_channels));
        let source_sample_rate = audio
            .sample_rate
            .as_deref()
            .and_then(|value| value.parse::<u64>().ok())
            .unwrap_or(0);
        let audio_duration = audio
            .duration
            .as_deref()
            .and_then(|value| value.parse::<f64>().ok())
            .filter(|value| value.is_finite() && *value > 0.0)
            .or_else(|| info.get_precise_duration())
            .ok_or_else(|| {
                dim_core::errors::StreamingErrors::InvalidMetadata(
                    "precise audio duration is missing".into(),
                )
            })?;
        let audio_plan = plan_audio_for_target(
            AudioSource {
                stream_index: audio.index,
                codec: audio.codec_name.clone(),
                codec_descriptor: audio_codec_descriptor(audio),
                channels: source_channels,
                channel_layout: audio.channel_layout.clone(),
                bitrate: source_bitrate,
                sample_rate: source_sample_rate,
                remux_supported: audio_remux_supported(audio),
            },
            capabilities,
            target,
        );
        let preserve = audio_plan.chosen_action == AudioAction::Preserve;
        plan.audio.push(audio_plan);
        let output = if target == PlaybackTargetKind::Airplay {
            // WebKit does not expose the selected route's channel capabilities. Apple's HLS
            // compatibility guidance identifies stereo AAC as the universal fallback.
            BrowserAacOutput {
                channels: 2,
                layout: "stereo",
                filter: None,
            }
        } else {
            browser_aac_output(source_channels, audio.channel_layout.as_deref())
        };
        let bitrate = if output.channels < source_channels {
            fallback_audio_bitrate(output.channels)
        } else {
            source_bitrate
        };
        let language = audio.get_language();
        let is_default = info.get_primary("audio") == Some(audio);
        let segment_durations = if target == PlaybackTargetKind::Airplay && !preserve {
            aac_segment_durations(audio_duration, 48_000, 5.0)?
        } else {
            Vec::new()
        };
        let context = ProfileContext {
            file: media.target_file.clone(),
            input_ctx: audio.clone().into(),
            output_ctx: OutputCtx {
                codec: if preserve {
                    audio.codec_name.clone()
                } else {
                    "aac".into()
                },
                start_num: 0,
                bitrate: (!preserve).then_some(bitrate),
                audio_channels: if preserve {
                    source_channels
                } else {
                    output.channels
                },
                audio_sample_rate: (target == PlaybackTargetKind::Airplay && !preserve)
                    .then_some(48_000),
                audio_channel_layout: (!preserve).then(|| output.layout.into()),
                audio_filter: if preserve {
                    None
                } else {
                    output.filter.map(Into::into)
                },
                media_duration: (target == PlaybackTargetKind::Airplay && !preserve)
                    .then_some(audio_duration),
                segment_durations: (!segment_durations.is_empty())
                    .then(|| std::sync::Arc::new(segment_durations.clone())),
                hls_segment_duration: segment_durations.first().copied(),
                ..Default::default()
            },
            ..Default::default()
        };
        let manifest_codec = if preserve {
            audio_codec_descriptor(audio).expect("preserved audio has a codec descriptor")
        } else {
            "mp4a.40.2".into()
        };
        let manifest_channels = if preserve {
            source_channels
        } else {
            output.channels
        };
        let label = format!(
            "{} ({} {}{})",
            language
                .as_deref()
                .and_then(dim_core::utils::lang_from_iso639)
                .unwrap_or("Unknown"),
            dim_core::utils::codec_pretty(audio.get_codec()),
            dim_core::utils::channels_pretty(manifest_channels as i64),
            if preserve { ", Direct Play" } else { "" }
        );
        tracks.push(PlannedTrack {
            manifest: {
                let manifest = VirtualManifest::new(Uuid::new_v4().to_string(), ContentType::Audio)
                    .set_mime("audio/mp4")
                    .set_duration(Some(duration))
                    .set_codecs(manifest_codec)
                    .set_bandwidth(if preserve {
                        source_bitrate
                    } else if target == PlaybackTargetKind::Airplay {
                        hls_transcode_peak_bandwidth(bitrate)
                    } else {
                        bitrate
                    })
                    .set_average_bandwidth(if target == PlaybackTargetKind::Airplay && !preserve {
                        0
                    } else if preserve {
                        source_bitrate
                    } else {
                        bitrate
                    })
                    .set_is_default(is_default)
                    .set_label(label)
                    .set_lang(language)
                    .set_audio_channels(Some(manifest_channels));
                let manifest = if target == PlaybackTargetKind::Airplay && !preserve {
                    manifest.set_segment_durations(segment_durations)
                } else {
                    manifest
                };
                if preserve {
                    manifest.set_direct()
                } else {
                    manifest
                }
            },
            context,
            profile: if preserve {
                PlannedProfile::DirectAudio
            } else {
                PlannedProfile::Audio
            },
        });
    }

    for subtitle in info.find_by_type("subtitle") {
        if !["subrip", "ass", "ssa", "srt", "webvtt", "vtt"].contains(&subtitle.codec_name.as_str())
        {
            continue;
        }
        let ass = force_ass && ["ass", "ssa"].contains(&subtitle.codec_name.as_str());
        let language = subtitle.get_language();
        let translated = language
            .as_deref()
            .and_then(dim_core::utils::lang_from_iso639)
            .unwrap_or("Unknown")
            .to_string();
        let title = subtitle
            .get_title()
            .unwrap_or_else(|| translated.clone())
            .replace('&', "and");
        let context = ProfileContext {
            file: media.target_file.clone(),
            input_ctx: subtitle.clone().into(),
            output_ctx: OutputCtx {
                codec: if ass { "ass" } else { "webvtt" }.into(),
                outdir: "-".into(),
                ..Default::default()
            },
            ..Default::default()
        };
        let public_id = Uuid::new_v4().to_string();
        let public_path = format!(
            "{public_id}/data/stream.{}",
            if ass { "ass" } else { "vtt" }
        );
        tracks.push(PlannedTrack {
            manifest: VirtualManifest::new(public_id, ContentType::Subtitle)
                .set_mime(if ass { "text/ass" } else { "text/vtt" })
                .set_duration(Some(duration))
                .set_codecs(if ass { "ass" } else { "vtt" })
                .set_bandwidth(1024)
                .set_is_default(info.get_primary("subtitle") == Some(subtitle))
                .set_label(subtitle.get_title().unwrap_or(translated))
                .set_lang(language)
                .set_args([("title", title)])
                .set_chunk_path(public_path),
            context,
            profile: PlannedProfile::Subtitle,
        });
    }
    Ok((tracks, plan))
}

fn cfr_video_segment_durations(
    duration: f64,
    frame_rate: f64,
    target: f64,
) -> Result<Vec<f64>, DimErrorWrapper> {
    if !duration.is_finite()
        || duration <= 0.0
        || !frame_rate.is_finite()
        || frame_rate <= 0.0
        || !target.is_finite()
        || target <= 0.0
    {
        return Err(dim_core::errors::StreamingErrors::InvalidMetadata(
            "invalid HLS video timing metadata".into(),
        )
        .into());
    }
    let total_frames = (duration * frame_rate).round().max(1.0) as u64;
    let cadence_frames = (target * frame_rate).ceil().max(1.0) as u64;
    let mut durations = Vec::new();
    let mut previous = 0_u64;
    while previous < total_frames {
        let boundary = previous.saturating_add(cadence_frames).min(total_frames);
        if boundary <= previous {
            return Err(dim_core::errors::StreamingErrors::InvalidMetadata(
                "non-advancing HLS video timeline".into(),
            )
            .into());
        }
        durations.push((boundary - previous) as f64 / frame_rate);
        previous = boundary;
    }
    Ok(durations)
}

/// A remote HLS rendition is advertised before its lazy encoder starts. The video encoder uses
/// `maxrate = target` with a half-second VBV buffer, so over a five-second HLS window its encoded
/// payload is bounded to 110% of target. Reserve a further 10% for fMP4/AAC container overhead and
/// integer rounding instead of claiming that the target average itself is the peak segment rate.
fn hls_transcode_peak_bandwidth(target: u64) -> u64 {
    target.saturating_mul(6).div_ceil(5)
}

fn aac_segment_durations(
    duration: f64,
    sample_rate: u64,
    target: f64,
) -> Result<Vec<f64>, DimErrorWrapper> {
    if !duration.is_finite()
        || duration <= 0.0
        || sample_rate == 0
        || !target.is_finite()
        || target <= 0.0
    {
        return Err(dim_core::errors::StreamingErrors::InvalidMetadata(
            "invalid HLS audio timing metadata".into(),
        )
        .into());
    }
    const AAC_FRAME_SAMPLES: f64 = 1024.0;
    // FFmpeg's native AAC encoder contributes one encoded frame of delay when output is bounded
    // with -t. Interior HLS boundaries are whole AAC frames; the final fragment carries the
    // remaining presentation duration.
    let total = duration + AAC_FRAME_SAMPLES / sample_rate as f64;
    let mut durations = Vec::new();
    let mut previous = 0.0;
    let cadence_frames = (target * sample_rate as f64 / AAC_FRAME_SAMPLES)
        .ceil()
        .max(1.0);
    let cadence = cadence_frames * AAC_FRAME_SAMPLES / sample_rate as f64;
    while previous + 0.000_000_5 < total {
        let boundary = (previous + cadence).min(total);
        if boundary <= previous {
            return Err(dim_core::errors::StreamingErrors::InvalidMetadata(
                "non-advancing HLS audio timeline".into(),
            )
            .into());
        }
        durations.push(boundary - previous);
        previous = boundary;
    }
    Ok(durations)
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
struct BrowserAacOutput {
    channels: u64,
    layout: &'static str,
    filter: Option<&'static str>,
}

fn browser_aac_output(channels: u64, layout: Option<&str>) -> BrowserAacOutput {
    let compatible_layout = match (channels, layout) {
        (1, Some("mono")) => Some("mono"),
        (2, Some("stereo")) => Some("stereo"),
        (3, Some("3.0")) => Some("3.0"),
        (4, Some("4.0")) => Some("4.0"),
        (5, Some("5.0")) => Some("5.0"),
        (6, Some("5.1")) => Some("5.1"),
        (8, Some("7.1")) => Some("7.1"),
        _ => None,
    };
    if let Some(layout) = compatible_layout {
        return BrowserAacOutput {
            channels,
            layout,
            filter: None,
        };
    }

    if channels == 6 && layout == Some("5.1(side)") {
        return BrowserAacOutput {
            channels: 6,
            layout: "5.1",
            filter: Some("pan=5.1|FL=FL|FR=FR|FC=FC|LFE=LFE|BL=SL|BR=SR"),
        };
    }

    BrowserAacOutput {
        channels: 2,
        layout: "stereo",
        filter: None,
    }
}

fn fallback_audio_bitrate(channels: u64) -> u64 {
    // The native AAC encoder receives one total bitrate for every channel. Keep the established
    // 128 kb/s stereo floor, but do not divide that same budget across larger channel layouts.
    channels.saturating_mul(64_000).max(128_000)
}

fn direct_play_label(height: u64, stream_bitrate: Option<u64>) -> String {
    match stream_bitrate {
        Some(bitrate) => format!(
            "Direct Play ({height}p, {})",
            bitrate_to_label(bitrate).replace(' ', "")
        ),
        None => format!("Direct Play ({height}p)"),
    }
}

fn direct_play_is_default(
    preference: &DefaultVideoQuality,
    source_height: u64,
    direct_play_supported: bool,
) -> bool {
    direct_play_supported
        && match preference {
            DefaultVideoQuality::DirectPlay => true,
            DefaultVideoQuality::Resolution(height, _) => *height >= source_height,
        }
}

#[derive(Deserialize)]
pub struct RemoteAccessParams {
    token: String,
}

#[derive(Deserialize)]
pub struct RemoteRouteState {
    state: RemotePlaybackState,
}

pub async fn update_remote_route_state(
    State(AppState {
        stream_tracking, ..
    }): State<AppState>,
    Path(gid): Path<String>,
    Extension(user): Extension<User>,
    Json(state): Json<RemoteRouteState>,
) -> Result<impl IntoResponse, DimErrorWrapper> {
    let gid =
        Uuid::parse_str(&gid).map_err(|_| dim_core::errors::StreamingErrors::GidParseError)?;
    stream_tracking
        .set_remote_playback_state(&gid, user.id.get(), state.state)
        .await?;
    Ok(StatusCode::NO_CONTENT)
}

pub async fn get_remote_playback_status(
    State(AppState {
        stream_tracking, ..
    }): State<AppState>,
    Path(gid): Path<String>,
    Extension(user): Extension<User>,
) -> Result<impl IntoResponse, DimErrorWrapper> {
    let gid =
        Uuid::parse_str(&gid).map_err(|_| dim_core::errors::StreamingErrors::GidParseError)?;
    Ok(Json(
        stream_tracking
            .remote_playback_status(&gid, user.id.get())
            .await?,
    ))
}

fn compile_remote_master(
    tracks: &[VirtualManifest],
    gid: Uuid,
    token: &str,
) -> Result<String, dim_core::errors::StreamingErrors> {
    let videos = tracks
        .iter()
        .filter(|track| {
            track.content_type == ContentType::Video && !track.segment_durations.is_empty()
        })
        .collect::<Vec<_>>();
    if videos.is_empty() {
        return Err(dim_core::errors::StreamingErrors::InvalidRequest);
    }
    let audio = tracks
        .iter()
        .find(|track| {
            track.content_type == ContentType::Audio
                && track.is_default
                && !track.segment_durations.is_empty()
        })
        .or_else(|| {
            tracks.iter().find(|track| {
                track.content_type == ContentType::Audio && !track.segment_durations.is_empty()
            })
        });
    let audio_group = audio.map(|track| {
        format!(
            "#EXT-X-MEDIA:TYPE=AUDIO,GROUP-ID=\"audio-aac-stereo\",NAME=\"{}\",LANGUAGE=\"{}\",DEFAULT=YES,AUTOSELECT=YES,CHANNELS=\"{}\",URI=\"/api/v1/remote/{gid}/{}/index.m3u8?token={token}\"\n",
            track.label.replace('"', ""),
            track.lang.as_deref().unwrap_or("und").replace('"', ""),
            track.audio_channels.unwrap_or(2),
            track.id,
        )
    });
    let mixed_video_range = videos
        .iter()
        .any(|track| track.video_range.as_deref() != Some("SDR"));
    let mut body = format!(
        "#EXTM3U\n#EXT-X-VERSION:7\n#EXT-X-INDEPENDENT-SEGMENTS\n{}",
        audio_group.unwrap_or_default()
    );
    for video in videos {
        let codecs = audio
            .map(|track| format!("{},{}", video.codecs, track.codecs))
            .unwrap_or_else(|| video.codecs.clone());
        let audio_bandwidth = audio.map(|track| track.bandwidth).unwrap_or(0);
        let audio_average = audio.map(|track| track.average_bandwidth).unwrap_or(0);
        let bandwidth = video.bandwidth.saturating_add(audio_bandwidth);
        let average_bandwidth = video.average_bandwidth.saturating_add(audio_average);
        let (width, height) = video
            .args
            .get("width")
            .zip(video.args.get("height"))
            .ok_or(dim_core::errors::StreamingErrors::InvalidRequest)?;
        let frame_rate = video
            .frame_rate
            .filter(|value| value.is_finite() && *value > 0.0)
            .ok_or(dim_core::errors::StreamingErrors::InvalidRequest)?;
        let video_range = if mixed_video_range {
            format!(
                ",VIDEO-RANGE={}",
                video.video_range.as_deref().unwrap_or("SDR")
            )
        } else {
            String::new()
        };
        let audio_attribute = audio
            .map(|_| ",AUDIO=\"audio-aac-stereo\"")
            .unwrap_or_default();
        let average_attribute = (average_bandwidth > 0)
            .then(|| format!(",AVERAGE-BANDWIDTH={average_bandwidth}"))
            .unwrap_or_default();
        body.push_str(&format!(
            "#EXT-X-STREAM-INF:BANDWIDTH={bandwidth}{average_attribute},CODECS=\"{codecs}\",RESOLUTION={width}x{height},FRAME-RATE={frame_rate:.3}{video_range}{audio_attribute},CLOSED-CAPTIONS=NONE\n/api/v1/remote/{gid}/{}/index.m3u8?token={token}\n",
            video.id,
        ));
    }
    Ok(body)
}

fn compile_remote_media_playlist(
    track: &VirtualManifest,
    gid: Uuid,
    token: &str,
) -> Result<String, dim_core::errors::StreamingErrors> {
    if track.segment_durations.is_empty()
        || track
            .segment_durations
            .iter()
            .any(|duration| !duration.is_finite() || *duration <= 0.0)
    {
        return Err(dim_core::errors::StreamingErrors::InvalidRequest);
    }
    let target_duration = track
        .segment_durations
        .iter()
        .copied()
        .fold(0.0_f64, f64::max)
        .round()
        .max(1.0) as u64;
    let mut body = format!(
        "#EXTM3U\n#EXT-X-VERSION:7\n#EXT-X-TARGETDURATION:{target_duration}\n#EXT-X-MEDIA-SEQUENCE:0\n#EXT-X-PLAYLIST-TYPE:VOD\n#EXT-X-MAP:URI=\"/api/v1/remote/{gid}/{}/init.mp4?token={token}\"\n",
        track.id,
    );
    for (index, length) in track.segment_durations.iter().enumerate() {
        body.push_str(&format!(
            "#EXTINF:{length:.6},\n/api/v1/remote/{gid}/{}/{index}.m4s?token={token}\n",
            track.id,
        ));
    }
    body.push_str("#EXT-X-ENDLIST\n");
    Ok(body)
}

pub async fn return_remote_master(
    State(AppState {
        stream_tracking, ..
    }): State<AppState>,
    Path(gid): Path<String>,
    Query(params): Query<RemoteAccessParams>,
) -> Result<impl IntoResponse, DimErrorWrapper> {
    let gid =
        Uuid::parse_str(&gid).map_err(|_| dim_core::errors::StreamingErrors::GidParseError)?;
    let owner = stream_tracking
        .authenticate_remote(&gid, &params.token)
        .await?;
    let tracks = stream_tracking.inspect(&gid, owner).await?;
    let body = compile_remote_master(&tracks, gid, &params.token)?;
    Ok((
        [
            (header::CONTENT_TYPE, "application/vnd.apple.mpegurl"),
            (header::CACHE_CONTROL, "no-store"),
        ],
        body,
    ))
}

pub async fn return_remote_media_playlist(
    State(AppState {
        stream_tracking, ..
    }): State<AppState>,
    Path((gid, public_id)): Path<(String, String)>,
    Query(params): Query<RemoteAccessParams>,
) -> Result<impl IntoResponse, DimErrorWrapper> {
    let gid =
        Uuid::parse_str(&gid).map_err(|_| dim_core::errors::StreamingErrors::GidParseError)?;
    let owner = stream_tracking
        .authenticate_remote(&gid, &params.token)
        .await?;
    let (track, process_id) = stream_tracking
        .remote_track(&gid, owner, &public_id)
        .await?;
    tracing::info!(
        session_id = %gid,
        owner,
        track_id = public_id,
        content_type = %track.content_type,
        codec = track.codecs,
        active = process_id.is_some(),
        "Remote HLS rendition playlist inspected"
    );
    let body = compile_remote_media_playlist(&track, gid, &params.token)?;
    Ok((
        [
            (header::CONTENT_TYPE, "application/vnd.apple.mpegurl"),
            (header::CACHE_CONTROL, "no-store"),
        ],
        body,
    ))
}

async fn resolve_remote_process(
    state: &StateManager,
    tracking: &StreamTracking,
    gid: &str,
    public_id: &str,
    token: &str,
    allow_video_replacement: bool,
) -> Result<(Uuid, i64, String), DimErrorWrapper> {
    let gid = Uuid::parse_str(gid).map_err(|_| dim_core::errors::StreamingErrors::GidParseError)?;
    let owner = tracking.authenticate_remote(&gid, token).await?;
    let (track, mut process_id) = tracking.remote_track(&gid, owner, public_id).await?;
    if process_id.is_none() {
        tracing::info!(
            session_id = %gid,
            owner,
            track_id = public_id,
            content_type = %track.content_type,
            codec = track.codecs,
            bandwidth = track.bandwidth,
            width = track.args.get("width").map(String::as_str).unwrap_or(""),
            height = track.args.get("height").map(String::as_str).unwrap_or(""),
            video_range = track.video_range.as_deref().unwrap_or(""),
            "HLS rendition activated on first media fetch"
        );
        process_id = Some(
            tracking
                .activate_remote_track(state, &gid, public_id, owner, allow_video_replacement)
                .await?,
        );
    }
    Ok((
        gid,
        owner,
        process_id.ok_or(dim_core::errors::StreamingErrors::InvalidRequest)?,
    ))
}

pub async fn get_remote_init(
    State(AppState {
        state,
        stream_tracking,
        ..
    }): State<AppState>,
    Path((gid, public_id)): Path<(String, String)>,
    Query(params): Query<RemoteAccessParams>,
    headers: HeaderMap,
) -> Result<Response<Body>, DimErrorWrapper> {
    let (gid, owner, process_id) = resolve_remote_process(
        &state,
        &stream_tracking,
        &gid,
        &public_id,
        &params.token,
        true,
    )
    .await?;
    match timeout_segment(
        || state.chunk_init_request(process_id.clone(), 0),
        Duration::from_millis(100),
        100,
    )
    .await
    {
        Ok(path) => Ok(reply_with_file(path, "video/mp4", &headers, true).await),
        Err(error) => {
            stop_failed_transcode(&state, &stream_tracking, gid, owner, &process_id, &error).await;
            Err(error.into())
        }
    }
}

pub async fn get_remote_chunk(
    State(AppState {
        state,
        stream_tracking,
        ..
    }): State<AppState>,
    Path((gid, public_id, chunk)): Path<(String, String, String)>,
    Query(params): Query<RemoteAccessParams>,
    headers: HeaderMap,
) -> Result<Response<Body>, DimErrorWrapper> {
    let chunk_num = chunk
        .strip_suffix(".m4s")
        .and_then(|value| value.parse::<u32>().ok())
        .ok_or(dim_core::errors::StreamingErrors::InvalidRequest)?;
    let (gid, owner, process_id) = resolve_remote_process(
        &state,
        &stream_tracking,
        &gid,
        &public_id,
        &params.token,
        false,
    )
    .await?;
    match timeout_segment(
        || state.chunk_request(process_id.clone(), chunk_num),
        Duration::from_millis(100),
        100,
    )
    .await
    {
        Ok(path) => Ok(reply_with_file(path, "video/iso.segment", &headers, true).await),
        Err(error) => {
            stop_failed_transcode(&state, &stream_tracking, gid, owner, &process_id, &error).await;
            Err(error.into())
        }
    }
}

#[derive(Deserialize)]
pub struct ManifestParams {
    start_num: Option<u64>,
    #[allow(dead_code)]
    should_kill: Option<bool>,
    includes: Option<String>,
    #[serde(default)]
    replace_video: bool,
}

pub async fn return_manifest(
    State(AppState {
        state,
        stream_tracking,
        ..
    }): State<AppState>,
    Path(gid): Path<String>,
    Query(params): Query<ManifestParams>,
    Extension(user): Extension<User>,
) -> Result<impl IntoResponse, DimErrorWrapper> {
    let gid =
        Uuid::parse_str(&gid).map_err(|_| dim_core::errors::StreamingErrors::GidParseError)?;
    let tracks = stream_tracking.inspect(&gid, user.id.get()).await?;
    let includes = params
        .includes
        .map(|value| {
            value
                .split(',')
                .filter(|value| !value.is_empty())
                .map(str::to_owned)
                .collect::<Vec<_>>()
        })
        .unwrap_or_else(|| {
            tracks
                .iter()
                .filter(|track| track.is_default && track.content_type != ContentType::Subtitle)
                .map(|track| track.id.clone())
                .collect()
        });
    let manifest = if params.replace_video {
        stream_tracking
            .replace_video_and_compile(
                &state,
                &gid,
                user.id.get(),
                params.start_num.unwrap_or(0),
                includes,
            )
            .await?
    } else {
        stream_tracking
            .activate_and_compile(
                &state,
                &gid,
                user.id.get(),
                params.start_num.unwrap_or(0),
                includes,
            )
            .await?
    };
    Ok((
        [
            (header::CONTENT_TYPE, "application/dash+xml"),
            (header::CACHE_CONTROL, "no-store"),
        ],
        manifest,
    ))
}

async fn timeout_segment<F, T>(
    f: impl Fn() -> F,
    tick_dur: Duration,
    tick_limit: usize,
) -> Result<T, NightfallError>
where
    F: Future<Output = Result<T, NightfallError>>,
{
    for _ in 0..tick_limit {
        match f().await {
            Err(NightfallError::ChunkNotDone) => tokio::time::sleep(tick_dur).await,
            result => return result,
        }
    }
    Err(NightfallError::ChunkNotDone)
}

async fn stop_failed_transcode(
    state: &StateManager,
    tracking: &StreamTracking,
    gid: Uuid,
    owner: i64,
    id: &str,
    error: &NightfallError,
) {
    let stderr = state.get_stderr(id.to_owned()).await.unwrap_or_default();
    tracing::error!(stream_id = id, error = %error, ffmpeg_stderr = %stderr, "FFmpeg stream failed");
    let _ = tracking.remove(state, &gid, owner).await;
}

#[derive(Deserialize)]
pub struct InitParams {
    start_num: Option<u32>,
}

pub async fn get_init(
    State(AppState {
        state,
        stream_tracking,
        ..
    }): State<AppState>,
    Path(id): Path<String>,
    Query(params): Query<InitParams>,
    Extension(user): Extension<User>,
    headers: HeaderMap,
) -> Result<Response<Body>, DimErrorWrapper> {
    let gid = stream_tracking
        .owner_for_process(&id, user.id.get())
        .await?;
    let file = timeout_segment(
        || state.chunk_init_request(id.clone(), params.start_num.unwrap_or(0)),
        Duration::from_millis(100),
        100,
    )
    .await;
    match file {
        Ok(path) => Ok(reply_with_file(path, "video/mp4", &headers, true).await),
        Err(error) => {
            stop_failed_transcode(&state, &stream_tracking, gid, user.id.get(), &id, &error).await;
            Err(error.into())
        }
    }
}

pub async fn get_chunk(
    State(AppState {
        state,
        stream_tracking,
        ..
    }): State<AppState>,
    Path((id, chunk)): Path<(String, PathBuf)>,
    Extension(user): Extension<User>,
    headers: HeaderMap,
) -> Result<Response<Body>, DimErrorWrapper> {
    let gid = stream_tracking
        .owner_for_process(&id, user.id.get())
        .await?;
    if chunk.extension().and_then(|value| value.to_str()) != Some("m4s") {
        return Err(dim_core::errors::StreamingErrors::InvalidRequest.into());
    }
    let chunk_num = chunk
        .file_stem()
        .and_then(|value| value.to_str())
        .and_then(|value| value.parse::<u32>().ok())
        .ok_or(dim_core::errors::StreamingErrors::InvalidRequest)?;
    let file = timeout_segment(
        || state.chunk_request(id.clone(), chunk_num),
        Duration::from_millis(100),
        100,
    )
    .await;
    match file {
        Ok(path) => Ok(reply_with_file(path, "video/mp4", &headers, true).await),
        Err(error) => {
            stop_failed_transcode(&state, &stream_tracking, gid, user.id.get(), &id, &error).await;
            Err(error.into())
        }
    }
}

async fn subtitle_response(
    state: StateManager,
    tracking: StreamTracking,
    id: String,
    owner: i64,
    mime: &'static str,
    headers: HeaderMap,
) -> Result<Response<Body>, DimErrorWrapper> {
    let process_id = match tracking.owner_for_process(&id, owner).await {
        Ok(_) => id,
        Err(_) => tracking.activate_public_track(&state, &id, owner).await?,
    };
    let gid = tracking.owner_for_process(&process_id, owner).await?;
    let file = timeout_segment(
        || state.get_sub(process_id.clone(), "stream".into()),
        Duration::from_millis(100),
        200,
    )
    .await;
    match file {
        Ok(path) => Ok(reply_with_file(path, mime, &headers, false).await),
        Err(error) => {
            stop_failed_transcode(&state, &tracking, gid, owner, &process_id, &error).await;
            Err(error.into())
        }
    }
}

pub async fn get_subtitle(
    State(AppState {
        state,
        stream_tracking,
        ..
    }): State<AppState>,
    Path(id): Path<String>,
    Extension(user): Extension<User>,
    headers: HeaderMap,
) -> Result<Response<Body>, DimErrorWrapper> {
    subtitle_response(
        state,
        stream_tracking,
        id,
        user.id.get(),
        "text/vtt; charset=utf-8",
        headers,
    )
    .await
}
pub async fn get_subtitle_ass(
    State(AppState {
        state,
        stream_tracking,
        ..
    }): State<AppState>,
    Path(id): Path<String>,
    Extension(user): Extension<User>,
    headers: HeaderMap,
) -> Result<Response<Body>, DimErrorWrapper> {
    subtitle_response(
        state,
        stream_tracking,
        id,
        user.id.get(),
        "text/ass; charset=utf-8",
        headers,
    )
    .await
}

pub async fn should_client_hard_seek(
    State(AppState {
        state,
        stream_tracking,
        ..
    }): State<AppState>,
    Path((gid, chunk_num)): Path<(String, u32)>,
    Extension(user): Extension<User>,
) -> Result<impl IntoResponse, DimErrorWrapper> {
    let gid =
        Uuid::parse_str(&gid).map_err(|_| dim_core::errors::StreamingErrors::GidParseError)?;
    let streams = stream_tracking
        .active_manifests(&gid, user.id.get())
        .await?;
    let mut should_seek = false;
    for (_, id) in streams {
        should_seek |= state.should_hard_seek(id, chunk_num).await?;
    }
    Ok(Json(json!({ "should_client_seek": should_seek })))
}

pub async fn session_get_stderr(
    State(AppState {
        state,
        stream_tracking,
        ..
    }): State<AppState>,
    Path(gid): Path<String>,
    Extension(user): Extension<User>,
) -> Result<impl IntoResponse, DimErrorWrapper> {
    let gid =
        Uuid::parse_str(&gid).map_err(|_| dim_core::errors::StreamingErrors::GidParseError)?;
    let errors = stream::iter(
        stream_tracking
            .active_manifests(&gid, user.id.get())
            .await?,
    )
    .filter_map(|(_, id)| {
        let state = state.clone();
        async move { state.get_stderr(id).await.ok() }
    })
    .collect::<Vec<_>>()
    .await;
    if !errors.is_empty() {
        tracing::warn!(session_id = %gid, process_errors = ?errors, "Playback process reported diagnostics");
    }
    let public_errors = errors
        .iter()
        .map(|_| {
            "Playback processing failed. See the local Dim logs for administrator diagnostics."
        })
        .collect::<Vec<_>>();
    Ok(Json(json!({ "errors": public_errors })))
}

pub async fn kill_session(
    State(AppState {
        state,
        stream_tracking,
        ..
    }): State<AppState>,
    Path(gid): Path<String>,
    Extension(user): Extension<User>,
) -> Result<impl IntoResponse, DimErrorWrapper> {
    let gid =
        Uuid::parse_str(&gid).map_err(|_| dim_core::errors::StreamingErrors::GidParseError)?;
    stream_tracking.remove(&state, &gid, user.id.get()).await?;
    Ok(StatusCode::NO_CONTENT)
}

fn parse_range(value: Option<&str>, size: u64) -> Result<Option<(u64, u64)>, ()> {
    let Some(value) = value else { return Ok(None) };
    let value = value.strip_prefix("bytes=").ok_or(())?;
    if value.contains(',') || size == 0 {
        return Err(());
    }
    let (start, end) = value.split_once('-').ok_or(())?;
    let (start, end) = if start.is_empty() {
        let suffix = end.parse::<u64>().map_err(|_| ())?.min(size);
        (size - suffix, size - 1)
    } else {
        let start = start.parse::<u64>().map_err(|_| ())?;
        let end = if end.is_empty() {
            size - 1
        } else {
            end.parse::<u64>().map_err(|_| ())?.min(size - 1)
        };
        (start, end)
    };
    if start > end || start >= size {
        return Err(());
    }
    Ok(Some((start, end)))
}

async fn reply_with_file(
    path: String,
    content_type: &'static str,
    headers: &HeaderMap,
    immutable: bool,
) -> Response<Body> {
    let Ok(mut file) = File::open(&path).await else {
        return Response::builder()
            .status(StatusCode::NOT_FOUND)
            .body(Body::empty())
            .unwrap();
    };
    let Ok(metadata) = file.metadata().await else {
        return Response::builder()
            .status(StatusCode::NOT_FOUND)
            .body(Body::empty())
            .unwrap();
    };
    let size = metadata.len();
    let range = match parse_range(
        headers
            .get(header::RANGE)
            .and_then(|value| value.to_str().ok()),
        size,
    ) {
        Ok(range) => range,
        Err(()) => {
            return Response::builder()
                .status(StatusCode::RANGE_NOT_SATISFIABLE)
                .header(header::CONTENT_RANGE, format!("bytes */{size}"))
                .body(Body::empty())
                .unwrap()
        }
    };
    let (status, start, length) = match range {
        Some((start, end)) => (StatusCode::PARTIAL_CONTENT, start, end - start + 1),
        None => (StatusCode::OK, 0, size),
    };
    if file.seek(std::io::SeekFrom::Start(start)).await.is_err() {
        return Response::builder()
            .status(StatusCode::INTERNAL_SERVER_ERROR)
            .body(Body::empty())
            .unwrap();
    }
    let stream = ReaderStream::new(file.take(length));
    let mut builder = Response::builder()
        .status(status)
        .header(header::CONTENT_TYPE, content_type)
        .header(header::CONTENT_LENGTH, length)
        .header(header::ACCEPT_RANGES, "bytes")
        .header(
            header::CACHE_CONTROL,
            if immutable {
                "private, max-age=31536000, immutable"
            } else {
                "private, no-store"
            },
        );
    if let Some((start, end)) = range {
        builder = builder.header(header::CONTENT_RANGE, format!("bytes {start}-{end}/{size}"));
    }
    builder.body(Body::wrap_stream(stream)).unwrap()
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::atomic::{AtomicUsize, Ordering};
    use std::sync::Arc;

    #[test]
    fn parses_standard_open_and_suffix_ranges() {
        assert_eq!(parse_range(Some("bytes=2-5"), 10), Ok(Some((2, 5))));
        assert_eq!(parse_range(Some("bytes=7-"), 10), Ok(Some((7, 9))));
        assert_eq!(parse_range(Some("bytes=-3"), 10), Ok(Some((7, 9))));
        assert!(parse_range(Some("bytes=12-14"), 10).is_err());
    }

    #[test]
    fn airplay_hls_uses_selected_tracks_and_scoped_urls() {
        let video = VirtualManifest::new("video".into(), ContentType::Video)
            .set_mime("video/mp4")
            .set_codecs("avc1.640028")
            .set_bandwidth(10_000_000)
            .set_duration(Some(12))
            .set_args([("width", 1920), ("height", 800)])
            .set_frame_rate(Some(24.0))
            .set_video_range(Some("SDR"))
            .set_segment_durations(vec![5.0, 5.0, 2.0])
            .set_is_default(true);
        let audio = VirtualManifest::new("audio".into(), ContentType::Audio)
            .set_mime("audio/mp4")
            .set_codecs("mp4a.40.2")
            .set_bandwidth(128_000)
            .set_duration(Some(12))
            .set_audio_channels(Some(2))
            .set_segment_durations(vec![5.0, 5.0, 2.0])
            .set_label("English (AAC Stereo)".into())
            .set_is_default(true);
        let gid = Uuid::nil();
        let master = compile_remote_master(&[video.clone(), audio], gid, "secret").unwrap();
        assert!(master.contains("CODECS=\"avc1.640028,mp4a.40.2\""));
        assert!(master.contains("RESOLUTION=1920x800"));
        assert!(master.contains("AVERAGE-BANDWIDTH=10128000"));
        assert!(master.contains("FRAME-RATE=24.000"));
        assert!(master.contains("CHANNELS=\"2\""));
        assert!(master.contains("/audio/index.m3u8?token=secret"));
        assert!(master.contains("/video/index.m3u8?token=secret"));

        let media = compile_remote_media_playlist(&video, gid, "secret").unwrap();
        assert!(media.contains("#EXT-X-MAP:URI="));
        assert_eq!(media.matches("#EXTINF:").count(), 3);
        assert!(media.contains("#EXTINF:2.000000,"));
        assert!(media.ends_with("#EXT-X-ENDLIST\n"));
        assert_eq!(media.matches("token=secret").count(), 4);
    }

    #[test]
    fn hls_lazy_transcodes_use_a_peak_envelope_and_omit_unproven_average() {
        assert_eq!(hls_transcode_peak_bandwidth(5_000_000), 6_000_000);
        let video = VirtualManifest::new("video".into(), ContentType::Video)
            .set_codecs("avc1.640028")
            .set_bandwidth(hls_transcode_peak_bandwidth(5_000_000))
            .set_average_bandwidth(0)
            .set_args([("width", 1280), ("height", 720)])
            .set_frame_rate(Some(24.0))
            .set_segment_durations(vec![5.0]);
        let master = compile_remote_master(&[video], Uuid::nil(), "token").unwrap();
        assert!(master.contains("BANDWIDTH=6000000"));
        assert!(!master.contains("AVERAGE-BANDWIDTH"));
    }

    #[test]
    fn hls_master_signals_mixed_codec_and_dynamic_range_candidates() {
        let source = VirtualManifest::new("source".into(), ContentType::Video)
            .set_direct()
            .set_mime("video/mp4")
            .set_codecs("av01.0.12M.10.0.110.09.16.09.0")
            .set_bandwidth(11_618_576)
            .set_duration(Some(12))
            .set_args([("width", 3840), ("height", 1600)])
            .set_frame_rate(Some(23.976))
            .set_segment_durations(vec![5.005, 5.005, 1.99])
            .set_video_range(Some("PQ"));
        let fallback = VirtualManifest::new("fallback".into(), ContentType::Video)
            .set_mime("video/mp4")
            .set_codecs("avc1.640033")
            .set_bandwidth(11_618_576)
            .set_duration(Some(12))
            .set_args([("width", 3840), ("height", 1600)])
            .set_frame_rate(Some(23.976))
            .set_segment_durations(vec![5.005, 5.005, 1.99])
            .set_video_range(Some("SDR"));
        let audio = VirtualManifest::new("audio".into(), ContentType::Audio)
            .set_mime("audio/mp4")
            .set_codecs("mp4a.40.2")
            .set_bandwidth(128_000)
            .set_duration(Some(12))
            .set_audio_channels(Some(2))
            .set_segment_durations(vec![5.013333, 4.992, 1.994667])
            .set_lang(Some("eng".into()))
            .set_label("English (AAC Stereo)".into())
            .set_is_default(true);

        let master = compile_remote_master(&[source, fallback, audio], Uuid::nil(), "token")
            .expect("multivariant master");
        assert_eq!(master.matches("#EXT-X-STREAM-INF:").count(), 2);
        assert!(master.contains("VIDEO-RANGE=PQ"));
        assert!(master.contains("VIDEO-RANGE=SDR"));
        assert!(master.contains("CODECS=\"av01.0.12M.10.0.110.09.16.09.0,mp4a.40.2\""));
        assert!(master.contains("CODECS=\"avc1.640033,mp4a.40.2\""));
        assert!(master.contains("LANGUAGE=\"eng\""));
    }

    #[test]
    fn fallback_audio_bitrate_scales_with_channel_count() {
        assert_eq!(fallback_audio_bitrate(1), 128_000);
        assert_eq!(fallback_audio_bitrate(2), 128_000);
        assert_eq!(fallback_audio_bitrate(6), 384_000);
        assert_eq!(fallback_audio_bitrate(8), 512_000);
    }

    #[test]
    fn hls_video_timeline_follows_the_output_frame_clock() {
        let timeline = cfr_video_segment_durations(12.262, 24_000.0 / 1_001.0, 5.0).unwrap();
        assert_eq!(timeline.len(), 3);
        assert!((timeline[0] - 5.005).abs() < 0.000_001);
        assert!((timeline[1] - 5.005).abs() < 0.000_001);
        assert!((timeline[2] - 2.25225).abs() < 0.000_001);
    }

    #[test]
    fn hls_aac_timeline_follows_whole_codec_frames() {
        let timeline = aac_segment_durations(12.3, 48_000, 5.0).unwrap();
        assert_eq!(timeline.len(), 3);
        assert!((timeline[0] - 5.013333333).abs() < 0.000_001);
        assert!((timeline[1] - 5.013333333).abs() < 0.000_001);
        assert!((timeline[2] - 2.294666667).abs() < 0.000_001);
    }

    #[test]
    fn hls_refuses_a_representation_without_a_proven_timeline() {
        let video = VirtualManifest::new("video".into(), ContentType::Video)
            .set_mime("video/mp4")
            .set_codecs("avc1.640028")
            .set_bandwidth(1_000_000)
            .set_args([("width", 1280), ("height", 720)])
            .set_frame_rate(Some(24.0));
        assert!(compile_remote_media_playlist(&video, Uuid::nil(), "token").is_err());
    }

    #[test]
    fn hls_master_omits_unproven_source_packaging_but_keeps_valid_fallbacks() {
        let source = VirtualManifest::new("source".into(), ContentType::Video)
            .set_direct()
            .set_codecs("avc1.640028")
            .set_bandwidth(8_000_000)
            .set_args([("width", 1920), ("height", 1080)])
            .set_frame_rate(Some(24.0));
        let fallback = VirtualManifest::new("fallback".into(), ContentType::Video)
            .set_codecs("avc1.64001f")
            .set_bandwidth(5_000_000)
            .set_args([("width", 1280), ("height", 720)])
            .set_frame_rate(Some(24.0))
            .set_segment_durations(vec![5.0, 5.0]);
        let master = compile_remote_master(&[source, fallback], Uuid::nil(), "token").unwrap();
        assert!(!master.contains("/source/index.m3u8"));
        assert!(master.contains("/fallback/index.m3u8"));
    }

    #[test]
    fn browser_aac_normalizes_five_one_side_without_losing_surround() {
        assert_eq!(
            browser_aac_output(6, Some("5.1(side)")),
            BrowserAacOutput {
                channels: 6,
                layout: "5.1",
                filter: Some("pan=5.1|FL=FL|FR=FR|FC=FC|LFE=LFE|BL=SL|BR=SR"),
            }
        );
    }

    #[test]
    fn browser_aac_preserves_standard_seven_one() {
        assert_eq!(
            browser_aac_output(8, Some("7.1")),
            BrowserAacOutput {
                channels: 8,
                layout: "7.1",
                filter: None,
            }
        );
    }

    #[test]
    fn browser_aac_falls_back_to_stereo_for_unverified_layouts() {
        assert_eq!(
            browser_aac_output(6, None),
            BrowserAacOutput {
                channels: 2,
                layout: "stereo",
                filter: None,
            }
        );
        assert_eq!(fallback_audio_bitrate(2), 128_000);
    }

    #[test]
    fn direct_play_label_uses_only_reliable_stream_bitrate() {
        assert_eq!(direct_play_label(1080, None), "Direct Play (1080p)");
        assert_eq!(
            direct_play_label(1080, Some(6_277_855)),
            "Direct Play (1080p, 6.28Mb/s)"
        );
    }

    #[test]
    fn source_resolution_preference_uses_non_redundant_direct_play() {
        assert!(direct_play_is_default(
            &DefaultVideoQuality::Resolution(1080, 10_000_000),
            1080,
            true
        ));
        assert!(!direct_play_is_default(
            &DefaultVideoQuality::Resolution(720, 5_000_000),
            1080,
            true
        ));
        assert!(!direct_play_is_default(
            &DefaultVideoQuality::DirectPlay,
            1080,
            false
        ));
    }

    #[test]
    fn invalid_dimensions_are_rejected_before_capability_planning() {
        assert!(positive_metadata(Some(3840), "video width").is_ok());
        assert!(positive_metadata(Some(0), "video width").is_err());
        assert!(positive_metadata(Some(-1), "video width").is_err());
        assert!(positive_metadata(None, "video width").is_err());
    }

    #[tokio::test]
    async fn range_response_streams_only_the_requested_bytes() {
        let file = tempfile::NamedTempFile::new().unwrap();
        std::fs::write(file.path(), b"0123456789").unwrap();
        let mut headers = HeaderMap::new();
        headers.insert(header::RANGE, "bytes=2-5".parse().unwrap());
        let response = reply_with_file(
            file.path().to_string_lossy().into_owned(),
            "video/mp4",
            &headers,
            true,
        )
        .await;
        assert_eq!(response.status(), StatusCode::PARTIAL_CONTENT);
        assert_eq!(response.headers()[header::CONTENT_RANGE], "bytes 2-5/10");
        assert_eq!(
            hyper::body::to_bytes(response.into_body()).await.unwrap(),
            "2345"
        );
    }

    #[tokio::test]
    async fn terminal_transcode_errors_do_not_retry() {
        let attempts = Arc::new(AtomicUsize::new(0));
        let counter = attempts.clone();
        let result = timeout_segment(
            move || {
                counter.fetch_add(1, Ordering::SeqCst);
                async { Err::<(), _>(NightfallError::ProfileChainExhausted) }
            },
            Duration::ZERO,
            10,
        )
        .await;
        assert!(matches!(result, Err(NightfallError::ProfileChainExhausted)));
        assert_eq!(attempts.load(Ordering::SeqCst), 1);
    }
}
