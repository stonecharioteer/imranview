#!/usr/bin/env bash
set -euo pipefail

VERSION="${1:?usage: package_dmg.sh <version> [dist_dir] [binary_path]}"
DIST_DIR="${2:-dist}"
BIN_PATH="${3:-target/release/imranview}"
APP_NAME="ImranView"
APP_BUNDLE="$DIST_DIR/macos/$APP_NAME.app"
CONTENTS_DIR="$APP_BUNDLE/Contents"
MACOS_DIR="$CONTENTS_DIR/MacOS"
RESOURCES_DIR="$CONTENTS_DIR/Resources"
ICONSET_DIR="$DIST_DIR/macos/icon.iconset"
ICON_ICNS="$RESOURCES_DIR/ImranView.icns"
DMG_OUT="$DIST_DIR/imranview-${VERSION}-macos.dmg"

if [[ ! -f "$BIN_PATH" ]]; then
  echo "macOS packaging failed: binary not found at $BIN_PATH" >&2
  exit 1
fi

rm -rf "$DIST_DIR/macos" "$DMG_OUT"
mkdir -p "$MACOS_DIR" "$RESOURCES_DIR" "$ICONSET_DIR"

cp "$BIN_PATH" "$MACOS_DIR/imranview"
chmod +x "$MACOS_DIR/imranview"

SOURCE_ICON="assets/branding/favicon.png"
if [[ ! -f "$SOURCE_ICON" ]]; then
  echo "macOS packaging failed: icon not found at $SOURCE_ICON" >&2
  exit 1
fi

for size in 16 32 128 256 512; do
  sips -z "$size" "$size" "$SOURCE_ICON" --out "$ICONSET_DIR/icon_${size}x${size}.png" >/dev/null
  sips -z "$((size * 2))" "$((size * 2))" "$SOURCE_ICON" --out "$ICONSET_DIR/icon_${size}x${size}@2x.png" >/dev/null
done
iconutil -c icns "$ICONSET_DIR" -o "$ICON_ICNS"

cat > "$CONTENTS_DIR/Info.plist" <<PLIST
<?xml version="1.0" encoding="UTF-8"?>
<!DOCTYPE plist PUBLIC "-//Apple//DTD PLIST 1.0//EN" "http://www.apple.com/DTDs/PropertyList-1.0.dtd">
<plist version="1.0">
<dict>
  <key>CFBundleName</key>
  <string>${APP_NAME}</string>
  <key>CFBundleDisplayName</key>
  <string>${APP_NAME}</string>
  <key>CFBundleIdentifier</key>
  <string>com.stonecharioteer.imranview</string>
  <key>CFBundleVersion</key>
  <string>${VERSION}</string>
  <key>CFBundleShortVersionString</key>
  <string>${VERSION}</string>
  <key>CFBundleExecutable</key>
  <string>imranview</string>
  <key>CFBundleIconFile</key>
  <string>ImranView.icns</string>
  <key>CFBundlePackageType</key>
  <string>APPL</string>
  <key>LSMinimumSystemVersion</key>
  <string>11.0</string>
  <key>NSHighResolutionCapable</key>
  <true/>
</dict>
</plist>
PLIST

hdiutil create -volname "$APP_NAME" -srcfolder "$APP_BUNDLE" -ov -format UDZO "$DMG_OUT" >/dev/null

echo "$DMG_OUT"
