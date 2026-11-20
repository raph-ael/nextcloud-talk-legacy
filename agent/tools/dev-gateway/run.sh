#!/bin/bash
# Startet Modell-Attrappe und Test-Gateway im Vordergrund (Strg+C beendet beides).
# Das App-Passwort des Agenten-Kontos muss in NCTALK_APP_PASSWORD stehen; es wird nirgends gespeichert.
here="$(cd "$(dirname "$0")" && pwd)"; cd "$here"
[ -n "$NCTALK_APP_PASSWORD" ] || { echo "NCTALK_APP_PASSWORD fehlt" >&2; exit 1; }
export OPENCLAW_HOME="$here/home"
MOCK_DEBUG=1 MOCK_DEBUG_FILE="$here/logs/mock-last-user.txt" node ../mock-model.mjs > logs/mock.log 2>&1 &
mock=$!; trap 'kill $mock 2>/dev/null' EXIT
./node_modules/.bin/openclaw gateway --port 19111
