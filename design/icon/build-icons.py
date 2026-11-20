#!/usr/bin/env python3
"""Builds every app icon of Wolkenfunk from one source: design/icon/wolkenfunk-symbolic.svg.

    python3 design/icon/build-icons.py [desktop] [android] [ios] [website]
    python3 design/icon/build-icons.py store --out DIR

Without arguments it builds all of them except `store`. Results are checked in, so builds (including the
Windows CI) need none of the tools. Running it again gives byte-identical files.

Needs rsvg-convert (librsvg2-bin), Python 3 with Pillow and, for the digits of the Windows
counter, the font DejaVu Sans (fonts-dejavu-core).

  design/icon/app-icon.svg                  full-bleed colour icon (1024, source for iOS)
  desktop/resources/icons/<N>x<N>.png       Linux (hicolor), rounded tile
  desktop/resources/icon.ico                Windows, 16–256
  desktop/resources/icon.icns               macOS, 16–1024, Big Sur grid (824 tile)
  desktop/resources/tray-on-light.png       monochrome tray glyph, dark, for light panels
  desktop/resources/tray-on-dark.png        … white, for dark panels (each with @2x)
  desktop/resources/trayTemplate.png        macOS menu bar template (black + alpha, with @2x)
  desktop/resources/tray*-dot.png           the three tray glyphs with a dot: something unread
  desktop/resources/tray-count-<n>.png      Windows taskbar overlay: 1–9 and "more" (9+)
  android/…/drawable/ic_launcher_*.xml      adaptive icon: background, foreground, monochrome
  android/…/drawable/ic_stat_wolkenfunk.xml notification small icon
  ios/…/AppIcon.appiconset/                 light, dark and tinted 1024 icons
  ios/…/CallKitIcon.imageset/               CallKit template glyph (white + alpha, 40 pt @1–3x)
  ios/…/LoginIcon.imageset/                 the light app icon for the login screen (72 pt @1–3x,
                                            square: the app rounds the corners)
  website/public/…                          via website/scripts/build-assets.py

`store` writes the Google Play graphics to DIR, not into the repo (font: Noto Sans):
  icon-512.png                  512, full square and opaque: Play rounds and shades it
  icon-512-maske-kontrolle.png  the icon under the 30 % and 20 % corner masks, and with the 384 keylines
  feature-1024x500-<variant>.png  feature graphic without alpha, variants "blau" and "hell"
"""
from __future__ import annotations

import io
import json
import re
import subprocess
import sys
import tempfile
from pathlib import Path

from PIL import Image

ROOT = Path(__file__).resolve().parents[2]
SRC = ROOT / "design" / "icon" / "wolkenfunk-symbolic.svg"

BLUE_LIGHT = "#2F7BF5"
BLUE_DARK = "#1650C4"
GLYPH_ON_DARK = "#7AAEFF"
TRAY_DARK = "#2E3436"  # Adwaita dark grey: the glyph on light panels
DOT_ON_LIGHT = BLUE_LIGHT  # "something unread" on the tray glyph and the Windows counter
DOT_ON_DARK = "#5B9BFF"


# --- Source -----------------------------------------------------------------------------

def source() -> tuple[str, list[tuple[str, str | None]]]:
    """The symbolic icon's group transform and its paths (d, fill-rule)."""
    svg = SRC.read_text()
    transform = re.search(r'<g transform="([^"]+)"', svg).group(1)
    paths = [(m.group(2), m.group(1)) for m in
             re.finditer(r'<path(?: fill-rule="(\w+)")? d="([^"]+)"', svg)]
    return transform, paths


def glyph(fill: str) -> str:
    """The glyph on the 512 grid in one colour."""
    transform, paths = source()
    rule = lambda r: f' fill-rule="{r}"' if r else ""
    body = "".join(f'<path{rule(r)} d="{d}"/>' for d, r in paths)
    return f'<g transform="{transform}" fill="{fill}">{body}</g>'


def placed(fill: str, scale: float) -> str:
    """Glyph scaled around the centre of the 512 grid."""
    off = 256 * (1 - scale)
    return f'<g transform="translate({off:.2f} {off:.2f}) scale({scale})">{glyph(fill)}</g>'


