#!/usr/bin/env python3
"""Bereitet website/public/ zum Ausspielen vor: kopiert alles nach ZIEL und markiert dabei jede
Download-Stelle als verfügbar, deren Datei auf pkg.wolkenfunk.org tatsächlich existiert.

    website/scripts/stage.py ZIEL          prüft jede data-pkg-URL per HEAD
    website/scripts/stage.py ZIEL --all    alles als verfügbar (Vorschau)
    website/scripts/stage.py ZIEL --none   nichts verfügbar (Vorschau)
    ... --placeholders                     fehlende Screenshots durch Platzhalter ersetzen (Vorschau)

In den HTML-Dateien trägt jede Download-Stelle `data-pkg="URL"`. Antwortet die URL mit 200,
bekommt das Element `data-available`, und styles.css zeigt die Knöpfe (`.ready`) statt „bald“
(`.soon`). Ohne diesen Schritt (oder wenn der Server nicht antwortet) bleibt alles „bald“,
es gibt also nie einen Knopf, der auf eine fehlende Datei zeigt.

Außerdem prüft es jedes lokale Bild (src/srcset): Fehlt eins, bricht es ab, außer mit
--placeholders. Weicht das Seitenverhältnis von width/height am <img> ab, warnt es.
Verkleinerte Varianten für srcset (NAME.<breite>w.webp) erzeugt es aus NAME.webp; ins Repo
kommen nur die Originale.

Neben einem Download-Knopf steht `<span data-version-of="URL"></span>`. Dort trägt es die
Versionsnummer ein, die hinter dem festen Link liegt (aus SHA256SUMS im selben Ordner). Mit
--all nimmt es die Version aus desktop/package.json, mit --none bleibt die Stelle leer.

Das Stylesheet verliert Kommentare und Einrückung. Zuletzt hängt es an jeden Verweis auf eine eigene Datei (CSS, Skript, Schrift, Bild, Manifest)
`?v=<Prüfsumme des Inhalts>`. Ändert sich eine Datei, ändert sich ihre Adresse; der Webserver
darf alles mit `?v=` deshalb ein Jahr lang im Browser-Cache lassen. Von Hand zählt niemand
mehr eine Versionsnummer hoch.
"""

import argparse
import hashlib
import json
import re
import shutil
import sys
import urllib.error
import urllib.request
from pathlib import Path

PUBLIC = Path(__file__).resolve().parents[1] / "public"
ATTR = re.compile(r'data-pkg="([^"]+)"')
VERSION_OF = re.compile(r'data-version-of="([^"]+)"></span>')
VERSION = re.compile(r"\d+\.\d+\.\d+")
# <picture> mit <source srcset> und <img src srcset width height>; lokale Pfade beginnen mit /.
PICTURE = re.compile(r"<picture>(.*?)</picture>", re.S)
SRC = re.compile(r'\s(?:src|srcset)="([^"]+)"')
SIZE = re.compile(r'<img[^>]*\swidth="(\d+)"[^>]*\sheight="(\d+)"')
# Verkleinerte Variante NAME.<breite>w.webp entsteht hier aus NAME.webp, liegt also nicht im Repo.
VARIANT = re.compile(r"^(.+)\.(\d+)w\.webp$")
# Verweis auf eine eigene Datei: beginnt mit /, steht in Anführungszeichen, in url(…) oder in
# einer srcset-Liste. Adressen mit https:// davor (og:image, JSON-LD) passen nicht und bleiben fest.
STATIC = re.compile(r'''(?<=["'\s,(])(/[A-Za-z0-9_./-]+\.(?:css|js|webp|png|svg|ico|woff2|webmanifest))(?=["'\s,)])''')


def exists(url: str) -> bool:
    req = urllib.request.Request(url, method="HEAD", headers={"User-Agent": "wolkenfunk-stage"})
    try:
        with urllib.request.urlopen(req, timeout=15) as r:
            return r.status == 200
    except urllib.error.HTTPError:
        return False
    except OSError as e:
        print(f"  ! {url}: {e}", file=sys.stderr)
        return False


def published_version(url: str) -> str | None:
    """Version der Datei hinter dem festen Link. In SHA256SUMS steht dieselbe Prüfsumme noch
    einmal unter dem Namen mit Versionsnummer; fehlt der feste Name dort, gilt die höchste."""
    base, name = url.rsplit("/", 1)
    req = urllib.request.Request(f"{base}/SHA256SUMS", headers={"User-Agent": "wolkenfunk-stage"})
    try:
        with urllib.request.urlopen(req, timeout=15) as r:
            lines = [ln.split(None, 1) for ln in r.read().decode().splitlines() if ln.strip()]
    except OSError as e:
        print(f"  ! {base}/SHA256SUMS: {e}", file=sys.stderr)
        return None
    sums = {n.strip().lstrip("*"): h for h, n in (ln for ln in lines if len(ln) == 2)}
    same = [n for n, h in sums.items() if h == sums.get(name) and n != name]
    found = [m[0] for n in (same or sums) if (m := VERSION.search(n.rsplit("/", 1)[-1]))]
    return max(found, key=lambda v: tuple(map(int, v.split(".")))) if found else None


def repo_version() -> str:
    return json.loads((PUBLIC.parents[1] / "desktop" / "package.json").read_text())["version"]


def local_refs(attr: str) -> list[str]:
    """Pfade aus src="…" bzw. srcset="a 400w, b 1280w"."""
    urls = [part.strip().split()[0] for part in attr.split(",") if part.strip()]
    return [u for u in urls if u.startswith("/") and re.search(r"\.(?:webp|png|jpg|avif)$", u)]


