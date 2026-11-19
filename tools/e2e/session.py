#!/usr/bin/env python3
"""Scripted rigging session: rigs a character from a PSD by driving Keyform
with real mouse and keyboard input (tools/e2e/harness.py).

usage: session.py PSD OUTDIR [--keyform BIN] [--display :N] [--until N] [--lang ja]

Outputs in OUTDIR: model.cmo3, model.can3, runtime/ (moc3, model3, physics3,
motion3), step PNGs, session.gif, report.json (steps, timings, workflow
notes)."""
import argparse
import json
import os
import sys
import time

sys.path.insert(0, os.path.dirname(os.path.abspath(__file__)))
from harness import Harness  # noqa: E402


def dump(h, label):
    m = h.settle(4)
    print(f"--- map after {label}: frame {m['frame']}")
    for n in m["nodes"]:
        t = n["label"] or n["value"]
        if t and n["role"] not in ("TextRun",):
            print(f"  {n['role']:12} {t!r:60} {[round(v) for v in n['rect']]}{' (disabled)' if n.get('disabled') else ''}")
    c = m.get("canvas")
    if c:
        print("  canvas meshes:", [(x["id"], [round(v) for v in x["bbox"]]) for x in c["meshes"]])
    h.grab().save(os.path.join(h.workdir, "dump.png"))


def main():
    ap = argparse.ArgumentParser()
    ap.add_argument("psd")
    ap.add_argument("out")
    ap.add_argument("--keyform", default="target/release/keyform")
    # Never the user's own display ($DISPLAY): the private test display only.
    ap.add_argument("--display", default=os.environ.get("KEYFORM_E2E_DISPLAY", ":7"))
    ap.add_argument("--until", type=int, default=999)
    ap.add_argument("--lang")
    a = ap.parse_args()
    if a.display in (":0", ":1", os.environ.get("DISPLAY")) and not os.environ.get("KEYFORM_E2E_DISPLAY"):
        sys.exit(f"refusing to drive display {a.display}: start tools/e2e/display.sh and pass --display")
    out = os.path.abspath(a.out)
    os.makedirs(out, exist_ok=True)
    psd = os.path.abspath(a.psd)
    h = Harness(out, a.keyform, a.display, lang=a.lang)
    t0 = time.time()
    steps = STEPS
    try:
        for i, (name, fn) in enumerate(steps, 1):
            if i > a.until:
                break
            try:
                fn(h, psd, out)
            except Exception:
                dump(h, f"FAILED step {i} ({name})")
                raise
            h.step(name)
        dump(h, f"step {min(a.until, len(steps))}")
    finally:
        report = {
            "steps": [s[0] for s in h.steps],
            "seconds": round(time.time() - t0, 1),
            "notes": h.issues,
        }
        with open(os.path.join(out, "report.json"), "w") as f:
            json.dump(report, f, indent=2)
        h.save_gif(os.path.join(out, "session.gif"))
        h.close()


# ---- steps ----------------------------------------------------------------

def open_psd(h, psd, out):
    h.answer_dialog(psd)
    h.click_label("Open…")
    h.find("< Create new model from PSD file >", timeout=20)


def new_model_from_psd(h, psd, out):
    h.click_label("< Create new model from PSD file >")
    time.sleep(2)


def select_all_meshes(h):
    """Rubber band over the whole canvas from empty space."""
    c = h.canvas()
    r = c["rect"]
    xs = [m["bbox"][0] for m in c["meshes"]] + [m["bbox"][2] for m in c["meshes"]]
    ys = [m["bbox"][1] for m in c["meshes"]] + [m["bbox"][3] for m in c["meshes"]]
    a = (max(r[0] + 4, min(xs) - 20), max(r[1] + 4, min(ys) - 20))
    b = (min(r[2] - 4, max(xs) + 20), min(r[3] - 4, max(ys) + 20))
    h.drag(a, b)


def inside(p, b, pad=2):
    return b[0] - pad <= p[0] <= b[2] + pad and b[1] - pad <= p[1] <= b[3] + pad


