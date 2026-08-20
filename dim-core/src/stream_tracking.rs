use std::collections::{HashMap, HashSet};
use std::sync::Arc;
use std::time::{Duration, Instant};

use crate::core::StateManager;
use crate::utils::ts_to_xml;
use nightfall::profiles::{
    get_profile_for, get_profile_for_with_type, ProfileContext, ProfileType, StreamType,
};
use serde::{Deserialize, Serialize};
use tokio::sync::RwLock;
use uuid::Uuid;
use xmlwriter::*;

#[derive(Debug, Clone, Copy, Eq, PartialEq, Serialize)]
#[serde(rename_all = "lowercase")]
pub enum ContentType {
    Video,
    Audio,
    Subtitle,
}

#[derive(Debug, Clone, Copy, Eq, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum RemotePlaybackState {
    Prepared,
    HandoffRequested,
    WirelessRouteReported,
    MediaDeliveryConfirmed,
    HandoffStalled,
    Failed,
    Disconnected,
}

#[derive(Debug, Clone, Copy, Eq, PartialEq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum RemoteRequestAttribution {
    SenderPreflight,
    SenderOrLocalProxy,
    AppleMediaIntermediaryCandidate,
    RemoteNetworkCandidate,
    OriginUnresolved,
    DisconnectedOrStale,
}

#[derive(Debug, Clone, Copy, Eq, PartialEq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum RemoteHlsStage {
    MasterPlaylist,
    MediaPlaylist,
    InitFragment,
    MediaSegment,
    Unknown,
}

#[derive(Debug, Clone, Serialize)]
pub struct RemotePlaybackStatus {
    pub state: RemotePlaybackState,
    pub handoff_elapsed_ms: Option<u128>,
    pub successful_remote_inits: usize,
    pub successful_remote_segments: usize,
    pub last_request_attribution: Option<RemoteRequestAttribution>,
    pub last_request_stage: Option<RemoteHlsStage>,
}

impl std::fmt::Display for ContentType {
    fn fmt(&self, fmt: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(
            fmt,
            "{}",
            match self {
                Self::Audio => "audio",
                Self::Subtitle => "text",
                Self::Video => "video",
            }
        )
    }
}

#[derive(Debug, Clone, Serialize)]
pub struct VirtualManifest {
    pub content_type: ContentType,
    /// Stable public selection id. The Nightfall id is intentionally private.
    pub id: String,
    pub set_id: usize,
    pub is_direct: bool,
    pub mime: String,
    pub codecs: String,
    pub bandwidth: u64,
    pub average_bandwidth: u64,
    #[serde(flatten)]
    pub args: HashMap<String, String>,
    pub duration: Option<i32>,
    pub chunk_path: String,
    pub init_seg: Option<String>,
    pub is_default: bool,
    pub label: String,
    pub lang: Option<String>,
    pub target_duration: u32,
    pub audio_channels: Option<u64>,
    pub frame_rate: Option<f64>,
    pub video_range: Option<String>,
    /// The media timeline advertised to HLS clients. It is representation-specific and is not
    /// part of the browser/DASH manifest API.
    #[serde(skip)]
    pub segment_durations: Vec<f64>,
}

impl VirtualManifest {
    pub fn new(id: String, content_type: ContentType) -> Self {
        Self {
            id,
            content_type,
            set_id: 0,
            is_direct: false,
            is_default: false,
            mime: String::new(),
            codecs: String::new(),
            bandwidth: 0,
            average_bandwidth: 0,
            args: Default::default(),
            duration: None,
            label: String::new(),
            lang: None,
            target_duration: 5,
            chunk_path: String::new(),
            init_seg: None,
            audio_channels: None,
            frame_rate: None,
            video_range: None,
            segment_durations: Vec::new(),
        }
    }
    pub fn set_direct(mut self) -> Self {
        self.is_direct = true;
        self
    }
    pub fn set_mime(mut self, value: impl Into<String>) -> Self {
        self.mime = value.into();
        self
    }
    pub fn set_codecs(mut self, value: impl Into<String>) -> Self {
        self.codecs = value.into();
        self
    }
    pub fn set_bandwidth(mut self, value: u64) -> Self {
        self.bandwidth = value;
        self.average_bandwidth = value;
        self
    }
    pub fn set_average_bandwidth(mut self, value: u64) -> Self {
        self.average_bandwidth = value;
        self
    }
    pub fn set_duration(mut self, value: Option<i32>) -> Self {
        self.duration = value;
        self
    }
    pub fn set_args(
        mut self,
        values: impl IntoIterator<Item = (impl ToString, impl ToString)>,
    ) -> Self {
        for (k, v) in values {
            self.args.insert(k.to_string(), v.to_string());
        }
        self
    }
    pub fn set_is_default(mut self, value: bool) -> Self {
        self.is_default = value;
        self
    }
    pub fn set_label(mut self, value: String) -> Self {
        self.label = value;
        self
    }
    pub fn set_lang(mut self, value: Option<String>) -> Self {
        self.lang = value;
        self
    }
    pub fn set_target_duration(mut self, value: u32) -> Self {
        self.target_duration = value;
        self
    }
    pub fn set_audio_channels(mut self, value: Option<u64>) -> Self {
        self.audio_channels = value;
        self
    }
    pub fn set_frame_rate(mut self, value: Option<f64>) -> Self {
        self.frame_rate = value;
        self
    }
    pub fn set_video_range(mut self, value: Option<impl Into<String>>) -> Self {
        self.video_range = value.map(Into::into);
        self
    }
    pub fn set_segment_durations(mut self, value: Vec<f64>) -> Self {
        self.segment_durations = value;
        self
    }
    pub fn set_chunk_path(mut self, value: impl Into<String>) -> Self {
        self.chunk_path = value.into();
        self
    }

    fn activated(&self, process_id: &str) -> Self {
        let mut manifest = self.clone();
        manifest.chunk_path = match manifest.content_type {
            ContentType::Subtitle if manifest.mime == "text/ass" => {
                format!("{process_id}/data/stream.ass")
            }
            ContentType::Subtitle => format!("{process_id}/data/stream.vtt"),
            _ => format!("{process_id}/data/$Number$.m4s"),
        };
        manifest.init_seg = (!matches!(manifest.content_type, ContentType::Subtitle))
            .then(|| format!("{process_id}/data/init.mp4"));
        manifest
    }

    fn compile(&self, writer: &mut XmlWriter, start_num: u64) {
        writer.start_element("AdaptationSet");
        writer.write_attribute("contentType", &self.content_type.to_string());
        writer.write_attribute("id", &self.set_id);
        if let Some(lang) = self.lang.as_ref() {
            writer.write_attribute("lang", lang);
        }

        writer.start_element("Representation");
        writer.write_attribute("id", &self.id);
        writer.write_attribute("bandwidth", &self.bandwidth);
        writer.write_attribute("mimeType", &self.mime);
        if !self.codecs.is_empty() {
            writer.write_attribute("codecs", &self.codecs);
        }
        for (key, value) in &self.args {
            writer.write_attribute(key, value);
        }

        if self.content_type == ContentType::Audio {
            writer.start_element("AudioChannelConfiguration");
            writer.write_attribute(
                "schemeIdUri",
                "urn:mpeg:dash:23003:3:audio_channel_configuration:2011",
            );
            writer.write_attribute("value", &self.audio_channels.unwrap_or(2));
            writer.end_element();
        }
        if self.is_default {
            writer.start_element("Role");
            writer.write_attribute("schemeIdUri", "urn:mpeg:dash:role:2011");
            writer.write_attribute("value", "main");
            writer.end_element();
        }

        if self.content_type == ContentType::Subtitle {
            writer.start_element("BaseURL");
            writer.write_text(&self.chunk_path);
            writer.end_element();
        } else if let Some(init) = self.init_seg.as_ref() {
            writer.start_element("SegmentTemplate");
            writer.write_attribute("timescale", &1);
            writer.write_attribute("duration", &self.target_duration);
            writer.write_attribute("initialization", &format!("{init}?start_num={start_num}"));
            writer.write_attribute("media", &self.chunk_path);
            writer.write_attribute("startNumber", &start_num);
            writer.end_element();
        }
        writer.end_element();
        writer.end_element();
    }
}

