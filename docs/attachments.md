# Anhänge – Dateien und Standort

Verbindlich für Android und iOS, wo nichts anderes steht. Der Desktop sendet Dateien schon (Büroklammer, Ablegen, Einfügen) und liest Standorte, er sendet keine. Bilder aus der Galerie, Kamera und Sprachnachrichten bleiben, wie sie sind.

Einstieg ist auf Mobilgeräten das Anhang-Menü (＋) in der Eingabeleiste, mit den Knöpfen „Datei“ und „Standort“. Sie stehen gleich nach „Galerie“, vor den Platzhaltern „Umfrage“ und „Kontakt“ (auf iOS steht dazwischen noch „Kamera“), damit sie auch auf schmalen Geräten ohne Wischen zu sehen sind. Beim Bearbeiten einer Nachricht ist das Menü ausgeblendet (`docs/chat.md`).

## Datei senden

| Punkt | Regel |
|---|---|
| Auswahl | „Datei“ öffnet den Dateiwähler des Systems, mit Mehrfachauswahl. Abbrechen ändert nichts. Die Liste übernimmt die Reihenfolge, die der Dateiwähler liefert. Auf Android ist das die Reihenfolge des Antippens, auf iOS legt sie der Dateiwähler selbst fest. |
| Sendebildschirm | Titel „Datei senden“ bzw. „3 Dateien senden“. Je Datei eine Zeile mit Symbol, Name (in der Mitte gekürzt), Größe und ✕ „Entfernen“. Darunter das Feld „Beschreibung hinzufügen“ und der Senden-Knopf. Ohne Datei schließt sich der Bildschirm. |
| Senden | Die Dateien werden in den Zwischenordner kopiert (der Knopf zeigt solange einen Kreisel) und der Reihe nach eingestellt. Wie am Desktop hängt das Antwort-Zitat an der ersten Datei und die Beschreibung an der letzten. Danach schließt der Bildschirm, die Antwortleiste auch, und der Chat steht am Ende. |
| Inhalt | Die Datei geht unverändert raus. Ein Bild über „Datei“ wird nicht verkleinert und behält seine Metadaten, auch den Aufnahmeort. Über „Galerie“ und Kamera bleibt es beim Verkleinern ohne Metadaten. |
| Name und Typ | Name und MIME-Typ kommen vom System, ohne Angabe `application/octet-stream`. Bei `image/*` liest die App Breite und Höhe aus dem Dateikopf, sonst sind beide 0. Ein gedreht gespeichertes Foto zählt so, wie es angezeigt wird. |
| Nicht lesbar | „Datei konnte nicht gelesen werden“, die übrigen Dateien bleiben in der Liste. |
| Grenzen | Größe und Speicherplatz entscheidet der Server. Der Kern meldet „Datei zu groß“ oder „Speicher voll“ an der Blase. |

### Dateikachel
- Symbol, Name (höchstens zwei Zeilen) und Größe. Beschreibung und Uhrzeit stehen darunter wie bei Text.
- **Ausstehend:** Statt des Symbols steht der Fortschrittsring (`PendingMessage.progress`, bei −1 ein Kreisel), auch bei „Wartet auf Netz …“. Eine fehlgeschlagene zeigt wieder das Symbol. Die Statuszeile ist die der anderen Nachrichten: „Wird gesendet …“, „Wartet auf Netz …“, „Nicht gesendet · Datei zu groß“. Antippen einer fehlgeschlagenen sendet erneut, das Menü hat „Erneut senden“ und „Löschen“.
- Bilder, die als Datei gesendet wurden, erscheinen wie jedes Bild (der Typ entscheidet, nicht der Weg).

### Empfangene Datei öffnen
- Antippen lädt die Datei über `fetchFile(path, size)`. Solange steht statt des Symbols ein Kreisel, ein zweites Antippen tut nichts. Der Kern behält die Datei, das nächste Öffnen braucht kein Netz.
- Danach öffnet sie sich: auf Android mit der passenden App („Öffnen mit“), auf iOS in der Vorschau mit Teilen-Knopf. Die App reicht sie unter ihrem echten Namen und Typ weiter, nicht unter dem Namen im Zwischenspeicher.
- Keine passende App: „Keine App zum Öffnen von {Name}“. Laden fehlgeschlagen: „Datei konnte nicht geladen werden“.
- Das gilt für alles, was als Dateikachel erscheint, auch für Videos. Bilder und Sprachnachrichten bleiben beim Betrachter und beim Player.
- Am Desktop öffnet ein Klick weiter die Nextcloud-Seite im Browser.

## Standort

