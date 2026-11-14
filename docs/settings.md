# Einstellungen, Darstellung und Chat-Menü

Die App bekommt einen eigenen Bereich „Einstellungen“ mit Unterseiten, ähnlich wie Signal; übernommen wird nur das Muster, keine Texte oder Farben.

Darin liegt „Darstellung“ mit Design und Akzentfarbe. Dazu kommt ein Menü an jedem Chat in der Liste: am Desktop per Rechtsklick, auf Android und iOS per langem Drücken.

## Einstieg

- Ein Klick oder Tippen auf den **eigenen Avatar** oben links in der Chatliste öffnet die Einstellungen direkt. Das gilt auf allen Plattformen.
- **Desktop:**
  - Das „⋯“-Menü enthält nur noch „Einstellungen“ und „Kategorien verwalten“ (nur bei `categoriesSupported`). Kontoinfo und „Abmelden“ ziehen in die Einstellungen.
  - Der Kurzbefehl ist Strg+, bzw. ⌘,.
  - Auf macOS steht im App-Menü unter „Über Wolkenfunk“ der Eintrag „Einstellungen …“ (⌘,).
- **Android:** Das Menü oben rechts enthält nur noch „Einstellungen“.
- **iOS:** Das bisherige Avatar-Menü wird ein Knopf, der die Einstellungen öffnet (Label „Einstellungen“).

## Aufbau

### Startseite „Einstellungen“

1. **Kopf:**
   - Avatar 64, darunter der Name.
   - Darunter gedämpft der Server ohne `https://`.
   - Bearbeiten lässt sich dort nichts.
2. **Zeilen** (Symbol links in `muted`, Titel, Pfeil rechts):
   | Zeile | Plattform | Symbol |
   |---|---|---|
   | Allgemein | nur Desktop | Zahnrad |
   | Darstellung | alle | Pinsel bzw. Palette |
   | Benachrichtigungen | nur Android und iOS | Glocke |
   | Chats | nur Desktop, nur bei `categoriesSupported` | Sprechblase |
   | Über Wolkenfunk | alle | Info-Kreis |
3. **Ganz unten** steht **„Abmelden“** in `danger`, mit der bisherigen Bestätigung der Plattform.

### „Allgemein“ (Desktop)

- Ein Schalter **„Beim Schließen im Hintergrund weiterlaufen“** mit dem Untertitel „Damit Anrufe und Nachrichten ankommen“.
- Er gilt nur unter Linux und Windows. Auf macOS fehlt die Seite, also auch ihre Zeile.
- Dahinter steht dieselbe Einstellung wie das Häkchen im Tray-Menü (`settings.closeToTray`). Beide zeigen immer denselben Stand.

### „Darstellung“ (alle)

1. **Vorschau** oben:
   - Eine fremde Blase „Wie gefällt dir die neue Farbe?“ und eine eigene Blase „Sieht gut aus!“.
   - Beide nutzen die echten Blasen-Stile, die eigene in der gewählten Farbe.
2. **„Design“** ist eine Auswahl mit drei Werten:
   - „System“ (Standard)
   - „Hell“
   - „Dunkel“
3. **„Akzentfarbe“** ist ein Raster runder Farbfelder, 40–44 groß und in mindestens 5 Spalten:
   - Jedes Feld zeigt den Akzent des aktuellen Modus.
   - Das gewählte Feld hat ein weißes Häkchen und einen Ring.
   - Das Label ist der Farbname, beim gewählten Feld mit „, ausgewählt“.
4. **Jede Änderung gilt sofort** in der ganzen App, auch in offenen Sheets und Dialogen. Einen Knopf „Speichern“ gibt es nicht.

**Was die Akzentfarbe färbt:**
- alles, was heute `accent` nutzt: Knöpfe, Links, Schalter, Häkchen, Badges, Auswahl, „Neue Gruppe“-Kreise
- die eigene Blase (`bubble`)

**Was gleich bleibt:**
- `danger`
- der grüne Anruf-Punkt
- das Anruf-Fenster, das immer dunkel ist
- das App-Icon

### „Benachrichtigungen“ (Android und iOS)