def check_images(dest: Path, pages: list[Path], placeholders: bool) -> None:
    """Jedes lokale Bild muss existieren und zu width/height am <img> passen; verkleinerte
    Varianten (NAME.<breite>w.webp) werden aus NAME.webp erzeugt."""
    wanted: dict[str, tuple[int, int] | None] = {}
    for p in pages:
        text = p.read_text()
        for pic in PICTURE.findall(text):
            m = SIZE.search(pic)
            for attr in SRC.findall(pic):
                for src in local_refs(attr):
                    wanted[src] = (int(m[1]), int(m[2])) if m else None
        for attr in SRC.findall(PICTURE.sub("", text)):
            for src in local_refs(attr):
                wanted.setdefault(src, None)
    missing, variants = [], []
    for src, size in sorted(wanted.items()):
        if VARIANT.match(src):
            variants.append(src)
            continue
        f = dest / src.lstrip("/")
        if not f.exists():
            if placeholders and size:
                placeholder(f, size)
                print(f"  Platzhalter {src}")
            else:
                missing.append(src)
            continue
        if size:
            from PIL import Image
            with Image.open(f) as im:
                w, h = im.size
            if abs(w / h - size[0] / size[1]) > 0.01:
                print(f"  ! {src}: {w}×{h}, im HTML {size[0]}×{size[1]}", file=sys.stderr)
    for src in variants:
        base_src, width = VARIANT.match(src).group(1) + ".webp", int(VARIANT.match(src).group(2))
        base, f = dest / base_src.lstrip("/"), dest / src.lstrip("/")
        if f.exists():
            continue
        if not base.exists():
            if base_src not in missing:
                missing.append(base_src)
            continue
        from PIL import Image
        with Image.open(base) as im:
            im.resize((width, round(im.height * width / im.width)), Image.LANCZOS).save(f, quality=82)
    if missing:
        sys.exit("fehlende Bilder (Vorschau mit --placeholders):\n  " + "\n  ".join(missing))


def slim_css(f: Path) -> None:
    """Kommentare, Einrückung und Leerzeilen raus; die Regeln selbst bleiben unverändert."""
    text = re.sub(r"/\*.*?\*/", "", f.read_text(), flags=re.S)
    f.write_text("\n".join(ln.strip() for ln in text.splitlines() if ln.strip()) + "\n")


def stamp(dest: Path, files: list[Path]) -> None:
    """Hängt ?v=<Prüfsumme> an jeden Verweis auf eine vorhandene eigene Datei."""
    def add(m: re.Match) -> str:
        f = dest / m.group(1).lstrip("/")
        if not f.is_file():
            return m.group(1)
        return f"{m.group(1)}?v={hashlib.sha256(f.read_bytes()).hexdigest()[:10]}"
    for p in files:
        text = p.read_text()
        new = STATIC.sub(add, text)
        if new != text:
            p.write_text(new)


def placeholder(f: Path, size: tuple[int, int]) -> None:
    from PIL import Image, ImageDraw
    dark = "-dunkel" in f.name
    im = Image.new("RGB", size, "#1B2638" if dark else "#E4EAF3")
    d = ImageDraw.Draw(im)
    w, h = size
    d.rectangle([0, 0, w - 1, h - 1], outline="#7AAEFF" if dark else "#1F63DE", width=max(4, w // 200))
    d.text((w // 2, h // 2), f.name, fill="#E3EAF5" if dark else "#142033", anchor="mm",
           font_size=max(16, w // 24))
    f.parent.mkdir(parents=True, exist_ok=True)
    im.save(f, quality=60)


def main() -> None:
    ap = argparse.ArgumentParser(description=__doc__, formatter_class=argparse.RawDescriptionHelpFormatter)
    ap.add_argument("dest", type=Path)
    g = ap.add_mutually_exclusive_group()
    g.add_argument("--all", action="store_true", help="alles als verfügbar markieren")
    g.add_argument("--none", action="store_true", help="nichts als verfügbar markieren")
    ap.add_argument("--placeholders", action="store_true", help="fehlende Screenshots als Platzhalter")
    a = ap.parse_args()

    if a.dest.resolve() == PUBLIC:
        sys.exit("ZIEL darf nicht public/ selbst sein")
    if a.dest.exists():
        shutil.rmtree(a.dest)
    shutil.copytree(PUBLIC, a.dest)

    pages = sorted(a.dest.rglob("*.html"))
    check_images(a.dest, pages, a.placeholders)
    urls = sorted({u for p in pages for u in ATTR.findall(p.read_text())})
    available = {u: (a.all or (not a.none and exists(u))) for u in urls}
    for u in urls:
        print(f"  {'verfügbar' if available[u] else 'bald     '}  {u}")

    wanted = sorted({u for p in pages for u in VERSION_OF.findall(p.read_text())})
    versions = {u: None if a.none else repo_version() if a.all else published_version(u) for u in wanted}
    for u in wanted:
        print(f"  Version {versions[u] or '–':9}  {u}")
    for p in pages:
        text = p.read_text()
        new = ATTR.sub(lambda m: m.group(0) + (" data-available" if available[m.group(1)] else ""), text)
        new = VERSION_OF.sub(
            lambda m: m.group(0).replace("></span>", f">Version {versions[m.group(1)]}</span>")
            if versions[m.group(1)] else m.group(0), new)
        if new != text:
            p.write_text(new)
    for css in sorted(a.dest.glob("*.css")):
        slim_css(css)
    # Erst Stylesheet und Manifest (ihre Prüfsumme hängt von Schrift und Icons ab), dann die Seiten.
    stamp(a.dest, sorted(a.dest.glob("*.css")) + sorted(a.dest.glob("*.webmanifest")))
    stamp(a.dest, pages)
    print(f"→ {a.dest}")


if __name__ == "__main__":
    main()
