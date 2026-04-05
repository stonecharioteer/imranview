#!/usr/bin/env bash
set -euo pipefail

TARGET="${1:-$(rustc -vV | awk '/host: /{print $2}')}"
DIST_DIR="dist"
BIN_NAME="imranview"

cargo build --release --target "$TARGET"

mkdir -p "$DIST_DIR"
if [[ "$TARGET" == *"windows"* ]]; then
  BIN_NAME="imranview.exe"
fi

BIN_PATH="target/$TARGET/release/$BIN_NAME"
if [[ ! -f "$BIN_PATH" ]]; then
  echo "packaging failed: binary not found at $BIN_PATH" >&2
  exit 1
fi

if [[ "$TARGET" == *"windows"* ]]; then
  ARCHIVE="$DIST_DIR/imranview-$TARGET.zip"
  if command -v zip >/dev/null 2>&1; then
    zip -j "$ARCHIVE" "$BIN_PATH"
  else
    echo "zip command not found; cannot package windows artifact locally" >&2
    exit 1
  fi
else
  ARCHIVE="$DIST_DIR/imranview-$TARGET.tar.gz"
  tar -czf "$ARCHIVE" -C "target/$TARGET/release" "$BIN_NAME"
fi

echo "packaged artifact: $ARCHIVE"
