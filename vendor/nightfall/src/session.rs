use crate::event::{
    ProgressPhase, SessionEventKind, SessionEvents, SessionLifecycle, SessionOutput,
    SessionProgress,
};
use crate::patch::init_segment::patch_init_segment_to;
use crate::patch::segment::patch_segment_to;
use crate::profiles::ProfileContext;
use crate::profiles::StreamType;
use crate::profiles::TranscodingProfile;
use crate::NightfallError;
use crate::Result as NightfallResult;

use std::collections::VecDeque;
use std::fmt;
use std::fs;
use std::fs::File;
use std::io;
use std::path::Path;
use std::process::Stdio;
use std::sync::Arc;
use std::sync::Mutex;
use std::sync::RwLock;
use std::time::Duration;
use std::time::Instant;

use tokio::io::BufReader;
use tokio::io::{AsyncBufReadExt, AsyncReadExt};
use tokio::process::Child;
use tokio::process::ChildStderr;
use tokio::process::ChildStdout;
use tokio::process::Command;
use tokio::sync::mpsc;
use tokio::sync::watch;
use tokio::task::JoinHandle;

use tracing::{debug, info};

/// Represents how many chunks we encode before we require a timeout reset.
/// Basically if within MAX_CHUNKS_AHEAD we do not get a timeout reset we kill the stream.
/// This can be tuned
const MAX_CHUNKS_AHEAD: u32 = 15;
const DIAGNOSTIC_CAPACITY: usize = 64 * 1024;
const RETAINED_PUBLICATION_GENERATIONS: u64 = 4;
const PIPE_DRAIN_TIMEOUT: Duration = Duration::from_secs(1);

pub struct Session {
    /// Id of a stream in the form of a UUID.
    pub id: String,
    /// Indicates whether this stream is currently being throttled or not.
    pub is_throttled: bool,
    /// A list of fallback transcoding profiles. Nightfall will start using profiles from here if
    /// the first profile fails.
    pub profile_chain: Vec<&'static dyn TranscodingProfile>,
    /// The current transcoding profile being used in this session.
    pub profile: &'static dyn TranscodingProfile,
    /// The profile context for this session. This struct contains important information like
    /// target bitrate and container.
    pub profile_ctx: ProfileContext,
    pub real_segment: u32,
    /// How many chunks have we returned so far since init.mp4 was returned.
    pub chunks_since_init: u32,
    pub chunk_size: u32,

    has_started: bool,
    last_chunk: u32,
    hard_timeout: Instant,
    child_pid: Option<u32>,
    process: Option<ManagedProcess>,
    process_state: ProcessState,
    publication_generation: u64,
    diagnostics: Arc<Mutex<DiagnosticRing>>,
    progress: Arc<RwLock<SessionProgress>>,
    events: SessionEvents,
    hard_seeked_at: u32,
    last_hard_seek: Instant,
}

#[derive(Clone, Debug, PartialEq, Eq)]
enum ProcessState {
    NotStarted,
    Running,
    ExitedSuccessfully,
    ExitedWithFailure(String),
    Cancelled,
}

impl ProcessState {
    fn is_terminal(&self) -> bool {
        matches!(
            self,
            Self::ExitedSuccessfully | Self::ExitedWithFailure(_) | Self::Cancelled
        )
    }

    fn lifecycle(&self) -> Option<SessionLifecycle> {
        match self {
            Self::NotStarted | Self::Running => None,
            Self::ExitedSuccessfully => Some(SessionLifecycle::ExitedSuccessfully),
            Self::ExitedWithFailure(reason) => {
                Some(SessionLifecycle::ExitedWithFailure(reason.clone()))
            }
            Self::Cancelled => Some(SessionLifecycle::Cancelled),
        }
    }
}

enum ProcessCommand {
    Cancel,
}

struct ManagedProcess {
    commands: mpsc::UnboundedSender<ProcessCommand>,
    state: watch::Receiver<ProcessState>,
    monitor: JoinHandle<()>,
    stdout: Option<ChildStdout>,
}

impl Session {
    pub fn new(
        id: String,
        mut profile_chain: Vec<&'static dyn TranscodingProfile>,
        profile_ctx: ProfileContext,
    ) -> Self {
        let profile = profile_chain.pop().expect("Profile chain is empty.");
        let events = SessionEvents::new();

        Self {
            id,
            profile,
            profile_chain,
            real_segment: profile_ctx.output_ctx.start_num,
            chunk_size: profile_ctx.output_ctx.target_gop,
            profile_ctx,
            last_chunk: 0,
            is_throttled: false,
            has_started: false,
            child_pid: None,
            process: None,
            process_state: ProcessState::NotStarted,
            publication_generation: 0,
            diagnostics: Arc::new(Mutex::new(DiagnosticRing::default())),
            progress: Arc::new(RwLock::new(SessionProgress::default())),
            events,
            hard_timeout: Instant::now() + Duration::from_secs(30 * 60),
            chunks_since_init: 0,
            hard_seeked_at: 0,
            last_hard_seek: Instant::now(),
        }
    }

    #[cfg(all(test, unix))]
    pub fn subscribe(&self) -> crate::event::SessionSubscription {
        self.events.subscribe()
    }

