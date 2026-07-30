//! Racnet core: protocol, sync, storage, crypto, and routing.
//!
//! The wire codec lives in [`wire`]; it implements `docs/PROTOCOL.md` v0.1.
//! Only [`version`] crosses the FFI boundary for now — the exported surface
//! will stabilize around session-level operations, not codecs.

pub mod sim;
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
