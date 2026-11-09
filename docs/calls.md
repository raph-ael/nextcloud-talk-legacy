# Anrufe – Vertrag zwischen Kern und Plattformen

Nur Audio, 1:1 und Gruppen, über das Talk High Performance Backend (HPB,
`nextcloud-spreed-signaling` ≥ 2.1) mit Janus-MCU, ohne MCU als Mesh-Fallback. Video kommt
später: dieselben Peer-Verbindungen bekommen dann zusätzliche Transceiver, die UI hat schon
einen Platz für den Video-Knopf.

**Aufteilung:**
- **Kern** (`core/talk-core/src/call/`): REST, Signaling-Websocket, Anruf-Zustandsautomat,
  alle Wiederholungen und Reconnects, Erkennen eingehender Anrufe (Push, notify_push,
  Raumliste), Klingel-Ende (anderswo angenommen / verpasst).
- **Plattform:** WebRTC (`MediaEngine`), Audio-Routing und -Session, Töne, Systemintegration
  (CallKit, Foreground-Service, Benachrichtigungen, Tray), Oberfläche.

Die Plattform trifft keine Protokollentscheidungen. Sie zeigt `CallView` an und führt
`MediaEngine`-Befehle aus.

## Zustände (`CallState`) und was die Plattform tut

| Zustand | Bedeutung | Text (1:1 / Gruppe) | Ton |
|---|---|---|---|
| `dialing` | ausgehend, Aufbau (Signaling, Beitritt) | „Anrufen …“ | – |
| `ringing` | ausgehend, Gegenseite benachrichtigt, noch niemand da | „Es klingelt …“ / „Warte auf andere …“ | Freizeichen (nur 1:1) |
| `incoming` | eingehend, klingelt hier | „Talk-Anruf“ / „Gruppenanruf“ + `callerName` | Klingelton, Vibration |
| `connecting` | angenommen oder laufendem Anruf beigetreten, Medien im Aufbau | „Verbindung wird hergestellt …“ | – |
| `connected` | Gespräch; `startedAt` gesetzt | Timer m:ss (ab 1 h h:mm:ss) | Verbinden-Ton einmal |
| `reconnecting` | Verbindung verloren, der Kern stellt sie selbst wieder her | „Verbindung wird neu hergestellt …“ (blinkend) | – |
| `ended` | Ende, einmalig mit `endReason`; danach kommt `CallChanged { call: None }` | siehe unten | Auflegen-Ton |

**`endReason`:**
- `hangup` (wir)
- `remoteHangup` „Anruf beendet“
- `noAnswer` „Keine Antwort“
- `declined`
- `answeredElsewhere` (still)
- `missed` „Verpasster Anruf“
- `failed { code, message }`
  - `code` z. B. `permission`, `lobby`, `e2ee-required`, `no-microphone`, `signaling`,
    `media`, `busy`
  - Die Plattform zeigt `message` kurz an.

Nach `ended` zeigt die Plattform den Endzustand ca. 1,5 s und schließt dann die
Anrufansicht. Neue Anrufe sind sofort möglich.

**Teilnehmer** (`participants`, ohne uns selbst):
- `sessionId` ist der stabile Schlüssel für Kacheln.
- `name`, `userId`: Avatar über `userAvatar(userId, dark)`; bei 1:1 reicht der
  Raum-Avatar.
- `muted`, `speaking` (Rahmen um die Kachel).
- `connected`: Ton dieses Teilnehmers kommt an. Ist es `false`, die Kachel abgeblendet
  mit „Verbinde …“ zeigen.

**1:1-Logik wie bei Signal:**
- Die Gegenseite tritt bei → `connected`.
- Die Gegenseite verlässt den Anruf → `ended(remoteHangup)`.
- Niemand nimmt binnen 60 s ab → `ended(noAnswer)`.

**Gruppe:** Der Anruf läuft, solange wir drin sind. Allein im Anruf heißt `ringing` bzw.
„Warte auf andere …“.

**Ablehnen:** Talk kennt kein Ablehnen. `declineCall` beendet nur hier das Klingeln. Der
Anrufer sieht nach 60 s „Keine Antwort“.

**Ein Anruf gleichzeitig:**
- Ein zweiter eingehender Anruf während eines Anrufs klingelt nicht. Der Kern meldet ihn
  später als verpassten Anruf.
- `startCall` während eines Anrufs schlägt fehl (`Call { code: "busy" }`).

## Events

`CallChanged { call: CallView | None }` kommt bei jeder Änderung: Zustand, Teilnehmer,
Stumm, Sprechen (gedrosselt auf höchstens ca. 4/s). `currentCall()` liefert den aktuellen
Stand, z. B. nach einem Neustart der Activity bzw. des Fensters.

`CallMissed { token, roomName, callerName, isGroup }` kommt, wenn ein eingehender Anruf
verpasst wurde: nicht angenommen (Ende mit `missed`) oder während eines anderen Anrufs
eingegangen (der klingelt dann nicht). Dafür eine Benachrichtigung „Verpasster Anruf“ zeigen.

