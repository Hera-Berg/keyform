#!/usr/bin/env python3
"""Black-box driver for the reference editor on a private X display.

Usage: bb.py shot OUT.png [x y w h]
       bb.py click X Y [button] [count]
       bb.py dclick X Y
       bb.py move X Y
       bb.py drag X1 Y1 X2 Y2 [button]
       bb.py key KEYSYM[+KEYSYM...]      e.g. ctrl+o, Return, alt+F4
       bb.py type TEXT
       bb.py windows
       bb.py raise WINDOW_ID
"""
import sys
import time

from PIL import Image
from Xlib import X, XK, display
from Xlib.ext import xtest

D = display.Display(":2")
ROOT = D.screen().root


def shot(out, x=0, y=0, w=None, h=None):
    """Composites the mapped top-level windows (rootless Xwayland has no
    readable root window), bottom to top."""
    g = ROOT.get_geometry()
    canvas = Image.new("RGB", (g.width, g.height), (40, 40, 40))
    for c in ROOT.query_tree().children:
        try:
            if c.get_attributes().map_state != X.IsViewable:
                continue
            cg = c.get_geometry()
            if cg.width < 4 or cg.height < 4:
                continue
            t = c.translate_coords(ROOT, 0, 0)
            raw = c.get_image(0, 0, cg.width, cg.height, X.ZPixmap, 0xFFFFFFFF)
            depth = raw.depth
            mode = "BGRX" if depth in (24, 32) else None
            if mode is None:
                continue
            img = Image.frombytes("RGB", (cg.width, cg.height), raw.data, "raw", mode)
            canvas.paste(img, (-t.x, -t.y))
        except Exception:
            continue
    if w and h:
        canvas = canvas.crop((x, y, x + w, y + h))
    canvas.save(out)


def move(x, y):
    xtest.fake_input(D, X.MotionNotify, x=x, y=y)
    D.sync()


def button(b, press):
    xtest.fake_input(D, X.ButtonPress if press else X.ButtonRelease, b)
    D.sync()


def click(x, y, b=1, count=1):
    move(x, y)
    time.sleep(0.3)
    for _ in range(count):
        button(b, True)
        time.sleep(0.12 if count == 1 else 0.04)
        button(b, False)
        time.sleep(0.08)


