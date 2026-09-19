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

## P1 — Link-average sync throughput (BLE L2CAP CoC)

The diagnostics/log field `link_avg_in_kbps` is a cumulative link average,
including handshake, reconciliation, and idle time. It is **not** isolated
radio or payload-transfer throughput. Do not compare a long-idle connection
with a fresh link. Old `tput_in_kbps` logs used a different denominator and
must not be used for this procedure.

1. Use disposable test app data. On A, with mesh off, create exactly one
   48 KiB test entry under **Peers & tools**. B must have an empty store.
   A 100 KiB entry does not fit a single protocol frame and is not supported.
2. Put the phones 1 m apart, screens on, and start both meshes. Capture logs.
3. Confirm B holds A's entry. Use B's first `sync_done` record reporting
   `entries=1`, with `bytes_in`, `link_dur_ms`, and `link_avg_in_kbps`.
4. Stop both meshes. Clear **B's disposable test data only**, relaunch,
   grant permissions, and repeat for three fresh connections. Reuse the
   same single entry on A. Record the median, label it "BLE link-average
   sync, 48 KiB, 1 hop, 1 m (includes handshake)", method "P1".
5. A separate variant can use four 10 KiB entries. Record the actual total
   and use the record reporting all four entries. Never mix the two trials.

For isolated radio throughput, additional transport instrumentation is
required; no such number should be inferred from the existing lifetime
counters.

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
