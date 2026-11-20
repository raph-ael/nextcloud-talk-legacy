# KI-Agenten in Talk

So sind KI-Agenten (OpenClaw) an Nextcloud Talk angebunden, so verhalten sie sich im Chat, und
das müssen die Clients dafür können. Code: `agent/openclaw-nctalk` (Plugin), `agent/tools` (Testwerkzeuge).

Stand: Nextcloud 34, Talk 24, externer Signaling-Server (HPB), OpenClaw 2026.11.3.

## Grundidee

- Jeder Agent ist ein **eigenes OpenClaw-Gateway** und in Talk ein **echtes Benutzerkonto** mit App-Passwort.
  Er ist damit ein normaler Teilnehmer: 1:1-Chats, Gruppen, Dateien, Lesemarke.
- Das Gateway baut nur **ausgehende** Verbindungen auf (Nextcloud und Signaling-Server). Es braucht keine
  öffentliche Adresse und keinen Webhook.
- Die Verbindung macht das OpenClaw-Kanal-Plugin `nctalk` (`agent/openclaw-nctalk`). Die Clients brauchen
  kein eigenes Protokoll: Alles, was der Agent tut, sind gewöhnliche Talk-Nachrichten.

### Warum kein Webhook-Bot

Die Bot-Schnittstelle von Talk (und das offizielle OpenClaw-Plugin `@openclaw/nextcloud-talk`, das sie nutzt)
kann Nachrichten weder bearbeiten noch löschen, keine Tipp-Anzeige senden und keine Dateien hochladen.
Webhooks haben 5 s Zeitlimit ohne Wiederholung und brauchen einen vom Server erreichbaren Endpunkt.
Ein Live-Status wie bei Telegram ist damit nicht machbar.

### Warum keine eigene Chat-Oberfläche am Gateway

Eine eigene Agenten-Ansicht direkt am Gateway bräuchte Zugang jedes Clients zu jedem Gateway (öffentlich
oder per VPN), ein zweites Anmelde- und Push-System und eine zweite Chat-Implementierung je Plattform.
Über Talk gibt es das alles schon. Der Gewinn wäre echtes Token-Streaming; die bearbeitete Statusnachricht
kommt dem nahe genug.

## Was im Chat passiert

Ablauf im Standardmodus `progress`:

| Zeit nach dem Absenden | Sichtbar im Chat |
|---|---|
| unter 1 s | Lesemarke rückt vor (der Agent hat die Nachricht angenommen) |
| unter 1 s | Tipp-Anzeige des Agenten (nur für Clients, die sie darstellen) |
| nach gut 1 s | **stille** Nachricht „👀“ (Quittung, keine Benachrichtigung) |
| bei echter Arbeit | dieselbe Nachricht wird zum Status („Arbeitet … / 🛠️ Exec: …“) und per Bearbeiten fortgeschrieben |
| am Ende | Antwort als **neue, normale** Nachricht (löst die Benachrichtigung aus), danach wird die Statusnachricht gelöscht |

Weitere Modi (`channels.nctalk.streaming.mode`):

- `partial`: Die Antwort wird normal gesendet, wächst per Bearbeiten mit und ist am Ende die fertige Nachricht.
  Nichts wird gelöscht. Die Benachrichtigung kommt mit den ersten Worten (ab 40 Zeichen).
- `off`: nur Tipp-Anzeige, dann die Antwort.

Einzelheiten und alle Einstellungen: `agent/openclaw-nctalk/README.md`.

### Regeln für Clients

Der Agent ist am Verhalten erkennbar, nicht an seinem Namen. Diese Regeln gelten deshalb für **jede** stille
Nachricht eines anderen Teilnehmers und brauchen keine Liste von Agenten-Konten:

1. **Stille Nachricht eines anderen** (`silent: true`): dezent darstellen (kleiner, gedämpfte Farbe), nie mit
   „bearbeitet“ markieren. Ein einzelnes Emoji wie 👀 bleibt klein.
2. **Gelöschte stille Nachricht eines anderen** (`messageType == "comment_deleted"` und `silent == true`; beide
   Felder bleiben nach dem Löschen erhalten): gar nicht anzeigen, auch live beim Löschen. Andere gelöschte
   Nachrichten zeigen weiter den Hinweis.
3. **Bearbeitungen im Abstand von 1,5–2,5 s**: dieselbe Blase an Ort und Stelle aktualisieren, kein Neuaufbau,
   kein Scroll-Sprung; eine wachsende Nachricht am unteren Rand scrollt mit.
