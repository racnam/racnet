# Measurements

Measured values from real hardware only. Desk estimates (brief §15) are never
copied here — if a number has no row in this file, it has not been measured.

Each row records the value, the exact hardware/OS on both ends, the
methodology, and the date.

## Radio throughput

| Path | Value | Devices (both ends) | OS versions | Method | Date |
|---|---|---|---|---|---|
| — | — | — | — | — | — |

## Range

| Radio | Environment | Value | Devices | Method | Date |
|---|---|---|---|---|---|
| — | — | — | — | — | — |

## Timing

| Event | Value | Device | OS | Method | Date |
|---|---|---|---|---|---|
| — | — | — | — | — | — |

## Device matrix test log

| Date | Devices | Scenario | Result | Notes |
|---|---|---|---|---|
| 2026-09-21 (UTC) | Google Pixel 6a / Android 17 | Offline post and process restart | Passed | ADB runner verified the retained entry ID and author with mesh disabled. |
| 2026-09-21 (UTC) | Samsung SM-A125U / Android 12 | Offline post and process restart | Passed | ADB runner verified the retained entry ID and author with mesh disabled. |
| 2026-09-21 (UTC) | Google Pixel 6a / Android 17 ↔ Samsung SM-A125U / Android 12 | Bidirectional transfer, receiver restart, and reconnect catch-up | Passed | ADB runner verified exact signed-entry IDs in both stores, received-entry retention after restarting the Samsung, and delivery of a queued entry after reconnecting it. |
| 2026-09-21 (UTC) | Google Pixel 6a / Android 17 ↔ Samsung SM-A125U / Android 12 | Controlled Bluetooth off/on recovery on each phone | Passed per-device checks | Each phone showed the radio-off error and stopped mesh. After enabling Bluetooth and restarting mesh, queued entries and replies reached the opposite store. The initial run stopped at an unsupported Samsung shell toggle; the Samsung checks passed on rerun using its Settings switch. |
| 2026-09-21 (UTC) | Google Pixel 6a / Android 17; Samsung SM-A125U / Android 12 | Draft retention across portrait-to-landscape rotation | Passed on both | Verified the draft text after an observed display rotation; restored rotation settings and removed only the synthetic draft afterward. |
| 2026-09-21 (UTC) | Google Pixel 6a / Android 17 ↔ Samsung SM-A125U / Android 12 | Five consecutive posts per phone, convergence, and app restarts | Passed | Posted each batch without waiting for remote delivery between posts. Verified all ten exact entry IDs on both phones after restarting both apps. UI automation cadence; not a maximum-rate load test. |

These are foreground functional checks using the 0.2.0 preview APK. Raw evidence
is retained locally. Distance and surroundings were not recorded; these runs
establish no range, isolated throughput, or unplugged background-survival result.
Maximum-rate posting, unplugged screen-off survival, and third-phone relay
remain untested in this hardware baseline. Bluetooth recovery requires explicitly
starting the mesh again after enabling the radio; automatic restart was not claimed.
