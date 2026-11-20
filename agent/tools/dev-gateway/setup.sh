#!/bin/bash
# Richtet das abgeschottete Test-Gateway ein: OpenClaw lokal installieren, Konfiguration aus der Vorlage erzeugen.
#   setup.sh <Nextcloud-Adresse> <Agenten-Konto> <freigegebenes Konto>
# Nichts davon landet im echten Home: der gesamte Zustand liegt unter dev-gateway/home (nicht einchecken).
set -euo pipefail
here="$(cd "$(dirname "$0")" && pwd)"
URL="${1:?Nextcloud-Adresse fehlt}"; AGENT="${2:?Agenten-Konto fehlt}"; ALLOW="${3:?freigegebenes Konto fehlt}"
cd "$here"
[ -d node_modules/openclaw ] || { [ -f package.json ] || npm init -y >/dev/null; npm install openclaw@2026.11.3 --no-fund --no-audit; }
mkdir -p home/.openclaw home/workspace logs
sed -e "s#REPLACE_WITH_ABSOLUTE_PATH/dev-gateway#$here#" -e "s#REPLACE_WITH_ABSOLUTE_PATH/\.\./openclaw-nctalk#$(cd "$here/../../openclaw-nctalk" && pwd)#" \
    -e "s#https://cloud.example.org#$URL#" -e "s#agent-testkonto#$AGENT#" -e "s#test-account-a#$ALLOW#" openclaw.example.json > home/.openclaw/openclaw.json
OPENCLAW_HOME="$here/home" ./node_modules/.bin/openclaw config validate | tail -1
