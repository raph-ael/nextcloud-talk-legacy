# Anrufe – Vertrag zwischen Kern und Plattformen

Audio und Video (Kamera), 1:1 und Gruppen, über das Talk High Performance Backend (HPB,
`nextcloud-spreed-signaling` ≥ 2.1) mit Janus-MCU, ohne MCU als Mesh-Fallback. Jeder Anruf
startet mit Ton, die Kamera schaltet man im Anruf zu (Abschnitt „Video“).
Bildschirmfreigabe gibt es noch nicht.

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
  - Eingehend (wir haben abgelehnt): still. Bei 1:1 beendet der Kern den Anruf auch
    beim Anrufer und auf unseren anderen Geräten (Talk kennt kein Ablehnen: der Kern
    beendet den Anruf „für alle“, das dürfen in 1:1 beide Seiten). Bei Gruppen hört
    nur hier das Klingeln auf.
  - Ausgehend 1:1 (die Gegenseite hat abgelehnt): „Abgelehnt“, Auflegen-Ton.
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
- `video`, `peerId`: siehe „Video“.
- `connected`: Ton dieses Teilnehmers kommt an. Ist es `false`, die Kachel abgeblendet
  mit „Verbinde …“ zeigen.

**1:1-Logik wie bei Signal:**
- Die Gegenseite tritt bei → `connected`.
- Die Gegenseite verlässt den Anruf → `ended(remoteHangup)`. Legt sie mit einem unserer
  Clients auf, sofort (Signaling-Nachricht `hangup`). Sonst (Web, offizielle Apps, oder
  die Gegenseite baut die Verbindung neu auf) zeigt der Kern bis zu 6 s `reconnecting`.
- Niemand nimmt binnen 60 s ab → `ended(noAnswer)`.
- Eingehend klingelt es höchstens 55 s, dann `ended(missed)` (vor dem 60-s-Limit von
  iOS/CallKit).

**Gruppe:** Der Anruf läuft, solange wir drin sind. Allein im Anruf heißt `ringing` bzw.
„Warte auf andere …“.

**Ablehnen:** siehe `declined` oben: Bei 1:1 endet der Anruf auch beim Anrufer
(„Abgelehnt“), bei Gruppen hört nur hier das Klingeln auf.

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
| `setCameraEnabled(bool)` | Kamera an/aus (Abschnitt „Video“) |
| `currentCall()` | aktueller `CallView` oder nichts |
| `userAvatar(userId, dark)` | Avatar für Gruppen-Kacheln |
| `networkChanged()` | gibt es schon; bei jedem Netzwechsel aufrufen (sofortiger Reconnect, ICE-Restart) |
| `startRecording(video)` | Aufnahme starten (nur bei `canRecord`), Fehler mit lesbarer `message` |
| `stopRecording()` | Aufnahme beenden |
| `consentToRecording()` | Antwort auf `recordingConsentPending`: Zustimmung, der Kern tritt dann bei |
| `recordingNotices()` | wartende Hinweise „Aufnahme verfügbar“ (Änderungen: `RecordingsChanged`) |
| `shareRecording(id)` | „In Chat teilen“ |
| `dismissRecording(id)` | „Verwerfen“ (die Datei bleibt in den Dateien) |

## Aufnahme

Die Aufnahme macht der Aufnahme-Server von Nextcloud, nicht die App. Er tritt dem Anruf
unsichtbar bei. Der Kern zeigt nur den Stand an und startet bzw. stoppt die Aufnahme.

**`CallView`:**

| Feld | Bedeutung |
|---|---|
| `recording` | `off`, `startingAudio`, `startingVideo`, `audio`, `video`, `failed`. Kommt live vom Server, auch wenn jemand anderes (z. B. im Web) die Aufnahme startet. |
| `canRecord` | Wir dürfen aufnehmen: Aufnahme-Server vorhanden, wir sind Moderator (eingeloggt) und im Anruf. Nur dann die Bedienelemente zeigen. |
| `recordingConsentPending` | Der Server verlangt eine Zustimmung, bevor wir dem Anruf beitreten. Der Kern wartet im Zustand `connecting` bis zu 2 min. Dialog zeigen, dann `consentToRecording()` oder `hangUp()`. Ohne Antwort endet der Anruf mit `failed{recording-consent}`. |

**Oberfläche (alle Plattformen gleich):**
- **Anzeige für alle** bei `recording` ≠ `off`:
  - Oben im Anrufbildschirm ein roter Punkt mit „Aufnahme läuft“ (der Punkt pulsiert sanft).
  - `starting*`: „Aufnahme startet …“.
  - `failed`: nur für `canRecord` „Aufnahme fehlgeschlagen“.
  - Die Anzeige erscheint auch in der Leiste „Zurück zum Anruf“ (● REC) und in der System-Anrufanzeige, wo möglich.
