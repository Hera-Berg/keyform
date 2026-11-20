#!/usr/bin/env python3
"""Writes test-data/e2e/character.psd: a small ORIGINAL layered character
(simple shapes drawn here, no third-party art) for the end-to-end test.

Layers (bottom to top): BackHair, ArmL/R, Body, Neck, Face, EyeWhiteL/R,
IrisL/R, EyelidL/R, BrowL/R, Mouth, MouthInner (teeth and tongue, meant to
be clipped by Mouth), SideHairL/R, FrontHair. RGB, 8 bit, RLE (PackBits) channel data.
Needs Pillow only to draw; the PSD writer is below."""
import struct
import sys
from PIL import Image, ImageDraw

W, H = 600, 900


def layer(draw_fn):
    im = Image.new("RGBA", (W, H), (0, 0, 0, 0))
    draw_fn(ImageDraw.Draw(im))
    return im


SKIN, SKIN_D = (250, 222, 200, 255), (225, 180, 160, 255)
HAIR, HAIR_D = (70, 110, 170, 255), (45, 75, 125, 255)
CLOTH, CLOTH_D = (240, 150, 60, 255), (200, 110, 40, 255)
LINE = (60, 40, 40, 255)


def eye(cx, cy):
    return (
        layer(lambda d: d.ellipse([cx - 34, cy - 24, cx + 34, cy + 24], fill=(255, 255, 255, 255), outline=LINE, width=3)),
        layer(lambda d: (d.ellipse([cx - 16, cy - 20, cx + 16, cy + 20], fill=(60, 140, 90, 255)),
                         d.ellipse([cx - 7, cy - 9, cx + 7, cy + 9], fill=(20, 40, 30, 255)),
                         d.ellipse([cx - 9, cy - 15, cx - 3, cy - 9], fill=(255, 255, 255, 255)))),
        layer(lambda d: d.chord([cx - 38, cy - 34, cx + 38, cy + 10], 180, 360, fill=SKIN_D, outline=LINE, width=4)),
    )


def build():
    L = []
    L.append(("BackHair", layer(lambda d: (d.ellipse([140, 60, 460, 420], fill=HAIR_D),
                                            d.rectangle([150, 240, 450, 560], fill=HAIR_D)))))
    for name, sx in (("ArmL", 1), ("ArmR", -1)):
        def arm(d, sx=sx):
            x = lambda v: 300 + sx * (v - 300)
            d.polygon([(x(212), 478), (x(176), 486), (x(112), 742), (x(148), 754)], fill=CLOTH_D, outline=LINE)
            d.ellipse([x(130) - 17, 748, x(130) + 17, 784], fill=SKIN, outline=LINE, width=2)
        L.append((name, layer(arm)))
    L.append(("Body", layer(lambda d: (d.polygon([(200, 470), (400, 470), (450, 860), (150, 860)], fill=CLOTH, outline=CLOTH_D),
                                        d.line([(300, 480), (300, 850)], fill=CLOTH_D, width=4)))))
    L.append(("Neck", layer(lambda d: d.rectangle([272, 400, 328, 490], fill=SKIN_D))))
    L.append(("Face", layer(lambda d: d.ellipse([170, 110, 430, 430], fill=SKIN, outline=LINE, width=3))))
    eL, eR = eye(245, 280), eye(355, 280)
    L += [("EyeWhiteL", eL[0]), ("EyeWhiteR", eR[0]), ("IrisL", eL[1]), ("IrisR", eR[1]),
          ("EyelidL", eL[2]), ("EyelidR", eR[2])]
    L.append(("BrowL", layer(lambda d: d.line([(212, 232), (240, 222), (275, 228)], fill=HAIR_D, width=7))))
    L.append(("BrowR", layer(lambda d: d.line([(325, 228), (360, 222), (388, 232)], fill=HAIR_D, width=7))))
    L.append(("Mouth", layer(lambda d: (d.chord([272, 350, 328, 390], 0, 180, fill=(190, 70, 80, 255), outline=LINE, width=3)))))
    L.append(("MouthInner", layer(lambda d: (d.rectangle([280, 370, 320, 377], fill=(255, 255, 255, 255)),
                                              d.ellipse([288, 380, 312, 396], fill=(225, 110, 120, 255))))))
    for name, sx in (("SideHairL", 1), ("SideHairR", -1)):
        def side(d, sx=sx):
            x = lambda v: 300 + sx * (v - 300)
            d.polygon([(x(158), 190), (x(200), 196), (x(192), 470), (x(166), 488), (x(146), 462)], fill=HAIR, outline=HAIR_D)
        L.append((name, layer(side)))
    L.append(("FrontHair", layer(lambda d: (d.pieslice([160, 80, 440, 330], 180, 360, fill=HAIR),
                                             d.polygon([(170, 205), (230, 205), (200, 300)], fill=HAIR),
                                             d.polygon([(260, 205), (330, 205), (290, 270)], fill=HAIR),
                                             d.polygon([(360, 205), (432, 205), (405, 300)], fill=HAIR)))))
    return L


