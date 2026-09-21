# ADR 0018 — Device evidence and emulator gates

Status: accepted. Foreground hardware results are recorded in
[MEASUREMENTS.md](../MEASUREMENTS.md); remaining acceptance gates stay separate.

Use a desktop Python/ADB runner rather than adding a remotely callable test
receiver or production debug endpoint. Device selection is explicit. Installation
preserves app data; the runner never uninstalls or clears storage. Test messages
are public and remain in the store, while existing drafts block automatic posting.

The runner reads canonical journal records through debug-only `run-as` and
compares exact entry IDs for delivery and restart checks. It does not replace
core signature validation. Reading may observe an in-progress append, so only
complete records count as evidence. The full journal and identity are not
included in reports. Capture metadata includes the installed APK hash as well
as checkout commit: a checkout alone does not prove which APK was installed.

Keep physical-device logs/screenshots in an ignored local directory. A capture
alone is not a test pass; missing evidence is explicit. Do not automatically
write MEASUREMENTS.md. Physical context and hardware versus emulator status
must be reviewed before promoting results. CI uploads only disposable emulator
evidence, with offline app smoke coverage on the minimum API 29 and target API
35. The emulator-runner CI action supplies emulator lifecycle/KVM setup; it
adds no app/core runtime dependency. Python uses only its standard library.

## Controlled regression scenarios

Keep Bluetooth recovery, draft rotation, and consecutive-post checks in the
same desktop runner, with explicit device selection and exact-entry evidence.
Bluetooth recovery verifies the stopped mesh and visible radio-off error, then
explicitly restarts mesh after the radio is enabled. Shell control is the
default; an explicit Settings-control mode supports OEMs that reject shell
commands. Do not silently tap an arbitrary Settings switch or change radio
auto-off policies to make a test pass.

Rotation checks must observe a real orientation change rather than trusting a
successful settings command. Refuse existing drafts and remove a synthetic draft
only when its text still matches exactly. Restore radio and rotation state on
failure as well as success; restoration errors make evidence incomplete and
must not produce a successful exit status. Keep mesh cleanup independent so a
failed restoration does not skip other cleanup.

Bound consecutive-post counts and describe their UI automation cadence. Exact
convergence and restart persistence are functional evidence, not throughput.
Custom evidence destinations within the repository must be ignored and untracked
before any directory is created. Test these safety and failure paths without
requiring connected phones; a software test pass does not establish a new
hardware measurement.
