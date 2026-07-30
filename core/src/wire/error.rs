//! Codec errors and the wire-level error code registry (PROTOCOL.md §7).

/// Errors produced while encoding or decoding wire data.
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum WireError {
    /// The message would exceed the maximum padded inner length.
    #[error("message exceeds maximum padded length")]
    TooLarge,
    /// The frame body length is not a valid padded length for its payload.
    #[error("invalid frame body length {0}")]
    BadFrameLength(usize),
    /// The frame body is not exactly the padded length required for its
    /// declared payload: the payload overruns the body, or the body is
    /// over- or under-padded.
    #[error("payload length {0} inconsistent with frame body length")]
    BadPayloadLength(usize),
    /// The payload decoded but violates a schema constraint (PROTOCOL.md §3.1).
    #[error("payload violates schema: {0}")]
    Schema(&'static str),
    /// The message type is unassigned or reserved.
    #[error("unknown or reserved message type {0:#04x}")]
    UnknownMsgType(u8),
    /// The CBOR payload failed to decode against its schema.
    #[error("invalid CBOR payload: {0}")]
    Cbor(String),
    /// Bytes remain in the payload after the CBOR item.
    #[error("trailing bytes after CBOR payload")]
    TrailingBytes,
    /// An entry signature failed verification.
    #[error("invalid entry signature")]
    BadSignature,
}

impl From<minicbor::decode::Error> for WireError {
    fn from(e: minicbor::decode::Error) -> Self {
        WireError::Cbor(e.to_string())
    }
}

/// Wire error codes carried by the ERROR message (PROTOCOL.md §7).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[repr(u64)]
pub enum ErrorCode {
    /// Malformed frame, payload, or state-machine breach.
    ProtocolViolation = 1,
    /// No protocol version in common.
    VersionIncompatible = 2,
    /// Message not permitted in the current link state.
    BadState = 3,
    /// A resource limit was exceeded.
    ResourceLimit = 4,
    /// Unspecified local failure.
    Internal = 5,
}

impl From<ErrorCode> for u64 {
    fn from(code: ErrorCode) -> u64 {
        code as u64
    }
}
