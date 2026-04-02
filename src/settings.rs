use std::env;
use std::fs;
use std::path::{Path, PathBuf};

use anyhow::{Context, Result};
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PersistedSettings {
    pub show_toolbar: bool,
    pub show_status_bar: bool,
    pub show_thumbnail_strip: bool,
    pub thumbnails_window_mode: bool,
    pub last_open_directory: Option<PathBuf>,
}

impl Default for PersistedSettings {
    fn default() -> Self {
        Self {
            show_toolbar: true,
            show_status_bar: true,
            show_thumbnail_strip: false,
            thumbnails_window_mode: false,
            last_open_directory: None,
        }
    }
}

pub fn load_settings() -> PersistedSettings {
    let path = settings_path();
    match fs::read_to_string(&path) {
        Ok(json) => match serde_json::from_str::<PersistedSettings>(&json) {
            Ok(settings) => settings,
            Err(error) => {
                eprintln!(
                    "[settings] invalid settings JSON at {}: {error}",
                    path.display()
                );
                PersistedSettings::default()
            }
        },
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => PersistedSettings::default(),
        Err(error) => {
            eprintln!("[settings] failed to read {}: {error}", path.display());
            PersistedSettings::default()
        }
    }
}

pub fn save_settings(settings: &PersistedSettings) -> Result<()> {
    let path = settings_path();
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent)
            .with_context(|| format!("failed to create settings dir {}", parent.display()))?;
    }
    let data = serde_json::to_string_pretty(settings).context("failed to serialize settings")?;
    fs::write(&path, data).with_context(|| format!("failed to write settings {}", path.display()))
}

fn settings_path() -> PathBuf {
    if let Some(explicit) = env::var_os("IMRANVIEW_CONFIG_FILE") {
        return PathBuf::from(explicit);
    }

    base_config_dir()
        .unwrap_or_else(|| PathBuf::from("."))
        .join("imranview")
        .join("settings.json")
}

fn base_config_dir() -> Option<PathBuf> {
    #[cfg(target_os = "windows")]
    {
        env::var_os("APPDATA").map(PathBuf::from)
    }

    #[cfg(not(target_os = "windows"))]
    {
        if let Some(xdg) = env::var_os("XDG_CONFIG_HOME") {
            return Some(PathBuf::from(xdg));
        }
        env::var_os("HOME")
            .map(PathBuf::from)
            .map(|home| home.join(".config"))
    }
}

pub fn is_existing_dir(path: &Path) -> bool {
    path.is_dir()
}
