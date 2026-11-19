# Erwähnungen – Vorschlagsliste, Markierung im Chat, Sprung zur Erwähnung

Verbindlich für alle Plattformen. Vorbild für die Bedienung ist Signal, übernommen wird nur das Muster. Wer „@“ tippt, bekommt eine Liste der Leute im Chat. Im Text steht immer der Name („@Lena Brandt“), nie die Kennung. Wer damit gemeint ist, reist als `MentionPick` neben dem Text, und erst der Kern macht daraus die Schreibweise des Servers (`@"lena"`).

**Begriffe**
- **Erwähnung:** „@“ und ein Anzeigename, hinter dem eine Person, eine Gruppe, ein Team, ein Gast oder alle stehen.
- **Auswahl** (`MentionPick { label, mentionId }`): eine aus der Liste gewählte Erwähnung. `label` ist der Name ohne „@“.

## Vorschlagsliste

| Punkt | Regel |
|---|---|
| Wo | In Gruppen und öffentlichen Chats. Im Direktchat liefert der Kern eine leere Liste, die Plattform muss nichts unterscheiden. |
| Auslöser | Ein „@“ am Textanfang oder nach Leerraum (Leerzeichen, Zeilenumbruch). Nach einer Klammer oder mitten im Wort zählt es nicht, der Server würde dort keine Erwähnung erkennen. |
| Suchtext | Die Zeichen vom „@“ bis zur Schreibmarke. Er darf Leerzeichen enthalten („@Lena Br“), aber keinen Zeilenumbruch, und ist höchstens 40 Zeichen lang. Gesucht wird das nächste „@“ vor der Schreibmarke, das die Auslöser-Regel erfüllt. Beginnt dort eine fertige Erwähnung (siehe „Im Eingabefeld“), gibt es keinen Suchtext. |
| Abfrage | Bei jeder Änderung von Text oder Schreibmarke `mentionCandidates(token, suchtext, 50)`. Kein Warten nach dem Tastendruck, überholte Antworten werden verworfen. |
| Sichtbar | Solange die Antwort nicht leer ist und das Feld den Fokus hat. |
| Reihenfolge | Wie der Kern sie liefert: Name beginnt mit dem Suchtext, dann ein Wort des Namens oder die Kennung beginnt damit, dann Wortteil. Innerhalb davon zuerst, wer zuletzt im Chat geschrieben hat. Groß/Klein und Akzente zählen nicht. „Alle“ steht am Ende. Der User selbst fehlt. |
| Zeile | Bild, Name (eine Zeile, zu lange Namen enden mit „…“). Hat der Eintrag eine `subline`, steht sie klein und in `muted` darunter: „Gruppe“, „Team“, „Gast“, „Alle im Chat“, bei doppelten Namen die Kennung. |
| Bild | `kind` `user`: das Benutzerbild von `userId`. `all`: das Bild des Chats. `group` und `team`: runder Kreis in `chip` mit dem Gruppen-Symbol der Plattform. Sonst der Kreis mit den Initialen. |
| Übernehmen | Der Bereich vom „@“ bis zur Schreibmarke wird durch `@{label}` und ein Leerzeichen ersetzt (kein zweites, wenn schon eines folgt). Die Schreibmarke steht hinter dem Leerzeichen. Die Auswahl kommt in die Liste der Auswahlen des Felds, eine ältere mit demselben `label` wird ersetzt. |
| Schließen | Leere Antwort, „@“ gelöscht, Schreibmarke außerhalb, Feld ohne Fokus, Nachricht gesendet. Esc (Desktop) und Zurück (Android) schließen die Liste für dieses „@“. Sie bleibt zu, bis sich der Suchtext ändert. |

### Android und iOS
- Die Liste liegt direkt über dem Eingabefeld und über der Leiste für Antwort oder Bearbeiten, in voller Breite, auf der Fläche `floating` mit einer Haarlinie oben.
- Zeilenhöhe 48, Bild 32. Höchstens viereinhalb Zeilen hoch, darüber hinaus scrollbar. Die beste Zeile steht oben.
- Die Tastatur bleibt offen. Antippen übernimmt. Zurück schließt zuerst die Liste.
- Die Nachrichtenliste darunter verschiebt sich nicht, die Liste liegt über ihr.

### Desktop
- Aufklappliste über dem Eingabefeld, linksbündig mit ihm, 320 breit (schmaler, wenn das Feld schmaler ist), nach dem Muster der Emoji-Auswahl.
- Zeilenhöhe 44, Bild 28. Höchstens sechs Zeilen, darüber hinaus scrollbar.
- Die erste Zeile ist vorgewählt. Pfeil hoch und runter wählen (am Ende geht es von vorn weiter), Enter oder Tab übernehmen, Esc schließt nur die Liste. Diese Tasten gelten vor allen anderen Regeln des Felds: Enter sendet nicht, Esc beendet weder Antwort noch Bearbeiten, Pfeil hoch öffnet nicht die letzte Nachricht.
- Die Maus wählt beim Überfahren, ein Klick übernimmt. Der Fokus bleibt im Feld.
- `role="listbox"`, die Zeilen `role="option"`, das Feld nennt die gewählte Zeile über `aria-activedescendant`.

