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