4. Normale Nachrichten, auch die mitwachsende Antwort in `partial`, bleiben unverändert und tragen „bearbeitet“.

In Wolkenfunk sind 1 bis 3 umgesetzt (`docs/chat.md` „Stille Nachrichten“). Der Talk-Webclient zeigt gelöschte Statusnachrichten als
„Nachricht vom Autor gelöscht“.

**Tipp-Anzeige:** Der Agent sendet sie, Wolkenfunk stellt sie bisher nicht dar (siehe „Offene Punkte“).

## Wie das Plugin arbeitet

### Empfang

- Für jeden Raum des Kontos (höchstens `maxWatchedRooms`, die zuletzt aktiven zuerst) hält das Plugin eine
  Sitzung am Signaling-Server: `POST /api/v4/room/{token}/participants/active`, WebSocket `…/spreed`,
  `hello` Version 2.0 mit den `helloAuthParams` aus `GET /api/v3/signaling/settings`, dann `room`.
- Der Signaling-Server meldet jede neue, bearbeitete, gelöschte Nachricht und jede Reaktion mit
  `{"type":"event","event":{"target":"room","type":"message","message":{"data":{"type":"chat","chat":{"refresh":true}}}}}`,
  ohne spürbare Verzögerung. Das geht **nur** an Sitzungen, die den Raum betreten haben; eine bloß
  verbundene Sitzung bekommt nichts.
- Auf das Signal folgt `GET /api/v1/chat/{token}?lookIntoFuture=1&timeout=0&lastKnownMessageId=…`.
- Zusätzlich gleicht das Plugin alle 5 s die Raumliste ab (`modifiedSince`). Das findet neue Räume und alles,
  was während eines Verbindungsabbruchs kam.
- Ohne Signaling-Server fällt der Empfang auf Long Polling je Raum zurück (`timeout=30`).
- Angenommene Nachrichten gehen in die dauerhafte Eingangswarteschlange von OpenClaw (Schlüssel: Nachrichten-ID).
  Der Lesestand je Raum wird gespeichert; nach einem Neustart werden ungelesene Nachrichten nachgeholt
  (bis `backlogMaxAgeSeconds` alt).

### Tipp-Anzeige

`{"type":"message","message":{"recipient":{"type":"session","sessionid":"<Ziel>"},"data":{"type":"startedTyping","to":"<Ziel>"}}}`
an jede andere Sitzung im Raum, alle 8 s erneut, am Ende `stoppedTyping`. Wer den Raum später betritt, bekommt
`startedTyping` sofort beim `join`-Ereignis. Beim Empfänger steht der Absender in `message.sender` (`sessionid`, `userid`).
Der Talk-Webclient lässt die Anzeige ohne Auffrischung nach etwa 15 s verfallen.

### Statusnachricht

- Senden mit `silent: true`, fortschreiben mit `PUT /api/v1/chat/{token}/{id}`, entfernen mit `DELETE`.
- Jede Bearbeitung legt eine unsichtbare Systemnachricht `message_edited` an, die alle Clients mitladen.
  Deshalb wird nur bei geändertem Text und höchstens alle 1,5 s bearbeitet (`partial`: 2,5 s,
  `streaming.editIntervalMs`).
- Bricht das Gateway mitten im Lauf ab, bleibt die Statusnachricht still im Verlauf stehen.

### Dateien

- Eingehend: `messageParameters.file.path` ist der Pfad im Dateibereich des Agenten-Kontos; geladen wird über WebDAV.
  Sprachnachrichten kommen als `messageType: "voice-message"` mit einer Audiodatei.
- Ausgehend: Hochladen nach `<Anhang-Ordner>/Agent/<Zeitstempel>/…`, dann Freigabe in den Raum
  (`shareType: 10`, Bildunterschrift und Typ in `talkMetaData`). Die Freigabe-API nennt die Nachrichten-ID nicht;
  das Plugin schlägt sie über die `referenceId` im Verlauf nach.

### Zugriff

`dmPolicy`/`allowFrom` für 1:1, `groupPolicy`/`groupAllowFrom`/`rooms` für Gruppen. In Gruppen reagiert der Agent
nur auf eine @-Erwähnung (`messageParameters["mention-…"]` mit seiner Benutzer-ID) oder eine Antwort auf seine
Nachricht. Nicht freigegebene Absender bekommen keine Antwort.

## Befunde

Geprüft zwischen zwei Testkonten bzw. Test-Gateway und Testkonto; Skripte in `agent/tools`. Laufzeiten eines
bestimmten Servers stehen hier nicht.

