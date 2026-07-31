//! The FFI surface: session-level operations behind a single [`Node`]
//! facade (ADR-0014).
//!
//! Everything here is a thin, exported mirror of the internal layers:
//! records and enums cross UniFFI by value, links are addressed by `u64`
//! ids, and every call returns the frames to write and the events that
//! happened — the sans-I/O contract of the layers below, preserved across
//! the language boundary. The transport shim owns sockets, threads, and
//! the clock; time arrives as microseconds and only deltas matter.

mod node;

use crate::link::CloseReason;
use crate::link::LinkEvent;
use crate::noise::Keypair;
use crate::store::EntryStore;
use crate::sync::SyncEvent;
use crate::wire::Entry;

pub use node::Node;

/// A device identity: the two seeds everything else derives from. The
/// caller persists these encrypted at rest; the `SecretKey` type itself
/// never crosses the boundary (ADR-0014).
#[derive(uniffi::Record)]
pub struct Identity {
    /// 32-byte X25519 static secret (Noise identity, PROTOCOL.md §4).
    pub noise_seed: Vec<u8>,
    /// 32-byte Ed25519 signing seed (entry authorship, PROTOCOL.md §3.5).
    pub signing_seed: Vec<u8>,
}

/// A sort-key window `[since, until)` in milliseconds since the Unix
/// epoch; `until` = 2^64 − 1 means "everything from `since`" (§6.2).
#[derive(uniffi::Record, Clone, Copy)]
pub struct SyncWindow {
    /// Inclusive lower bound.
    pub since_ms: u64,
    /// Exclusive upper bound.
    pub until_ms: u64,
}

impl From<SyncWindow> for crate::wire::SortKeyWindow {
    fn from(window: SyncWindow) -> crate::wire::SortKeyWindow {
        crate::wire::SortKeyWindow {
            since: window.since_ms,
            until: window.until_ms,
        }
    }
}

/// A newly opened link.
#[derive(uniffi::Record)]
pub struct LinkOpen {
    /// Handle for every later call about this link.
    pub link_id: u64,
    /// Frames to write immediately: the initiator's HELLO; empty for a
    /// responder, which speaks only once the peer's HELLO arrives.
    pub initial_frames: Vec<Vec<u8>>,
}

/// Output of feeding a link: write the frames to that link's transport
/// in order, then act on the events (a close may carry a final ERROR
/// frame that must still be written).
#[derive(uniffi::Record, Default)]
pub struct LinkIo {
    /// Complete outer frames for the link the call named, in order.
    pub frames: Vec<Vec<u8>>,
    /// What happened, in order.
    pub events: Vec<Event>,
}

/// A successfully opened reconciliation session.
#[derive(uniffi::Record)]
pub struct SyncStart {
    /// The session id (§6.2).
    pub session_id: u64,
    /// Frames to write and events, as with any link call.
    pub io: LinkIo,
}

/// One stored entry, as the app sees it.
#[derive(uniffi::Record)]
pub struct EntryView {
    /// 32-byte entry id (§3.5).
    pub id: Vec<u8>,
    /// Author's 32-byte Ed25519 public key.
    pub author: Vec<u8>,
    /// Sort key: milliseconds since the Unix epoch, as claimed by the
    /// author (§6.1).
    pub sort_key_ms: u64,
    /// Application-level entry kind.
    pub kind: u64,
    /// Application payload, opaque to this layer.
    pub payload: Vec<u8>,
}

/// Point-in-time link state.
#[derive(uniffi::Record)]
pub struct LinkStatus {
    /// True once the handshake completed.
    pub established: bool,
    /// The peer's 32-byte fingerprint, once established.
    pub remote_fingerprint: Option<Vec<u8>>,
}

