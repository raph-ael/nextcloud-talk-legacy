# Screenshots

Die Bilder für die Webseite und die Stores entstehen gegen den Demo-Server `core/talk-demo`, also mit erfundenen Personen und Chats. Echte Konten oder Server sind nie beteiligt.

```
scripts/screenshots/demo-server.sh start      # baut und startet talk-demo auf :8099
scripts/screenshots/desktop.sh                # Linux-Desktop (Electron, CDP)
scripts/screenshots/android.sh                # Emulator
scripts/screenshots/ios.sh                    # auf dem Mac-Host: Simulator
scripts/screenshots/demo-server.sh stop
```

- **Login:** Jeder Benutzer und jedes Passwort gehen. Mit dem Benutzer `mira` zeigt der eigene Avatar „M“.
- **Uhrzeiten:** Die Nachrichten liegen relativ zum Serverstart. Tagsüber starten, damit die heutigen Nachrichten auf „Heute“ fallen.
- **Cache leeren:** Vor jedem Lauf den Cache der App leeren, sonst stehen alte Zeiten drin.
- **Inhalt ändern:** Texte und Personen stehen in `core/talk-demo/fixtures/`, Bilder und Ton erzeugt `fixtures/make-assets.py`.

## Szenen

Die Szenen sind auf allen Plattformen gleich:

| Name | Inhalt |
|---|---|
| `chats` | Chatliste mit den Kategorien „Familie & Freunde“ und „Arbeit“. Auf dem Desktop ist zusätzlich „Team Website“ offen. |
| `ungelesen` | „Gartenprojekt“ geöffnet an der Linie „5 ungelesene Nachrichten“, mit Bild |
| `sprachnachricht` | „Familie“ mit der Sprachnachricht |

Jede Szene gibt es `hell` und `dunkel`.

## Ausgabe

- Ziel ist `website/public/screenshots/<plattform>-<szene>-<hell|dunkel>.webp`, mit WebP-Qualität 85. `<plattform>` ist `desktop`, `mac`, `android` oder `ios`.
- Rohbilder kommen nicht ins Repo.
- Für AppStream bzw. Flathub braucht es zusätzlich `website/public/screenshots/desktop-chat.png`.
- **Feste Größen:**
  - Desktop: Fenster 1280×800 bei Faktor 1.
  - Android: Emulator Pixel, 1080×2400, Statusleiste im Demo-Modus mit 9:41.
  - iOS: iPhone 16 Pro, Statusleiste per `simctl status_bar override` auf 9:41.
