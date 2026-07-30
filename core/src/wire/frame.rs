//! Outer frames: length-prefixed bodies over a reliable stream (PROTOCOL.md §1.1).
//!
//! The outer frame is a 2-byte big-endian length followed by the body. In the
//! cleartext epoch the body is the padded inner message; after the session
//! layer lands (M3) it is the Noise message of the same padded plaintext, so
//! this layer stays unchanged.

use super::error::WireError;
use super::msg::{decode_message, encode_message, Message};

/// Encodes a message as a complete outer frame.
pub fn encode_frame(msg: &Message) -> Result<Vec<u8>, WireError> {
    let body = encode_message(msg)?;
    let mut out = Vec::with_capacity(2 + body.len());
    out.extend_from_slice(
        &u16::try_from(body.len())
            .expect("bounded by padded_len")
            .to_be_bytes(),
    );
    out.extend_from_slice(&body);
    Ok(out)
}

/// Incremental frame decoder, safe against arbitrary partial reads.
///
/// Feed stream bytes with [`push`](Self::push) as they arrive; pull completed
/// messages with [`next_message`](Self::next_message).
#[derive(Debug, Default)]
pub struct FrameDecoder {
    buf: Vec<u8>,
}

impl FrameDecoder {
    /// Creates an empty decoder.
    pub fn new() -> FrameDecoder {
        FrameDecoder::default()
    }

    /// Appends received stream bytes.
    pub fn push(&mut self, bytes: &[u8]) {
        self.buf.extend_from_slice(bytes);
    }

    /// Removes and returns the next complete frame body, if one is buffered.
    pub fn next_body(&mut self) -> Option<Vec<u8>> {
        if self.buf.len() < 2 {
            return None;
        }
        let len = usize::from(u16::from_be_bytes([self.buf[0], self.buf[1]]));
        if self.buf.len() < 2 + len {
            return None;
        }
        let body = self.buf[2..2 + len].to_vec();
        self.buf.drain(..2 + len);
        Some(body)
    }

    /// Decodes the next complete message, if one is buffered.
    ///
    /// An error means the peer violated the protocol; the caller should treat
    /// it as terminal for the link (PROTOCOL.md §7).
    pub fn next_message(&mut self) -> Result<Option<Message>, WireError> {
        match self.next_body() {
            Some(body) => decode_message(&body).map(Some),
            None => Ok(None),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::wire::payload::{ErrorMsg, Hello};

    fn hello() -> Message {
        Message::Hello(Hello {
            versions: vec![1],
            features: 0,
            transports: vec![1],
        })
    }

    #[test]
    fn round_trips_a_frame() {
        let frame = encode_frame(&hello()).unwrap();
        assert_eq!(frame.len(), 2 + 256);
        let mut dec = FrameDecoder::new();
        dec.push(&frame);
        assert_eq!(dec.next_message().unwrap(), Some(hello()));
        assert_eq!(dec.next_message().unwrap(), None);
    }

    #[test]
    fn reassembles_across_partial_reads() {
        let frame = encode_frame(&hello()).unwrap();
        let mut dec = FrameDecoder::new();
        for chunk in frame.chunks(7) {
            dec.push(chunk);
        }
        assert_eq!(dec.next_message().unwrap(), Some(hello()));
    }

    #[test]
    fn yields_multiple_frames_in_order() {
        let a = encode_frame(&hello()).unwrap();
        let b = encode_frame(&Message::Error(ErrorMsg { code: 2 })).unwrap();
        let mut dec = FrameDecoder::new();
        dec.push(&a);
        dec.push(&b);
        assert_eq!(dec.next_message().unwrap(), Some(hello()));
        assert_eq!(
            dec.next_message().unwrap(),
            Some(Message::Error(ErrorMsg { code: 2 }))
        );
        assert_eq!(dec.next_message().unwrap(), None);
    }
}
