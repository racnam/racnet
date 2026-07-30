#!/usr/bin/env bash
# Runs the upstream Negentropy conformance suite against racnet-core's
# engine through the line-protocol harness example (ADR-0010).
#
# Requirements: git, cargo, perl with Session::Token; node for the
# cross-implementation runs. LANGS selects the implementations under test
# (default rust,js — interop matrix, byte-for-byte delta comparison against
# the upstream JS reference, and protocol-version negotiation). Use
# LANGS=rust on machines without node: self-interop and version tests only.
set -euo pipefail

UPSTREAM_REPO="https://github.com/hoytech/negentropy"
# Pinned conformance commit; recorded in ADR-0010. Update deliberately.
UPSTREAM_COMMIT="76f3cf6e69be505e7295edb08a6152fce30261f1"
LANGS="${LANGS:-rust,js}"

root="$(cd "$(dirname "$0")/.." && pwd)"
work="${NEGENTROPY_WORK_DIR:-$root/target/negentropy-conformance}"

if ! perl -MSession::Token -e1 2>/dev/null; then
    echo "error: perl module Session::Token is missing" >&2
    echo "  (Debian/Ubuntu: libsession-token-perl; or cpan Session::Token" >&2
    echo "   built with CCFLAGS=-std=gnu17 on C23-defaulting compilers)" >&2
    exit 1
fi
case ",$LANGS," in
*,js,*)
    if ! command -v node >/dev/null; then
        echo "error: node is required for LANGS containing js" >&2
        exit 1
    fi
    ;;
esac

cargo build --release -p racnet-core --example negentropy_harness

mkdir -p "$work"
if [ ! -d "$work/negentropy/.git" ]; then
    git clone "$UPSTREAM_REPO" "$work/negentropy"
fi
git -C "$work/negentropy" checkout --quiet --force "$UPSTREAM_COMMIT"

# The upstream harness table resolves the rust harness as
# ../../rust-negentropy/target/debug/harness relative to test/ — a sibling
# checkout layout. Provide our binary at that path.
mkdir -p "$work/rust-negentropy/target/debug"
cp "$root/target/release/examples/negentropy_harness" \
    "$work/rust-negentropy/target/debug/harness"

cd "$work/negentropy/test"
perl test.pl "$LANGS"

# test.pl's fuzz.pl invocations are all seed-0 deterministic; follow with a
# time-boxed randomized fuzz pass (FUZZ_SECONDS per language pair).
fuzz_deadline=$(($(date +%s) + ${FUZZ_SECONDS:-60}))
fuzz_runs=0
while [ "$(date +%s)" -lt "$fuzz_deadline" ]; do
    seed=$((RANDOM * 32768 + RANDOM))
    recs=$((RANDOM % 20000 + 100))
    fl1=$((RANDOM % 60000 + 4096))
    fl2=$((RANDOM % 60000 + 4096))
    for pair in $(echo "$LANGS" | tr ',' ' '); do
        SEED=$seed RECS=$recs P1=2 P2=2 P3=7 \
            FRAMESIZELIMIT1=$fl1 FRAMESIZELIMIT2=$fl2 \
            perl fuzz.pl rust "$pair" >/dev/null
    done
    fuzz_runs=$((fuzz_runs + 1))
done
echo "randomized fuzz: $fuzz_runs seeds against: $LANGS"
echo "negentropy conformance OK: $LANGS at ${UPSTREAM_COMMIT:0:7}"
