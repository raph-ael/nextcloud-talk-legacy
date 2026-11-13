# Chat – gemeinsame UI-Regeln der Plattformen

Verbindlich für Android, iOS, Desktop Linux und macOS. Der Kern liefert die Daten (`RoomView`, Nachrichten), die Regeln hier betreffen nur die Oberfläche.

## Zum Ende springen

Ein runder, schwebender Knopf unten rechts in der Nachrichtenliste. Er erscheint, sobald man zurückscrollt, und bringt einen zur neuesten Nachricht.

**Sichtbarkeit**
- Der Knopf erscheint, sobald die Liste mehr als eine halbe Bildschirmhöhe vom Ende entfernt ist. Wer nur leicht scrollt, sieht ihn also nicht.
- Er blendet in etwa 150 ms ein und aus (Fade und Scale), statt hart zu erscheinen.
- Die Lesemarke folgt weiter der engen Schwelle „unten“ (etwa 40 px/pt bzw. die ersten 1–2 Einträge). Beide Schwellen sind voneinander unabhängig.

**Aussehen**
- Der Knopf ist rund: 40 dp/pt auf Mobilgeräten, 36 px am Desktop.
- Hintergrund ist die schwebende Oberfläche (`floating`) mit leichtem Schatten, darauf ein Chevron nach unten in `muted`.
- Abstand: 12–16 dp/pt/px zum rechten Rand und über dem Eingabefeld.
- Eigener Chevron bzw. Systemicon, keine fremden Assets.

**Zähler**
- Ist `room.unreadMessages > 0`, zeigt der Knopf oben ein kleines Abzeichen in der Akzentfarbe mit der Zahl, ab 100 „99+“.
- Das stimmt von selbst: Solange man nicht unten ist, wird keine Lesemarke gesetzt, also zählt `unreadMessages` genau die neuen Nachrichten.

**Tippen**
- Ist das Ende nah (weniger als etwa 15 Einträge), scrollt die Liste weich dorthin.
- Ist es weit weg, springt die Liste erst ohne Animation in die Nähe des Endes (etwa 10 Einträge davor bzw. eine Bildschirmhöhe) und scrollt den Rest weich.
- Danach setzt die bestehende Logik „unten → `markVisibleRead`“ die Lesemarke, und das Abzeichen verschwindet.

**Barrierefreiheit**
- Bezeichnung: „Zum Ende springen“, mit Zähler „Zum Ende springen, 3 neue Nachrichten“ (bei einer: „1 neue Nachricht“).
- Am Desktop gibt es zusätzlich `title` als Tooltip.

**Nicht im Umfang:** ein eigener Knopf für @-Erwähnungen. Zur ersten ungelesenen Nachricht springt der Chat schon beim Öffnen (siehe „Ungelesen-Linie“).

## Datum und Ungelesen-Linie (wie Signal)

Das Muster stammt von Signal (Desktop, Android, iOS). Übernommen wird nur das Muster, kein Code und keine Texte. Der Kern ändert sich nicht: `RoomView` hat schon `lastReadMessage`/`unreadMessages`, Nachrichten haben `id`, `timestamp`, Absender und `kind`.

### Tagestrenner
**Aussehen:**
- zentrierter Text, 13 px/sp/pt, Medium, Farbe `muted`
- kein Hintergrund, keine Linien (die frühere Pille entfällt)
- Abstand 16 oben, 8 unten

**Beschriftung** (`dayLabel`, auf allen Plattformen gleich):

| Wann | Text |
|---|---|
| heute | „Heute“ |
| gestern | „Gestern“ |
| bis 6 Monate zurück | „Mi., 11. Nov.“ (Wochentag kurz, Tag, Monat kurz) |
| älter | „11. Nov. 2025“ |

**Wo:** über der ersten Nachricht und bei jedem Tageswechsel, wie bisher.

**Barrierefreiheit:** Der Trenner ist eine Überschrift (Android `heading()`, iOS `.isHeader`, Desktop `role="heading"`). So kann man mit dem Screenreader von Tag zu Tag springen.

### Datum beim Scrollen
**Aussehen:** eine kleine schwebende Pille oben mittig, 8–10 unter der Kopfleiste.
- Oberfläche `floating` mit leichtem Schatten, wie „Zum Ende springen“.
- Text 12, `muted`, Abstand innen 10×4, ganz rund.

**Inhalt:** das Datum der obersten sichtbaren Nachricht, Text wie beim Tagestrenner.

**Wann:**
- Sie erscheint beim Scrollen, aber nicht, solange man unten ist (die enge Schwelle der Lesemarke). Neue Nachrichten und Tippen lösen sie also nicht aus.
- **Desktop:** sie verschwindet 3 s nach dem letzten Scroll-Ereignis und blendet in 250 ms aus.
- **Android/iOS:** sie blendet beim Scrollen in 100 ms ein und 0,4 s nach dem Ende des Scrollens in 100 ms aus. Scrollt man vorher weiter, bleibt sie.
- Bei „Bewegung reduzieren“ ohne Animation.

