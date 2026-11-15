# Benachrichtigungen

Nachrichten, Erwähnungen, Anrufe und verpasste Anrufe kommen auf allen Plattformen an. Sie verschwinden, wenn sie woanders gelesen wurden, und führen beim Antippen in den richtigen Chat. Dieser Vertrag legt fest, was überall gleich ist. Für mehrere Konten gilt zusätzlich `docs/accounts.md`, für Anrufe `docs/calls.md`.

## Zwei Wege

| Weg | Wer nutzt ihn | Wann |
|---|---|---|
| **Push** (Server → Push-Proxy → FCM bzw. APNs) | Android, iOS | immer, auch bei beendeter App |
| **`messageNotices`** aus dem Kern (Abgleich der Chatliste) | Desktop | immer, solange die App läuft |
| | Android | nur ohne Push (kein Google-Dienst, keine Proxy-Adresse im Build) und nach `onDeletedMessages` |

iOS nutzt `messageNotices` nicht. Dort liefert APNs auch bei geöffneter App.

**Ohne laufende App gibt es am Desktop nichts.** Dafür gibt es „Beim Anmelden starten“.

### Was der Server pusht
Das entscheidet der Server, die App kann es nicht ändern:

- Jede Talk-Benachrichtigung kommt mit hoher Priorität. Ein Anruf ist `voip`, alles andere `alert`, eine Löschung `background`.
- **Kein Push** bei:
  - Status „Nicht stören“, außer der Chat ist als wichtig markiert. Das gilt auch für Anrufe.
  - Stufe „Aus“, stumm gesendeten Nachrichten, Lobby.
  - einer gewöhnlichen Nachricht, wenn der Benutzer mit einer anderen Sitzung gerade in diesem Chat aktiv ist. Erwähnungen und Antworten kommen trotzdem.
  - einem Server mit mehr als 1000 Benutzern ohne Abo. Der schickt an keinen Proxy. „Push testen“ zeigt das.
- **Gelesen an anderer Stelle** kommt als Löschung mit den IDs der Benachrichtigungen.
- **Ein verpasster Anruf hat keinen eigenen Push.** Die gespeicherte Benachrichtigung wird nur umbenannt.
- Der Text ist kurz: der Server kürzt die Nachricht auf 100 Zeichen, und alles zusammen muss in rund 200 Byte passen. Umlaute und Emoji zählen mehrfach.

### Wie lange der Proxy wartet

| Art | Android (FCM) | iOS (APNs) |
|---|---|---|
| Anruf | 60 s | gar nicht |
| Nachricht | 24 h | 24 h |
| Löschung | 24 h | gar nicht |

## Wann eine Nachricht meldet

Für Push gilt der Server (oben). Für `messageNotices` rechnet der Kern, mit demselben Ergebnis:

- Der Chat zählt: nicht archiviert, Stufe „Alle Nachrichten“, oder „Nur Erwähnungen“ mit einer Erwähnung (Tabelle in `docs/settings.md`).
- Es gibt eine neuere Nachricht als die zuletzt gemeldete, sie ist nicht die eigene und nicht stumm gesendet, und der Chat hat Ungelesenes.
- Der eigene Status ist nicht „Nicht stören“, außer der Chat ist als wichtig markiert.

Der Kern merkt sich pro Chat die zuletzt gemeldete Nachricht und speichert das. Folgen:

- Die erste Nachricht eines neuen Chats meldet.
- „Woanders gelesen, dann Neues“ meldet, auch wenn die Zahl der Ungelesenen dabei gesunken ist.
- „Als ungelesen markieren“ meldet nicht.
- Nach einer neuen Anmeldung ist der erste Stand die Ausgangslage, es gibt keine Flut.
- Nach dem Start meldet der Kern, was seit dem letzten Lauf dazugekommen ist, mit `sinceStart: true`.

## Was die Benachrichtigung zeigt

| Teil | Inhalt |
|---|---|
| Titel | Name des Chats, aus der Chatliste (`rooms()`) über den Token |
| Text, Einzelchat | die Nachricht |
| Text, Gruppe | „{Autor}: {Nachricht}“, auf Android der Autor als `Person` im `MessagingStyle` |
| Anhang | die Bezeichnungen der Chatliste: „🎤 Sprachnachricht“, „📷 Foto“, „🎬 Video“, „📎 {Name}“ |
| ohne lesbaren Text | „Neue Nachricht“ |
| Kontozeile | ab zwei Konten „{Name} · {Server}“, wie in `docs/accounts.md`. Android zeichnet `setSubText` im `MessagingStyle` nicht: Dort steht sie zusätzlich im Gesprächstitel, „{Chat} – {Name} · {Server}“ (`setConversationTitle`, mit `setGroupConversation` passend zum Chat). |
| Ton | der Standardton des Systems |

