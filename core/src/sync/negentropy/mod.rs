//! Negentropy V1, implemented in-house (ADR-0010).
//!
//! The encoding here is defined by the upstream Negentropy specification,
//! not by PROTOCOL.md: racnet carries these messages as opaque byte strings
//! (§6.3, ADR-0007), and byte-exactness is checked by running the upstream
//! conformance suite against this implementation
//! (`scripts/negentropy-conformance.sh`, pinned commit in ADR-0010).

pub mod bound;
pub mod fingerprint;
pub mod varint;
