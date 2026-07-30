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
