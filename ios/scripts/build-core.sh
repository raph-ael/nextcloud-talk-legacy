#!/usr/bin/env bash
# Builds the Rust core for iOS and produces:
#   ios/Generated/TalkCoreFFI.xcframework  (static library + C header/modulemap)
#   ios/Generated/talk_ffi.swift           (UniFFI Swift bindings)
# Runs on macOS with Xcode (locally or on Codemagic).
set -euo pipefail

ROOT="$(cd "$(dirname "$0")/../.." && pwd)"
CORE="$ROOT/core"
OUT="$ROOT/ios/Generated"
PROFILE="${PROFILE:-release}"
TARGETS=(aarch64-apple-ios aarch64-apple-ios-sim)

export IPHONEOS_DEPLOYMENT_TARGET=17.0

cd "$CORE"
rustup target add "${TARGETS[@]}"
for t in "${TARGETS[@]}"; do
  cargo build -p talk-ffi --profile "$PROFILE" --target "$t"
done

rm -rf "$OUT"
mkdir -p "$OUT/headers" "$OUT/bindings"

# Library mode reads the UniFFI metadata from the compiled library.
cargo run -q -p talk-ffi --bin uniffi-bindgen -- generate \
  --library "target/aarch64-apple-ios/$PROFILE/libtalk_ffi.dylib" \
  --language swift --no-format --out-dir "$OUT/bindings"

cp "$OUT/bindings/talk_ffi.swift" "$OUT/"
cp "$OUT/bindings/talk_ffiFFI.h" "$OUT/headers/"
cp "$OUT/bindings/talk_ffiFFI.modulemap" "$OUT/headers/module.modulemap"

xcodebuild -create-xcframework \
  -library "target/aarch64-apple-ios/$PROFILE/libtalk_ffi.a" -headers "$OUT/headers" \
  -library "target/aarch64-apple-ios-sim/$PROFILE/libtalk_ffi.a" -headers "$OUT/headers" \
  -output "$OUT/TalkCoreFFI.xcframework"

echo "Built $OUT/TalkCoreFFI.xcframework"
