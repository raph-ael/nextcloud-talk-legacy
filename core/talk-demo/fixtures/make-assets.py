#!/usr/bin/env python3
"""Generates the demo server's pictures and the voice message (all invented).

    python3 core/talk-demo/fixtures/make-assets.py

Needs rsvg-convert, Pillow and ffmpeg (with libmp3lame). The results are checked in, so
the server itself needs none of these tools. Re-run after changing a drawing below.

- avatars/<id>.png   people (flat illustrated faces) and group pictures, 512×512
- media/garden.jpg   the shared photo (an illustrated garden), 1600×1200
- media/voice.mp3    the voice message: a few seconds of a soft, speech-like hum
"""

import io
import math
import random
import struct
import subprocess
import tempfile
import wave
from pathlib import Path

from PIL import Image

HERE = Path(__file__).resolve().parent
AVATARS = HERE / "avatars"
MEDIA = HERE / "media"

SKIN = {
    "light": "#F6D7C3",
    "fair": "#F1C7A8",
    "medium": "#D9A27C",
    "tan": "#B97E57",
    "dark": "#8D5A3B",
}

# id: background, skin, hair colour, hair style, shirt colour, extras
PEOPLE = {
    "mira": ("#7C9CF5", "fair", "#6B4226", "long", "#FFFFFF", []),
    "lena": ("#F6B26B", "light", "#B5502A", "curly_long", "#2E7D6B", []),
    "tobias": ("#6FC2B0", "medium", "#2B2118", "short", "#37474F", ["beard"]),
    "hilde": ("#C9A7EB", "light", "#D9D9DE", "bun", "#8E5BB5", ["glasses"]),
    "paul": ("#F28B82", "light", "#E3B34C", "short_spiky", "#3F51B5", []),
    "sophie": ("#81C995", "tan", "#1E1A18", "bob", "#F9E79F", []),
    "jonas": ("#FDD663", "dark", "#1B1210", "curly_short", "#E65100", []),
    "clara": ("#78D9EC", "fair", "#EBCB7A", "ponytail", "#C2185B", ["glasses"]),
    "emil": ("#A7C7E7", "light", "#9E9E9E", "receding", "#5D4037", ["mustache"]),
}


