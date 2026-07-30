#!/usr/bin/env bash
# Runs the coverage-guided fuzz targets over the racnet parsers of
# untrusted bytes (ADR-0013): the frame/message codec, the link driver
# state machine, and the Noise handshake reader.
#
# Requirements: a nightly toolchain (rustup toolchain install nightly)
# and cargo-fuzz (cargo install cargo-fuzz --locked). Each target runs
# for FUZZ_SECONDS (default 60), seeded from the committed corpus in
# fuzz/seeds/<target>/. Coverage-grown inputs accumulate locally in
# fuzz/corpus/<target>/ (gitignored); promote durable finds into seeds/.
# Crash artifacts land in fuzz/artifacts/<target>/.
set -euo pipefail

root="$(cd "$(dirname "$0")/.." && pwd)"
cd "$root"

if ! rustup toolchain list 2>/dev/null | grep -q '^nightly'; then
    echo "error: the nightly toolchain is required (rustup toolchain install nightly)" >&2
    exit 1
fi
if ! command -v cargo-fuzz >/dev/null; then
    echo "error: cargo-fuzz is required (cargo install cargo-fuzz --locked)" >&2
    exit 1
fi

targets="frame_decode link_driver noise_handshake"
for target in $targets; do
    echo "fuzzing $target for ${FUZZ_SECONDS:-60}s"
    mkdir -p "fuzz/corpus/$target"
    cargo +nightly fuzz run "$target" \
        "fuzz/corpus/$target" "fuzz/seeds/$target" \
        -- -max_total_time="${FUZZ_SECONDS:-60}"
done
echo "fuzz OK: $targets"