#[derive(Clone, Debug)]
pub enum PlannedProfile {
    DirectVideo,
    DirectAudio,
    Video,
    Audio,
    Subtitle,
}

#[derive(Clone, Debug)]
pub struct PlannedTrack {
    pub manifest: VirtualManifest,
    pub context: ProfileContext,
    pub profile: PlannedProfile,
}

#[derive(Clone, Copy, Debug)]
pub struct TranscodePolicy {
    pub global_limit: usize,
    pub per_user_limit: usize,
    pub per_session_limit: usize,
    pub session_ttl: Duration,
}

impl Default for TranscodePolicy {
    fn default() -> Self {
        fn env_usize(name: &str, legacy_name: &str, default: usize) -> usize {
            std::env::var(name)
                .or_else(|_| std::env::var(legacy_name))
                .ok()
                .and_then(|v| v.parse().ok())
                .filter(|v| *v > 0)
                .unwrap_or(default)
        }
        Self {
            global_limit: env_usize(
                "ECLIPSE_TRANSCODE_GLOBAL_LIMIT",
                "DIM_TRANSCODE_GLOBAL_LIMIT",
                8,
            ),
            per_user_limit: env_usize(
                "ECLIPSE_TRANSCODE_USER_LIMIT",
                "DIM_TRANSCODE_USER_LIMIT",
                4,
            ),
            per_session_limit: env_usize(
                "ECLIPSE_TRANSCODE_SESSION_LIMIT",
                "DIM_TRANSCODE_SESSION_LIMIT",
                3,
            ),
            session_ttl: Duration::from_secs(env_usize(
                "ECLIPSE_STREAM_SESSION_TTL_SECS",
                "DIM_STREAM_SESSION_TTL_SECS",
                30 * 60,
            ) as u64),
        }
    }
}

#[derive(Debug, thiserror::Error, PartialEq, Eq)]
pub enum TrackingError {
    #[error("streaming session was not found")]
    NotFound,
    #[error("streaming session belongs to another user")]
    NotOwner,
    #[error("playback lifecycle generation does not own this session")]
    LifecycleMismatch,
    #[error("invalid or empty track selection")]
    InvalidSelection,
    #[error(
        "transcoding capacity reached ({scope} limit {limit}); stop another stream or retry later"
    )]
    AdmissionLimited { scope: &'static str, limit: usize },
    #[error("transcoder failed: {0}")]
    Transcoder(String),
    #[error("media duration is missing or invalid")]
    InvalidMetadata,
}

#[derive(Debug, Clone, Default)]
pub struct PlaybackLifecycle {
    pub frontend_instance_id: Option<String>,
    pub media_file_id: Option<i64>,
    pub source_generation: Option<u64>,
    pub creation_reason: String,
}

#[derive(Debug, Clone)]
pub struct PlaybackTeardown {
    pub reason: String,
    pub caller: String,
    pub frontend_instance_id: Option<String>,
    pub source_generation: Option<u64>,
}

impl PlaybackTeardown {
    pub fn server(reason: impl Into<String>, caller: impl Into<String>) -> Self {
        Self {
            reason: reason.into(),
            caller: caller.into(),
            frontend_instance_id: None,
            source_generation: None,
        }
    }
}

fn check_admission(
    policy: TranscodePolicy,
    global_active: usize,
    user_active: usize,
    session_active: usize,
    requested: usize,
) -> Result<(), TrackingError> {
    for (scope, active, limit) in [
        ("global", global_active, policy.global_limit),
        ("user", user_active, policy.per_user_limit),
        ("session", session_active, policy.per_session_limit),
    ] {
        if active + requested > limit {
            return Err(TrackingError::AdmissionLimited { scope, limit });
        }
    }
    Ok(())
}

#[derive(Debug)]
struct TrackState {
    plan: PlannedTrack,
    process_id: Option<String>,
}
#[derive(Debug)]
struct Session {
    owner: i64,
    created_at: Instant,
    last_activity: Instant,
    admitted_at: Option<Instant>,
    lifecycle: PlaybackLifecycle,
    tracks: Vec<TrackState>,
    remote_access_token: Option<String>,
    remote_playback_state: RemotePlaybackState,
    handoff_started_at: Option<Instant>,
    successful_remote_inits: HashSet<String>,
    successful_remote_segments: HashSet<String>,
    last_request_attribution: Option<RemoteRequestAttribution>,
    last_request_stage: Option<RemoteHlsStage>,
}
#[derive(Debug)]
struct Inner {
    sessions: HashMap<Uuid, Session>,
    process_index: HashMap<String, Uuid>,
}

#[derive(Debug, Clone)]
pub struct StreamTracking {
    inner: Arc<RwLock<Inner>>,
    policy: TranscodePolicy,
}

impl StreamTracking {
    const HANDOFF_STALL_AFTER: Duration = Duration::from_secs(15);
    const HANDOFF_CLEANUP_AFTER: Duration = Duration::from_secs(30);

    pub fn with_policy(policy: TranscodePolicy) -> Self {
        Self {
            inner: Arc::new(RwLock::new(Inner {
                sessions: HashMap::new(),
                process_index: HashMap::new(),
            })),
            policy,
        }
    }

    pub async fn create_session(&self, gid: Uuid, owner: i64, tracks: Vec<PlannedTrack>) {
        self.create_session_with_lifecycle(gid, owner, tracks, PlaybackLifecycle::default())
            .await;
    }

    pub async fn create_session_with_lifecycle(
        &self,
        gid: Uuid,
        owner: i64,
        tracks: Vec<PlannedTrack>,
        lifecycle: PlaybackLifecycle,
    ) {
        let mut inner = self.inner.write().await;
        tracing::info!(
            session_id = %gid,
            owner,
            track_count = tracks.len(),
            frontend_instance_id = ?lifecycle.frontend_instance_id,
            media_file_id = ?lifecycle.media_file_id,
            source_generation = ?lifecycle.source_generation,
            creation_reason = lifecycle.creation_reason,
            "Playback session created"
        );
        inner.sessions.insert(
            gid,
            Session {
                owner,
                created_at: Instant::now(),
                last_activity: Instant::now(),
                admitted_at: None,
                lifecycle,
                tracks: tracks
                    .into_iter()
                    .enumerate()
                    .map(|(set_id, mut plan)| {
                        plan.manifest.set_id = set_id;
                        TrackState {
                            plan,
                            process_id: None,
                        }
                    })
                    .collect(),
                remote_access_token: None,
                remote_playback_state: RemotePlaybackState::Prepared,
                handoff_started_at: None,
                successful_remote_inits: HashSet::new(),
                successful_remote_segments: HashSet::new(),
                last_request_attribution: None,
                last_request_stage: None,
            },
        );
    }

    pub async fn enable_remote_access(
        &self,
        gid: &Uuid,
        owner: i64,
    ) -> Result<String, TrackingError> {
        let mut inner = self.inner.write().await;
        let session = inner.sessions.get_mut(gid).ok_or(TrackingError::NotFound)?;
        if session.owner != owner {
            return Err(TrackingError::NotOwner);
        }
        let token = Uuid::new_v4().as_simple().to_string();
        session.remote_access_token = Some(token.clone());
        tracing::info!(session_id = %gid, owner, "Remote playback token issued");
        Ok(token)
    }

