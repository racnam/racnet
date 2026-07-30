//! Negentropy range bounds: delta-encoded timestamps plus id prefixes.
//!
//! A bound names a position in the (timestamp, id) item order. On the wire
//! a timestamp of zero means infinity and every other value is `1 + delta`
//! from the previous timestamp in the same message; the id prefix carries
//! only the bytes needed to separate adjacent items, with omitted trailing
//! bytes implicitly zero.

use crate::sync::negentropy::varint::{read_varint, write_varint};
use crate::sync::SyncError;

/// Size in bytes of a Negentropy element id (the racnet entry id, §6.3).
pub const ID_SIZE: usize = 32;

/// Timestamp sentinel meaning "infinity": sorts after every real key.
pub const TS_INFINITY: u64 = u64::MAX;

/// A range bound: a (timestamp, id-prefix) position in the item order.
///
/// An item compares against a bound as if the prefix were zero-padded to
/// [`ID_SIZE`] bytes.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Bound {
    /// Bound timestamp; [`TS_INFINITY`] sorts after everything.
    pub ts: u64,
    prefix: [u8; ID_SIZE],
    prefix_len: u8,
}

impl Bound {
    /// A bound at `ts` with an id prefix.
    ///
    /// Internal invariant: `prefix` holds at most [`ID_SIZE`] bytes.
    pub fn new(ts: u64, prefix: &[u8]) -> Bound {
        assert!(prefix.len() <= ID_SIZE, "bound prefix too long");
        let mut buf = [0u8; ID_SIZE];
        buf[..prefix.len()].copy_from_slice(prefix);
        Bound {
            ts,
            prefix: buf,
            prefix_len: prefix.len() as u8,
        }
    }

    /// A bound at `ts` with an empty id prefix.
    pub fn from_ts(ts: u64) -> Bound {
        Bound::new(ts, &[])
    }

    /// The bound after every item.
    pub fn infinity() -> Bound {
        Bound::from_ts(TS_INFINITY)
    }

    /// The stored id prefix bytes.
    pub fn prefix(&self) -> &[u8] {
        &self.prefix[..usize::from(self.prefix_len)]
    }

    /// Whether the item `(ts, id)` sorts strictly before this bound.
    pub fn item_is_before(&self, ts: u64, id: &[u8; ID_SIZE]) -> bool {
        if ts != self.ts {
            return ts < self.ts;
        }
        // The prefix compares as if zero-padded to full width.
        *id < self.prefix
    }

    /// The shortest bound separating two adjacent items: strictly after
    /// `(prev_ts, prev_id)` and at or before `(curr_ts, curr_id)`.
    pub fn min_separating(
        prev_ts: u64,
        prev_id: &[u8; ID_SIZE],
        curr_ts: u64,
        curr_id: &[u8; ID_SIZE],
    ) -> Bound {
        if curr_ts != prev_ts {
            return Bound::from_ts(curr_ts);
        }
        let shared = prev_id
            .iter()
            .zip(curr_id.iter())
            .take_while(|(a, b)| a == b)
            .count();
        Bound::new(curr_ts, &curr_id[..shared + 1])
    }
}

/// Per-message bound codec holding the running timestamp delta state.
///
/// Delta state resets at each message boundary: use a fresh codec per
/// encoded or decoded Negentropy message.
#[derive(Debug, Default)]
pub struct BoundCodec {
    last_ts_in: u64,
    last_ts_out: u64,
}

impl BoundCodec {
    /// A codec with zeroed delta state, for one message.
    pub fn new() -> BoundCodec {
        BoundCodec::default()
    }

    /// Appends the encoding of `bound` to `out`.
    pub fn encode(&mut self, bound: &Bound, out: &mut Vec<u8>) {
        if bound.ts == TS_INFINITY {
            self.last_ts_out = TS_INFINITY;
            write_varint(0, out);
        } else {
            // A hostile peer can present bounds out of order, making the
            // delta negative when an input bound is echoed back; saturate
            // rather than wrap. The mismatched range re-splits next round.
            let delta = bound.ts.saturating_sub(self.last_ts_out);
            self.last_ts_out = bound.ts;
            write_varint(delta + 1, out);
        }
        write_varint(bound.prefix().len() as u64, out);
        out.extend_from_slice(bound.prefix());
    }

