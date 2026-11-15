# Mehrere Konten

Jeder Client kann mit mehreren Nextcloud-Konten gleichzeitig angemeldet sein, auch auf verschiedenen Servern. Eines davon ist das **aktive Konto**: Dessen Chats zeigt die App. Alle anderen laufen im Hintergrund weiter und melden sich mit Badge, Benachrichtigungen und klingelnden Anrufen.

Im Kern ist ein `TalkSession` genau ein Konto, und mehrere bestehen nebeneinander. Die Kontoliste, die Geheimnisse und die Oberfläche gehören der Plattform. Dieser Vertrag legt fest, was auf allen Plattformen gleich ist.

## Begriffe

- **Konto-`id`:** eine zufällige lokale Kennung (mindestens 8 Hex-Zeichen), die beim Anmelden entsteht und nie wiederverwendet wird. Sie steht in Ordnernamen, Schlüsselnamen und Benachrichtigungs-IDs. Sie verlässt das Gerät nicht.
- **Name:** der Anzeigename vom Server (`displayName()`), ersatzweise die Benutzer-ID.
- **Server:** die Adresse ohne `https://`, wie im Kopf der Einstellungen.
- **Laufzeit:** eine gestartete `TalkSession` mit eigenem Listener, der die Konto-`id` kennt. Ereignisse aus dem Kern tragen kein Konto, nur der Listener weiß, zu wem sie gehören.

## Bedienung

### Avatar oben links

| Konten | Aussehen | Klick oder Tippen |
|---|---|---|
| eins | wie bisher | öffnet die Einstellungen (`docs/settings.md`) |
| zwei und mehr | Zahl oben rechts am Avatar, wenn es in den **anderen** Konten Neues gibt | öffnet das Kontomenü |

Die Zahl ist die Summe von `badge().chats` der anderen Konten, also die Chats mit Neuem, das zählt (Regel in `docs/settings.md`, „Stummgeschaltete Chats“). Über 99 steht „99+“. Die Zahl steht in `accent` mit weißer Schrift, wie die Zahl der Ungelesenen in der Chatliste.

### Kontomenü

1. **Eine Zeile pro Konto,** in der Reihenfolge der Anmeldung:
   - Avatar 36, daneben der Name, darunter gedämpft der Server.
   - Rechts beim aktiven Konto ein Häkchen, bei den anderen ihre Zahl (`badge().chats`), sofern größer als null.
   - Ein Klick auf ein anderes Konto wechselt dorthin. Ein Klick auf das aktive schließt nur das Menü.
2. Eine Trennlinie.
3. **„Konto hinzufügen“** mit Plus-Symbol. Fehlt ab 5 Konten.
4. **„Einstellungen“** mit Zahnrad.

| Plattform | Form |
|---|---|
| Desktop | Popover am Avatar, sieht aus wie das Nachrichtenmenü. Pfeiltasten, Enter und Esc funktionieren. |
| Android | `ModalBottomSheet` |
| iOS | Sheet in halber Höhe (`.presentationDetents([.medium])`) |

### Einstellungen

- Der Kopf zeigt das aktive Konto, jetzt mit dem Namen statt des Anmeldenamens.
- Direkt darunter steht neu der Abschnitt **„Konten“**:
  - die anderen Konten als Zeilen wie im Kontomenü (Tippen wechselt, die Einstellungen bleiben offen),
  - darunter „Konto hinzufügen“.
- So lässt sich ein zweites Konto auch dann hinzufügen, wenn der Avatar noch direkt die Einstellungen öffnet.
- „Abmelden“ ganz unten meldet das aktive Konto ab.

### Konto hinzufügen

- Es erscheint der bisherige Anmeldebildschirm, mit dem Titel „Konto hinzufügen“ und einem Knopf „Abbrechen“, der zurückführt. Die anderen Konten laufen währenddessen weiter.
- Nach der Anmeldung im Browser fragt die App mit `resolveAccount(server, loginName, appPassword)` nach, wer sich angemeldet hat. Erst danach wird das Konto gespeichert.
- **Schon angemeldet:** Hat ein Konto der Liste denselben `key`, wechselt die App dorthin und zeigt den Toast „Dieses Konto ist schon angemeldet“. Das frische App-Passwort gibt sie mit `revokeAppPassword(...)` zurück.
- **Sonst** wird das neue Konto gespeichert, gestartet und aktiv.
- Scheitert `resolveAccount` (kein Netz), bleibt die Anmeldung offen und zeigt den Fehler. Es wird nichts gespeichert.

