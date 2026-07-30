//! Negentropy range fingerprints.
//!
//! The fingerprint of a range is the first 16 bytes of the SHA-256 of the
//! sum of its element ids — 32-byte little-endian integers added modulo
//! 2^256 — followed by the element count as a varint. The sum is a
//! commutative monoid, so subtree summaries compose (ADR-0011).

use sha2::{Digest, Sha256};

use crate::sync::negentropy::bound::ID_SIZE;
use crate::sync::negentropy::varint::write_varint;

/// Byte length of a range fingerprint on the wire.
pub const FINGERPRINT_SIZE: usize = 16;

/// A computed range fingerprint.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Fingerprint(
    /// The fingerprint bytes as they appear on the wire.
    pub [u8; FINGERPRINT_SIZE],
);

/// Sum of element ids modulo 2^256, in four little-endian u64 limbs.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct IdSum {
    limbs: [u64; 4],
}

impl IdSum {
    /// The sum of no ids.
    pub fn zero() -> IdSum {
        IdSum::default()
    }

    /// Adds one id, interpreted as a 32-byte little-endian integer.
    pub fn add_id(&mut self, id: &[u8; ID_SIZE]) {
        let mut other = [0u64; 4];
        for (limb, chunk) in other.iter_mut().zip(id.chunks_exact(8)) {
            *limb = u64::from_le_bytes(chunk.try_into().expect("8-byte chunk"));
        }
        self.add_limbs(&other);
    }

    /// Adds another sum: the monoid combine for composed range summaries.
    pub fn combine(&mut self, other: &IdSum) {
        self.add_limbs(&other.limbs);
    }

    fn add_limbs(&mut self, other: &[u64; 4]) {
        let mut carry = false;
        for (limb, &add) in self.limbs.iter_mut().zip(other.iter()) {
            let (sum, c1) = limb.overflowing_add(add);
            let (sum, c2) = sum.overflowing_add(u64::from(carry));
            *limb = sum;
            carry = c1 || c2;
        }
        // A final carry falls off the end: arithmetic is modulo 2^256.
    }

    /// Negates the sum modulo 2^256, so `a + (-b)` subtracts: summaries
    /// over `[i, j)` can be computed as prefix-sum differences.
    pub fn negate(&mut self) {
        for limb in self.limbs.iter_mut() {
            *limb = !*limb;
        }
        let mut one = [0u8; ID_SIZE];
        one[0] = 1;
        self.add_id(&one);
    }

    /// The sum as 32 little-endian bytes.
    pub fn to_le_bytes(self) -> [u8; ID_SIZE] {
        let mut out = [0u8; ID_SIZE];
        for (chunk, limb) in out.chunks_exact_mut(8).zip(self.limbs.iter()) {
            chunk.copy_from_slice(&limb.to_le_bytes());
        }
        out
    }

    /// The fingerprint of a range with this id sum and `count` elements.
    pub fn fingerprint(&self, count: u64) -> Fingerprint {
        let mut input = Vec::with_capacity(ID_SIZE + 10);
        input.extend_from_slice(&self.to_le_bytes());
        write_varint(count, &mut input);
        let digest = Sha256::digest(&input);
        let mut out = [0u8; FINGERPRINT_SIZE];
        out.copy_from_slice(&digest[..FINGERPRINT_SIZE]);
        Fingerprint(out)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn empty_fingerprint_hashes_zero_sum_and_zero_count() {
        let mut expected_input = [0u8; ID_SIZE + 1].to_vec();
        expected_input[ID_SIZE] = 0x00; // varint(0)
        let digest = Sha256::digest(&expected_input);
        assert_eq!(IdSum::zero().fingerprint(0).0, digest[..FINGERPRINT_SIZE]);
    }

    #[test]
    fn id_bytes_are_interpreted_little_endian() {
        let mut id = [0u8; ID_SIZE];
        id[0] = 2;
        let mut sum = IdSum::zero();
        sum.add_id(&id);
        assert_eq!(sum.limbs, [2, 0, 0, 0]);
        let mut hi = [0u8; ID_SIZE];
        hi[31] = 1;
        sum.add_id(&hi);
        assert_eq!(sum.limbs, [2, 0, 0, 1 << 56]);
    }

    #[test]
    fn addition_carries_across_limbs_and_wraps_mod_2_256() {
        let mut sum = IdSum::zero();
        sum.add_id(&[0xFF; ID_SIZE]); // 2^256 - 1
        let mut one = [0u8; ID_SIZE];
        one[0] = 1;
        sum.add_id(&one); // wraps to zero
        assert_eq!(sum, IdSum::zero());

        let mut sum = IdSum::zero();
        let mut low_max = [0u8; ID_SIZE];
        low_max[..8].copy_from_slice(&u64::MAX.to_le_bytes());
        sum.add_id(&low_max);
        sum.add_id(&one); // carries into the second limb
        assert_eq!(sum.limbs, [0, 1, 0, 0]);
    }

    #[test]
    fn combine_equals_adding_ids_directly() {
        let ids: Vec<[u8; ID_SIZE]> = (0u8..8).map(|b| [b.wrapping_mul(37); ID_SIZE]).collect();
        let mut all = IdSum::zero();
        for id in &ids {
            all.add_id(id);
        }
        let (left, right) = ids.split_at(3);
        let mut a = IdSum::zero();
        for id in left {
            a.add_id(id);
        }
        let mut b = IdSum::zero();
        for id in right {
            b.add_id(id);
        }
        a.combine(&b);
        assert_eq!(a, all);
        assert_eq!(a.fingerprint(8), all.fingerprint(8));
        assert_ne!(a.fingerprint(8), all.fingerprint(9));
    }
}
