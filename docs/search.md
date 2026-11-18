# Suche – Nachrichten in allen Chats und im Chat

Verbindlich für alle Plattformen. Vorbild für die Bedienung ist Signal. Die Suche läuft auf dem Gerät: Der Kern lädt den Verlauf aller Chats im Hintergrund und durchsucht ihn ohne Server, also auch ohne Netz und mit jedem Tastendruck.

Die Suche nach Chatnamen bleibt, wie sie ist (docs/categories.md „Suche“). Sie steht über den Treffern in den Nachrichten.

## Suchregel

| Punkt | Regel |
|---|---|
| Ab wann | Ab 2 Zeichen (ohne Leerzeichen gezählt). Darunter gibt es keine Nachrichtentreffer. |
| Wörter | Mehrere Wörter müssen alle vorkommen, in beliebiger Reihenfolge. |
| Wortteile | Jedes Wort trifft auch mitten im Wort: „burg“ findet „Hüpfburg“. |
| Schreibweise | Groß- und Kleinschreibung und Akzente zählen nicht: „hupf“ findet „Hüpfburg“. „ß“ bleibt „ß“. |
| Durchsucht | Text, Bildunterschrift, Dateiname, Name eines Kontakts, Bezeichnung eines Standorts, Frage einer Umfrage. |
| Nicht durchsucht | Systemmeldungen, gelöschte und abgelaufene Nachrichten, Sprachnachrichten, Dateiinhalte. |
| Reihenfolge | Neueste zuerst. |

## Suche über alle Chats

Einstieg ist das vorhandene Suchfeld der Chatliste.

| Punkt | Regel |
|---|---|
| Aufbau | Oben die Abschnitte der Chats, deren Name passt (`roomSections(query)`, wie bisher). Darunter die Überschrift „Nachrichten“ mit den Treffern. Gibt es keine passenden Chats, steht „Nachrichten“ ganz oben. |
| Archiv | Passt der Name eines archivierten Chats, liefert `roomSections` ihn in einem eigenen Abschnitt „Archiviert“ (`kind: "archived"`) am Ende. Der Abschnitt lässt sich nicht einklappen und hat kein Kategorie-Menü. Nachrichten archivierter Chats werden wie alle anderen gefunden. |
| Trefferzeile | Avatar und Name des Chats, rechts das Datum wie in der Chatliste. Darunter der Ausschnitt, höchstens zwei Zeilen. Suchwörter sind fett und in der Textfarbe, der Rest in `muted`. |
| Absender | Mit `showActor` steht der Absender vor dem Ausschnitt: „Du: “ bei eigenen Nachrichten, sonst „{Name}: “. Das gilt für eigene Nachrichten und für Gruppen. |
| Ausschnitt | `snippet` kommt fertig vom Kern: eine Zeile um den ersten Treffer, mit „…“, wo Text fehlt. Die Plattform setzt nur die Teile mit `hit` fett. |
| Antippen | Öffnet den Chat an der Nachricht (siehe „Chat an einer Nachricht öffnen“). Zurück führt in die Suche, Suchtext und Position der Liste bleiben. Am Desktop bleibt die Trefferliste stehen, der gewählte Treffer ist hinterlegt. |
| Mehr | Die Liste beginnt mit 50 Treffern. Ist `more` gesetzt und das Ende der Liste erreicht, fragt die Plattform mit 50 mehr. |
| Leer | Weder Chats noch Nachrichten: „Keine Treffer“. |
| Tempo | `searchMessages` läuft bei jeder Änderung des Suchtexts, ohne Wartezeit. Es liest eine Datenbank und gehört deshalb nicht in den Hauptthread. Ein Ergebnis zu einem überholten Suchtext wird verworfen. |
| Verlauf lädt noch | Solange `chatsDone < chatsTotal`, steht unter den Treffern in `muted`: „Ältere Nachrichten werden noch geladen ({chatsDone} von {chatsTotal} Chats)“. |
| Ergänzung vom Server | Ist `complete` falsch, ruft die Plattform 300 ms nach dem letzten Tastendruck `searchMessagesEverywhere` mit demselben Suchtext und ersetzt die Liste, wenn der Suchtext noch gilt. Ein Fehler dabei (kein Netz) ändert nichts an der Liste und zeigt keinen Hinweis. |

## Suche im Chat

### Android und iOS

