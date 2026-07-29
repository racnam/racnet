# ADR-0001: Single Rust core with UniFFI bindings

**Status:** accepted · **Date:** 2026-07-29

## Context

Racnet's hard part is a byte-exact wire protocol plus sync, storage, crypto,
and routing logic that must behave identically on iOS, Android, and Linux
anchor nodes. The packet parser consumes untrusted bytes arriving over radio
from strangers, in deployment scenarios where memory-safety bugs can get
people hurt. The protocol layer must be testable without holding six phones.

## Decision

Implement protocol, sync, storage, crypto, and routing once, in Rust
(`core/`). Generate Swift and Kotlin bindings with Mozilla UniFFI (proc-macro
interface, `uniffi` crate pinned in `Cargo.toml`). Native UI per platform
(SwiftUI / Jetpack Compose). Only the BLE/MPC/WiFi-Aware transport shims and
UI are platform-specific. The Linux anchor daemon reuses the same core.

Supporting choices:

- **XcodeGen** defines the iOS project (`ios/project.yml`); the `.pbxproj` is
  generated, not committed. Rationale: the project is authored from Linux, and
  hand-maintaining pbxproj files without Xcode is error-prone. XcodeGen is
  widely used and actively maintained.
- **cargo-ndk** drives Android cross-compilation; JNA carries the JNI surface
  UniFFI's Kotlin bindings require.

Dependency justifications (mesh core): `uniffi` — the binding generator this
ADR selects; proven at scale in Firefox mobile.

## Consequences

- One implementation of the wire protocol: no cross-platform drift by
  construction.
- Protocol correctness is unit-tested on the host against a simulated
  transport; CI needs no phones for most of the suite.
- Memory-safe parsing of untrusted radio input.
- Cost: cross-compilation toil (NDK, XCFramework packaging), async Rust across
  the FFI boundary, two-language debugging. Milestone 0 exists to surface this
  before anything depends on it.

## Alternatives rejected

- **Native per platform (Swift + Kotlin):** two implementations of a
  byte-exact protocol will drift; would require a conformance suite both must
  pass just to hold ground the single core gets for free. Kept as the fallback
  if the FFI toolchain proves unworkable in M0.
- **React Native:** no RN library exposes L2CAP CoC or MPC, so the hard parts
  are custom native modules anyway, and the JS bridge would sit in the hot
  path of a 10-second iOS background sync window.
- **Kotlin Multiplatform:** weaker story for the Linux anchor target and for
  fuzzing/hardening a parser of untrusted bytes; smaller crypto ecosystem than
  Rust's.
