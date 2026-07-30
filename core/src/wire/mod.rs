//! Wire protocol codec: framing, padding, and message serialization.
//!
//! Implements `docs/PROTOCOL.md` v0.1. The spec is the source of truth; where
//! this module disagrees with it, this module is wrong. The session layer
//! (Noise) specified there is not implemented yet; in the cleartext epoch the
//! outer frame body is the padded inner message directly.

mod entry;
mod error;
mod frame;
mod msg;
mod pad;
mod payload;

pub use entry::Entry;
pub use error::{ErrorCode, WireError};
pub use frame::{encode_frame, FrameDecoder};
pub use msg::{decode_message, encode_message, Message, MsgType};
pub use pad::{padded_len, BLOCK_SIZES, MAX_PADDED_LEN};
pub use payload::{
    ErrorMsg, GossipPush, Handshake, Hello, ReconDone, ReconInit, ReconMsg, SortKeyWindow,
};

/// The wire protocol version implemented by this crate (spec v0.1).
pub const WIRE_VERSION: u64 = 1;
