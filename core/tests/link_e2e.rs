//! End-to-end encrypted sync over the simulated transport: two link
//! drivers establish a Noise session through the full wire codec on a
//! segmented stream link, reconcile and transfer entries, survive a
//! partition with a full re-handshake, and cross the rekey boundary —
//! deterministically per seed.

#![cfg(feature = "sim")]

use ed25519_dalek::SigningKey;

use racnet_core::link::{CloseReason, LinkDriver, LinkDriverConfig, LinkEvent};
use racnet_core::noise::{Fingerprint, Keypair, REKEY_INTERVAL};
use racnet_core::sim::{Delivery, LinkConfig, LinkKind, NodeId, SimNet};
use racnet_core::store::EntryStore;
use racnet_core::sync::LinkRole;
use racnet_core::wire::{decode_message, Entry, GossipPush, Message, SortKeyWindow};

const STATIC_A: [u8; 32] = [0x11; 32];
const STATIC_B: [u8; 32] = [0x22; 32];

fn entry(seed: u8, sort_key: u64) -> Entry {
    Entry::sign(
        &SigningKey::from_bytes(&[seed; 32]),
        sort_key,
        0,
        vec![seed; 8],
    )
}

fn full_window() -> SortKeyWindow {
    SortKeyWindow {
        since: 0,
        until: u64::MAX,
    }
}

/// One simulated peer: store, link driver, accumulated events.
struct Node {
    id: NodeId,
    peer: NodeId,
    store: EntryStore,
    driver: LinkDriver,
    events: Vec<LinkEvent>,
}

impl Node {
    fn new(
        id: NodeId,
        peer: NodeId,
        role: LinkRole,
        static_secret: [u8; 32],
        ephemeral_secret: [u8; 32],
        now_us: u64,
    ) -> Node {
        Node {
            id,
            peer,
            store: EntryStore::new(),
            driver: LinkDriver::new(
                role,
                Keypair::from_secret_bytes(static_secret),
                Keypair::from_secret_bytes(ephemeral_secret),
                LinkDriverConfig::default(),
                now_us,
            ),
            events: Vec::new(),
        }
    }

    fn send_frames(&self, net: &mut SimNet, frames: &[Vec<u8>]) {
        for frame in frames {
            net.send(self.id, self.peer, frame).expect("link is up");
        }
    }

    /// Drains this node's inbox; returns true if anything was handled.
    fn drain(&mut self, net: &mut SimNet) -> bool {
        let deliveries = net.take_inbox(self.id);
        let handled = !deliveries.is_empty();
        for delivery in deliveries {
            match delivery {
                Delivery::Data { bytes, .. } => {
                    let now = net.now_us();
                    let out = self.driver.on_bytes(&mut self.store, &bytes, now);
                    self.events.extend(out.events.iter().cloned());
                    self.send_frames(net, &out.frames);
                }
                Delivery::LinkDown { .. } => panic!("unexpected link drop"),
            }
        }
        handled
    }
}

/// Runs the network until both nodes are quiet.
fn settle(net: &mut SimNet, a: &mut Node, b: &mut Node) {
    for _ in 0..4096 {
        net.run_until_idle();
        let progressed = a.drain(net) | b.drain(net);
        if !progressed {
            return;
        }
    }
    panic!("network did not settle");
}

/// Establishes the link: initiator HELLO out, settle until both report
/// Established.
fn establish(net: &mut SimNet, a: &mut Node, b: &mut Node) {
    let out = a.driver.start();
    a.send_frames(net, &out.frames);
    let out = b.driver.start();
    b.send_frames(net, &out.frames);
    settle(net, a, b);
    assert!(
        a.driver.is_established() && b.driver.is_established(),
        "handshake completes"
    );
}

/// Opens reconciliation sessions in both directions and settles.
fn sync_both_ways(net: &mut SimNet, a: &mut Node, b: &mut Node) {
    let (_, out) = a.driver.open_sync(&a.store, full_window()).unwrap();
    a.send_frames(net, &out.frames);
    let (_, out) = b.driver.open_sync(&b.store, full_window()).unwrap();
    b.send_frames(net, &out.frames);
    settle(net, a, b);
}

