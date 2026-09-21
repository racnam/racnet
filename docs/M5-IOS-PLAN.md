# M5 — iOS BLE transport plan

Status: proposed implementation plan; no iOS transport implementation or
hardware acceptance is claimed. Android remains the preview release target
under [ADR-0017](DECISIONS/0017-android-first-release.md). M4 foreground
results are in [MEASUREMENTS.md](MEASUREMENTS.md); unplugged background,
range, throughput/timing, and physical-device upgrade preservation remain
separate acceptance work. This plan does not close those gates.

## Starting point and scope

`ios/` is the M0 SwiftUI version-display scaffold, with an iOS 16 deployment
target. `bindings/swift/build-xcframework.sh` builds device/simulator slices
and generates Swift bindings. The existing macOS workflow builds the app
for the simulator; it does not run Swift tests or establish radio behavior.

The core already provides `Node.open`, identity generation, entry persistence,
`connect`/`accept`, byte input, typed events, ticks, and reconciliation through
[ADR-0014](DECISIONS/0014-ffi-node-facade.md). M5 should host that facade,
not duplicate Noise, framing, storage, or sync in Swift. No new FFI surface
is assumed necessary until a concrete integration gap is demonstrated.

M5 covers dual-role CoreBluetooth, L2CAP streams, durable identity and entries,
restoration, bounded background work, and Android/iOS interoperability.
A minimal public-board and diagnostics surface can exercise the existing
kind-1 UTF-8 posts and 4096-byte app limit. Private messages, new identity
verification, M6 routing, files, fast radios, and store distribution are out
of scope. Product decisions still open in `gossip-brief.md` §14 remain with
the maintainer; this plan does not approve an iOS release or a security tier.

## Resolve before implementation

Record an iOS integration ADR before adopting the proposed architecture below.
It must cover identity storage/accessibility, file protection and backup,
execution ownership, restoration, and bounded queues. Preserve unreadable
identity or journal data and report failure; never silently rotate identity
or erase posts to recover startup.

Two transport details require an explicit compatibility review against
[PROTOCOL.md](PROTOCOL.md) §9.1, which remains authoritative:

- §9.1.6 requires an observed six-octet BLE address for the handshake limiter.
  CoreBluetooth exposes a system-assigned [peer UUID](https://developer.apple.com/documentation/CoreBluetooth/CBPeer/identifier).
  Do not pretend this is a MAC address or truncate it into one. Specify the
  platform-local limiter key and its limitations in an ADR and a separately
  versioned protocol clarification before implementing the affected path.
  Preserve the global half-open cap independently of any per-peer key.
- §9.1.3 says the GATT connection SHOULD close before L2CAP opens. Validate
  the connected-peripheral lifecycle required by CoreBluetooth; document any
  justified platform exception and resolve needed spec text before code.

Neither issue justifies inventing a new over-the-air identifier or frame.
The current UUIDs, little-endian two-byte PSM characteristic, Noise roles,
stream framing, connection jitter, and duplicate-link tiebreak remain the
binding in [ADR-0015](DECISIONS/0015-ble-transport-binding.md).

## Platform constraints to design and test

Apple assigns the PSM when publishing an L2CAP listener. Publish with
`withEncryption: false`, advertise the protocol service, and expose the
current PSM through its read-only characteristic only when the listener is
ready. Republish safely after Bluetooth resets; never serve a stale PSM.
Apple leaves PSM discovery to the application. [L2CAP publication](https://developer.apple.com/documentation/corebluetooth/cbperipheralmanager/publishl2capchannel(withencryption:))

Declare both Bluetooth background modes and a meaningful
`NSBluetoothAlwaysUsageDescription`. Background central discovery and
connection are permitted, with coalesced discoveries and slower scanning.
Background peripheral service UUIDs move to an overflow area that Apple
documents as discoverable only by explicitly scanning iOS peers. Therefore
Android discovery of a backgrounded iOS advertiser cannot be assumed; test
iOS-as-central connecting to Android separately. The brief's categorical
claim that background iOS cannot initiate new connections is not a valid
implementation premise. [Apple background guide](https://developer.apple.com/library/archive/documentation/NetworkingInternetWeb/Conceptual/CoreBluetooth_concepts/CoreBluetoothBackgroundProcessingForIOSApps/PerformingTasksWhileYourAppIsInTheBackground.html)

Restoration is event-driven and conditional. Use stable, distinct restoration
identifiers for both managers, recreate them during launch, and handle both
restoration delegates. Do not assume that restored managers preserve live
L2CAP streams, Rust link IDs, or Noise state; validate channel recovery and
reconnect with a fresh handshake when necessary. Test system termination,
force quit, Settings Bluetooth power changes, Control Center changes, and
reboot separately. Apple's current relaunch table includes iOS 26
AccessorySetupKit qualifications for some user actions; Racnet must not
claim those exceptions or adopt accessory setup without a separate design.
[Apple restoration rules](https://developer.apple.com/documentation/technotes/tn3115-bluetooth-state-restoration-app-relaunch-rules)

`BGAppRefreshTask` provides execution opportunities at the system's discretion.
Register and reschedule it, implement expiration cancellation and completion,
and make useful progress even if it never runs. An earliest begin date is
not a delivery deadline; the brief's 15–30 minute cadence is not a guarantee.
[Apple scheduling contract](https://developer.apple.com/documentation/backgroundtasks/bgtaskrequest/earliestbegindate)

The existing iOS 16 target stays the baseline. Newer Live Activity Bluetooth
privileges are a separate future evaluation, not a dependency of M5 or an
assumed cross-platform background-discovery fix. [Current CoreBluetooth overview](https://developer.apple.com/documentation/corebluetooth)

## Proposed architecture

- One app-owned runtime serializes FFI calls, event dispatch, connection
  ownership, and reconciliation scheduling. UI snapshots reach the main
  actor; radio and storage work must not block it. Use injected monotonic
  microseconds for link time and wall-clock milliseconds for entry ordering.
- Central and peripheral adapters own their CoreBluetooth delegates and
  explicit states for permission, power, discovery, connection, and shutdown.
  User mesh-off cancels pending work as well as active channels; restoration
  must honor that intent. Handle denied permission and unavailable Bluetooth
  visibly, with no automatic system-setting changes.
- Each `CBL2CAPChannel` has one stream pump and one core link. Handle partial
  reads/writes, no-progress writes, EOF, errors, ordering, bounded buffering,
  and exactly-once teardown. Deliver returned frames before associated close
  handling when possible. CoreBluetooth exposes Foundation input/output
  streams, not Android socket/MTU semantics. [Channel API](https://developer.apple.com/documentation/corebluetooth/cbl2capchannel)
- Port the behavior of Android's `SyncScheduler`, connection policy, and
  duplicate policy into testable Swift components: one local sync per link,
  coalesced dirty requests, retry refused starts, and dirty other live links
  when an entry arrives. Do not copy Android service lifecycle assumptions.
- Propose Keychain-backed seed storage and a private persistent journal.
  The ADR must choose accessibility and file protection together so locked
  operation is intentional, specify backup/migration policy, and test first
  unlock and unavailable-protected-data behavior. Foreign seed copies remain
  outside Rust's zeroization guarantee; never put seeds in logs.
- The priority work queue is local scheduling: lifecycle/handshake and stream
  progress before new bulk work, bounded by available execution time. It
  does not add message-priority wire fields or resume stale Noise sessions.
  The ADR must resolve the brief's disk-backed work requirement: define what
  scheduling intent is persisted and what can be reconstructed from the journal,
  with restart/interruption tests proving work is recovered without depending
  on an in-memory queue. Persist accepted entries and reconcile again after
  interruption. Tick during active execution and on resume; do not rely on
  timers running while suspended.

## Implementation phases and gates

| Phase | Deliverable | Required gate |
| --- | --- | --- |
| 1. Contracts | iOS ADR; resolve limiter-key and connection-lifecycle questions; approved spec clarification if needed | Spec commit precedes affected implementation; no unresolved protocol workaround |
| 2. Runtime and storage | Swift runtime, identity/journal integration, offline board, fake transports and clocks | Swift tests with actual generated bindings; offline post/restart and storage-failure cases; macOS build |
| 3. Foreground BLE | Dual roles, GATT PSM service/read, L2CAP pump, jitter, dedup, sync scheduling | Physical Android/iOS transfer with each side initiating; iOS/iOS dual-role check; reconnect and persistence |
| 4. Lifecycle | Both restoration paths, mesh intent, protected-data handling, priority queue, BGAppRefresh expiration | Deterministic lifecycle tests plus device observations for background, termination, reboot and power changes |
| 5. Acceptance | Reproducible testing guide, diagnostics, sanitized measurements and limitations | Review full diff against activated milestone plan; all implemented scope tested or explicitly deferred |

Host tests must exercise segmented/coalesced input, short writes, queue overflow,
stale callbacks after close, simultaneous links, sync refusal/retry, updates
during reconciliation, clock progression across sleep, permission/power state
changes, restored managers, unavailable storage, and task expiration. Add a
Swift test target and macOS test job rather than treating a successful build
as runtime validation. Keep the existing Rust and Android regressions green.

Physical acceptance needs Android/iOS and iOS/iOS roles recorded explicitly:
both-direction posts, batch posting, offline/restart persistence, reconnect
catch-up, Bluetooth recovery with and without restarting mesh, user mesh-off,
locked/unplugged receiver, both apps backgrounded, and restoration scenarios.
Separate foreground discovery from established-link survival and new
background discovery. Report unsupported cases as limitations, not passes.
Range, throughput, energy and timing require measurements; brief estimates
are not acceptance results.

Use [DEVICE-TESTING.md](DEVICE-TESTING.md) as the acceptance pattern, but do
not assume the ADB runner can drive iOS. Define an iOS capture/manual procedure
when implementation begins. Keep raw evidence local and ignored; publish only
sanitized project results with model/OS, conditions and reproducible steps.

Linux can review Swift and run shared-core checks; it cannot validate Xcode,
Keychain integration or CoreBluetooth. macOS CI is the build/test gate;
simulator success is not physical BLE or background acceptance. Activate M5
in [PLAN.md](PLAN.md) in its own implementation session after this preparation,
carrying forward the unresolved Android acceptance items explicitly.
