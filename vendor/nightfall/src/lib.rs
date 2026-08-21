#![doc = include_str!("../README.md")]

/// Contains all the error types for this crate.
pub mod error;
/// Typed, retained per-session lifecycle, progress, and output events.
pub mod event;
/// Contains utils that patch segments to make them appear continuous.
pub mod patch;
/// Contains all profiles currently implemented.
pub mod profiles;
/// Contains the struct representing a streaming session.
mod session;
/// Contains utils that make my life easier.
pub mod utils;

use crate::error::*;
use crate::profiles::*;
use crate::session::Session;

use std::collections::{HashMap, VecDeque};
use std::fmt;
use std::sync::{Arc, Mutex as StdMutex, RwLock};
use std::time::{Duration, Instant};
use tokio::sync::Mutex;
use tracing::{debug, info};

pub use event::{
    ProgressPhase, SessionEvent, SessionEventKind, SessionLifecycle, SessionOutput,
    SessionProgress, SessionSubscription,
};
pub use tokio::process::ChildStdout;

const TERMINAL_HISTORY_LIMIT: usize = 64;
const TERMINAL_DIAGNOSTIC_LIMIT: usize = 64 * 1024;

struct SessionEntry {
    session: Mutex<Session>,
    events: event::SessionEvents,
}

#[derive(Default)]
struct TerminalHistory {
    entries: VecDeque<(String, String)>,
}

impl TerminalHistory {
    fn insert(&mut self, id: String, mut stderr: String) {
        if stderr.len() > TERMINAL_DIAGNOSTIC_LIMIT {
            let mut start = stderr.len() - TERMINAL_DIAGNOSTIC_LIMIT;
            while !stderr.is_char_boundary(start) {
                start += 1;
            }
            stderr.drain(..start);
        }
        if let Some(index) = self
            .entries
            .iter()
            .position(|(existing, _)| existing == &id)
        {
            self.entries.remove(index);
        }
        self.entries.push_back((id, stderr));
        while self.entries.len() > TERMINAL_HISTORY_LIMIT {
            self.entries.pop_front();
        }
    }

    fn get(&self, id: &str) -> Option<String> {
        self.entries
            .iter()
            .rev()
            .find_map(|(existing, stderr)| (existing == id).then(|| stderr.clone()))
    }
}

struct StateManagerInner {
    outdir: String,
    ffmpeg: String,
    sessions: RwLock<HashMap<String, Arc<SessionEntry>>>,
    exit_statuses: StdMutex<TerminalHistory>,
}

/// A cloneable registry of independently serialized transcoding sessions.
///
/// The registry lock is held only long enough to clone or remove a session entry. Process waits,
/// publication, filesystem cleanup, and all other slow work run under that session's own mutex.
#[derive(Clone)]
pub struct StateManager {
    inner: Arc<StateManagerInner>,
}

impl fmt::Debug for StateManager {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let session_count = self
            .inner
            .sessions
            .read()
            .unwrap_or_else(|poisoned| poisoned.into_inner())
            .len();
        f.debug_struct("StateManager")
            .field("outdir", &self.inner.outdir)
            .field("ffmpeg", &self.inner.ffmpeg)
            .field("session_count", &session_count)
            .finish()
    }
}

impl StateManager {
    /// Preserve Nightfall's historical constructor shape while no longer creating a global actor.
    pub fn new<S>(_spawner: &mut S, outdir: String, ffmpeg: String) -> Self {
        Self {
            inner: Arc::new(StateManagerInner {
                outdir,
                ffmpeg,
                sessions: RwLock::new(HashMap::new()),
                exit_statuses: StdMutex::new(TerminalHistory::default()),
            }),
        }
    }

    fn session(&self, id: &str) -> Result<Arc<SessionEntry>> {
        self.inner
            .sessions
            .read()
            .unwrap_or_else(|poisoned| poisoned.into_inner())
            .get(id)
            .cloned()
            .ok_or(NightfallError::SessionDoesntExist)
    }

