//! Errors for the sync layer (PROTOCOL.md §6–§7).

/// Errors produced by reconciliation and sync-session handling.
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum SyncError {
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
