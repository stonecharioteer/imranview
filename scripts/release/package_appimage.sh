#!/usr/bin/env bash
set -euo pipefail

VERSION="${1:?usage: package_appimage.sh <version> [dist_dir] [binary_path]}"
DIST_DIR="${2:-dist}"
BIN_PATH="${3:-target/release/imranview}"
APPDIR="$DIST_DIR/AppDir"
OUTFILE="$DIST_DIR/imranview-${VERSION}-linux-x86_64.AppImage"
APPIMAGE_TOOL="$DIST_DIR/appimagetool-x86_64.AppImage"

if [[ ! -f "$BIN_PATH" ]]; then
  echo "AppImage packaging failed: binary not found at $BIN_PATH" >&2
  exit 1
fi

rm -rf "$APPDIR" "$OUTFILE"
mkdir -p \
  "$APPDIR/usr/bin" \
  "$APPDIR/usr/share/applications" \
  "$APPDIR/usr/share/icons/hicolor/256x256/apps"

cp "$BIN_PATH" "$APPDIR/usr/bin/imranview"
chmod +x "$APPDIR/usr/bin/imranview"

cp assets/branding/favicon.png "$APPDIR/imranview.png"
cp assets/branding/favicon.png "$APPDIR/usr/share/icons/hicolor/256x256/apps/imranview.png"

cat > "$APPDIR/AppRun" <<'APPRUN'
#!/bin/sh
HERE="$(dirname "$(readlink -f "$0")")"
exec "$HERE/usr/bin/imranview" "$@"
APPRUN
chmod +x "$APPDIR/AppRun"

cat > "$APPDIR/usr/share/applications/imranview.desktop" <<DESKTOP
[Desktop Entry]
Type=Application
Name=ImranView
Comment=Lightweight desktop image viewer
Exec=imranview %F
Icon=imranview
Terminal=false
Categories=Graphics;Viewer;
StartupNotify=true
DESKTOP

if [[ ! -x "$APPIMAGE_TOOL" ]]; then
  curl -sSL "https://github.com/AppImage/AppImageKit/releases/download/continuous/appimagetool-x86_64.AppImage" -o "$APPIMAGE_TOOL"
  chmod +x "$APPIMAGE_TOOL"
fi

ARCH=x86_64 APPIMAGE_EXTRACT_AND_RUN=1 "$APPIMAGE_TOOL" "$APPDIR" "$OUTFILE" >/dev/null

echo "$OUTFILE"