Bei Push kommt der Text aus dem Push selbst, ohne Netz: `pushChatParts(subject, roomName)` liefert Absender und Text. Den Titel bestimmt die App trotzdem aus der Chatliste, denn der Server schreibt bei Gruppen „Bob in Projekt“ in die erste Zeile, in der Sprache des Benutzers. Kennt die App den Chat noch nicht, nimmt sie die erste Zeile.

Reaktionen und Erinnerungen kommen ebenfalls als Nachrichten-Push. Sie haben keinen Zeilenumbruch und stehen dann ganz im Text.

Ein Push, der nicht zu Talk gehört (auch der von „Push testen“), erscheint als einfache Benachrichtigung: Titel „Wolkenfunk“, Text aus dem Push, ohne Chat. Antippen öffnet die App.

## Eine Benachrichtigung pro Chat

- Pro Konto und Chat gibt es höchstens eine Benachrichtigung. Eine neue Nachricht ersetzt bzw. ergänzt sie.
  - **Android:** `notify(tag = "<Konto>|<Token>", id = 0)` mit `MessagingStyle`. Der bisherige Verlauf kommt aus der aktiven Benachrichtigung (`getActiveNotifications`, `extractMessagingStyleFromNotification`). Dazu die Gruppen-Zusammenfassung des Systems (`setGroup` plus Summary).
  - **iOS:** Thread „<Konto>.<Token>“ wie bisher.
  - **Desktop:** Die App hält die `Notification` je Konto und Chat fest, schließt die alte und zeigt die neue. Beim Beenden schließt sie alle.
- **Viele auf einmal (Desktop):** Kommen in einem Ereignis mehr als 3 Chats oder trägt es `sinceStart`, erscheint eine einzige Benachrichtigung: Titel „Wolkenfunk“, Text „Neue Nachrichten in {n} Chats“, bei einem Chat die normale.
  - {n} zählt den Chat nicht mit, der gerade offen und im Vordergrund ist.
  - Ein Klick zeigt das Fenster und wechselt ins Konto der Zusammenfassung. Sie trägt ab zwei Konten die Kontozeile.
  - Sie verschwindet, sobald der letzte ihrer Chats gelesen ist.
- **Mobil** zeigt nie etwas für `sinceStart`. Die App ist dann ohnehin vorn.

## Wann sie verschwindet

- Der Server meldet „gelesen“: Android und iOS entfernen die Benachrichtigungen mit diesen IDs, nur im Konto des Push. Dafür merkt sich jede Benachrichtigung alle ihre IDs.
- `noticesCleared { tokens }` aus dem Kern: Der Chat ist gelesen, archiviert oder weg. Alle Plattformen entfernen seine Benachrichtigung.
  - Es kommt auch für Benachrichtigungen aus einem früheren Lauf: wenn der Chat beim letzten Mal Ungelesenes hatte und jetzt keines mehr, und immer, wenn hier gelesen, geantwortet oder „Als gelesen markieren“ gewählt wurde.
  - Es kann für denselben Chat mehrfach kommen. Das Entfernen muss das vertragen.
- **Android** gleicht zusätzlich bei jedem `roomsChanged` ab: Die Benachrichtigung eines Chats ohne Ungelesenes verschwindet, wenn sie älter als 15 Sekunden ist. Die Frist schützt eine frische Benachrichtigung vor einer Chatliste, die vor ihr abgerufen wurde.
- Der Benutzer öffnet den Chat.
- **iOS** gleicht zusätzlich bei jedem Wechsel in den Vordergrund ab: Benachrichtigungen von Chats ohne Ungelesenes verschwinden, der Zähler wird neu gesetzt. Löschungen kommen dort nicht zuverlässig an.

## Wann sie gar nicht erscheint

Nur wenn genau dieser Chat im aktiven Konto offen und die App im Vordergrund ist. Der Kern meldet auch dann, das Unterdrücken ist Sache der Plattform.

## Antippen

Wechselt ins Konto und öffnet den Chat (`docs/accounts.md`). Während eines Anrufs in einem anderen Konto bleibt es beim Toast „Während eines Anrufs nicht möglich“.

## Aktionen (Android, iOS)

