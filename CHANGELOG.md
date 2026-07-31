# Changelog

All notable changes to this project will be documented in this file.

The format is based on [Keep a Changelog](https://keepachangelog.com/en/1.1.0/),
and this project adheres to [Semantic Versioning](https://semver.org/spec/v2.0.0.html).

## [Unreleased]

### Added

- Repository scaffold: protocol spec skeleton, ADRs, CI, license.
- Milestone 0 toolchain proof: `racnet-core` Rust crate exposing `version()`
  through UniFFI to a SwiftUI iOS app, a Jetpack Compose Android app, and the
  `racnet-anchor` Linux binary.
- Wire protocol spec v0.1 in `docs/PROTOCOL.md`: framing, message type
  registry, CDDL payload schemas, Noise session establishment, version
  negotiation, RBSR/Negentropy message set, error handling, and conformance
  vectors (ADRs 0003–0008).
- Milestone 1 implementation in `racnet-core`: frame and message codec with
  fixed-block padding, signed entry encoding and identity, conformance-vector
  and round-trip property tests, and a deterministic simulated transport with
  configurable latency, loss, partition, and MTU.
- Threat-model posture (ADR-0009): adversarial-tier wire discipline, modest
  security claims until an external audit.
- Spec 0.1.1: session ids carry direction via parity, and the maximal
  sort key is reserved.
- Milestone 2 sync core in `racnet-core`: in-house Negentropy V1 engine
  validated against the pinned upstream conformance suite in CI
  (ADR-0010); entry store with a weight-balanced order-statistics index
  and sealed per-session snapshots (ADR-0011); session layer driving
  reconciliation and entry transfer over the message codec, tested
  end-to-end on the simulated transport.
- Spec 0.1.2: session-layer clarifications — empty AEAD associated data,
  the pre-decryption ciphertext-length gate, rekeying defined by the
  CipherState nonce, teardown-and-reconnect as the only rehandshake,
  no handshake pipelining past the HELLO exchange, silent rate-limit
  refusals — and §8.7 establishment vectors generated with an
  independent Noise implementation.
- Spec 0.2.0: §9 transport bindings — the BLE L2CAP CoC binding with the
  advertised service UUID, GATT-published PSM, insecure-channel rule,
  duplicate-link tiebreak, and the BLE address as the rate-limiting key.
- Milestone 4 Android BLE transport: the first real FFI surface — a
  `Node` facade over store, limiter, and links, driven by link ids with
  return-value events (ADR-0014) — and the first radio: dual-role BLE
  with L2CAP CoC links, GATT PSM discovery, a `connectedDevice`
  foreground service, Keystore-wrapped identity, permission and
  battery-optimization onboarding, a diagnostics screen, and stable
  `RacnetMeas` measurement records with procedures in
  `docs/MEASUREMENT-PROCEDURES.md` (ADRs 0015–0016). The simulator is
  feature-gated out of shipped mobile artifacts.
- Milestone 3 session layer in `racnet-core`: in-house Noise XX engine
  (`Noise_XX_25519_ChaChaPoly_SHA256`) over primitive crates, validated
  against the cacophony vectors, live snow interop, and the §8.7
  transcript (ADR-0012); sans-I/O link driver running HELLO/version
  negotiation, the handshake, and the encrypted transport epoch with
  counter-based rekeying, lifetime caps, and silent-close failure
  discipline; per-address handshake rate limiter; end-to-end encrypted
  sync in simulation; coverage-guided fuzz targets for every parser of
  untrusted bytes, time-boxed in CI (ADR-0013).
