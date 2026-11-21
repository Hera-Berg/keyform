#!/usr/bin/env python3
"""Reachability audit: can a person reach every command?

usage: audit.py OUTDIR [--keyform BIN] [--display :N] [--session DIR]
                [--doc docs/reachability.md] [--check]

Drives Keyform on the private test display (as tools/e2e/session.py does)
through several states (no document; a model; a model with a mesh or a
deformer selected; an animation; a runtime model3.json). In each it opens
every menu and submenu, the context menus of the palettes, the canvas and
the timeline, every dock tab, the mode and other combo boxes, and every
"…" menu item's dialog (file dialogs are answered Cancel). Everything seen
is recorded with how it was reached.

The commands come from:
- the spec's menu tree (keyform-spec/50-ui/menus.md, when present);
- the app's code: every literal label passed to a button, menu, checkbox,
  selectable or radio widget in crates/keyform-app/src;
- the app's keyboard shortcuts (consume_shortcut / consume_key).

Fails (--check) when, not counting the entries of
tools/e2e/reachability-notes.tsv (label, how to reach it, by hand) and
tools/e2e/reachability-known-gaps.tsv (unfixed gaps, also listed in
IMPLEMENTATION_STATUS.md):
- a code label was seen nowhere (no visible entry point);
- a menu item was disabled in every state (never enabled);
- a shortcut is shown on no menu item;
- a text field has neither a label nor a hint (a value the UI never shows).
Writes OUTDIR/audit.json and, with --doc, the feature table."""
import argparse
import glob
import json
import os
import re
import shutil
import subprocess
import sys
import time

HERE = os.path.dirname(os.path.abspath(__file__))
ROOT = os.path.dirname(os.path.dirname(HERE))
sys.path.insert(0, HERE)
from harness import Harness  # noqa: E402

SHORTCUT_RE = re.compile(r"\s+((?:Ctrl|Shift|Alt|Cmd|⌘)(?:\+\S+)+|F\d+|Delete|Del|Esc|Escape|Enter|Home|End|Page Up|Page Down)$")
LABEL_CALL_RE = re.compile(
    r"(?:\.button|Button::new|\.menu_button|\.checkbox\([^,()]*(?:\([^()]*\))?[^,()]*,|SelectableLabel::new\([^,]*,|"
    r"\.selectable_label\([^,]*,|\.selectable_value\([^,]*,[^,]*,|\.radio_value\([^,]*,[^,]*,|\.radio\([^,]*,|"
    r"\.toggle_value\([^,]*,|Checkbox::new\([^,]*,|\.small_button)\s*\(?\s*(?:&)?crate::i18n::tr\(\s*\"((?:[^\"\\]|\\.)+)\"\s*\)"
)
KEY_RE = re.compile(r"(?:K::new\(\s*([^,]+?)\s*,\s*(?:egui::)?Key::(\w+)\s*\)|consume_key\(\s*([^,]+?)\s*,\s*(?:egui::)?Key::(\w+)\s*\))")


def norm(label):
    t = (label or "").strip()
    t = t.replace(" ⏵", "").strip()
    t = SHORTCUT_RE.sub("", t).strip()
    return t


def shortcut_of(label):
    m = SHORTCUT_RE.search((label or "").strip())
    return m.group(1) if m else ""


def key_name(mods, key):
    mods = mods.replace(" ", "")
    parts = []
    if "COMMAND" in mods or "CTRL" in mods or "shift_cmd" in mods or "cmd" in mods.lower():
        parts.append("Ctrl")
    if "SHIFT" in mods or "shift_cmd" in mods:
        parts.append("Shift")
    if "ALT" in mods:
        parts.append("Alt")
    k = key[3:] if key.startswith("Num") else key
    parts.append(k)
    return "+".join(parts)


