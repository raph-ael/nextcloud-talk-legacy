# Kategorien in der Chatliste

In Talk kann man Chats in eigene Abschnitte ordnen. Talk nennt sie „Tags“ (Feature `conversation-tags`). In unseren Apps heißen sie **„Kategorie“**, denn „Gruppe“ ist schon der Gruppenchat (docs/groups.md).

Die Liste zeigt **Favoriten | eigene Kategorien | Chats** in der Reihenfolge vom Server, genau wie Talk-Web. Jeder Abschnitt lässt sich einklappen. Ein eingeklappter Abschnitt ist leer, und sein Kopf zeigt, was darin neu ist.

Standardmäßig steht statt „Chats“ das Paar **„Gruppen“ | „Direktchats“** (Abschnitt „Gruppen und Direktchats“ unten). Das lässt sich in den Einstellungen abschalten.

**Wer was kann:**
- Alle Plattformen zeigen die Abschnitte an und klappen sie ein und aus.
- Alle Plattformen wählen die Sortierung der Chats (Abschnitt „Sortierung“ unten).
- Nur der Desktop verwaltet die Kategorien: anlegen, umbenennen, löschen, verschieben und Chats zuordnen.

## Server (spreed ≥ 24, `/ocs/v2.php/apps/spreed/api/v4`)

| Aufruf | Zweck |
|---|---|
| `GET /tags` | Alle Kategorien `{id, name, sortOrder, collapsed, type}`. `type` ist `favorites`, `other` oder `custom`. Die beiden eingebauten Einträge legt der Server beim ersten Abruf selbst an. |
| `POST /tags {name}` | Anlegen. 400 `error: name` heißt leer, über 250 Zeichen oder schon vergeben; 400 `error: limit` heißt mehr als 100. |
| `PUT /tags/{id} {name}`, `DELETE /tags/{id}` | Umbenennen bzw. Löschen. Geht nur bei `custom`. Beim Löschen verlieren alle Chats diese Kategorie. |
| `PUT /tags/reorder {orderedIds}` | Neue Reihenfolge aller Einträge, die eingebauten eingeschlossen. |
| `PUT /tags/{id}/collapsed {collapsed}` | Eingeklappt-Zustand, gilt für alle Geräte und auch für Talk-Web. |
| `POST /room/{token}/tags {tagIds}` | Ersetzt die Kategorien eines Chats (höchstens 20). Jeder Raum liefert `tagIds`. |

## Abschnitte (Kern: `room_sections(query, active_token, split)`)

**Zuordnung**, wie Talk-Web:
- Ein Favorit steht unter „Favoriten“.
- Jede eigene Kategorie enthält die Chats mit ihrer id in `tagIds`. Ein Chat kann in mehreren Kategorien stehen, auch zusätzlich zu „Favoriten“.
- Ein Chat, der kein Favorit ist und in keiner (vorhandenen) Kategorie steht, kommt unter „Chats“.
- Archivierte Chats fehlen, leere Abschnitte ebenso.

**`SectionView`:**
- `id`
- `kind`: `favorites`, `custom`, `other`, `groups` oder `direct`
- `title`: „Favoriten“, der Name, „Chats“, „Gruppen“ bzw. „Direktchats“
- `collapsed`
- `rooms`: die Zeilen, die angezeigt werden
- `total`: alle Chats des Abschnitts
- `unread`, `unreadQuiet`, `unreadMention`, `hasCall`: für den Kopf eines eingeklappten Abschnitts

**Eingeklappt:**
- `rooms` ist dann leer. Das gilt auch für den offenen Chat, für ungelesene Chats und für Chats mit laufendem Anruf.
- Damit nichts untergeht, rechnet der Kern für den Kopf über alle Chats des Abschnitts:
  | Feld | Inhalt |
  |---|---|
  | `unread` | Summe der ungelesenen Nachrichten. Chats auf „Aus“ zählen nicht. |
  | `unreadQuiet` | Kein Chat des Abschnitts zählt für den Badge am App-Symbol (`badge()`, docs/settings.md „Stummgeschaltete Chats“): Alle ungelesenen stehen auf „Nur Erwähnungen“ und haben keine Erwähnung. Die Zahl ist dann grau, wie an der Chat-Zeile. |
  | `unreadMention` | In einem Chat steht eine ungelesene Erwähnung. Chats auf „Aus“ zählen nicht. |
  | `hasCall` | In einem Chat läuft ein Anruf. |
