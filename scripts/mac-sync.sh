#!/usr/bin/env bash
# Mirrors the working tree to a Mac build host (for iOS / macOS builds) and optionally runs a
# command there:   scripts/mac-sync.sh [command ...]
# The host comes from MAC_HOST (env or .env), e.g. MAC_HOST=user@192.168.1.20; the checkout
# lives in MAC_DIR (default ~/repos/talk-legacy).
set -euo pipefail
ROOT="$(cd "$(dirname "$0")/.." && pwd)"
if [[ -z "${MAC_HOST:-}" && -f "$ROOT/.env" ]]; then
  MAC_HOST="$(grep -E '^MAC_HOST=' "$ROOT/.env" | cut -d= -f2- || true)"
fi
: "${MAC_HOST:?set MAC_HOST=user@host}"
MAC_DIR="${MAC_DIR:-repos/talk-legacy}"

rsync -az --delete \
  --exclude '.git/' --exclude '.env' --exclude '.env.*' --exclude '*.p8' \
  --exclude '/core/target/' --exclude '/push-proxy/target/' \
  --exclude 'node_modules/' --exclude '/desktop/out/' --exclude '/desktop/dist/' \
  --exclude '/desktop/native/' --exclude '/ios/Generated/' --exclude '/ios/*.xcodeproj/' \
  --exclude '/ios/build/' --exclude '/android/' \
  "$ROOT/" "$MAC_HOST:$MAC_DIR/"

if [[ $# -gt 0 ]]; then
  ssh "$MAC_HOST" "zsh -lc 'cd $MAC_DIR && $*'"
fi
