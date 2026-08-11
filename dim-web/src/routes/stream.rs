use crate::{error::DimErrorWrapper, AppState};
use axum::body::Body;
use axum::extract::{Path, Query, State};
use axum::response::{IntoResponse, Json, Response};
use axum::Extension;
use dim_core::core::StateManager;
use dim_core::stream_tracking::{
    ContentType, PlannedProfile, PlannedTrack, StreamTracking, VirtualManifest,
};
use dim_core::streaming::ffprobe::{FFPStream, FFProbeCtx};
use dim_core::streaming::planner::{
    plan_video, BrowserCapabilities, PlaybackPlan, PlaybackStrategy, VideoSource,
};
use dim_core::streaming::{get_avc1_tag, get_qualities, level_to_tag};
use dim_core::utils::quality_to_label;
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
    av1_main10_bt709_1080p24_6_3mbps_fmp4: bool,
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
    headers: HeaderMap,
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
    let capabilities = BrowserCapabilities {
        av1_main10_bt709_1080p24_6_3mbps_fmp4: params.av1_main10_bt709_1080p24_6_3mbps_fmp4
            && headers
                .get(header::USER_AGENT)
                .and_then(|value| value.to_str().ok())
                .is_some_and(is_verified_chromium_151_macos),
        ..BrowserCapabilities::default()
    };
    let (tracks, plan) = build_tracks(&info, &media, &user.prefs, params.force_ass, &capabilities)?;
    let gid = Uuid::new_v4();
    stream_tracking.create_session(gid, owner, tracks).await;
    Ok(Json(json!({
        "tracks": stream_tracking.inspect(&gid, owner).await?,
        "gid": gid.to_string(),
        "playback_plan": plan,
        "probe_source": if reused_probe { "ingestion" } else { "fallback" },
    }))
    .into_response())
}

