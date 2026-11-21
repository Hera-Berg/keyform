#!/usr/bin/env python3
"""Scripted rigging session: rigs a character from a PSD by driving Keyform
with real mouse and keyboard input (tools/e2e/harness.py).

usage: session.py PSD OUTDIR [--keyform BIN] [--display :N] [--until N] [--lang ja]

Steps: see STEPS at the end (31: deformers, keys for every standard parameter,
clipping masks, arm parameters, three hair physics groups, atlas, save,
export, scene keys, motion export, model3 motion entry).

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
    ap.add_argument("--profile", help="JSON rig profile (layer roles) for another PSD")
    ap.add_argument("--lang")
    # Debugging only (not an acceptance run): a failed step saves
    # OUT/checkpoint.cmo3 through File ▸ Save as…; --resume opens such a file
    # through File ▸ Open… and continues at step --from.
    ap.add_argument("--resume")
    ap.add_argument("--from", dest="start", type=int, default=1)
    a = ap.parse_args()
    if a.profile:
        R.update(json.load(open(a.profile)))
    allowed = os.environ.get("KEYFORM_E2E_DISPLAY", ":7")
    if a.display != allowed:
        sys.exit(f"refusing to drive display {a.display}: only the private test display {allowed}")
    out = os.path.abspath(a.out)
    os.makedirs(out, exist_ok=True)
    psd = os.path.abspath(a.psd)
    h = Harness(out, a.keyform, a.display, lang=a.lang)
    t0 = time.time()
    steps = STEPS
    try:
        if a.resume:
            h.answer_dialog(os.path.abspath(a.resume))
            h.menu("File", "Open…")
            time.sleep(2.5)
            h.note(f"DEBUG RUN: resumed from {a.resume} at step {a.start}")
        for i, (name, fn) in enumerate(steps, 1):
            if i > a.until:
                break
            if i < a.start:
                continue
            try:
                fn(h, psd, out)
            except Exception:
                dump(h, f"FAILED step {i} ({name})")
                checkpoint(h, out, i)
                raise
            h.step(name)
        dump(h, f"step {min(a.until, len(steps))}")
    finally:
        times = [t0] + [st[1] for st in h.steps]
        report = {
            "steps": [{"step": st[0], "seconds": round(times[i + 1] - times[i], 1)} for i, st in enumerate(h.steps)],
            "seconds": round(time.time() - t0, 1),
            "notes": h.issues,
        }
        with open(os.path.join(out, "report.json"), "w") as f:
            json.dump(report, f, indent=2)
        h.save_gif(os.path.join(out, "session.gif"))
        h.close()


def checkpoint(h, out, i):
    """Saves the model as it is through File ▸ Save as… (debugging aid)."""
    try:
        h.key("Escape")
        for _ in range(4):
            close = [n for n in h.nodes("Close window", role="Button") if h.reachable(n)]
            if not close:
                break
            h.click(h.center(close[0]))
        path = os.path.join(out, "checkpoint.cmo3")
        h.answer_dialog(path)
        h.menu("File", "Save as…")
        time.sleep(1.5)
        print(f"[session] checkpoint before step {i}: {path} (resume with --resume {path} --from {i})")
    except Exception as e:  # noqa: BLE001
        print(f"[session] no checkpoint: {e}")


# ---- steps ----------------------------------------------------------------

def accept_resolution(h):
    """Large PSDs ask for the model image's display resolution: keep the
    recommended one (OK)."""
    if any(h.reachable(n) for n in h.nodes("Please select the resolution for displaying the model image")):
        h.note("large PSD: accepted the recommended display resolution")
        h.click(h.center(h.in_window("Confirm", label="OK")[0]))
        return True
    return False


def open_psd(h, psd, out):
    h.answer_dialog(psd)
    h.click_label("Open…")
    target = "< Create new model from PSD file >"
    for _ in range(240):
        if any(h.reachable(n) for n in h.nodes(target)):
            return
        accept_resolution(h)
        time.sleep(0.5)
    raise RuntimeError(f"widget not found: {target!r}")


def new_model_from_psd(h, psd, out):
    label = "< Create new model from PSD file >"
    for attempt in range(3):
        h.click_label(label)
        for _ in range(240):  # large PSDs take a while
            c = h.canvas()
            if c and c.get("meshes"):
                return
            if accept_resolution(h) or not h.exists(label):
                break
            time.sleep(0.25)
        else:
            h.note(f"'{label}' needed another click (the UI was slow to respond: frame {(h.settle(1) or {}).get('frame')})")
            continue
        for _ in range(600):
            c = h.canvas()
            if c and c.get("meshes"):
                return
            accept_resolution(h)
            time.sleep(0.25)
    raise RuntimeError("no model was created from the PSD")


def select_all_meshes(h):
    """Rubber band over the whole canvas from empty space."""
    c = h.canvas()
    r = c["rect"]
    xs = [m["bbox"][0] for m in c["meshes"]] + [m["bbox"][2] for m in c["meshes"]]
    ys = [m["bbox"][1] for m in c["meshes"]] + [m["bbox"][3] for m in c["meshes"]]
    a = (max(r[0] + 4, min(xs) - 20), max(r[1] + 4, min(ys) - 20))
    b = (min(r[2] - 4, max(xs) + 20), min(r[3] - 4, max(ys) + 20))
    h.drag(h.canvas_point(a), b)


def inside(p, b, pad=2):
    return b[0] - pad <= p[0] <= b[2] + pad and b[1] - pad <= p[1] <= b[3] + pad


def exposed_point(h, mesh_id, avoid_vertices=0):
    """A point of the mesh that no mesh drawn above it covers (canvas list
    order is top first), so a click there hits this mesh."""
    meshes = h.canvas()["meshes"]
    k = next(i for i, m in enumerate(meshes) if mesh_id in (m["id"], m.get("name")))
    target = meshes[k]
    above = [m["bbox"] for m in meshes[:k]]
    pts = target["points"]
    cx = sum(p[0] for p in pts) / len(pts)
    cy = sum(p[1] for p in pts) / len(pts)
    cands = [(cx, cy)] + [((p[0] + cx) / 2, (p[1] + cy) / 2) for p in pts] + [tuple(p) for p in pts]
    far = lambda c: all((c[0] - p[0]) ** 2 + (c[1] - p[1]) ** 2 >= avoid_vertices ** 2 for p in pts)
    for c in cands:
        if inside(c, target["bbox"], -2) and not any(inside(c, b) for b in above) and far(c):
            return c
    raise RuntimeError(f"no exposed point on {mesh_id}")


def palette_region(h, which):
    """(top, bottom) of the Parts or the Deformer palette's list area."""
    tabs = {n["label"]: n for n in h.nodes(role="Tab")}
    parts, deformer = tabs.get("Parts"), tabs.get("Deformer")
    if which == "parts":
        return (parts["rect"][3] + 30 if parts else 0, deformer["rect"][1] if deformer else 10000)
    return (deformer["rect"][3] if deformer else 0, 10000)


