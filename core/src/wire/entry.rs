//! Signed append-only entries: encoding, identity, signatures (PROTOCOL.md §3.5).
//!
//! Entries are a fixed positional CBOR array because they are hashed and
//! signed: positional encoding removes any key-ordering ambiguity from the
//! signing input. Identity and signatures are computed over the canonical
//! re-encoding of the decoded fields, never over raw received bytes, so a
//! non-canonical transmission cannot fork an entry's id.

use ed25519_dalek::{Signature, Signer, SigningKey, VerifyingKey};
use minicbor::Encode;
use sha2::{Digest, Sha256};

use super::error::WireError;

/// A signed append-only entry (PROTOCOL.md §3.5).
#[derive(Debug, Clone, PartialEq, Eq, Encode)]
#[cbor(array)]
pub struct Entry {
    /// Author's Ed25519 public key.
    #[n(0)]
    #[cbor(with = "minicbor::bytes")]
    pub author: [u8; 32],
    /// u64 sort key: milliseconds since the Unix epoch (PROTOCOL.md §6.1).
    #[n(1)]
    pub sort_key: u64,
    /// Application-level entry kind.
    #[n(2)]
    pub kind: u64,
    /// Application payload, opaque to the wire layer.
    #[n(3)]
    #[cbor(with = "minicbor::bytes")]
    pub payload: Vec<u8>,
    /// Ed25519 signature over the to-be-signed encoding.
    #[n(4)]
    #[cbor(with = "minicbor::bytes")]
    pub sig: [u8; 64],
}

/// Hand-written decoder: the derive would tolerate extra array elements,
/// but the spec requires rejecting any arity other than exactly 5
/// (PROTOCOL.md §3.1).
impl<'b, C> minicbor::Decode<'b, C> for Entry {
    fn decode(d: &mut minicbor::Decoder<'b>, ctx: &mut C) -> Result<Self, minicbor::decode::Error> {
        if d.array()? != Some(5) {
            return Err(minicbor::decode::Error::message(
                "entry is a fixed 5-element array",
            ));
        }
        Ok(Entry {
            author: minicbor::bytes::decode(d, ctx)?,
            sort_key: d.u64()?,
            kind: d.u64()?,
            payload: minicbor::bytes::decode(d, ctx)?,
            sig: minicbor::bytes::decode(d, ctx)?,
        })
    }
}

impl Entry {
    /// Creates and signs an entry.
    pub fn sign(key: &SigningKey, sort_key: u64, kind: u64, payload: Vec<u8>) -> Entry {
        let author = key.verifying_key().to_bytes();
        let tbs = tbs_bytes(&author, sort_key, kind, &payload);
        let sig = key.sign(&tbs).to_bytes();
        Entry {
            author,
            sort_key,
            kind,
            payload,
            sig,
        }
    }

    /// Verifies the entry's signature against its author key.
    pub fn verify(&self) -> Result<(), WireError> {
        let key = VerifyingKey::from_bytes(&self.author).map_err(|_| WireError::BadSignature)?;
        let sig = Signature::from_bytes(&self.sig);
        let tbs = self.tbs_bytes();
        key.verify_strict(&tbs, &sig)
            .map_err(|_| WireError::BadSignature)
    }

    /// The entry id: SHA-256 of the canonical entry encoding.
    pub fn id(&self) -> [u8; 32] {
        Sha256::digest(self.to_bytes()).into()
    }

    /// The canonical CBOR encoding of the full entry.
    pub fn to_bytes(&self) -> Vec<u8> {
        minicbor::to_vec(self).expect("Vec<u8> writes are infallible")
    }

    /// The to-be-signed encoding: the entry as a 4-element array without `sig`.
    pub fn tbs_bytes(&self) -> Vec<u8> {
        tbs_bytes(&self.author, self.sort_key, self.kind, &self.payload)
    }
}

fn tbs_bytes(author: &[u8; 32], sort_key: u64, kind: u64, payload: &[u8]) -> Vec<u8> {
    let mut enc = minicbor::Encoder::new(Vec::new());
    enc.array(4)
        .and_then(|e| e.bytes(author))
        .and_then(|e| e.u64(sort_key))
        .and_then(|e| e.u64(kind))
        .and_then(|e| e.bytes(payload))
        .expect("Vec<u8> writes are infallible");
    enc.into_writer()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn test_key() -> SigningKey {
        SigningKey::from_bytes(&[7u8; 32])
    }

    #[test]
    fn sign_then_verify_round_trips() {
        let entry = Entry::sign(&test_key(), 1_700_000_000_000, 0, b"payload".to_vec());
        entry.verify().expect("fresh signature verifies");
    }

    #[test]
    fn tampered_entry_fails_verification() {
        let mut entry = Entry::sign(&test_key(), 1_700_000_000_000, 0, b"payload".to_vec());
        entry.kind = 1;
        assert_eq!(entry.verify(), Err(WireError::BadSignature));
    }

    #[test]
    fn entry_arity_is_exactly_five() {
        let entry = Entry::sign(&test_key(), 1, 0, vec![]);
        let canonical = entry.to_bytes();
        assert_eq!(minicbor::decode::<Entry>(&canonical).unwrap(), entry);

        // Six elements: bump the array head and append one item.
        let mut extended = canonical.clone();
        extended[0] = 0x86;
        extended.push(0x00);
        assert!(minicbor::decode::<Entry>(&extended).is_err());

        // Four elements: the tbs encoding is not an entry.
        assert!(minicbor::decode::<Entry>(&entry.tbs_bytes()).is_err());
    }

    #[test]
    fn id_covers_signature() {
        let a = Entry::sign(&test_key(), 1, 0, vec![]);
        let mut b = a.clone();
        b.sig[0] ^= 1;
        assert_ne!(a.id(), b.id());
    }
}
