# Plan — Milestone 3

Scope of this plan: M3 only. Later milestones are listed in the project brief
(a private planning document) and get planned one at a time; "brief §n"
references point into that document.

Completed milestones (plans preserved in git history at this file's path):

- **Milestone 0 — Toolchain proof (complete).** Rust core through UniFFI into
  running iOS and Android apps plus the anchor binary, CI green on all three.
- **Milestone 1 — Protocol spec v0.1 (complete).** PROTOCOL.md v0.1 with
  conformance vectors; framing + CBOR codec + signed entries in `racnet-core`;
  deterministic simulated transport (ADRs 0003–0009).
- **Milestone 2 — Sync core (complete).** In-house Negentropy V1 engine
  proven against the upstream conformance suite; order-statistics entry
  store; sync session layer; end-to-end simulated sync (spec 0.1.1, ADRs
  0010–0011).

Decisions locked before this plan (see ADRs): Rust core + UniFFI (ADR-0001),
deterministic CBOR (ADR-0003), frame layout with the encryption seam between
outer frame and inner message (ADR-0004), fixed-block padding inside the
encryption boundary (ADR-0005), threat-model posture — wire discipline to the
adversarial tier, no error oracles, silent close on crypto failure
(ADR-0009).

---

## Milestone 3 — Noise session layer

**Goal:** links speak `Noise_XX_25519_ChaChaPoly_SHA256` end to end: HELLO
exchange and version negotiation, the XX handshake carried in HANDSHAKE
messages with both HELLO bodies as prologue, an encrypted transport epoch
with counter-based rekeying and lifetime limits, handshake rate limiting,
and fuzzing of every parser that touches untrusted bytes — all proven
against the canonical Noise vectors, live interop with `snow`, and
end-to-end encrypted sync on the simulated transport. Still no radio.

### Deliverables

- Spec patch, PROTOCOL.md 0.1.1 → 0.1.2, its own commit landing before any
  code — clarifications only, wire version stays 1: AEAD associated data is
  empty and the outer `len` prefix is unauthenticated; transport-epoch
  bodies whose length is not 16 + a valid padded length are rejected before
  decryption; rekeying is defined by the CipherState nonce (Rekey when `n`
  reaches a multiple of 1024, `n` not reset; `n = 2^64 − 1` reserved, so
  reaching it closes the link); the 24-hour limit is met by teardown and
  reconnection — no in-band rehandshake in version 1; the initiator MUST
  NOT send HANDSHAKE message 1 before the responder's HELLO arrives; rate-
  limited handshakes are dropped silently, with a SHOULD timeout for
  half-open handshakes; "link" terminology tying transport initiator =
  Noise initiator = even-sid opener; §8.7 conformance vectors — pinned
  static and ephemeral keys giving a byte-exact transcript of two HELLO
  frames, three HANDSHAKE frames, and one transport frame; stale-text
  fixes (§5 "spec 0.1.0", header ADR range).
- `core/src/noise/`: an in-house Noise XX engine over primitive crates —
  `Keypair`/`PublicKey`/`SecretKey` (X25519 via `curve25519-dalek`
  directly), `Fingerprint` = SHA-256 of the static public key,
  `CipherState` (hand-built 4-zero-bytes ‖ LE64 nonce, Noise `Rekey`),
  `SymmetricState` (two-output HKDF over `hmac` + `sha2`),
  `HandshakeState::new_xx` with injected ephemerals (determinism for
  vectors and simulation; production uses `Keypair::generate()` over
  `getrandom`), `TransportState` owning the rekey-every-1024 policy.
  Long-lived key material zeroized on drop. No panics on untrusted input;
  no UniFFI exports this milestone (ADR-0012).
- `core/src/link/`: `LinkDriver`, a sans-I/O state machine with injected
  time — HELLO exchange and version negotiation (empty intersection is the
  only wire-visible cleartext failure: ERROR 2 then close; everything else
  closes silently), XX sequencing, transport-epoch encrypt/decrypt around
  the existing `encode_message`/`FrameDecoder::next_body` seam with the
  pre-decrypt length gate (`wire::pad::is_padded_len`), demux to an owned
  `Syncer` (its API unchanged), encrypted ERROR for post-handshake
  protocol violations, 24-hour and nonce-lifetime enforcement, local-only
  `CloseReason`. `HandshakeLimiter`: per-remote-address token bucket
  (burst 3, refill 1 per 5 s), half-open cap 32 with timeout, bucket
  garbage collection so the limiter is not itself a memory-DoS lever.
- Conformance and interop: cacophony Noise vectors for
  `Noise_XX_25519_ChaChaPoly_SHA256` transcribed into
  `core/tests/noise_vectors.rs`; `snow` as a dev-dependency in
  `core/tests/noise_interop.rs` — both roles, transport traffic both ways,
  and the rekey boundary at 1024; §8.7 spec transcript reproduced
  byte-exact by two `LinkDriver`s in `core/tests/vectors.rs`.
- Tests: property tests — garbage bytes never panic in any driver state,
  bit-flipped or truncated transcripts close silently with zero output
  frames, epoch violations rejected, `read_message` total at every step;
  SimNet end-to-end encrypted sync with partition/heal and full
  re-handshake on reconnect, reproducible by seed; transport-epoch bytes
  verified to be ciphertext; >1024 messages each way across the rekey
  boundary; 24-hour expiry via injected time; limiter unit tests with an
  injected clock.
- Fuzzing: `fuzz/` cargo-fuzz crate excluded from the workspace, targets
  `frame_decode`, `link_driver` (every state × role, honest prefix then
  arbitrary bytes), and `noise_handshake`; committed seed corpus;
  `scripts/fuzz.sh` time-boxed via `FUZZ_SECONDS`; a `fuzz` CI job in
  `rust.yml` on the nightly toolchain (ADR-0013).
- ADR-0012 (in-house Noise over primitive crates, snow as dev-only oracle,
  dependency justifications, entropy injection, zeroization scope) and
  ADR-0013 (parser-fuzzing infrastructure).

### Order of work

Plan commit, then the spec patch commit, then ADRs, then implementation
commits (noise primitives → handshake and transport with vectors → snow
interop → rate limiter → link driver → simulated end-to-end), then fuzz
infrastructure, then changelog/README. Every commit passes fmt, clippy -D
warnings, and the full test suite.

### Done when

- `cargo fmt --all -- --check`, `cargo clippy --workspace --all-targets --
  -D warnings`, `cargo test --workspace` pass locally; anchor smoke run
  still prints the version.
- Noise vector tests, snow interop (both roles and the rekey boundary),
  the §8.7 transcript test, and the simulated encrypted end-to-end sync
  all pass.
- `scripts/fuzz.sh` runs clean locally within its time box.
- All CI workflows green on the GitHub remote, including the new fuzz job.
- An independent fresh-context review of the diff against this plan has
  been done and any gaps are resolved.
