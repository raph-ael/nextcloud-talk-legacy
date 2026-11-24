#!/usr/bin/env python3
"""Erzeugt Favicons, App-Icons und das Social-Preview-Bild der Webseite
aus dem Symbol-Icon design/icon/wolkenfunk-symbolic.svg.

Voraussetzungen: rsvg-convert (librsvg2-bin), Python 3 mit Pillow,
Chrome oder Chromium (nur für og-image.png; das Bild zeigt zwei Screenshots der Webseite).

Aufruf (aus dem Repo-Root oder website/):  python3 website/scripts/build-assets.py
Die Ergebnisse landen in website/public/ und werden mit eingecheckt,
damit der Deploy kein Werkzeug braucht.
"""
from __future__ import annotations

import re
import shutil
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


OG_HTML = """<!doctype html><html lang="de"><meta charset="utf-8">
<style>
@font-face {{ font-family: "Inter"; font-weight: 100 900; src: url("{pub}/fonts/inter-latin.woff2") format("woff2"); }}
* {{ box-sizing: border-box; margin: 0; }}
html, body {{ width: 1200px; height: 630px; overflow: hidden; }}
body {{ font-family: "Inter", sans-serif; color: #1D1D1F; position: relative;
  background: linear-gradient(135deg, #FFFFFF 0%, #FFFFFF 35%, #E6EEFB 100%); -webkit-font-smoothing: antialiased; }}
.text {{ position: absolute; left: 72px; top: 0; bottom: 0; width: 530px;
  display: flex; flex-direction: column; justify-content: center; }}
.brand {{ display: flex; align-items: center; gap: 16px; font-size: 38px; font-weight: 600; letter-spacing: -0.02em; }}
.brand svg {{ width: 68px; height: 68px; margin-left: -6px; }}
h1 {{ margin-top: 44px; font-size: 63px; line-height: 1.06; letter-spacing: -0.035em; font-weight: 700; }}
p {{ margin-top: 30px; font-size: 25px; line-height: 1.35; color: #6E6E73; letter-spacing: -0.01em; }}
.mac {{ position: absolute; left: 626px; top: 96px; width: 650px; border-radius: 14px; display: block;
  box-shadow: 0 0 0 1px rgba(0,0,0,.08), 0 30px 70px -20px rgba(20,40,90,.35), 0 10px 24px -10px rgba(20,40,90,.2); }}
.phone {{ position: absolute; left: 968px; top: 200px; width: 196px; padding: 7px; border-radius: 30px;
  background: #111114; box-shadow: 0 30px 60px -18px rgba(10,20,50,.5), 0 0 0 1px rgba(0,0,0,.2); }}
.phone img {{ display: block; width: 100%; border-radius: 23px; }}
</style>
<div class="text">
  <div class="brand"><svg viewBox="0 0 512 512">{glyph}</svg>Wolkenfunk</div>
  <h1>Der schnelle Messenger für deine Nextcloud.</h1>
  <p>Kostenlos für Windows, macOS,<br>Linux und Android</p>
</div>
<img class="mac" src="{pub}/screenshots/mac-chats-hell.webp" alt="">
<div class="phone"><img src="{pub}/screenshots/android-chats-hell.webp" alt=""></div>
</html>
"""


def og_image() -> None:
    """Vorschaubild für geteilte Links: Text links, rechts die App am Mac und auf dem Handy.
    Gesetzt in HTML mit der Schrift und den Bildern der Webseite, fotografiert mit Chrome."""
    chrome = next((c for c in ("google-chrome", "chromium", "chromium-browser") if shutil.which(c)), None)
    if not chrome:
        print("og-image.png übersprungen: kein Chrome/Chromium gefunden", file=sys.stderr)
        return
    with tempfile.TemporaryDirectory() as d:
        page, shot = Path(d) / "og.html", Path(d) / "og.png"
        page.write_text(OG_HTML.format(pub=OUT.as_uri(), glyph=glyph(GLYPH_ON_LIGHT)))
        subprocess.run([chrome, "--headless=new", "--no-sandbox", "--hide-scrollbars",
                        "--allow-file-access-from-files", "--force-device-scale-factor=1",
                        f"--user-data-dir={d}/profil", "--window-size=1200,630",
                        f"--screenshot={shot}", page.as_uri()],
                       check=True, stdout=subprocess.DEVNULL, stderr=subprocess.DEVNULL)
        with Image.open(shot) as im:
            if im.size != (1200, 630):
                sys.exit(f"og-image: {im.size} statt 1200×630")
            im.convert("RGB").save(OUT / "og-image.png", optimize=True)


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
    og_image()

    for p in sorted(OUT.glob("*")):
        if p.suffix in {".png", ".ico", ".svg"}:
            print(f"{p.relative_to(ROOT)}  {p.stat().st_size} B")


if __name__ == "__main__":
    sys.exit(main())
