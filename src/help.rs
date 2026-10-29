// Rusty Solitaire - help.rs
//
// Help → Contents and Help on <game>: the rules pages, built into the
// program, shown in the web browser (ar-help.c opened the GNOME help viewer).
// The pages are one self-contained HTML file (made by tools/gen-help.py),
// written to the cache folder with the page to show first. One file, so it
// also works from the Flatpak sandbox, which can hand the browser a single
// file but not a folder of them.
//
// Copyright (C) 2026 The Rusty Solitaire authors (Rust port)
// SPDX-License-Identifier: GPL-3.0-or-later

use std::{env, fs, path::PathBuf};

static RULES: &str = include_str!("../assets/help/rules.html");

/// The page with the general help and the list of games.
pub const CONTENTS: &str = "index";

fn cache_dir() -> Option<PathBuf> {
    let base = if cfg!(target_os = "windows") {
        env::var_os("LOCALAPPDATA").map(PathBuf::from)
    } else if cfg!(target_os = "macos") {
        env::var_os("HOME").map(|h| PathBuf::from(h).join("Library/Caches"))
    } else {
        env::var_os("XDG_CACHE_HOME")
            .map(PathBuf::from)
            .or_else(|| env::var_os("HOME").map(|h| PathBuf::from(h).join(".cache")))
    };
    base.map(|b| b.join("rusty-solitaire").join("help"))
}

/// The help as it opens on `page`: a game's file name (e.g. "klondike"),
/// CONTENTS, "keyboard" or "glossary".
pub fn page_html(page: &str) -> String {
    RULES.replacen("@START@", page, 1)
}

/// Open the help on `page` in the web browser.
pub fn open(page: &str) -> Result<(), String> {
    let dir = cache_dir().unwrap_or_else(env::temp_dir);
    fs::create_dir_all(&dir).map_err(|e| format!("{}: {e}", dir.display()))?;
    let path = dir.join(format!("{page}.html"));
    fs::write(&path, page_html(page)).map_err(|e| format!("{}: {e}", path.display()))?;
    open::that_detached(&path).map_err(|e| format!("No web browser could be started.\n\n{e}"))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn every_game_has_a_rules_page() {
        for file in crate::rules::game_files() {
            assert!(RULES.contains(&format!("<div class=\"page\" id=\"{file}\"")), "no help page for {file}");
        }
        assert!(RULES.contains(&format!("id=\"{CONTENTS}\"")));
        assert!(page_html("klondike").contains("data-start=\"klondike\""));
    }
}