def exposed_point(h, mesh_id):
    """A point of the mesh that no mesh drawn above it covers (canvas list
    order is top first), so a click there hits this mesh."""
    meshes = h.canvas()["meshes"]
    ids = [m["id"] for m in meshes]
    k = ids.index(mesh_id)
    target = meshes[k]
    above = [m["bbox"] for m in meshes[:k]]
    pts = target["points"]
    cx = sum(p[0] for p in pts) / len(pts)
    cy = sum(p[1] for p in pts) / len(pts)
    cands = [(cx, cy)] + [((p[0] + cx) / 2, (p[1] + cy) / 2) for p in pts] + [tuple(p) for p in pts]
    for c in cands:
        if inside(c, target["bbox"], -2) and not any(inside(c, b) for b in above):
            return c
    raise RuntimeError(f"no exposed point on {mesh_id}")


def palette_row(h, label):
    """The Parts palette row of an object (the topmost match: the Deformer
    palette below lists objects too)."""
    rows = [n for n in h.nodes(label, role="Button") if not n.get("disabled")]
    if not rows:
        raise RuntimeError(f"no palette row {label!r}")
    return min(rows, key=lambda n: n["rect"][1])


def select_rows(h, ids):
    """Click the first row, Ctrl+click the others (Parts palette)."""
    for k, i in enumerate(ids):
        row = palette_row(h, i)
        if k == 0:
            h.click(h.center(row))
        else:
            h.key_down("Control_L")
            h.click(h.center(row))
            h.key_up("Control_L")


HEAD = ["Face", "EyeWhiteL", "EyeWhiteR", "IrisL", "IrisR", "EyelidL", "EyelidR",
        "BrowL", "BrowR", "Mouth", "FrontHair", "BackHair"]
FACE = ["Face", "EyeWhiteL", "EyeWhiteR", "IrisL", "IrisR", "EyelidL", "EyelidR", "BrowL", "BrowR", "Mouth"]
HAIR = ["FrontHair", "BackHair"]


def name_and_create(h, title, name):
    field = h.in_window(title, role="TextInput")[0]
    h.set_field(field, name)
    h.click(h.center(h.in_window(title, label="Create")[0]))
    time.sleep(0.5)
    if h.exists(title, role="Window"):
        h.click(h.center(h.in_window(title, label="Close")[0]))


def deformer_row(h, name):
    """A Deformer palette row (the lowest match: the Parts palette is above)."""
    rows = [n for n in h.nodes(name, role="Button") if not n.get("disabled")]
    if not rows:
        raise RuntimeError(f"no deformer row {name!r}")
    return max(rows, key=lambda n: n["rect"][1])


def head_rotation(h, psd, out):
    # Workflow fix found here: the Parts palette had no multi-select
    # (Ctrl/Shift+click) and highlighted only the primary object; eye
    # whites covered by iris and lid can't be Shift+clicked on the canvas.
    select_rows(h, HEAD)
    h.menu("Modeling", "Deformer", "Create rotation deformer…")
    name_and_create(h, "Create Rotation Deformer", "Head")
    deformer_row(h, "Head")


def face_warp(h, psd, out):
    select_rows(h, FACE)
    h.menu("Modeling", "Deformer", "Create Warp Deformer…")
    name_and_create(h, "Create Warp Deformer", "FaceWarp")
    deformer_row(h, "FaceWarp")


def hair_warp(h, psd, out):
    select_rows(h, HAIR)
    h.menu("Modeling", "Deformer", "Create Warp Deformer…")
    name_and_create(h, "Create Warp Deformer", "HairWarp")
    deformer_row(h, "HairWarp")


def param_label(h, name):
    """The Parameter palette label of a parameter, scrolled into view."""
    for _ in range(40):
        ls = [n for n in h.nodes(name, role="Label")]
        c = h.canvas() or {}
        pal = [n for n in h.nodes("Parameter", role="Button")] + [n for n in h.nodes("Parameter", role="Label")]
        vis = [n for n in ls if 0 < n["rect"][1] < 990]
        if vis:
            return vis[0]
        # Scroll the Parameter palette down (mouse wheel over it).
        anchor = h.find("Reset", role="Button")
        p = (anchor["rect"][0] - 200, anchor["rect"][3] + 200)
        h.move(p)
        h.button(5, True)
        h.button(5, False)
        h.settle(2)
    raise RuntimeError(f"parameter {name!r} not visible")


def param_slider(h, name):
    lab = param_label(h, name)
    sl = [n for n in h.nodes(role="Slider") if n["rect"][1] >= lab["rect"][1] and abs(n["rect"][0] - lab["rect"][0]) < 40]
    sl.sort(key=lambda n: n["rect"][1])
    if not sl:
        raise RuntimeError(f"no slider for {name!r}")
    return sl[0]


