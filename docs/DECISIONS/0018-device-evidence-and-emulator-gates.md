# ADR 0018 — Device evidence and emulator gates

Status: accepted for testing delegation with hardware acceptance deferred.

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
