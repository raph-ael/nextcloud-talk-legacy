# Releases

So entsteht eine neue Version von Wolkenfunk für alle Plattformen, und so kommt sie zu den Nutzern.
Für jeden Schritt gibt es ein Skript, von Hand kommt nichts dazu.

## Kanäle und Updates

| Plattform | Paket | Woher | Updates |
|---|---|---|---|
| Windows | NSIS-Installer `.exe`, pro Benutzer, ohne Admin | pkg.wolkenfunk.org/windows | **aktualisiert sich selbst** (electron-updater, `latest.yml`) |
| macOS | `.dmg`, universal, ad-hoc signiert | pkg.wolkenfunk.org/mac | keine (erst mit Developer-ID-Signatur und Notarisierung) |
| Linux | `.deb` mit APT-Paketquelle | pkg.wolkenfunk.org/apt | über `apt upgrade` |
| Linux | AppImage | pkg.wolkenfunk.org/appimage | **aktualisiert sich selbst** (`latest-linux.yml`) |
| Linux | Flatpak | Flathub (in Vorbereitung) | über Flathub bzw. das Software-Center |
| Android | APK, signiert | pkg.wolkenfunk.org/android | keine (neue APK installieren; Play Store/F-Droid später) |
| iOS | App Store | App Store Connect über Codemagic | über den App Store |

Selbst aktualisieren sich nur Pakete ohne Paketverwaltung dahinter. Das prüft `desktop/src/main/update.ts`:
- Linux nur mit `APPIMAGE`.
- Windows nur, wenn die App verpackt ist.
- Ein `.deb` oder Flatpak aktualisiert sich nie selbst.

**IDs:**
- Desktop `org.wolkenfunk.Wolkenfunk` (Flathub, Desktop-Datei, Windows AppUserModelId).
- Android und iOS `org.wolkenfunk.app`.

## Aufbau von pkg.wolkenfunk.org

```
apt/        signiertes APT-Repo (dists/, pool/), wolkenfunk.asc, wolkenfunk.sources, wolkenfunk_amd64.deb →
appimage/   Wolkenfunk-X.Y.Z-x86_64.AppImage, latest-linux.yml, Wolkenfunk-x86_64.AppImage →
windows/    Wolkenfunk-Setup-X.Y.Z.exe(.blockmap), latest.yml, Wolkenfunk-Setup.exe →
mac/        Wolkenfunk-X.Y.Z-universal.dmg, Wolkenfunk.dmg →
android/    wolkenfunk-X.Y.Z[-abi].apk, wolkenfunk.apk →
latest.json neueste Version je Kanal mit Größe und SHA-256
```

- **Feste Links** (`→`, Symlink auf die neueste Datei) verlinkt die Webseite. Sie ändern sich nie.
- **Aufbewahrung:** Jeder Ordner behält die letzten 3 Versionen und hat eine `SHA256SUMS`.
- **Server:** Die Dateien liegen statisch auf dem Webserver der Webseite. Betrieb und Zugang sind in der Server-Doku beschrieben, nicht hier.

## Eine Version veröffentlichen

1. **Stand prüfen:** main ist fertig, gepusht und getestet. Die Bot-Tests der Plattformen sind grün.
2. **Probelauf:** `DRY_RUN=1 scripts/release.sh X.Y.Z` baut alles und legt es in `~/.cache/wolkenfunk/pkg` ab.
   - Ohne Commit, Tag oder Upload, der Arbeitsbaum bleibt unverändert.
   - Windows braucht dafür einen CI-Lauf.
