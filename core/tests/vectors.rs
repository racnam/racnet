//! Conformance vectors from PROTOCOL.md §8. Every hex constant below is
//! transcribed from the spec; the implementation must reproduce each byte
//! sequence exactly, and decode each back to the same message.

use ed25519_dalek::SigningKey;
use racnet_core::link::{LinkDriver, LinkDriverConfig, LinkEvent};
use racnet_core::noise::{Fingerprint, Keypair};
use racnet_core::store::EntryStore;
use racnet_core::sync::LinkRole;
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

/// PROTOCOL.md §8.7: the full establishment transcript and the first
/// transport message, reproduced by two link drivers from the pinned keys.
/// The spec bytes were generated with an independent Noise implementation
/// (snow 0.10.0), so byte equality here is an interop statement.
#[test]
fn session_establishment_matches_spec() {
    let hello_frame = {
        let mut hex_str = String::from("0100010009a30081010100028101");
        hex_str.push_str(&"00".repeat(256 - 12));
        hex::decode(hex_str).unwrap()
    };
    let handshake_frame_1 = {
        let mut hex_str = String::from(
            "0100020024a10058207b0d47d93427f8311160781c7c733fd89f88970aef490d\
             8aa0ee19a4cb8a1b14",
        );
        hex_str.push_str(&"00".repeat(256 - 39));
        hex::decode(hex_str).unwrap()
    };
    let handshake_frame_2 = {
        let mut hex_str = String::from(
            "0100020064a1005860ff2ee45601ec1b67310c7790404585ae697331eee1c1f8\
             cf2419731c1fff3e6bfcadb15080d9fd0434a18565751d3b6022bec571f33b62\
             12486a1bffa54d1a1ee60dcda08460b009fb2fb84181369eb00b8fe4f251c8de\
             e26310282c86c76148",
        );
        hex_str.push_str(&"00".repeat(256 - 103));
        hex::decode(hex_str).unwrap()
    };
    let handshake_frame_3 = {
        let mut hex_str = String::from(
            "0100020044a1005840a7ea7dd47dddbcfcd736b91b174c6107b2ad26c161965a\
             119a7b644e0c6b3d06c59f012ae05b63213c8ca7ddaaa7104b82a41b2ca5526d\
             bf14f2813b45be6f74",
        );
        hex_str.push_str(&"00".repeat(256 - 71));
        hex::decode(hex_str).unwrap()
    };
    let transport_frame = hex::decode(
        "0110c230a85995f0797437e98182b9337d34ea891016fac9735d90e3881f20ef\
         3167be9b1e558aaf4dd173d8a2baac24322902ec9a5c316ea661cef69f93011d\
         c155a9603db73f89c9e7c4df1ae6a03cbd3ef1408f8d8b9dc41d0242f38970ef\
         3dc4b42598ca2470b0f20607c7c1d4aface579cfad9713b92728516ab17341da\
         7b3d937fa7fb25620b3b2f59d5c2f02b57c282c06ce5841a0613cf2ba6aadcb0\
         14f2a73d10170ede86b04c194b54622fa55092b8b4898a8c674102d550d29692\
         5214166e8be9975e8f941bb966f0352218e5e34d7e2fbf905cff3e66813fc640\
         51ff63a2357517de3b4a2edfcdc68ec3c2ddd4e30c78f101f3eb6c770f54178c\
         fa8dcbe1db19729bcd030830f6fb3b978b23",
    )
    .unwrap();

    let mut initiator = LinkDriver::new(
        LinkRole::Initiator,
        Keypair::from_secret_bytes([0x11; 32]),
        Keypair::from_secret_bytes([0x33; 32]),
        LinkDriverConfig::default(),
        0,
    );
    let mut responder = LinkDriver::new(
        LinkRole::Responder,
        Keypair::from_secret_bytes([0x22; 32]),
        Keypair::from_secret_bytes([0x44; 32]),
        LinkDriverConfig::default(),
        0,
    );
    let mut i_store = EntryStore::new();
    let mut r_store = EntryStore::new();

    // I → R: HELLO (the §8.2 frame).
    let out = initiator.start();
    assert_eq!(out.frames, vec![hello_frame.clone()]);
    assert!(responder.start().frames.is_empty());

    // R → I: HELLO, on receiving the initiator's.
    let out = responder.on_bytes(&mut r_store, &hello_frame, 0);
    assert_eq!(out.frames, vec![hello_frame.clone()]);

    // I → R: HANDSHAKE 1, only after the responder HELLO arrives (§4.1).
    let out = initiator.on_bytes(&mut i_store, &hello_frame, 0);
    assert_eq!(out.frames, vec![handshake_frame_1.clone()]);

    // R → I: HANDSHAKE 2.
    let out = responder.on_bytes(&mut r_store, &handshake_frame_1, 0);
    assert_eq!(out.frames, vec![handshake_frame_2.clone()]);

    // I → R: HANDSHAKE 3; the initiator is established.
    let out = initiator.on_bytes(&mut i_store, &handshake_frame_2, 0);
    assert_eq!(out.frames, vec![handshake_frame_3.clone()]);
    let i_fp = Fingerprint::of(&Keypair::from_secret_bytes([0x22; 32]).public);
    assert_eq!(out.events, vec![LinkEvent::Established { remote: i_fp }]);
    assert!(initiator.is_established());

    // The responder is established on reading message 3.
    let out = responder.on_bytes(&mut r_store, &handshake_frame_3, 0);
    assert!(out.frames.is_empty());
    let r_fp = Fingerprint::of(&Keypair::from_secret_bytes([0x11; 32]).public);
    assert_eq!(out.events, vec![LinkEvent::Established { remote: r_fp }]);

    // I → R, nonce 0: the §8.3 GOSSIP_PUSH as the first transport message.
    let push = Message::GossipPush(GossipPush {
        entries: vec![sample_entry()],
        ttl: 4,
    });
    let out = initiator.send(&push).unwrap();
    assert_eq!(out.frames, vec![transport_frame.clone()]);

    // And the responder decrypts, verifies, and stores the entry.
    let out = responder.on_bytes(&mut r_store, &transport_frame, 0);
    assert!(matches!(
        out.events.as_slice(),
        [LinkEvent::Sync(racnet_core::sync::SyncEvent::Stored(_))]
    ));
}
