#!/usr/bin/env bash
# Renders the application icon files from the two master SVGs:
#
#   packaging/linux/io.github.rustysolitaire.RustySolitaire.svg   48 px and up
#   assets/icons/rusty-solitaire-small.svg                        16-32 px, simplified
#
# into assets/icons/aisleriot-256.png (window icon, Linux packages, Flatpak)
# and assets/icons/aisleriot.ico (Windows .exe and MSI). With a website
# folder as argument it also writes that site's img/icon.png and favicon.png.
#
# Usage: tools/gen-icons.sh [../website]
#
# Needs rsvg-convert (apt install librsvg2-bin) and ImageMagick. ImageMagick's
# own SVG renderer gets the gradients and transforms wrong, so rsvg-convert
# does all the rendering.
set -euo pipefail
cd "$(dirname "$0")/.."

big=packaging/linux/io.github.rustysolitaire.RustySolitaire.svg
small=assets/icons/rusty-solitaire-small.svg
tmp=$(mktemp -d)
trap 'rm -rf "$tmp"' EXIT

for size in 16 24 32; do rsvg-convert -w $size -h $size "$small" -o "$tmp/$size.png"; done
for size in 48 64 128 256; do rsvg-convert -w $size -h $size "$big" -o "$tmp/$size.png"; done

cp "$tmp/256.png" assets/icons/aisleriot-256.png
convert "$tmp"/{16,24,32,48,64,128,256}.png assets/icons/aisleriot.ico
echo "wrote assets/icons/aisleriot-256.png and assets/icons/aisleriot.ico"

if [ $# -gt 0 ]; then
    site=$1/src
    cp "$tmp/256.png" "$site/img/icon.png"
    cp "$tmp/32.png" "$site/favicon.png"
    echo "wrote $site/img/icon.png and $site/favicon.png"
fi
