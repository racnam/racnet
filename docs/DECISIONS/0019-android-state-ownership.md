# ADR 0019 — Android state ownership across lifecycle changes

Status: accepted for preview correctness fixes; no wire or product-scope change.

Keep pending board writes in an Activity-retained ViewModel rather than a
screen composition's coroutine scope. A configuration change or temporary
navigation must not cancel the UI's handling of an already committed write.
Persist draft text through saved state; keep active-operation state local to
the process. Clear a submitted draft only if its revision has not changed
while the write was pending. Report storage errors while retaining the draft.
This does not promise exactly-once submission across process death; that would
require a separate durable submission protocol.

Separate correctness-bearing link lifecycle callbacks from best-effort
observations. NodeRuntime delivers establishment directly to the owning
transport while serializing core events. The transport updates peer and
duplicate-link bookkeeping synchronously. These callbacks must not block or
reenter the runtime; teardown reports back asynchronously. The SharedFlow
remains suitable for UI and measurement observations, where dropped events
must not change network correctness.

Retain receive-side resource failures in runtime state so missing or delayed
observers cannot suppress the warning. A new mesh start clears that warning.
When a core close carries final frames, drain queued frames before closing
its socket so a terminal ERROR has a chance to reach the peer. Silent closes
discard queued output immediately. Force socket closure after
a two-second drain deadline; stalled writes must not retain radio resources
indefinitely. Socket failures and explicit service shutdown still tear down
immediately. This deadline is a local resource policy, not a delivery guarantee.

Use immutable snapshots for diagnostics rather than exposing mutable metrics
objects as Compose state. Test state ownership with paused saves, recreated
subscribers, absent or stalled event collectors, and the real registry's
duplicate-link policy. A small connection interface permits those registry
tests without Bluetooth sockets or a new mocking framework.