### Wechseln

- Der offene Chat schließt (`closeChat`), eine laufende Sprachaufnahme wird beendet und als Entwurf abgelegt. Dann zeigt die App die Chatliste des anderen Kontos, sofort aus dessen Laufzeit (`rooms()`), ohne Ladezustand.
- Textentwürfe und angehängte Dateien bleiben pro Konto und Chat erhalten.
- Das aktive Konto wird gespeichert und ist nach einem Neustart wieder aktiv.
- **Während eines Anrufs** ist der Wechsel gesperrt: Die Zeilen der anderen Konten sind ausgegraut, ein Klick zeigt den Toast „Während eines Anrufs nicht möglich“. Das gilt auch für den Klick auf eine Benachrichtigung eines anderen Kontos.

### Abmelden

- „Abmelden“ fragt auf **allen** Plattformen nach:
  - Titel „Abmelden?“
  - Text „{Name} ({Server}) wird auf diesem Gerät abgemeldet. Ungesendete Nachrichten und der Offline-Speicher dieses Kontos werden gelöscht.“
  - Knöpfe „Abmelden“ (in `danger`) und „Abbrechen“.
- Abgemeldet wird nur dieses Konto: Push abmelden (`unregisterPush`), `logout()`, dann seinen Eintrag, sein Geheimnis, seinen Push-Schlüssel und seinen Datenordner löschen. Die anderen Konten bleiben unberührt, auch ihre Benachrichtigungen und ihr Postausgang.
- Danach wird das erste verbleibende Konto aktiv. Ist keines mehr da, erscheint die Anmeldung.

### Anmeldung abgelaufen

Meldet eine Laufzeit `authFailed`, wird genau dieses Konto entfernt, ohne Rückfrage und ohne `logout()` am Server. Die App zeigt „{Name} ({Server}) wurde abgemeldet. Bitte neu anmelden.“:
- als Toast, wenn die App sichtbar ist,
- am Desktop zusätzlich als Benachrichtigung, wenn das Fenster nicht im Vordergrund ist.

War es das aktive Konto, wird das erste verbleibende aktiv. Der Hinweis erscheint trotzdem, damit niemand unbemerkt im falschen Konto schreibt.

## Im Hintergrund

Alle Laufzeiten sind gestartet (`start()`), nicht nur die aktive. Nur die aktive füllt die Oberfläche und hält den Long-Poll eines offenen Chats.

| Ereignis einer inaktiven Laufzeit | Was die Plattform tut |
|---|---|
| `roomsChanged` | Badge neu rechnen; Desktop: Benachrichtigungen wie im aktiven Konto |
| `callChanged`, `callMissed` | an die eine Anrufsteuerung, die sich das Konto des Anrufs merkt |
| `outboxChanged` | fehlgeschlagene Sendungen melden wie bisher |
| `recordingsChanged` | wie bisher, mit Konto |
| `authFailed` | Konto entfernen, siehe oben |
| alles andere | verwerfen |

- **Postausgang:** Die Postausgänge aller Konten senden weiter. Hintergrundarbeit (Android `OutboxWorker`, iOS `BackgroundSender`, Desktop beim Beenden) läuft über alle Konten, mit einer gemeinsamen Frist.
- **App-Badge:** die Summe über alle Konten, in der bisherigen Einheit der Plattform: Desktop `badge().chats`, iOS `badge().messages`. Android hat weiter keinen.
- **Benachrichtigungen:**
  - Ab zwei Konten nennt jede Benachrichtigung das Konto als „{Name} · {Server}“: als Untertitel, wo es einen gibt (macOS, iOS), sonst hinter dem Titel.
  - Ein Klick wechselt ins Konto und öffnet den Chat.
  - Unterdrückt wird nur, wenn genau dieser Chat im **aktiven** Konto offen und die App im Vordergrund ist.
  - Mit nur einem Konto sieht alles aus wie bisher.
