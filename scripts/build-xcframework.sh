#!/usr/bin/env bash
# Builds the Rust engine for iOS device, iOS simulator and macOS, generates the
# Swift bindings, and packages apps/ios/Packages/MLEngine/MLEngineFFI.xcframework.
# The macOS slice is arm64 only: Apple Silicon is required for `swift test`.
# Everything is built under build/engine first and moved into the package only
# after every step succeeds, so a failed run never leaves a half-updated package.
set -euo pipefail

export PATH="$HOME/.cargo/bin:$PATH"
export IPHONEOS_DEPLOYMENT_TARGET=26.0
export MACOSX_DEPLOYMENT_TARGET=26.0

ROOT="$(cd "$(dirname "$0")/.." && pwd)"
ENGINE="$ROOT/engine"
PKG="$ROOT/apps/ios/Packages/MLEngine"
BUILD="$ROOT/build/engine"
TARGETS=(aarch64-apple-ios aarch64-apple-ios-sim aarch64-apple-darwin)

cd "$ENGINE"
# Run inside engine/ so rust-toolchain.toml selects the toolchain.
for target in "${TARGETS[@]}"; do
  if ! rustup target list --installed | grep -qx "$target"; then
    echo "missing Rust target $target: run 'rustup target add $target'" >&2
    exit 1
  fi
done

for target in "${TARGETS[@]}"; do
  cargo build -p mlengine-ffi --release --target "$target"
done

rm -rf "$BUILD"
mkdir -p "$BUILD/bindings" "$BUILD/headers/mlengine_ffiFFI"
cargo run -q -p mlengine-ffi --features cli --bin uniffi-bindgen -- generate \
  --library "$ENGINE/target/aarch64-apple-darwin/release/libmlengine_ffi.dylib" \
  --language swift --out-dir "$BUILD/bindings"

# Per-module subdirectory so a second static XCFramework cannot collide on include/module.modulemap.
cp "$BUILD/bindings/mlengine_ffiFFI.h" "$BUILD/headers/mlengine_ffiFFI/"
cp "$BUILD/bindings/mlengine_ffiFFI.modulemap" "$BUILD/headers/mlengine_ffiFFI/module.modulemap"

args=()
for target in "${TARGETS[@]}"; do
  args+=(-library "$ENGINE/target/$target/release/libmlengine_ffi.a" -headers "$BUILD/headers")
done
xcodebuild -create-xcframework "${args[@]}" -output "$BUILD/MLEngineFFI.xcframework"

# All steps succeeded: swap the new artifacts into the package.
rm -rf "$PKG/MLEngineFFI.xcframework"
mv "$BUILD/MLEngineFFI.xcframework" "$PKG/MLEngineFFI.xcframework"
cp "$BUILD/bindings/mlengine_ffi.swift" "$PKG/Sources/MLEngine/mlengine_ffi.swift"
echo "MLEngineFFI.xcframework and Swift bindings updated."
