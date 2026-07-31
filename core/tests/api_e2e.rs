//! End-to-end tests of the FFI facade: two [`racnet_core::api::Node`]s
//! driven exactly as two transport shims would drive them, over an
//! in-memory byte pipe with adversarially odd segmentation.

use std::sync::Arc;

use racnet_core::api::{
    generate_identity, ApiError, CloseCause, Event, Identity, Node, SyncWindow,
};

const HOUR_US: u64 = 3_600_000_000;

fn identity(byte: u8) -> Identity {
    Identity {
        noise_seed: vec![byte; 32],
        signing_seed: vec![byte.wrapping_add(0x40); 32],
    }
}

fn full_window() -> SyncWindow {
    SyncWindow {
        since_ms: 0,
        until_ms: u64::MAX,
    }
}

/// One side of a connected pair: a node plus the link it runs.
struct Shim {
    node: Arc<Node>,
    link_id: u64,
    events: Vec<Event>,
    /// Bytes queued toward the peer.
    outbox: Vec<u8>,
}

impl Shim {
    fn record(&mut self, frames: Vec<Vec<u8>>, events: Vec<Event>) {
        for frame in frames {
            self.outbox.extend_from_slice(&frame);
        }
        self.events.extend(events);
    }

    fn closed(&self) -> Option<&CloseCause> {
        self.events.iter().find_map(|event| match event {
            Event::Closed { cause, .. } => Some(cause),
            _ => None,
        })
    }

    fn established(&self) -> bool {
        self.node
            .link_status(self.link_id)
            .is_some_and(|status| status.established)
    }
}

/// Connects two nodes into a linked pair (a = initiator) without pumping.
fn pair(a: &Arc<Node>, b: &Arc<Node>, now_us: u64) -> (Shim, Shim) {
    let open_a = a.connect(now_us).expect("connect");
    let open_b = b
        .accept(b"addr-of-a".to_vec(), now_us)
        .expect("accept")
        .expect("admitted");
    let mut shim_a = Shim {
        node: a.clone(),
        link_id: open_a.link_id,
        events: Vec::new(),
        outbox: Vec::new(),
    };
    let shim_b = Shim {
        node: b.clone(),
        link_id: open_b.link_id,
        events: Vec::new(),
        outbox: Vec::new(),
    };
    shim_a.record(open_a.initial_frames, Vec::new());
    assert!(open_b.initial_frames.is_empty(), "responder waits");
    (shim_a, shim_b)
}

/// Delivers both outboxes until quiescence, feeding bytes in chunks of
/// rotating odd sizes so the facade sees every segmentation the radio
/// could produce.
fn pump(a: &mut Shim, b: &mut Shim, now_us: u64) {
    let chunk_sizes = [1usize, 3, 17, 64, 1024];
    let mut turn = 0usize;
    loop {
        let (from, to) = if turn.is_multiple_of(2) {
            (&mut *a, &mut *b)
        } else {
            (&mut *b, &mut *a)
        };
        turn += 1;
        if from.outbox.is_empty() {
            if a.outbox.is_empty() && b.outbox.is_empty() {
                return;
            }
            continue;
        }
        let bytes = std::mem::take(&mut from.outbox);
        for (i, chunk) in bytes
            .chunks(chunk_sizes[turn % chunk_sizes.len()])
            .enumerate()
        {
            // Vary the timestamp a little, including backwards, to mimic
            // racing reader threads.
            let jitter = if i.is_multiple_of(2) {
                now_us
            } else {
                now_us.saturating_sub(1)
            };
            let io = to.node.on_bytes(to.link_id, chunk.to_vec(), jitter);
            to.record(io.frames, io.events);
        }
    }
}

fn establish(a: &Arc<Node>, b: &Arc<Node>, now_us: u64) -> (Shim, Shim) {
    let (mut shim_a, mut shim_b) = pair(a, b, now_us);
    pump(&mut shim_a, &mut shim_b, now_us);
    assert!(shim_a.established() && shim_b.established());
    (shim_a, shim_b)
}

