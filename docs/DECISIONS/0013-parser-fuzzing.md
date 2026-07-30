# ADR-0013: Parser fuzzing infrastructure

**Status:** accepted · **Date:** 2026-07-30

## Context

The M3 brief item is "fuzz the packet parser." The property tests already
throw arbitrary and mutated bytes at every parser, but property testing
samples a distribution; coverage-guided fuzzing finds the inputs the
distribution never draws. The parsers of untrusted radio input — the
frame/message codec, the link driver's epoch state machine, the Noise
handshake reader — are exactly where ADR-0001 promised this discipline.

cargo-fuzz (libFuzzer) requires a nightly toolchain and its own crate,
which conflicts with the workspace's stable `clippy -D warnings` and
hermetic `cargo test` if included as a member.

## Decision

A `fuzz/` crate at the repo root, excluded from the workspace (its own
`[workspace]` table plus `exclude = ["fuzz"]` in the root manifest), with
three coverage-guided targets:

- `frame_decode`: the outer-frame decoder and inner-message codec under
  arbitrary segmentation, with a decode→encode→decode round-trip check on
  anything that parses.
- `link_driver`: the full link state machine — a scenario byte selects
  role and epoch, an honest deterministic transcript prefix drives the
  driver to that state, and the remaining input arrives as stream bytes.
  Asserts no panic and that a closed driver stays inert.
- `noise_handshake`: `read_message` at each XX step under fixed keys.

A committed seed corpus per target (the §8 conformance frames, an honest
establishment transcript, truncations). `scripts/fuzz.sh` runs all targets
time-boxed (`FUZZ_SECONDS`, default 60), matching the conformance script's
shape; a `fuzz` job in `rust.yml` runs it on nightly in CI. Dev-only
dependencies: `cargo-fuzz`/`libfuzzer-sys` in the excluded crate; nothing
enters the mesh core.

## Consequences

- Every parser of untrusted bytes has a coverage-guided harness that CI
  exercises on every push, time-boxed so the pipeline stays fast; longer
  local runs are one environment variable away.
- The fuzz crate has its own lockfile and toolchain requirement; stable
  workspace commands never touch it.
- A time-boxed CI fuzz is a smoke test, not a campaign; it will catch
  regressions in reachable panics quickly but deep states only with
  corpus growth, which is why crash artifacts and interesting inputs get
  committed back into the corpus.

## Alternatives rejected

- **Property tests only:** already present and kept, but they sample;
  coverage guidance is qualitatively different at finding parser cliffs.
- **AFL++:** heavier setup for no additional value at this crate size;
  libFuzzer integrates with cargo directly.
- **Fuzz crate as workspace member:** breaks stable `cargo clippy
  --workspace` and drags libfuzzer into every developer's default build.
