#!/usr/bin/env bash
# Renders design/icon/app-icon.svg into build/icons/<N>x<N>.png for the Linux packages
# (electron-builder installs them under /usr/share/icons/hicolor). Needs rsvg-convert (librsvg2-bin).
set -euo pipefail
cd "$(dirname "$0")/.."
SVG=../design/icon/app-icon.svg
OUT=build/icons
mkdir -p "$OUT"
for s in 16 24 32 48 64 128 256 512; do
  rsvg-convert -w $s -h $s "$SVG" -o "$OUT/${s}x${s}.png"
done
echo "→ $OUT"
