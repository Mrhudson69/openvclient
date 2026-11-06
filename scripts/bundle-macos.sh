#!/bin/bash
# Build OpenVClient in release mode and wrap it in dist/OpenVClient.app.
set -euo pipefail
cd "$(dirname "$0")/.."
cargo build --release -p openvclient-app
VERSION=$(grep -m1 '^version' Cargo.toml | cut -d'"' -f2)
# Bundle versions must be numeric: 0.1.0-beta.1 -> 0.1.0.
BUNDLE_VERSION=${VERSION%%-*}
APP=dist/OpenVClient.app
rm -rf "$APP"
mkdir -p "$APP/Contents/MacOS" "$APP/Contents/Resources"
cp target/release/openvclient "$APP/Contents/MacOS/OpenVClient"
cp assets/icon/openvclient.icns "$APP/Contents/Resources/OpenVClient.icns"
cat > "$APP/Contents/Info.plist" <<PLIST
<?xml version="1.0" encoding="UTF-8"?>
<!DOCTYPE plist PUBLIC "-//Apple//DTD PLIST 1.0//EN" "http://www.apple.com/DTDs/PropertyList-1.0.dtd">
<plist version="1.0">
<dict>
  <key>CFBundleName</key><string>OpenVClient</string>
  <key>CFBundleDisplayName</key><string>OpenVClient</string>
  <key>CFBundleIdentifier</key><string>dev.openvclient.client</string>
  <key>CFBundleVersion</key><string>${BUNDLE_VERSION}</string>
  <key>CFBundleShortVersionString</key><string>${BUNDLE_VERSION}</string>
  <key>CFBundleExecutable</key><string>OpenVClient</string>
  <key>CFBundleIconFile</key><string>OpenVClient</string>
  <key>CFBundleInfoDictionaryVersion</key><string>6.0</string>
  <key>NSHumanReadableCopyright</key><string>© OpenVClient contributors · MIT OR Apache-2.0</string>
  <key>LSApplicationCategoryType</key><string>public.app-category.developer-tools</string>
  <key>CFBundlePackageType</key><string>APPL</string>
  <key>LSMinimumSystemVersion</key><string>11.0</string>
  <key>NSHighResolutionCapable</key><true/>
</dict>
</plist>
PLIST
# Ad-hoc signature so Keychain access and Gatekeeper treat the bundle consistently.
codesign --force --deep --sign - "$APP"
echo "Built $APP ($(du -sh "$APP" | cut -f1))"