- Eine Zeile **„Systemeinstellungen öffnen“**:
  - Android: `Settings.ACTION_APP_NOTIFICATION_SETTINGS`
  - iOS: `UIApplication.openNotificationSettingsURLString`
- Darunter ein Hinweis in `muted`: „Für einzelne Chats: Chat in der Liste lange gedrückt halten.“

### „Chats“ (Desktop)

- Die Zeile **„Kategorien verwalten“** öffnet die bestehende Seite (docs/categories.md).

### „Über Wolkenfunk“ (alle)

- **App-Icon** 64 und „Wolkenfunk“.
- Darunter „Version X.Y.Z“ und gedämpft „Kern X.Y.Z“.
- **„Nach Updates suchen“** gibt es nur im AppImage und unter Windows (wo `update.ts` aktiv ist):
  - Während der Suche steht dort „Suche …“.
  - Danach „Wolkenfunk ist aktuell“ oder der vorhandene Ablauf „Update bereit“.
- **Links**, die im Browser öffnen:
  - „Webseite“ → `https://wolkenfunk.org`
  - „Quellcode“ → `https://github.com/raph-ael/nextcloud-talk-legacy`
- **„Lizenz: AGPL-3.0“** als Text.

## Layout je Plattform

- **Desktop:**
  - Die Einstellungen ersetzen das ganze Fenster und bestehen aus zwei Spalten.
  - **Links**, in der Breite der Chatliste:
    - der Kopf mit dem Zurück-Pfeil „Zurück zu den Chats“ und dem Titel „Einstellungen“,
    - darunter Profil und Zeilen.
    - Die gewählte Zeile ist hervorgehoben wie ein aktiver Chat.
  - **Rechts** steht die Unterseite mit dem Seitentitel oben. Beim Öffnen ist „Darstellung“ gewählt.
  - Esc oder der Pfeil führen zurück zu den Chats. Ein offener Chat bleibt mit Entwurf und Scrollposition darunter erhalten. Ein laufender Anruf ist davon nicht betroffen.
- **Android:**
  - Ein eigener Bildschirm als Overlay, gleitet von rechts herein wie `GroupInfoScreen`.
  - Unterseiten gleiten innerhalb davon herein. Die Zurück-Taste geht eine Ebene zurück.
- **iOS:** Push in den bestehenden `NavigationStack`, mit Routen für die Startseite und je Unterseite, in einer gruppierten Liste.

## Speicherung

Die Einstellungen gelten nur für das Gerät. Sie gehören nicht zum Konto und bleiben beim Abmelden erhalten.

| Plattform | Ort | Schlüssel |
|---|---|---|
| Desktop | `settings.json` im Datenordner (Main-Prozess) | `theme`: `system`, `light` oder `dark`; `accent`: Farb-id |
| Android | SharedPreferences `ui` | `theme`, `accent` |
| iOS | UserDefaults | `appearance.theme`, `appearance.accent` |

Fehlt der Wert oder ist die id unbekannt, gelten `system` bzw. `blau`.

## Akzentfarben

**Quelle:** `design/theme/accents.json`. Nach jeder Änderung läuft `python3 design/theme/build-theme.py`. Das Skript:
- prüft den Kontrast:
  - weiße Schrift auf der eigenen Blase, dem hellen Akzent und Hover: mindestens 4,5:1,
  - dunkler Akzent: mindestens 4:1, sowohl mit weißer Schrift als auch auf #121212,
- schreibt die eingecheckten Tabellen:
  - `desktop/src/renderer/src/accents.ts`
  - `android/…/ui/Accents.kt`
  - `ios/TalkApp/UI/Accents.swift`

Diese Dateien werden nie von Hand geändert.

**Jeder Eintrag** hat:
- `id` und `name`
- je Modus (`light`/`dark`): `accent`, `hover` und `bubble`. Die Blase ist in beiden Modi dieselbe, damit weiße Schrift gut lesbar bleibt.

**Reihenfolge im Raster:** Blau (Standard), Indigo, Violett, Pink, Rot, Orange, Bernstein, Grün, Petrol, Schiefergrau.

