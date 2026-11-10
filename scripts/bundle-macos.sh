#!/usr/bin/env bash
# Assembles dist/YADM.app with the bundled aria2c sidecar and browser extension.
# Run scripts/build-aria2.sh first (or set ARIA2C=/path/to/aria2c).
set -euo pipefail

ROOT="$(cd "$(dirname "$0")/.." && pwd)"
cd "$ROOT"
ARIA2C="${ARIA2C:-$ROOT/vendor/aria2c}"
[ -x "$ARIA2C" ] || { echo "aria2c not found at $ARIA2C — run scripts/build-aria2.sh" >&2; exit 1; }

VERSION="$(grep -m1 '^version' Cargo.toml | cut -d'"' -f2)"
cargo build --release -p yadm-app

APP="$ROOT/dist/YADM.app"
rm -rf "$APP"
mkdir -p "$APP/Contents/MacOS" "$APP/Contents/Resources"
cp target/release/yadm "$APP/Contents/MacOS/YADM"
cp "$ARIA2C" "$APP/Contents/MacOS/aria2c"
cp -R extension "$APP/Contents/Resources/extension"

# App icon (assets/icon.svg, rasterized to assets/icon.png).
ICONSET="$(mktemp -d)/YADM.iconset"
mkdir -p "$ICONSET"
for s in 16 32 128 256 512; do
  sips -z $s $s assets/icon.png --out "$ICONSET/icon_${s}x${s}.png" >/dev/null
  sips -z $((s*2)) $((s*2)) assets/icon.png --out "$ICONSET/icon_${s}x${s}@2x.png" >/dev/null
done
iconutil -c icns "$ICONSET" -o "$APP/Contents/Resources/YADM.icns"

cat > "$APP/Contents/Info.plist" <<PLIST
<?xml version="1.0" encoding="UTF-8"?>
<!DOCTYPE plist PUBLIC "-//Apple//DTD PLIST 1.0//EN" "http://www.apple.com/DTDs/PropertyList-1.0.dtd">
<plist version="1.0">
<dict>
  <key>CFBundleName</key><string>YADM</string>
  <key>CFBundleDisplayName</key><string>YADM</string>
  <key>CFBundleIdentifier</key><string>dev.yadm.app</string>
  <key>CFBundleExecutable</key><string>YADM</string>
  <key>CFBundleIconFile</key><string>YADM</string>
  <key>CFBundlePackageType</key><string>APPL</string>
  <key>CFBundleShortVersionString</key><string>$VERSION</string>
  <key>CFBundleVersion</key><string>$VERSION</string>
  <key>LSMinimumSystemVersion</key><string>12.0</string>
  <key>NSHighResolutionCapable</key><true/>
  <key>LSApplicationCategoryType</key><string>public.app-category.utilities</string>
  <key>CFBundleDocumentTypes</key>
  <array>
    <dict>
      <key>CFBundleTypeName</key><string>BitTorrent Document</string>
      <key>CFBundleTypeRole</key><string>Viewer</string>
      <key>LSItemContentTypes</key><array><string>org.bittorrent.torrent</string></array>
      <key>CFBundleTypeExtensions</key><array><string>torrent</string></array>
    </dict>
  </array>
</dict>
</plist>
PLIST

codesign --force --deep --sign - "$APP"
echo "Built $APP"