# ---- sources of commands ------------------------------------------------
def code_labels():
    out = {}
    for f in sorted(glob.glob(os.path.join(ROOT, "crates/keyform-app/src/**/*.rs"), recursive=True)):
        text = open(f, encoding="utf8").read()
        # Skip test modules.
        cut = text.find("#[cfg(test)]")
        body = text if cut < 0 else text[:cut]
        for m in LABEL_CALL_RE.finditer(body):
            lab = m.group(1).encode().decode("unicode_escape") if "\\" in m.group(1) else m.group(1)
            line = body.count("\n", 0, m.start()) + 1
            out.setdefault(lab, f"{os.path.relpath(f, ROOT)}:{line}")
    return out


def code_shortcuts():
    out = {}
    for f in sorted(glob.glob(os.path.join(ROOT, "crates/keyform-app/src/**/*.rs"), recursive=True)):
        text = open(f, encoding="utf8").read()
        cut = text.find("#[cfg(test)]")
        body = text if cut < 0 else text[:cut]
        for m in KEY_RE.finditer(body):
            mods, key = (m.group(1), m.group(2)) if m.group(2) else (m.group(3), m.group(4))
            if not re.fullmatch(r"[\w:| ]+", mods):
                continue  # a variable key list: reported by its own entries
            line = body.count("\n", 0, m.start()) + 1
            out.setdefault(key_name(mods, key), f"{os.path.relpath(f, ROOT)}:{line}")
    return out


def spec_menu_items():
    path = os.path.join(os.path.dirname(ROOT), "keyform-spec", "50-ui", "menus.md")
    if not os.path.exists(path):
        return []
    items, top, stack = [], None, []
    for raw in open(path, encoding="utf8"):
        line = raw.rstrip("\n")
        h = re.match(r"^## (.+)$", line)
        if h:
            top, stack = h.group(1).strip(), []
            continue
        b = re.match(r"^(\s*)- (.+)$", line)
        if not (top and b):
            continue
        depth = len(b.group(1)) // 2
        text = b.group(2)
        if text.startswith("<") or text.strip() == "---":
            continue
        sc = re.search(r"\[([^\]]+)\]", text)
        label = re.sub(r"\[[^\]]*\]", "", text).replace("…", "").replace("☑", "").strip()
        # A trailing note in parentheses is dropped, except a view qualifier
        # ("Show Grid (Modeling View)"), which tells items apart.
        if not re.search(r"\((?:[^()]*\b[Vv]iew)\)$", label):
            label = re.sub(r"\s+\(.*\)$", "", label).strip()
        stack = stack[:depth] + [label]
        items.append({"path": [top] + stack, "shortcut": sc.group(1) if sc else "", "dialog": "…" in text})
    return items


