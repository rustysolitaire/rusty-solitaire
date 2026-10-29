#!/usr/bin/env bash
# The part of build-mac.sh that runs on the Mac, from the copied source
# folder: builds for both chips and makes the three disk images in
# target/macos/. build-mac.sh starts it in the background, so a dropped
# network connection doesn't stop the build.
#
# Usage (on the Mac): packaging/macos/build-on-mac.sh VERSION
#
# Copyright (C) 2026 The Rusty Solitaire authors
# SPDX-License-Identifier: GPL-3.0-or-later
set -euo pipefail
cd "$(dirname "$0")/../.."
version=$1
# Keep the Mac from going to sleep until this script ends.
caffeinate -i -w $$ &
[ -f "$HOME/.cargo/env" ] && . "$HOME/.cargo/env"
command -v cargo >/dev/null || { echo "Rust is not installed on the Mac (see https://rustup.rs)" >&2; exit 1; }
xcode-select -p >/dev/null 2>&1 || { echo "Run 'xcode-select --install' on the Mac first" >&2; exit 1; }

rustup target add aarch64-apple-darwin x86_64-apple-darwin >/dev/null
. packaging/release-flags.sh
release_flags
cargo build --release --target aarch64-apple-darwin
cargo build --release --target x86_64-apple-darwin

out=target/macos
rm -rf "$out" && mkdir -p "$out"

# App icon: every size macOS asks for, from the 1024 px rendering.
iconset="$out/RustySolitaire.iconset"
mkdir -p "$iconset"
for size in 16 32 128 256 512; do
    sips -z $size $size packaging/macos/icon-1024.png --out "$iconset/icon_${size}x${size}.png" >/dev/null
    double=$((size * 2))
    sips -z $double $double packaging/macos/icon-1024.png --out "$iconset/icon_${size}x${size}@2x.png" >/dev/null
done
iconutil -c icns "$iconset" -o "$out/RustySolitaire.icns"

# make_dmg VARIANT PROGRAM...: the app with the given program(s), in
# Rusty-Solitaire-<version>-macos-VARIANT.dmg.
make_dmg() {
    local variant=$1; shift
    local work="$out/$variant"
    local app="$work/Rusty Solitaire.app"
    mkdir -p "$app/Contents/MacOS" "$app/Contents/Resources"
    # Two programs are joined into one that runs natively on both chips.
    lipo -create -output "$app/Contents/MacOS/rusty-solitaire" "$@"
    sed "s/@VERSION@/$version/g" packaging/macos/Info.plist > "$app/Contents/Info.plist"
    cp assets/licenses/COPYING.GPL3 "$app/Contents/Resources/LICENSE.txt"
    cp "$out/RustySolitaire.icns" "$app/Contents/Resources/"

    # Ad hoc signature: required to run on Apple Silicon. (A Developer ID
    # signature and notarization would also remove Gatekeeper's warning.)
    codesign --force --deep --sign - "$app"
    codesign --verify --deep --strict "$app"

    # Disk image with the usual "drag to Applications" layout.
    local stage="$work/dmg"
    mkdir -p "$stage"
    cp -R "$app" "$stage/"
    ln -s /Applications "$stage/Applications"
    hdiutil create -quiet -volname "Rusty Solitaire" -srcfolder "$stage" -ov -format UDZO \
        "$out/Rusty-Solitaire-$version-macos-$variant.dmg"
    echo "$variant: $(lipo -archs "$app/Contents/MacOS/rusty-solitaire")"
}

arm=target/aarch64-apple-darwin/release/rusty-solitaire
intel=target/x86_64-apple-darwin/release/rusty-solitaire
make_dmg universal "$arm" "$intel"
make_dmg apple-silicon "$arm"
make_dmg intel "$intel"
