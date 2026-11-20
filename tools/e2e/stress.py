#!/usr/bin/env python3
"""Stress pass: a large PSD through the same UI steps as the session's
start (open, new model, Automatic Mesh, atlas, save, export), with real
input on the private display. Records each step's time and Keyform's peak
memory (VmHWM) in OUTDIR/stress.json; then reopens the saved model to time
loading it.

usage: stress.py PSD OUTDIR [--keyform BIN] [--display :N]

Intended for local PSDs that may not be committed: OUTDIR should be outside
the repository (or gitignored)."""
import argparse
import json
import os
import sys
import time

HERE = os.path.dirname(os.path.abspath(__file__))
sys.path.insert(0, HERE)
import session as S  # noqa: E402
from harness import Harness  # noqa: E402


def vm(pid, key):
    try:
        for line in open(f"/proc/{pid}/status"):
            if line.startswith(key + ":"):
                return int(line.split()[1]) // 1024  # MB
    except OSError:
        pass
    return None


def main():
    ap = argparse.ArgumentParser()
    ap.add_argument("psd")
    ap.add_argument("out")
    ap.add_argument("--keyform", default="target/debug/keyform")
    ap.add_argument("--display", default=os.environ.get("KEYFORM_E2E_DISPLAY", ":7"))
    a = ap.parse_args()
    allowed = os.environ.get("KEYFORM_E2E_DISPLAY", ":7")
    if a.display != allowed:
        sys.exit(f"refusing to drive display {a.display}: only the private test display {allowed}")
    out = os.path.abspath(a.out)
    os.makedirs(out, exist_ok=True)
    psd = os.path.abspath(a.psd)
    S.R["expect_physics"] = False  # no physics in the stress pass
    steps = [
        ("open PSD", S.open_psd),
        ("new model from PSD", S.new_model_from_psd),
        ("Automatic Mesh (all meshes)", S.auto_mesh),
        ("texture atlas + Auto Layout", S.texture_atlas),
        ("save .cmo3", S.save_cmo3),
        ("export runtime", S.export_runtime),
    ]
    rows, failed = [], None
    h = Harness(out, a.keyform, a.display)
    try:
        pid = h.proc.pid
        for name, fn in steps:
            t = time.time()
            try:
                fn(h, psd, out)
            except Exception as e:  # noqa: BLE001
                failed = f"{name}: {e}"
                S.dump(h, f"FAILED {name}")
                break
            h.step(name)
            rows.append({"step": name, "seconds": round(time.time() - t, 1),
                         "rss_mb": vm(pid, "VmRSS"), "peak_mb": vm(pid, "VmHWM")})
            print(f"[stress] {name}: {rows[-1]}", flush=True)
        meshes = len((h.canvas() or {}).get("meshes", []))
    finally:
        h.close()
    reopen = None
    model = os.path.join(out, "model.cmo3")
    if not failed and os.path.exists(model):
        h = Harness(os.path.join(out, "reopen"), a.keyform, a.display)
        try:
            t = time.time()
            h.answer_dialog(model)
            h.menu("File", "Open…")
            for _ in range(600):
                c = h.canvas()
                if c and len(c.get("meshes", [])) >= meshes:
                    break
                time.sleep(0.2)
            reopen = {"seconds": round(time.time() - t, 1), "peak_mb": vm(h.proc.pid, "VmHWM")}
        finally:
            h.close()
    sizes = {f: os.path.getsize(os.path.join(out, f)) for f in ("model.cmo3",) if os.path.exists(os.path.join(out, f))}
    report = {"psd": os.path.basename(psd), "psd_bytes": os.path.getsize(psd), "meshes": meshes,
              "steps": rows, "reopen": reopen, "files": sizes, "failed": failed}
    json.dump(report, open(os.path.join(out, "stress.json"), "w"), indent=2)
    print(json.dumps(report, indent=2))
    sys.exit(1 if failed else 0)


if __name__ == "__main__":
    main()
