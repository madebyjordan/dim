use serde::Serialize;
use thiserror::Error;

pub type Result<T> = std::result::Result<T, NightfallError>;

#[derive(Clone, Debug, Error, Serialize)]
pub enum NightfallError {
    #[error("The requested session does not exist")]
    SessionDoesntExist,
    #[error("Chunk requested is not ready yet")]
    ChunkNotDone,
    #[error("Request aborted")]
    Aborted,
    #[error("Failed to patch segment {0}")]
    SegmentPatchError(String),
    #[error("I/O error: {0}")]
    IoError(String),
    #[error("Box missing in segment")]
    MissingSegmentBox,
    #[error("Invalid fMP4 fragment: {0}")]
    InvalidFragment(String),
    #[error("Invalid FFmpeg command context: {0}")]
    InvalidContext(String),
    #[error("Profile not supported: {0}")]
    ProfileNotSupported(String),
    #[error("Profile chain exhausted")]
    ProfileChainExhausted,
    #[error("Transcoding process failed: {0}")]
    TranscodeFailed(String),
    #[error("Transcoding process was cancelled")]
    TranscodeCancelled,
    #[error("Transcoding completed without producing {0}")]
    MissingOutput(String),
    #[error("Parsed a partial segment")]
    #[serde(skip_serializing)]
    PartialSegment(crate::patch::segment::Segment),
}

impl From<mp4::Error> for NightfallError {
    fn from(e: mp4::Error) -> Self {
        Self::SegmentPatchError(e.to_string())
    }
}

impl From<std::io::Error> for NightfallError {
    fn from(error: std::io::Error) -> Self {
        Self::IoError(error.to_string())
    }
}