    pub async fn set_remote_playback_state(
        &self,
        gid: &Uuid,
        owner: i64,
        state: RemotePlaybackState,
    ) -> Result<(), TrackingError> {
        let mut inner = self.inner.write().await;
        let session = inner.sessions.get_mut(gid).ok_or(TrackingError::NotFound)?;
        if session.owner != owner {
            return Err(TrackingError::NotOwner);
        }
        if session.remote_playback_state == state {
            return Ok(());
        }
        // A picker request and WebKit's route callback are reported independently. Ignore a late
        // intent update after the stronger route signal, but reject client attempts to claim
        // server-derived delivery/stall states or revive a terminal session.
        if session.remote_playback_state == RemotePlaybackState::WirelessRouteReported
            && state == RemotePlaybackState::HandoffRequested
        {
            return Ok(());
        }
        let transition_allowed = match (session.remote_playback_state, state) {
            (
                RemotePlaybackState::Prepared,
                RemotePlaybackState::HandoffRequested
                | RemotePlaybackState::WirelessRouteReported
                | RemotePlaybackState::Failed
                | RemotePlaybackState::Disconnected,
            )
            | (
                RemotePlaybackState::HandoffRequested,
                RemotePlaybackState::WirelessRouteReported
                | RemotePlaybackState::Failed
                | RemotePlaybackState::Disconnected,
            )
            | (
                RemotePlaybackState::WirelessRouteReported,
                RemotePlaybackState::Failed | RemotePlaybackState::Disconnected,
            )
            | (
                RemotePlaybackState::MediaDeliveryConfirmed,
                RemotePlaybackState::Failed | RemotePlaybackState::Disconnected,
            ) => true,
            _ => false,
        };
        if !transition_allowed {
            return Err(TrackingError::InvalidSelection);
        }
        if matches!(
            state,
            RemotePlaybackState::HandoffRequested | RemotePlaybackState::WirelessRouteReported
        ) && session.handoff_started_at.is_none()
        {
            session.handoff_started_at = Some(Instant::now());
            session.successful_remote_inits.clear();
            session.successful_remote_segments.clear();
        }
        session.remote_playback_state = state;
        session.last_activity = Instant::now();
        tracing::info!(session_id = %gid, owner, remote_playback_state = ?state, "AirPlay playback state updated");
        Ok(())
    }

    pub async fn remote_playback_state(&self, gid: &Uuid) -> Option<RemotePlaybackState> {
        self.inner
            .read()
            .await
            .sessions
            .get(gid)
            .map(|session| session.remote_playback_state)
    }

    pub async fn remote_playback_status(
        &self,
        gid: &Uuid,
        owner: i64,
    ) -> Result<RemotePlaybackStatus, TrackingError> {
        let mut inner = self.inner.write().await;
        let session = inner.sessions.get_mut(gid).ok_or(TrackingError::NotFound)?;
        if session.owner != owner {
            return Err(TrackingError::NotOwner);
        }
        let elapsed = session.handoff_started_at.map(|started| started.elapsed());
        if matches!(
            session.remote_playback_state,
            RemotePlaybackState::HandoffRequested | RemotePlaybackState::WirelessRouteReported
        ) && elapsed.is_some_and(|value| value >= Self::HANDOFF_STALL_AFTER)
        {
            session.remote_playback_state = RemotePlaybackState::HandoffStalled;
            tracing::warn!(
                session_id = %gid,
                owner,
                successful_remote_inits = session.successful_remote_inits.len(),
                successful_remote_segments = session.successful_remote_segments.len(),
                "AirPlay handoff stalled without confirmed remote media delivery"
            );
        }
        session.last_activity = Instant::now();
        Ok(RemotePlaybackStatus {
            state: session.remote_playback_state,
            handoff_elapsed_ms: elapsed.map(|value| value.as_millis()),
            successful_remote_inits: session.successful_remote_inits.len(),
            successful_remote_segments: session.successful_remote_segments.len(),
            last_request_attribution: session.last_request_attribution,
            last_request_stage: session.last_request_stage,
        })
    }

    pub async fn observe_remote_hls_response(
        &self,
        gid: &Uuid,
        stage: RemoteHlsStage,
        attribution: RemoteRequestAttribution,
        path: &str,
        successful: bool,
    ) {
        let mut inner = self.inner.write().await;
        let Some(session) = inner.sessions.get_mut(gid) else {
            return;
        };
        session.last_request_attribution = Some(attribution);
        session.last_request_stage = Some(stage);
        if !successful
            || session.remote_playback_state != RemotePlaybackState::WirelessRouteReported
            || !matches!(
                attribution,
                RemoteRequestAttribution::AppleMediaIntermediaryCandidate
                    | RemoteRequestAttribution::RemoteNetworkCandidate
            )
        {
            return;
        }
        match stage {
            RemoteHlsStage::InitFragment => {
                session.successful_remote_inits.insert(path.to_owned());
            }
            RemoteHlsStage::MediaSegment => {
                session.successful_remote_segments.insert(path.to_owned());
            }
            _ => {}
        }
        // Two distinct successful post-route segments demonstrate sustained remote-bound media
        // delivery. This is intentionally not called proof that a physical display rendered it.
        if session.successful_remote_segments.len() >= 2 {
            session.remote_playback_state = RemotePlaybackState::MediaDeliveryConfirmed;
            tracing::info!(
                session_id = %gid,
                owner = session.owner,
                attribution = ?attribution,
                successful_remote_inits = session.successful_remote_inits.len(),
                successful_remote_segments = session.successful_remote_segments.len(),
                "AirPlay remote media delivery confirmed"
            );
        }
    }

    pub async fn authenticate_remote(&self, gid: &Uuid, token: &str) -> Result<i64, TrackingError> {
        let mut inner = self.inner.write().await;
        let Some(session) = inner.sessions.get_mut(gid) else {
            tracing::warn!(session_id = %gid, "Remote playback token rejected: session not found");
            return Err(TrackingError::NotFound);
        };
        if session.remote_access_token.as_deref() != Some(token) {
            tracing::warn!(session_id = %gid, owner = session.owner, "Remote playback token rejected: token mismatch");
            return Err(TrackingError::NotOwner);
        }
        session.last_activity = Instant::now();
        tracing::debug!(session_id = %gid, owner = session.owner, "Remote playback token accepted");
        Ok(session.owner)
    }

    pub async fn remote_track(
        &self,
        gid: &Uuid,
        owner: i64,
        public_id: &str,
    ) -> Result<(VirtualManifest, Option<String>), TrackingError> {
        let mut inner = self.inner.write().await;
        let session = inner.sessions.get_mut(gid).ok_or(TrackingError::NotFound)?;
        if session.owner != owner {
            return Err(TrackingError::NotOwner);
        }
        session.last_activity = Instant::now();
        let track = session
            .tracks
            .iter()
            .find(|track| track.plan.manifest.id == public_id)
            .ok_or(TrackingError::NotFound)?;
        Ok((track.plan.manifest.clone(), track.process_id.clone()))
    }

    pub async fn inspect(
        &self,
        gid: &Uuid,
        owner: i64,
    ) -> Result<Vec<VirtualManifest>, TrackingError> {
        let mut inner = self.inner.write().await;
        let session = inner.sessions.get_mut(gid).ok_or(TrackingError::NotFound)?;
        if session.owner != owner {
            return Err(TrackingError::NotOwner);
        }
        tracing::debug!(session_id = %gid, owner, age_ms = session.created_at.elapsed().as_millis(), "Playback manifest activity");
        session.last_activity = Instant::now();
        Ok(session
            .tracks
            .iter()
            .map(|track| track.plan.manifest.clone())
            .collect())
    }

    pub async fn owner_for_process(
        &self,
        process_id: &str,
        owner: i64,
    ) -> Result<Uuid, TrackingError> {
        let mut inner = self.inner.write().await;
        let gid = *inner
            .process_index
            .get(process_id)
            .ok_or(TrackingError::NotFound)?;
        let session = inner
            .sessions
            .get_mut(&gid)
            .ok_or(TrackingError::NotFound)?;
        if session.owner != owner {
            return Err(TrackingError::NotOwner);
        }
        tracing::debug!(session_id = %gid, owner, process_id, age_ms = session.created_at.elapsed().as_millis(), "Playback segment activity");
        session.last_activity = Instant::now();
        Ok(gid)
    }

    pub async fn activate_public_track(
        &self,
        state: &StateManager,
        public_id: &str,
        owner: i64,
    ) -> Result<String, TrackingError> {
        let mut inner = self.inner.write().await;
        let gid = inner
            .sessions
            .iter()
            .find_map(|(gid, session)| {
                (session.owner == owner
                    && session
                        .tracks
                        .iter()
                        .any(|track| track.plan.manifest.id == public_id))
                .then_some(*gid)
            })
            .ok_or(TrackingError::NotFound)?;
        self.activate_locked(state, &mut inner, gid, owner, &[public_id.to_string()])
            .await?;
        let session = inner.sessions.get(&gid).ok_or(TrackingError::NotFound)?;
        Ok(session
            .tracks
            .iter()
            .find(|track| track.plan.manifest.id == public_id)
            .and_then(|track| track.process_id.clone())
            .ok_or(TrackingError::NotFound)?)
    }

