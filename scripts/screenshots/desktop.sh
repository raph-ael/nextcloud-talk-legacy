#!/usr/bin/env bash
# Linux desktop screenshots against the demo server (scripts/screenshots/README.md):
#   scripts/screenshots/desktop.sh [scene …]     without a scene: all of them
# writes website/public/screenshots/desktop-<scene>-<hell|dunkel>.webp for the scenes chats,
# ungelesen, sprachnachricht, kompakt and konten, and desktop-chat.png (AppStream/Flathub: the
# "ungelesen" scene, light).
#
# The unpackaged app (electron-vite build) in a hidden X server (xvfb-run), window content
# 1280×800 at factor 1, steered over the DevTools protocol. Each theme is a fresh run: demo server
# restarted (data reset), empty profile (so an empty cache), logged in as mira. Never touches the
# normal profile in ~/.config/Wolkenfunk, the keyring or the desktop's notifications (own D-Bus).
#
# "konten" shows two accounts on two servers. Their names are invented (.example) and never
# resolve: the app reaches both demo servers through demo-proxy.mjs (http_proxy).
#   SKIP_BUILD=1   use the existing build (desktop/out, desktop/native)
#   DEMO_PORT, DEMO_PORT_2, PROXY_PORT   default 8099, 8098, 8097
set -euo pipefail
HERE="$(cd "$(dirname "$0")" && pwd)"
ROOT="$(cd "$HERE/../.." && pwd)"
OUT="$ROOT/website/public/screenshots"
W=1280 H=800
DEMO_PORT="${DEMO_PORT:-8099}" DEMO_PORT_2="${DEMO_PORT_2:-8098}" PROXY_PORT="${PROXY_PORT:-8097}"
HOST_1=cloud.familie-lange.example HOST_2=cloud.werkhof.example
export CDP_PAGE_PORT="${CDP_PAGE_PORT:-9362}" CDP_MAIN_PORT="${CDP_MAIN_PORT:-9363}"
SCENES=("$@")
[ ${#SCENES[@]} -gt 0 ] || SCENES=(chats ungelesen sprachnachricht kompakt konten)
RAW="$(mktemp -d)"
APP= PROXY=

wanted() {   # wanted <scene> …: one of them was asked for
  local s w
  for w in "$@"; do for s in "${SCENES[@]}"; do [ "$s" = "$w" ] && return 0; done; done
  return 1
}
drive() { node "$HERE/mac-drive.mjs" "$@"; }
ddrive() { node "$HERE/desktop-drive.mjs" "$@"; }
stop_app() {   # the app's whole process group: Electron, the X server, the D-Bus daemon
  if [ -n "$APP" ]; then
    kill -- "-$APP" 2>/dev/null || true
    wait "$APP" 2>/dev/null || true
    for _ in $(seq 20); do pgrep -g "$APP" >/dev/null || break; sleep 0.25; done
    kill -9 -- "-$APP" 2>/dev/null || true
  fi
  APP=
}
stop_servers() {   # only the instances of this script
  [ -n "$PROXY" ] && kill "$PROXY" 2>/dev/null && wait "$PROXY" 2>/dev/null || true
  PROXY=
  "$HERE/demo-server.sh" stop "$DEMO_PORT" >/dev/null 2>&1 || true
  "$HERE/demo-server.sh" stop "$DEMO_PORT_2" >/dev/null 2>&1 || true
}
cleanup() {
  stop_app
  stop_servers
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

start_app() {   # start_app <theme> <VAR=value …>: empty profile, signed in with the given accounts
  local theme="$1"
  shift
  rm -rf "$RAW/profile"
  setsid env -u WAYLAND_DISPLAY -u GNOME_KEYRING_CONTROL XDG_SESSION_TYPE=x11 "$@" \
    dbus-run-session -- xvfb-run -a -s "-screen 0 1440x900x24" ./node_modules/.bin/electron . \
    --no-sandbox --ozone-platform=x11 --password-store=basic --mute-audio \
    --force-device-scale-factor=1 --user-data-dir="$RAW/profile" \
    --remote-debugging-port="$CDP_PAGE_PORT" --inspect="$CDP_MAIN_PORT" >"$RAW/app.log" 2>&1 &
  APP=$!
  for _ in $(seq 60); do curl -sf "127.0.0.1:$CDP_PAGE_PORT/json/list" | grep -q '"page"' && break; sleep 0.5; done
  ddrive size $W $H
  drive theme "$theme"
}

shoot() {   # shoot <scene> <hell|dunkel>
  ddrive pointer 800 25   # over the chat title: the X pointer sits mid-window otherwise
  drive ready
  ddrive shot "$RAW/$1.png"
  webp "$RAW/$1.png" "$OUT/desktop-$1-$2.webp"
}

mkdir -p "$OUT"
for theme in light dark; do
  name=$([ "$theme" = light ] && echo hell || echo dunkel)

  if wanted chats ungelesen sprachnachricht; then
    "$HERE/demo-server.sh" start "127.0.0.1:$DEMO_PORT" >/dev/null
    start_app "$theme" TALK_DEV_SERVER="http://127.0.0.1:$DEMO_PORT" TALK_DEV_USER=mira TALK_DEV_PASS=demo
    # Chat list first: opening "Team Website" reads only that chat, the others keep their badges.
    # Every chat is opened in this order, whichever scenes are asked for: the list looks the same.
    for scene in chats ungelesen sprachnachricht; do
      case "$scene" in
        chats) chat="Team Website" ;;
        ungelesen) chat="Gartenprojekt" ;;
        sprachnachricht) chat="Familie" ;;
      esac
      drive open "$chat"
      wanted "$scene" || continue
      shoot "$scene" "$name"
      if [ "$scene-$theme" = ungelesen-light ]; then
        cp "$RAW/$scene.png" "$OUT/desktop-chat.png"
        echo "→ ${OUT#"$ROOT/"}/desktop-chat.png"
      fi
    done
    stop_app
    stop_servers
  fi

  # The compact view (docs/compact-view.md) with a group open: one head per run of messages.
  if wanted kompakt; then
    "$HERE/demo-server.sh" start "127.0.0.1:$DEMO_PORT" >/dev/null
    start_app "$theme" TALK_DEV_SERVER="http://127.0.0.1:$DEMO_PORT" TALK_DEV_USER=mira TALK_DEV_PASS=demo
    ddrive layout compact
    drive open "Team Website"
    shoot kompakt "$name"
    stop_app
    stop_servers
  fi

  # Two accounts (docs/accounts.md): the same person on two servers, the menu open. The second
  # account has chats with news, so its row and the own avatar carry a number.
  if wanted konten; then
    "$HERE/demo-server.sh" start "127.0.0.1:$DEMO_PORT" >/dev/null
    "$HERE/demo-server.sh" start "127.0.0.1:$DEMO_PORT_2" >/dev/null
    node "$HERE/demo-proxy.mjs" "$PROXY_PORT" "$HOST_1=$DEMO_PORT" "$HOST_2=$DEMO_PORT_2" &
    PROXY=$!
    start_app "$theme" http_proxy="http://127.0.0.1:$PROXY_PORT" \
      TALK_DEV_SERVER="http://$HOST_1" TALK_DEV_USER=mira TALK_DEV_PASS=demo \
      TALK_DEV_SERVER_2="http://$HOST_2" TALK_DEV_USER_2=mira TALK_DEV_PASS_2=demo
    drive open "Team Website"
    ddrive accounts
    shoot konten "$name"
    stop_app
    stop_servers
  fi
done
