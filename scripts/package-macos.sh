#!/usr/bin/env bash
set -euo pipefail

if [[ $# -ne 3 ]]; then
  echo "usage: $0 <version> <binary> <output-dir>" >&2
  exit 2
fi

VERSION="${1#v}"
BUNDLE_VERSION="${VERSION%%[-+]*}"
BINARY="$(cd "$(dirname "$2")" && pwd)/$(basename "$2")"
OUTPUT_DIR="$(mkdir -p "$3" && cd "$3" && pwd)"
ROOT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
STEM="zeff-boy-v${VERSION}-aarch64-apple-darwin"

if [[ ! "$VERSION" =~ ^[0-9]+\.[0-9]+\.[0-9]+([.-][0-9A-Za-z]+)*$ ]]; then
  echo "invalid version: $VERSION" >&2
  exit 2
fi
if [[ ! -x "$BINARY" ]]; then
  echo "binary is missing or not executable: $BINARY" >&2
  exit 2
fi

WORK_DIR="$(mktemp -d)"
MOUNT_DIR=""
cleanup() {
  if [[ -n "$MOUNT_DIR" && -d "$MOUNT_DIR" ]]; then
    hdiutil detach -quiet "$MOUNT_DIR" >/dev/null 2>&1 || true
  fi
  rm -rf "$WORK_DIR"
}
trap cleanup EXIT
APP="$WORK_DIR/Zeff Boy.app"
CONTENTS="$APP/Contents"
mkdir -p "$CONTENTS/MacOS" "$CONTENTS/Resources/licenses"
install -m755 "$BINARY" "$CONTENTS/MacOS/zeff-boy"
install -m644 "$ROOT_DIR/LICENSE-MIT" "$CONTENTS/Resources/licenses/LICENSE-MIT"
install -m644 "$ROOT_DIR/LICENSE-APACHE" "$CONTENTS/Resources/licenses/LICENSE-APACHE"

ICONSET="$WORK_DIR/zeff-boy.iconset"
mkdir -p "$ICONSET"
for size in 16 32 128 256 512; do
  sips -z "$size" "$size" "$ROOT_DIR/assets/icon.png" \
    --out "$ICONSET/icon_${size}x${size}.png" >/dev/null
done
for size in 16 32 128 256 512; do
  doubled=$((size * 2))
  sips -z "$doubled" "$doubled" "$ROOT_DIR/assets/icon.png" \
    --out "$ICONSET/icon_${size}x${size}@2x.png" >/dev/null
done
iconutil -c icns "$ICONSET" -o "$CONTENTS/Resources/zeff-boy.icns"

cat > "$CONTENTS/Info.plist" <<EOF
<?xml version="1.0" encoding="UTF-8"?>
<!DOCTYPE plist PUBLIC "-//Apple//DTD PLIST 1.0//EN" "https://www.apple.com/DTDs/PropertyList-1.0.dtd">
<plist version="1.0">
<dict>
  <key>CFBundleDisplayName</key><string>Zeff Boy</string>
  <key>CFBundleExecutable</key><string>zeff-boy</string>
  <key>CFBundleIconFile</key><string>zeff-boy</string>
  <key>CFBundleIdentifier</key><string>com.github.zeffuro.zeff-boy</string>
  <key>CFBundleInfoDictionaryVersion</key><string>6.0</string>
  <key>CFBundleName</key><string>Zeff Boy</string>
  <key>CFBundlePackageType</key><string>APPL</string>
  <key>CFBundleShortVersionString</key><string>$BUNDLE_VERSION</string>
  <key>CFBundleVersion</key><string>$BUNDLE_VERSION</string>
  <key>LSMinimumSystemVersion</key><string>11.0</string>
  <key>NSCameraUsageDescription</key><string>Camera access is used for emulated camera peripherals.</string>
  <key>NSHighResolutionCapable</key><true/>
</dict>
</plist>
EOF

codesign --force --deep --sign - "$APP"
plutil -lint "$CONTENTS/Info.plist"
codesign --verify --deep --strict "$APP"
[[ "$(lipo -archs "$CONTENTS/MacOS/zeff-boy")" == *arm64* ]]

DMG_ROOT="$WORK_DIR/dmg"
mkdir -p "$DMG_ROOT"
ditto "$APP" "$DMG_ROOT/Zeff Boy.app"
ln -s /Applications "$DMG_ROOT/Applications"
hdiutil create -quiet -fs HFS+ -format UDZO -volname "Zeff Boy" \
  -srcfolder "$DMG_ROOT" "$OUTPUT_DIR/${STEM}.dmg"
ditto -c -k --sequesterRsrc --keepParent "$APP" "$OUTPUT_DIR/${STEM}.zip"

hdiutil verify "$OUTPUT_DIR/${STEM}.dmg"
unzip -t "$OUTPUT_DIR/${STEM}.zip" >/dev/null

MOUNT_DIR="$WORK_DIR/mounted"
mkdir -p "$MOUNT_DIR"
hdiutil attach -quiet -readonly -nobrowse -mountpoint "$MOUNT_DIR" \
  "$OUTPUT_DIR/${STEM}.dmg"
MOUNTED_APP="$MOUNT_DIR/Zeff Boy.app"
for path in \
  "Contents/Info.plist" \
  "Contents/MacOS/zeff-boy" \
  "Contents/Resources/zeff-boy.icns" \
  "Contents/Resources/licenses/LICENSE-MIT" \
  "Contents/Resources/licenses/LICENSE-APACHE"; do
  test -e "$MOUNTED_APP/$path"
done
plutil -lint "$MOUNTED_APP/Contents/Info.plist"
codesign --verify --deep --strict "$MOUNTED_APP"
[[ "$(lipo -archs "$MOUNTED_APP/Contents/MacOS/zeff-boy")" == *arm64* ]]
hdiutil detach -quiet "$MOUNT_DIR"
MOUNT_DIR=""

ZIP_DIR="$WORK_DIR/unzipped"
mkdir -p "$ZIP_DIR"
ditto -x -k "$OUTPUT_DIR/${STEM}.zip" "$ZIP_DIR"
test -x "$ZIP_DIR/Zeff Boy.app/Contents/MacOS/zeff-boy"
codesign --verify --deep --strict "$ZIP_DIR/Zeff Boy.app"
