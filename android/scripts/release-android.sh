#!/usr/bin/env bash
# Publishes the signed Android APKs on https://pkg.wolkenfunk.org/android:
#   wolkenfunk-X.Y.Z-arm64-v8a.apk, wolkenfunk-X.Y.Z-armeabi-v7a.apk, wolkenfunk-X.Y.Z.apk (universal),
#   wolkenfunk.apk (symlink to the newest universal APK, linked from the website) and SHA256SUMS.
#
#   scripts/release-android.sh              build (assembleRelease with ABI splits), stage, upload
#   SKIP_BUILD=1 scripts/release-android.sh use the APKs already in app/build/outputs/apk/release
#   DRY_RUN=1 ...                           stage in $PKG_STAGE/android, upload nothing
#
# The version comes from app/build.gradle.kts (set it with scripts/set-version.sh first); a version
# that is already published is refused. Only the android/ folder of the webroot is touched.
#
# Signing: WOLKENFUNK_KEYSTORE (default ~/claudes/talk-secrets/wolkenfunk-release.jks),
# WOLKENFUNK_KEYSTORE_PASSWORD, WOLKENFUNK_KEY_ALIAS, WOLKENFUNK_KEY_PASSWORD. Values that are not
# in the environment are read from the file $WOLKENFUNK_ENV (default ~/claudes/talk-legacy/.env),
# only its WOLKENFUNK_* lines. Without a complete signing config the script stops.
set -euo pipefail
cd "$(dirname "$0")/.."

HOST="${PKG_HOST:-jw.helsinki}"
REMOTE="${PKG_REMOTE:-/root/claudes/wolkenfunk/pkg}"
STAGE="${PKG_STAGE:-$HOME/.cache/wolkenfunk/pkg}/android"
KEEP=3
DRY_RUN="${DRY_RUN:-0}"

# --- signing config --------------------------------------------------------------------------
ENV_FILE="${WOLKENFUNK_ENV:-$HOME/claudes/talk-legacy/.env}"
if [ -f "$ENV_FILE" ]; then
  while IFS='=' read -r key value; do
    [[ "$key" =~ ^WOLKENFUNK_[A-Z_]+$ ]] || continue
    [ -n "${!key:-}" ] && continue
    value="${value%$'\r'}"; value="${value#[\"\']}"; value="${value%[\"\']}"
    export "$key=$value"
  done < <(grep -E '^\s*(export\s+)?WOLKENFUNK_[A-Z_]+=' "$ENV_FILE" | sed -E 's/^\s*(export\s+)?//')
fi
export WOLKENFUNK_KEYSTORE="${WOLKENFUNK_KEYSTORE:-$HOME/claudes/talk-secrets/wolkenfunk-release.jks}"
[ -f "$WOLKENFUNK_KEYSTORE" ] || { echo "keystore not found: $WOLKENFUNK_KEYSTORE" >&2; exit 1; }
for v in WOLKENFUNK_KEYSTORE_PASSWORD WOLKENFUNK_KEY_ALIAS WOLKENFUNK_KEY_PASSWORD; do
  [ -n "${!v:-}" ] || { echo "$v is not set (environment or $ENV_FILE)" >&2; exit 1; }
done

# --- version -----------------------------------------------------------------------------------
VERSION=$(sed -nE 's/^\s*versionName = "([^"]+)".*/\1/p' app/build.gradle.kts)
CODE=$(sed -nE 's/^\s*versionCode = ([0-9]+).*/\1/p' app/build.gradle.kts)
[[ "$VERSION" =~ ^[0-9]+\.[0-9]+\.[0-9]+$ ]] || { echo "no versionName X.Y.Z in app/build.gradle.kts" >&2; exit 1; }
IFS=. read -r X Y Z <<<"$VERSION"
[ "$CODE" = $((X * 10000 + Y * 100 + Z)) ] || { echo "versionCode $CODE does not match $VERSION (use scripts/set-version.sh)" >&2; exit 1; }

