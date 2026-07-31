# ADR-0016: Android platform integration choices

**Status:** accepted · **Date:** 2026-07-31

## Context

M4 turns the Android stub into the first real transport host: a
foreground service owning BLE dual-role operation, a persisted device
identity, and the runtime-permission and battery-optimization flows the
platform demands. Several choices are easy to get subtly wrong and worth
recording.

## Decision

**minSdk moves 26 → 29.** The transport this milestone exists to build —
`BluetoothAdapter#listenUsingInsecureL2capChannel`,
`BluetoothDevice#createInsecureL2capChannel`,
`BluetoothSocket#getMaxTransmitPacketSize` — is API 29+. The project
brief's transport matrix already states BLE L2CAP CoC as Android 10+;
supporting 26–28 would mean reflection into hidden APIs for the core
feature. Android 10 and 11 (API 29–30) still need the legacy permission
set (`BLUETOOTH`, `BLUETOOTH_ADMIN`, `ACCESS_FINE_LOCATION` for scanning)
alongside the Android 12+ split (`BLUETOOTH_SCAN` with `neverForLocation`,
`BLUETOOTH_CONNECT`, `BLUETOOTH_ADVERTISE`), so the manifest carries both
with `maxSdkVersion` fences and the onboarding UI computes the request
set from the running SDK.

**Foreground service, nothing more aggressive.** `MeshService` is a
`START_STICKY` foreground service with type `connectedDevice` and a
persistent notification showing peer and entry counts. No wakelock and no
per-OEM heroics in code: the brief identifies OEM battery killers as the
real risk, and the honest response is the standard
`isIgnoringBatteryOptimizations` check with the request intent, per-OEM
guidance links (dontkillmyapp.com) in onboarding, and the device-matrix
measurement log (P4) to tell us per vendor whether more is needed —
measured, not presumed.

**Identity is a Keystore-wrapped file, hand-rolled.** The two 32-byte
seeds from `generate_identity()` (ADR-0014) are encrypted with an
AndroidKeyStore AES-256-GCM key (StrongBox when available) and stored as
`IV ‖ ciphertext` in `noBackupFilesDir` (backups are already disabled).
Jetpack `security-crypto` is deprecated and unmaintained — the wrong
dependency for the one file in the app that must outlive it. The envelope
is ~60 lines with the encode/decode as a pure, unit-tested function. If
the Keystore key is lost the identity rotates; acceptable, since the
protocol has no session resumption to lose and entries re-sync.

Dependency justifications (Android app; the mesh core gains none):

- `kotlinx-coroutines-android`: structured concurrency for socket loops
  and the single-parallelism dispatcher serializing FFI calls; the
  de facto platform standard.
- `androidx.lifecycle:lifecycle-service`: `LifecycleService`, so
  coroutine scopes die with the service instead of leaking read loops.
- `androidx.lifecycle:lifecycle-runtime-compose`: lifecycle-aware
  `StateFlow` collection in the UI.
- `androidx.core:core-ktx`: `NotificationCompat` for the foreground
  notification across API levels.
- Test-only: `junit`, `kotlinx-coroutines-test`.
- Deliberately absent: Hilt (manual service-locator suffices at this
  size), Accompanist permissions (archived upstream; a
  `rememberLauncherForActivityResult` wrapper is ~20 lines), navigation
  libraries (three screens, one `when`), Robolectric (its Bluetooth
  shadows are stubs — the testable logic is extracted into pure policy
  classes instead).

## Consequences

- Devices on Android 8–9 are dropped. Per the brief they never had the
  transport; nothing real is lost.
- The persistent notification is the price of near-continuous BLE; it is
  made useful (peer/entry counts) rather than apologized for.
- Whether a foreground service alone survives Samsung/Xiaomi/Huawei
  battery policy is an open measurement question (P4), by design.
- The pure-policy extraction (connection policy, duplicate-link
  tiebreak, write chunking, identity envelope) is what keeps the app
  testable in CI with no emulator and no radio.

## Alternatives rejected

- **minSdk 26 with reflection/`@RequiresApi` gating:** the app would
  install on devices where its entire purpose cannot work.
- **Jetpack `security-crypto`:** deprecated; freezing a maintained
  60-line envelope beats depending on an unmaintained wrapper.
- **Hardware-backed (non-exportable) identity keys:** the Keystore
  cannot do raw X25519 for the in-house Noise engine (ADR-0014).
- **Wakelocks / OEM-specific keep-alive hacks from day one:** adds
  battery cost and fragility before any measurement says it is needed.