Gesendet wird der eigene Standort im Moment des Sendens, einmal. Es gibt keinen Live-Standort und keine Ortswahl auf der Karte.

### Senden
| Punkt | Regel |
|---|---|
| Einstieg | „Standort“ im Anhang-Menü. Beim ersten Mal fragt das System nach der Berechtigung, nur „beim Verwenden der App“. |
| Abgelehnt | Das Blatt zeigt „Wolkenfunk darf den Standort nicht verwenden“ und „Einstellungen öffnen“. Bei ausgeschalteter Ortung: „Die Ortung ist ausgeschaltet“, auf Android mit „Einstellungen öffnen“ (Standort-Einstellungen des Systems). |
| Blatt | Titel „Standort senden“. Oben das Kartenbild mit Pin, darunter die Bezeichnung und in `muted` „Auf etwa 20 m genau“ (ab 1000 m in km). Unten der Knopf „Diesen Standort senden“. |
| Suche | Bis eine Position da ist: Kreisel und „Standort wird bestimmt …“, der Knopf ist aus. Wird die Position genauer, zieht das Blatt nach. Nach 30 s ohne Position: „Standort nicht gefunden“ und „Erneut versuchen“. |
| Bezeichnung | Die Adresse vom System: „{Straße} {Hausnummer}, {Ort}“, ohne Straße „{Ort}“. Ohne Adressdienst, ohne Netz oder nach 5 s ohne Antwort: „Standort“. Das Senden wartet nicht auf die Adresse. Gesendet wird, was das Blatt zeigt, also auch „Standort“ und nie eine leere Bezeichnung. |
| Senden | `enqueueLocation(token, latitude, longitude, name, referenceId)`. Das Blatt und das Anhang-Menü schließen, der Chat steht am Ende. |
| Antwort | Talk kennt bei Standorten kein Zitat. Der Standort geht ohne Zitat raus, eine offene Antwortleiste bleibt für die nächste Nachricht stehen. |
| Schließen | Das Blatt zu schließen bricht ab und beendet die Ortung. Die App ortet nie im Hintergrund. |

### Blase (Android, iOS)
- Kartenbild 240 × 150 dp/pt mit den Ecken eines Bildes in der Blase, der Pin in `accent` mit der Spitze auf dem Ort, Kartenausschnitt etwa 1 km breit (auf Android je nach Zoomstufe 0,7 bis 1,4 km). Die Karte ist auch im dunklen Design hell.
- Darunter die Bezeichnung in der Textfarbe der Blase, höchstens zwei Zeilen, bei leerer Bezeichnung „Standort“. Die Uhrzeit steht darunter wie bei Text.
- Antippen öffnet die Karten-App mit Pin und Bezeichnung. Gibt es auf Android keine, öffnet sich die Stelle auf openstreetmap.org im Browser.
- Das Menü beim langen Druck ist das der anderen Nachrichten, ohne „Bearbeiten“. „Kopieren“ legt die Bezeichnung und in der zweiten Zeile den Link `https://www.openstreetmap.org/?mlat={Breite}&mlon={Länge}#map=16/{Breite}/{Länge}` ab. Breite und Länge stehen im Link immer mit sechs Nachkommastellen und Punkt (`52.528300`), wie im Kern und in Talk-Web. Reagieren und Löschen wie sonst.
- **Ausstehend:** dieselbe Blase mit der Statuszeile. Ohne Netz wartet der Standort im Postausgang wie Text.
- **Kartenquelle:**
  - iOS: Apple-Karten über `MKMapSnapshotter`.
  - Android: Kacheln von `tile.openstreetmap.org`, zu einem Bild zusammengesetzt, höchstens sechs je Bild und zwei Abrufe gleichzeitig. Jede Anfrage trägt `User-Agent: Wolkenfunk/{Version}`. Im Bild steht unten rechts klein „© OpenStreetMap“. Die Kacheln liegen mindestens sieben Tage im Zwischenspeicher der App (höchstens 20 MB), nichts wird auf Vorrat geladen.
  - Das Bild lädt erst, wenn die Blase sichtbar wird. Der Kartendienst erfährt dabei die ungefähre Gegend, auch bei empfangenen Standorten.
  - Ohne Netz oder bei einem Fehler: eine Fläche in `chip` mit dem Pin. Der nächste Versuch kommt, wenn die Blase wieder sichtbar wird.