- **Ansage**, sobald `recording` zu `audio`/`video` wird (auch beim Beitritt in eine laufende Aufnahme):
  - Toast „Dieser Anruf wird aufgezeichnet“
  - ein leiser selbst erzeugter Hinweiston
  - TalkBack/VoiceOver-Ansage
- **Bedienung nur bei `canRecord`:**
  - Knopf „Mehr“ (⋯) in der Steuerleiste; der Video-Platz bleibt frei.
  - Menü „Audio aufnehmen“ / „Video aufnehmen“. Läuft eine Aufnahme: „Aufnahme beenden“ mit Rückfrage „Aufnahme beenden?“.
  - Fehler als Toast.
  - Video zeichnet die Talk-Weboberfläche auf (Avatare, Bildschirmfreigaben aus dem Web).
- **Zustimmung** (`recordingConsentPending`): Dialog „Dieser Anruf kann aufgezeichnet werden“ mit [Abbrechen] → `hangUp()` und [Zustimmen und beitreten] → `consentToRecording()`. Er erscheint auch nach „Annehmen“ (iOS: nach dem Annehmen über CallKit in der App).

**Nach dem Anruf:**
- Wer die Aufnahme gestartet hat, bekommt sie in seinen Dateien und einen Hinweis.
- `RecordingsChanged { items }` liefert immer die ganze Liste, `recordingNotices()` den aktuellen Stand.
  - Das erste Event nach dem Start ist der Ausgangsstand und kommt immer, auch leer. Dafür keine Systembenachrichtigung zeigen (der Banner aber schon).
  - Ids, die später dazukommen, sind neu: dann die Systembenachrichtigung.
- `RecordingNotice`: `{ id, token, roomName, kind, fileName, timestamp, canShare }`
  - `kind`: `recording` | `transcript` | `summary` | `recordingFailed` | `transcriptFailed` | `summaryFailed`
- Banner in der Gesprächsliste und im Chat des Gesprächs:
  - „Aufnahme verfügbar · ‹Datei›“ (bzw. Transkript, Zusammenfassung) mit [In Chat teilen] (nur bei `canShare`) und [Verwerfen].
  - Fehler mit [OK] → `dismissRecording`.
- Läuft die App, zusätzlich eine leise Systembenachrichtigung.
- Der Kern holt die Hinweise beim Start, bei jeder notify_push-Benachrichtigung und nach einer eigenen Aufnahme 15 min lang alle 30 s.

## Video

**Grundprinzip:** Jede Peer-Verbindung hat von Anfang an einen Audio- **und** einen
Video-Transceiver. Die Kamera schaltet man, indem man ihren Track an den Video-Sender hängt
(`replaceTrack`) bzw. abnimmt. Es gibt keine Neuverhandlung, also kein SDP und keine
MCU-Runde. Kamera an ist dadurch sofort da, und Reconnects ändern nichts.

**Kern ↔ Server:**
- Wir treten immer mit Flags 7 bei (im Anruf | Audio | Video), wie der Web-Client mit
  Kamera. Die anderen abonnieren uns dann mit Video.
- Ob die Kamera gerade an ist, sagt Signaling `unmute`/`mute` mit `payload.name: "video"`.
  Es geht an alle und an jeden Neuen. Das Web zeigt daraufhin Video bzw. Avatar.
- Abonnements fragen beim MCU per `selectStream` die passende Simulcast-Stufe an, denn
  Web-Clients senden drei: bis 2 Videos die volle, bis 6 die mittlere, sonst die kleinste.
- Offers von `roomType: "screen"` (Bildschirmfreigabe) werden ignoriert.

**`CallView`:**
- `cameraOn`: unsere Kamera ist an.

**`CallParticipant`:**
- `video`: Die Kamera der Person ist an. Das gilt bei Flag 4, solange sie kein
  `mute video` geschickt hat.
- `peerId`: die Verbindung (MediaEngine-Peer), die ihren Ton und ihr Video trägt.
  Video-Tracks dieses Peers gehören auf ihre Kachel.

**Plattform:**
- `setCameraEnabled(true)` (MediaEngine) öffnet die Kamera und hängt den Track an den
  Video-Sender jedes Publisher- und Mesh-Peers. `false` nimmt ihn ab und stoppt die Kamera,
  die Kamera-LED geht aus.
- Der Kern ruft es nach jedem neuen Publisher- bzw. Mesh-Peer erneut auf. Ist die Kamera
  schon offen, nur anhängen.
- Fehler: Gibt es keine Kamera, ist sie verweigert, abgezogen oder von einer anderen App
  belegt, meldet die Plattform `mediaCameraState(false)`. Der Kern setzt dann `cameraOn`
  zurück und sagt es den anderen. Den Grund zeigt die Plattform selbst als Hinweis.
- `stopAudio` gibt auch die Kamera frei.
- Video der anderen anzeigen, solange `video` gesetzt ist **und** Bilder ankommen. Sonst den
  Avatar zeigen, z. B. wenn der Track `muted` ist oder seit über 2 s keine Frames kamen.

