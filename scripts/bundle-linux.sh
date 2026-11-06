#!/bin/bash
# Build OpenVClient for Linux and package it. Every package installs a desktop
# entry and icons, so OpenVClient appears in the application menu/launcher.
#
#   ./scripts/bundle-linux.sh                 # host target, release build
#   TARGET=aarch64-unknown-linux-gnu ./scripts/bundle-linux.sh
#   SKIP_BUILD=1 BIN=path/to/openvclient ./scripts/bundle-linux.sh
#
# Output (each when its tool is available):
#   dist/openvclient-<version>-linux-<arch>.tar.gz   always (with install.sh)
#   dist/openvclient_<version>_<debarch>.deb         dpkg-deb   (Debian, Ubuntu, Mint)
#   dist/openvclient-<version>-1.<arch>.rpm          rpmbuild   (Fedora, RHEL, openSUSE)
#   dist/OpenVClient-<version>-<arch>.AppImage       appimagetool (any distro, no install)
set -euo pipefail
cd "$(dirname "$0")/.."
ROOT=$(pwd)

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
ICON_MASTER=assets/icon/openvclient.png
SUMMARY="Open-source universal database and infrastructure manager"

DESKTOP='[Desktop Entry]
Type=Application
Name=OpenVClient
GenericName=Database and infrastructure manager
Comment=Open-source universal database and infrastructure manager
Exec=openvclient
Icon=openvclient
Terminal=false
Categories=Development;Database;Network;
Keywords=database;sql;postgres;mysql;sqlserver;redis;mongodb;docker;ssh;s3;azure;
StartupWMClass=openvclient
StartupNotify=true'

# hicolor icon theme sizes, scaled from the 1024 px master (Pillow or ImageMagick).
icons_into() { # $1 = share dir (…/share)
  local share=$1
  for s in 16 32 48 64 128 256 512; do
    local dir="$share/icons/hicolor/${s}x${s}/apps"
    mkdir -p "$dir"
    if command -v python3 >/dev/null && python3 -c "import PIL" 2>/dev/null; then
      python3 -c "from PIL import Image; Image.open('$ICON_MASTER').resize(($s,$s), Image.LANCZOS).save('$dir/openvclient.png')"
    elif command -v convert >/dev/null; then
      convert "$ICON_MASTER" -resize "${s}x${s}" "$dir/openvclient.png"
    elif [[ $s == 256 ]]; then
      cp assets/icon/openvclient-256.png "$dir/openvclient.png"
    else
      rmdir "$dir" 2>/dev/null || true
    fi
  done
  mkdir -p "$share/icons/hicolor/512x512/apps"
  [[ -f "$share/icons/hicolor/512x512/apps/openvclient.png" ]] || cp "$ICON_MASTER" "$share/icons/hicolor/512x512/apps/openvclient.png"
}

# Common filesystem layout under a prefix ($1 = root, e.g. …/usr).
layout_into() {
  local usr=$1
  install -Dm755 "$BIN" "$usr/bin/openvclient"
  install -d "$usr/share/applications" "$usr/share/doc/openvclient" "$usr/share/metainfo"
  printf '%s\n' "$DESKTOP" > "$usr/share/applications/openvclient.desktop"
  cp README.md LICENSE-MIT LICENSE-APACHE "$usr/share/doc/openvclient/" 2>/dev/null || true
  # AppStream metadata: shows the app properly in GNOME Software / KDE Discover.
  cat > "$usr/share/metainfo/dev.openvclient.client.metainfo.xml" <<XML
<?xml version="1.0" encoding="UTF-8"?>
<component type="desktop-application">
  <id>dev.openvclient.client</id>
  <name>OpenVClient</name>
  <summary>${SUMMARY}</summary>
  <metadata_license>MIT</metadata_license>
  <project_license>MIT OR Apache-2.0</project_license>
  <launchable type="desktop-id">openvclient.desktop</launchable>
  <url type="homepage">https://github.com/Mrhudson69/openvclient</url>
  <description><p>Manage PostgreSQL, MySQL/MariaDB, SQL Server, SQLite, ClickHouse, MongoDB, Redis, Elasticsearch, RabbitMQ, Docker, SSH, FTP, S3-compatible and Azure Blob storage from one native window. Secrets stay in the system keyring.</p></description>
  <releases><release version="${VERSION}"/></releases>
  <content_rating type="oars-1.1"/>
</component>
XML
  icons_into "$usr/share"
}

