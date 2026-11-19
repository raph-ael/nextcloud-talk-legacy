# Kompakte Ansicht (nur Desktop)

Eine zweite Ansicht für Linux, Windows und macOS. Sie zeigt eine einzeilige Chatliste und im Chat Nachrichten untereinander statt Blasen links und rechts, nach dem Muster von Team-Messengern. Farben, Schrift, Abstände und Akzentfarbe bleiben unsere. Es gibt keine farbige Seitenleiste und keine Kanal-Symbole.

**Einstieg:** Einstellungen → „Darstellung“ → „Ansicht“: „Blasen“ (Standard) oder „Kompakt“ (docs/settings.md). Der Wechsel gilt sofort, auch in einem offenen Chat.

**Technik:**
- Gespeichert ist `layout: "bubbles" | "compact"` in `settings.json`.
- Der Renderer setzt `data-layout` auf `<html>`.
- Die Regeln stehen in einem eigenen CSS-Block unter `:root[data-layout="compact"]` und nutzen nur die vorhandenen Farb-Tokens. So funktionieren Hell, Dunkel und alle Akzentfarben ohne neue Werte.
- In JSX verzweigen nur `RoomItem` (`Sidebar.tsx`) und `MessageRow` (`Chat.tsx`).

## Chatliste

| Punkt | Regel |
|---|---|
| Zeile | 34 hoch, Avatar 22, Abstand 8, Ecken 6. Nur der Name, einzeilig mit „…“. Keine Vorschau, keine Uhrzeit. |
| Ungelesen | Name halbfett in `text`. Gelesene Chats in normaler Schrift und `muted`. |
| Rechts | in dieser Reihenfolge: Anrufpunkt, „@“-Schild, Zähler, alle wie bisher. Die durchgestrichene Glocke steht wie bisher hinter dem Namen. |
| Aktiv, Überfahren | wie bisher (`--fill-pressed`, `--fill`) |
| Unverändert | Abschnittsköpfe, Chat-Menü und Tastatur. Treffer in Nachrichten (`HitRow`) behalten die zweite Zeile mit dem Auszug. |
| Barrierefreiheit | Das Label der Zeile nennt weiter die letzte Nachricht und die Uhrzeit, auch wenn sie nicht zu sehen sind. |

## Chat

**Aufbau einer Nachricht:**
| Punkt | Regel |
|---|---|
| Ausrichtung | Alle Nachrichten linksbündig, auch die eigenen. Keine Blasenfläche. Volle Breite mit 20 Rand. |
| Kopf einer Folge | Links ein Avatar 36. Daneben in einer Zeile: Name (halbfett, `text`) und Uhrzeit (12, `muted`). Darunter der Text. |
| Folge | Dieselbe Person innerhalb von 3 Minuten (`continues()`) bekommt keinen neuen Kopf. Beim Überfahren steht die Uhrzeit klein und `muted` in der Avatar-Spalte. |
| Einzelchats | Auch dort Avatar und Name, damit beide Ansichten gleich lesbar sind. |
| Überfahren | Die Zeile wird leicht hinterlegt (`fill` zu 60 %). Reaktion, Antworten und „Mehr“ liegen als kleine schwebende Leiste oben rechts in der Zeile (`bg-elev`, dünner Rand, leichter Schatten). |

**Inhalte:**
| Punkt | Regel |
|---|---|
| Zitat | Schmaler Balken links (`text` zu 14 %), darin Name und Text in `muted`. Ein Klick springt wie bisher. |
| Bild | Höchstens 360 breit, Ecken 8. Die Uhrzeit steht im Kopf, nicht auf dem Bild. |
| Datei, Kontakt, Standort, Sprachnachricht | Als Karte mit dünnem Rand (`text` zu 14 %), Ecken 10, höchstens 420 breit. |
| Reaktionen | Unter der Nachricht, linksbündig, ohne Überlappung. Das Popover öffnet nach rechts. |
| Am Ende des Textes | Klein in `muted`: „bearbeitet“, „Wird gesendet …“ bzw. „Fehlgeschlagen“ (in `danger`, ein Klick sendet neu) und bei der letzten eigenen Nachricht einer Folge die Häkchen. Bei einem Bild ohne Text steht das unter dem Bild. |
| Unverändert | Systemmeldungen, Datumstrenner, Ungelesen-Linie, Datums-Pille, Erwähnungen (`docs/mentions.md`), Suchmarken, Aufleuchten nach einem Sprung, Kontextmenü, Doppelklick zum Antworten, Eingabefeld |

**Technische Haken, die bleiben müssen:**
- Das Element mit dem Text hat weiter die Klasse `bubble` und `data-id`. Daran hängen der Sprung, „Erwähnung gesehen“ und der Anker des Reaktions-Popovers. Aufleuchten lässt in der kompakten Ansicht die ganze Zeile.
- Eigene Nachrichten tragen hier die Klasse `other`. So greift keine Regel für eigene Blasen (Farbe, rechts).
- Die Zeile hat weiter `data-ts`. Daran hängen die Datums-Pille und das Halten der Scrollposition.
- Beim Umschalten bleiben der Entwurf und ungefähr die Scrollposition erhalten.

## Test

Gegen `talk-demo` (`cargo run -p talk-demo` in `core/`), in Hell und Dunkel, unter Linux und auf dem Mac:
1. Chatliste einzeilig. Zähler, „@“-Schild, Anrufpunkt und stumme Glocke sind zu sehen.
2. Eigene und fremde Nachrichten stehen linksbündig. Folgen haben keinen erneuten Kopf, beim Überfahren erscheint die Uhrzeit.
3. Antwort, Zitat, Bild, Datei, Sprachnachricht, Standort, Reaktion, Erwähnung, Bearbeiten und Löschen. Wartende und fehlgeschlagene Nachrichten erscheinen ebenfalls.
4. Der Sprung aus der Suche und der „@“-Knopf lassen die richtige Nachricht aufleuchten. Ungelesen-Linie und Datums-Pille stimmen.
5. Hin- und Zurückschalten bei offenem Chat: Entwurf und Scrollposition bleiben.