| Punkt | Regel |
|---|---|
| Einstieg | Lupe im Chat-Kopf, links neben dem Anruf-Knopf. Bezeichnung „Im Chat suchen“. |
| Kopf | Das Suchfeld ersetzt Avatar und Namen, die Tastatur öffnet sich. Platzhalter „Suchen“. Der Zurück-Pfeil schließt die Suche und zeigt wieder den Kopf, erst ein weiteres Zurück verlässt den Chat. |
| Leiste | Statt der Eingabeleiste steht eine Leiste mit dem Zähler links („3 von 17“, ohne Treffer „Keine Treffer“, bei leerem Feld nichts) und rechts zwei Pfeilen: nach oben zum älteren, nach unten zum neueren Treffer. Am Ende der Reihe ist der Pfeil ausgegraut. |
| Treffer | `searchMessages(text, token, 500)` bei jeder Änderung. Die Treffer sind die Reihe, `total` der Zähler. Der neueste Treffer ist gewählt (Nummer 1), und der Chat springt zu ihm. |
| Sprung | Wie „Chat an einer Nachricht öffnen“. Der gewählte Treffer leuchtet kurz auf. |
| Hervorhebung | Solange die Suche offen ist, sind die Suchwörter in allen sichtbaren Blasen hinterlegt (Farbe `searchMark`), ohne Groß/Klein und ohne Akzente, auch mitten im Wort. Das betrifft den angezeigten Text und die Bildunterschrift, nicht Zitat und Dateinamen. |
| Mehr als 500 | Der Zähler nennt `total`. Die Pfeile gehen durch die ersten 500. |
| Schließen | Der Chat bleibt, wo er ist. Steht er nicht am Ende, zeigt er „Zum Ende springen“. |
| Verlauf lädt noch | Ist `complete` falsch, gilt die Ergänzung vom Server wie oben, mit dem Chat als `token`. |

### Desktop

Wie Signal Desktop läuft die Suche im Chat über die Seitenleiste.

| Punkt | Regel |
|---|---|
| Einstieg | Lupe im Chat-Kopf (Titel „Im Chat suchen“) und Strg+Umschalt+F (macOS: ⌘⇧F). Strg+F (⌘F) setzt den Fokus in das Suchfeld ohne Marke, also für alle Chats. |
| Marke | Im Suchfeld der Seitenleiste steht vor dem Text die Marke „in {Chatname}“. Die Rücktaste im leeren Feld oder das ✕ der Marke entfernt sie, dann gilt der Suchtext für alle Chats. Esc leert Feld und Marke. |
| Liste | Mit Marke zeigt die Seitenleiste nur die Nachrichtentreffer dieses Chats, keine Chatnamen. Die Zeile nennt statt des Chats den Absender (eigene: „Du“) und das Datum, darunter den Ausschnitt. Über der Liste steht die Zahl: „17 Treffer“, „1 Treffer“ oder „Keine Treffer“. |
| Klick | Der Chat springt zur Nachricht, sie leuchtet auf. Pfeil hoch und runter im Suchfeld gehen durch die Treffer, Enter springt zum gewählten. |
| Hervorhebung | Wie mobil: Solange die Marke dieses Chats gesetzt ist, sind die Suchwörter in den Blasen hinterlegt. |
| Chatwechsel | Wer mit gesetzter Marke einen anderen Chat öffnet, behält die Trefferliste. Die Marke bleibt beim alten Chat. |

### Farbe
`searchMark` ist ein Gelb (`#FFD60A`), in Hell mit 45 % Deckkraft, in Dunkel mit 35 %. Es liegt hinter dem Text, die Textfarbe bleibt. Für das Aufleuchten füllt es die ganze Blase.

## Chat an einer Nachricht öffnen

Ein Treffer liegt oft weit zurück. Der Chat lädt dann nicht alles dazwischen, sondern zeigt ein Fenster in den Verlauf.

