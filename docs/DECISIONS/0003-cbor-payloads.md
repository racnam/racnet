# ADR-0003: Deterministic CBOR payloads, minicbor codec

**Status:** accepted · **Date:** 2026-07-30

## Context

Every wire payload needs one serialization format. The parser consumes
untrusted radio input; entries are hashed and signed, so the format must
have a byte-exact canonical form; the spec must be implementable
independently from `docs/PROTOCOL.md` alone; and future payloads must be
extensible without wire breaks.

## Decision

All payloads are CBOR (RFC 8949) restricted to core deterministic encoding
(RFC 8949 §4.2.1), with unsigned-integer map keys, definite lengths only,
and no floats or tags. Schemas are written in CDDL (RFC 8610) in the spec.
Unknown map keys are ignored on decode — the additive extension mechanism.

Codec: `minicbor`. Its derive emits integer-keyed maps in ascending key
order, which is exactly the deterministic form, and it allows hand-written
encoders where the spec demands a fixed positional array (`entry`). It is
no_std-capable, keeping embedded targets open.

Dependency justifications (mesh core): `minicbor` — small, actively
maintained CBOR codec with precise control over emitted bytes and no
serde indirection.

## Consequences

- One stated set of encoding rules covers every payload; conformance
  vectors pin the bytes.
- Additive protocol evolution without version bumps.
- The deterministic subset must be enforced by tests, since CBOR at large
  permits many encodings of the same data.

## Alternatives rejected

- **Protocol Buffers:** no canonical serialization — unacceptable for
  signed, content-addressed entries; schema compiler toolchain burden.
- **Hand-rolled TLV:** byte-exact but forfeits self-description, schema
  language, and tooling; every field addition is bespoke parser code in the
  most security-critical module.
- **postcard/bincode:** Rust-shaped formats without a language-neutral
  schema story; the spec must be implementable outside Rust.
- **ciborium (serde CBOR):** serde indirection surrenders control over
  exact emitted bytes, which canonical encoding needs.
