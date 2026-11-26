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

**Fenster in den Verlauf:** Öffnet der Chat an einer alten Nachricht (Suchtreffer, Zitat), ist der Knopf immer da und führt zu den neuesten Nachrichten zurück (docs/search.md „Chat an einer Nachricht öffnen“).

**Erwähnungen:** Über dem Knopf steht bei ungesehenen Erwähnungen der „@“-Knopf (docs/mentions.md „„@“-Knopf im Chat“).

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
- **Fremde Reaktionen im offenen Chat** kommen nach höchstens etwa 6 s als `MessagesUpdated`, im Schnitt nach 2 bis 3 s.
  - Der Grund für die Wartezeit liegt am Server (Talk 24): Er merkt sich je Unterhaltung 30 s lang, dass es nach dem zuletzt gefragten Stand nichts Neues gibt, und beantwortet die Warteabfrage des Chats aus diesem Merker. Eine neue Nachricht oder eine entfernte Reaktion löscht ihn, eine neu gesetzte Reaktion nicht. Sie käme sonst erst nach bis zu 30 s (gemessen: 27 s).
  - Der Kern fragt deshalb neben der Warteabfrage alle 5 s mit dem Stand davor nach, an diesem Merker vorbei. Die 5 s zählen ab dem Ende der letzten Nachfrage, ihre Laufzeit kommt also dazu (auf einem langsamen Telefon gemessen: 1,5 bis 6 s). Das sind knapp 12 kleine Anfragen pro Minute je offenem Chat. Die Plattform muss nichts tun.
  - `networkChanged()` und ein Chat-Push lösen die Nachfrage sofort aus. Nach `handlePush` für eine Reaktion steht sie also schon im Chat bzw. im Zwischenspeicher.
  - Gemessen im Testraum (8 Läufe): 0,9 bis 4,8 s, im Schnitt 2,5 s. Entfernen: 0,5 s.
- **Reagierbar ist** eine Nachricht mit `kind == "text"` (auch Datei und Sprachnachricht), die zugestellt ist, also nicht in der Outbox steht. System- und gelöschte Nachrichten nicht.

**Schnellleiste**
- Sechs Emojis wie bei Signal: ❤️ 👍 👎 😂 😮 😢, dazu ein **＋** für die volle Auswahl.
- Die eigene, aktive Reaktion ist in der Leiste hervorgehoben (Kreis in `accent` mit geringer Deckkraft). Ein erneutes Tippen nimmt sie zurück.
- Ist die eigene Reaktion keines der sechs, steht sie hervorgehoben an letzter Stelle statt 😢. Das gilt nur für diese Anzeige. Eine anpassbare Leiste gibt es nicht.
- Nach der Auswahl schließen Leiste und Menü sofort.
- Ohne `canReact`, bei ausstehenden, System- und gelöschten Nachrichten gibt es keine Leiste. Das übrige Menü bleibt.

**Pillen unter der Blase**
- Emoji und ab 2 die Zahl, die eigene ist hervorgehoben.
- Die Pillen liegen unter der Blase und ragen in ihren unteren Rand hinein, auf allen Plattformen. In der Blase stehen sie nie.
- Tippen bzw. Klicken öffnet „Wer hat reagiert“, die Pillen schalten also nicht um.
- Eine neu gesetzte Pille „ploppt“ kurz (Skalierung 0,6 → 1, etwa 150 ms). Mehr Animation gibt es nicht.
- Farben: Fläche `reaction` (hell `#E9E9E9`, dunkel `#1B1B1B`), die eigene `reactionMine` (hell `#B9B9B9`, dunkel `#5E5E5E`). Der Rand hat die Farbe des Chat-Hintergrunds, so hebt sich die Pille von der Blase ab.
- Die Uhrzeit bleibt, wo sie ohne Reaktion stünde: bei Bildern auf dem Bild, bei Sprachnachrichten in der Fußzeile des Players.