- **Derselbe Chat in zwei Konten** (zwei Konten desselben Servers im selben Raum): Benachrichtigung und Badge kommen einmal pro Konto. Ein Anruf klingelt nur einmal.

## Anrufe

Über alle Konten gibt es **einen** Anruf gleichzeitig. Das erzwingt der Kern (`docs/calls.md`, „Ein Anruf gleichzeitig“): Klingelt oder läuft ein Anruf in einem Konto, sind alle anderen besetzt.

- Ein Anruf in einem inaktiven Konto klingelt wie jeder andere. Ab zwei Konten nennt die Klingelanzeige das Konto.
- **Annehmen, Ablehnen, Auflegen, Stumm, Kamera, Bildschirm und alle Media-Reports gehen an die Laufzeit des Anrufs,** nicht an die aktive. Die Anrufsteuerung merkt sich dazu bei `callChanged` das Konto.
- **Annehmen** schickt zuerst `acceptCall` an dieses Konto und wechselt danach dorthin. **Ablehnen** wechselt nicht.
- Kommt während eines Anrufs ein zweiter, egal in welchem Konto, meldet der Kern ihn als verpasst (`callMissed`). Die Benachrichtigung „Verpasster Anruf“ nennt ab zwei Konten das Konto.
- `callChanged { call: null }` kommt immer vom Konto des Anrufs und immer, bevor ein anderes Konto klingeln kann.
- `startCall` in einem Konto schlägt mit `busy` fehl, solange ein anderes klingelt oder telefoniert.
- Das Abmelden eines Kontos legt nur dessen eigenen Anruf auf.
- Die `MediaEngine` gibt es einmal pro Prozess. Sie wird jeder Laufzeit übergeben (`setMediaEngine`).

## Push (Android, iOS)

Eine Push-Nachricht nennt ihr Konto nicht. Deshalb hat **jedes Konto einen eigenen Geräteschlüssel**, und der Schlüssel, der entschlüsselt, bestimmt das Konto:

```
decryptPushFor(keys: [{accountId, keyPem}], subject) -> {accountId, message}?
```

- `null` heißt: Kein Schlüssel passt, das Konto gibt es nicht mehr. Dann nichts anzeigen. iOS muss den PushKit-Anruf trotzdem melden und sofort beenden.
- **Registrieren:** Jede Laufzeit registriert dasselbe Push-Token mit ihrem eigenen Schlüssel (`registerPush`), bei jedem Start und für **alle** Konten, wenn sich das Token ändert. Ob ein Konto registriert ist, wird pro Konto gemerkt.
- **Abmelden eines Kontos** meldet nur dieses Konto ab (`unregisterPush`). Android löscht das FCM-Token erst, wenn das letzte Konto geht.
- **Vor dem Start der App** (iOS PushKit, Notification-Erweiterung; Android FCM bei beendetem Prozess) müssen die Schlüssel aller Konten samt `id` und „{Name} · {Server}“ synchron lesbar sein, ohne Laufzeit und ohne Netz. Auf iOS liegen sie dafür in der geteilten Keychain-Gruppe.
- **Anruf per Push:** Konto bestimmen, dann `handlePush` an genau dessen Laufzeit. Die Laufzeiten aller Konten müssen dafür beim Kaltstart entstehen.
- **Gelöschte Benachrichtigungen** (`Delete`) gelten nur für das Konto der Push-Nachricht. Benachrichtigungs-IDs verschiedener Server überschneiden sich.

## Speicherung

### Kontoliste

Jede Plattform führt eine Liste und die `id` des aktiven Kontos:

| Feld | Inhalt |
|---|---|
| `id` | siehe oben |
| `server`, `loginName` | wie von der Anmeldung geliefert |
| `userId`, `name` | aus `resolveAccount`; `name` nach jedem `start()` mit `displayName()` auffrischen |
| `key` | aus `resolveAccount`, für „schon angemeldet“ |
| `dir` | Datenordner der Laufzeit |

