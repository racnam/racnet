# ADR-0011: Weight-balanced order-statistics store, snapshot per session

**Status:** accepted · **Date:** 2026-07-30

## Context

Range-based reconciliation needs a dynamic ordered set supporting
composable summaries over arbitrary contiguous ranges and navigation by
rank (brief §4.3): Negentropy splits ranges at balanced points and
compares `(count, id-sum mod 2^256)` fingerprints over them. Entries are
append-only, so the store never deletes. Reconciliation sessions run over
radio links while gossip keeps inserting entries.

## Decision

`core/src/store/` provides `OrderIndex`: an arena-allocated
weight-balanced binary tree (`Vec` of nodes, `u32` child indices, no
`unsafe`, no interior mutability, no RNG) storing `(sort-key, entry-id)`
items in the §6.1 total order. Each node carries the subtree summary
`(count, id-sum mod 2^256)`; the balance invariant is the subtree size the
rank navigation needs anyway, and insert-only weight balancing needs just
the classic single/double rotations. Insert, rank lookup, lower bound, and
range summary are all O(log n). `EntryStore` pairs the index with an
id-keyed entry map and owns the §3.5 rule that signatures are verified
before storage.

Each reconciliation session reconciles a **sealed snapshot** of the
requested sort-key window (a sorted `Vec` copied at session open), not the
live index. The engine consumes a `NegentropyStore` trait implemented by
both the snapshot and the live index.

## Consequences

- Negentropy's rank/fingerprint arithmetic runs against an immutable set,
  as its upstream `seal()` contract assumes; concurrent inserts cannot
  shift ranks mid-session. Entries arriving during a session are simply
  absent from it and are picked up by the next one — racnet reconciles
  periodically by design.
- Snapshot cost is O(window) memory per open session (40 bytes per item);
  acceptable in-memory, revisit when a persistent backend arrives.
- The trait boundary lets a persistent backend (e.g. a monoid skip-list
  over a KV store) replace the arena tree without touching the engine.

## Alternatives rejected

- **Sorted `Vec` only (upstream's storage):** O(n) insert on a store that
  ingests continuously; fine for sealed snapshots (we use exactly that per
  session) but not as the live index.
- **AVL/red-black with augmented summaries:** carries a second invariant
  (height/color) alongside the subtree sizes weight-balancing already
  maintains; more state, same asymptotics.
- **Skip list:** needs an RNG, complicating the determinism this repo's
  tests rely on, and safe-Rust singly-owned links fit a tree better.
- **Reconciling the live index directly:** ranks and fingerprints chasing
  concurrent inserts make rounds internally inconsistent; correctness
  would depend on the caller never inserting mid-session.