3. **Echter Lauf:** `scripts/release.sh X.Y.Z [development|stable]`. Das Skript:
   1. prüft: main, sauber, gleich `origin/main`, Tag noch frei,
   2. setzt die Version mit `scripts/set-version.sh`: Desktop, Android `versionCode = X*10000+Y*100+Z`, iOS-Build-Nummer gleich, AppStream-`<release>`,
   3. committet „Release X.Y.Z“, setzt den Tag `vX.Y.Z` und pusht. Der Tag startet die Windows-CI (`.github/workflows/desktop-windows.yml`),
   4. führt die Kanäle nacheinander aus:

      | Kanal | Skript | Was es tut |
      |---|---|---|
      | Linux | `desktop/scripts/release-linux.sh` | baut `.deb` und AppImage, nimmt das Paket ins APT-Repo auf, signiert, lädt hoch |
      | Android | `android/scripts/release-android.sh` | `assembleRelease`, signiert mit dem Release-Schlüssel, lädt hoch |
      | macOS | `desktop/scripts/release-mac.sh` | synchronisiert zum Mac-Host, `npm run dist:mac` dort, holt die `.dmg`, lädt hoch |
      | Windows | `desktop/scripts/release-windows.sh` | wartet auf die CI, holt die Artefakte per `gh run download`, lädt hoch |

   5. schreibt zum Schluss `latest.json`.
4. **Kanäle einzeln:** Bricht ein Kanal ab, geht es mit `ONLY=mac,windows scripts/release.sh X.Y.Z` weiter. Vorher Tag und Commit entfernen oder das Kanal-Skript direkt aufrufen. Jedes Kanal-Skript läuft auch allein, mit `DRY_RUN=1` und `SKIP_BUILD=1`.
5. **iOS:** über Codemagic, Tag `ios-vX.Y.Z` oder manuell. Nur nach Absprache, TestFlight oder App Store nicht automatisch.
6. **Flathub:** neue Version im Flathub-Repo eintragen (`desktop/packaging/flatpak/README.md`).
7. **Prüfen:**
   - Alle festen Links liefern 200.
   - `latest.json` zeigt die neue Version.
   - Ein installiertes AppImage bzw. Windows meldet das Update und installiert es beim Beenden.

## Schlüssel

Im Repo steht kein Schlüssel und kein Passwort. Die Skripte lesen sie aus der Umgebung bzw. aus der gitignorierten `.env` (nur die genannten Variablen):

| Wofür | Wo | Variable |
|---|---|---|
| APT-Repo signieren | `~/.config/wolkenfunk/apt-gnupg` (GnuPG-Home, ed25519, siehe `desktop/packaging/README.md`) | `WOLKENFUNK_GNUPGHOME` |
| Android-APK signieren | Keystore außerhalb des Repos | `WOLKENFUNK_KEYSTORE`, `WOLKENFUNK_KEYSTORE_PASSWORD`, `WOLKENFUNK_KEY_ALIAS`, `WOLKENFUNK_KEY_PASSWORD` |
| Mac-Host für den macOS-Build | SSH-Ziel | `MAC_HOST` |
| Upload | SSH-Zugang zum Paket-Server | `PKG_HOST`, `PKG_REMOTE` |
| iOS | Codemagic (App-Store-Connect-API-Key, Distribution-Zertifikat) | nur dort hinterlegt |

**Alle Schlüssel sichern**, etwa im Passwortmanager:
- Ohne den Android-Schlüssel lassen sich installierte APKs nicht mehr aktualisieren.
- Ohne den APT-Schlüssel muss jeder Nutzer die Paketquelle neu einrichten.

**Noch nicht signiert:**
- Windows: SmartScreen zeigt „Unbekannter Herausgeber“.
- macOS: Gatekeeper fragt beim ersten Öffnen.

Beides kommt mit eigenen Zertifikaten später.

## Icons und Screenshots
- **Icons:** `python3 design/icon/build-icons.py` erzeugt alle Icons aus `design/icon/wolkenfunk-symbolic.svg`: Desktop, Tray, Android, iOS und Webseite. Die Ergebnisse sind eingecheckt, der Build braucht kein Werkzeug.
- **Screenshots** für Webseite und Stores entstehen gegen den Demo-Server `core/talk-demo`, mit erfundenen Daten. Siehe `scripts/screenshots/`.
