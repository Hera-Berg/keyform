"""Real-input harness for Keyform's end-to-end test.

Keyform runs as an X client with KEYFORM_UI_MAP set; this module reads that
map (widget labels and bounds, canvas mesh positions) to find targets and
drives the app with XTest mouse and keyboard events, like a person would.
File dialogs are answered through KEYFORM_FILE_DIALOG (the OS dialog is not
Keyform's UI).
"""
import json
import os
import subprocess
import time

from PIL import Image
from Xlib import X, XK, display
from Xlib.ext import xtest


class Harness:
    def __init__(self, workdir, keyform, env_display, size=(1600, 1000), lang=None, extra_args=()):
        # Synthetic input must never reach the user's desktop session.
        if env_display == os.environ.get("DISPLAY") and not os.environ.get("KEYFORM_E2E_DISPLAY"):
            raise RuntimeError(f"refusing to drive the session display {env_display}; use tools/e2e/display.sh")
        self.workdir = workdir
        os.makedirs(workdir, exist_ok=True)
        self.map_path = os.path.join(workdir, "uimap.json")
        self.answers = os.path.join(workdir, "dialog-answers.txt")
        open(self.answers, "w").close()
        self.log = open(os.path.join(workdir, "keyform.log"), "w")
        env = dict(os.environ)
        env.pop("WAYLAND_DISPLAY", None)
        env["DISPLAY"] = env_display
        env["KEYFORM_UI_MAP"] = self.map_path
        # One window pixel per point: stable coordinates on any test display.
        env["WINIT_X11_SCALE_FACTOR"] = "1"
        env["KEYFORM_FILE_DIALOG"] = self.answers
        # A clean settings home so remembered layouts don't move widgets.
        import shutil
        shutil.rmtree(os.path.join(workdir, "home"), ignore_errors=True)
        env["HOME"] = os.path.join(workdir, "home")
        env["XDG_CONFIG_HOME"] = os.path.join(workdir, "home", ".config")
        env["XDG_DATA_HOME"] = os.path.join(workdir, "home", ".local", "share")
        os.makedirs(env["XDG_CONFIG_HOME"], exist_ok=True)
        args = [keyform, "--size", f"{size[0]}x{size[1]}", *extra_args]
        if lang:
            args += ["--lang", lang]
        self.proc = subprocess.Popen(args, env=env, stdout=self.log, stderr=subprocess.STDOUT)
        self.d = display.Display(env_display)
        self.root = self.d.screen().root
        self.win = None
        self.steps = []
        self.frames = []  # (label, PIL image) for the GIF
        self.issues = []  # workflow notes
        self.wait_map(30)
        self.find_window()

    # ---- window and map -------------------------------------------------
    def find_window(self, timeout=20):
        end = time.time() + timeout
        while time.time() < end:
            for w in self._windows(self.root):
                try:
                    name = w.get_wm_name() or ""
                except Exception:
                    name = ""
                if name.startswith("Keyform") or "keyform" in name.lower():
                    g = w.get_geometry()
                    if g.width > 300:
                        self.win = w
                        self.focus()
                        return w
            time.sleep(0.3)
        raise RuntimeError("Keyform window not found")

    def focus(self):
        """Gives the window keyboard focus (no window manager does it on a
        bare test display)."""
        self.win.set_input_focus(X.RevertToParent, X.CurrentTime)
        self.d.sync()
        time.sleep(0.1)

    def _windows(self, w):
        try:
            kids = w.query_tree().children
        except Exception:
            return []
        out = []
        for k in kids:
            out.append(k)
            out.extend(self._windows(k))
        return out

    def origin(self):
        t = self.win.translate_coords(self.root, 0, 0)
        return (-t.x, -t.y)

    def wait_map(self, timeout=10, after=None):
        end = time.time() + timeout
        while time.time() < end:
            m = self.read_map()
            if m and (after is None or m["frame"] > after):
                return m
            time.sleep(0.1)
        raise RuntimeError("no UI map")

    def read_map(self):
        try:
            with open(self.map_path) as f:
                return json.load(f)
        except Exception:
            return None

    def settle(self, frames=3, timeout=10):
        """Waits until the app has drawn `frames` more frames."""
        m = self.read_map()
        start = m["frame"] if m else 0
        end = time.time() + timeout
        while time.time() < end:
            # Moving the pointer by 0 px still wakes the event loop.
            self._nudge()
            m = self.read_map()
            if m and m["frame"] >= start + frames:
                return m
            time.sleep(0.05)
        return self.read_map()

    def _nudge(self):
        p = self.root.query_pointer()
        xtest.fake_input(self.d, X.MotionNotify, x=p.root_x + 1, y=p.root_y)
        xtest.fake_input(self.d, X.MotionNotify, x=p.root_x, y=p.root_y)
        self.d.sync()

    def nodes(self, label=None, role=None, contains=False):
        m = self.settle(2) or {}
        out = []
        for n in m.get("nodes", []):
            if role and n["role"] != role:
                continue
            if label is not None:
                text = n["label"] or n["value"]
                if contains:
                    if label.lower() not in text.lower():
                        continue
                elif text != label:
                    continue
            out.append(n)
        return out

    def find(self, label, role=None, contains=False, timeout=8, index=0):
        end = time.time() + timeout
        while time.time() < end:
            ns = [n for n in self.nodes(label, role, contains) if not n.get("disabled")]
            if len(ns) > index:
                return ns[index]
            time.sleep(0.2)
        raise RuntimeError(f"widget not found: {label!r} ({role})")

    def find_item(self, label, timeout=8):
        """A menu item: exact label, or the label followed by its shortcut."""
        end = time.time() + timeout
        while time.time() < end:
            for n in self.nodes():
                t = n["label"] or n["value"]
                if not n.get("disabled") and (t == label or t.startswith(label + " ")):
                    return n
            time.sleep(0.2)
        raise RuntimeError(f"menu item not found: {label!r}")

    def in_window(self, title, role=None, label=None, timeout=8):
        """Widgets inside the dialog window `title` (top to bottom, left to right)."""
        w = self.find(title, role="Window", timeout=timeout)
        r = w["rect"]
        out = []
        for n in self.nodes(label, role):
            c = self.center(n)
            if n is not w and r[0] <= c[0] <= r[2] and r[1] <= c[1] <= r[3] and n["role"] != "Window":
                out.append(n)
        out.sort(key=lambda n: (round(n["rect"][1]), n["rect"][0]))
        return out

    def exists(self, label, role=None, contains=False):
        return bool(self.nodes(label, role, contains))

    def canvas(self):
        m = self.settle(2) or {}
        return m.get("canvas")

    def mesh(self, mesh_id):
        c = self.canvas() or {}
        for mm in c.get("meshes", []):
            if mm["id"] == mesh_id:
                return mm
        raise RuntimeError(f"mesh not on canvas: {mesh_id}")

    # ---- input ----------------------------------------------------------
    def to_screen(self, p):
        m = self.read_map() or {"ppp": 1.0}
        ox, oy = self.origin()
        s = m.get("ppp", 1.0)
        return (int(round(ox + p[0] * s)), int(round(oy + p[1] * s)))

    def move(self, p, steps=1):
        x, y = self.to_screen(p)
        xtest.fake_input(self.d, X.MotionNotify, x=x, y=y)
        self.d.sync()
        time.sleep(0.03)

    def button(self, b, down):
        xtest.fake_input(self.d, X.ButtonPress if down else X.ButtonRelease, b)
        self.d.sync()
        time.sleep(0.04)

    def click(self, p, button=1, double=False):
        self.move(p)
        time.sleep(0.08)
        for k in range(2 if double else 1):
            self.button(button, True)
            # Hold for a frame: widgets that act while pressed (sliders)
            # need to see the button down.
            if not double:
                self.settle(1, timeout=2)
            self.button(button, False)
        self.settle(2)

    def drag(self, a, b, steps=20, button=1, mods=()):
        for k in mods:
            self.key_down(k)
        self.move(a)
        time.sleep(0.1)
        self.button(button, True)
        for i in range(1, steps + 1):
            t = i / steps
            self.move((a[0] + (b[0] - a[0]) * t, a[1] + (b[1] - a[1]) * t))
            time.sleep(0.02)
        time.sleep(0.1)
        self.button(button, False)
        for k in reversed(mods):
            self.key_up(k)
        self.settle(3)

    def stroke(self, pts, button=1):
        self.move(pts[0])
        self.button(button, True)
        for p in pts[1:]:
            self.move(p)
            time.sleep(0.02)
        self.button(button, False)
        self.settle(3)

    def center(self, node):
        r = node["rect"]
        return ((r[0] + r[2]) / 2, (r[1] + r[3]) / 2)

    def click_label(self, label, role=None, contains=False, double=False, index=0):
        n = self.find(label, role, contains, index=index)
        self.click(self.center(n), double=double)
        return n

    def menu(self, *path):
        """Opens a menu path: menu bar item, submenus, then the item."""
        for i, label in enumerate(path):
            if 0 < i < len(path) - 1:
                # Submenu buttons carry an arrow.
                try:
                    n = self.find(label + " ⏵", timeout=2)
                except RuntimeError:
                    n = self.find(label)
            else:
                n = self.find_item(label)
            if i < len(path) - 1 and i > 0:
                # Submenus open on hover.
                self.move(self.center(n))
                time.sleep(0.4)
                self.settle(3)
            else:
                self.click(self.center(n))
                time.sleep(0.2)

    def _keycode(self, name):
        sym = XK.string_to_keysym(name)
        if sym == 0 and len(name) == 1:
            sym = ord(name)
        return self.d.keysym_to_keycode(sym)

    def key_down(self, name):
        if self.d.get_input_focus().focus != self.win:
            self.focus()
        xtest.fake_input(self.d, X.KeyPress, self._keycode(name))
        self.d.sync()

    def key_up(self, name):
        xtest.fake_input(self.d, X.KeyRelease, self._keycode(name))
        self.d.sync()

    def key(self, combo):
        parts = combo.split("+")
        names = [{"ctrl": "Control_L", "shift": "Shift_L", "alt": "Alt_L"}.get(p.lower(), p) for p in parts]
        for n in names:
            self.key_down(n)
            time.sleep(0.02)
        for n in reversed(names):
            self.key_up(n)
            time.sleep(0.02)
        self.settle(2)

    def type(self, text):
        for ch in text:
            shift = ch.isupper() or ch in '~!@#$%^&*()_+{}|:"<>?'
            name = {" ": "space", ".": "period", "-": "minus", "_": "underscore", "/": "slash", ":": "colon"}.get(ch, ch)
            if shift and name == ch and ch.isalpha():
                name = ch.lower()
            if shift:
                self.key_down("Shift_L")
            self.key_down(name)
            self.key_up(name)
            if shift:
                self.key_up("Shift_L")
            time.sleep(0.015)
        self.settle(2)

    def set_field(self, node, text):
        """Replaces a text / number field's content."""
        # One click puts a text field or a DragValue into editing.
        self.click(self.center(node))
        self.key("ctrl+a")
        self.type(text)
        self.key("Return")

    def answer_dialog(self, path):
        with open(self.answers, "a") as f:
            f.write(f"{path}\n")

    # ---- recording ------------------------------------------------------
    def grab(self):
        g = self.win.get_geometry()
        raw = self.win.get_image(0, 0, g.width, g.height, X.ZPixmap, 0xFFFFFFFF)
        return Image.frombytes("RGB", (g.width, g.height), raw.data, "raw", "BGRX")

    def step(self, name):
        self.steps.append((name, time.time()))
        img = self.grab()
        self.frames.append((name, img.resize((img.width // 2, img.height // 2))))
        img.save(os.path.join(self.workdir, f"step{len(self.steps):02d}.png"))
        print(f"[step {len(self.steps):02d}] {name} ({img.width}x{img.height}, win {hex(self.win.id)})", flush=True)

    def note(self, text):
        """A place where the mouse workflow felt wrong (reported)."""
        self.issues.append(text)
        print(f"  NOTE: {text}", flush=True)

    def save_gif(self, path):
        if not self.frames:
            return
        imgs = [f[1] for f in self.frames]
        imgs[0].save(path, save_all=True, append_images=imgs[1:], duration=900, loop=0)

    def close(self):
        try:
            self.proc.terminate()
            self.proc.wait(10)
        except Exception:
            self.proc.kill()
