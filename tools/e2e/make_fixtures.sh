#!/bin/sh
# make_fixtures.sh KF OUTDIR — builds files from the committed original
# character PSD with the `kf` tool: character.cmo3 (with a texture atlas),
# character.can3 and the runtime set in OUTDIR/runtime. Used by CI and the
# release workflow to check the binaries open each file type.
set -eu
KF=$1 OUT=$2
ROOT=$(cd "$(dirname "$0")/../.." && pwd)
mkdir -p "$OUT"
"$KF" newpsd "$ROOT/test-data/e2e/character.psd" "$OUT/imported.cmo3"
"$KF" atlas "$OUT/imported.cmo3" "$OUT/character.cmo3" new 1024x1024 visible
"$KF" newanim "$OUT/character.cmo3" "$OUT/character.can3"
"$KF" export "$OUT/character.cmo3" "$OUT/runtime"
rm -f "$OUT/imported.cmo3"
ls -R "$OUT"