    /// Activate a track selected by a remote HLS client.
    ///
    /// An HLS receiver may inspect one video rendition and then select another. A remote
    /// playback session owns one active video recipe, so an init-fragment request for a new
    /// rendition replaces the previous recipe instead of consuming another admission slot.
    /// Segment requests cannot initiate that replacement: this prevents late requests for the
    /// retired rendition from switching playback back during an ABR transition.
    pub async fn activate_remote_track(
        &self,
        state: &StateManager,
        gid: &Uuid,
        public_id: &str,
        owner: i64,
        allow_video_replacement: bool,
    ) -> Result<String, TrackingError> {
        let mut inner = self.inner.write().await;
        let session = inner.sessions.get(gid).ok_or(TrackingError::NotFound)?;
        if session.owner != owner {
            return Err(TrackingError::NotOwner);
        }
        let requested = session
            .tracks
            .iter()
            .find(|track| track.plan.manifest.id == public_id)
            .ok_or(TrackingError::NotFound)?;
        if let Some(process_id) = requested.process_id.clone() {
            return Ok(process_id);
        }

        let handoff_identity = (
            session.lifecycle.frontend_instance_id.clone(),
            session.lifecycle.media_file_id,
            session.lifecycle.source_generation,
        );
        let transfer_local_capacity = session.remote_access_token.is_some()
            && session.remote_playback_state == RemotePlaybackState::WirelessRouteReported
            && handoff_identity.0.is_some();

        let retiring = if requested.plan.manifest.content_type == ContentType::Video {
            session
                .tracks
                .iter()
                .enumerate()
                .filter(|(_, track)| {
                    track.plan.manifest.content_type == ContentType::Video
                        && track.plan.manifest.id != public_id
                })
                .filter_map(|(index, track)| Some((index, track.process_id.clone()?)))
                .collect::<Vec<_>>()
        } else {
            Vec::new()
        };
        if !retiring.is_empty() && !allow_video_replacement {
            tracing::debug!(
                session_id = %gid,
                owner,
                track_id = public_id,
                "Ignored late remote HLS request for an inactive video rendition"
            );
            return Err(TrackingError::InvalidSelection);
        }

        // The sender and receiver use different plans, but one frontend playback instance owns
        // both sessions. Once WebKit reports a wireless route, atomically retire the matching
        // local session's processes before admitting the first remote rendition. This prevents
        // a legitimate handoff from being rejected merely because local and remote plans briefly
        // overlap at the per-user admission boundary. Prepared remote sessions and sender
        // preflight requests are deliberately not allowed to claim this transfer.
        let handoff_retiring = if transfer_local_capacity {
            inner
                .sessions
                .iter()
                .filter(|(candidate_gid, candidate)| {
                    *candidate_gid != gid
                        && candidate.owner == owner
                        && candidate.remote_access_token.is_none()
                        && candidate.lifecycle.frontend_instance_id == handoff_identity.0
                        && candidate.lifecycle.media_file_id == handoff_identity.1
                        && candidate.lifecycle.source_generation == handoff_identity.2
                })
                .flat_map(|(candidate_gid, candidate)| {
                    candidate
                        .tracks
                        .iter()
                        .enumerate()
                        .filter_map(|(index, track)| {
                            Some((*candidate_gid, index, track.process_id.clone()?))
                        })
                })
                .collect::<Vec<_>>()
        } else {
            Vec::new()
        };

        // Admit against the post-swap process count. StateManager::create registers a lazy
        // Nightfall process; FFmpeg starts only after this method returns and the init request is
        // dispatched, so the retiring and replacement transcoders do not run concurrently.
        for (index, process_id) in &retiring {
            inner.process_index.remove(process_id);
            inner.sessions.get_mut(gid).unwrap().tracks[*index].process_id = None;
        }
        for (candidate_gid, index, process_id) in &handoff_retiring {
            inner.process_index.remove(process_id);
            inner.sessions.get_mut(candidate_gid).unwrap().tracks[*index].process_id = None;
        }
        if let Err(error) = self
            .activate_locked(state, &mut inner, *gid, owner, &[public_id.to_string()])
            .await
        {
            for (index, process_id) in &retiring {
                inner.process_index.insert(process_id.clone(), *gid);
                inner.sessions.get_mut(gid).unwrap().tracks[*index].process_id =
                    Some(process_id.clone());
            }
            for (candidate_gid, index, process_id) in &handoff_retiring {
                inner
                    .process_index
                    .insert(process_id.clone(), *candidate_gid);
                inner.sessions.get_mut(candidate_gid).unwrap().tracks[*index].process_id =
                    Some(process_id.clone());
            }
            return Err(error);
        }
        let process_id = inner.sessions[gid]
            .tracks
            .iter()
            .find(|track| track.plan.manifest.id == public_id)
            .and_then(|track| track.process_id.clone())
            .ok_or(TrackingError::NotFound)?;
        drop(inner);

        if !handoff_retiring.is_empty() {
            tracing::info!(
                session_id = %gid,
                owner,
                frontend_instance_id = ?handoff_identity.0,
                retired_process_count = handoff_retiring.len(),
                new_process_id = process_id,
                "AirPlay handoff transferred playback capacity from the local session"
            );
        }
        for (local_gid, _, retired_process_id) in handoff_retiring {
            if let Err(error) = state.die(retired_process_id.clone()).await {
                tracing::warn!(
                    session_id = %gid,
                    local_session_id = %local_gid,
                    owner,
                    process_id = retired_process_id,
                    %error,
                    "Retired local playback process cleanup failed during AirPlay handoff"
                );
            }
        }

        for (_, retired_process_id) in retiring {
            tracing::info!(
                session_id = %gid,
                owner,
                track_id = public_id,
                old_process_id = retired_process_id,
                new_process_id = process_id,
                "Remote HLS video rendition replaced"
            );
            if let Err(error) = state.die(retired_process_id.clone()).await {
                tracing::warn!(
                    session_id = %gid,
                    owner,
                    process_id = retired_process_id,
                    %error,
                    "Retired remote video process cleanup failed"
                );
            }
        }
        Ok(process_id)
    }