fn is_verified_chromium_151_macos(user_agent: &str) -> bool {
    user_agent.contains("Macintosh; Intel Mac OS X 10_15_7")
        && user_agent.contains(" Chrome/151.")
        && !user_agent.contains(" Edg/")
        && !user_agent.contains(" OPR/")
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
            if info.get_primary("video").is_some()
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
) -> Result<(Vec<PlannedTrack>, PlaybackPlan), DimErrorWrapper> {
    let video = info
        .get_primary("video")
        .cloned()
        .ok_or(dim_core::errors::StreamingErrors::FileIsCorrupt)?;
    let height = video
        .height
        .and_then(|value| u64::try_from(value).ok())
        .filter(|value| *value > 0)
        .ok_or_else(|| {
            dim_core::errors::StreamingErrors::InvalidMetadata("video height is missing".into())
        })?;
    let width = video
        .width
        .and_then(|value| u64::try_from(value).ok())
        .filter(|value| *value > 0)
        .ok_or_else(|| {
            dim_core::errors::StreamingErrors::InvalidMetadata("video width is missing".into())
        })?;
    let bitrate = video
        .get_bitrate()
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
    let frame_rate = video.frame_rate().unwrap_or(30);
    let plan = plan_video(
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
        },
        capabilities,
    );
    let mut tracks = Vec::new();

    if plan.direct_play_supported {
        let id = Uuid::new_v4().to_string();
        let (output_codec, codec_tag) = if video.codec_name == "av1" {
            ("av1", "av01.0.08M.10.0.111.01.01.01.0".to_string())
        } else {
            let codec = video
                .level
                .and_then(level_to_tag)
                .unwrap_or_else(|| get_avc1_tag(width, height, bitrate, frame_rate));
            ("h264", codec.to_string())
        };
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
                .set_is_default(matches!(
                    prefs.default_video_quality,
                    DefaultVideoQuality::DirectPlay
                ))
                .set_target_duration(10)
                .set_label(format!("{height}p (Direct Play)")),
            context,
            profile: PlannedProfile::DirectVideo,
        });
    }

    let mut assigned_default = plan.preferred_strategy == PlaybackStrategy::DirectPlay
        && matches!(prefs.default_video_quality, DefaultVideoQuality::DirectPlay);
    let preferred_resolution_available = match prefs.default_video_quality {
        DefaultVideoQuality::Resolution(resolution, _) => plan
            .renditions
            .iter()
            .any(|quality| quality.height == resolution),
        DefaultVideoQuality::DirectPlay => plan.direct_play_supported,
    };
    for quality in get_qualities(height, bitrate) {
        let target_width =
            ((width as f64 * quality.height as f64 / height as f64).round() as u64).max(2) & !1;
        let is_preferred_resolution = matches!(prefs.default_video_quality, DefaultVideoQuality::Resolution(res, _) if res == quality.height);
        let first_transcode = tracks.iter().all(|track| track.manifest.is_direct);
        let is_default = !assigned_default
            && (is_preferred_resolution || first_transcode && !preferred_resolution_available);
        assigned_default |= is_default;
        let mut input_ctx: nightfall::profiles::InputCtx = video.clone().into();
        input_ctx.fps = frame_rate as f64;
        let context = ProfileContext {
            file: media.target_file.clone(),
            input_ctx,
            output_ctx: OutputCtx {
                codec: "h264".into(),
                start_num: 0,
                bitrate: Some(quality.bitrate),
                width: Some(target_width as i64),
                height: Some(quality.height as i64),
                ..Default::default()
            },
            ..Default::default()
        };
        tracks.push(PlannedTrack {
            manifest: VirtualManifest::new(Uuid::new_v4().to_string(), ContentType::Video)
                .set_mime("video/mp4")
                .set_duration(Some(duration))
                .set_codecs(
                    get_avc1_tag(target_width, quality.height, quality.bitrate, frame_rate)
                        .to_string(),
                )
                .set_bandwidth(quality.bitrate)
                .set_args([("width", target_width), ("height", quality.height)])
                .set_is_default(is_default)
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
        let channels = audio
            .channels
            .and_then(|value| u64::try_from(value).ok())
            .filter(|value| *value > 0);
        let bitrate = audio
            .bit_rate
            .as_deref()
            .and_then(|value| value.parse::<u64>().ok())
            .or_else(|| audio.get_bitrate())
            .unwrap_or_else(|| fallback_audio_bitrate(channels.unwrap_or(2)));
        let language = audio.get_language();
        let is_default = info.get_primary("audio") == Some(audio);
        let context = ProfileContext {
            file: media.target_file.clone(),
            input_ctx: audio.clone().into(),
            output_ctx: OutputCtx {
                codec: "aac".into(),
                start_num: 0,
                bitrate: Some(bitrate),
                audio_channels: channels.unwrap_or(2),
                ..Default::default()
            },
            ..Default::default()
        };
        let label = format!(
            "{} ({} {})",
            language
                .as_deref()
                .and_then(dim_core::utils::lang_from_iso639)
                .unwrap_or("Unknown"),
            dim_core::utils::codec_pretty(audio.get_codec()),
            dim_core::utils::channels_pretty(audio.channels.unwrap_or(2))
        );
        tracks.push(PlannedTrack {
            manifest: VirtualManifest::new(Uuid::new_v4().to_string(), ContentType::Audio)
                .set_mime("audio/mp4")
                .set_duration(Some(duration))
                .set_codecs("mp4a.40.2")
                .set_bandwidth(bitrate)
                .set_is_default(is_default)
                .set_label(label)
                .set_lang(language)
                .set_audio_channels(channels),
            context,
            profile: PlannedProfile::Audio,
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

fn fallback_audio_bitrate(channels: u64) -> u64 {
    // The native AAC encoder receives one total bitrate for every channel. Keep the established
    // 128 kb/s stereo floor, but do not divide that same budget across larger channel layouts.
    channels.saturating_mul(64_000).max(128_000)
}

#[derive(Deserialize)]
pub struct ManifestParams {
    start_num: Option<u64>,
    #[allow(dead_code)]
    should_kill: Option<bool>,
    includes: Option<String>,
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
    let manifest = stream_tracking
        .activate_and_compile(
            &state,
            &gid,
            user.id.get(),
            params.start_num.unwrap_or(0),
            includes,
        )
        .await?;
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
    fn fallback_audio_bitrate_scales_with_channel_count() {
        assert_eq!(fallback_audio_bitrate(1), 128_000);
        assert_eq!(fallback_audio_bitrate(2), 128_000);
        assert_eq!(fallback_audio_bitrate(6), 384_000);
        assert_eq!(fallback_audio_bitrate(8), 512_000);
    }

    #[test]
    fn av1_client_evidence_is_limited_to_the_verified_browser_runtime() {
        let verified = "Mozilla/5.0 (Macintosh; Intel Mac OS X 10_15_7) \
            AppleWebKit/537.36 (KHTML, like Gecko) Chrome/151.0.0.0 Safari/537.36";
        assert!(is_verified_chromium_151_macos(verified));
        assert!(!is_verified_chromium_151_macos(
            &verified.replace("Chrome/151", "Chrome/152")
        ));
        assert!(!is_verified_chromium_151_macos(&verified.replace(
            "Macintosh; Intel Mac OS X 10_15_7",
            "Windows NT 10.0"
        )));
        assert!(!is_verified_chromium_151_macos(&format!(
            "{verified} Edg/151.0"
        )));
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
