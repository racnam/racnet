//! Property tests for the Negentropy engine: two engines over random
//! overlapping sets always converge on the exact difference, within the
//! frame-size budget; malformed input never panics.

use std::collections::BTreeSet;

use proptest::prelude::*;

use racnet_core::store::Item;
use racnet_core::sync::negentropy::store::SealedVec;
use racnet_core::sync::negentropy::{Negentropy, FRAME_LIMIT_FLOOR};

/// Upper bound on rounds before we call a session non-terminating.
const MAX_ROUNDS: usize = 64;

fn item_strategy() -> impl Strategy<Value = Item> {
    // Narrow key/id spaces force overlaps, shared prefixes, and sort-key
    // ties — the cases that stress bounds and bucket splitting. The id
    // incorporates the sort key because real entry ids are hashes over the
    // entry including its sort key: one id cannot have two sort keys.
    (0u64..10_000, any::<u8>(), 0u8..4).prop_map(|(sort_key, seed, spread)| {
        let mut id = [seed; 32];
        id[23..31].copy_from_slice(&sort_key.to_be_bytes());
        id[31] = spread;
        Item { sort_key, id }
    })
}

fn set_strategy(max: usize) -> impl Strategy<Value = Vec<Item>> {
    proptest::collection::vec(item_strategy(), 0..max)
}

/// Drives a full session; returns (initiator have, initiator need, rounds).
fn run_session(
    ours: &[Item],
    theirs: &[Item],
    limit_a: Option<usize>,
    limit_b: Option<usize>,
) -> (BTreeSet<[u8; 32]>, BTreeSet<[u8; 32]>, usize) {
    let mut a = Negentropy::new(SealedVec::new(ours.to_vec()), limit_a).unwrap();
    let mut b = Negentropy::new(SealedVec::new(theirs.to_vec()), limit_b).unwrap();
    let mut have = BTreeSet::new();
    let mut need = BTreeSet::new();
    let mut msg = a.initiate().unwrap();
    for rounds in 1..=MAX_ROUNDS {
        let response = b.reconcile(&msg).unwrap();
        assert!(response.have.is_empty() && response.need.is_empty());
        let reply = response.reply.expect("a responder always replies");
        if let Some(limit) = limit_b {
            assert!(reply.len() <= limit, "responder exceeded its frame limit");
        }
        let round = a.reconcile(&reply).unwrap();
        have.extend(round.have);
        need.extend(round.need);
        match round.reply {
            None => return (have, need, rounds),
            Some(next) => {
                if let Some(limit) = limit_a {
                    assert!(next.len() <= limit, "initiator exceeded its frame limit");
                }
                msg = next;
            }
        }
    }
    panic!("session did not terminate within {MAX_ROUNDS} rounds");
}

proptest! {
    #[test]
    fn sessions_converge_on_the_exact_difference(
        ours in set_strategy(400),
        theirs in set_strategy(400),
        limit_a in proptest::option::of(FRAME_LIMIT_FLOOR..8192usize),
        limit_b in proptest::option::of(FRAME_LIMIT_FLOOR..8192usize),
    ) {
        let our_ids: BTreeSet<[u8; 32]> = ours.iter().map(|i| i.id).collect();
        let their_ids: BTreeSet<[u8; 32]> = theirs.iter().map(|i| i.id).collect();
        let (have, need, _) = run_session(&ours, &theirs, limit_a, limit_b);
        let expected_have: BTreeSet<[u8; 32]> = our_ids.difference(&their_ids).copied().collect();
        let expected_need: BTreeSet<[u8; 32]> = their_ids.difference(&our_ids).copied().collect();
        prop_assert_eq!(have, expected_have);
        prop_assert_eq!(need, expected_need);
    }

    #[test]
    fn equal_sets_finish_in_one_round_trip(set in set_strategy(300)) {
        let (have, need, rounds) = run_session(&set, &set, None, None);
        prop_assert!(have.is_empty() && need.is_empty());
        prop_assert_eq!(rounds, 1);
    }

    #[test]
    fn arbitrary_bytes_never_panic(
        bytes in proptest::collection::vec(any::<u8>(), 0..512),
        set in set_strategy(64),
        as_initiator in any::<bool>(),
    ) {
        let mut ne = Negentropy::new(SealedVec::new(set), None).unwrap();
        if as_initiator {
            ne.initiate().unwrap();
        }
        // Ok or Err are both acceptable; only a panic is a failure.
        let _ = ne.reconcile(&bytes);
    }

    #[test]
    fn mutated_valid_messages_never_panic(
        ours in set_strategy(64),
        theirs in set_strategy(64),
        flips in proptest::collection::vec((0usize..4096, 0u8..8), 1..8),
    ) {
        let mut a = Negentropy::new(SealedVec::new(ours), None).unwrap();
        let mut b = Negentropy::new(SealedVec::new(theirs), None).unwrap();
        let mut msg = a.initiate().unwrap();
        for (pos, bit) in flips {
            if msg.is_empty() {
                break;
            }
            let pos = pos % msg.len();
            msg[pos] ^= 1 << bit;
            match b.reconcile(&msg) {
                Ok(round) => match round.reply {
                    Some(next) => msg = next,
                    None => break,
                },
                Err(_) => break,
            }
        }
    }
}
