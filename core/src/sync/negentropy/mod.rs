//! Negentropy V1, implemented in-house (ADR-0010).
//!
//! The encoding here is defined by the upstream Negentropy specification,
//! not by PROTOCOL.md: racnet carries these messages as opaque byte strings
//! (§6.3, ADR-0007), and byte-exactness is checked by running the upstream
//! conformance suite against this implementation
//! (`scripts/negentropy-conformance.sh`, pinned commit in ADR-0010).

pub mod bound;
pub mod fingerprint;
pub mod store;
pub mod varint;

use std::collections::HashSet;

use crate::store::EntryId;
use crate::sync::SyncError;
use bound::{Bound, BoundCodec, ID_SIZE};
use fingerprint::FINGERPRINT_SIZE;
use store::NegentropyStore;
use varint::{read_varint, write_varint};

/// The Negentropy protocol version byte this engine speaks (V1).
pub const PROTOCOL_VERSION: u8 = 0x61;

/// Number of fingerprint buckets a mismatched range splits into.
const BUCKETS: usize = 16;

/// Ranges smaller than this are sent as id lists instead of fingerprints.
const ID_LIST_THRESHOLD: usize = BUCKETS * 2;

/// Safety margin subtracted from the frame-size limit, as upstream does.
const FRAME_MARGIN: usize = 200;

/// Smallest accepted frame-size limit, matching the upstream floor. The
/// floor is what makes constrained sessions terminate: the remaining
/// budget at the start of a message must always fit one full range split
/// (an id list runs to ~1 KB), or that range could never be resolved and
/// reconciliation would livelock (ADR-0010).
pub const FRAME_LIMIT_FLOOR: usize = 4096;

/// Range modes on the wire.
const MODE_SKIP: u64 = 0;
const MODE_FINGERPRINT: u64 = 1;
const MODE_ID_LIST: u64 = 2;

/// The outcome of processing one incoming Negentropy message.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Round {
    /// The next message to send, or `None` when an initiator has finished.
    /// A responder always has a reply.
    pub reply: Option<Vec<u8>>,
    /// Ids we hold and the peer lacks. Only an initiator learns these; may
    /// repeat across rounds after frame-limit continuations.
    pub have: Vec<EntryId>,
    /// Ids the peer holds and we lack. Only an initiator learns these; may
    /// repeat across rounds after frame-limit continuations.
    pub need: Vec<EntryId>,
}

/// A Negentropy V1 reconciliation engine over a sealed item set.
///
/// One engine serves one session. The side that opens the session calls
/// [`initiate`](Negentropy::initiate) once and then feeds every peer reply
/// to [`reconcile`](Negentropy::reconcile); the other side only calls
/// `reconcile`. The engine is stateless between rounds apart from the
/// initiator flag — that is Negentropy's design.
#[derive(Debug)]
pub struct Negentropy<S> {
    store: S,
    frame_limit: Option<usize>,
    is_initiator: bool,
}

impl<S: NegentropyStore> Negentropy<S> {
    /// An engine over `store`, capping outgoing messages at `frame_limit`
    /// bytes when given (`initiate` output is not capped, matching
    /// upstream; it is bounded by ~1 KB regardless of store size).
    pub fn new(store: S, frame_limit: Option<usize>) -> Result<Negentropy<S>, SyncError> {
        if frame_limit.is_some_and(|limit| limit < FRAME_LIMIT_FLOOR) {
            return Err(SyncError::FrameLimitTooSmall(FRAME_LIMIT_FLOOR));
        }
        Ok(Negentropy {
            store,
            frame_limit,
            is_initiator: false,
        })
    }

    /// Opens reconciliation: marks this engine the initiator and returns
    /// the first message.
    pub fn initiate(&mut self) -> Result<Vec<u8>, SyncError> {
        if self.is_initiator {
            return Err(SyncError::AlreadyInitiated);
        }
        self.is_initiator = true;
        let mut out = vec![PROTOCOL_VERSION];
        let mut codec = BoundCodec::new();
        self.split_range(
            0,
            self.store.len(),
            &Bound::infinity(),
            &mut codec,
            &mut out,
        );
        Ok(out)
    }