    pub(crate) fn event_source(&self) -> SessionEvents {
        self.events.clone()
    }

    pub fn notify_removed(&self) {
        self.events
            .emit(SessionEventKind::Lifecycle(SessionLifecycle::Removed));
    }

    pub async fn start(&mut self) -> NightfallResult<()> {
        if self.process.is_some() {
            return Err(NightfallError::InvalidContext(
                "FFmpeg process is already assigned to this session".into(),
            ));
        }
        let started_at = Instant::now();
        let command = self.profile.build(self.profile_ctx.clone())?;
        std::fs::create_dir_all(&self.profile_ctx.output_ctx.outdir)?;
        crate::profiles::video::prepare_hdr_luts(&self.profile_ctx)?;
        let args = command.args();
        let diagnostics = Arc::new(Mutex::new(DiagnosticRing::default()));
        diagnostics
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner())
            .append(format!("{} {}\n", command.executable(), args.join(" ")).as_bytes());
        self.diagnostics = diagnostics.clone();

        let stdout: Stdio = if self.profile.stream_type() == StreamType::Subtitle {
            File::create(format!("{}/stream", &self.profile_ctx.output_ctx.outdir))?.into()
        } else {
            Stdio::piped()
        };

        let mut process = Command::new(command.executable())
            .kill_on_drop(true)
            .stdout(stdout)
            .stderr(Stdio::piped())
            .stdin(Stdio::null())
            .args(args)
            .spawn()?;

        let pid = process.id().ok_or_else(|| {
            io::Error::other("spawned FFmpeg process did not expose a process id")
        })?;
        self.child_pid = Some(pid);
        self.has_started = true;
        self.is_throttled = false;
        self.process_state = ProcessState::Running;

        info!(
            process_id = self.id,
            pid = self.child_pid,
            profile = self.profile.tag(),
            start_segment = self.start_num(),
            start_seconds = u64::from(self.start_num()) * u64::from(self.chunk_size),
            elapsed_ms = started_at.elapsed().as_millis(),
            ffmpeg = %self.profile_ctx.ffmpeg_bin,
            "Nightfall FFmpeg process started"
        );
        debug!(pid = self.child_pid, ffmpeg = %self.profile_ctx.ffmpeg_bin, ?args, "Started ffmpeg");

        let stdout = process.stdout.take();
        let stderr_parser = process
            .stderr
            .take()
            .map(|stderr| tokio::spawn(capture_stderr(stderr, diagnostics)));
        let (stdout, progress_parser) = if self.profile.is_stdio_stream() {
            (stdout, None)
        } else {
            let parser = stdout.map(|stdout| {
                tokio::spawn(
                    StdoutParser::new(stdout, self.progress.clone(), self.events.clone()).handle(),
                )
            });
            (None, parser)
        };
        let (commands, command_receiver) = mpsc::unbounded_channel();
        let (state_sender, state) = watch::channel(ProcessState::Running);
        let monitor_events = self.events.clone();
        let monitor = tokio::spawn(async move {
            monitor_process(
                process,
                command_receiver,
                state_sender,
                monitor_events,
                progress_parser,
                stderr_parser,
            )
            .await;
        });
        self.process = Some(ManagedProcess {
            commands,
            state,
            monitor,
            stdout,
        });
        self.events
            .emit(SessionEventKind::Lifecycle(SessionLifecycle::Running {
                pid,
            }));