def scrolled_row(h, label, which):
    """A row of the Parts or Deformer palette, wheeled into view when that
    palette is scrolled (wheel over the palette, toward the row)."""
    for _ in range(40):
        top, bottom = palette_region(h, which)
        rows = [n for n in h.nodes(label, role="Button") if not n.get("disabled") and n["rect"][0] < 450]
        if not rows:
            raise RuntimeError(f"no palette row {label!r}")
        shown = [n for n in rows if h.reachable(n) and top <= n["visible"][1] < bottom]
        if shown:
            return shown[0]
        hidden = [n for n in rows if not n.get("visible")]
        if not hidden:
            raise RuntimeError(f"palette row {label!r} not in the {which} palette")
        n = hidden[0]
        vis = [x for x in h.nodes(role="Button") if h.reachable(x) and x["rect"][0] < 450 and top <= x["visible"][1] < bottom]
        if not vis:
            raise RuntimeError(f"{which} palette not visible")
        near = min(vis, key=lambda x: abs(x["rect"][1] - n["rect"][1]))
        h.move(h.vcenter(near))
        b = 5 if n["rect"][1] > near["rect"][1] else 4
        h.button(b, True)
        h.button(b, False)
        h.settle(2)
    raise RuntimeError(f"palette row {label!r} not reachable")


def palette_row(h, label):
    """The Parts palette row of an object."""
    return scrolled_row(h, label, "parts")


def parts_filter(h, text):
    """Types into the Parts palette's Filter by Text box ("" clears it)."""
    boxes = [n for n in h.nodes("Filter by Text", role="TextInput") if h.reachable(n) and n["rect"][0] < 450]
    if not boxes:
        raise RuntimeError("no Parts palette filter box")
    box = min(boxes, key=lambda n: n["rect"][1])
    h.click(h.center(box))
    h.key("ctrl+a")
    if text:
        h.type(text)
    else:
        h.key("BackSpace")
    h.key("Return")
    h.settle(2)


def click_parts_row(h, label, ctrl=False):
    """Clicks (or Ctrl+clicks) an object's Parts palette row. Rows inside
    collapsed folders aren't drawn: then filter the palette by the name,
    as a person would, click, and clear the filter."""
    try:
        row, filtered = palette_row(h, label), False
    except RuntimeError:
        parts_filter(h, label)
        row, filtered = palette_row(h, label), True
    if ctrl:
        h.key_down("Control_L")
    h.click(h.center(row))
    if ctrl:
        h.key_up("Control_L")
    if filtered:
        parts_filter(h, "")


def select_rows(h, ids):
    """Click the first row, Ctrl+click the others (Parts palette)."""
    for k, i in enumerate(ids):
        click_parts_row(h, i, ctrl=k > 0)


# The rig profile: which layers play which role. The default is the
# committed generated PSD; --profile FILE.json overrides keys (e.g. for a
# local PSD with other layer names).
R = {
    "face": "Face",                      # the head mesh dragged for Angle X / Y
    "face_group": ["Face", "EyeWhiteL", "EyeWhiteR", "IrisL", "IrisR", "EyelidL", "EyelidR",
                   "BrowL", "BrowR", "Mouth", "MouthInner"],
    "eyelids": ["EyelidL", "EyelidR"],
    "irises": ["IrisL", "IrisR"],
    "whites": ["EyeWhiteL", "EyeWhiteR"],
    "brows": ["BrowL", "BrowR"],
    "mouth": "Mouth",                    # lower points open, corners form
    "mouth_inner": ["MouthInner"],       # clipped by mouth_mask
    "mouth_mask": "Mouth",
    "hair_front": ["FrontHair"],
    "hair_side": ["SideHairL", "SideHairR"],
    "hair_back": ["BackHair"],
    "body_group": ["Body", "Neck", "ArmL", "ArmR"],
    "body": "Body",
    "breath_group": ["Body"],            # inside BodyWarp: Body Z and Breath
    "arms": ["ArmL", "ArmR"],
    "delete": [],                        # layers to delete after the import
}


def head_group():
    return R["face_group"] + R["hair_front"] + R["hair_side"] + R["hair_back"]


