//! Property tests: every message round-trips through the frame codec intact,
//! under arbitrary payload contents, sizes straddling every padding block
//! boundary, and arbitrary stream segmentation.

use proptest::prelude::*;
use racnet_core::wire::{
    encode_frame, padded_len, Entry, ErrorMsg, FrameDecoder, GossipPush, Handshake, Hello, Message,
    ReconDone, ReconInit, ReconMsg, SortKeyWindow, BLOCK_SIZES, MAX_PADDED_LEN,
};

fn entry_strategy() -> impl Strategy<Value = Entry> {
    (
        any::<[u8; 32]>(),
        any::<u64>(),
        any::<u64>(),
        proptest::collection::vec(any::<u8>(), 0..256),
        any::<[u8; 64]>(),
    )
        .prop_map(|(author, sort_key, kind, payload, sig)| Entry {
            author,
            sort_key,
            kind,
            payload,
            sig,
        })
}

fn message_strategy() -> impl Strategy<Value = Message> {
    prop_oneof![
        (
            proptest::collection::vec(any::<u64>(), 1..8),
            any::<u64>(),
            proptest::collection::vec(any::<u64>(), 0..8),
        )
            .prop_map(|(versions, features, transports)| Message::Hello(Hello {
                versions,
                features,
                transports,
            })),
        // Sizes up to 5000 straddle the 256/512/1024/2048/4096 boundaries.
        proptest::collection::vec(any::<u8>(), 0..5000)
            .prop_map(|msg| Message::Handshake(Handshake { msg })),
        (
            proptest::collection::vec(entry_strategy(), 1..4),
            any::<u64>()
        )
            .prop_map(|(entries, ttl)| Message::GossipPush(GossipPush { entries, ttl })),
        (
            any::<u64>(),
            any::<u64>(),
            any::<u64>(),
            proptest::collection::vec(any::<u8>(), 0..512),
        )
            .prop_map(|(sid, since, until, msg)| Message::ReconInit(ReconInit {
                sid,
                window: SortKeyWindow { since, until },
                msg,
            })),
        (any::<u64>(), proptest::collection::vec(any::<u8>(), 0..512))
            .prop_map(|(sid, msg)| Message::ReconMsg(ReconMsg { sid, msg })),
        any::<u64>().prop_map(|sid| Message::ReconDone(ReconDone { sid })),
        any::<u64>().prop_map(|code| Message::Error(ErrorMsg { code })),
    ]
}

proptest! {
    #[test]
    fn round_trips_through_the_frame_codec(msg in message_strategy()) {
        let frame = encode_frame(&msg).unwrap();
        let mut dec = FrameDecoder::new();
        dec.push(&frame);
        prop_assert_eq!(dec.next_message().unwrap(), Some(msg));
        prop_assert_eq!(dec.next_message().unwrap(), None);
    }

    #[test]
    fn round_trips_under_arbitrary_segmentation(
        msg in message_strategy(),
        chunk in 1usize..64,
    ) {
        let frame = encode_frame(&msg).unwrap();
        let mut dec = FrameDecoder::new();
        for part in frame.chunks(chunk) {
            dec.push(part);
        }
        prop_assert_eq!(dec.next_message().unwrap(), Some(msg));
    }

    #[test]
    fn frame_bodies_are_valid_padded_lengths(msg in message_strategy()) {
        let frame = encode_frame(&msg).unwrap();
        let body_len = frame.len() - 2;
        let is_block = BLOCK_SIZES.contains(&body_len);
        let is_multiple = body_len.is_multiple_of(BLOCK_SIZES[BLOCK_SIZES.len() - 1])
            && body_len <= MAX_PADDED_LEN;
        prop_assert!(is_block || is_multiple);
    }

    #[test]
    fn concatenated_frames_decode_in_order(
        msgs in proptest::collection::vec(message_strategy(), 1..5),
    ) {
        let mut dec = FrameDecoder::new();
        for msg in &msgs {
            dec.push(&encode_frame(msg).unwrap());
        }
        for msg in &msgs {
            let decoded = dec.next_message().unwrap();
            prop_assert_eq!(decoded.as_ref(), Some(msg));
        }
        prop_assert_eq!(dec.next_message().unwrap(), None);
    }
}

#[test]
fn padded_len_is_exhaustively_minimal() {
    for n in 1..=MAX_PADDED_LEN {
        let padded = padded_len(n).unwrap();
        assert!(padded >= n);
        // Minimality: no smaller permitted padded length also fits n.
        let smaller_fits = BLOCK_SIZES
            .iter()
            .copied()
            .chain((1..).map(|k| k * BLOCK_SIZES[BLOCK_SIZES.len() - 1]))
            .take_while(|&p| p < padded)
            .any(|p| p >= n);
        assert!(!smaller_fits, "padded_len({n}) = {padded} is not minimal");
    }
    assert_eq!(padded_len(MAX_PADDED_LEN + 1), None);
}
