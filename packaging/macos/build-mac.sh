#!/usr/bin/env bash
# Build the macOS app on a Mac over SSH:
#   dist/Rusty-Solitaire-<version>-macos-universal.dmg
#
# Usage: packaging/macos/build-mac.sh [user@mac]
#   (default: $MAC_HOST, or MAC_HOST= in packaging/macos/local.env)
#
# Copies the source to the Mac, builds for Apple Silicon and Intel, joins
# them into one universal program, wraps it in "Rusty Solitaire.app" (icon,
# version, copyright), signs it ad hoc and packs it in a drag-to-Applications
# disk image, which is copied back here.
#
# The Mac needs Remote Login with this machine's SSH key, Apple's
# command-line tools (xcode-select --install) and Rust (rustup.rs).
#
# Copyright (C) 2026 The Rusty Solitaire authors
# SPDX-License-Identifier: GPL-3.0-or-later
set -euo pipefail
cd "$(dirname "$0")/../.."

# The Mac to build on: the argument, else $MAC_HOST, else MAC_HOST= from
# packaging/macos/local.env (kept out of version control).
if [ -z "${1:-}${MAC_HOST:-}" ] && [ -f packaging/macos/local.env ]; then
    . packaging/macos/local.env
fi
MAC=${1:-${MAC_HOST:-}}
[ -n "$MAC" ] || { echo "Usage: $0 user@mac   (or set MAC_HOST, or put MAC_HOST=user@mac in packaging/macos/local.env)" >&2; exit 1; }
REMOTE_DIR=rusty-solitaire-build
VERSION=$(grep -m1 '^version' Cargo.toml | cut -d'"' -f2)
DMG="Rusty-Solitaire-$VERSION-macos-universal.dmg"
SSH=(ssh -o BatchMode=yes -o ConnectTimeout=15)

# A sharp 1024 px icon for the .icns, rendered from the SVG here (the Mac
# can't rasterise SVG without extra tools).
mkdir -p target/macos dist
convert -background none -density 384 packaging/linux/io.github.rustysolitaire.RustySolitaire.svg \
    -resize 1024x1024 target/macos/icon-1024.png

echo "Copying the source to $MAC..."
rsync -az --delete -e "${SSH[*]}" \
    --exclude /target --exclude /dist --exclude /reference --exclude /.git \
    ./ "$MAC:$REMOTE_DIR/"
rsync -az -e "${SSH[*]}" target/macos/icon-1024.png "$MAC:$REMOTE_DIR/packaging/macos/"

echo "Building on the Mac..."
"${SSH[@]}" "$MAC" bash -s -- "$REMOTE_DIR" "$VERSION" "$DMG" <<'REMOTE'
set -euo pipefail
dir=$1 version=$2 dmg=$3
cd "$HOME/$dir"
[ -f "$HOME/.cargo/env" ] && . "$HOME/.cargo/env"
command -v cargo >/dev/null || { echo "Rust is not installed on the Mac (see https://rustup.rs)" >&2; exit 1; }
xcode-select -p >/dev/null 2>&1 || { echo "Run 'xcode-select --install' on the Mac first" >&2; exit 1; }

rustup target add aarch64-apple-darwin x86_64-apple-darwin >/dev/null
. packaging/release-flags.sh
release_flags
cargo build --release --target aarch64-apple-darwin
cargo build --release --target x86_64-apple-darwin

out=target/macos
app="$out/Rusty Solitaire.app"
rm -rf "$out" && mkdir -p "$app/Contents/MacOS" "$app/Contents/Resources"

# One program that runs natively on Apple Silicon and Intel Macs.
lipo -create -output "$app/Contents/MacOS/rusty-solitaire" \
    target/aarch64-apple-darwin/release/rusty-solitaire \
    target/x86_64-apple-darwin/release/rusty-solitaire

sed "s/@VERSION@/$version/g" packaging/macos/Info.plist > "$app/Contents/Info.plist"
cp assets/licenses/COPYING.GPL3 "$app/Contents/Resources/LICENSE.txt"

# App icon: every size macOS asks for, from the 1024 px rendering.
iconset="$out/RustySolitaire.iconset"
mkdir -p "$iconset"
for size in 16 32 128 256 512; do
    sips -z $size $size packaging/macos/icon-1024.png --out "$iconset/icon_${size}x${size}.png" >/dev/null
    double=$((size * 2))
    sips -z $double $double packaging/macos/icon-1024.png --out "$iconset/icon_${size}x${size}@2x.png" >/dev/null
done
iconutil -c icns "$iconset" -o "$app/Contents/Resources/RustySolitaire.icns"

# Ad hoc signature: required to run on Apple Silicon. (A Developer ID
# signature and notarization would also remove Gatekeeper's warning.)
codesign --force --deep --sign - "$app"
codesign --verify --deep --strict "$app"

# Disk image with the usual "drag to Applications" layout.
stage="$out/dmg"
mkdir -p "$stage"
cp -R "$app" "$stage/"
ln -s /Applications "$stage/Applications"
hdiutil create -quiet -volname "Rusty Solitaire" -srcfolder "$stage" -ov -format UDZO "$out/$dmg"
lipo -info "$app/Contents/MacOS/rusty-solitaire"
REMOTE

rsync -az -e "${SSH[*]}" "$MAC:$REMOTE_DIR/target/macos/$DMG" "dist/$DMG"
echo "Built dist/$DMG"
