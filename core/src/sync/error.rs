//! Errors for the sync layer (PROTOCOL.md §6–§7).

use crate::wire::ErrorCode;

/// Errors produced by reconciliation and sync-session handling.
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum SyncError {
    /// The peer broke a session or entry rule (PROTOCOL.md §6–§7).
    #[error("protocol violation: {0}")]
    Violation(&'static str),
    /// A local resource limit (e.g. concurrent sessions) was exceeded.
    #[error("resource limit exceeded")]
    ResourceLimit,
    /// A Negentropy message failed to parse or violated an encoding rule.
    #[error("malformed negentropy message: {0}")]
    Malformed(&'static str),
    /// The peer runs a Negentropy protocol version we do not speak.
    #[error("unsupported negentropy protocol version byte {0:#04x}")]
    UnsupportedVersion(u8),
    /// The configured frame-size limit is below the supported floor.
    #[error("frame size limit below {0}-byte floor")]
    FrameLimitTooSmall(usize),
    /// `initiate` was called on an engine that already initiated.
    #[error("reconciliation already initiated")]
    AlreadyInitiated,
}

impl SyncError {
    /// The §7 wire error code a link should report before closing when
    /// handling a peer message fails with this error.
    pub fn error_code(&self) -> ErrorCode {
        match self {
            SyncError::Violation(_) | SyncError::Malformed(_) => ErrorCode::ProtocolViolation,
            SyncError::ResourceLimit => ErrorCode::ResourceLimit,
            // Local misuse or an unsupported peer, not a peer violation.
            SyncError::UnsupportedVersion(_)
            | SyncError::FrameLimitTooSmall(_)
            | SyncError::AlreadyInitiated => ErrorCode::Internal,
        }
    }
}