fn store_ids(store: &EntryStore) -> Vec<[u8; 32]> {
    store
        .snapshot(full_window())
        .into_iter()
        .map(|item| item.id)
        .collect()
}

/// Full scenario, returned for determinism comparison.
fn scenario(seed: u64) -> (Vec<[u8; 32]>, Vec<[u8; 32]>, Vec<LinkEvent>) {
    let mut net = SimNet::new(seed);
    let a_id = net.add_node();
    let b_id = net.add_node();
    net.connect(
        a_id,
        b_id,
        LinkConfig {
            latency_us: 500,
            mtu: 23,
            kind: LinkKind::Stream,
            loss_per_mille: 0,
        },
    );

    let mut a = Node::new(a_id, b_id, LinkRole::Initiator, STATIC_A, [0x33; 32], 0);
    let mut b = Node::new(b_id, a_id, LinkRole::Responder, STATIC_B, [0x44; 32], 0);
    establish(&mut net, &mut a, &mut b);

    // Both sides authenticated the right identity.
    let fp_a = Fingerprint::of(&Keypair::from_secret_bytes(STATIC_A).public);
    let fp_b = Fingerprint::of(&Keypair::from_secret_bytes(STATIC_B).public);
    assert_eq!(a.driver.remote_fingerprint(), Some(fp_b));
    assert_eq!(b.driver.remote_fingerprint(), Some(fp_a));

    for i in 0..30u8 {
        a.store.insert(entry(i, u64::from(i) * 100)).unwrap();
    }
    for i in 20..50u8 {
        b.store.insert(entry(i, u64::from(i) * 100)).unwrap();
    }

    sync_both_ways(&mut net, &mut a, &mut b);
    assert_eq!(a.store.len(), 50, "first sync converges");
    assert_eq!(store_ids(&a.store), store_ids(&b.store));

    // Partition: the transport connection is gone. Per spec 0.1.2 there
    // is no session resumption — reconnecting means new drivers, fresh
    // ephemerals, and a full re-handshake.
    net.partition(&[&[a_id], &[b_id]]);
    a.store.insert(entry(100, 10_000)).unwrap();
    a.store.insert(entry(101, 10_100)).unwrap();
    b.store.insert(entry(200, 20_000)).unwrap();
    net.heal();

    let now = net.now_us();
    a.driver = LinkDriver::new(
        LinkRole::Initiator,
        Keypair::from_secret_bytes(STATIC_A),
        Keypair::from_secret_bytes([0x55; 32]),
        LinkDriverConfig::default(),
        now,
    );
    b.driver = LinkDriver::new(
        LinkRole::Responder,
        Keypair::from_secret_bytes(STATIC_B),
        Keypair::from_secret_bytes([0x66; 32]),
        LinkDriverConfig::default(),
        now,
    );
    establish(&mut net, &mut a, &mut b);
    sync_both_ways(&mut net, &mut a, &mut b);

    assert_eq!(a.store.len(), 53, "post-heal sync converges");
    assert_eq!(store_ids(&a.store), store_ids(&b.store));
    assert!(a.store.contains(&entry(200, 20_000).id()));
    assert!(b.store.contains(&entry(100, 10_000).id()));

    (store_ids(&a.store), store_ids(&b.store), a.events)
}

#[test]
fn two_nodes_sync_over_an_encrypted_link_across_a_partition() {
    let (a_ids, b_ids, events) = scenario(7);
    assert_eq!(a_ids.len(), 53);
    assert_eq!(a_ids, b_ids);
    // Two links, two establishments, no closures.
    let established = events
        .iter()
        .filter(|e| matches!(e, LinkEvent::Established { .. }))
        .count();
    assert_eq!(established, 2);
    assert!(!events.iter().any(|e| matches!(e, LinkEvent::Closed(_))));
}

#[test]
fn runs_are_reproducible_from_the_seed() {
    assert_eq!(scenario(7), scenario(7));
}

