#!/usr/bin/env bash
# macOS screenshots of the desktop app against the demo server, with the real macOS window frame:
#   scripts/screenshots/mac.sh [scene ...]     default: chats   (scenes: chats ungelesen sprachnachricht)
# Writes website/public/screenshots/mac-<scene>-<hell|dunkel>.webp: exactly the window (1280×800 pt,
# no shadow, transparent rounded corners), light and dark.
#
# Runs on Linux and drives the Mac build host over SSH: syncs the tree (scripts/mac-sync.sh),
# starts the demo server and the unpackaged app there (TALK_DEV_*), steers it over the DevTools
# protocol (mac-drive.mjs) and takes screenshots of the display, once over a black and once over
# a white backdrop (mac-matte.py cuts the window out). The Mac must be unlocked.
#
# Screenshots come from a small helper on the Mac that has the screen recording permission and
# answers on a Unix socket (MACCTL_SOCK, default ~/.macctl.sock; GET /screenshot?scale=2,
# /move?x=&y=, /info). `screencapture` over SSH has no screen recording permission.
# MAC_HOST comes from the environment or from the .env file (WOLKENFUNK_ENV, default .env in the
# repository root); MAC_DIR is the checkout on the Mac (default repos/wolkenfunk-desktop).
set -euo pipefail
HERE="$(cd "$(dirname "$0")" && pwd)"
ROOT="$(cd "$HERE/../.." && pwd)"
SCENES=("${@:-chats}")
OUT="$ROOT/website/public/screenshots"
X=160 Y=38 W=1280 H=800   # window in points on the Mac display, clear of menu bar and Dock

ENV_FILE="${WOLKENFUNK_ENV:-$ROOT/.env}"
if [ -z "${MAC_HOST:-}" ] && [ -f "$ENV_FILE" ]; then
  MAC_HOST="$(grep -E '^MAC_HOST=' "$ENV_FILE" | cut -d= -f2- || true)"
fi
: "${MAC_HOST:?set MAC_HOST=user@host (environment or .env)}"
export MAC_HOST
export MAC_DIR="${MAC_DIR:-repos/wolkenfunk-desktop}"
SOCK="${MACCTL_SOCK:-\$HOME/.macctl.sock}"
export CDP_PAGE_PORT="${CDP_PAGE_PORT:-9352}" CDP_MAIN_PORT="${CDP_MAIN_PORT:-9353}"
PROFILE=/tmp/wolkenfunk-screenshots
RAW="$(mktemp -d)"

macctl() { ssh "$MAC_HOST" "curl -sS --fail --unix-socket $SOCK 'http://m$1'"; }
drive() { node "$HERE/mac-drive.mjs" "$@"; }
stop_app() { ssh "$MAC_HOST" "pkill -f 'user-data-dir=$PROFILE' || true"; }
TUNNEL=
cleanup() {
  [ -n "$TUNNEL" ] && kill "$TUNNEL" 2>/dev/null || true
  stop_app
  ssh "$MAC_HOST" "cd $MAC_DIR && scripts/screenshots/demo-server.sh stop" >/dev/null 2>&1 || true
  rm -rf "$RAW"
}
trap cleanup EXIT

macctl /info | grep -q '"screenRecording" : true' || { echo "screenshot helper on the Mac not ready" >&2; exit 1; }
if ssh "$MAC_HOST" "curl -sS --unix-socket $SOCK http://m/windows" | grep -q '"owner" : "loginwindow"'; then
  echo "the Mac is locked" >&2; exit 1
fi

# Demo server, app build (the native core only if it is missing), Electron binary.
"$ROOT/scripts/mac-sync.sh" "scripts/screenshots/demo-server.sh start 127.0.0.1:8099 && cd desktop \
  && { [ -d node_modules ] || npm ci; } && { [ -d node_modules/electron/dist ] || node node_modules/electron/install.js; } \
  && { [ -f native/talk_node.node ] || scripts/build-core.sh; } && npx electron-vite build >/dev/null"

# Unpackaged app with an empty profile (and so an empty cache), through LaunchServices like a
# normal app, so its window gets focus.
stop_app
ssh "$MAC_HOST" "rm -rf $PROFILE && D=\$HOME/$MAC_DIR/desktop && open -n -a \$D/node_modules/electron/dist/Electron.app \
  --env TALK_DEV_SERVER=http://127.0.0.1:8099 --env TALK_DEV_USER=mira --env TALK_DEV_PASS=demo \
  --args \$D --user-data-dir=$PROFILE --remote-debugging-port=$CDP_PAGE_PORT --inspect=$CDP_MAIN_PORT"
ssh -N -o ExitOnForwardFailure=yes -L "$CDP_PAGE_PORT:127.0.0.1:$CDP_PAGE_PORT" -L "$CDP_MAIN_PORT:127.0.0.1:$CDP_MAIN_PORT" "$MAC_HOST" &
TUNNEL=$!
for _ in $(seq 60); do curl -sf "127.0.0.1:$CDP_PAGE_PORT/json/list" | grep -q '"page"' && break; sleep 0.5; done

drive prepare $X $Y $W $H
macctl "/move?x=1590&y=12" >/dev/null   # pointer out of the window
mkdir -p "$OUT"
for scene in "${SCENES[@]}"; do
  case "$scene" in
    chats) chat="Team Website" ;;
    ungelesen) chat="Gartenprojekt" ;;
    sprachnachricht) chat="Familie" ;;
    *) echo "unknown scene $scene" >&2; exit 2 ;;
  esac
  for theme in light dark; do
    name=$([ "$theme" = light ] && echo hell || echo dunkel)
    drive theme "$theme"
    drive open "$chat"
    drive ready
    read -r BX BY BW BH < <(drive bounds | node -pe 'const b = JSON.parse(require("fs").readFileSync(0)); [b.x, b.y, b.width, b.height].join(" ")')
    [ "$BW $BH" = "$W $H" ] || { echo "window is ${BW}x$BH, not ${W}x$H" >&2; exit 1; }
    drive backdrop '#000000'; sleep 0.7; macctl "/screenshot?scale=2" > "$RAW/black.png"
    drive backdrop '#ffffff'; sleep 0.7; macctl "/screenshot?scale=2" > "$RAW/white.png"
    python3 "$HERE/mac-matte.py" "$RAW/black.png" "$RAW/white.png" "$BX" "$BY" "$BW" "$BH" "$OUT/mac-$scene-$name.webp"
  done
done
drive finish
