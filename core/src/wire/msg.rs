//! Message type registry and inner-message codec (PROTOCOL.md §§1.2, 2).
//!
//! Registry values not listed here are reserved or unassigned: 0x00 is never
//! assigned, 0x11 is reserved for Plumtree lazy IHAVE, 0x12–0x13 for Plumtree
//! control, 0x30–0x3F for chunk transfer, and 0xF0–0xFF for private use.
//! Receiving any of them is a protocol violation (PROTOCOL.md §2).

use minicbor::Decoder;

use super::error::WireError;
use super::pad::{padded_len, BLOCK_SIZES};
use super::payload::{ErrorMsg, GossipPush, Handshake, Hello, ReconDone, ReconInit, ReconMsg};

/// Inner-message header length: type (1) + payload length (2).
const HEADER_LEN: usize = 3;

/// The active message types of protocol version 1 (PROTOCOL.md §2).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[repr(u8)]
pub enum MsgType {
    /// Version and capability announce.
    Hello = 0x01,
    /// Noise handshake carrier.
    Handshake = 0x02,
    /// Eager push of entries.
    GossipPush = 0x10,
    /// Opens a reconciliation session.
    ReconInit = 0x20,
    /// One Negentropy round.
    ReconMsg = 0x21,
    /// Closes a reconciliation session.
    ReconDone = 0x22,
    /// Terminal error.
    Error = 0x70,
}

impl TryFrom<u8> for MsgType {
    type Error = WireError;

    fn try_from(value: u8) -> Result<Self, WireError> {
        match value {
            0x01 => Ok(MsgType::Hello),
            0x02 => Ok(MsgType::Handshake),
            0x10 => Ok(MsgType::GossipPush),
            0x20 => Ok(MsgType::ReconInit),
            0x21 => Ok(MsgType::ReconMsg),
            0x22 => Ok(MsgType::ReconDone),
            0x70 => Ok(MsgType::Error),
            other => Err(WireError::UnknownMsgType(other)),
        }
    }
}

/// A decoded wire message: type plus payload.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Message {
    /// HELLO (0x01).
    Hello(Hello),
    /// HANDSHAKE (0x02).
    Handshake(Handshake),
    /// GOSSIP_PUSH (0x10).
    GossipPush(GossipPush),
    /// RECON_INIT (0x20).
    ReconInit(ReconInit),
    /// RECON_MSG (0x21).
    ReconMsg(ReconMsg),
    /// RECON_DONE (0x22).
    ReconDone(ReconDone),
    /// ERROR (0x70).
    Error(ErrorMsg),
}

impl Message {
    /// The registry type of this message.
    pub fn msg_type(&self) -> MsgType {
        match self {
            Message::Hello(_) => MsgType::Hello,
            Message::Handshake(_) => MsgType::Handshake,
            Message::GossipPush(_) => MsgType::GossipPush,
            Message::ReconInit(_) => MsgType::ReconInit,
            Message::ReconMsg(_) => MsgType::ReconMsg,
            Message::ReconDone(_) => MsgType::ReconDone,
            Message::Error(_) => MsgType::Error,
        }
    }

    fn payload_bytes(&self) -> Vec<u8> {
        let mut out = Vec::new();
        let result = match self {
            Message::Hello(p) => minicbor::encode(p, &mut out),
            Message::Handshake(p) => minicbor::encode(p, &mut out),
            Message::GossipPush(p) => minicbor::encode(p, &mut out),
            Message::ReconInit(p) => minicbor::encode(p, &mut out),
            Message::ReconMsg(p) => minicbor::encode(p, &mut out),
            Message::ReconDone(p) => minicbor::encode(p, &mut out),
            Message::Error(p) => minicbor::encode(p, &mut out),
        };
        result.expect("Vec<u8> writes are infallible");
        out
    }
}

/// CDDL occurrence constraints are normative in both directions
/// (PROTOCOL.md §3.1).
fn validate(msg: &Message) -> Result<(), WireError> {
    match msg {
        Message::Hello(h) if h.versions.is_empty() => {
            Err(WireError::Schema("hello versions list must not be empty"))
        }
        Message::GossipPush(p) if p.entries.is_empty() => Err(WireError::Schema(
            "gossip-push entries list must not be empty",
        )),
        _ => Ok(()),
    }
}