    async fn activate_locked(
        &self,
        state: &StateManager,
        inner: &mut Inner,
        gid: Uuid,
        owner: i64,
        includes: &[String],
    ) -> Result<(), TrackingError> {
        let selected = includes.iter().map(String::as_str).collect::<HashSet<_>>();
        let session = inner.sessions.get(&gid).ok_or(TrackingError::NotFound)?;
        if session.owner != owner {
            return Err(TrackingError::NotOwner);
        }
        if selected.is_empty()
            || selected.iter().any(|id| {
                !session
                    .tracks
                    .iter()
                    .any(|track| track.plan.manifest.id == *id)
            })
        {
            return Err(TrackingError::InvalidSelection);
        }
        let requested = session
            .tracks
            .iter()
            .filter(|track| {
                selected.contains(track.plan.manifest.id.as_str()) && track.process_id.is_none()
            })
            .count();
        let global_active = inner.process_index.len();
        let user_active = inner
            .sessions
            .values()
            .filter(|entry| entry.owner == owner)
            .flat_map(|entry| &entry.tracks)
            .filter(|track| track.process_id.is_some())
            .count();
        let session_active = session
            .tracks
            .iter()
            .filter(|track| track.process_id.is_some())
            .count();
        tracing::info!(
            session_id = %gid,
            owner,
            frontend_instance_id = ?session.lifecycle.frontend_instance_id,
            media_file_id = ?session.lifecycle.media_file_id,
            source_generation = ?session.lifecycle.source_generation,
            global_active,
            user_active,
            session_active,
            requested,
            "Playback admission requested"
        );
        if let Err(error) = check_admission(
            self.policy,
            global_active,
            user_active,
            session_active,
            requested,
        ) {
            tracing::warn!(session_id = %gid, owner, %error, "Playback admission rejected");
            return Err(error);
        }

        let session = inner
            .sessions
            .get_mut(&gid)
            .ok_or(TrackingError::NotFound)?;
        session.last_activity = Instant::now();
        session.admitted_at.get_or_insert_with(Instant::now);
        let mut activated: Vec<String> = Vec::new();
        for track in session.tracks.iter_mut().filter(|track| {
            selected.contains(track.plan.manifest.id.as_str()) && track.process_id.is_none()
        }) {
            let profiles = match track.plan.profile {
                PlannedProfile::DirectVideo => get_profile_for_with_type(
                    StreamType::Video,
                    ProfileType::Transmux,
                    &track.plan.context,
                ),
                PlannedProfile::DirectAudio => get_profile_for_with_type(
                    StreamType::Audio,
                    ProfileType::Transmux,
                    &track.plan.context,
                ),
                PlannedProfile::Video => {
                    let profiles = get_profile_for(StreamType::Video, &track.plan.context);
                    if crate::settings::get_global_settings().enable_hwaccel
                        && !track.plan.context.output_ctx.force_cfr
                    {
                        profiles
                    } else {
                        profiles
                            .into_iter()
                            .filter(|profile| {
                                profile.profile_type() != ProfileType::HardwareTranscode
                            })
                            .collect()
                    }
                }
                PlannedProfile::Audio => get_profile_for(StreamType::Audio, &track.plan.context),
                PlannedProfile::Subtitle => {
                    get_profile_for(StreamType::Subtitle, &track.plan.context)
                }
            };
            if profiles.is_empty() {
                return Err(TrackingError::Transcoder(
                    "no compatible FFmpeg profile".into(),
                ));
            }
            let process_id = match state.create(profiles, track.plan.context.clone()).await {
                Ok(process_id) => process_id,
                Err(error) => {
                    for process_id in &activated {
                        let _ = state.die(process_id.clone()).await;
                    }
                    for created in &activated {
                        if let Some(track) = session
                            .tracks
                            .iter_mut()
                            .find(|track| track.process_id.as_ref() == Some(created))
                        {
                            track.process_id = None;
                        }
                    }
                    return Err(TrackingError::Transcoder(error.to_string()));
                }
            };
            activated.push(process_id.clone());
            tracing::info!(session_id = %gid, owner, process_id, "Playback process created");
            track.process_id = Some(process_id);
        }
        for process_id in activated {
            inner.process_index.insert(process_id, gid);
        }
        Ok(())
    }

    pub async fn activate_and_compile(
        &self,
        state: &StateManager,
        gid: &Uuid,
        owner: i64,
        start_num: u64,
        includes: Vec<String>,
    ) -> Result<String, TrackingError> {
        let mut inner = self.inner.write().await;
        self.activate_locked(state, &mut inner, *gid, owner, &includes)
            .await?;
        let selected = includes.into_iter().collect::<HashSet<_>>();
        let session = inner.sessions.get(gid).ok_or(TrackingError::NotFound)?;
        let manifests = session
            .tracks
            .iter()
            .filter(|track| selected.contains(&track.plan.manifest.id))
            .filter_map(|track| Some(track.plan.manifest.activated(track.process_id.as_deref()?)))
            .collect::<Vec<_>>();
        compile_manifest(&manifests, start_num)
    }

    /// Replace the active video recipe without accumulating inactive video processes.
    ///
    /// The replacement is admitted against the post-swap process count. Creating a Nightfall
    /// session does not start FFmpeg; the new process starts only when dash.js requests its init
    /// segment. If planning the replacement fails, the previous video remains registered.
    pub async fn replace_video_and_compile(
        &self,
        state: &StateManager,
        gid: &Uuid,
        owner: i64,
        start_num: u64,
        includes: Vec<String>,
    ) -> Result<String, TrackingError> {
        let selected = includes.iter().map(String::as_str).collect::<HashSet<_>>();
        let mut inner = self.inner.write().await;
        let session = inner.sessions.get(gid).ok_or(TrackingError::NotFound)?;
        if session.owner != owner {
            return Err(TrackingError::NotOwner);
        }
        let selected_video_count = session
            .tracks
            .iter()
            .filter(|track| {
                selected.contains(track.plan.manifest.id.as_str())
                    && track.plan.manifest.content_type == ContentType::Video
            })
            .count();
        let selects_unstarted_non_video = session.tracks.iter().any(|track| {
            selected.contains(track.plan.manifest.id.as_str())
                && track.plan.manifest.content_type != ContentType::Video
                && track.process_id.is_none()
        });
        if selected_video_count != 1 || selects_unstarted_non_video {
            return Err(TrackingError::InvalidSelection);
        }

        // Manifest compilation is deterministic apart from process IDs. Validate it before
        // changing admission state so malformed metadata cannot strand the previous rendition.
        let prospective_manifests = session
            .tracks
            .iter()
            .filter(|track| selected.contains(track.plan.manifest.id.as_str()))
            .map(|track| {
                track
                    .plan
                    .manifest
                    .activated(track.process_id.as_deref().unwrap_or("pending"))
            })
            .collect::<Vec<_>>();
        compile_manifest(&prospective_manifests, start_num)?;

        let retiring = session
            .tracks
            .iter()
            .enumerate()
            .filter(|(_, track)| {
                track.plan.manifest.content_type == ContentType::Video
                    && !selected.contains(track.plan.manifest.id.as_str())
            })
            .filter_map(|(index, track)| Some((index, track.process_id.clone()?)))
            .collect::<Vec<_>>();

        // Temporarily remove the old video from admission accounting. Nightfall's create call is
        // lazy, so no replacement FFmpeg process exists concurrently with the retiring process.
        for (index, process_id) in &retiring {
            inner.process_index.remove(process_id);
            inner.sessions.get_mut(gid).unwrap().tracks[*index].process_id = None;
        }
        if let Err(error) = self
            .activate_locked(state, &mut inner, *gid, owner, &includes)
            .await
        {
            for (index, process_id) in &retiring {
                inner.process_index.insert(process_id.clone(), *gid);
                inner.sessions.get_mut(gid).unwrap().tracks[*index].process_id =
                    Some(process_id.clone());
            }
            return Err(error);
        }

        let manifests = inner.sessions[gid]
            .tracks
            .iter()
            .filter(|track| selected.contains(track.plan.manifest.id.as_str()))
            .filter_map(|track| Some(track.plan.manifest.activated(track.process_id.as_deref()?)))
            .collect::<Vec<_>>();
        let manifest = compile_manifest(&manifests, start_num)?;
        drop(inner);

        for (_, process_id) in retiring {
            if let Err(error) = state.die(process_id.clone()).await {
                tracing::warn!(session_id = %gid, owner, process_id, %error, "Retired video process cleanup failed");
            }
        }
        Ok(manifest)
    }

    pub async fn active_manifests(
        &self,
        gid: &Uuid,
        owner: i64,
    ) -> Result<Vec<(VirtualManifest, String)>, TrackingError> {
        let mut inner = self.inner.write().await;
        let session = inner.sessions.get_mut(gid).ok_or(TrackingError::NotFound)?;
        if session.owner != owner {
            return Err(TrackingError::NotOwner);
        }
        session.last_activity = Instant::now();
        Ok(session
            .tracks
            .iter()
            .filter_map(|track| Some((track.plan.manifest.clone(), track.process_id.clone()?)))
            .collect())
    }

    pub async fn remove(
        &self,
        state: &StateManager,
        gid: &Uuid,
        owner: i64,
    ) -> Result<(), TrackingError> {
        self.remove_with_context(
            state,
            gid,
            owner,
            PlaybackTeardown::server("explicit-remove", "stream-tracking-api"),
        )
        .await
    }

