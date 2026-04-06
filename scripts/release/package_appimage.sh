#!/usr/bin/env bash
set -euo pipefail

VERSION="${1:?usage: package_appimage.sh <version> [dist_dir] [binary_path]}"
DIST_DIR="${2:-dist}"
BIN_PATH="${3:-target/release/imranview}"
APPDIR="$DIST_DIR/AppDir"
OUTFILE="$DIST_DIR/imranview-${VERSION}-linux-x86_64.AppImage"
APPIMAGE_TOOL="$DIST_DIR/appimagetool-x86_64.AppImage"
APP_BIN="$APPDIR/usr/bin/imranview"
APP_LIB_DIR="$APPDIR/usr/lib"

if [[ ! -f "$BIN_PATH" ]]; then
  echo "AppImage packaging failed: binary not found at $BIN_PATH" >&2
  exit 1
fi

rm -rf "$APPDIR" "$OUTFILE"
mkdir -p \
  "$APPDIR/usr/bin" \
  "$APPDIR/usr/lib" \
  "$APPDIR/usr/share" \
  "$APPDIR/usr/share/applications" \
  "$APPDIR/usr/share/icons/hicolor/256x256/apps"

cp "$BIN_PATH" "$APP_BIN"
chmod +x "$APP_BIN"

cp assets/branding/favicon.png "$APPDIR/imranview.png"
cp assets/branding/favicon.png "$APPDIR/usr/share/icons/hicolor/256x256/apps/imranview.png"

copy_runtime_deps() {
  local target="$1"
  if [[ ! -e "$target" ]]; then
    return
  fi

  while IFS= read -r line; do
    local dep=""
    if [[ "$line" == *"=>"* ]]; then
      dep="$(awk '{print $3}' <<<"$line")"
    else
      dep="$(awk '{print $1}' <<<"$line")"
    fi
    if [[ -z "$dep" || "$dep" == "not" || "$dep" == "linux-vdso.so.1" ]]; then
      continue
    fi
    if [[ ! -f "$dep" ]]; then
      continue
    fi

    local base
    base="$(basename "$dep")"
    case "$base" in
      ld-linux*|libc.so.*|libm.so.*|libpthread.so.*|librt.so.*|libdl.so.*)
        continue
        ;;
    esac

    cp -L "$dep" "$APP_LIB_DIR/$base"
  done < <(ldd "$target" 2>/dev/null || true)
}

copy_runtime_deps "$APP_BIN"

cat > "$APPDIR/AppRun" <<'APPRUN'
#!/bin/sh
HERE="$(dirname "$(readlink -f "$0")")"
export LD_LIBRARY_PATH="$HERE/usr/lib${LD_LIBRARY_PATH:+:$LD_LIBRARY_PATH}"
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
