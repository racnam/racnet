# ADR-0012: In-house Noise XX over primitive crates

**Status:** accepted · **Date:** 2026-07-30

## Context

M3 implements PROTOCOL.md §4: `Noise_XX_25519_ChaChaPoly_SHA256` sessions.
The obvious dependency is `snow`, the de facto Rust Noise framework. But
snow's own README disclaims a formal security audit, it is a
framework-sized dependency (resolver machinery, many optional cipher
suites) for one fixed pattern and suite, and the handshake parser is
untrusted-input code — the class ADR-0010 chose to keep in-house. The
Noise state machine itself (CipherState, SymmetricState, HandshakeState,
one pattern) is a few hundred lines against a precise spec with a
canonical vector corpus; the cryptographic primitives underneath it are
exactly what must never be hand-rolled. rust-libp2p made the same split
for its Noise layer.

Determinism is a second constraint: SimNet tests and the §8.7 conformance
vectors need bit-reproducible handshakes, so entropy has to be injectable,
which is awkward through a framework and trivial in our own constructors.

## Decision

Implement the Noise XX state machine in `racnet-core` (`core/src/noise/`),
XX only, no PSK arms, over boring primitive crates. Correctness is checked
three ways rather than asserted: the cacophony/snow vector corpus for
`Noise_XX_25519_ChaChaPoly_SHA256`, live interop against `snow` as a
dev-dependency (both roles, plus the rekey boundary), and the §8.7 spec
transcript, which was generated with snow — an independent implementation
— and must be reproduced byte-exact by ours.

All key material is injected: `HandshakeState::new_xx` takes the static
and ephemeral keypairs explicitly, and `Keypair::generate()` is the single
entropy call in the crate. Long-lived key material (static/ephemeral
secrets, CipherState keys, chaining keys) zeroizes on drop. That claim is
deliberately modest (ADR-0009): transient plaintext buffers and
reallocation copies are not scrubbed. Nothing in `noise/` or `link/` is
exported over UniFFI this milestone; `SecretKey` must never be.

Dependency justifications (mesh core):

- `chacha20poly1305` (RustCrypto): the AEAD itself — the primitive an
  in-house implementation must never include; NCC-audited lineage.
- `hmac` (RustCrypto): Noise's two-output HKDF is ~15 lines over
  HMAC-SHA256; the `hkdf` crate would add a dependency to save them.
- `curve25519-dalek` (direct): X25519 via `MontgomeryPoint::mul_clamped` /
  `mul_base_clamped`; already in the tree transitively via
  `ed25519-dalek`, so a direct use adds zero lockfile entries where
  `x25519-dalek` would add one for newtypes.
- `getrandom`: platform entropy for `Keypair::generate()`; the smallest
  possible entropy surface, already in the lockfile transitively.
- `zeroize`: key-material scrubbing; already in the lockfile via the
  dalek crates.

Dev-only: `snow` as the interop oracle, mirroring the role the upstream
Negentropy suite plays for ADR-0010.

## Consequences

- The handshake parser and epoch state machine stay under this repo's
  review, lint, fuzz, and no-panic discipline.
- We owe byte-exact fidelity to the Noise spec (revision 34); the vector
  corpus and snow interop, not code review, are the arbiter.
- `racnet-core` gains its first entropy dependency; determinism in tests
  survives because entropy never enters implicitly.
- Any future pattern or suite change (PSK, resumption, a second DH) is
  real implementation work here, not a builder-string change. That
  friction is acceptable: the suite is fixed in the spec, and changing it
  is a version bump anyway.

## Alternatives rejected

- **`snow` as a shipped dependency:** an unaudited third-party parser of
  hostile radio input plus a framework's transitive surface, for one
  pattern and one suite; test-only ephemeral injection would also leak a
  test hook into production configuration.
- **`x25519-dalek` / `hkdf`:** each wraps functionality the tree already
  contains in a few lines; two more supply-chain entries for zero
  capability.
- **Skipping zeroization:** free to keep given the crates already ship
  it; "we don't scrub keys" is the wrong default for this threat model.
