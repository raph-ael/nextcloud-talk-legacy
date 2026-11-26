# Webseite wolkenfunk.org

Statische Seite ohne Build-Schritt: was in `public/` liegt, wird 1:1 ausgeliefert.
Keine externen Fonts, Skripte oder Tracker. Einziges Skript ist `platform.js` (Plattformerkennung
auf Startseite und Download-Seite); ohne JS zeigt die Seite einfach alle Plattformen.

```
public/                 ausgelieferte Dateien
  index.html            Startseite: Kernaussage, Download-Knopf fürs erkannte System, drei Vorteile,
                        Kacheln je System, Kurzliste weiterer Funktionen (SEO-Meta, Open Graph, JSON-LD)
  funktionen/           alle Funktionen in acht Gruppen, einfach erklärt, mit Marken „Android“/„Computer“
  download/             Kacheln je System, darunter Installationshinweise, Versionsnummer und Update-Tabelle;
                        Dateien liegen auf pkg.wolkenfunk.org
  platform.js           erkennt das Betriebssystem und setzt data-os am <html>: Hauptknopf der Startseite,
                        hervorgehobene Kachel (CSP script-src 'self'); steht im <head>, damit nichts
                        nachträglich verrutscht; ohne JS bleibt ein allgemeiner Knopf
  404.html              Fehlerseite (siehe „Server“)
  impressum/, datenschutz/   Rechtliches (CODELABOR UG, Angaben wie auf codelabor.io), bleiben noindex
  styles.css            ein Stylesheet für alle Seiten, Hell/Dunkel über prefers-color-scheme
  fonts/                Inter (variabel, lateinischer Zeichensatz, OFL) aus @fontsource-variable/inter;
                        Apple-Geräte nehmen ihre Systemschrift
  screenshots/          Bilder vom Demo-Server (scripts/screenshots/README.md), nicht von Hand
  favicon.svg/.ico, apple-touch-icon.png, icon-*.png, og-image.png   ← generiert
  site.webmanifest, robots.txt, sitemap.xml, .well-known/security.txt
  llms.txt              Kurzbeschreibung der Seite für KI-Suchdienste
scripts/
  build-assets.py       erzeugt alle Icons aus design/icon/wolkenfunk-symbolic.svg und das Vorschaubild
                        og-image.png (Text plus zwei Screenshots, fotografiert mit Chrome)
  stage.py              kopiert public/, markiert vorhandene Downloads, trägt Versionsnummern ein,
                        prüft Bilder, erzeugt kleine Varianten, verschlankt das Stylesheet und hängt
                        an jeden Verweis auf eine eigene Datei ?v=<Prüfsumme>
  deploy.sh             stage.py, dann rsync auf den Server (PKG_HOST), danach Statusprüfung
```

Leiste, Fuß und die Symbole (eingebettetes SVG, Klasse `i`) stehen in jeder HTML-Datei: Wer sie
ändert, ändert sie in allen sechs Seiten. Inline-Styles gehen wegen der CSP nicht.

## Lokal ansehen

```sh
website/scripts/stage.py /tmp/wf --all    # Vorschau: alles verfügbar (--none: alles „bald“)
python3 -m http.server -d /tmp/wf 8080    # → http://localhost:8080
```

Direkt aus `public/` fehlen die verkleinerten Bilder, deshalb immer über `stage.py`.

## Downloads und „bald“

Download-Links zeigen nur auf die festen Links in pkg.wolkenfunk.org (`windows/Wolkenfunk-Setup.exe`,
`mac/Wolkenfunk.dmg`, `apt/wolkenfunk_amd64.deb`, `appimage/Wolkenfunk-x86_64.AppImage`,
`android/wolkenfunk.apk`, je Ordner `SHA256SUMS`). Jede Stelle trägt `data-pkg="URL"` und enthält
einen Knopf (`.ready`) und einen „bald“-Platzhalter (`.soon`).

`scripts/stage.py` prüft beim Ausspielen jede URL per HEAD und setzt bei 200 `data-available`.
Ohne das gilt alles als „bald“, es gibt also nie einen Link auf eine fehlende Datei. Nach einem
Release genügt `deploy.sh`, die Seite muss nicht angepasst werden.

Android hat zwei Wege: den Knopf zu Google Play (fester Link auf den Eintrag der App, ohne
`data-pkg`, immer sichtbar) und daneben die APK wie oben. Die iPhone-App ist noch in keinem Store.

Neben den Knöpfen der Download-Seite steht `<span class="ver" data-version-of="URL"></span>`.
`stage.py` trägt dort die Version ein, die hinter dem festen Link liegt: In `SHA256SUMS` steht
dieselbe Prüfsumme noch einmal unter dem Dateinamen mit Versionsnummer. Mit `--all` nimmt es die
Version aus `desktop/package.json`.

## Screenshots

Die Bilder in `public/screenshots/` kommen aus `scripts/screenshots/` (Demo-Server, erfundene Daten),
benannt `<plattform>-<szene>-<hell|dunkel>.webp`. Die Seiten binden sie per `<picture>` ein: das
dunkle Bild über `prefers-color-scheme`, mit `width`/`height`, unterhalb des ersten Bildschirms
`loading="lazy"`.