- Die Felder sind auch bei einem offenen Abschnitt gefüllt. Angezeigt werden sie nur, solange er eingeklappt ist.
- `active_token` wird nicht mehr gebraucht. Der Parameter bleibt, damit sich die Bindungen nicht ändern.

**Suche:** `query` filtert nach Namen, ohne Rücksicht auf Groß- und Kleinschreibung und Akzente. Während der Suche ist nichts eingeklappt.
- Passt der Name eines archivierten Chats, steht er in einem eigenen Abschnitt „Archiviert“ (`kind: "archived"`) am Ende. Er hat keine Kategorie dahinter: kein Einklappen, kein Menü.
- Unter den Chats stehen die Treffer in den Nachrichten (docs/search.md).

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
  - wenn sich der Suchtext ändert.

**Aufrufe:**
- `toggle_section(id)`
  - Zeigt die Änderung sofort, speichert dann auf dem Server und nimmt sie bei einem Fehler zurück.
  - Gibt `null` oder einen deutschen Fehlertext zurück; den zeigt ihr als Toast.

## Gruppen und Direktchats

Eine Einstellung je Gerät, standardmäßig an (docs/settings.md, „Darstellung“). Die Plattform reicht ihren Wert als `split` an `room_sections` durch. Ändert er sich, ruft sie `room_sections` sofort neu auf.

**Mit `split`:**
- An der Stelle von „Chats“ stehen zwei Abschnitte, erst „Gruppen“, dann „Direktchats“.
- Die Kategorien-Seite am Desktop zeigt weiter „Chats“ als einen Eintrag. Verschiebt man ihn, wandern beide Abschnitte mit.
- „Favoriten“ und eigene Kategorien bleiben gemischt. Ein Favorit steht also nur unter „Favoriten“, nicht zusätzlich unter „Direktchats“.

**Zuordnung nach `roomType`:**
| Abschnitt | `id` und `kind` | Arten |
|---|---|---|
| „Direktchats“ | `direct` | Einzelchat (1), Talk-Updates (4), ehemaliger Einzelchat (5), Notizen an mich (6) |
| „Gruppen“ | `groups` | alle anderen (2 Gruppe, 3 öffentlich) |

**Regeln:**
- Für beide gelten die Regeln von oben: Leere Abschnitte fehlen, bei nur einem Abschnitt gibt es keinen Kopf, bei einer Suche ist nichts eingeklappt, und „Archiviert“ steht am Ende.
- Einklappen geht wie bei jedem Abschnitt mit `toggle_section("groups")` bzw. `toggle_section("direct")`.
  - Der Zustand gilt nur auf diesem Gerät und Konto (`collapsed_local`), denn Talk-Web kennt die Trennung nicht.
  - Der Eingeklappt-Zustand von „Chats“ auf dem Server spielt dann keine Rolle.

**Ohne `split`** sieht die Liste aus wie bisher. Die Bindungen nehmen `false` an, wenn der Wert fehlt.

## Sortierung

Eine Wahl für die ganze Liste, in den Einstellungen unter „Darstellung“, „Chatliste“ (docs/settings.md).

| Wert | Reihenfolge in jedem Abschnitt |
|---|---|
| `activity` „Letzte Aktivität“ (Standard) | Favoriten zuerst, dann nach der letzten Aktivität, wie bisher |
| `alphabetical` „Name (A–Z)“ | Nach dem Namen, ohne Rücksicht auf Groß- und Kleinschreibung und Akzente (dieselbe Faltung wie die Suche). Bei gleichem Namen entscheidet die letzte Aktivität. |

**Regeln:**
- Die Sortierung gilt in „Favoriten“, in eigenen Kategorien, in „Chats“ bzw. „Gruppen“ und „Direktchats“, in „Archiviert“ und auch während einer Suche.
- Die Reihenfolge der Abschnitte selbst ändert sich nicht.
- Eine Reihenfolge von Hand gibt es nicht: Talk kennt keine Position einzelner Chats.