/// Establishes two drivers back-to-back (no simulator), returning them
/// with their stores.
fn direct_pair() -> (LinkDriver, LinkDriver, EntryStore, EntryStore) {
    let mut a = LinkDriver::new(
        LinkRole::Initiator,
        Keypair::from_secret_bytes(STATIC_A),
        Keypair::from_secret_bytes([0x33; 32]),
        LinkDriverConfig::default(),
        0,
    );
    let mut b = LinkDriver::new(
        LinkRole::Responder,
        Keypair::from_secret_bytes(STATIC_B),
        Keypair::from_secret_bytes([0x44; 32]),
        LinkDriverConfig::default(),
        0,
    );
    let mut a_store = EntryStore::new();
    let mut b_store = EntryStore::new();
    let mut to_b = a.start().frames;
    while !to_b.is_empty() {
        let mut to_a = Vec::new();
        for frame in to_b.drain(..) {
            to_a.extend(b.on_bytes(&mut b_store, &frame, 0).frames);
        }
        for frame in to_a {
            to_b.extend(a.on_bytes(&mut a_store, &frame, 0).frames);
        }
    }
    assert!(a.is_established() && b.is_established());
    (a, b, a_store, b_store)
}

#[test]
fn wire_bytes_are_ciphertext_after_the_handshake() {
    let (mut a, _, _, _) = direct_pair();
    let out = a
        .send(&Message::GossipPush(GossipPush {
            entries: vec![entry(9, 900)],
            ttl: 0,
        }))
        .unwrap();
    let frame = &out.frames[0];
    let body = &frame[2..];
    // §1.1: ciphertext length is a permitted padded length plus the tag,
    // and the body is not a decodable cleartext inner message.
    assert_eq!(body.len(), 256 + 16);
    assert!(decode_message(body).is_err(), "body must be ciphertext");
}

#[test]
fn rekey_boundary_survives_1024_messages_each_way() {
    let (mut a, mut b, mut a_store, mut b_store) = direct_pair();
    let author = SigningKey::from_bytes(&[0x77; 32]);
    let total = REKEY_INTERVAL + 40;
    // a → b crosses the boundary...
    for i in 0..total {
        let push = Message::GossipPush(GossipPush {
            entries: vec![Entry::sign(&author, i + 1, 0, i.to_be_bytes().to_vec())],
            ttl: 0,
        });
        let out = a.send(&push).unwrap();
        assert_eq!(out.frames.len(), 1);
        let reply = b.on_bytes(&mut b_store, &out.frames[0], 0);
        assert!(b.close_reason().is_none(), "message {i} must decrypt");
        assert!(reply.frames.is_empty());
    }
    // ...and b → a crosses it too (replies count toward b's send nonce).
    for i in 0..total {
        let push = Message::GossipPush(GossipPush {
            entries: vec![Entry::sign(
                &author,
                total + i + 1,
                0,
                i.to_le_bytes().to_vec(),
            )],
            ttl: 0,
        });
        let out = b.send(&push).unwrap();
        let _ = a.on_bytes(&mut a_store, &out.frames[0], 0);
        assert!(a.close_reason().is_none(), "message {i} must decrypt");
    }
    assert_eq!(b_store.len(), total as usize);
    assert_eq!(a_store.len(), total as usize);
}

#[test]
fn link_lifetime_expires_silently() {
    let (mut a, _, mut a_store, _) = direct_pair();
    // Just under the cap: still alive.
    let out = a.on_tick(86_400_000_000 - 1);
    assert!(out.events.is_empty());
    // At the cap: silent close, and inert afterwards.
    let out = a.on_tick(86_400_000_000);
    assert!(out.frames.is_empty());
    assert_eq!(
        out.events,
        vec![LinkEvent::Closed(CloseReason::LifetimeExpired)]
    );
    let out = a.on_bytes(&mut a_store, &[0u8; 64], 86_400_000_001);
    assert!(out.frames.is_empty() && out.events.is_empty());
}