| Punkt | Regel |
|---|---|
| Schon geladen | Ist die Nachricht in der Liste, scrollt der Chat zu ihr. |
| Fenster | Sonst ersetzt `messagesAround(token, id)` die Liste: bis zu 50 Nachrichten davor, die Nachricht, bis zu 50 danach. |
| Aus der Suche über alle Chats | Der Chat öffnet wie immer mit `openChat` (das startet die Live-Nachrichten), zeigt aber nicht dessen Seite, sondern gleich das Fenster. Das Ende des Chats blitzt dabei nicht auf. |
| Position | Die Nachricht steht im oberen Drittel des Sichtbereichs. |
| Aufleuchten | Ihre Blase ist etwa 1,5 s lang hinterlegt (`searchMark`) und blendet dann aus. |
| Nachladen | Nach oben wie immer mit `loadOlder(oldestId)`, solange `hasMore`. Nach unten mit `loadNewer(newestId)`, solange `hasNewer`. |
| Neue Nachrichten | Solange `hasNewer` gilt, hängt die Plattform `messagesAdded` nicht an die Liste (es entstünde eine Lücke). Sie merkt sich die Nachrichten und zählt sie im Knopf „Zum Ende springen“. `messagesUpdated` wirkt auf alles, was in der Liste steht. |
| Ende erreicht | Liefert `loadNewer` `hasNewer: false`, ist das Fenster am Ende angekommen. Die Plattform hängt die gemerkten Nachrichten an, die neuer sind als die letzte der Liste, und der Chat läuft wieder wie gewohnt. |
| Zum Ende springen | Im Fenster ist der Knopf immer da. Er ersetzt die Liste durch `cachedChat(token)`, hängt die gemerkten Nachrichten an und scrollt ans Ende. |
| Lesemarke | Im Fenster bewegt sich die Lesemarke nicht. Erst am Ende gilt wieder „unten → `markVisibleRead`“ (docs/chat.md). |
| Ausstehende | Eigene Nachrichten, die noch gesendet werden, stehen am Ende des Chats. Im Fenster sind sie nicht zu sehen. Wer im Fenster etwas sendet, springt damit ans Ende. |
| Nicht gefunden | Gibt es die Nachricht nicht mehr (Fehler von `messagesAround`): „Nachricht nicht gefunden“, der Chat bleibt, wie er ist. Ohne Netz: „Keine Verbindung“. |

### Zitat antippen
Ein Antippen des Zitats in einer Blase springt zur zitierten Nachricht, nach denselben Regeln (`replyTo.id`). Das Zitat einer gelöschten Nachricht tut nichts.

## Verlauf auf dem Gerät

- Der Kern lädt den Verlauf aller Chats im Hintergrund, die zuletzt aktiven zuerst. Es läuft immer nur eine Anfrage (200 Nachrichten), mit einer kurzen Pause danach, und keine während eines Anrufs.
- Der Stand bleibt gespeichert. Nach einem Neustart macht der Kern dort weiter, und was schon geladen ist, ist sofort durchsuchbar.
- Gespeichert wird je Nachricht nur, was die Suche braucht: Text, Absender und Zeit. Die Datei `search.sqlite` liegt im Datenordner des Kontos neben dem Cache, im privaten App-Speicher und außerhalb von Backups. Sie ist wie der Cache nicht verschlüsselt.
- Neue Nachrichten sind sofort durchsuchbar: im offenen Chat mit der Nachricht selbst, sonst mit dem nächsten Blick auf die Chatliste.
- Bearbeiten und Löschen wirken im offenen Chat sofort. In einem Chat, der nicht offen ist, kommen sie mit seiner nächsten Nachricht an, spätestens beim nächsten Start der App.
- „Verlauf geleert“ leert auch den Index des Chats. Ein verlassener oder gelöschter Chat verschwindet aus dem Index, Abmelden löscht ihn ganz.
- Abgelaufene Nachrichten werden nicht gefunden und regelmäßig entfernt.
- **Grenze:** Je Chat hält der Index 20.000 Nachrichten. Bei längeren Chats fehlt der älteste Teil, `complete` bleibt dann falsch und der Server ergänzt.
- Die Lesemarke bleibt beim Laden unberührt.

### Ergänzung vom Server
`searchMessagesEverywhere` fragt die Suche von Talk (`talk-message`), solange der Verlauf nicht ganz geladen ist. Sie ist nur eine Brücke:
- Talk sucht eine zusammenhängende Zeichenfolge. Der Kern fragt nach dem längsten Wort des Suchtexts und prüft die übrigen Wörter selbst.
- Was der Index schon kennt, kommt nie doppelt und nie vom Server.
- Talk liefert höchstens 25 Treffer je Anfrage, der Kern holt bis zu drei Seiten.
- Dateinamen und Umlaute in Bildunterschriften findet Talk nicht.

## Kern-Vertrag

