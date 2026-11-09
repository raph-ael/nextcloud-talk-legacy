#!/usr/bin/env bash
# Builds the Rust core for iOS and produces:
#   ios/Generated/TalkCoreFFI.xcframework  (static library + C header/modulemap)
#   ios/Generated/talk_ffi.swift           (UniFFI Swift bindings)
# Runs on macOS with Xcode (locally or on Codemagic).
#
#   PROFILE=dev|release (default release)
#   SLICES="device sim"  (default both; "sim" = arm64 + x86_64 simulator, lipo'd together)
set -euo pipefail

ROOT="$(cd "$(dirname "$0")/../.." && pwd)"
CORE="$ROOT/core"
OUT="$ROOT/ios/Generated"
PROFILE="${PROFILE:-release}"
SLICES="${SLICES:-device sim}"
# cargo puts the dev profile into target/<triple>/debug.
DIR="$PROFILE"; [[ "$PROFILE" == dev ]] && DIR=debug

export IPHONEOS_DEPLOYMENT_TARGET=17.0

TARGETS=()
[[ " $SLICES " == *" device "* ]] && TARGETS+=(aarch64-apple-ios)
[[ " $SLICES " == *" sim "* ]] && TARGETS+=(aarch64-apple-ios-sim x86_64-apple-ios)

cd "$CORE"
rustup target add "${TARGETS[@]}"
for t in "${TARGETS[@]}"; do
  cargo build -p talk-ffi --profile "$PROFILE" --target "$t"
done

rm -rf "$OUT"
mkdir -p "$OUT/headers" "$OUT/bindings"

# Library mode reads the UniFFI metadata from the compiled library (any slice will do).
cargo run -q -p talk-ffi --bin uniffi-bindgen -- generate \
  --library "target/${TARGETS[0]}/$DIR/libtalk_ffi.dylib" \
  --language swift --no-format --out-dir "$OUT/bindings"

cp "$OUT/bindings/talk_ffi.swift" "$OUT/"
cp "$OUT/bindings/talk_ffiFFI.h" "$OUT/headers/"
cp "$OUT/bindings/talk_ffiFFI.modulemap" "$OUT/headers/module.modulemap"

ARGS=()
if [[ " $SLICES " == *" device "* ]]; then
  ARGS+=(-library "target/aarch64-apple-ios/$DIR/libtalk_ffi.a" -headers "$OUT/headers")
fi
if [[ " $SLICES " == *" sim "* ]]; then
  # One simulator slice for Apple Silicon and Intel Macs.
  mkdir -p "$OUT/sim"
  lipo -create \
    "target/aarch64-apple-ios-sim/$DIR/libtalk_ffi.a" \
    "target/x86_64-apple-ios/$DIR/libtalk_ffi.a" \
    -output "$OUT/sim/libtalk_ffi.a"
  ARGS+=(-library "$OUT/sim/libtalk_ffi.a" -headers "$OUT/headers")
fi

xcodebuild -create-xcframework "${ARGS[@]}" -output "$OUT/TalkCoreFFI.xcframework"
rm -rf "$OUT/sim"

echo "Built $OUT/TalkCoreFFI.xcframework ($SLICES, $PROFILE)"
