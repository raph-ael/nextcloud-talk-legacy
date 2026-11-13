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

**Nicht im Umfang:** „Zur ersten ungelesenen Nachricht“ und ein eigener Knopf für @-Erwähnungen.

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
