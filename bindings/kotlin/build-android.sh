#!/usr/bin/env bash
# Build racnet-core for Android and generate Kotlin bindings into the app.
#
# Requires: Android NDK (ANDROID_NDK_HOME or auto-detected by cargo-ndk),
# cargo-ndk (`cargo install cargo-ndk`), and the Rust targets:
#   rustup target add aarch64-linux-android x86_64-linux-android
set -euo pipefail

REPO_ROOT="$(cd "$(dirname "$0")/../.." && pwd)"
JNI_LIBS="$REPO_ROOT/android/app/src/main/jniLibs"
KOTLIN_OUT="$REPO_ROOT/android/app/src/main/java"

cd "$REPO_ROOT"

# --no-default-features leaves the simulator out of shipped artifacts.
cargo ndk -t arm64-v8a -t x86_64 -o "$JNI_LIBS" build --release -p racnet-core --no-default-features

# Bindings are generated from a host build of the same crate; the interface
# metadata is identical across targets.
cargo build --release -p racnet-core --no-default-features
cargo run --release -p uniffi-bindgen -- generate \
    --library "$REPO_ROOT/target/release/libracnet_core.so" \
    --language kotlin \
    --no-format \
    --out-dir "$KOTLIN_OUT"

echo "OK: .so in $JNI_LIBS, Kotlin bindings in $KOTLIN_OUT/uniffi/racnet_core"
