# Keyform

A clean-room 2D-mesh character rigging and animation editor, compatible with
Live2D Cubism Editor 5.3 project files (`.cmo3`, `.can3`) and runtime exports
(`.moc3` + JSON). Implemented only from the written specification in
`../keyform-spec/` (see `CLEANROOM.md` there) and black-box observation.

## Layout

| Crate | Purpose |
|---|---|
| `keyform-model` | data model (parameters, parts, deformers, art meshes, glue, keyform grids, blend shapes) — no GPU, no UI |
| `keyform-eval` | deterministic CPU evaluation: keyform interpolation, blend shapes, warp/rotation deformers, glue, draw order |
| `keyform-formats` | CAFF container, lossless XML object graph, `.cmo3` loader, moc3 reader/writer/import, runtime JSON, PNG, texture cleanup |
| `keyform-render` | rendering: deterministic CPU reference rasterizer and the wgpu renderer (feature `gpu`, default), kept pixel-equivalent by tests |
| `keyform-cli` | `kf` command-line tool |
| `fuzz/` | cargo-fuzz targets (outside the workspace, nightly) |

`keyform-eval` and `keyform-formats` have no GPU or UI dependencies.

## Build and test

    cargo build --release
    cargo test --workspace
    cargo clippy --workspace --all-targets

Sample models are licensed test inputs and are **not** in this repository
(`../keyform-spec/test-data/README.md`). Point the tests at local copies with
`KEYFORM_SAMPLES` (colon-separated directories; default: repository root):

    KEYFORM_SAMPLES=/path/to/samples:/path/to/runtime cargo test --release

With samples present, the tests check: byte-exact `main.xml` round trip of
every `.cmo3`/`.can3`; byte-exact raw moc3 round trip; Keyform's moc3 export
and `cdi3.json` equal to reference exports found next to the project; texture
cleanup bit-exact on locked reference pages; and mutation robustness of
every parser. `tests/gpu_vs_cpu.rs` checks the GPU renderer against the CPU
renderer on a synthetic scene (skipped without a GPU adapter).

## `kf` tool

    kf info model.cmo3
    kf roundtrip in.cmo3 out.cmo3
    kf export model.cmo3 out-dir [version]       # moc3 + textures + cdi3 + model3.json
    kf export model.cmo3 model.moc3 [version]    # moc3 only
    kf render model.cmo3|.moc3|.model3.json out.png [--size N] [--ss N] [--bg] [--gpu] [ParamId=value …]
    kf gpudiff model.cmo3 diff.png [render options]   # CPU vs GPU renderer
    kf sweep model.cmo3 sheet.png ParamAngleX 5 [--size N]
    kf compare a.cmo3 b.moc3 [samples]           # evaluate both, report geometry differences
    kf diffmoc a.moc3 b.moc3                     # section-by-section moc3 diff

## Assets

Keyform uses only its own assets (`assets/`). Nothing from the reference
product is copied.
