# Gruppen – Neue Nachricht, Gruppe erstellen, Gruppeneinstellungen

Verbindlich für Android, iOS, Desktop Linux und macOS. Das Vorbild ist Signal: Wir übernehmen die Abläufe und den Aufbau, keinen Code, keine Icons und keine Texte.

**Begriffe**
- In der App heißt es **„Gruppe“**. Technisch ist das eine Talk-Unterhaltung vom Typ 2 (nur auf Einladung), mit Gruppenlink vom Typ 3.
- Signal nennt die Rolle „Admin“, bei uns heißt sie **„Moderator“**. In Talk ist das der Besitzer oder ein Moderator.
- Den Besitzer zeigen wir nicht gesondert an, er trägt ebenfalls das Schild „Moderator“. Seine Rechte lassen sich nicht ändern, und er kann nicht entfernt werden.

## Kern-Vertrag

Alle ändernden Aufrufe liefern `null` bei Erfolg, sonst einen kurzen deutschen Fehlertext. Ihn zeigt die Plattform nur als Hinweis (Toast). Nach jeder Änderung aktualisiert der Kern die Unterhaltungen selbst und sendet `roomsChanged`, die Plattform muss nichts nachladen.

**RoomView** (neu)

| Feld | Bedeutung |
|---|---|
| `isModerator` | Darf die Gruppe und ihre Mitglieder bearbeiten. |
| `canLeave` | Der Wert des Servers (`canLeaveConversation`). Er deckt den letzten Moderator **nicht** ab, dafür gibt es `leaveNeedsModerator`. |
| `canDelete` | Darf die Gruppe für alle löschen. |
| `isPublic` | Der Gruppenlink ist an (Typ 3). |
| `hasCustomAvatar` | Es ist ein Foto gesetzt, es lässt sich also entfernen. |
| `notificationLevel` | Gab es schon: 1 alle, 2 nur Erwähnungen, 3 aus (0 Server-Standard). |

Gruppeneinstellungen gibt es für die Typen 2 und 3.

**Aufrufe**

| Aufruf | Ergebnis | Bemerkung |
|---|---|---|
| `canCreateGroups()` | bool | Ohne dieses Recht gibt es die Zeile „Neue Gruppe“ nicht. |
| `searchUsers(query, excludeToken?)` | `UserView[]` (`id`, `name`, `subline`) | Bei leerer Suche alle Nutzer, die der Server auflistet, alphabetisch, sonst die Treffer. Man selbst fehlt immer. Mit `excludeToken` fehlen auch die Mitglieder dieser Gruppe. |
| `openDirect(userId)` | `{token, error}` | Der vorhandene oder ein neuer 1:1-Chat. Er steht sofort in der Liste. |
| `createGroup(name, userIds, photo?)` | `{token, error}` | `photo` sind beliebige Bildbytes (JPEG, PNG oder WebP). Der Kern dreht sie nach EXIF, schneidet sie mittig quadratisch zu und lädt 512 px als JPEG hoch. Ist das Foto unbrauchbar, wird die Gruppe trotzdem angelegt. |
| `members(token)` | `MemberView[]` | Felder `attendeeId`, `actorType`, `actorId`, `name`, `isSelf`, `isModerator`, `isOwner`. Sortiert: du zuerst, dann die Moderatoren, dann alphabetisch. |
| `addMembers(token, userIds)` | Fehlertext | |
| `removeMember(token, attendeeId)` | Fehlertext | |
| `setModerator(token, attendeeId, bool)` | Fehlertext | |
| `renameGroup(token, name)` | Fehlertext | |
| `setDescription(token, text)` | Fehlertext | Höchstens 2000 Zeichen, leer entfernt die Beschreibung. |
| `setGroupPhoto(token, photo?)` | Fehlertext | `null` entfernt das Foto. |
| `setGroupLink(token, on)` | Fehlertext | |
| `groupLink(token)` | Link | Zum Teilen bzw. Kopieren. |
| `setNotificationLevel(token, level)` | Fehlertext | |
| `leaveNeedsModerator(token)` | bool | Beim Tippen auf „Gruppe verlassen“ fragen. `true`, wenn man der einzige Moderator ist und andere bleiben: Dann kommt zuerst „Neuen Moderator wählen“. |
| `leaveGroup(token)` | Fehlertext | Danach fehlt die Gruppe in `roomsChanged`. |
| `deleteGroup(token)` | Fehlertext | Danach fehlt die Gruppe in `roomsChanged`. |

