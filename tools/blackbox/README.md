# Black-box testing against the reference editor

Scripts for driving the official Live2D Cubism Editor / Viewer **as a black
box** (run it, export files, take screenshots) per `../../../keyform-spec/CLEANROOM.md`.
Never point them at the reference's installation files.

## Setup (Linux)

    # Wine prefix with the editor (the installer is NSIS: /S = silent)
    WINEPREFIX=~/.wine-cubism wineboot -i
    WINEPREFIX=~/.wine-cubism wine Live2D_Cubism_Setup_5.3.04.exe /S

    # A private headless display, so nothing opens on the desktop:
    weston --backend=headless --socket=cubism-bb --xwayland --width=1920 --height=1080 &
    # (Xwayland listens on the next free display, e.g. :2 — see weston's log)

    cd ~/.wine-cubism/drive_c/Program\ Files/Live2D\ Cubism\ 5.3
    env -u WAYLAND_DISPLAY DISPLAY=:2 WINEPREFIX=~/.wine-cubism wine CubismEditor5.exe &
    env -u WAYLAND_DISPLAY DISPLAY=:2 WINEPREFIX=~/.wine-cubism wine CubismViewer5.exe 'C:\...\model.model3.json' &

The editor starts with a licence choice; "Start as FREE version" needs no
account (projects over the FREE limits open read-only, and image export is
capped at about 1000 px).

## Tools (need python-xlib and Pillow; `cmp.py` also numpy)

- `bb.py` — screenshots (`shot`, composited from the top-level windows
  because rootless Xwayland has no readable root window), input through
  XTEST (`click`, `dclick`, `drag`, `key ctrl+o`, `type`), `windows`,
  `wait TITLE`, `waitgone TITLE`, `close TITLE`, `filename TITLE` (clicks a
  file dialog's File Name field). Display `:2` is hard-coded.
- `open.sh WINPATH` — File ▸ Open.
- `export.sh MAG WINPATH` — File ▸ Export Image/Video ▸ Image at MAG %,
  then closes the Explorer window the editor opens on the output folder.
- `cmp.py REF.png KF.png DIFF.png` — premultiplied RGBA difference stats.

The editor cannot write to Wine's `Z:` drive ("lack of access privileges");
use `C:\users\<you>\Documents\…`, which Wine links to `~/Documents`.

Typical comparison:

    tools/blackbox/export.sh 25 'C:\users\me\Documents\…\ref.png'
    kf render model.cmo3 kf.png --scale 0.25 --ss 4 ParamAngleX=17
    python3 tools/blackbox/cmp.py ref.png kf.png diff.png

Outputs go under `test-data/blackbox/` (gitignored: sample-derived).