Sie ist reine Deko: nicht anklickbar und für Screenreader verborgen.

### Ungelesen-Linie
**Aussehen:**
- eine Linie über die ganze Breite der Nachrichtenliste, 1 px/dp/pt, Farbe `muted`, also kräftiger als die übrigen Trenner
- 3 darunter zentriert der Text, 12–13, fett, Farbe `onSurface`
- Abstand 24 oben, 16 unten
- gleich auf allen Plattformen

**Text:** „1 ungelesene Nachricht“ bzw. „N ungelesene Nachrichten“.

**Festlegen beim Öffnen:**
- Die Plattform merkt sich beim Öffnen immer (nicht nur bei `unreadMessages > 0`):
  - `unreadFrom = room.lastReadMessage`
  - `unreadBase = room.unreadMessages`
- Das geschieht im `openRoom` (Store, SessionManager, SessionStore), **bevor** irgendetwas die Lesemarke setzt.
- Es ist reiner UI-Zustand, nur für den gerade offenen Chat.
- Ist `unreadFrom <= 0`, gibt es keine Linie.

**Zahl und Platz werden aus den geladenen Nachrichten abgeleitet**, nicht hochgezählt:
- „Zählt“ heißt: `id > unreadFrom`, nicht von einem selbst, keine Systemnachricht.
- **Platz:** über der ersten geladenen Nachricht, die zählt. Gibt es (noch) keine, gibt es (noch) keine Linie.
- **Zahl:** `max(unreadBase, Anzahl der geladenen Nachrichten, die zählen)`.

So stimmt die Zahl auch, wenn:
- die Chatliste beim Klick noch den alten Stand hatte (die Nachricht kam vor weniger als ein paar Sekunden, `unreadMessages` war noch 0),
- dieselbe Nachricht über die Seite und über ein Ereignis zweimal ankommt.

**Bleibt stehen:** Die Linie bleibt, auch wenn die Lesemarke inzwischen gesetzt ist. `unreadFrom` ändert sich dadurch nicht.

**Neue Nachrichten, während der Chat offen ist:**
- Gibt es die Linie, zählen sie automatisch mit (sie sind geladen und haben eine höhere id).
- Gibt es keine, und man ist nicht unten oder das Fenster hat keinen Fokus, entsteht eine neue Linie: `unreadFrom` = die bisher neueste Nachricht, `unreadBase` = 0.

**Weg ist sie:**
- sobald man selbst etwas sendet (Text, Datei, Sprachnachricht, Umfrage …),
- wenn man den Chat verlässt oder einen anderen öffnet.

Beim nächsten Öffnen ist alles gelesen, also gibt es keine Linie mehr.

**Sprachnachrichten** zählen ganz normal mit. Einen eigenen „nicht angehört“-Punkt gibt es nicht.

**Barrierefreiheit:**
- Der Text wird vorgelesen.
- Desktop: `role="separator"` mit `aria-label` = Text.

### Öffnen an der Linie
**Wohin:**
- Gibt es eine Linie, öffnet der Chat so, dass sie im oberen Viertel des Sichtbereichs steht.
- Weiter als ganz unten geht es nie. Passen alle neuen Nachrichten auf den Schirm, bleibt die Liste unten wie bisher.

**Erste ungelesene Nachricht noch nicht geladen** (mehr als eine Seite, also über 50):
- Die Plattform ruft `loadOlder` auf, bis sie da ist, höchstens 10 Seiten.
- Reicht das nicht, steht die älteste geladene Nachricht oben. Die Linie erscheint, sobald man weiter hochscrollt.

**Lesemarke:**
- Es bleibt bei „unten → `markVisibleRead`“. Wer an der Linie öffnet, liest erst, wenn er unten ankommt.
- Bis dahin zeigt „Zum Ende springen“ die Zahl wie bisher.

### Test
Nur mit Bots unter `bot-lock.sh`, nur in `CALL_TEST_ROOM_TOKEN`/`TEST_ROOM_TOKEN`, keine echten Chats.

1. Bot B sendet drei Nachrichten: `talk-cli -a b send <CALL_TEST_ROOM_TOKEN> "Test 1"` …
2. Bot A öffnet den Chat: Die Linie „3 ungelesene Nachrichten“ steht über „Test 1“.
3. Bot B sendet noch eine: Die Zahl wird 4. Bot A sendet selbst: Die Linie ist weg.
4. Verlassen und wieder öffnen: keine Linie.
5. 60 Nachrichten von Bot B: Der Chat öffnet an der Linie über der ersten. Die Lesemarke (`talk-cli -a a rooms --json`, `lastReadMessage`) bleibt alt, bis man unten ist.
6. Hochscrollen über Tageswechsel: Die Trenner sind reiner Text, die Pille zeigt das passende Datum und verschwindet danach. Ganz unten erscheint sie nicht.

## Reaktionen

