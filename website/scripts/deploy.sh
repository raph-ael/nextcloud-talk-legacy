#!/usr/bin/env bash
# Spielt website/public/ nach wolkenfunk.org aus.
# Ziel: der Ordner site/ neben dem Paket-Ordner auf dem Server (PKG_HOST aus der Umgebung oder
# aus .env, siehe scripts/pkg-env.sh). Er ist in den Webserver eingehängt: sofort live.
#   website/scripts/deploy.sh            # ausspielen
#   website/scripts/deploy.sh --dry-run  # nur anzeigen, was sich ändern würde
# Vorher prüft stage.py, welche Downloads auf pkg.wolkenfunk.org schon existieren; die übrigen
# erscheinen als „bald“. Nach einem Release also einfach erneut ausspielen.
set -euo pipefail

source "$(dirname "$0")/../../scripts/pkg-env.sh"
HOST="$PKG_HOST"
DEST="$(dirname "$PKG_REMOTE")/site/"
STAGE="$(mktemp -d)"
trap 'rm -rf "$STAGE"' EXIT

"$(dirname "$0")/stage.py" "$STAGE/public"

rsync -rlt --checksum --delete --itemize-changes \
  --chown=101000:101000 --chmod=D755,F644 \
  "$@" "$STAGE/public/" "$HOST:$DEST"

if [[ " $* " != *" --dry-run "* && " $* " != *" -n "* ]]; then
  # Prüfen über den Reverse-Proxy (öffentliche IP ist vom Host aus nicht erreichbar)
  ssh "$HOST" 'pct exec 100 -- curl -fsS -o /dev/null -w "https://wolkenfunk.org/ → %{http_code}\n" \
    --resolve wolkenfunk.org:443:127.0.0.1 https://wolkenfunk.org/'
fi