### Kachel (Desktop)
Pin, Bezeichnung (leer: „Standort“) und darunter in `muted` die Koordinaten mit vier Nachkommastellen, „52,5283° N, 13,4356° O“. Ein Klick öffnet den Link von oben im Browser, im Menü steht dafür zusätzlich „Im Browser öffnen“ wie bei Dateien. Kein Kartenbild, also auch kein Abruf bei einem Kartendienst. „Kopieren“ wie auf Mobilgeräten.

### Vorschau
Chatliste, Zitat und Benachrichtigung zeigen „📍 {Bezeichnung}“, bei leerer Bezeichnung „📍 Standort“. Das gilt auf allen Plattformen und reiht sich in die Bezeichnungen aus `docs/notifications.md` ein.

Ausnahme ist die Benachrichtigung aus einem Push: Ihr Text kommt aus dem Push selbst, und der trägt nur die Bezeichnung, ohne „📍“ (bei Dateien ebenso nur den Dateinamen).

## Kern-Vertrag

- **Standort lesen:** `MessageView.location`, `ReplyPreview.location` und `MessagePreview.location` sind `GeoLocation { latitude, longitude, name }` (WGS 84, Grad) oder leer. `text` bleibt die Bezeichnung, damit nichts leer wird, wo der Standort nicht ausgewertet ist. Wer `location` hat, zeichnet die Blase und zeigt `text` nicht noch einmal.
- **Standort senden:** `enqueueLocation` stellt ihn in den Postausgang und gibt die ausstehende Blase zurück (`location` gesetzt, `id` negativ). Er läuft wie Text: `OutboxChanged`, „Wartet auf Netz“, `retrySend`, `discard`, übersteht einen Neustart. Die Antwort des Servers ersetzt ihn über die `referenceId`.
- **Form am Server:** `POST …/chat/{token}/share` mit `objectType=geo-location`, `objectId=geo:{Breite},{Länge}` (sechs Nachkommastellen) und `metaData` mit `name`, `latitude`, `longitude`. So lesen und senden ihn auch die offiziellen Apps. Ohne Bezeichnung trägt der Kern die Koordinaten als Namen ein, weil der Server ein Objekt ohne Namen ablehnt.
- **Rechte:** `editable` ist bei Standorten immer falsch. Reagieren und Löschen gehen wie bei Text.
- **Datei senden:** `enqueueFile(token, localPath, name, mime, width, height, caption, replyTo, referenceId)`. Der Kern verschiebt die Datei in den Postausgang, die App darf den Pfad danach nicht mehr verwenden. Über 10 MiB lädt er in Stücken und setzt nach einem Abbruch fort. Den MIME-Typ rät er nicht.
- **Datei laden:** `fetchFile(path, size)` gibt den lokalen Pfad zurück, lädt nur einmal und setzt nach einem Abbruch fort. Einen Fortschritt meldet er nicht.
- **Zum Testen:** `talk-cli send-location <token> <breite> <länge> -n <name>`. Der Demo-Server nimmt Standorte an, hat einen im Chat „Sommerfest“ und liefert mit `POST /demo/incoming` (`token`, `text`, `lat`, `lon`, optional `from`) einen fremden.

## Test

Standort-Tests laufen mit einer gesetzten Position (Emulator, Simulator), nicht mit der echten.

1. PDF mit Beschreibung senden: Kachel mit Ring, danach fertig. Die Gegenseite sieht Datei und Beschreibung.
2. Drei Dateien auf einmal: Reihenfolge wie gewählt, Beschreibung an der letzten, Zitat an der ersten.
3. Datei über 10 MiB: Der Ring läuft, auch nach dem Wechsel in den Hintergrund.
4. Ohne Netz senden: „Wartet auf Netz …“, geht danach von selbst raus.
5. Bild über „Datei“: erscheint als Bild, in Originalgröße.
6. Empfangene PDF und ein Video antippen: Kreisel, dann offen. Zweites Öffnen ohne Netz geht. Datei ohne passende App: Hinweis.
7. Standort: Berechtigung ablehnen (Hinweis mit „Einstellungen öffnen“), dann erlauben. Das Blatt zeigt Karte, Bezeichnung und Genauigkeit. Senden: Blase mit Karte, in Hell und Dunkel.
8. Standort bei offener Antwortleiste: geht ohne Zitat raus, die Leiste bleibt.
9. Fremder Standort: Blase mit Karte, Antippen öffnet die Karten-App. Chatliste, Zitat und Benachrichtigung zeigen „📍 …“.
10. Standort ohne Netz: wartet und geht danach raus. Karte ohne Netz: Fläche mit Pin.
11. Desktop: fremder Standort als Kachel in Hell und Dunkel, Klick öffnet den Browser, Seitenleiste zeigt „📍 …“.