## Im Eingabefeld

Das Feld hält neben dem Text die Liste der Auswahlen. Welche Stellen Erwähnungen sind, ergibt sich allein aus dem Text. Die Plattform merkt sich keine Positionen.

**Regel (dieselbe wie im Kern):** `@{label}` einer Auswahl ist eine Erwähnung, wenn
- davor der Textanfang, Leerraum oder eines der Zeichen `-` `.` `'` `@` steht,
- und danach kein Buchstabe und keine Ziffer folgt.

Passen mehrere Auswahlen, gilt der längste Name.

| Punkt | Regel |
|---|---|
| Aussehen | Jede Erwähnung ist hinterlegt (Farbe `mention`, siehe „Farben“). Schriftschnitt und Breite bleiben, damit nichts springt. |
| Löschen | Rückwärts löschen mit der Schreibmarke direkt hinter einer Erwähnung (ohne Auswahl) entfernt sie als Ganzes samt „@“. |
| Hineintippen | Wer im Namen tippt, macht daraus gewöhnlichen Text. Die Hinterlegung verschwindet von selbst, weil die Regel nicht mehr passt. |
| Einfügen | Eingefügter Text „@Name“ einer vorhandenen Auswahl wird zur Erwähnung. |
| Entwurf | Der Entwurf eines Chats merkt sich Text und Auswahlen. |
| Senden | `enqueueText(token, text, auswahlen, …)`. Auswahlen, die im Text nicht mehr vorkommen, stören nicht. Danach ist die Liste der Auswahlen leer. |
| Bearbeiten | `editSource` liefert `{ text, mentions }`: Der Text zeigt Namen, `mentions` sind die Auswahlen. Speichern mit `editMessage(token, id, text, auswahlen)`. Liefert `editSource` nichts, gilt `MessageView.text`, und aus jedem Eintrag von `MessageView.mentions` wird eine Auswahl (`label` ohne das „@“, `mentionId` = `id`). |
| Gleiche Namen | Zwei gewählte Personen mit demselben Namen: Die zuletzt gewählte gilt für alle Stellen. |

Die Regel zum Nachprüfen, mit den Auswahlen „Lena“, „Lena Brandt“ und „Alle“:

| Text | Erwähnungen |
|---|---|
| `Hallo @Lena Brandt, hallo @Lena.` | „@Lena Brandt“ und „@Lena“ |
| `@Alle` ↵ `@Lena` | beide |
| `@Lenas Rad` | keine |
| `@Jonas a@Lena (@Lena)` | keine |

## In der Nachricht

`MessageView.mentions` nennt die Erwähnungen einer Nachricht in der Reihenfolge des Texts: `{ label, kind, id, isSelf }`. `label` steht so in `MessageView.text` („@Lena Brandt“).