#[test]
fn establishment_survives_odd_segmentation_and_cross_reports_fingerprints() {
    let a = Node::new(identity(0x11)).unwrap();
    let b = Node::new(identity(0x22)).unwrap();
    let (shim_a, shim_b) = establish(&a, &b, 0);

    let remote_of_a = shim_a.events.iter().find_map(|event| match event {
        Event::Established {
            remote_fingerprint, ..
        } => Some(remote_fingerprint.clone()),
        _ => None,
    });
    let remote_of_b = shim_b.events.iter().find_map(|event| match event {
        Event::Established {
            remote_fingerprint, ..
        } => Some(remote_fingerprint.clone()),
        _ => None,
    });
    assert_eq!(remote_of_a, Some(b.fingerprint()));
    assert_eq!(remote_of_b, Some(a.fingerprint()));
}

#[test]
fn generated_identities_establish_too() {
    let a = Node::new(generate_identity().unwrap()).unwrap();
    let b = Node::new(generate_identity().unwrap()).unwrap();
    establish(&a, &b, 0);
}

#[test]
fn bidirectional_sync_converges_the_stores() {
    let a = Node::new(identity(0x11)).unwrap();
    let b = Node::new(identity(0x22)).unwrap();
    for i in 0..5u64 {
        a.create_entry(0, format!("from-a-{i}").into_bytes(), 1_000 + i)
            .unwrap();
        b.create_entry(0, format!("from-b-{i}").into_bytes(), 2_000 + i)
            .unwrap();
    }
    let (mut shim_a, mut shim_b) = establish(&a, &b, 0);

    let start_a = a.start_sync(shim_a.link_id, full_window()).unwrap();
    shim_a.record(start_a.io.frames, start_a.io.events);
    let start_b = b.start_sync(shim_b.link_id, full_window()).unwrap();
    shim_b.record(start_b.io.frames, start_b.io.events);
    // §6.2: initiator sessions are even, responder sessions odd.
    assert_eq!(start_a.session_id % 2, 0);
    assert_eq!(start_b.session_id % 2, 1);
    pump(&mut shim_a, &mut shim_b, 10);

    assert_eq!(a.entry_count(), 10);
    assert_eq!(b.entry_count(), 10);
    let list_a = a.entries(full_window());
    let list_b = b.entries(full_window());
    assert_eq!(
        list_a.iter().map(|e| e.id.clone()).collect::<Vec<_>>(),
        list_b.iter().map(|e| e.id.clone()).collect::<Vec<_>>(),
        "identical §6.1 order on both sides"
    );
    assert!(list_a
        .windows(2)
        .all(|w| (w[0].sort_key_ms, &w[0].id) < (w[1].sort_key_ms, &w[1].id)));
    assert!(shim_a
        .events
        .iter()
        .any(|e| matches!(e, Event::EntryStored { .. })));
    assert!(shim_a.closed().is_none() && shim_b.closed().is_none());
}

#[test]
fn limiter_refuses_the_fourth_burst_accept_and_times_out_half_open() {
    let b = Node::new(identity(0x22)).unwrap();
    let addr = b"same-remote".to_vec();
    let mut opened = Vec::new();
    for _ in 0..3 {
        opened.push(b.accept(addr.clone(), 0).unwrap().expect("within burst"));
    }
    assert!(
        b.accept(addr.clone(), 0).unwrap().is_none(),
        "burst of 3 exhausted"
    );

    // Nothing ever arrives on the admitted links: the 30 s half-open
    // timeout must retire them, exactly once each.
    assert!(b.tick(29_000_000).is_empty());
    let mut events = b.tick(30_000_000);
    events.sort_by_key(|event| match event {
        Event::Closed { link_id, .. } => *link_id,
        _ => u64::MAX,
    });
    let expected: Vec<Event> = opened
        .iter()
        .map(|open| Event::Closed {
            link_id: open.link_id,
            cause: CloseCause::HandshakeTimeout,
        })
        .collect();
    assert_eq!(events, expected);
    assert!(b.tick(31_000_000).is_empty(), "reported once");
    for open in &opened {
        assert!(b.link_status(open.link_id).is_none());
    }
}

