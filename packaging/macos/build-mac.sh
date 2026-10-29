#!/usr/bin/env bash
# Build the macOS app on a Mac over SSH, as three disk images:
#   dist/Rusty-Solitaire-<version>-macos-universal.dmg      any Mac
#   dist/Rusty-Solitaire-<version>-macos-apple-silicon.dmg  M1 and later
#   dist/Rusty-Solitaire-<version>-macos-intel.dmg          Intel Macs
#
# Usage: packaging/macos/build-mac.sh [user@mac]
#   (default: $MAC_HOST, or MAC_HOST= in packaging/macos/local.env)
#
# Copies the source to the Mac and builds for Apple Silicon and Intel. Each
# image holds "Rusty Solitaire.app" (icon, version, copyright), signed ad
# hoc, with the Apple Silicon program, the Intel one, or both joined into one
# universal program; they are copied back here.
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
VARIANTS=(universal apple-silicon intel)
SSH=(ssh -o BatchMode=yes -o ConnectTimeout=15 -o ServerAliveInterval=15 -o ServerAliveCountMax=4)

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
# Start the build in the background on the Mac and check on it every 20
# seconds: Wi-Fi connections to a Mac can drop during a long build, and a
# build tied to the connection would stop with it.
"${SSH[@]}" "$MAC" "cd $REMOTE_DIR && rm -f target/build-mac.status && mkdir -p target && \
    (nohup bash -c 'bash packaging/macos/build-on-mac.sh $VERSION > target/build-mac.log 2>&1; echo \$? > target/build-mac.status' >/dev/null 2>&1 &)"
status=""
while [ -z "$status" ]; do
    sleep 20
    status=$("${SSH[@]}" "$MAC" "cat $REMOTE_DIR/target/build-mac.status 2>/dev/null" || true)
done
"${SSH[@]}" "$MAC" "tail -n 20 $REMOTE_DIR/target/build-mac.log"
[ "$status" = 0 ] || { echo "The build on the Mac failed (see ~/$REMOTE_DIR/target/build-mac.log there)" >&2; exit 1; }

for variant in "${VARIANTS[@]}"; do
    dmg="Rusty-Solitaire-$VERSION-macos-$variant.dmg"
    # Retry: the connection may drop for a moment.
    for try in 1 2 3 4 5; do
        rsync -az --partial -e "${SSH[*]}" "$MAC:$REMOTE_DIR/target/macos/$dmg" "dist/$dmg" && break
        [ "$try" = 5 ] && exit 1
        sleep 10
    done
    echo "Built dist/$dmg"
done
