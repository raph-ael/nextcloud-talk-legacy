# Anhänge – Dateien, Standort und Kontakt

Verbindlich für Android und iOS, wo nichts anderes steht. Der Desktop sendet Dateien schon (Büroklammer, Ablegen, Einfügen) und liest Standorte und Kontakte, er sendet keine. Bilder aus der Galerie, Kamera und Sprachnachrichten bleiben, wie sie sind.

Einstieg ist auf Mobilgeräten das Anhang-Menü (＋) in der Eingabeleiste. Es hat der Reihe nach „Galerie“, „Datei“, „Standort“ und „Kontakt“, auf iOS steht vor „Kontakt“ noch „Kamera“. Jeder Knopf tut etwas: Einen Platzhalter mit „kommt bald“ gibt es nicht mehr. „Umfrage“ ist ausgeblendet, bis es Umfragen gibt. Beim Bearbeiten einer Nachricht ist das Menü ausgeblendet (`docs/chat.md`).

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

## Kontakt

Gesendet wird ein Eintrag aus dem Adressbuch des Geräts. Talk kennt dafür keine eigene Nachricht: Ein Kontakt ist eine vCard-Datei (`.vcf`), die der Server als Kontakt ausweist. So teilen ihn auch die offiziellen Apps, und Talk-Web zeigt ihn als Kontakt.

### Senden
| Punkt | Regel |
|---|---|
| Einstieg | „Kontakt“ im Anhang-Menü öffnet die Kontaktauswahl des Systems, für einen Kontakt. Abbrechen ändert nichts. |
| Berechtigung | iOS braucht keine: Die Auswahl läuft im System, die App bekommt nur den gewählten Eintrag. Android fragt beim ersten Mal nach der Berechtigung „Kontakte“, bevor sich die Auswahl öffnet. Abgelehnt: „Wolkenfunk darf die Kontakte nicht lesen“ mit „Einstellungen öffnen“. Wer dort erlaubt und zurückkommt, landet von selbst in der Auswahl. Die App liest nur den gewählten Eintrag und nur in diesem Moment, nie das Adressbuch als Ganzes und nie im Hintergrund. |
| Blatt | Titel „Kontakt senden“. Oben der Avatar mit den Initialen, der Name und in `muted` die Organisation. Darunter je Telefonnummer, E-Mail-Adresse und Anschrift eine Zeile mit Haken, dem Wert und in `muted` der Art („Mobil“, „Privat“, „Arbeit“, sonst nichts). Alle Zeilen sind angehakt, Antippen nimmt den Haken weg. Unten der Knopf „Kontakt senden“. |
| Was mitgeht | Der Name, die Organisation und die angehakten Zeilen. Nie das Foto, Notizen, Geburtstag, Spitzname, Beziehungen, Webseiten oder andere Felder. |
| Leer | Ohne angehakte Zeile ist der Knopf aus. Hat der Eintrag weder Nummer noch E-Mail-Adresse noch Anschrift, steht statt der Zeilen „Dieser Kontakt hat keine Nummer und keine Adresse“. |
| Senden | `enqueueContact(token, card, replyTo, referenceId)`. Das Blatt und das Anhang-Menü schließen, die Antwortleiste auch (das Zitat geht mit), und der Chat steht am Ende. Eine Beschreibung gibt es nicht. |
| Name | Der Name, wie ihn das Adressbuch zeigt. Fehlt er, nimmt der Kern Vor- und Nachname, sonst die Organisation, sonst die erste Nummer oder E-Mail-Adresse, die mitgeht. Die App übergibt als `name` nur einen echten Namen: Zeigt das Adressbuch an seiner Stelle die Organisation, eine Nummer oder den Spitznamen, bleibt `name` leer. Sonst stünde womöglich eine abgewählte Nummer im Namen. Das Blatt zeigt den Namen, der gesendet wird. |
| Doppelte | Gleiche Werte stehen nur einmal im Blatt (verknüpfte Einträge liefern sie mehrfach). Nummern gelten ohne Leer- und Trennzeichen als gleich, E-Mail-Adressen ohne Rücksicht auf Groß und Klein. Eine Anschrift, die nur als ein Text gespeichert ist, geht als Straße. |
| Fehler | „Kontakt konnte nicht gelesen werden“, wenn der gewählte Eintrag sich nicht lesen lässt. „Kontakt konnte nicht gespeichert werden“, wenn der Kern die Karte nicht ablegen kann. Beim Öffnen einer Kontaktkachel ohne passende App: „Keine Kontakte-App gefunden“. |