Das App-Passwort und der Push-Schlüssel liegen pro Konto im sicheren Speicher der Plattform, nie in der Liste.

| Plattform | Liste | Geheimnisse |
|---|---|---|
| Desktop | `accounts.json` in `userData` | App-Passwort mit `safeStorage` verschlüsselt im Eintrag |
| Android | SharedPreferences `account` | wie bisher mit dem Keystore-Schlüssel `talk-account` verschlüsselt, ein Wert pro Konto |
| iOS | UserDefaults | Keychain, ein Eintrag pro Konto; Push-Schlüssel in der geteilten Gruppe |

Neue Konten bekommen den Datenordner `accounts/<id>/` neben dem bisherigen `talk/`.

### Übernahme des bisherigen Kontos

Beim ersten Start mit dieser Version wird das vorhandene Konto der erste Eintrag, ohne neue Anmeldung:

- Es bekommt eine neue `id`, behält aber seinen Datenordner **`talk/`**. Der Ordner wird nicht verschoben: Der Postausgang merkt sich absolute Dateipfade und würde wartende Anhänge sonst verwerfen.
- Es behält seinen Push-Schlüssel, damit die bestehende Registrierung gilt.
- `userId`, `name` und `key` kommen aus dem Cache (`userId`, `displayName()`), ohne Netz. Der `key` wird nach dem ersten `start()` mit `resolveAccount` nachgetragen.
- Die alten Einträge (Desktop `account.json`, iOS Keychain „primary“, Android die einzelnen Prefs-Schlüssel) werden erst gelöscht, wenn die neue Liste geschrieben ist.
- iOS: PushKit und die Notification-Erweiterung können laufen, bevor die App übernommen hat. Finden sie keine Schlüsselliste, nehmen sie den alten Schlüssel `device-key`.

### Lesefehler ist nicht „kein Konto“

Scheitert das Lesen aus Keychain, Keystore oder `safeStorage` (gesperrtes Gerät vor dem ersten Entsperren, gesperrter Schlüsselbund), dann:
- wird **nichts gelöscht**, weder Einträge noch Datenordner,
- gilt die Übernahme nicht als erledigt,
- startet das betroffene Konto in diesem Lauf nicht; die App versucht es beim nächsten Start wieder.

Nur ein Eintrag, der sicher fehlt (kein Fehler, sondern „nicht vorhanden“), heißt „kein Konto“.

### Die Konto-`id` gehört überall dazu

Alles, was bisher nur einen Chat-Token, eine Benutzer-ID, eine Datei-ID oder eine Benachrichtigungs-ID kannte, bekommt die Konto-`id` dazu. Token und IDs verschiedener Server überschneiden sich, und zwei Konten desselben Servers teilen sich Räume.

- Benachrichtigungen: IDs, Tags, Thread-IDs, `PendingIntent`-Request-Codes, `userInfo`
- Caches für Avatare, Vorschauen und Seitenverhältnisse
- Entwürfe (Text, Dateien, Sprachnachricht)
- Desktop: jeder IPC-Aufruf, die Ereignisse an den Renderer und die URLs `talk-avatar://` und `talk-media://`
- Anruf: wem er gehört
- Merker wie „schon gemeldet“ (fehlgeschlagene Sendungen, Aufnahme-Hinweise, verpasste Anrufe)

**Desktop:** Jeder IPC-Aufruf trägt die Konto-`id`, und der Hauptprozess führt ihn in genau diesem Konto aus, auch wenn inzwischen ein anderes aktiv ist. Gibt es das Konto nicht mehr, schlägt der Aufruf fehl. Der Renderer verwirft Antworten, die nach einem Wechsel eintreffen. So landet nichts im falschen Konto, wenn der Wechsel vom Hauptprozess ausgeht (Benachrichtigung, Anruf, abgelaufene Anmeldung).

### Geräteeinstellungen

Design, Akzentfarbe und die übrigen Einstellungen aus `docs/settings.md` gehören weiter dem Gerät, nicht dem Konto.

## Kern-Vertrag

Neu in `talk-node` und `talk-ffi`:

