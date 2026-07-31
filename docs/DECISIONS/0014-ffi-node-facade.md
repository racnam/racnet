# ADR-0014: FFI surface is a single Node facade

**Status:** accepted · **Date:** 2026-07-31

## Context

M4 puts the core behind a real radio for the first time, so the Kotlin
shim must drive links from outside the crate. Until now nothing but
`version()` crossed UniFFI, and ADR-0012 explicitly deferred the question:
"Nothing in `noise/` or `link/` is exported over UniFFI this milestone;
`SecretKey` must never be."

Exporting `LinkDriver` directly does not work: `on_bytes` takes
`&mut EntryStore`, and every link on a node shares one store, so exported
link objects would each need an `Arc<Mutex<EntryStore>>` anyway — at which
point the mutex may as well cover everything. The `HandshakeLimiter`
likewise spans links by design. UniFFI's own constraints point the same
way: exported objects are `Arc`-shared with `&self` methods, fixed-size
arrays and tuple returns don't cross, and callback interfaces invoked
under a lock invite re-entrancy deadlocks.

## Decision

One exported object, `Node` (`core/src/api/`), holding a single mutex
over the `EntryStore`, the `HandshakeLimiter`, and a map of `u64` link
ids to `LinkDriver`s. The shim addresses links by id: `connect`/`accept`
open one, `on_bytes` feeds one, `on_transport_closed` and `tick` retire
them. Everything the shim needs comes back as return values — records of
frames-to-write plus typed events — never callbacks: the transport side
already has a per-connection read loop that is the natural dispatch
point, and return values keep the facade as sans-I/O and as testable as
the layers under it. The internal layers (`noise/`, `link/`, `sync/`,
`wire/`, `store/`) are untouched; the facade is additive.

Identity crosses the boundary as two 32-byte seeds (X25519 static,
Ed25519 signing) from `generate_identity()`, accepted back by
`Node::new`. This is a deliberate, scoped revision of ADR-0012's
prohibition: the `SecretKey` type remains unexported and un-constructible
from foreign code, but the seed bytes must live somewhere the app can
persist, because Android Keystore keys are non-exportable and cannot
perform the raw X25519 operations our in-house Noise engine needs. The
Kotlin side stores the seeds encrypted at rest (ADR-0016); the
zeroization claim stays as modest as ADR-0012's — the foreign copy is
beyond Rust's reach.

Per-link ephemerals are generated inside `connect`/`accept`, leaving no
ephemeral-injection hook in the production API; the crate's entropy use
stays confined to `Keypair::generate()` plus `generate_identity()`'s two
seed draws, all in explicitly named constructors. Time stays injected as
u64 microseconds; the node clamps it monotonically, since per-connection
threads can read a clock and then lose the race to the mutex. `tick` and
`on_transport_closed` return events only — every close today is silent by
protocol design, so no code path produces frames on those calls; if that
ever changes the frame-routing question reopens here.

No new fuzz targets: the facade parses no untrusted bytes. `on_bytes`
feeds the same `FrameDecoder`/`LinkDriver` path the `frame_decode` and
`link_driver` targets already cover (ADR-0013); the remote address is an
opaque map key; seeds are length-checked local input. No new
dependencies: `uniffi`, `getrandom`, `ed25519-dalek`, `zeroize`, and
`thiserror` were already direct dependencies of the core.

The `sim` module moves behind a default-on cargo feature and out of the
shipped mobile artifacts (`--no-default-features` in both binding build
scripts): a deterministic network simulator has no business in an app
binary.

## Consequences

- The whole FFI is one lock. That is deliberate: per-call work is
  microseconds of ChaCha20 on ≤64 KiB and the radio delivers ~200 Kbps;
  contention is not the bottleneck, and a single lock cannot deadlock
  against itself re-entering through a callback.
- `core/tests/api_e2e.rs` drives two `Node`s exactly as two shims would,
  so facade behavior (limiter wiring, close/reconnect lifecycles, event
  mapping) is proven host-side before a phone is involved.
- The exported enums mirror internal ones (`CloseReason` → `CloseCause`
  plus transport-only causes); an exhaustive match keeps the mapping
  compiler-enforced when internal variants change.
- Identity seeds are the one secret that crosses the boundary, forever a
  place where review must look at the foreign side too.

## Alternatives rejected

- **Exporting `LinkDriver` per link:** aliases `&mut EntryStore` across
  objects — unrepresentable over UniFFI without the shared mutex that
  collapses this into the chosen design with more surface.
- **Callback interface for events:** either fires under the node lock
  (re-entrancy deadlock the moment Kotlin calls back in) or needs a
  deferred dispatch queue — machinery with no benefit over returning
  events to the thread that is already there.
- **Sealed opaque identity object with a `serialize()` method:** its
  output is the same 32 bytes with extra ceremony; hardware-backed keys
  that never leave the Keystore are incompatible with an in-house Noise
  engine doing raw DH.
- **Async exports:** the core is sans-I/O by contract (ADR-0012's
  determinism argument depends on it); concurrency belongs to the shim.