| Punkt | Regel |
|---|---|
| Wo | Im Text der Blase und in der Bildunterschrift, auch in wartenden Nachrichten. Nicht im Zitat, nicht in der Chatliste, nicht in Benachrichtigungen: Dort bleibt es schlichter Text. |
| Finden | Jedes Vorkommen eines `label` im fertigen Text (nach Markdown), dem kein Buchstabe und keine Ziffer folgt. Der längste Name zuerst. Nicht in Code (`` ` `` und Code-Block). Ein Durchgang nach dem Muster der Suchhervorhebung. |
| Aussehen | Halbfett (600) und hinterlegt mit `mention`. |
| Du wirst erwähnt | Ist `isSelf` wahr und die Nachricht nicht die eigene, ist die Fläche `mentionSelf`. `isSelf` gilt für den eigenen Namen und für „@Alle“. |
| „Alle“ | Der Kern nennt die Erwähnung aller „@Alle“, nicht mehr nach dem Chat. |
| Suche | Liegt ein Suchtreffer auf einer Erwähnung, gewinnt dort `searchMark`. |
| Systemmeldungen | Haben keine Erwähnungen und kein „@“ mehr vor dem Handelnden. |

### Farben
| Farbe | Wert |
|---|---|
| `mention` | Die Schriftfarbe der Blase (im Eingabefeld die des Felds) mit 12 % Deckkraft. |
| `mentionSelf` | `accent` mit 22 % Deckkraft in Hell, 32 % in Dunkel. |

Am Desktop hat die Fläche 4 px Eckenradius und links und rechts 2 px Luft, ohne die Zeilenhöhe zu ändern. Auf Android und iOS ist sie ein schlichter Hintergrund wie bei der Suche.

### Antippen
| Punkt | Regel |
|---|---|
| Wer | Nur `kind` `user`, und nicht der User selbst. Alle anderen Erwähnungen haben keine Aktion. |
| Chat vorhanden | `existingDirect(id)` liefert ein Token: Dieser Chat öffnet sofort. |
| Kein Chat | Nachfrage „Chat mit {Name} beginnen?“ mit „Abbrechen“ und „Chat öffnen“. Erst danach `openDirect(id)`. So legt ein versehentlicher Tipp keinen leeren Chat an. |
| Zurück | Android und iOS: Zurück führt in die Gruppe. Desktop: Der Direktchat wird der offene Chat. |
| Fehler | Der Text von `openDirect` als Hinweis, wie bei „Neue Nachricht“. |
| Sonst | Langes Drücken und Rechtsklick bleiben das Menü der Nachricht. Am Desktop hat die Erwähnung den Zeiger einer Schaltfläche. |

**Barrierefreiheit:** Eine antippbare Erwähnung ist als Verweis erreichbar, Bezeichnung „Chat mit {Name} öffnen“.

## „@“-Knopf im Chat

Wurde der User erwähnt und hat die Stelle noch nicht gesehen, führt ein Knopf dorthin.

| Punkt | Regel |
|---|---|
| Liste | Der Chat hält die ids der ungesehenen Erwähnungen, aufsteigend. |
| Quellen | `unreadMentions` jeder Seite, die der Chat bekommt (`cachedChat`, `openChat`, `loadOlder`, `messagesAround`, `loadNewer`), zusammengeführt. Dazu jede Nachricht aus `messagesAdded` mit `mentionsMe`. Bei sehr vielen ungelesenen Nachrichten kennt die erste Seite nur die neuesten Erwähnungen, die älteren kommen mit dem Nachladen. Eine einmal gesehene id kommt nicht wieder in die Liste. |
| Gesehen | Eine id fällt heraus, sobald ihre Blase mindestens zur Hälfte sichtbar war, nach dem Sprung zu ihr, und wenn die Nachricht gelöscht wurde. Beim Verlassen des Chats ist die Liste leer. |
| Knopf | Rund, Größe und Fläche wie „Zum Ende springen“, direkt darüber mit 8 Abstand. Zeichen „@“. Ab zwei Erwähnungen die Zahl im selben Schild wie der Zähler von „Zum Ende springen“. |
| Sichtbar | Solange die Liste nicht leer ist. Auf Android und iOS nicht im Suchmodus (dort ersetzt die Suchleiste das Eingabefeld). Am Desktop gibt es keinen Suchmodus, der Knopf bleibt auch mit der Marke „in {Chat}“ sichtbar. |
| Tippen | Springt zur ältesten id der Liste, mit demselben Sprung wie die Suche (docs/search.md „Chat an einer Nachricht öffnen“). Die Blase leuchtet auf. |
| Lesemarke | Bleibt, wie sie ist: Sie bewegt sich nur am Ende des Chats. |

Die Erwähnung einer Gruppe oder eines Teams trägt kein `mentionsMe`, weil die App die Mitgliedschaft nicht kennt. Meldet der Server trotzdem eine Erwähnung, nennt `unreadMentions` die erste solche Nachricht hinter der Lesemarke.

**Barrierefreiheit:** Bezeichnung „Zur Erwähnung springen“, mit Zahl „Zur Erwähnung springen, 3 Erwähnungen“. Am Desktop zusätzlich `title`.

## Kern-Vertrag

| Aufruf | Bedeutung |
|---|---|
| `mentionCandidates(token, query, limit) → [MentionCandidate]` | Wer erwähnt werden kann und zum Suchtext passt. Der Kern holt die Liste des Chats beim Öffnen vom Server und hält sie im Speicher. Danach antwortet jeder Aufruf sofort. Nach zwei Minuten und nach Ein- oder Austritten wird sie im Hintergrund erneuert. Leer im Direktchat, ohne Treffer und wenn die Liste ohne Netz noch nie geladen wurde. |
| `enqueueText(token, text, mentions, replyTo, referenceId)` | Neu ist `mentions`: die Auswahlen des Felds. Die wartende Blase zeigt den Text mit Namen. |
| `enqueueFile(…, caption, mentions, replyTo, referenceId)` | Dasselbe für die Bildunterschrift. |
| `editSource(token, messageId) → EditDraft?` | `{ text, mentions }` fürs Eingabefeld. |
| `editMessage(token, messageId, text, mentions)` | Speichert mit den Auswahlen. Unveränderter Text fragt niemanden. |
| `existingDirect(userId) → token?` | Der Direktchat mit einer Person, wenn es ihn schon gibt. Legt nichts an. |

Neue Felder:
- `MentionCandidate { mentionId, label, kind, userId?, subline }`. `label` ist der Name ohne „@“.
- `MessageView.mentions: [MentionView { label, kind, id, isSelf }]` und `MessageView.mentionsMe`.
- `ChatPage.unreadMentions: [id]`: was der Cache hinter der Lesemarke kennt, und was die Seite selbst bringt.
- `kind`: `user`, `group`, `team`, `guest`, `email`, `federated` oder `all`. `id` ist bei `user` die Benutzerkennung, sonst die `mentionId`.

Der Kern setzt beim Senden jede Erwähnung in `@"mentionId"` um. Eine Kennung mit Zeichen, die der Server nicht liest, bleibt als Name stehen.

**Nicht im Umfang:** Erwähnungen im eigenen Unterschrift-Feld der Bild- und Dateivorschau auf Android und iOS, der Status einer Person in der Liste, die Einstellung, wer „Alle“ erwähnen darf.

## Test

Gegen `talk-demo`. Der User ist Mira Lange. Im Chat „Sommerfest“ gibt es die Gruppe „Helferkreis“, das Team „Orga-Team“ und drei Nachrichten mit Erwähnungen. Im Chat „Familie“ heißen drei Leute „Lange“. Im Chat „Team Website“ wird Mira erwähnt. Eingehende Nachrichten: `POST /demo/incoming` mit `token`, `from`, `text` (`@mira`, `@all`, `@"group/helfer kreis"`, `@"team/orga"`). Ohne App: `talk-cli mentions <token> [text]`.

1. „Sommerfest“, „@“ tippen: Liste mit Lena, Jonas, Sophie, Paul, Tobias, Helferkreis, Orga-Team, Alle. Mira fehlt.
2. „@kru“ zeigt Tobias Krüger. „@Lena H“ zeigt Lena Hoffmann. „@Lena x“ schließt die Liste. Jeder Buchstabe wirkt ohne spürbare Pause.
3. „Familie“, „@lan“: Hilde, Paul und Emil Lange.
4. Direktchat: „@“ zeigt keine Liste.
5. Übernehmen: „@Lena Hoffmann “ steht hinterlegt im Feld. Einmal rückwärts löschen entfernt das Leerzeichen, noch einmal die ganze Erwähnung.
6. Mitten im Namen tippen: Die Hinterlegung verschwindet. Senden ergibt gewöhnlichen Text.
7. Person, Gruppe, Team und „Alle“ in einer Nachricht senden: Die wartende und die gesendete Blase zeigen alle vier hinterlegt, „@Alle“ heißt „@Alle“.
8. „Team Website“: „@Mira Lange“ in der fremden Blase ist in `mentionSelf` hinterlegt. „Sommerfest“: „@Alle“ ebenso, „@Helferkreis“ in `mention`.
9. Nachricht mit Erwähnung bearbeiten: Das Feld zeigt den Namen hinterlegt. Eine zweite Erwähnung dazu, speichern: beide bleiben Erwähnungen.
10. Chat verlassen und zurück: Der Entwurf mit Erwähnung ist wieder da und wird als Erwähnung gesendet.
11. Erwähnung von Lena antippen: Der Direktchat öffnet, Zurück führt in die Gruppe. Erwähnung von Jonas antippen: Nachfrage. „Helferkreis“, „Alle“ und der eigene Name tun nichts.
12. „@“-Knopf: Per `/demo/incoming` in einen Chat, der nicht offen ist, erst 30 Nachrichten schicken, dann eine mit `@mira`, dann 30 weitere. Chat öffnen: Er steht an der Ungelesen-Linie, die Erwähnung liegt weiter unten, der Knopf steht über „Zum Ende springen“. Tippen springt zur Erwähnung, die Blase leuchtet, der Knopf verschwindet. (Ist die Erwähnung beim Öffnen schon zu sehen, erscheint der Knopf nicht: Sie gilt als gesehen.)
13. Chat offen und hochgescrollt, zwei Nachrichten mit `@mira` und `@all` treffen ein: Der Knopf zeigt „2“ und zählt beim Tippen herunter. Hinscrollen bis zur Erwähnung zählt ebenso.
14. Suche im Chat nach dem Namen: Die Erwähnung ist gelb hinterlegt.
15. Systemmeldungen („… hat die Unterhaltung erstellt“) zeigen kein „@“.
16. Desktop: Pfeiltasten, Enter, Tab, Maus. Esc schließt nur die Liste, die Antwort-Leiste bleibt.
17. Hell und Dunkel.