        Ok(())
    }

    // NOTE: This will only work for RawVideo streams.
    pub fn take_stdout(&mut self) -> Option<ChildStdout> {
        self.process
            .as_mut()
            .and_then(|process| process.stdout.take())
    }

    pub fn start_num(&self) -> u32 {
        self.profile_ctx.output_ctx.start_num
    }

    pub fn next_profile(&mut self) -> Option<&str> {
        self.profile = self.profile_chain.pop()?;
        Some(self.profile.tag())
    }

    pub async fn join(&mut self) -> NightfallResult<()> {
        let Some(mut process) = self.process.take() else {
            if self.process_state == ProcessState::NotStarted {
                self.process_state = ProcessState::Cancelled;
                self.events
                    .emit(SessionEventKind::Lifecycle(SessionLifecycle::Cancelled));
            }
            self.child_pid = None;
            return Ok(());
        };

        if *process.state.borrow() == ProcessState::Running {
            let _ = process.commands.send(ProcessCommand::Cancel);
        }
        while !process.state.borrow().is_terminal() {
            process
                .state
                .changed()
                .await
                .map_err(|_| NightfallError::Aborted)?;
        }
        self.process_state = process.state.borrow().clone();
        self.child_pid = None;

        if process.monitor.await.is_err() {
            return Err(NightfallError::Aborted);
        }
        Ok(())
    }

    pub async fn reap_terminal(&mut self) -> NightfallResult<()> {
        self.refresh_process_state();
        if self.process_state.is_terminal() {
            self.join().await
        } else {
            Ok(())
        }
    }

    pub fn stderr(&self) -> Option<String> {
        let diagnostics = self
            .diagnostics
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner());
        (!diagnostics.is_empty()).then(|| diagnostics.to_string_lossy())
    }

    pub fn refresh_process_state(&mut self) {
        if let Some(process) = &self.process {
            self.process_state = process.state.borrow().clone();
            if self.process_state.is_terminal() {
                self.child_pid = None;
            }
        }
    }

    pub fn failed(&self) -> bool {
        matches!(self.process_state, ProcessState::ExitedWithFailure(_))
    }

    pub fn terminal_output_error(&self, path: &str) -> Option<NightfallError> {
        match &self.process_state {
            ProcessState::ExitedSuccessfully => Some(NightfallError::MissingOutput(path.into())),
            ProcessState::ExitedWithFailure(reason) => {
                Some(NightfallError::TranscodeFailed(reason.clone()))
            }
            ProcessState::Cancelled => Some(NightfallError::TranscodeCancelled),
            ProcessState::NotStarted | ProcessState::Running => None,
        }
    }

    pub fn is_terminal(&self) -> bool {
        self.process.as_ref().map_or_else(
            || self.process_state.is_terminal(),
            |process| process.state.borrow().is_terminal(),
        )
    }

    pub fn is_hard_timeout(&self) -> bool {
        Instant::now() > self.hard_timeout
    }

    pub fn delete_tmp(&self) {
        let _ = fs::remove_dir_all(&self.profile_ctx.output_ctx.outdir);
    }

    pub fn pause(&mut self) {
        if let Some(x) = self.child_pid {
            if !self.is_throttled {
                crate::utils::pause_proc(x as i32);
                self.is_throttled = true;
                self.events
                    .emit(SessionEventKind::Lifecycle(SessionLifecycle::Paused {
                        pid: x,
                    }));
            }
        }
    }

    pub fn cont(&mut self) {
        if let Some(x) = self.child_pid {
            if self.is_throttled {
                crate::utils::cont_proc(x as i32);
                self.is_throttled = false;
                self.events
                    .emit(SessionEventKind::Lifecycle(SessionLifecycle::Running {
                        pid: x,
                    }));
            }
        }
    }

    fn progress(&self) -> SessionProgress {
        self.progress
            .read()
            .unwrap_or_else(|poisoned| poisoned.into_inner())
            .clone()
    }

    pub fn current_chunk(&self) -> u32 {
        let frame_rate = self.profile_ctx.input_ctx.fps.max(1.0);
        let progress = self.progress();
        match self.profile.stream_type() {
            StreamType::Audio => {
                let seconds = progress.out_time_us.unwrap_or(0) as f64 / 1_000_000.0;
                saturating_u32(seconds / f64::from(self.chunk_size.max(1))).max(self.last_chunk)
            }
            StreamType::Video => {
                let relative = progress.frame.unwrap_or(0) as f64
                    / (f64::from(self.chunk_size.max(1)) * frame_rate);
                saturating_u32(relative).saturating_add(self.profile_ctx.output_ctx.start_num)
            }
            _ => 0,
        }
    }

    pub fn raw_speed(&self) -> f64 {
        self.progress().speed.unwrap_or(1.0)
    }

    pub fn effective_raw_speed(&self) -> f64 {
        valid_raw_speed(self.raw_speed())
    }

    // returns how many chunks per second
    pub fn speed(&self) -> f64 {
        chunks_per_second(self.raw_speed(), self.chunk_size)
    }

    pub fn eta_for(&self, chunk: u32) -> Duration {
        let cps = self.speed();

        let current_chunk = self.current_chunk() as f64;
        let diff = (chunk as f64 - current_chunk).abs();

        Duration::from_secs((diff / cps).abs().ceil() as u64)
    }

    /// Method does some math magic to guess if a chunk has been fully written by ffmpeg yet
    /// only works when `ffmpeg` writes files to tmp then renames them.
    pub fn is_chunk_done(&self, chunk_num: u32) -> bool {
        Path::new(&format!(
            "{}/{}.m4s",
            &self.profile_ctx.output_ctx.outdir, chunk_num
        ))
        .is_file()
    }

    pub fn subtitle(&self, file: String) -> Option<String> {
        if !matches!(self.profile.stream_type(), StreamType::Subtitle) {
            return None;
        }

        let name = file;
        let file = format!("{}/{}", &self.profile_ctx.output_ctx.outdir, name);
        let path = Path::new(&file);

        // NOTE: This will not check if the ffmpeg process is dead, thus this will return immediately
        if path.is_file() {
            let path = path.to_str().map(ToString::to_string)?;
            self.events
                .emit(SessionEventKind::Output(SessionOutput::Subtitle {
                    name,
                    path: path.clone(),
                }));
            return Some(path);
        }

        None
    }

    pub fn is_timeout(&self) -> bool {
        self.current_chunk() > self.last_chunk.saturating_add(MAX_CHUNKS_AHEAD)
    }

    pub fn reset_timeout(&mut self, last_requested: u32) {
        self.last_chunk = last_requested;
        self.hard_timeout = Instant::now() + Duration::from_secs(30 * 60);
    }

    pub fn hard_seeked_at(&self) -> u32 {
        self.hard_seeked_at
    }

    pub fn last_hard_seek(&self) -> Instant {
        self.last_hard_seek
    }

    pub fn record_hard_seek(&mut self, chunk: u32) {
        self.hard_seeked_at = chunk;
        self.last_hard_seek = Instant::now();
    }

    pub fn chunk_to_path(&self, chunk_num: u32) -> String {
        format!("{}/{}.m4s", self.profile_ctx.output_ctx.outdir, chunk_num)
    }

    pub fn init_seg(&self) -> String {
        format!(
            "{}/{}_init.mp4",
            self.profile_ctx.output_ctx.outdir,
            self.start_num()
        )
    }

    pub fn custom_init_seg(&self, start_num: u32) -> String {
        format!(
            "{}/{}_init.mp4",
            self.profile_ctx.output_ctx.outdir, start_num
        )
    }

    fn publication_dir(&self) -> String {
        format!(
            "{}/published-{}",
            self.profile_ctx.output_ctx.outdir, self.publication_generation
        )
    }

    pub fn published_chunk(&self, chunk_num: u32) -> String {
        format!("{}/{}.m4s", self.publication_dir(), chunk_num)
    }

    pub fn published_init(&self, start_num: u32) -> String {
        format!("{}/{}_init.mp4", self.publication_dir(), start_num)
    }

    pub fn normalized_init(&self) -> String {
        format!("{}/normalized_init.mp4", self.publication_dir())
    }

    pub async fn publish_init(&mut self, start_num: u32) -> NightfallResult<String> {
        let published = self.published_init(start_num);
        if Path::new(&published).is_file() {
            self.chunks_since_init = 0;
            return Ok(published);
        }

        let normalized = self.normalized_init();
        let source = if Path::new(&normalized).is_file() {
            normalized
        } else {
            self.custom_init_seg(start_num)
        };
        crate::patch::publish_copy(source, published.clone()).await?;
        self.chunks_since_init = 0;
        self.events
            .emit(SessionEventKind::Output(SessionOutput::Init {
                start_num,
                path: published.clone(),
            }));
        Ok(published)
    }

    pub async fn publish_chunk(&mut self, chunk_num: u32) -> NightfallResult<String> {
        let published = self.published_chunk(chunk_num);
        if Path::new(&published).is_file() {
            return Ok(published);
        }

        let raw = self.chunk_to_path(chunk_num);
        let next_sequence =
            match patch_segment_to(raw.clone(), published.clone(), self.real_segment).await {
                Ok(next_sequence) => Some(next_sequence),
                Err(NightfallError::PartialSegment(_)) if self.chunks_since_init >= 1 => Some(
                    patch_init_segment_to(
                        self.init_seg(),
                        published.clone(),
                        self.normalized_init(),
                        self.real_segment,
                    )
                    .await?,
                ),
                Err(NightfallError::PartialSegment(_)) => {
                    crate::patch::publish_copy(raw, published.clone()).await?;
                    None
                }
                Err(error) => return Err(error),
            };

        if let Some(next_sequence) = next_sequence {
            self.real_segment = next_sequence;
        }
        self.reset_timeout(chunk_num);
        self.chunks_since_init += 1;
        self.events
            .emit(SessionEventKind::Output(SessionOutput::Segment {
                chunk: chunk_num,
                path: published.clone(),
            }));
        Ok(published)
    }

    pub fn has_started(&self) -> bool {
        self.has_started
    }

    pub fn reset_to(&mut self, chunk: u32) -> NightfallResult<()> {
        if self.process.is_some() {
            return Err(NightfallError::Aborted);
        }
        let next_generation = self.publication_generation.checked_add(1).ok_or_else(|| {
            NightfallError::InvalidContext("publication generation overflow".into())
        })?;
        self.prune_oldest_publication(next_generation)?;
        self.clear_unpublished_segments()?;
        self.profile_ctx.output_ctx.start_num = chunk;
        self.process = None;
        self.last_chunk = chunk;
        self.has_started = false;
        self.is_throttled = true;
        self.real_segment = chunk;
        self.child_pid = None;
        self.process_state = ProcessState::NotStarted;
        self.publication_generation = next_generation;
        *self
            .progress
            .write()
            .unwrap_or_else(|poisoned| poisoned.into_inner()) = SessionProgress::default();
        self.events.emit(SessionEventKind::Reset {
            generation: self.publication_generation,
            start_num: chunk,
        });
        Ok(())
    }

    fn prune_oldest_publication(&self, next_generation: u64) -> NightfallResult<()> {
        let Some(expired) = next_generation.checked_sub(RETAINED_PUBLICATION_GENERATIONS) else {
            return Ok(());
        };
        let path =
            Path::new(&self.profile_ctx.output_ctx.outdir).join(format!("published-{expired}"));
        match fs::remove_dir_all(path) {
            Ok(()) => Ok(()),
            Err(error) if error.kind() == io::ErrorKind::NotFound => Ok(()),
            Err(error) => Err(error.into()),
        }
    }

    fn clear_unpublished_segments(&self) -> NightfallResult<()> {
        let outdir = Path::new(&self.profile_ctx.output_ctx.outdir);
        let entries = match fs::read_dir(outdir) {
            Ok(entries) => entries,
            Err(error) if error.kind() == io::ErrorKind::NotFound => return Ok(()),
            Err(error) => return Err(error.into()),
        };
        for entry in entries {
            let entry = entry?;
            if !entry.file_type()?.is_file() {
                continue;
            }
            let name = entry.file_name();
            let name = name.to_string_lossy();
            if name.ends_with(".m4s")
                || name.ends_with(".m4s.tmp")
                || name.ends_with("_init.mp4")
                || name == "playlist.m3u8"
            {
                fs::remove_file(entry.path())?;
            }
        }
        Ok(())
    }
}

