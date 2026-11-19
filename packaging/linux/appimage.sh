#!/bin/sh
# appimage.sh VERSION BINARY OUTDIR — builds Keyform-VERSION-x86_64.AppImage.
# Needs `appimagetool` on PATH (the release workflow downloads a pinned one).
set -eu
VERSION=$1 BIN=$2 OUT=$3
HERE=$(cd "$(dirname "$0")" && pwd)
ROOT=$(cd "$HERE/../.." && pwd)
APPDIR=$(mktemp -d)/Keyform.AppDir
mkdir -p "$APPDIR/usr/bin" "$APPDIR/usr/share/applications" "$APPDIR/usr/share/mime/packages"
install -m 755 "$BIN" "$APPDIR/usr/bin/keyform"
install -m 644 "$HERE/keyform.desktop" "$APPDIR/usr/share/applications/keyform.desktop"
install -m 644 "$HERE/keyform-mime.xml" "$APPDIR/usr/share/mime/packages/keyform.xml"
for n in 16 32 48 64 128 256 512; do
  d="$APPDIR/usr/share/icons/hicolor/${n}x${n}/apps"
  mkdir -p "$d"
  install -m 644 "$ROOT/assets/icons/keyform-$n.png" "$d/keyform.png"
done
cp "$HERE/keyform.desktop" "$APPDIR/keyform.desktop"
cp "$ROOT/assets/icons/keyform-256.png" "$APPDIR/keyform.png"
cat > "$APPDIR/AppRun" <<'RUN'
#!/bin/sh
HERE=$(dirname "$(readlink -f "$0")")
exec "$HERE/usr/bin/keyform" "$@"
RUN
chmod 755 "$APPDIR/AppRun"
mkdir -p "$OUT"
ARCH=x86_64 appimagetool --no-appstream "$APPDIR" "$OUT/Keyform-$VERSION-x86_64.AppImage"
