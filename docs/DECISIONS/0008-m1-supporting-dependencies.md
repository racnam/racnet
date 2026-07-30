# ADR-0008: Milestone 1 supporting dependencies

**Status:** accepted · **Date:** 2026-07-30

## Context

The M1 codec and test suite need error types, property testing, and hex
handling. Supply chain is part of the threat model: every mesh-core
dependency needs a recorded justification, and the preference is boring,
well-maintained crates — or none.

## Decision

Dependency justifications (mesh core): `thiserror` — the de facto standard
derive for typed error enums, no runtime cost. Dev-dependencies (not
shipped): `proptest` — property-based round-trip tests with good shrinking,
actively maintained; `hex` — conformance-vector encoding in tests.

The simulated transport's randomness (datagram loss) uses an in-tree
SplitMix64 generator — a public-domain, ~15-line algorithm — rather than
adding `rand` to the mesh core. Deterministic cross-platform replay from a
seed is a hard requirement for reproducible protocol tests, and a full RNG
framework is unwarranted supply-chain surface for one test harness.

## Consequences

- Shipped mesh-core dependency additions in M1: `minicbor` (ADR-0003),
  `sha2`, `ed25519-dalek` (ADR-0006), `thiserror`. Nothing else.
- Simulation runs are bit-reproducible from a seed on every platform.
- SplitMix64 is not cryptographic and is confined to test-harness use.

## Alternatives rejected

- **quickcheck:** effectively dormant; weaker shrinking than proptest.
- **rand / rand_chacha in the core:** framework-sized dependency for a
  deterministic test-harness RNG; also drags platform entropy plumbing
  toward the staticlib builds.
- **anyhow in the core:** boxes errors at the boundary where typed enums
  are wanted; fine for binaries, wrong for a library crate.
