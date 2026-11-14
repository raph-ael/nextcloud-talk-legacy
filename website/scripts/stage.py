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
"""

import argparse
import re
import shutil
import sys
import urllib.error
import urllib.request
from pathlib import Path

PUBLIC = Path(__file__).resolve().parents[1] / "public"
ATTR = re.compile(r'data-pkg="([^"]+)"')
# <picture> mit <source srcset> und <img src srcset width height>; lokale Pfade beginnen mit /.
PICTURE = re.compile(r"<picture>(.*?)</picture>", re.S)
SRC = re.compile(r'\s(?:src|srcset)="([^"]+)"')
SIZE = re.compile(r'<img[^>]*\swidth="(\d+)"[^>]*\sheight="(\d+)"')
# Verkleinerte Variante NAME.<breite>w.webp entsteht hier aus NAME.webp, liegt also nicht im Repo.
VARIANT = re.compile(r"^(.+)\.(\d+)w\.webp$")


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

    for p in pages:
        text = p.read_text()
        new = ATTR.sub(lambda m: m.group(0) + (" data-available" if available[m.group(1)] else ""), text)
        if new != text:
            p.write_text(new)
    print(f"→ {a.dest}")


if __name__ == "__main__":
    main()
