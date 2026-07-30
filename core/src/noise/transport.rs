//! Post-handshake transport state, owning the PROTOCOL.md §4.3 rekey
//! policy.

use super::{CipherState, NoiseError};

/// Rekey interval in messages per direction (PROTOCOL.md §4.3).
pub const REKEY_INTERVAL: u64 = 1024;

/// Both directions of an established session. Encrypt/decrypt apply the
/// §4.3 rule themselves: immediately before the message whose nonce is a
/// nonzero multiple of 1024, the CipherState rekeys, and the nonce
/// counter runs on across rekeys.
pub struct TransportState {
    send: CipherState,
    recv: CipherState,
    hash: [u8; 32],
    /// Set on the first decrypt failure. Failure is terminal (§4.4), and
    /// retrying a boundary message would otherwise rekey twice — Rekey is
    /// not idempotent — so a failed state refuses all further work.
    failed: bool,
}

impl TransportState {
    pub(crate) fn new(send: CipherState, recv: CipherState, hash: [u8; 32]) -> TransportState {
        TransportState {
            send,
            recv,
            hash,
            failed: false,
        }
    }

    fn rekey_if_due(cipher: &mut CipherState) {
        let n = cipher.nonce();
        if n > 0 && n.is_multiple_of(REKEY_INTERVAL) {
            cipher.rekey_preserving_nonce();
        }
    }

    /// Encrypt one transport message (empty associated data, §1.1).
    pub fn encrypt(&mut self, plaintext: &[u8]) -> Result<Vec<u8>, NoiseError> {
        if self.failed {
            return Err(NoiseError::DecryptFailed);
        }
        Self::rekey_if_due(&mut self.send);
        self.send.encrypt_with_ad(&[], plaintext)
    }

    /// Decrypt one transport message. Any error is terminal for the link
    /// (§4.4): callers close silently, and this state refuses everything
    /// afterwards.
    pub fn decrypt(&mut self, ciphertext: &[u8]) -> Result<Vec<u8>, NoiseError> {
        if self.failed {
            return Err(NoiseError::DecryptFailed);
        }
        Self::rekey_if_due(&mut self.recv);
        let result = self.recv.decrypt_with_ad(&[], ciphertext);
        if result.is_err() {
            self.failed = true;
        }
        result
    }

    /// The handshake hash (channel binding).
    pub fn handshake_hash(&self) -> [u8; 32] {
        self.hash
    }

    /// Nonce of the next message to send (lifetime accounting).
    pub fn send_nonce(&self) -> u64 {
        self.send.nonce()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn linked_pair() -> (TransportState, TransportState) {
        let mut c1a = CipherState::new();
        let mut c1b = CipherState::new();
        let mut c2a = CipherState::new();
        let mut c2b = CipherState::new();
        c1a.initialize_key([0xaa; 32]);
        c1b.initialize_key([0xaa; 32]);
        c2a.initialize_key([0xbb; 32]);
        c2b.initialize_key([0xbb; 32]);
        (
            TransportState::new(c1a, c2a, [0; 32]),
            TransportState::new(c2b, c1b, [0; 32]),
        )
    }

    #[test]
    fn traffic_survives_the_rekey_boundary() {
        let (mut a, mut b) = linked_pair();
        // Cross n = 1024 and n = 2048 in one direction.
        for i in 0..(2 * REKEY_INTERVAL + 3) {
            let msg = i.to_be_bytes();
            let ct = a.encrypt(&msg).unwrap();
            assert_eq!(b.decrypt(&ct).unwrap(), msg);
        }
        assert_eq!(a.send_nonce(), 2 * REKEY_INTERVAL + 3);
        // And a few messages the other way.
        for i in 0..3u64 {
            let ct = b.encrypt(&i.to_be_bytes()).unwrap();
            assert_eq!(a.decrypt(&ct).unwrap(), i.to_be_bytes());
        }
    }

    #[test]
    fn a_failed_state_refuses_further_work_without_rekeying_again() {
        let (mut a, mut b) = linked_pair();
        // Advance a's send side to the boundary so b's next decrypt is
        // the rekey-triggering message.
        for _ in 0..REKEY_INTERVAL {
            let ct = a.encrypt(b"x").unwrap();
            b.decrypt(&ct).unwrap();
        }
        let mut ct = a.encrypt(b"boundary").unwrap();
        ct[0] ^= 0xff;
        assert_eq!(b.decrypt(&ct).unwrap_err(), NoiseError::DecryptFailed);
        // Retrying — even with the honest bytes — must not run Rekey a
        // second time; the state is terminally failed.
        ct[0] ^= 0xff;
        assert_eq!(b.decrypt(&ct).unwrap_err(), NoiseError::DecryptFailed);
        assert_eq!(b.encrypt(b"x").unwrap_err(), NoiseError::DecryptFailed);
    }

    #[test]
    fn keys_actually_change_at_the_boundary() {
        let (mut a, mut b) = linked_pair();
        for _ in 0..REKEY_INTERVAL {
            let ct = a.encrypt(b"x").unwrap();
            b.decrypt(&ct).unwrap();
        }
        // b's receive side has rekeyed... only when the next message
        // arrives. Skip b's rekey by decrypting message 1024 with a
        // *fresh* copy of the original key: it must fail.
        let ct = a.encrypt(b"post-boundary").unwrap();
        let mut stale = CipherState::new();
        stale.initialize_key([0xaa; 32]);
        // Advance the stale state's nonce to 1024 without rekeying.
        for _ in 0..REKEY_INTERVAL {
            let _ = stale.encrypt_with_ad(&[], b"advance");
        }
        assert_eq!(
            stale.decrypt_with_ad(&[], &ct),
            Err(NoiseError::DecryptFailed)
        );
        // The honest peer still reads it.
        assert_eq!(b.decrypt(&ct).unwrap(), b"post-boundary");
    }
}
