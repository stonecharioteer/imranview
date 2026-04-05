use std::env;
use std::fs;
use std::path::{Path, PathBuf};

use anyhow::{Context, Result};
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(default)]
pub struct PersistedSettings {
    pub show_toolbar: bool,
    pub show_status_bar: bool,
    pub show_metadata_panel: bool,
    pub show_thumbnail_strip: bool,
    pub thumbnails_window_mode: bool,
    pub last_open_directory: Option<PathBuf>,
    pub recent_files: Vec<PathBuf>,
    pub recent_directories: Vec<PathBuf>,
    pub slideshow_interval_secs: f32,
    pub thumbnail_sidebar_width: f32,
    pub thumbnail_grid_card_width: f32,
    pub thumb_cache_entry_cap: usize,
    pub thumb_cache_max_mb: usize,
    pub preload_cache_entry_cap: usize,
    pub preload_cache_max_mb: usize,
    pub window_position: Option<[f32; 2]>,
    pub window_inner_size: Option<[f32; 2]>,
    pub window_maximized: bool,
    pub window_fullscreen: bool,
    pub checkerboard_background: bool,
    pub smooth_main_scaling: bool,
    pub default_jpeg_quality: u8,
    pub auto_reopen_after_save: bool,
    pub hide_toolbar_in_fullscreen: bool,
    pub enable_color_management: bool,
    pub simulate_srgb_output: bool,
    pub display_gamma: f32,
    pub browsing_wrap_navigation: bool,
    pub zoom_step_percent: f32,
    pub video_frame_step_ms: u32,
    pub ui_language: String,
    pub skin_name: String,
    pub plugin_search_path: String,
    pub keep_single_instance: bool,
    pub confirm_delete: bool,
    pub confirm_overwrite: bool,
}

impl Default for PersistedSettings {
    fn default() -> Self {
        Self {
            show_toolbar: true,
            show_status_bar: true,
            show_metadata_panel: false,
            show_thumbnail_strip: false,
            thumbnails_window_mode: false,
            last_open_directory: None,
            recent_files: Vec::new(),
            recent_directories: Vec::new(),
            slideshow_interval_secs: 2.5,
            thumbnail_sidebar_width: 220.0,
            thumbnail_grid_card_width: 128.0,
            thumb_cache_entry_cap: 320,
            thumb_cache_max_mb: 96,
            preload_cache_entry_cap: 6,
            preload_cache_max_mb: 192,
            window_position: None,
            window_inner_size: None,
            window_maximized: false,
            window_fullscreen: false,
            checkerboard_background: false,
            smooth_main_scaling: true,
            default_jpeg_quality: 92,
            auto_reopen_after_save: true,
            hide_toolbar_in_fullscreen: false,
            enable_color_management: false,
            simulate_srgb_output: true,
            display_gamma: 2.2,
            browsing_wrap_navigation: true,
            zoom_step_percent: 20.0,
            video_frame_step_ms: 40,
            ui_language: "System".to_owned(),
            skin_name: "Classic".to_owned(),
            plugin_search_path: String::new(),
            keep_single_instance: true,
            confirm_delete: true,
            confirm_overwrite: true,
        }
    }
}

pub fn load_settings() -> PersistedSettings {
    let path = settings_path();
    match fs::read_to_string(&path) {
        Ok(json) => match serde_json::from_str::<PersistedSettings>(&json) {
            Ok(settings) => settings,
            Err(error) => {
                log::warn!(
                    target: "imranview::settings",
                    "invalid settings JSON at {}: {error}",
                    path.display()
                );
                PersistedSettings::default()
            }
        },
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => PersistedSettings::default(),
        Err(error) => {
            log::warn!(
                target: "imranview::settings",
                "failed to read {}: {error}",
                path.display()
            );
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
