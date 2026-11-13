# Kategorien in der Chatliste

In Talk kann man Chats in eigene Abschnitte ordnen. Talk nennt sie „Tags“ (Feature `conversation-tags`). In unseren Apps heißen sie **„Kategorie“**, denn „Gruppe“ ist schon der Gruppenchat (docs/groups.md).

Die Liste zeigt **Favoriten | eigene Kategorien | Chats** in der Reihenfolge vom Server, genau wie Talk-Web. Jeder Abschnitt lässt sich einklappen.

**Wer was kann:**
- Alle Plattformen zeigen die Abschnitte an und klappen sie ein und aus.
- Nur der Desktop verwaltet sie: anlegen, umbenennen, löschen, sortieren und Chats zuordnen.

## Server (spreed ≥ 24, `/ocs/v2.php/apps/spreed/api/v4`)

| Aufruf | Zweck |
|---|---|
| `GET /tags` | Alle Kategorien `{id, name, sortOrder, collapsed, type}`. `type` ist `favorites`, `other` oder `custom`. Die beiden eingebauten Einträge legt der Server beim ersten Abruf selbst an. |
| `POST /tags {name}` | Anlegen. 400 `error: name` heißt leer, über 250 Zeichen oder schon vergeben; 400 `error: limit` heißt mehr als 100. |
| `PUT /tags/{id} {name}`, `DELETE /tags/{id}` | Umbenennen bzw. Löschen. Geht nur bei `custom`. Beim Löschen verlieren alle Chats diese Kategorie. |
| `PUT /tags/reorder {orderedIds}` | Neue Reihenfolge aller Einträge, die eingebauten eingeschlossen. |
| `PUT /tags/{id}/collapsed {collapsed}` | Eingeklappt-Zustand, gilt für alle Geräte und auch für Talk-Web. |
| `POST /room/{token}/tags {tagIds}` | Ersetzt die Kategorien eines Chats (höchstens 20). Jeder Raum liefert `tagIds`. |

## Abschnitte (Kern: `room_sections(query, active_token)`)

**Zuordnung**, wie Talk-Web:
- Ein Favorit steht unter „Favoriten“.
- Jede eigene Kategorie enthält die Chats mit ihrer id in `tagIds`. Ein Chat kann in mehreren Kategorien stehen, auch zusätzlich zu „Favoriten“.
- Ein Chat, der kein Favorit ist und in keiner (vorhandenen) Kategorie steht, kommt unter „Chats“.
- Archivierte Chats fehlen, leere Abschnitte ebenso.

**`SectionView`:**
- `id`
- `kind`: `favorites`, `custom` oder `other`
- `title`: „Favoriten“, der Name bzw. „Chats“
- `collapsed`
- `rooms`: die Zeilen, die angezeigt werden
- `total`: alle Chats des Abschnitts

**Eingeklappt:**
- `rooms` enthält dann nur:
  - den offenen Chat (`active_token`),
  - ungelesene Chats,
  - Chats mit laufendem Anruf.
- So geht nichts unter.

**Suche:** `query` filtert nach Namen. Während der Suche ist nichts eingeklappt.

**Nur ein Abschnitt:**
- Gibt es nur einen Abschnitt (zum Beispiel nur „Chats“), ist `title` leer.
- Die Plattform zeigt dann keinen Kopf, und der Abschnitt ist nie eingeklappt.

**Ohne Server-Unterstützung** (oder bevor der erste Abruf geklappt hat):
- Es gibt feste Abschnitte `fav` und `other`.
- Ihr Eingeklappt-Zustand wird nur lokal gemerkt.

**Aktualität:**
- Der Kern lädt die Kategorien beim Start und bei jeder vollen Aktualisierung der Räume.
- Jede Änderung sendet `RoomsChanged`.
- Die Plattformen rufen `room_sections` neu auf:
  - bei `RoomsChanged`,
  - wenn sich der Suchtext ändert,
  - wenn ein anderer Chat geöffnet wird.

**Aufrufe:**
- `toggle_section(id)`
  - Zeigt die Änderung sofort, speichert dann auf dem Server und nimmt sie bei einem Fehler zurück.
  - Gibt `null` oder einen deutschen Fehlertext zurück; den zeigt ihr als Toast.

