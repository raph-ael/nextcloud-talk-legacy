#!/usr/bin/env bash
# Publishes the Linux build on https://pkg.wolkenfunk.org: the .deb into the signed APT repository,
# the AppImage plus its update feed (latest-linux.yml, read by electron-updater in the app).
#
#   scripts/release-linux.sh            build (npm run dist:linux), stage, sign, upload
#   SKIP_BUILD=1 scripts/release-linux.sh   use what is already in dist/
#   DRY_RUN=1 ...                       stage and sign locally, upload nothing
#
# Bump "version" in package.json first: a version that is already published is refused.
# Server: jw.helsinki:/root/claudes/wolkenfunk/pkg (bind mount into CT122, served by nginx there).
# Signing key: ~/.config/wolkenfunk/apt-gnupg (override with WOLKENFUNK_GNUPGHOME).
set -euo pipefail
cd "$(dirname "$0")/.."

HOST="${PKG_HOST:-jw.helsinki}"
REMOTE="${PKG_REMOTE:-/root/claudes/wolkenfunk/pkg}"
STAGE="${PKG_STAGE:-$HOME/.cache/wolkenfunk/pkg}"
KEEP=3

[ "${SKIP_BUILD:-0}" = 1 ] || npm run dist:linux

VERSION=$(node -p 'require("./package.json").version')
DEB=$(ls dist/wolkenfunk_"$VERSION"_*.deb)
APPIMAGE=$(ls dist/Wolkenfunk-"$VERSION"-*.AppImage)
FEED=dist/latest-linux.yml
for f in $DEB $APPIMAGE $FEED; do [ -f "$f" ] || { echo "missing $f" >&2; exit 1; }; done
grep -q "version: $VERSION" "$FEED" || { echo "$FEED is not for $VERSION" >&2; exit 1; }

# Mirror the published state, so the pool keeps the older versions.
mkdir -p "$STAGE"
if [ "${DRY_RUN:-0}" != 1 ]; then
  rsync -a --delete "$HOST:$REMOTE/" "$STAGE/"
fi

# APT
scripts/apt-repo.py "$STAGE/apt" --add $DEB --keep "$KEEP"

# AppImage: versioned file, "latest" symlink for the download link, feed for the updater.
mkdir -p "$STAGE/appimage"
cp "$APPIMAGE" "$STAGE/appimage/"
cp "$FEED" "$STAGE/appimage/latest-linux.yml"
ARCH_FILE=$(basename "$APPIMAGE" | sed "s/-$VERSION-/-/")   # Wolkenfunk-x86_64.AppImage
ln -sfn "$(basename "$APPIMAGE")" "$STAGE/appimage/$ARCH_FILE"
ls -t "$STAGE"/appimage/Wolkenfunk-*-*.AppImage | grep -v -- "/$ARCH_FILE$" | tail -n +$((KEEP + 1)) | xargs -r rm -v

if [ "${DRY_RUN:-0}" = 1 ]; then
  echo "DRY_RUN: staged in $STAGE, nothing uploaded"
  exit 0
fi

# Upload: new files first, then switch the metadata over (--delay-updates), old files last.
rsync -a --links --delay-updates --delete-after --chown=101000:101000 "$STAGE/" "$HOST:$REMOTE/"
echo "→ published $VERSION on https://pkg.wolkenfunk.org"
