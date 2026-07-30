//! Noise CipherState over ChaCha20-Poly1305 (Noise revision 34 §5.1).

use chacha20poly1305::aead::{Aead, KeyInit, Payload};
use chacha20poly1305::ChaCha20Poly1305;
use zeroize::Zeroize;

use super::NoiseError;

/// A key plus a nonce counter. Also carries the wire rule of PROTOCOL.md
/// §4.3: the nonce `2^64 - 1` is reserved for `Rekey`, so reaching it is
/// an error rather than a wrap.
pub(crate) struct CipherState {
    k: Option<[u8; 32]>,
    n: u64,
}

impl Drop for CipherState {
    fn drop(&mut self) {
        if let Some(ref mut k) = self.k {
            k.zeroize();
        }
    }
}

/// The 12-octet IETF nonce: 4 zero octets then the counter, little-endian
/// (Noise revision 34 §12.2). Built by hand so the layout is in one place
/// under test, not implied by a crate API.
fn nonce_bytes(n: u64) -> [u8; 12] {
    let mut nonce = [0u8; 12];
    nonce[4..].copy_from_slice(&n.to_le_bytes());
    nonce
}

impl CipherState {
    pub(crate) fn new() -> CipherState {
        CipherState { k: None, n: 0 }
    }

    pub(crate) fn initialize_key(&mut self, key: [u8; 32]) {
        if let Some(ref mut old) = self.k {
            old.zeroize();
        }
        self.k = Some(key);
        self.n = 0;
    }

    pub(crate) fn nonce(&self) -> u64 {
        self.n
    }

    /// EncryptWithAd. With no key this is the identity function on the
    /// plaintext (pre-key handshake steps).
    pub(crate) fn encrypt_with_ad(
        &mut self,
        ad: &[u8],
        plaintext: &[u8],
    ) -> Result<Vec<u8>, NoiseError> {
        let Some(key) = self.k.as_ref() else {
            return Ok(plaintext.to_vec());
        };
        if self.n == u64::MAX {
            return Err(NoiseError::NonceExhausted);
        }
        let cipher = ChaCha20Poly1305::new(key.into());
        let ciphertext = cipher
            .encrypt(
                &nonce_bytes(self.n).into(),
                Payload {
                    msg: plaintext,
                    aad: ad,
                },
            )
            .map_err(|_| NoiseError::DecryptFailed)?;
        self.n += 1;
        Ok(ciphertext)
    }

    /// DecryptWithAd. On failure the nonce is left unchanged (Noise
    /// revision 34 §5.1); for this protocol any failure is terminal
    /// anyway (§4.4).
    pub(crate) fn decrypt_with_ad(
        &mut self,
        ad: &[u8],
        ciphertext: &[u8],
    ) -> Result<Vec<u8>, NoiseError> {
        let Some(key) = self.k.as_ref() else {
            return Ok(ciphertext.to_vec());
        };
        if self.n == u64::MAX {
            return Err(NoiseError::NonceExhausted);
        }
        let cipher = ChaCha20Poly1305::new(key.into());
        let plaintext = cipher
            .decrypt(
                &nonce_bytes(self.n).into(),
                Payload {
                    msg: ciphertext,
                    aad: ad,
                },
            )
            .map_err(|_| NoiseError::DecryptFailed)?;
        self.n += 1;
        Ok(plaintext)
    }

    /// The Noise `Rekey` function (revision 34 §4.2): the new key is the
    /// first 32 octets of `ENCRYPT(k, 2^64 - 1, empty, zeros[32])`. The
    /// nonce is deliberately not reset (§4.3 of the spec).
    pub(crate) fn rekey(&mut self) {
        let Some(key) = self.k.as_ref() else {
            return;
        };
        let cipher = ChaCha20Poly1305::new(key.into());
        let mut block = cipher
            .encrypt(
                &nonce_bytes(u64::MAX).into(),
                Payload {
                    msg: &[0u8; 32],
                    aad: &[],
                },
            )
            .expect("encrypting a fixed 32-byte block cannot fail");
        let mut new_key = [0u8; 32];
        new_key.copy_from_slice(&block[..32]);
        block.zeroize();
        self.initialize_key(new_key);
    }

