#!/usr/bin/env bash
# Publishes the Windows build on https://pkg.wolkenfunk.org/windows: the NSIS installer, its
# blockmap and latest.yml (read by electron-updater in the app). Built by the CI
# (.github/workflows/desktop-windows.yml) on the tag vX.Y.Z; this script fetches the artifact.
#
#   scripts/release-windows.sh X.Y.Z               wait for the CI run of tag vX.Y.Z, stage, upload
#   SKIP_BUILD=1 scripts/release-windows.sh X.Y.Z  use what is already in dist/windows/
#   DRY_RUN=1 ...                                  stage locally, upload nothing; without a tag run
#                                                  it takes the last successful workflow_dispatch run
#   RUN_ID=123 ...                                 take this CI run instead
#
# Only touches windows/ on the server. A version that is already published is refused.
set -euo pipefail
cd "$(dirname "$0")/.."

VERSION="${1:-$(node -p 'require("./package.json").version')}"
HOST="${PKG_HOST:-jw.helsinki}"
REMOTE="${PKG_REMOTE:-/root/claudes/wolkenfunk/pkg}"
STAGE="${PKG_STAGE:-$HOME/.cache/wolkenfunk/pkg}"
DRY_RUN="${DRY_RUN:-0}"
WORKFLOW=desktop-windows.yml
ARTIFACT=wolkenfunk-windows
KEEP=3
DIST=dist/windows
FIXED=Wolkenfunk-Setup.exe

tag_run() {
  gh run list --workflow "$WORKFLOW" --event push --branch "v$VERSION" -L 1 \
    --json databaseId --jq '.[0].databaseId // empty'
}

# --- Artifact ----------------------------------------------------------------------------
if [ "${SKIP_BUILD:-0}" != 1 ]; then
  run="${RUN_ID:-}"
  if [ -z "$run" ]; then
    run=$(tag_run)
    if [ -z "$run" ] && [ "$DRY_RUN" = 1 ]; then
      run=$(gh run list --workflow "$WORKFLOW" --event workflow_dispatch --status success -L 1 \
        --json databaseId --jq '.[0].databaseId // empty')
      [ -n "$run" ] && echo "DRY_RUN: no run for v$VERSION, taking workflow_dispatch run $run"
    fi
    # The tag was just pushed: give GitHub a few minutes to start the run.
    for _ in $(seq 60); do
      [ -n "$run" ] && break
      sleep 10
      run=$(tag_run)
    done
    [ -n "$run" ] || { echo "no CI run for v$VERSION" >&2; exit 1; }
  fi
  echo "CI run $run"
  gh run watch "$run" --exit-status --interval 30 >/dev/null
  rm -rf "$DIST"
  gh run download "$run" -n "$ARTIFACT" -D "$DIST"
fi

YML="$DIST/latest.yml"
[ -f "$YML" ] || { echo "missing $YML" >&2; exit 1; }
BUILT=$(sed -n 's/^version: *//p' "$YML" | tr -d "'\"")
if [ "$BUILT" != "$VERSION" ]; then
  [ "$DRY_RUN" = 1 ] || { echo "$YML is for $BUILT, not $VERSION" >&2; exit 1; }
  echo "DRY_RUN: the build is $BUILT, staging that instead of $VERSION"
  VERSION=$BUILT
fi
EXE="Wolkenfunk-Setup-$VERSION.exe"
for f in "$DIST/$EXE" "$DIST/$EXE.blockmap"; do [ -f "$f" ] || { echo "missing $f" >&2; exit 1; }; done
# latest.yml must describe exactly this installer (electron-updater checks the sha512).
SHA512=$(openssl dgst -sha512 -binary "$DIST/$EXE" | base64 -w0)
grep -q "path: $EXE" "$YML" && grep -q "sha512: $SHA512" "$YML" || { echo "$YML does not match $EXE" >&2; exit 1; }

# --- Stage -------------------------------------------------------------------------------
OUT="$STAGE/windows"
mkdir -p "$OUT"
if [ "$DRY_RUN" != 1 ] && ssh "$HOST" test -d "$REMOTE/windows"; then
  rsync -a --delete "$HOST:$REMOTE/windows/" "$OUT/"
fi
if [ -e "$OUT/$EXE" ] && [ "$DRY_RUN" != 1 ]; then
  echo "$VERSION is already published" >&2; exit 1
fi

cp "$DIST/$EXE" "$DIST/$EXE.blockmap" "$OUT/"
cp "$YML" "$OUT/latest.yml"
ln -sfn "$EXE" "$OUT/$FIXED"
# Keep the newest $KEEP versions (by version, not date); the blockmaps go with their installer.
ls "$OUT" | sed -n 's/^Wolkenfunk-Setup-\(.*\)\.exe$/\1/p' | sort -V | head -n -"$KEEP" | while read -r old; do
  rm -v "$OUT/Wolkenfunk-Setup-$old.exe" "$OUT/Wolkenfunk-Setup-$old.exe.blockmap"
done
(cd "$OUT" && sha256sum $FIXED Wolkenfunk-Setup-*.exe > SHA256SUMS)

if [ "$DRY_RUN" = 1 ]; then
  echo "DRY_RUN: staged in $OUT, nothing uploaded"
  exit 0
fi

# --- Upload ------------------------------------------------------------------------------
# New files first, then latest.yml and the link switch over (--delay-updates), old files last.
rsync -a --links --delay-updates --delete-after --chown=101000:101000 "$OUT/" "$HOST:$REMOTE/windows/"
echo "→ published Windows $VERSION on https://pkg.wolkenfunk.org/windows"
