#!/usr/bin/env python3
"""Cut the Lunar Battle Pack (assets/lunar, CC0) into GBA-sized sprite sheets in gfx/.

The game imports these with agb's include_aseprite! / include_background_gfx! macros.
Run from the repo root after changing any art:  python3 tools/make_gfx.py
Needs Pillow.
"""
import random
from pathlib import Path
from PIL import Image, ImageDraw

ROOT = Path(__file__).resolve().parent.parent
SRC = ROOT / "assets" / "lunar"
OUT = ROOT / "gfx"
OUT.mkdir(exist_ok=True)
random.seed(7)  # stable output, so regenerating doesn't churn the PNGs


def load(name):
    return Image.open(SRC / name).convert("RGBA")


def cell(sheet, cx, cy, size):
    """size x size crop centred on (cx, cy)."""
    h = size // 2
    return sheet.crop((cx - h, cy - h, cx + h, cy + h))


def strip(frames):
    w, h = frames[0].size
    out = Image.new("RGBA", (w * len(frames), h), (0, 0, 0, 0))
    for i, f in enumerate(frames):
        out.paste(f, (i * w, 0))
    return out


def save(img, name):
    img.save(OUT / name)
    colours = {img.getpixel((x, y)) for y in range(img.height) for x in range(img.width) if img.getpixel((x, y))[3] > 0}
    print(f"{name:18} {img.size[0]:>3}x{img.size[1]:<3} {len(colours):>3} colours")


def brighten(img, k=1.9, add=(18, 16, 26)):
    px = img.load()
    for y in range(img.height):
        for x in range(img.width):
            r, g, b, a = px[x, y]
            if a:
                px[x, y] = (min(255, int(r * k + add[0])), min(255, int(g * k + add[1])), min(255, int(b * k * 0.9 + add[2])), 255)
    return img


# Ships, one 32x32 frame each (all face right, angle 0). Spaceships.png is 5 sizes x 3 colours of 64x64 cells.
ships = load("Spaceships.png")
SHIPS = [  # (row, col): rows green/red/blue, cols small..large
    (2, 1),  # 0 player
    (1, 0),  # 1 lone fighter / I-type
    (0, 2),  # 2 P-type (crescent)
    (1, 1),  # 3 E-type (dart)
    (0, 0),  # 4 spy
]
save(strip([cell(ships, c * 64 + 32, r * 64 + 32, 32) for r, c in SHIPS]), "ships.png")

save(cell(load("Fx_02.png"), 24, 8, 16), "bolt.png")
save(cell(load("Missiles.png"), 64 + 32, 32, 16), "missile.png")
save(load("Explosion.png"), "explosion.png")  # 8 frames of 32x32

# Station parts. The pack has no space station, so the hull is drawn here in its steel blues;
# pods are the pack's turret orb, the core is drawn in the game's amber.
STEEL_DARK, STEEL, STEEL_LIGHT, FILL = (20, 40, 74), (61, 95, 146), (111, 147, 200), (14, 26, 48)
hull = Image.new("RGBA", (64, 64), (0, 0, 0, 0))
d = ImageDraw.Draw(hull)
R = 24
hexpts = [(32 + round(R * c), 32 + round(R * s)) for c, s in ((1, 0), (0.5, 0.866), (-0.5, 0.866), (-1, 0), (-0.5, -0.866), (0.5, -0.866))]
d.polygon(hexpts, fill=FILL)
d.line(hexpts + [hexpts[0]], fill=STEEL_DARK, width=5)
d.line(hexpts + [hexpts[0]], fill=STEEL_LIGHT, width=1)
for x, y in hexpts:  # spokes
    d.line([(32 + (x - 32) * 0.45, 32 + (y - 32) * 0.45), (32 + (x - 32) * 0.72, 32 + (y - 32) * 0.72)], fill=STEEL, width=1)
save(hull, "hull.png")

turret = load("Turrets.png").crop((23, 24, 39, 40))
save(turret, "pod.png")

AMBER, AMBER_DARK, CORE_FILL = (255, 200, 97), (184, 120, 40), (43, 15, 34)
core = Image.new("RGBA", (32, 16), (0, 0, 0, 0))
d = ImageDraw.Draw(core)
for i, (fill, line) in enumerate(((CORE_FILL, AMBER), ((255, 255, 255), AMBER))):  # frame 0 normal, 1 hit flash
    ox = i * 16
    pts = [(ox + 8 + round(7 * c), 8 + round(7 * s)) for c, s in ((0.866, 0.5), (0, 1), (-0.866, 0.5), (-0.866, -0.5), (0, -1), (0.866, -0.5))]
    d.polygon(pts, fill=fill, outline=line)
    if i == 0:
        d.ellipse((ox + 6, 6, ox + 10, 10), fill=AMBER_DARK)
        d.point((ox + 8, 8), fill=AMBER)
save(core, "core.png")

rocks = brighten(load("Asteroids_Foreground.png"))
save(cell(rocks, 64 + 32, 32, 64), "rock_big.png")
save(cell(rocks, 192 + 32, 32 + 3, 32), "rock_small.png")

# 5x7 digits on 8x8 cells for the score, plus a ship icon for lives
DIGITS = [
    "01110 10001 10011 10101 11001 10001 01110", "00100 01100 00100 00100 00100 00100 01110",
    "01110 10001 00001 00010 00100 01000 11111", "11111 00010 00100 00010 00001 10001 01110",
    "00010 00110 01010 10010 11111 00010 00010", "11111 10000 11110 00001 00001 10001 01110",
    "00110 01000 10000 11110 10001 10001 01110", "11111 00001 00010 00100 01000 01000 01000",
    "01110 10001 10001 01110 10001 10001 01110", "01110 10001 10001 01111 00001 00010 01100",
]
INK, SHADOW = (236, 232, 255), (40, 34, 80)
font = Image.new("RGBA", (8 * 11, 8), (0, 0, 0, 0))
for i, glyph in enumerate(DIGITS):
    for y, row in enumerate(glyph.split()):
        for x, bit in enumerate(row):
            if bit == "1":
                font.putpixel((i * 8 + x + 2, y + 1), SHADOW)
    for y, row in enumerate(glyph.split()):
        for x, bit in enumerate(row):
            if bit == "1":
                font.putpixel((i * 8 + x + 1, y), INK)
d = ImageDraw.Draw(font)  # frame 10: life icon, a small amber arrow pointing up
d.polygon([(84, 0), (87, 7), (84, 5), (81, 7)], fill=AMBER)
save(font, "digits.png")

# Parallax star layers, 256x256 each so the hardware wraps them seamlessly.
stars = load("Star_Tiles.png")
px = stars.load()
for y in range(stars.height):  # key out the tiles' navy backdrop
    for x in range(stars.width):
        r, g, b, a = px[x, y]
        if r < 8 and abs(g - 16) < 8 and abs(b - 32) < 8:
            px[x, y] = (0, 0, 0, 0)
debris = load("debris.png")


def scatter(img, src, w, h, frames, n):
    for _ in range(n):
        f = random.randrange(*frames)
        img.alpha_composite(src.crop((f * w, 0, f * w + w, h)), (random.randrange(0, 256 - w), random.randrange(0, 256 - h)))


far = Image.new("RGBA", (256, 256), (0, 0, 0, 0))
scatter(far, debris, 64, 64, (0, 6), 3)
scatter(far, stars, 16, 16, (1, 7), 26)
save(far, "stars_far.png")
near = Image.new("RGBA", (256, 256), (0, 0, 0, 0))
scatter(near, stars, 16, 16, (1, 7), 12)
save(near, "stars_near.png")