**Qualität:**
- VP8 (`videocodec` in der Publisher-Offer).
- Desktop 1280×720@30, höchstens 1,5 Mbit/s.
- Mobil 640×480@24, höchstens 600 kbit/s, schwache Geräte 480×360.
- `degradationPreference: balanced`.

**Oberfläche (alle Plattformen gleich):**
- **Steuerleiste:** Ausgabe · **Kamera** · Mehr · Stumm · Auflegen.
  - Der Kamera-Knopf ist durchgestrichen, solange die Kamera aus ist.
  - Bezeichnung „Kamera einschalten“ / „Kamera ausschalten“.
  - Desktop-Kürzel Strg/⌘+Shift+V.
- **Jeder Anruf startet ohne Kamera**, auch beim Annehmen. Es gibt keinen eigenen
  „Videoanruf“.
- **1:1:** Das Video der Gegenseite füllt die Bühne, ohne Video der Avatar wie bisher.
- **Gruppe:** In der Kachel das Video statt des Avatars, Sprech-Rahmen und Stumm-Symbol
  bleiben.
- **Eigene Vorschau:** Eine kleine Kachel unten rechts, gespiegelt, nur solange die eigene
  Kamera an ist. Am Handy gibt es dort einen Knopf „Kamera wechseln“ (vorn/hinten).
- **Mobil:**
  - Kamera an → ohne Headset auf den Lautsprecher, kein Näherungssensor, der Bildschirm
    bleibt an.
  - Verlässt man die Anrufansicht (minimiert, andere App), wird die Kamera pausiert. Für
    die anderen ist das „Video aus“.
  - Zurück in der Ansicht läuft sie weiter. Das ist kein Kamera-Foreground-Service, und es
    gibt keinen Konflikt mit der Chat-Kamera.
- **Desktop-Geräte wie bei Meet:**
  - Alle `getUserMedia`-Aufrufe laufen über einen seriellen Gerätemanager.
  - Folgt man dem Systemstandard, schaltet ein neues Headset sofort um. Ein Hinweis zeigt
    „Mikrofon: …“ / „Lautsprecher: …“.
  - Wird ein gewähltes Gerät abgezogen, geht es zurück zum Standard. Kommt es wieder,
    zurück zum gewählten Gerät.
  - Ein beendeter Track (Ruhezustand, USB) wird mit Backoff neu geöffnet. Danach kommt der
    Hinweis mit „Erneut versuchen“.
  - Wird die Kamera abgezogen, geht sie aus. Kommt sie binnen 60 s wieder, geht sie wieder
    an.
  - Das Gerätemenü hat Mikrofon (mit Pegel), Lautsprecher (mit Testton) und Kamera (mit
    Vorschau).
  - Vollbild per Knopf und Doppelklick.

## MediaEngine (Plattform implementiert, Kern ruft auf)

```
createPeer(peerId, kind, iceServers)        kind: publisher | subscriber | mesh
createOffer(peerId, iceRestart)
createAnswer(peerId)
setRemoteDescription(peerId, sdpType, sdp)  sdpType: offer | answer
addRemoteCandidate(peerId, candidate)       { candidate, sdpMid, sdpMLineIndex }
closePeer(peerId)
setMicrophoneEnabled(enabled)
setCameraEnabled(enabled)
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
   - `publisher`: nur Senden (`sendonly`), eigenes Mikrofon und eigene Kamera.
   - `subscriber`: nur Empfangen (`recvonly`), ein Teilnehmer vom MCU.
   - `mesh`: Senden und Empfangen zu einem Teilnehmer (ohne MCU).
   - Immer ein Audio- (Opus) und ein Video-Transceiver (VP8), auch ohne Kamera (siehe
     „Video“).
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
  `{ cmd: "setCameraEnabled", enabled }`, `{ cmd: "startAudio", callId }`,
  `{ cmd: "stopAudio", callId }`.
- Main leitet sie per IPC in derselben Reihenfolge an den Renderer weiter.
- Meldungen gehen über `mediaReport(json)` zurück: `{ type: "localDescription", peerId,
  sdpType, sdp }`, `{ type: "localCandidate", peerId, candidate }`, `{ type: "peerState",
  peerId, state }`, `{ type: "peerError", peerId, message }`, `{ type: "audioLevel",
  peerId, level }`, `{ type: "cameraState", on: false }`.

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
  - Video: `--video` schaltet die Fake-Kamera von Chrome zu (`--video-after N`,
    `--video-off-after N`).
  - `--expect-video` verlangt angekommene Bilder.
  - Das Log zeigt „Video von …: an/aus“ für die Gegenseite.
- **Testräume:**
  - **1:1 Bot A ↔ Bot B:** `CALL_TEST_ROOM_TOKEN` in `.env`. Klingeln ist erlaubt.
  - **Gruppe** im Testraum (`TEST_ROOM_TOKEN`) **nur mit `silent=true`**.
- Nach jedem Test muss `GET call/{token}` leer sein.
