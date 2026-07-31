#!/usr/bin/env bash
# Build racnet-core for iOS device + simulator, generate Swift bindings, and
# package RacnetCore.xcframework. macOS only.
#
# Requires Xcode command-line tools and the Rust targets:
#   rustup target add aarch64-apple-ios aarch64-apple-ios-sim x86_64-apple-ios
set -euo pipefail

REPO_ROOT="$(cd "$(dirname "$0")/../.." && pwd)"
OUT="$REPO_ROOT/bindings/swift/out"
SWIFT_SRC="$REPO_ROOT/ios/Sources/Generated"

cd "$REPO_ROOT"
rm -rf "$OUT"
mkdir -p "$OUT/headers" "$SWIFT_SRC"

# --no-default-features leaves the simulator out of shipped artifacts.
for target in aarch64-apple-ios aarch64-apple-ios-sim x86_64-apple-ios; do
    cargo build --release -p racnet-core --target "$target" --no-default-features
done

cargo run --release -p uniffi-bindgen -- generate \
    --library "$REPO_ROOT/target/aarch64-apple-ios/release/libracnet_core.a" \
    --language swift \
    --no-format \
    --out-dir "$OUT/gen"

# The FFI header + modulemap go into the framework; the .swift wrapper is
# compiled as app source. UniFFI requires the modulemap be named module.modulemap.
cp "$OUT/gen/racnet_coreFFI.h" "$OUT/headers/"
cp "$OUT/gen/racnet_coreFFI.modulemap" "$OUT/headers/module.modulemap"
cp "$OUT/gen/racnet_core.swift" "$SWIFT_SRC/"

# Simulator slices must be lipo'd into one library per platform.
mkdir -p "$OUT/sim"
lipo -create \
    "$REPO_ROOT/target/aarch64-apple-ios-sim/release/libracnet_core.a" \
    "$REPO_ROOT/target/x86_64-apple-ios/release/libracnet_core.a" \
    -output "$OUT/sim/libracnet_core.a"

xcodebuild -create-xcframework \
    -library "$REPO_ROOT/target/aarch64-apple-ios/release/libracnet_core.a" \
    -headers "$OUT/headers" \
    -library "$OUT/sim/libracnet_core.a" \
    -headers "$OUT/headers" \
    -output "$OUT/RacnetCore.xcframework"

echo "OK: $OUT/RacnetCore.xcframework, Swift bindings in $SWIFT_SRC"
