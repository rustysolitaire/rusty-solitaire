#!/usr/bin/env bash
# Build Linux distribution packages:
#   dist/rusty-solitaire_<version>-1_<amd64|arm64>.deb     (Debian, Ubuntu, Mint, …)
#   dist/rusty-solitaire-<version>-1.<x86_64|aarch64>.rpm  (Fedora, openSUSE, RHEL 10, …)
#
# Usage: packaging/linux/build-packages.sh [x86_64|aarch64|all]   (default: all)
#
# Both install the program as /usr/bin/rusty-solitaire with an applications
# menu entry, icons, AppStream metadata and a manual page. The program needs
# glibc 2.35 or newer (Ubuntu 22.04, Debian 12, Fedora 36 and later).
#
# Needs: dpkg-deb and rpmbuild (Debian/Ubuntu: apt install rpm); for aarch64
# also `rustup target add aarch64-unknown-linux-gnu` and gcc-aarch64-linux-gnu.
#
# Copyright (C) 2026 The Rusty Solitaire authors
# SPDX-License-Identifier: GPL-3.0-or-later
set -euo pipefail
cd "$(dirname "$0")/../.."

APP_ID=io.github.rustysolitaire.RustySolitaire
NAME=rusty-solitaire
VERSION=$(grep -m1 '^version' Cargo.toml | cut -d'"' -f2)
. packaging/release-flags.sh
release_flags
RELEASE=1
HOMEPAGE=https://github.com/rustysolitaire/rusty-solitaire
MAINTAINER="The Rusty Solitaire authors <349638865+rustysolitaire@users.noreply.github.com>"
SUMMARY="Play 91 different solitaire card games"
DESCRIPTION="Rusty Solitaire is a collection of 91 solitaire card games, including
Klondike, FreeCell, Spider, Golf, Yukon and Scorpion. It is a Rust port of
GNOME AisleRiot and plays every game by AisleRiot's original rules, with hints,
unlimited undo, game options and 26 card designs."

if ! command -v cargo >/dev/null && [ -x "$HOME/.cargo/bin/cargo" ]; then
    export PATH="$HOME/.cargo/bin:$PATH"
fi

# Libraries the program loads at runtime (it isn't linked against them, so
# the packaging tools can't find them by themselves): fonts, keyboard,
# Wayland and X11 windows, and OpenGL.
DEB_DEPENDS="libc6 (>= 2.35), libgcc-s1, libfontconfig1, libxkbcommon0, libxkbcommon-x11-0, \
libwayland-client0, libwayland-cursor0, libwayland-egl1, libx11-6, libx11-xcb1, libxcb1, \
libxcursor1, libxi6, libxrandr2, libegl1, libgl1"
RPM_LIBS="libfontconfig.so.1 libxkbcommon.so.0 libxkbcommon-x11.so.0 libwayland-client.so.0 \
libwayland-cursor.so.0 libwayland-egl.so.1 libX11.so.6 libX11-xcb.so.1 libxcb.so.1 \
libXcursor.so.1 libXi.so.6 libXrandr.so.2 libEGL.so.1 libGL.so.1"
# Sound (ALSA, also loaded at runtime): the game plays fine without it.
DEB_RECOMMENDS="libasound2t64 | libasound2"
RPM_RECOMMENDS="libasound.so.2()(64bit)"

# Lay out the installed files under $1 (a staging root), with the program in
# $4 (Debian puts games in /usr/games, Fedora and openSUSE in /usr/bin).
stage_files() {
    local root=$1 binary=$2 strip=$3 bindir=$4
    install -d "$root$bindir"
    "$strip" -o "$root$bindir/$NAME" "$binary"
    chmod 755 "$root$bindir/$NAME"
    install -Dm644 packaging/linux/$APP_ID.desktop "$root/usr/share/applications/$APP_ID.desktop"
    install -Dm644 packaging/linux/$APP_ID.metainfo.xml "$root/usr/share/metainfo/$APP_ID.metainfo.xml"
    install -Dm644 packaging/linux/$APP_ID.svg "$root/usr/share/icons/hicolor/scalable/apps/$APP_ID.svg"
    install -Dm644 assets/icons/aisleriot-256.png "$root/usr/share/icons/hicolor/256x256/apps/$APP_ID.png"
    install -d "$root/usr/share/man/man6"
    gzip -9n < packaging/linux/$NAME.6 > "$root/usr/share/man/man6/$NAME.6.gz"
    chmod 644 "$root/usr/share/man/man6/$NAME.6.gz"
}

