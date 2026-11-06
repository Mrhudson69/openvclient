#!/bin/bash
# Build dist/OpenVClient-<version>-macos-<arch>.dmg: the familiar
# "drag OpenVClient to Applications" installer window, with background,
# Applications shortcut, fixed icon layout and a volume icon.
#
#   ./scripts/package-macos-dmg.sh            # builds the .app first
#   SKIP_BUILD=1 APP=path/to/OpenVClient.app DMG_ARCH=universal ./scripts/package-macos-dmg.sh
#
# Signing and notarization (optional, used by the release workflow):
#   MACOS_SIGN_IDENTITY="Developer ID Application: …"  sign app + dmg
#   APPLE_ID / APPLE_TEAM_ID / APPLE_APP_PASSWORD      notarize and staple
set -euo pipefail
cd "$(dirname "$0")/.."
ROOT=$(pwd)

APP=${APP:-dist/OpenVClient.app}
# Build the bundle unless SKIP_BUILD=1 (CI passes a prebuilt universal bundle).
if [ -z "${SKIP_BUILD:-}" ]; then
  ./scripts/bundle-macos.sh
fi
[ -d "$APP" ] || { echo "No app bundle at $APP" >&2; exit 1; }

VERSION=$(grep -m1 '^version' Cargo.toml | cut -d'"' -f2)
ARCH=${DMG_ARCH:-$(uname -m | sed 's/x86_64/x86_64/;s/arm64/aarch64/')}
DMG="dist/OpenVClient-${VERSION}-macos-${ARCH}.dmg"

if [ -n "${MACOS_SIGN_IDENTITY:-}" ]; then
  echo "Signing $APP with $MACOS_SIGN_IDENTITY"
  codesign --force --deep --options runtime --timestamp --sign "$MACOS_SIGN_IDENTITY" "$APP"
fi

# dmgbuild writes the Finder layout directly (no Finder/AppleScript needed),
# so this works headless and in CI. Installed into a private venv if missing.
DMGBUILD=$(command -v dmgbuild || true)
if [ -z "$DMGBUILD" ]; then
  VENV="target/dmg-venv"
  if [ ! -x "$VENV/bin/dmgbuild" ]; then
    python3 -m venv "$VENV"
    "$VENV/bin/pip" install --quiet "dmgbuild==1.6.5"
  fi
  DMGBUILD="$VENV/bin/dmgbuild"
fi

rm -f "$DMG"
"$DMGBUILD" -s scripts/dmg-settings.py -D app="$APP" -D root="$ROOT" "OpenVClient ${VERSION}" "$DMG"

if [ -n "${MACOS_SIGN_IDENTITY:-}" ]; then
  codesign --force --timestamp --sign "$MACOS_SIGN_IDENTITY" "$DMG"
  if [ -n "${APPLE_ID:-}" ] && [ -n "${APPLE_TEAM_ID:-}" ] && [ -n "${APPLE_APP_PASSWORD:-}" ]; then
    xcrun notarytool submit "$DMG" --apple-id "$APPLE_ID" --team-id "$APPLE_TEAM_ID" --password "$APPLE_APP_PASSWORD" --wait
    xcrun stapler staple "$DMG"
  fi
fi

shasum -a 256 "$DMG" > "$DMG.sha256"
echo "Built $DMG ($(du -h "$DMG" | cut -f1))"