def face_svg(bg, skin_key, hair, style, shirt, extras):
    skin = SKIN[skin_key]
    shade = "#000000"
    back = ""  # hair behind the head
    front = ""  # hair on top of the head
    if style == "long":
        back = f'<path d="M150 250 Q150 120 256 112 Q362 120 362 250 L372 420 Q256 450 140 420 Z" fill="{hair}"/>'
        front = f'<path d="M160 240 Q170 128 256 124 Q350 128 356 236 Q300 170 236 178 Q190 190 160 240 Z" fill="{hair}"/>'
    elif style == "curly_long":
        circles = "".join(
            f'<circle cx="{256 + 120 * math.cos(a):.0f}" cy="{250 + 130 * math.sin(a):.0f}" r="46" fill="{hair}"/>'
            for a in [math.pi * (0.75 + i * 0.125) for i in range(13)]
        )
        back = circles + f'<path d="M140 260 L130 400 Q256 440 382 400 L372 260 Z" fill="{hair}"/>'
        front = "".join(
            f'<circle cx="{x}" cy="{y}" r="34" fill="{hair}"/>'
            for x, y in [(186, 168), (230, 146), (282, 146), (326, 168), (350, 206), (164, 206)]
        )
    elif style == "short":
        front = f'<path d="M164 236 Q160 130 256 122 Q352 130 348 236 Q340 188 300 176 Q256 196 210 176 Q172 190 164 236 Z" fill="{hair}"/>'
    elif style == "short_spiky":
        front = (
            f'<path d="M164 230 L170 160 L196 176 L210 128 L238 160 L262 116 L284 158 L314 126 L322 172 '
            f'L350 160 L348 230 Q330 186 256 182 Q186 186 164 230 Z" fill="{hair}"/>'
        )
    elif style == "bun":
        back = f'<circle cx="256" cy="104" r="50" fill="{hair}"/>'
        front = f'<path d="M160 244 Q156 132 256 126 Q356 132 352 244 Q340 176 256 168 Q172 176 160 244 Z" fill="{hair}"/>'
    elif style == "bob":
        back = f'<path d="M144 250 Q144 116 256 112 Q368 116 368 250 L368 330 Q256 344 144 330 Z" fill="{hair}"/>'
        front = f'<path d="M156 250 Q160 130 256 124 Q352 130 356 250 Q352 190 300 168 L200 168 Q160 190 156 250 Z" fill="{hair}"/>'
    elif style == "curly_short":
        front = "".join(
            f'<circle cx="{x}" cy="{y}" r="30" fill="{hair}"/>'
            for x, y in [(176, 196), (196, 156), (236, 134), (276, 134), (316, 156), (336, 196), (256, 150), (216, 160), (296, 160)]
        )
    elif style == "ponytail":
        back = f'<path d="M330 170 Q420 200 400 330 Q390 380 360 400 Q380 320 340 230 Z" fill="{hair}"/>'
        front = f'<path d="M160 240 Q160 128 256 122 Q352 128 352 240 Q330 176 256 170 Q182 176 160 240 Z" fill="{hair}"/>'
    elif style == "receding":
        front = (
            f'<path d="M160 250 Q156 190 176 168 Q190 200 196 236 Z" fill="{hair}"/>'
            f'<path d="M352 250 Q356 190 336 168 Q322 200 316 236 Z" fill="{hair}"/>'
        )

    extra = ""
    if "glasses" in extras:
        extra += (
            '<g fill="none" stroke="#3B3B4F" stroke-width="7">'
            '<circle cx="216" cy="252" r="30"/><circle cx="296" cy="252" r="30"/>'
            '<path d="M246 250 Q256 242 266 250"/></g>'
        )
    if "beard" in extras:
        extra += f'<path d="M180 270 Q186 360 256 372 Q326 360 332 270 Q320 320 256 326 Q192 320 180 270 Z" fill="{hair}"/>'
    if "mustache" in extras:
        extra += f'<path d="M222 300 Q256 284 290 300 Q270 312 256 304 Q242 312 222 300 Z" fill="{hair}"/>'

    return f"""<svg xmlns="http://www.w3.org/2000/svg" width="512" height="512" viewBox="0 0 512 512">
  <rect width="512" height="512" fill="{bg}"/>
  <circle cx="420" cy="90" r="120" fill="#FFFFFF" opacity="0.12"/>
  {back}
  <path d="M96 512 Q100 400 256 392 Q412 400 416 512 Z" fill="{shirt}"/>
  <path d="M216 330 L296 330 L300 400 Q256 420 212 400 Z" fill="{skin}"/>
  <path d="M216 360 Q256 380 296 360 L296 340 L216 340 Z" fill="{shade}" opacity="0.08"/>
  <ellipse cx="164" cy="262" rx="18" ry="26" fill="{skin}"/>
  <ellipse cx="348" cy="262" rx="18" ry="26" fill="{skin}"/>
  <ellipse cx="256" cy="250" rx="92" ry="108" fill="{skin}"/>
  {front}
  <ellipse cx="216" cy="254" rx="10" ry="12" fill="#2B2B3A"/>
  <ellipse cx="296" cy="254" rx="10" ry="12" fill="#2B2B3A"/>
  <circle cx="219" cy="250" r="3" fill="#FFFFFF"/>
  <circle cx="299" cy="250" r="3" fill="#FFFFFF"/>
  <path d="M200 226 Q216 216 232 224" stroke="#3A2A20" stroke-width="5" fill="none" stroke-linecap="round" opacity="0.6"/>
  <path d="M280 224 Q296 216 312 226" stroke="#3A2A20" stroke-width="5" fill="none" stroke-linecap="round" opacity="0.6"/>
  <circle cx="196" cy="294" r="16" fill="#FF7A7A" opacity="0.25"/>
  <circle cx="316" cy="294" r="16" fill="#FF7A7A" opacity="0.25"/>
  <path d="M250 270 Q256 292 262 270" stroke="{shade}" stroke-width="4" fill="none" opacity="0.15" stroke-linecap="round"/>
  <path d="M226 312 Q256 336 286 312" stroke="#7A3B2E" stroke-width="7" fill="none" stroke-linecap="round"/>
  {extra}
</svg>"""


