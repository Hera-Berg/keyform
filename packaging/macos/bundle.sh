#!/bin/sh
# bundle.sh VERSION X86_64_BINARY AARCH64_BINARY OUTDIR — universal Keyform.app
# and Keyform-VERSION-universal.dmg (macOS: lipo, iconutil, hdiutil).
# Signing and notarization run only when the signing identity is given
# (MACOS_SIGN_IDENTITY, plus NOTARY_* for notarytool).
set -eu
VERSION=$1 X86=$2 ARM=$3 OUT=$4
HERE=$(cd "$(dirname "$0")" && pwd)
ROOT=$(cd "$HERE/../.." && pwd)
WORK=$(mktemp -d)
APP="$WORK/Keyform.app"
mkdir -p "$APP/Contents/MacOS" "$APP/Contents/Resources"
lipo -create -output "$APP/Contents/MacOS/keyform" "$X86" "$ARM"
sed "s/@VERSION@/$VERSION/g" "$HERE/Info.plist" > "$APP/Contents/Info.plist"
ICONSET="$WORK/keyform.iconset"
mkdir -p "$ICONSET"
for n in 16 32 128 256 512; do
  cp "$ROOT/assets/icons/keyform-$n.png" "$ICONSET/icon_${n}x${n}.png"
done
cp "$ROOT/assets/icons/keyform-32.png" "$ICONSET/icon_16x16@2x.png"
cp "$ROOT/assets/icons/keyform-64.png" "$ICONSET/icon_32x32@2x.png"
cp "$ROOT/assets/icons/keyform-256.png" "$ICONSET/icon_128x128@2x.png"
cp "$ROOT/assets/icons/keyform-512.png" "$ICONSET/icon_256x256@2x.png"
cp "$ROOT/assets/icons/keyform.png" "$ICONSET/icon_512x512@2x.png"
iconutil -c icns -o "$APP/Contents/Resources/keyform.icns" "$ICONSET"
if [ -n "${MACOS_SIGN_IDENTITY:-}" ]; then
  codesign --force --options runtime --timestamp --sign "$MACOS_SIGN_IDENTITY" "$APP"
fi
mkdir -p "$OUT"
DMG="$OUT/Keyform-$VERSION-universal.dmg"
hdiutil create -volname Keyform -srcfolder "$APP" -ov -format UDZO "$DMG"
if [ -n "${MACOS_SIGN_IDENTITY:-}" ] && [ -n "${NOTARY_APPLE_ID:-}" ]; then
  codesign --force --timestamp --sign "$MACOS_SIGN_IDENTITY" "$DMG"
  xcrun notarytool submit "$DMG" --apple-id "$NOTARY_APPLE_ID" \
    --team-id "$NOTARY_TEAM_ID" --password "$NOTARY_PASSWORD" --wait
  xcrun stapler staple "$DMG"
fi
