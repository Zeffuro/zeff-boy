#!/usr/bin/env bash
set -euo pipefail

if [[ $# -ne 3 ]]; then
  echo "usage: $0 <version> <binary> <output-dir>" >&2
  exit 2
fi

VERSION="${1#v}"
RPM_VERSION="${VERSION%%-*}"
RPM_RELEASE="1"
DEB_VERSION="$VERSION"
if [[ "$VERSION" == *-* ]]; then
  RPM_SUFFIX="${VERSION#*-}"
  RPM_RELEASE="0.1.${RPM_SUFFIX//[^0-9A-Za-z]/.}"
  DEB_VERSION="${RPM_VERSION}~${RPM_SUFFIX}"
fi
BINARY="$(realpath "$2")"
OUTPUT_DIR="$(realpath -m "$3")"
ROOT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"

if [[ ! "$VERSION" =~ ^[0-9]+\.[0-9]+\.[0-9]+([.-][0-9A-Za-z]+)*$ ]]; then
  echo "invalid version: $VERSION" >&2
  exit 2
fi
if [[ ! -x "$BINARY" ]]; then
  echo "binary is missing or not executable: $BINARY" >&2
  exit 2
fi

WORK_DIR="$(mktemp -d)"
trap 'rm -rf "$WORK_DIR"' EXIT
mkdir -p "$OUTPUT_DIR"

install_payload() {
  local root="$1"
  install -Dm755 "$BINARY" "$root/usr/bin/zeff-boy"
  install -Dm644 "$ROOT_DIR/packaging/zeff-boy.desktop" \
    "$root/usr/share/applications/zeff-boy.desktop"
  install -Dm644 "$ROOT_DIR/packaging/com.github.zeffuro.zeff-boy.metainfo.xml" \
    "$root/usr/share/metainfo/com.github.zeffuro.zeff-boy.metainfo.xml"
  install -Dm644 "$ROOT_DIR/assets/icon.png" \
    "$root/usr/share/icons/hicolor/512x512/apps/zeff-boy.png"
}

DEB_ROOT="$WORK_DIR/deb"
install_payload "$DEB_ROOT"
install -Dm644 "$ROOT_DIR/LICENSE-MIT" "$DEB_ROOT/usr/share/doc/zeff-boy/LICENSE-MIT"
install -Dm644 "$ROOT_DIR/LICENSE-APACHE" "$DEB_ROOT/usr/share/doc/zeff-boy/LICENSE-APACHE"
cat > "$DEB_ROOT/usr/share/doc/zeff-boy/copyright" <<'EOF'
Format: https://www.debian.org/doc/packaging-manuals/copyright-format/1.0/
Upstream-Name: zeff-boy
Source: https://github.com/Zeffuro/zeff-boy

Files: *
Copyright: 2026 Zeffuro
License: MIT or Apache-2.0
 The complete license texts are installed as LICENSE-MIT and LICENSE-APACHE.
EOF
CHANGELOG_DATE="$(LC_ALL=C date -u -R)"
printf 'zeff-boy (%s) stable; urgency=medium\n\n  * Package zeff-boy %s.\n\n -- Zeffuro <Jeffroiscool@gmail.com>  %s\n' \
  "$DEB_VERSION" "$VERSION" "$CHANGELOG_DATE" \
  | gzip -n -9 > "$DEB_ROOT/usr/share/doc/zeff-boy/changelog.Debian.gz"
mkdir -p "$DEB_ROOT/DEBIAN"
cat > "$DEB_ROOT/DEBIAN/control" <<EOF
Package: zeff-boy
Version: $DEB_VERSION
Section: games
Priority: optional
Architecture: amd64
Maintainer: Zeffuro <Jeffroiscool@gmail.com>
Depends: libc6 (>= 2.35), libgcc-s1, libasound2, libudev1, libxkbcommon0, libxkbcommon-x11-0, libx11-6, libx11-xcb1, libxcursor1, libxi6, libwayland-client0, libvulkan1 | libegl1
Recommends: xdg-desktop-portal
Homepage: https://github.com/Zeffuro/zeff-boy
Description: Multi-system emulator written in Rust
 Emulates Game Boy, Game Boy Color, Game Boy Advance, NES, WonderSwan,
 and Sega 8-bit systems.
EOF
dpkg-deb --root-owner-group --build "$DEB_ROOT" \
  "$OUTPUT_DIR/zeff-boy_${VERSION}_amd64.deb"

RPM_TOP="$WORK_DIR/rpmbuild"
mkdir -p "$RPM_TOP"/{BUILD,BUILDROOT,RPMS,SOURCES,SPECS,SRPMS}
install -m755 "$BINARY" "$RPM_TOP/SOURCES/zeff-boy"
install -m644 "$ROOT_DIR/packaging/zeff-boy.desktop" "$RPM_TOP/SOURCES/zeff-boy.desktop"
install -m644 "$ROOT_DIR/packaging/com.github.zeffuro.zeff-boy.metainfo.xml" \
  "$RPM_TOP/SOURCES/com.github.zeffuro.zeff-boy.metainfo.xml"
install -m644 "$ROOT_DIR/assets/icon.png" "$RPM_TOP/SOURCES/zeff-boy.png"
install -m644 "$ROOT_DIR/LICENSE-MIT" "$RPM_TOP/SOURCES/LICENSE-MIT"
install -m644 "$ROOT_DIR/LICENSE-APACHE" "$RPM_TOP/SOURCES/LICENSE-APACHE"

cat > "$RPM_TOP/SPECS/zeff-boy.spec" <<EOF
Name: zeff-boy
Version: $RPM_VERSION
Release: $RPM_RELEASE%{?dist}
Summary: Multi-system emulator written in Rust
License: MIT OR Apache-2.0
URL: https://github.com/Zeffuro/zeff-boy
Requires: glibc >= 2.35
Requires: libgcc
Requires: alsa-lib
Requires: systemd-libs
Requires: libxkbcommon
Requires: libxkbcommon-x11
Requires: libX11
Requires: libX11-xcb
Requires: libXcursor
Requires: libXi
Requires: wayland-libs
Requires: (vulkan-loader or libglvnd-egl)
Recommends: xdg-desktop-portal

%description
Emulates Game Boy, Game Boy Color, Game Boy Advance, NES, WonderSwan,
and Sega 8-bit systems.

%install
install -Dm755 %{_sourcedir}/zeff-boy %{buildroot}%{_bindir}/zeff-boy
install -Dm644 %{_sourcedir}/zeff-boy.desktop %{buildroot}%{_datadir}/applications/zeff-boy.desktop
install -Dm644 %{_sourcedir}/com.github.zeffuro.zeff-boy.metainfo.xml %{buildroot}%{_datadir}/metainfo/com.github.zeffuro.zeff-boy.metainfo.xml
install -Dm644 %{_sourcedir}/zeff-boy.png %{buildroot}%{_datadir}/icons/hicolor/512x512/apps/zeff-boy.png
install -Dm644 %{_sourcedir}/LICENSE-MIT %{buildroot}%{_datadir}/licenses/zeff-boy/LICENSE-MIT
install -Dm644 %{_sourcedir}/LICENSE-APACHE %{buildroot}%{_datadir}/licenses/zeff-boy/LICENSE-APACHE

%files
%{_bindir}/zeff-boy
%{_datadir}/applications/zeff-boy.desktop
%{_datadir}/metainfo/com.github.zeffuro.zeff-boy.metainfo.xml
%{_datadir}/icons/hicolor/512x512/apps/zeff-boy.png
%license %{_datadir}/licenses/zeff-boy/LICENSE-MIT
%license %{_datadir}/licenses/zeff-boy/LICENSE-APACHE

%changelog
* $(LC_ALL=C date -u '+%a %b %d %Y') Zeffuro <Jeffroiscool@gmail.com> - $RPM_VERSION-$RPM_RELEASE
- Package zeff-boy $VERSION
EOF

rpmbuild --define "_topdir $RPM_TOP" --define "debug_package %{nil}" \
  -bb "$RPM_TOP/SPECS/zeff-boy.spec"
find "$RPM_TOP/RPMS" -type f -name '*.rpm' -exec cp {} \
  "$OUTPUT_DIR/zeff-boy-${RPM_VERSION}-${RPM_RELEASE}.x86_64.rpm" \;

dpkg-deb --info "$OUTPUT_DIR/zeff-boy_${VERSION}_amd64.deb" >/dev/null
dpkg-deb --contents "$OUTPUT_DIR/zeff-boy_${VERSION}_amd64.deb" >/dev/null
rpm -qip "$OUTPUT_DIR/zeff-boy-${RPM_VERSION}-${RPM_RELEASE}.x86_64.rpm" >/dev/null
rpm -qlp "$OUTPUT_DIR/zeff-boy-${RPM_VERSION}-${RPM_RELEASE}.x86_64.rpm" >/dev/null