# --- tar.gz (portable, with a per-user installer script) --------------------
NAME="openvclient-${VERSION}-linux-${ARCH}"
mkdir -p "$STAGE/$NAME"
layout_into "$STAGE/$NAME/usr"
cat > "$STAGE/$NAME/install.sh" <<'SH'
#!/bin/sh
# Install OpenVClient for the current user (~/.local) or, with --system, for
# everyone (/usr/local, needs sudo). Remove with: ./install.sh --uninstall
set -e
cd "$(dirname "$0")"
PREFIX="$HOME/.local"
case "${1:-}" in
  --system) PREFIX=/usr/local ;;
  --uninstall) for p in "$HOME/.local" /usr/local; do rm -f "$p/bin/openvclient" "$p/share/applications/openvclient.desktop" "$p/share/metainfo/dev.openvclient.client.metainfo.xml"; find "$p/share/icons/hicolor" -name openvclient.png -delete 2>/dev/null || true; done; echo "OpenVClient removed."; exit 0 ;;
esac
# The window system loads these at start-up; name any that are missing.
missing=""
for lib in libxkbcommon.so.0 libxkbcommon-x11.so.0 libX11.so.6 libX11-xcb.so.1 libXcursor.so.1 libXrandr.so.2 libXi.so.6 libwayland-client.so.0 libEGL.so.1 libGL.so.1 libssl.so.3; do
  ldconfig -p 2>/dev/null | grep -q "$lib" || missing="$missing $lib"
done
if [ -n "$missing" ]; then
  echo "Warning: these system libraries were not found:$missing"
  echo "  Debian/Ubuntu: sudo apt install libxkbcommon-x11-0 libx11-xcb1 libxcursor1 libxrandr2 libxi6 libwayland-client0 libegl1 libgl1 libssl3"
  echo "  Fedora:        sudo dnf install libxkbcommon-x11 libX11-xcb libXcursor libXrandr libXi libwayland-client mesa-libEGL mesa-libGL openssl-libs"
fi
mkdir -p "$PREFIX"
cp -R usr/bin usr/share "$PREFIX/"
command -v update-desktop-database >/dev/null && update-desktop-database "$PREFIX/share/applications" 2>/dev/null || true
command -v gtk-update-icon-cache >/dev/null && gtk-update-icon-cache -q -t "$PREFIX/share/icons/hicolor" 2>/dev/null || true
echo "OpenVClient installed to $PREFIX. Start it from your application menu or run: openvclient"
case ":$PATH:" in *":$PREFIX/bin:"*) ;; *) echo "Note: add $PREFIX/bin to PATH to run it from a terminal." ;; esac
SH
chmod 755 "$STAGE/$NAME/install.sh"
tar -C "$STAGE" -czf "dist/$NAME.tar.gz" "$NAME"
echo "Built dist/$NAME.tar.gz ($(du -h "dist/$NAME.tar.gz" | cut -f1))"

# --- .deb -------------------------------------------------------------------
if command -v dpkg-deb >/dev/null; then
  DEB="$STAGE/deb"
  layout_into "$DEB/usr"
  mkdir -p "$DEB/DEBIAN"
  SIZE=$(du -sk "$DEB" | cut -f1)
  # Runtime: OpenSSL (native-tls for PG/MySQL/Redis) plus the windowing and GL
  # libraries. winit/glutin load the X11, Wayland, xkbcommon and EGL/GL
  # libraries with dlopen at start-up, so dpkg-shlibdeps cannot see them and
  # they must be listed here; without libxkbcommon-x11 the app aborts on X11.
  # The keyring and file dialogs talk to D-Bus services (recommended only).
  cat > "$DEB/DEBIAN/control" <<CONTROL
Package: openvclient
Version: ${VERSION}
Section: database
Priority: optional
Architecture: ${DEBARCH}
Installed-Size: ${SIZE}
Depends: libc6 (>= 2.35), libssl3 | libssl3t64, libgl1, libegl1, libxkbcommon0, libxkbcommon-x11-0, libx11-6, libx11-xcb1, libxcb1, libxcursor1, libxrandr2, libxi6, libwayland-client0, libwayland-cursor0, libwayland-egl1
Recommends: gnome-keyring | kwalletmanager | keepassxc, xdg-desktop-portal
Suggests: postgresql-client, default-mysql-client, openssh-client
Maintainer: OpenVClient contributors <maintainers@users.noreply.github.com>
Homepage: https://github.com/Mrhudson69/openvclient
Description: ${SUMMARY}
 Native desktop client for PostgreSQL, MySQL/MariaDB, SQL Server, SQLite,
 ClickHouse, Redis, MongoDB, Elasticsearch, RabbitMQ, Docker, SSH, FTP,
 S3-compatible and Azure Blob storage, with SSH tunnels and SOCKS5/HTTP
 proxies. Secrets are kept in the system keyring.