GRADIENT = (
    '<defs><linearGradient id="bg" x1="0" y1="0" x2="1" y2="1">'
    f'<stop offset="0" stop-color="{BLUE_LIGHT}"/><stop offset="1" stop-color="{BLUE_DARK}"/>'
    "</linearGradient></defs>"
)


def svg512(body: str) -> str:
    return f'<svg xmlns="http://www.w3.org/2000/svg" viewBox="0 0 512 512">{body}</svg>'


def tray_mark(fill: str, dot: str | None = None) -> str:
    """The tray glyph filling the canvas; `dot`: with a dot in the upper right corner, the
    glyph cut back around it so the dot stands free at 16 px."""
    mark = placed(fill, 1.25)
    if dot is None:
        return svg512(mark)
    cx, cy, r, gap = 396, 116, 96, 36
    return svg512(
        f'<defs><mask id="cut"><rect width="512" height="512" fill="#fff"/>'
        f'<circle cx="{cx}" cy="{cy}" r="{r + gap}" fill="#000"/></mask></defs>'
        f'<g mask="url(#cut)">{mark}</g><circle cx="{cx}" cy="{cy}" r="{r}" fill="{dot}"/>')


def count_badge(label: str) -> str:
    """Windows taskbar overlay: a filled circle with the number of chats."""
    size = 340 if len(label) == 1 else 250
    return svg512(
        f'<circle cx="256" cy="256" r="248" fill="{BLUE_LIGHT}"/>'
        f'<text x="256" y="{256 + size * 0.365:.0f}" text-anchor="middle" font-family="DejaVu Sans" '
        f'font-weight="bold" font-size="{size}" fill="#FFFFFF">{label}</text>')


def square_tile() -> str:
    """Full-bleed square (iOS masks it itself)."""
    return svg512(GRADIENT + '<rect width="512" height="512" fill="url(#bg)"/>' + placed("#FFFFFF", 0.78))


def rounded_tile(inset: float = 0, radius: float = 112) -> str:
    """Rounded tile, optionally inset (macOS grid: 824 of 1024 → inset 50 of 512)."""
    size = 512 - 2 * inset
    scale = 0.78 * size / 512
    return svg512(GRADIENT + f'<rect x="{inset}" y="{inset}" width="{size}" height="{size}" '
                  f'rx="{radius}" fill="url(#bg)"/>' + placed("#FFFFFF", scale))


def render(svg: str, size: int) -> Image.Image:
    with tempfile.NamedTemporaryFile("w", suffix=".svg", delete=False) as f:
        f.write(svg)
        tmp = f.name
    try:
        png = subprocess.run(["rsvg-convert", "-w", str(size), "-h", str(size), tmp],
                             check=True, capture_output=True).stdout
    finally:
        Path(tmp).unlink()
    return Image.open(io.BytesIO(png)).convert("RGBA")


def save_png(img: Image.Image, out: Path) -> None:
    out.parent.mkdir(parents=True, exist_ok=True)
    img.save(out, optimize=True)
    print(f"{out.relative_to(ROOT) if out.is_relative_to(ROOT) else out}")


def write(out: Path, text: str) -> None:
    out.parent.mkdir(parents=True, exist_ok=True)
    out.write_text(text)
    print(f"{out.relative_to(ROOT)}")


# --- Targets ----------------------------------------------------------------------------

def desktop() -> None:
    res = ROOT / "desktop" / "resources"
    tile = rounded_tile()
    for s in (16, 24, 32, 48, 64, 128, 256, 512):
        save_png(render(tile, s), res / "icons" / f"{s}x{s}.png")

    ico = res / "icon.ico"
    render(tile, 256).save(ico, sizes=[(s, s) for s in (16, 24, 32, 48, 64, 128, 256)])
    print(f"{ico.relative_to(ROOT)}")

    icns = res / "icon.icns"
    render(rounded_tile(inset=50, radius=92), 1024).save(icns)
    print(f"{icns.relative_to(ROOT)}")

    # Tray: the glyph alone, filling the canvas (it spans 77 % of the 512 grid) so it reads at 16–22 px.
    for name, fill in (("tray-on-light", TRAY_DARK), ("tray-on-dark", "#FFFFFF"),
                       ("trayTemplate", "#000000")):
        base = 16 if name == "trayTemplate" else 32
        # A template image has one colour: there the dot is told apart by the gap alone.
        dot = {"tray-on-light": DOT_ON_LIGHT, "tray-on-dark": DOT_ON_DARK}.get(name, fill)
        for suffix, mark in (("", tray_mark(fill)), ("-dot", tray_mark(fill, dot))):
            save_png(render(mark, base), res / f"{name}{suffix}.png")
            save_png(render(mark, base * 2), res / f"{name}{suffix}@2x.png")

    # Windows scales the overlay to the small icon size (16 px at 100 %).
    for n in range(1, 10):
        save_png(render(count_badge(str(n)), 32), res / f"tray-count-{n}.png")
    save_png(render(count_badge("9+"), 32), res / "tray-count-more.png")


