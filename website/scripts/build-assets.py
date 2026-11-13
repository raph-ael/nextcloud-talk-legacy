#!/usr/bin/env python3
"""Erzeugt Favicons, App-Icons und das Social-Preview-Bild der Webseite
aus dem Symbol-Icon design/icon/wolkenfunk-symbolic.svg.

Voraussetzungen: rsvg-convert (librsvg2-bin), Python 3 mit Pillow,
Schrift „Noto Sans“ (nur für og-image.png).

Aufruf (aus dem Repo-Root oder website/):  python3 website/scripts/build-assets.py
Die Ergebnisse landen in website/public/ und werden mit eingecheckt,
damit der Deploy kein Werkzeug braucht.
"""
from __future__ import annotations

import re
import subprocess
import sys
import tempfile
from pathlib import Path

from PIL import Image

ROOT = Path(__file__).resolve().parents[2]
SRC = ROOT / "design" / "icon" / "wolkenfunk-symbolic.svg"
OUT = ROOT / "website" / "public"

# Markenfarben (wie design/icon/app-icon.svg)
BLUE_LIGHT = "#2F7BF5"
BLUE_DARK = "#1650C4"
GLYPH_ON_LIGHT = "#1F63DE"
GLYPH_ON_DARK = "#7AAEFF"


def glyph(fill: str | None) -> str:
    """Inhalt des Symbol-SVGs (ohne <svg>-Hülle) mit neuer Füllfarbe;
    fill=None entfernt die Farbe (dann setzt CSS sie)."""
    svg = SRC.read_text()
    inner = re.search(r"<svg[^>]*>(.*)</svg>", svg, re.S).group(1)
    new = f' fill="{fill}"' if fill else ""
    return re.sub(r' fill="#[0-9a-fA-F]{3,6}"', new, inner, count=1)


def placed(fill: str, scale: float) -> str:
    """Glyph (512er-Raster) zentriert auf scale verkleinert."""
    off = 256 * (1 - scale)
    return f'<g transform="translate({off:.2f} {off:.2f}) scale({scale})">{glyph(fill)}</g>'


GRADIENT = (
    '<defs><linearGradient id="bg" x1="0" y1="0" x2="1" y2="1">'
    f'<stop offset="0" stop-color="{BLUE_LIGHT}"/><stop offset="1" stop-color="{BLUE_DARK}"/>'
    "</linearGradient></defs>"
)


def svg512(body: str) -> str:
    return f'<svg xmlns="http://www.w3.org/2000/svg" viewBox="0 0 512 512">{body}</svg>'


def render(svg: str, out: Path, w: int, h: int | None = None) -> None:
    with tempfile.NamedTemporaryFile("w", suffix=".svg", delete=False) as f:
        f.write(svg)
        tmp = f.name
    args = ["rsvg-convert", "-w", str(w)]
    if h:
        args += ["-h", str(h)]
    subprocess.run(args + ["-o", str(out), tmp], check=True)
    Path(tmp).unlink()


def main() -> None:
    OUT.mkdir(parents=True, exist_ok=True)

    # 1) favicon.svg – farbig, passt sich hellem/dunklem Browser-Theme an
    fav = svg512(
        f"<style>g{{fill:{GLYPH_ON_LIGHT}}}"
        f"@media (prefers-color-scheme:dark){{g{{fill:{GLYPH_ON_DARK}}}}}</style>"
        + glyph(None)
    )
    (OUT / "favicon.svg").write_text(fav + "\n")

    # 2) Kachel-Icons: weißes Symbol auf Marken-Verlauf
    rounded = svg512(GRADIENT + '<rect width="512" height="512" rx="112" fill="url(#bg)"/>'
                     + placed("#FFFFFF", 0.78))
    square = svg512(GRADIENT + '<rect width="512" height="512" fill="url(#bg)"/>'
                    + placed("#FFFFFF", 0.78))
    # maskable: Symbol in der 80%-Sicherheitszone
    maskable = svg512(GRADIENT + '<rect width="512" height="512" fill="url(#bg)"/>'
                      + placed("#FFFFFF", 0.62))

    render(square, OUT / "apple-touch-icon.png", 180)   # iOS rundet selbst ab
    render(rounded, OUT / "icon-192.png", 192)
    render(rounded, OUT / "icon-512.png", 512)
    render(maskable, OUT / "icon-maskable-512.png", 512)

    # 3) favicon.ico (16/32/48) für alte Browser und /favicon.ico-Anfragen
    with tempfile.TemporaryDirectory() as d:
        big = Path(d) / "ico.png"
        render(rounded, big, 256)
        Image.open(big).save(OUT / "favicon.ico", sizes=[(16, 16), (32, 32), (48, 48)])

    # 4) og-image.png 1200×630 für Link-Vorschauen (Signal, Mastodon, Slack …)
    og = f"""<svg xmlns="http://www.w3.org/2000/svg" width="1200" height="630" viewBox="0 0 1200 630">
  {GRADIENT}
  <rect width="1200" height="630" fill="url(#bg)"/>
  <g transform="translate(96 155) scale(0.625)">{glyph("#FFFFFF")}</g>
  <g font-family="Noto Sans, sans-serif" fill="#FFFFFF">
    <text x="470" y="300" font-size="104" font-weight="700" letter-spacing="-2">Wolkenfunk</text>
    <text x="474" y="372" font-size="38" fill-opacity="0.88">Schnelle, native Clients</text>
    <text x="474" y="420" font-size="38" fill-opacity="0.88">für Nextcloud Talk</text>
  </g>
</svg>"""
    render(og, OUT / "og-image.png", 1200, 630)

    for p in sorted(OUT.glob("*")):
        if p.suffix in {".png", ".ico", ".svg"}:
            print(f"{p.relative_to(ROOT)}  {p.stat().st_size} B")


if __name__ == "__main__":
    sys.exit(main())
