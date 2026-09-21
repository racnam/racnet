# Delegate phone testing and recording

When both Android phones are available, connect them to the computer running
this checkout, unlock them, enable **Developer options → USB debugging**, and
accept each phone's USB debugging authorization dialog. Enable Bluetooth on
both; Android 10/11 also needs the system Location switch on for discovery.
Then say **“The phones are connected; run the tests and record the results.”**
No manual transcription or screenshots are needed for the baseline tests.
Use spare/test phones or be comfortable retaining public synthetic test posts.

## What can be automated

With authorized ADB access, the test runner can install the debug APK in place,
grant its runtime permissions, open the board, post unique markers, read the
app's committed entry IDs, verify transfer in both directions, restart the
receiver, and check offline retention and reconnect catch-up. Additional commands
check Bluetooth off/on recovery, draft retention through rotation, and batches
of consecutive posts. It also records
models, OS/build versions, package versions, git commit, UTC times, test steps,
app-tagged logcat, screenshots, and UI XML in one evidence directory.

The desktop reader uses `run-as`, so this workflow requires a **debug APK**.
It compares exact canonical signed-entry hashes, rather than treating a peer
count or `sync_done` log as proof of delivery. The Rust core verifies signatures
before storing entries; the Python reader does not independently verify them.
It never saves the full journal or identity seeds into the evidence bundle.

Preparation does not uninstall, clear app data, bypass a screen lock, change
battery exemptions, or disable device security. A signing-key mismatch stops
installation; do not uninstall merely to make an update work. Keep the same
local debug keystore between builds. CI debug APKs may use different keys.

## What still needs your hands

- Plug in/unlock phones and accept USB/debugging or OEM system dialogs.
- Move phones and describe actual distances, walls, and surroundings for range.
- Choose the battery settings being tested and leave phones in that condition.
- For screen-off/battery claims, unplug charging cables when the procedure
  calls for it. USB power changes idle behavior. Long physical tests can use
  wireless ADB on a trusted local network or collect logs after reconnecting;
  setup and permissions vary by phone. Do not expose ADB to the Internet.

A two-phone USB baseline does not establish unplugged background survival,
range, third-hop relay on real radios, or a security review.

## Commands (optional; these can be run for you)

Python 3, `adb`, and Git are needed. From the repository root:

```sh
# Read-only: show connected/unauthorized devices.
python3 scripts/device_test.py inventory

# Install the selected APK and grant runtime permissions on both phones.
python3 scripts/device_test.py prepare \
  --serial SERIAL_A --serial SERIAL_B --apk racnet-0.2.0-preview.apk

# One-phone offline post and process-restart persistence.
python3 scripts/device_test.py offline --serial SERIAL_A

# Bidirectional sync, received-message persistence, and reconnect catch-up.
python3 scripts/device_test.py pair --serial SERIAL_A --serial SERIAL_B \
  --timeout 90 --note 'Same room, screens on, USB powered; distance not measured'

# Passive capture while you perform a physical scenario; does not grade it.
python3 scripts/device_test.py capture --serial SERIAL_A --serial SERIAL_B \
  --seconds 600 --note 'Describe the actual scenario and settings here'

# Toggle each phone's Bluetooth, check the radio-off warning, restart mesh,
# and verify queued messages and replies by exact entry ID.
python3 scripts/device_test.py bluetooth-recovery \
  --serial SERIAL_A --serial SERIAL_B --timeout 90

# For devices that reject shell Bluetooth control, use the Settings switch.
# The same selected control method is used on both phones.
python3 scripts/device_test.py bluetooth-recovery \
  --serial SERIAL_A --serial SERIAL_B --bluetooth-control settings

# Check a synthetic draft across rotation on each explicitly selected phone.
python3 scripts/device_test.py draft-rotation \
  --serial SERIAL_A --serial SERIAL_B

# Post five messages on each phone, then check convergence and persistence.
python3 scripts/device_test.py batch \
  --serial SERIAL_A --serial SERIAL_B --count 5 --timeout 90
```

`offline`, `pair`, `bluetooth-recovery`, and `batch` post synthetic public
messages and leave them in the store. Functional tests stop the mesh at the
end. Existing unsent drafts block posting rather
than being overwritten. Keep the phones unlocked on Racnet during automated
UI steps. A notification, lock screen, or unexpected layout can stop a run;
that is a test-runner limitation to investigate, not proof the radio failed.
Only `prepare` installs/grants permissions. Every device must be explicitly
selected; the script does not auto-pick a connected personal phone.

`bluetooth-recovery` requires Bluetooth on initially. It verifies actual radio
state changes and the app's stopped-mesh/error state before enabling the radio
and explicitly starting mesh again. This does not test automatic mesh restart.
The default shell control must work on both phones; use the explicit Settings
mode for OEMs that reject it. Settings mode accepts only a uniquely identified
Bluetooth switch and stops for an unsupported layout. Radio state is restored
after the scenario, including failure paths. Bluetooth auto-off policies and
battery exemptions are not changed by the runner.

`draft-rotation` refuses existing drafts, enters a unique synthetic draft, and
requires an observed orientation change plus exact retained text. It restores
the original rotation settings and removes only its unchanged synthetic draft.
If the text has changed, it is preserved and cleanup is reported as incomplete.

`batch` accepts `--count` from 1 to 20, defaulting to five per phone. It finishes
posting both batches before checking remote delivery, then verifies every entry
on both phones again after app restarts. The pace includes UI automation and
local persistence checks; it is not a maximum-rate or radio-throughput test.

## Evidence and recording

Each run creates an ignored `test-runs/<UTC-time>-<id>/` directory:

- `SUMMARY.md`: device matrix and per-step results.
- `report.json`: exact entry IDs, authors, step evidence/errors, and build context.
- `device-N/device.json`, `logcat.txt`, `events.json`, `ui.xml`, `screen.png`.

Screenshots/XML can include board messages and device identifiers. Evidence
stays local and is excluded from git. Review/redact it before sharing. No logs
are uploaded by this script. CI uploads only its own disposable emulator data.
Custom `--output` directories inside the checkout must also be ignored and
untracked; the runner rejects other repository destinations before writing.
An output directory outside the checkout remains private evidence and must not
be uploaded without review.

A passive capture is `captured_not_evaluated`, never an automatic pass.
Missing evidence is marked incomplete. Timing is desktop-observed automation
latency, not BLE throughput. Event counts may include the start-second boundary
and are diagnostic only. Emulator results are explicitly identified.
Failed state restoration or draft cleanup prevents a complete passing result
and produces a nonzero exit status.

After a run, the evidence can be analyzed here, failures fixed, and a concise
verified result added to `docs/MEASUREMENTS.md`. Physical conditions not present
in logs must come from you. Measurements are never automatically promoted from
emulator runs, inferred from event counts, or fabricated to fill a table.