def pascal(name):
    b = name.encode("ascii")[:255]
    s = bytes([len(b)]) + b
    return s + b"\0" * ((4 - len(s) % 4) % 4)


def packbits(row):
    out, i, n = bytearray(), 0, len(row)
    while i < n:
        j = i
        while j < n and j - i < 128 and row[j] == row[i]:
            j += 1
        if j - i >= 2:
            out += bytes([257 - (j - i), row[i]])
            i = j
            continue
        j = i
        while j < n and j - i < 128 and not (j + 1 < n and row[j] == row[j + 1]):
            j += 1
        j = max(j, i + 1)
        out += bytes([j - i - 1]) + row[i:j]
        i = j
    return bytes(out)


def rle_channel(ch):
    w, h = ch.size
    raw = ch.tobytes()
    rows = [packbits(raw[y * w:(y + 1) * w]) for y in range(h)]
    return struct.pack(">H", 1) + b"".join(struct.pack(">H", len(r)) for r in rows) + b"".join(rows)


def write_psd(path, layers):
    recs, data = b"", b""
    for name, im in layers:
        x0, y0, x1, y1 = im.getbbox() or (0, 0, 1, 1)
        crop = im.crop((x0, y0, x1, y1))
        r, g, b, a = crop.split()
        chans = [(-1, a), (0, r), (1, g), (2, b)]
        rec = struct.pack(">iiiiH", y0, x0, y1, x1, len(chans))
        enc = [rle_channel(ch) for _, ch in chans]
        for (cid, _), e in zip(chans, enc):
            rec += struct.pack(">hI", cid, len(e))
        rec += b"8BIMnorm" + struct.pack(">BBBB", 255, 0, 0, 0)
        extra = struct.pack(">I", 0) + struct.pack(">I", 0) + pascal(name)
        rec += struct.pack(">I", len(extra)) + extra
        recs += rec
        data += b"".join(enc)
    info = struct.pack(">h", len(layers)) + recs + data
    if len(info) % 2:
        info += b"\0"
    lmi = struct.pack(">I", len(info)) + info + struct.pack(">I", 0)
    merged = Image.new("RGBA", (W, H), (255, 255, 255, 0))
    for _, im in layers:
        merged = Image.alpha_composite(merged, im)
    out = b"8BPS" + struct.pack(">H6xHIIHH", 1, 4, H, W, 8, 3)
    out += struct.pack(">I", 0) + struct.pack(">I", 0)
    out += struct.pack(">I", len(lmi)) + lmi
    planes = merged.split()
    rows = [packbits(c.tobytes()[y * W:(y + 1) * W]) for c in planes for y in range(H)]
    out += struct.pack(">H", 1) + b"".join(struct.pack(">H", len(r)) for r in rows) + b"".join(rows)
    with open(path, "wb") as f:
        f.write(out)


if __name__ == "__main__":
    out = sys.argv[1] if len(sys.argv) > 1 else "test-data/e2e/character.psd"
    write_psd(out, build())
    print("wrote", out)