def android() -> None:
    draw = ROOT / "android" / "app" / "src" / "main" / "res" / "drawable"
    transform, paths = source()
    tx, ty, sc = map(float, re.match(
        r"translate\(([-\d.]+) ([-\d.]+)\) scale\(([-\d.]+)\)", transform).groups())

    def vector(size_dp: int, scale: float, color: str) -> str:
        items = "\n".join(
            f'            <path android:fillColor="{color}"'
            + (' android:fillType="evenOdd"' if rule == "evenodd" else "")
            + f'\n                android:pathData="{d}" />'
            for d, rule in paths)
        return f"""<?xml version="1.0" encoding="utf-8"?>
<!-- Generated by design/icon/build-icons.py from design/icon/wolkenfunk-symbolic.svg. -->
<vector xmlns:android="http://schemas.android.com/apk/res/android"
    android:width="{size_dp}dp" android:height="{size_dp}dp"
    android:viewportWidth="512" android:viewportHeight="512">
    <group android:pivotX="256" android:pivotY="256"
        android:scaleX="{scale}" android:scaleY="{scale}">
        <group android:translateX="{tx:g}" android:translateY="{ty:g}"
            android:scaleX="{sc:g}" android:scaleY="{sc:g}">
{items}
        </group>
    </group>
</vector>
"""

    # Adaptive icon: 108 dp canvas, 72 dp visible, 66 dp safe circle. The glyph fills about
    # 60 % of the visible part, like the tile on the website and on iOS.
    write(draw / "ic_launcher_foreground.xml", vector(108, 0.52, "#FFFFFFFF"))
    write(draw / "ic_launcher_monochrome.xml", vector(108, 0.52, "#FF000000"))
    write(draw / "ic_stat_wolkenfunk.xml", vector(24, 1.0, "#FFFFFFFF"))
    write(draw / "ic_launcher_background.xml", f"""<?xml version="1.0" encoding="utf-8"?>
<!-- Generated by design/icon/build-icons.py. -->
<vector xmlns:android="http://schemas.android.com/apk/res/android"
    xmlns:aapt="http://schemas.android.com/aapt"
    android:width="108dp" android:height="108dp" android:viewportWidth="108" android:viewportHeight="108">
    <path android:pathData="M0,0h108v108h-108z">
        <aapt:attr name="android:fillColor">
            <gradient android:type="linear" android:startX="0" android:startY="0" android:endX="108" android:endY="108">
                <item android:offset="0" android:color="#FF{BLUE_LIGHT[1:]}" />
                <item android:offset="1" android:color="#FF{BLUE_DARK[1:]}" />
            </gradient>
        </aapt:attr>
    </path>
</vector>
""")
    write(draw.parent / "mipmap-anydpi-v26" / "ic_launcher.xml", """<?xml version="1.0" encoding="utf-8"?>
<adaptive-icon xmlns:android="http://schemas.android.com/apk/res/android">
    <background android:drawable="@drawable/ic_launcher_background" />
    <foreground android:drawable="@drawable/ic_launcher_foreground" />
    <monochrome android:drawable="@drawable/ic_launcher_monochrome" />
</adaptive-icon>
""")


