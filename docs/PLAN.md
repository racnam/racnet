# Plan — Milestone 2

Scope of this plan: M2 only. Later milestones are listed in the project brief
(a private planning document) and get planned one at a time; "brief §n"
references point into that document.

Completed milestones (plans preserved in git history at this file's path):

- **Milestone 0 — Toolchain proof (complete).** Rust core through UniFFI into
  running iOS and Android apps plus the anchor binary, CI green on all three.
- **Milestone 1 — Protocol spec v0.1 (complete).** PROTOCOL.md v0.1 with
  conformance vectors; framing + CBOR codec + signed entries in `racnet-core`;
  deterministic simulated transport (ADRs 0003–0009).

Decisions locked before this plan (see ADRs): Rust core + UniFFI (ADR-0001),
AGPL-3.0 (ADR-0002), deterministic CBOR (ADR-0003), entry encoding and
sort-key semantics (ADR-0006), Negentropy carried opaquely (ADR-0007).

---

## Milestone 2 — Sync core

**Goal:** working set reconciliation between two in-process peers: an
in-house implementation of the Negentropy V1 wire protocol, an
order-statistics entry store with range summaries, and a session layer
driving RECON_INIT / RECON_MSG / RECON_DONE over the M1 message codec —
proven against the upstream Negentropy conformance suite and end-to-end on
the simulated transport. Still no radio.

### Deliverables

- Spec patch, PROTOCOL.md 0.1.0 → 0.1.1, its own commit landing before any
  code: (a) §6.2 session ids carry direction — transport initiator uses even
  sids, responder odd, wrong parity in RECON_INIT is a protocol violation —
  removing the ambiguity when both peers open the same sid; (b) §6.1
  reserves `sort-key = 2^64 − 1`, which collides with Negentropy's infinity
  timestamp sentinel and the `until` window sentinel.
- `core/src/store/`: `Item` with the §6.1 total order; `OrderIndex`, an
  arena-allocated weight-balanced tree (insert-only) with per-subtree
  `(count, id-sum mod 2^256)` monoid summaries giving O(log n) insert, rank
  navigation, and range fingerprints; `EntryStore` combining an id-keyed map
  with the index — signature-verified insert (§3.5), duplicate ids ignored,
  reserved sort key rejected, `snapshot(window)` producing the sealed
  item set a session reconciles (ADR-0011).
- `core/src/sync/negentropy/`: Negentropy V1 byte-for-byte — varint, bound
  (delta timestamps, id prefixes), fingerprint (SHA-256 over id-sum ‖
  varint count, first 16 bytes), and the range-splitting engine with
  initiator/responder roles, frame-size-limit continuation, and the
  single-byte version-negotiation reply — behind a `NegentropyStore` trait
  implemented by both the sealed snapshot and `OrderIndex` (ADR-0010). No
  new dependencies; no panics on untrusted input.
- `core/src/sync/session.rs`: `Syncer` managing concurrent directional
  sessions keyed by (opened-by-us, sid) with parity allocation, snapshot
  per session, GOSSIP_PUSH ingest (verify → store → dedup), pushing of
  reconciled `have` entries chunked under the padding ceiling, and
  protocol-violation surfacing per §7. Transport-agnostic: consumes and
  produces `wire::Message` values.
- Conformance: `core/examples/negentropy_harness.rs` speaking the upstream
  suite's line protocol (item/seal/initiate/msg → msg/have/need/done,
  `FRAMESIZELIMIT` honored); `scripts/negentropy-conformance.sh` pinning
  upstream `hoytech/negentropy` at commit `76f3cf6` and running
  `test.pl rust,rust`, `test.pl rust,js`, `protoversion.pl`, and a
  time-boxed `fuzz.pl` run; a CI job in `rust.yml` executing the script.
- Tests: unit vectors for varint/bound/fingerprint; property tests —
  `OrderIndex` against a `BTreeSet` oracle, two-engine convergence over
  random overlapping sets and frame limits, malformed-bytes never panic,
  session layer under interleaved sessions and garbage sids; SimNet
  end-to-end sync with partition/heal converging to identical stores,
  reproducible by seed.
- ADR-0010 (in-house Negentropy implementation, pinned conformance commit,
  frame-limit floor) and ADR-0011 (weight-balanced monoid store behind a
  trait, snapshot-per-session semantics).

### Order of work

Plan commit, then the spec patch commit, then ADRs, then implementation
commits (primitives → store → engine → conformance harness → session
layer), then changelog/README. Every commit passes fmt, clippy -D warnings,
and the full test suite.

### Done when

- `cargo fmt --all -- --check`, `cargo clippy --workspace --all-targets --
  -D warnings`, `cargo test --workspace` pass locally; anchor smoke run
  still prints the version.
- `scripts/negentropy-conformance.sh` passes locally and in CI (rust↔rust,
  rust↔js, protocol-version negotiation, time-boxed fuzz).
- All CI workflows green on the GitHub remote.
- An independent fresh-context review of the diff against this plan has
  been done and any gaps are resolved.
