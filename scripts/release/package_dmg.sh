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
FRAMEWORKS_DIR="$CONTENTS_DIR/Frameworks"
ICONSET_DIR="$DIST_DIR/macos/icon.iconset"
ICON_ICNS="$RESOURCES_DIR/ImranView.icns"
DMG_OUT="$DIST_DIR/imranview-${VERSION}-macos.dmg"
APP_BIN="$MACOS_DIR/imranview-bin"
APP_LAUNCHER="$MACOS_DIR/imranview"

if [[ ! -f "$BIN_PATH" ]]; then
  echo "macOS packaging failed: binary not found at $BIN_PATH" >&2
  exit 1
fi

if ! command -v brew >/dev/null 2>&1; then
  echo "macOS packaging failed: Homebrew is required to resolve runtime libraries" >&2
  exit 1
fi

HOMEBREW_PREFIX="$(brew --prefix)"

rm -rf "$DIST_DIR/macos" "$DMG_OUT"
mkdir -p "$MACOS_DIR" "$RESOURCES_DIR" "$FRAMEWORKS_DIR" "$ICONSET_DIR"

cp "$BIN_PATH" "$APP_BIN"
chmod +x "$APP_BIN"

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

resolve_dylib_path() {
  local dep="$1"
  local base
  base="$(basename "$dep")"

  if [[ "$dep" = /* && -f "$dep" ]]; then
    echo "$dep"
    return 0
  fi
  if [[ -f "$FRAMEWORKS_DIR/$base" ]]; then
    echo "$FRAMEWORKS_DIR/$base"
    return 0
  fi

  local candidate
  for candidate in \
    "$HOMEBREW_PREFIX/lib/$base" \
    "$HOMEBREW_PREFIX/opt/glib/lib/$base" \
    "$HOMEBREW_PREFIX/opt/jpeg-turbo/lib/$base" \
    "$HOMEBREW_PREFIX/opt/gettext/lib/$base"; do
    if [[ -f "$candidate" ]]; then
      echo "$candidate"
      return 0
    fi
  done

  candidate="$(find "$HOMEBREW_PREFIX/Cellar" -name "$base" -type f 2>/dev/null | head -n 1 || true)"
  if [[ -n "$candidate" && -f "$candidate" ]]; then
    echo "$candidate"
    return 0
  fi
  return 1
}

bundle_dylib_tree() {
  local root_binary="$1"
  local -a queue=("$root_binary")
  local current dep resolved bundled base
  declare -A seen

  while (( ${#queue[@]} > 0 )); do
    current="${queue[0]}"
    queue=("${queue[@]:1}")

    local key
    key="$(realpath "$current" 2>/dev/null || echo "$current")"
    if [[ -n "${seen[$key]:-}" ]]; then
      continue
    fi
    seen[$key]=1

    while IFS= read -r dep; do
      [[ -z "$dep" ]] && continue
      if [[ "$dep" == /System/* || "$dep" == /usr/lib/* ]]; then
        continue
      fi

      if ! resolved="$(resolve_dylib_path "$dep")"; then
        echo "warning: unable to resolve dylib dependency $dep referenced by $current" >&2
        continue
      fi

      base="$(basename "$resolved")"
      bundled="$FRAMEWORKS_DIR/$base"
      if [[ ! -f "$bundled" ]]; then
        cp -L "$resolved" "$bundled"
        chmod u+w "$bundled" || true
        queue+=("$bundled")
      fi

      if [[ "$current" == "$root_binary" ]]; then
        install_name_tool -change "$dep" "@executable_path/../Frameworks/$base" "$current" || true
      else
        install_name_tool -change "$dep" "@loader_path/$base" "$current" || true
      fi
    done < <(otool -L "$current" | tail -n +2 | awk '{print $1}')

    if [[ "$current" != "$root_binary" ]]; then
      install_name_tool -id "@loader_path/$(basename "$current")" "$current" || true
    fi
  done
}

bundle_dylib_tree "$APP_BIN"

cat > "$APP_LAUNCHER" <<'LAUNCHER'
#!/bin/sh
HERE="$(cd "$(dirname "$0")" && pwd)"
CONTENTS_DIR="$(cd "$HERE/.." && pwd)"

export DYLD_LIBRARY_PATH="$CONTENTS_DIR/Frameworks${DYLD_LIBRARY_PATH:+:$DYLD_LIBRARY_PATH}"

exec "$HERE/imranview-bin" "$@"
LAUNCHER
chmod +x "$APP_LAUNCHER"

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