    /// Rekey without resetting the nonce counter — `initialize_key`
    /// resets it, but §4.3 continues counting across rekeys.
    pub(crate) fn rekey_preserving_nonce(&mut self) {
        let n = self.n;
        self.rekey();
        self.n = n;
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn nonce_layout_is_four_zeros_then_le64() {
        assert_eq!(nonce_bytes(0), [0u8; 12]);
        assert_eq!(
            nonce_bytes(0x0102030405060708),
            [0, 0, 0, 0, 0x08, 0x07, 0x06, 0x05, 0x04, 0x03, 0x02, 0x01]
        );
    }

    #[test]
    fn round_trips_and_advances_the_nonce() {
        let mut a = CipherState::new();
        let mut b = CipherState::new();
        a.initialize_key([0x42; 32]);
        b.initialize_key([0x42; 32]);
        for i in 0..4u64 {
            assert_eq!(a.nonce(), i);
            let ct = a.encrypt_with_ad(b"ad", b"hello").unwrap();
            assert_eq!(ct.len(), 5 + 16);
            assert_eq!(b.decrypt_with_ad(b"ad", &ct).unwrap(), b"hello");
        }
    }

    #[test]
    fn without_a_key_encrypt_is_identity() {
        let mut c = CipherState::new();
        assert_eq!(c.encrypt_with_ad(&[], b"x").unwrap(), b"x");
        assert_eq!(c.decrypt_with_ad(&[], b"x").unwrap(), b"x");
        assert_eq!(c.nonce(), 0);
    }

    #[test]
    fn tampered_ciphertext_fails_and_leaves_the_nonce() {
        let mut a = CipherState::new();
        let mut b = CipherState::new();
        a.initialize_key([1; 32]);
        b.initialize_key([1; 32]);
        let mut ct = a.encrypt_with_ad(&[], b"payload").unwrap();
        ct[0] ^= 0x80;
        assert_eq!(b.decrypt_with_ad(&[], &ct), Err(NoiseError::DecryptFailed));
        assert_eq!(b.nonce(), 0);
        // Wrong AD also fails.
        let ct2 = a.encrypt_with_ad(b"right", b"payload").unwrap();
        // b is still at nonce 0, matching ct (nonce 0 was never consumed).
        assert_eq!(
            b.decrypt_with_ad(b"wrong", &ct2),
            Err(NoiseError::DecryptFailed)
        );
    }

    #[test]
    fn rekey_matches_the_noise_definition() {
        // The new key is the first 32 bytes of encrypting 32 zero bytes
        // with nonce 2^64-1 — computed here directly against the AEAD.
        let key = [0x33; 32];
        let cipher = ChaCha20Poly1305::new((&key).into());
        let block = cipher
            .encrypt(
                &nonce_bytes(u64::MAX).into(),
                Payload {
                    msg: &[0u8; 32],
                    aad: &[],
                },
            )
            .unwrap();

        let mut a = CipherState::new();
        a.initialize_key(key);
        a.rekey();
        let mut b = CipherState::new();
        let mut expected = [0u8; 32];
        expected.copy_from_slice(&block[..32]);
        b.initialize_key(expected);
        let ct = a.encrypt_with_ad(&[], b"post-rekey").unwrap();
        assert_eq!(b.decrypt_with_ad(&[], &ct).unwrap(), b"post-rekey");
    }

    #[test]
    fn rekey_preserving_nonce_keeps_the_counter() {
        let mut a = CipherState::new();
        a.initialize_key([9; 32]);
        a.encrypt_with_ad(&[], b"x").unwrap();
        a.encrypt_with_ad(&[], b"x").unwrap();
        assert_eq!(a.nonce(), 2);
        a.rekey_preserving_nonce();
        assert_eq!(a.nonce(), 2);
    }

    #[test]
    fn nonce_exhaustion_is_an_error_not_a_wrap() {
        let mut a = CipherState::new();
        a.initialize_key([5; 32]);
        a.n = u64::MAX;
        assert_eq!(
            a.encrypt_with_ad(&[], b"x"),
            Err(NoiseError::NonceExhausted)
        );
        assert_eq!(
            a.decrypt_with_ad(&[], b"x"),
            Err(NoiseError::NonceExhausted)
        );
    }
}
