#!/usr/bin/env bash
# Builds the Rust engine for iOS device, iOS simulator and macOS, generates the
# Swift bindings, and packages apps/ios/Packages/MLEngine/MLEngineFFI.xcframework.
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
for target in "${TARGETS[@]}"; do
  cargo build -p mlengine-ffi --release --target "$target"
done

rm -rf "$BUILD"
mkdir -p "$BUILD/bindings" "$BUILD/headers"
cargo run -q -p mlengine-ffi --features cli --bin uniffi-bindgen -- generate \
  --library "$ENGINE/target/aarch64-apple-darwin/release/libmlengine_ffi.dylib" \
  --language swift --out-dir "$BUILD/bindings"

cp "$BUILD/bindings/mlengine_ffiFFI.h" "$BUILD/headers/"
cp "$BUILD/bindings/mlengine_ffiFFI.modulemap" "$BUILD/headers/module.modulemap"

args=()
for target in "${TARGETS[@]}"; do
  args+=(-library "$ENGINE/target/$target/release/libmlengine_ffi.a" -headers "$BUILD/headers")
done
rm -rf "$PKG/MLEngineFFI.xcframework"
xcodebuild -create-xcframework "${args[@]}" -output "$PKG/MLEngineFFI.xcframework"

cp "$BUILD/bindings/mlengine_ffi.swift" "$PKG/Sources/MLEngine/mlengine_ffi.swift"
echo "MLEngineFFI.xcframework and Swift bindings updated."