def hair_warps():
    return [("FrontHairWarp", R["hair_front"]), ("SideHairWarp", R["hair_side"]), ("BackHairWarp", R["hair_back"])]


def name_and_create(h, title, name):
    field = h.in_window(title, role="TextInput")[0]
    h.set_field(field, name)
    h.click(h.center(h.in_window(title, label="Create")[0]))
    time.sleep(0.5)
    if h.exists(title, role="Window"):
        h.click(h.center(h.in_window(title, label="Close")[0]))


def deformer_row(h, name):
    """A Deformer palette row."""
    return scrolled_row(h, name, "deformer")


def head_rotation(h, psd, out):
    # Workflow fix found here: the Parts palette had no multi-select
    # (Ctrl/Shift+click) and highlighted only the primary object; eye
    # whites covered by iris and lid can't be Shift+clicked on the canvas.
    select_rows(h, head_group())
    h.menu("Modeling", "Deformer", "Create rotation deformer…")
    name_and_create(h, "Create Rotation Deformer", "Head")
    deformer_row(h, "Head")


def face_warp(h, psd, out):
    select_rows(h, R["face_group"])
    h.menu("Modeling", "Deformer", "Create Warp Deformer…")
    name_and_create(h, "Create Warp Deformer", "FaceWarp")
    deformer_row(h, "FaceWarp")


def make_hair_warps(h, psd, out):
    for name, meshes in hair_warps():
        select_rows(h, meshes)
        h.menu("Modeling", "Deformer", "Create Warp Deformer…")
        name_and_create(h, "Create Warp Deformer", name)
        deformer_row(h, name)


def body_deformers(h, psd, out):
    """A warp for the body (with neck and arms), then a rotation deformer
    per arm inside it."""
    select_rows(h, R["body_group"])
    h.menu("Modeling", "Deformer", "Create Warp Deformer…")
    name_and_create(h, "Create Warp Deformer", "BodyWarp")
    deformer_row(h, "BodyWarp")
    # Body Z and Breath on a second warp inside, so no object carries more
    # than two parameters (the reference's FREE limit; PRO allows three).
    select_rows(h, R["breath_group"])
    h.menu("Modeling", "Deformer", "Create Warp Deformer…")
    name_and_create(h, "Create Warp Deformer", "BreathWarp")
    deformer_row(h, "BreathWarp")
    for arm, name in zip(R["arms"], ("ArmLRot", "ArmRRot")):
        select_rows(h, [arm])
        h.menu("Modeling", "Deformer", "Create rotation deformer…")
        name_and_create(h, "Create Rotation Deformer", name)
        deformer_row(h, name)


def param_label(h, name):
    """The Parameter palette label of a parameter, scrolled into view
    (wheel up or down over the palette, toward the label)."""
    reset = h.find("Reset", role="Button")
    top, bottom = reset["rect"][3] + 8, 975
    over = (reset["rect"][0] - 200, reset["rect"][3] + 150)
    for _ in range(60):
        ls = [n for n in h.nodes(name, role="Label") if abs(n["rect"][0] - (reset["rect"][0] - 347)) < 60 or n["rect"][0] > 440]
        # The label and its slider (the next slider below it) both in view.
        sliders = h.nodes(role="Slider")

        def slider_ok(lab):
            below = [x for x in sliders if x["rect"][1] >= lab["rect"][1] and abs(x["rect"][0] - lab["rect"][0]) < 40]
            return bool(below) and h.reachable(min(below, key=lambda x: x["rect"][1]))
        vis = [n for n in ls if top < n["rect"][1] and n["rect"][3] < bottom and slider_ok(n)]
        if vis:
            return vis[0]
        up = bool(ls) and ls[0]["rect"][1] <= top
        if ls and not up and ls[0]["rect"][1] < bottom:
            up = False  # just below: wheel down a little
        h.move(over)
        b = 4 if up or not ls and _ % 20 >= 10 else 5
        h.button(b, True)
        h.button(b, False)
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
    sl = param_slider(h, name)
    h.vcenter(sl)  # reachable (visible, on top, enabled)
    r = sl["rect"]
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
        if row_fn is palette_row:
            click_parts_row(h, name)
        else:
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


def on_mesh(m, p):
    """Whether screen point p lies inside one of mesh m's triangles."""
    pts, tri = m["points"], m.get("triangles") or []
    def side(a, b, c):
        return (b[0] - a[0]) * (c[1] - a[1]) - (b[1] - a[1]) * (c[0] - a[0])
    for i in range(0, len(tri) - 2, 3):
        try:
            a, b, c = pts[tri[i]], pts[tri[i + 1]], pts[tri[i + 2]]
        except IndexError:
            continue
        d1, d2, d3 = side(a, b, p), side(b, c, p), side(c, a, p)
        if not ((d1 < 0 or d2 < 0 or d3 < 0) and (d1 > 0 or d2 > 0 or d3 > 0)):
            return True
    return not tri  # no triangles known: don't filter


