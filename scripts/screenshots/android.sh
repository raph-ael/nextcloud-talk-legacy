#!/usr/bin/env bash
# Android screenshots against the demo server (scripts/screenshots/README.md):
#   scripts/screenshots/android.sh
# writes website/public/screenshots/android-<chats|ungelesen|sprachnachricht>-<hell|dunkel>.webp.
#
# Needs a running emulator (Pixel, 1080×2400), e.g.
#   emulator -avd <name> -no-window -no-snapshot -gpu swiftshader_indirect -port 5560
#   ANDROID_SERIAL   device (default: the only emulator adb lists)
#   APK              app to install (default: debug build, built unless SKIP_BUILD=1)
# Each theme is a fresh run: demo server restarted (data reset), app data cleared, logged in again.
set -euo pipefail
cd "$(dirname "$0")/../.."

PKG=org.wolkenfunk.app
SERVER=http://10.0.2.2:8099
OUT=website/public/screenshots
RAW=$(mktemp -d)
trap 'cleanup' EXIT

export ANDROID_SERIAL="${ANDROID_SERIAL:-$(adb devices | awk '/^emulator-/{print $1; exit}')}"
[ -n "$ANDROID_SERIAL" ] || { echo "no emulator found (adb devices)" >&2; exit 1; }
[ "$(adb shell wm size | awk '{print $3}')" = 1080x2400 ] || { echo "emulator must be 1080x2400" >&2; exit 1; }

APK="${APK:-android/app/build/outputs/apk/debug/app-debug.apk}"
if [ "${SKIP_BUILD:-0}" != 1 ] && [ "$APK" = android/app/build/outputs/apk/debug/app-debug.apk ]; then
  (cd android && ./gradlew -q :app:assembleDebug)
fi

# --- helpers -------------------------------------------------------------------------------------
# Visible nodes as "text<TAB>content-desc<TAB>x<TAB>y" (centre).
nodes() {
  adb shell uiautomator dump /sdcard/ui.xml >/dev/null 2>&1 || true
  adb exec-out cat /sdcard/ui.xml | python3 -c '
import sys, re, html
x = sys.stdin.read()
for n in re.findall(r"<node [^>]*>", x):
    g = lambda k: html.unescape((re.search(k + r"=\"([^\"]*)\"", n) or [None, ""])[1])
    a = list(map(int, re.findall(r"\d+", g("bounds"))))
    if len(a) == 4 and (g("text") or g("content-desc")):
        print(g("text"), g("content-desc"), (a[0] + a[2]) // 2, (a[1] + a[3]) // 2, sep="\t")'
}
# Waits until a node with exactly this text (or content description) is visible; prints "x y".
wait_for() {
  local label=$1 timeout=${2:-30} hit
  for _ in $(seq "$timeout"); do
    hit=$(nodes | awk -F'\t' -v l="$label" '!h && ($1 == l || $2 == l) {print $3, $4; h = 1}' || true)
    [ -n "$hit" ] && { echo "$hit"; return 0; }
    sleep 1
  done
  echo "timeout waiting for \"$label\"" >&2
  return 1
}
tap() { local xy; xy=$(wait_for "$@") && adb shell input tap $xy; }
gone() {   # waits until no node contains this text (spinners, "Lädt …")
  for _ in $(seq 30); do nodes | grep -F "$1" >/dev/null || return 0; sleep 1; done
  echo "still visible: $1" >&2; return 1
}
shot() {   # shot <name>: raw PNG → WebP (quality 85)
  adb exec-out screencap -p >"$RAW/$1.png"
  python3 -c '
import sys
from PIL import Image
im = Image.open(sys.argv[1]).convert("RGB")
assert im.size == (1080, 2400), im.size
im.save(sys.argv[2], "WEBP", quality=85, method=6)' "$RAW/$1.png" "$OUT/android-$1.webp"
  echo "→ $OUT/android-$1.webp"
}
demo_mode() {
  adb shell settings put global sysui_demo_allowed 1
  local b="am broadcast -a com.android.systemui.demo -e command"
  adb shell "$b enter" >/dev/null
  adb shell "$b clock -e hhmm 0941" >/dev/null
  adb shell "$b network -e wifi show -e level 4 -e fully true" >/dev/null
  adb shell "$b network -e mobile hide" >/dev/null
  adb shell "$b battery -e level 100 -e plugged false" >/dev/null
  adb shell "$b notifications -e visible false" >/dev/null
}
cleanup() {
  adb shell am broadcast -a com.android.systemui.demo -e command exit >/dev/null 2>&1 || true
  adb shell cmd uimode night no >/dev/null 2>&1 || true
  scripts/screenshots/demo-server.sh stop >/dev/null 2>&1 || true
  rm -rf "$RAW"
}

login() {
  adb shell am force-stop "$PKG"
  adb shell pm clear "$PKG" >/dev/null
  for p in POST_NOTIFICATIONS CAMERA RECORD_AUDIO READ_PHONE_STATE BLUETOOTH_CONNECT READ_MEDIA_IMAGES; do
    adb shell pm grant "$PKG" "android.permission.$p" 2>/dev/null || true
  done
  adb shell am start -n "$PKG/.MainActivity" >/dev/null
  tap "cloud.example.org"
  adb shell input text "$SERVER"
  adb shell input keyevent KEYCODE_BACK
  # Login Flow v2: the demo server grants as soon as the flow page is opened. Open it from here
  # as well, so a browser first-run dialog cannot block the run. Older flows may still be listed
  # in the activity stack, so only a new flow URL counts.
  flows() { adb shell dumpsys activity activities | grep -o "$SERVER/index.php/login/v2/flow/[0-9a-f]*" | sort -u || true; }
  local before url=""
  before=$(flows)
  tap "Anmelden"
  for _ in $(seq 20); do
    url=$(comm -13 <(echo "$before") <(flows) | head -1)
    [ -n "$url" ] && break
    sleep 0.5
  done
  [ -n "$url" ] || { echo "login flow did not open" >&2; exit 1; }
  curl -s "${url/10.0.2.2/127.0.0.1}" >/dev/null || true   # 404 once the app has collected it
  sleep 2
  adb shell am start -n "$PKG/.MainActivity" >/dev/null
  wait_for "Chats" 60 >/dev/null
}

scenes() {   # scenes <hell|dunkel>
  local theme=$1
  sleep 4   # avatars and previews
  shot "chats-$theme"

  tap "Gartenprojekt"
  wait_for "5 ungelesene Nachrichten" 30 >/dev/null
  sleep 4   # image
  shot "ungelesen-$theme"
  adb shell input keyevent KEYCODE_BACK
  wait_for "Chats" >/dev/null

  tap "Familie"
  sleep 4
  shot "sprachnachricht-$theme"
  adb shell input keyevent KEYCODE_BACK
}

mkdir -p "$OUT"
adb install -r "$APK" >/dev/null
for theme in hell dunkel; do
  if [ "$theme" = dunkel ]; then adb shell cmd uimode night yes >/dev/null; else adb shell cmd uimode night no >/dev/null; fi
  sleep 2
  demo_mode   # again after the theme switch, the status bar may have been rebuilt
  scripts/screenshots/demo-server.sh start
  login
  scenes "$theme"
done
echo "done: $(ls "$OUT"/android-*.webp | wc -l) images in $OUT"