| Aufruf | Bedeutung |
|---|---|
| `searchMessages(query, token?, limit) → SearchResults` | Sucht auf dem Gerät, in einem Chat oder (`token` leer) in allen. Antwortet in Millisekunden. |
| `searchMessagesEverywhere(query, token?, limit) → SearchResults` | Dasselbe, um Treffer des Servers ergänzt. Nur nötig, solange `complete` falsch ist. |
| `messagesAround(token, messageId) → ChatPage` | Fenster um eine Nachricht, aus dem Cache oder vom Server. |
| `loadNewer(token, afterId) → ChatPage` | Die nächsten 50 Nachrichten nach `afterId` (`ChatPage.newestId`). |
| `roomSections(query, …)` | Liefert bei einer Suche zusätzlich den Abschnitt `archived`. Der Vergleich ignoriert jetzt auch Akzente. |

`SearchResults`:
- `hits`: die Treffer, neueste zuerst.
- `total`: in einem Chat alle Treffer, auch über `hits` hinaus. In allen Chats so viele wie `hits`.
- `more`: Es gibt mehr Treffer als `hits`.
- `complete`: Alles Durchsuchbare liegt auf dem Gerät.
- `chatsDone`, `chatsTotal`: Chats mit geladenem Verlauf und alle Chats (im Chat: 0 oder 1 von 1).

`SearchHit`: `token`, `messageId`, `timestamp`, `roomName`, `actorName`, `isOwn`, `showActor`, `snippet` (Teile mit `text` und `hit`).

`ChatPage` hat neu `hasNewer` und `newestId`. Bei allen bisherigen Aufrufen ist `hasNewer` falsch.

**Prüfen ohne App:** `talk-cli search <text> [-t token] [--sync] [--remote]` zeigt Treffer und Dauer, ohne Text den Stand des Index. `talk-demo` hat den archivierten Chat „Hofladen-Archiv“ (`hofladen`) mit rund 1.200 Nachrichten. Darin stehen drei, die es nur einmal gibt: „Quittenmarmelade“ (Notiz 37, ganz am Anfang), „Hüpfburg“ (Notiz 412) und „Rhabarberschorle“ (Notiz 905). Jede Nachricht trägt ihre Nummer („Notiz 618“).

## Test

Gegen `talk-demo`, je in Hell und Dunkel.

1. Chatliste, Suchtext „hofladen“: Der Abschnitt „Archiviert“ zeigt „Hofladen-Archiv“, darunter stehen Nachrichtentreffer.
2. Suchtext „quitten“: ein Treffer im „Hofladen-Archiv“ mit fettem „Quitten“. Die Liste folgt dem Tippen ohne spürbare Pause.
3. Antippen: Der Chat öffnet an der Nachricht, sie leuchtet auf. Nach unten lädt der Chat nach, bis zum Ende. „Zum Ende springen“ führt direkt dorthin.
4. Zurück: Die Suche steht noch da, mit Suchtext und Treffern.
5. Suchtext „buro HUPF“: derselbe eine Treffer wie bei „Hüpfburg Büro“.
6. Suchtext „notiz“: 50 Treffer, beim Scrollen kommen weitere.
7. Suchtext „karla brandt“: der Kontakt im „Sommerfest“. „volkspark“: der Standort.
8. Suche im Chat „Hofladen-Archiv“ mit „schlüssel“: Zähler und Pfeile stimmen, die Wörter sind in den Blasen hinterlegt, jeder Sprung landet auf dem Treffer.
9. Direkt nach der ersten Anmeldung (Verlauf lädt noch): Die Zeile „Ältere Nachrichten werden noch geladen …“ erscheint und verschwindet, die Treffer des Servers kommen dazu.
10. Ohne Netz nach dem Laden: Die Suche geht weiter, der Sprung in einen nie geöffneten Teil meldet „Keine Verbindung“.
11. Neue Nachricht über `POST /demo/incoming`: Sie wird gefunden. Im offenen Chat bearbeitet (`/demo/edit`) und gelöscht (`/demo/delete`): Die Suche zeigt den neuen Text, dann nichts mehr.
12. Im Fenster kommt eine neue Nachricht: Sie erscheint nicht in der Liste, der Knopf „Zum Ende springen“ zählt sie.
13. Zitat antippen: springt zur zitierten Nachricht.
14. Desktop: Strg+F, Strg+Umschalt+F, Marke „in {Chat}“, Rücktaste, Esc, Pfeiltasten und Enter.
