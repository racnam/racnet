//! Handshake rate limiting (PROTOCOL.md §4.5).
//!
//! Handshake DH operations are the cheapest denial-of-service lever
//! against a node anyone within radio range can reach. This limiter is
//! owned by whatever accepts transport connections — it spans links, so
//! it cannot live inside a link driver. Refusals are silent by spec:
//! `admit` returning `None` means drop the connection, send nothing.

use std::collections::HashMap;

/// §4.5 local-policy defaults, in the simulator's microsecond units.
#[derive(Debug, Clone)]
pub struct LimiterConfig {
    /// Token bucket capacity per remote address.
    pub burst: u32,
    /// One token refills per this many microseconds.
    pub refill_interval_us: u64,
    /// Cap on concurrent half-open handshakes.
    pub max_half_open: usize,
    /// A half-open handshake older than this has timed out.
    pub half_open_timeout_us: u64,
}

impl Default for LimiterConfig {
    fn default() -> LimiterConfig {
        LimiterConfig {
            burst: 3,
            refill_interval_us: 5_000_000,
            max_half_open: 32,
            half_open_timeout_us: 30_000_000,
        }
    }
}

#[derive(Debug)]
struct Bucket {
    tokens: u32,
    last_refill_us: u64,
}

/// Acceptor-side handshake gate: a token bucket per remote transport
/// address (opaque bytes — a BLE address later, a test label today) plus
/// a global half-open cap with timeouts. All time is injected.
pub struct HandshakeLimiter {
    config: LimiterConfig,
    buckets: HashMap<Vec<u8>, Bucket>,
    /// ticket → deadline_us
    half_open: HashMap<u64, u64>,
    next_ticket: u64,
}

impl HandshakeLimiter {
    pub fn new(config: LimiterConfig) -> HandshakeLimiter {
        HandshakeLimiter {
            config,
            buckets: HashMap::new(),
            half_open: HashMap::new(),
            next_ticket: 0,
        }
    }

    /// A remote address wants to start a handshake. `None` means refuse:
    /// drop the connection silently (§4.5). `Some(ticket)` admits it; the
    /// caller reports the outcome via [`established`](Self::established)
    /// or [`closed`](Self::closed), or lets [`expire`](Self::expire)
    /// collect it.
    pub fn admit(&mut self, remote_addr: &[u8], now_us: u64) -> Option<u64> {
        if self.half_open.len() >= self.config.max_half_open {
            return None;
        }

        let bucket = self.buckets.entry(remote_addr.to_vec()).or_insert(Bucket {
            tokens: self.config.burst,
            last_refill_us: now_us,
        });
        // Lazy refill.
        let elapsed = now_us.saturating_sub(bucket.last_refill_us);
        if let Some(refilled @ 1..) = elapsed.checked_div(self.config.refill_interval_us) {
            bucket.tokens = bucket
                .tokens
                .saturating_add(refilled.min(u64::from(self.config.burst)) as u32)
                .min(self.config.burst);
            bucket.last_refill_us += refilled * self.config.refill_interval_us;
        }
        if bucket.tokens == 0 {
            return None;
        }
        bucket.tokens -= 1;

        // A bucket that would be full after refill carries no state worth
        // keeping; collecting those here bounds the map so the limiter is
        // not itself a memory-DoS lever under address churn. Refill is
        // lazy, so fullness is computed against `now_us`, not stored
        // tokens.
        let burst = self.config.burst;
        let refill = self.config.refill_interval_us;
        self.buckets.retain(|_, b| {
            let deficit = u64::from(burst - b.tokens);
            now_us.saturating_sub(b.last_refill_us) < deficit.saturating_mul(refill)
        });

        let ticket = self.next_ticket;
        self.next_ticket += 1;
        self.half_open.insert(
            ticket,
            now_us.saturating_add(self.config.half_open_timeout_us),
        );
        Some(ticket)
    }

    /// The admitted handshake completed; it no longer counts as half-open.
    pub fn established(&mut self, ticket: u64) {
        self.half_open.remove(&ticket);
    }

    /// The admitted handshake's link closed before completing.
    pub fn closed(&mut self, ticket: u64) {
        self.half_open.remove(&ticket);
    }

