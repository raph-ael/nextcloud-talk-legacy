#!/bin/bash
# Startet die Agenten-Attrappe als Test-Account B im 1:1 der beiden Test-Accounts.
#   agent-sim.sh [Sekunden]        (Standard 300)
# Zugangsdaten kommen aus der .env im Repo-Root (oder aus TALK_ENV); ausgegeben wird davon nichts.
# Anderer Raum: SIM_ROOM=<Raumtoken>.
here="$(cd "$(dirname "$0")" && pwd)"
set -a; . "${TALK_ENV:-$here/../../.env}"; set +a
cd "$here"
SIM_USER="$BOT_B_USER" SIM_PASS="$BOT_B_PASS" exec node agent-sim.mjs "${SIM_ROOM:-$CALL_TEST_ROOM_TOKEN}" "${1:-300}"
