# Measurement procedures

Repeatable procedures whose results go into `docs/MEASUREMENTS.md`. That
file records measured values only; this one records how to measure them.
Rows always include both devices' models and OS versions, the procedure
id as the method, and the date.

All procedures capture the app's measurement records:

```sh
adb logcat -v time -s RacnetMeas
```

Every record is one line, `MEAS event=<name> key=value ...`, timed with
the device's monotonic clock. The diagnostics screen shows the same
per-link numbers and offers copy-to-clipboard.

## P1 — Radio throughput (BLE L2CAP CoC)

1. Two devices, 1 m apart, foreground, screens on, mesh service running
   on both, both otherwise idle.
2. On device A create one 100 KiB test entry (status screen). Device B
   holds no entries.
3. Sync runs automatically on the established link. Read
   `tput_in_kbps` and `bytes_in` from B's `MEAS event=sync_done` line
   (or B's diagnostics screen).
4. Repeat 3×, deleting the app's data on B between runs (fresh store).
   Record the median in the *Radio throughput* table, path
   "BLE L2CAP CoC, 1 hop, 1 m", method "P1".
5. Variant worth one row: 10 × 10 KiB entries instead of 1 × 100 KiB —
   many-entry sync exercises reconciliation differently than one large
   payload.

## P2 — Timing (discovery, establishment, sync)

1. Setup as P1. Force-stop the app on both devices, then start both and
   enable the mesh, so discovery starts cold.
2. From the dialing device's `MEAS` lines and diagnostics phases,
   record rows in the *Timing* table for: `scan->gatt`, `gatt->psm`
   (the §9.1.3 GATT read cost), `psm->l2cap`,
   `l2cap->established` (HELLO exchange + XX handshake), and
   `established->sync_done` for a known entry delta (state the delta in
   the row's notes). Method "P2".

## P3 — Range

1. Start as P1 at 1 m in a named environment (open field; indoor
   through walls — say which walls).
2. Create a 10 KiB entry every ~10 s on A while walking B away in
   ~5 m steps. Watch B's `MEAS` lines: sync events continuing means the
   link lives; `link_closed` followed by no re-establishment within
   60 s means out of range.
3. Record the last distance at which sync still completed, per
   environment, in the *Range* table. Method "P3".

## P4 — Device matrix (background survival)

Scenarios per device pair, each a row in the *Device matrix test log*
(result + notes, including whether the battery-optimization exemption
was granted):

1. **Foreground↔foreground:** baseline; P1 must pass.
2. **Screen off 10 min:** turn both screens off, wait 10 minutes,
   create an entry via `adb shell am start` + the status screen (or on
   a third device), verify it syncs while screens stay off.
3. **Doze:** `adb shell dumpsys deviceidle force-idle` on B, create an
   entry on A, record whether and when B syncs; then
   `deviceidle unforce`.
4. **OEM battery policy:** repeat scenario 2 with the exemption denied,
   then granted, on each vendor's device. dontkillmyapp.com documents
   the per-vendor settings that matter.

Aggressive-OEM devices (Samsung, Xiaomi, Huawei, Oppo…) are the point
of this procedure: whether a `connectedDevice` foreground service alone
survives their killers is an open question the log answers per vendor
(ADR-0016).