## Darstellung (alle Plattformen)

**Kopf:**
- Derselbe Stil wie bisher „Favoriten“/„Chats“.
- Rechts neben dem Titel steht ein kleines, gedämpftes Chevron:
  - Es zeigt „>“, wenn der Abschnitt eingeklappt ist.
  - Aufgeklappt ist es um 90° nach unten gedreht.
  - Die Drehung dauert etwa 150 ms; bei „Bewegung reduzieren“ entfällt sie.
- Der ganze Kopf ist anklickbar.
- Mehr gibt es nicht: keine Zähler, keine Linien.

**Zeilen:**
- Sie bleiben unverändert.
- Der Schlüssel einer Zeile ist `sectionId:token`, denn ein Chat kann mehrfach erscheinen.

**Barrierefreiheit:**
- Der Kopf ist ein Button mit `aria-expanded` bzw. dem Zustand „ausgeklappt“/„eingeklappt“.
- Das Label nennt den Titel, zum Beispiel „Arbeit, Kategorie“.
- Ist der Abschnitt eingeklappt und sind Chats ausgeblendet, kommt „, N Chats ausgeblendet“ hinzu.

## Desktop: Seite „Kategorien“

**Einstieg:**
- Im „⋯“-Menü steht „Kategorien verwalten“, nur bei `categoriesSupported()`.
- Die Seite ersetzt den Chatbereich mit Zurück-Pfeil, wie die Gruppeneinstellungen.
- Öffnet man einen Chat, schließt sie.

**Liste** aus `categories()` in Reihenfolge:
- Jede Zeile zeigt den Namen, darunter „N Chats“.
- Rechts stehen ↑ und ↓ zum Verschieben (`moveCategory`). Das gilt auch für „Favoriten“ und „Chats“; an den Enden sind die Pfeile deaktiviert.
- Eigene Kategorien haben zusätzlich ein Menü mit:
  - „Umbenennen“: ein Modal mit Namensfeld und „Speichern“.
  - „Löschen“: rot, mit der Bestätigung „Kategorie „Name“ löschen? Die Chats bleiben erhalten.“
- Ein Klick auf eine eigene Kategorie öffnet „Chats auswählen“.

**„Neue Kategorie“** oben auf der Seite:
- Zuerst ein Modal mit Namensfeld „Name der Kategorie“; „Weiter“ ist erst mit Namen aktiv.
- Danach „Chats auswählen“.
- „Erstellen“ ruft `createCategory(name, tokens)` auf.

**„Chats auswählen“:**
- Oben eine Suche, darunter alle nicht archivierten Chats mit Avatar, Name und rundem Häkchen rechts.
- Es ist dieselbe Auswahl wie bei „Mitglieder hinzufügen“.
- „Speichern“ ruft `setCategoryRooms(id, tokens)` auf; danach sind genau diese Chats in der Kategorie.

**Fehler** kommen als Toast. Mögliche Texte:
- „Name ist leer oder schon vergeben“
- „Zu viele Kategorien“
- „Keine Verbindung“
- „Kategorie nicht gefunden“

## Test

**Testdaten:**
- Kategorien nur auf den Bot-Konten anlegen, Name „Test …“, und am Ende löschen.
- Zuordnen nur `TEST_ROOM_TOKEN`, `CALL_TEST_ROOM_TOKEN` oder das Bot-1:1.
- Danach mit `talk-cli -a a category list` prüfen, dass nur „Favoriten“ und „Chats“ übrig sind, und mit `talk-cli -a a rooms --json` prüfen, dass alle `tagIds` leer sind.

**Werkzeuge:**
- Abschnitte prüfen: `talk-cli -a a sections [suche] [--active TOKEN]`
- Gegenseite und Vorbereitung: `talk-cli -a a category list|create|rename|delete|move|collapse|set …`
- Bot-Tests laufen unter `bot-lock.sh`.

**Wenn ihr einklappt:**
- Den Zustand von „Favoriten“ und „Chats“ am Ende wieder aufklappen.
- Er liegt auf dem Server und gilt auch in Talk-Web.
