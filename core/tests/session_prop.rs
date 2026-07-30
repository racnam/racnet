//! Property tests for the sync session layer: concurrent sessions with
//! randomly interleaved delivery always converge, and arbitrary sync
//! messages — garbage sids included — never panic the handler.

use ed25519_dalek::SigningKey;
use proptest::prelude::*;

use racnet_core::store::EntryStore;
use racnet_core::sync::{LinkRole, SyncConfig, SyncError, Syncer};
use racnet_core::wire::{
    Entry, GossipPush, Message, ReconDone, ReconInit, ReconMsg, SortKeyWindow,
};

fn entry(seed: u8, sort_key: u64) -> Entry {
    Entry::sign(
        &SigningKey::from_bytes(&[seed; 32]),
        sort_key,
        0,
        vec![seed],
    )
}

fn full_window() -> SortKeyWindow {
    SortKeyWindow {
        since: 0,
        until: u64::MAX,
    }
}

fn store_with(seeds: &[u8]) -> EntryStore {
    let mut store = EntryStore::new();
    for &seed in seeds {
        store
            .insert(entry(seed, u64::from(seed) * 50))
            .expect("valid entry");
    }
    store
}

proptest! {
    /// Several sessions per direction, message delivery order randomized
    /// across the two pending queues: stores still converge exactly and
    /// every session closes.
    #[test]
    fn interleaved_concurrent_sessions_converge(
        a_seeds in proptest::collection::btree_set(0u8..60, 0..25),
        b_seeds in proptest::collection::btree_set(30u8..90, 0..25),
        sessions_per_side in 1usize..3,
        schedule in proptest::collection::vec(any::<prop::sample::Index>(), 0..600),
    ) {
        let a_seeds: Vec<u8> = a_seeds.into_iter().collect();
        let b_seeds: Vec<u8> = b_seeds.into_iter().collect();
        let mut a_store = store_with(&a_seeds);
        let mut b_store = store_with(&b_seeds);
        let mut a = Syncer::new(LinkRole::Initiator, SyncConfig::default());
        let mut b = Syncer::new(LinkRole::Responder, SyncConfig::default());

        // Queues of undelivered messages, one per direction.
        let mut to_b: Vec<Message> = Vec::new();
        let mut to_a: Vec<Message> = Vec::new();
        for _ in 0..sessions_per_side {
            to_b.push(a.open_session(&a_store, full_window()).unwrap().1);
            to_a.push(b.open_session(&b_store, full_window()).unwrap().1);
        }

        // Deliver in an order picked by the random schedule (falling back
        // to round-robin once the schedule runs dry). Any interleaving
        // across sessions is legal; within a session causality holds
        // because a reply only exists once its trigger was handled.
        let mut schedule = schedule.into_iter();
        for _ in 0..10_000 {
            if to_a.is_empty() && to_b.is_empty() {
                break;
            }
            let toward_b = if to_a.is_empty() {
                true
            } else if to_b.is_empty() {
                false
            } else {
                schedule.next().map(|ix| ix.index(2) == 0).unwrap_or(true)
            };
            let (queue, syncer, store) = if toward_b {
                (&mut to_b, &mut b, &mut b_store)
            } else {
                (&mut to_a, &mut a, &mut a_store)
            };
            let pick = schedule
                .next()
                .map(|ix| ix.index(queue.len()))
                .unwrap_or(0);
            let msg = queue.remove(pick);
            let output = syncer.handle_message(store, &msg).expect("honest peers");
            if toward_b {
                to_a.extend(output.replies);
            } else {
                to_b.extend(output.replies);
            }
        }

        prop_assert!(to_a.is_empty() && to_b.is_empty(), "queues never drained");
        prop_assert_eq!(a.open_sessions(), 0);
        prop_assert_eq!(b.open_sessions(), 0);
        let expected: std::collections::BTreeSet<u8> =
            a_seeds.iter().chain(b_seeds.iter()).copied().collect();
        prop_assert_eq!(a_store.len(), expected.len());
        prop_assert_eq!(b_store.len(), expected.len());
        for &seed in &expected {
            let id = entry(seed, u64::from(seed) * 50).id();
            prop_assert!(a_store.contains(&id) && b_store.contains(&id));
        }
    }

    /// Random sync messages against a syncer with open state: garbage
    /// sids, arbitrary negentropy bytes, arbitrary windows. Errors are
    /// fine; panics are not, and handler errors must leave no half-open
    /// session behind.
    #[test]
    fn arbitrary_sync_messages_never_panic(
        as_initiator in any::<bool>(),
        msgs in proptest::collection::vec(
            (0u8..4, any::<u64>(), proptest::collection::vec(any::<u8>(), 0..64), any::<u64>(), any::<u64>()),
            0..20,
        ),
    ) {
        let role = if as_initiator { LinkRole::Initiator } else { LinkRole::Responder };
        let mut store = store_with(&[1, 2, 3]);
        let mut syncer = Syncer::new(role, SyncConfig::default());
        syncer.open_session(&store, full_window()).unwrap();

        for (variant, sid, bytes, since, until) in msgs {
            let msg = match variant {
                0 => Message::ReconInit(ReconInit {
                    sid,
                    window: SortKeyWindow { since, until },
                    msg: bytes,
                }),
                1 => Message::ReconMsg(ReconMsg { sid, msg: bytes }),
                2 => Message::ReconDone(ReconDone { sid }),
                _ => Message::GossipPush(GossipPush {
                    entries: vec![entry((sid % 200) as u8, since % 1_000_000)],
                    ttl: until,
                }),
            };
            match syncer.handle_message(&mut store, &msg) {
                Ok(_) => {}
                Err(
                    SyncError::Violation(_)
                    | SyncError::Malformed(_)
                    | SyncError::ResourceLimit
                    | SyncError::UnsupportedVersion(_),
                ) => {}
                Err(other) => prop_assert!(false, "unexpected error class: {other:?}"),
            }
        }
    }
}
