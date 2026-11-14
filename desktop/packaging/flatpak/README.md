# Flatpak / Flathub

Wolkenfunk als Flatpak, App-ID `org.wolkenfunk.Wolkenfunk`. Gebaut wird vollständig aus dem Quellcode und
ohne Netz: der Rust-Kern (napi-Modul) mit cargo, die Electron-App mit npm, electron-vite und
electron-builder (`--dir`, also ohne AppImage/deb).

| Datei | Zweck |
|---|---|
| `org.wolkenfunk.Wolkenfunk.yml` | Manifest: Runtime `org.freedesktop.Platform` 26.08, Basis `org.electronjs.Electron2.BaseApp`, SDK-Erweiterungen node24 und rust-stable, Berechtigungen (jede kommentiert) |
| `wolkenfunk.sh` | Startskript `/app/bin/wolkenfunk`, startet Electron über `zypak-wrapper` (Chromium-Sandbox im Flatpak) |
| `org.wolkenfunk.Wolkenfunk.desktop` | Starter; `StartupWMClass` = Wayland-app_id aus `desktopName` in `package.json` |
| `update-sources.sh` | erzeugt `generated-sources.json` (npm, inkl. Electron-Zip) und `cargo-sources.json` (Crates) aus den Lockfiles |
| `../linux/org.wolkenfunk.Wolkenfunk.metainfo.xml` | AppStream-Daten (gemeinsam mit den anderen Linux-Paketen) |

Die beiden generierten JSON-Dateien sind hier per `.gitignore` ausgenommen; sie liegen im
Flathub-Repo neben dem Manifest.

## Quellen neu erzeugen
Nach jeder Änderung an `desktop/package-lock.json` oder `core/Cargo.lock`:
```
desktop/packaging/flatpak/update-sources.sh      # braucht git und uv
```

## Lokal bauen und testen
Einmalig: `flatpak install --user flathub org.flatpak.Builder`.
```
cd desktop/packaging/flatpak
flatpak run org.flatpak.Builder --user --force-clean --install-deps-from=flathub \
  --repo=repo --install build-dir org.wolkenfunk.Wolkenfunk.yml
flatpak run org.wolkenfunk.Wolkenfunk
flatpak run --command=flatpak-builder-lint org.flatpak.Builder manifest org.wolkenfunk.Wolkenfunk.yml
flatpak run --command=flatpak-builder-lint org.flatpak.Builder repo repo
flatpak uninstall --user org.wolkenfunk.Wolkenfunk
```
Das Manifest baut lokal aus dem Arbeitsverzeichnis (`type: dir`). Für Flathub stattdessen die
auskommentierte `git`-Quelle mit Tag und Commit eines Releases verwenden.

## Bei Flathub einreichen
1. Vorher: Screenshots in die Metainfo (bei wolkenfunk.org gehostet), Release-Eintrag zur Version,
   Metainfo- und Repo-Lint ohne Fehler.
2. Release taggen, im Manifest die `dir`-Quelle durch die `git`-Quelle (Tag + Commit) ersetzen.
3. [github.com/flathub/flathub](https://github.com/flathub/flathub) forken, Branch vom Branch
   `new-pr` abzweigen, Manifest, `generated-sources.json`, `cargo-sources.json`, `wolkenfunk.sh`
   und die `.desktop`-Datei hinzufügen, Pull Request gegen `new-pr` öffnen
   ([Anleitung](https://docs.flathub.org/docs/for-app-authors/submission)).
4. Nach der Aufnahme entsteht `flathub/org.wolkenfunk.Wolkenfunk`; Updates sind dann PRs dort
   (neuer Tag, neu erzeugte Quellen).
5. Verifizierung der Domain: Flathub zeigt im Entwicklerportal einen Token; der kommt nach
   `https://wolkenfunk.org/.well-known/org.flathub.VerifiedApps.txt` (Datei unter
   `website/public/.well-known/`), danach trägt die App den Haken „Verifiziert“.

Updates: Das AppImage-Selbstupdate der App läuft nur, wenn `APPIMAGE` gesetzt ist, im Flatpak also
nie. Flatpak-Nutzer bekommen Updates über Flathub.
