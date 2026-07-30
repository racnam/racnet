//! Negentropy varint codec.
//!
//! Base-128 digits, most significant digit first, with as few digits as
//! possible; bit eight is set on every byte except the last. Matches the
//! upstream Negentropy V1 encoding byte for byte.

use crate::sync::SyncError;

/// Appends the varint encoding of `n` to `out`.
pub fn write_varint(n: u64, out: &mut Vec<u8>) {
    if n == 0 {
        out.push(0);
        return;
    }
    let digits = (64 - n.leading_zeros() as usize).div_ceil(7);
    for i in (0..digits).rev() {
        let digit = ((n >> (7 * i)) & 0x7F) as u8;
        out.push(if i == 0 { digit } else { digit | 0x80 });
    }
}

/// Decodes a varint from the front of `input`, advancing the slice.
///
/// Mirrors the upstream decoder's tolerance of redundant leading zero
/// digits, but rejects values that overflow u64 — the reference
/// implementations silently wrap or lose precision there, which a parser
/// of untrusted bytes must not do.
pub fn read_varint(input: &mut &[u8]) -> Result<u64, SyncError> {
    let mut res: u64 = 0;
    loop {
        let (&byte, rest) = input
            .split_first()
            .ok_or(SyncError::Malformed("varint ends prematurely"))?;
        *input = rest;
        if res > u64::MAX >> 7 {
            return Err(SyncError::Malformed("varint overflows u64"));
        }
        res = (res << 7) | u64::from(byte & 0x7F);
        if byte & 0x80 == 0 {
            return Ok(res);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn enc(n: u64) -> Vec<u8> {
        let mut out = Vec::new();
        write_varint(n, &mut out);
        out
    }

    #[test]
    fn encoding_vectors() {
        assert_eq!(enc(0), [0x00]);
        assert_eq!(enc(1), [0x01]);
        assert_eq!(enc(127), [0x7F]);
        assert_eq!(enc(128), [0x81, 0x00]);
        assert_eq!(enc(16_383), [0xFF, 0x7F]);
        assert_eq!(enc(16_384), [0x81, 0x80, 0x00]);
        assert_eq!(
            enc(u64::MAX),
            [0x81, 0xFF, 0xFF, 0xFF, 0xFF, 0xFF, 0xFF, 0xFF, 0xFF, 0x7F]
        );
    }

    #[test]
    fn round_trips() {
        for n in [
            0,
            1,
            127,
            128,
            255,
            300,
            16_383,
            16_384,
            u64::MAX / 2,
            u64::MAX,
        ] {
            let bytes = enc(n);
            let mut cursor = bytes.as_slice();
            assert_eq!(read_varint(&mut cursor), Ok(n));
            assert!(cursor.is_empty());
        }
    }

    #[test]
    fn decoder_advances_past_the_varint_only() {
        let bytes = [0x81, 0x00, 0xAB];
        let mut cursor = bytes.as_slice();
        assert_eq!(read_varint(&mut cursor), Ok(128));
        assert_eq!(cursor, [0xAB]);
    }

    #[test]
    fn redundant_leading_zero_digits_are_tolerated() {
        // Non-minimal but decodable, as in the upstream decoder.
        let bytes = [0x80, 0x80, 0x01];
        let mut cursor = bytes.as_slice();
        assert_eq!(read_varint(&mut cursor), Ok(1));
    }

    #[test]
    fn truncation_is_an_error() {
        let bytes = [0x81];
        let mut cursor = bytes.as_slice();
        assert_eq!(
            read_varint(&mut cursor),
            Err(SyncError::Malformed("varint ends prematurely"))
        );
        assert_eq!(
            read_varint(&mut &[][..]),
            Err(SyncError::Malformed("varint ends prematurely"))
        );
    }

    #[test]
    fn overflow_is_an_error() {
        // Eleven meaningful digits: 2^70 - 1.
        let bytes = [0xFF; 10];
        let mut cursor = &bytes[..];
        assert_eq!(
            read_varint(&mut cursor),
            Err(SyncError::Malformed("varint overflows u64"))
        );
    }
}
