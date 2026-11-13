#!/usr/bin/env bash
# Spielt website/public/ nach wolkenfunk.org aus.
# Ziel: Bind-Mount auf dem PVE-Host jw.helsinki → CT122 /var/www/wolkenfunk.org (sofort live).
#   website/scripts/deploy.sh            # ausspielen
#   website/scripts/deploy.sh --dry-run  # nur anzeigen, was sich ändern würde
set -euo pipefail

HOST="${WOLKENFUNK_HOST:-jw.helsinki}"
DEST="/root/claudes/wolkenfunk/site/"
SRC="$(cd "$(dirname "$0")/../public" && pwd)/"

rsync -rlt --checksum --delete --itemize-changes \
  --chown=101000:101000 --chmod=D755,F644 \
  "$@" "$SRC" "$HOST:$DEST"

if [[ " $* " != *" --dry-run "* && " $* " != *" -n "* ]]; then
  # Prüfen über den Reverse-Proxy (öffentliche IP ist vom Host aus nicht erreichbar)
  ssh "$HOST" 'pct exec 100 -- curl -fsS -o /dev/null -w "https://wolkenfunk.org/ → %{http_code}\n" \
    --resolve wolkenfunk.org:443:127.0.0.1 https://wolkenfunk.org/'
fi
