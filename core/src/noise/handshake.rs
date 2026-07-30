//! The XX handshake pattern (Noise revision 34 §7.5).
//!
//! ```text
//! XX:
//!   -> e
//!   <- e, ee, s, es
//!   -> s, se
//! ```
//!
//! Only XX is implemented; the message sequence is written out step by
//! step rather than driven from a pattern table, because three fixed
//! steps read better than an interpreter for one pattern.

use zeroize::Zeroize;

use super::symmetric::SymmetricState;
use super::transport::TransportState;
use super::{Keypair, NoiseError, PublicKey};

/// Which side of the handshake this state drives. The transport
/// connection opener is the initiator (PROTOCOL.md §4.1).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Role {
    Initiator,
    Responder,
}

/// DH output lengths, used for exact message-length checks.
const DHLEN: usize = 32;
/// AEAD tag length.
const TAGLEN: usize = 16;

/// One XX handshake in progress. Construct with [`HandshakeState::new_xx`],
/// alternate [`write_message`](Self::write_message) and
/// [`read_message`](Self::read_message) per the pattern, then convert with
/// [`into_transport`](Self::into_transport).
pub struct HandshakeState {
    symmetric: SymmetricState,
    s: Keypair,
    e: Keypair,
    re: Option<PublicKey>,
    rs: Option<PublicKey>,
    role: Role,
    /// Next pattern step, 1-based; 4 means finished.
    step: u8,
}

impl HandshakeState {
    /// Start an XX handshake. Both keypairs are injected: `s` is the
    /// long-term static identity, `e` the ephemeral for exactly this
    /// handshake (fresh entropy in production, fixed bytes in tests —
    /// reusing an ephemeral forfeits forward secrecy).
    pub fn new_xx(role: Role, s: Keypair, e: Keypair, prologue: &[u8]) -> HandshakeState {
        let mut symmetric = SymmetricState::new_for_protocol();
        symmetric.mix_hash(prologue);
        HandshakeState {
            symmetric,
            s,
            e,
            re: None,
            rs: None,
            role,
            step: 1,
        }
    }

    fn our_turn_to_write(&self) -> bool {
        match self.role {
            Role::Initiator => self.step == 1 || self.step == 3,
            Role::Responder => self.step == 2,
        }
    }

    /// Produce the next handshake message with `payload` (empty on racnet
    /// links; arbitrary for the vector tests).
    pub fn write_message(&mut self, payload: &[u8]) -> Result<Vec<u8>, NoiseError> {
        if self.step > 3 || !self.our_turn_to_write() {
            return Err(NoiseError::OutOfOrder);
        }
        let mut out = Vec::new();
        match self.step {
            1 => {
                // -> e
                out.extend_from_slice(&self.e.public.0);
                self.symmetric.mix_hash(&self.e.public.0);
            }
            2 => {
                // <- e, ee, s, es
                out.extend_from_slice(&self.e.public.0);
                self.symmetric.mix_hash(&self.e.public.0);
                let re = self.re.as_ref().ok_or(NoiseError::OutOfOrder)?;
                let mut ee = self.e.dh(re);
                self.symmetric.mix_key(&ee);
                ee.zeroize();
                let enc_s = self.symmetric.encrypt_and_hash(&self.s.public.0)?;
                out.extend_from_slice(&enc_s);
                let mut es = self.s.dh(re);
                self.symmetric.mix_key(&es);
                es.zeroize();
            }
            3 => {
                // -> s, se
                let enc_s = self.symmetric.encrypt_and_hash(&self.s.public.0)?;
                out.extend_from_slice(&enc_s);
                let re = self.re.as_ref().ok_or(NoiseError::OutOfOrder)?;
                let mut se = self.s.dh(re);
                self.symmetric.mix_key(&se);
                se.zeroize();
            }
            _ => unreachable!("step checked above"),
        }
        let enc_payload = self.symmetric.encrypt_and_hash(payload)?;
        out.extend_from_slice(&enc_payload);
        self.step += 1;
        Ok(out)
    }

    /// Exact expected length of the message about to be read, given the
    /// payload it carries. Racnet payloads are empty; vectors carry data.
    fn expected_overhead(&self) -> usize {
        match self.step {
            1 => DHLEN,                           // e
            2 => DHLEN + DHLEN + TAGLEN + TAGLEN, // e, enc(s), enc(payload) tag
            3 => DHLEN + TAGLEN + TAGLEN,         // enc(s), enc(payload) tag
            _ => 0,
        }
    }

