//! Conformance vectors from PROTOCOL.md §8. Every hex constant below is
//! transcribed from the spec; the implementation must reproduce each byte
//! sequence exactly, and decode each back to the same message.

use ed25519_dalek::SigningKey;
use racnet_core::wire::{
    decode_message, encode_frame, Entry, ErrorMsg, FrameDecoder, GossipPush, Handshake, Hello,
    Message, ReconMsg,
};

/// PROTOCOL.md §8: the fixed test key.
fn test_key() -> SigningKey {
    SigningKey::from_bytes(&[0x01; 32])
}

/// PROTOCOL.md §8.1: the sample entry.
fn sample_entry() -> Entry {
    Entry::sign(&test_key(), 1_700_000_000_000, 0, b"hello".to_vec())
}

fn assert_frame_matches(msg: &Message, spec_hex: &str) {
    let expected = hex::decode(spec_hex).expect("valid hex in test");
    let frame = encode_frame(msg).expect("vector messages encode");
    assert_eq!(hex::encode(&frame), hex::encode(&expected));

    // The same bytes must decode back to the same message, including via
    // arbitrary stream segmentation.
    assert_eq!(&decode_message(&expected[2..]).unwrap(), msg);
    let mut dec = FrameDecoder::new();
    for chunk in expected.chunks(3) {
        dec.push(chunk);
    }
    assert_eq!(dec.next_message().unwrap().as_ref(), Some(msg));
}

#[test]
fn test_key_matches_spec() {
    assert_eq!(
        hex::encode(test_key().verifying_key().as_bytes()),
        "8a88e3dd7409f195fd52db2d3cba5d72ca6709bf1d94121bf3748801b40f6f5c"
    );
}

#[test]
fn entry_tbs_matches_spec() {
    assert_eq!(
        hex::encode(sample_entry().tbs_bytes()),
        "8458208a88e3dd7409f195fd52db2d3cba5d72ca6709bf1d94121bf3748801b4\
         0f6f5c1b0000018bcfe56800004568656c6c6f"
    );
}

#[test]
fn entry_signature_matches_spec() {
    assert_eq!(
        hex::encode(sample_entry().sig),
        "81cd254612a44bc4fced2ab12fd17a8ae5f29748da91ee7cf428fa5d43fab4a5\
         75b8eff21223cfe54215c76a05440b2b4c7815ede94d0f328a6214a928a90008"
    );
}

#[test]
fn entry_encoding_matches_spec() {
    let entry = sample_entry();
    entry.verify().expect("spec entry verifies");
    assert_eq!(
        hex::encode(entry.to_bytes()),
        "8558208a88e3dd7409f195fd52db2d3cba5d72ca6709bf1d94121bf3748801b4\
         0f6f5c1b0000018bcfe56800004568656c6c6f584081cd254612a44bc4fced2a\
         b12fd17a8ae5f29748da91ee7cf428fa5d43fab4a575b8eff21223cfe54215c7\
         6a05440b2b4c7815ede94d0f328a6214a928a90008"
    );
}

#[test]
fn entry_id_matches_spec() {
    assert_eq!(
        hex::encode(sample_entry().id()),
        "9785dd7e1d47c2c71c38988f98d46f4674dbf8dd7ed738284569ab6df2b08e8d"
    );
}

#[test]
fn hello_frame_matches_spec() {
    let msg = Message::Hello(Hello {
        versions: vec![1],
        features: 0,
        transports: vec![1],
    });
    let mut spec_hex = String::from("0100010009a30081010100028101");
    spec_hex.push_str(&"00".repeat(256 - 12));
    assert_frame_matches(&msg, &spec_hex);
}

#[test]
fn gossip_push_frame_matches_spec() {
    let msg = Message::GossipPush(GossipPush {
        entries: vec![sample_entry()],
        ttl: 4,
    });
    let mut spec_hex = String::from(
        "010010007aa200818558208a88e3dd7409f195fd52db2d3cba5d72ca6709bf1d\
         94121bf3748801b40f6f5c1b0000018bcfe56800004568656c6c6f584081cd25\
         4612a44bc4fced2ab12fd17a8ae5f29748da91ee7cf428fa5d43fab4a575b8ef\
         f21223cfe54215c76a05440b2b4c7815ede94d0f328a6214a928a900080104",
    );
    spec_hex.push_str(&"00".repeat(256 - 125));
    assert_frame_matches(&msg, &spec_hex);
}

#[test]
fn error_frame_matches_spec() {
    let mut spec_hex = String::from("0100700003a10002");
    spec_hex.push_str(&"00".repeat(256 - 6));
    assert_frame_matches(&Message::Error(ErrorMsg { code: 2 }), &spec_hex);
}

#[test]
fn recon_msg_frame_matches_spec() {
    let msg = Message::ReconMsg(ReconMsg {
        sid: 1,
        msg: vec![0x00, 0x01, 0x02, 0x03],
    });
    let mut spec_hex = String::from("0100210009a2000101440001020300");
    spec_hex.push_str(&"00".repeat(256 - 13));
    assert_frame_matches(&msg, &spec_hex);
}

#[test]
fn padding_boundary_matches_spec() {
    let mut spec_hex = String::from("01000200fda10058f9");
    spec_hex.push_str(&"aa".repeat(249));
    assert_frame_matches(
        &Message::Handshake(Handshake {
            msg: vec![0xaa; 249],
        }),
        &spec_hex,
    );

    let over = encode_frame(&Message::Handshake(Handshake {
        msg: vec![0xaa; 250],
    }))
    .unwrap();
    assert_eq!(over.len(), 514);
}