# Already published? ssh exits 0 (yes), 1 (no) or 255 (server not reachable).
set +e
ssh -o BatchMode=yes -o ConnectTimeout=10 "$HOST" test -e "$REMOTE/android/wolkenfunk-$VERSION.apk"
status=$?
set -e
case "$status" in
  0) echo "version $VERSION is already published" >&2; exit 1 ;;
  1) ;;
  *) if [ "$DRY_RUN" = 1 ]; then
       echo "note: $HOST not reachable, published-version check skipped (DRY_RUN)"
     else
       echo "$HOST not reachable" >&2; exit 1
     fi ;;
esac

# --- build -------------------------------------------------------------------------------------
OUT=app/build/outputs/apk/release
if [ "${SKIP_BUILD:-0}" != 1 ]; then
  rm -rf "$OUT"
  ./gradlew --no-configuration-cache -Pwolkenfunk.abiSplits=true :app:assembleRelease
fi

SDK=$(sed -nE 's/^sdk\.dir=(.*)/\1/p' local.properties 2>/dev/null || true)
SDK="${ANDROID_HOME:-${SDK:-$HOME/Android/Sdk}}"
BUILD_TOOLS=$(ls -d "$SDK"/build-tools/* | sort -V | tail -1)
declare -A APKS=(
  ["wolkenfunk-$VERSION-arm64-v8a.apk"]="$OUT/app-arm64-v8a-release.apk"
  ["wolkenfunk-$VERSION-armeabi-v7a.apk"]="$OUT/app-armeabi-v7a-release.apk"
  ["wolkenfunk-$VERSION.apk"]="$OUT/app-universal-release.apk"
)
for name in "${!APKS[@]}"; do
  apk="${APKS[$name]}"
  [ -f "$apk" ] || { echo "missing $apk (unsigned build? see signing above)" >&2; exit 1; }
  "$BUILD_TOOLS/apksigner" verify "$apk" || { echo "$apk is not signed" >&2; exit 1; }
  badging=$("$BUILD_TOOLS/aapt2" dump badging "$apk" | sed -n 1p)
  grep -q "name='org.wolkenfunk.app' versionCode='$CODE' versionName='$VERSION'" <<<"$badging" \
    || { echo "$apk is not org.wolkenfunk.app $VERSION ($CODE): $badging" >&2; exit 1; }
done

# --- stage -------------------------------------------------------------------------------------
mkdir -p "$STAGE"
if [ "$DRY_RUN" != 1 ]; then
  rsync -a --delete "$HOST:$REMOTE/android/" "$STAGE/"
fi
for name in "${!APKS[@]}"; do cp "${APKS[$name]}" "$STAGE/$name"; done
ln -sfn "wolkenfunk-$VERSION.apk" "$STAGE/wolkenfunk.apk"

# Keep the newest $KEEP versions (by version number, not by file date).
mapfile -t versions < <(ls "$STAGE" | sed -nE 's/^wolkenfunk-([0-9]+\.[0-9]+\.[0-9]+)\.apk$/\1/p' | sort -V)
for old in "${versions[@]:0:$(( ${#versions[@]} > KEEP ? ${#versions[@]} - KEEP : 0 ))}"; do
  rm -v "$STAGE"/wolkenfunk-"$old".apk "$STAGE"/wolkenfunk-"$old"-*.apk 2>/dev/null || true
done

(cd "$STAGE" && find . -maxdepth 1 -name 'wolkenfunk-*.apk' -type f -printf '%f\n' | sort -V | xargs sha256sum > SHA256SUMS)

if [ "$DRY_RUN" = 1 ]; then
  ls -l "$STAGE"
  echo "DRY_RUN: staged $VERSION in $STAGE, nothing uploaded"
  exit 0
fi

# Upload: new files first, then switch the symlink over (--delay-updates), old files last.
rsync -a --links --delay-updates --delete-after --chown=101000:101000 "$STAGE/" "$HOST:$REMOTE/android/"
echo "→ published Android $VERSION on https://pkg.wolkenfunk.org/android/"
