#!/usr/bin/env bash
# Build the Linux Flatpak bundles: dist/Rusty-Solitaire-<version>-<arch>.flatpak
#
# Usage: packaging/flatpak/build-flatpak.sh [x86_64|aarch64|all]   (default: all)
#
# Packages the release build of the program on the Freedesktop runtime. Users
# install a bundle with:  flatpak install --user Rusty-Solitaire-<version>-<arch>.flatpak
# which also adds "Rusty Solitaire" to their applications menu. Flatpak fetches
# the runtime from Flathub if it isn't installed yet.
#
# Needs: flatpak; for aarch64 also `rustup target add aarch64-unknown-linux-gnu`
# and a cross linker (Debian/Ubuntu: apt install gcc-aarch64-linux-gnu). The
# runtime doesn't need to be installed to build the bundles.
#
# Copyright (C) 2026 The Rusty Solitaire authors
# SPDX-License-Identifier: GPL-3.0-or-later
set -euo pipefail
cd "$(dirname "$0")/../.."

APP_ID=io.github.rustysolitaire.RustySolitaire
RUNTIME=org.freedesktop.Platform
RUNTIME_BRANCH=26.08
VERSION=$(grep -m1 '^version' Cargo.toml | cut -d'"' -f2)
. packaging/release-flags.sh
release_flags

# Software centres show the release notes from the metadata: every version
# needs an entry there (and in CHANGELOG.md) before it is packaged.
if ! grep -q "<release version=\"$VERSION\"" packaging/linux/$APP_ID.metainfo.xml; then
    echo "packaging/linux/$APP_ID.metainfo.xml has no <release> entry for $VERSION" >&2
    exit 1
fi

if ! command -v cargo >/dev/null && [ -x "$HOME/.cargo/bin/cargo" ]; then
    export PATH="$HOME/.cargo/bin:$PATH"
fi

build() {
    local arch=$1 target
    case $arch in
        x86_64) target=x86_64-unknown-linux-gnu ;;
        aarch64) target=aarch64-unknown-linux-gnu ;;
        *)
            echo "unknown architecture: $arch (use x86_64, aarch64 or all)" >&2
            exit 1
            ;;
    esac
    cargo build --release --target "$target"

    local build=target/flatpak/$arch/build repo=target/flatpak/repo
    rm -rf "$build"
    mkdir -p "$build/files" "$build/var" dist

    # What "flatpak build-init" writes; done by hand because build-init
    # insists on the runtime for that architecture being installed, which
    # packaging a ready-made program doesn't need.
    cat > "$build/metadata" <<EOF
[Application]
name=$APP_ID
runtime=$RUNTIME/$arch/$RUNTIME_BRANCH
sdk=$RUNTIME/$arch/$RUNTIME_BRANCH
EOF

    local f=$build/files
    install -Dm755 "target/$target/release/rusty-solitaire" "$f/bin/rusty-solitaire"
    install -Dm644 packaging/linux/$APP_ID.desktop "$f/share/applications/$APP_ID.desktop"
    install -Dm644 packaging/linux/$APP_ID.metainfo.xml "$f/share/metainfo/$APP_ID.metainfo.xml"
    install -Dm644 packaging/linux/$APP_ID.svg "$f/share/icons/hicolor/scalable/apps/$APP_ID.svg"
    install -Dm644 assets/icons/aisleriot-256.png "$f/share/icons/hicolor/256x256/apps/$APP_ID.png"
    install -Dm644 assets/licenses/COPYING.GPL3 "$f/share/licenses/$APP_ID/COPYING"

    # Sandbox: a window on Wayland or X11, the GPU and sound output. No
    # files, network or other devices; settings live in
    # ~/.var/app/io.github.rustysolitaire.RustySolitaire. The help opens in
    # the browser through the desktop portal.
    flatpak build-finish "$build" \
        --command=rusty-solitaire \
        --socket=wayland \
        --socket=fallback-x11 \
        --socket=pulseaudio \
        --share=ipc \
        --device=dri

    flatpak build-export --arch="$arch" "$repo" "$build" stable
    local out="dist/Rusty-Solitaire-$VERSION-$arch.flatpak"
    flatpak build-bundle --arch="$arch" \
        --runtime-repo=https://dl.flathub.org/repo/flathub.flatpakrepo \
        "$repo" "$out" "$APP_ID" stable
    echo "Built $out"
}

case ${1:-all} in
    all) build x86_64; build aarch64 ;;
    *) build "$1" ;;
esac
