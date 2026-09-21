# Plan — Android first release

The active scope is a usable Android first release. This replaces the
milestone-by-milestone scope of M4; M0–M3 are implemented. Android foreground
phone validation has passed, as recorded in [MEASUREMENTS.md](MEASUREMENTS.md).
M4 remains open: radio performance and background acceptance are incomplete.

## Deliverables

- Durable, bounded signed-entry storage, including received entries; verify
  records on restart, recover interrupted appends, and report storage failures.
- A nearby public message board: compose/read UTF-8 messages offline, show
  author identifiers and timestamps, retain messages across process restarts,
  and sync automatically when peers connect. Clearly explain that everyone
  in the mesh can read and relay these messages and delivery is opportunistic.
- Coalesce sync requests and retry refused attempts. Relay newly received
  entries to other connected peers using existing reconciliation; no new
  routing protocol or wire-format change.
- Surface startup/radio errors; bound outgoing buffering and clean up sockets
  on cancellation. Preserve diagnostics and test-entry tooling.
- Local Rust checks, Android lint/unit tests/APK build, and regression tests
  for persistence and sync scheduling. Provide the APK and a short deferred
  device acceptance checklist.

## Boundaries

This is an Android preview for sideload testing, not a public-store launch or
security-reviewed release. iOS transport, private/direct messages, QR identity
verification, chunk transfer, fast radios, anchor service, mesh sites, and an
external security audit remain future work. No throughput/range claims or
hardware pass results are made without measurements. No store submission,
publishing, or distribution decision is implied.

## Acceptance

The app can create and retain readable messages offline; automated tests prove
restart recovery and signed-entry sync; an installable APK builds. Physical
phones have passed offline persistence, bidirectional transfer, received-entry
persistence, reconnect catch-up, Bluetooth recovery with an explicit mesh
restart, draft retention through rotation, and consecutive-post convergence.
The consecutive-post check does not establish maximum-rate capacity.
[MEASUREMENTS.md](MEASUREMENTS.md) is the source for hardware, dates, methods,
and result limits.

Unplugged screen-off/background survival, throughput/timing/range procedures,
and physical-phone upgrade preservation remain pending. Third-phone relay is
an optional acceptance check. Foreground results do not complete M4.

## Remaining work and follow-up

- Repeat foreground checks after relevant app or transport changes using the
  ADB runner and [TESTING-WITH-PHONES.md](TESTING-WITH-PHONES.md).
- Run offline install/post/restart checks in CI on API 29 and 35 emulators.
- Complete the pending physical acceptance checks in
  [DEVICE-TESTING.md](DEVICE-TESTING.md) and record results in MEASUREMENTS.md.
- Keep raw testing evidence local and ignored; publish only reviewed project
  results without personal or workstation details.
- Prepare future iOS transport work using [M5-IOS-PLAN.md](M5-IOS-PLAN.md).
  That document is planning only; Android-first remains the active scope and
  iOS implementation is not part of this preview.
