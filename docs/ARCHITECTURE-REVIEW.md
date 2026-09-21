# Architecture readiness review

The project aims to be a durable, offline-first sync substrate for intermittent
contacts, with messaging, files, sites, and anchors as clients. The Android
preview demonstrates a narrower but useful foundation: a persistent public
entry set that reconciles across encrypted nearby links. Its boundaries are
documented in [PLAN.md](PLAN.md) and [THREAT-MODEL.md](THREAT-MODEL.md).

## What the current design supports

- The shared Rust core owns canonical entries, signature checks, durable
  storage, reconciliation, framing, and Noise sessions. These responsibilities
  can be reused by other transports and clients through the Node facade.
- The core takes bytes and clocks as inputs rather than owning platform radio
  threads. Segmentation, partitions, reconnects, and malformed traffic can be
  tested independently of an OEM Bluetooth stack.
- Exact entry IDs provide a useful convergence test. Persistence and immutable
  reconciliation snapshots support interruption and retry; accepted changes
  must still notify the host so other live links can reconcile again.
- Android supplies a real transport and public-board client. The recorded
  foreground results establish transfer and persistence on that device pair,
  with the limits in [MEASUREMENTS.md](MEASUREMENTS.md).

These choices fit a reusable substrate. They do not yet establish a complete
multi-platform mesh product.

## Gaps between the preview and the intended system

| Area | Present behavior | Work needed before claiming the broader capability |
| --- | --- | --- |
| Intermittent background contacts | Android service and persistence; foreground acceptance only | Unplugged and locked-device tests, OEM lifecycle handling, and separately measured discovery versus existing-link survival |
| Platform portability | Shared core, Android runtime, iOS scaffold | Resolve the M5 protocol/platform questions, implement the iOS adapter, and test both connection roles across platforms |
| Independent implementations | Versioned protocol specification, fixed vectors, Negentropy conformance and Noise interoperability checks | Keep an implementation-independent conformance corpus for the complete protocol; reusing one Rust core on two platforms alone does not demonstrate that another implementation can interoperate |
| Relay and routing | Android requests reconciliation on other established links after entries arrive | Physical third-hop testing; M6 routing and buffer policy remain separate work, with directed messages requiring an explicit protocol/application design |
| Capacity and long-term operation | Public append-only store stops at 64 MiB or 10,000 entries | Decide retention, admission and recovery policy before wider deployment; a bounded store avoids unbounded growth but can still be filled by valid unwanted content |
| Multiple clients | Kind-tagged opaque entries with a single public dataset; board displays UTF-8 posts | Define client selection and resource budgets; do not equate a generic payload field with implemented subscriptions, private messaging or file transfer |
| Scheduling across platforms | Core exposes sync primitives; Android owns coalescing, retries, and relay scheduling | Reuse behavioral tests for each adapter and decide whether common scheduling should move into the core before implementations diverge |
| Read-path scale | Current app reads stored entry views through FFI for board refresh | Measure worst-case memory and latency at storage limits; add bounded/paged reads if needed before enlarging the store or adding large-content clients |
| Files, fast radios, anchors and sites | Roadmap layers; anchor currently prints the core version | Implement and validate M7–M11 explicitly rather than presenting the architecture diagram as shipped functionality |
| Trust and release | Public signed posts and authenticated encrypted links | Identity verification, admission/abuse policy, threat-tier decisions and external review remain separate gates; link encryption does not make public posts private |

## Development order

1. Keep correctness fixes and their regressions ahead of new transports: accepted
   entries must survive failures and remain observable, dead handshakes must
   expire, and test harness reconnects must not invent successful delivery.
2. Complete the pending Android hardware matrix when physical testing resumes.
   Foreground passes do not substitute for background, performance, range, or
   upgrade-preservation evidence.
3. Resolve the M5 compatibility and integration decisions in
   [M5-IOS-PLAN.md](M5-IOS-PLAN.md), then implement against shared core tests and
   measured Android/iOS behavior. Keep unknown platform behavior explicit.
4. Choose retention/admission and application boundaries before increasing
   content volume or promising sustained open-mesh operation. Record accepted
   architectural choices in ADRs and any wire changes in versioned protocol
   commits before implementation.
5. Build later routing and content layers with their own acceptance gates.
   The public board is a client of the current substrate, not proof that every
   later client or network condition is supported.

This review is an engineering assessment of the current scope, not an external
security audit or a new milestone acceptance result.
