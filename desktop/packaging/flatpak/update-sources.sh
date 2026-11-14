#!/usr/bin/env bash
# Regenerates the offline sources of the Flatpak build from the lock files:
#   generated-sources.json  ← desktop/package-lock.json (flatpak-node-generator)
#   cargo-sources.json      ← core/Cargo.lock          (flatpak-cargo-generator)
# Run after every change to either lock file. Needs git and uv (https://docs.astral.sh/uv/).
# The tools come from a pinned flatpak-builder-tools checkout in $TOOLS (default: ~/.cache).
set -euo pipefail
HERE="$(cd "$(dirname "$0")" && pwd)"
ROOT="$(cd "$HERE/../../.." && pwd)"
TOOLS="${TOOLS:-$HOME/.cache/wolkenfunk/flatpak-builder-tools}"
TOOLS_COMMIT=9d86d336a39b9af38b86f114b6ce5820fe260f91

if [ ! -d "$TOOLS/.git" ]; then
  git clone -q https://github.com/flatpak/flatpak-builder-tools "$TOOLS"
fi
git -C "$TOOLS" fetch -q origin "$TOOLS_COMMIT" 2>/dev/null || git -C "$TOOLS" fetch -q origin
git -C "$TOOLS" checkout -q "$TOOLS_COMMIT"

echo "→ generated-sources.json (npm)"
uvx --from "$TOOLS/node" flatpak-node-generator npm \
  "$ROOT/desktop/package-lock.json" -o "$HERE/generated-sources.json"

echo "→ cargo-sources.json (crates)"
uv run --script "$TOOLS/cargo/flatpak-cargo-generator.py" \
  "$ROOT/core/Cargo.lock" -o "$HERE/cargo-sources.json"
