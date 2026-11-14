#!/usr/bin/env bash
# Publishes the macOS build on https://pkg.wolkenfunk.org/mac: a universal .dmg (x86_64 + arm64,
# ad-hoc signed, no updater until there is a Developer ID signature). Runs on the Linux release
# machine and builds on a Mac build host over SSH (scripts/mac-sync.sh).
#
#   scripts/release-mac.sh                    sync, build on the Mac (npm run dist:mac), fetch, upload
#   SKIP_BUILD=1 scripts/release-mac.sh       fetch the .dmg already built on the Mac
#   DRY_RUN=1 ...                             stage locally, upload nothing
#
# Set the version with scripts/set-version.sh first: a version that is already published (with
# other content) is refused.
# MAC_HOST (user@host) comes from the environment or from the .env file (WOLKENFUNK_ENV, default
# .env in the repository root); MAC_DIR is the checkout on the Mac (default repos/wolkenfunk-desktop).
set -euo pipefail
cd "$(dirname "$0")/.."
ROOT="$(cd .. && pwd)"

HOST="${PKG_HOST:-jw.helsinki}"
REMOTE="${PKG_REMOTE:-/root/claudes/wolkenfunk/pkg}"
STAGE="${PKG_STAGE:-$HOME/.cache/wolkenfunk/pkg}/mac"
KEEP=3

ENV_FILE="${WOLKENFUNK_ENV:-$ROOT/.env}"
if [ -z "${MAC_HOST:-}" ] && [ -f "$ENV_FILE" ]; then
  MAC_HOST="$(grep -E '^MAC_HOST=' "$ENV_FILE" | cut -d= -f2- || true)"
fi
: "${MAC_HOST:?set MAC_HOST=user@host (environment or .env)}"
export MAC_HOST
export MAC_DIR="${MAC_DIR:-repos/wolkenfunk-desktop}"

VERSION=$(node -p 'require("./package.json").version')
DMG="Wolkenfunk-$VERSION-universal.dmg"
LINK=Wolkenfunk.dmg

if [ "${SKIP_BUILD:-0}" != 1 ]; then
  "$ROOT/scripts/mac-sync.sh" "cd desktop && npm ci && npm run dist:mac && hdiutil verify dist/$DMG"
fi

mkdir -p dist
rsync -a "$MAC_HOST:$MAC_DIR/desktop/dist/$DMG" "dist/$DMG"
[ -s "dist/$DMG" ] || { echo "missing dist/$DMG" >&2; exit 1; }

# Mirror the published folder (this script touches mac/ only), so the older versions stay.
mkdir -p "$STAGE"
if [ "${DRY_RUN:-0}" != 1 ] && ssh "$HOST" test -d "$REMOTE/mac"; then
  rsync -a --delete "$HOST:$REMOTE/mac/" "$STAGE/"
fi
if [ -e "$STAGE/$DMG" ] && ! cmp -s "$STAGE/$DMG" "dist/$DMG"; then
  echo "$DMG is already published with different content; set a new version" >&2
  exit 1
fi

# Versioned file, fixed link for the download page, checksums, the last $KEEP versions.
cp "dist/$DMG" "$STAGE/$DMG"
ln -sfn "$DMG" "$STAGE/$LINK"
ls "$STAGE"/Wolkenfunk-*-universal.dmg | sort -V | head -n -"$KEEP" | xargs -r rm -v
(cd "$STAGE" && sha256sum "$LINK" Wolkenfunk-*-universal.dmg > SHA256SUMS)

if [ "${DRY_RUN:-0}" = 1 ]; then
  echo "DRY_RUN: staged in $STAGE, nothing uploaded"
  exit 0
fi

# Upload: new files first, then switch the link over (--delay-updates), old files last.
rsync -a --links --delay-updates --delete-after --chown=101000:101000 "$STAGE/" "$HOST:$REMOTE/mac/"
echo "→ published $DMG on https://pkg.wolkenfunk.org/mac/$LINK"
