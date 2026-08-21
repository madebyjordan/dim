use crate::error::NightfallError;
use crate::Result;

use tokio::sync::watch;

/// A typed snapshot of the FFmpeg fields Nightfall uses for demand and ETA decisions.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct SessionProgress {
    pub frame: Option<u64>,
    pub out_time_us: Option<u64>,
    pub speed: Option<f64>,
    pub phase: Option<ProgressPhase>,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ProgressPhase {
    Continue,
    End,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum SessionLifecycle {
    Created,
    Running { pid: u32 },
    Paused { pid: u32 },
    ExitedSuccessfully,
    ExitedWithFailure(String),
    Cancelled,
    Removed,
}

impl SessionLifecycle {
    pub fn is_terminal(&self) -> bool {
        matches!(
            self,
            Self::ExitedSuccessfully | Self::ExitedWithFailure(_) | Self::Cancelled | Self::Removed
        )
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum SessionOutput {
    Init { start_num: u32, path: String },
    Segment { chunk: u32, path: String },
    Subtitle { name: String, path: String },
}

#[derive(Clone, Debug, PartialEq)]
pub enum SessionEventKind {
    Lifecycle(SessionLifecycle),
    Progress(SessionProgress),
    Output(SessionOutput),
    Reset { generation: u64, start_num: u32 },
}

/// A retained, monotonically ordered per-session event.
#[derive(Clone, Debug, PartialEq)]
pub struct SessionEvent {
    pub revision: u64,
    pub kind: SessionEventKind,
}

/// An asynchronous subscription to one session. The latest event is retained, so a change that
/// occurs between checking session state and awaiting `changed` cannot be lost.
pub struct SessionSubscription {
    receiver: watch::Receiver<SessionEvent>,
}

impl SessionSubscription {
    pub fn current(&self) -> SessionEvent {
        self.receiver.borrow().clone()
    }

    pub async fn changed(&mut self) -> Result<SessionEvent> {
        self.receiver
            .changed()
            .await
            .map_err(|_| NightfallError::SessionDoesntExist)?;
        Ok(self.current())
    }
}

#[derive(Clone)]
pub(crate) struct SessionEvents {
    sender: watch::Sender<SessionEvent>,
}

impl SessionEvents {
    pub(crate) fn new() -> Self {
        let (sender, _) = watch::channel(SessionEvent {
            revision: 0,
            kind: SessionEventKind::Lifecycle(SessionLifecycle::Created),
        });
        Self { sender }
    }

    pub(crate) fn subscribe(&self) -> SessionSubscription {
        SessionSubscription {
            receiver: self.sender.subscribe(),
        }
    }

    pub(crate) fn emit(&self, kind: SessionEventKind) {
        self.sender.send_modify(|current| {
            *current = SessionEvent {
                revision: current.revision.wrapping_add(1),
                kind,
            };
        });
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn subscription_retains_a_change_that_wins_the_await_race() {
        let events = SessionEvents::new();
        let mut subscription = events.subscribe();
        events.emit(SessionEventKind::Progress(SessionProgress {
            frame: Some(12),
            ..SessionProgress::default()
        }));

        let event = subscription.changed().await.unwrap();
        assert_eq!(event.revision, 1);
        assert!(matches!(
            event.kind,
            SessionEventKind::Progress(SessionProgress {
                frame: Some(12),
                ..
            })
        ));
    }

    #[tokio::test]
    async fn observed_events_are_monotonically_ordered_per_session() {
        let events = SessionEvents::new();
        let mut subscription = events.subscribe();
        events.emit(SessionEventKind::Lifecycle(SessionLifecycle::Running {
            pid: 42,
        }));
        let running = subscription.changed().await.unwrap();
        events.emit(SessionEventKind::Lifecycle(
            SessionLifecycle::ExitedSuccessfully,
        ));
        let terminal = subscription.changed().await.unwrap();

        assert_eq!(running.revision, 1);
        assert_eq!(terminal.revision, 2);
        assert!(matches!(
            terminal.kind,
            SessionEventKind::Lifecycle(SessionLifecycle::ExitedSuccessfully)
        ));
    }
}
