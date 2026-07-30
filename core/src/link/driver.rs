//! The link driver: PROTOCOL.md §4 as a sans-I/O state machine.
//!
//! A [`LinkDriver`] owns one link end to end: the HELLO exchange and
//! version negotiation (§5), the XX handshake carried in HANDSHAKE
//! messages with both HELLO bodies as prologue (§4.1–§4.2), and the
//! transport epoch, where it encrypts and decrypts padded inner messages
//! and demuxes sync traffic to an owned [`Syncer`]. Bytes go in, frames
//! and events come out; the transport shim owns the socket and the clock.
//!
//! Failure discipline (§4.4, §7): the only wire-visible cleartext failure
//! is ERROR code 2 on empty version intersection. Every other cleartext
//! or cryptographic failure closes the link silently — the driver drops
//! any frames it had queued in the same call, reports a local-only
//! [`CloseReason`], and stays inert forever after.

use crate::noise::{Fingerprint, HandshakeState, Keypair, NoiseError, Role, TransportState};
use crate::store::EntryStore;
use crate::sync::{LinkRole, SyncConfig, SyncError, SyncEvent, Syncer};
use crate::wire::{
    decode_message, encode_message, is_padded_len, ErrorMsg, FrameDecoder, Handshake, Hello,
    Message, MsgType, WIRE_VERSION,
};

/// AEAD tag length appended to every transport-epoch body.
const TAGLEN: usize = 16;

/// 24 hours in microseconds (§4.3).
const MAX_LIFETIME_US: u64 = 86_400_000_000;

/// Tunables for one link.
#[derive(Debug, Clone)]
pub struct LinkDriverConfig {
    /// Protocol versions offered in HELLO; defaults to `[WIRE_VERSION]`.
    pub versions: Vec<u64>,
    /// Feature bitset; 0 in version 1.
    pub features: u64,
    /// Transport registry values offered in HELLO.
    pub transports: Vec<u64>,
    /// Session lifetime cap; enforced by `on_bytes`/`on_tick` (§4.3).
    pub max_lifetime_us: u64,
    /// Configuration for the owned [`Syncer`].
    pub sync: SyncConfig,
}

impl Default for LinkDriverConfig {
    fn default() -> LinkDriverConfig {
        LinkDriverConfig {
            versions: vec![WIRE_VERSION],
            features: 0,
            transports: vec![1],
            max_lifetime_us: MAX_LIFETIME_US,
            sync: SyncConfig::default(),
        }
    }
}

/// Why a link closed. Local-only — never serialized, never sent — so it
/// can carry detail the wire must not (§7).
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum CloseReason {
    /// Empty version intersection; ERROR 2 was sent or received (§5).
    VersionIncompatible,
    /// The cleartext epoch went wrong: bad frame, wrong message type or
    /// state, or a failed handshake step. Closed silently.
    HandshakeFailed,
    /// A transport-epoch body failed the pre-decryption length gate
    /// (§1.1). Closed silently.
    BadCiphertextLength,
    /// Transport-message decryption or authentication failed (§4.4).
    /// Closed silently.
    DecryptFailed,
    /// The peer broke a rule inside the transport epoch; an encrypted
    /// ERROR with the given code was sent (§7).
    ProtocolViolation(u64),
    /// The peer sent ERROR with this code; the link is closed (§7).
    PeerError(u64),
    /// The §4.3 lifetime cap passed. Closed silently; reconnect to
    /// rehandshake.
    LifetimeExpired,
    /// A CipherState nonce ran out (§4.3). Closed silently.
    NonceExhausted,
}

/// Something the link learned or did.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum LinkEvent {
    /// The handshake completed; the peer's static key authenticated.
    Established {
        /// SHA-256 fingerprint of the peer's static public key (§4).
        remote: Fingerprint,
    },
    /// An event from the owned sync layer.
    Sync(SyncEvent),
    /// The link closed. Terminal: no further frames or events follow.
    Closed(CloseReason),
}

/// Frames to write and events to surface after one driver call.
#[derive(Debug, Default)]
pub struct LinkOutput {
    /// Complete outer frames, to be written to the transport in order.
    pub frames: Vec<Vec<u8>>,
    /// What happened, in order.
    pub events: Vec<LinkEvent>,
}

