# Device testing walkthrough

For assisted operation, scripted tests, and automatic evidence bundles, see
[Testing with phones](TESTING-WITH-PHONES.md).

The maintainer-run validation for milestone 4 and the field workflow for
every milestone after it: getting builds onto phones, the first-sync
smoke test, log capture, and where results go. Measurement *procedures*
(the exact steps behind each number) live in
`docs/MEASUREMENT-PROCEDURES.md`; results live in
`docs/MEASUREMENTS.md`; this file is the operating manual around them.

## Prerequisites

- Two phones on Android 10+ (API 29 — the L2CAP CoC floor). Different
  vendors is a feature, not a problem: OEM battery behavior is one of
  the things under test.
- `adb` and the GitHub CLI (`gh`) on the machine you work from, with USB
  debugging enabled on both phones.
- The dev machine does not need the Android SDK: CI builds the APK.

## 1. Get the APK

Every green `android` workflow run uploads the debug APK as the
`racnet-debug-apk` artifact. Download the latest from `main`:

```sh
RUN_ID=$(gh run list --workflow android --branch main --status success \
    --limit 1 --json databaseId --jq '.[0].databaseId')
gh run download "$RUN_ID" -n racnet-debug-apk
```

This leaves `app-debug.apk` in the current directory. Artifacts expire
(90 days by default), so always pull a fresh one rather than reusing an
old download.

## 2. Install on both phones

With one phone connected at a time:

```sh
adb install -r app-debug.apk
```

or, with both connected, per serial (`adb devices` lists them):

```sh
adb -s <SERIAL_A> install -r app-debug.apk
adb -s <SERIAL_B> install -r app-debug.apk
```

`-r` reinstalls in place, keeping app data — including the device
identity, which lives in a Keystore-wrapped file and survives
reinstalls. To reset a phone to a factory-fresh mesh state (new
identity, empty store): `adb shell pm clear org.racnet.android`.

## 3. First-run setup, per phone

1. Open Racnet. The onboarding screen appears on first run.
2. **Grant permissions.** On Android 12+ these are the three Bluetooth
   permissions (plus notifications on 13+). On Android 10/11 it is
   location — required by Android for BLE scanning; the app never reads
   location. On 10/11 the system **location toggle** in quick settings
   must also be on, or scans silently return nothing.
3. **Request the battery-optimization exemption.** On Samsung, Xiaomi,
   Huawei, Oppo, and similar, also follow the vendor link the app shows
   (dontkillmyapp.com) — the standard exemption is often not enough,
   and P4 exists to find out per device.
4. Continue to the status screen and **toggle the mesh on**. The
   persistent notification ("Racnet mesh — 0 peers · 0 entries") is the
   sign the foreground service is up.

## 4. The first-sync smoke test

This checks the §9.1 binding on real radios. The current foreground baseline
is recorded in [MEASUREMENTS.md](MEASUREMENTS.md); repeat this smoke test after
transport changes. It does not complete the remaining M4 acceptance checks.

1. Both phones: mesh on, screens on, within a few meters.
2. Within ~5–15 s each status screen should show one peer. The
   fingerprints shown must cross-match: A's peer list shows B's
   fingerprint (compare its prefix with "You:" on B), and vice versa.
   One phone shows "(dialed)", the other "(accepted)" — or, after a
   crossed connection, the duplicate-link rule may briefly close one
   link and keep the other; a single stable peer entry per phone is the
   success condition either way.
3. On phone A, create a 1 KiB test entry. It should appear in B's entry
   list within a couple of seconds, and both notifications should read
   "1 peers · 1 entries".
4. Create one on B; confirm it reaches A. Bidirectional sync working
   means the milestone's radio path is real.

Expected `RacnetMeas` lines during this (see §5 for capture):
`service_started`, `listening psm=…`, `psm_read`, `link_open`,
`established`, `reconciled`, `sync_done`, and — if the phones crossed
connections — `duplicate_link_closed`.

## 5. Capturing logs

All measurement records go to logcat under one tag. Capture from each
phone into a file while testing:

```sh
adb -s <SERIAL> logcat -v time -s RacnetMeas | tee phoneA-$(date +%F).log
```

Every line is machine-readable: `MEAS event=<name> key=value ...`.
`sync_done` lines include `link_avg_in_kbps`, a cumulative link average
including handshake and idle time, not isolated radio throughput. The
in-app diagnostics screen shows the same per-link phase timings with
copy-to-clipboard, which is often faster for transcribing one run.

For debugging (as opposed to measuring), capture the full log instead:
`adb logcat -v time > full.log` — the transport tags are
`RacnetPeripheral` and `RacnetCentral`.