def param_value(h, name):
    lab = param_label(h, name)
    y = (lab["rect"][1] + lab["rect"][3]) / 2
    for n in h.nodes(role="Label"):
        t = n["label"] or n["value"]
        if n is not lab and abs((n["rect"][1] + n["rect"][3]) / 2 - y) < 4 and n["rect"][0] > lab["rect"][2]:
            try:
                return float(t)
            except ValueError:
                pass
    raise RuntimeError(f"no value shown for {name!r}")


def set_param(h, name, where, lo=None, hi=None):
    """Drags the slider handle past the slider's end ('min' / 'max'); the
    slider clamps, so the value lands exactly on the end key. 'mid' uses the
    palette's Reset (all parameters to their defaults)."""
    if where == "mid":
        reset_params(h)
        return
    r = param_slider(h, name)["rect"]
    inset = (r[3] - r[1]) / 2
    v = param_value(h, name)
    lo = -30.0 if lo is None else lo
    hi = 30.0 if hi is None else hi
    t = 0.0 if hi == lo else min(max((v - lo) / (hi - lo), 0.0), 1.0)
    y = (r[1] + r[3]) / 2
    x = r[0] + inset + t * (r[2] - r[0] - 2 * inset)
    end = r[0] - 40 if where == "min" else r[2] + 40
    h.drag((x, y), (end, y), steps=12)


def inspector_bounds(h):
    """(x0, top, bottom) of the Inspector's visible area: from its Name field
    down to the Parameter palette's filter box (the next panel)."""
    tis = [n for n in h.nodes(role="TextInput")]
    name = min((n for n in tis if 480 < n["rect"][0] < 560), key=lambda n: n["rect"][1])
    below = [n for n in tis if n["rect"][1] > name["rect"][1] + 60 and abs(n["rect"][0] - 455) < 30]
    bottom = min(n["rect"][1] for n in below) - 30 if below else 500
    return name["rect"][0] - 50, name["rect"][1] - 4, bottom


def inspector_field(h, pred):
    """A field of the Inspector, scrolled into the panel's visible area."""
    for _ in range(20):
        x0, top, bottom = inspector_bounds(h)
        fs = [n for n in h.nodes() if pred(n) and n["rect"][0] >= x0]
        if fs:
            f = fs[0]
            cy = (f["rect"][1] + f["rect"][3]) / 2
            if top < cy < bottom - 4:
                return f
        h.move((x0 + 200, (top + bottom) / 2))
        h.button(5, True)
        h.button(5, False)
        h.settle(2)
    raise RuntimeError("Inspector field not reachable")


def key_op(h, name, op):
    """Right-click the parameter's name → key operation."""
    lab = param_label(h, name)
    h.click(h.center(lab), button=3)
    h.click_label(op, role="Button")


def reset_params(h):
    h.click_label("Reset", role="Button")


def selected_name(h):
    tis = [n for n in h.nodes(role="TextInput") if 480 < n["rect"][0] < 560]
    return min(tis, key=lambda n: n["rect"][1])["value"] if tis else None


def _select(h, row_fn, name):
    for attempt in range(3):
        h.click(h.center(row_fn(h, name)))
        if selected_name(h) == name:
            return
        if attempt == 0:
            h.note(f"selecting {name!r} in a palette needed a second click")
    raise RuntimeError(f"could not select {name!r}")


def select_deformer(h, name):
    _select(h, deformer_row, name)


def select_mesh(h, name):
    _select(h, palette_row, name)


def canvas_drag_object(h, mesh_id, dx, dy):
    """Drags the selected object on the canvas from an exposed point of one
    of its meshes (moves it in the current keyform)."""
    p = exposed_point(h, mesh_id)
    h.drag(p, (p[0] + dx, p[1] + dy), steps=15)


def rubber_band_points(h, mesh_id, region):
    """With the mesh selected, rubber-band its points in a fraction of its
    bbox: region = (fx0, fy0, fx1, fy1)."""
    b = h.mesh(mesh_id)["bbox"]
    w, ht = b[2] - b[0], b[3] - b[1]
    a = (b[0] + w * region[0] - 6, b[1] + ht * region[1] - 6)
    z = (b[0] + w * region[2] + 6, b[1] + ht * region[3] + 6)
    h.drag(a, z, steps=10)