**Umsetzung:**
- **Desktop:**
  - Der Main-Prozess setzt `nativeTheme.themeSource`. Damit folgen `prefers-color-scheme` im CSS, der Fensterhintergrund, das Tray-Symbol und die Avatare.
  - Der Renderer setzt `--accent`, `--accent-hover` und `--bubble-own` auf `:root`, passend zum aktuellen Modus. Bei einem Moduswechsel setzt er sie neu.
  - Die IPC-Aufrufe sind `appearance()` mit `{theme, accent}`, `setAppearance(theme, accent)` und das Ereignis `appearanceChanged`.
- **Android:**
  - `TalkTheme(dark, accent)` liest einen `AppearanceStore` (einen StateFlow über die SharedPreferences).
  - `primary` und `TalkColors.accent`/`bubbleOwn` kommen aus der Palette.
  - Die Systemleisten folgen dem gewählten Design (`enableEdgeToEdge(SystemBarStyle…)`).
  - `CallActivity` bleibt dunkel.
- **iOS:**
  - `Talk.accent` und `Talk.bubbleOwn` lesen ein beobachtbares `Appearance`.
  - An der Wurzel sitzen `.tint` und `.preferredColorScheme`. Damit auch Sheets und Alerts folgen, setzt die App zusätzlich `overrideUserInterfaceStyle` am Fenster.
  - Das Asset `AccentColor` bekommt die Blau-Werte (Startbildschirm).

## Chat-Menü

Es öffnet sich am Desktop per Rechtsklick auf einen Chat in der Liste, mit der Tastatur über die Kontextmenü-Taste oder Umschalt+F10 auf dem fokussierten Chat. Auf Android und iOS öffnet es langes Drücken.

**Einträge, in dieser Reihenfolge:**

| Eintrag | Wann | Aufruf |
|---|---|---|
| „Als gelesen markieren“ | nur bei `unreadMessages > 0` | `markRoomRead(token)` |
| „Zu Favoriten hinzufügen“ bzw. „Aus Favoriten entfernen“ | immer | `setFavorite(token, on)` |
| „Kategorie ›“ | nur bei `categoriesSupported` | siehe unten |
| „Benachrichtigungen ›“ | immer | `setNotificationLevel(token, 1/2/3)` |
| „Gruppeneinstellungen“ | nur Raumtyp 2 und 3 | öffnet die Gruppeneinstellungen dieses Chats |

**„Kategorie ›“:**
- Darin stehen alle Kategorien mit `kind == "custom"` aus `categories()` in ihrer Reihenfolge, mit Häkchen, wenn ihre id in `tagIds` des Chats steht.
- Ein Klick schaltet die Kategorie um: `setRoomCategories(token, neue Liste)`. Das Menü bleibt dabei offen (Ausnahme iOS, siehe unten).
- Danach kommt ein Trenner und **„Neue Kategorie …“**: Ein Namensfeld wie auf der Seite „Kategorien“ fragt den Namen ab, dann wird die Kategorie mit `createCategory(name, [token])` angelegt.
- Ohne eigene Kategorien steht dort nur „Neue Kategorie …“.

**„Benachrichtigungen ›“:**
- Eine Auswahl aus „Alle Nachrichten“ (1), „Nur Erwähnungen“ (2) und „Aus“ (3), markiert nach `notificationLevel`.
- Bei Einzelchats (Raumtyp 1, 5 und 6) fehlt „Nur Erwähnungen“.

**Fehler:** Die Aufrufe geben `null` oder einen deutschen Text zurück, der als Toast erscheint. Favorit und „Als gelesen“ zeigen die Änderung sofort, denn der Kern sendet `RoomsChanged` vor dem Serveraufruf und nimmt die Änderung bei einem Fehler zurück.

**Je Plattform:**
- **Desktop:**
  - Das vorhandene HTML-Menü (`MessageMenu.tsx`) bekommt Untermenüs und Einträge mit Häkchen oder Radio.
  - Ein Untermenü öffnet sich, wenn man darüberfährt oder → drückt, und schließt mit ← oder Esc.
  - So folgt das Menü Design und Akzentfarbe, was ein natives Menü nicht täte.
- **Android:**
  - Langes Drücken (`combinedClickable`, mit haptischem Feedback) öffnet ein `ModalBottomSheet`. Oben stehen Avatar und Name, darunter die Zeilen.
  - „Kategorie“ und „Benachrichtigungen“ öffnen jeweils ein zweites Blatt mit Häkchen bzw. Radio.