## 6. Recording measurements

Run the procedures in `docs/MEASUREMENT-PROCEDURES.md`:

| Id | What | Effort |
|----|------|--------|
| P1 | Throughput: 48 KiB entry sync at 1 m, median of 3 | ~15 min |
| P2 | Timing: cold-start phase deltas + sync duration | ~15 min |
| P3 | Range: walk-away until the link stops recovering | ~30 min, needs space |
| P4 | Background survival: screen-off, doze, OEM matrix | hours, spread out |

Transcribe results into the matching table in `docs/MEASUREMENTS.md`.
Every row carries the value, **both** device models, **both** OS
versions, the procedure id as the method, and the date. Commit the rows
on their own (`Record <what> measurements from <devices>`); never
round-trip numbers through anywhere else — if it has no row in that
file, it has not been measured.

## 7. Troubleshooting

**No peer appears.**
- Bluetooth on, on both? Mesh toggle on, on both (notification
  present)?
- Android 10/11: is the system location toggle on? Scans need it.
- Check the accepting side's log for `listening psm=…` — absence means
  the L2CAP listen or GATT server failed to start (full logcat,
  `RacnetPeripheral` tag).
- Check the dialing side for `psm_read` — its absence means discovery
  or the GATT read is failing (`RacnetCentral` tag); presence without a
  following `link_open` means the L2CAP connect is failing.
- A phone never sees *itself*; two phones minimum.

**Peer appears, then vanishes and reappears in a loop.** Watch for
`link_closed cause=…` lines: `DecryptFailed`/`BadCiphertextLength`
suggests stream corruption (a transport bug — collect logs);
`HandshakeTimeout` on one side suggests the other stopped mid-handshake
(often the OS killing the app — see P4).

**Entries stop syncing after the screen has been off a while.** That is
exactly what P4 measures. Note the elapsed time and the battery
settings state, and log it as a device-matrix row rather than treating
it as a bug.

**Anything unexplained:** save both phones' full logcat plus the
`RacnetMeas` capture and the diagnostics screen contents, and bring
them to a working session — that bundle is enough to debug from.

## 8. When this needs doing

- **Before iOS transport implementation:** review the recorded foreground
  baseline and remaining M4 gates against the [M5 plan](M5-IOS-PLAN.md).
  iOS will implement the same wire binding, so Android findings inform it.
  Planning does not establish completion of M4 or begin iOS implementation.
- **Remaining M4 validation:** P1–P4 and physical-phone upgrade preservation.
  More device pairs and environments make the results more useful. Keep
  incomplete procedures explicitly pending.
- **After any transport-touching change:** rerun §4 as the regression
  smoke test on real hardware.

## Android preview acceptance

The Android-first scope remains active. Foreground phone checks have passed;
[MEASUREMENTS.md](MEASUREMENTS.md) records the hardware, dates, methods, and
limits. Background survival, throughput/timing/range, and physical-phone
upgrade preservation remain pending; optional third-phone relay is untested.
M4 is not complete. The future [M5 iOS plan](M5-IOS-PLAN.md) does not start iOS
implementation or expand this preview.

The public board is the main screen; diagnostic entry buttons are under
**Peers & tools**. Use this checklist for regression and remaining acceptance:

1. **One phone, offline:** post two messages with mesh off. Force-stop and
   reopen. Both messages and the author identity must remain. Rotate the
   screen with a draft and confirm the draft survives.
2. **Two phones:** enable Bluetooth, grant permissions, turn mesh on, and
   verify both directions of message transfer. Post several messages quickly.
   Force-stop and reopen the receiver; received messages must remain too.
   Consecutive-post convergence has passed, but maximum-rate posting remains
   untested; do not interpret UI automation cadence as a capacity measurement.
3. **Reconnect:** stop the mesh on B, post on A, restart B's mesh, and confirm
   convergence. Repeat after disabling/re-enabling Bluetooth; the app should
   report the stopped radio. Explicitly restart mesh after enabling Bluetooth
   and verify convergence. The recorded pass does not claim automatic restart.
4. **Optional third phone:** connect A–B and B–C with A and C out of radio
   range. Post at A and verify C receives it through B. Reverse the direction.
5. **Screen off:** run P4, recording the actual device/OS/battery settings.
6. **Upgrade:** reinstall a newer APK signed with the same key using `-r`;
   verify messages and identity remain. Debug APKs built on different machines
   may have different signing keys; do not uninstall to resolve this unless
   losing that phone's data is acceptable.

A locally built APK is a test build, not a store release. Keep full logs if
any check fails. Never clear app data simply to work around a startup error
without first deciding whether the messages and identity can be discarded.