def group_svg(bg, body):
    return f"""<svg xmlns="http://www.w3.org/2000/svg" width="512" height="512" viewBox="0 0 512 512">
  <rect width="512" height="512" fill="{bg}"/>
  <circle cx="100" cy="420" r="160" fill="#FFFFFF" opacity="0.10"/>
  <circle cx="440" cy="80" r="110" fill="#FFFFFF" opacity="0.10"/>
  {body}
</svg>"""


GROUPS = {
    # Familie: a house with a heart
    "familie": group_svg(
        "#F4A261",
        """<path d="M256 120 L410 250 L370 250 L370 392 L142 392 L142 250 L102 250 Z" fill="#FFF4E6"/>
           <path d="M256 104 L430 252 L400 270 L256 148 L112 270 L82 252 Z" fill="#B5523B"/>
           <rect x="330" y="132" width="30" height="64" fill="#B5523B"/>
           <path d="M256 330 C200 290 190 250 218 232 C236 222 252 232 256 246 C260 232 276 222 294 232 C322 250 312 290 256 330 Z" fill="#E76F51"/>""",
    ),
    # Gartenprojekt: a sprout in a pot
    "garten": group_svg(
        "#8BC34A",
        """<path d="M256 330 Q250 250 256 190" stroke="#2E7D32" stroke-width="16" fill="none" stroke-linecap="round"/>
           <path d="M256 250 Q180 250 150 180 Q230 170 256 250 Z" fill="#43A047"/>
           <path d="M256 210 Q320 200 360 130 Q270 120 256 210 Z" fill="#66BB6A"/>
           <path d="M176 320 L336 320 L314 420 L198 420 Z" fill="#D9825B"/>
           <rect x="166" y="304" width="180" height="32" rx="8" fill="#C46A45"/>""",
    ),
    # Lesekreis: an open book
    "lesekreis": group_svg(
        "#9575CD",
        """<path d="M256 170 Q190 130 110 150 L110 370 Q190 350 256 390 Z" fill="#FFFFFF"/>
           <path d="M256 170 Q322 130 402 150 L402 370 Q322 350 256 390 Z" fill="#F3EEFF"/>
           <path d="M256 170 L256 390" stroke="#7E57C2" stroke-width="6"/>
           <g stroke="#B39DDB" stroke-width="8" stroke-linecap="round">
             <path d="M140 200 Q190 190 226 206"/><path d="M140 240 Q190 230 226 246"/><path d="M140 280 Q190 270 226 286"/>
             <path d="M286 206 Q322 190 372 200"/><path d="M286 246 Q322 230 372 240"/><path d="M286 286 Q322 270 372 280"/>
           </g>
           <path d="M330 140 L330 230 L350 214 L370 230 L370 140 Z" fill="#E57373"/>""",
    ),
    # Team Website: a browser window with layout blocks
    "website": group_svg(
        "#4A90E2",
        """<rect x="96" y="136" width="320" height="240" rx="22" fill="#FFFFFF"/>
           <path d="M96 158 Q96 136 118 136 L394 136 Q416 136 416 158 L416 180 L96 180 Z" fill="#DCE8F8"/>
           <circle cx="124" cy="158" r="8" fill="#F28B82"/><circle cx="150" cy="158" r="8" fill="#FDD663"/><circle cx="176" cy="158" r="8" fill="#81C995"/>
           <rect x="124" y="204" width="130" height="80" rx="10" fill="#A7C7F2"/>
           <rect x="272" y="204" width="116" height="16" rx="8" fill="#C9D9EE"/>
           <rect x="272" y="236" width="90" height="16" rx="8" fill="#C9D9EE"/>
           <rect x="272" y="268" width="104" height="16" rx="8" fill="#C9D9EE"/>
           <rect x="124" y="306" width="264" height="44" rx="12" fill="#FFB74D"/>""",
    ),
    # Sommerfest: balloons
    "sommerfest": group_svg(
        "#F06292",
        """<path d="M210 300 Q220 360 250 420" stroke="#FFFFFF" stroke-width="5" fill="none"/>
           <path d="M300 280 Q290 350 262 420" stroke="#FFFFFF" stroke-width="5" fill="none"/>
           <path d="M256 330 Q256 380 256 420" stroke="#FFFFFF" stroke-width="5" fill="none"/>
           <ellipse cx="200" cy="220" rx="62" ry="76" fill="#FFD54F"/>
           <ellipse cx="310" cy="200" rx="62" ry="76" fill="#4FC3F7"/>
           <ellipse cx="256" cy="262" rx="58" ry="70" fill="#FFFFFF"/>
           <ellipse cx="180" cy="196" rx="14" ry="22" fill="#FFFFFF" opacity="0.5"/>
           <ellipse cx="290" cy="176" rx="14" ry="22" fill="#FFFFFF" opacity="0.5"/>""",
    ),
}