- **iOS:**
  - `.contextMenu` an der Zeile.
  - „Kategorie“ ist ein verschachteltes `Menu` mit Toggles, „Benachrichtigungen“ ein `Picker` als Menü.
  - Anders als am Desktop schließt das Menü nach einem Kategorie-Haken. iOS zeichnet ein offenes Menü nicht neu, der Haken wäre sonst erst beim nächsten Öffnen zu sehen.
  - „Neue Kategorie …“ öffnet einen Alert mit Textfeld.

## Stummgeschaltete Chats in der Liste

`notificationLevel` liefert immer 1, 2 oder 3: Der Kern ersetzt den Server-Standard 0, bei Einzelchats durch 1 und bei Gruppen durch 2, wie Talk selbst.

| Stufe | Liste | Badge an App, Dock bzw. Tray |
|---|---|---|
| 1 „Alle Nachrichten“ | wie bisher | zählt mit |
| 2 „Nur Erwähnungen“ | Zahl der Ungelesenen grau statt in `accent`, außer bei `unreadMention` | zählt nur bei `unreadMention` |
| 3 „Aus“ | kleine durchgestrichene Glocke in `muted` hinter dem Namen, Zahl grau | zählt nicht mit |

Das „@“-Schild bei einer Erwähnung bleibt immer in `accent`.

## Kern-Vertrag

Desktop (`talk-node`) und mobil (`talk-ffi`) bekommen dieselben Aufrufe:

| Aufruf | Rückgabe |
|---|---|
| `setFavorite(token, on)` | Fehlertext oder `null` |
| `setRoomCategories(token, ids)` | Fehlertext oder `null`; danach steht der Chat genau in diesen eigenen Kategorien |
| `markRoomRead(token)` | Fehlertext oder `null` |
| `setNotificationLevel(token, level)` | vorhanden |

Nur in `talk-ffi`, auf dem Desktop schon vorhanden:
- `categoriesSupported()`
- `categories()`
- `createCategory(name, tokens)`, gibt `{id, error}` zurück

## Barrierefreiheit

| Element | Bezeichnung |
|---|---|
| Eigener Avatar oben links | „Einstellungen“ |
| Zeile der Startseite | ihr Titel, als Knopf |
| Farbfeld | „{Farbname}“ bzw. „{Farbname}, ausgewählt“ |
| Design-Auswahl | Radiogruppe „Design“ |
| Chat-Zeile | zusätzliche Aktion „Chat-Menü“ (Android `customActions`, iOS `accessibilityActions` bzw. das Kontextmenü, Desktop `aria-haspopup="menu"`) |
| Durchgestrichene Glocke | „stummgeschaltet“, im Label der Zeile |
| Untermenü-Eintrag (Desktop) | `aria-haspopup="menu"` und `aria-expanded`; Einträge mit `menuitemcheckbox` bzw. `menuitemradio` |

## Test

- **Der Demo-Server reicht für alles.** Ihn startet `cargo run -p talk-demo` in `core/`, jede Anmeldung geht. Er speichert Favorit, Kategorien eines Chats, Benachrichtigungsstufe und Lesemarke im Speicher.
  - „Sommerfest“ steht dort auf „Aus“.
  - „Gartenprojekt“, „Team Website“ (mit Erwähnung) und „Familie“ haben Ungelesenes.
- **Echte Server:** nur in den Testräumen bzw. dem Bot-1:1, unter `bot-lock.sh`. Echte Chats werden nie als Favorit markiert, stummgeschaltet oder als gelesen markiert.
- **Zu prüfen:**
  - Design umschalten: System, Hell, Dunkel; auch mit offenem Sheet.
  - Alle 10 Farben in hell und dunkel: Knöpfe, eigene Blase, Schalter, Badge, Links.
  - Nach einem Neustart sind die Werte noch da.
  - Das Chat-Menü komplett, jeder Eintrag.
  - Stumm-Anzeige und Badge-Zählung wie in der Tabelle oben.
- **Kern:** `cargo test -p talk-core -p talk-demo` (Ende-zu-Ende gegen den Demo-Server).
