#!/usr/bin/env python3
"""Bereitet website/public/ zum Ausspielen vor: kopiert alles nach ZIEL und markiert dabei jede
Download-Stelle als verfügbar, deren Datei auf pkg.wolkenfunk.org tatsächlich existiert.

    website/scripts/stage.py ZIEL          prüft jede data-pkg-URL per HEAD
    website/scripts/stage.py ZIEL --all    alles als verfügbar (Vorschau)
    website/scripts/stage.py ZIEL --none   nichts verfügbar (Vorschau)

In den HTML-Dateien trägt jede Download-Stelle `data-pkg="URL"`. Antwortet die URL mit 200,
bekommt das Element `data-available`, und styles.css zeigt die Knöpfe (`.ready`) statt „bald“
(`.soon`). Ohne diesen Schritt (oder wenn der Server nicht antwortet) bleibt alles „bald“,
es gibt also nie einen Knopf, der auf eine fehlende Datei zeigt.
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


def main() -> None:
    ap = argparse.ArgumentParser(description=__doc__, formatter_class=argparse.RawDescriptionHelpFormatter)
    ap.add_argument("dest", type=Path)
    g = ap.add_mutually_exclusive_group()
    g.add_argument("--all", action="store_true", help="alles als verfügbar markieren")
    g.add_argument("--none", action="store_true", help="nichts als verfügbar markieren")
    a = ap.parse_args()

    if a.dest.resolve() == PUBLIC:
        sys.exit("ZIEL darf nicht public/ selbst sein")
    if a.dest.exists():
        shutil.rmtree(a.dest)
    shutil.copytree(PUBLIC, a.dest)

    pages = sorted(a.dest.rglob("*.html"))
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