| Prüfung | Ergebnis |
|---|---|
| Tipp-Anzeige ohne Browser (nur App-Passwort) | kommt an |
| Signal „chat refresh“ nach Senden/Bearbeiten/Reagieren/Löschen | kommt für alle vier |
| Senden und Bearbeiten | beim Empfänger per Long Polling in unter 1 s |
| 1:1 vom Agenten angelegt | Die Gegenseite wird erst mit der ersten Nachricht Teilnehmer (vorher 404) |
| Kurze Antwort (Modell-Attrappe) | unter 2 s nach dem Absenden |
| Nachricht bei gestopptem Gateway | wird nach dem Start beantwortet |

### Benachrichtigungen

Empfänger A, Absender B, 1:1, Benachrichtigungsstufe „immer“:

| Zustand von A | Neue Nachricht | Reaktion auf A's Nachricht |
|---|---|---|
| keine Sitzung im Raum | Benachrichtigung | Benachrichtigung |
| aktive Sitzung im Raum (`participants/active` + Signaling) | **keine** | Benachrichtigung |
| 2 s nach Verlassen der Sitzung | keine | nicht gemessen |
| 45 s nach Verlassen | Benachrichtigung | nicht gemessen |
| stille Nachricht (`silent`) | keine | – |
| Stufe „nur Erwähnungen“ oder „nie“ | nicht gemessen | keine |

Folgen:

- **Eine aktive Sitzung im Raum unterdrückt die Chat-Benachrichtigungen des ganzen Kontos**, auf allen Geräten,
  und das wirkt nach dem Verlassen noch eine Weile nach. Ein Client, der für die Tipp-Anzeige geöffnete Chats
  betritt, braucht eine Regel, wann er die Sitzung hält (etwa nur bei sichtbarem Chat).
- **Eine Reaktion taugt nicht als stille Quittung**: Sie benachrichtigt den Autor, auch bei offenem Chat, und die
  Benachrichtigung bleibt nach dem Entfernen der Reaktion stehen. Deshalb sendet der Agent ein stilles „👀“ als
  Nachricht.

## Betrieb

- Ein Gateway bedient genau ein Konto; dasselbe Konto darf nicht in zwei Gateways laufen.
- Empfohlen: jedes Gateway in einem eigenen Container in einem Netz, das nur ins Internet und zur Nextcloud
  darf, nicht zu anderen Diensten und nicht zu anderen Agenten. Ein Agent mit Shell-Zugriff ist sonst ein
  Sprungbrett.
- Einrichten eines Containers: `agent/openclaw-nctalk/deploy/install-in-container.sh` (Proxmox/LXC).
- Mehrere Kanäle (z. B. Telegram und Talk) teilen sich bei OpenClaw standardmäßig denselben Gesprächsverlauf.

## Entwickeln und testen

Siehe `agent/tools/README.md`: Agenten-Attrappe für Client-Tests, Testfahrer, Test-Gateway mit Modell-Attrappe.

Geprüft mit dem Test-Gateway: Empfang, Tippen, Statusnachricht, `partial`, stille Quittung, Datei in beide
Richtungen, Neustart, Gruppe mit und ohne Erwähnung, nicht freigegebener Absender, zweite Nachricht im
laufenden Auftrag. **Nicht** geprüft: Sprachnachrichten und Bilder (gleicher Weg wie Dateien), der Fehlerfall
(die Statusnachricht sollte nach der Fehlermeldung gelöscht werden; abgeleitet aus dem OpenClaw-Standard).

## Offene Punkte

- **Tipp-Anzeige in Wolkenfunk.** Der Kern hat eine Signaling-Sitzung bisher nur im Anruf. Für die Anzeige
  müsste er geöffnete Chats betreten; das unterdrückt Benachrichtigungen (siehe oben) und braucht deshalb eine
  Regel für das Halten der Sitzung. Vorerst zurückgestellt; die Quittung „👀“ und die Statusnachricht decken den
  Bedarf.
- **Stopp-Knopf.** Ein laufender Auftrag lässt sich heute nur per Textbefehl abbrechen (`/stop`).
- **Bearbeitete oder gelöschte Nachrichten der Nutzer** lösen keinen neuen Lauf aus.
- **Tabellen** sendet der Agent als Codeblock (`channels.nctalk.markdown.tables`), bis geklärt ist, welche Clients
  Markdown-Tabellen darstellen.
- **Plugin-Schnittstelle von OpenClaw** gilt als experimentell; nach einem OpenClaw-Update das Plugin mit dem
  Test-Gateway prüfen.