/// Local API misuse; nothing here reflects peer behavior.
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum LinkError {
    /// The link is not (or not yet) in the transport epoch.
    #[error("link not established")]
    NotEstablished,
    /// The message type is not permitted in the transport epoch.
    #[error("message type not permitted on an established link")]
    NotPermitted,
    /// The sync layer refused the operation.
    #[error(transparent)]
    Sync(#[from] SyncError),
}

enum LinkState {
    /// Waiting for the peer's HELLO (both roles; the initiator has
    /// already queued its own via [`LinkDriver::start`]).
    AwaitPeerHello,
    /// HELLOs exchanged; the XX handshake is running.
    Handshaking(Box<HandshakeState>),
    /// Established (§4.1 after message 5).
    Transport(Box<TransportState>),
    /// Terminal.
    Closed(CloseReason),
}

/// One link, driven sans-I/O. See the module docs.
pub struct LinkDriver {
    role: LinkRole,
    state: LinkState,
    config: LinkDriverConfig,
    decoder: FrameDecoder,
    syncer: Syncer,
    /// Our static identity and this link's ephemeral, held until the
    /// handshake starts.
    keys: Option<(Keypair, Keypair)>,
    /// The padded body of the HELLO we sent (prologue material, §4.2).
    our_hello_body: Vec<u8>,
    /// The peer's fingerprint, once the handshake authenticated it.
    remote_fingerprint: Option<Fingerprint>,
    /// When the link opened, for the §4.3 lifetime cap.
    opened_at_us: u64,
}

impl LinkDriver {
    /// A driver for one transport connection. `role` fixes every
    /// directional role at once (§4): the transport opener is the Noise
    /// initiator and uses even sync session ids. `ephemeral` is used for
    /// exactly this link's handshake — pass fresh entropy in production
    /// ([`Keypair::generate`]); tests inject fixed bytes.
    pub fn new(
        role: LinkRole,
        static_keys: Keypair,
        ephemeral: Keypair,
        config: LinkDriverConfig,
        now_us: u64,
    ) -> LinkDriver {
        let syncer = Syncer::new(role, config.sync);
        LinkDriver {
            role,
            state: LinkState::AwaitPeerHello,
            config,
            decoder: FrameDecoder::new(),
            syncer,
            keys: Some((static_keys, ephemeral)),
            our_hello_body: Vec::new(),
            remote_fingerprint: None,
            opened_at_us: now_us,
        }
    }

    /// Frames to write as soon as the transport connects: the initiator
    /// opens with its HELLO; the responder waits.
    pub fn start(&mut self) -> LinkOutput {
        let mut out = LinkOutput::default();
        if self.role == LinkRole::Initiator && matches!(self.state, LinkState::AwaitPeerHello) {
            self.queue_hello(&mut out);
        }
        out
    }

    /// True once the handshake has completed and the link is usable.
    pub fn is_established(&self) -> bool {
        matches!(self.state, LinkState::Transport(_))
    }

    /// The peer's fingerprint, once established.
    pub fn remote_fingerprint(&self) -> Option<Fingerprint> {
        self.remote_fingerprint
    }

    /// Why the link closed, if it has.
    pub fn close_reason(&self) -> Option<&CloseReason> {
        match &self.state {
            LinkState::Closed(reason) => Some(reason),
            _ => None,
        }
    }

    /// Feed raw stream bytes (any segmentation). Returns frames to write
    /// and events, in order. A closed driver returns empty output.
    pub fn on_bytes(&mut self, store: &mut EntryStore, bytes: &[u8], now_us: u64) -> LinkOutput {
        let mut out = LinkOutput::default();
        if self.lifetime_expired(now_us, &mut out) {
            return out;
        }
        if matches!(self.state, LinkState::Closed(_)) {
            return out;
        }
        self.decoder.push(bytes);
        while let Some(body) = self.decoder.next_body() {
            self.handle_body(store, &body, &mut out);
            if matches!(self.state, LinkState::Closed(_)) {
                break;
            }
        }
        out
    }

    /// Time-only advancement: enforces the §4.3 lifetime cap.
    pub fn on_tick(&mut self, now_us: u64) -> LinkOutput {
        let mut out = LinkOutput::default();
        self.lifetime_expired(now_us, &mut out);
        out
    }

    /// Opens a reconciliation session (transport epoch only). Returns the
    /// session id and the frames to write.
    pub fn open_sync(
        &mut self,
        store: &EntryStore,
        window: crate::wire::SortKeyWindow,
    ) -> Result<(u64, LinkOutput), LinkError> {
        if !self.is_established() {
            return Err(LinkError::NotEstablished);
        }
        let (sid, msg) = self.syncer.open_session(store, window)?;
        let mut out = LinkOutput::default();
        self.queue_encrypted(&msg, &mut out);
        Ok((sid, out))
    }

    /// Originates one application message (e.g. a GOSSIP_PUSH) on the
    /// established link.
    pub fn send(&mut self, msg: &Message) -> Result<LinkOutput, LinkError> {
        if !self.is_established() {
            return Err(LinkError::NotEstablished);
        }
        if matches!(msg.msg_type(), MsgType::Hello | MsgType::Handshake) {
            return Err(LinkError::NotPermitted);
        }
        let mut out = LinkOutput::default();
        self.queue_encrypted(msg, &mut out);
        Ok(out)
    }

    fn lifetime_expired(&mut self, now_us: u64, out: &mut LinkOutput) -> bool {
        if matches!(self.state, LinkState::Closed(_)) {
            return true;
        }
        if now_us.saturating_sub(self.opened_at_us) >= self.config.max_lifetime_us {
            self.close_silently(CloseReason::LifetimeExpired, out);
            return true;
        }
        false
    }

    /// Silent close: drop everything queued, emit only the Closed event
    /// (§4.4 — no error oracle).
    fn close_silently(&mut self, reason: CloseReason, out: &mut LinkOutput) {
        out.frames.clear();
        self.state = LinkState::Closed(reason.clone());
        self.keys = None;
        out.events.push(LinkEvent::Closed(reason));
    }

    /// ERROR-bearing close: queue the ERROR frame (cleartext for code 2
    /// during establishment, encrypted inside the transport epoch), then
    /// close.
    fn close_with_error(&mut self, code: u64, reason: CloseReason, out: &mut LinkOutput) {
        let error = Message::Error(ErrorMsg { code });
        match &mut self.state {
            LinkState::Transport(transport) => {
                if let Ok(frame) = encrypt_frame(transport, &error) {
                    out.frames.push(frame);
                }
            }
            _ => {
                if let Ok(frame) = crate::wire::encode_frame(&error) {
                    out.frames.push(frame);
                }
            }
        }
        self.state = LinkState::Closed(reason.clone());
        self.keys = None;
        out.events.push(LinkEvent::Closed(reason));
    }

    fn queue_hello(&mut self, out: &mut LinkOutput) {
        let hello = Message::Hello(Hello {
            versions: self.config.versions.clone(),
            features: self.config.features,
            transports: self.config.transports.clone(),
        });
        let body = encode_message(&hello).expect("own HELLO encodes");
        let mut frame = Vec::with_capacity(2 + body.len());
        frame.extend_from_slice(&(body.len() as u16).to_be_bytes());
        frame.extend_from_slice(&body);
        self.our_hello_body = body;
        out.frames.push(frame);
    }

    fn queue_encrypted(&mut self, msg: &Message, out: &mut LinkOutput) {
        let LinkState::Transport(transport) = &mut self.state else {
            return;
        };
        match encrypt_frame(transport, msg) {
            Ok(frame) => out.frames.push(frame),
            Err(_) => self.close_silently(CloseReason::NonceExhausted, out),
        }
    }

    fn handle_body(&mut self, store: &mut EntryStore, body: &[u8], out: &mut LinkOutput) {
        match std::mem::replace(
            &mut self.state,
            LinkState::Closed(CloseReason::HandshakeFailed),
        ) {
            LinkState::AwaitPeerHello => self.handle_hello_body(body, out),
            LinkState::Handshaking(hs) => self.handle_handshake_body(*hs, body, out),
            LinkState::Transport(transport) => {
                self.handle_transport_body(*transport, store, body, out)
            }
            closed @ LinkState::Closed(_) => self.state = closed,
        }
    }

    /// Cleartext epoch, waiting for the peer's HELLO.
    fn handle_hello_body(&mut self, body: &[u8], out: &mut LinkOutput) {
        let msg = match decode_message(body) {
            Ok(msg) => msg,
            Err(_) => return self.close_silently(CloseReason::HandshakeFailed, out),
        };
        let hello = match msg {
            Message::Hello(hello) => hello,
            Message::Error(err) => {
                return self.close_silently(CloseReason::PeerError(err.code), out)
            }
            _ => return self.close_silently(CloseReason::HandshakeFailed, out),
        };

        // §5: highest common version, or ERROR 2 — the only wire-visible
        // cleartext failure.
        let compatible = hello
            .versions
            .iter()
            .any(|v| self.config.versions.contains(v));
        if !compatible {
            return self.close_with_error(2, CloseReason::VersionIncompatible, out);
        }

        // Responder answers with its own HELLO now; the prologue needs it.
        if self.role == LinkRole::Responder {
            self.queue_hello(out);
        }

        // §4.2: prologue = initiator HELLO body ‖ responder HELLO body.
        let prologue = match self.role {
            LinkRole::Initiator => [self.our_hello_body.as_slice(), body].concat(),
            LinkRole::Responder => [body, self.our_hello_body.as_slice()].concat(),
        };
        let (static_keys, ephemeral) = self.keys.take().expect("keys present until handshake");
        let noise_role = match self.role {
            LinkRole::Initiator => Role::Initiator,
            LinkRole::Responder => Role::Responder,
        };
        let mut hs = HandshakeState::new_xx(noise_role, static_keys, ephemeral, &prologue);

        if self.role == LinkRole::Initiator {
            // §4.1 message 3 (Noise message 1) — only now that the
            // responder HELLO fixed the prologue.
            match hs.write_message(&[]) {
                Ok(noise_msg) => self.queue_handshake_frame(&noise_msg, out),
                Err(_) => return self.close_silently(CloseReason::HandshakeFailed, out),
            }
        }
        self.state = LinkState::Handshaking(Box::new(hs));
    }

    /// Cleartext epoch, XX in progress.
    fn handle_handshake_body(&mut self, mut hs: HandshakeState, body: &[u8], out: &mut LinkOutput) {
        let msg = match decode_message(body) {
            Ok(msg) => msg,
            Err(_) => return self.close_silently(CloseReason::HandshakeFailed, out),
        };
        let handshake = match msg {
            Message::Handshake(handshake) => handshake,
            Message::Error(err) => {
                return self.close_silently(CloseReason::PeerError(err.code), out)
            }
            _ => return self.close_silently(CloseReason::HandshakeFailed, out),
        };

        // Read the peer's Noise message; §4.1 payloads are empty.
        match hs.read_message(&handshake.msg) {
            Ok(payload) if payload.is_empty() => {}
            _ => return self.close_silently(CloseReason::HandshakeFailed, out),
        }
        if !hs.is_finished() {
            // Our turn to write (responder after message 1, initiator
            // after message 2).
            match hs.write_message(&[]) {
                Ok(noise_msg) => self.queue_handshake_frame(&noise_msg, out),
                Err(_) => return self.close_silently(CloseReason::HandshakeFailed, out),
            }
        }
        if hs.is_finished() {
            match hs.into_transport() {
                Ok((transport, remote_static)) => {
                    let remote = Fingerprint::of(&remote_static);
                    self.remote_fingerprint = Some(remote);
                    self.state = LinkState::Transport(Box::new(transport));
                    out.events.push(LinkEvent::Established { remote });
                }
                Err(_) => self.close_silently(CloseReason::HandshakeFailed, out),
            }
        } else {
            self.state = LinkState::Handshaking(Box::new(hs));
        }
    }

    /// Transport epoch: gate, decrypt, decode, demux.
    fn handle_transport_body(
        &mut self,
        mut transport: TransportState,
        store: &mut EntryStore,
        body: &[u8],
        out: &mut LinkOutput,
    ) {
        // §1.1 pre-decryption gate: a body that is not a permitted padded
        // length plus the tag cannot be authentic.
        if body.len() < TAGLEN || !is_padded_len(body.len() - TAGLEN) {
            return self.close_silently(CloseReason::BadCiphertextLength, out);
        }
        let plaintext = match transport.decrypt(body) {
            Ok(plaintext) => plaintext,
            Err(NoiseError::NonceExhausted) => {
                return self.close_silently(CloseReason::NonceExhausted, out)
            }
            Err(_) => return self.close_silently(CloseReason::DecryptFailed, out),
        };
        self.state = LinkState::Transport(Box::new(transport));

        // The plaintext authenticated, so failures from here are peer
        // protocol violations, reported encrypted (§7).
        let msg = match decode_message(&plaintext) {
            Ok(msg) => msg,
            Err(_) => {
                return self.close_with_error(1, CloseReason::ProtocolViolation(1), out);
            }
        };
        match msg {
            Message::Hello(_) | Message::Handshake(_) => {
                // §4.1: cleartext-epoch types must not reappear.
                self.close_with_error(3, CloseReason::ProtocolViolation(3), out);
            }
            Message::Error(err) => {
                self.close_silently(CloseReason::PeerError(err.code), out);
            }
            msg => match self.syncer.handle_message(store, &msg) {
                Ok(output) => {
                    for reply in &output.replies {
                        self.queue_encrypted(reply, out);
                        if matches!(self.state, LinkState::Closed(_)) {
                            return;
                        }
                    }
                    out.events
                        .extend(output.events.into_iter().map(LinkEvent::Sync));
                }
                Err(err) => {
                    let code = u64::from(err.error_code());
                    self.close_with_error(code, CloseReason::ProtocolViolation(code), out);
                }
            },
        }
    }

    fn queue_handshake_frame(&mut self, noise_msg: &[u8], out: &mut LinkOutput) {
        let msg = Message::Handshake(Handshake {
            msg: noise_msg.to_vec(),
        });
        match crate::wire::encode_frame(&msg) {
            Ok(frame) => out.frames.push(frame),
            Err(_) => self.close_silently(CloseReason::HandshakeFailed, out),
        }
    }
}

/// Pad-then-encrypt (§1): the padded inner message becomes the Noise
/// transport payload; the frame is the 2-octet length plus ciphertext.
fn encrypt_frame(transport: &mut TransportState, msg: &Message) -> Result<Vec<u8>, NoiseError> {
    let plaintext = encode_message(msg).map_err(|_| NoiseError::BadMessage)?;
    let ciphertext = transport.encrypt(&plaintext)?;
    let mut frame = Vec::with_capacity(2 + ciphertext.len());
    frame.extend_from_slice(&(ciphertext.len() as u16).to_be_bytes());
    frame.extend_from_slice(&ciphertext);
    Ok(frame)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::wire::{encode_frame, GossipPush};

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

    /// Runs a full establishment; returns both drivers.
    fn established() -> (LinkDriver, LinkDriver, EntryStore, EntryStore) {
        let mut i = driver(LinkRole::Initiator);
        let mut r = driver(LinkRole::Responder);
        let mut i_store = EntryStore::new();
        let mut r_store = EntryStore::new();
        let mut to_r = i.start().frames;
        loop {
            let mut to_i = Vec::new();
            for frame in to_r.drain(..) {
                to_i.extend(r.on_bytes(&mut r_store, &frame, 0).frames);
            }
            if to_i.is_empty() {
                break;
            }
            for frame in to_i {
                to_r.extend(i.on_bytes(&mut i_store, &frame, 0).frames);
            }
            if to_r.is_empty() {
                break;
            }
        }
        assert!(i.is_established() && r.is_established());
        (i, r, i_store, r_store)
    }

    fn hello_frame(versions: Vec<u64>) -> Vec<u8> {
        encode_frame(&Message::Hello(Hello {
            versions,
            features: 0,
            transports: vec![1],
        }))
        .unwrap()
    }

    #[test]
    fn empty_version_intersection_sends_error_2() {
        let mut r = driver(LinkRole::Responder);
        let mut store = EntryStore::new();
        let out = r.on_bytes(&mut store, &hello_frame(vec![999]), 0);
        assert_eq!(out.frames.len(), 1);
        let msg = decode_message(&out.frames[0][2..]).unwrap();
        assert_eq!(msg, Message::Error(ErrorMsg { code: 2 }));
        assert_eq!(r.close_reason(), Some(&CloseReason::VersionIncompatible));
    }

    #[test]
    fn cleartext_sync_message_closes_silently() {
        let mut r = driver(LinkRole::Responder);
        let mut store = EntryStore::new();
        let push = encode_frame(&Message::GossipPush(GossipPush {
            entries: vec![crate::wire::Entry::sign(
                &ed25519_dalek::SigningKey::from_bytes(&[1; 32]),
                7,
                0,
                b"x".to_vec(),
            )],
            ttl: 0,
        }))
        .unwrap();
        let out = r.on_bytes(&mut store, &push, 0);
        assert!(out.frames.is_empty(), "silent: no ERROR in cleartext");
        assert_eq!(r.close_reason(), Some(&CloseReason::HandshakeFailed));
        // Closed drivers stay inert.
        let out = r.on_bytes(&mut store, &hello_frame(vec![1]), 0);
        assert!(out.frames.is_empty() && out.events.is_empty());
    }

    #[test]
    fn garbage_handshake_bytes_close_silently() {
        let (_, mut r) = (driver(LinkRole::Initiator), driver(LinkRole::Responder));
        let mut store = EntryStore::new();
        r.on_bytes(&mut store, &hello_frame(vec![1]), 0);
        let bogus = encode_frame(&Message::Handshake(Handshake {
            msg: vec![0x5a; 32],
        }))
        .unwrap();
        // A 32-byte "e" parses, but then message 3 will fail; feed a
        // wrong-length one instead so the failure is immediate.
        let short = encode_frame(&Message::Handshake(Handshake { msg: vec![0x5a; 5] })).unwrap();
        let out = r.on_bytes(&mut store, &short, 0);
        assert!(out.frames.is_empty());
        assert_eq!(r.close_reason(), Some(&CloseReason::HandshakeFailed));
        drop(bogus);
    }

    #[test]
    fn encrypted_hello_after_establishment_gets_error_3() {
        let (mut i, mut r, _, mut r_store) = established();
        // The initiator misbehaves: an encrypted HELLO.
        let LinkState::Transport(transport) = &mut i.state else {
            panic!("established")
        };
        let frame = encrypt_frame(
            transport,
            &Message::Hello(Hello {
                versions: vec![1],
                features: 0,
                transports: vec![1],
            }),
        )
        .unwrap();
        let out = r.on_bytes(&mut r_store, &frame, 0);
        assert_eq!(out.frames.len(), 1, "encrypted ERROR 3");
        assert_eq!(r.close_reason(), Some(&CloseReason::ProtocolViolation(3)));
        // The ERROR frame decrypts on the initiator side and closes it.
        let mut i_store = EntryStore::new();
        let out = i.on_bytes(&mut i_store, &out.frames[0], 0);
        assert!(out.frames.is_empty());
        assert_eq!(i.close_reason(), Some(&CloseReason::PeerError(3)));
    }

    #[test]
    fn tampered_transport_frame_closes_silently() {
        let (mut i, mut r, _, mut r_store) = established();
        let mut out = i
            .send(&Message::GossipPush(GossipPush {
                entries: vec![crate::wire::Entry::sign(
                    &ed25519_dalek::SigningKey::from_bytes(&[1; 32]),
                    7,
                    0,
                    b"x".to_vec(),
                )],
                ttl: 0,
            }))
            .unwrap();
        let frame = &mut out.frames[0];
        let last = frame.len() - 1;
        frame[last] ^= 0x01;
        let reply = r.on_bytes(&mut r_store, frame, 0);
        assert!(reply.frames.is_empty(), "no error oracle");
        assert_eq!(r.close_reason(), Some(&CloseReason::DecryptFailed));
    }

    #[test]
    fn wrong_length_transport_body_is_rejected_before_decryption() {
        let (_, mut r, _, mut r_store) = established();
        // 100 bytes is not a padded length + tag.
        let mut frame = vec![0u8; 102];
        frame[0] = 0;
        frame[1] = 100;
        let out = r.on_bytes(&mut r_store, &frame, 0);
        assert!(out.frames.is_empty());
        assert_eq!(r.close_reason(), Some(&CloseReason::BadCiphertextLength));
    }

    #[test]
    fn lifetime_cap_closes_silently() {
        let (mut i, _, _, _) = established();
        let out = i.on_tick(MAX_LIFETIME_US);
        assert!(out.frames.is_empty());
        assert_eq!(
            out.events,
            vec![LinkEvent::Closed(CloseReason::LifetimeExpired)]
        );
        assert_eq!(i.close_reason(), Some(&CloseReason::LifetimeExpired));
    }

    #[test]
    fn send_and_open_sync_require_establishment() {
        let mut i = driver(LinkRole::Initiator);
        assert!(matches!(
            i.send(&Message::Error(ErrorMsg { code: 5 })),
            Err(LinkError::NotEstablished)
        ));
        let store = EntryStore::new();
        assert!(matches!(
            i.open_sync(
                &store,
                crate::wire::SortKeyWindow {
                    since: 0,
                    until: u64::MAX
                }
            ),
            Err(LinkError::NotEstablished)
        ));
        let (mut i, _, _, _) = established();
        assert!(matches!(
            i.send(&Message::Hello(Hello {
                versions: vec![1],
                features: 0,
                transports: vec![1],
            })),
            Err(LinkError::NotPermitted)
        ));
    }
}