def garden_svg():
    rnd = random.Random(7)
    parts = []
    # sky, sun, clouds
    parts.append(
        """<defs>
             <linearGradient id="sky" x1="0" y1="0" x2="0" y2="1">
               <stop offset="0" stop-color="#8EC9F2"/><stop offset="1" stop-color="#E3F3FC"/>
             </linearGradient>
             <linearGradient id="grass" x1="0" y1="0" x2="0" y2="1">
               <stop offset="0" stop-color="#8CCB5E"/><stop offset="1" stop-color="#5E9E3A"/>
             </linearGradient>
           </defs>
           <rect width="1600" height="1200" fill="url(#sky)"/>
           <g stroke="#FFD54F" stroke-width="14" stroke-linecap="round">
             <path d="M1300 50 L1300 80"/><path d="M1300 340 L1300 370"/><path d="M1140 210 L1170 210"/><path d="M1430 210 L1460 210"/>
             <path d="M1187 97 L1208 118"/><path d="M1392 302 L1413 323"/><path d="M1187 323 L1208 302"/><path d="M1392 118 L1413 97"/>
           </g>
           <circle cx="1300" cy="210" r="110" fill="#FFD54F"/>
           <circle cx="1300" cy="210" r="88" fill="#FFE07A"/>"""
    )
    for cx, cy, s in [(300, 180, 1.0), (760, 120, 0.8), (1000, 260, 0.6)]:
        parts.append(
            f'<g fill="#FFFFFF" opacity="0.9" transform="translate({cx} {cy}) scale({s})">'
            '<ellipse cx="0" cy="0" rx="110" ry="46"/><ellipse cx="-70" cy="10" rx="70" ry="36"/>'
            '<ellipse cx="80" cy="12" rx="80" ry="38"/><ellipse cx="10" cy="-30" rx="70" ry="44"/></g>'
        )
    # hills
    parts.append('<path d="M0 640 Q300 470 640 600 Q980 720 1300 540 Q1460 470 1600 520 L1600 1200 L0 1200 Z" fill="#A9D98B"/>')
    parts.append('<path d="M0 720 Q400 600 800 700 Q1200 800 1600 660 L1600 1200 L0 1200 Z" fill="#93C96F"/>')
    # trees in the distance
    for x, y, r in [(180, 600, 60), (260, 620, 46), (1380, 560, 70), (1470, 580, 50)]:
        parts.append(f'<rect x="{x - 8}" y="{y}" width="16" height="70" fill="#7A5A3A"/>')
        parts.append(f'<circle cx="{x}" cy="{y - 10}" r="{r}" fill="#5FA049"/>')
    # fence
    parts.append('<rect x="0" y="720" width="1600" height="16" fill="#E8D5B5"/>')
    parts.append('<rect x="0" y="790" width="1600" height="16" fill="#E8D5B5"/>')
    for x in range(20, 1600, 70):
        parts.append(f'<path d="M{x} 700 L{x + 18} 680 L{x + 36} 700 L{x + 36} 840 L{x} 840 Z" fill="#F4E6CC"/>')
    # lawn
    parts.append('<path d="M0 820 Q800 780 1600 820 L1600 1200 L0 1200 Z" fill="url(#grass)"/>')
    for _ in range(140):
        x = rnd.uniform(0, 1600)
        y = rnd.uniform(860, 1190)
        parts.append(f'<path d="M{x:.0f} {y:.0f} q4 -18 8 0" stroke="#4E8A30" stroke-width="3" fill="none" opacity="0.6"/>')
    # raised bed
    parts.append('<path d="M330 900 L1270 900 L1310 1010 L290 1010 Z" fill="#6B4A2F"/>')
    for i, y in enumerate([1010, 1050, 1090]):
        shade = ["#B07A4A", "#A06C40", "#B07A4A"][i]
        parts.append(f'<rect x="290" y="{y}" width="1020" height="40" fill="{shade}"/>')
    parts.append('<rect x="290" y="1010" width="1020" height="120" fill="none" stroke="#8A5C34" stroke-width="4"/>')
    for x in [290, 790, 1290]:
        parts.append(f'<rect x="{x}" y="1000" width="24" height="134" fill="#8A5C34"/>')
    # sunflowers behind the bed
    for x, h in [(380, 360), (470, 420), (1180, 390)]:
        top = 900 - h
        parts.append(f'<path d="M{x} 905 L{x} {top}" stroke="#4E8A30" stroke-width="12"/>')
        parts.append(f'<path d="M{x} {top + 160} q-60 -10 -80 30 q50 20 80 -30" fill="#5FA049"/>')
        parts.append(f'<path d="M{x} {top + 230} q60 -10 80 30 q-50 20 -80 -30" fill="#5FA049"/>')
        for k in range(14):
            a = k * math.tau / 14
            px = x + 52 * math.cos(a)
            py = top + 52 * math.sin(a)
            parts.append(
                f'<ellipse cx="{px:.0f}" cy="{py:.0f}" rx="30" ry="13" fill="#FFC928" '
                f'transform="rotate({math.degrees(a):.0f} {px:.0f} {py:.0f})"/>'
            )
        parts.append(f'<circle cx="{x}" cy="{top}" r="34" fill="#7A4A1E"/>')
    # tomatoes on stakes
    for x in [640, 760, 880]:
        parts.append(f'<rect x="{x - 5}" y="700" width="10" height="210" fill="#C8A26E"/>')
        for _ in range(9):
            lx = x + rnd.uniform(-50, 50)
            ly = rnd.uniform(720, 890)
            parts.append(f'<ellipse cx="{lx:.0f}" cy="{ly:.0f}" rx="34" ry="18" fill="#4F9A3A" transform="rotate({rnd.uniform(-40, 40):.0f} {lx:.0f} {ly:.0f})"/>')
        for _ in range(5):
            tx = x + rnd.uniform(-40, 40)
            ty = rnd.uniform(740, 880)
            parts.append(f'<circle cx="{tx:.0f}" cy="{ty:.0f}" r="17" fill="#E53935"/><circle cx="{tx - 5:.0f}" cy="{ty - 6:.0f}" r="5" fill="#FFFFFF" opacity="0.5"/>')
    # lettuce heads
    for x in [1000, 1090]:
        for r, c in [(62, "#7CC35A"), (46, "#97D670"), (28, "#B5E58F")]:
            parts.append(f'<circle cx="{x}" cy="{900 - r * 0.4:.0f}" r="{r}" fill="{c}"/>')
    # flowers in the lawn
    for _ in range(26):
        x = rnd.uniform(40, 1560)
        y = rnd.uniform(1080, 1180) if 280 < x < 1320 else rnd.uniform(880, 1180)
        col = rnd.choice(["#FFFFFF", "#F8BBD0", "#FFF59D", "#CE93D8"])
        for k in range(5):
            a = k * math.tau / 5
            parts.append(f'<circle cx="{x + 9 * math.cos(a):.0f}" cy="{y + 9 * math.sin(a):.0f}" r="7" fill="{col}"/>')
        parts.append(f'<circle cx="{x:.0f}" cy="{y:.0f}" r="5" fill="#FFB300"/>')
    # watering can
    parts.append(
        """<g transform="translate(1340 1000)">
             <path d="M0 0 L110 0 L100 110 L10 110 Z" fill="#4FA3D9"/>
             <path d="M110 30 L200 -40 L210 -30 L120 60 Z" fill="#4FA3D9"/>
             <ellipse cx="205" cy="-36" rx="22" ry="12" fill="#3A8BC2" transform="rotate(-40 205 -36)"/>
             <path d="M20 0 Q55 -70 90 0" stroke="#3A8BC2" stroke-width="12" fill="none"/>
           </g>"""
    )
    # a butterfly
    parts.append(
        """<g transform="translate(980 560) rotate(-15)">
             <ellipse cx="-22" cy="-10" rx="24" ry="18" fill="#FF8A65"/><ellipse cx="22" cy="-10" rx="24" ry="18" fill="#FF8A65"/>
             <ellipse cx="-16" cy="14" rx="14" ry="11" fill="#FFAB91"/><ellipse cx="16" cy="14" rx="14" ry="11" fill="#FFAB91"/>
             <rect x="-3" y="-24" width="6" height="46" rx="3" fill="#4E342E"/>
           </g>"""
    )
    return '<svg xmlns="http://www.w3.org/2000/svg" width="1600" height="1200" viewBox="0 0 1600 1200">' + "".join(parts) + "</svg>"