    /// Tickets whose half-open deadline has passed. Each is returned once;
    /// the caller closes those links silently.
    pub fn expire(&mut self, now_us: u64) -> Vec<u64> {
        let expired: Vec<u64> = self
            .half_open
            .iter()
            .filter(|(_, &deadline)| deadline <= now_us)
            .map(|(&ticket, _)| ticket)
            .collect();
        for ticket in &expired {
            self.half_open.remove(ticket);
        }
        expired
    }

    /// Current number of half-open handshakes (test observability).
    pub fn half_open_count(&self) -> usize {
        self.half_open.len()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const SEC: u64 = 1_000_000;

    fn limiter() -> HandshakeLimiter {
        HandshakeLimiter::new(LimiterConfig::default())
    }

    #[test]
    fn burst_of_three_then_refill_one_per_five_seconds() {
        let mut l = limiter();
        let addr = b"peer-a";
        let t0 = 100 * SEC;
        for _ in 0..3 {
            assert!(l.admit(addr, t0).is_some());
        }
        assert!(l.admit(addr, t0).is_none(), "burst exhausted");
        assert!(l.admit(addr, t0 + 4 * SEC).is_none(), "not yet refilled");
        assert!(l.admit(addr, t0 + 5 * SEC).is_some(), "one token back");
        assert!(l.admit(addr, t0 + 5 * SEC).is_none(), "only one");
        // Refill never exceeds the burst.
        assert!(l.admit(addr, t0 + 500 * SEC).is_some());
        assert!(l.admit(addr, t0 + 500 * SEC).is_some());
        assert!(l.admit(addr, t0 + 500 * SEC).is_some());
        assert!(l.admit(addr, t0 + 500 * SEC).is_none());
    }

    #[test]
    fn addresses_have_independent_buckets() {
        let mut l = limiter();
        for _ in 0..3 {
            assert!(l.admit(b"a", 0).is_some());
        }
        assert!(l.admit(b"a", 0).is_none());
        assert!(l.admit(b"b", 0).is_some());
    }

    #[test]
    fn half_open_cap_of_32_rejects_the_33rd() {
        let mut l = limiter();
        for i in 0..32u32 {
            // Distinct addresses so the per-address bucket never trips.
            assert!(l.admit(&i.to_be_bytes(), 0).is_some());
        }
        assert_eq!(l.half_open_count(), 32);
        assert!(l.admit(b"one-more", 0).is_none());
        // Completing one frees a slot.
        l.established(0);
        assert!(l.admit(b"one-more", 0).is_some());
    }

    #[test]
    fn expired_half_open_handshakes_are_reported_once() {
        let mut l = limiter();
        let t1 = l.admit(b"a", 0).unwrap();
        let t2 = l.admit(b"b", SEC).unwrap();
        assert!(l.expire(29 * SEC).is_empty());
        let expired = l.expire(30 * SEC);
        assert_eq!(expired, vec![t1]);
        assert!(l.expire(30 * SEC).is_empty(), "reported once");
        let expired = l.expire(31 * SEC);
        assert_eq!(expired, vec![t2]);
        assert_eq!(l.half_open_count(), 0);
    }

    #[test]
    fn closed_frees_the_half_open_slot() {
        let mut l = limiter();
        let t = l.admit(b"a", 0).unwrap();
        assert_eq!(l.half_open_count(), 1);
        l.closed(t);
        assert_eq!(l.half_open_count(), 0);
    }

    #[test]
    fn full_buckets_are_garbage_collected() {
        let mut l = limiter();
        // Churn through many addresses, each admitted once, then let time
        // pass so every bucket refills to full and is collected on the
        // next admit.
        for i in 0..1000u32 {
            assert!(l.admit(&i.to_be_bytes(), u64::from(i) * SEC).is_some());
            l.closed(u64::from(i));
        }
        // Long after every bucket has refilled, one more admission
        // triggers collection of the rest.
        let _ = l.admit(b"final", 10_000 * SEC);
        assert!(
            l.buckets.len() <= 2,
            "bucket map must stay bounded, found {}",
            l.buckets.len()
        );
    }
}