def ios() -> None:
    write(ROOT / "design" / "icon" / "app-icon.svg",
          square_tile().replace("<svg ", '<svg width="1024" height="1024" ', 1) + "\n")
    out = ROOT / "ios" / "TalkApp" / "Assets.xcassets" / "AppIcon.appiconset"
    # Light: no alpha allowed.
    save_png(render(square_tile(), 1024).convert("RGB"), out / "AppIcon-1024.png")
    # Dark: glyph on transparent, the system draws the dark background.
    save_png(render(svg512(placed(GLYPH_ON_DARK, 0.78)), 1024), out / "AppIcon-Dark-1024.png")
    # Tinted: greyscale glyph on transparent, the system tints it.
    save_png(render(svg512(placed("#FFFFFF", 0.78)), 1024), out / "AppIcon-Tinted-1024.png")
    entry = {"idiom": "universal", "platform": "ios", "size": "1024x1024"}
    contents = {
        "images": [
            {**entry, "filename": "AppIcon-1024.png"},
            {**entry, "filename": "AppIcon-Dark-1024.png",
             "appearances": [{"appearance": "luminosity", "value": "dark"}]},
            {**entry, "filename": "AppIcon-Tinted-1024.png",
             "appearances": [{"appearance": "luminosity", "value": "tinted"}]},
        ],
        "info": {"author": "xcode", "version": 1},
    }
    write(out / "Contents.json", json.dumps(contents, indent=2) + "\n")
    # CallKit: the glyph alone, white on transparent (the system tints it), 40 pt.
    kit = ROOT / "ios" / "TalkApp" / "Assets.xcassets" / "CallKitIcon.imageset"
    images = []
    for scale in (1, 2, 3):
        name = f"CallKitIcon@{scale}x.png" if scale > 1 else "CallKitIcon.png"
        save_png(render(svg512(placed("#FFFFFF", 0.92)), 40 * scale), kit / name)
        images.append({"idiom": "universal", "filename": name, "scale": f"{scale}x"})
    write(kit / "Contents.json", json.dumps(
        {"images": images, "info": {"author": "xcode", "version": 1},
         "properties": {"template-rendering-intent": "template"}}, indent=2) + "\n")
    # Login screen: the app icon itself, as an image (an appiconset cannot be loaded), 72 pt.
    login = ROOT / "ios" / "TalkApp" / "Assets.xcassets" / "LoginIcon.imageset"
    images = []
    for scale in (1, 2, 3):
        name = f"LoginIcon@{scale}x.png" if scale > 1 else "LoginIcon.png"
        save_png(render(square_tile(), 72 * scale).convert("RGB"), login / name)
        images.append({"idiom": "universal", "filename": name, "scale": f"{scale}x"})
    write(login / "Contents.json", json.dumps(
        {"images": images, "info": {"author": "xcode", "version": 1}}, indent=2) + "\n")


def website() -> None:
    subprocess.run([sys.executable, str(ROOT / "website" / "scripts" / "build-assets.py")], check=True)


# --- Google Play (not checked in: `store --out DIR`) --------------------------------------

FONT = "Noto Sans"
# Feature graphic variants: background gradient, glyph, name, tagline. Play advises against
# pure white, black or dark grey (they blend into its UI), so "hell" is a clear light blue.
FEATURE = {
    "blau": ((BLUE_LIGHT, BLUE_DARK), "#FFFFFF", "#FFFFFF", "#FFFFFF"),
    "hell": (("#E4EEFE", "#C6DAFC"), "#1F63DE", "#142033", "#2B3B55"),
}
TAGLINE = "Nachrichten und Anrufe für deine Nextcloud"


def text_width(text: str, size: int, bold: bool, spacing: float = 0) -> float:
    from PIL import ImageFont
    path = subprocess.run(["fc-match", "-f", "%{file}", f"{FONT}:{'bold' if bold else 'regular'}"],
                          check=True, capture_output=True, text=True).stdout
    return ImageFont.truetype(path, size).getlength(text) + spacing * (len(text) - 1)


def render_wh(svg: str, w: int, h: int) -> Image.Image:
    with tempfile.NamedTemporaryFile("w", suffix=".svg", delete=False) as f:
        f.write(svg)
        tmp = f.name
    try:
        png = subprocess.run(["rsvg-convert", "-w", str(w), "-h", str(h), tmp],
                             check=True, capture_output=True).stdout
    finally:
        Path(tmp).unlink()
    return Image.open(io.BytesIO(png))