    fn remember_terminal(&self, id: String, stderr: String) {
        self.inner
            .exit_statuses
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner())
            .insert(id, stderr);
    }

    pub async fn create(
        &self,
        profile_chain: Vec<&'static dyn TranscodingProfile>,
        mut profile_args: ProfileContext,
    ) -> Result<String> {
        let first_tag = if let Some(profile) = profile_chain.first() {
            profile.tag()
        } else {
            tracing::error!(profile = ?profile_args, "Supplied profile chain is empty");
            return Err(NightfallError::ProfileChainExhausted);
        };

        let chain = profile_chain
            .iter()
            .map(|profile| profile.tag())
            .collect::<Vec<_>>()
            .join(" -> ");
        let session_id = uuid::Uuid::new_v4().hyphenated().to_string();
        let tag = if let Some(width) = profile_args.output_ctx.width {
            let bitrate = profile_args
                .output_ctx
                .bitrate
                .map(|value| format!("@{}", value))
                .unwrap_or_default();
            let height = profile_args.output_ctx.height.unwrap_or(-2);
            format!("{} ({}x{}{})", first_tag, width, height, bitrate)
        } else {
            let bitrate = profile_args
                .output_ctx
                .bitrate
                .map(|value| format!("@{}", value))
                .unwrap_or_default();
            format!("{}{}", first_tag, bitrate)
        };

        info!(
            "New session {} map {} -> {}",
            &session_id, profile_args.input_ctx.stream, tag
        );
        if self.inner.outdir.is_empty() || self.inner.outdir.contains('\0') {
            return Err(NightfallError::InvalidContext(
                "Nightfall output root is empty or contains a NUL byte".into(),
            ));
        }
        profile_args.output_ctx.outdir = std::path::Path::new(&self.inner.outdir)
            .join(&session_id)
            .to_str()
            .ok_or_else(|| {
                NightfallError::InvalidContext("Nightfall output path is not valid UTF-8".into())
            })?
            .to_owned();
        profile_args.ffmpeg_bin = self.inner.ffmpeg.clone();
        for (index, profile) in profile_chain.iter().enumerate() {
            let command = profile.build(profile_args.clone())?;
            if let FallbackSemantics::Hardware {
                software_profile_tag,
            } = profile.fallback_semantics()
            {
                let has_ordered_fallback = profile_chain[..index].iter().any(|fallback| {
                    fallback.profile_type() == ProfileType::Transcode
                        && fallback.tag() == software_profile_tag
                });
                if !has_ordered_fallback {
                    return Err(NightfallError::InvalidContext(format!(
                        "hardware profile {} requires earlier software fallback {}",
                        profile.tag(),
                        software_profile_tag
                    )));
                }
            }
            debug!(profile = profile.tag(), contract = ?command.representation(), "Validated Nightfall command contract");
        }
        info!("Session {} chain {}", &session_id, chain);

        let session = Session::new(session_id.clone(), profile_chain, profile_args);
        let events = session.event_source();
        self.inner
            .sessions
            .write()
            .unwrap_or_else(|poisoned| poisoned.into_inner())
            .insert(
                session_id.clone(),
                Arc::new(SessionEntry {
                    session: Mutex::new(session),
                    events,
                }),
            );
        Ok(session_id)
    }

    pub fn subscribe(&self, id: &str) -> Result<SessionSubscription> {
        let entry = self.session(id)?;
        Ok(entry.events.subscribe())
    }

    pub async fn chunk_init_request(&self, id: String, chunk: u32) -> Result<String> {
        let entry = self.session(&id)?;
        let mut session = entry.session.lock().await;
        session.refresh_process_state();
        debug!(
            process_id = id,
            requested_segment = chunk,
            start_segment = session.start_num(),
            current_segment = session.current_chunk(),
            segment_ready = session.is_chunk_done(chunk),
            "Nightfall init demand received"
        );

        let published_init = session.published_init(chunk);
        if std::path::Path::new(&published_init).is_file() {
            return session.publish_init(chunk).await;
        }

        if session.failed() {
            if let Some(profile) = session.next_profile().map(str::to_owned) {
                info!("Session {} chunk={} trying profile {}", &id, chunk, profile);
                session.reap_terminal().await?;
                let start_num = session.start_num();
                session.reset_to(start_num)?;
            } else {
                return Err(NightfallError::ProfileChainExhausted);
            }
        }

        if !session.is_chunk_done(chunk) {
            if let Some(error) = session.terminal_output_error(&session.custom_init_seg(chunk)) {
                return Err(error);
            }
            if session.start_num() != chunk {
                session.join().await?;
                session.reset_to(chunk)?;
                session.start().await?;
                session.record_hard_seek(chunk);
            }
            session.cont();
        }

        if !session.has_started() {
            session.start().await?;
        }

        if session.is_chunk_done(chunk) {
            let path = session.publish_init(chunk).await?;
            debug!(
                process_id = id,
                requested_segment = chunk,
                path,
                "Nightfall init demand resolved"
            );
            return Ok(path);
        }
        Err(NightfallError::ChunkNotDone)
    }

    pub async fn chunk_request(&self, id: String, chunk: u32) -> Result<String> {
        let entry = self.session(&id)?;
        let mut session = entry.session.lock().await;
        session.refresh_process_state();
        debug!(
            process_id = id,
            requested_segment = chunk,
            start_segment = session.start_num(),
            current_segment = session.current_chunk(),
            segment_ready = session.is_chunk_done(chunk),
            "Nightfall segment demand received"
        );

        let published_chunk = session.published_chunk(chunk);
        if std::path::Path::new(&published_chunk).is_file() {
            return session.publish_chunk(chunk).await;
        }

        if session.failed() {
            if session.next_profile().is_some() {
                session.reap_terminal().await?;
                session.reset_to(chunk)?;
            } else {
                return Err(NightfallError::ProfileChainExhausted);
            }
        }

        if !session.has_started() {
            session.start().await?;
        }

        if !session.is_chunk_done(chunk) {
            if let Some(error) = session.terminal_output_error(&session.chunk_to_path(chunk)) {
                return Err(error);
            }
            let current_chunk = session.current_chunk();
            let raw_speed = session.raw_speed();
            let effective_raw_speed = session.effective_raw_speed();
            let eta = session.eta_for(chunk).as_millis() as f64;
            let eta_tol = (10_000.0 / effective_raw_speed).max(8_000.0);
            let hard_seek_reason = if chunk < session.start_num() {
                Some("backward-before-process-start")
            } else if chunk > current_chunk.saturating_add(15)
                && Instant::now() < session.last_hard_seek() + Duration::from_secs(15)
                && chunk > session.hard_seeked_at()
            {
                Some("forward-after-recent-hard-seek")
            } else if eta > eta_tol {
                Some("sequential-eta-exceeds-threshold")
            } else {
                None
            };

            session.cont();
            if let Some(reason) = hard_seek_reason {
                let restart_started = Instant::now();
                let previous_start_segment = session.start_num();
                info!(
                    process_id = id,
                    reason,
                    previous_start_segment,
                    current_segment = current_chunk,
                    requested_segment = chunk,
                    segment_duration_seconds = session.chunk_size,
                    raw_speed,
                    effective_raw_speed,
                    estimated_sequential_eta_ms = eta as u64,
                    hard_seek_threshold_ms = eta_tol as u64,
                    "Nightfall hard seek started"
                );
                session.join().await?;
                let stop_elapsed_ms = restart_started.elapsed().as_millis();
                session.reset_to(chunk)?;
                session.start().await?;
                session.record_hard_seek(chunk);
                info!(
                    process_id = id,
                    reason,
                    previous_start_segment,
                    requested_segment = chunk,
                    stop_elapsed_ms,
                    restart_elapsed_ms = restart_started.elapsed().as_millis(),
                    "Nightfall hard seek completed"
                );
            }
            Err(NightfallError::ChunkNotDone)
        } else {
            let chunk_path = session.chunk_to_path(chunk);
            let real_segment = session.real_segment;
            if chunk.saturating_add(2) >= session.current_chunk() {
                session.cont();
            }
            let patch_started = Instant::now();
            debug!(
                process_id = id,
                requested_segment = chunk,
                path = chunk_path,
                real_segment,
                "Nightfall segment patch started"
            );
            let published_path = session.publish_chunk(chunk).await?;
            debug!(
                process_id = id,
                requested_segment = chunk,
                elapsed_ms = patch_started.elapsed().as_millis(),
                raw_path = chunk_path,
                path = published_path,
                "Nightfall segment demand resolved"
            );
            Ok(published_path)
        }
    }

    pub async fn chunk_eta(&self, id: String, chunk: u32) -> Result<u64> {
        let entry = self.session(&id)?;
        let session = entry.session.lock().await;
        Ok(session.eta_for(chunk).as_secs())
    }

    pub async fn should_hard_seek(&self, id: String, chunk: u32) -> Result<bool> {
        let entry = self.session(&id)?;
        let session = entry.session.lock().await;
        if !session.has_started() {
            return Ok(false);
        }
        if chunk < session.start_num() {
            return Ok(true);
        }
        if chunk > session.current_chunk().saturating_add(15)
            && Instant::now() < session.last_hard_seek() + Duration::from_secs(15)
        {
            return Ok(true);
        }
        Ok((session.eta_for(chunk).as_millis() as f64)
            > (10_000.0 / session.effective_raw_speed()).max(5_000.0))
    }

    pub async fn die(&self, id: String) -> Result<()> {
        let entry = self
            .inner
            .sessions
            .write()
            .unwrap_or_else(|poisoned| poisoned.into_inner())
            .remove(&id)
            .ok_or(NightfallError::SessionDoesntExist)?;
        info!("Killing session {}", id);
        let mut session = entry.session.lock().await;
        let join_result = session.join().await;
        self.remember_terminal(id, session.stderr().unwrap_or_default());
        session.notify_removed();
        session.delete_tmp();
        join_result
    }

    pub async fn die_ignore_gc(&self, id: String) -> Result<()> {
        self.die(id).await
    }

    pub async fn get_sub(&self, id: String, name: String) -> Result<String> {
        let entry = self.session(&id)?;
        let mut session = entry.session.lock().await;
        session.refresh_process_state();
        if !session.has_started() {
            session.start().await?;
            return Err(NightfallError::ChunkNotDone);
        }

        if session.failed() {
            if session.next_profile().is_some() {
                session.reap_terminal().await?;
                session.reset_to(0)?;
                return Err(NightfallError::ChunkNotDone);
            }
            return Err(NightfallError::ProfileChainExhausted);
        }
        if !session.is_terminal() {
            return Err(NightfallError::ChunkNotDone);
        }

        let expected_path = format!("{}/{}", session.profile_ctx.output_ctx.outdir, name);
        session.subtitle(name).ok_or_else(|| {
            session
                .terminal_output_error(&expected_path)
                .unwrap_or(NightfallError::ChunkNotDone)
        })
    }

    pub async fn get_stderr(&self, id: String) -> Result<String> {
        if let Ok(entry) = self.session(&id) {
            let session = entry.session.lock().await;
            return session.stderr().ok_or(NightfallError::Aborted);
        }
        self.inner
            .exit_statuses
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner())
            .get(&id)
            .ok_or(NightfallError::SessionDoesntExist)
    }

    pub async fn garbage_collect(&self) -> Result<()> {
        let sessions = self
            .inner
            .sessions
            .read()
            .unwrap_or_else(|poisoned| poisoned.into_inner())
            .iter()
            .map(|(id, entry)| (id.clone(), entry.clone()))
            .collect::<Vec<_>>();
        let mut reaped = 0;
        let mut paused = 0;

        for (id, entry) in sessions {
            let mut session = entry.session.lock().await;
            session.refresh_process_state();
            if session.is_hard_timeout() {
                let removed = {
                    let mut sessions = self
                        .inner
                        .sessions
                        .write()
                        .unwrap_or_else(|poisoned| poisoned.into_inner());
                    if sessions
                        .get(&id)
                        .is_some_and(|current| Arc::ptr_eq(current, &entry))
                    {
                        sessions.remove(&id);
                        true
                    } else {
                        false
                    }
                };
                if removed {
                    let join_result = session.join().await;
                    self.remember_terminal(id, session.stderr().unwrap_or_default());
                    session.notify_removed();
                    session.delete_tmp();
                    if let Err(error) = join_result {
                        tracing::warn!(%error, "Failed to reap Nightfall process");
                    }
                    reaped += 1;
                }
            } else if session.is_timeout() && !session.is_throttled && !session.is_terminal() {
                session.pause();
                paused += 1;
            }
        }
        if reaped != 0 {
            info!("Reaped {} streams", reaped);
        }
        if paused != 0 {
            info!("Paused {} streams", paused);
        }
        Ok(())
    }

    /// Stop and reap every active transcoding process before application shutdown.
    pub async fn shutdown_all(&self) -> Result<()> {
        let entries = self
            .inner
            .sessions
            .write()
            .unwrap_or_else(|poisoned| poisoned.into_inner())
            .drain()
            .collect::<Vec<_>>();
        let mut tasks = Vec::with_capacity(entries.len());
        for (id, entry) in entries {
            let manager = self.clone();
            tasks.push(tokio::spawn(async move {
                let mut session = entry.session.lock().await;
                let result = session.join().await;
                manager.remember_terminal(id, session.stderr().unwrap_or_default());
                session.notify_removed();
                session.delete_tmp();
                result
            }));
        }

        let mut first_error = None;
        for task in tasks {
            match task.await {
                Ok(Err(error)) => {
                    first_error.get_or_insert(error);
                }
                Err(_) => {
                    first_error.get_or_insert(NightfallError::Aborted);
                }
                Ok(Ok(())) => {}
            }
        }
        first_error.map_or(Ok(()), Err)
    }

    pub async fn take_stdout(&self, id: String) -> Result<ChildStdout> {
        let entry = self.session(&id)?;
        let mut session = entry.session.lock().await;
        session.take_stdout().ok_or(NightfallError::Aborted)
    }

    pub async fn start(&self, id: String) -> Result<()> {
        let entry = self.session(&id)?;
        let mut session = entry.session.lock().await;
        session.start().await
    }

    pub async fn is_done(&self, id: String) -> Result<bool> {
        let entry = self.session(&id)?;
        let mut session = entry.session.lock().await;
        session.refresh_process_state();
        Ok(session.is_terminal())
    }

    pub async fn has_started(&self, id: String) -> Result<bool> {
        let entry = self.session(&id)?;
        let session = entry.session.lock().await;
        Ok(session.has_started())
    }
}

