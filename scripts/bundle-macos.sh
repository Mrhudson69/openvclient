#!/bin/bash
# Build OpenVClient in release mode and wrap it in dist/OpenVClient.app.
set -euo pipefail
cd "$(dirname "$0")/.."
cargo build --release -p openvclient-app
VERSION=$(grep -m1 '^version' Cargo.toml | cut -d'"' -f2)
APP=dist/OpenVClient.app
rm -rf "$APP"
mkdir -p "$APP/Contents/MacOS" "$APP/Contents/Resources"
cp target/release/openvclient "$APP/Contents/MacOS/OpenVClient"
cat > "$APP/Contents/Info.plist" <<PLIST
<?xml version="1.0" encoding="UTF-8"?>
<!DOCTYPE plist PUBLIC "-//Apple//DTD PLIST 1.0//EN" "http://www.apple.com/DTDs/PropertyList-1.0.dtd">
<plist version="1.0">
<dict>
  <key>CFBundleName</key><string>OpenVClient</string>
  <key>CFBundleDisplayName</key><string>OpenVClient</string>
  <key>CFBundleIdentifier</key><string>dev.openvclient.client</string>
  <key>CFBundleVersion</key><string>${VERSION}</string>
  <key>CFBundleShortVersionString</key><string>${VERSION}</string>
  <key>CFBundleExecutable</key><string>OpenVClient</string>
  <key>CFBundlePackageType</key><string>APPL</string>
  <key>LSMinimumSystemVersion</key><string>11.0</string>
  <key>NSHighResolutionCapable</key><true/>
  <key>LSApplicationCategoryType</key><string>public.app-category.developer-tools</string>
</dict>
</plist>
PLIST
# Ad-hoc signature so Keychain access and Gatekeeper treat the bundle consistently.
codesign --force --deep --sign - "$APP"
echo "Built $APP ($(du -sh "$APP" | cut -f1))"