| | Desktop | Android, iOS (wie Signal Mobile) |
|---|---|---|
| Seite | fremde Nachricht rechts an der Blase, eigene links | fremde Nachricht links an der Blase, eigene rechts |
| Überlappung | 6 px | 4 dp/pt |
| Einzug vom Blasenrand | 8 px | 8 dp/pt, damit die Pille nicht auf der runden Ecke sitzt |
| Pille | 22 px hoch, Rand 1 px, Emoji 14, Zahl 11 halbfett | 26 hoch, Rand 1,5, Emoji 15, Zahl 12 halbfett |
| Abstände | 2 px zwischen den Pillen, 4 px zwischen Emoji und Zahl | 2 zwischen den Pillen, 3 zwischen Emoji und Zahl, 8 vom Außenrand der Pille zum Inhalt |
| Viele Emojis | alle, die Reihe bricht um | höchstens drei Pillen |

Auf Mobilgeräten gilt bei mehr als drei verschiedenen Emojis:
- Es stehen die zwei häufigsten und eine Pille „+N“. N ist die Zahl der übrigen Reaktionen, nicht der übrigen Emojis.
- Ist die eigene Reaktion unter den übrigen, ist „+N“ hervorgehoben.
- „Wer hat reagiert“ zeigt immer alle.
- Die Reihe darf über die Blase hinausragen (kurze Nachricht, drei Pillen). Sie wächst von ihrer Seite der Blase zur Bildschirmmitte.
- Die Zeile wird durch die Pillen um 22 höher. Der Abstand zur nächsten Nachricht bleibt.
- Beim langen Druck bleiben die Pillen der Nachricht sichtbar, das Aktionsmenü steht unter ihnen. Kürzt iOS eine sehr lange Nachricht im Overlay, fallen sie wie die Uhrzeit mit weg.

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

**Barrierefreiheit:** Die Knöpfe der Leiste heißen „Mit ❤️ reagieren“ und „Weitere Reaktionen“, die Pillen „❤️ 2, Reaktionen anzeigen“, die Sammelpille „3 weitere, Reaktionen anzeigen“.

## Bearbeiten und Löschen

Eigene Nachrichten lassen sich bearbeiten und löschen. Moderatoren einer Gruppe können auch fremde Nachrichten löschen. Gelöscht wird immer für alle; „nur für mich“ kennt Talk nicht.

**Was der Server erlaubt**

| | Bearbeiten | Löschen |
|---|---|---|
| Wer | eigene Nachrichten | eigene; Moderatoren in Gruppen auch fremde, nie im 1:1-Chat |
| Wie lange | 24 Stunden, in „Notiz an mich“ unbegrenzt | unbegrenzt |
| Was | Text, Bildunterschrift einer Datei | Text, Datei, Sprachnachricht, Standort |
| Nicht | Sprachnachricht, Umfrage, Standort; leerer Text | Systemnachrichten |

Bei einer Datei endet nur die Freigabe für den Chat. Die Datei selbst bleibt beim Besitzer.

**Kern-Vertrag**
- `RoomView.canChat`: Der User darf hier schreiben. `RoomView.canDeleteOthers`: Er darf fremde Nachrichten löschen.
- `MessageView.deletable`, `editable` und `editUntil` (Unix-Sekunden, leer heißt unbegrenzt) sagen, was mit der Nachricht geht. Die Plattform rechnet nur noch, und zwar beim Öffnen des Menüs:
  - **Löschen:** `deletable` und (`isOwn` mit `canChat` oder fremd mit `canDeleteOthers`).
  - **Bearbeiten:** `editable`, `canChat` und (`editUntil` leer oder jetzt < `editUntil`).
  - Beides nur für zugestellte Nachrichten. Im Postausgang bleibt „Löschen“ das lokale Verwerfen (`discard`), ohne Nachfrage.
- `editSource(token, messageId) → { text, mentions } | null`: der Text fürs Eingabefeld und wer mit den Namen darin gemeint ist (docs/mentions.md „Im Eingabefeld“). Ohne die `mentions` wären Erwähnungen nach dem Speichern nur noch Text. Bei `null` nimmt die Plattform `MessageView.text` und `MessageView.mentions`.
- `editMessage(token, messageId, text, mentions)` und `deleteMessage(token, messageId)`:
  - Das Ergebnis ist ein kurzer deutscher Fehlertext oder `null`. Die Plattform zeigt den Text als Hinweis (Toast).
  - Nichts erscheint vorab. Die neue Fassung kommt als `MessagesUpdated`, bevor der Aufruf zurückkehrt.
  - Derselbe Text noch einmal ist ein Erfolg ohne Abruf.
  - Es gibt keine Outbox: Offline kommt „Keine Verbindung – Nachricht nicht geändert“ bzw. „… nicht gelöscht“.
