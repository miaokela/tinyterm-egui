#!/usr/bin/env bash
# Build a proper macOS .app bundle so TinyTerm gets a real Dock icon.
#
# winit ignores `set_window_icon` on macOS — the Dock/Finder icon only comes from
# a bundle whose Info.plist points at an .icns. This script assembles one from
# the release binary and the vendored artwork.
set -euo pipefail

cd "$(dirname "$0")/.."
ROOT="$(pwd)"
APP_NAME="TinyTerm"
BUNDLE="$ROOT/target/release/$APP_NAME.app"

echo "==> building release binary"
cargo build --release

echo "==> assembling $BUNDLE"
rm -rf "$BUNDLE"
mkdir -p "$BUNDLE/Contents/MacOS" "$BUNDLE/Contents/Resources"

cp "$ROOT/target/release/tinyterm-egui" "$BUNDLE/Contents/MacOS/$APP_NAME"
cp "$ROOT/assets/icon.icns" "$BUNDLE/Contents/Resources/AppIcon.icns"

# Keep the bundle version in step with the crate (the CI job does the same).
VERSION="$(sed -n 's/^version = "\(.*\)"/\1/p' "$ROOT/Cargo.toml" | head -1)"

cat > "$BUNDLE/Contents/Info.plist" <<PLIST
<?xml version="1.0" encoding="UTF-8"?>
<!DOCTYPE plist PUBLIC "-//Apple//DTD PLIST 1.0//EN" "http://www.apple.com/DTDs/PropertyList-1.0.dtd">
<plist version="1.0">
<dict>
  <key>CFBundleName</key><string>$APP_NAME</string>
  <key>CFBundleDisplayName</key><string>$APP_NAME</string>
  <key>CFBundleExecutable</key><string>$APP_NAME</string>
  <key>CFBundleIdentifier</key><string>com.tinyterm.egui</string>
  <key>CFBundleIconFile</key><string>AppIcon</string>
  <key>CFBundlePackageType</key><string>APPL</string>
  <key>CFBundleShortVersionString</key><string>$VERSION</string>
  <key>CFBundleVersion</key><string>1</string>
  <key>LSMinimumSystemVersion</key><string>11.0</string>
  <key>NSHighResolutionCapable</key><true/>
</dict>
</plist>
PLIST

echo "==> creating DMG"
DMG="$ROOT/target/release/$APP_NAME.dmg"
"$ROOT/scripts/make-dmg.sh" "$BUNDLE" "$DMG" "$APP_NAME"

echo "==> done"
echo "    app: $BUNDLE"
echo "    dmg: $DMG"
echo "    open \"$DMG\""