| Aktion | Wirkung |
|---|---|
| „Antworten“ | Der Text geht sofort in den Postausgang (`enqueueText`), auch bei beendeter App. Danach zeigt die Benachrichtigung die eigene Antwort bzw. verschwindet, sobald der Chat als gelesen gilt. Die App muss sie in jedem Fall neu setzen, sonst dreht der Ladekreis weiter. |
| „Als gelesen markieren“ | `markRoomRead(token)`, die Benachrichtigung verschwindet. Der Kern fragt dafür die neueste Nachricht beim Server ab, weil die Chatliste nach einem Kaltstart älter sein kann als die Benachrichtigung. |

Beide gehen an das Konto der Benachrichtigung, ohne es aktiv zu machen.

## Erlaubnis und Einstellungen (Android, iOS)

- Die App fragt beim nächsten Start nach der Erlaubnis, sobald ein Konto angemeldet ist. Das gilt auch für bestehende Installationen, die bisher erst beim Senden gefragt haben. Sie fragt einmal.
- Die Seite „Benachrichtigungen“ in den Einstellungen zeigt den Zustand:
  - Sind sie aus, steht oben „Benachrichtigungen sind ausgeschaltet“ mit dem Hinweis „Neue Nachrichten und Anrufe bemerkst du dann nur bei geöffneter App.“ und dem Knopf „Einschalten“. Der fragt nach bzw. öffnet die Systemeinstellungen.
  - Die Startseite der Einstellungen zeigt an der Zeile „Benachrichtigungen“ dann „Aus“.
- **„Push testen“** ruft `testPush()` im aktiven Konto auf.
  - Erfolg: „Test angefordert. Die Benachrichtigung sollte in wenigen Sekunden erscheinen.“
  - Fehler: „Der Server hat den Test nicht angenommen.“
  - Build ohne Push: Die Zeile heißt „Diese App-Version hat keinen Push“ und tut nichts.

## iOS: Zähler am App-Symbol bei beendeter App

- Die App schreibt bei jeder Änderung die Summe (`badge().messages` über alle Konten) in den geteilten Schlüsselbund.
- Die Notification-Erweiterung liest den Wert, erhöht ihn für jede Nachricht um eins, setzt ihn als Zähler und schreibt ihn zurück.
- Beim nächsten Öffnen überschreibt die App ihn mit dem wahren Wert.

## Desktop

- **Tray-Symbol:** bei Ungelesenem (Summe von `badge().chats` größer als null) die Variante mit Punkt, sonst die normale. Gewechselt wird nur, wenn sich der Zustand ändert.
- **Windows:** dazu ein Zähler am Taskleistensymbol (`setOverlayIcon`): 1 bis 9, darüber „9+“. Ohne Ungelesenes ist er weg.
- Beide Bildsätze erzeugt `design/icon/build-icons.py`.
- **„Beim Anmelden starten“** unter „Allgemein“, standardmäßig aus, Untertitel „Wolkenfunk startet im Hintergrund, damit Nachrichten und Anrufe ankommen“.
  - Windows und macOS: `setLoginItemSettings`. Auf macOS zeigt der Schalter den Stand des Systems: Wer das Anmeldeobjekt in den Systemeinstellungen entfernt, sieht ihn aus.
  - Linux: ein Eintrag in `~/.config/autostart`, beim AppImage mit `$APPIMAGE` als Programm. Im Flatpak fehlt der Schalter.
  - Ein so gestarteter Lauf zeigt kein Fenster, wo das System das zulässt. Ohne Tray-Dienst (GNOME ohne AppIndicator) bleibt er unsichtbar, bis man die App normal startet oder eine Benachrichtigung anklickt, wie bei „Beim Schließen im Hintergrund weiterlaufen“.
  - Auf macOS steht der Schalter als einziger auf der Seite „Allgemein“.
- **Festgehalten** werden auch „Verpasster Anruf“, „Nachricht nicht gesendet“, Aufnahme-Hinweise und die Klingel-Benachrichtigung, damit ihr Klick nicht verloren geht.

## Anrufe

Klingeln und „Verpasster Anruf“ bleiben wie in `docs/calls.md`. Neu:

- **Verpasst, während die App nicht lief:** Nach dem Start und nach jeder Wiederverbindung meldet der Kern Einzelanrufe, die inzwischen verpasst wurden, einmal als `callMissed`. Er merkt sich dazu die höchste gesehene Benachrichtigungs-ID. Beim ersten Lauf meldet er nichts nach, und Anrufe, die älter als 24 Stunden sind, auch nicht.
- **„Nicht stören“:** Der Kern klingelt nicht und meldet nichts als verpasst, außer in wichtigen Chats. So verhält sich auch der Server beim Push. Den Status liest der Kern mit jedem vollständigen Abruf der Chatliste, er wirkt also nach spätestens einer Minute.
- **iOS:** Weist das System einen per Push gemeldeten Anruf ab (Fokus, blockiert), zeigt die App „Verpasster Anruf“.