- **Was der Kern dazutut,** auch wenn jemand anderes ändert:
  - Eine geänderte Antwort behält ihr Zitat und die eigene Reaktion. Der Server schickt beides nicht mit.
  - Antworten auf eine geänderte oder gelöschte Nachricht kommen im selben `MessagesUpdated` mit dem neuen Zitat.
  - Ist es die letzte Nachricht des Chats, kommt `RoomsChanged` mit der neuen Vorschau.
- Die Anzeige bleibt wie bisher: „bearbeitet ·“ in der Meta-Zeile, „Diese Nachricht wurde gelöscht.“ in der Blase, „Nachricht gelöscht“ im Zitat und in der Chatliste. Die Ausnahme steht unter „Stille Nachrichten“.
- Eine gelöschte Antwort kommt ohne `replyTo`: Sie zeigt kein Zitat mehr.

**Menü der Nachricht**
- „Bearbeiten“ steht direkt nach „Antworten“.
- „Löschen“ steht an letzter Stelle und ist als gefährlich markiert (rot).

**Bearbeiten**
- Über dem Eingabefeld erscheint die Leiste, die es fürs Antworten gibt:
  - Titel „Nachricht bearbeiten“ in der Akzentfarbe, davor ein Stift.
  - Darunter eine Zeile mit dem bisherigen Text, bei einer Datei ohne Bildunterschrift der Dateiname.
  - Rechts ein ✕, „Bearbeiten abbrechen“.
- Das Eingabefeld enthält den Text aus `editSource()`, die Marke steht am Ende, das Feld hat den Fokus.
- Der Senden-Knopf wird zum Haken, „Änderung speichern“. Er ist aus, solange der Text leer ist oder dem Ausgangstext gleicht (beide ohne Leerraum am Rand).
- Anhang und Mikrofon sind solange ausgeblendet.
- Ein angefangener Entwurf und eine angefangene Antwort bleiben erhalten. Nach dem Speichern oder Abbrechen stehen sie wieder da. Der bearbeitete Text wird nie zum Entwurf.
- Während des Speicherns ist der Haken aus. Bei Erfolg endet der Modus.
- Bei einem Fehler bleibt der Modus offen und der Text stehen, dazu kommt der Hinweis.
- Der Modus endet auch, wenn man den Chat verlässt oder die Nachricht inzwischen gelöscht wurde.

**Löschen**
- Immer mit Nachfrage: „Nachricht löschen?“, Knöpfe „Abbrechen“ und „Löschen“ (gefährlich).
- Text bei einer eigenen Nachricht: „Sie wird für alle im Chat gelöscht.“
- Text bei einer fremden: „Die Nachricht von {Name} wird für alle im Chat gelöscht.“
- Hat die Nachricht eine Datei, folgt: „Die Datei selbst bleibt erhalten.“
- Danach zeigt die Blase „Diese Nachricht wurde gelöscht.“ Sie kommt über `MessagesUpdated`, die Plattform entfernt nichts selbst.

**Pro Plattform**
- **Desktop:**
  - Pfeil nach oben im leeren Eingabefeld bearbeitet die letzte eigene Nachricht, die sich noch bearbeiten lässt.
  - Esc bricht ab, Enter speichert.
- **Android und iOS:** Die Tastatur öffnet sich mit dem Modus. Die Zurück-Geste bricht erst das Bearbeiten ab.

**Barrierefreiheit:** Die Leiste heißt „Nachricht bearbeiten“, ihre Knöpfe „Bearbeiten abbrechen“ und „Änderung speichern“.

**Nicht im Umfang:** Vormerken ohne Verbindung, fremde Nachrichten bearbeiten, eine Bildunterschrift ganz entfernen (der Server lehnt leeren Text ab), ein Verlauf früherer Fassungen.

