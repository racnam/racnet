# Plan — Milestones 0 and 1

Scope of this plan: M0 and M1 only. Later milestones are listed in the project
brief (a private planning document) and get planned one at a time; "brief §n"
references below point into that document.

Decisions locked before this plan (see ADRs): Rust core + UniFFI (ADR-0001),
AGPL-3.0 (ADR-0002), solo contributor posture, substrate-not-app positioning.

---

## Milestone 0 — Toolchain proof

**Goal:** a trivial Rust core (`version() -> String`) built through UniFFI into
a running iOS app, a running Android app, and the anchor binary, with CI green
on all three. No protocol work. This exists to de-risk cross-compilation —
the most annoying part of the architecture — before anything depends on it.

### Deliverables

- Rust workspace: `core/` (`racnet-core`, lib + staticlib + cdylib,
  `#[uniffi::export] fn version()`), `anchor/` (`racnet-anchor`, prints core
  version), `uniffi-bindgen/` (binding-generator bin, standard UniFFI pattern).
- `bindings/kotlin/build-android.sh`: cargo-ndk builds `.so` for `arm64-v8a` +
  `x86_64` into `android/app/src/main/jniLibs/`; generates Kotlin bindings into
  the app source set.
- `bindings/swift/build-xcframework.sh`: builds `aarch64-apple-ios` +
  simulator slices, generates Swift bindings, packages `RacnetCore.xcframework`.
  macOS-only; exercised by CI.
- `android/`: minimal Compose app showing `Core version: <version()>`.
- `ios/`: minimal SwiftUI app showing the same; project defined by XcodeGen
  `project.yml` (no hand-maintained `.pbxproj` — authored from Linux).
- CI: `rust.yml` (fmt, clippy -D warnings, test, anchor smoke run on ubuntu),
  `android.yml` (SDK + NDK + cargo-ndk + assembleDebug on ubuntu),
  `ios.yml` (xcframework + xcodegen + simulator build on macOS).

### Done when

- `cargo fmt --check`, `cargo clippy -- -D warnings`, `cargo test --workspace`
  pass locally; `cargo run -p racnet-anchor` prints the version.
- All three CI workflows are green on the GitHub remote.
- An independent fresh-context review of the diff against this plan has been
  done and any gaps are resolved.

### Constraints / environment notes

- Dev machine is Linux: iOS builds only in CI; Android builds locally only if
  SDK/NDK are installed. The macOS CI job is the arbiter for "running iOS app"
  until physical devices are in the loop.
- On-device verification (version string rendering on real phones) is recorded
  in `docs/MEASUREMENTS.md` when hardware is available; it is not a blocker for
  M0 completion, CI green is.

---

## Milestone 1 — Protocol spec v0.1 (complete)

**Goal:** `docs/PROTOCOL.md` v0.1 plus a Rust implementation of framing and
serialization, tested over a simulated transport. Still no radio.

### Deliverables

- Spec sections, byte-exact where wire-visible:
  - Framing: field tables, endianness, length rules, padding to fixed block
    sizes (256/512/1024/2048).
  - Message type registry — including a reserved type for Plumtree lazy-IHAVE
    (brief §4.1) so the v2 optimization needs no wire break.
  - Payload schemas in CDDL (CBOR).
  - Session: `Noise_XX_25519_ChaChaPoly_SHA256` handshake sequence, rekey
    policy, replay window, handshake rate limits.
  - Version negotiation.
  - RBSR/Negentropy message set; u64 sort-key convention for entries.
  - Error handling.
  - Conformance vectors: fixed byte sequences any implementation must
    reproduce.
- Rust: framing + CBOR serialization in `racnet-core` behind typed module
  boundaries; round-trip tests + vector tests; simulated in-process transport
  (configurable latency, loss, partition, MTU) as the harness for M2+.
- ADRs for each non-obvious wire choice (CBOR, padding scheme, sort-key
  semantics, framing layout).

### Order of work

Spec commit lands first, versioned; implementation commits follow it.

### Done when

- PROTOCOL.md v0.1 complete with conformance vectors.
- `racnet-core` reproduces every vector; round-trip property tests pass on CI.
- Independent fresh-context diff review against this plan, gaps resolved.
