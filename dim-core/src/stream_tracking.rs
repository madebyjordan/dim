use std::collections::{HashMap, HashSet};
use std::sync::Arc;
use std::time::{Duration, Instant};

use crate::core::StateManager;
use crate::utils::ts_to_xml;
use nightfall::profiles::{
    get_profile_for, get_profile_for_with_type, ProfileContext, ProfileType, StreamType,
};
use serde::Serialize;
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
            args: Default::default(),
            duration: None,
            label: String::new(),
            lang: None,
            target_duration: 5,
            chunk_path: String::new(),
            init_seg: None,
            audio_channels: None,
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
        fn env_usize(name: &str, default: usize) -> usize {
            std::env::var(name)
                .ok()
                .and_then(|v| v.parse().ok())
                .filter(|v| *v > 0)
                .unwrap_or(default)
        }
        Self {
            global_limit: env_usize("DIM_TRANSCODE_GLOBAL_LIMIT", 8),
            per_user_limit: env_usize("DIM_TRANSCODE_USER_LIMIT", 4),
            per_session_limit: env_usize("DIM_TRANSCODE_SESSION_LIMIT", 3),
            session_ttl: Duration::from_secs(
                env_usize("DIM_STREAM_SESSION_TTL_SECS", 30 * 60) as u64
            ),
        }
    }
}

#[derive(Debug, thiserror::Error, PartialEq, Eq)]
pub enum TrackingError {
    #[error("streaming session was not found")]
    NotFound,
    #[error("streaming session belongs to another user")]
    NotOwner,
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
    tracks: Vec<TrackState>,
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
        let mut inner = self.inner.write().await;
        tracing::info!(session_id = %gid, owner, track_count = tracks.len(), "Playback session created");
        inner.sessions.insert(
            gid,
            Session {
                owner,
                created_at: Instant::now(),
                last_activity: Instant::now(),
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
            },
        );
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
                PlannedProfile::Video => {
                    let profiles = get_profile_for(StreamType::Video, &track.plan.context);
                    if crate::settings::get_global_settings().enable_hwaccel {
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
        let process_ids = {
            let mut inner = self.inner.write().await;
            let session = inner.sessions.get(gid).ok_or(TrackingError::NotFound)?;
            if session.owner != owner {
                return Err(TrackingError::NotOwner);
            }
            tracing::info!(
                session_id = %gid,
                owner,
                age_ms = session.created_at.elapsed().as_millis(),
                inactive_ms = session.last_activity.elapsed().as_millis(),
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
        tracing::info!(session_id = %gid, owner, "Playback session removed");
        Ok(())
    }

    pub async fn cleanup_expired(&self, state: &StateManager) -> usize {
        let (mut gids, active) = {
            let inner = self.inner.read().await;
            let expired = inner
                .sessions
                .iter()
                .filter_map(|(gid, session)| {
                    (session.last_activity.elapsed() >= self.policy.session_ttl).then(|| {
                        tracing::info!(
                            session_id = %gid,
                            owner = session.owner,
                            age_ms = session.created_at.elapsed().as_millis(),
                            inactive_ms = session.last_activity.elapsed().as_millis(),
                            "Playback session expired"
                        );
                        (*gid, session.owner)
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
            if process_ids.is_empty() || gids.iter().any(|(existing, _)| *existing == gid) {
                continue;
            }
            let mut all_complete = true;
            for process_id in process_ids {
                let complete = state.has_started(process_id.clone()).await.unwrap_or(false)
                    && state.is_done(process_id).await.unwrap_or(false);
                all_complete &= complete;
            }
            if all_complete {
                gids.push((gid, owner));
            }
        }
        for (gid, owner) in &gids {
            let _ = self.remove(state, gid, *owner).await;
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
            let _ = self.remove(state, &gid, owner).await;
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
        let xml = compile_manifest(&[manifest, subtitle], 0).unwrap();
        assert!(xml.contains("AdaptationSet"));
        assert!(!xml.contains("AdapationSet"));
        assert!(xml.contains("value=\"6\""));
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