def canvas_drag_object(h, mesh_id, dx, dy, inside=False):
    """inside=True when the drag is on the selected mesh itself (not on a
    deformer around it): the press must land on the mesh, not just in its
    bounding box, or it would pick the mesh on top."""
    """Drags the selected object on the canvas from an exposed point of one
    of its meshes (moves it in the current keyform)."""
    # Away from vertices: a press on a vertex of a selected object picks
    # the vertex instead of moving the object.
    try:
        p = exposed_point(h, mesh_id, avoid_vertices=12)
        if inside and not on_mesh(h.mesh(mesh_id), p):
            raise RuntimeError("exposed point not on the mesh")
    except RuntimeError:
        # Covered by other meshes' bounds (e.g. irises under the eyelids):
        # a drag on a selected object takes priority, so a person drags it
        # where they see it, inside it and away from its vertices.
        m = h.mesh(mesh_id)
        pts, b = m["points"], m["bbox"]
        gap = lambda c: min((c[0] - q[0]) ** 2 + (c[1] - q[1]) ** 2 for q in pts) ** 0.5
        cx, cy = (b[0] + b[2]) / 2, (b[1] + b[3]) / 2
        grid = [(b[0] + (b[2] - b[0]) * fx / 8, b[1] + (b[3] - b[1]) * fy / 8) for fx in range(2, 7) for fy in range(2, 7)]
        grid.sort(key=lambda c: (c[0] - cx) ** 2 + (c[1] - cy) ** 2)
        m_now = h.mesh(mesh_id)
        far = [c for c in grid if gap(c) >= 12 and (not inside or on_mesh(m_now, c))]
        if far:
            p = far[0]  # nearest the centre, well clear of the vertices
        else:
            # Thin or dense meshes: the point of the middle farthest from
            # every vertex, outside the vertex pick radius (6 px).
            fine = [(b[0] + (b[2] - b[0]) * fx / 24, b[1] + (b[3] - b[1]) * fy / 24) for fx in range(2, 23) for fy in range(2, 23)]
            if inside:
                fine = [c for c in fine if on_mesh(m_now, c)] or fine
            p = max(fine, key=gap)
            if gap(p) < 8:
                raise RuntimeError(f"no point of {mesh_id} away from its vertices (best {gap(p):.1f} px)")
    p = h.canvas_point(p)
    h.drag(p, (p[0] + dx, p[1] + dy), steps=15)


def rubber_band_points(h, mesh_id, region):
    """With the mesh selected, rubber-band its points in a fraction of its
    bbox: region = (fx0, fy0, fx1, fy1)."""
    b = h.mesh(mesh_id)["bbox"]
    w, ht = b[2] - b[0], b[3] - b[1]
    a = (b[0] + w * region[0] - 6, b[1] + ht * region[1] - 6)
    z = (b[0] + w * region[2] + 6, b[1] + ht * region[3] + 6)
    h.drag(h.canvas_point(a), z, steps=10)


def drag_selected_points(h, mesh_id, region, dx, dy):
    """Drags one of the selected points (inside region)."""
    m = h.mesh(mesh_id)
    b = m["bbox"]
    w, ht = b[2] - b[0], b[3] - b[1]
    x0, y0, x1, y1 = b[0] + w * region[0] - 6, b[1] + ht * region[1] - 6, b[0] + w * region[2] + 6, b[1] + ht * region[3] + 6
    pts = [p for p in m["points"] if x0 <= p[0] <= x1 and y0 <= p[1] <= y1]
    if not pts:
        raise RuntimeError(f"no points of {mesh_id} in {region}")
    p = h.canvas_point(tuple(pts[0]))
    h.drag(p, (p[0] + dx, p[1] + dy), steps=15)


def face_view(h, min_px=250):
    """Zooms the canvas in at the face (wheel at the cursor) when it is too
    small on screen for work on eyes, brows and mouth, as a person would.
    Models whose face is already large enough are left as they are."""
    zoomed = False
    for _ in range(40):
        b = h.mesh(R["face"])["bbox"]
        if b[3] - b[1] >= min_px:
            break
        c = h.canvas()["rect"]
        p = ((b[0] + b[2]) / 2, (b[1] + b[3]) / 2)
        p = (min(max(p[0], c[0] + 20), c[2] - 20), min(max(p[1], c[1] + 20), c[3] - 20))
        h.move(h.canvas_point(p))
        h.button(4, True)
        h.button(4, False)
        h.settle(3)
        zoomed = True
    if zoomed:
        R["_zoomed"] = True
        h.note("zoomed in on the face for the face steps (the model is small on screen)")


def fit_view(h):
    """Show ▸ Fit to View, when the view was zoomed in."""
    if R.get("_zoomed"):
        h.menu("Show", "Fit to View")
        h.settle(3)
        R["_zoomed"] = False


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
            canvas_drag_object(h, R["face"], *d)
        reset_params(h)
    select_deformer(h, "BackHairWarp")
    key_op(h, "Angle X", "Add 3 keys")
    for where, d in (("min", (-10, 0)), ("max", (10, 0))):
        set_param(h, "Angle X", where)
        canvas_drag_object(h, R["hair_back"][0], *d)
    reset_params(h)


def keys_eyes(h, psd, out):
    face_view(h)
    for mesh, param in zip(R["eyelids"], ("EyeL Open", "EyeR Open")):
        select_mesh(h, mesh)
        key_op(h, param, "Add 2 keys")
        set_param(h, param, "min", 0.0, 1.0)
        # Workflow fix found here: Edit ▸ Select All (Ctrl+A) selects the
        # points of the selected object; Escape now deselects as the menu said.
        h.shortcut("ctrl+a", ("Edit", "Select All"))
        drag_selected_points(h, mesh, (0.0, 0.0, 1.0, 1.0), 0, 22)
        reset_params(h)