Klingeln funktioniert auch ohne `setMediaEngine`. Annehmen ohne MediaEngine endet mit
`failed{no-media-engine}`. `createPeer` kommt nie vor `acceptCall` bzw. `startCall`, und
`stopAudio` kommt bei jedem Ende nach `startAudio`, immer vor `CallChanged { None }`.

**Logs:**
- Android/iOS: `initLogging(sink, filter)` mit einer eigenen `LogSink`-Implementierung
  (Android → `android.util.Log`, iOS → `os.Logger`).
- Desktop: `initLogging(filter)` bzw. die Umgebungsvariable `TALK_LOG`. Die Logs gehen auf
  stderr.
- Filter wie `RUST_LOG`, z. B. `talk_core=info,talk_core::call=debug`.

## Methoden der Session

| Methode | Wirkung |
|---|---|
| `setMediaEngine(engine)` | einmal nach dem Erzeugen der Session |
| `startCall(token, silent, callId?)` | anrufen bzw. laufendem Anruf beitreten; `silent=true`: niemand wird angeklingelt (nur Tests) |
| `acceptCall(token)` | eingehenden Anruf annehmen |
| `declineCall(token)` | Klingeln hier beenden |
| `hangUp()` | auflegen / Wählen abbrechen |
| `hangUpAndWait(timeoutMs)` | auflegen und warten, bis der Server es weiß (vor dem Beenden der App; `shutdown` legt nur best effort auf) |
| `setCallMuted(bool)` | stumm |
| `currentCall()` | aktueller `CallView` oder nichts |
| `userAvatar(userId, dark)` | Avatar für Gruppen-Kacheln |
| `networkChanged()` | gibt es schon; bei jedem Netzwechsel aufrufen (sofortiger Reconnect, ICE-Restart) |

## MediaEngine (Plattform implementiert, Kern ruft auf)

```
createPeer(peerId, kind, iceServers)        kind: publisher | subscriber | mesh
createOffer(peerId, iceRestart)
createAnswer(peerId)
setRemoteDescription(peerId, sdpType, sdp)  sdpType: offer | answer
addRemoteCandidate(peerId, candidate)       { candidate, sdpMid, sdpMLineIndex }
closePeer(peerId)
setMicrophoneEnabled(enabled)
startAudio(callId)
stopAudio(callId)
```

**Regeln:**
1. Alle Befehle sind nicht blockierend und kommen auf einem Hintergrund-Thread an. Pro
   `peerId` streng der Reihe nach ausführen (serielle Queue pro Peer). Zwischen Peers ist
   die Reihenfolge egal.
2. Eine `peerId` wird nie wiederverwendet. Befehle für unbekannte oder geschlossene IDs
   ignorieren und für sie nie etwas melden.
3. **`kind`:**
   - `publisher`: nur Senden (`sendonly`), eigenes Mikrofon.
   - `subscriber`: nur Empfangen (`recvonly`), ein Teilnehmer vom MCU.
   - `mesh`: Senden und Empfangen zu einem Teilnehmer (ohne MCU).
   - Nur Audio, Opus. Kein Video-Transceiver, solange es Video noch nicht gibt.
4. `iceServers` direkt in die `RTCConfiguration` übernehmen (`urls`, `username`,
   `credential`). Bundle-Policy `max-bundle`, RTCP-Mux `require`, Continual Gathering an
   (Android/iOS), damit der Kern bei Netzwechseln nachziehen kann.
5. `createOffer`/`createAnswer` setzen die Beschreibung auch lokal
   (`setLocalDescription`) und melden sie dann mit `mediaLocalDescription`.
6. Jeden lokalen ICE-Kandidaten mit `mediaLocalCandidate` melden. Das Ende der Kandidaten
   (`null`) nicht melden.
7. Zustandsänderungen mit `mediaPeerState(peerId, state)` melden:
   - Zustände: `new`, `connecting`, `connected`, `disconnected`, `failed`, `closed`.
   - Quelle ist der `connectionState` der PeerConnection, sonst der ICE-Zustand.
   - `failed` muss auch dann kommen, wenn nur einer der beiden es meldet.
8. Fehler einer Operation (SDP ungültig usw.) mit `mediaPeerError(peerId, message)`
   melden. Der Kern entscheidet, ob neu aufgebaut wird.
9. Pegel ca. alle 300 ms mit `mediaAudioLevel(peerId, 0…1)` melden: je Subscriber bzw.
   Mesh-Peer der empfangene Pegel, für den Publisher der eigene Mikrofonpegel.
10. **Audio:**
    - Vor `startAudio(callId)` darf kein Mikrofon laufen und kein Anruf-Audio aktiv sein.
      Es kommt beim Wählen (ausgehend) bzw. beim Annehmen.
    - `stopAudio(callId)` gibt Mikrofon, Audiofokus und Routing frei. Danach den vorherigen
      Audio-Zustand wiederherstellen.
    - **iOS:** `useManualAudio = true`. Audio wirklich erst aktivieren, wenn
      `startAudio` gekommen ist **und** CallKit `didActivate` gemeldet hat.
