# Sourced by the package build scripts. Compiled programs normally contain the
# build machine's folder names (for error messages), which include the user
# name; these settings rewrite them, so released programs don't reveal who or
# where they were built.
#
# Usage: release_flags [extra rustc flags…]   (run from the app/ folder)
release_flags() {
    local home=${HOME%/}
    # Later prefixes win: the specific folders, then anything else in $HOME.
    export RUSTFLAGS="--remap-path-prefix=$home=~ \
--remap-path-prefix=$home/.cargo=/cargo --remap-path-prefix=$home/.rustup=/rustup \
--remap-path-prefix=$PWD=/rusty-solitaire $*"
    # The same for the little C code some dependencies compile.
    export CFLAGS="-ffile-prefix-map=$home=~ -ffile-prefix-map=$PWD=/rusty-solitaire"
}