def keys_mouth(h, psd, out):
    face_view(h)
    select_mesh(h, R["mouth"])
    key_op(h, "Mouth Open", "Add 2 keys")
    set_param(h, "Mouth Open", "max", 0.0, 1.0)
    # The lower lip's vertices: click one, Shift+click the others (a rubber
    # band would start on the face mesh under the mouth and grab the face).
    m = h.mesh(R["mouth"])
    b = m["bbox"]
    low = [p for p in m["points"] if p[1] > b[1] + 0.55 * (b[3] - b[1])]
    if not low:
        raise RuntimeError("no lower mouth vertices")
    h.click(h.canvas_point(tuple(low[0])))
    for p in low[1:]:
        h.key_down("Shift_L")
        h.click(h.canvas_point(tuple(p)))
        h.key_up("Shift_L")
    h.drag(h.canvas_point(tuple(low[0])), (low[0][0], low[0][1] + 18), steps=15)
    reset_params(h)


def select_points(h, mesh_id, pred):
    """Selects exactly the mesh's vertices matching pred(point, bbox): click
    one, then Shift+click to add the others and to drop vertices left
    selected from before (the canvas map lists the selected vertices)."""
    m = h.mesh(mesh_id)
    b = m["bbox"]
    want = [k for k, p in enumerate(m["points"]) if pred(p, b)]
    if not want:
        raise RuntimeError(f"no matching vertices on {mesh_id}")
    pt = lambda k: tuple(m["points"][k])

    def selected():
        return {i for mid, i in (h.canvas() or {}).get("selected_points", []) if mid == m["id"]}

    h.click(h.canvas_point(pt(want[0])))
    for _ in range(3):
        sel = selected()
        flip = [k for k in want if k not in sel] + [k for k in sel if k not in want]
        if not flip:
            return [pt(k) for k in want]
        for k in flip:
            h.key_down("Shift_L")
            h.click(h.canvas_point(pt(k)))
            h.key_up("Shift_L")
    raise RuntimeError(f"could not select vertices {want} of {mesh_id} (selected {sorted(selected())})")


def keys_eye_smile(h, psd, out):
    face_view(h)
    for mesh, param in zip(R["eyelids"], ("EyeL Smile", "EyeR Smile")):
        select_mesh(h, mesh)
        key_op(h, param, "Add 2 keys")
        set_param(h, param, "max", 0.0, 1.0)
        h.shortcut("ctrl+a", ("Edit", "Select All"))
        drag_selected_points(h, mesh, (0.0, 0.0, 1.0, 1.0), 0, 9)
        reset_params(h)


def keys_eyeballs(h, psd, out):
    face_view(h)
    select_rows(h, R["irises"])
    for param, dmin, dmax in (("Eyeball X", (-9, 0), (9, 0)), ("Eyeball Y", (0, 9), (0, -9))):
        key_op(h, param, "Add 3 keys")
        for where, d in (("min", dmin), ("max", dmax)):
            set_param(h, param, where, -1.0, 1.0)
            canvas_drag_object(h, R["irises"][0], *d, inside=True)
        reset_params(h)


def keys_brows(h, psd, out):
    face_view(h)
    for mesh, py, pa, outer in ((R["brows"][0], "BrowL Y", "BrowL Angle", 0), (R["brows"][1], "BrowR Y", "BrowR Angle", 1)):
        select_mesh(h, mesh)
        key_op(h, py, "Add 3 keys")
        for where, d in (("min", (0, 9)), ("max", (0, -9))):
            set_param(h, py, where, -1.0, 1.0)
            canvas_drag_object(h, mesh, *d, inside=True)
        reset_params(h)
        key_op(h, pa, "Add 3 keys")
        # Drags beyond egui's click tolerance (6 px) so they register.
        for where, dy in (("min", 10), ("max", -10)):
            set_param(h, pa, where, -1.0, 1.0)
            # The inner third of the brow (toward the nose; the outer ends
            # sit under the front hair) goes down (min) or up (max).
            pts = select_points(h, mesh, lambda p, b, o=outer: (p[0] > b[2] - (b[2] - b[0]) / 3) if o == 0 else (p[0] < b[0] + (b[2] - b[0]) / 3))
            h.drag(h.canvas_point(pts[0]), (pts[0][0], pts[0][1] + dy), steps=12)
        reset_params(h)


def keys_mouth_form(h, psd, out):
    face_view(h)
    select_mesh(h, R["mouth"])
    key_op(h, "Mouth Form", "Add 3 keys")
    for where, dy in (("min", 10), ("max", -10)):
        set_param(h, "Mouth Form", where, -1.0, 1.0)
        pts = select_points(h, R["mouth"], lambda p, b: p[0] < b[0] + 0.15 * (b[2] - b[0]) or p[0] > b[2] - 0.15 * (b[2] - b[0]))
        h.drag(h.canvas_point(pts[0]), (pts[0][0], pts[0][1] + dy), steps=12)
    reset_params(h)


def clipping_masks(h, psd, out):
    """Irises clipped by the eye whites, the inner mouth by the mouth
    (Inspector ▸ Add mask…)."""
    pairs = list(zip(R["irises"], R["whites"])) + [(m, R["mouth_mask"]) for m in R["mouth_inner"]]
    import re
    ids_of = lambda v: set(re.split(r"[,\s]+", v or ""))
    for mesh, mask in pairs:
        select_mesh(h, mesh)
        # The Clipping field lists mask IDs; hidden masks aren't on the
        # canvas, so the ID comes from the picker's "name  (ID)" label.
        try:
            mask_id = h.mesh(mask)["id"]
        except RuntimeError:
            mask_id = None
        btn = inspector_field(h, lambda n: n["role"] == "Button" and n["label"] == "Add mask…")
        if mask_id and any(n["role"] == "TextInput" and mask_id in ids_of(n["value"]) and abs(n["rect"][1] - btn["rect"][1]) < 40 for n in h.nodes()):
            continue  # already set
        h.click(h.center(btn))
        picked = popup_pick(h, f"{mask}  (", h.center(btn), prefix=True)
        mask_id = picked.rsplit("(", 1)[1].rstrip(")")
        h.settle(2)
        clip = inspector_field(h, lambda n: n["role"] == "TextInput" and mask_id in ids_of(n["value"]))
        if not clip:
            raise RuntimeError(f"{mesh} shows no mask {mask}")