Schlank wie bei Signal: **eine Reaktion pro Person und Nachricht.** Ein anderes Emoji ersetzt die eigene Reaktion, dasselbe noch mal nimmt sie zurück. Mehrfach-Reaktionen aus Talk-Web werden weiter korrekt angezeigt (Zähler pro Emoji), und beim nächsten Wechsel räumt der Kern alle eigenen ab.

**Kern-Vertrag**
- `setReaction(token, messageId, emoji | null)`: Der Kern setzt die Signal-Regel selbst um. Die Plattform reicht nur das angetippte Emoji durch, oder `null` zum Entfernen.
  - Die Änderung erscheint sofort als `MessagesUpdated`, die Antwort des Servers ersetzt sie gleich danach.
  - Das Ergebnis ist ein kurzer deutscher Fehlertext oder `null`. Bei einem Fehler ist die Nachricht schon auf den Stand des Servers zurückgesetzt, die Plattform zeigt nur den Text als Hinweis (Toast).
  - Änderungen laufen nacheinander. Schnelles Umentscheiden ist also unkritisch, die Plattform muss nichts sperren.
  - Es gibt keine Outbox: Offline kommt sofort „Keine Verbindung – Reaktion nicht gesendet“.
- `reactions(token, messageId) → ReactionView[]` (`emoji`, `actorType`, `actorId`, `name`, `isOwn`, `timestamp`): wer reagiert hat, die eigenen zuerst, dann die neuesten.
- `RoomView.canReact`: Darf der User reagieren? Das ist Talk-Berechtigung 256, auf älteren Servern 128, und nie in schreibgeschützten Räumen.
- **Reagierbar ist** eine Nachricht mit `kind == "text"` (auch Datei und Sprachnachricht), die zugestellt ist, also nicht in der Outbox steht. System- und gelöschte Nachrichten nicht.

**Schnellleiste**
- Sechs Emojis wie bei Signal: ❤️ 👍 👎 😂 😮 😢, dazu ein **＋** für die volle Auswahl.
- Die eigene, aktive Reaktion ist in der Leiste hervorgehoben (Kreis in `accent` mit geringer Deckkraft). Ein erneutes Tippen nimmt sie zurück.
- Ist die eigene Reaktion keines der sechs, steht sie hervorgehoben an letzter Stelle statt 😢. Das gilt nur für diese Anzeige. Eine anpassbare Leiste gibt es nicht.
- Nach der Auswahl schließen Leiste und Menü sofort.
- Ohne `canReact`, bei ausstehenden, System- und gelöschten Nachrichten gibt es keine Leiste. Das übrige Menü bleibt.

**Pillen unter der Blase**
- Wie bisher: Emoji und ab 2 die Zahl, die eigene ist hervorgehoben.
- Tippen bzw. Klicken öffnet „Wer hat reagiert“, die Pillen schalten also nicht um.
- Eine neu gesetzte Pille „ploppt“ kurz (Skalierung 0,6 → 1, etwa 150 ms). Mehr Animation gibt es nicht.

**„Wer hat reagiert“**
- Ein Bottom-Sheet auf Mobilgeräten, ein kleines Modal am Desktop.
- Reiter: „Alle · N“ und dann je Emoji mit Anzahl, sortiert nach Anzahl.
- Jede Zeile zeigt Avatar, Name und rechts das Emoji.
- Die eigene Zeile heißt „Du“ mit dem Untertitel „Tippen zum Entfernen“. Tippen ruft `setReaction(…, null)` auf und schließt das Fenster, wenn danach nichts mehr übrig ist.
- Die Daten kommen bei jedem Öffnen frisch per `reactions()`. Während des Ladens erscheint ein schlichter Platzhalter.

**Pro Plattform**
- **Desktop:**
  - Hover-Aktion „Reagieren“ (Smiley) vor „Antworten“ öffnet die Leiste als Popover über der Blase. ＋ öffnet dort die vorhandene Emoji-Auswahl.
  - Das Kontextmenü (Rechtsklick bzw. „Mehr“) hat die Leiste als oberste Zeile.
  - Beim Überfahren einer Pille zeigt ein Tooltip die Namen. Sie werden einmal geholt und pro Nachricht und Zählerstand gemerkt.
- **Android:**
  - Langer Druck zeigt die Leiste schwebend über der Blase, das bestehende Menü bleibt darunter. Beide schließen gemeinsam.
  - ＋ öffnet ein Bottom-Sheet mit der Emoji-Auswahl (`emoji2-emojipicker`).
  - Kurzes haptisches Feedback beim Setzen.
- **iOS:**
  - Langer Druck öffnet ein eigenes Overlay statt des Aktionsblatts: Der Hintergrund wird abgedunkelt, die Nachricht bleibt hervorgehoben, darüber steht die Leiste, darunter die bisherigen Aktionen.
  - ＋ öffnet die Emoji-Tastatur. Das erste Emoji wird übernommen, dann schließt sie.
  - Leichtes haptisches Feedback beim Setzen.

**Barrierefreiheit:** Die Knöpfe der Leiste heißen „Mit ❤️ reagieren“ und „Weitere Reaktionen“, die Pillen „❤️ 2, Reaktionen anzeigen“.
