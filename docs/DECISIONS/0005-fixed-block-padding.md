# ADR-0005: Fixed-block padding inside the encryption boundary

**Status:** accepted · **Date:** 2026-07-30

## Context

Message sizes leak activity even when contents are encrypted: a 117-octet
entry push and a 60-octet control message are distinguishable on the air.
The project brief mandates padding to fixed block sizes to blunt traffic
analysis.

## Decision

Pad every inner message to the smallest of 256, 512, 1024, or 2048 octets
that fits; above 2048, to the next multiple of 2048, capped at 63 488 so
ciphertext plus AEAD tag stays within the Noise message limit. Zero fill,
mandatory smallest block, padding contents ignored by receivers but
over-padding rejected (a covert channel otherwise). Padding is part of the
plaintext — pad-then-encrypt — so observers see only the size class, never
the true length.

This is a single fixed policy. How aggressive padding must be ultimately
depends on the threat-model tier (festival convenience vs. activist
safety), which is an open maintainer decision; this ADR deliberately does
not introduce tiered or configurable padding. If the maintainer later
selects a stricter tier, block sizes change with a wire version bump.

## Consequences

- Frame lengths take at most 34 distinct values; small control messages
  are indistinguishable from small entry pushes.
- Up to 255 octets of overhead on tiny messages — significant on BLE
  budgets, and accepted deliberately.
- Receivers validate padding arithmetic strictly; conformance vectors pin
  a boundary case (exactly 256) and a spill case (257 → 512).

## Alternatives rejected

- **No padding:** free, and gives away message types by length.
- **Random-length padding:** weaker than fixed classes (lengths remain
  statistically distinguishable) and needs a covert-channel-free RNG story.
- **Padmé or per-tier configurable padding:** more machinery, and picks a
  threat-model posture that is not this ADR's to pick.