- Für `srcset` verweisen die Seiten zusätzlich auf verkleinerte Varianten `NAME.<breite>w.webp`.
  Die erzeugt `stage.py` beim Ausspielen aus dem Original, ins Repo kommen nur die Originale.
  `sizes` nennt die Breite, in der das Bild wirklich erscheint (gemessen, nicht geschätzt). Wer
  das Layout eines Bildes ändert, zieht `sizes` nach, sonst lädt der Browser zu große Dateien.
- `stage.py` bricht ab, wenn ein Bild fehlt, und warnt, wenn das Seitenverhältnis nicht zu
  `width`/`height` passt (z. B. wenn sich die Größe der Mac-Bilder mit Fensterrahmen ändert).
- Vorschau ohne die echten Bilder: `stage.py /tmp/wf --all --placeholders`.
- `android-suche`, `android-erwaehnungen`, `android-standort`, `android-darstellung` und
  `android-benachrichtigung` haben das Format der Aufnahmen für Google Play (1080×1920), nicht
  1080×2400 wie die übrigen Android-Bilder.
- `screenshots/desktop-chat.png` ist das AppStream-Bild aus
  `desktop/packaging/linux/org.wolkenfunk.Wolkenfunk.metainfo.xml` (Flathub), keine Seite nutzt es.

## Icons neu erzeugen

Nach Änderungen am Symbol (`design/icon/wolkenfunk-symbolic.svg`) oder an den Farben im Skript:

```sh
python3 design/icon/build-icons.py website   # ruft build-assets.py auf; braucht rsvg-convert, Pillow, Chrome
```

Die erzeugten Dateien werden eingecheckt. `og-image.png` zeigt `mac-chats-hell` und
`android-chats-hell`; ändern sich diese Bilder oder die Überschrift, das Skript neu laufen lassen.

## Ausspielen

```sh
website/scripts/deploy.sh --dry-run   # zeigt die Änderungen
website/scripts/deploy.sh             # live
```

Ziel ist der Ordner `site/` neben dem Paket-Ordner auf dem Server (`PKG_HOST` aus der Umgebung
oder aus `.env`, siehe `scripts/pkg-env.sh`). Er ist in den Webserver eingehängt (nginx :80
hinter dem Reverse-Proxy). `--delete`: was nicht in `public/` liegt, verschwindet vom Server.

## Suchmaschinen und Vorschau

Startseite, `funktionen/` und `download/` dürfen in den Suchindex (`robots: index, follow`),
Impressum, Datenschutz und die Fehlerseite bleiben `noindex`. Jede der drei Seiten hat einen
eigenen Titel, eine Beschreibung, `canonical`, Open-Graph- und Twitter-Angaben mit dem
Vorschaubild sowie strukturierte Daten (JSON-LD: Seite, Herausgeber, auf der Startseite die App,
auf den Unterseiten der Pfad). Titel und Beschreibung stehen je Seite dreimal (`<title>`/
`description`, `og:`/`twitter:`, JSON-LD): bei Änderungen alle Stellen nachziehen.

- `lastmod` in `sitemap.xml` pflegen, bei neuen Seiten Einträge ergänzen (auch in `llms.txt`).
- `Expires` in `.well-known/security.txt` spätestens jährlich verlängern.
- `?v=` zählt niemand mehr von Hand hoch: `stage.py` setzt es aus dem Inhalt der Datei.
- Impressum: USt-IdNr. ergänzen, sobald es eine gibt. Datenschutz anpassen, wenn weitere Skripte,
  Formulare oder Dienste Dritter dazukommen.
- Nachmessen: `npx lighthouse https://wolkenfunk.org/ --preset=desktop` (und ohne `--preset` für
  die mobile Sicht), dazu pagespeed.web.dev.

## Server (CT122, nicht Teil dieses Repos)

nginx-vhost und Betriebsdoku liegen auf dem PVE-Host unter `/root/claudes/wolkenfunk/`
(`README.md`, `ops/ct122/nginx-wolkenfunk.org.conf`). Der vhost liefert `404.html` als
Fehlerseite, komprimiert per gzip und setzt Sicherheits-Header (`nosniff`, `Referrer-Policy`,
`X-Frame-Options`, `Permissions-Policy`, `Strict-Transport-Security` für ein Jahr, ohne
`includeSubDomains`).

Cache-Zeiten:
- Mit `?v=<Prüfsumme>` (setzt `stage.py` an jeden Verweis auf eine eigene Datei): ein Jahr,
  `immutable`. Die Adresse hängt am Inhalt, eine geänderte Datei bekommt eine neue.
- Ohne `?v=`: HTML `no-cache`, CSS und JS 1 h, Manifest 1 Tag, Bilder, Icons und Schrift 7 Tage.
  Das gilt etwa für `og-image.png` und für Adressen, die jemand von Hand aufruft.

Weiterleitungen auf den Pfad mit Schrägstrich (`/funktionen` → `/funktionen/`) kommen ohne
Schema und Host, bleiben also auf https.