/// Why a link closed. Local-only, like the internal reason it mirrors
/// (§7): safe for logs and diagnostics, never sent anywhere.
#[derive(uniffi::Enum, Debug, Clone, PartialEq, Eq)]
pub enum CloseCause {
    /// Empty version intersection (§5).
    VersionIncompatible,
    /// The cleartext epoch failed: bad frame, wrong message, or a failed
    /// handshake step.
    HandshakeFailed,
    /// A transport-epoch body failed the pre-decryption length gate.
    BadCiphertextLength,
    /// Transport-message decryption or authentication failed.
    DecryptFailed,
    /// The peer broke a rule; an encrypted ERROR with this code was sent.
    ProtocolViolation {
        /// The §7 error code.
        code: u64,
    },
    /// The peer sent ERROR with this code.
    PeerError {
        /// The §7 error code.
        code: u64,
    },
    /// The 24-hour lifetime cap passed (§4.3); reconnect to rehandshake.
    LifetimeExpired,
    /// A CipherState nonce ran out (§4.3).
    NonceExhausted,
    /// The shim reported the transport connection gone.
    TransportClosed,
    /// The §4.5 half-open timeout fired; close the transport connection.
    HandshakeTimeout,
}

/// Something a node call learned or did. Every variant names its link.
#[derive(uniffi::Enum, Debug, Clone, PartialEq, Eq)]
pub enum Event {
    /// The handshake completed; the peer's static key authenticated.
    Established {
        /// The link that established.
        link_id: u64,
        /// SHA-256 fingerprint of the peer's static public key.
        remote_fingerprint: Vec<u8>,
    },
    /// A pushed entry was verified and stored.
    EntryStored {
        /// The link it arrived on.
        link_id: u64,
        /// The stored entry's id.
        entry_id: Vec<u8>,
    },
    /// A pushed entry was already held; ignored.
    DuplicateIgnored {
        /// The link it arrived on.
        link_id: u64,
        /// The duplicate entry's id.
        entry_id: Vec<u8>,
    },
    /// A reconciliation round reported differences (§6.2).
    Reconciled {
        /// The link the session runs on.
        link_id: u64,
        /// The session id.
        session_id: u64,
        /// Newly discovered entries only we hold (already queued as
        /// pushes to the peer).
        have: Vec<Vec<u8>>,
        /// Newly discovered entries only the peer holds (they arrive via
        /// the peer's own pushes).
        need: Vec<Vec<u8>>,
    },
    /// A reconciliation session closed, by us or by the peer.
    SyncSessionClosed {
        /// The link the session ran on.
        link_id: u64,
        /// The session id.
        session_id: u64,
        /// Whether we had opened it.
        opened_by_us: bool,
    },
    /// The link closed. Terminal: drop the transport connection after
    /// writing any frames returned alongside this event.
    Closed {
        /// The link that closed.
        link_id: u64,
        /// Why, locally.
        cause: CloseCause,
    },
}

/// Local API misuse or environment failure; nothing here reflects peer
/// behavior on the wire.
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error, uniffi::Error)]
pub enum ApiError {
    /// Identity seeds must be exactly 32 bytes each.
    #[error("identity seeds must be exactly 32 bytes each")]
    InvalidIdentity,
    /// The link id names no live link.
    #[error("unknown link id")]
    UnknownLink,
    /// The link is not (or not yet) established.
    #[error("link not established")]
    NotEstablished,
    /// The sync layer refused the operation.
    #[error("sync refused: {reason}")]
    SyncRefused {
        /// Human-readable local detail.
        reason: String,
    },
    /// A caller-supplied value is out of range.
    #[error("invalid argument: {reason}")]
    InvalidArgument {
        /// Human-readable local detail.
        reason: String,
    },
    /// Platform entropy was unavailable.
    #[error("platform entropy unavailable")]
    Entropy,
}

/// Generates a fresh device identity from platform entropy.
#[uniffi::export]
pub fn generate_identity() -> Result<Identity, ApiError> {
    let mut noise_seed = vec![0u8; 32];
    let mut signing_seed = vec![0u8; 32];
    getrandom::fill(&mut noise_seed).map_err(|_| ApiError::Entropy)?;
    getrandom::fill(&mut signing_seed).map_err(|_| ApiError::Entropy)?;
    Ok(Identity {
        noise_seed,
        signing_seed,
    })
}