## Abmelden ohne Netz (Android, iOS)

Kann die App beim Abmelden das App-Passwort nicht zurückgeben, merkt sie es sich im sicheren Speicher und holt es beim nächsten Start nach (`revokeAppPassword`). Sonst pusht der Server weiter an ein Konto, das es auf dem Gerät nicht mehr gibt, und iOS zeigt dafür Platzhalter.

## Kern-Vertrag

Neue Ereignisse, in `talk-node` und `talk-ffi`:

| Ereignis | Inhalt |
|---|---|
| `messageNotices` | `items: [{ token, roomName, isGroup, mention, unread, message }]`, `sinceStart`. `message` ist die Vorschau wie in der Chatliste (`author`, `text`, `isSystem`, `attachment`) oder fehlt bei Chats ohne lesbare Vorschau. |
| `noticesCleared` | `tokens: [...]` |

Neue Aufrufe:

| Aufruf | Rückgabe |
|---|---|
| `session.testPush()` (nur `talk-ffi`) | Fehler, wenn der Server ablehnt |
| `pushChatParts(subject, roomName)` (frei, nur `talk-ffi`) | `{ sender, text }`; `sender` ist leer, wenn der Push keinen nennt |

Geändert:

- Bei jedem vollständigen Abruf der Chatliste liest der Kern den eigenen Status mit („Nicht stören“).
- Meldet notify_push eine neue Benachrichtigung, lädt der Kern die Chatliste sofort nach. `roomsChanged` und `messageNotices` kommen damit nach etwa einer Sekunde statt nach bis zu zehn.
- `callMissed` kommt auch für Anrufe, die bei nicht laufender App verpasst wurden.
- Die eigene Kopie der Regel am Desktop (`notifyNewMessages`, `wantsNotification`) entfällt.

## Barrierefreiheit

| Element | Bezeichnung |
|---|---|
| Tray-Symbol mit Punkt | Tooltip „Wolkenfunk, neue Nachrichten“ |
| Zähler unter Windows | Beschreibung „{n} Chats mit Neuem“, bei einem „1 Chat mit Neuem“ |
| Zeile „Benachrichtigungen“ in den Einstellungen | „Benachrichtigungen, aus“, wenn sie aus sind |

## Test

- **Demo-Server** (`docs/accounts.md`, „Test“), für alles ohne Push:
  ```sh
  curl -d token=mira -d text=Hallo http://127.0.0.1:8098/demo/incoming      # jemand schreibt
  curl -d status=dnd http://127.0.0.1:8098/demo/status                       # „Nicht stören“ an, status=online wieder aus
  curl -d token=mira http://127.0.0.1:8098/demo/missed-call                  # ein verpasster Anruf
  ```
- **Angezeigte Benachrichtigungen mitlesen:**
  - Linux: `dbus-monitor "interface='org.freedesktop.Notifications'"`
  - Android: `adb shell dumpsys notification --noredact`
  - iOS: der Debug-Auszug der zugestellten Benachrichtigungen (`LogDump`)
- **Echter Push** nur in den Testräumen bzw. im Bot-1:1, unter `bot-lock.sh`.
- **Zu prüfen, mobil:**
  - Zustände: App offen in einem anderen Chat, im Hintergrund, beendet, Bildschirm aus, fünf Minuten offline und dann wieder online.
  - Erwähnung in einer Gruppe auf „Nur Erwähnungen“ meldet, eine gewöhnliche Nachricht dort nicht. Ein Chat auf „Aus“ bleibt still.
  - Zwei Nachrichten im selben Chat ergeben eine Benachrichtigung mit beiden.
  - Woanders gelesen räumt ab.
  - Antippen wechselt Konto und Chat.
  - „Antworten“ und „Als gelesen markieren“ bei beendeter App.
  - Erlaubnis verweigert: Die Einstellungen zeigen es, „Einschalten“ führt hin.
- **Zu prüfen, Desktop:**
  - Einzelmeldung, zweite Nachricht ersetzt die erste, Lesen schließt sie.
  - Erste Nachricht eines neuen Chats.
  - Fünf Chats auf einmal ergeben eine Zusammenfassung.
  - Start im Hintergrund nach Nachrichten bei geschlossener App: eine Zusammenfassung.
  - „Nicht stören“: nichts, auch kein Klingeln.
  - Tray-Punkt an und aus, Autostart-Eintrag an und aus.
- **Kern:** `cargo test -p talk-core -p talk-demo`.