/// Encodes a message as a padded inner message: header, CBOR payload, zero
/// padding to the block boundary (PROTOCOL.md §1.2–1.3).
pub fn encode_message(msg: &Message) -> Result<Vec<u8>, WireError> {
    validate(msg)?;
    let payload = msg.payload_bytes();
    let unpadded = HEADER_LEN + payload.len();
    let padded = padded_len(unpadded).ok_or(WireError::TooLarge)?;
    let mut out = Vec::with_capacity(padded);
    out.push(msg.msg_type() as u8);
    out.extend_from_slice(
        &u16::try_from(payload.len())
            .expect("bounded by padded_len")
            .to_be_bytes(),
    );
    out.extend_from_slice(&payload);
    out.resize(padded, 0);
    Ok(out)
}

/// Decodes a padded inner message from a frame body (PROTOCOL.md §1.2–1.3).
///
/// The body length must be exactly the padded length for the declared payload
/// — over-padding is a protocol violation. Padding contents are ignored.
pub fn decode_message(body: &[u8]) -> Result<Message, WireError> {
    if body.len() < BLOCK_SIZES[0] {
        return Err(WireError::BadFrameLength(body.len()));
    }
    let payload_len = usize::from(u16::from_be_bytes([body[1], body[2]]));
    let unpadded = HEADER_LEN + payload_len;
    if padded_len(unpadded) != Some(body.len()) {
        return Err(WireError::BadPayloadLength(payload_len));
    }
    let msg_type = MsgType::try_from(body[0])?;
    let payload = &body[HEADER_LEN..unpadded];
    let mut dec = Decoder::new(payload);
    let msg = match msg_type {
        MsgType::Hello => Message::Hello(dec.decode()?),
        MsgType::Handshake => Message::Handshake(dec.decode()?),
        MsgType::GossipPush => Message::GossipPush(dec.decode()?),
        MsgType::ReconInit => Message::ReconInit(dec.decode()?),
        MsgType::ReconMsg => Message::ReconMsg(dec.decode()?),
        MsgType::ReconDone => Message::ReconDone(dec.decode()?),
        MsgType::Error => Message::Error(dec.decode()?),
    };
    if dec.position() != payload.len() {
        return Err(WireError::TrailingBytes);
    }
    validate(&msg)?;
    Ok(msg)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn hello() -> Message {
        Message::Hello(Hello {
            versions: vec![1],
            features: 0,
            transports: vec![1],
        })
    }

    #[test]
    fn encode_pads_to_smallest_block() {
        let body = encode_message(&hello()).unwrap();
        assert_eq!(body.len(), 256);
        assert_eq!(decode_message(&body).unwrap(), hello());
    }

    #[test]
    fn unknown_and_reserved_types_are_rejected() {
        let mut body = encode_message(&hello()).unwrap();
        for bad in [0x00, 0x11, 0x30, 0xF0] {
            body[0] = bad;
            assert_eq!(decode_message(&body), Err(WireError::UnknownMsgType(bad)));
        }
    }

    #[test]
    fn over_padding_is_rejected() {
        let mut body = encode_message(&hello()).unwrap();
        body.resize(512, 0);
        assert!(matches!(
            decode_message(&body),
            Err(WireError::BadPayloadLength(_))
        ));
    }

    #[test]
    fn short_body_is_rejected() {
        assert_eq!(
            decode_message(&[0u8; 10]),
            Err(WireError::BadFrameLength(10))
        );
    }

    #[test]
    fn empty_occurrence_constraints_are_rejected() {
        let empty_hello = Message::Hello(Hello {
            versions: vec![],
            features: 0,
            transports: vec![],
        });
        assert!(matches!(
            encode_message(&empty_hello),
            Err(WireError::Schema(_))
        ));

        // The same payload arriving off the wire is rejected on decode.
        let mut out = Vec::new();
        minicbor::encode(
            &Hello {
                versions: vec![],
                features: 0,
                transports: vec![],
            },
            &mut out,
        )
        .unwrap();
        let mut body = vec![MsgType::Hello as u8];
        body.extend_from_slice(&(out.len() as u16).to_be_bytes());
        body.extend_from_slice(&out);
        body.resize(256, 0);
        assert!(matches!(decode_message(&body), Err(WireError::Schema(_))));
    }

    #[test]
    fn trailing_bytes_are_rejected() {
        let payload = {
            let mut out = Vec::new();
            minicbor::encode(
                &Hello {
                    versions: vec![1],
                    features: 0,
                    transports: vec![],
                },
                &mut out,
            )
            .unwrap();
            out.push(0x00); // a second CBOR item inside the declared payload
            out
        };
        let mut body = vec![MsgType::Hello as u8];
        body.extend_from_slice(&(payload.len() as u16).to_be_bytes());
        body.extend_from_slice(&payload);
        body.resize(256, 0);
        assert_eq!(decode_message(&body), Err(WireError::TrailingBytes));
    }
}
