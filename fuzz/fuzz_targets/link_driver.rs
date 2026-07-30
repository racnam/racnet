//! Fuzzes the link driver's full state machine (ADR-0013).
//!
//! The first input byte selects a role and how far along the honest
//! transcript the driver is (fresh, mid-handshake, established) before
//! the remaining bytes arrive as stream input. Fixed keys make the
//! honest prefix deterministic. Asserts nothing panics and that a
//! closed driver stays inert.

#![no_main]

use libfuzzer_sys::fuzz_target;
use racnet_core::link::{LinkDriver, LinkDriverConfig};
use racnet_core::noise::Keypair;
use racnet_core::store::EntryStore;
use racnet_core::sync::LinkRole;

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

/// The frames each side receives during an honest establishment.
fn honest_inboxes() -> (Vec<Vec<u8>>, Vec<Vec<u8>>) {
    let mut a = driver(LinkRole::Initiator);
    let mut b = driver(LinkRole::Responder);
    let mut a_store = EntryStore::new();
    let mut b_store = EntryStore::new();
    let mut to_b = a.start().frames;
    let mut a_inbox = Vec::new();
    let mut b_inbox = Vec::new();
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
    (a_inbox, b_inbox)
}

fuzz_target!(|data: &[u8]| {
    let Some((&selector, bytes)) = data.split_first() else {
        return;
    };
    let role = if selector & 1 == 0 {
        LinkRole::Initiator
    } else {
        LinkRole::Responder
    };
    let prefix = usize::from(selector >> 1) % 4;

    let (a_inbox, b_inbox) = honest_inboxes();
    let inbox = match role {
        LinkRole::Initiator => a_inbox,
        LinkRole::Responder => b_inbox,
    };

    let mut d = driver(role);
    let mut store = EntryStore::new();
    let _ = d.start();
    for frame in inbox.iter().take(prefix) {
        let _ = d.on_bytes(&mut store, frame, 0);
    }

    let chunk = usize::from(selector).max(1);
    for piece in bytes.chunks(chunk) {
        let _ = d.on_bytes(&mut store, piece, 0);
    }
    if d.close_reason().is_some() {
        let out = d.on_bytes(&mut store, b"closed drivers stay closed", 0);
        assert!(out.frames.is_empty() && out.events.is_empty());
    }
});
