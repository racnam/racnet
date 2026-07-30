//! In-house Noise XX engine for the session layer of `docs/PROTOCOL.md` §4.
//!
//! Implements exactly one protocol, `Noise_XX_25519_ChaChaPoly_SHA256`
//! (Noise spec revision 34), over primitive crates (ADR-0012). All key
//! material is injected by the caller — [`Keypair::generate`] is the only
//! entropy call in this crate — so handshakes are deterministic wherever
//! tests need them to be.

mod cipher;
mod dh;
mod error;
mod handshake;
mod symmetric;
mod transport;

pub use dh::{Fingerprint, Keypair, PublicKey, SecretKey};
pub use error::NoiseError;
pub use handshake::{HandshakeState, Role};
pub use transport::{TransportState, REKEY_INTERVAL};

pub(crate) use cipher::CipherState;

/// The full Noise protocol name. Exactly 32 octets, so `SymmetricState`
/// initializes its hash to the name directly rather than hashing it.
pub const PROTOCOL_NAME: &[u8; 32] = b"Noise_XX_25519_ChaChaPoly_SHA256";