**Event `participantsChanged {token}`**
- Es kommt nach eigenen Änderungen an den Mitgliedern und bei Systemnachrichten über Mitglieder in offenen Chats, etwa wenn jemand hinzugefügt, entfernt, befördert oder herabgestuft wurde.
- Ist die Mitgliederliste offen, lädt die Plattform `members()` neu.
- Name, Beschreibung, Foto und Gruppenlink kommen ohnehin über `roomsChanged`.

**Entfernt oder gelöscht**
- Wird man aus der Gruppe entfernt oder die Gruppe gelöscht, verschwindet sie aus `roomsChanged`. Bei einem offenen Chat geht das sofort, sonst spätestens nach einer Minute.
- Steht der offene Chat bzw. die offene Gruppe danach nicht mehr in der Liste, schließt die Plattform beides. Den Hinweis „Du bist nicht mehr in dieser Gruppe“ zeigt sie nur, wenn man nicht selbst verlassen oder gelöscht hat.

**Fehlertexte** (eine Auswahl):
- „Keine Verbindung“
- „Nur Moderatoren können das“
- „Ernenne zuerst einen anderen Moderator“
- „Bild konnte nicht verwendet werden“
- „Bitte einen Gruppennamen eingeben“
- „Nutzer nicht gefunden“

`searchUsers` und `members` werfen bei einem Fehler. Dann zeigt die Plattform in der Liste „Keine Verbindung“ bzw. „Konnte nicht geladen werden“ mit dem Knopf „Erneut versuchen“.

## Neue Nachricht

**Einstieg**
- Ein Bleistift-Symbol „Neue Nachricht“.
- Desktop: im Kopf der Seitenleiste neben „⋯“, Kürzel Strg/⌘+N.
- Android: ein schwebender Knopf (FAB) unten rechts in der Chatliste.
- iOS: rechts in der Navigationsleiste der Chatliste.

**Aufbau von oben nach unten**
1. Titel „Neue Nachricht“ mit Zurück bzw. Schließen.
2. Suchfeld „Name suchen“, beim Öffnen fokussiert (Desktop) bzw. nicht fokussiert (Mobil, die Liste ist wichtiger).
3. Die Zeile **„Neue Gruppe“** mit Personen-Symbol in einem Kreis in `accent`. Nur bei `canCreateGroups`, und sie verschwindet, sobald man sucht.
4. Der Abschnitt **„Kontakte“**: Zeilen mit Avatar (40), Name und `subline` gedämpft darunter, falls vorhanden.

**Verhalten**
- Die Suche läuft mit einer Verzögerung von 250 ms über `searchUsers`.
- Beim Laden erscheinen drei Platzhalterzeilen. Ohne Treffer steht dort „Keine Treffer“.
- Ein Kontakt ruft `openDirect` auf und öffnet den Chat. Bei einem Fehler bleibt die Liste stehen und ein Toast erscheint.

## Neue Gruppe

**Schritt 1, „Mitglieder hinzufügen“**
- Oben das Suchfeld. Darunter erscheinen die ausgewählten Personen als Chips (Avatar 24, Vorname, ×), sie umbrechen und lassen sich mit × abwählen.
- Die Kontaktliste wie oben, jede Zeile rechts mit einem runden Häkchen: leer als Kreis in `muted`, ausgewählt gefüllt in `accent` mit weißem Haken. Tippen auf die Zeile schaltet um.
- Nach einer Auswahl wird das Suchfeld geleert.
- Unten bzw. als FAB der Knopf „Weiter“. Ohne Auswahl heißt er „Überspringen“, eine Gruppe nur mit einem selbst ist erlaubt.
- Die Auswahl bleibt erhalten, wenn man aus Schritt 2 zurückgeht.

