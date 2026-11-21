#!/usr/bin/env python3
"""Measures automatic meshes of the stroke test PSD (make_automesh_psd.py)
from a .cmo3's XML dump (`kf xml`): per layer the ring (boundary) vertex
count, ring perimeter and spacing, margin (ring distance outside the
stroke), skeleton points (within the stroke, near its centre line) and
other inner points. Prints a table; with two dumps, both side by side.

usage: automesh_table.py DESC.json A.xml [B.xml] [--json OUT]"""
import json
import math
import re
import sys


def meshes(xml_path):
    x = open(xml_path).read()
    out = {}
    for m in re.finditer(r"<CArtMeshSource.*?</CArtMeshSource>", x, re.S):
        blk = m.group(0)
        name = re.search(r'<s xs.n="localName">([^<]*)</s>', blk)
        pos = re.search(r'xs.n="positions" count="\d+">([^<]*)<', blk)
        idx = re.search(r'xs.n="indices" count="\d+">([^<]*)<', blk)
        if not (name and pos and idx):
            continue
        v = [float(t) for t in pos.group(1).split()]
        out[name.group(1)] = ([(v[i], v[i + 1]) for i in range(0, len(v), 2)], [int(t) for t in idx.group(1).split()])
    return out


def seg_dist(p, a, b):
    ax, ay, bx, by = a[0], a[1], b[0], b[1]
    dx, dy = bx - ax, by - ay
    l2 = dx * dx + dy * dy or 1e-9
    t = max(0.0, min(1.0, ((p[0] - ax) * dx + (p[1] - ay) * dy) / l2))
    return math.hypot(p[0] - ax - t * dx, p[1] - ay - t * dy)


def poly_dist(p, line):
    return min(seg_dist(p, line[i], line[i + 1]) for i in range(len(line) - 1))


def measure(points, tris, centre, thick):
    edges = {}
    for i in range(0, len(tris) - 2, 3):
        a, b, c = tris[i:i + 3]
        for e in ((a, b), (b, c), (c, a)):
            k = (min(e), max(e))
            edges[k] = edges.get(k, 0) + 1
    border = [k for k, n in edges.items() if n == 1]
    ring = sorted({i for e in border for i in e})
    perim = sum(math.dist(points[a], points[b]) for a, b in border)
    margins = [poly_dist(points[i], centre) - thick / 2 for i in ring]
    inner = [i for i in range(len(points)) if i not in set(ring)]
    skel = [i for i in inner if poly_dist(points[i], centre) <= max(thick / 2, 1.5)]
    return {
        "total": len(points), "ring": len(ring), "perimeter": round(perim, 1),
        "spacing": round(perim / max(len(ring), 1), 1),
        "margin": round(sum(margins) / max(len(margins), 1), 1),
        "skeleton": len(skel), "other_inner": len(inner) - len(skel),
    }


def main():
    args = [a for a in sys.argv[1:] if not a.startswith("--")]
    out_json = sys.argv[sys.argv.index("--json") + 1] if "--json" in sys.argv else None
    if out_json in args:
        args.remove(out_json)
    desc = json.load(open(args[0]))
    dumps = [meshes(p) for p in args[1:]]
    rows = []
    hdr = f"{'layer':10} {'len':>4} {'t':>3} |" + "|".join(f" {'tot':>3} {'ring':>4} {'spc':>5} {'mrg':>5} {'skl':>3} {'in':>3} " for _ in dumps)
    print(hdr)
    for d in desc:
        row = {"layer": d["layer"], "kind": d["kind"], "length": d["length"], "thickness": d["thickness"]}
        cells = []
        for k, m in enumerate(dumps):
            if d["layer"] not in m:
                cells.append(" (missing) ")
                continue
            r = measure(*m[d["layer"]], d["centre"], d["thickness"])
            row[f"m{k}"] = r
            cells.append(f" {r['total']:>3} {r['ring']:>4} {r['spacing']:>5} {r['margin']:>5} {r['skeleton']:>3} {r['other_inner']:>3} ")
        rows.append(row)
        print(f"{d['layer']:10} {d['length']:>4} {d['thickness']:>3} |" + "|".join(cells))
    if out_json:
        json.dump(rows, open(out_json, "w"), indent=1)


if __name__ == "__main__":
    main()
