"""Draws Hostline's mark and everything made from it:

    python tools/logo.py

assets/icon.png (512), assets/mark.png and assets/mark-out.png (32: the globe in the dusk, and
out), assets/mark-spin.png and assets/mark-spin-lit.png (the globe turning in the dusk and lit,
SPIN frames of 32x32 side by side, for the page a link opens), assets/banner.webp (the
README header, the globe turning) with assets/banner.png (the same banner, still, for reduced
motion) and assets/preview.png (1200x630, the link preview of hostline.luannzin.com).

The mark is 32x32 pixel art, Deskline's and Sightline's sibling: a globe drawn as glowing lines,
its meridians and parallels in the dusk from luannzin.com's dither strip (violet at the top,
plum, ember, gold low down), seen from a little above, with a spark running its equator: your
localhost, out on the world. It burns gold while someone has the link open and goes dim when the
link ends. Pixels are placed one by one and scaled up with nearest-neighbour, so every size stays
crisp.

The banner and the preview are the same world at their sizes: a field of dots on a grid dithered
from the night down to the dusk glow, the mark, and the name in a 5x7 pixel face drawn here.
"""

import math
import os

from PIL import Image

ROOT = os.path.join(os.path.dirname(os.path.abspath(__file__)), "..", "assets")


def rgb(h):
    return tuple(int(h[i : i + 2], 16) for i in (1, 3, 5)) + (255,)


NIGHT = rgb("#110d24")
DUSK = [rgb("#3a2a6b"), rgb("#7a3361"), rgb("#e0703f"), rgb("#ffd180")]
LINE = rgb("#1a1424")  # outlines
WALL = rgb("#e8e4f0")
WALL_LIT = rgb("#ffffff")
WALL_SHADE = rgb("#b4acc8")
INK = rgb("#f5f5f5")
MUTED = rgb("#b2b2b2")
CLEAR = (0, 0, 0, 0)
BAYER = ((0, 8, 2, 10), (12, 4, 14, 6), (3, 11, 1, 9), (15, 7, 13, 5))


def dither(ramp, t, x, y):
    """Colour of cell (x, y) at position t (0..1) along `ramp`, ordered-dithered."""
    step = t * (len(ramp) - 1) + BAYER[y % 4][x % 4] / 16 - 0.5
    return ramp[max(0, min(len(ramp) - 1, round(step)))]


CX, CY, R = 15.5, 15.5, 13.0  # the globe
TILT = math.radians(24)  # seen from a little above, so the parallels curve and the pole shows
MERIDIAN, PARALLEL = math.pi / 4, math.pi / 6  # the grid's spacing
DEEP = rgb("#1c1640")  # the globe's own dark, a step above the night it sits on


def cell(x, y, spin):
    """Which grid cell the globe shows at pixel (x, y), or None off the globe."""
    dx, up = (x + 0.5 - CX) / R, (CY - y - 0.5) / R
    d2 = dx * dx + up * up
    if d2 > 1:
        return None
    nz = math.sqrt(1 - d2)
    lat = math.asin(up * math.cos(TILT) + nz * math.sin(TILT))
    lon = math.atan2(dx, nz * math.cos(TILT) - up * math.sin(TILT)) + spin
    # the meridians stop at the polar circles, so the pole is a clean cap, not a knot
    meridian = math.floor(lon / MERIDIAN) if abs(lat) < 2 * PARALLEL else None
    return meridian, math.floor((lat + math.pi / 2) / PARALLEL)


def on_equator(lon):
    """The pixel showing the equator at `lon` (0 faces you)."""
    x = CX + R * math.sin(lon)
    y = CY + R * math.cos(lon) * math.sin(TILT)
    return round(x - 0.5), round(y - 0.5)


