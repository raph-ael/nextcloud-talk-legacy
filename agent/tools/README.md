# Werkzeuge für die Agenten-Anbindung

Alles hier läuft mit Node ≥ 24 ohne zusätzliche Pakete (Ausnahme: das Test-Gateway installiert OpenClaw lokal).
Zugangsdaten kommen aus der `.env` im Repo-Root (`NC_URL`, `BOT_A_USER/PASS`, `BOT_B_USER/PASS`,
`CALL_TEST_ROOM_TOKEN`) oder aus der Datei, auf die `TALK_ENV` zeigt. Kein Skript gibt Zugangsdaten aus.

| Datei | Zweck |
|---|---|
| `agent-sim.mjs`, `agent-sim.sh` | **Agenten-Attrappe** für Client-Tests: verhält sich im Chat wie das Plugin, ohne Gateway und Modell |
| `drive.mjs`, `drive.sh` | **Testfahrer**: schreibt als Test-Account und protokolliert, was ein Mensch im Chat sähe |
| `mock-model.mjs` | **Modell-Attrappe** (OpenAI-kompatibel) für das Test-Gateway: gestreamte Antworten, auf Wunsch ein Werkzeugaufruf |
| `dev-gateway/` | abgeschottetes **Test-Gateway** mit dem Plugin (eigenes `OPENCLAW_HOME`) |
| `push-probe.mjs` | Messung: Unterdrückt eine aktive Sitzung im Raum die Benachrichtigungen? |
| `reaction-probe.mjs` | Messung: Wann erzeugt eine Reaktion eine Benachrichtigung? |
| `ws-probe.mjs` | Messung: Welche Ereignisse liefert der Signaling-Server bei Senden, Bearbeiten, Reagieren, Löschen? |
| `talk-spike.mjs` | erster Machbarkeitstest (Tippen ohne Browser, Bearbeiten, Löschen, Latenzen) |

## Agenten-Attrappe

```bash
agent/tools/agent-sim.sh 180        # 180 s lang als Test-Account B im 1:1 der Test-Accounts
```

Sie antwortet auf jede Nachricht der Gegenseite. Stichworte in der Nachricht steuern den Ablauf:

| Stichwort | Ablauf |
|---|---|
| (keins) | Tippen, stille Statusnachricht „Arbeitet …“, drei Bearbeitungen, Antwort als neue Nachricht, Statusnachricht wird gelöscht |
| `quittung` | wie oben, aber sofort ein stilles „👀“, das zur Statusnachricht wird (Standardverhalten des Plugins) |
| `kurz` | nur Tippen, dann eine kurze Antwort |
| `partial` | die Antwort wächst per Bearbeiten mit und ist am Ende die fertige Nachricht |
| `warte N` | N Sekunden nur Tippen, dann antworten |
| `fehler` | statt der Antwort eine Fehlermeldung; die Statusnachricht wird ebenfalls gelöscht |
| `abbruch` | die Statusnachricht bleibt stehen, es kommt nichts mehr (Gateway mitten im Lauf beendet) |

Die Attrappe benutzt `nc-client.js` und `signaling.js` des Plugins, sendet also dieselben Aufrufe.
Wie das Plugin setzt sie dem Konto den Status 🤖 und nimmt ihn am Ende wieder weg (`SIM_STATUS=0` lässt ihn in Ruhe).

## Testfahrer

```bash
AGENT_USER=<agent-konto> agent/tools/drive.sh "Hallo" 20
```

Ausgabe: eine Zeitleiste mit `GESENDET`, `TIPPEN an/aus`, `NACHRICHT` (mit „(still)“), `BEARBEITET`, `GELÖSCHT`.
Weitere Schalter stehen im Kopf von `drive.sh`. Mit `MAKE_GROUP=<Name>` legt der Fahrer einen Gruppenraum
mit dem Agenten an, mit `DELETE_ROOM=<Token>` löscht er ihn wieder.

## Test-Gateway

Ein echtes OpenClaw-Gateway mit dem Plugin, aber mit der Modell-Attrappe statt eines Modells.
Es braucht ein Nextcloud-Konto, das den Agenten spielt (eigenes Testkonto oder Test-Account B).

```bash
cd agent/tools/dev-gateway
./setup.sh https://cloud.example.org <agent-konto> <freigegebenes-konto>   # einmalig; lädt OpenClaw (~650 MB)
NCTALK_APP_PASSWORD=… ./run.sh                                              # Modell-Attrappe + Gateway
./oc.sh channels status --probe                                             # in einem zweiten Terminal
./oc.sh config set channels.nctalk.streaming.mode partial                   # wirkt ohne Neustart
```

Die Modell-Attrappe reagiert auf „antworte kurz“ (kurze Antwort) und „Werkzeug (tool)“ (ruft `exec` mit
`sleep 5` auf); sonst streamt sie eine längere Antwort Wort für Wort.

Wichtig:
- **Ein Konto, ein Gateway.** Läuft dasselbe Konto schon in einem anderen Gateway, antworten beide.
- Das Gateway holt beim Start ungelesene Nachrichten des Kontos nach (`backlogMaxAgeSeconds`, in der
  Vorlage 20 s) und setzt Lesemarken in allen Räumen des Kontos.
- `dev-gateway/home`, `logs` und `node_modules` werden nicht eingecheckt.

## Mess-Skripte

Alle räumen ihre Testnachrichten wieder ab. Umgebung jeweils im Kopf der Datei
(`A_USER/A_PASS`, `B_USER/B_PASS`, `ROOM` bzw. `AGENT_USER/AGENT_PASS`, `OBS_USER/OBS_PASS`).
Die Befunde stehen in `docs/agents.md`.
