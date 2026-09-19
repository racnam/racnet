# Android 0.2.0 preview handoff

This preview implements a public nearby message board on the existing Racnet
protocol. It is ready for sideload testing; real-radio acceptance is deferred.
No APK has been published or submitted to an app store.

## Build and install

Run `bindings/kotlin/build-android.sh`, then in `android/` run
`./gradlew lint testDebugUnitTest assembleDebug`. The binding script builds
both Android ABIs and the host library needed by the JVM integration tests.
The APK is `android/app/build/outputs/apk/debug/app-debug.apk`.

```sh
adb -s <SERIAL> install -r android/app/build/outputs/apk/debug/app-debug.apk
```

For this handoff, a copy is also placed at `racnet-0.2.0-preview.apk` in the
repository root (ignored by git). Its SHA-256 is
`9fbb24f691816c7bc8bd1b7bc9404d1310c4326a73ec16d42f0842647db4c1ab`.
The APK signature verifies. Keep the debug signing key if you want to
install future builds over this one without clearing data. An APK signed by
a different key cannot update it in place.

## Verified locally on 2026-09-18

- Rust workspace: 171 tests passed; formatting and clippy with warnings as
  errors passed. The anchor smoke run printed the core version.
- Mobile core configuration: 155 tests passed without the simulator feature.
- Parser fuzzing: all three targets passed a 10-second-per-target smoke run.
- Android: lint (zero errors; eight advisory warnings), 33 JVM tests,
  generated Kotlin bindings, arm64/x86_64 libraries, and debug APK build passed.
- The JVM integration test uses the real native core: 100 rapid posts through
  A–B–C converge, a reply returns to A, and C's received posts survive reopening.
- Android API 35 x86_64 emulator: installed APK, startup, offline post, process
  force-stop/restart, retained message/author identity, and an in-place APK update verified.
- All five persistence tests also passed directly on that Android emulator:
  exclusive-writer locking, restart, incomplete-tail recovery, complete-record
  corruption rejection, and journal bounds. This caught and resolved the
  Android std file-locking incompatibility before handoff.

The emulator does not establish real BLE radio performance. The simulator
and native-runtime tests do not validate OEM Bluetooth stacks or background
execution. macOS/iOS CI was not run in this Linux session.

## Deferred acceptance

Follow the Android preview checklist in [DEVICE-TESTING.md](DEVICE-TESTING.md):
two physical phones, both-direction message transfer, reconnect, Bluetooth
switching, and screen-off behavior. Optionally test a third phone for relay.
Put actual radio results in MEASUREMENTS.md, with device models and OS versions.

## Product limits

Posts are public to every peer, without private/direct-message semantics,
verified real-world identities, or delivery receipts. Copies can be retained
and relayed by peers. Storage stops at 64 MiB or 10,000 entries without automatic
eviction; clearing app data is destructive and also replaces the identity.
Only the identity envelope has separate at-rest encryption. There has been no
external security review. iOS, chunked files, fast radios, and the other
roadmap clients are outside this preview's scope.

## Testing automation follow-up, 2026-09-19

The local APK above includes the permission/backup lint cleanup. Android build,
33 JVM tests, and lint passed again (zero errors, six advisory warnings).
Twelve Python tests cover the evidence parser, incomplete records, result
classification, and pair-test orchestration. The ADB runner passed a fresh
install, offline post, and restart on an API 35 emulator; passive capture was
correctly recorded as unevaluated. These are not physical BLE test results.

The preview commit's Android and iOS remote workflows passed. A Rust 1.98
Clippy rule flagged two older chunk conversions; they were updated without a
wire/algorithm change. The follow-up adds API 29/35 offline emulator CI gates.
See [phone-testing delegation](TESTING-WITH-PHONES.md) for the next session.
