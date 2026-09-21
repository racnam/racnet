# Racnet

An offline-first, infrastructure-free peer-to-peer **sync substrate** for consumer phones. No servers, no towers, no ISP — the mesh is the people carrying it.

**Status: Android preview; foreground phone checks passed.** Android now has a
public nearby message board, durable signed entries, and automatic BLE sync
and relay. Messages can be written offline and survive process restarts.
The Rust core is tested through simulated transports and the Android runtime.
Two-phone transfer, persistence, reconnect, and controlled Bluetooth recovery
have passed; background survival, throughput, and range remain unverified.
iOS is still a scaffold. This is a sideload preview, not
an audited or public-store release.

## Try the Android preview

Build the APK with the commands below or download the `racnet-debug-apk`
artifact from a successful Android CI run. Install it on Android 10+:

```sh
adb install -r android/app/build/outputs/apk/debug/app-debug.apk
```

Open Racnet, grant permissions, and post a message to the public board.
Turn the mesh on on two nearby phones to exchange messages. The board is
public: peers can read, retain, and relay posts. Link encryption does not
make posts private. Author labels are signing-key identifiers, not verified
real-world identities. Delivery is opportunistic, with no delivery receipt.

Messages and identity survive app restarts and compatible updates. App-data
clearing or uninstalling removes this phone's copy and identity; copies on
other phones remain. Storage is capped at 64 MiB or 10,000 entries. There is
no automatic eviction or remote deletion. Bodies live in private app storage
without separate at-rest encryption; the identity is Keystore-wrapped.

See [preview handoff](docs/ANDROID-PREVIEW.md) for validation results and
[device testing](docs/DEVICE-TESTING.md) for the deferred acceptance
checklist and [release scope](docs/PLAN.md) for what remains outside this
preview.

## What this is (and isn't)

Racnet is a general-purpose, **spec-first** data layer that reconciles a signed, content-addressed data set between nearby devices over Bluetooth Low Energy, with opportunistic upgrades to faster radios (Multipeer/AWDL on iOS, WiFi Aware on Android). Messaging, static "mesh sites", and large-file distribution are clients of that layer, not the layer itself. The Android preview includes a small public message-board client.

The design goal that separates it from existing BLE mesh messengers: a wire protocol specified well enough (`docs/PROTOCOL.md`) that independent implementations can be written and verified against conformance vectors, with set reconciliation (RBSR/Negentropy) rather than ad-hoc gossip as the sync primitive.

**Governing principle:** connectivity is opportunistic, never guaranteed. Every feature must degrade to "syncs eventually, when two phones happen to be near each other." BLE-only correctness first; every faster radio is an optimization.

## Architecture roadmap

The diagram includes future layers. The implemented preview consists of signed
entry storage, reconciliation, Noise sessions, Android BLE, and a public board.
Directed routing, chunk transfer, fast radios, and mesh sites remain roadmap work.

```
App          Chat | Mesh sites | File sharing | Local boards
Sync         Set reconciliation over signed, content-addressed entries
Transfer     Chunk swarming for large binary payloads
Session      Noise_XX — mutual auth, forward secrecy, per-link
Transport    BLE L2CAP CoC (universal) | MPC/AWDL (iOS↔iOS) | WiFi Aware (Android↔Android)
Mesh         Dual-role discovery, topology, store-carry-forward routing
Radio        BLE 5 / BLE Coded PHY / AWDL / NAN
```

One Rust core (`core/`) implements protocol, sync, storage, and crypto. UniFFI
generates Swift and Kotlin bindings. Android hosts the runtime and BLE adapter;
iOS and the Linux anchor are currently scaffolds. See the
[architecture review](docs/ARCHITECTURE-REVIEW.md) for the path from this preview
to the broader substrate and [M5 plan](docs/M5-IOS-PLAN.md) for iOS preparation.

## Repository layout

```
core/           Rust core: protocol, sync, storage, crypto
anchor/         Linux scaffold; full anchor daemon is milestone 9
uniffi-bindgen/ Binding-generator binary for the workspace
bindings/       Swift XCFramework + Kotlin/JNI build scripts
ios/            SwiftUI app (project generated with XcodeGen)
android/        Jetpack Compose app
docs/           PROTOCOL.md (source of truth), ADRs, plans, measurements
```

## Building

```sh
# Core + anchor (any platform with Rust)
cargo test --workspace
cargo run -p racnet-anchor

# Android (needs SDK + NDK + cargo-ndk)
bindings/kotlin/build-android.sh && cd android && ./gradlew assembleDebug

# iOS (needs macOS + Xcode + xcodegen)
bindings/swift/build-xcframework.sh && cd ios && xcodegen && xcodebuild -scheme Racnet build
```

## Honest limitations

- **Background delivery is conditional.** Android unplugged/background acceptance remains pending. iOS has no transport implementation yet; its discovery and restoration restrictions are explicit in the [M5 plan](docs/M5-IOS-PLAN.md).
- **BLE is slow.** ~200 Kbps usable single-hop is the planning estimate; multi-hop divides it. Large files move at walking pace across a mesh, by design.
- **iOS↔Android bulk transfer has no fast path.** AWDL and WiFi Aware are mutually incompatible; cross-platform hops fall back to BLE.
- **Nothing has been security-reviewed.** No external audit has been performed. Do not rely on this project for safety-critical communication, and treat every security property as unverified until an external review is completed. The current [threat model](docs/THREAT-MODEL.md) documents controls and unresolved risks.
- **Throughput/range figures in docs are desk estimates**, not measurements, until they appear in `docs/MEASUREMENTS.md` with the hardware that produced them.

## License

[AGPL-3.0](LICENSE).

For assisted device tests and automatic evidence collection, see
[testing with your phones](docs/TESTING-WITH-PHONES.md).
