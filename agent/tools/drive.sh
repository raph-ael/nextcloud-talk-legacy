#!/bin/bash
# Testfahrer: schreibt als Test-Account A an den Agenten und protokolliert, was im Chat sichtbar wird.
#   AGENT_USER=<Konto des Agenten> drive.sh "<Nachricht>" [Sekunden]
#   ROOM_TOKEN=<Raum>              drive.sh "<Nachricht>" [Sekunden]     (statt des 1:1 mit AGENT_USER)
#   AS=b …                         als Test-Account B schreiben
#   drive.sh "@datei:/pfad/zur/datei" [Sekunden] ["Bildunterschrift"]    Datei senden
#   SECOND="zweite Nachricht" SECOND_DELAY=2500 …                        zweite Nachricht mitten im Lauf
here="$(cd "$(dirname "$0")" && pwd)"
set -a; . "${TALK_ENV:-$here/../../.env}"; set +a
if [ "$AS" = "b" ]; then export OBS_USER="$BOT_B_USER" OBS_PASS="$BOT_B_PASS"; else export OBS_USER="$BOT_A_USER" OBS_PASS="$BOT_A_PASS"; fi
cd "$here"
exec node drive.mjs "$@"