#[cfg(test)]
mod manager_tests {
    use super::*;
    use crate::profiles::{ProfileType, StreamType};
    use std::sync::atomic::{AtomicBool, Ordering};
    use tokio::sync::oneshot;

    #[derive(Debug)]
    struct NoopProfile;

    impl TranscodingProfile for NoopProfile {
        fn profile_type(&self) -> ProfileType {
            ProfileType::Transcode
        }

        fn stream_type(&self) -> StreamType {
            StreamType::Video
        }

        fn build_args(
            &self,
            _: &ProfileContext,
            _: &crate::profiles::Representation,
        ) -> Vec<String> {
            vec!["-c".into(), "exit 0".into()]
        }

        fn supports(&self, _: &ProfileContext) -> Result<()> {
            Ok(())
        }

        fn tag(&self) -> &str {
            "noop"
        }

        fn name(&self) -> &str {
            "noop"
        }
    }

    static NOOP_PROFILE: NoopProfile = NoopProfile;

    #[derive(Debug)]
    struct HardwareNoopProfile;

    impl TranscodingProfile for HardwareNoopProfile {
        fn profile_type(&self) -> ProfileType {
            ProfileType::HardwareTranscode
        }

        fn stream_type(&self) -> StreamType {
            StreamType::Video
        }

