#!/usr/bin/env python3
"""Checks the outputs of tools/e2e/session.py.

usage: check.py OUTDIR [--keyform BIN] [--kf BIN] [--golden DIR] [--update]
                [--params ID,ID,...] [--physics IN:OUT,...] [--curves ID,...]

- every saved / exported file opens (keyform --check-open);
- the .cmo3 and .can3 re-save byte-identically (kf roundtrip);
- every rigged parameter visibly changes the render at its minimum and its
  maximum (kf render of the exported runtime model);
- those renders match golden snapshots (the session is deterministic, so
  almost exactly); --update writes them;
- physics moves each OUT when IN swings (kf physics);
- the exported motion has the expected curves.
Writes OUTDIR/check.json and exits 1 on any failure."""
import argparse
import json
import os
import subprocess
import sys

from PIL import Image, ImageChops

DEFAULT_PARAMS = ("ParamAngleX,ParamAngleY,ParamAngleZ,ParamEyeLOpen,ParamEyeROpen,ParamEyeLSmile,ParamEyeRSmile,"
                  "ParamEyeBallX,ParamEyeBallY,ParamBrowLY,ParamBrowRY,ParamBrowLAngle,ParamBrowRAngle,"
                  "ParamMouthForm,ParamMouthOpenY,ParamBodyAngleX,ParamBodyAngleY,ParamBodyAngleZ,ParamBreath,"
                  "ParamHairFront,ParamHairSide,ParamHairBack,ParamArmLA,ParamArmRA")
DEFAULT_PHYSICS = "ParamAngleX:ParamHairFront,ParamAngleX:ParamHairSide,ParamAngleX:ParamHairBack"
HEADP = ["Face", "FrontHair", "SideHairL", "SideHairR", "BackHair"]
# The layers each parameter should move (the session's rig).
PARTS = {
    "ParamAngleX": HEADP, "ParamAngleY": HEADP, "ParamAngleZ": HEADP,
    "ParamEyeLOpen": ["EyelidL"], "ParamEyeROpen": ["EyelidR"],
    "ParamEyeLSmile": ["EyelidL"], "ParamEyeRSmile": ["EyelidR"],
    "ParamEyeBallX": ["IrisL", "IrisR"], "ParamEyeBallY": ["IrisL", "IrisR"],
    "ParamBrowLY": ["BrowL"], "ParamBrowLAngle": ["BrowL"], "ParamBrowRY": ["BrowR"], "ParamBrowRAngle": ["BrowR"],
    "ParamMouthForm": ["Mouth", "MouthInner"], "ParamMouthOpenY": ["Mouth", "MouthInner"],
    "ParamBodyAngleX": ["Body", "Neck", "ArmL", "ArmR"], "ParamBodyAngleY": ["Body", "Neck", "ArmL", "ArmR"],
    "ParamBodyAngleZ": ["Body", "Neck", "ArmL", "ArmR"], "ParamBreath": ["Body", "Neck", "ArmL", "ArmR"],
    "ParamHairFront": ["FrontHair"], "ParamHairSide": ["SideHairL", "SideHairR"], "ParamHairBack": ["BackHair"],
    "ParamArmLA": ["ArmL"], "ParamArmRA": ["ArmR"],
}
DEFAULT_CURVES = "ParamAngleX,ParamEyeLOpen,ParamEyeROpen,ParamMouthOpenY"


def run(cmd):
    p = subprocess.run(cmd, capture_output=True, text=True)
    return p.returncode, (p.stdout + p.stderr).strip()


def ranges(model3, kf):
    """(min, default, max) per parameter id, from `kf moc3info`."""
    code, out = run([kf, "moc3info", model3])
    r = {}
    for line in out.splitlines():
        parts = line.split()
        # "  ParamAngleX  -30 … 30 (default 0)" style lines
        if len(parts) >= 2 and parts[0].startswith("Param"):
            try:
                nums = [float(x.strip("(),")) for x in parts[1:] if x.strip("(),").replace(".", "", 1).replace("-", "", 1).isdigit()]
                if len(nums) >= 3:
                    r[parts[0]] = (nums[0], nums[2], nums[1])
            except ValueError:
                pass
    return r