build_deb() {
    local arch=$1 binary=$2 strip=$3
    local root=target/linux-packages/deb-$arch
    rm -rf "$root"
    stage_files "$root" "$binary" "$strip" /usr/games
    # /usr/games isn't on every user's PATH: point the menu entry at it.
    sed -i "s|^Exec=$NAME|Exec=/usr/games/$NAME|" "$root/usr/share/applications/$APP_ID.desktop"

    # Not uploaded to Debian's archive, so there are no bug reports to close.
    install -d "$root/usr/share/lintian/overrides"
    echo "$NAME: initial-upload-closes-no-bugs" > "$root/usr/share/lintian/overrides/$NAME"
    chmod 644 "$root/usr/share/lintian/overrides/$NAME"

    # Debian wants the licence in a machine-readable copyright file and a
    # compressed changelog in the package's doc folder.
    local doc=$root/usr/share/doc/$NAME
    install -d "$doc"
    {
        echo "Format: https://www.debian.org/doc/packaging-manuals/copyright-format/1.0/"
        echo "Upstream-Name: Rusty Solitaire"
        echo "Upstream-Contact: $MAINTAINER"
        echo
        echo "Files: *"
        echo "Copyright: 2026 The Rusty Solitaire authors"
        echo " 1998-2026 The GNOME AisleRiot authors"
        echo "License: GPL-3+"
        echo " On Debian systems, the full text of the GNU General Public License"
        echo " version 3 can be found in /usr/share/common-licenses/GPL-3."
    } > "$doc/copyright"
    {
        echo "$NAME ($VERSION-$RELEASE) stable; urgency=medium"
        echo
        echo "  * Release $VERSION. See CHANGELOG.md in the source for details."
        echo
        echo " -- $MAINTAINER  $(date -R)"
    } | gzip -9n > "$doc/changelog.Debian.gz"
    chmod 644 "$doc/copyright" "$doc/changelog.Debian.gz"

    install -d "$root/DEBIAN"
    local size
    size=$(du -sk --exclude=DEBIAN "$root" | cut -f1)
    {
        echo "Package: $NAME"
        echo "Version: $VERSION-$RELEASE"
        echo "Architecture: $arch"
        echo "Maintainer: $MAINTAINER"
        echo "Installed-Size: $size"
        echo "Depends: $DEB_DEPENDS"
        echo "Recommends: $DEB_RECOMMENDS"
        echo "Homepage: $HOMEPAGE"
        echo "Section: games"
        echo "Priority: optional"
        echo "Description: $SUMMARY"
        echo "$DESCRIPTION" | sed 's/^/ /'
    } > "$root/DEBIAN/control"

    local out="dist/${NAME}_${VERSION}-${RELEASE}_${arch}.deb"
    fakeroot dpkg-deb --build -Zxz "$root" "$out" >/dev/null
    echo "Built $out"
}

build_rpm() {
    local arch=$1 binary=$2 strip=$3
    local top=$PWD/target/linux-packages/rpm-$arch
    rm -rf "$top"
    mkdir -p "$top"/{BUILD,RPMS,SOURCES,SPECS,SRPMS,stage}
    stage_files "$top/stage" "$binary" "$strip" /usr/bin
    install -Dm644 assets/licenses/COPYING.GPL3 "$top/stage/usr/share/licenses/$NAME/COPYING"

    local requires=""
    for lib in $RPM_LIBS; do
        requires+="Requires: $lib()(64bit)"$'\n'
    done
    cat > "$top/SPECS/$NAME.spec" <<EOF
Name:           $NAME
Version:        $VERSION
Release:        $RELEASE
Summary:        $SUMMARY
License:        GPL-3.0-or-later
URL:            $HOMEPAGE
Group:          Amusements/Games
Packager:       $MAINTAINER
Requires:       libc.so.6(GLIBC_2.35)(64bit)
$requires
Recommends:     $RPM_RECOMMENDS
# Prebuilt, already stripped program: no build, no debug packages.
%global debug_package %{nil}
%global __os_install_post %{nil}

%description
$DESCRIPTION

%install
cp -a $top/stage/. %{buildroot}/

%files
/usr/bin/$NAME
/usr/share/applications/$APP_ID.desktop
/usr/share/metainfo/$APP_ID.metainfo.xml
/usr/share/icons/hicolor/scalable/apps/$APP_ID.svg
/usr/share/icons/hicolor/256x256/apps/$APP_ID.png
%{_mandir}/man6/$NAME.6*
%license /usr/share/licenses/$NAME/COPYING

%changelog
* $(LC_ALL=C date '+%a %b %d %Y') $MAINTAINER - $VERSION-$RELEASE
- Release $VERSION. See CHANGELOG.md in the source for details.
EOF
    rpmbuild --quiet -bb --target "$arch" --define "_topdir $top" "$top/SPECS/$NAME.spec"
    local out="dist/$NAME-$VERSION-$RELEASE.$arch.rpm"
    cp "$top/RPMS/$arch/$NAME-$VERSION-$RELEASE.$arch.rpm" "$out"
    echo "Built $out"
}

build() {
    local arch=$1 target strip deb_arch
    case $arch in
        x86_64) target=x86_64-unknown-linux-gnu; strip=strip; deb_arch=amd64 ;;
        aarch64) target=aarch64-unknown-linux-gnu; strip=aarch64-linux-gnu-strip; deb_arch=arm64 ;;
        *)
            echo "unknown architecture: $arch (use x86_64, aarch64 or all)" >&2
            exit 1
            ;;
    esac
    cargo build --release --target "$target"
    mkdir -p dist
    local binary=target/$target/release/$NAME
    build_deb "$deb_arch" "$binary" "$strip"
    build_rpm "$arch" "$binary" "$strip"
}

case ${1:-all} in
    all) build x86_64; build aarch64 ;;
    *) build "$1" ;;
esac