    /// Whether this engine initiated the session.
    pub fn is_initiator(&self) -> bool {
        self.is_initiator
    }

    /// Processes one incoming message and produces the [`Round`] outcome.
    ///
    /// Never panics on any input; malformed bytes yield an error, which is
    /// terminal for the session.
    pub fn reconcile(&mut self, msg: &[u8]) -> Result<Round, SyncError> {
        let mut have = Vec::new();
        let mut need = Vec::new();
        let mut cursor = msg;

        let (&version, rest) = cursor
            .split_first()
            .ok_or(SyncError::Malformed("empty message"))?;
        cursor = rest;
        if !(0x60..=0x6F).contains(&version) {
            return Err(SyncError::Malformed("invalid protocol version byte"));
        }
        if version != PROTOCOL_VERSION {
            if self.is_initiator {
                return Err(SyncError::UnsupportedVersion(version));
            }
            // Version negotiation: answer with our preferred version alone.
            return Ok(Round {
                reply: Some(vec![PROTOCOL_VERSION]),
                have,
                need,
            });
        }

        let n = self.store.len();
        let mut full = vec![PROTOCOL_VERSION];
        let mut codec = BoundCodec::new();
        let mut prev_rank = 0usize;
        let mut prev_bound = Bound::from_ts(0);
        let mut skip = false;

        while !cursor.is_empty() {
            let mut o: Vec<u8> = Vec::new();
            let curr_bound = codec.decode(&mut cursor)?;
            let mode = read_varint(&mut cursor)?;

            let lower = prev_rank;
            let mut upper = self.store.lower_bound(prev_rank, n, &curr_bound);

            match mode {
                MODE_SKIP => skip = true,
                MODE_FINGERPRINT => {
                    if cursor.len() < FINGERPRINT_SIZE {
                        return Err(SyncError::Malformed("fingerprint ends prematurely"));
                    }
                    let (their_fp, rest) = cursor.split_at(FINGERPRINT_SIZE);
                    cursor = rest;
                    let ours = self.store.fingerprint(lower..upper);
                    if their_fp != ours.0 {
                        flush_skip(&mut skip, &prev_bound, &mut codec, &mut o);
                        self.split_range(lower, upper, &curr_bound, &mut codec, &mut o);
                    } else {
                        skip = true;
                    }
                }
                MODE_ID_LIST => {
                    let num_ids = read_varint(&mut cursor)?;
                    if num_ids > (cursor.len() / ID_SIZE) as u64 {
                        return Err(SyncError::Malformed("id list longer than the message"));
                    }
                    let mut their_order: Vec<EntryId> = Vec::with_capacity(num_ids as usize);
                    let mut theirs: HashSet<EntryId> = HashSet::with_capacity(num_ids as usize);
                    for _ in 0..num_ids {
                        let (id_bytes, rest) = cursor.split_at(ID_SIZE);
                        cursor = rest;
                        let id: EntryId = id_bytes.try_into().expect("split at ID_SIZE");
                        if theirs.insert(id) {
                            their_order.push(id);
                        }
                    }

                    if self.is_initiator {
                        skip = true;
                        self.store.for_each(lower..upper, &mut |_, item| {
                            if !theirs.remove(&item.id) {
                                have.push(item.id);
                            }
                            true
                        });
                        // Ids still unmatched exist only on their side.
                        need.extend(their_order.iter().filter(|id| theirs.contains(*id)));
                    } else {
                        flush_skip(&mut skip, &prev_bound, &mut codec, &mut o);
                        let mut response_ids: Vec<u8> = Vec::new();
                        let mut num_response = 0u64;
                        let mut end_bound = curr_bound;
                        let limit = self.frame_limit;
                        let full_len = full.len();
                        self.store.for_each(lower..upper, &mut |rank, item| {
                            if exceeds_limit(limit, full_len + response_ids.len()) {
                                end_bound = Bound::at_item(item.sort_key, &item.id);
                                // Shrink so the remaining range gets the
                                // correct continuation fingerprint.
                                upper = rank;
                                return false;
                            }
                            response_ids.extend_from_slice(&item.id);
                            num_response += 1;
                            true
                        });
                        codec.encode(&end_bound, &mut o);
                        write_varint(MODE_ID_LIST, &mut o);
                        write_varint(num_response, &mut o);
                        o.extend_from_slice(&response_ids);
                        full.append(&mut o);
                    }
                }
                _ => return Err(SyncError::Malformed("unknown range mode")),
            }

            if exceeds_limit(self.frame_limit, full.len() + o.len()) {
                // Stop processing and cover everything after `upper` with
                // one coalesced fingerprint; it re-splits next round.
                let remaining = self.store.fingerprint(upper..n);
                codec.encode(&Bound::infinity(), &mut full);
                write_varint(MODE_FINGERPRINT, &mut full);
                full.extend_from_slice(&remaining.0);
                break;
            }
            full.append(&mut o);
            prev_rank = upper;
            prev_bound = curr_bound;
        }

        let reply = if self.is_initiator && full.len() == 1 {
            None
        } else {
            Some(full)
        };
        Ok(Round { reply, have, need })
    }