/// Generates a fresh per-link ephemeral keypair.
fn fresh_ephemeral() -> Result<Keypair, ApiError> {
    Keypair::generate().map_err(|_| ApiError::Entropy)
}

/// Maps an internal close reason to its exported cause. Exhaustive by
/// construction: a new internal variant fails compilation here.
fn close_cause(reason: &CloseReason) -> CloseCause {
    match reason {
        CloseReason::VersionIncompatible => CloseCause::VersionIncompatible,
        CloseReason::HandshakeFailed => CloseCause::HandshakeFailed,
        CloseReason::BadCiphertextLength => CloseCause::BadCiphertextLength,
        CloseReason::DecryptFailed => CloseCause::DecryptFailed,
        CloseReason::ProtocolViolation(code) => CloseCause::ProtocolViolation { code: *code },
        CloseReason::PeerError(code) => CloseCause::PeerError { code: *code },
        CloseReason::LifetimeExpired => CloseCause::LifetimeExpired,
        CloseReason::NonceExhausted => CloseCause::NonceExhausted,
    }
}

/// Maps one internal link event to its exported form.
fn map_event(link_id: u64, event: &LinkEvent) -> Event {
    match event {
        LinkEvent::Established { remote } => Event::Established {
            link_id,
            remote_fingerprint: remote.0.to_vec(),
        },
        LinkEvent::Sync(sync) => match sync {
            SyncEvent::Stored(id) => Event::EntryStored {
                link_id,
                entry_id: id.to_vec(),
            },
            SyncEvent::DuplicateIgnored(id) => Event::DuplicateIgnored {
                link_id,
                entry_id: id.to_vec(),
            },
            SyncEvent::Reconciled { sid, have, need } => Event::Reconciled {
                link_id,
                session_id: *sid,
                have: have.iter().map(|id| id.to_vec()).collect(),
                need: need.iter().map(|id| id.to_vec()).collect(),
            },
            SyncEvent::SessionClosed { sid, opened_by_us } => Event::SyncSessionClosed {
                link_id,
                session_id: *sid,
                opened_by_us: *opened_by_us,
            },
        },
        LinkEvent::Closed(reason) => Event::Closed {
            link_id,
            cause: close_cause(reason),
        },
    }
}

/// One stored entry as its exported view.
fn entry_view(entry: &Entry) -> EntryView {
    EntryView {
        id: entry.id().to_vec(),
        author: entry.author.to_vec(),
        sort_key_ms: entry.sort_key,
        kind: entry.kind,
        payload: entry.payload.clone(),
    }
}

/// The entries in `window`, in §6.1 order.
fn entries_in_window(store: &EntryStore, window: SyncWindow) -> Vec<EntryView> {
    store
        .snapshot(window.into())
        .iter()
        .filter_map(|item| store.get(&item.id))
        .map(entry_view)
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn generated_identities_are_distinct_32_byte_seeds() {
        let a = generate_identity().unwrap();
        let b = generate_identity().unwrap();
        assert_eq!(a.noise_seed.len(), 32);
        assert_eq!(a.signing_seed.len(), 32);
        assert_ne!(a.noise_seed, b.noise_seed);
        assert_ne!(a.signing_seed, b.signing_seed);
        assert_ne!(a.noise_seed, a.signing_seed);
    }

    #[test]
    fn close_causes_carry_their_codes() {
        assert_eq!(
            close_cause(&CloseReason::ProtocolViolation(3)),
            CloseCause::ProtocolViolation { code: 3 }
        );
        assert_eq!(
            close_cause(&CloseReason::PeerError(4)),
            CloseCause::PeerError { code: 4 }
        );
    }

    #[test]
    fn sync_events_map_with_their_link_id() {
        let event = LinkEvent::Sync(SyncEvent::Reconciled {
            sid: 2,
            have: vec![[1u8; 32]],
            need: vec![[2u8; 32], [3u8; 32]],
        });
        assert_eq!(
            map_event(7, &event),
            Event::Reconciled {
                link_id: 7,
                session_id: 2,
                have: vec![vec![1u8; 32]],
                need: vec![vec![2u8; 32], vec![3u8; 32]],
            }
        );
    }
}
