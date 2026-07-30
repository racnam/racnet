//! Session-crypto errors.
//!
//! Deliberately detail-free: PROTOCOL.md §4.4 and §7 require every
//! cryptographic failure to close the link silently, so nothing here may
//! carry information that could become an oracle if it ever leaked into
//! wire-visible behavior.

/// An error from the Noise engine. Every variant is terminal for the
/// handshake or session that produced it.
#[derive(Debug, Clone, Copy, PartialEq, Eq, thiserror::Error)]
pub enum NoiseError {
    /// AEAD decryption or authentication failed.
    #[error("decryption failed")]
    DecryptFailed,
    /// A handshake message did not have the exact length its pattern step
    /// requires.
    #[error("malformed handshake message")]
    BadMessage,
    /// The nonce reached `2^64 - 1`, which Noise reserves for `Rekey`.
    #[error("nonce space exhausted")]
    NonceExhausted,
    /// Local API misuse: a read where a write was expected, a message
    /// after the handshake finished, or a conversion before it did.
    #[error("handshake state misuse")]
    OutOfOrder,
    /// Platform entropy was unavailable.
    #[error("system entropy unavailable")]
    Entropy,
}