### Kontaktkachel (alle Plattformen)
- Avatar-Kreis mit den Initialen wie bei einem Chat ohne Bild, daneben der Name (höchstens zwei Zeilen) und darunter in `muted` „Kontakt“. Die Uhrzeit steht darunter wie bei einer Datei. Ein Foto zeigt die Kachel nicht.
- Sie erscheint für jede Datei mit `attachment.contactName`, gleich von wem und womit sie gesendet wurde, also auch für eine `.vcf`, die am Desktop als Datei angehängt wurde. Eine Beschreibung steht darunter wie bei Dateien.
- **Ausstehend:** wie die Dateikachel. Der Ring steht statt des Avatars, die Statuszeile ist dieselbe, Antippen einer fehlgeschlagenen sendet erneut.
- **Antippen** (Android, iOS) lädt die Datei wie unter „Empfangene Datei öffnen“ und reicht sie dem System: auf Android an die Kontakte-App („Öffnen mit“, Typ `text/x-vcard`), auf iOS an die Vorschau, die den Kontakt mit „Neuen Kontakt erstellen“ zeigt. Dort entscheidet der Benutzer über das Speichern. Die App selbst schreibt nichts ins Adressbuch.
- **Desktop:** dieselbe Kachel. Ein Klick öffnet wie bei Dateien die Nextcloud-Seite im Browser.
- Das Menü ist das einer Datei.

### Vorschau
Chatliste, Zitat und Benachrichtigung zeigen „👤 {Name}“, auf allen Plattformen. Mit Beschreibung steht sie wie bei Dateien dahinter. Die Benachrichtigung aus einem Push trägt nur den Dateinamen („Karla Brandt.vcf“), siehe oben.

## Kern-Vertrag

