//! The link layer: PROTOCOL.md §4 driven over the M1 wire codec.
//!
//! Sans-I/O throughout — time arrives as `now_us` arguments (the same
//! virtual microseconds `sim::SimNet` uses) and bytes arrive from
//! whatever transport owns the socket. Nothing here does I/O, reads a
//! clock, or draws entropy.

mod driver;
mod limiter;

pub use driver::{CloseReason, LinkDriver, LinkDriverConfig, LinkError, LinkEvent, LinkOutput};
pub use limiter::{HandshakeLimiter, LimiterConfig};
