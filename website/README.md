# Webseite wolkenfunk.org

Statische Seite ohne Build-Schritt: was in `public/` liegt, wird 1:1 ausgeliefert.
Keine externen Fonts, Skripte oder Tracker.

```
public/                 ausgelieferte Dateien
  index.html            Startseite (SEO-Meta, Open Graph, JSON-LD)
  404.html              Fehlerseite (siehe „Server“)
  styles.css            Hell/Dunkel über prefers-color-scheme
  favicon.svg/.ico, apple-touch-icon.png, icon-*.png, og-image.png   ← generiert
  site.webmanifest, robots.txt, sitemap.xml, .well-known/security.txt
scripts/
  build-assets.py       erzeugt alle Icons + og-image.png aus design/icon/wolkenfunk-symbolic.svg
  deploy.sh             rsync nach jw.helsinki (CT122), danach Statusprüfung
```

## Lokal ansehen

```sh
python3 -m http.server -d website/public 8080   # → http://localhost:8080
```

## Icons neu erzeugen

Nach Änderungen am Symbol (`design/icon/wolkenfunk-symbolic.svg`) oder an den Farben im Skript:

```sh
python3 website/scripts/build-assets.py   # braucht rsvg-convert, Pillow, Schrift „Noto Sans“
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

## Server (CT122, nicht Teil dieses Repos)

nginx-vhost und Betriebsdoku liegen auf dem PVE-Host unter `/root/claudes/wolkenfunk/`
(`README.md`, `ops/ct122/nginx-wolkenfunk.org.conf`). Der vhost liefert `404.html` als
Fehlerseite, komprimiert per gzip, setzt Sicherheits-Header (`nosniff`, `Referrer-Policy`,
`X-Frame-Options`, `Permissions-Policy`) und Cache-Zeiten: HTML `no-cache`, CSS 1 h,
Manifest 1 Tag, Bilder/Icons 7 Tage. Bei Icon-Änderungen ggf. Dateinamen wechseln.
