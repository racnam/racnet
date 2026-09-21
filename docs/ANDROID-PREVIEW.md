# Android 0.2.0 preview handoff

This preview implements a public nearby message board on the existing Racnet
protocol. It is ready for sideload testing; foreground phone checks have passed.
Remaining physical acceptance is listed below.
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

## Physical acceptance status

Foreground checks passed for offline persistence, bidirectional transfer,
received-entry persistence, reconnect catch-up, Bluetooth recovery with an
explicit mesh restart, draft rotation, and consecutive-post convergence.
See [MEASUREMENTS.md](MEASUREMENTS.md) for the dated device results and limits;
the batch check is not a maximum-rate load test.

Unplugged screen-off/background survival, throughput/timing/range procedures,
and physical-phone upgrade preservation remain pending. Third-phone relay is
optional and untested. Follow [DEVICE-TESTING.md](DEVICE-TESTING.md) for the
remaining checks and repeat foreground checks after relevant changes. M4 is
not complete. [M5-IOS-PLAN.md](M5-IOS-PLAN.md) prepares future iOS work without
changing the active Android-first scope or starting iOS implementation.

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

All final workflows passed at `100608c`: [Rust checks, conformance and fuzzing](https://github.com/racnam/racnet/actions/runs/35444871041),
[Android build and API 29/35 offline emulator checks](https://github.com/racnam/racnet/actions/runs/35444869609),
and [iOS build](https://github.com/racnam/racnet/actions/runs/35444869626).
Rust 1.98 Clippy fixes preserve the wire format and algorithm. The pinned
Negentropy reference checkout now lives outside the Rust build cache and can
recover missing Git objects; local conformance and randomized checks passed too.

## Correctness audit, 2026-09-21

The current source passed 177 Rust workspace tests, 160 mobile-configuration
tests, formatting, Clippy with warnings denied, and the anchor smoke run.
Negentropy reference conformance and randomized interoperability checks passed,
as did all three parser fuzz targets in a 10-second-per-target smoke run.

Android native libraries, generated bindings, lint, all 52 JVM tests and the
debug APK build passed. Lint reports zero errors and six advisory warnings.
The device runner's 42 Python tests passed. New regressions cover interrupted
journal initialization, partial-push notifications, handshake expiry, retained
draft/save state, reliable peer and receive-error state, and bounded final-frame
draining. See [ARCHITECTURE-REVIEW.md](ARCHITECTURE-REVIEW.md) for the assessment
of the foundation and the remaining product gaps.

These checks do not refresh physical acceptance or establish iOS behavior.
The newly built APK is at the standard build output path; the root preview
copy and its hash above remain the earlier checkpoint. Repeat foreground
phone checks after these transport and lifecycle changes before adopting the
new build as the physical-testing baseline.

## Resume after a context reset

Read `CLAUDE.md`, this handoff, `docs/PLAN.md`, and
[TESTING-WITH-PHONES.md](TESTING-WITH-PHONES.md) first. Inspect `git status`
and current history before editing. The build and automation sections above
are dated historical checkpoints; use [MEASUREMENTS.md](MEASUREMENTS.md) for
the subsequent physical-phone results.

Completed and pushed to `main`:

- `3f92e0c`: persistent Android message-board preview.
- `03260eb`: device evidence runner, emulator CI, testing and threat documentation.
- `100608c`: conformance checkout/cache repair.

Foreground phone acceptance has a recorded baseline; remaining physical gates
are listed above. Inspect connected devices with
`python3 scripts/device_test.py inventory`, then use explicit serials for
preparation and repeatable checks. Follow the linked guide; handle unexpected
screens or failures as evidence to investigate, not a pass. The test operator
handles unlock/USB prompts and physical movement. Keep raw logs, screenshots,
and per-step reports in ignored `test-runs/`. Record verified hardware results
in `docs/MEASUREMENTS.md` with device/OS and physical conditions. Do not infer
range, timing, maximum-rate capacity, or background survival from foreground
passes. Do not redo completed implementation work after a context reset.