    pub async fn remove_with_context(
        &self,
        state: &StateManager,
        gid: &Uuid,
        owner: i64,
        teardown: PlaybackTeardown,
    ) -> Result<(), TrackingError> {
        let process_ids = {
            let mut inner = self.inner.write().await;
            let session = inner.sessions.get(gid).ok_or(TrackingError::NotFound)?;
            if session.owner != owner {
                return Err(TrackingError::NotOwner);
            }
            if teardown.frontend_instance_id.is_some()
                && teardown.frontend_instance_id != session.lifecycle.frontend_instance_id
                || teardown.source_generation.is_some()
                    && teardown.source_generation != session.lifecycle.source_generation
            {
                tracing::warn!(
                    session_id = %gid,
                    owner,
                    frontend_instance_id = ?teardown.frontend_instance_id,
                    session_frontend_instance_id = ?session.lifecycle.frontend_instance_id,
                    source_generation = ?teardown.source_generation,
                    session_source_generation = ?session.lifecycle.source_generation,
                    teardown_reason = teardown.reason,
                    teardown_caller = teardown.caller,
                    "Stale playback teardown rejected"
                );
                return Err(TrackingError::LifecycleMismatch);
            }
            tracing::info!(
                session_id = %gid,
                owner,
                frontend_instance_id = ?session.lifecycle.frontend_instance_id,
                media_file_id = ?session.lifecycle.media_file_id,
                source_generation = ?session.lifecycle.source_generation,
                creation_reason = session.lifecycle.creation_reason,
                teardown_reason = teardown.reason,
                teardown_caller = teardown.caller,
                age_ms = session.created_at.elapsed().as_millis(),
                inactive_ms = session.last_activity.elapsed().as_millis(),
                playback_admitted = session.admitted_at.is_some(),
                admission_elapsed_ms = ?session.admitted_at.map(|at| at.elapsed().as_millis()),
                process_count = session.tracks.iter().filter(|track| track.process_id.is_some()).count(),
                "Playback session removal started"
            );
            let session = inner.sessions.remove(gid).ok_or(TrackingError::NotFound)?;
            let ids = session
                .tracks
                .into_iter()
                .filter_map(|track| track.process_id)
                .collect::<Vec<_>>();
            for id in &ids {
                inner.process_index.remove(id);
            }
            ids
        };
        for id in process_ids {
            match state.die(id.clone()).await {
                Ok(()) => {
                    tracing::info!(session_id = %gid, owner, process_id = id, "Playback process terminated")
                }
                Err(error) => {
                    tracing::warn!(session_id = %gid, owner, process_id = id, %error, "Playback process termination failed")
                }
            }
        }
        tracing::info!(
            session_id = %gid,
            owner,
            teardown_reason = teardown.reason,
            teardown_caller = teardown.caller,
            "Playback session removed"
        );
        Ok(())
    }

    pub async fn cleanup_expired(&self, state: &StateManager) -> usize {
        let (mut gids, active) = {
            let inner = self.inner.read().await;
            let expired = inner
                .sessions
                .iter()
                .filter_map(|(gid, session)| {
                    let abandoned_handoff = session.handoff_started_at.is_some_and(|started| {
                        started.elapsed() >= Self::HANDOFF_CLEANUP_AFTER
                            && matches!(
                                session.remote_playback_state,
                                RemotePlaybackState::HandoffRequested
                                    | RemotePlaybackState::WirelessRouteReported
                                    | RemotePlaybackState::HandoffStalled
                                    | RemotePlaybackState::Failed
                                    | RemotePlaybackState::Disconnected
                            )
                    });
                    (session.last_activity.elapsed() >= self.policy.session_ttl
                        || abandoned_handoff)
                        .then(|| {
                            tracing::info!(
                                session_id = %gid,
                                owner = session.owner,
                                age_ms = session.created_at.elapsed().as_millis(),
                                inactive_ms = session.last_activity.elapsed().as_millis(),
                                abandoned_handoff,
                                "Playback session expired"
                            );
                            (
                                *gid,
                                session.owner,
                                if abandoned_handoff {
                                    "remote-handoff-abandoned"
                                } else {
                                    "session-ttl-expired"
                                },
                            )
                        })
                })
                .collect::<Vec<_>>();
            let active = inner
                .sessions
                .iter()
                .map(|(gid, session)| {
                    (
                        *gid,
                        session.owner,
                        session
                            .tracks
                            .iter()
                            .filter_map(|track| track.process_id.clone())
                            .collect::<Vec<_>>(),
                    )
                })
                .collect::<Vec<_>>();
            (expired, active)
        };
        for (gid, owner, process_ids) in active {
            if process_ids.is_empty() || gids.iter().any(|(existing, _, _)| *existing == gid) {
                continue;
            }
            let mut all_complete = true;
            for process_id in process_ids {
                let complete = state.has_started(process_id.clone()).await.unwrap_or(false)
                    && state.is_done(process_id).await.unwrap_or(false);
                all_complete &= complete;
            }
            if all_complete {
                gids.push((gid, owner, "playback-processes-complete"));
            }
        }
        for (gid, owner, reason) in &gids {
            let _ = self
                .remove_with_context(
                    state,
                    gid,
                    *owner,
                    PlaybackTeardown::server(*reason, "session-reaper"),
                )
                .await;
        }
        gids.len()
    }

    pub async fn shutdown(&self, state: &StateManager) {
        let sessions = {
            let inner = self.inner.read().await;
            inner
                .sessions
                .iter()
                .map(|(gid, session)| (*gid, session.owner))
                .collect::<Vec<_>>()
        };
        for (gid, owner) in sessions {
            let _ = self
                .remove_with_context(
                    state,
                    &gid,
                    owner,
                    PlaybackTeardown::server("server-shutdown", "stream-tracking-shutdown"),
                )
                .await;
        }
    }
}

fn compile_manifest(
    manifests: &[VirtualManifest],
    start_num: u64,
) -> Result<String, TrackingError> {
    let duration = manifests
        .iter()
        .find_map(|manifest| manifest.duration)
        .filter(|duration| *duration > 0)
        .ok_or(TrackingError::InvalidMetadata)?;
    let duration = ts_to_xml(duration as u64);
    let mut writer = XmlWriter::new(Default::default());
    writer.write_declaration();
    writer.start_element("MPD");
    writer.write_attribute("xmlns", "urn:mpeg:dash:schema:mpd:2011");
    writer.write_attribute("profiles", "urn:mpeg:dash:profile:isoff-on-demand:2011");
    writer.write_attribute("type", "static");
    writer.write_attribute("mediaPresentationDuration", &duration);
    writer.write_attribute("minBufferTime", "PT5S");
    writer.start_element("Period");
    writer.write_attribute("duration", &duration);
    writer.start_element("BaseURL");
    writer.write_text("/api/v1/stream/");
    writer.end_element();
    for manifest in manifests {
        manifest.compile(&mut writer, start_num);
    }
    Ok(writer.end_document())
}