fn chunks_per_second(raw_speed: f64, chunk_size: u32) -> f64 {
    // FFmpeg reports encoding speed as a real-time multiplier. Preserve fractional
    // speeds: treating a 0.4x transcode as a fast encoder makes distant seeks wait
    // for minutes of sequential output instead of restarting near the target.
    valid_raw_speed(raw_speed) / chunk_size.max(1) as f64
}

fn valid_raw_speed(raw_speed: f64) -> f64 {
    if raw_speed.is_finite() && raw_speed > 0.0 {
        raw_speed
    } else {
        1.0
    }
}

fn saturating_u32(value: f64) -> u32 {
    if !value.is_finite() || value <= 0.0 {
        0
    } else if value >= f64::from(u32::MAX) {
        u32::MAX
    } else {
        value.floor() as u32
    }
}

impl fmt::Debug for Session {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("Session")
            .field("id", &self.id)
            .field("start_number", &self.profile_ctx.output_ctx.start_num)
            .field("last_chunk", &self.last_chunk)
            .finish()
    }
}

struct StdoutParser {
    process_stdout: ChildStdout,
    progress: Arc<RwLock<SessionProgress>>,
    events: SessionEvents,
}

impl StdoutParser {
    fn new(
        process_stdout: ChildStdout,
        progress: Arc<RwLock<SessionProgress>>,
        events: SessionEvents,
    ) -> Self {
        Self {
            process_stdout,
            progress,
            events,
        }
    }