11. `setMicrophoneEnabled(false)` schaltet nur den Track ab, ohne Neuverhandlung.
    Stummschaltung bleibt über Reconnects erhalten, der Kern ruft es nach jedem
    `createPeer(publisher)` erneut auf.
12. Sprachnachrichten-Wiedergabe und -Aufnahme bei `startAudio` beenden.

**Desktop (napi):**
- Der Kern läuft im Main-Prozess, die `MediaEngine` im Renderer.
- Befehle kommen als JSON über den Callback von `setMediaEngine((command) => …)`:
  `{ cmd: "createPeer", peerId, kind, iceServers }`, `{ cmd: "createOffer", peerId,
  iceRestart }`, `{ cmd: "setRemoteDescription", peerId, sdpType, sdp }`,
  `{ cmd: "addRemoteCandidate", peerId, candidate: { candidate, sdpMid, sdpMLineIndex } }`,
  `{ cmd: "closePeer", peerId }`, `{ cmd: "setMicrophoneEnabled", enabled }`,
  `{ cmd: "startAudio", callId }`, `{ cmd: "stopAudio", callId }`.
- Main leitet sie per IPC in derselben Reihenfolge an den Renderer weiter.
- Meldungen gehen über `mediaReport(json)` zurück: `{ type: "localDescription", peerId,
  sdpType, sdp }`, `{ type: "localCandidate", peerId, candidate }`, `{ type: "peerState",
  peerId, state }`, `{ type: "peerError", peerId, message }`, `{ type: "audioLevel",
  peerId, level }`.

## Ablauf (zur Orientierung, macht alles der Kern)

**Ausgehend mit MCU:**
1. Signaling-Settings holen (frische TURN-Daten).
2. Websocket `hello`.
3. `POST room/{t}/participants/active`.
4. WS `room`, auf Bestätigung warten.
5. `POST call/{t}` (flags 3) → `ringing`.
6. `createPeer(publisher)` → `createOffer` → Offer an die eigene Session → Answer vom MCU
   → `setRemoteDescription`.
7. Für jede Session mit Ton im Anruf: `requestoffer` → Offer vom MCU →
   `createPeer(subscriber)` + `setRemoteDescription(offer)` + `createAnswer` → Answer an den
   MCU.

**Eingehend:**
1. Push, notify_push oder Raumliste melden den Anruf → `incoming`.
2. Der Kern fragt alle 5 s `call/{t}/notification-state`. Wurde anderswo angenommen oder
   beendet, kommt `ended`.
3. `acceptCall` → `connecting` → wie ausgehend ab Schritt 1, aber ohne Klingeln.

**Stabilität (alles im Kern):**
- Websocket: Resume binnen 30 s mit bestehenden Peers. Danach Neuaufbau (Raum, Anruf, alle
  Peers).
- Publisher: kommt binnen 10 s keine Answer → neu. ICE `failed` → ICE-Restart, nach 5 s
  ohne Erfolg kompletter Neuaufbau.
- Subscriber: `requestoffer` alle 8 s wiederholen. Bei `failed` → neue Subscription.

## Plattform-Integration

**Android** (selbst verwaltet wie Signal, ohne Telecom):
- `CallService` als Foreground-Service mit Typ `phoneCall|microphone`.
- Laufender Anruf: `CallStyle.forOngoingCall`.
- Eingehend: Kanal HIGH mit Full-Screen-Intent und `CallStyle.forIncomingCall`.
- Audio: `MODE_IN_COMMUNICATION`, `setCommunicationDevice`, Proximity- und WLAN-Lock.

**iOS:**
- **Ausgehend:** `CXStartCallAction` → im `perform` `startCall`. Bei `ringing`
  `reportOutgoingCall(startedConnectingAt:)`, bei `connected` `connectedAt`.
- **Eingehend:** bei `incoming` `reportNewIncomingCall`, falls PushKit das nicht schon getan
  hat. Bei `CXAnswerCallAction` `acceptCall`.
- **Ende:** bei `ended` `reportCall(endedAt:reason:)`. Gründe: `remoteHangup` →
  `.remoteEnded`, `noAnswer`/`missed` → `.unanswered`, `answeredElsewhere` →
  `.answeredElsewhere`, `failed` → `.failed`.

**Desktop:** Anrufansicht im Hauptfenster, eingehende Anrufe als Leiste mit Klingelton,
Tray bzw. Menüleiste damit es klingeln kann.

## Tests

- **Bot-Gegenstelle:** `talk-cli call …` (Feature `call-bot`) ruft an, nimmt an, sendet
  einen Ton und prüft, ob Ton ankommt.
- **Testräume:**
  - **1:1 Bot A ↔ Bot B:** `CALL_TEST_ROOM_TOKEN` in `.env`. Klingeln ist erlaubt.
  - **Gruppe** im Testraum (`TEST_ROOM_TOKEN`) **nur mit `silent=true`**.
- Nach jedem Test muss `GET call/{token}` leer sein.
