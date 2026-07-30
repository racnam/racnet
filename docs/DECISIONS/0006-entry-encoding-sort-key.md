# ADR-0006: Entry encoding, identity, and sort-key semantics

**Status:** accepted · **Date:** 2026-07-30

## Context

Entries are the signed, content-addressed unit of replication. Their
encoding is the input to both the signature and the id hash, so any
ambiguity forks identity. Negentropy reconciliation additionally requires
every element to carry a u64 sort key in a total order.

## Decision

An entry is a fixed positional CBOR array —
`[author, sort-key, kind, payload, sig]` — never a map: positional
encoding removes key-ordering ambiguity from the signing input, and the
array is frozen within a wire version. The to-be-signed form is the same
array without `sig`. Signature: Ed25519 over the tbs bytes. Entry id:
SHA-256 of the full 5-element encoding, so distinct signatures are
distinct entries. Both are computed over the canonical re-encoding of
decoded fields, never raw received bytes: a non-canonical transmission
cannot fork an id.

Sort key: milliseconds since the Unix epoch at creation, author-claimed,
with ties broken by bytewise entry-id comparison. Milliseconds keep u64
range for 500 million years while making same-second entries (common in a
burst from one device) mostly order-distinct. The sort key orders the set
for range reconciliation; it is never treated as a verified clock.

Dependency justifications (mesh core): `sha2` — RustCrypto SHA-256, the
standard pure-Rust implementation; `ed25519-dalek` — the standard
pure-Rust Ed25519, built without its RNG feature so no platform
entropy dependency enters the staticlib builds.

## Consequences

- Byte-exact signing and identity, pinned by conformance vectors.
- Adding an entry field requires a wire version bump — deliberate rigidity
  where signatures are involved.
- Authors control their own placement in the reconciliation order; a lying
  clock misplaces only that author's entries.

## Alternatives rejected

- **Map-encoded entries:** ignorable-key extensibility is exactly what a
  signed structure must not have.
- **Seconds-granularity sort key:** collision-heavy, pushing work onto the
  tiebreak; milliseconds cost nothing in u64.
- **Hybrid logical clocks:** more structure than range reconciliation
  needs; the u64 convention is what Negentropy interoperates with.
- **Id over received bytes:** lets an attacker mint many ids for one
  signed entry via non-canonical re-encoding.
