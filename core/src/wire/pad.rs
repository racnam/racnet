//! Fixed-block padding (PROTOCOL.md §1.3).
//!
//! Inner messages are padded to the smallest block that fits so that frame
//! sizes reveal only a coarse size class. Padding is part of the plaintext:
//! once the session layer lands, the Noise transport payload is the padded
//! inner message.

/// The fixed block sizes, in bytes. Sizes above the largest block round up to
/// the next multiple of it.
pub const BLOCK_SIZES: [usize; 4] = [256, 512, 1024, 2048];

/// Largest permitted padded inner message: 31 × 2048. Keeps the future Noise
/// ciphertext (padded plaintext + 16-byte AEAD tag) within the 65 535-byte
/// Noise message limit.
pub const MAX_PADDED_LEN: usize = 63_488;

/// Whether `len` is one of the permitted padded lengths of §1.3. The
/// transport-epoch pre-decryption gate (§1.1): a Noise message body must
/// be a permitted padded length plus the 16-octet tag.
pub fn is_padded_len(len: usize) -> bool {
    padded_len(len) == Some(len)
}

/// Returns the padded length for an inner message of `unpadded` bytes, or
/// `None` if it exceeds [`MAX_PADDED_LEN`].
pub fn padded_len(unpadded: usize) -> Option<usize> {
    for block in BLOCK_SIZES {
        if unpadded <= block {
            return Some(block);
        }
    }
    let last = BLOCK_SIZES[BLOCK_SIZES.len() - 1];
    let padded = unpadded.div_ceil(last) * last;
    (padded <= MAX_PADDED_LEN).then_some(padded)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn pads_to_smallest_fitting_block() {
        assert_eq!(padded_len(1), Some(256));
        assert_eq!(padded_len(256), Some(256));
        assert_eq!(padded_len(257), Some(512));
        assert_eq!(padded_len(512), Some(512));
        assert_eq!(padded_len(513), Some(1024));
        assert_eq!(padded_len(1024), Some(1024));
        assert_eq!(padded_len(1025), Some(2048));
        assert_eq!(padded_len(2048), Some(2048));
    }

    #[test]
    fn rounds_up_to_multiples_of_largest_block() {
        assert_eq!(padded_len(2049), Some(4096));
        assert_eq!(padded_len(4096), Some(4096));
        assert_eq!(padded_len(4097), Some(6144));
        assert_eq!(padded_len(MAX_PADDED_LEN), Some(MAX_PADDED_LEN));
    }

    #[test]
    fn rejects_oversized() {
        assert_eq!(padded_len(MAX_PADDED_LEN + 1), None);
    }

    #[test]
    fn recognizes_exact_padded_lengths() {
        for len in [256, 512, 1024, 2048, 4096, 6144, MAX_PADDED_LEN] {
            assert!(is_padded_len(len), "{len}");
        }
        for len in [0, 1, 255, 257, 511, 2049, 4095, MAX_PADDED_LEN + 2048] {
            assert!(!is_padded_len(len), "{len}");
        }
    }
}
