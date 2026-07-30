//! Noise SymmetricState and the two-output HKDF (revision 34 §5.2, §4.3).

use hmac::digest::KeyInit;
use hmac::{Hmac, Mac};
use sha2::{Digest, Sha256};
use zeroize::Zeroize;

use super::{CipherState, NoiseError, PROTOCOL_NAME};

type HmacSha256 = Hmac<Sha256>;

fn hmac(key: &[u8; 32], data: &[&[u8]]) -> [u8; 32] {
    let mut mac = HmacSha256::new_from_slice(key).expect("HMAC accepts any key length");
    for part in data {
        mac.update(part);
    }
    mac.finalize().into_bytes().into()
}

/// Noise HKDF with two outputs (revision 34 §4.3). XX with no PSK never
/// needs the three-output form, so it is deliberately not implemented.
pub(crate) fn hkdf2(chaining_key: &[u8; 32], ikm: &[u8]) -> ([u8; 32], [u8; 32]) {
    let temp = hmac(chaining_key, &[ikm]);
    let out1 = hmac(&temp, &[&[0x01]]);
    let out2 = hmac(&temp, &[&out1, &[0x02]]);
    (out1, out2)
}

pub(crate) struct SymmetricState {
    ck: [u8; 32],
    h: [u8; 32],
    cipher: CipherState,
}

impl Drop for SymmetricState {
    fn drop(&mut self) {
        self.ck.zeroize();
    }
}

impl SymmetricState {
    /// InitializeSymmetric. The protocol name is exactly 32 octets
    /// (asserted in tests), so `h` starts as the name itself; the
    /// hash-if-longer branch is kept so the code matches the Noise spec
    /// rather than a lucky constant.
    pub(crate) fn initialize(protocol_name: &[u8]) -> SymmetricState {
        let h: [u8; 32] = if protocol_name.len() <= 32 {
            let mut h = [0u8; 32];
            h[..protocol_name.len()].copy_from_slice(protocol_name);
            h
        } else {
            Sha256::digest(protocol_name).into()
        };
        SymmetricState {
            ck: h,
            h,
            cipher: CipherState::new(),
        }
    }

    pub(crate) fn mix_hash(&mut self, data: &[u8]) {
        let mut hasher = Sha256::new();
        hasher.update(self.h);
        hasher.update(data);
        self.h = hasher.finalize().into();
    }

    pub(crate) fn mix_key(&mut self, ikm: &[u8]) {
        let (ck, temp_k) = hkdf2(&self.ck, ikm);
        self.ck.zeroize();
        self.ck = ck;
        self.cipher.initialize_key(temp_k);
    }

    /// The running handshake hash.
    pub(crate) fn hash(&self) -> [u8; 32] {
        self.h
    }

    /// EncryptAndHash: AD is the running hash; the ciphertext is mixed in.
    pub(crate) fn encrypt_and_hash(&mut self, plaintext: &[u8]) -> Result<Vec<u8>, NoiseError> {
        let h = self.h;
        let ciphertext = self.cipher.encrypt_with_ad(&h, plaintext)?;
        self.mix_hash(&ciphertext);
        Ok(ciphertext)
    }

    /// DecryptAndHash. The ciphertext is mixed into the hash only after
    /// it authenticates.
    pub(crate) fn decrypt_and_hash(&mut self, ciphertext: &[u8]) -> Result<Vec<u8>, NoiseError> {
        let h = self.h;
        let plaintext = self.cipher.decrypt_with_ad(&h, ciphertext)?;
        self.mix_hash(ciphertext);
        Ok(plaintext)
    }

    /// Split: the two transport CipherStates plus the handshake hash
    /// (channel binding).
    pub(crate) fn split(self) -> (CipherState, CipherState, [u8; 32]) {
        let (temp_k1, temp_k2) = hkdf2(&self.ck, &[]);
        let mut c1 = CipherState::new();
        let mut c2 = CipherState::new();
        c1.initialize_key(temp_k1);
        c2.initialize_key(temp_k2);
        (c1, c2, self.h)
    }
}

impl SymmetricState {
    pub(crate) fn new_for_protocol() -> SymmetricState {
        SymmetricState::initialize(PROTOCOL_NAME)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn protocol_name_is_exactly_32_octets() {
        assert_eq!(PROTOCOL_NAME.len(), 32);
        let s = SymmetricState::new_for_protocol();
        assert_eq!(&s.h, PROTOCOL_NAME);
        assert_eq!(s.ck, s.h);
    }

    #[test]
    fn long_protocol_names_are_hashed() {
        let name = b"Noise_XXfallback+psk0_25519_ChaChaPoly_BLAKE2s"; // > 32 octets
        let s = SymmetricState::initialize(name);
        let expected: [u8; 32] = Sha256::digest(name).into();
        assert_eq!(s.h, expected);
    }

    #[test]
    fn hkdf2_matches_a_manual_expansion() {
        // Independent recomputation of the two-output Noise HKDF from its
        // definition: temp = HMAC(ck, ikm); o1 = HMAC(temp, 0x01);
        // o2 = HMAC(temp, o1 || 0x02).
        let ck = [0xab; 32];
        let ikm = [0xcd; 32];
        let (o1, o2) = hkdf2(&ck, &ikm);
        let temp = hmac(&ck, &[&ikm]);
        assert_eq!(o1, hmac(&temp, &[&[0x01u8][..]]));
        assert_eq!(o2, hmac(&temp, &[&o1[..], &[0x02u8][..]]));
        assert_ne!(o1, o2);
    }

    #[test]
    fn encrypt_and_hash_round_trips_between_mirrored_states() {
        let mut a = SymmetricState::new_for_protocol();
        let mut b = SymmetricState::new_for_protocol();
        for s in [&mut a, &mut b] {
            s.mix_hash(b"prologue");
            s.mix_key(b"shared secret material");
        }
        let ct = a.encrypt_and_hash(b"payload").unwrap();
        assert_ne!(ct, b"payload");
        assert_eq!(b.decrypt_and_hash(&ct).unwrap(), b"payload");
        assert_eq!(a.h, b.h);
    }

    #[test]
    fn split_produces_mirrored_transport_keys() {
        let mut a = SymmetricState::new_for_protocol();
        let mut b = SymmetricState::new_for_protocol();
        for s in [&mut a, &mut b] {
            s.mix_key(b"ikm");
        }
        let (mut a1, mut a2, ha) = a.split();
        let (mut b1, mut b2, hb) = b.split();
        assert_eq!(ha, hb);
        let ct = a1.encrypt_with_ad(&[], b"one way").unwrap();
        assert_eq!(b1.decrypt_with_ad(&[], &ct).unwrap(), b"one way");
        let ct = b2.encrypt_with_ad(&[], b"other way").unwrap();
        assert_eq!(a2.decrypt_with_ad(&[], &ct).unwrap(), b"other way");
    }
}