| Aufruf | Rückgabe |
|---|---|
| `resolveAccount(server, loginName, appPassword)` (frei) | `{userId, displayName, key}`; `key` ist für denselben Benutzer auf demselben Server immer gleich |
| `revokeAppPassword(server, loginName, appPassword)` (frei) | gibt ein App-Passwort ohne Session zurück |
| `session.displayName()` | Name aus dem Cache, nach `start()` aktuell; bis dahin die Benutzer-ID bzw. der Anmeldename |
| `session.badge()` | `{chats, messages}` nach der Regel für stummgeschaltete Chats |

Nur in `talk-ffi`: `decryptPushFor(keys, subject)`, siehe oben.

Die eigenen Kopien der Badge-Regel entfallen: Desktop `countsForBadge`, iOS `badgeCount`. Das Grau der Zahl in der Chatliste bleibt Sache der Plattform.

## Barrierefreiheit

| Element | Bezeichnung |
|---|---|
| Eigener Avatar, ein Konto | „Einstellungen“ |
| Eigener Avatar, mehrere Konten | „Konten“, bei einer Zahl „Konten, Neues in {n} Chats“ |
| Zeile im Kontomenü | „{Name}, {Server}“, dazu „aktiv“ bzw. „{n} Chats mit Neuem“ |
| Ausgegraute Zeile während eines Anrufs | zusätzlich „nicht verfügbar“ |

## Test

- **Zwei Demo-Server,** je einer pro Konto:
  ```sh
  scripts/screenshots/demo-server.sh start                                         # :8099, Mira
  scripts/screenshots/demo-server.sh start 0.0.0.0:8098 core/talk-demo/fixtures-b  # :8098, Jonas
  curl -d token=mira -d text=Hallo http://127.0.0.1:8098/demo/incoming             # jemand schreibt Jonas
  ```
  - Der zweite Server startet ohne Ungelesenes. „Fahrradwerkstatt“ (`werkstatt`) steht auf „Nur Erwähnungen“: Erst `text=@jonas …` zählt.
  - Den Token `garten` gibt es auf beiden Servern, mit anderem Namen und Bild. Was nur nach Token zwischenspeichert, zeigt dort das Falsche.
  - `from=<id>` wählt den Absender, sonst schreibt das erste andere Mitglied.
- **Ohne Browser anmelden:** Zusätzlich zu `TALK_DEV_SERVER`, `TALK_DEV_USER` und `TALK_DEV_PASS` gibt es dieselben Variablen mit der Endung `_2` für ein zweites Konto. Ein solcher Lauf startet nur diese Konten, nie die gespeicherten, und schreibt nichts in die Kontoliste.
- **Zu prüfen:**
  - Zweites Konto hinzufügen, wechseln, Neustart: Konten und aktives Konto sind noch da.
  - Nachricht im inaktiven Konto: Zahl am Avatar, Zahl im Kontomenü, App-Badge als Summe, Benachrichtigung mit Kontozeile; der Klick wechselt und öffnet den Chat.
  - Nach dem Wechsel zeigt `garten` Namen, Bild und Nachrichten des richtigen Servers.
  - Entwurf in Konto A schreiben, wechseln, zurück: Der Entwurf ist noch da und taucht in B nicht auf.
  - Dasselbe Konto noch einmal anmelden: Toast, kein zweiter Eintrag.
  - Ein Konto abmelden: Rückfrage mit Namen, das andere läuft weiter.
  - Übernahme von der Vorversion, mit einer wartenden Nachricht samt Anhang im Postausgang: kein neues Anmelden, der Anhang geht raus.
- **Echter Server, zwei Konten desselben Servers** (nur Testräume bzw. Bot-1:1, unter `bot-lock.sh`):
  - Schreibt ein Konto im Testraum, bekommt das andere, inaktive die Zahl.
  - Ein Anruf im Anruf-Testraum klingelt genau einmal, nicht einmal pro Konto.
  - Annehmen aus dem inaktiven Konto wechselt dorthin; Ablehnen beendet das Klingeln und wechselt nicht.
  - Während des Anrufs ist der Wechsel gesperrt.
- **Kern:** `cargo test -p talk-core -p talk-demo`.