def mark(spin=0.0, state="dusk", spark=0.3):
    """The 32x32 mark: a globe drawn as glowing lines, its meridians and parallels in the dusk
    (violet at the top, gold low), and a spark running its equator: your localhost, out on the
    world. `spin` turns the globe (radians), `spark` is where the spark is along the front of the
    equator (0 to 1, left to right). `state`: "dusk" (the link opening), "lit" (someone's in: the
    lines warm to ember and gold) or "out" (no one: the lines go dim, no spark)."""
    img = Image.new("RGBA", (32, 32), CLEAR)
    px = img.load()
    ramp = {"dusk": DUSK, "lit": [DUSK[1], DUSK[2], DUSK[3], DUSK[3]], "out": [DUSK[0], DUSK[0], DUSK[1]]}[state]
    for y in range(32):
        for x in range(32):
            here = cell(x, y, spin)
            if here is None:
                continue
            dx, dy = (x + 0.5 - CX) / R, (y + 0.5 - CY) / R
            d2 = dx * dx + dy * dy
            # a line where the grid cell changes to the right or below, or at the globe's edge
            right, below = cell(x + 1, y, spin), cell(x, y + 1, spin)
            edge = right is None or below is None or d2 > 0.9
            grid = (right is not None and right[0] != here[0]) or (below is not None and below[1] != here[1])
            # the dusk by height, in solid bands: violet at the top, warming to gold low down
            band = ramp[min(len(ramp) - 1, int((0.15 + 0.85 * (dy + 1) / 2) * len(ramp)))]
            px[x, y] = band if edge or grid else DEEP
    # a glint on the upper left
    if state != "out":
        for gx, gy in ((7, 6), (6, 7), (7, 7)):
            px[gx, gy] = WALL_LIT
    # the spark: a bright head running the equator's front, its tail fading behind it
    if state != "out":
        lon = (spark - 0.5) * math.pi * 0.9
        for i, c in enumerate((WALL_LIT, DUSK[3], DUSK[3], DUSK[2], DUSK[1])):
            px[on_equator(lon - i * 0.09)] = c
        hx, hy = on_equator(lon)
        for ox, oy in ((0, -1), (0, 1), (1, 0), (-1, 0)):
            if px[hx + ox, hy + oy] != WALL_LIT:
                px[hx + ox, hy + oy] = DUSK[3]
        px[hx, hy] = WALL_LIT
    # the family's dark outline: every clear pixel touching the art
    art = [[px[x, y][3] > 0 for x in range(32)] for y in range(32)]
    for y in range(32):
        for x in range(32):
            if art[y][x]:
                continue
            if any(0 <= x + ox < 32 and 0 <= y + oy < 32 and art[y + oy][x + ox] for ox, oy in ((-1, 0), (1, 0), (0, -1), (0, 1))):
                px[x, y] = LINE
    return img