        fn build_args(
            &self,
            _: &ProfileContext,
            _: &crate::profiles::Representation,
        ) -> Vec<String> {
            vec!["-c".into(), "unused".into()]
        }

        fn supports(&self, _: &ProfileContext) -> Result<()> {
            Ok(())
        }

        fn tag(&self) -> &str {
            "hardware-noop"
        }

        fn name(&self) -> &str {
            "hardware-noop"
        }
    }

    static HARDWARE_NOOP_PROFILE: HardwareNoopProfile = HardwareNoopProfile;

    async fn create_test_session(manager: &StateManager) -> String {
        manager
            .create(vec![&NOOP_PROFILE], valid_context())
            .await
            .unwrap()
    }

    fn valid_context() -> ProfileContext {
        ProfileContext {
            file: "unused-test-input".into(),
            input_ctx: crate::profiles::InputCtx {
                codec: "h264".into(),
                pix_fmt: "yuv420p".into(),
                fps: 24.0,
                ..Default::default()
            },
            output_ctx: crate::profiles::OutputCtx {
                codec: "h264".into(),
                ..Default::default()
            },
            ..Default::default()
        }
    }

    #[tokio::test]
    async fn per_session_ordering_does_not_create_cross_session_contention() {
        let temp = tempfile::tempdir().unwrap();
        let manager = StateManager::new(
            &mut (),
            temp.path().to_string_lossy().into_owned(),
            "/bin/sh".into(),
        );
        let slow_id = create_test_session(&manager).await;
        let fast_id = create_test_session(&manager).await;
        let slow_entry = manager.session(&slow_id).unwrap();
        let (locked, locked_rx) = oneshot::channel();
        let (release, release_rx) = oneshot::channel();
        let holder = tokio::spawn(async move {
            let _session = slow_entry.session.lock().await;
            let _ = locked.send(());
            let _ = release_rx.await;
        });
        locked_rx.await.unwrap();

        let queued_manager = manager.clone();
        let mut queued_same_session =
            tokio::spawn(async move { queued_manager.has_started(slow_id).await });
        assert!(
            tokio::time::timeout(Duration::from_millis(20), &mut queued_same_session)
                .await
                .is_err(),
            "an operation for the locked session must remain ordered behind it"
        );

        let fast_completed = Arc::new(AtomicBool::new(false));
        let marker = fast_completed.clone();
        let result = tokio::time::timeout(Duration::from_millis(100), async {
            let result = manager.has_started(fast_id).await;
            marker.store(true, Ordering::SeqCst);
            result
        })
        .await;
        assert!(!result.unwrap().unwrap());
        assert!(fast_completed.load(Ordering::SeqCst));

        let _ = release.send(());
        holder.await.unwrap();
        assert!(!queued_same_session.await.unwrap().unwrap());
    }

