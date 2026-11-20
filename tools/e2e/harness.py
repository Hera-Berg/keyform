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


class HonestyError(RuntimeError):
    """The script tried something a person at the screen couldn't do."""


class Harness:
    def __init__(self, workdir, keyform, env_display, size=(1600, 1000), lang=None, extra_args=()):
        # Synthetic input must only reach the private test display: :7
        # (tools/e2e/display.sh) or the one named by KEYFORM_E2E_DISPLAY.
        allowed = os.environ.get("KEYFORM_E2E_DISPLAY", ":7")
        if env_display != allowed:
            raise RuntimeError(
                f"refusing to drive display {env_display}: only the private test display {allowed} "
                "(start tools/e2e/display.sh, or set KEYFORM_E2E_DISPLAY)"
            )
        print(f"[harness] driving private test display {env_display}", flush=True)
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
        import atexit
        atexit.register(self.close)
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
            pid_atom = self.d.intern_atom("_NET_WM_PID")
            for w in self._windows(self.root):
                try:
                    name = w.get_wm_name() or ""
                    prop = w.get_full_property(pid_atom, X.AnyPropertyType)
                    pid = prop.value[0] if prop else None
                except Exception:
                    name, pid = "", None
                if pid != self.proc.pid:
                    continue
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
        try:
            self.win.set_input_focus(X.RevertToParent, X.CurrentTime)
            self.d.sync()
        except Exception:
            # Not mapped yet (BadMatch): retried before the next key press.
            pass
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

    def reachable(self, n):
        """Visible (unclipped part), on top, enabled, inside the window."""
        v = n.get("visible")
        if n.get("disabled") or n.get("covered") or not v:
            return False
        if v[2] - v[0] < 2 or v[3] - v[1] < 2:
            return False
        g = self.win.get_geometry()
        c = ((v[0] + v[2]) / 2, (v[1] + v[3]) / 2)
        return 0 <= c[0] < g.width and 0 <= c[1] < g.height

    def find(self, label, role=None, contains=False, timeout=8, index=0):
        """The `index`-th reachable widget with this label (a person could
        see and click it)."""
        end = time.time() + timeout
        while time.time() < end:
            ns = [n for n in self.nodes(label, role, contains) if self.reachable(n)]
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
                if self.reachable(n) and (t == label or t.startswith(label + " ")):
                    return n
            time.sleep(0.2)
        raise RuntimeError(f"menu item not found: {label!r}")

    def in_window(self, title, role=None, label=None, timeout=8):
        """Widgets inside the dialog window `title` (top to bottom, left to right)."""
        w = self.find(title, role="Window", timeout=timeout)
        r = w["rect"]
        out = []
        for n in self.nodes(label, role):
            if not self.reachable(n):
                continue
            c = self.vcenter(n)
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
            if mesh_id in (mm["id"], mm.get("name")):
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
            # (at most 0.5 s: egui drops a press held longer than 0.8 s as
            # a click, and slow frames on big models would exceed that).
            if not double:
                self.settle(1, timeout=0.5)
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

    def vcenter(self, node):
        """Centre of the widget's visible part; fails (HonestyError) when a
        person couldn't click it: disabled, clipped away, zero-size, covered
        by a window or popup, or outside the window."""
        if not self.reachable(node):
            raise HonestyError(f"widget not reachable by a person: {node}")
        v = node["visible"]
        return ((v[0] + v[2]) / 2, (v[1] + v[3]) / 2)

    center = vcenter

    def click_node(self, node, button=1, double=False):
        self.click(self.vcenter(node), button=button, double=double)

    def canvas_point(self, p):
        """Fails when canvas point `p` isn't on the visible canvas or is under
        a window / popup."""
        c = self.canvas()
        r = c["rect"]
        if not (r[0] <= p[0] <= r[2] and r[1] <= p[1] <= r[3]):
            raise HonestyError(f"canvas point {p} is off the visible canvas {r}")
        for n in self.nodes(role="Window"):
            v = n.get("visible")
            if v and v[0] <= p[0] <= v[2] and v[1] <= p[1] <= v[3]:
                raise HonestyError(f"canvas point {p} is covered by window {n['label']!r}")
        return p

    def shortcut(self, combo, menu):
        """Presses a keyboard shortcut after checking that the menu item
        `menu` (path) shows it, as a person would learn it."""
        *path, item = menu
        for i, label in enumerate(path):
            n = self.find(label + " ⏵", timeout=2) if i > 0 and self.exists(label + " ⏵") else self.find(label)
            if i == 0:
                self.click_node(n)
            else:
                self.move(self.vcenter(n))
                time.sleep(0.4)
        it = self.find_item(item)
        shown = (it["label"] or it["value"]).lower().replace(" ", "")
        if combo.lower().replace(" ", "") not in shown:
            raise HonestyError(f"shortcut {combo} is not shown on menu item {it['label']!r}")
        # Close the menu again (click its title), then use the shortcut.
        self.click_node(self.find(path[0]))
        self.key(combo)

    def click_label(self, label, role=None, contains=False, double=False, index=0):
        n = self.find(label, role, contains, index=index)
        self.click_node(n, double=double)
        return n

    def menu(self, *path):
        """Opens a menu path: menu bar item, submenus, then the item."""
        for i, label in enumerate(path):
            if i == 0:
                # The menu bar title: exact (prefix matching would take
                # "Edit Level:" for "Edit").
                bar = [x for x in self.nodes(label, role="Button") if x["rect"][1] < 6 and self.reachable(x)]
                if not bar:
                    raise RuntimeError(f"menu bar item not found: {label!r}")
                n = bar[0]
            elif 0 < i < len(path) - 1:
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
        if self.proc.poll() is not None:
            return
        try:
            self.proc.terminate()
            self.proc.wait(10)
        except Exception:
            self.proc.kill()