def create_parameter(h, name, pid, lo, default, hi):
    lab = param_label(h, "Angle X")
    h.click(h.center(lab), button=3)
    h.click_label("Create Parameter", role="Button")
    t = "Create Parameter"
    fields = h.in_window(t, role="TextInput")
    h.set_field(fields[0], name)
    fields = h.in_window(t, role="TextInput")
    h.set_field(fields[1], pid)
    spins = h.in_window(t, role="SpinButton")
    for f, v in zip(spins[:3], (lo, default, hi)):
        h.set_field(f, v)
        spins = h.in_window(t, role="SpinButton")
    h.click(h.center(h.in_window(t, label="OK")[0]))
    param_label(h, name)


def arm_params(h, psd, out):
    create_parameter(h, "Arm L A", "ParamArmLA", "-10", "0", "10")
    create_parameter(h, "Arm R A", "ParamArmRA", "-10", "0", "10")


def keys_hair(h, psd, out):
    fit_view(h)
    for warp, param, mesh in (("FrontHairWarp", "Hair Move Front", R["hair_front"][0]),
                              ("SideHairWarp", "Hair Move Side", R["hair_side"][0]),
                              ("BackHairWarp", "Hair Move Back", R["hair_back"][0])):
        select_deformer(h, warp)
        key_op(h, param, "Add 3 keys")
        for where, d in (("min", (-14, 0)), ("max", (14, 0))):
            set_param(h, param, where, -1.0, 1.0)
            canvas_drag_object(h, mesh, *d)
        reset_params(h)


def keys_arms(h, psd, out):
    fit_view(h)
    for rot, param, sign in (("ArmLRot", "Arm L A", 1), ("ArmRRot", "Arm R A", -1)):
        select_deformer(h, rot)
        key_op(h, param, "Add 3 keys")
        for where, angle in (("min", -14 * sign), ("max", 14 * sign)):
            set_param(h, param, where, -10.0, 10.0)
            field = inspector_field(h, lambda n: n["role"] == "SpinButton" and n["value"].endswith("°"))
            h.set_field(field, str(angle))
        reset_params(h)


def save_cmo3(h, psd, out):
    path = os.path.join(out, "model.cmo3")
    if os.path.exists(path):
        os.remove(path)
    h.answer_dialog(path)
    h.menu("File", "Save as…")
    for _ in range(50):
        if os.path.exists(path):
            break
        time.sleep(0.2)
    if not os.path.exists(path):
        raise RuntimeError("model.cmo3 was not written")


def keys_body(h, psd, out):
    fit_view(h)
    for warp, mesh, params in (("BodyWarp", R["body"], (("Body X", (-12, 0), (12, 0)), ("Body Y", (0, 9), (0, -9)))),
                               ("BreathWarp", R["breath_group"][0], (("Body Z", (-9, 3), (9, 3)),))):
        select_deformer(h, warp)
        for param, dmin, dmax in params:
            key_op(h, param, "Add 3 keys")
            for where, d in (("min", dmin), ("max", dmax)):
                set_param(h, param, where, -10.0, 10.0)
                canvas_drag_object(h, mesh, *d)
            reset_params(h)
    key_op(h, "Breath", "Add 2 keys")
    set_param(h, "Breath", "max", 0.0, 1.0)
    canvas_drag_object(h, R["breath_group"][0], 0, -9)
    reset_params(h)


PHYSICS_GROUPS = [("Hair Front", "Hair Move Front"), ("Hair Side", "Hair Move Side"), ("Hair Back", "Hair Move Back")]


def physics_open(h, psd, out):
    h.menu("Modeling", "Physics Settings…")
    h.find("Physics Settings", role="Window")


def physics_add_group(h, name):
    for attempt in range(2):
        h.click(h.center(h.in_window("Physics Settings", label="Add…")[0]))
        try:
            h.find("Add Physics Group", role="Window", timeout=4)
            break
        except RuntimeError:
            if attempt:
                raise
            h.note("Physics Settings ▸ Add… needed a second click (the first opened nothing)")
    field = h.in_window("Add Physics Group", role="TextInput")[0]
    h.set_field(field, name)
    if h.exists("Add Physics Group", role="Window"):
        h.click(h.center(h.in_window("Add Physics Group", label="OK")[0]))
    time.sleep(0.5)


def popup_pick(h, label, anchor, visible=330, prefix=False):
    """Clicks item `label` of a popup list opened at `anchor` (a menu or a
    combo list; it may open above or below). Items scrolled out of the
    popup are wheeled into view first."""
    match = lambda n: n["label"] == label or prefix and n["label"].startswith(label)
    for _ in range(40):
        items = [n for n in h.nodes(role="Button") if abs(n["rect"][0] - anchor[0]) < 160
                 and abs(n["rect"][1] - anchor[1]) > 4 and match(n)]
        if not items:
            raise RuntimeError(f"popup item {label!r} not found")
        t = min(items, key=lambda n: abs(n["rect"][0] - anchor[0]) + 0.01 * abs(n["rect"][1] - anchor[1]))
        if h.reachable(t):
            h.click(h.center(t))
            return t["label"]
        # Wheel over the popup's visible items, toward the target.
        col = [n for n in h.nodes(role="Button") if abs(n["rect"][0] - t["rect"][0]) < 2 and h.reachable(n)]
        if not col:
            raise RuntimeError(f"popup of {label!r} not visible")
        near = min(col, key=lambda n: abs(n["rect"][1] - t["rect"][1]))
        h.move(h.vcenter(near))
        b = 5 if t["rect"][1] > near["rect"][1] else 4
        h.button(b, True)
        h.button(b, False)
        h.settle(2)
    raise RuntimeError(f"popup item {label!r} not reachable")


