//! Live interop between the in-house Noise engine and `snow` (dev-only
//! oracle, ADR-0012): both roles, transport traffic both ways, and the
//! §4.3 rekey boundary.

use racnet_core::noise::{HandshakeState, Keypair, Role, REKEY_INTERVAL};

const PATTERN: &str = "Noise_XX_25519_ChaChaPoly_SHA256";
const PROLOGUE: &[u8] = b"interop prologue";

fn snow_builder(secret: &[u8; 32]) -> snow::Builder<'_> {
    snow::Builder::new(PATTERN.parse().unwrap())
        .prologue(PROLOGUE)
        .unwrap()
        .local_private_key(secret)
        .unwrap()
}

#[test]
fn our_initiator_completes_against_snow_responder() {
    let ours = HandshakeState::new_xx(
        Role::Initiator,
        Keypair::from_secret_bytes([0x51; 32]),
        Keypair::from_secret_bytes([0x52; 32]),
        PROLOGUE,
    );
    let mut ours = ours;
    let mut theirs = snow_builder(&[0x61; 32]).build_responder().unwrap();
    let mut buf = [0u8; 1024];

    let m1 = ours.write_message(&[]).unwrap();
    theirs.read_message(&m1, &mut buf).unwrap();
    let n = theirs.write_message(&[], &mut buf).unwrap();
    ours.read_message(&buf[..n]).unwrap();
    let m3 = ours.write_message(&[]).unwrap();
    theirs.read_message(&m3, &mut [0u8; 1024]).unwrap();

    assert_eq!(
        ours.handshake_hash().as_slice(),
        theirs.get_handshake_hash()
    );
    let (mut ot, snow_static) = ours.into_transport().unwrap();
    assert_eq!(
        snow_static.0.as_slice(),
        Keypair::from_secret_bytes([0x61; 32]).public.0
    );
    let mut tt = theirs.into_transport_mode().unwrap();

    // Traffic both ways.
    for i in 0..10u64 {
        let msg = i.to_le_bytes();
        let ct = ot.encrypt(&msg).unwrap();
        let n = tt.read_message(&ct, &mut buf).unwrap();
        assert_eq!(&buf[..n], msg);
        let n = tt.write_message(&msg, &mut buf).unwrap();
        assert_eq!(ot.decrypt(&buf[..n]).unwrap(), msg);
    }
}

#[test]
fn our_responder_completes_against_snow_initiator() {
    let mut ours = HandshakeState::new_xx(
        Role::Responder,
        Keypair::from_secret_bytes([0x71; 32]),
        Keypair::from_secret_bytes([0x72; 32]),
        PROLOGUE,
    );
    let mut theirs = snow_builder(&[0x81; 32]).build_initiator().unwrap();
    let mut buf = [0u8; 1024];

    let n = theirs.write_message(&[], &mut buf).unwrap();
    ours.read_message(&buf[..n]).unwrap();
    let m2 = ours.write_message(&[]).unwrap();
    theirs.read_message(&m2, &mut [0u8; 1024]).unwrap();
    let n = theirs.write_message(&[], &mut buf).unwrap();
    ours.read_message(&buf[..n]).unwrap();

    assert_eq!(
        ours.handshake_hash().as_slice(),
        theirs.get_handshake_hash()
    );
    let (mut ot, snow_static) = ours.into_transport().unwrap();
    assert_eq!(
        snow_static.0.as_slice(),
        Keypair::from_secret_bytes([0x81; 32]).public.0
    );
    let mut tt = theirs.into_transport_mode().unwrap();

    for i in 0..10u64 {
        let msg = i.to_le_bytes();
        let n = tt.write_message(&msg, &mut buf).unwrap();
        assert_eq!(ot.decrypt(&buf[..n]).unwrap(), msg);
        let ct = ot.encrypt(&msg).unwrap();
        let n = tt.read_message(&ct, &mut buf).unwrap();
        assert_eq!(&buf[..n], msg);
    }
}

/// Crosses the 1024 and 2048 rekey boundaries in both directions, calling
/// snow's rekey functions at exactly the counts where our TransportState
/// rekeys itself. If snow's Rekey semantics (spec REKEY, nonce not reset)
/// ever diverged from ours, every message after 1024 would fail.
#[test]
fn rekey_interops_with_snow_at_the_1024_boundary() {
    let mut ours = HandshakeState::new_xx(
        Role::Initiator,
        Keypair::from_secret_bytes([0x91; 32]),
        Keypair::from_secret_bytes([0x92; 32]),
        PROLOGUE,
    );
    let mut theirs = snow_builder(&[0xa1; 32]).build_responder().unwrap();
    let mut buf = [0u8; 1024];

    let m1 = ours.write_message(&[]).unwrap();
    theirs.read_message(&m1, &mut buf).unwrap();
    let n = theirs.write_message(&[], &mut buf).unwrap();
    ours.read_message(&buf[..n]).unwrap();
    let m3 = ours.write_message(&[]).unwrap();
    theirs.read_message(&m3, &mut [0u8; 1024]).unwrap();

    let (mut ot, _) = ours.into_transport().unwrap();
    let mut tt = theirs.into_transport_mode().unwrap();

    let total = 2 * REKEY_INTERVAL + 3;
    for i in 0..total {
        // snow does not rekey on its own; mirror our schedule on its side.
        if i > 0 && i % REKEY_INTERVAL == 0 {
            tt.rekey_incoming();
            tt.rekey_outgoing();
        }
        let msg = i.to_le_bytes();
        let ct = ot.encrypt(&msg).unwrap();
        let n = tt
            .read_message(&ct, &mut buf)
            .unwrap_or_else(|e| panic!("snow failed to decrypt message {i}: {e:?}"));
        assert_eq!(&buf[..n], msg, "message {i} ours→snow");
        let n = tt.write_message(&msg, &mut buf).unwrap();
        assert_eq!(
            ot.decrypt(&buf[..n])
                .unwrap_or_else(|e| panic!("we failed to decrypt snow message {i}: {e:?}")),
            msg,
            "message {i} snow→ours"
        );
    }
}
