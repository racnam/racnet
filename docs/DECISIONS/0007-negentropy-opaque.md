# ADR-0007: Negentropy messages carried as opaque byte strings

**Status:** accepted · **Date:** 2026-07-30

## Context

Racnet adopts Negentropy (Log Periodic) as its range-based set
reconciliation primitive. Negentropy defines its own compact wire encoding
and ships conformance tests, which the project brief directs us to run
against our implementation. Racnet payloads are otherwise CBOR (ADR-0003).

## Decision

RECON_INIT and RECON_MSG carry Negentropy protocol messages byte-for-byte
as opaque CBOR byte strings (PROTOCOL.md §6.3). Racnet frames supply
session id, sort-key window, and transport; Negentropy's encoding is not
re-expressed in CBOR. The mapping at the boundary: Negentropy element id =
racnet entry id, Negentropy timestamp = racnet sort key.

## Consequences

- The upstream Negentropy conformance suite applies unmodified to the M2
  reconciliation implementation — the point of adopting a specified
  primitive.
- The spec need not restate (and drift from) Negentropy's encoding.
- Two encoding disciplines coexist inside one payload; the bstr boundary
  keeps them cleanly separated.

## Alternatives rejected

- **Re-encoding RBSR in CBOR:** forfeits the upstream test suite and
  invents a second, unproven encoding of a subtle protocol for zero wire
  savings.
- **Negentropy messages as bare inner payloads (no CBOR wrapper):** loses
  the session id and window fields, and makes the one non-CBOR payload a
  permanent special case in every implementation.
