#!/usr/bin/env bash
# One release of every app, on the Linux release machine (docs/release.md):
#
#   scripts/release.sh X.Y.Z [development|stable]
#   ONLY=linux,android scripts/release.sh X.Y.Z    only some channels (linux, windows, mac, android)
#   DRY_RUN=1 scripts/release.sh X.Y.Z             build and stage locally, no commit, tag or upload
#
# Steps: checks → scripts/set-version.sh → commit "Release X.Y.Z" + tag vX.Y.Z → push (starts the
# Windows CI) → release-linux.sh, release-android.sh, release-mac.sh, release-windows.sh (waits for
# the CI) → https://pkg.wolkenfunk.org/latest.json. iOS goes through Codemagic separately.
#
# Each channel script only touches its own folder on the server and can run alone; this script
# runs them one after the other and stops at the first failure (rerun with ONLY= for the rest).
set -euo pipefail
cd "$(dirname "$0")/.."
ROOT=$PWD

VERSION="${1:?usage: scripts/release.sh X.Y.Z [development|stable]}"
TYPE="${2:-development}"
ONLY="${ONLY:-linux,android,mac,windows}"
DRY_RUN="${DRY_RUN:-0}"
HOST="${PKG_HOST:-jw.helsinki}"
REMOTE="${PKG_REMOTE:-/root/claudes/wolkenfunk/pkg}"
STAGE="${PKG_STAGE:-$HOME/.cache/wolkenfunk/pkg}"
export PKG_HOST="$HOST" PKG_REMOTE="$REMOTE" PKG_STAGE="$STAGE" DRY_RUN

want() { [[ ",$ONLY," == *",$1,"* ]]; }
step() { printf '\n== %s\n' "$*"; }

# Deployment settings and key passwords from .env, only the ones the channel scripts need
# (never the whole file: it also holds account passwords).
load_env() {
  local key line
  for key in "$@"; do
    [[ -n "${!key:-}" ]] && continue
    line=$(grep -E "^$key=" .env 2>/dev/null | tail -n1) || continue
    line="${line#*=}"; line="${line%\"}"; line="${line#\"}"
    export "$key=$line"
  done
}
load_env MAC_HOST WOLKENFUNK_KEYSTORE WOLKENFUNK_KEYSTORE_PASSWORD WOLKENFUNK_KEY_ALIAS WOLKENFUNK_KEY_PASSWORD

# --- Checks ------------------------------------------------------------------------------
step "checks"
[[ "$(git rev-parse --abbrev-ref HEAD)" == main ]] || { echo "not on main" >&2; exit 1; }
[[ -z "$(git status --porcelain)" ]] || { echo "working tree not clean" >&2; exit 1; }
git fetch -q origin main
[[ "$(git rev-parse HEAD)" == "$(git rev-parse origin/main)" ]] || { echo "main differs from origin/main" >&2; exit 1; }
if git rev-parse -q --verify "refs/tags/v$VERSION" >/dev/null; then
  echo "tag v$VERSION exists already" >&2; exit 1
fi

# --- Version, commit, tag ----------------------------------------------------------------
step "version $VERSION"
scripts/set-version.sh "$VERSION" "$TYPE"
VERSION_FILES=(desktop/package.json desktop/package-lock.json android/app/build.gradle.kts ios/project.yml
               desktop/packaging/linux/org.wolkenfunk.Wolkenfunk.metainfo.xml)
if [[ "$DRY_RUN" == 1 ]]; then
  # Leave the tree as it was, whatever happens.
  trap 'git -C "$ROOT" checkout -q -- "${VERSION_FILES[@]}"' EXIT
else
  git add "${VERSION_FILES[@]}"
  git diff --cached --quiet || git commit -q -m "Release $VERSION"
  git tag -a "v$VERSION" -m "Wolkenfunk $VERSION"
  git push -q origin main "v$VERSION"
fi

# --- Channels ----------------------------------------------------------------------------
if want linux;   then step "linux";   (cd desktop && scripts/release-linux.sh); fi
if want android; then step "android"; android/scripts/release-android.sh; fi
if want mac;     then step "mac";     desktop/scripts/release-mac.sh; fi
if want windows; then step "windows"; desktop/scripts/release-windows.sh "$VERSION"; fi

# --- latest.json -------------------------------------------------------------------------
# What the website and humans read: the newest version per channel with checksums. Built from
# the stage, which every channel script mirrors from the server before adding its files.
step "latest.json"
if [[ "$DRY_RUN" != 1 ]]; then
  for dir in apt appimage windows mac android; do
    mkdir -p "$STAGE/$dir"
    rsync -a --delete "$HOST:$REMOTE/$dir/" "$STAGE/$dir/" 2>/dev/null || true
  done
fi
python3 - "$STAGE" "$VERSION" <<'EOF'
import hashlib, json, os, sys, datetime
stage, version = sys.argv[1:]
links = {  # channel: fixed link (symlink to the newest file), see docs/release.md
    "windows": "windows/Wolkenfunk-Setup.exe",
    "mac": "mac/Wolkenfunk.dmg",
    "linux-deb": "apt/wolkenfunk_amd64.deb",
    "linux-appimage": "appimage/Wolkenfunk-x86_64.AppImage",
    "android": "android/wolkenfunk.apk",
}
out = {"version": version, "date": datetime.date.today().isoformat(), "downloads": {}}
for channel, link in links.items():
    path = os.path.join(stage, link)
    if not os.path.exists(path):
        continue
    target = os.path.join(os.path.dirname(link), os.path.basename(os.path.realpath(path)))
    data = open(path, "rb").read()
    out["downloads"][channel] = {
        "url": f"https://pkg.wolkenfunk.org/{link}",
        "file": f"https://pkg.wolkenfunk.org/{target}",
        "size": len(data),
        "sha256": hashlib.sha256(data).hexdigest(),
    }
json.dump(out, open(os.path.join(stage, "latest.json"), "w"), indent=2)
print(json.dumps(out, indent=2))
EOF
if [[ "$DRY_RUN" == 1 ]]; then
  echo "DRY_RUN: staged in $STAGE, nothing committed, tagged or uploaded"
  exit 0
fi
rsync -a --chown=101000:101000 "$STAGE/latest.json" "$HOST:$REMOTE/latest.json"
echo "→ Wolkenfunk $VERSION published. iOS: Codemagic (tag ios-v$VERSION), only when agreed."
