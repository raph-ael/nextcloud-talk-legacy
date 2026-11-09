#!/usr/bin/env bash
# Renders design/icon/app-icon.svg into resources/icon.icns (run on macOS).
set -euo pipefail
cd "$(dirname "$0")/.."
SVG=../design/icon/app-icon.svg
TMP="$(mktemp -d)"
SET="$TMP/icon.iconset"
mkdir -p "$SET"
# Quick Look renders SVG without extra tools.
qlmanage -t -s 1024 -o "$TMP" "$SVG" >/dev/null
BASE="$TMP/$(basename "$SVG").png"
for s in 16 32 128 256 512; do
  sips -z $s $s "$BASE" --out "$SET/icon_${s}x${s}.png" >/dev/null
  sips -z $((s * 2)) $((s * 2)) "$BASE" --out "$SET/icon_${s}x${s}@2x.png" >/dev/null
done
mkdir -p resources
iconutil -c icns "$SET" -o resources/icon.icns
rm -rf "$TMP"
echo "→ resources/icon.icns"
