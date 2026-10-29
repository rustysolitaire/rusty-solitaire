// Rusty Solitaire - settings.rs
//
// A tiny key=value preferences file in the platform's config directory,
// standing in for GSettings.
//
// Copyright (C) 2026 The Rusty Solitaire authors (Rust port)
// SPDX-License-Identifier: GPL-3.0-or-later

use std::{collections::BTreeMap, env, fs, path::PathBuf};

/// The settings folder's name inside the platform's config directory.
const DIR: &str = "rusty-solitaire";

/// A file in the app's settings folder.
pub fn data_file_path(name: &str) -> Option<PathBuf> {
    config_path(DIR).map(|p| p.with_file_name(name))
}

pub struct Settings {
    values: BTreeMap<String, String>,
}

fn config_path(dir: &str) -> Option<PathBuf> {
    let base = if cfg!(target_os = "windows") {
        env::var_os("APPDATA").map(PathBuf::from)
    } else if cfg!(target_os = "macos") {
        env::var_os("HOME").map(|h| PathBuf::from(h).join("Library/Application Support"))
    } else {
        env::var_os("XDG_CONFIG_HOME")
            .map(PathBuf::from)
            .or_else(|| env::var_os("HOME").map(|h| PathBuf::from(h).join(".config")))
    };
    base.map(|b| b.join(dir).join("settings.ini"))
}

impl Settings {
    pub fn load() -> Self {
        let values = config_path(DIR)
            .and_then(|p| fs::read_to_string(p).ok())
            .map(|text| {
                text.lines()
                    .filter_map(|line| line.split_once('='))
                    .map(|(k, v)| (k.trim().to_string(), v.trim().to_string()))
                    .collect()
            })
            .unwrap_or_default();
        Settings { values }
    }

    pub fn save(&self) {
        let Some(path) = config_path(DIR) else { return };
        if let Some(dir) = path.parent() {
            let _ = fs::create_dir_all(dir);
        }
        let text: String = self.values.iter().map(|(k, v)| format!("{k}={v}\n")).collect();
        let _ = fs::write(path, text);
    }

    pub fn get_bool(&self, key: &str, default: bool) -> bool {
        self.values.get(key).map(|v| v == "true").unwrap_or(default)
    }

    pub fn set_bool(&mut self, key: &str, value: bool) {
        self.values.insert(key.to_string(), value.to_string());
    }

    pub fn get_string(&self, key: &str) -> Option<&str> {
        self.values.get(key).map(String::as_str)
    }

    pub fn set_string(&mut self, key: &str, value: &str) {
        self.values.insert(key.to_string(), value.to_string());
    }
}