### Test
Im Testraum mit Bot A und Bot B, unter `bot-lock.sh`. Gegenstelle ist `talk-cli` (`edit`, `delete-message`, `changes`).

1. Eigene Nachricht bearbeiten: neuer Text und „bearbeitet ·“, bei der Gegenstelle ebenso.
2. Entwurf im Feld, dann bearbeiten und speichern: Der Entwurf steht wieder da.
3. Unverändert oder leer: Der Haken ist aus.
4. Eigene Antwort bearbeiten: Zitat und eigene Reaktion bleiben.
5. Die Gegenstelle bearbeitet eine Nachricht, auf die geantwortet wurde: Das Zitat in der Antwort ist neu.
6. Nachricht mit Erwähnung bearbeiten: Die Erwähnung bleibt eine Erwähnung.
7. Eigene Nachricht löschen: Nachfrage, danach „Diese Nachricht wurde gelöscht.“, die Chatliste zeigt „Nachricht gelöscht“.
8. Als Moderator eine fremde Nachricht in der Gruppe löschen. Im 1:1-Chat fehlt „Löschen“ bei fremden Nachrichten.
9. Nachricht älter als 24 Stunden: „Bearbeiten“ fehlt, „Löschen“ ist da.
10. Sprachnachricht: nur „Löschen“. Datei: „Bearbeiten“ ändert die Bildunterschrift.
11. Ohne Netz bearbeiten: Hinweis „Keine Verbindung – Nachricht nicht geändert“, der Text bleibt im Feld.
12. Desktop: Pfeil nach oben, Esc, Enter.

## Stille Nachrichten

Talk kennt Nachrichten, die ohne Benachrichtigung gesendet werden (`silent`). KI-Agenten nutzen das für ihren Arbeitsstand: Sie senden eine stille Nachricht wie „Arbeitet …“, schreiben sie im Sekundentakt fort und löschen sie, sobald die Antwort als eigene Nachricht da ist. Im Chat soll das wie ein Status aussehen und keine Spur hinterlassen.

Die Regeln gelten für jede stille Nachricht eines anderen, auch von Menschen. Der Kern erkennt keine Agenten. Eigene stille Nachrichten sehen aus wie alle eigenen.

| Punkt | Regel |
|---|---|
| Aussehen | Der Text ist kleiner (13 statt 14 bzw. 15) und in `muted`. Die Blase und in der kompakten Ansicht die Zeile bleiben wie bei jeder Nachricht. |
| Ein Emoji allein | bleibt klein, ohne die große Darstellung. |
| „bearbeitet“ | entfällt. Das Fortschreiben ist der Zweck der Nachricht. |
| Fortschreiben | Die Nachricht ändert sich an Ort und Stelle. Steht die Liste am Ende, bleibt sie dort, auch wenn die Nachricht wächst. |
| Gelöscht | Die Nachricht verschwindet ganz, ohne „Diese Nachricht wurde gelöscht.“ Das gilt live und beim nächsten Öffnen. |
| Zitat | Eine Antwort auf eine so gelöschte Nachricht zeigt wie bisher „Nachricht gelöscht“. |
| Chatliste | unverändert: Die Vorschau zeigt die stille Nachricht, solange sie die letzte ist. |
| Benachrichtigung | keine, wie bisher (docs/notifications.md). |

**Kern-Vertrag**
- `MessageView.quiet`: Jemand anderes hat die Nachricht still gesendet. Die Plattform zeichnet sie gedämpft und nie groß. `edited` ist bei `quiet` immer `false`.
- `MessagesRemoved { token, ids }`: Diese Nachrichten gehören nicht mehr in den Chat. Die Plattform nimmt sie aus der Liste, aus zurückgehaltenen Nachrichten eines Fensters und aus den offenen Erwähnungen. Für sie kommt kein `MessagesUpdated` mit `kind == "deleted"`.
- Seiten (`openChat`, `cachedChat`, ältere Seiten) enthalten solche Nachrichten nicht.
- Andere gelöschte Nachrichten kommen wie bisher als `MessagesUpdated` (oben „Bearbeiten und Löschen“).

