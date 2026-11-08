#!/usr/bin/env bash
# Builds the napi-rs binding of the Rust core and copies it to desktop/native/.
set -euo pipefail
command -v cargo >/dev/null || source "$HOME/.cargo/env"
cd "$(dirname "$0")/../../core"
PROFILE="${PROFILE:-dev}"
cargo build -p talk-node --profile "$PROFILE"
DIR=$([ "$PROFILE" = dev ] && echo debug || echo "$PROFILE")
case "$(uname -s)" in
  Linux) LIB=libtalk_node.so ;;
  Darwin) LIB=libtalk_node.dylib ;;
  *) LIB=talk_node.dll ;;
esac
mkdir -p ../desktop/native
cp "target/$DIR/$LIB" ../desktop/native/talk_node.node
echo "→ desktop/native/talk_node.node ($PROFILE)"