def drag(x1, y1, x2, y2, b=1, steps=20):
    move(x1, y1)
    time.sleep(0.05)
    button(b, True)
    for i in range(1, steps + 1):
        move(x1 + (x2 - x1) * i // steps, y1 + (y2 - y1) * i // steps)
        time.sleep(0.02)
    button(b, False)


ALIASES = {"ctrl": "Control_L", "shift": "Shift_L", "alt": "Alt_L", "enter": "Return",
           "esc": "Escape", "del": "Delete", "tab": "Tab", "space": "space"}


def keycode(name):
    name = ALIASES.get(name.lower(), name) if len(name) > 1 else name
    ks = XK.string_to_keysym(name)
    if ks == 0 and len(name) == 1:
        ks = ord(name)
    kc = D.keysym_to_keycode(ks)
    if kc == 0:
        raise SystemExit(f"no keycode for {name!r}")
    return kc


def key(combo):
    codes = [keycode(k) for k in combo.split("+")]
    for c in codes:
        xtest.fake_input(D, X.KeyPress, c)
    D.sync()
    time.sleep(0.03)
    for c in reversed(codes):
        xtest.fake_input(D, X.KeyRelease, c)
    D.sync()
    time.sleep(0.05)


SHIFTED = {c: True for c in '~!@#$%^&*()_+{}|:"<>?ABCDEFGHIJKLMNOPQRSTUVWXYZ'}
NAMES = {" ": "space", ".": "period", "/": "slash", "\\": "backslash", "-": "minus",
         "_": "underscore", ":": "colon", ",": "comma", "=": "equal", "+": "plus",
         "(": "parenleft", ")": "parenright"}


def type_text(text):
    for ch in text:
        name = NAMES.get(ch, ch)
        kc = D.keysym_to_keycode(XK.string_to_keysym(name))
        shift = ch in SHIFTED
        if shift:
            xtest.fake_input(D, X.KeyPress, keycode("Shift_L"))
        xtest.fake_input(D, X.KeyPress, kc)
        xtest.fake_input(D, X.KeyRelease, kc)
        if shift:
            xtest.fake_input(D, X.KeyRelease, keycode("Shift_L"))
        D.sync()
        time.sleep(0.02)


def windows():
    def walk(w, depth=0):
        for c in w.query_tree().children:
            try:
                a = c.get_attributes()
                n = c.get_wm_name()
                if a.map_state == X.IsViewable and (n or depth == 0):
                    g = c.get_geometry()
                    t = c.translate_coords(ROOT, 0, 0)
                    print("  " * depth, hex(c.id), repr(n), f"{g.width}x{g.height}+{-t.x}+{-t.y}")
            except Exception:
                continue
            if depth < 1:
                walk(c, depth + 1)
    walk(ROOT)


def find(title):
    """(x, y, w, h) of the first mapped window whose name contains `title`."""
    for f in ROOT.query_tree().children:
        try:
            if f.get_attributes().map_state != X.IsViewable:
                continue
            for c in [f] + list(f.query_tree().children):
                n = c.get_wm_name()
                if n and title in n:
                    g = c.get_geometry()
                    t = c.translate_coords(ROOT, 0, 0)
                    return (-t.x, -t.y, g.width, g.height)
        except Exception:
            continue
    return None


def wait(title, timeout=30.0, gone=False):
    end = time.time() + timeout
    while time.time() < end:
        if (find(title) is None) == gone:
            time.sleep(0.5)
            return True
        time.sleep(0.3)
    raise SystemExit(f"timeout waiting for {title!r}{' to close' if gone else ''}")


def main(a):
    cmd = a[0]
    if cmd == "wait":
        wait(a[1], float(a[2]) if len(a) > 2 else 30.0)
        return
    if cmd == "filename":
        # File Name field of a file dialog: 422 px right, 86 px above the bottom.
        x, y, w, h = find(a[1])
        click(x + 422, y + h - 86)
        return
    if cmd == "close":
        # Politely close windows whose name contains the title (WM_DELETE_WINDOW).
        from Xlib.protocol import event
        proto = D.intern_atom("WM_PROTOCOLS")
        delete = D.intern_atom("WM_DELETE_WINDOW")
        for f in ROOT.query_tree().children:
            for c in [f] + list(f.query_tree().children):
                try:
                    n = c.get_wm_name()
                except Exception:
                    continue
                if n and n.startswith(a[1]):
                    ev = event.ClientMessage(window=c, client_type=proto, data=(32, [delete, X.CurrentTime, 0, 0, 0]))
                    c.send_event(ev)
        D.sync()
        return
    if cmd == "waitgone":
        wait(a[1], float(a[2]) if len(a) > 2 else 30.0, gone=True)
        return
    if cmd == "shot":
        shot(a[1], *map(int, a[2:6]))
    elif cmd == "click":
        click(int(a[1]), int(a[2]), int(a[3]) if len(a) > 3 else 1, int(a[4]) if len(a) > 4 else 1)
    elif cmd == "dclick":
        click(int(a[1]), int(a[2]), 1, 2)
    elif cmd == "move":
        move(int(a[1]), int(a[2]))
    elif cmd == "drag":
        drag(*map(int, a[1:5]), int(a[5]) if len(a) > 5 else 1)
    elif cmd == "key":
        key(a[1])
    elif cmd == "type":
        type_text(a[1])
    elif cmd == "windows":
        windows()
    elif cmd == "raise":
        w = D.create_resource_object("window", int(a[1], 16))
        w.configure(stack_mode=X.Above)
        w.set_input_focus(X.RevertToParent, X.CurrentTime)
        D.sync()
    else:
        raise SystemExit(__doc__)


if __name__ == "__main__":
    main(sys.argv[1:])
