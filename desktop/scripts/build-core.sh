#!/usr/bin/env bash
# Builds the napi-rs binding of the Rust core and copies it to desktop/native/.
#   PROFILE=dev|release (default dev)
#   UNIVERSAL=1 on macOS: x86_64 + arm64 in one file (for `electron-builder --universal`).
set -euo pipefail
command -v cargo >/dev/null || source "$HOME/.cargo/env"
cd "$(dirname "$0")/../../core"
PROFILE="${PROFILE:-dev}"
DIR=$([ "$PROFILE" = dev ] && echo debug || echo "$PROFILE")
mkdir -p ../desktop/native
OUT=../desktop/native/talk_node.node

if [ "$(uname -s)" = Darwin ] && [ "${UNIVERSAL:-0}" = 1 ]; then
  export MACOSX_DEPLOYMENT_TARGET=12.0
  libs=()
  for t in x86_64-apple-darwin aarch64-apple-darwin; do
    rustup target add "$t" >/dev/null
    cargo build -p talk-node --profile "$PROFILE" --target "$t"
    libs+=("target/$t/$DIR/libtalk_node.dylib")
  done
  lipo -create "${libs[@]}" -output "$OUT"
  echo "→ desktop/native/talk_node.node ($PROFILE, $(lipo -archs "$OUT"))"
  exit 0
fi

cargo build -p talk-node --profile "$PROFILE"
case "$(uname -s)" in
  Linux) LIB=libtalk_node.so ;;
  Darwin) LIB=libtalk_node.dylib ;;
  *) LIB=talk_node.dll ;;
esac
cp "target/$DIR/$LIB" "$OUT"
echo "→ desktop/native/talk_node.node ($PROFILE)"