    /// Covers `lower..upper` in the output: an id list when small, sixteen
    /// fingerprint buckets otherwise.
    fn split_range(
        &self,
        lower: usize,
        upper: usize,
        upper_bound: &Bound,
        codec: &mut BoundCodec,
        out: &mut Vec<u8>,
    ) {
        let num_elems = upper - lower;
        if num_elems < ID_LIST_THRESHOLD {
            codec.encode(upper_bound, out);
            write_varint(MODE_ID_LIST, out);
            write_varint(num_elems as u64, out);
            self.store.for_each(lower..upper, &mut |_, item| {
                out.extend_from_slice(&item.id);
                true
            });
        } else {
            let items_per_bucket = num_elems / BUCKETS;
            let buckets_with_extra = num_elems % BUCKETS;
            let mut curr = lower;
            for i in 0..BUCKETS {
                let bucket_size = items_per_bucket + usize::from(i < buckets_with_extra);
                let fp = self.store.fingerprint(curr..curr + bucket_size);
                curr += bucket_size;
                let next_bound = if curr == upper {
                    *upper_bound
                } else {
                    let prev = self.store.item_at(curr - 1);
                    let next = self.store.item_at(curr);
                    Bound::min_separating(prev.sort_key, &prev.id, next.sort_key, &next.id)
                };
                codec.encode(&next_bound, out);
                write_varint(MODE_FINGERPRINT, out);
                out.extend_from_slice(&fp.0);
            }
        }
    }
}

/// Emits the pending coalesced Skip range, if any.
fn flush_skip(skip: &mut bool, prev_bound: &Bound, codec: &mut BoundCodec, out: &mut Vec<u8>) {
    if *skip {
        *skip = false;
        codec.encode(prev_bound, out);
        write_varint(MODE_SKIP, out);
    }
}

/// Whether an output of `len` bytes exceeds the frame-size budget.
fn exceeds_limit(limit: Option<usize>, len: usize) -> bool {
    limit.is_some_and(|limit| len > limit - FRAME_MARGIN)
}

#[cfg(test)]
mod tests {
    use super::store::SealedVec;
    use super::*;
    use crate::store::Item;

    fn item(sort_key: u64, seed: u8) -> Item {
        Item {
            sort_key,
            id: [seed; 32],
        }
    }

    fn engine(items: Vec<Item>, limit: Option<usize>) -> Negentropy<SealedVec> {
        Negentropy::new(SealedVec::new(items), limit).unwrap()
    }