    async fn handle(self) {
        let mut stdio = BufReader::new(self.process_stdout).lines();
        let mut changed = false;
        while let Ok(Some(line)) = stdio.next_line().await {
            let Some((key, value)) = line.split_once('=') else {
                debug!(line = %line, "Ignoring malformed FFmpeg progress output");
                continue;
            };
            let value = value.trim();
            let mut progress = self
                .progress
                .write()
                .unwrap_or_else(|poisoned| poisoned.into_inner());
            changed |= match key {
                "frame" => update_if_parsed(&mut progress.frame, value),
                "out_time_us" => update_if_parsed(&mut progress.out_time_us, value),
                "speed" => update_if_parsed(&mut progress.speed, value.trim_end_matches('x')),
                "progress" => {
                    let phase = match value {
                        "continue" => Some(ProgressPhase::Continue),
                        "end" => Some(ProgressPhase::End),
                        _ => None,
                    };
                    let changed = phase.is_some() && progress.phase != phase;
                    if phase.is_some() {
                        progress.phase = phase;
                    }
                    changed
                }
                _ => false,
            };
            if key == "progress" && changed {
                let snapshot = progress.clone();
                drop(progress);
                self.events.emit(SessionEventKind::Progress(snapshot));
                changed = false;
            }
        }
        if changed {
            let snapshot = self
                .progress
                .read()
                .unwrap_or_else(|poisoned| poisoned.into_inner())
                .clone();
            self.events.emit(SessionEventKind::Progress(snapshot));
        }
    }
}

fn update_if_parsed<T>(target: &mut Option<T>, value: &str) -> bool
where
    T: std::str::FromStr + PartialEq,
{
    let Ok(value) = value.parse() else {
        return false;
    };
    if target.as_ref() == Some(&value) {
        return false;
    }
    *target = Some(value);
    true
}

async fn monitor_process(
    mut process: Child,
    mut commands: mpsc::UnboundedReceiver<ProcessCommand>,
    state: watch::Sender<ProcessState>,
    events: SessionEvents,
    progress_parser: Option<JoinHandle<()>>,
    stderr_parser: Option<JoinHandle<()>>,
) {
    let mut cancelled = false;
    let terminal = loop {
        tokio::select! {
            result = process.wait() => {
                break match result {
                    Ok(status) if cancelled => ProcessState::Cancelled,
                    Ok(status) if status.success() => ProcessState::ExitedSuccessfully,
                    Ok(status) => ProcessState::ExitedWithFailure(status.to_string()),
                    Err(error) => ProcessState::ExitedWithFailure(error.to_string()),
                };
            }
            command = commands.recv(), if !cancelled => {
                cancelled = true;
                if matches!(command, Some(ProcessCommand::Cancel) | None) {
                    let _ = process.start_kill();
                }
            }
        }
    };
    finish_pipe_reader(progress_parser).await;
    finish_pipe_reader(stderr_parser).await;
    state.send_replace(terminal.clone());
    if let Some(lifecycle) = terminal.lifecycle() {
        events.emit(SessionEventKind::Lifecycle(lifecycle));
    }
}

async fn finish_pipe_reader(parser: Option<JoinHandle<()>>) {
    let Some(mut parser) = parser else { return };
    if tokio::time::timeout(PIPE_DRAIN_TIMEOUT, &mut parser)
        .await
        .is_err()
    {
        parser.abort();
        let _ = parser.await;
    }
}

#[derive(Debug)]
struct DiagnosticRing {
    bytes: VecDeque<u8>,
}

