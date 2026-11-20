# openclaw-nctalk

OpenClaw-Kanal für Nextcloud Talk, bei dem der Agent ein **echtes Nextcloud-Benutzerkonto** ist
(kein Webhook-Bot). Dadurch kann er, was die Bot-Schnittstelle von Talk nicht hergibt:

- **Live-Status**: eine Statusnachricht, die per Bearbeiten fortgeschrieben wird, während der Agent arbeitet
- **Tipp-Anzeige** über den Signaling-Server (HPB)
- **Dateien und Sprachnachrichten** in beide Richtungen
- **Sofortiger Empfang**: Der Signaling-Server meldet neue Nachrichten ohne spürbare Verzögerung; kein öffentlicher Webhook nötig
- von sich aus schreiben (Heartbeat, Cron, `message`-Werkzeug), auch in ein neues 1:1

Getestet mit OpenClaw 2026.11.3, Nextcloud 34, Talk 24 und externem Signaling-Server.

## Einrichten

1. In Nextcloud ein Konto für den Agenten anlegen und dafür ein App-Passwort erzeugen.
2. Plugin in den Container legen und den Kanal eintragen – auf dem Proxmox-Host:

   ```bash
   deploy/pack.sh /tmp/openclaw-nctalk.tgz                # auf dem Entwicklungsrechner, dann nach /root/agents/ auf den Host kopieren
   NC_BASE_URL=https://cloud.example.org deploy/install-in-container.sh <CT-ID> <agent-konto> <besitzer-konto>
   ```

   Das Skript kopiert das Plugin nach `/opt/openclaw-nctalk`, legt das App-Passwort unter
   `~/.openclaw/credentials/nctalk-app-password` ab, trägt den Kanal ein und startet das Gateway neu.

Von Hand sieht die Konfiguration so aus (`~/.openclaw/openclaw.json`):

```json5
{
  plugins: { load: { paths: ["/opt/openclaw-nctalk"] }, entries: { nctalk: { enabled: true } } },
  channels: {
    nctalk: {
      baseUrl: "https://cloud.example.org",
      user: "agent-konto",
      passwordFile: "/home/openclaw/.openclaw/credentials/nctalk-app-password",
      dmPolicy: "allowlist",
      allowFrom: ["besitzer"],          // Nextcloud-Benutzer-IDs, die den Agenten nutzen dürfen
      streaming: { mode: "progress" },
    },
  },
}
```

## Einstellungen (`channels.nctalk`)

| Schlüssel | Standard | Bedeutung |
|---|---|---|
| `baseUrl`, `user` | – | Nextcloud-Adresse und Konto des Agenten |
| `passwordFile` / `password` | – | App-Passwort; ersatzweise Umgebungsvariable `NCTALK_APP_PASSWORD` |
| `dmPolicy` | `pairing` | `allowlist`, `pairing`, `open` oder `disabled` für 1:1-Chats |
| `allowFrom` | `[]` | freigegebene Benutzer-IDs |
| `groupPolicy`, `groupAllowFrom` | `allowlist`, wie `allowFrom` | wer den Agenten in Gruppen ansprechen darf |
| `rooms.<token>` | – | je Raum: `requireMention` (Standard `true`), `allowFrom`, `systemPrompt`, `skills`, `enabled` |
| `streaming.mode` | `progress` | siehe unten |
| `streaming.progress.*` | – | wie bei den anderen OpenClaw-Kanälen (`label`, `toolProgress`, `maxLines` …) |
| `streaming.ack` | `"👀"` | stille Quittung im Modus `progress`: sofort beim Annehmen, wird zum Live-Status und am Ende gelöscht; `false` schaltet sie ab |
| `streaming.editIntervalMs` | `1500` (`partial`: `2500`) | Mindestabstand zwischen zwei Bearbeitungen; bearbeitet wird nur bei geändertem Text |
| `typing` | `true` | Tipp-Anzeige senden |
| `textChunkLimit` | `8000` | längere Antworten werden geteilt (Talk erlaubt 32000 Zeichen) |
| `mediaMaxMb` | `25` | Obergrenze für Dateien |
| `maxWatchedRooms` | `12` | so viele Räume bekommen eine eigene Signaling-Sitzung |
| `discoveryIntervalSeconds` | `5` | Abgleich der Raumliste (findet neue Räume und Verpasstes) |
| `backlogMaxAgeSeconds` | `86400` | ältere ungelesene Nachrichten werden nach einem Ausfall nicht nachgeholt |

### Live-Status (`streaming.mode`)

- **`progress`** (Standard): Sofort beim Annehmen erscheint still eine Quittung („👀“, ohne Benachrichtigung).
  Sobald der Agent wirklich arbeitet, wird genau diese Nachricht zum Status („Arbeitet … / Exec: running“)
  und fortgeschrieben. Die Antwort kommt als **neue** Nachricht (mit Benachrichtigung), die Statusnachricht
  wird danach gelöscht. Der Talk-Webclient zeigt dafür „Nachricht gelöscht“; Wolkenfunk blendet gelöschte
  stille Nachrichten aus. Mit `streaming.ack: false` entsteht die Statusnachricht erst bei echter Arbeit,
  bei schnellen Antworten also gar nicht.
- **`partial`**: Die Antwort wächst sichtbar mit und ist am Ende die fertige Nachricht – nichts wird gelöscht.
  Die Benachrichtigung kommt mit den ersten Worten, nicht mit der fertigen Antwort.
- **`off`**: nur Tipp-Anzeige, dann die Antwort.

Talk legt für jede Bearbeitung eine unsichtbare Systemnachricht an, die alle Clients mitladen; deshalb wird
nur bei geändertem Text und höchstens alle 1,5 s (`partial`: 2,5 s) bearbeitet.

## Verhalten

- **Ziele** für `message`/Cron: `nctalk:<raumtoken>` oder `nctalk:user:<benutzer>` (1:1 wird bei Bedarf angelegt).
- **Gruppen**: Der Agent reagiert nur auf @-Erwähnung oder eine Antwort auf seine Nachricht; seine Antwort
  zitiert die auslösende Nachricht. Im 1:1 zitiert er nur, wenn er es ausdrücklich will.
- **Lesebestätigung**: Der Lesestand des Agenten rückt vor, sobald eine Nachricht angenommen ist.
- **Ausfälle**: Angenommene Nachrichten liegen in der dauerhaften Eingangswarteschlange von OpenClaw;
  nach einem Neustart werden ungelesene Nachrichten nachgeholt.
- **Ohne Signaling-Server** fällt der Empfang auf Long Polling zurück; die Tipp-Anzeige entfällt dann.
- **Tabellen** werden standardmäßig als Codeblock gesendet (`channels.nctalk.markdown.tables` ändert das).

## Grenzen

- Die Tipp-Anzeige sehen nur Clients, die den Chat gerade offen haben (so funktioniert Talk).
- Ein Gateway bedient genau ein Konto. Dasselbe Konto darf nicht von zwei Gateways gleichzeitig benutzt werden.
- Bearbeitete oder gelöschte Nachrichten der Nutzer lösen keinen neuen Lauf aus.

## Entwicklung

Siehe `../tools/README.md`: abgeschottetes Test-Gateway (eigenes `OPENCLAW_HOME`), Modell-Attrappe,
Testfahrer, Agenten-Attrappe für Client-Tests und die Mess-Skripte. Hintergründe stehen in `docs/agents.md`.

## Lizenz

AGPL-3.0-or-later, wie das übrige Repo (`LICENSE` im Repo-Root).
