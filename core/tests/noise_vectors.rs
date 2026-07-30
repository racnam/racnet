//! Canonical Noise vector for `Noise_XX_25519_ChaChaPoly_SHA256`.
//!
//! Transcribed from the cacophony conformance corpus as shipped in the
//! `snow` 0.10.0 crate (`tests/vectors/cacophony.txt`), the same corpus
//! snow itself is tested against. Messages 0–2 are the XX handshake;
//! messages 3–5 are transport messages, the direction continuing to
//! alternate from the handshake (3 is responder→initiator).

use racnet_core::noise::{HandshakeState, Keypair, Role};

fn key(hex_str: &str) -> Keypair {
    let mut bytes = [0u8; 32];
    hex::decode_to_slice(hex_str, &mut bytes).unwrap();
    Keypair::from_secret_bytes(bytes)
}

const INIT_STATIC: &str = "e61ef9919cde45dd5f82166404bd08e38bceb5dfdfded0a34c8df7ed542214d1";
const INIT_EPHEMERAL: &str = "893e28b9dc6ca8d611ab664754b8ceb7bac5117349a4439a6b0569da977c464a";
const RESP_STATIC: &str = "4a3acbfdb163dec651dfa3194dece676d437029c62a408b4c5ea9114246e4893";
const RESP_EPHEMERAL: &str = "bbdb4cdbd309f1a1f2e1456967fe288cadd6f712d65dc7b7793d5e63da6b375b";
const PROLOGUE: &str = "4a6f686e2047616c74";
const HANDSHAKE_HASH: &str = "c8e5f64e846193be2a834104c2a009868d6c9f3bd3c186299888b488b2f1f58e";

/// (payload, ciphertext) pairs, in message order.
const MESSAGES: [(&str, &str); 6] = [
    (
        "4c756477696720766f6e204d69736573",
        "ca35def5ae56cec33dc2036731ab14896bc4c75dbb07a61f879f8e3afa4c79444c756477696720766f6e204d69736573",
    ),
    (
        "4d757272617920526f746862617264",
        "95ebc60d2b1fa672c1f46a8aa265ef51bfe38e7ccb39ec5be34069f14480884381cbad1f276e038c48378ffce2b65285e08d6b68aaa3629a5a8639392490e5b9bd5269c2f1e4f488ed8831161f19b7815528f8982ffe09be9b5c412f8a0db50f8814c7194e83f23dbd8d162c9326ad",
    ),
    (
        "462e20412e20486179656b",
        "c7195ffacac1307ff99046f219750fc47693e23c3cb08b89c2af808b444850a80ae475b9df0f169ae80a89be0865b57f58c9fea0d4ec82a286427402f113e4b6ae769a1d95941d49b25030",
    ),
    (
        "4361726c204d656e676572",
        "96763ed773f8e47bb3712f0e29b3060ffc956ffc146cee53d5e1df",
    ),
    (
        "4a65616e2d426170746973746520536179",
        "3e40f15f6f3a46ae446b253bf8b1d9ffb6ed9b174d272328ff91a7e2e5c79c07f5",
    ),
    (
        "457567656e2042f6686d20766f6e2042617765726b",
        "eb3f3515110702e047a6c9da4478b6ead94873c11c0f2d710ddb3f09fce024b3a58502ae3f",
    ),
];

fn states() -> (HandshakeState, HandshakeState) {
    let prologue = hex::decode(PROLOGUE).unwrap();
    (
        HandshakeState::new_xx(
            Role::Initiator,
            key(INIT_STATIC),
            key(INIT_EPHEMERAL),
            &prologue,
        ),
        HandshakeState::new_xx(
            Role::Responder,
            key(RESP_STATIC),
            key(RESP_EPHEMERAL),
            &prologue,
        ),
    )
}

#[test]
fn xx_vector_transcript_matches() {
    let (mut init, mut resp) = states();
    for (idx, (payload_hex, ciphertext_hex)) in MESSAGES.iter().take(3).enumerate() {
        let payload = hex::decode(payload_hex).unwrap();
        let expected = hex::decode(ciphertext_hex).unwrap();
        let (writer, reader) = if idx % 2 == 0 {
            (&mut init, &mut resp)
        } else {
            (&mut resp, &mut init)
        };
        let produced = writer.write_message(&payload).unwrap();
        assert_eq!(produced, expected, "handshake message {idx}");
        assert_eq!(reader.read_message(&expected).unwrap(), payload);
    }
    assert!(init.is_finished() && resp.is_finished());
    assert_eq!(
        hex::encode(init.handshake_hash()),
        HANDSHAKE_HASH,
        "handshake hash"
    );
    assert_eq!(init.handshake_hash(), resp.handshake_hash());
}

#[test]
fn xx_vector_transport_messages_match() {
    let (mut init, mut resp) = states();
    for (idx, (payload_hex, _)) in MESSAGES.iter().take(3).enumerate() {
        let payload = hex::decode(payload_hex).unwrap();
        let (writer, reader) = if idx % 2 == 0 {
            (&mut init, &mut resp)
        } else {
            (&mut resp, &mut init)
        };
        let msg = writer.write_message(&payload).unwrap();
        reader.read_message(&msg).unwrap();
    }
    let (mut it, resp_static_seen) = init.into_transport().unwrap();
    let (mut rt, init_static_seen) = resp.into_transport().unwrap();
    assert_eq!(resp_static_seen, key(RESP_STATIC).public);
    assert_eq!(init_static_seen, key(INIT_STATIC).public);

    for (idx, (payload_hex, ciphertext_hex)) in MESSAGES.iter().enumerate().skip(3) {
        let payload = hex::decode(payload_hex).unwrap();
        let expected = hex::decode(ciphertext_hex).unwrap();
        let (sender, receiver) = if idx % 2 == 0 {
            (&mut it, &mut rt)
        } else {
            (&mut rt, &mut it)
        };
        let produced = sender.encrypt(&payload).unwrap();
        assert_eq!(produced, expected, "transport message {idx}");
        assert_eq!(receiver.decrypt(&expected).unwrap(), payload);
    }
}
