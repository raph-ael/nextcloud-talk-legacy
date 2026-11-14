#!/usr/bin/env bash
# Linux desktop screenshots against the demo server (scripts/screenshots/README.md):
#   scripts/screenshots/desktop.sh
# writes website/public/screenshots/desktop-<chats|ungelesen|sprachnachricht>-<hell|dunkel>.webp
# and desktop-chat.png (AppStream/Flathub: the "ungelesen" scene, light).
#
# The unpackaged app (electron-vite build) in a hidden X server (xvfb-run), window content
# 1280×800 at factor 1, steered over the DevTools protocol. Each theme is a fresh run: demo server
# restarted (data reset), empty profile (so an empty cache), logged in as mira. Never touches the
# normal profile in ~/.config/Wolkenfunk.
#   SKIP_BUILD=1   use the existing build (desktop/out, desktop/native)
set -euo pipefail
HERE="$(cd "$(dirname "$0")" && pwd)"
ROOT="$(cd "$HERE/../.." && pwd)"
OUT="$ROOT/website/public/screenshots"
W=1280 H=800
export CDP_PAGE_PORT="${CDP_PAGE_PORT:-9362}" CDP_MAIN_PORT="${CDP_MAIN_PORT:-9363}"
RAW="$(mktemp -d)"
APP=

drive() { node "$HERE/mac-drive.mjs" "$@"; }
ddrive() { node "$HERE/desktop-drive.mjs" "$@"; }
stop_app() {
  [ -n "$APP" ] && kill "$APP" 2>/dev/null && wait "$APP" 2>/dev/null || true
  pkill -f -- "--user-data-dir=$RAW/profile" 2>/dev/null || true
  APP=
}
cleanup() {
  stop_app
  "$HERE/demo-server.sh" stop >/dev/null 2>&1 || true
  rm -rf "$RAW"
}
trap cleanup EXIT

cd "$ROOT/desktop"
if [ "${SKIP_BUILD:-0}" != 1 ]; then
  [ -d node_modules ] || npm ci
  [ -d node_modules/electron/dist ] || node node_modules/electron/install.js
  [ -f native/talk_node.node ] || scripts/build-core.sh
  npx electron-vite build >/dev/null
fi

webp() {   # webp <png> <out.webp>
  python3 -c '
import sys
from PIL import Image
Image.open(sys.argv[1]).convert("RGB").save(sys.argv[2], "WEBP", quality=85, method=6)' "$1" "$2"
  echo "→ ${2#"$ROOT/"}"
}

mkdir -p "$OUT"
for theme in light dark; do
  name=$([ "$theme" = light ] && echo hell || echo dunkel)
  "$HERE/demo-server.sh" start 127.0.0.1:8099 >/dev/null
  rm -rf "$RAW/profile"
  env -u WAYLAND_DISPLAY XDG_SESSION_TYPE=x11 \
    TALK_DEV_SERVER=http://127.0.0.1:8099 TALK_DEV_USER=mira TALK_DEV_PASS=demo \
    xvfb-run -a -s "-screen 0 1440x900x24" ./node_modules/.bin/electron . --no-sandbox --ozone-platform=x11 \
    --force-device-scale-factor=1 --user-data-dir="$RAW/profile" \
    --remote-debugging-port="$CDP_PAGE_PORT" --inspect="$CDP_MAIN_PORT" >"$RAW/app.log" 2>&1 &
  APP=$!
  for _ in $(seq 60); do curl -sf "127.0.0.1:$CDP_PAGE_PORT/json/list" | grep -q '"page"' && break; sleep 0.5; done
  ddrive size $W $H
  drive theme "$theme"
  # Chat list first: opening "Team Website" reads only that chat, the others keep their badges.
  for scene in chats ungelesen sprachnachricht; do
    case "$scene" in
      chats) chat="Team Website" ;;
      ungelesen) chat="Gartenprojekt" ;;
      sprachnachricht) chat="Familie" ;;
    esac
    drive open "$chat"
    ddrive pointer 800 25   # over the chat title: the X pointer sits mid-window otherwise
    drive ready
    ddrive shot "$RAW/$scene.png"
    webp "$RAW/$scene.png" "$OUT/desktop-$scene-$name.webp"
    if [ "$scene-$theme" = ungelesen-light ]; then
      cp "$RAW/$scene.png" "$OUT/desktop-chat.png"
      echo "→ ${OUT#"$ROOT/"}/desktop-chat.png"
    fi
  done
  stop_app
  "$HERE/demo-server.sh" stop >/dev/null
done