### Test
Gegen `talk-demo`: `curl -d token=lena -d text=Arbeitet -d silent=1 http://127.0.0.1:8099/demo/incoming`, danach `/demo/edit` und `/demo/delete` mit der `id` aus der Antwort.

1. Die stille Nachricht erscheint gedämpft und klein, ohne Benachrichtigung.
2. Bearbeiten: Der Text wechselt ohne Flackern, ohne „bearbeitet“, die Liste bleibt am Ende.
3. Eine normale Nachricht danach sieht normal aus.
4. Löschen: Die stille Nachricht ist weg, es bleibt keine Blase. Nach Schließen und Öffnen des Chats ebenso.
5. Eine normale Nachricht löschen: „Diese Nachricht wurde gelöscht.“ bleibt wie bisher.
6. Eine Nachricht, die per Bearbeiten wächst (nicht still): Die Liste bleibt am Ende, „bearbeitet“ steht da.

## Eingabefeld vergrößern (Desktop)

Am Desktop wächst das Eingabefeld mit dem Text bis 72 px, danach scrollt es. Für lange Texte lässt es sich vergrößern, nach dem Muster von Signal Desktop: mit einer kleinen Lasche über dem Eingabebereich. Android und iOS haben das nicht, dort wächst das Feld mit dem Text.

| Punkt | Regel |
|---|---|
| Lasche | 48 × 24 px, mittig an der Oberkante des Eingabebereichs, 18 px ragen darüber hinaus in die Nachrichtenliste. Fläche in der Hintergrundfarbe (`bg`), oben 12 px Rundung, unten gerade. Darin ein flacher Pfeil nach oben in `muted`, als eigene gezeichnete Linie, kein fremdes Asset. |
| Sichtbar | Die Lasche ist unsichtbar. Sie blendet in 200 ms ein, solange die Maus über dem Eingabebereich ist oder die Lasche den Tastaturfokus hat. Ohne Maus (Touch-Bildschirm) ist sie immer sichtbar. Mit der Textmenge hat das nichts zu tun. |
| Groß | Das Feld ist fest 212 px hoch, in niedrigen Fenstern höchstens 40 % der Fensterhöhe. Längerer Text scrollt im Feld. Der Pfeil zeigt nach unten. |
| Klein | Das Feld wächst mit dem Text bis 72 px. Der Pfeil zeigt nach oben. |
| Zurück auf klein | Nach dem Senden (auch einer Sprachnachricht), nach dem Speichern oder Abbrechen einer Bearbeitung und beim Wechsel des Chats. Der Beginn einer Bearbeitung lässt den Zustand, ebenso das Abbrechen einer Aufnahme. Der Zustand wird nicht gespeichert. |
| Wo | Beim Schreiben und beim Bearbeiten einer Nachricht. Nicht bei laufender Sprachaufnahme und nicht bei einem Sprach-Entwurf: Dort gibt es kein Textfeld. |
| Fokus | Ein Klick auf die Lasche lässt Fokus und Schreibmarke im Feld. Per Tastatur umgeschaltet bleibt der Fokus auf der Lasche. Nach dem Umschalten scrollt das Feld so, dass die Schreibmarke sichtbar ist. |
| Erwähnungen | Die Marken hinter dem Text und die Liste nach „@“ (docs/mentions.md) sitzen im großen Feld wie im kleinen. |

**Barrierefreiheit**
- Bezeichnung und Tooltip: „Eingabefeld vergrößern“, im großen Zustand „Eingabefeld verkleinern“.
- Die Lasche ist ein Knopf und per Tab erreichbar. Leertaste und Eingabe schalten um.
- Der Fokusring liegt in der Akzentfarbe 2 px nach innen.

