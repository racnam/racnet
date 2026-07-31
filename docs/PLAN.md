# Plan — Milestone 4

Scope of this plan: M4 only. Later milestones are listed in the project brief
(a private planning document) and get planned one at a time; "brief §n"
references point into that document.

Completed milestones (plans preserved in git history at this file's path):

- **Milestone 0 — Toolchain proof (complete).** Rust core through UniFFI into
  running iOS and Android apps plus the anchor binary, CI green on all three.
- **Milestone 1 — Protocol spec v0.1 (complete).** PROTOCOL.md v0.1 with
  conformance vectors; framing + CBOR codec + signed entries in `racnet-core`;
  deterministic simulated transport (ADRs 0003–0009).
- **Milestone 2 — Sync core (complete).** In-house Negentropy V1 engine
  proven against the upstream conformance suite; order-statistics entry
  store; sync session layer; end-to-end simulated sync (spec 0.1.1, ADRs
  0010–0011).
- **Milestone 3 — Noise session layer (complete).** In-house Noise XX engine
  proven against the cacophony vectors and live snow interop; sans-I/O link
  driver with encrypted transport epochs, rekeying, and rate limiting;
  end-to-end encrypted sync in simulation; parser fuzzing in CI (spec 0.1.2,
  ADRs 0012–0013).

Decisions locked before this plan (see ADRs): Rust core + UniFFI (ADR-0001),
threat-model posture — wire discipline to the adversarial tier, no error
oracles, silent close on crypto failure (ADR-0009), in-house Noise with
`snow` as dev-only oracle and no FFI exports from `noise/`/`link/`
(ADR-0012, deliberately revisited this milestone), parser fuzzing
(ADR-0013).

---

## Milestone 4 — Android BLE transport

**Goal:** the first real radio. Dual-role BLE on Android — every device
advertises and scans simultaneously — with an L2CAP connection-oriented
channel carrying the existing encrypted frame stream, a foreground service
keeping the mesh alive, OEM battery-optimization flows, peer discovery, and
the first real two-device sync. The core grows its first real UniFFI
surface; the spec grows a normative BLE transport binding so iOS (M5) can
implement the same procedure. Every number is measured on hardware into
`docs/MEASUREMENTS.md` via a repeatable procedure — desk estimates never
land there.

The protocol itself does not change: `LinkDriver` is sans-I/O and already
tolerates arbitrary stream segmentation, and L2CAP CoC is exactly the
reliable ordered byte stream the spec assumes. Wire version stays 1.

### Deliverables

- Spec patch, PROTOCOL.md 0.1.2 → 0.2.0, its own commit landing before any
  code — additive only, wire version stays 1: a new §9 "Transport bindings"
  with §9.1 "BLE L2CAP CoC" — dual-role operation with the L2CAP connector
  as the link initiator of §4; a fixed random 128-bit service UUID as the
  entire advertisement payload; PSM published as a read-only GATT
  characteristic (u16 LE) under the same service, read then GATT-closed
  before the channel opens; the channel is the "insecure" L2CAP variant —
  all security is the Noise layer; duplicate-link SHOULDs (0–2 s connect
  jitter; post-handshake, both sides keep the link whose initiator has the
  lexicographically smaller fingerprint and silently close the other); the
  §4.5 remote transport address is the observed 6-byte BLE device address,
  with RPA rotation noted and the half-open cap as the backstop; a
  fingerprinting note recording the fixed UUID as a scoped, deliberate
  deviation from the no-constants posture; plus one clarifying sentence in
  §1 (framing stays transport-independent; §9 sections are normative for
  implementations claiming that transport).
- `core/src/api/`: the first real FFI surface (ADR-0014) — a single
  `Node` object behind one mutex owning the `EntryStore`, the
  `HandshakeLimiter`, and a link-id → `LinkDriver` map. Kotlin drives it
  per-connection: `connect`/`accept` (accept consults the limiter; refusal
  means drop the socket silently), `on_bytes` → frames-to-write + events,
  `on_transport_closed`, `tick` (24-hour cap + limiter expiry),
  `start_sync`, `create_entry` (frame-size guard), `entries`,
  `link_status`, `fingerprint`. Identity is two 32-byte seeds from
  `generate_identity()` (X25519 + Ed25519) — a deliberate, scoped revision
  of ADR-0012's export ban; the `SecretKey` type stays unexported and
  ephemerals are generated inside core. Events are return values, not
  callbacks. Time stays injected u64 microseconds with an internal
  monotone clamp. No new core dependencies. The `sim` module moves behind
  a default-on feature and out of shipped mobile artifacts
  (`--no-default-features` in both binding build scripts).
- Android app (`org.racnet.android`): minSdk 29 (L2CAP CoC APIs);
  foreground `MeshService` (`START_STICKY`, type `connectedDevice`,
  notification showing peer and entry counts); `IdentityStore` persisting
  the seeds in a file wrapped by an Android Keystore AES-GCM key;
  `NodeRuntime` serializing all FFI calls on a single-parallelism
  dispatcher with `SystemClock.elapsedRealtimeNanos()/1000` as the clock,
  a 1 s tick loop, and auto-sync on establishment and local entry
  creation; `BlePeripheral` (L2CAP listen → PSM → GATT server +
  advertiser, limiter admission before registration); `BleCentral`
  (filtered scan → pure `ConnectionPolicy` with jitter and per-address
  backoff → GATT PSM read → L2CAP connect as initiator); `LinkConnection`
  (blocking read loop, write channel chunked to
  `socket.maxTransmitPacketSize`, teardown → `on_transport_closed`);
  `ConnectionRegistry`; pure `DuplicateLinkPolicy` implementing the spec
  tiebreak; `Meas` emitting stable `MEAS event=…` logcat lines (tag
  `RacnetMeas`) for measurement capture. Minimal Compose UI (the real app
  is M10): permission onboarding with the SDK-split permission sets and
  battery-optimization flow with per-OEM dontkillmyapp links; a status
  screen (service toggle, own fingerprint, peers, entries, create-test-
  entry at 100 B / 1 KiB / 10 KiB / 100 KiB); a diagnostics screen with
  per-link phase timings, byte counters, and computed throughput,
  copyable for transcription into MEASUREMENTS.md.
- Tests: `core/tests/api_e2e.rs` driving two `Node`s exactly as the
  Kotlin shim will — establishment under odd re-chunking, bidirectional
  sync to convergence, limiter burst and half-open timeout, transport
  close mid-handshake with reconnect under a fresh link id, 24-hour
  expiry, tolerance of stale link ids and non-monotonic clocks — plus
  api unit tests (seed validation, entry guards, exhaustive event
  mapping). No new fuzz targets: the facade adds no new parser of
  untrusted bytes (recorded in ADR-0014). Kotlin unit tests for the pure
  parts: both policies, write chunking boundaries, the identity
  envelope, the Meas formatter. No instrumentation tests — emulators
  have no BLE; the pure-policy extraction is what makes that acceptable.
- CI: the Android job runs `lint testDebugUnitTest assembleDebug`.
- ADR-0014 (Node facade FFI surface), ADR-0015 (BLE transport binding:
  UUID/fingerprintability, GATT PSM over advertisement encoding, insecure
  L2CAP, duplicate-link rule), ADR-0016 (Android platform integration:
  minSdk 29, foreground-service posture, Keystore identity envelope,
  per-dependency justifications).
- `docs/MEASUREMENT-PROCEDURES.md` (persists across milestones): P1
  throughput (100 KiB entry sync at 1 m, median of 3), P2 timing
  (per-phase deltas from `MEAS` lines), P3 range (walk-away in named
  environments), P4 device matrix (foreground / screen-off / doze / OEM
  battery-optimization scenarios), all captured via
  `adb logcat -s RacnetMeas`. `docs/MEASUREMENTS.md` itself stays empty
  until the maintainer runs the procedures on hardware — the app and the
  procedures are the harness; no number in this repo is ever invented.

### Order of work

Plan commit, then the spec patch commit, then ADRs, then implementation
commits (sim feature gate → api module → api tests → binding scripts →
Android scaffolding → identity + NodeRuntime → peripheral path → central
path → service wiring + Meas → UI → Kotlin tests + CI), then measurement
procedures, then changelog/README. Every commit passes fmt, clippy -D
warnings, and the full test suite.

### Done when

- `cargo fmt --all -- --check`, `cargo clippy --workspace --all-targets --
  -D warnings`, `cargo test --workspace` pass locally; anchor smoke run
  still prints the version; `scripts/fuzz.sh` runs clean in its time box.
- `bindings/kotlin/build-android.sh` then `cd android && ./gradlew lint
  testDebugUnitTest assembleDebug` pass.
- All CI workflows green on the GitHub remote, including the extended
  Android job and the macOS iOS job (which validates the
  `--no-default-features` change to the Swift build script).
- An independent fresh-context review of the diff against this plan has
  been done and any gaps are resolved.
- Maintainer-run, after this session: two-device sync observed on real
  hardware and MEASUREMENTS.md rows filled via
  `docs/MEASUREMENT-PROCEDURES.md`. The milestone is not complete until
  the radio has real numbers; the agent's deliverable is everything
  needed to produce them.