def feature_graphic(bg: tuple[str, str], mark: str, name: str, tag: str) -> str:
    """1024×500: glyph and name side by side, tagline below, all in the middle. The lower
    middle stays free for the play button Play lays over it when there is a video."""
    w, h = 1024, 500
    glyph_px, gap, name_size, tag_size, spacing = 150, 28, 104, 40, -2
    name_w = text_width("Wolkenfunk", name_size, True, spacing)
    # The glyph spans about 77 % of its 512 grid; scale so that part is glyph_px tall.
    scale = glyph_px / (512 * 0.77)
    row_w = glyph_px + gap + name_w
    x0 = (w - row_w) / 2
    cy = 200  # middle of the glyph/name row
    # The glyph sits centred on its grid (see placed()).
    gx, gy = x0 + glyph_px / 2 - 256 * scale, cy - 256 * scale
    return (
        f'<svg xmlns="http://www.w3.org/2000/svg" width="{w}" height="{h}" viewBox="0 0 {w} {h}">'
        '<defs><linearGradient id="bg" x1="0" y1="0" x2="1" y2="1">'
        f'<stop offset="0" stop-color="{bg[0]}"/><stop offset="1" stop-color="{bg[1]}"/>'
        f'</linearGradient></defs><rect width="{w}" height="{h}" fill="url(#bg)"/>'
        f'<g transform="translate({gx:.2f} {gy:.2f}) scale({scale:.4f})">{glyph(mark)}</g>'
        f'<g font-family="{FONT}, sans-serif">'
        f'<text x="{x0 + glyph_px + gap:.1f}" y="{cy + name_size * 0.36:.1f}" font-size="{name_size}" '
        f'font-weight="700" letter-spacing="{spacing}" fill="{name}">Wolkenfunk</text>'
        f'<text x="{w / 2}" y="{cy + 118}" font-size="{tag_size}" text-anchor="middle" '
        f'fill="{tag}" fill-opacity="0.92">{TAGLINE}</text></g></svg>')


def mask_check(icon: Image.Image) -> Image.Image:
    """Control sheet: the icon under Play's corner mask (30 % per the current spec, 20 % as
    asked), and with the 384 px keyline square/circle drawn in."""
    from PIL import ImageDraw
    pad, size = 48, 512
    sheet = Image.new("RGB", (3 * size + 4 * pad, size + 2 * pad), "#E8EAED")
    for i, radius in enumerate((0.30, 0.20)):
        m = Image.new("L", (size, size), 0)
        ImageDraw.Draw(m).rounded_rectangle((0, 0, size - 1, size - 1), radius=size * radius, fill=255)
        sheet.paste(icon, (pad + i * (size + pad), pad), m)
    x = pad + 2 * (size + pad)
    sheet.paste(icon, (x, pad))
    d = ImageDraw.Draw(sheet)
    k = (size - 384) / 2
    d.rectangle((x + k, pad + k, x + size - k, pad + size - k), outline="#FF3B30", width=2)
    d.ellipse((x + k, pad + k, x + size - k, pad + size - k), outline="#FFCC00", width=2)
    return sheet


def store(out: Path) -> None:
    icon = render(square_tile(), 512)  # opaque, 32 bit as Play asks; Play rounds and shades
    save_png(icon, out / "icon-512.png")
    save_png(mask_check(icon), out / "icon-512-maske-kontrolle.png")
    for variant, (bg, mark, name, tag) in FEATURE.items():
        # No alpha channel allowed.
        save_png(render_wh(feature_graphic(bg, mark, name, tag), 1024, 500).convert("RGB"),
                 out / f"feature-1024x500-{variant}.png")


TARGETS = {"desktop": desktop, "android": android, "ios": ios, "website": website}

if __name__ == "__main__":
    args = sys.argv[1:]
    if args[:1] == ["store"]:
        if len(args) != 3 or args[1] != "--out":
            sys.exit("usage: build-icons.py store --out DIR  (Play graphics, not checked in)")
        store(Path(args[2]).resolve())
        sys.exit()
    wanted = args or list(TARGETS)
    for name in wanted:
        if name not in TARGETS:
            sys.exit(f"unknown target {name!r}, choose from {', '.join(TARGETS)} (or store --out DIR)")
        TARGETS[name]()