#[test]
fn transport_close_mid_handshake_frees_the_half_open_slot() {
    let b = Node::new(identity(0x22)).unwrap();
    // Fill the global half-open cap from distinct addresses.
    let mut opened = Vec::new();
    for i in 0..32u32 {
        opened.push(
            b.accept(i.to_be_bytes().to_vec(), 0)
                .unwrap()
                .expect("cap not yet reached"),
        );
    }
    assert!(
        b.accept(b"straggler".to_vec(), 0).unwrap().is_none(),
        "33rd is over the cap"
    );

    // The BLE connection of one admitted link dies before establishing.
    let events = b.on_transport_closed(opened[0].link_id, 1);
    assert_eq!(
        events,
        vec![Event::Closed {
            link_id: opened[0].link_id,
            cause: CloseCause::TransportClosed,
        }]
    );
    assert!(
        b.accept(b"straggler".to_vec(), 1).unwrap().is_some(),
        "slot freed"
    );
}

#[test]
fn reconnect_after_transport_close_establishes_under_a_fresh_link_id() {
    let a = Node::new(identity(0x11)).unwrap();
    let b = Node::new(identity(0x22)).unwrap();
    let (shim_a, shim_b) = establish(&a, &b, 0);

    // The radio drops; both sides notice independently.
    assert_eq!(a.on_transport_closed(shim_a.link_id, HOUR_US).len(), 1);
    assert_eq!(b.on_transport_closed(shim_b.link_id, HOUR_US).len(), 1);
    // Stale calls with the dead ids are tolerated.
    assert!(a
        .on_bytes(shim_a.link_id, vec![0xaa; 300], HOUR_US)
        .events
        .is_empty());

    // Reconnect: fresh link ids, full re-handshake (spec 0.1.2 has no
    // resumption), same identities.
    let (shim_a2, shim_b2) = establish(&a, &b, HOUR_US);
    assert_ne!(shim_a2.link_id, shim_a.link_id);
    assert_ne!(shim_b2.link_id, shim_b.link_id);
}

#[test]
fn lifetime_cap_closes_after_24_hours_with_no_frames() {
    let a = Node::new(identity(0x11)).unwrap();
    let b = Node::new(identity(0x22)).unwrap();
    let (shim_a, _shim_b) = establish(&a, &b, 0);

    assert!(a.tick(23 * HOUR_US).is_empty());
    let events = a.tick(24 * HOUR_US);
    assert_eq!(
        events,
        vec![Event::Closed {
            link_id: shim_a.link_id,
            cause: CloseCause::LifetimeExpired,
        }]
    );
    assert!(a.link_status(shim_a.link_id).is_none());
}

#[test]
fn start_sync_needs_an_established_link() {
    let a = Node::new(identity(0x11)).unwrap();
    let open = a.connect(0).unwrap();
    assert!(matches!(
        a.start_sync(open.link_id, full_window()),
        Err(ApiError::NotEstablished)
    ));
}

#[test]
fn garbage_bytes_on_a_live_link_close_it_silently() {
    let a = Node::new(identity(0x11)).unwrap();
    let b = Node::new(identity(0x22)).unwrap();
    let (mut shim_a, mut shim_b) = establish(&a, &b, 0);

    // Corrupt one transport frame from a to b.
    let push = a
        .create_entry(0, b"tamper-target".to_vec(), 5_000)
        .and_then(|_| a.start_sync(shim_a.link_id, full_window()))
        .unwrap();
    shim_a.record(push.io.frames, push.io.events);
    let last = shim_a.outbox.len() - 1;
    shim_a.outbox[last] ^= 0x01;
    pump(&mut shim_a, &mut shim_b, 20);

    assert_eq!(shim_b.closed(), Some(&CloseCause::DecryptFailed));
    assert!(
        shim_b.outbox.is_empty(),
        "silent close: no bytes toward the peer"
    );
}