def scaled(img, size):
    """Nearest-neighbour up to a multiple, then down to `size` with a box filter so odd sizes
    stay sharp-edged without uneven pixels."""
    k = max(1, -(-size // img.width))
    big = img.resize((img.width * k, img.height * k), Image.NEAREST)
    return big if big.width == size else big.resize((size, size), Image.BOX)


# 5x7 pixel face for the banner: one string of 35 cells per glyph, rows top to bottom.
FONT = {
    "A": ".###.#...##...#######...##...##...#",
    "C": ".###.#...##....#....#....#...#.###.",
    "E": "######....#....####.#....#....#####",
    "H": "#...##...##...#######...##...##...#",
    "I": ".###...#....#....#....#....#...###.",
    "K": "#...##..#.#.#..##...#.#..#..#.#...#",
    "L": "#....#....#....#....#....#....#####",
    "N": "#...###..##.#.##..###...##...##...#",
    "O": ".###.#...##...##...##...##...#.###.",
    "P": "####.#...##...#####.#....#....#....",
    "R": "####.#...##...#####.#.#..#..#.#...#",
    "S": ".#####....#.....###.....#....#####.",
    "T": "#####..#....#....#....#....#....#..",
    "U": "#...##...##...##...##...##...#.###.",
    "V": "#...##...##...##...##...#.#.#...#..",
    "Y": "#...##...#.#.#...#....#....#....#..",
    "·": "." * 17 + "#" + "." * 17,
    " ": "." * 35,
}


def text(img, s, x, y, scale, color):
    """Draw `s` in the 5x7 face, `scale` image pixels per font pixel, 1 font pixel between
    letters. Returns the x after the last letter."""
    px = img.load()
    for ch in s.upper():
        for i, c in enumerate(FONT[ch]):
            if c == "#":
                gx, gy = x + (i % 5) * scale, y + (i // 5) * scale
                for yy in range(gy, gy + scale):
                    for xx in range(gx, gx + scale):
                        px[xx, yy] = color
        x += 6 * scale
    return x


def field(w, h, glow, reach, cell=4):
    """A field of dots on a grid, night at the top dithering down to the dusk glow at `glow`."""
    img = Image.new("RGBA", (w, h), NIGHT)
    px = img.load()
    ramp = [NIGHT] + DUSK
    for y in range(0, h, cell):
        for x in range(0, w, cell):
            near = max(0.0, 1 - ((x - glow[0]) ** 2 + ((y - glow[1]) * 1.6) ** 2) ** 0.5 / reach)
            t = max(near ** 1.6, 0.08 + 0.25 * (y / h) ** 2)
            c = dither(ramp, t, x // cell, y // cell)
            if c != NIGHT:
                for yy in range(y, min(h, y + cell - 1)):  # a dot, with a 1px gap: the grid shows
                    for xx in range(x, min(w, x + cell - 1)):
                        px[xx, yy] = c
    return img


def banner(frame=5):
    """1600x420: the field with the glow at the bottom right, where the mark sits, the name
    and two lines in the pixel face."""
    img = field(1600, 420, (1300, 330), 900)
    img.alpha_composite(scaled(spun(frame), 256), (1172, 82))
    text(img, "HOSTLINE", 110, 120, 14, INK)
    text(img, "YOUR LOCALHOST AS A LINK", 112, 260, 4, INK)
    text(img, "ANY PORT · NO ACCOUNT · NO TUNNEL SERVER", 112, 310, 3, MUTED)
    return img


# One turn of the loop moves the globe one meridian on, where its grid looks the same again, and
# runs the spark once along the equator, so the frames loop with no seam.
SPIN = 16
FRAME = 90  # ms


def spun(frame, state="dusk"):
    """The mark at `frame` of the turn."""
    k = frame / SPIN
    return mark(spin=k * MERIDIAN, state=state, spark=k)


def sprite(state):
    """The turn as one strip, frame after frame, for a CSS steps() animation."""
    img = Image.new("RGBA", (32 * SPIN, 32), CLEAR)
    for f in range(SPIN):
        img.alpha_composite(spun(f, state), (32 * f, 0))
    return img


def preview():
    """1200x630, what chat apps show under a pasted hostline.luannzin.com link."""
    img = field(1200, 630, (600, 700), 820)
    img.alpha_composite(scaled(mark(), 224), ((1200 - 224) // 2, 96))
    name = "HOSTLINE"
    w = len(name) * 6 * 10 - 10
    text(img, name, (1200 - w) // 2, 360, 10, INK)
    line = "YOUR LOCALHOST AS A LINK"
    w = len(line) * 6 * 4 - 4
    text(img, line, (1200 - w) // 2, 470, 4, INK)
    return img


def main():
    os.makedirs(ROOT, exist_ok=True)
    scaled(mark(), 512).save(os.path.join(ROOT, "icon.png"))
    mark().save(os.path.join(ROOT, "mark.png"))
    mark(state="out").save(os.path.join(ROOT, "mark-out.png"))
    sprite("dusk").save(os.path.join(ROOT, "mark-spin.png"))
    sprite("lit").save(os.path.join(ROOT, "mark-spin-lit.png"))
    banner().save(os.path.join(ROOT, "banner.png"))
    # The README shows this one; banner.png stays for prefers-reduced-motion.
    loop = [banner(f) for f in range(SPIN)]
    loop[0].save(
        os.path.join(ROOT, "banner.webp"),
        save_all=True,
        append_images=loop[1:],
        duration=FRAME,
        loop=0,
        lossless=True,
        method=6,
    )
    preview().convert("RGB").save(os.path.join(ROOT, "preview.png"), optimize=True)
    print("wrote", ", ".join(sorted(os.listdir(ROOT))))


if __name__ == "__main__":
    main()