def render(svg: str, width: int) -> Image.Image:
    png = subprocess.run(
        ["rsvg-convert", "-w", str(width), "-f", "png"],
        input=svg.encode(),
        capture_output=True,
        check=True,
    ).stdout
    return Image.open(io.BytesIO(png)).convert("RGB")


def voice(path: Path):
    """About 6 s of a soft hum with a syllable rhythm, so the waveform looks like speech."""
    rate = 22050
    rnd = random.Random(3)
    samples = []
    t = 0.0
    phrases = [1.6, 1.3, 1.9]
    phase = 0.0
    for p, length in enumerate(phrases):
        n = int(length * rate)
        syll = rnd.uniform(4.0, 5.5)
        base = 150 + 25 * p
        for i in range(n):
            x = i / rate
            pitch = base * (1 + 0.08 * math.sin(math.tau * 0.7 * x) + 0.04 * math.sin(math.tau * syll * x / 2))
            phase += math.tau * pitch / rate
            env = max(0.0, math.sin(math.pi * x * syll)) ** 1.5
            env *= min(1.0, x * 8, (length - x) * 8)
            v = (0.6 * math.sin(phase) + 0.25 * math.sin(2 * phase) + 0.1 * math.sin(3 * phase)) * env * 0.35
            samples.append(v)
            t += 1 / rate
        samples.extend([0.0] * int(0.35 * rate))
    with tempfile.TemporaryDirectory() as tmp:
        wav = Path(tmp) / "voice.wav"
        with wave.open(str(wav), "wb") as w:
            w.setnchannels(1)
            w.setsampwidth(2)
            w.setframerate(rate)
            w.writeframes(b"".join(struct.pack("<h", int(max(-1, min(1, s)) * 32000)) for s in samples))
        subprocess.run(
            ["ffmpeg", "-y", "-loglevel", "error", "-i", str(wav), "-codec:a", "libmp3lame",
             "-b:a", "32k", "-ac", "1", "-ar", "22050", "-map_metadata", "-1", str(path)],
            check=True,
        )


def main():
    AVATARS.mkdir(exist_ok=True)
    MEDIA.mkdir(exist_ok=True)
    for pid, spec in PEOPLE.items():
        render(face_svg(*spec), 512).save(AVATARS / f"{pid}.png", optimize=True)
    for gid, svg in GROUPS.items():
        render(svg, 512).save(AVATARS / f"{gid}.png", optimize=True)
    render(garden_svg(), 1600).save(MEDIA / "garden.jpg", quality=86, optimize=True, progressive=True)
    voice(MEDIA / "voice.mp3")
    for f in sorted(list(AVATARS.iterdir()) + list(MEDIA.iterdir())):
        print(f"{f.relative_to(HERE)}  {f.stat().st_size // 1024} KB")


if __name__ == "__main__":
    main()