    /// Decodes one bound from the front of `input`, advancing the slice.
    pub fn decode(&mut self, input: &mut &[u8]) -> Result<Bound, SyncError> {
        let raw = read_varint(input)?;
        let ts = if raw == 0 || self.last_ts_in == TS_INFINITY {
            TS_INFINITY
        } else {
            // Saturation clamps to infinity, as the references do.
            (raw - 1).saturating_add(self.last_ts_in)
        };
        self.last_ts_in = ts;
        let len = read_varint(input)?;
        if len > ID_SIZE as u64 {
            return Err(SyncError::Malformed("bound prefix longer than an id"));
        }
        let len = len as usize;
        if input.len() < len {
            return Err(SyncError::Malformed("bound prefix ends prematurely"));
        }
        let (prefix, rest) = input.split_at(len);
        let bound = Bound::new(ts, prefix);
        *input = rest;
        Ok(bound)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn timestamps_are_delta_encoded_within_a_message() {
        let mut codec = BoundCodec::new();
        let mut out = Vec::new();
        codec.encode(&Bound::from_ts(1000), &mut out);
        codec.encode(&Bound::from_ts(1000), &mut out);
        codec.encode(&Bound::from_ts(1005), &mut out);
        // 1001 = 0x87 0x69; delta 0 -> 1; delta 5 -> 6. Empty prefixes.
        assert_eq!(out, [0x87, 0x69, 0x00, 0x01, 0x00, 0x06, 0x00]);

        let mut codec = BoundCodec::new();
        let mut cursor = out.as_slice();
        for expected in [1000, 1000, 1005] {
            let bound = codec.decode(&mut cursor).unwrap();
            assert_eq!(bound.ts, expected);
            assert!(bound.prefix().is_empty());
        }
        assert!(cursor.is_empty());
    }

    #[test]
    fn infinity_encodes_as_zero_and_poisons_the_delta_state() {
        let mut codec = BoundCodec::new();
        let mut out = Vec::new();
        codec.encode(&Bound::infinity(), &mut out);
        assert_eq!(out, [0x00, 0x00]);

        let mut codec = BoundCodec::new();
        let mut cursor = out.as_slice();
        assert_eq!(codec.decode(&mut cursor).unwrap().ts, TS_INFINITY);
        // After infinity, every later timestamp in the message is infinity.
        let more = [0x05, 0x00];
        let mut cursor = more.as_slice();
        assert_eq!(codec.decode(&mut cursor).unwrap().ts, TS_INFINITY);
    }

    #[test]
    fn prefixes_round_trip() {
        let mut codec = BoundCodec::new();
        let mut out = Vec::new();
        codec.encode(&Bound::new(7, &[0xAA, 0xBB]), &mut out);
        assert_eq!(out, [0x08, 0x02, 0xAA, 0xBB]);

        let mut codec = BoundCodec::new();
        let mut cursor = out.as_slice();
        let bound = codec.decode(&mut cursor).unwrap();
        assert_eq!(bound.ts, 7);
        assert_eq!(bound.prefix(), [0xAA, 0xBB]);
    }

    #[test]
    fn oversized_or_truncated_prefixes_are_errors() {
        let mut codec = BoundCodec::new();
        // len 33 > ID_SIZE
        let bytes = [0x01, 0x21];
        assert_eq!(
            codec.decode(&mut bytes.as_slice()),
            Err(SyncError::Malformed("bound prefix longer than an id"))
        );
        // len 2 with only 1 byte present
        let bytes = [0x01, 0x02, 0xAA];
        assert_eq!(
            codec.decode(&mut bytes.as_slice()),
            Err(SyncError::Malformed("bound prefix ends prematurely"))
        );
    }

    #[test]
    fn item_comparison_zero_pads_the_prefix() {
        let bound = Bound::new(5, &[0x10]);
        let low = [0x0F; ID_SIZE];
        let exact = {
            let mut id = [0u8; ID_SIZE];
            id[0] = 0x10;
            id
        };
        assert!(bound.item_is_before(4, &[0xFF; ID_SIZE]));
        assert!(bound.item_is_before(5, &low));
        // Equal to the zero-padded prefix: not before.
        assert!(!bound.item_is_before(5, &exact));
        assert!(!bound.item_is_before(5, &[0x11; ID_SIZE]));
        assert!(!bound.item_is_before(6, &[0x00; ID_SIZE]));
    }

    #[test]
    fn minimal_bound_splits_at_the_first_differing_byte() {
        let prev = [0xAA; ID_SIZE];
        let mut curr = [0xAA; ID_SIZE];
        curr[3] = 0xAB;
        // Different timestamps: timestamp alone separates.
        let bound = Bound::min_separating(10, &prev, 11, &curr);
        assert_eq!((bound.ts, bound.prefix()), (11, &[][..]));
        // Equal timestamps: shared prefix plus one byte.
        let bound = Bound::min_separating(10, &prev, 10, &curr);
        assert_eq!(bound.ts, 10);
        assert_eq!(bound.prefix(), [0xAA, 0xAA, 0xAA, 0xAB]);
        assert!(!bound.item_is_before(10, &curr));
        assert!(bound.item_is_before(10, &prev));
    }
}
