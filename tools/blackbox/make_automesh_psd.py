#!/usr/bin/env python3
"""Writes an original test PSD for fitting Automatic Mesh on thin strokes
(Q-0047): one layer per stroke, straight (S) and curved (C, a 120° arc),
lengths 60 / 120 / 240 px, thicknesses 4 / 7 / 12 / 20 / 40 px. Layer names
encode the shape: S060_T04 … C240_T40. Prints a JSON description (layer,
kind, length, thickness, centre line points) for the analysis script.

usage: make_automesh_psd.py OUT.psd [DESC.json] [--thick 25,30,40,...]

--thick replaces the thicknesses (Q-0049 measures 25-80 px strokes)."""
import json
import math
import os
import sys

sys.path.insert(0, os.path.join(os.path.dirname(os.path.abspath(__file__)), "..", "e2e"))
import make_character_psd as mk  # noqa: E402
from PIL import Image, ImageDraw  # noqa: E402

LENGTHS = (60, 120, 240)
THICK = (4, 7, 12, 20, 40)
CELL = 320
COLOUR = (40, 60, 120, 255)


def centre_line(kind, length, cx, cy, n=64):
    if kind == "S":
        return [(cx - length / 2 + length * i / (n - 1), cy) for i in range(n)]
    sweep = math.radians(120)
    r = length / sweep
    a0 = math.radians(270) - sweep / 2
    ox, oy = cx, cy + r * 0.75  # arc bulging upwards, roughly centred
    return [(ox + r * math.cos(a0 + sweep * i / (n - 1)), oy + r * math.sin(a0 + sweep * i / (n - 1))) for i in range(n)]


def outline_polygon(pts, t):
    """The stroke as a polygon: the centre line offset by ±t/2 along its
    normals (flat ends), so thick curves have clean edges."""
    left, right = [], []
    n = len(pts)
    for i in range(n):
        a, b = pts[max(i - 1, 0)], pts[min(i + 1, n - 1)]
        dx, dy = b[0] - a[0], b[1] - a[1]
        l = math.hypot(dx, dy) or 1.0
        nx, ny = -dy / l * t / 2, dx / l * t / 2
        left.append((pts[i][0] + nx, pts[i][1] + ny))
        right.append((pts[i][0] - nx, pts[i][1] - ny))
    return left + right[::-1]


def main():
    global THICK
    args = sys.argv[1:]
    if "--thick" in args:
        k = args.index("--thick")
        THICK = tuple(int(t) for t in args[k + 1].split(","))
        del args[k:k + 2]
    out = args[0]
    desc_path = args[1] if len(args) > 1 else os.path.splitext(out)[0] + ".json"
    cols = len(THICK)
    rows = 2 * len(LENGTHS)
    mk.W, mk.H = cols * CELL, rows * CELL
    layers, desc = [], []
    r = 0
    for kind in ("S", "C"):
        for length in LENGTHS:
            for c, t in enumerate(THICK):
                cx, cy = c * CELL + CELL / 2, r * CELL + CELL / 2
                pts = centre_line(kind, length, cx, cy)
                im = Image.new("RGBA", (mk.W, mk.H), (0, 0, 0, 0))
                d = ImageDraw.Draw(im)
                d.polygon(outline_polygon(pts, t), fill=COLOUR)
                name = f"{kind}{length:03d}_T{t:02d}"
                layers.append((name, im))
                desc.append({"layer": name, "kind": kind, "length": length, "thickness": t,
                             "centre": [[round(x, 2), round(y, 2)] for x, y in pts]})
            r += 1
    mk.write_psd(out, layers)
    json.dump(desc, open(desc_path, "w"), indent=1)
    print("wrote", out, desc_path, len(layers), "layers", mk.W, "x", mk.H)


if __name__ == "__main__":
    main()