    /// Consume the peer's next handshake message, returning its payload.
    /// Total on arbitrary input: wrong lengths and failed authentication
    /// return errors, never panic.
    pub fn read_message(&mut self, message: &[u8]) -> Result<Vec<u8>, NoiseError> {
        if self.step > 3 || self.our_turn_to_write() {
            return Err(NoiseError::OutOfOrder);
        }
        // The payload tag only exists once a key is mixed; step 1 has no
        // key, so its payload is plaintext and the minimum is bare DHLEN.
        let min_len = match self.step {
            1 => DHLEN,
            _ => self.expected_overhead(),
        };
        if message.len() < min_len {
            return Err(NoiseError::BadMessage);
        }
        let mut rest = message;
        match self.step {
            1 => {
                // -> e
                let (e_bytes, r) = rest.split_at(DHLEN);
                rest = r;
                let re = PublicKey(e_bytes.try_into().expect("split_at(32)"));
                self.symmetric.mix_hash(&re.0);
                self.re = Some(re);
            }
            2 => {
                // <- e, ee, s, es
                let (e_bytes, r) = rest.split_at(DHLEN);
                rest = r;
                let re = PublicKey(e_bytes.try_into().expect("split_at(32)"));
                self.symmetric.mix_hash(&re.0);
                self.re = Some(re);
                let mut ee = self.e.dh(&re);
                self.symmetric.mix_key(&ee);
                ee.zeroize();
                let (enc_s, r) = rest.split_at(DHLEN + TAGLEN);
                rest = r;
                let s_plain = self.symmetric.decrypt_and_hash(enc_s)?;
                let rs = PublicKey(s_plain.as_slice().try_into().expect("32-byte plaintext"));
                self.rs = Some(rs);
                let mut es = self.e.dh(&rs);
                self.symmetric.mix_key(&es);
                es.zeroize();
            }
            3 => {
                // -> s, se
                let (enc_s, r) = rest.split_at(DHLEN + TAGLEN);
                rest = r;
                let s_plain = self.symmetric.decrypt_and_hash(enc_s)?;
                let rs = PublicKey(s_plain.as_slice().try_into().expect("32-byte plaintext"));
                self.rs = Some(rs);
                let mut se = self.e.dh(&rs);
                self.symmetric.mix_key(&se);
                se.zeroize();
            }
            _ => unreachable!("step checked above"),
        }
        let payload = self.symmetric.decrypt_and_hash(rest)?;
        self.step += 1;
        Ok(payload)
    }

    /// True once message 3 has been written (initiator) or read
    /// (responder).
    pub fn is_finished(&self) -> bool {
        self.step > 3
    }

    /// Split into the transport state and the peer's authenticated static
    /// key, from which the caller derives the peer fingerprint.
    pub fn into_transport(self) -> Result<(TransportState, PublicKey), NoiseError> {
        if !self.is_finished() {
            return Err(NoiseError::OutOfOrder);
        }
        let rs = self.rs.ok_or(NoiseError::OutOfOrder)?;
        let (c1, c2, hash) = self.symmetric.split();
        // The initiator sends on the first CipherState, the responder on
        // the second (Noise revision 34 §5.3).
        let transport = match self.role {
            Role::Initiator => TransportState::new(c1, c2, hash),
            Role::Responder => TransportState::new(c2, c1, hash),
        };
        Ok((transport, rs))
    }