def physics_groups(h, psd, out):
    for group, output in PHYSICS_GROUPS:
        physics_add_group(h, group)
        physics_inputs_outputs(h, output)
    h.click(h.center(h.in_window("Physics Settings", label="Close window")[0]))


def physics_inputs_outputs(h, output):
    w = "Physics Settings"
    h.click(h.center(h.in_window(w, label="Input settings")[0]))
    for param in ("Angle X", "Body X"):
        add = [n for n in h.in_window(w, label="Add…") if "Input parameter" not in n["label"]]
        # The inputs' Add… sits left of the hint text.
        hint = h.find("Input parameter can be added by right clicking on menu.")
        btn = min(add, key=lambda n: abs(n["rect"][1] - hint["rect"][1]) + abs(n["rect"][0] - 23))
        h.click(h.center(btn))
        popup_pick(h, param, h.center(btn))
    # New inputs start at 0 % effectiveness (physics-editor.md §5.1): set them.
    head = h.find("Effectiveness (%)")
    for value in ("60", "40"):
        rows = [n for n in h.in_window(w, role="SpinButton")
                if abs(n["rect"][0] - head["rect"][0]) < 30 and n["rect"][1] > head["rect"][3] and n["value"] in ("0", "0.0")]
        if not rows:
            raise RuntimeError("no input effectiveness field at 0")
        h.set_field(rows[0], value)
    h.click(h.center(h.in_window(w, label="Output Settings")[0]))
    h.click(h.center(h.in_window(w, label="Add", role="Button")[0]))
    combo = [n for n in h.in_window(w, role="ComboBox") if n["value"] == "Angle X"]
    if not combo:
        raise RuntimeError("no output parameter combo")
    h.click(h.center(combo[-1]))
    popup_pick(h, output, h.center(combo[-1]))


def texture_atlas(h, psd, out):
    fit_view(h)
    h.menu("Modeling", "Texture Atlas", "Edit Texture Atlas…")
    h.click(h.center(h.in_window("Create texture atlas", label="OK")[0]))
    # The initial grid packs image rectangles; meshes that overhang their
    # images (auto-mesh margins) need Auto Layout, which packs mesh bounds.
    h.click(h.center(h.in_window("Texture Atlas", label="Auto Layout…")[0]))
    h.click(h.center(h.in_window("Texture Atlas", label="Run Auto Layout")[0]))
    time.sleep(1)
    h.click(h.center(h.in_window("Texture Atlas", label="Close window")[0]))


def export_runtime(h, psd, out):
    rt = os.path.join(out, "runtime")
    os.makedirs(rt, exist_ok=True)
    for f in os.listdir(rt):
        p = os.path.join(rt, f)
        if os.path.isfile(p):
            os.remove(p)
    h.answer_dialog(rt)
    h.menu("File", "Export For Runtime", "Export as moc3 file…")
    for _ in range(50):
        if any(f.endswith(".moc3") for f in os.listdir(rt)):
            break
        time.sleep(0.2)
    names = sorted(os.listdir(rt))
    want_physics = R.get("expect_physics", True)
    if not any(f.endswith(".moc3") for f in names) or (want_physics and not any(f.endswith(".physics3.json") for f in names)):
        raise RuntimeError(f"runtime export incomplete: {names}")


def new_animation(h, psd, out):
    h.answer_dialog(os.path.join(out, "model.cmo3"))
    h.menu("File", "New", "Animation…")
    time.sleep(2)


def timeline_row(h, name):
    """A timeline attribute row, scrolled into view (wheel over the rows)."""
    for i in range(40):
        rows = [n for n in h.nodes(name, role="Row")]
        if not rows:
            raise RuntimeError(f"no timeline row {name!r}")
        vis = [n for n in rows if h.reachable(n)]
        if vis:
            return vis[0]
        ruler = h.find("Frame", role="Slider")
        over = (ruler["rect"][0] - 60, ruler["rect"][3] + 40)
        # Rows scrolled out of view have empty bounds at their own y.
        down = rows[0]["rect"][1] > ruler["rect"][3]
        h.move(over)
        h.button(5 if down else 4, True)
        h.button(5 if down else 4, False)
        h.settle(2)
    raise RuntimeError(f"timeline row {name!r} not reachable")


def set_frame(h, frame):
    ruler = h.find("Frame", role="Slider")
    h.vcenter(ruler)
    total = float(ruler["value"].split("/")[1])
    r = ruler["rect"]
    x = r[0] + (r[2] - r[0]) * frame / total
    h.click((x, (r[1] + r[3]) / 2))


def key_at(h, attr, frame, value):
    """Selects the attribute row, puts the playhead on `frame`, adds a key
    there (curve editor) and types its value."""
    h.click(h.center(timeline_row(h, attr)))
    set_frame(h, frame)
    h.click_label("Add key at playhead", role="Button")
    lab = h.find("key @", contains=True)
    field = [n for n in h.nodes(role="SpinButton") if abs(n["rect"][1] - lab["rect"][1]) < 6 and n["rect"][0] > lab["rect"][0]]
    if not field:
        raise RuntimeError("no key value field")
    h.set_field(field[0], value)


