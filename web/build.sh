#!/bin/bash
# Builds the web version of Rusty Solitaire: the game compiled to WebAssembly
# plus a page that runs it on its own, in web/dist/. website/build.sh copies
# the game from there to the website's play page.
#
#   web/build.sh            build into web/dist/
#   web/build.sh --serve    build, then serve web/dist/ on http://localhost:8090/
#
# Needs the wasm32-unknown-unknown Rust target and wasm-bindgen-cli at the
# same version as the wasm-bindgen crate in Cargo.lock:
#   rustup target add wasm32-unknown-unknown
#   cargo install wasm-bindgen-cli --version <version> --locked
set -euo pipefail

here=$(cd "$(dirname "$0")" && pwd)
app=$(dirname "$here")
dist=$here/dist

cd "$app"
cargo build --release --target wasm32-unknown-unknown

rm -rf "$dist"
mkdir -p "$dist"
wasm-bindgen --target web --no-typescript --out-dir "$dist" \
  target/wasm32-unknown-unknown/release/rusty-solitaire.wasm
# Smaller and faster with binaryen's wasm-opt, when it is installed.
if command -v wasm-opt >/dev/null; then
  wasm-opt -O2 --enable-bulk-memory --enable-nontrapping-float-to-int --enable-sign-ext \
    "$dist/rusty-solitaire_bg.wasm" -o "$dist/rusty-solitaire_bg.wasm"
fi
cp "$here/index.html" "$dist/"
cp "$app/assets/icons/aisleriot.ico" "$dist/favicon.ico"

echo "Rusty Solitaire for the web: $dist ($(du -h "$dist/rusty-solitaire_bg.wasm" | cut -f1) of WebAssembly)"
if [ "${1:-}" = --serve ]; then
  echo "Serving on http://localhost:8090/"
  exec python3 -I -m http.server 8090 --bind 127.0.0.1 --directory "$dist"
fi
