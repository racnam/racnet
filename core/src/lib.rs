//! Racnet core: protocol, sync, storage, crypto, and routing.
//!
//! Milestone 0 exposes only [`version`], proving the UniFFI toolchain across
//! iOS, Android, and the Linux anchor before any protocol work lands.

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
