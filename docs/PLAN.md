# Plan — Android first release

The maintainer has selected a usable Android first release, with hardware
validation deferred. This replaces the milestone-by-milestone scope of M4;
M0–M3 are implemented and M4's BLE radio remains unmeasured.

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
restart recovery and signed-entry sync; an installable APK builds. Two-phone
BLE discovery, bidirectional transfer, process-restart persistence on actual
phones, Bluetooth toggling, and screen-off behavior remain the maintainer's
hardware acceptance gate. Record actual results in MEASUREMENTS.md later.

## Work while physical testing is deferred

- Commit/push the preview and resolve failures reported by remote CI.
- Add an ADB runner for explicit-device preparation, offline checks,
  two-phone sync/reconnect checks, and private evidence capture.
- Run offline install/post/restart checks in CI on API 29 and 35 emulators.
- Document the trust boundaries and current threats without claiming an audit.
- Keep physical BLE/background/range acceptance pending; later use captured
  evidence and maintainer-supplied physical context to record real results.
