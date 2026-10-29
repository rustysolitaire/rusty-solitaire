#!/usr/bin/env bash
# Build the Windows installers dist/Rusty-Solitaire-<version>-<arch>.msi and
# the standalone programs dist/Rusty-Solitaire-<version>-<arch>.exe
#
# Usage: installer/build-msi.sh [x64|arm64|all]    (default: all)
#
# Needs:
#  - wixl and msitools (Debian/Ubuntu: apt install wixl)
#  - x64:   rustup target add x86_64-pc-windows-gnu; apt install mingw-w64
#  - arm64: rustup target add aarch64-pc-windows-gnullvm, and llvm-mingw
#           (https://github.com/mstorsjo/llvm-mingw) in ~/.local/opt or $LLVM_MINGW
#
# Copyright (C) 2026 The Rusty Solitaire authors
# SPDX-License-Identifier: GPL-3.0-or-later
set -euo pipefail
cd "$(dirname "$0")/.."

# rustup installs cargo in ~/.cargo/bin, which isn't on PATH until a new
# login shell picks up the profile change.
if ! command -v cargo >/dev/null && [ -x "$HOME/.cargo/bin/cargo" ]; then
    export PATH="$HOME/.cargo/bin:$PATH"
fi

VERSION=$(grep -m1 '^version' Cargo.toml | cut -d'"' -f2)
. packaging/release-flags.sh

build() {
    local arch=$1 target strip template
    case $arch in
        x64)
            target=x86_64-pc-windows-gnu
            strip=x86_64-w64-mingw32-strip
            template="x64;1033"
            ;;
        arm64)
            target=aarch64-pc-windows-gnullvm
            strip=aarch64-w64-mingw32-strip
            template="Arm64;1033"
            local llvm_mingw=${LLVM_MINGW:-$(ls -d "$HOME"/.local/opt/llvm-mingw-* 2>/dev/null | sort | tail -1)}
            if [ -z "$llvm_mingw" ] || [ ! -x "$llvm_mingw/bin/aarch64-w64-mingw32-clang" ]; then
                echo "llvm-mingw not found: set LLVM_MINGW or unpack it in ~/.local/opt" >&2
                exit 1
            fi
            export PATH="$llvm_mingw/bin:$PATH"
            ;;
        *)
            echo "unknown architecture: $arch (use x64, arm64 or all)" >&2
            exit 1
            ;;
    esac

    if [ "$arch" = arm64 ]; then
        # Windows doesn't ship libunwind: link it in (as .cargo/config.toml does).
        release_flags -C target-feature=+crt-static
    else
        release_flags
    fi
    cargo build --release --target "$target"

    mkdir -p dist "target/msi/$arch"
    "$strip" -o "target/msi/$arch/rusty-solitaire.exe" "target/$target/release/rusty-solitaire.exe"
    # The program on its own too, for running without installing.
    cp "target/msi/$arch/rusty-solitaire.exe" "dist/Rusty-Solitaire-$VERSION-$arch.exe"

    local out="dist/Rusty-Solitaire-$VERSION-$arch.msi"
    # wixl only knows Intel architectures; an ARM64 package differs only in
    # its platform template, which is set below.
    (cd installer && wixl --arch x64 \
        -D Version="$VERSION" \
        -D ExePath="../target/msi/$arch/rusty-solitaire.exe" \
        -D Assets=../assets \
        --extdir ext --ext ui \
        -o "../$out" \
        rusty-solitaire.wxs)

    # Fix-ups wixl gets wrong:
    #  - HKMU must be Root -1 ("HKCU or HKLM depending on ALLUSERS"); wixl writes 4.
    #  - Show the scope page after costing, just before the progress page, like
    #    the standard WiX wizards (wixl ignores Before= here).
    #  - The install-folder actions must run after AppSearch (50), before
    #    CostFinalize (1000), with the per-user folder restore last.
    msibuild "$out" -q "UPDATE \`Registry\` SET \`Root\` = -1 WHERE \`Root\` = 4"
    msibuild "$out" -q "UPDATE \`InstallUISequence\` SET \`Sequence\` = 1297 WHERE \`Action\` = 'InstallScopeDlg'"
    for step in "SetPerUserInstallFolder 901" "RestoreMachineFolder 902" "RestoreUserFolder 903"; do
        set -- $step
        msibuild "$out" -q "UPDATE \`InstallExecuteSequence\` SET \`Sequence\` = $2 WHERE \`Action\` = '$1'"
    done
    msibuild "$out" -s "Rusty Solitaire $VERSION Installer" "The Rusty Solitaire authors" "$template"
    echo "Built $out"
}

case ${1:-all} in
    all) build x64; build arm64 ;;
    *) build "$1" ;;
esac