def parts_from_profile(r):
    """Rebuilds PARTS from a session.py rig profile (roles → layers)."""
    g = lambda k, d: r.get(k, d)
    head = [g("face", "Face")] + g("hair_front", []) + g("hair_side", []) + g("hair_back", [])
    lids, irises, brows = g("eyelids", ["EyelidL", "EyelidR"]), g("irises", ["IrisL", "IrisR"]), g("brows", ["BrowL", "BrowR"])
    mouth = [g("mouth", "Mouth")] + g("mouth_inner", [])
    body = g("body_group", ["Body"])
    arms = g("arms", ["ArmL", "ArmR"])
    PARTS.update({
        "ParamAngleX": head, "ParamAngleY": head, "ParamAngleZ": head,
        "ParamEyeLOpen": lids[:1], "ParamEyeROpen": lids[1:], "ParamEyeLSmile": lids[:1], "ParamEyeRSmile": lids[1:],
        "ParamEyeBallX": irises, "ParamEyeBallY": irises,
        "ParamBrowLY": brows[:1], "ParamBrowLAngle": brows[:1], "ParamBrowRY": brows[1:], "ParamBrowRAngle": brows[1:],
        "ParamMouthForm": mouth, "ParamMouthOpenY": mouth,
        "ParamBodyAngleX": body, "ParamBodyAngleY": body, "ParamBodyAngleZ": body, "ParamBreath": body,
        "ParamHairFront": g("hair_front", []), "ParamHairSide": g("hair_side", []), "ParamHairBack": g("hair_back", []),
        "ParamArmLA": arms[:1], "ParamArmRA": arms[1:],
    })


def layer_boxes(psd):
    try:
        im = Image.open(psd)
        return {name: box for name, _mode, box in (l[:3] for l in im.layers)}, im.size
    except Exception:  # noqa: BLE001
        return {}, None