# ---- exploration ----------------------------------------------------------
class Explorer:
    def __init__(self, a, out):
        self.a = a
        self.out = out
        self.seen = {}  # norm label -> list of how
        self.menu_items = {}  # tuple(path) -> {"enabled_in": set, "label": raw, "states": set}
        self.inputs = []
        self.dialogs_done = set()
        self.clicked = set()
        self.tooltips = {}  # norm label -> tooltip text
        self.h = None

    def record(self, label, how):
        n = norm(label)
        if not n:
            return
        lst = self.seen.setdefault(n, [])
        if how not in lst and len(lst) < 4:
            lst.append(how)

    def snapshot(self):
        m = self.h.settle(2) or {}
        out = []
        for n in m.get("nodes", []):
            v = n.get("visible")
            if not v or n.get("covered") or v[2] - v[0] < 2 or v[3] - v[1] < 2:
                continue
            out.append(n)
        return out

    @staticmethod
    def key(n):
        return (n["role"], n["label"] or n["value"], tuple(round(x) for x in n["rect"]))

    def record_all(self, nodes, how):
        for n in nodes:
            t = n["label"] or n["value"]
            if t and n["role"] not in ("TextRun",):
                self.record(t, how)
            if n["role"] in ("TextInput", "MultilineTextInput", "SearchInput") and not (n["label"] or n["value"]):
                self.inputs.append({"how": how, "rect": n["rect"], "nodes": nodes})

    def start(self, state, path=None, setup=None):
        if self.h:
            self.h.close()
        if getattr(self, "state", None) != state:
            self.clicked = set()
        work = os.path.join(self.out, "runs", re.sub(r"\W+", "-", state))
        self.h = Harness(work, self.a.keyform, self.a.display)
        self.h.settle(4)
        if path:
            self.h.answer_dialog(path)
            self.h.menu("File", "Open…")
            time.sleep(2.5)
            self.h.settle(4)
        if setup:
            setup(self.h)
        self.state = state
        self.path = path
        self.setup = setup

    def restart(self):
        self.start(self.state, self.path, self.setup)

    def escape_all(self):
        """Closes an open menu or popup: one Escape (a second one would be
        Deselect)."""
        self.h.key("Escape")
        self.h.settle(2)

    def menubar(self):
        nodes = self.snapshot()
        # The menu bar: buttons in the top row of the window.
        top = [n for n in nodes if n["role"] == "Button" and n["rect"][1] < 6 and n["rect"][3] < 26]
        top.sort(key=lambda n: n["rect"][0])
        return top

    def walk_menus(self):
        for t in self.menubar():
            title = t["label"]
            base = {self.key(n) for n in self.snapshot()}
            self.h.click(self.h.vcenter(t))
            time.sleep(0.2)
            after = self.snapshot()
            # Menu items line up under the title (toolbar widgets that
            # change state meanwhile are not menu items).
            x0 = t["rect"][0]
            items = [n for n in after if self.key(n) not in base and n["role"] != "Window"
                     and x0 - 8 <= n["rect"][0] <= x0 + 40 and n["rect"][1] >= t["rect"][3] - 2]
            items.sort(key=lambda n: n["rect"][1])
            self.walk_items([title], items, base | {self.key(n) for n in after})
            self.escape_all()

    def walk_items(self, path, items, known, depth=0):
        for n in items:
            raw = n["label"] or n["value"]
            if not raw:
                continue
            p = tuple(path + [norm(raw)])
            e = self.menu_items.setdefault(p, {"label": raw, "enabled_in": [], "states": []})
            if self.state not in e["states"]:
                e["states"].append(self.state)
            if not n.get("disabled") and self.state not in e["enabled_in"]:
                e["enabled_in"].append(self.state)
            self.record(raw, " ▸ ".join(p) + f" ({self.state})")
            if raw.endswith("⏵") and depth < 3 and not n.get("disabled"):
                self.h.move(self.h.vcenter(n))
                time.sleep(0.45)
                sub_nodes = self.snapshot()
                sub = [s for s in sub_nodes if self.key(s) not in known and s["role"] != "Window"]
                sub.sort(key=lambda s: s["rect"][1])
                self.walk_items(list(p), sub, known | {self.key(s) for s in sub_nodes}, depth + 1)

    def open_menu_path(self, p):
        """Opens menu path p (titles without shortcuts) and returns the item node."""
        h = self.h
        bar = {norm(n["label"]): n for n in self.menubar()}
        h.click(h.vcenter(bar[p[0]]))
        time.sleep(0.2)
        node = None
        for i, label in enumerate(p[1:], start=1):
            cands = [n for n in self.snapshot() if norm(n["label"] or n["value"]) == label and n["role"] != "Window"]
            if not cands:
                return None
            node = cands[-1]
            if i < len(p) - 1:
                h.move(h.vcenter(node))
                time.sleep(0.45)
        return node

    def dialogs(self):
        for p, e in sorted(self.menu_items.items()):
            if self.state not in e["enabled_in"] or p in self.dialogs_done:
                continue
            if not (e["label"].split("  ")[0].rstrip().endswith("…") or "…" in norm(e["label"])):
                continue
            self.dialogs_done.add(p)
            before = {self.key(n) for n in self.snapshot()}
            try:
                node = self.open_menu_path(list(p))
                if node is None or node.get("disabled"):
                    self.escape_all()
                    continue
                self.h.click(self.h.vcenter(node))
                time.sleep(1.2)
                after = self.snapshot()
            except Exception as ex:  # noqa: BLE001
                print(f"  dialog {' ▸ '.join(p)}: {ex}", flush=True)
                self.restart()
                continue
            new = [n for n in after if self.key(n) not in before]
            wins = [n for n in new if n["role"] == "Window"]
            how = f"dialog via {' ▸ '.join(p)} ({self.state})"
            self.record_all(new, how)
            # Tabs inside the dialog: click each and record what appears.
            for w in wins:
                r = w["rect"]
                for tab in [n for n in new if n["role"] in ("Tab", "RadioButton") and r[0] <= n["rect"][0] <= r[2] and r[1] <= n["rect"][1] <= r[3]][:8]:
                    try:
                        self.h.click(self.h.vcenter(tab))
                        self.record_all(self.snapshot(), how + f" ▸ {tab['label']}")
                    except Exception:  # noqa: BLE001
                        pass
            if wins:
                w = wins[0]
                def reopen(p=p, title=w["label"]):
                    node = self.open_menu_path(list(p))
                    if node is None or node.get("disabled"):
                        self.escape_all()
                        return False
                    self.h.click(self.h.vcenter(node))
                    time.sleep(1.2)
                    return any(x["role"] == "Window" and x["label"] == title for x in self.snapshot())
                region = w["rect"]
                self.click_through(region=region, how_prefix=f"{w['label']} dialog via {' ▸ '.join(p)}", reopen=reopen)
                before = {self.key(n) for n in self.snapshot() if n["role"] != "Window" or n["label"] != w["label"]}
            self.close_dialogs(before)

    def close_dialogs(self, before, restart=True):
        for attempt in range(4):
            extra = [n for n in self.snapshot() if n["role"] == "Window" and self.key(n) not in before]
            if not extra:
                return
            if attempt == 0:
                self.h.key("Escape")
            else:
                for lab in ("Cancel", "Close", "×", "✕"):
                    c = [n for n in self.snapshot() if (n["label"] or "") == lab and n["role"] == "Button" and not n.get("disabled")]
                    if c:
                        self.h.click(self.h.vcenter(c[0]))
                        break
            time.sleep(0.4)
        if restart:
            self.restart()

    SKIP = {"Exit", "Close", "Close all", "×", "✕", "x", "X", "OK", "Apply", "Cancel", "Done", "Finish",
            "Confirm", "Yes", "No", "Delete", "Remove", "Save", "Save as…", "Open…"}

    def panel_of(self, n, nodes):
        """The dock tab (or dialog) a widget sits in."""
        c = ((n["rect"][0] + n["rect"][2]) / 2, n["rect"][1])
        best = None
        for t in nodes:
            if t["role"] not in ("Tab", "Window") or not (t["label"] or "").strip():
                continue
            r = t["rect"]
            if r[1] <= c[1] and (t["role"] == "Window" and r[0] <= c[0] <= r[2] and c[1] <= r[3] or
                                 t["role"] == "Tab" and r[0] - 20 <= c[0] <= r[0] + 460 and t.get("selected") is not False):
                if best is None or r[1] > best["rect"][1]:
                    best = t
        return best["label"] if best else "the toolbar"

    def click_through(self, region=None, how_prefix="", reopen=None):
        """Presses each labelled, enabled button once (per label), records
        what appears (popups, dialogs, tool panels), then closes it."""
        done_here = set()
        for _ in range(400):
            nodes = self.snapshot()
            todo = []
            for n in nodes:
                lab = norm(n["label"] or n["value"] or "") if n["role"] == "ComboBox" else norm(n["label"] or "")
                if n["role"] not in ("Button", "ComboBox") or n.get("disabled") or not lab or lab in self.SKIP:
                    continue
                if n["rect"][1] < 6 and n["rect"][3] < 26:
                    continue  # the menu bar: walked separately
                if region and not (region[0] <= n["rect"][0] <= region[2] and region[1] <= n["rect"][1] <= region[3]):
                    continue
                k = (region is not None, how_prefix or self.panel_of(n, nodes), lab)
                if k in self.clicked or k in done_here:
                    continue
                todo.append((n, k))
            if not todo:
                return
            n, k = todo[0]
            lab = k[2]
            done_here.add(k)
            self.clicked.add(k)
            where = k[1]
            before = {self.key(x) for x in nodes}
            # Hover first: the tooltip is where tool shortcuts are shown.
            try:
                if n["rect"][3] > 50 or region is not None:
                    raise StopIteration  # only the toolbar's tools carry shortcuts in tooltips
                # egui shows a tooltip once the pointer has been still for a
                # moment, on a repaint: nudge (repaint) early, then wait.
                self.h.move(self.h.vcenter(n))
                time.sleep(0.3)
                self.h.settle(1)
                time.sleep(0.9)
                # Tooltips aren't interactive, so the UI map's hit test marks
                # their text "covered": read the map without that filter.
                raw = (self.h.settle(2) or {}).get("nodes", [])
                tips = [x for x in raw if self.key(x) not in before and x.get("visible") and x["role"] in ("Label", "StaticText") and (x["label"] or x["value"])]
                if tips:
                    self.tooltips[lab] = " ".join(x["label"] or x["value"] for x in tips)
                    self.record_all(tips, f"tooltip of {lab} in {where} ({self.state})")
            except (Exception, StopIteration):  # noqa: BLE001
                pass
            titles = {x["label"] for x in nodes if x["role"] == "Tab"}
            try:
                self.h.click(self.h.vcenter(n))
                time.sleep(0.5)
                after = self.snapshot()
            except Exception:  # noqa: BLE001
                continue
            new = [x for x in after if self.key(x) not in before]
            self.record_all(new, f"{where} ▸ {lab} ({self.state})")
            modified = any(x["role"] == "Tab" and x["label"].endswith("*") and x["label"] not in titles for x in after)
            gone = region is not None and not any(x["role"] == "Window" and self.key(x) in before for x in after if x["role"] == "Window")
            self.escape_all()
            extra = [x for x in self.snapshot() if x["role"] == "Window" and self.key(x) not in before]
            if extra:
                self.close_dialogs(before, restart=False)
            if modified or (region is not None and gone) or [x for x in self.snapshot() if x["role"] == "Window" and self.key(x) not in before]:
                self.restart()
                if reopen:
                    if not reopen():
                        return

    def context_menu(self, label, where, role=None, index=0):
        cands = [n for n in self.snapshot() if (n["label"] or n["value"]) == label and (role is None or n["role"] == role) and not n.get("disabled")]
        if len(cands) <= index:
            return
        self.context_at(self.h.vcenter(cands[index]), where)

    def context_at(self, p, where):
        base = {self.key(n) for n in self.snapshot()}
        self.h.click(p, button=3)
        time.sleep(0.3)
        after = self.snapshot()
        items = [n for n in after if self.key(n) not in base and n["role"] != "Window"]
        items.sort(key=lambda n: n["rect"][1])
        if items:
            self.walk_items([f"context menu of {where}"], items, base | {self.key(n) for n in after})
        self.escape_all()

    def tabs_and_combos(self):
        nodes = self.snapshot()
        self.record_all(nodes, f"main window ({self.state})")
        for t in [n for n in nodes if n["role"] == "Tab"]:
            try:
                self.h.click(self.h.vcenter(t))
                self.record_all(self.snapshot(), f"{t['label']} tab ({self.state})")
            except Exception:  # noqa: BLE001
                pass
        for c in [n for n in self.snapshot() if n["role"] == "ComboBox" and not n.get("disabled")][:12]:
            base = {self.key(n) for n in self.snapshot()}
            try:
                self.h.click(self.h.vcenter(c))
                time.sleep(0.2)
                new = [n for n in self.snapshot() if self.key(n) not in base]
                self.record_all(new, f"combo box {c['label'] or c['value']!r} ({self.state})")
            except Exception:  # noqa: BLE001
                pass
            self.escape_all()