CONTROL
  # Refresh menus and icon caches after install/remove.
  for s in postinst postrm; do
    printf '#!/bin/sh\nset -e\ncommand -v update-desktop-database >/dev/null && update-desktop-database -q /usr/share/applications || true\ncommand -v gtk-update-icon-cache >/dev/null && gtk-update-icon-cache -q -t /usr/share/icons/hicolor || true\n' > "$DEB/DEBIAN/$s"
    chmod 755 "$DEB/DEBIAN/$s"
  done
  OUT="dist/openvclient_${VERSION}_${DEBARCH}.deb"
  dpkg-deb --root-owner-group --build "$DEB" "$OUT" >/dev/null
  echo "Built $OUT ($(du -h "$OUT" | cut -f1))"
else
  echo "dpkg-deb not found; skipped .deb"
fi

# --- .rpm -------------------------------------------------------------------
if command -v rpmbuild >/dev/null; then
  RPMTOP="$STAGE/rpm"
  mkdir -p "$RPMTOP"/{BUILD,RPMS,SOURCES,SPECS,SRPMS,BUILDROOT}
  RPMROOT="$STAGE/rpmroot"
  layout_into "$RPMROOT/usr"
  cat > "$RPMTOP/SPECS/openvclient.spec" <<SPEC
Name:           openvclient
Version:        ${VERSION}
Release:        1
Summary:        ${SUMMARY}
License:        MIT OR Apache-2.0
URL:            https://github.com/Mrhudson69/openvclient
# Library sonames (distro-neutral) for what winit/glutin dlopen at start-up.
Requires:       libssl.so.3()(64bit), libGL.so.1()(64bit), libEGL.so.1()(64bit)
Requires:       libxkbcommon.so.0()(64bit), libxkbcommon-x11.so.0()(64bit)
Requires:       libX11.so.6()(64bit), libX11-xcb.so.1()(64bit), libxcb.so.1()(64bit)
Requires:       libXcursor.so.1()(64bit), libXrandr.so.2()(64bit), libXi.so.6()(64bit)
Requires:       libwayland-client.so.0()(64bit), libwayland-cursor.so.0()(64bit), libwayland-egl.so.1()(64bit)
Recommends:     gnome-keyring
AutoReqProv:    no

%description
Native desktop client for databases, object storage and servers.

%install
mkdir -p %{buildroot}
cp -a ${RPMROOT}/usr %{buildroot}/

%files
/usr/bin/openvclient
/usr/share/applications/openvclient.desktop
/usr/share/metainfo/dev.openvclient.client.metainfo.xml
/usr/share/icons/hicolor/*/apps/openvclient.png
%doc /usr/share/doc/openvclient

%post
update-desktop-database -q /usr/share/applications 2>/dev/null || :
gtk-update-icon-cache -q -t /usr/share/icons/hicolor 2>/dev/null || :

%postun
update-desktop-database -q /usr/share/applications 2>/dev/null || :
SPEC
  rpmbuild --quiet --define "_topdir $RPMTOP" --target "$ARCH" -bb "$RPMTOP/SPECS/openvclient.spec"
  find "$RPMTOP/RPMS" -name '*.rpm' -exec cp {} dist/ \;
  echo "Built $(ls dist/*.rpm | tail -1)"
else
  echo "rpmbuild not found; skipped .rpm (Fedora: sudo dnf install rpm-build)"
fi

# --- AppImage ---------------------------------------------------------------
APPIMAGETOOL=${APPIMAGETOOL:-$(command -v appimagetool || true)}
if [[ -n "$APPIMAGETOOL" ]]; then
  APPDIR="$STAGE/OpenVClient.AppDir"
  layout_into "$APPDIR/usr"
  printf '%s\n' "$DESKTOP" > "$APPDIR/openvclient.desktop"
  cp "$ICON_MASTER" "$APPDIR/openvclient.png"
  ln -s openvclient.png "$APPDIR/.DirIcon"
  printf '#!/bin/sh\nHERE="$(dirname "$(readlink -f "$0")")"\nexec "$HERE/usr/bin/openvclient" "$@"\n' > "$APPDIR/AppRun"
  chmod 755 "$APPDIR/AppRun"
  OUT="dist/OpenVClient-${VERSION}-${ARCH}.AppImage"
  ARCH="$ARCH" "$APPIMAGETOOL" --no-appstream "$APPDIR" "$OUT" >/dev/null
  echo "Built $OUT ($(du -h "$OUT" | cut -f1))"
else
  echo "appimagetool not found; skipped AppImage (https://github.com/AppImage/appimagetool/releases)"
fi

(cd dist && sha256sum openvclient* OpenVClient-*.AppImage 2>/dev/null > SHA256SUMS-linux || true)
