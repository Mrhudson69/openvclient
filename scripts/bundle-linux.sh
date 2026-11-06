#!/bin/bash
# Build OpenVClient for Linux and package it as a tar.gz, plus a .deb when
# dpkg-deb is available.
#
#   ./scripts/bundle-linux.sh                 # host target, release build
#   TARGET=aarch64-unknown-linux-gnu ./scripts/bundle-linux.sh
#   SKIP_BUILD=1 BIN=path/to/openvclient ./scripts/bundle-linux.sh
#
# Output: dist/openvclient-<version>-linux-<arch>.tar.gz
#         dist/openvclient_<version>_<debarch>.deb
set -euo pipefail
cd "$(dirname "$0")/.."

VERSION=$(grep -m1 '^version' Cargo.toml | cut -d'"' -f2)
TARGET=${TARGET:-}
if [[ -z "${SKIP_BUILD:-}" ]]; then
  if [[ -n "$TARGET" ]]; then
    cargo build --release --locked -p openvclient-app --target "$TARGET"
  else
    cargo build --release --locked -p openvclient-app
  fi
fi
if [[ -n "$TARGET" ]]; then
  BIN=${BIN:-target/$TARGET/release/openvclient}
else
  BIN=${BIN:-target/release/openvclient}
fi
[[ -x "$BIN" ]] || { echo "binary not found: $BIN" >&2; exit 1; }

ARCH=$(file -b "$BIN" | grep -q 'aarch64' && echo aarch64 || echo x86_64)
case "$ARCH" in
  aarch64) DEBARCH=arm64 ;;
  *) DEBARCH=amd64 ;;
esac

mkdir -p dist
STAGE=$(mktemp -d)
trap 'rm -rf "$STAGE"' EXIT

DESKTOP='[Desktop Entry]
Type=Application
Name=OpenVClient
GenericName=Database and infrastructure manager
Comment=Open-source universal database and infrastructure manager
Exec=openvclient
Icon=openvclient
Terminal=false
Categories=Development;Database;
Keywords=database;sql;postgres;mysql;sqlserver;redis;mongodb;docker;
StartupWMClass=openvclient'

# --- tar.gz -----------------------------------------------------------------
NAME="openvclient-${VERSION}-linux-${ARCH}"
mkdir -p "$STAGE/$NAME"
install -m755 "$BIN" "$STAGE/$NAME/openvclient"
printf '%s\n' "$DESKTOP" > "$STAGE/$NAME/openvclient.desktop"
cp README.md LICENSE-MIT LICENSE-APACHE "$STAGE/$NAME/" 2>/dev/null || true
[[ -f assets/icon.png ]] && cp assets/icon.png "$STAGE/$NAME/openvclient.png"
tar -C "$STAGE" -czf "dist/$NAME.tar.gz" "$NAME"
echo "Built dist/$NAME.tar.gz ($(du -h "dist/$NAME.tar.gz" | cut -f1))"

# --- .deb -------------------------------------------------------------------
if command -v dpkg-deb >/dev/null; then
  DEB="$STAGE/deb"
  mkdir -p "$DEB/DEBIAN" "$DEB/usr/bin" "$DEB/usr/share/applications" "$DEB/usr/share/doc/openvclient"
  install -m755 "$BIN" "$DEB/usr/bin/openvclient"
  printf '%s\n' "$DESKTOP" > "$DEB/usr/share/applications/openvclient.desktop"
  cp LICENSE-MIT LICENSE-APACHE "$DEB/usr/share/doc/openvclient/" 2>/dev/null || true
  if [[ -f assets/icon.png ]]; then
    mkdir -p "$DEB/usr/share/icons/hicolor/512x512/apps"
    cp assets/icon.png "$DEB/usr/share/icons/hicolor/512x512/apps/openvclient.png"
  fi
  SIZE=$(du -sk "$DEB" | cut -f1)
  # Runtime: OpenSSL (native-tls for PG/MySQL/Redis), GL, xkbcommon. The keyring
  # and file dialogs talk to D-Bus services that are recommended, not required.
  cat > "$DEB/DEBIAN/control" <<CONTROL
Package: openvclient
Version: ${VERSION}
Section: database
Priority: optional
Architecture: ${DEBARCH}
Installed-Size: ${SIZE}
Depends: libc6 (>= 2.35), libssl3 | libssl3t64, libgl1, libxkbcommon0
Recommends: gnome-keyring | kwalletmanager | keepassxc, xdg-desktop-portal
Suggests: postgresql-client, default-mysql-client
Maintainer: OpenVClient contributors <maintainers@users.noreply.github.com>
Homepage: https://github.com/Mrhudson69/openvclient
Description: Open-source universal database and infrastructure manager
 Native desktop client for PostgreSQL, MySQL/MariaDB, SQL Server, SQLite,
 ClickHouse, Redis, MongoDB, Elasticsearch and Docker, with SSH tunnels and
 SOCKS5/HTTP proxies. Secrets are kept in the system keyring.
CONTROL
  OUT="dist/openvclient_${VERSION}_${DEBARCH}.deb"
  dpkg-deb --root-owner-group --build "$DEB" "$OUT" >/dev/null
  echo "Built $OUT ($(du -h "$OUT" | cut -f1))"
else
  echo "dpkg-deb not found; skipped .deb"
fi