    #[tokio::test]
    async fn hardware_profile_requires_an_ordered_software_fallback() {
        let temp = tempfile::tempdir().unwrap();
        let manager = StateManager::new(
            &mut (),
            temp.path().to_string_lossy().into_owned(),
            "/bin/false".into(),
        );
        let error = manager
            .create(vec![&HARDWARE_NOOP_PROFILE], valid_context())
            .await
            .unwrap_err();
        assert!(matches!(
            error,
            NightfallError::InvalidContext(message) if message.contains("software fallback h264")
        ));

        manager
            .create(
                vec![
                    &crate::profiles::H264TranscodeProfile,
                    &HARDWARE_NOOP_PROFILE,
                ],
                valid_context(),
            )
            .await
            .unwrap();
    }

    #[tokio::test]
    async fn terminal_history_is_bounded() {
        let mut history = TerminalHistory::default();
        for index in 0..(TERMINAL_HISTORY_LIMIT + 5) {
            history.insert(index.to_string(), index.to_string());
        }
        assert_eq!(history.entries.len(), TERMINAL_HISTORY_LIMIT);
        assert!(history.get("0").is_none());
        let latest = (TERMINAL_HISTORY_LIMIT + 4).to_string();
        assert_eq!(history.get(&latest), Some(latest));
    }

    #[test]
    fn terminal_history_caps_each_diagnostic() {
        let mut history = TerminalHistory::default();
        history.insert(
            "session".into(),
            "x".repeat(TERMINAL_DIAGNOSTIC_LIMIT + 128),
        );
        assert_eq!(
            history.get("session").unwrap().len(),
            TERMINAL_DIAGNOSTIC_LIMIT
        );
    }
}