**Schritt 2, „Gruppe benennen“**
- Oben mittig ein großer runder Avatar (80) mit Kamera-Symbol. Er öffnet die Fotoauswahl. Mit Foto zeigt er die Vorschau (quadratisch zugeschnitten) und darunter einen kleinen Knopf „Entfernen“.
- Das Feld „Gruppenname (erforderlich)“, fokussiert, höchstens 255 Zeichen.
- Darunter „Mitglieder“ mit Anzahl und der Liste der Ausgewählten, nur zur Ansicht.
- Der Knopf **„Erstellen“** ist aktiv, sobald der Name nicht leer ist.
  - Während des Anlegens zeigt er einen Spinner, alles ist gesperrt.
  - Danach schließt sich der Ablauf und der neue Chat öffnet sich.
  - Bei einem Fehler bleibt man auf dem Schritt, und ein Toast erscheint.

## Gruppeneinstellungen

**Öffnen und schließen**
- Tippen bzw. Klicken auf den Chat-Kopf (Avatar und Name) öffnet die Einstellungen, nur bei Gruppen.
- Desktop: Sie ersetzen den Chatbereich mit einem Zurück-Pfeil, Esc geht ebenfalls zurück.
- Android: ein eigener Bildschirm.
- iOS: Push in den Navigationsstapel.

**Aufbau**

1. **Kopf** (mittig)
   - Avatar 80, Name (groß) und „Gruppe · N Mitglieder“ gedämpft. Ist der Gruppenlink an: „Gruppe mit Link · N Mitglieder“.
   - Die Beschreibung, höchstens 3 Zeilen mit „Mehr“. Ohne Beschreibung sehen Moderatoren „Beschreibung hinzufügen“ als Link, andere nichts.
   - Moderatoren haben den Knopf „Bearbeiten“. Er öffnet **„Gruppe bearbeiten“**:
     - Foto mit „Ändern“ und, falls gesetzt, „Entfernen“
     - Name und Beschreibung (mehrzeilig, mit Zähler ab 1800)
     - Knöpfe „Abbrechen“ und „Speichern“
     - Gespeichert werden nur die Felder, die sich geändert haben, nacheinander. Beim ersten Fehler erscheinen ein Toast und das Formular bleibt offen.
2. **Benachrichtigungen**
   - Eine Zeile mit Glocke und dem aktuellen Wert: „Alle Nachrichten“, „Nur Erwähnungen“ oder „Aus“.
   - Sie öffnet eine Auswahl mit diesen drei Werten, das ist `setNotificationLevel` mit 1, 2 oder 3. Steht der Wert auf 0, wird „Alle Nachrichten“ angezeigt.
3. **„N Mitglieder“** (Abschnittsüberschrift)
   - Moderatoren sehen als erste Zeile „Mitglieder hinzufügen“ (Plus im Kreis in `accent`). Sie öffnet die Auswahl aus Schritt 1 mit `excludeToken`, Titel „Mitglieder hinzufügen“ und dem Knopf „Hinzufügen (N)“ statt „Weiter“, ohne Schritt 2.
   - Jede Mitglied-Zeile zeigt Avatar 40 und Name. Die eigene heißt „Du“, und Moderatoren tragen rechts das Schild „Moderator“ in `muted`.
   - Gäste, Gruppen und E-Mail-Gäste erscheinen mit ihrem Namen und dem Untertitel „Gast“, „Nextcloud-Gruppe“ bzw. „E-Mail“.
   - Tippen auf ein Mitglied, außer auf einen selbst, öffnet das **Mitglied-Blatt** (Mobil als Bottom-Sheet, Desktop als kleines Modal):
     - Großer Avatar und Name.
     - „Nachricht senden“ (nur `users`): `openDirect`, dann der Chat.
     - Nur für Moderatoren und nicht beim Besitzer: „Zum Moderator machen“ bzw. „Moderatorrechte entziehen“.
     - Nur für Moderatoren und nicht beim Besitzer: „Aus der Gruppe entfernen“ in `danger`, mit der Bestätigung „{Name} aus der Gruppe entfernen?“ und den Knöpfen „Entfernen“ und „Abbrechen“.
     - Nach einer Aktion schließt das Blatt, die Liste lädt über `participantsChanged` neu.
   - Bei mehr als 10 Mitgliedern stehen zunächst 10 da und die Zeile „Alle anzeigen“.
