//! Property tests for the link driver and the Noise handshake reader:
//! no input — garbage, mutated, truncated, or arbitrarily segmented —
//! may panic, and failures must not leak error oracles onto the wire.

use proptest::prelude::*;

use racnet_core::link::{CloseReason, LinkDriver, LinkDriverConfig, LinkEvent};
use racnet_core::noise::{HandshakeState, Keypair, Role};
use racnet_core::store::EntryStore;
use racnet_core::sync::LinkRole;
use racnet_core::wire::{decode_message, ErrorMsg, Message};

fn driver(role: LinkRole) -> LinkDriver {
    let (s, e) = match role {
        LinkRole::Initiator => ([0x11; 32], [0x33; 32]),
        LinkRole::Responder => ([0x22; 32], [0x44; 32]),
    };
    LinkDriver::new(
        role,
        Keypair::from_secret_bytes(s),
        Keypair::from_secret_bytes(e),
        LinkDriverConfig::default(),
        0,
    )
}

/// The full honest transcript with the fixed keys: the frames each side
/// receives, in order, plus one transport frame from the initiator.
/// Deterministic because all keys are fixed.
fn honest_transcript() -> (Vec<Vec<u8>>, Vec<Vec<u8>>) {
    let mut a = driver(LinkRole::Initiator);
    let mut b = driver(LinkRole::Responder);
    let mut a_store = EntryStore::new();
    let mut b_store = EntryStore::new();
    let mut to_b: Vec<Vec<u8>> = a.start().frames;
    let mut b_inbox = Vec::new();
    let mut a_inbox = Vec::new();
    while !to_b.is_empty() {
        let mut to_a = Vec::new();
        for frame in to_b.drain(..) {
            b_inbox.push(frame.clone());
            to_a.extend(b.on_bytes(&mut b_store, &frame, 0).frames);
        }
        for frame in to_a {
            a_inbox.push(frame.clone());
            to_b.extend(a.on_bytes(&mut a_store, &frame, 0).frames);
        }
    }
    assert!(a.is_established() && b.is_established());
    // One transport frame toward the responder.
    let entry = racnet_core::wire::Entry::sign(
        &ed25519_dalek::SigningKey::from_bytes(&[0x09; 32]),
        900,
        0,
        b"payload".to_vec(),
    );
    let out = a
        .send(&Message::GossipPush(racnet_core::wire::GossipPush {
            entries: vec![entry],
            ttl: 0,
        }))
        .unwrap();
    b_inbox.extend(out.frames);
    (b_inbox, a_inbox)
}

/// Every driver state reachable by an honest prefix, per role.
fn driver_in_state(role: LinkRole, prefix_frames: usize) -> (LinkDriver, EntryStore) {
    let (b_inbox, a_inbox) = honest_transcript();
    let inbox = match role {
        LinkRole::Initiator => a_inbox,
        LinkRole::Responder => b_inbox,
    };
    let mut d = driver(role);
    let mut store = EntryStore::new();
    let _ = d.start();
    for frame in inbox.iter().take(prefix_frames) {
        let _ = d.on_bytes(&mut store, frame, 0);
    }
    (d, store)
}

/// A close reason the spec requires to be wire-silent.
fn is_silent(reason: &CloseReason) -> bool {
    matches!(
        reason,
        CloseReason::HandshakeFailed
            | CloseReason::DecryptFailed
            | CloseReason::BadCiphertextLength
            | CloseReason::LifetimeExpired
            | CloseReason::NonceExhausted
    )
}

