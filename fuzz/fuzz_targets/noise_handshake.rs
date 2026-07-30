//! Fuzzes the Noise handshake reader at every XX step (ADR-0013).
//!
//! Fixed keys keep the honest prefix deterministic; the first input byte
//! selects which step receives the remaining bytes.

#![no_main]

use libfuzzer_sys::fuzz_target;
use racnet_core::noise::{HandshakeState, Keypair, Role};

fn pair() -> (HandshakeState, HandshakeState) {
    (
        HandshakeState::new_xx(
            Role::Initiator,
            Keypair::from_secret_bytes([0x11; 32]),
            Keypair::from_secret_bytes([0x33; 32]),
            b"fuzz prologue",
        ),
        HandshakeState::new_xx(
            Role::Responder,
            Keypair::from_secret_bytes([0x22; 32]),
            Keypair::from_secret_bytes([0x44; 32]),
            b"fuzz prologue",
        ),
    )
}

fuzz_target!(|data: &[u8]| {
    let Some((&step, bytes)) = data.split_first() else {
        return;
    };
    let (mut i, mut r) = pair();
    match step % 3 {
        0 => {
            let _ = r.read_message(bytes);
        }
        1 => {
            let m1 = i.write_message(&[]).unwrap();
            r.read_message(&m1).unwrap();
            let _ = i.read_message(bytes);
        }
        _ => {
            let m1 = i.write_message(&[]).unwrap();
            r.read_message(&m1).unwrap();
            let m2 = r.write_message(&[]).unwrap();
            i.read_message(&m2).unwrap();
            let _ = r.read_message(bytes);
        }
    }
});