impl Default for StreamTracking {
    fn default() -> Self {
        Self::with_policy(TranscodePolicy::default())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use xtra::spawn::Tokio;

    fn policy() -> TranscodePolicy {
        TranscodePolicy {
            global_limit: 2,
            per_user_limit: 1,
            per_session_limit: 1,
            session_ttl: Duration::ZERO,
        }
    }

    fn planned_track() -> PlannedTrack {
        PlannedTrack {
            manifest: VirtualManifest::new("track".into(), ContentType::Video)
                .set_duration(Some(60)),
            context: ProfileContext::default(),
            profile: PlannedProfile::Video,
        }
    }

    fn direct_video(id: &str) -> PlannedTrack {
        let mut context = ProfileContext::default();
        context.input_ctx.codec = "h264".into();
        context.output_ctx.codec = "h264".into();
        PlannedTrack {
            manifest: VirtualManifest::new(id.into(), ContentType::Video).set_duration(Some(60)),
            context,
            profile: PlannedProfile::DirectVideo,
        }
    }

    fn audio_track(id: &str) -> PlannedTrack {
        let mut context = ProfileContext::default();
        context.output_ctx.codec = "aac".into();
        PlannedTrack {
            manifest: VirtualManifest::new(id.into(), ContentType::Audio).set_duration(Some(60)),
            context,
            profile: PlannedProfile::Audio,
        }
    }
    #[test]
    fn representative_manifest_has_valid_adaptation_sets_and_channels() {
        let manifest = VirtualManifest::new("audio".into(), ContentType::Audio)
            .set_mime("audio/mp4")
            .set_codecs("mp4a.40.2")
            .set_bandwidth(128_000)
            .set_duration(Some(60))
            .set_audio_channels(Some(6))
            .activated("process");
        let subtitle = VirtualManifest::new("subtitle".into(), ContentType::Subtitle)
            .set_mime("text/vtt")
            .set_codecs("vtt")
            .set_bandwidth(1024)
            .set_duration(Some(60))
            .activated("subtitle-process");
        let direct_eac3 = VirtualManifest::new("eac3".into(), ContentType::Audio)
            .set_direct()
            .set_mime("audio/mp4")
            .set_codecs("ec-3")
            .set_bandwidth(768_000)
            .set_duration(Some(60))
            .set_audio_channels(Some(6))
            .activated("eac3-process");
        let xml = compile_manifest(&[manifest, direct_eac3, subtitle], 0).unwrap();
        assert!(xml.contains("AdaptationSet"));
        assert!(!xml.contains("AdapationSet"));
        assert!(xml.contains("value=\"6\""));
        assert!(xml.contains("codecs=\"ec-3\""));
        assert!(xml.contains("eac3-process/data/init.mp4"));
        assert!(xml.contains("contentType=\"text\""));
        assert!(xml.contains("subtitle-process/data/stream.vtt"));
    }
    #[test]
    fn invalid_duration_is_rejected_without_panicking() {
        let manifest =
            VirtualManifest::new("video".into(), ContentType::Video).activated("process");
        assert_eq!(
            compile_manifest(&[manifest], 0),
            Err(TrackingError::InvalidMetadata)
        );
    }

    #[test]
    fn admission_limits_are_actionable_and_scoped() {
        assert_eq!(
            check_admission(policy(), 0, 1, 0, 1),
            Err(TrackingError::AdmissionLimited {
                scope: "user",
                limit: 1
            })
        );
        assert!(check_admission(policy(), 0, 0, 0, 1).is_ok());
    }

    #[tokio::test]
    async fn sessions_are_owned_and_tracks_remain_lazy_until_selected() {
        let tracking = StreamTracking::with_policy(policy());
        let gid = Uuid::new_v4();
        tracking.create_session(gid, 7, vec![planned_track()]).await;
        assert!(matches!(
            tracking.inspect(&gid, 8).await,
            Err(TrackingError::NotOwner)
        ));
        let inner = tracking.inner.read().await;
        assert!(inner.process_index.is_empty());
        assert!(inner.sessions[&gid].tracks[0].process_id.is_none());
    }

    #[tokio::test]
    async fn remote_access_is_random_session_scoped_and_does_not_activate_tracks() {
        let tracking = StreamTracking::with_policy(policy());
        let first = Uuid::new_v4();
        let second = Uuid::new_v4();
        tracking
            .create_session(first, 7, vec![planned_track()])
            .await;
        tracking
            .create_session(second, 7, vec![planned_track()])
            .await;
        let token = tracking.enable_remote_access(&first, 7).await.unwrap();
        assert_eq!(tracking.authenticate_remote(&first, &token).await, Ok(7));
        assert_eq!(
            tracking.authenticate_remote(&second, &token).await,
            Err(TrackingError::NotOwner)
        );
        assert_eq!(
            tracking.authenticate_remote(&first, "wrong").await,
            Err(TrackingError::NotOwner)
        );
        let inner = tracking.inner.read().await;
        assert!(inner.process_index.is_empty());
        assert!(inner.sessions[&first].tracks[0].process_id.is_none());
    }

    #[tokio::test]
    async fn remote_playback_state_requires_the_session_owner() {
        let tracking = StreamTracking::with_policy(policy());
        let gid = Uuid::new_v4();
        tracking.create_session(gid, 7, vec![planned_track()]).await;
        assert_eq!(
            tracking.remote_playback_state(&gid).await,
            Some(RemotePlaybackState::Prepared)
        );
        assert_eq!(
            tracking
                .set_remote_playback_state(&gid, 8, RemotePlaybackState::WirelessRouteReported,)
                .await,
            Err(TrackingError::NotOwner)
        );
        tracking
            .set_remote_playback_state(&gid, 7, RemotePlaybackState::HandoffRequested)
            .await
            .unwrap();
        tracking
            .set_remote_playback_state(&gid, 7, RemotePlaybackState::WirelessRouteReported)
            .await
            .unwrap();
        assert_eq!(
            tracking.remote_playback_state(&gid).await,
            Some(RemotePlaybackState::WirelessRouteReported)
        );
    }

    #[tokio::test]
    async fn remote_delivery_requires_sustained_non_sender_segment_traffic() {
        let tracking = StreamTracking::with_policy(policy());
        let gid = Uuid::new_v4();
        tracking.create_session(gid, 7, vec![planned_track()]).await;
        tracking
            .set_remote_playback_state(&gid, 7, RemotePlaybackState::HandoffRequested)
            .await
            .unwrap();
        tracking
            .set_remote_playback_state(&gid, 7, RemotePlaybackState::WirelessRouteReported)
            .await
            .unwrap();
        tracking
            .observe_remote_hls_response(
                &gid,
                RemoteHlsStage::MediaSegment,
                RemoteRequestAttribution::SenderOrLocalProxy,
                "/video/0.m4s",
                true,
            )
            .await;
        assert_eq!(
            tracking.remote_playback_state(&gid).await,
            Some(RemotePlaybackState::WirelessRouteReported)
        );
        for path in ["/video/0.m4s", "/video/1.m4s"] {
            tracking
                .observe_remote_hls_response(
                    &gid,
                    RemoteHlsStage::MediaSegment,
                    RemoteRequestAttribution::RemoteNetworkCandidate,
                    path,
                    true,
                )
                .await;
        }
        assert_eq!(
            tracking.remote_playback_state(&gid).await,
            Some(RemotePlaybackState::MediaDeliveryConfirmed)
        );
    }

    #[tokio::test]
    async fn route_report_without_remote_traffic_becomes_stalled() {
        let tracking = StreamTracking::with_policy(policy());
        let gid = Uuid::new_v4();
        tracking.create_session(gid, 7, vec![planned_track()]).await;
        tracking
            .set_remote_playback_state(&gid, 7, RemotePlaybackState::HandoffRequested)
            .await
            .unwrap();
        tracking
            .set_remote_playback_state(&gid, 7, RemotePlaybackState::WirelessRouteReported)
            .await
            .unwrap();
        tracking
            .inner
            .write()
            .await
            .sessions
            .get_mut(&gid)
            .unwrap()
            .handoff_started_at = Some(Instant::now() - StreamTracking::HANDOFF_STALL_AFTER);
        let status = tracking.remote_playback_status(&gid, 7).await.unwrap();
        assert_eq!(status.state, RemotePlaybackState::HandoffStalled);
        assert_eq!(status.successful_remote_segments, 0);
    }

    #[tokio::test]
    async fn abandoned_handoff_cleanup_revokes_the_session_and_token() {
        let tracking = StreamTracking::with_policy(TranscodePolicy {
            session_ttl: Duration::from_secs(60),
            ..policy()
        });
        let gid = Uuid::new_v4();
        tracking.create_session(gid, 7, vec![planned_track()]).await;
        let token = tracking.enable_remote_access(&gid, 7).await.unwrap();
        tracking
            .set_remote_playback_state(&gid, 7, RemotePlaybackState::HandoffRequested)
            .await
            .unwrap();
        tracking
            .inner
            .write()
            .await
            .sessions
            .get_mut(&gid)
            .unwrap()
            .handoff_started_at = Some(Instant::now() - StreamTracking::HANDOFF_CLEANUP_AFTER);
        let temp = tempfile::tempdir().unwrap();
        let state = StateManager::new(
            &mut Tokio::Global,
            temp.path().to_string_lossy().into_owned(),
            "/bin/false".into(),
        );
        assert_eq!(tracking.cleanup_expired(&state).await, 1);
        assert_eq!(
            tracking.authenticate_remote(&gid, &token).await,
            Err(TrackingError::NotFound)
        );
    }

    #[tokio::test]
    async fn stale_frontend_generation_cannot_remove_an_active_session() {
        let tracking = StreamTracking::with_policy(policy());
        let gid = Uuid::new_v4();
        tracking
            .create_session_with_lifecycle(
                gid,
                7,
                vec![planned_track()],
                PlaybackLifecycle {
                    frontend_instance_id: Some("player-a".into()),
                    media_file_id: Some(42),
                    source_generation: Some(2),
                    creation_reason: "player-initialization".into(),
                },
            )
            .await;
        let temp = tempfile::tempdir().unwrap();
        let state = StateManager::new(
            &mut Tokio::Global,
            temp.path().to_string_lossy().into_owned(),
            "unused-ffmpeg".into(),
        );

        assert_eq!(
            tracking
                .remove_with_context(
                    &state,
                    &gid,
                    7,
                    PlaybackTeardown {
                        reason: "component-unmounted".into(),
                        caller: "old-player".into(),
                        frontend_instance_id: Some("player-a".into()),
                        source_generation: Some(1),
                    },
                )
                .await,
            Err(TrackingError::LifecycleMismatch)
        );
        assert!(tracking.inspect(&gid, 7).await.is_ok());
    }

    #[tokio::test]
    async fn video_replacement_uses_post_swap_admission_and_retires_the_old_recipe() {
        nightfall::profiles::profiles_init("/bin/false".into());
        let tracking = StreamTracking::with_policy(TranscodePolicy {
            global_limit: 2,
            per_user_limit: 2,
            per_session_limit: 2,
            session_ttl: Duration::from_secs(60),
        });
        let gid = Uuid::new_v4();
        tracking
            .create_session(
                gid,
                7,
                vec![
                    direct_video("video-a"),
                    direct_video("video-b"),
                    audio_track("audio"),
                ],
            )
            .await;
        let temp = tempfile::tempdir().unwrap();
        let state = StateManager::new(
            &mut Tokio::Global,
            temp.path().to_string_lossy().into_owned(),
            "/bin/false".into(),
        );
        tracking
            .activate_and_compile(&state, &gid, 7, 0, vec!["video-a".into(), "audio".into()])
            .await
            .unwrap();

        let xml = tracking
            .replace_video_and_compile(&state, &gid, 7, 0, vec!["video-b".into(), "audio".into()])
            .await
            .unwrap();
        assert!(xml.contains("video-b"));
        assert!(!xml.contains("video-a"));
        let active = tracking.active_manifests(&gid, 7).await.unwrap();
        assert_eq!(active.len(), 2);
        assert!(active.iter().any(|(track, _)| track.id == "video-b"));
        assert!(active.iter().any(|(track, _)| track.id == "audio"));
    }

    #[tokio::test]
    async fn rejected_video_replacement_keeps_the_current_recipe_active() {
        nightfall::profiles::profiles_init("/bin/false".into());
        let tracking = StreamTracking::with_policy(TranscodePolicy {
            global_limit: 2,
            per_user_limit: 2,
            per_session_limit: 2,
            session_ttl: Duration::from_secs(60),
        });
        let gid = Uuid::new_v4();
        tracking
            .create_session(gid, 7, vec![direct_video("video-a"), audio_track("audio")])
            .await;
        let temp = tempfile::tempdir().unwrap();
        let state = StateManager::new(
            &mut Tokio::Global,
            temp.path().to_string_lossy().into_owned(),
            "/bin/false".into(),
        );
        tracking
            .activate_and_compile(&state, &gid, 7, 0, vec!["video-a".into(), "audio".into()])
            .await
            .unwrap();

        assert_eq!(
            tracking
                .replace_video_and_compile(
                    &state,
                    &gid,
                    7,
                    0,
                    vec!["missing-video".into(), "audio".into()],
                )
                .await,
            Err(TrackingError::InvalidSelection)
        );
        let active = tracking.active_manifests(&gid, 7).await.unwrap();
        assert_eq!(active.len(), 2);
        assert!(active.iter().any(|(track, _)| track.id == "video-a"));
        assert!(active.iter().any(|(track, _)| track.id == "audio"));
    }

    #[tokio::test]
    async fn remote_init_replaces_video_without_consuming_another_admission_slot() {
        nightfall::profiles::profiles_init("/bin/false".into());
        let tracking = StreamTracking::with_policy(TranscodePolicy {
            global_limit: 2,
            per_user_limit: 2,
            per_session_limit: 2,
            session_ttl: Duration::from_secs(60),
        });
        let gid = Uuid::new_v4();
        tracking
            .create_session(
                gid,
                7,
                vec![
                    direct_video("video-a"),
                    direct_video("video-b"),
                    audio_track("audio"),
                ],
            )
            .await;
        let temp = tempfile::tempdir().unwrap();
        let state = StateManager::new(
            &mut Tokio::Global,
            temp.path().to_string_lossy().into_owned(),
            "/bin/false".into(),
        );
        tracking
            .activate_and_compile(&state, &gid, 7, 0, vec!["video-a".into(), "audio".into()])
            .await
            .unwrap();

        tracking
            .activate_remote_track(&state, &gid, "video-b", 7, true)
            .await
            .unwrap();
        let active = tracking.active_manifests(&gid, 7).await.unwrap();
        assert_eq!(active.len(), 2);
        assert!(active.iter().any(|(track, _)| track.id == "video-b"));
        assert!(active.iter().any(|(track, _)| track.id == "audio"));

        assert_eq!(
            tracking
                .activate_remote_track(&state, &gid, "video-a", 7, false)
                .await,
            Err(TrackingError::InvalidSelection)
        );
        let active = tracking.active_manifests(&gid, 7).await.unwrap();
        assert!(active.iter().any(|(track, _)| track.id == "video-b"));
        assert!(!active.iter().any(|(track, _)| track.id == "video-a"));
    }

    #[tokio::test]
    async fn wireless_handoff_transfers_matching_local_admission_capacity() {
        nightfall::profiles::profiles_init("/bin/false".into());
        let tracking = StreamTracking::with_policy(TranscodePolicy {
            global_limit: 1,
            per_user_limit: 1,
            per_session_limit: 1,
            session_ttl: Duration::from_secs(60),
        });
        let local_gid = Uuid::new_v4();
        let remote_gid = Uuid::new_v4();
        let lifecycle = || PlaybackLifecycle {
            frontend_instance_id: Some("player-a".into()),
            media_file_id: Some(42),
            source_generation: Some(3),
            creation_reason: "test".into(),
        };
        tracking
            .create_session_with_lifecycle(
                local_gid,
                7,
                vec![direct_video("local-video")],
                lifecycle(),
            )
            .await;
        tracking
            .create_session_with_lifecycle(
                remote_gid,
                7,
                vec![direct_video("remote-video")],
                lifecycle(),
            )
            .await;
        tracking.enable_remote_access(&remote_gid, 7).await.unwrap();
        let temp = tempfile::tempdir().unwrap();
        let state = StateManager::new(
            &mut Tokio::Global,
            temp.path().to_string_lossy().into_owned(),
            "/bin/false".into(),
        );
        tracking
            .activate_and_compile(&state, &local_gid, 7, 0, vec!["local-video".into()])
            .await
            .unwrap();

        // A sender preflight must not evict local playback just because it has a remote token.
        assert!(matches!(
            tracking
                .activate_remote_track(&state, &remote_gid, "remote-video", 7, true)
                .await,
            Err(TrackingError::AdmissionLimited { .. })
        ));
        tracking
            .set_remote_playback_state(&remote_gid, 7, RemotePlaybackState::WirelessRouteReported)
            .await
            .unwrap();
        tracking
            .activate_remote_track(&state, &remote_gid, "remote-video", 7, true)
            .await
            .unwrap();

        assert!(tracking
            .active_manifests(&local_gid, 7)
            .await
            .unwrap()
            .is_empty());
        let remote = tracking.active_manifests(&remote_gid, 7).await.unwrap();
        assert_eq!(remote.len(), 1);
        assert_eq!(remote[0].0.id, "remote-video");
    }

    #[tokio::test]
    async fn ttl_cleanup_removes_session_map_entries_idempotently() {
        let tracking = StreamTracking::with_policy(policy());
        let gid = Uuid::new_v4();
        tracking.create_session(gid, 7, vec![planned_track()]).await;
        let temp = tempfile::tempdir().unwrap();
        let state = StateManager::new(
            &mut Tokio::Global,
            temp.path().to_string_lossy().into_owned(),
            "/bin/false".into(),
        );
        assert_eq!(tracking.cleanup_expired(&state).await, 1);
        assert_eq!(tracking.cleanup_expired(&state).await, 0);
        assert!(matches!(
            tracking.inspect(&gid, 7).await,
            Err(TrackingError::NotFound)
        ));
    }
}