    #[test]
    fn frame_limit_floor_is_enforced() {
        assert_eq!(
            Negentropy::new(SealedVec::default(), Some(FRAME_LIMIT_FLOOR - 1)).unwrap_err(),
            SyncError::FrameLimitTooSmall(FRAME_LIMIT_FLOOR)
        );
        assert!(Negentropy::new(SealedVec::default(), Some(FRAME_LIMIT_FLOOR)).is_ok());
    }

    #[test]
    fn initiate_twice_is_an_error() {
        let mut ne = engine(vec![], None);
        ne.initiate().unwrap();
        assert_eq!(ne.initiate(), Err(SyncError::AlreadyInitiated));
    }

    #[test]
    fn empty_stores_finish_in_one_round_trip() {
        let mut a = engine(vec![], None);
        let mut b = engine(vec![], None);
        let init = a.initiate().unwrap();
        // Empty store: one empty id-list range up to infinity.
        assert_eq!(init, [0x61, 0x00, 0x00, 0x02, 0x00]);
        let reply = b
            .reconcile(&init)
            .unwrap()
            .reply
            .expect("responder replies");
        let done = a.reconcile(&reply).unwrap();
        assert_eq!(done.reply, None);
        assert!(done.have.is_empty() && done.need.is_empty());
    }

    #[test]
    fn small_sets_resolve_via_id_lists() {
        let shared: Vec<Item> = (0..10).map(|i| item(i, i as u8)).collect();
        let mut ours = shared.clone();
        ours.push(item(100, 100));
        let mut theirs = shared;
        theirs.push(item(200, 200));

        let mut a = engine(ours, None);
        let mut b = engine(theirs, None);
        let mut msg = a.initiate().unwrap();
        let mut have = Vec::new();
        let mut need = Vec::new();
        for _ in 0..8 {
            let round = b.reconcile(&msg).unwrap();
            let reply = round.reply.expect("responder always replies");
            let round = a.reconcile(&reply).unwrap();
            have.extend(round.have);
            need.extend(round.need);
            match round.reply {
                Some(next) => msg = next,
                None => break,
            }
        }
        assert_eq!(have, [[100u8; 32]]);
        assert_eq!(need, [[200u8; 32]]);
    }

    #[test]
    fn version_negotiation_replies_with_our_version_byte() {
        // A responder answers an unknown (future) version with 0x61 alone.
        let mut responder = engine(vec![item(1, 1)], None);
        let round = responder.reconcile(&[0x62, 0x00, 0x00, 0x00]).unwrap();
        assert_eq!(round.reply, Some(vec![PROTOCOL_VERSION]));

        // An initiator treats that reply as unsupported-version.
        let mut initiator = engine(vec![item(1, 1)], None);
        initiator.initiate().unwrap();
        assert_eq!(
            initiator.reconcile(&[0x62]),
            Err(SyncError::UnsupportedVersion(0x62))
        );

        // Bytes outside the version range are malformed.
        let mut other = engine(vec![], None);
        assert_eq!(
            other.reconcile(&[0x42]),
            Err(SyncError::Malformed("invalid protocol version byte"))
        );
        assert_eq!(
            other.reconcile(&[]),
            Err(SyncError::Malformed("empty message"))
        );
    }

    #[test]
    fn truncated_and_oversized_inputs_are_errors() {
        let mut ne = engine(vec![item(1, 1)], None);
        // Fingerprint mode with a truncated fingerprint.
        assert_eq!(
            ne.reconcile(&[0x61, 0x00, 0x00, 0x01, 0xAA]),
            Err(SyncError::Malformed("fingerprint ends prematurely"))
        );
        // Id list claiming more ids than the message holds.
        assert_eq!(
            ne.reconcile(&[0x61, 0x00, 0x00, 0x02, 0x05, 0x01]),
            Err(SyncError::Malformed("id list longer than the message"))
        );
        // Unknown mode.
        assert_eq!(
            ne.reconcile(&[0x61, 0x00, 0x00, 0x03]),
            Err(SyncError::Malformed("unknown range mode"))
        );
    }
}