def select_row(name, panel_hint):
    def f(h):
        ns = [n for n in h.nodes(name) if h.reachable(n)]
        if not ns:
            print(f"  (no row {name!r} to select in the {panel_hint})")
            return
        # Parts rows sit above the Deformer palette's rows.
        ns.sort(key=lambda n: n["rect"][1])
        n = ns[0] if panel_hint == "Parts palette" else ns[-1]
        h.click(h.vcenter(n))
    return f


def enter_mesh_edit(h):
    select_row("Face", "Parts palette")(h)
    h.menu("Modeling", "ArtMesh", "Mesh edit")
    time.sleep(0.5)


def explore(a, out):
    ex = Explorer(a, out)
    fx = os.path.join(out, "files")
    os.makedirs(fx, exist_ok=True)
    s = a.session
    files = {}
    for name in ("model.cmo3", "model.can3"):
        if os.path.exists(os.path.join(s, name)):
            shutil.copy(os.path.join(s, name), fx)
            files[name] = os.path.join(fx, name)
    rt = os.path.join(s, "runtime")
    if os.path.isdir(rt):
        shutil.copytree(rt, os.path.join(fx, "runtime"), dirs_exist_ok=True)
        m3 = [f for f in os.listdir(rt) if f.endswith(".model3.json")]
        if m3:
            files["model3"] = os.path.join(fx, "runtime", m3[0])
    states = [("no document", None, None)]
    if "model.cmo3" in files:
        cm = files["model.cmo3"]
        states += [
            ("model", cm, None),
            ("model, mesh selected", cm, select_row("Face", "Parts palette")),
            ("model, deformer selected", cm, select_row("Head", "Deformer palette")),
            ("mesh edit", cm, enter_mesh_edit),
        ]
    if "model.can3" in files:
        states.append(("animation", files["model.can3"], None))
    if "model3" in files:
        states.append(("runtime model", files["model3"], None))
    timings = {}
    for state, path, setup in states:
        t0 = time.time()
        print(f"[audit] state: {state}", flush=True)
        ex.start(state, path, setup)
        ex.walk_menus()
        ex.tabs_and_combos()
        if setup is not None or state in ("model", "animation"):
            for lab, where in (("Face", "a Parts row"), ("Head", "a Deformer row"), ("Angle X", "a Parameter row")):
                ex.context_menu(lab, where)
            c = ex.h.canvas()
            if c:
                r = c["rect"]
                ex.context_at(((r[0] + r[2]) / 2, (r[1] + r[3]) / 2), "the canvas")
        if state == "animation":
            rows = [n for n in ex.snapshot() if n["role"] == "Row"]
            if rows:
                ex.context_at(ex.h.vcenter(rows[0]), "a timeline row")
        if state in ("no document", "model", "animation", "runtime model", "mesh edit"):
            ex.click_through()
        ex.dialogs()
        timings[state] = round(time.time() - t0, 1)
    if ex.h:
        ex.h.close()
    return ex, timings


