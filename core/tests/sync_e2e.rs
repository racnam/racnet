//! End-to-end sync over the simulated transport: two nodes reconcile and
//! transfer entries through the full wire codec on a segmented stream
//! link, across a partition, deterministically per seed.

use ed25519_dalek::SigningKey;

use racnet_core::sim::{Delivery, LinkConfig, LinkKind, NodeId, SimNet};
use racnet_core::store::EntryStore;
use racnet_core::sync::{LinkRole, SyncConfig, SyncEvent, Syncer};
use racnet_core::wire::{encode_frame, Entry, FrameDecoder, Message, SortKeyWindow};

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

/// One simulated peer: store, syncer, and a frame decoder for its inbox.
struct Node {
    id: NodeId,
    peer: NodeId,
    store: EntryStore,
    syncer: Syncer,
    decoder: FrameDecoder,
    events: Vec<SyncEvent>,
}

impl Node {
    fn new(id: NodeId, peer: NodeId, role: LinkRole) -> Node {
        Node {
            id,
            peer,
            store: EntryStore::new(),
            syncer: Syncer::new(role, SyncConfig::default()),
            decoder: FrameDecoder::new(),
            events: Vec::new(),
        }
    }

    fn send(&self, net: &mut SimNet, messages: &[Message]) {
        for msg in messages {
            let frame = encode_frame(msg).expect("encodable message");
            net.send(self.id, self.peer, &frame).expect("link is up");
        }
    }

    /// Drains this node's inbox; returns true if anything was handled.
    fn drain(&mut self, net: &mut SimNet) -> bool {
        let deliveries = net.take_inbox(self.id);
        let mut handled = !deliveries.is_empty();
        for delivery in deliveries {
            match delivery {
                Delivery::Data { bytes, .. } => self.decoder.push(&bytes),
                Delivery::LinkDown { .. } => panic!("unexpected link drop"),
            }
        }
        let mut replies = Vec::new();
        while let Some(msg) = self.decoder.next_message().expect("well-formed frames") {
            let output = self
                .syncer
                .handle_message(&mut self.store, &msg)
                .expect("no protocol violations between honest peers");
            replies.extend(output.replies);
            self.events.extend(output.events);
            handled = true;
        }
        self.send(net, &replies);
        handled
    }
}

/// Runs the network until both nodes are quiet.
fn settle(net: &mut SimNet, a: &mut Node, b: &mut Node) {
    for _ in 0..1024 {
        net.run_until_idle();
        let progressed = a.drain(net) | b.drain(net);
        if !progressed {
            return;
        }
    }
    panic!("network did not settle");
}

/// Opens sessions in both directions and settles the network.
fn sync_both_ways(net: &mut SimNet, a: &mut Node, b: &mut Node) {
    let (_, a_init) = a
        .syncer
        .open_session(&a.store, full_window())
        .expect("session opens");
    let (_, b_init) = b
        .syncer
        .open_session(&b.store, full_window())
        .expect("session opens");
    a.send(net, &[a_init]);
    b.send(net, &[b_init]);
    settle(net, a, b);
}

fn store_ids(store: &EntryStore) -> Vec<[u8; 32]> {
    store
        .snapshot(full_window())
        .into_iter()
        .map(|item| item.id)
        .collect()
}

/// Full scenario, returned as (a ids, b ids, a events) for determinism
/// comparison.
fn scenario(seed: u64) -> (Vec<[u8; 32]>, Vec<[u8; 32]>, Vec<SyncEvent>) {
    let mut net = SimNet::new(seed);
    let a_id = net.add_node();
    let b_id = net.add_node();
    // BLE-ish stream: tiny MTU segments every frame, nonzero latency.
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

    let mut a = Node::new(a_id, b_id, LinkRole::Initiator);
    let mut b = Node::new(b_id, a_id, LinkRole::Responder);
    for i in 0..30u8 {
        a.store.insert(entry(i, u64::from(i) * 100)).unwrap();
    }
    for i in 20..50u8 {
        b.store.insert(entry(i, u64::from(i) * 100)).unwrap();
    }

    sync_both_ways(&mut net, &mut a, &mut b);
    assert_eq!(a.store.len(), 50, "first sync converges");
    assert_eq!(store_ids(&a.store), store_ids(&b.store));

    // Partition: both sides keep writing while disconnected.
    net.partition(&[&[a_id], &[b_id]]);
    a.store.insert(entry(100, 10_000)).unwrap();
    a.store.insert(entry(101, 10_100)).unwrap();
    b.store.insert(entry(200, 20_000)).unwrap();
    net.heal();

    // Entries written during the partition were invisible to the earlier
    // sessions' snapshots; a fresh session pair picks them up.
    sync_both_ways(&mut net, &mut a, &mut b);
    assert_eq!(a.store.len(), 53, "post-heal sync converges");
    assert_eq!(store_ids(&a.store), store_ids(&b.store));
    assert!(a.store.contains(&entry(200, 20_000).id()));
    assert!(b.store.contains(&entry(100, 10_000).id()));

    assert_eq!(a.syncer.open_sessions(), 0);
    assert_eq!(b.syncer.open_sessions(), 0);

    (store_ids(&a.store), store_ids(&b.store), a.events)
}

#[test]
fn two_nodes_converge_over_a_segmented_stream_across_a_partition() {
    let (a_ids, b_ids, _) = scenario(7);
    assert_eq!(a_ids.len(), 53);
    assert_eq!(a_ids, b_ids);
}

#[test]
fn runs_are_reproducible_from_the_seed() {
    assert_eq!(scenario(7), scenario(7));
}
