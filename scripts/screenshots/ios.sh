#!/usr/bin/env bash
# iOS screenshots for the website (scripts/screenshots/README.md), run from the Linux machine:
#   scripts/screenshots/ios.sh
# Mirrors the tree to the Mac build host (scripts/mac-sync.sh, MAC_HOST/MAC_DIR), starts the
# demo server there, and runs ScreenshotTests on a fresh simulator install, light and dark,
# with the status bar at 9:41. Writes website/public/screenshots/ios-<scene>-<hell|dunkel>.webp.
#
#   SIM=...        simulator name (default: iPhone 16 Pro, else iPhone 17 Pro)
#   DEMO_PORT=...  demo server port on the Mac (default 8097; mac.sh uses 8099 on the same host)
#   KEEP_RAW=dir   also keep the raw PNGs there
set -euo pipefail
ROOT="$(cd "$(dirname "$0")/../.." && pwd)"
cd "$ROOT"
if [[ -z "${MAC_HOST:-}" && -f "$ROOT/.env" ]]; then
  MAC_HOST="$(grep -E '^MAC_HOST=' "$ROOT/.env" | cut -d= -f2- || true)"
fi
: "${MAC_HOST:?set MAC_HOST=user@host}"
export MAC_HOST
MAC_DIR="${MAC_DIR:-repos/talk-legacy}"
SIM="${SIM:-}"
PORT="${DEMO_PORT:-8097}"
OUT="$ROOT/website/public/screenshots"
RAW="$(mktemp -d)"
trap 'rm -rf "$RAW"' EXIT

scripts/mac-sync.sh

# On the Mac: build once, then per appearance a fresh demo server and a fresh app install.
ssh "$MAC_HOST" "zsh -l -s -- '$MAC_DIR' '$SIM' '$PORT'" <<'REMOTE'
set -euo pipefail
DIR=$1 SIM=$2 PORT=$3
cd ~/$DIR
SHOTS=/tmp/wolkenfunk-shots-ios
rm -rf $SHOTS; mkdir -p $SHOTS
# Own state dir: demo-server.sh keeps its pid there (mac.sh may run another one).
export XDG_RUNTIME_DIR=$SHOTS/run; mkdir -p $XDG_RUNTIME_DIR
devices=$(xcrun simctl list devices available)
if [[ -z $SIM ]]; then
  for s in "iPhone 16 Pro" "iPhone 17 Pro"; do
    print -r -- $devices | grep -qF "    $s (" && { SIM=$s; break; }
  done
fi
UDID=$(print -r -- $devices | grep -F "    $SIM (" | head -1 | sed -E 's/.*\(([0-9A-F-]{36})\).*/\1/')
[[ -n $UDID ]] || { echo "no simulator '$SIM'" >&2; exit 1; }
echo "simulator $SIM ($UDID)"
[[ -d ios/Generated/TalkCoreFFI.xcframework ]] || SLICES=sim ios/scripts/build-core.sh
cd ios
xcodegen generate -q
xcrun simctl boot $UDID 2>/dev/null || true
xcrun simctl bootstatus $UDID -b >/dev/null
xcodebuild build-for-testing -project TalkLegacy.xcodeproj -scheme TalkLegacy \
  -destination "id=$UDID" -derivedDataPath build/shots -quiet 2>&1 | grep -E 'error:' || true
trap '../scripts/screenshots/demo-server.sh stop >/dev/null 2>&1 || true; xcrun simctl status_bar $UDID clear 2>/dev/null || true' EXIT
for mode in light dark; do
  ../scripts/screenshots/demo-server.sh start 127.0.0.1:$PORT
  xcrun simctl uninstall $UDID org.wolkenfunk.app 2>/dev/null || true
  xcrun simctl ui $UDID appearance $mode
  xcrun simctl status_bar $UDID override --time 9:41 --dataNetwork wifi --wifiMode active --wifiBars 3 \
    --cellularMode active --cellularBars 4 --batteryState discharging --batteryLevel 100
  env TEST_RUNNER_TALK_SCREENSHOTS=1 TEST_RUNNER_TALK_DEV_SERVER=http://127.0.0.1:$PORT \
      TEST_RUNNER_TALK_DEV_USER=mira TEST_RUNNER_TALK_DEV_PASS=demo \
    xcodebuild test-without-building -project TalkLegacy.xcodeproj -scheme TalkLegacy \
      -destination "id=$UDID" -derivedDataPath build/shots -resultBundlePath $SHOTS/$mode.xcresult \
      -only-testing:TalkLegacyUITests/ScreenshotTests/testScenes 2>&1 \
    | grep -E "error:|Test Case .*(passed|failed)" || true
  ../scripts/screenshots/demo-server.sh stop >/dev/null
  xcrun xcresulttool export attachments --path $SHOTS/$mode.xcresult --output-path $SHOTS/$mode >/dev/null
done
REMOTE

scp -rq "$MAC_HOST:/tmp/wolkenfunk-shots-ios/light" "$MAC_HOST:/tmp/wolkenfunk-shots-ios/dark" "$RAW/"
mkdir -p "$OUT"
python3 -I - "$RAW" "$OUT" "${KEEP_RAW:-}" <<'PY'
import json, shutil, sys
from pathlib import Path
from PIL import Image
raw, out, keep = Path(sys.argv[1]), Path(sys.argv[2]), sys.argv[3]
scenes = {"chats", "ungelesen", "sprachnachricht"}
done = []
for mode, name in (("light", "hell"), ("dark", "dunkel")):
    for test in json.loads((raw / mode / "manifest.json").read_text()):
        for a in test["attachments"]:
            scene = a["suggestedHumanReadableName"].split("_0_")[0]
            if scene not in scenes:
                continue
            src = raw / mode / a["exportedFileName"]
            dst = out / f"ios-{scene}-{name}.webp"
            Image.open(src).convert("RGB").save(dst, "WEBP", quality=85, method=6)
            if keep:
                Path(keep).mkdir(parents=True, exist_ok=True)
                shutil.copy(src, Path(keep) / f"ios-{scene}-{name}.png")
            done.append(dst.name)
            print(dst.relative_to(out.parents[2]))
missing = {f"ios-{s}-{n}.webp" for s in scenes for n in ("hell", "dunkel")} - set(done)
if missing:
    sys.exit("missing: " + ", ".join(sorted(missing)))
PY