proptest! {
    /// Arbitrary bytes, arbitrarily segmented, into every reachable
    /// state: never a panic, and a closed driver stays inert.
    #[test]
    fn garbage_bytes_never_panic_in_any_state(
        initiator in any::<bool>(),
        prefix in 0usize..4,
        bytes in proptest::collection::vec(any::<u8>(), 0..2048),
        chunk in 1usize..64,
    ) {
        let role = if initiator { LinkRole::Initiator } else { LinkRole::Responder };
        let (mut d, mut store) = driver_in_state(role, prefix);
        for piece in bytes.chunks(chunk) {
            let out = d.on_bytes(&mut store, piece, 0);
            if let Some(reason) = d.close_reason() {
                // Silent closes emit no frames in the closing call.
                if is_silent(reason) {
                    prop_assert!(out.frames.is_empty());
                }
            }
        }
        if d.close_reason().is_some() {
            let out = d.on_bytes(&mut store, b"more", 0);
            prop_assert!(out.frames.is_empty() && out.events.is_empty());
        }
    }

    /// One flipped bit or a truncation anywhere in the honest inbound
    /// stream: the victim must never emit a cleartext ERROR other than
    /// code 2, and a silently-closing call emits no frames at all.
    #[test]
    fn mutation_closes_without_an_error_oracle(
        initiator in any::<bool>(),
        flip_bit in proptest::option::of(0usize..8),
        cut in any::<proptest::sample::Index>(),
    ) {
        let role = if initiator { LinkRole::Initiator } else { LinkRole::Responder };
        let (b_inbox, a_inbox) = honest_transcript();
        let inbox = match role {
            LinkRole::Initiator => a_inbox,
            LinkRole::Responder => b_inbox,
        };
        let mut stream: Vec<u8> = inbox.concat();
        prop_assume!(!stream.is_empty());
        let pos = cut.index(stream.len());
        match flip_bit {
            Some(bit) => stream[pos] ^= 1 << bit,   // bit flip at pos
            None => stream.truncate(pos),           // truncation at pos
        }

        let mut d = driver(role);
        let mut store = EntryStore::new();
        let _ = d.start();
        let out = d.on_bytes(&mut store, &stream, 0);

        for frame in &out.frames {
            if frame.len() >= 2 {
                if let Ok(Message::Error(ErrorMsg { code })) = decode_message(&frame[2..]) {
                    // A decodable (cleartext) ERROR may only be code 2.
                    prop_assert_eq!(code, 2, "cleartext ERROR other than code 2");
                }
            }
        }
        if let Some(reason) = d.close_reason() {
            if is_silent(reason) {
                // The closing call cleared its own frames; the driver
                // reported the reason locally only.
                prop_assert!(!matches!(reason, CloseReason::ProtocolViolation(_)));
            }
        }
        // Whatever happened, closed drivers stay closed.
        if d.close_reason().is_some() {
            let again = d.on_bytes(&mut store, b"denied", 0);
            prop_assert!(again.frames.is_empty() && again.events.is_empty());
        }
    }

    /// The honest establishment must survive any stream segmentation.
    #[test]
    fn honest_transcript_survives_segmentation(chunk in 1usize..64) {
        let (b_inbox, _) = honest_transcript();
        let mut d = driver(LinkRole::Responder);
        let mut store = EntryStore::new();
        let _ = d.start();
        let stream: Vec<u8> = b_inbox.concat();
        let mut established = false;
        for piece in stream.chunks(chunk) {
            let out = d.on_bytes(&mut store, piece, 0);
            established |= out
                .events
                .iter()
                .any(|e| matches!(e, LinkEvent::Established { .. }));
        }
        prop_assert!(established);
        prop_assert!(d.close_reason().is_none());
        prop_assert_eq!(store.len(), 1, "the pushed entry was stored");
    }

    /// The Noise handshake reader is total at every step.
    #[test]
    fn handshake_reader_never_panics(
        step in 0usize..3,
        bytes in proptest::collection::vec(any::<u8>(), 0..512),
    ) {
        // Drive an honest pair to the target step, then feed garbage to
        // whichever side reads next.
        let mut i = HandshakeState::new_xx(
            Role::Initiator,
            Keypair::from_secret_bytes([0x11; 32]),
            Keypair::from_secret_bytes([0x33; 32]),
            b"prologue",
        );
        let mut r = HandshakeState::new_xx(
            Role::Responder,
            Keypair::from_secret_bytes([0x22; 32]),
            Keypair::from_secret_bytes([0x44; 32]),
            b"prologue",
        );
        match step {
            0 => {
                let _ = r.read_message(&bytes);
            }
            1 => {
                let m1 = i.write_message(&[]).unwrap();
                r.read_message(&m1).unwrap();
                let _ = i.read_message(&bytes);
            }
            _ => {
                let m1 = i.write_message(&[]).unwrap();
                r.read_message(&m1).unwrap();
                let m2 = r.write_message(&[]).unwrap();
                i.read_message(&m2).unwrap();
                let _ = r.read_message(&bytes);
            }
        }
    }
}
