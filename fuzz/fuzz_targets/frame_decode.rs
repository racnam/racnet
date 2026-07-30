//! Fuzzes the outer-frame decoder and inner-message codec (ADR-0013).
//!
//! The input's first byte picks a chunking pattern so the decoder sees
//! arbitrary stream segmentation. Anything that decodes must re-encode
//! and decode back to the same message.

#![no_main]

use libfuzzer_sys::fuzz_target;
use racnet_core::wire::{decode_message, encode_message, FrameDecoder};

fuzz_target!(|data: &[u8]| {
    let Some((&selector, stream)) = data.split_first() else {
        return;
    };
    let chunk = usize::from(selector).max(1);
    let mut decoder = FrameDecoder::new();
    for piece in stream.chunks(chunk) {
        decoder.push(piece);
        loop {
            match decoder.next_message() {
                Ok(Some(msg)) => {
                    // Round-trip: what decodes must re-encode decodably to
                    // the same value.
                    let body = encode_message(&msg).expect("decoded message re-encodes");
                    let again = decode_message(&body).expect("re-encoded message decodes");
                    assert_eq!(again, msg);
                }
                Ok(None) => break,
                Err(_) => return, // rejection is fine; panicking is not
            }
        }
    }
});
