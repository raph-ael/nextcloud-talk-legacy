# Webseite wolkenfunk.org

Statische Seite ohne Build-Schritt: was in `public/` liegt, wird 1:1 ausgeliefert.
Keine externen Fonts, Skripte oder Tracker. Einziges Skript ist `platform.js` (Plattformerkennung
auf der Startseite); ohne JS zeigt die Seite einfach alle Plattformen.

```
public/                 ausgelieferte Dateien
  index.html            Startseite (SEO-Meta, Open Graph, JSON-LD), Download-Knopf je Plattform, Hero-Bilder
  funktionen/           Funktionen mit Screenshots
  platform.js           erkennt das Betriebssystem und zeigt nur dessen Knopf (CSP script-src 'self')
  404.html              Fehlerseite (siehe „Server“)
  impressum/, datenschutz/   Rechtliches (CODELABOR UG i.G., Angaben wie auf codelabor.io), bleiben noindex
  download/             alle Plattformen mit Hinweisen und Update-Tabelle; Dateien liegen auf pkg.wolkenfunk.org
  styles.css            Hell/Dunkel über prefers-color-scheme
  screenshots/          Bilder vom Demo-Server (scripts/screenshots/README.md), nicht von Hand
  favicon.svg/.ico, apple-touch-icon.png, icon-*.png, og-image.png   ← generiert
  site.webmanifest, robots.txt, sitemap.xml, .well-known/security.txt
scripts/
  build-assets.py       erzeugt alle Icons + og-image.png aus design/icon/wolkenfunk-symbolic.svg
  stage.py              kopiert public/, markiert vorhandene Downloads, prüft Bilder, erzeugt kleine Varianten
  deploy.sh             stage.py, dann rsync nach jw.helsinki (CT122), danach Statusprüfung
```

## Lokal ansehen

```sh
python3 -m http.server -d website/public 8080   # → http://localhost:8080 (Downloads alle „bald“)
```

## Downloads und „bald“

Download-Links zeigen nur auf die festen Links in pkg.wolkenfunk.org (`windows/Wolkenfunk-Setup.exe`,
`mac/Wolkenfunk.dmg`, `apt/wolkenfunk_amd64.deb`, `appimage/Wolkenfunk-x86_64.AppImage`,
`android/wolkenfunk.apk`, je Ordner `SHA256SUMS`). Jede Stelle trägt `data-pkg="URL"` und enthält
einen Knopf (`.ready`) und einen „bald“-Platzhalter (`.soon`).

`scripts/stage.py` prüft beim Ausspielen jede URL per HEAD und setzt bei 200 `data-available`.
Ohne das gilt alles als „bald“, es gibt also nie einen Link auf eine fehlende Datei. Nach einem
Release genügt `deploy.sh`, die Seite muss nicht angepasst werden.

```sh
website/scripts/stage.py /tmp/wf --all    # Vorschau: alles verfügbar (--none: alles „bald“)
python3 -m http.server -d /tmp/wf 8080
```


## Screenshots

Die Bilder in `public/screenshots/` kommen aus `scripts/screenshots/` (Demo-Server, erfundene Daten),
benannt `<plattform>-<szene>-<hell|dunkel>.webp`. Die Seiten binden sie per `<picture>` ein: das
dunkle Bild über `prefers-color-scheme`, mit `width`/`height`, unterhalb des ersten Bildschirms
`loading="lazy"`.

- Für `srcset` verweisen die Seiten zusätzlich auf verkleinerte Varianten `NAME.<breite>w.webp`.
  Die erzeugt `stage.py` beim Ausspielen aus dem Original, ins Repo kommen nur die Originale.
- `stage.py` bricht ab, wenn ein Bild fehlt, und warnt, wenn das Seitenverhältnis nicht zu
  `width`/`height` passt (z. B. wenn sich die Größe der Mac-Bilder mit Fensterrahmen ändert).
- Vorschau ohne die echten Bilder: `stage.py /tmp/wf --all --placeholders`.
- `screenshots/desktop-chat.png` ist das AppStream-Bild aus
  `desktop/packaging/linux/org.wolkenfunk.Wolkenfunk.metainfo.xml` (Flathub), keine Seite nutzt es.

## Icons neu erzeugen

Nach Änderungen am Symbol (`design/icon/wolkenfunk-symbolic.svg`) oder an den Farben im Skript:

```sh
python3 design/icon/build-icons.py website   # ruft build-assets.py auf; braucht rsvg-convert, Pillow, „Noto Sans“
```

Die erzeugten Dateien werden eingecheckt.

## Ausspielen

```sh
website/scripts/deploy.sh --dry-run   # zeigt die Änderungen
website/scripts/deploy.sh             # live
```

Ziel ist `/root/claudes/wolkenfunk/site/` auf dem PVE-Host (Bind-Mount in CT122, nginx :80
hinter dem Reverse-Proxy CT100). `--delete`: was nicht in `public/` liegt, verschwindet vom Server.

## Vor dem Launch

- In `index.html` die Zeile `<meta name="robots" content="noindex">` entfernen.
- `lastmod` in `sitemap.xml` pflegen, bei neuen Seiten Einträge ergänzen.
- `Expires` in `.well-known/security.txt` spätestens jährlich verlängern.
- In `download/` und `funktionen/` ebenfalls `noindex` entfernen.
- Wer `styles.css` oder `platform.js` ändert, zählt `?v=` in den HTML-Dateien hoch (Cache 1 h).
- Impressum nachziehen, sobald die UG eingetragen ist (HRB, ggf. USt-IdNr.; „i.G.“ streichen).
  Datenschutz anpassen, wenn weitere Skripte, Formulare oder Dienste Dritter dazukommen.

## Server (CT122, nicht Teil dieses Repos)

nginx-vhost und Betriebsdoku liegen auf dem PVE-Host unter `/root/claudes/wolkenfunk/`
(`README.md`, `ops/ct122/nginx-wolkenfunk.org.conf`). Der vhost liefert `404.html` als
Fehlerseite, komprimiert per gzip, setzt Sicherheits-Header (`nosniff`, `Referrer-Policy`,
`X-Frame-Options`, `Permissions-Policy`) und Cache-Zeiten: HTML `no-cache`, CSS 1 h,
Manifest 1 Tag, Bilder/Icons 7 Tage. Bei Icon-Änderungen ggf. Dateinamen wechseln.