### Test
1. Ohne Maus über dem Eingabebereich ist die Lasche unsichtbar, mit Maus sichtbar und mittig, in hell und dunkel.
2. Langen Text tippen, Klick auf die Lasche: Das Feld ist 212 px hoch, der Pfeil zeigt nach unten, die Schreibmarke bleibt im Feld. Zweiter Klick: wieder klein.
3. Nach dem Senden, nach einem Chatwechsel, nach Speichern und nach Abbrechen einer Bearbeitung ist das Feld klein.
4. Eine lange Nachricht bearbeiten und vergrößern.
5. Im großen Feld „@“ tippen: Die Liste steht über dem Feld, die Marke liegt genau hinter dem Namen, auch nach dem Scrollen im Feld.
6. Mit Antwortleiste und angehängter Datei sitzt die Lasche an der Oberkante des Eingabebereichs und verdeckt nichts Bedienbares.
7. Während einer Sprachaufnahme und bei einem Sprach-Entwurf gibt es keine Lasche.
8. Fenster 380 breit und 480 hoch: Die Nachrichtenliste bleibt sichtbar, die Lasche stößt nicht an „Zum Ende springen“.
9. Tab erreicht die Lasche, sie wird sichtbar, Leertaste und Eingabe schalten um.

## Kopieren und Kontextmenü (Desktop)

Am Desktop gibt es zwei Arten von Kontextmenü: das eigene Menü der App an einer Nachricht und an einem Chat in der Liste (`docs/settings.md`), und überall sonst ein Menü des Betriebssystems. Android und iOS sind davon nicht berührt.

### Menü der Nachricht

Das Menü (Rechtsklick auf die Nachricht oder „Mehr“) richtet sich nach der Markierung und nach dem, was unter dem Zeiger liegt.

| Fall | Eintrag |
|---|---|
| Nichts markiert | „Text kopieren“: der ganze Text der Nachricht. |
| Eine Markierung berührt die Nachricht | „Auswahl kopieren“ statt „Text kopieren“: nur der markierte Text. |
| Der Zeiger steht auf einem Link im Text | zusätzlich „Link kopieren“, direkt über dem Kopier-Eintrag. |
| Standort | „Kopieren“ bleibt (`docs/attachments.md`), mit Markierung steht „Auswahl kopieren“ darüber. |

- **Reihenfolge:** Antworten, Bearbeiten, Link kopieren, Auswahl kopieren bzw. Text kopieren, Bild öffnen, Im Browser öffnen, Löschen. Die Leiste der Reaktionen bleibt die oberste Zeile.
- **Markierung:** Sie zählt, wenn sie mehr als Leerraum enthält und mindestens zum Teil in dieser Nachricht liegt. Kopiert wird die ganze Markierung, auch wenn sie über mehrere Nachrichten reicht. Eine Markierung nur in einer anderen Nachricht zählt nicht.
- **Vor dem Klick:** Die Markierung zählt nur, wenn sie schon vor dem Drücken der Maustaste da war und der Klick sie stehen lässt. Ein Rechtsklick neben die Markierung hebt sie auf, am Mac markiert er stattdessen das Wort unter dem Zeiger: Beides ist keine Auswahl, dann steht „Text kopieren“ da. So kopiert „Auswahl kopieren“ immer das, was sichtbar markiert ist. Der Text wird beim Öffnen des Menüs festgehalten.
- **Link:** Gemeint sind Links im Text der Nachricht, auch die aus Markdown. Kopiert wird die Adresse, wie sie in der Nachricht steht, bei `[Text](Adresse)` also die Adresse. Datei- und Standortzeilen zählen nicht, dort bleibt es bei „Im Browser öffnen“ bzw. „Kopieren“.
- **„Mehr“:** Für die Markierung gilt dieselbe Regel. Einen Link gibt es dort nicht.
- Wartende, fehlgeschlagene und gelöschte Nachrichten behalten ihr Menü.
- Ein Rechtsklick auf ein offenes Menü der App öffnet nichts.

### Menü des Betriebssystems

Wo die App kein eigenes Menü hat, öffnet der Rechtsklick ein Menü des Betriebssystems. Es zeigt nur, was gerade möglich ist. Ist nichts möglich, erscheint kein Menü.

