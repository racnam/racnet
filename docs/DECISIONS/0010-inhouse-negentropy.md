# ADR-0010: In-house implementation of Negentropy V1

**Status:** accepted · **Date:** 2026-07-30

## Context

M2 needs a working Negentropy V1 engine behind the opaque byte strings of
ADR-0007. A third-party Rust implementation exists (the `negentropy` crate
used by the nostr ecosystem). The engine parses untrusted bytes arriving
from strangers' radios, which is the code this project's threat model
(ADR-0009) cares most about, and every mesh-core dependency is a supply
chain exposure.

## Decision

Implement Negentropy V1 in `racnet-core` (`core/src/sync/negentropy/`), no
new dependencies: the protocol is small (varints, bounds, three range
modes, a SHA-256 fingerprint the existing `sha2` covers), and correctness
is checked against the upstream conformance suite rather than asserted.
`scripts/negentropy-conformance.sh` pins upstream `hoytech/negentropy` at
commit `76f3cf6e69be505e7295edb08a6152fce30261f1` and drives its Perl
harness against our implementation (rust↔rust interop and delta tests,
rust↔js interop, protocol-version negotiation, time-boxed fuzzing) via
`core/examples/negentropy_harness.rs`, locally and in CI.

The engine keeps the upstream 4096-byte frame-size-limit floor. A lower
floor was considered so sessions could target small padded blocks
(ADR-0005), but it breaks termination, which is why upstream floors where
it does: progress requires that the budget remaining at the start of a
message always fits one full range split (an id list of 31 ids runs to
~1 KB) — under a smaller budget that range is discarded every round and
reconciliation livelocks. Property tests demonstrated exactly that.
Reconciliation messages near the limit therefore pad to 4096-byte frames
(a valid ADR-0005 padded length) or the next block above it, rather
than 2048.

## Consequences

- The untrusted-input parser stays under this repo's review, lint, fuzz,
  and no-panic discipline instead of a third party's release cadence.
- We owe byte-exact fidelity to the upstream encoding; the pinned
  conformance suite, not code review, is the arbiter.
- Conformance runs need network access (a clone of the pinned commit) and
  node for the cross-implementation tests; they run as a separate CI job
  so `cargo test` stays hermetic.

## Alternatives rejected

- **The `negentropy` crate:** adds a third-party parser of hostile input
  to the mesh core for code we can write and verify in a few hundred
  lines; its release cadence and transitive surface become our exposure.
- **Treating our own vectors as the conformance surface:** vectors we
  generate can only prove self-consistency; interop against the reference
  implementations is what ADR-0007 bought.