def changed_inside(a, b, boxes, canvas, margin=40, threshold=12):
    """Fraction of the pixels that changed between renders a and b lying in
    the union of `boxes` (canvas px, grown by `margin`)."""
    a, b = Image.open(a).convert("RGBA"), Image.open(b).convert("RGBA")
    k = canvas[1] / a.height
    d = ImageChops.difference(a, b)
    px = list(d.get_flattened_data()) if hasattr(d, "get_flattened_data") else list(d.getdata())
    total = inside = 0
    for i, p in enumerate(px):
        if max(p) <= threshold:
            continue
        total += 1
        x, y = (i % a.width) * k, (i // a.width) * k
        if any(bx[0] - margin <= x <= bx[2] + margin and bx[1] - margin <= y <= bx[3] + margin for bx in boxes):
            inside += 1
    return (inside / total if total else 1.0), total


def diff_stats(a, b, threshold=3):
    d = ImageChops.difference(a.convert("RGBA"), b.convert("RGBA"))
    px = list(d.get_flattened_data()) if hasattr(d, 'get_flattened_data') else list(d.getdata())
    total = sum(sum(p) for p in px)
    bad = sum(1 for p in px if max(p) > threshold)
    return total / (len(px) * 4), bad


def main():
    ap = argparse.ArgumentParser()
    ap.add_argument("out")
    ap.add_argument("--keyform", default="target/release/keyform")
    ap.add_argument("--kf", default="target/release/kf")
    ap.add_argument("--golden")
    ap.add_argument("--update", action="store_true")
    ap.add_argument("--params", default=DEFAULT_PARAMS)
    ap.add_argument("--physics", default=DEFAULT_PHYSICS)
    ap.add_argument("--curves", default=DEFAULT_CURVES)
    ap.add_argument("--size", type=int, default=400, help="render height (raise it for large canvases)")
    ap.add_argument("--profile", help="session.py rig profile (JSON) for the right-parts map")
    ap.add_argument("--psd", default="test-data/e2e/character.psd", help="for the layer bounds of the right-parts check")
    ap.add_argument("--ranges", default="", help="ID=min:max,... for parameters not in the model's standard set")
    a = ap.parse_args()
    out = os.path.abspath(a.out)
    rt = os.path.join(out, "runtime")
    model3 = next(os.path.join(rt, f) for f in os.listdir(rt) if f.endswith(".model3.json"))
    physics3 = next(os.path.join(rt, f) for f in os.listdir(rt) if f.endswith(".physics3.json"))
    results = []

    def record(name, ok, detail=""):
        results.append({"check": name, "ok": ok, "detail": detail})
        print(f"{'ok  ' if ok else 'FAIL'} {name}{': ' + detail if detail else ''}", flush=True)

    files = [os.path.join(out, "model.cmo3"), os.path.join(out, "model.can3"), model3]
    files += [os.path.join(rt, f) for f in os.listdir(rt) if f.endswith(".moc3")]
    for f in files:
        code, msg = run([a.keyform, "--check-open", f])
        record(f"opens {os.path.basename(f)}", code == 0, msg.splitlines()[-1] if msg else "")
    for f in ("model.cmo3", "model.can3"):
        src = os.path.join(out, f)
        dst = os.path.join(out, "resaved-" + f)
        code, msg = run([a.kf, "roundtrip", src, dst])
        same = code == 0 and open(src, "rb").read() == open(dst, "rb").read()
        record(f"{f} re-saves unchanged", same, "" if same else msg)

    # Parameter ranges: standard parameters of new models, plus overrides.
    std = {"ParamAngleX": (-30, 30), "ParamAngleY": (-30, 30), "ParamAngleZ": (-30, 30),
           "ParamEyeLOpen": (0, 1), "ParamEyeROpen": (0, 1), "ParamMouthOpenY": (0, 1),
           "ParamBodyAngleX": (-10, 10), "ParamBodyAngleY": (-10, 10), "ParamBodyAngleZ": (-10, 10),
           "ParamHairSway": (-1, 1), "ParamEyeLSmile": (0, 1), "ParamEyeRSmile": (0, 1),
           "ParamEyeBallX": (-1, 1), "ParamEyeBallY": (-1, 1), "ParamBrowLY": (-1, 1), "ParamBrowRY": (-1, 1),
           "ParamBrowLAngle": (-1, 1), "ParamBrowRAngle": (-1, 1), "ParamMouthForm": (-1, 1), "ParamBreath": (0, 1),
           "ParamHairFront": (-1, 1), "ParamHairSide": (-1, 1), "ParamHairBack": (-1, 1),
           "ParamArmLA": (-10, 10), "ParamArmRA": (-10, 10)}
    for item in filter(None, a.ranges.split(",")):
        k, v = item.split("=")
        lo, hi = v.split(":")
        std[k] = (float(lo), float(hi))
    snap = os.path.join(out, "snapshots")
    os.makedirs(snap, exist_ok=True)
    base = os.path.join(snap, "default.png")
    code, msg = run([a.kf, "render", model3, base, "--size", str(a.size)])
    record("render at defaults", code == 0, msg)
    images = {"default": base}
    boxes, canvas = layer_boxes(a.psd)
    if a.profile:
        parts_from_profile(json.load(open(a.profile)))
    defaults = {"ParamEyeLOpen": 1.0, "ParamEyeROpen": 1.0, "ParamEyeLSmile": 0.0}
    for pid in filter(None, a.params.split(",")):
        lo, hi = std.get(pid, (-30, 30))
        dflt = defaults.get(pid, 0.0 if lo < 0 < hi or lo == 0 else lo)
        paths = {}
        for tag, v in (("min", lo), ("max", hi)):
            path = os.path.join(snap, f"{pid}_{tag}.png")
            code, msg = run([a.kf, "render", model3, path, "--size", str(a.size), f"{pid}={v}"])
            if code != 0:
                record(f"{pid} {tag} renders", False, msg)
                continue
            paths[tag] = path
            images[f"{pid}_{tag}"] = path
        # Each end that isn't the default must differ from the default; an
        # end that is the default is compared with the other end instead.
        for tag, v in (("min", lo), ("max", hi)):
            if tag not in paths:
                continue
            other = paths.get("max" if tag == "min" else "min") if abs(v - dflt) < 1e-6 else base
            if other is None:
                continue
            mean, bad = diff_stats(Image.open(other), Image.open(paths[tag]))
            ref = "the other end" if other != base else "the default"
            record(f"{pid} at {tag} ({v}) moves the model", bad > 20, f"{bad} pixels differ from {ref}")
            want = [boxes[n] for n in PARTS.get(pid, []) if n in boxes]
            if want and bad > 20:
                frac, n = changed_inside(other, paths[tag], want, canvas, margin=0.045 * canvas[1])
                record(f"{pid} at {tag} moves the right parts ({', '.join(PARTS[pid])})", frac >= 0.8,
                       f"{frac:.0%} of {n} changed pixels on those layers")
    if a.golden:
        os.makedirs(a.golden, exist_ok=True)
        for name, path in images.items():
            g = os.path.join(a.golden, f"{name}.png")
            if a.update or not os.path.exists(g):
                Image.open(path).save(g)
                record(f"golden {name} written", True)
                continue
            # The session is deterministic (same input, same moc3), so the
            # renders must match almost exactly.
            mean, bad = diff_stats(Image.open(g), Image.open(path))
            record(f"golden {name}", mean < 0.05 and bad < 20, f"mean {mean:.3f}, {bad} pixels > 3")
    for pair in filter(None, a.physics.split(",")):
        i, o = pair.split(":")
        code, msg = run([a.kf, "physics", model3, physics3, i, o])
        record(f"physics: {o} swings with {i}", code == 0, msg.splitlines()[-1] if msg else "")
    motions = [f for f in os.listdir(rt) if f.endswith(".motion3.json")]
    if motions:
        m = json.load(open(os.path.join(rt, motions[0])))
        ids = {c["Id"] for c in m["Curves"]}
        for cid in filter(None, a.curves.split(",")):
            record(f"motion has {cid}", cid in ids)
    else:
        record("motion3.json exported", False)
    with open(os.path.join(out, "check.json"), "w") as f:
        json.dump(results, f, indent=2)
    failed = [r for r in results if not r["ok"]]
    print(f"{len(results) - len(failed)}/{len(results)} checks passed")
    sys.exit(1 if failed else 0)


if __name__ == "__main__":
    main()
