#!/bin/sh
# test-desktop-integration.sh KEYFORM_BINARY SAMPLE_FILE
# Installs keyform.desktop and the MIME types into a private XDG data dir,
# then checks that a .cmo3 / .can3 / .moc3 maps to Keyform via xdg-mime and
# that the desktop file's Exec line hands the file to the binary as argv
# (what a double-click runs), using the hidden --check-open flag.
set -eu
BIN=$(readlink -f "$1") SAMPLE=$(readlink -f "$2")
HERE=$(cd "$(dirname "$0")" && pwd)
T=$(mktemp -d)
trap 'rm -rf "$T"' EXIT
# GNOME makes xdg-mime use GIO (shared-mime-info globs), as desktops do.
export XDG_CURRENT_DESKTOP=GNOME XDG_DATA_HOME="$T/data" XDG_CONFIG_HOME="$T/config" XDG_DATA_DIRS="$T/data:/usr/share"
mkdir -p "$XDG_DATA_HOME/applications" "$XDG_DATA_HOME/mime/packages" "$XDG_CONFIG_HOME"
desktop-file-validate "$HERE/keyform.desktop"
cp "$HERE/keyform.desktop" "$XDG_DATA_HOME/applications/"
cp "$HERE/keyform-mime.xml" "$XDG_DATA_HOME/mime/packages/keyform.xml"
update-mime-database "$XDG_DATA_HOME/mime"
update-desktop-database "$XDG_DATA_HOME/applications" 2>/dev/null || true
fail=0
for ext in cmo3 can3 moc3; do
  f="$T/sample.$ext"; printf 'CAFF\000\001\002\003\377\376' > "$f"
  type=$(xdg-mime query filetype "$f")
  [ "$type" = "application/x-keyform-$ext" ] || { echo "FAIL .$ext → $type"; fail=1; continue; }
  xdg-mime default keyform.desktop "$type"
  app=$(xdg-mime query default "$type")
  [ "$app" = "keyform.desktop" ] || { echo "FAIL $type → $app"; fail=1; continue; }
  echo "ok .$ext → $type → $app"
done
# The Exec line with %F replaced by the file, run with --check-open.
exec_line=$(sed -n 's/^Exec=//p' "$HERE/keyform.desktop")
cmd=$(printf '%s' "$exec_line" | sed "s|^keyform|\"$BIN\" --check-open|; s|%F|\"$SAMPLE\"|")
echo "running: $cmd"
sh -c "$cmd" || fail=1
exit $fail
