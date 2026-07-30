//! CBOR payload schemas for the active message types (PROTOCOL.md §3).
//!
//! All payloads are maps with unsigned-integer keys except where the spec
//! defines a fixed positional array. Encoding follows RFC 8949 §4.2.1 core
//! deterministic encoding; the derive emits keys in ascending order, which is
//! canonical for these key values. Decoders ignore unknown map keys, giving
//! additive extensibility without a version bump.

use minicbor::{Decode, Encode};

use super::entry::Entry;

/// HELLO payload: version and capability announce (PROTOCOL.md §3.2).
#[derive(Debug, Clone, PartialEq, Eq, Encode, Decode)]
#[cbor(map)]
pub struct Hello {
    /// Protocol versions the sender can speak.
    #[n(0)]
    pub versions: Vec<u64>,
    /// Feature bitset; zero in v0.1.
    #[n(1)]
    pub features: u64,
    /// Transport identifiers the sender can accept a data plane on.
    #[n(2)]
    pub transports: Vec<u64>,
}

/// HANDSHAKE payload: one Noise handshake message (PROTOCOL.md §3.3).
#[derive(Debug, Clone, PartialEq, Eq, Encode, Decode)]
#[cbor(map)]
pub struct Handshake {
    /// Raw Noise handshake message bytes.
    #[n(0)]
    #[cbor(with = "minicbor::bytes")]
    pub msg: Vec<u8>,
}

/// GOSSIP_PUSH payload: eager push of entries (PROTOCOL.md §3.4).
#[derive(Debug, Clone, PartialEq, Eq, Encode, Decode)]
#[cbor(map)]
pub struct GossipPush {
    /// The entries being pushed.
    #[n(0)]
    pub entries: Vec<Entry>,
    /// Remaining hop budget; decremented before relaying.
    #[n(1)]
    pub ttl: u64,
}

/// Sort-key range scoping a reconciliation session (PROTOCOL.md §6.2).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Encode, Decode)]
#[cbor(array)]
pub struct SortKeyWindow {
    /// Inclusive lower bound on entry sort keys.
    #[n(0)]
    pub since: u64,
    /// Exclusive upper bound on entry sort keys.
    #[n(1)]
    pub until: u64,
}

/// RECON_INIT payload: opens a reconciliation session (PROTOCOL.md §6.2).
#[derive(Debug, Clone, PartialEq, Eq, Encode, Decode)]
#[cbor(map)]
pub struct ReconInit {
    /// Session identifier, unique per link while the session is open.
    #[n(0)]
    pub sid: u64,
    /// Sort-key window this session reconciles.
    #[n(1)]
    pub window: SortKeyWindow,
    /// First Negentropy message, carried opaquely.
    #[n(2)]
    #[cbor(with = "minicbor::bytes")]
    pub msg: Vec<u8>,
}

/// RECON_MSG payload: one Negentropy round (PROTOCOL.md §6.2).
#[derive(Debug, Clone, PartialEq, Eq, Encode, Decode)]
#[cbor(map)]
pub struct ReconMsg {
    /// Session identifier from the opening RECON_INIT.
    #[n(0)]
    pub sid: u64,
    /// Negentropy message bytes, carried opaquely.
    #[n(1)]
    #[cbor(with = "minicbor::bytes")]
    pub msg: Vec<u8>,
}

/// RECON_DONE payload: closes a reconciliation session (PROTOCOL.md §6.2).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Encode, Decode)]
#[cbor(map)]
pub struct ReconDone {
    /// Session identifier being closed.
    #[n(0)]
    pub sid: u64,
}

/// ERROR payload: a terminal error code, nothing else (PROTOCOL.md §7).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Encode, Decode)]
#[cbor(map)]
pub struct ErrorMsg {
    /// Error code from the registry in PROTOCOL.md §7.
    #[n(0)]
    pub code: u64,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn derive_output_is_rfc8949_deterministic() {
        // Hand-computed canonical encoding: map keys in ascending order,
        // shortest-form integers, definite lengths (RFC 8949 §4.2.1).
        let hello = Hello {
            versions: vec![1],
            features: 0,
            transports: vec![1],
        };
        let mut out = Vec::new();
        minicbor::encode(&hello, &mut out).unwrap();
        assert_eq!(out, [0xa3, 0x00, 0x81, 0x01, 0x01, 0x00, 0x02, 0x81, 0x01]);
    }

    #[test]
    fn integers_use_shortest_form() {
        let mut out = Vec::new();
        minicbor::encode(ReconDone { sid: 23 }, &mut out).unwrap();
        assert_eq!(out, [0xa1, 0x00, 0x17]);
        out.clear();
        minicbor::encode(ReconDone { sid: 24 }, &mut out).unwrap();
        assert_eq!(out, [0xa1, 0x00, 0x18, 0x18]);
    }

    #[test]
    fn unknown_map_keys_are_ignored() {
        // {0: 5, 9: 7} — key 9 is unknown to ReconDone and must be skipped.
        let bytes = [0xa2, 0x00, 0x05, 0x09, 0x07];
        let done: ReconDone = minicbor::decode(&bytes).unwrap();
        assert_eq!(done, ReconDone { sid: 5 });
    }
}