def drag_selected_points(h, mesh_id, region, dx, dy):
    """Drags one of the selected points (inside region)."""
    m = h.mesh(mesh_id)
    b = m["bbox"]
    w, ht = b[2] - b[0], b[3] - b[1]
    x0, y0, x1, y1 = b[0] + w * region[0] - 6, b[1] + ht * region[1] - 6, b[0] + w * region[2] + 6, b[1] + ht * region[3] + 6
    pts = [p for p in m["points"] if x0 <= p[0] <= x1 and y0 <= p[1] <= y1]
    if not pts:
        raise RuntimeError(f"no points of {mesh_id} in {region}")
    p = pts[0]
    h.drag(p, (p[0] + dx, p[1] + dy), steps=15)


def keys_head_angle_z(h, psd, out):
    select_deformer(h, "Head")
    key_op(h, "Angle Z", "Add 3 keys")
    for where, angle in (("max", "-12"), ("min", "12")):
        set_param(h, "Angle Z", where)
        field = inspector_field(h, lambda n: n["role"] == "SpinButton" and n["value"].endswith("°"))
        h.set_field(field, angle)
    reset_params(h)


def keys_face_angles(h, psd, out):
    select_deformer(h, "FaceWarp")
    for param, (dmin, dmax) in (("Angle X", ((-18, 0), (18, 0))), ("Angle Y", ((0, 14), (0, -14)))):
        key_op(h, param, "Add 3 keys")
        for where, d in (("min", dmin), ("max", dmax)):
            set_param(h, param, where)
            canvas_drag_object(h, "Face", *d)
        reset_params(h)
    select_deformer(h, "HairWarp")
    key_op(h, "Angle X", "Add 3 keys")
    for where, d in (("min", (-10, 0)), ("max", (10, 0))):
        set_param(h, "Angle X", where)
        canvas_drag_object(h, "BackHair", *d)
    reset_params(h)


def keys_eyes(h, psd, out):
    for mesh, param in (("EyelidL", "EyeL Open"), ("EyelidR", "EyeR Open")):
        select_mesh(h, mesh)
        key_op(h, param, "Add 2 keys")
        set_param(h, param, "min", 0.0, 1.0)
        # Workflow fix found here: Edit ▸ Select All (Ctrl+A) selects the
        # points of the selected object; Escape now deselects as the menu said.
        h.key("ctrl+a")
        drag_selected_points(h, mesh, (0.0, 0.0, 1.0, 1.0), 0, 22)
        reset_params(h)


def keys_mouth(h, psd, out):
    select_mesh(h, "Mouth")
    key_op(h, "Mouth Open", "Add 2 keys")
    set_param(h, "Mouth Open", "max", 0.0, 1.0)
    # The lower lip's vertices: click one, Shift+click the others (a rubber
    # band would start on the face mesh under the mouth and grab the face).
    m = h.mesh("Mouth")
    b = m["bbox"]
    low = [p for p in m["points"] if p[1] > b[1] + 0.55 * (b[3] - b[1])]
    if not low:
        raise RuntimeError("no lower mouth vertices")
    h.click(tuple(low[0]))
    for p in low[1:]:
        h.key_down("Shift_L")
        h.click(tuple(p))
        h.key_up("Shift_L")
    h.drag(tuple(low[0]), (low[0][0], low[0][1] + 18), steps=15)
    reset_params(h)


def auto_mesh(h, psd, out):
    select_all_meshes(h)
    h.menu("Modeling", "ArtMesh", "Automatic Mesh generator…")
    h.find("Automatic Mesh Generator", role="Window")
    h.click_label("OK")
    time.sleep(1)
    tri = sum(len(m["points"]) for m in h.canvas()["meshes"])
    if tri <= 4 * 14:
        raise RuntimeError(f"auto mesh made no new vertices ({tri})")


STEPS = [
    ("File ▸ Open the PSD", open_psd),
    ("Create a new model from the PSD", new_model_from_psd),
    ("Select all meshes; Automatic Mesh generator", auto_mesh),
    ("Rotation deformer for the head", head_rotation),
    ("Warp deformer for the face", face_warp),
    ("Warp deformer for the hair", hair_warp),
    ("Head rotation keys on Angle Z", keys_head_angle_z),
    ("Face and hair keys on Angle X / Y", keys_face_angles),
    ("Eyelid keys on EyeL / EyeR Open", keys_eyes),
    ("Mouth Open keys (points dragged)", keys_mouth),
]

if __name__ == "__main__":
    main()