- **Standort lesen:** `MessageView.location`, `ReplyPreview.location` und `MessagePreview.location` sind `GeoLocation { latitude, longitude, name }` (WGS 84, Grad) oder leer. `text` bleibt die Bezeichnung, damit nichts leer wird, wo der Standort nicht ausgewertet ist. Wer `location` hat, zeichnet die Blase und zeigt `text` nicht noch einmal.
- **Standort senden:** `enqueueLocation` stellt ihn in den Postausgang und gibt die ausstehende Blase zurück (`location` gesetzt, `id` negativ). Er läuft wie Text: `OutboxChanged`, „Wartet auf Netz“, `retrySend`, `discard`, übersteht einen Neustart. Die Antwort des Servers ersetzt ihn über die `referenceId`.
- **Form am Server:** `POST …/chat/{token}/share` mit `objectType=geo-location`, `objectId=geo:{Breite},{Länge}` (sechs Nachkommastellen) und `metaData` mit `name`, `latitude`, `longitude`. So lesen und senden ihn auch die offiziellen Apps. Ohne Bezeichnung trägt der Kern die Koordinaten als Namen ein, weil der Server ein Objekt ohne Namen ablehnt.
- **Rechte:** `editable` ist bei Standorten immer falsch. Reagieren und Löschen gehen wie bei Text.
- **Datei senden:** `enqueueFile(token, localPath, name, mime, width, height, caption, replyTo, referenceId)`. Der Kern verschiebt die Datei in den Postausgang, die App darf den Pfad danach nicht mehr verwenden. Über 10 MiB lädt er in Stücken und setzt nach einem Abbruch fort. Den MIME-Typ rät er nicht.
- **Datei laden:** `fetchFile(path, size)` gibt den lokalen Pfad zurück, lädt nur einmal und setzt nach einem Abbruch fort. Einen Fortschritt meldet er nicht.
- **Kontakt lesen:** `FileAttachment.contactName` ist der Name der Person, wenn die Datei eine vCard ist (`text/vcard`), sonst leer. Bei einer vCard ist er nie leer: Der Server liest ihn aus der Karte (`contact-name`), sonst nimmt der Kern den Dateinamen ohne `.vcf`. Das Feld steht überall, wo ein Anhang steht (`MessageView`, `ReplyPreview`, `MessagePreview`). Wer es hat, zeichnet die Kontaktkachel statt der Dateikachel. Eine ausstehende Datei mit der Endung `.vcf` hat es von Anfang an, gleich welchen Typ das System gemeldet hat, denn der Server weist jede `.vcf` als vCard aus.
- **Kontakt senden:** `enqueueContact(token, card, replyTo, referenceId)` baut aus `ContactCard` eine vCard 3.0, legt sie als „{Name}.vcf“ in den Postausgang und gibt die ausstehende Blase zurück (`attachment.contactName` gesetzt). Danach läuft sie wie jede Datei. Der Kern sendet genau, was in `card` steht: Die Auswahl trifft die App.
  - `ContactCard { name, givenName, familyName, organization, phones, emails, addresses }`. `phones` und `emails` sind `ContactValue { kind, value }`, `addresses` sind `ContactAddress { kind, street, city, region, postalCode, country }`.
  - `kind` ist „mobile“, „home“, „work“ oder leer. Alles andere (Fax, Pager, eigene Bezeichnungen) geht als leer.
  - Leere Werte lässt der Kern weg. Der Dateiname enthält keine Zeichen, die ein Dateisystem ablehnt.
- **Zum Testen:** `talk-cli send-location <token> <breite> <länge> -n <name>`, für einen Kontakt `talk-cli send-file <token> <datei.vcf>`. Der Demo-Server nimmt Standorte und Kontakte an, hat im Chat „Sommerfest“ einen Standort und den Kontakt „Karla Brandt“ und liefert mit `POST /demo/incoming` (`token`, `text`, optional `from`) eine fremde Nachricht: mit `lat` und `lon` einen Standort, mit `contact=1` einen Kontakt, `text` ist dann der Name.

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

Kontakt-Tests laufen mit erfundenen Kontakten im Emulator und im Simulator. Auf einem echten Gerät wird kein Eintrag aus dem Adressbuch gewählt und die Kontaktauswahl nicht fotografiert.

12. Anhang-Menü: „Galerie“, „Datei“, „Standort“, „Kontakt“ (iOS mit „Kamera“), keine „Umfrage“, ohne Wischen zu sehen.
13. Kontakt mit zwei Nummern, E-Mail-Adresse und Anschrift wählen: Das Blatt zeigt alles angehakt. Eine Nummer abwählen und senden: Kachel mit Ring, danach fertig. Die Gegenseite sieht in Talk-Web einen Kontakt, die Datei enthält die abgewählte Nummer nicht.
14. Kontakt ohne Nummer und Adresse: Hinweis, der Knopf ist aus. Alle Haken weg: Knopf aus.
15. Android: Berechtigung ablehnen (Hinweis mit „Einstellungen öffnen“), dann erlauben.
16. Kontakt bei offener Antwortleiste: geht mit Zitat raus, die Leiste schließt.
17. Ohne Netz senden: „Wartet auf Netz …“, geht danach von selbst raus.
18. Fremder Kontakt: Kachel mit Initialen und Name, in Hell und Dunkel. Antippen öffnet ihn im System. Chatliste, Zitat und Benachrichtigung zeigen „👤 …“.
19. Desktop: fremder Kontakt als Kachel in Hell und Dunkel, Seitenleiste zeigt „👤 …“. Eine am Desktop angehängte `.vcf` erscheint als Kontakt.
