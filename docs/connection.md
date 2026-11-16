# Verbindungsanzeige

Ein kurzer Aussetzer ist kein Fehler. Die App zeigt dann nur einen kleinen Ladekreis in der Kopfzeile. Erst wenn die Verbindung wirklich weg ist, steht dort grau „Keine Verbindung“. Einen Balken gibt es nicht. Dieser Vertrag legt fest, was auf allen Plattformen gleich ist.

## Drei Zustände

Der Kern meldet den Zustand je Konto als `connection { state }` und beantwortet ihn jederzeit mit `connectionState()`. Das Ereignis kommt nur bei einem Wechsel.

| Zustand | Bedeutung | Anzeige |
|---|---|---|
| `online` | Der letzte Abruf wurde beantwortet. | nichts |
| `connecting` | Die Sitzung ist gestartet und hat noch keine Antwort, oder ein Abruf ist gerade fehlgeschlagen und der Kern versucht es neu. | Ladekreis |
| `offline` | Auch die Neuversuche sind fehlgeschlagen. | „Keine Verbindung“, in der Chatliste mit Ladekreis |

Eine Sitzung beginnt als `connecting`. Dafür kommt kein Ereignis: Die Plattform setzt den Anfangswert selbst oder fragt `connectionState()`.

### Wie der Kern wechselt
- **Ein Abruf bekommt keine Antwort** (kein Netz, Zeitüberschreitung, DNS, TLS): `online` wird zu `connecting`.
  - Der Kern holt die Chatliste nach 0,5 s neu, danach nach 1 s, 2 s und 4 s.
  - Schlagen zwei Neuversuche fehl oder sind 8 s ohne Antwort vergangen, wird daraus `offline`. Ohne Netz sind das etwa 1,5 s.
  - Danach versucht es der gewöhnliche Abgleich weiter, alle 10 s.
- **Ein Abruf wird beantwortet:** `online`, aus jedem Zustand.
- **`networkChanged()`** löst sofort einen Neuversuch aus. Aus `offline` wird dabei nicht erst `connecting`: Der Zustand bleibt stehen, bis eine Antwort kommt. So flackert die Anzeige nicht, wenn das Gerät ein Netz meldet, das den Server nicht erreicht.
- Eine Fehlerantwort des Servers (500, Wartung) ändert den Zustand nicht. Ausnahme ist der Start: Liefert er nur Fehler, zählt das wie „keine Antwort“.
- Beim Start trägt der Kern erst die Chatliste ein (`roomsChanged`) und meldet dann `online`.

### Was die Plattform beisteuert
- `networkChanged()` bei jedem Netzwechsel und immer, wenn die App in den Vordergrund kommt. Nach dem Aufwachen sind offene Verbindungen oft tot; der erste Abruf scheitert dann, und der Neuversuch soll sofort laufen.
- Den Zustand je Konto merken. Beim Kontowechsel zeigt die Anzeige den Zustand des neuen Kontos.

## Anzeige

### Chatliste
- `connecting`: ein Ladekreis neben dem Titel „Chats“.
- `offline`: Ladekreis und daneben „Keine Verbindung“ in der gedämpften Textfarbe, als zweite Zeile unter dem Titel oder an der Stelle, die die Kopfzeile der Plattform dafür hat.

### Chat
- `connecting`: ein Ladekreis neben dem Namen.
- `offline`: „Keine Verbindung“ in der gedämpften Textfarbe unter dem Namen, ohne Ladekreis. Eine Beschreibungszeile, die dort sonst steht, weicht solange.

### Leere Chatliste
- `connecting`: ein Ladekreis in der Mitte, in der Größe, die die Plattform sonst für ladende Inhalte nutzt. Die Liste lädt noch. Der Ladekreis in der Kopfzeile entfällt solange, es steht nie zweimal derselbe da.
- `online`: „Noch keine Chats“.
- `offline`: nichts in der Mitte. Die Kopfzeile sagt, was los ist.

### Regeln für alle
- Der Ladekreis in der Kopfzeile ist klein (14 bis 16 Punkte) und dünn. Jeder Ladekreis hier hat die gedämpfte Textfarbe, nicht die Akzentfarbe.
- Er blendet erst ein, wenn der Zustand 500 ms angehalten hat, auch der in der leeren Liste. Kürzere Phasen bleiben unsichtbar. Das Ausblenden geht sofort.
- Kopfzeile und Liste springen nicht: Der Platz für Ladekreis und Text ist reserviert oder liegt so, dass nichts verrutscht.
- Für Vorlesefunktionen heißt der Ladekreis „Verbinde …“.
- Farben kommen aus den vorhandenen Tönen des Designs, hell und dunkel.

## Was unverändert bleibt
- Eine Nachricht im Postausgang zeigt „Wartet auf Netz“, sobald ihr eigener Versuch gescheitert ist. Nachrichten dahinter zeigen es erst bei `offline`.
- Fehlertexte in Dialogen und Hinweisen („Keine Verbindung“ beim Suchen, bei Reaktionen, in den Gruppeneinstellungen).
- Die Echtzeit-Verbindung (notify_push) hat keine eigene Anzeige. Sie verbindet sich still neu.

## Zu prüfen
- [ ] Aussetzer von 2 s: nur der Ladekreis, kein Text.
- [ ] 15 s ohne Server: „Keine Verbindung“, danach wieder nichts.
- [ ] App nach einigen Minuten im Hintergrund öffnen: höchstens kurz der Ladekreis.
- [ ] Flugmodus an und aus.
- [ ] Zwei Konten, eines ohne Verbindung: Jedes zeigt seinen Zustand.
- [ ] Erster Start nach der Anmeldung: Ladekreis in der leeren Liste, nie „Noch keine Chats“.
- [ ] Hell und dunkel.