| Wo | Einträge |
|---|---|
| Eingabefeld (Nachricht, Suche, Anmeldung, Gruppenname und andere) | Bei einem als falsch markierten Wort zuerst die Vorschläge, ohne Vorschläge grau „Keine Vorschläge“, dann eine Trennlinie. Danach „Rückgängig“, „Wiederholen“, Trennlinie, „Ausschneiden“, „Kopieren“, „Einfügen“, Trennlinie, „Alles auswählen“. |
| Markierter Text außerhalb einer Nachricht (etwa Gruppenname, Gruppenbeschreibung) | „Kopieren“ |
| Link außerhalb einer Nachricht (etwa in den Einstellungen) | „Link kopieren“, nur für `http`, `https` und `mailto`. Mit Markierung steht „Kopieren“ darunter. |

- Ein Klick auf einen Vorschlag ersetzt das Wort.
- Am Mac heißen die Einträge wie im Menü „Bearbeiten“: „Widerrufen“ und „Einsetzen“ statt „Rückgängig“ und „Einfügen“.
- „Alles auswählen“ gibt es nur in Eingabefeldern.

### Test
1. Zwei Wörter in einer Nachricht markieren, Rechtsklick darauf: „Auswahl kopieren“, in der Zwischenablage stehen genau die zwei Wörter.
2. Ohne Markierung: „Text kopieren“ legt die ganze Nachricht ab.
3. Markierung in Nachricht A, Rechtsklick auf Nachricht B: Bei B steht „Text kopieren“. Ebenso beim Rechtsklick in A neben die Markierung.
4. Rechtsklick auf einen Link im Text und auf einen Markdown-Link: „Link kopieren“ legt die Adresse ab. Auf dem übrigen Text derselben Nachricht fehlt der Eintrag.
5. Link und Markierung zugleich: „Link kopieren“ steht über „Auswahl kopieren“.
6. Datei, Bild, Standort, wartende und gelöschte Nachricht: Menü wie bisher.
7. Rechtsklick auf eine Erwähnung öffnet das Menü der Nachricht (`docs/mentions.md`).
8. Markierung, dann „Mehr“: „Auswahl kopieren“.
9. Eingabefeld: Rechtsklick auf ein falsch geschriebenes Wort zeigt Vorschläge, ein Klick ersetzt das Wort. Ausschneiden, Kopieren, Einfügen und Alles auswählen wirken.
10. Markierte Gruppenbeschreibung: „Kopieren“. Link in den Einstellungen: „Link kopieren“. Rechtsklick ins Leere: kein Menü.
11. Rechtsklick auf einen Chat in der Liste öffnet weiter das Chat-Menü.
12. Mac: Rechtsklick auf ein Wort ohne vorherige Markierung zeigt „Text kopieren“, mit vorheriger Markierung „Auswahl kopieren“.

## Bild von „Notizen an mich“

Für den Chat „Notizen an mich“ liefert der Server ein Emoji-Bild. Die Clients zeigen stattdessen ein eigenes und rufen dafür nichts vom Server ab.

**Bedingung:** `roomType == 6`, sonst nichts. `hasCustomAvatar` hilft hier nicht: Der Server setzt das Emoji-Bild beim Anlegen des Chats als eigenes Raumbild und meldet deshalb immer `true`. Ein selbst gesetztes Bild für diesen Chat wird dadurch nicht gezeigt.

**Aussehen:** ein Kreis in der Akzentfarbe (die gewählte, auf dem Desktop `--accent`), darauf ein weißes Notiz-Symbol in Linien. Hell und dunkel sind gleich aufgebaut.

**Symbol** im 24er-Raster, nur Linien, Strichstärke 1.8, runde Enden und Ecken:
- Blatt: abgerundetes Rechteck bei x 5, y 3.5, Breite 14, Höhe 17, Radius 2.5.
- Zeilen: (8.5, 8.5)–(15.5, 8.5), (8.5, 12)–(15.5, 12) und (8.5, 15.5)–(12.5, 15.5).
- Das 24er-Feld ist halb so groß wie der Durchmesser des Avatars und sitzt mittig.

**Wo:** überall, wo das Bild eines Chats über sein Token gezeigt wird, also Chatliste, Chat-Kopf, Suche, Weiterleiten, Auswahl der Chats einer Kategorie und Erwähnungen. Jede Plattform löst das an einer Stelle (Desktop: `Avatar`).
