# Linux-Pakete

Der Gesamtablauf eines Releases für alle Plattformen steht in [docs/release.md](../../docs/release.md).

Drei Wege zu Wolkenfunk unter Linux, alle mit automatischen Updates:

| Kanal | Für wen | Updates | Quelle |
|---|---|---|---|
| **Flathub** | alle Distributionen, Software-Center | Flathub / Software-Center | `flatpak/` (eigenes Repo bei Flathub) |
| **APT-Repo** | Debian 12+, Ubuntu 22.04+ | `apt upgrade` / Systemupdates | https://pkg.wolkenfunk.org/apt |
| **AppImage** | portabel, ohne Root | aktualisiert sich selbst (electron-updater) | https://pkg.wolkenfunk.org/appimage |

App-ID überall `org.wolkenfunk.Wolkenfunk` (Flathub, Desktop-Datei, Wayland-`app_id`), Paketname
und Programm `wolkenfunk`. Die ID ist auf Flathub nicht mehr änderbar.

```
linux/
  org.wolkenfunk.Wolkenfunk.metainfo.xml   AppStream (Flathub, Software-Center)
  apt/wolkenfunk.asc                       öffentlicher Repo-Schlüssel, liegt im .deb
  deb/after-install.tpl, after-remove.tpl  Maintainer-Skripte: electron-builder-Vorlage + Paketquelle
flatpak/                                   Flathub-Manifest, siehe flatpak/README.md
../scripts/apt-repo.py                     baut und signiert das APT-Repo
../scripts/release-linux.sh                Build → Repo → Upload auf pkg.wolkenfunk.org
```

## Veröffentlichen

1. `version` in `package.json` hochzählen (und `<release>` in der Metainfo ergänzen).
2. `scripts/release-linux.sh` – baut `.deb` und AppImage, holt den Stand vom Server, nimmt das
   Paket ins Repo auf (die letzten 3 Versionen bleiben), signiert, lädt hoch.
   `DRY_RUN=1` baut und signiert nur lokal (`~/.cache/wolkenfunk/pkg`), `SKIP_BUILD=1` nimmt `dist/`.
3. Flathub: neue Version im Flathub-Repo eintragen (`flatpak/README.md`).

Server: `jw.helsinki:/root/claudes/wolkenfunk/pkg` (Bind-Mount in CT122, nginx; TLS auf CT100).
Betriebsdoku dort im `README.md`.

## APT-Repo

Ein schlichtes Debian-Repo aus statischen Dateien (`dists/stable/main`, `pool/`), signiert mit
einem eigenen Schlüssel:

- Fingerprint `F980 BE28 D9EA 63A0 7A86 6E13 3641 FF50 244C 4126`, ed25519, gültig bis 13.11.2029.
- Der private Schlüssel liegt nur auf dem Release-Rechner in `~/.config/wolkenfunk/apt-gnupg`
  (`WOLKENFUNK_GNUPGHOME` überschreibt), ohne Passphrase, damit das Skript signieren kann.
  **Sichern** (Passwortmanager/Tresor) und später als CI-Secret hinterlegen. Ein
  Widerrufszertifikat liegt dort unter `openpgp-revocs.d/`.
- Verlängern vor Ablauf: `gpg --quick-set-expire <fpr> 3y`, dann `apt/wolkenfunk.asc` hier neu
  exportieren und ein Release machen – das Paket frischt die Paketquelle beim Update auf.

Das `.deb` richtet die Paketquelle selbst ein (`after-install.tpl`): Es schreibt
`/etc/apt/sources.list.d/wolkenfunk.sources` im deb822-Format mit dem Schlüssel inline
(`Signed-By:`-Block, daher Debian 12/Ubuntu 22.04 oder neuer). Regeln:

- erste Installation: Datei anlegen, falls es sie noch nicht gibt;
- Update: Datei neu schreiben, solange die Markerzeile `# Managed by the wolkenfunk package` drin
  steht (neuer Schlüssel, neue URL); wer die Datei oder die Zeile löscht, behält es so;
- `apt remove` lässt sie stehen, `apt purge` entfernt sie.

Dieselbe Datei liegt unter https://pkg.wolkenfunk.org/apt/wolkenfunk.sources für die Einrichtung
ohne `.deb`; `wolkenfunk_amd64.deb` dort zeigt auf das neueste Paket.

## AppImage

- Gebaut mit der statischen AppImage-Laufzeit (`toolsets.appimage` in `package.json`), braucht
  daher kein `libfuse2` (fehlt auf aktuellen Ubuntu-Versionen).
- Selbst-Update in `src/main/update.ts`, nur wenn `APPIMAGE` gesetzt ist: prüft beim Start und alle
  6 h `https://pkg.wolkenfunk.org/appimage/latest-linux.yml`, lädt im Hintergrund und installiert
  beim Beenden. `.deb` und Flatpak aktualisieren sich nie selbst.
- `Wolkenfunk-x86_64.AppImage` auf dem Server zeigt auf die neueste Version (Download-Link).

## Umbenennung von „Sirplus Talk“

Die App hieß vorher „Sirplus Talk“ (`org.sirplus.talk`). Beim ersten Start zieht sie den alten
Datenordner (`~/.config/Sirplus Talk`) nach `~/.config/Wolkenfunk` um. Das gespeicherte
App-Passwort liegt im Schlüsselbund unter dem alten Namen und lässt sich nicht mehr öffnen: Man
meldet sich einmal neu an. Das alte Paket `sirplus-talk-desktop` muss man von Hand entfernen.
