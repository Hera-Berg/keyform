#!/usr/bin/env python3
"""Sorts the spec menu items that aren't in Keyform's menus.

usage: menu_coverage.py AUDIT_JSON [--overrides tools/e2e/menu-coverage-overrides.tsv] [--out FILE]

Classes:
  a  excluded by policy (sign-in, nizima, edition gating, licensing, Live2D web pages, …)
  p  in a Keyform menu under another name or place (rename / move only)
  b  present elsewhere (palette, context menu, dialog, toolbar or shortcut):
     needs its menu entry for parity
  c  missing feature
  k+ keyboard-only command (no menu in the reference either) that Keyform has
  k- keyboard-only command Keyform lacks

The automatic pass matches labels against what the reachability audit saw and
against the app's label strings; tools/e2e/menu-coverage-overrides.tsv
(spec path<TAB>class<TAB>note) records hand review and wins."""
import argparse
import glob
import json
import os
import re
import sys

HERE = os.path.dirname(os.path.abspath(__file__))
ROOT = os.path.dirname(os.path.dirname(HERE))
sys.path.insert(0, HERE)
import audit  # noqa: E402

POLICY = re.compile(r"sign ?in|log ?in|log ?out|account|nizima|licen[cs]e|activat|purchase|register|serial|"
                    r"pro version|free version|edition|upgrade|subscription|trial|bug report|feedback|"
                    r"check for update|update check|cubism website|community|tutorial video", re.I)
KEYBOARD = "Keyboard-only commands"


def norm(t):
    t = t.lower().replace("…", "").replace("...", "")
    t = re.sub(r"\[[^\]]*\]", "", t)
    t = re.sub(r"[^a-z0-9 ]+", " ", t)
    return " ".join(t.split())


def app_strings():
    out = set()
    for f in glob.glob(os.path.join(ROOT, "crates/keyform-app/src/**/*.rs"), recursive=True):
        for m in re.finditer(r'tr\(\s*"((?:[^"\\]|\\.)+)"', open(f, encoding="utf8").read()):
            out.add(norm(m.group(1)))
    return out


def main():
    ap = argparse.ArgumentParser()
    ap.add_argument("audit_json")
    ap.add_argument("--overrides", default=os.path.join(HERE, "menu-coverage-overrides.tsv"))
    ap.add_argument("--out")
    ap.add_argument("--doc", help="replace the spec-menu section of docs/reachability.md with this report")
    a = ap.parse_args()
    data = json.load(open(a.audit_json))
    seen = {norm(k): v for k, v in data["seen"].items()}
    # Menu labels as the audit normalises them now (older audit.json files
    # kept shortcuts such as "Enter"), plus variants without a trailing
    # parenthesis or shortcut digit ("Still Image (Sequence)…", "Level 1
    # (refine) 1"), which the spec's labels leave out.
    def variants(label):
        t = audit.norm(label)
        out = {norm(t)}
        t2 = re.sub(r"\s+[0-9]$", "", t)
        out.add(norm(t2))
        out.add(norm(re.sub(r"\s*\([^()]*\)\s*(…|\.\.\.)?$", "", t2)))
        return out

    have_menu = {tuple(json.loads(k)) for k in data["menu_items"]}
    have_norm = set()
    for p in have_menu:
        combos = [()]
        for x in p:
            combos = [c + (v,) for c in combos for v in variants(x)]
        have_norm.update(combos)
    strings = app_strings()
    over = {}
    if os.path.exists(a.overrides):
        for line in open(a.overrides, encoding="utf8"):
            if line.strip() and not line.startswith("#"):
                parts = line.rstrip("\n").split("\t")
                over[parts[0].strip()] = (parts[1].strip(), parts[2].strip() if len(parts) > 2 else "")
    rows = []
    for it in audit.spec_menu_items():
        path = it["path"]
        if tuple(norm(x) for x in path) in have_norm:
            continue  # in Keyform's menus at the spec place
        key = " ▸ ".join(path)
        label = norm(path[-1])
        keyboard = path[0].startswith(KEYBOARD)
        how = ""
        if key.strip() in over:
            cls, how = over[key.strip()]
        elif POLICY.search(key):
            cls = "a"
        else:
            hit = seen.get(label)
            if hit:
                cls, how = "b", "; ".join(hit[:1])
            elif label in strings:
                cls, how = "b", "label exists in the app (place not seen by the audit)"
            else:
                words = set(label.split())
                fuzzy = [k for k in seen if len(words) >= 2 and words <= set(k.split())]
                if fuzzy:
                    cls, how = "b?", "similar: " + seen[fuzzy[0]][0]
                else:
                    cls = "c"
            if keyboard:
                cls = "k+" if cls in ("b", "b?") else ("a" if cls == "a" else "k-")
        if keyboard and cls in ("b", "c"):
            cls = "k+" if cls == "b" else "k-"
        rows.append((path[0] if not keyboard else KEYBOARD, key, cls, how, it.get("shortcut", "")))
    counts = {}
    for r in rows:
        counts[r[2]] = counts.get(r[2], 0) + 1
    lines = ["## Spec menu coverage", "",
             "Every spec menu item (`keyform-spec/50-ui/menus.md`) that isn't in Keyform's menus at its",
             "spec place, sorted by `tools/e2e/menu_coverage.py` (label matching against what the",
             "audit saw and the app's strings, then hand review in",
             "`tools/e2e/menu-coverage-overrides.tsv`).", "",
             "| Class | Meaning | Count |", "|---|---|---|"]
    meaning = {"p": "in a Keyform menu under another name or place (rename / move only)", "a": "excluded by policy", "b": "present elsewhere — needs its menu entry",
               "b?": "probably present elsewhere (fuzzy match, to review)", "c": "missing feature",
               "k+": "keyboard-only command, present", "k-": "keyboard-only command, missing"}
    for c in ("a", "p", "b", "b?", "c", "k+", "k-"):
        lines.append(f"| {c} | {meaning[c]} | {counts.get(c, 0)} |")
    lines.append("")
    order = ["File", "Edit", "Show", "Modeling", "Animation", "Form Animation", "Mesh Edit mode", "Window", "Help", KEYBOARD]
    for menu in order + sorted({r[0] for r in rows} - set(order)):
        sel = [r for r in rows if r[0] == menu]
        if not sel:
            continue
        lines += [f"### {menu}", "", "| Spec item | Class | Where it is now / note |", "|---|---|---|"]
        for _, key, cls, how, sc in sel:
            item = key.split(" ▸ ", 1)[1] if " ▸ " in key else key
            lines.append(f"| {item}{' [' + sc + ']' if sc else ''} | {cls} | {how.replace('|', '/')} |")
        lines.append("")
    text = "\n".join(lines)
    if a.out:
        open(a.out, "w", encoding="utf8").write(text)
    if a.doc:
        doc = open(a.doc, encoding="utf8").read()
        cut = doc.find("## Spec menu items not in Keyform's menus")
        if cut < 0:
            cut = doc.find("## Spec menu coverage")
        doc = (doc[:cut] if cut >= 0 else doc.rstrip() + "\n\n") + text + "\n"
        open(a.doc, "w", encoding="utf8").write(doc)
    print(json.dumps(counts))


if __name__ == "__main__":
    main()