impl Default for DiagnosticRing {
    fn default() -> Self {
        Self {
            bytes: VecDeque::with_capacity(DIAGNOSTIC_CAPACITY),
        }
    }
}

impl DiagnosticRing {
    fn append(&mut self, bytes: &[u8]) {
        if bytes.len() >= DIAGNOSTIC_CAPACITY {
            self.bytes.clear();
            self.bytes
                .extend(bytes[bytes.len() - DIAGNOSTIC_CAPACITY..].iter().copied());
            return;
        }
        let overflow = self
            .bytes
            .len()
            .saturating_add(bytes.len())
            .saturating_sub(DIAGNOSTIC_CAPACITY);
        self.bytes.drain(..overflow);
        self.bytes.extend(bytes.iter().copied());
    }

    fn is_empty(&self) -> bool {
        self.bytes.is_empty()
    }

    fn to_string_lossy(&self) -> String {
        let bytes = self.bytes.iter().copied().collect::<Vec<_>>();
        String::from_utf8_lossy(&bytes).into_owned()
    }
}

async fn capture_stderr(mut stderr: ChildStderr, diagnostics: Arc<Mutex<DiagnosticRing>>) {
    let mut chunk = [0_u8; 8 * 1024];
    loop {
        let read = match stderr.read(&mut chunk).await {
            Ok(0) | Err(_) => return,
            Ok(read) => read,
        };
        diagnostics
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner())
            .append(&chunk[..read]);
    }
}

#[cfg(test)]
mod eta_tests {
    use super::chunks_per_second;

    #[test]
    fn fractional_encoder_speed_is_preserved_for_seek_eta() {
        let chunks_per_second = chunks_per_second(0.4, 5);
        let eta_seconds = 42.0 / chunks_per_second;
        assert!((eta_seconds - 525.0).abs() < f64::EPSILON);
        let threshold_seconds = (10.0 / 0.4_f64).max(8.0);
        assert!(eta_seconds > threshold_seconds);
        assert!(12.5 < threshold_seconds, "an adjacent segment should wait");
    }

    #[test]
    fn fast_encoder_speed_is_not_artificially_changed() {
        let chunks_per_second = chunks_per_second(20.0, 5);
        let eta_seconds = 42.0 / chunks_per_second;
        assert!((eta_seconds - 10.5).abs() < f64::EPSILON);
    }

    #[test]
    fn invalid_progress_speed_uses_a_safe_realtime_estimate() {
        assert_eq!(chunks_per_second(0.0, 5), 0.2);
        assert_eq!(chunks_per_second(f64::NAN, 5), 0.2);
    }
}

#[cfg(all(test, unix))]
mod tests {
    use super::*;
    use crate::error::NightfallError;
    use crate::profiles::{ProfileType, TranscodingProfile};
    use std::io::Read;

    #[derive(Debug)]
    struct ShellProfile;