4. **Gruppenlink**
   - Moderatoren sehen den Schalter „Gruppenlink“ mit dem Untertitel „Jeder mit dem Link kann beitreten“.
   - Ist der Link an, für alle:
     - die Zeile mit dem Link (eine Zeile, gekürzt)
     - „Link kopieren“, danach der Toast „Link kopiert“
     - mobil zusätzlich „Teilen“ über das System-Teilen
5. **Ganz unten** in `danger`, jeweils mit Bestätigung:
   - **„Gruppe verlassen“**
     - Bestätigung: „Gruppe verlassen? Du erhältst keine Nachrichten mehr aus dieser Gruppe.“
     - Liefert `leaveNeedsModerator` `true`, kommt stattdessen „Neuen Moderator wählen“: „Bevor du gehst, ernenne jemanden zum Moderator.“ mit der Mitgliederliste ohne einen selbst zur Auswahl. Danach `setModerator(…, true)` und `leaveGroup`.
   - **„Gruppe löschen“**, nur mit `canDelete`.
     - Bestätigung: „Gruppe für alle löschen? Alle Nachrichten gehen verloren.“ mit den Knöpfen „Löschen“ und „Abbrechen“.
   - Nach Verlassen oder Löschen schließen Einstellungen und Chat. Man landet in der Chatliste.

**Darstellung**
- Abschnitte als Gruppen mit Überschrift wie in den Systemeinstellungen. Zeilen 48 bis 56 hoch, Symbole links in `muted`.
- Farben nur aus den vorhandenen Tokens, hell und dunkel.
- Ein Ladezustand nur in der Mitgliederliste (Platzhalterzeilen). Alles andere kommt sofort aus `RoomView`.

## Barrierefreiheit

| Element | Bezeichnung |
|---|---|
| Bleistift | „Neue Nachricht“ |
| Zeile in der Auswahl | „{Name}, ausgewählt“ bzw. „{Name}, nicht ausgewählt“ |
| Chip | „{Name} entfernen“ |
| Avatar im Schritt „Gruppe benennen“ | „Gruppenbild wählen“ |
| Mitglied-Zeile | „{Name}, Moderator“ bzw. „{Name}“ |
| Chat-Kopf einer Gruppe | „Gruppeneinstellungen“ (Desktop: `title`) |

## Tests

- **Nur mit Bot A und Bot B.** Testgruppen heißen „Test …“ und werden nach dem Test gelöscht. Niemals echte Nutzer hinzufügen.
- Die Nutzersuche zeigt echte Namen. Das ist erlaubt, aber nur lesend.
- **talk-cli** (in `core/`: `cargo build -p talk-cli`):
  - `talk-cli -a a users [Suche] [--exclude <token>]`
  - `talk-cli -a a group create "Test …" -m <bot-b-id> [--photo bild.png]` gibt das Token aus.
  - `talk-cli -a b group info|members <token>`
  - `talk-cli -a a group add|remove|promote|demote|rename|describe|photo|link|mute|leave|delete …`
  - `talk-cli -a b group watch <token> --seconds 60` zeigt, was die App von Bot B bekommt (`Mitglieder geändert`, Raumänderungen, „nicht mehr in der Liste“).