    /// The running handshake hash (channel binding; exposed for vector
    /// tests).
    pub fn handshake_hash(&self) -> [u8; 32] {
        self.symmetric.hash()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn pair(role: Role) -> HandshakeState {
        let (s, e) = match role {
            Role::Initiator => ([0x11; 32], [0x33; 32]),
            Role::Responder => ([0x22; 32], [0x44; 32]),
        };
        HandshakeState::new_xx(
            role,
            Keypair::from_secret_bytes(s),
            Keypair::from_secret_bytes(e),
            b"prologue",
        )
    }

    fn run_handshake() -> (HandshakeState, HandshakeState) {
        let mut i = pair(Role::Initiator);
        let mut r = pair(Role::Responder);
        let m1 = i.write_message(&[]).unwrap();
        r.read_message(&m1).unwrap();
        let m2 = r.write_message(&[]).unwrap();
        i.read_message(&m2).unwrap();
        let m3 = i.write_message(&[]).unwrap();
        r.read_message(&m3).unwrap();
        (i, r)
    }

    #[test]
    fn completes_and_agrees() {
        let (i, r) = run_handshake();
        assert!(i.is_finished() && r.is_finished());
        assert_eq!(i.handshake_hash(), r.handshake_hash());
        let (mut it, i_sees) = i.into_transport().unwrap();
        let (mut rt, r_sees) = r.into_transport().unwrap();
        assert_eq!(i_sees, Keypair::from_secret_bytes([0x22; 32]).public);
        assert_eq!(r_sees, Keypair::from_secret_bytes([0x11; 32]).public);
        let ct = it.encrypt(b"initiator speaks").unwrap();
        assert_eq!(rt.decrypt(&ct).unwrap(), b"initiator speaks");
        let ct = rt.encrypt(b"responder replies").unwrap();
        assert_eq!(it.decrypt(&ct).unwrap(), b"responder replies");
    }

    #[test]
    fn message_lengths_are_exact() {
        let mut i = pair(Role::Initiator);
        let mut r = pair(Role::Responder);
        let m1 = i.write_message(&[]).unwrap();
        assert_eq!(m1.len(), 32);
        r.read_message(&m1).unwrap();
        let m2 = r.write_message(&[]).unwrap();
        assert_eq!(m2.len(), 96);
        i.read_message(&m2).unwrap();
        let m3 = i.write_message(&[]).unwrap();
        assert_eq!(m3.len(), 64);
    }

    #[test]
    fn different_prologues_fail_at_message_2() {
        let mut i = HandshakeState::new_xx(
            Role::Initiator,
            Keypair::from_secret_bytes([0x11; 32]),
            Keypair::from_secret_bytes([0x33; 32]),
            b"prologue A",
        );
        let mut r = HandshakeState::new_xx(
            Role::Responder,
            Keypair::from_secret_bytes([0x22; 32]),
            Keypair::from_secret_bytes([0x44; 32]),
            b"prologue B",
        );
        let m1 = i.write_message(&[]).unwrap();
        r.read_message(&m1).unwrap();
        let m2 = r.write_message(&[]).unwrap();
        // Message 2 is the first authenticated one; the prologue mismatch
        // surfaces here, exactly the downgrade-detection property §4.2
        // relies on.
        assert_eq!(i.read_message(&m2), Err(NoiseError::DecryptFailed));
    }

    #[test]
    fn truncated_and_oversized_messages_are_rejected() {
        let mut i = pair(Role::Initiator);
        let mut r = pair(Role::Responder);
        let m1 = i.write_message(&[]).unwrap();
        assert_eq!(r.read_message(&m1[..31]), Err(NoiseError::BadMessage));
        r.read_message(&m1).unwrap();
        let m2 = r.write_message(&[]).unwrap();
        assert_eq!(i.read_message(&m2[..95]), Err(NoiseError::BadMessage));
        // Extra trailing bytes corrupt the payload ciphertext.
        let mut long = m2.clone();
        long.push(0);
        assert_eq!(i.read_message(&long), Err(NoiseError::DecryptFailed));
    }

    #[test]
    fn out_of_order_calls_are_errors() {
        let mut i = pair(Role::Initiator);
        assert_eq!(i.read_message(&[0u8; 32]), Err(NoiseError::OutOfOrder));
        let mut r = pair(Role::Responder);
        assert_eq!(r.write_message(&[]), Err(NoiseError::OutOfOrder));
        let unfinished = pair(Role::Initiator);
        assert!(matches!(
            unfinished.into_transport(),
            Err(NoiseError::OutOfOrder)
        ));
    }

    #[test]
    fn payloads_ride_the_handshake() {
        let mut i = pair(Role::Initiator);
        let mut r = pair(Role::Responder);
        let m1 = i.write_message(b"hi from message 1").unwrap();
        assert_eq!(r.read_message(&m1).unwrap(), b"hi from message 1");
        let m2 = r.write_message(b"hi from message 2").unwrap();
        assert_eq!(i.read_message(&m2).unwrap(), b"hi from message 2");
        let m3 = i.write_message(b"hi from message 3").unwrap();
        assert_eq!(r.read_message(&m3).unwrap(), b"hi from message 3");
    }
}