    impl TranscodingProfile for ShellProfile {
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
            vec!["-c".into(), "echo malformed-progress; sleep 30".into()]
        }
        fn supports(&self, _: &ProfileContext) -> Result<(), NightfallError> {
            Ok(())
        }
        fn tag(&self) -> &str {
            "shell"
        }
        fn name(&self) -> &str {
            "shell"
        }
    }

    static SHELL_PROFILE: ShellProfile = ShellProfile;

    #[derive(Debug)]
    struct FailingProfile;
    impl TranscodingProfile for FailingProfile {
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
            vec!["-c".into(), "exit 7".into()]
        }
        fn supports(&self, _: &ProfileContext) -> Result<(), NightfallError> {
            Ok(())
        }
        fn tag(&self) -> &str {
            "failing-shell"
        }
        fn name(&self) -> &str {
            "failing-shell"
        }
    }
    static FAILING_PROFILE: FailingProfile = FailingProfile;

    #[derive(Debug)]
    struct SuccessfulProfile;
    impl TranscodingProfile for SuccessfulProfile {
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
        fn supports(&self, _: &ProfileContext) -> Result<(), NightfallError> {
            Ok(())
        }
        fn tag(&self) -> &str {
            "successful-shell"
        }
        fn name(&self) -> &str {
            "successful-shell"
        }
    }
    static SUCCESSFUL_PROFILE: SuccessfulProfile = SuccessfulProfile;

    #[derive(Debug)]
    struct ProgressProfile;
    impl TranscodingProfile for ProgressProfile {
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
            vec![
                "-c".into(),
                "printf 'frame=25\\nout_time_us=5000000\\nspeed=0.5x\\nprogress=continue\\n'; sleep 30".into(),
            ]
        }
        fn supports(&self, _: &ProfileContext) -> Result<(), NightfallError> {
            Ok(())
        }
        fn tag(&self) -> &str {
            "progress-shell"
        }
        fn name(&self) -> &str {
            "progress-shell"
        }
    }
    static PROGRESS_PROFILE: ProgressProfile = ProgressProfile;

    #[derive(Debug)]
    struct LoudFailureProfile;
    impl TranscodingProfile for LoudFailureProfile {
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
            vec![
                "-c".into(),
                "head -c 131072 /dev/zero | tr '\\0' x >&2; printf TAIL >&2; exit 9".into(),
            ]
        }
        fn supports(&self, _: &ProfileContext) -> Result<(), NightfallError> {
            Ok(())
        }
        fn tag(&self) -> &str {
            "loud-failure-shell"
        }
        fn name(&self) -> &str {
            "loud-failure-shell"
        }
    }
    static LOUD_FAILURE_PROFILE: LoudFailureProfile = LoudFailureProfile;

    fn context(outdir: &Path, binary: &str) -> ProfileContext {
        ProfileContext {
            file: "unused-test-input".into(),
            ffmpeg_bin: binary.into(),
            input_ctx: crate::profiles::InputCtx {
                codec: "h264".into(),
                pix_fmt: "yuv420p".into(),
                fps: 24.0,
                ..Default::default()
            },
            output_ctx: crate::profiles::OutputCtx {
                codec: "h264".into(),
                outdir: outdir.to_string_lossy().into_owned(),
                ..Default::default()
            },
        }
    }

    fn write_media_segment(path: &Path, payload: &[u8]) {
        use mp4::mp4box::{MdatBox, MoofBox};

        let mut segment = crate::patch::segment::Segment::default().gen_styp();
        segment.moof = Some(MoofBox::default());
        segment.mdat = Some(MdatBox {
            data: payload.to_vec(),
        });
        segment
            .write(&mut File::create(path).unwrap())
            .expect("test segment should be written");
    }

    #[tokio::test]
    async fn spawn_failure_does_not_mark_the_session_started() {
        let temp = tempfile::tempdir().unwrap();
        let mut session = Session::new(
            "spawn-failure".into(),
            vec![&SHELL_PROFILE],
            context(temp.path(), "/missing/dim-ffmpeg"),
        );
        assert!(session.start().await.is_err());
        assert!(!session.has_started());
        assert!(session.process.is_none());
    }

    #[tokio::test]
    async fn cancellation_reaps_process_and_malformed_progress_is_safe() {
        let temp = tempfile::tempdir().unwrap();
        let mut session = Session::new(
            "cancel".into(),
            vec![&SHELL_PROFILE],
            context(temp.path(), "/bin/sh"),
        );
        let mut events = session.subscribe();
        session.start().await.unwrap();
        let running = events.changed().await.unwrap();
        assert!(matches!(
            running.kind,
            SessionEventKind::Lifecycle(SessionLifecycle::Running { .. })
        ));
        session.join().await.unwrap();
        let cancelled = events.changed().await.unwrap();
        assert!(matches!(
            cancelled.kind,
            SessionEventKind::Lifecycle(SessionLifecycle::Cancelled)
        ));
        assert!(cancelled.revision > running.revision);
        assert!(session.process.is_none());
        assert!(session.child_pid.is_none());
        assert_eq!(session.process_state, ProcessState::Cancelled);
        assert!(matches!(
            session.terminal_output_error("missing.m4s"),
            Some(NightfallError::TranscodeCancelled)
        ));
    }

    #[tokio::test]
    async fn typed_progress_event_is_delivered_without_map_cloning_or_pid_polling() {
        let temp = tempfile::tempdir().unwrap();
        let mut session = Session::new(
            "progress".into(),
            vec![&PROGRESS_PROFILE],
            context(temp.path(), "/bin/sh"),
        );
        let mut events = session.subscribe();
        session.start().await.unwrap();
        let progress = loop {
            let event = events.changed().await.unwrap();
            if let SessionEventKind::Progress(progress) = event.kind {
                break progress;
            }
        };
        assert_eq!(progress.frame, Some(25));
        assert_eq!(progress.out_time_us, Some(5_000_000));
        assert_eq!(progress.speed, Some(0.5));
        assert_eq!(progress.phase, Some(ProgressPhase::Continue));
        session.join().await.unwrap();
    }

    #[tokio::test]
    async fn nonzero_exit_is_observed_for_profile_fallback() {
        let temp = tempfile::tempdir().unwrap();
        let mut session = Session::new(
            "failure".into(),
            vec![&FAILING_PROFILE],
            context(temp.path(), "/bin/sh"),
        );
        let mut events = session.subscribe();
        session.start().await.unwrap();
        while !matches!(
            events.changed().await.unwrap().kind,
            SessionEventKind::Lifecycle(SessionLifecycle::ExitedWithFailure(_))
        ) {}
        session.refresh_process_state();
        assert!(session.failed());
        assert!(session.child_pid.is_none());
        assert!(matches!(
            session.terminal_output_error("missing.m4s"),
            Some(NightfallError::TranscodeFailed(_))
        ));
        session.join().await.unwrap();
    }

    #[tokio::test]
    async fn stderr_is_captured_incrementally_in_a_fixed_tail_ring() {
        let temp = tempfile::tempdir().unwrap();
        let mut session = Session::new(
            "diagnostic-cap".into(),
            vec![&LOUD_FAILURE_PROFILE],
            context(temp.path(), "/bin/sh"),
        );
        let mut events = session.subscribe();
        session.start().await.unwrap();
        while !matches!(
            events.changed().await.unwrap().kind,
            SessionEventKind::Lifecycle(SessionLifecycle::ExitedWithFailure(_))
        ) {}
        session.join().await.unwrap();
        let diagnostic = session.stderr().unwrap();
        assert!(diagnostic.len() <= DIAGNOSTIC_CAPACITY);
        assert!(diagnostic.ends_with("TAIL"));
        assert!(!diagnostic.contains("loud-failure-shell"));
    }

    #[tokio::test]
    async fn successful_exit_with_missing_output_is_terminal() {
        let temp = tempfile::tempdir().unwrap();
        let mut session = Session::new(
            "success-without-output".into(),
            vec![&SUCCESSFUL_PROFILE],
            context(temp.path(), "/bin/sh"),
        );
        let mut events = session.subscribe();
        session.start().await.unwrap();
        while !matches!(
            events.changed().await.unwrap().kind,
            SessionEventKind::Lifecycle(SessionLifecycle::ExitedSuccessfully)
        ) {}
        session.refresh_process_state();
        assert_eq!(session.process_state, ProcessState::ExitedSuccessfully);
        assert!(matches!(
            session.terminal_output_error("0.m4s"),
            Some(NightfallError::MissingOutput(path)) if path == "0.m4s"
        ));
        assert!(session.is_terminal());
        session.join().await.unwrap();
    }

    #[tokio::test]
    async fn repeated_publication_is_idempotent_and_keeps_open_readers_stable() {
        let temp = tempfile::tempdir().unwrap();
        let mut session = Session::new(
            "publication".into(),
            vec![&SUCCESSFUL_PROFILE],
            context(temp.path(), "/bin/sh"),
        );
        let raw_path = temp.path().join("0.m4s");
        write_media_segment(&raw_path, b"first-payload");
        let raw_before = fs::read(&raw_path).unwrap();

        let published = session.publish_chunk(0).await.unwrap();
        let published_before = fs::read(&published).unwrap();
        assert_eq!(fs::read(&raw_path).unwrap(), raw_before);
        assert_ne!(Path::new(&published), raw_path);
        assert_eq!(session.real_segment, 1);

        let mut open_reader = File::open(&published).unwrap();
        write_media_segment(&raw_path, b"replacement-payload");
        assert_eq!(session.publish_chunk(0).await.unwrap(), published);
        assert_eq!(session.real_segment, 1);
        assert_eq!(fs::read(&published).unwrap(), published_before);

        let mut bytes_from_original_handle = Vec::new();
        open_reader
            .read_to_end(&mut bytes_from_original_handle)
            .unwrap();
        assert_eq!(bytes_from_original_handle, published_before);

        session.reset_to(0).unwrap();
        assert!(!raw_path.exists());
        assert_eq!(fs::read(published).unwrap(), published_before);
    }

    #[tokio::test]
    async fn patch_failure_is_returned_without_publishing_or_advancing_state() {
        let temp = tempfile::tempdir().unwrap();
        let mut session = Session::new(
            "patch-failure".into(),
            vec![&SUCCESSFUL_PROFILE],
            context(temp.path(), "/bin/sh"),
        );
        fs::write(temp.path().join("0.m4s"), b"x").unwrap();

        assert!(session.publish_chunk(0).await.is_err());
        assert!(!Path::new(&session.published_chunk(0)).exists());
        assert_eq!(session.real_segment, 0);
        assert_eq!(session.chunks_since_init, 0);
    }

    #[tokio::test]
    async fn initialization_publication_is_immutable_and_generation_scoped() {
        let temp = tempfile::tempdir().unwrap();
        let mut session = Session::new(
            "init-publication".into(),
            vec![&SUCCESSFUL_PROFILE],
            context(temp.path(), "/bin/sh"),
        );
        let raw_init = temp.path().join("0_init.mp4");
        fs::write(&raw_init, b"first-init").unwrap();

        let first_path = session.publish_init(0).await.unwrap();
        let first_bytes = fs::read(&first_path).unwrap();
        fs::write(&raw_init, b"second-init").unwrap();
        assert_eq!(session.publish_init(0).await.unwrap(), first_path);
        assert_eq!(fs::read(&first_path).unwrap(), first_bytes);

        session.reset_to(0).unwrap();
        fs::write(&raw_init, b"second-init").unwrap();
        let second_path = session.publish_init(0).await.unwrap();
        assert_ne!(first_path, second_path);
        assert_eq!(fs::read(second_path).unwrap(), b"second-init");
        assert_eq!(fs::read(first_path).unwrap(), b"first-init");
    }

    #[test]
    fn publication_generation_retention_is_fixed_and_overflow_safe() {
        let temp = tempfile::tempdir().unwrap();
        let mut session = Session::new(
            "generation-retention".into(),
            vec![&SUCCESSFUL_PROFILE],
            context(temp.path(), "/bin/sh"),
        );
        fs::create_dir(temp.path().join("published-0")).unwrap();
        for generation in 1..=6 {
            session.reset_to(generation as u32).unwrap();
            fs::create_dir(temp.path().join(format!("published-{generation}"))).unwrap();
        }
        for expired in 0..=2 {
            assert!(!temp.path().join(format!("published-{expired}")).exists());
        }
        for retained in 3..=6 {
            assert!(temp.path().join(format!("published-{retained}")).is_dir());
        }

        session.publication_generation = u64::MAX;
        assert!(matches!(
            session.reset_to(0),
            Err(NightfallError::InvalidContext(message)) if message.contains("generation overflow")
        ));
    }
}