# ---- report ---------------------------------------------------------------
def load_tsv(name):
    path = os.path.join(HERE, name)
    out = {}
    if os.path.exists(path):
        for line in open(path, encoding="utf8"):
            if line.strip() and not line.startswith("#"):
                k, _, v = line.rstrip("\n").partition("\t")
                out[k.strip()] = v.strip()
    return out


def main():
    ap = argparse.ArgumentParser()
    ap.add_argument("out")
    ap.add_argument("--keyform", default="target/debug/keyform")
    ap.add_argument("--display", default=os.environ.get("KEYFORM_E2E_DISPLAY", ":7"))
    ap.add_argument("--session", default="e2e-out", help="session.py output folder (model.cmo3, model.can3, runtime/)")
    ap.add_argument("--doc")
    ap.add_argument("--check", action="store_true")
    ap.add_argument("--from-json", help="rebuild the report from an earlier audit.json (no UI run)")
    a = ap.parse_args()
    out = os.path.abspath(a.out)
    os.makedirs(out, exist_ok=True)
    if a.from_json:
        data = json.load(open(a.from_json))
        seen, menu_items, inputs, timings = data["seen"], {tuple(json.loads(k)): v for k, v in data["menu_items"].items()}, data["inputs"], data["timings"]
        tooltips = data.get("tooltips", {})
        # Normalised again: the shortcut pattern may have changed since.
        renormed = {}
        for k, v in seen.items():
            renormed.setdefault(norm(k), []).extend(v)
        seen = renormed
    else:
        ex, timings = explore(a, out)
        seen, menu_items, timings, tooltips = ex.seen, ex.menu_items, timings, ex.tooltips
        inputs = [{"how": i["how"], "rect": i["rect"], "near": nearby_labels(i)} for i in ex.inputs]
        json.dump({"seen": seen, "menu_items": {json.dumps(list(k)): v for k, v in menu_items.items()},
                   "inputs": inputs, "timings": timings, "tooltips": tooltips}, open(os.path.join(out, "audit.json"), "w"), indent=1, ensure_ascii=False)
    notes = load_tsv("reachability-notes.tsv")
    gaps = load_tsv("reachability-known-gaps.tsv")
    labels = code_labels()
    keys = code_shortcuts()
    spec = spec_menu_items()
    shown_shortcuts = {}
    for p, e in menu_items.items():
        sc = shortcut_of(e["label"])
        if sc:
            shown_shortcuts.setdefault(sc.replace("Cmd", "Ctrl"), " ▸ ".join(p))
    failures, rows = [], []

    def status(key, ok_how):
        if ok_how:
            return "yes", ok_how
        if key in notes:
            return "by hand", notes[key]
        if key in gaps:
            return "GAP (known)", gaps[key]
        return "NO", ""

    for lab, where in sorted(labels.items(), key=lambda kv: kv[0].lower()):
        n = norm(lab)
        how = "; ".join(seen.get(n, [])[:2])
        st, desc = status(lab, how)
        rows.append(("Command", lab, desc, st, where))
        if st == "NO":
            failures.append(f"no visible entry point seen for {lab!r} ({where})")
    for p, e in sorted(menu_items.items()):
        if not e["enabled_in"]:
            key = " ▸ ".join(p)
            st, desc = status(key, "")
            if st == "NO":
                failures.append(f"menu item never enabled: {key} (seen in: {', '.join(e['states'])})")
            rows.append(("Menu item never enabled", key, desc or f"disabled in: {', '.join(e['states'])}", "NO" if st == "NO" else st, ""))
    def in_tooltip(combo):
        pats = [f"({combo})", f"[{combo}]", f"({combo},", f", {combo})"] + ([f" {combo}", f"({combo}"] if "+" in combo else [])
        for lab, tip in tooltips.items():
            if any(p.lower() in tip.lower() for p in pats):
                return f"shown in the tooltip of {lab}"
        return ""

    for combo, where in sorted(keys.items()):
        how = shown_shortcuts.get(combo, "")
        st, desc = status(combo, f"shown on {how}" if how else in_tooltip(combo))
        rows.append(("Shortcut", combo, desc, st, where))
        if st == "NO":
            failures.append(f"shortcut {combo} is shown on no menu item ({where})")
    for i in inputs:
        if not i.get("near"):
            key = f"unlabeled field in {i['how']}"
            st, desc = status(key, "")
            rows.append(("Unlabeled field", key, desc, st, str([round(v) for v in i["rect"]])))
            if st == "NO":
                failures.append(f"text field with no label or hint: {i['how']} at {[round(v) for v in i['rect']]}")
    spec_missing = []
    have = {tuple(p) for p in menu_items}
    have_norm = {tuple(x.lower().replace("…", "").replace("...", "").strip() for x in p) for p in have}
    leaf = {}
    for q in have:
        leaf.setdefault(q[-1].lower().replace("…", "").replace("...", "").strip(), " ▸ ".join(q))
    for it in spec:
        p = tuple(x.lower() for x in it["path"])
        if p not in have_norm:
            it = dict(it)
            it["elsewhere"] = leaf.get(p[-1], "")
            spec_missing.append(it)
    write_doc(a, rows, spec_missing, timings, failures)
    print(f"[audit] {len(labels)} code labels, {len(menu_items)} menu items, {len(keys)} shortcuts; "
          f"{len(spec_missing)} spec menu items not at their spec place ({sum(1 for i in spec_missing if not i.get('elsewhere'))} absent); {len(failures)} failures")
    for f in failures:
        print("FAIL", f)
    if a.check and failures:
        sys.exit(1)


