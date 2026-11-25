#!/bin/bash
# Build "Rust Terminal.app" (and a .dmg) for macOS.
#
#   tools/macos/bundle.sh            # this Mac's architecture
#   UNIVERSAL=1 tools/macos/bundle.sh  # Apple Silicon + Intel in one app
#                                     # (needs: rustup target add x86_64-apple-darwin aarch64-apple-darwin)
#
# Signing: ad-hoc by default (runs on this Mac). For distribution set
#   SIGN_ID="Developer ID Application: Your Name (TEAMID)"
# and to notarize also NOTARY_PROFILE=<profile created with `xcrun notarytool store-credentials`>.
set -euo pipefail
cd "$(dirname "$0")/../.."

NAME="Rust Terminal"
BIN=rust-terminal
VERSION=$(grep -m1 '^version' Cargo.toml | cut -d'"' -f2)
OUT=target/macos
APP="$OUT/$NAME.app"

if [ "${UNIVERSAL:-0}" = "1" ]; then
  cargo build --release --target aarch64-apple-darwin
  cargo build --release --target x86_64-apple-darwin
  mkdir -p target/release
  lipo -create -output target/release/$BIN \
    target/aarch64-apple-darwin/release/$BIN target/x86_64-apple-darwin/release/$BIN
else
  cargo build --release
fi

rm -rf "$APP"
mkdir -p "$APP/Contents/MacOS" "$APP/Contents/Resources"
cp target/release/$BIN "$APP/Contents/MacOS/$BIN"

# icon
ICONSET="$OUT/icon.iconset"
rm -rf "$ICONSET"; mkdir -p "$ICONSET"
for s in 16 32 128 256 512; do
  sips -z $s $s assets/icon.png --out "$ICONSET/icon_${s}x${s}.png" >/dev/null
  d=$((s * 2))
  sips -z $d $d assets/icon.png --out "$ICONSET/icon_${s}x${s}@2x.png" >/dev/null
done
iconutil -c icns "$ICONSET" -o "$APP/Contents/Resources/icon.icns"
rm -rf "$ICONSET"

cat > "$APP/Contents/Info.plist" <<PLIST
<?xml version="1.0" encoding="UTF-8"?>
<!DOCTYPE plist PUBLIC "-//Apple//DTD PLIST 1.0//EN" "http://www.apple.com/DTDs/PropertyList-1.0.dtd">
<plist version="1.0">
<dict>
  <key>CFBundleName</key><string>$NAME</string>
  <key>CFBundleDisplayName</key><string>$NAME</string>
  <key>CFBundleIdentifier</key><string>dev.rustterminal.app</string>
  <key>CFBundleVersion</key><string>$VERSION</string>
  <key>CFBundleShortVersionString</key><string>$VERSION</string>
  <key>CFBundleExecutable</key><string>$BIN</string>
  <key>CFBundleIconFile</key><string>icon</string>
  <key>CFBundlePackageType</key><string>APPL</string>
  <key>LSMinimumSystemVersion</key><string>11.0</string>
  <key>NSHighResolutionCapable</key><true/>
  <key>LSApplicationCategoryType</key><string>public.app-category.finance</string>
</dict>
</plist>
PLIST

ID="${SIGN_ID:--}"
if [ "$ID" = "-" ]; then
  codesign --force --deep --sign - "$APP"
else
  codesign --force --deep --options runtime --timestamp --sign "$ID" "$APP"
fi

DMG="$OUT/RustTerminal-$VERSION.dmg"
rm -f "$DMG"
hdiutil create -volname "$NAME" -srcfolder "$APP" -ov -format UDZO "$DMG" >/dev/null
if [ -n "${NOTARY_PROFILE:-}" ] && [ "$ID" != "-" ]; then
  codesign --sign "$ID" --timestamp "$DMG"
  xcrun notarytool submit "$DMG" --keychain-profile "$NOTARY_PROFILE" --wait
  xcrun stapler staple "$DMG"
fi

echo "app: $APP"
echo "dmg: $DMG"
