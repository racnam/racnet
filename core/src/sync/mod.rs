//! Set reconciliation (PROTOCOL.md §6).
//!
//! Racnet reconciles entry sets with Negentropy V1, carried opaquely inside
//! RECON_INIT and RECON_MSG payloads (ADR-0007). The [`negentropy`] module
//! implements the Negentropy wire protocol itself (ADR-0010); its
//! conformance surface is the upstream test suite, not this spec.

pub mod error;
pub mod negentropy;

pub use error::SyncError;