def anim_keys(h, psd, out):
    for f, v in ((0, "0"), (30, "30"), (60, "-30"), (90, "0")):
        key_at(h, "Angle X", f, v)
    for f, v in ((40, "1"), (45, "0"), (50, "1")):
        key_at(h, "EyeL Open", f, v)
        key_at(h, "EyeR Open", f, v)
    for f, v in ((10, "0"), (20, "1"), (30, "0")):
        key_at(h, "Mouth Open", f, v)


def save_can3(h, psd, out):
    path = os.path.join(out, "model.can3")
    if os.path.exists(path):
        os.remove(path)
    h.answer_dialog(path)
    h.menu("File", "Save as…")
    for _ in range(50):
        if os.path.exists(path):
            break
        time.sleep(0.2)
    if not os.path.exists(path):
        raise RuntimeError("model.can3 was not written")


def export_motions(h, psd, out):
    rt = os.path.join(out, "runtime")
    h.answer_dialog(rt)
    h.menu("File", "Export For Runtime", "Export motion file…")
    for _ in range(50):
        if any(f.endswith(".motion3.json") for f in os.listdir(rt)) or os.path.isdir(os.path.join(rt, "motions")):
            break
        time.sleep(0.2)


def viewer_motion(h, psd, out):
    """Open the exported model3.json, add the motion in Model Settings
    (group Idle) and save the model3.json, so a viewer plays it."""
    rt = os.path.join(out, "runtime")
    model3 = next(os.path.join(rt, f) for f in sorted(os.listdir(rt)) if f.endswith(".model3.json"))
    motion = next(os.path.join(rt, f) for f in sorted(os.listdir(rt)) if f.endswith(".motion3.json"))
    h.answer_dialog(model3)
    h.menu("File", "Open…")
    time.sleep(2)
    h.click_label("Model Settings")
    h.settle(2)
    h.answer_dialog(motion)
    h.click_label("Add motions…")
    h.settle(2)
    h.click_label("Save model3.json")
    for _ in range(50):
        if "Motions" in json.load(open(model3)).get("FileReferences", {}):
            break
        time.sleep(0.2)
    refs = json.load(open(model3)).get("FileReferences", {})
    if os.path.basename(motion) not in json.dumps(refs.get("Motions", {})):
        raise RuntimeError(f"model3.json lists no motion: {refs}")


def delete_layers(h, psd, out):
    """Deletes the meshes the profile marks for deletion (e.g. a layer the
    artist named "[DELETE]"): select the row, Edit ▸ Delete."""
    for name in R["delete"]:
        select_mesh(h, name)
        h.menu("Edit", "Delete")
        time.sleep(0.5)
        if any(n for n in h.nodes(name, role="Button") if h.reachable(n)):
            raise RuntimeError(f"{name} was not deleted")


def auto_mesh(h, psd, out):
    before = sum(len(m["points"]) for m in h.canvas()["meshes"])
    select_all_meshes(h)
    h.menu("Modeling", "ArtMesh", "Automatic Mesh generator…")
    h.find("Automatic Mesh Generator", role="Window")
    h.click_label("OK")
    time.sleep(1)
    for _ in range(300):  # large PSDs take a while
        tri = sum(len(m["points"]) for m in h.canvas()["meshes"])
        if tri > before:
            break
        time.sleep(0.5)
    if tri <= before:
        raise RuntimeError(f"auto mesh made no new vertices ({before} → {tri})")



STEPS = [
    ("File ▸ Open the PSD", open_psd),
    ("Create a new model from the PSD", new_model_from_psd),
    ("Delete the layers marked for deletion", delete_layers),
    ("Select all meshes; Automatic Mesh generator", auto_mesh),
    ("Rotation deformer for the head", head_rotation),
    ("Warp deformer for the face", face_warp),
    ("Warp deformers for front, side and back hair", make_hair_warps),
    ("Body and breath warps, arm rotation deformers", body_deformers),
    ("Head rotation keys on Angle Z", keys_head_angle_z),
    ("Face and hair keys on Angle X / Y", keys_face_angles),
    ("Eyelid keys on EyeL / EyeR Open", keys_eyes),
    ("Mouth Open keys (points dragged)", keys_mouth),
    ("Eye smile keys", keys_eye_smile),
    ("Eyeball X / Y keys (both irises)", keys_eyeballs),
    ("Brow Y and angle keys", keys_brows),
    ("Mouth Form keys (corners)", keys_mouth_form),
    ("Clipping masks: irises in the eye whites, inner mouth in the mouth", clipping_masks),
    ("Create the Arm L A / Arm R A parameters", arm_params),
    ("Hair sway keys (front, side, back)", keys_hair),
    ("Arm rotation keys", keys_arms),
    ("Body X / Y / Z and Breath keys", keys_body),
    ("Modeling ▸ Physics Settings", physics_open),
    ("Physics groups Hair Front / Side / Back (inputs Angle X, Body X)", physics_groups),
    ("Texture atlas (create, Auto Layout)", texture_atlas),
    ("File ▸ Save as model.cmo3", save_cmo3),
    ("File ▸ Export For Runtime (moc3, model3, physics3)", export_runtime),
    ("File ▸ New ▸ Animation", new_animation),
    ("Scene keys: head turn, blink, mouth", anim_keys),
    ("File ▸ Save as model.can3", save_can3),
    ("Export motion file (motion3.json)", export_motions),
    ("Model Settings: add the motion to model3.json", viewer_motion),
]

if __name__ == "__main__":
    main()