**Speicherort:**
- Talk kennt die Benutzereinstellung `conversations_sort_order`. Der Server meldet sie in den Capabilities unter `config.conversations.sort-order`, und Talk-Web liest sie ebenfalls.
- Der Kern übernimmt den Wert bei jedem Start und jeder vollen Aktualisierung und merkt ihn sich im Cache. So startet die Liste auch offline richtig sortiert.
- Geschrieben wird über die Benutzer-Konfiguration von Nextcloud: `POST /ocs/v2.php/apps/provisioning_api/api/v1/config/users/spreed/conversations_sort_order` mit `configValue`. Ein ungültiger Wert gibt 400.
- Meldet der Server die Einstellung nicht, gilt die Wahl nur auf diesem Gerät und Konto.

**Aufrufe:**
- `sort_order()` gibt `activity` oder `alphabetical` zurück.
- `set_sort_order(order)` zeigt die Änderung sofort, speichert dann und nimmt sie bei einem Fehler zurück. Es gibt `null` oder einen deutschen Fehlertext zurück („Sortierung konnte nicht gespeichert werden“ oder „Keine Verbindung“).
- Beide Male kommt `RoomsChanged`. Die Plattform liest danach `sort_order()` neu, auch wenn die Änderung von einem anderen Gerät kam.

## Darstellung (alle Plattformen)

**Kopf:**
- Derselbe Stil wie bisher „Favoriten“/„Chats“.
- Rechts neben dem Titel steht ein kleines, gedämpftes Chevron:
  - Es zeigt „>“, wenn der Abschnitt eingeklappt ist.
  - Aufgeklappt ist es um 90° nach unten gedreht.
  - Die Drehung dauert etwa 150 ms; bei „Bewegung reduzieren“ entfällt sie.
- Der ganze Kopf ist anklickbar.
- Solange der Abschnitt offen ist, steht dort sonst nichts: keine Zähler, keine Linien.

**Kopf eines eingeklappten Abschnitts:** Am rechten Rand stehen dieselben Zeichen wie an einer Chat-Zeile, in dieser Reihenfolge und nur bei Bedarf:
| Zeichen | Wann |
|---|---|
| grüner Anrufpunkt | `hasCall` |
| „@“-Schild in `accent` | `unreadMention` |
| Zähler | `unread > 0`, über 99 als „99+“. In `accent`, bei `unreadQuiet` grau wie an einem leisen Chat. |

Es sind die vorhandenen Teile der Chat-Zeile, ohne neue Farben oder Größen.

**Zeilen:**
- Sie bleiben unverändert.
- Der Schlüssel einer Zeile ist `sectionId:token`, denn ein Chat kann mehrfach erscheinen.

**Barrierefreiheit:**
- Der Kopf ist ein Button mit `aria-expanded` bzw. dem Zustand „ausgeklappt“/„eingeklappt“.
- Das Label nennt den Titel, zum Beispiel „Arbeit, Kategorie“.
- Ist der Abschnitt eingeklappt, kommen hinzu, jeweils nur bei Bedarf:
  - „N Chats“ bzw. „1 Chat“ (immer)
  - „M ungelesene Nachrichten“ bzw. „1 ungelesene Nachricht“
  - „Erwähnung“
  - „laufender Anruf“
- Beispiel: „Arbeit, Kategorie, 4 Chats, 7 ungelesene Nachrichten, Erwähnung“.

## Desktop: Seite „Kategorien“

**Einstieg:**
- In den Einstellungen steht die Zeile „Kategorien“, nur bei `categoriesSupported()` (docs/settings.md).
- Die Seite steht rechts neben der Navigation der Einstellungen, wie jede andere Seite dort.

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
- Abschnitte prüfen: `talk-cli -a a sections [suche] [--split]`
- Sortierung lesen und setzen: `talk-cli -a a sort [activity|alphabetical]`. Am Ende wieder `activity` setzen.
- Gegenseite und Vorbereitung: `talk-cli -a a category list|create|rename|delete|move|collapse|set …`
- Bot-Tests laufen unter `bot-lock.sh`.

**Wenn ihr einklappt:**
- Den Zustand von „Favoriten“ und „Chats“ am Ende wieder aufklappen.
- Er liegt auf dem Server und gilt auch in Talk-Web.
