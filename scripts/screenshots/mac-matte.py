#!/usr/bin/env python3
"""Cuts the app window out of two screenshots of the whole display (window over a black and
over a white backdrop) for scripts/screenshots/mac.sh. The two backdrops give the exact alpha of
the rounded window corners; everything else is the black shot. Saved as WebP at window size.

  mac-matte.py BLACK.png WHITE.png X Y W H OUT.webp      bounds in points
"""
import sys

import numpy as np
from PIL import Image

black_path, white_path, x, y, w, h, out = sys.argv[1:8]
x, y, w, h = (int(v) for v in (x, y, w, h))
black = Image.open(black_path).convert("RGB")
white = Image.open(white_path).convert("RGB")
scale = black.width / int(sys.argv[8]) if len(sys.argv) > 8 else 2  # pixels per point
box = tuple(round(v * scale) for v in (x, y, x + w, y + h))
b = np.asarray(black.crop(box), dtype=np.float32)
wh = np.asarray(white.crop(box), dtype=np.float32)

# alpha = 1 - (white - black): opaque pixels look the same on both backdrops.
alpha = np.clip(1.0 - (wh - b).mean(axis=2) / 255.0, 0.0, 1.0)
# Only the corners are see-through: elsewhere a pixel that differs (a clock, a caret) is content.
r = round(40 * scale)
mask = np.zeros(alpha.shape, dtype=bool)
mask[:r, :r] = mask[:r, -r:] = mask[-r:, :r] = mask[-r:, -r:] = True
alpha = np.where(mask, alpha, 1.0)
rgb = np.where(alpha[..., None] > 0, b / np.maximum(alpha[..., None], 1e-6), 0)
rgba = np.dstack([np.clip(rgb, 0, 255), alpha * 255]).round().astype(np.uint8)

image = Image.fromarray(rgba, "RGBA").resize((w, h), Image.LANCZOS)
image.save(out, "WEBP", quality=85, method=6)
print(f"{out} {w}x{h}")
