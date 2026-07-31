//! Racnet core: protocol, sync, storage, crypto, and routing.
//!
//! The wire codec lives in [`wire`]; it implements `docs/PROTOCOL.md` v0.2.0.
//! The FFI boundary is [`api`]: session-level operations behind a single
//! [`api::Node`] facade (ADR-0014), never codecs or key types.

pub mod api;
pub mod link;
pub mod noise;
#[cfg(feature = "sim")]
pub mod sim;
pub mod store;
pub mod sync;
pub mod wire;

uniffi::setup_scaffolding!();

/// Returns the racnet-core crate version.
#[uniffi::export]
pub fn version() -> String {
    env!("CARGO_PKG_VERSION").to_string()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn version_matches_crate_metadata() {
        assert_eq!(version(), env!("CARGO_PKG_VERSION"));
    }
}
