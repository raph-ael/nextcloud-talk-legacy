#!/bin/bash
# Packt das Plugin als Archiv (ohne Entwicklungsdateien) für install-in-container.sh.
set -euo pipefail
cd "$(dirname "$0")/.."
OUT="${1:-/tmp/openclaw-nctalk.tgz}"
tar -czf "$OUT" --owner=0 --group=0 package.json openclaw.plugin.json index.js setup-entry.js README.md src
echo "$OUT"
