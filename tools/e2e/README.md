# End-to-end tests

A scripted session rigs a character from a PSD by driving Keyform with real
mouse and keyboard input, then checks the outputs. A second script audits
whether a person can reach every command.

All scripted input goes to a **private X display**, never your desktop:

```sh
tools/e2e/display.sh start      # Xvfb :99 when installed (CI), else weston + rootful Xwayland :7
export KEYFORM_E2E_DISPLAY=:7   # the harness refuses any other display
```

The harness needs a **debug** build: the UI map (`KEYFORM_UI_MAP`) and the
scripted file dialogs (`KEYFORM_FILE_DIALOG`) are compiled out of release
builds.

```sh
cargo build -p keyform-app -p keyform-cli
python3 tools/e2e/session.py test-data/e2e/character.psd e2e-out --keyform target/debug/keyform --display :7
python3 tools/e2e/check.py e2e-out --keyform target/debug/keyform --kf target/debug/kf --golden test-data/e2e/golden
python3 tools/e2e/audit.py e2e-out/audit --session e2e-out --display :7 --doc docs/reachability.md --check
python3 tools/e2e/menu_coverage.py e2e-out/audit/audit.json --doc docs/reachability.md   # spec menu coverage section
tools/e2e/display.sh stop
```

| Script | What it does |
|---|---|
| `harness.py` | XTest input, the UI map, honesty checks: only widgets a person could click (visible, unclipped, on top, enabled), shortcuts only after reading them on their menu item |
| `session.py` | the rig, step by step (`report.json` has the steps, their timings and workflow notes; `session.gif`, step PNGs). `--profile FILE.json` maps layer roles for another PSD |
| `check.py` | every output opens, `.cmo3` / `.can3` re-save byte-identically, each parameter moves the render at min and max and moves the right layers, golden snapshots (`--update` rewrites them), physics, motion curves |
| `audit.py` | opens every menu, submenu, context menu, dock tab, combo box and `…` dialog, presses each labelled button once and reads its tooltip, in several states; fails on commands with no visible entry point, menu items never enabled, shortcuts shown nowhere, unlabeled text fields. Hand-written notes: `reachability-notes.tsv`; unfixed gaps: `reachability-known-gaps.tsv` |
| `menu_coverage.py` | sorts the spec menu items that aren't in Keyform's menus (policy / rename / present elsewhere / missing / keyboard-only), with hand review in `menu-coverage-overrides.tsv` |
| `stress.py` | a large PSD through import, Automatic Mesh, atlas, save, export and reopen, with time and peak memory per step |
| `make_character_psd.py` | regenerates the committed original PSD (with an image-resources section, which the reference editor requires) |
| `make_fixtures.sh` | builds one file of each type with `kf` (for `--check-open` in CI) |

Local PSDs that may not be committed (third-party art) are passed by path
and stay outside the repository, with their profiles under `local/`
(gitignored). Their outputs go to a folder outside the repository too.
