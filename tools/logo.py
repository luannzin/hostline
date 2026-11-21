"""Draws Hostline's mark and everything made from it:

    python tools/logo.py

assets/icon.png (512), assets/mark.png, assets/mark-lit.png and assets/mark-out.png (32: the
window with its dusk, lit, and out), assets/banner.webp (the README header, its window lighting
up) with assets/banner.png (the same banner, still, for reduced motion) and assets/preview.png
(1200x630, the link preview of hostline.luannzin.com).

The mark is 32x32 pixel art, Deskline's and Sightline's sibling: a house with a dark outline, and
in its window the dusk from luannzin.com's dither strip (violet, plum, ember, gold) in a 4x4 Bayer
ordered dither. Your localhost is home; the window lights up while someone has the link open and
goes out when the link ends. Pixels are placed one by one and scaled up with nearest-neighbour, so
every size stays crisp.

The banner and the preview are the same world at their sizes: a field of dots on a grid dithered
from the night down to the dusk glow, the mark, and the name in a 5x7 pixel face drawn here.
"""

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


WINDOW = (10, 18, 20, 24)  # the glass, cross included: x0, y0, x1, y1
CROSS = (15, 21)  # the column and row of the window's cross


def glass(x, y, light):
    """The window's colour at (x, y). `light` 0 is out (the night), 1 is the dusk, 2 is lit (gold);
    in between, the next state's dots arrive in Bayer order."""
    x0, y0, x1, y1 = WINDOW
    t = (y - y0) / (y1 - y0)
    states = [
        dither([NIGHT, DUSK[0]], 0.15, x, y),  # out
        dither(DUSK, t, x, y),  # the dusk, darkest at the top
        dither(DUSK[2:], 0.55 + 0.45 * t, x, y),  # lit
    ]
    i = min(1, int(light))
    k = light - i
    return states[i + 1] if BAYER[y % 4][x % 4] / 16 < k else states[i]


def mark(light=1.0):
    """The 32x32 mark, its window out (0), holding the dusk (1) or lit (2)."""
    img = Image.new("RGBA", (32, 32), CLEAR)
    px = img.load()

    def rect(x0, y0, x1, y1, c):
        for y in range(y0, y1 + 1):
            for x in range(x0, x1 + 1):
                px[x, y] = c

    # chimney, behind the roof
    rect(20, 4, 24, 12, LINE)
    rect(21, 5, 23, 12, WALL_SHADE)
    rect(21, 5, 21, 12, WALL)
    # walls: outline, fill, light from the upper left
    rect(5, 13, 26, 29, LINE)
    rect(6, 14, 25, 28, WALL)
    rect(6, 14, 6, 28, WALL_LIT)
    rect(7, 28, 25, 28, WALL_SHADE)
    rect(25, 14, 25, 27, WALL_SHADE)
    # roof: a 45-degree gable that overhangs the walls by two pixels, its left slope in the light
    for y in range(2, 16):
        half = y - 2
        left, right = 15 - half, 16 + half
        if y == 15:  # the eave's underside
            left, right = 2, 29
        for x in range(left, right + 1):
            edge = x in (left, right) or y in (2, 15)
            px[x, y] = LINE if edge else (WALL_LIT if x <= 15 and x - left < 3 else WALL_SHADE if x > 15 else WALL)
    # the window: frame, glass, cross
    x0, y0, x1, y1 = WINDOW
    rect(x0 - 1, y0 - 1, x1 + 1, y1 + 1, LINE)
    for y in range(y0, y1 + 1):
        for x in range(x0, x1 + 1):
            px[x, y] = glass(x, y, light)
    rect(CROSS[0], y0, CROSS[0], y1, LINE)
    rect(x0, CROSS[1], x1, CROSS[1], LINE)
    # the sill
    rect(x0 - 2, y1 + 2, x1 + 2, y1 + 2, WALL_SHADE)
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


def banner(light=1.0):
    """1600x420: the field with the glow at the bottom right, where the mark sits, the name
    and two lines in the pixel face."""
    img = field(1600, 420, (1300, 330), 900)
    img.alpha_composite(scaled(mark(light), 256), (1172, 92))
    text(img, "HOSTLINE", 110, 120, 14, INK)
    text(img, "YOUR LOCALHOST AS A LINK", 112, 260, 4, INK)
    text(img, "ANY PORT · NO ACCOUNT · NO TUNNEL SERVER", 112, 310, 3, MUTED)
    return img


# The window's light over the loop and how long each state rests (ms): the dusk, someone opens
# the link and it lights up, then the link ends and it goes back to the dusk.
LIGHTS = [(1.0, 1600), (2.0, 1600)]
STEPS = 8  # frames for the dots of one state to arrive in Bayer order


def banner_loop():
    """The banner as frames with durations: the window's dots change in Bayer order, one
    sixteenth at a time, then rest."""
    out = []
    for (a, rest), (b, _) in zip(LIGHTS, LIGHTS[1:] + LIGHTS[:1]):
        out.append((banner(a), rest))
        for i in range(1, STEPS):
            # lighting up adds the lit dots; going back adds the dusk's dots back over them
            light = a + (b - a) * i / STEPS
            out.append((banner(light), 60))
    return out


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
    mark(2.0).save(os.path.join(ROOT, "mark-lit.png"))
    mark(0.0).save(os.path.join(ROOT, "mark-out.png"))
    banner().save(os.path.join(ROOT, "banner.png"))
    # The README shows this one; banner.png stays for prefers-reduced-motion.
    loop = banner_loop()
    loop[0][0].save(
        os.path.join(ROOT, "banner.webp"),
        save_all=True,
        append_images=[f for f, _ in loop[1:]],
        duration=[ms for _, ms in loop],
        loop=0,
        lossless=True,
        method=6,
    )
    preview().convert("RGB").save(os.path.join(ROOT, "preview.png"), optimize=True)
    print("wrote", ", ".join(sorted(os.listdir(ROOT))))


if __name__ == "__main__":
    main()
