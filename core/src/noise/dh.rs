//! X25519 key material and the peer fingerprint.
//!
//! X25519 comes from `curve25519-dalek` directly: `mul_clamped` and
//! `mul_base_clamped` are the whole function, clamping included
//! (ADR-0012). The Ed25519 keys that sign entries (§3.5) are a different
//! type in a different module with no conversion in either direction —
//! the spec keeps the two identities unlinked, and so does the type
//! system.

use curve25519_dalek::montgomery::MontgomeryPoint;
use sha2::{Digest, Sha256};
use zeroize::Zeroize;

use super::NoiseError;

/// An X25519 secret key. Not `Clone`, not `Copy`; zeroized on drop.
pub struct SecretKey([u8; 32]);

impl Drop for SecretKey {
    fn drop(&mut self) {
        self.0.zeroize();
    }
}

impl std::fmt::Debug for SecretKey {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str("SecretKey(..)")
    }
}

/// An X25519 public key.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct PublicKey(pub [u8; 32]);

/// A static or ephemeral X25519 keypair. An ephemeral is a keypair used
/// for exactly one handshake.
#[derive(Debug)]
pub struct Keypair {
    secret: SecretKey,
    pub public: PublicKey,
}

impl Keypair {
    /// Deterministic construction from raw secret bytes (clamped on use).
    /// This is how tests, vectors, and the simulator stay reproducible.
    pub fn from_secret_bytes(bytes: [u8; 32]) -> Keypair {
        let public = PublicKey(MontgomeryPoint::mul_base_clamped(bytes).to_bytes());
        Keypair {
            secret: SecretKey(bytes),
            public,
        }
    }

    /// Generate from platform entropy — the only entropy call in this
    /// crate. Production transports use this; tests inject fixed bytes.
    pub fn generate() -> Result<Keypair, NoiseError> {
        let mut bytes = [0u8; 32];
        getrandom::fill(&mut bytes).map_err(|_| NoiseError::Entropy)?;
        let pair = Keypair::from_secret_bytes(bytes);
        bytes.zeroize();
        Ok(pair)
    }

    /// X25519(secret, peer). Per Noise revision 34 §12.1 the all-zero
    /// output is deliberately not rejected here.
    pub(crate) fn dh(&self, peer: &PublicKey) -> [u8; 32] {
        MontgomeryPoint(peer.0)
            .mul_clamped(self.secret.0)
            .to_bytes()
    }
}

/// A peer identity fingerprint: SHA-256 of the static public key (§4).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct Fingerprint(pub [u8; 32]);

impl Fingerprint {
    pub fn of(public: &PublicKey) -> Fingerprint {
        Fingerprint(Sha256::digest(public.0).into())
    }
}

impl std::fmt::Display for Fingerprint {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        for byte in self.0 {
            write!(f, "{byte:02x}")?;
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn keypair_matches_the_spec_vectors() {
        // PROTOCOL.md §8.7 static keys.
        let initiator = Keypair::from_secret_bytes([0x11; 32]);
        let responder = Keypair::from_secret_bytes([0x22; 32]);
        assert_eq!(
            hex::encode(initiator.public.0),
            "7b4e909bbe7ffe44c465a220037d608ee35897d31ef972f07f74892cb0f73f13"
        );
        assert_eq!(
            hex::encode(responder.public.0),
            "0faa684ed28867b97f4a6a2dee5df8ce974e76b7018e3f22a1c4cf2678570f20"
        );
    }

    #[test]
    fn dh_is_symmetric() {
        let a = Keypair::from_secret_bytes([0x11; 32]);
        let b = Keypair::from_secret_bytes([0x22; 32]);
        assert_eq!(a.dh(&b.public), b.dh(&a.public));
        assert_ne!(a.dh(&b.public), [0u8; 32]);
    }

    #[test]
    fn rfc7748_key_agreement_vectors() {
        // RFC 7748 §6.1: Alice and Bob's keys and their shared secret.
        let mut alice_secret = [0u8; 32];
        let mut bob_secret = [0u8; 32];
        hex::decode_to_slice(
            "77076d0a7318a57d3c16c17251b26645df4c2f87ebc0992ab177fba51db92c2a",
            &mut alice_secret,
        )
        .unwrap();
        hex::decode_to_slice(
            "5dab087e624a8a4b79e17f8b83800ee66f3bb1292618b6fd1c2f8b27ff88e0eb",
            &mut bob_secret,
        )
        .unwrap();
        let alice = Keypair::from_secret_bytes(alice_secret);
        let bob = Keypair::from_secret_bytes(bob_secret);
        assert_eq!(
            hex::encode(alice.public.0),
            "8520f0098930a754748b7ddcb43ef75a0dbf3a0d26381af4eba4a98eaa9b4e6a"
        );
        assert_eq!(
            hex::encode(bob.public.0),
            "de9edb7d7b7dc1b4d35b61c2ece435373f8343c85b78674dadfc7e146f882b4f"
        );
        assert_eq!(
            hex::encode(alice.dh(&bob.public)),
            "4a5d9d5ba4ce2de1728e3bf480350f25e07e21c947d19e3376f09b3c1e161742"
        );
    }

    #[test]
    fn fingerprint_is_sha256_of_the_public_key() {
        let key = Keypair::from_secret_bytes([0x11; 32]);
        let expected: [u8; 32] = Sha256::digest(key.public.0).into();
        assert_eq!(Fingerprint::of(&key.public).0, expected);
        assert_eq!(
            Fingerprint::of(&key.public).to_string(),
            hex::encode(expected)
        );
    }

    #[test]
    fn generate_produces_distinct_keys() {
        let a = Keypair::generate().unwrap();
        let b = Keypair::generate().unwrap();
        assert_ne!(a.public, b.public);
    }

    #[test]
    fn secret_key_debug_hides_contents() {
        let key = Keypair::from_secret_bytes([0x7f; 32]);
        assert_eq!(format!("{:?}", key.secret), "SecretKey(..)");
    }
}