def nearby_labels(i):
    r = i["rect"]
    near = []
    for n in i["nodes"]:
        if n["role"] not in ("Label", "StaticText") or not (n["label"] or n["value"]):
            continue
        q = n["rect"]
        left = q[2] <= r[0] + 2 and r[0] - q[2] < 220 and abs((q[1] + q[3]) / 2 - (r[1] + r[3]) / 2) < 14
        above = q[3] <= r[1] + 2 and r[1] - q[3] < 30 and q[0] < r[2] and q[2] > r[0]
        if left or above:
            near.append(n["label"] or n["value"])
    return near


def write_doc(a, rows, spec_missing, timings, failures):
    if not a.doc:
        return
    lines = [
        "# Reachability",
        "",
        "How a person reaches each command, as found by `tools/e2e/audit.py`",
        "(it opens every menu, submenu, context menu, dock tab, combo box and `…`",
        "dialog in several states and records what is visible, on top and",
        "enabled). Generated; do not edit by hand. Notes for entries the",
        "automatic walk cannot reach are in `tools/e2e/reachability-notes.tsv`,",
        "unfixed gaps in `tools/e2e/reachability-known-gaps.tsv`.",
        "",
        "Checked: **yes** = seen by the audit; **by hand** = described in the",
        "notes (a state the audit doesn't set up); **GAP** = no entry point yet.",
        "",
    ]
    if failures:
        lines += ["## Failures", ""] + [f"- {f}" for f in failures] + [""]
    for kind in ("Command", "Shortcut", "Menu item never enabled", "Unlabeled field"):
        sel = [r for r in rows if r[0] == kind]
        if not sel:
            continue
        lines += [f"## {kind}s" if not kind.endswith("enabled") else "## Menu items never enabled", "",
                  "| Feature | How to reach it | Checked | Code |", "|---|---|---|---|"]
        for _, lab, how, st, where in sel:
            esc = lambda s: s.replace("|", "\\|")
            lines.append(f"| {esc(lab)} | {esc(how)} | {st} | `{where}` |" if where else f"| {esc(lab)} | {esc(how)} | {st} | |")
        lines.append("")
    if spec_missing:
        lines += ["## Spec menu items not in Keyform's menus", "",
                  "From `keyform-spec/50-ui/menus.md`; unimplemented or named differently.", ""]
        moved = [it for it in spec_missing if it.get("elsewhere")]
        absent = [it for it in spec_missing if not it.get("elsewhere")]
        if moved:
            lines += ["### In another place in Keyform", ""]
            lines += [f"- {' ▸ '.join(it['path'])} → {it['elsewhere']}" for it in moved]
            lines.append("")
        lines += ["### Not in Keyform's menus", ""]
        lines += [f"- {' ▸ '.join(it['path'])}" + (f" [{it['shortcut']}]" if it["shortcut"] else "") for it in absent]
        lines.append("")
    os.makedirs(os.path.dirname(os.path.abspath(a.doc)), exist_ok=True)
    open(a.doc, "w", encoding="utf8").write("\n".join(lines))


if __name__ == "__main__":
    main()
