use std::fs;
use std::path::{Path, PathBuf};
use std::sync::Arc;
#[cfg(test)]
use std::time::Instant;

use anyhow::{Context, Result};
use image::DynamicImage;

use crate::image_io::LoadedImagePayload;
#[cfg(test)]
use crate::image_io::{
    collect_images_in_directory, load_image_payload, payload_from_working_image, save_image,
};
#[cfg(test)]
use crate::perf::{
    EDIT_IMAGE_BUDGET, NAVIGATION_BUDGET, OPEN_IMAGE_BUDGET, SAVE_IMAGE_BUDGET, log_timing,
};
use crate::settings::{PersistedSettings, is_existing_dir};

const ZOOM_MIN: f32 = 0.1;
const ZOOM_MAX: f32 = 16.0;
const THUMBNAIL_WINDOW_RADIUS_STRIP: usize = 90;
const THUMBNAIL_DECODE_RADIUS_STRIP: usize = 12;
const THUMBNAIL_DECODE_RADIUS_WINDOW_MODE: usize = 36;
const THUMBNAIL_SIDEBAR_WIDTH_MIN: f32 = 160.0;
const THUMBNAIL_SIDEBAR_WIDTH_MAX: f32 = 420.0;
const THUMBNAIL_GRID_CARD_WIDTH_MIN: f32 = 96.0;
const THUMBNAIL_GRID_CARD_WIDTH_MAX: f32 = 240.0;
const RECENT_ITEMS_LIMIT: usize = 20;
const SLIDESHOW_INTERVAL_MIN_SECS: f32 = 0.5;
const SLIDESHOW_INTERVAL_MAX_SECS: f32 = 30.0;
const THUMB_CACHE_ENTRY_CAP_MIN: usize = 64;
const THUMB_CACHE_ENTRY_CAP_MAX: usize = 4096;
const THUMB_CACHE_MAX_MB_MIN: usize = 16;
const THUMB_CACHE_MAX_MB_MAX: usize = 1024;
const PRELOAD_CACHE_ENTRY_CAP_MIN: usize = 1;
const PRELOAD_CACHE_ENTRY_CAP_MAX: usize = 64;
const PRELOAD_CACHE_MAX_MB_MIN: usize = 32;
const PRELOAD_CACHE_MAX_MB_MAX: usize = 2048;
const EDIT_HISTORY_CAP: usize = 48;

#[derive(Clone)]
pub struct ThumbnailEntry {
    pub label: String,
    pub path: PathBuf,
    pub current: bool,
    pub decode_hint: bool,
}

#[derive(Clone)]
pub struct LoadedImageState {
    pub preview_rgba: Arc<[u8]>,
    pub preview_width: u32,
    pub preview_height: u32,
    pub original_width: u32,
    pub original_height: u32,
    pub downscaled_for_preview: bool,
    pub working_image: Arc<DynamicImage>,
}

impl LoadedImageState {
    pub fn from_payload(payload: LoadedImagePayload) -> Self {
        Self {
            preview_rgba: Arc::from(payload.preview_rgba.into_boxed_slice()),
            preview_width: payload.preview_width,
            preview_height: payload.preview_height,
            original_width: payload.original_width,
            original_height: payload.original_height,
            downscaled_for_preview: payload.downscaled_for_preview,
            working_image: payload.working_image,
        }
    }
}

enum ZoomMode {
    Fit,
    Manual(f32),
}

pub struct AppState {
    current_file: Option<PathBuf>,
    current_directory: Option<PathBuf>,
    images_in_directory: Vec<PathBuf>,
    current_index: Option<usize>,
    current_image: Option<LoadedImageState>,
    undo_stack: Vec<LoadedImageState>,
    redo_stack: Vec<LoadedImageState>,
    zoom_mode: ZoomMode,
    show_toolbar: bool,
    show_status_bar: bool,
    show_metadata_panel: bool,
    show_thumbnail_strip: bool,
    thumbnails_window_mode: bool,
    recent_files: Vec<PathBuf>,
    recent_directories: Vec<PathBuf>,
    slideshow_interval_secs: f32,
    thumbnail_sidebar_width: f32,
    thumbnail_grid_card_width: f32,
    thumb_cache_entry_cap: usize,
    thumb_cache_max_mb: usize,
    preload_cache_entry_cap: usize,
    preload_cache_max_mb: usize,
    window_position: Option<[f32; 2]>,
    window_inner_size: Option<[f32; 2]>,
    window_maximized: bool,
    window_fullscreen: bool,
    last_error: Option<String>,
}

impl AppState {
    #[cfg(test)]
    pub fn new() -> Self {
        Self::new_with_settings(PersistedSettings::default())
    }

    pub fn new_with_settings(settings: PersistedSettings) -> Self {
        let thumbnails_window_mode = settings.thumbnails_window_mode;
        let show_thumbnail_strip = if thumbnails_window_mode {
            false
        } else {
            settings.show_thumbnail_strip
        };

        Self {
            current_file: None,
            current_directory: settings
                .last_open_directory
                .filter(|path| is_existing_dir(path)),
            images_in_directory: Vec::new(),
            current_index: None,
            current_image: None,
            undo_stack: Vec::new(),
            redo_stack: Vec::new(),
            zoom_mode: ZoomMode::Fit,
            show_toolbar: settings.show_toolbar,
            show_status_bar: settings.show_status_bar,
            show_metadata_panel: settings.show_metadata_panel,
            show_thumbnail_strip,
            thumbnails_window_mode,
            recent_files: normalize_recent_paths(settings.recent_files),
            recent_directories: normalize_recent_paths(settings.recent_directories),
            slideshow_interval_secs: settings
                .slideshow_interval_secs
                .clamp(SLIDESHOW_INTERVAL_MIN_SECS, SLIDESHOW_INTERVAL_MAX_SECS),
            thumbnail_sidebar_width: settings
                .thumbnail_sidebar_width
                .clamp(THUMBNAIL_SIDEBAR_WIDTH_MIN, THUMBNAIL_SIDEBAR_WIDTH_MAX),
            thumbnail_grid_card_width: settings
                .thumbnail_grid_card_width
                .clamp(THUMBNAIL_GRID_CARD_WIDTH_MIN, THUMBNAIL_GRID_CARD_WIDTH_MAX),
            thumb_cache_entry_cap: settings
                .thumb_cache_entry_cap
                .clamp(THUMB_CACHE_ENTRY_CAP_MIN, THUMB_CACHE_ENTRY_CAP_MAX),
            thumb_cache_max_mb: settings
                .thumb_cache_max_mb
                .clamp(THUMB_CACHE_MAX_MB_MIN, THUMB_CACHE_MAX_MB_MAX),
            preload_cache_entry_cap: settings
                .preload_cache_entry_cap
                .clamp(PRELOAD_CACHE_ENTRY_CAP_MIN, PRELOAD_CACHE_ENTRY_CAP_MAX),
            preload_cache_max_mb: settings
                .preload_cache_max_mb
                .clamp(PRELOAD_CACHE_MAX_MB_MIN, PRELOAD_CACHE_MAX_MB_MAX),
            window_position: settings.window_position,
            window_inner_size: settings.window_inner_size,
            window_maximized: settings.window_maximized,
            window_fullscreen: settings.window_fullscreen,
            last_error: None,
        }
    }

    pub fn to_settings(&self) -> PersistedSettings {
        PersistedSettings {
            show_toolbar: self.show_toolbar,
            show_status_bar: self.show_status_bar,
            show_metadata_panel: self.show_metadata_panel,
            show_thumbnail_strip: self.show_thumbnail_strip,
            thumbnails_window_mode: self.thumbnails_window_mode,
            recent_files: self.recent_files.clone(),
            recent_directories: self.recent_directories.clone(),
            slideshow_interval_secs: self.slideshow_interval_secs,
            thumbnail_sidebar_width: self.thumbnail_sidebar_width,
            thumbnail_grid_card_width: self.thumbnail_grid_card_width,
            thumb_cache_entry_cap: self.thumb_cache_entry_cap,
            thumb_cache_max_mb: self.thumb_cache_max_mb,
            preload_cache_entry_cap: self.preload_cache_entry_cap,
            preload_cache_max_mb: self.preload_cache_max_mb,
            window_position: self.window_position,
            window_inner_size: self.window_inner_size,
            window_maximized: self.window_maximized,
            window_fullscreen: self.window_fullscreen,
            last_open_directory: self
                .current_directory
                .as_ref()
                .filter(|path| is_existing_dir(path))
                .cloned(),
            checkerboard_background: false,
            smooth_main_scaling: true,
            default_jpeg_quality: 92,
            auto_reopen_after_save: true,
            hide_toolbar_in_fullscreen: false,
            enable_color_management: false,
            simulate_srgb_output: true,
            display_gamma: 2.2,
            browsing_wrap_navigation: true,
            browsing_sort_mode: "name".to_owned(),
            browsing_sort_descending: false,
            thumbnails_sort_mode: "name".to_owned(),
            thumbnails_sort_descending: false,
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

    #[cfg(test)]
    pub fn open_image(&mut self, path: impl Into<PathBuf>) -> Result<()> {
        let started = Instant::now();
        let path = path.into();
        let payload = load_image_payload(&path)?;
        let directory = path
            .parent()
            .unwrap_or_else(|| Path::new("."))
            .to_path_buf();
        let files = collect_images_in_directory(&directory)?;

        self.apply_open_payload(path, directory, files, payload);
        log_timing("open_image", started.elapsed(), OPEN_IMAGE_BUDGET);
        Ok(())
    }

    pub fn apply_open_payload(
        &mut self,
        path: PathBuf,
        directory: PathBuf,
        files: Vec<PathBuf>,
        payload: LoadedImagePayload,
    ) {
        let first_open = self.current_image.is_none();
        let current_index = resolve_current_index(&files, &path);
        log::debug!(
            target: "imranview::state",
            "apply_open_payload path={} files={} current_index={:?} first_open={}",
            path.display(),
            files.len(),
            current_index,
            first_open
        );
        self.current_file = Some(path);
        self.current_directory = Some(directory);
        self.images_in_directory = files;
        self.current_index = current_index;
        self.current_image = Some(LoadedImageState::from_payload(payload));
        self.undo_stack.clear();
        self.redo_stack.clear();
        if let Some(current_path) = self.current_file.clone() {
            push_recent_path(&mut self.recent_files, current_path);
        }
        if let Some(current_directory) = self.current_directory.clone() {
            push_recent_path(&mut self.recent_directories, current_directory);
        }
        self.zoom_mode = ZoomMode::Fit;
        self.last_error = None;
        if first_open && !self.show_thumbnail_strip && !self.thumbnails_window_mode {
            self.show_thumbnail_strip = true;
        }
    }

    pub fn apply_transform_payload(&mut self, payload: LoadedImagePayload) -> Result<()> {
        let Some(current) = self.current_image.clone() else {
            anyhow::bail!("no image loaded")
        };
        self.undo_stack.push(current);
        if self.undo_stack.len() > EDIT_HISTORY_CAP {
            let drop_count = self.undo_stack.len() - EDIT_HISTORY_CAP;
            self.undo_stack.drain(0..drop_count);
        }
        self.current_image = Some(LoadedImageState::from_payload(payload));
        self.redo_stack.clear();
        self.last_error = None;
        Ok(())
    }

    pub fn can_undo(&self) -> bool {
        !self.undo_stack.is_empty()
    }

    pub fn can_redo(&self) -> bool {
        !self.redo_stack.is_empty()
    }

    pub fn undo_edit(&mut self) -> Result<()> {
        let Some(previous) = self.undo_stack.pop() else {
            anyhow::bail!("nothing to undo")
        };
        let Some(current) = self.current_image.clone() else {
            anyhow::bail!("no image loaded")
        };
        self.redo_stack.push(current);
        self.current_image = Some(previous);
        self.last_error = None;
        Ok(())
    }

    pub fn redo_edit(&mut self) -> Result<()> {
        let Some(next) = self.redo_stack.pop() else {
            anyhow::bail!("nothing to redo")
        };
        let Some(current) = self.current_image.clone() else {
            anyhow::bail!("no image loaded")
        };
        self.undo_stack.push(current);
        if self.undo_stack.len() > EDIT_HISTORY_CAP {
            let drop_count = self.undo_stack.len() - EDIT_HISTORY_CAP;
            self.undo_stack.drain(0..drop_count);
        }
        self.current_image = Some(next);
        self.last_error = None;
        Ok(())
    }

    #[cfg(test)]
    pub fn open_next(&mut self) -> Result<()> {
        self.open_adjacent("navigate_next", 1)
    }

    #[cfg(test)]
    pub fn open_previous(&mut self) -> Result<()> {
        self.open_adjacent("navigate_previous", -1)
    }

    pub fn resolve_next_path_with_wrap(&self, wrap_navigation: bool) -> Result<PathBuf> {
        self.resolve_adjacent_path(1, wrap_navigation)
    }

    pub fn resolve_previous_path_with_wrap(&self, wrap_navigation: bool) -> Result<PathBuf> {
        self.resolve_adjacent_path(-1, wrap_navigation)
    }

    #[cfg(test)]
    pub fn save_current_as(&mut self, path: PathBuf) -> Result<()> {
        self.save_to_path(path, true)
    }

    #[cfg(test)]
    pub fn rotate_right(&mut self) -> Result<()> {
        self.apply_edit("rotate_right", |image| image.rotate90())
    }

    pub fn current_working_image(&self) -> Result<Arc<DynamicImage>> {
        self.current_image
            .as_ref()
            .map(|image| Arc::clone(&image.working_image))
            .context("no image loaded")
    }

    pub fn current_file_path(&self) -> Option<PathBuf> {
        self.current_file.clone()
    }

    pub fn current_file_path_ref(&self) -> Option<&Path> {
        self.current_file.as_deref()
    }

    pub fn set_zoom_fit(&mut self) {
        self.zoom_mode = ZoomMode::Fit;
    }

    pub fn set_zoom_actual(&mut self) {
        if self.current_image.is_some() {
            self.zoom_mode = ZoomMode::Manual(1.0);
        }
    }

    pub fn zoom_in_by_percent(&mut self, percent: f32) {
        let step = zoom_step_factor(percent);
        self.apply_zoom_step(1.0 + step);
    }

    pub fn zoom_out_by_percent(&mut self, percent: f32) {
        let step = zoom_step_factor(percent);
        self.apply_zoom_step(1.0 / (1.0 + step));
    }

    pub fn set_show_toolbar(&mut self, show: bool) {
        self.show_toolbar = show;
    }

    pub fn set_show_status_bar(&mut self, show: bool) {
        self.show_status_bar = show;
    }

    pub fn set_show_metadata_panel(&mut self, show: bool) {
        self.show_metadata_panel = show;
    }

    pub fn toggle_thumbnail_strip(&mut self) {
        self.show_thumbnail_strip = !self.show_thumbnail_strip;
        if self.show_thumbnail_strip {
            self.thumbnails_window_mode = false;
        }
    }

    pub fn set_show_thumbnail_strip(&mut self, show: bool) {
        self.show_thumbnail_strip = show;
        if show {
            self.thumbnails_window_mode = false;
        }
    }

    pub fn toggle_thumbnails_window_mode(&mut self) {
        self.thumbnails_window_mode = !self.thumbnails_window_mode;
        if self.thumbnails_window_mode {
            self.show_thumbnail_strip = false;
        }
    }

    pub fn set_thumbnails_window_mode(&mut self, show: bool) {
        self.thumbnails_window_mode = show;
        if show {
            self.show_thumbnail_strip = false;
        }
    }

    pub fn set_thumbnail_sidebar_width(&mut self, width: f32) {
        self.thumbnail_sidebar_width =
            width.clamp(THUMBNAIL_SIDEBAR_WIDTH_MIN, THUMBNAIL_SIDEBAR_WIDTH_MAX);
    }

    pub fn set_thumbnail_grid_card_width(&mut self, width: f32) {
        self.thumbnail_grid_card_width =
            width.clamp(THUMBNAIL_GRID_CARD_WIDTH_MIN, THUMBNAIL_GRID_CARD_WIDTH_MAX);
    }

    pub fn show_toolbar(&self) -> bool {
        self.show_toolbar
    }

    pub fn show_status_bar(&self) -> bool {
        self.show_status_bar
    }

    pub fn show_metadata_panel(&self) -> bool {
        self.show_metadata_panel
    }

    pub fn show_thumbnail_strip(&self) -> bool {
        self.show_thumbnail_strip
    }

    pub fn thumbnails_window_mode(&self) -> bool {
        self.thumbnails_window_mode
    }

    pub fn thumbnail_sidebar_width(&self) -> f32 {
        self.thumbnail_sidebar_width
    }

    pub fn thumbnail_grid_card_width(&self) -> f32 {
        self.thumbnail_grid_card_width
    }

    pub fn recent_files(&self) -> &[PathBuf] {
        &self.recent_files
    }

    pub fn thumb_cache_entry_cap(&self) -> usize {
        self.thumb_cache_entry_cap
    }

    pub fn thumb_cache_max_mb(&self) -> usize {
        self.thumb_cache_max_mb
    }

    pub fn preload_cache_entry_cap(&self) -> usize {
        self.preload_cache_entry_cap
    }

    pub fn preload_cache_max_mb(&self) -> usize {
        self.preload_cache_max_mb
    }

    pub fn set_thumb_cache_entry_cap(&mut self, value: usize) {
        self.thumb_cache_entry_cap =
            value.clamp(THUMB_CACHE_ENTRY_CAP_MIN, THUMB_CACHE_ENTRY_CAP_MAX);
    }

    pub fn set_thumb_cache_max_mb(&mut self, value: usize) {
        self.thumb_cache_max_mb = value.clamp(THUMB_CACHE_MAX_MB_MIN, THUMB_CACHE_MAX_MB_MAX);
    }

    pub fn set_preload_cache_entry_cap(&mut self, value: usize) {
        self.preload_cache_entry_cap =
            value.clamp(PRELOAD_CACHE_ENTRY_CAP_MIN, PRELOAD_CACHE_ENTRY_CAP_MAX);
    }

    pub fn set_preload_cache_max_mb(&mut self, value: usize) {
        self.preload_cache_max_mb = value.clamp(PRELOAD_CACHE_MAX_MB_MIN, PRELOAD_CACHE_MAX_MB_MAX);
    }

    pub fn recent_directories(&self) -> &[PathBuf] {
        &self.recent_directories
    }

    pub fn slideshow_interval_secs(&self) -> f32 {
        self.slideshow_interval_secs
    }

    pub fn set_slideshow_interval_secs(&mut self, value: f32) {
        self.slideshow_interval_secs =
            value.clamp(SLIDESHOW_INTERVAL_MIN_SECS, SLIDESHOW_INTERVAL_MAX_SECS);
    }

    pub fn folder_label(&self) -> String {
        self.current_directory
            .as_ref()
            .map(|path| path.display().to_string())
            .unwrap_or_else(|| "No folder loaded".to_owned())
    }

    pub fn preferred_open_directory(&self) -> Option<PathBuf> {
        self.current_directory
            .as_ref()
            .filter(|path| is_existing_dir(path))
            .cloned()
    }

    pub fn current_directory_path(&self) -> Option<PathBuf> {
        self.current_directory.clone()
    }

    pub fn images_in_directory(&self) -> &[PathBuf] {
        &self.images_in_directory
    }

    pub fn reorder_images_in_directory(&mut self, files: Vec<PathBuf>) {
        self.images_in_directory = files;
        self.current_index = self
            .current_file
            .as_ref()
            .and_then(|path| resolve_current_index(&self.images_in_directory, path));
    }

    pub fn suggested_save_name(&self) -> Option<String> {
        self.current_file.as_ref().and_then(|path| {
            path.file_name()
                .map(|name| name.to_string_lossy().into_owned())
        })
    }

    pub fn set_error(&mut self, message: impl Into<String>) {
        self.last_error = Some(message.into());
    }

    pub fn clear_error(&mut self) {
        self.last_error = None;
    }

    pub fn error_message(&self) -> Option<&str> {
        self.last_error.as_deref()
    }

    pub fn has_image(&self) -> bool {
        self.current_image.is_some()
    }

    pub fn image_width(&self) -> f32 {
        self.current_image
            .as_ref()
            .map(|image| image.preview_width as f32)
            .unwrap_or(0.0)
    }

    pub fn image_height(&self) -> f32 {
        self.current_image
            .as_ref()
            .map(|image| image.preview_height as f32)
            .unwrap_or(0.0)
    }

    pub fn original_dimensions(&self) -> Option<(u32, u32)> {
        self.current_image
            .as_ref()
            .map(|image| (image.original_width, image.original_height))
    }

    pub fn preview_dimensions(&self) -> Option<(u32, u32)> {
        self.current_image
            .as_ref()
            .map(|image| (image.preview_width, image.preview_height))
    }

    pub fn downscaled_for_preview(&self) -> bool {
        self.current_image
            .as_ref()
            .map(|image| image.downscaled_for_preview)
            .unwrap_or(false)
    }

    pub fn zoom_is_fit(&self) -> bool {
        matches!(self.zoom_mode, ZoomMode::Fit)
    }

    pub fn zoom_factor(&self) -> f32 {
        match self.zoom_mode {
            ZoomMode::Fit => 1.0,
            ZoomMode::Manual(factor) => factor,
        }
    }

    pub fn zoom_label(&self) -> String {
        match self.zoom_mode {
            ZoomMode::Fit => "Fit".to_owned(),
            ZoomMode::Manual(factor) => format!("{:.0}%", factor * 100.0),
        }
    }

    pub fn image_counter_label(&self) -> String {
        if let Some(index) = self.current_index {
            format!("{}/{}", index + 1, self.images_in_directory.len())
        } else {
            "0/0".to_owned()
        }
    }

    pub fn current_preview_rgba(&self) -> Option<(Arc<[u8]>, u32, u32)> {
        self.current_image.as_ref().map(|image| {
            (
                Arc::clone(&image.preview_rgba),
                image.preview_width,
                image.preview_height,
            )
        })
    }

    pub fn thumbnail_entries(&self) -> Vec<ThumbnailEntry> {
        if !self.show_thumbnail_strip && !self.thumbnails_window_mode {
            return Vec::new();
        }

        let Some(current_index) = self.current_index else {
            return Vec::new();
        };

        if self.thumbnails_window_mode {
            return self.thumbnail_entries_for_window_mode(current_index);
        }
        self.thumbnail_entries_for_strip_mode(current_index)
    }

    pub fn status_dimensions(&self) -> String {
        if let Some(error) = &self.last_error {
            return format!("Error: {error}");
        }

        let Some(image) = &self.current_image else {
            return "Open image...".to_owned();
        };

        format!(
            "{} x {} x 32 BPP",
            image.original_width, image.original_height
        )
    }

    pub fn status_index(&self) -> String {
        if self.last_error.is_some() {
            return String::new();
        }
        self.image_counter_label()
    }

    pub fn status_zoom(&self) -> String {
        if self.last_error.is_some() {
            return String::new();
        }
        self.zoom_label()
    }

    pub fn status_size(&self) -> String {
        if self.last_error.is_some() {
            return String::new();
        }
        self.current_file
            .as_ref()
            .and_then(|path| fs::metadata(path).ok())
            .map(|meta| human_file_size(meta.len()))
            .unwrap_or_default()
    }

    pub fn status_preview(&self) -> String {
        if self.last_error.is_some() {
            return String::new();
        }

        self.current_image
            .as_ref()
            .map(|image| {
                if image.downscaled_for_preview {
                    format!(
                        "Preview: {} x {}",
                        image.preview_width, image.preview_height
                    )
                } else {
                    "Original".to_owned()
                }
            })
            .unwrap_or_default()
    }

    pub fn status_name(&self) -> String {
        if self.last_error.is_some() {
            return String::new();
        }
        self.current_file
            .as_ref()
            .and_then(|path| path.file_name())
            .map(|name| name.to_string_lossy().into_owned())
            .unwrap_or_default()
    }

    pub fn window_title(&self) -> String {
        let Some(path) = &self.current_file else {
            return "ImranView".to_owned();
        };

        let file_name = path
            .file_name()
            .map(|name| name.to_string_lossy().into_owned())
            .unwrap_or_else(|| path.display().to_string());

        let Some(image) = &self.current_image else {
            return format!("{file_name} - ImranView");
        };

        match self.zoom_mode {
            ZoomMode::Fit => format!("{file_name} - ImranView"),
            ZoomMode::Manual(scale) => {
                let zoom_w = (image.preview_width as f32 * scale).max(1.0).round() as u32;
                let zoom_h = (image.preview_height as f32 * scale).max(1.0).round() as u32;
                format!("{file_name} - ImranView (Zoom: {zoom_w} x {zoom_h})")
            }
        }
    }

    #[cfg(test)]
    fn open_adjacent(&mut self, perf_label: &str, step: isize) -> Result<()> {
        let next_path = self.resolve_adjacent_path(step, true)?;

        let started = Instant::now();
        self.open_image(next_path)?;
        log_timing(perf_label, started.elapsed(), NAVIGATION_BUDGET);
        Ok(())
    }

    #[cfg(test)]
    fn save_to_path(&mut self, path: PathBuf, switch_current_file: bool) -> Result<()> {
        let started = Instant::now();
        let loaded = self
            .current_image
            .as_ref()
            .context("no image loaded to save")?;
        save_image(&path, loaded.working_image.as_ref())?;
        log_timing("save_image", started.elapsed(), SAVE_IMAGE_BUDGET);

        if switch_current_file {
            self.open_image(path)?;
        } else {
            self.last_error = None;
        }
        Ok(())
    }

    #[cfg(test)]
    fn apply_edit<F>(&mut self, perf_label: &str, transform: F) -> Result<()>
    where
        F: Fn(&DynamicImage) -> DynamicImage,
    {
        let started = Instant::now();
        let loaded = self.current_image.as_ref().context("no image loaded")?;
        let transformed = transform(loaded.working_image.as_ref());
        let payload = payload_from_working_image(Arc::new(transformed));
        self.current_image = Some(LoadedImageState::from_payload(payload));
        self.last_error = None;
        log_timing(perf_label, started.elapsed(), EDIT_IMAGE_BUDGET);
        Ok(())
    }

    fn apply_zoom_step(&mut self, step_factor: f32) {
        if self.current_image.is_none() {
            return;
        }
        let current = match self.zoom_mode {
            ZoomMode::Fit => 1.0,
            ZoomMode::Manual(factor) => factor,
        };
        self.zoom_mode = ZoomMode::Manual((current * step_factor).clamp(ZOOM_MIN, ZOOM_MAX));
    }

    fn thumbnail_entries_for_strip_mode(&self, current_index: usize) -> Vec<ThumbnailEntry> {
        let total = self.images_in_directory.len();
        let start = current_index.saturating_sub(THUMBNAIL_WINDOW_RADIUS_STRIP);
        let end = (current_index + THUMBNAIL_WINDOW_RADIUS_STRIP + 1).min(total);
        let mut items = Vec::with_capacity(end.saturating_sub(start));

        for index in start..end {
            items.push(self.make_thumbnail_entry(
                index,
                current_index,
                THUMBNAIL_DECODE_RADIUS_STRIP,
            ));
        }
        items
    }

    fn thumbnail_entries_for_window_mode(&self, current_index: usize) -> Vec<ThumbnailEntry> {
        let mut items = Vec::with_capacity(self.images_in_directory.len());
        for index in 0..self.images_in_directory.len() {
            items.push(self.make_thumbnail_entry(
                index,
                current_index,
                THUMBNAIL_DECODE_RADIUS_WINDOW_MODE,
            ));
        }
        items
    }

    fn make_thumbnail_entry(
        &self,
        index: usize,
        current_index: usize,
        decode_radius: usize,
    ) -> ThumbnailEntry {
        let path = self.images_in_directory[index].clone();
        let label = path
            .file_name()
            .map(|name| name.to_string_lossy().into_owned())
            .unwrap_or_else(|| path.display().to_string());

        ThumbnailEntry {
            label,
            path,
            current: index == current_index,
            decode_hint: index.abs_diff(current_index) <= decode_radius,
        }
    }

    fn resolve_adjacent_path(&self, step: isize, wrap_navigation: bool) -> Result<PathBuf> {
        let Some(current_index) = self.current_index else {
            anyhow::bail!("no image loaded");
        };
        let total = self.images_in_directory.len();
        if total == 0 {
            anyhow::bail!("no image loaded");
        }

        if !wrap_navigation {
            let candidate = current_index as isize + step;
            if candidate < 0 {
                anyhow::bail!("already at first image");
            }
            if candidate >= total as isize {
                anyhow::bail!("already at last image");
            }
            let next_index = candidate as usize;
            return self
                .images_in_directory
                .get(next_index)
                .cloned()
                .context("failed to resolve adjacent image path");
        }

        let next_index = wrapped_index(current_index, total, step);
        self.images_in_directory
            .get(next_index)
            .cloned()
            .context("failed to resolve adjacent image path")
    }

    pub fn update_window_state(
        &mut self,
        window_position: Option<[f32; 2]>,
        window_inner_size: Option<[f32; 2]>,
        window_maximized: Option<bool>,
        window_fullscreen: Option<bool>,
    ) -> bool {
        let mut changed = false;
        if self.window_position != window_position {
            self.window_position = window_position;
            changed = true;
        }
        if self.window_inner_size != window_inner_size {
            self.window_inner_size = window_inner_size;
            changed = true;
        }
        if let Some(window_maximized) = window_maximized {
            if self.window_maximized != window_maximized {
                self.window_maximized = window_maximized;
                changed = true;
            }
        }
        if let Some(window_fullscreen) = window_fullscreen {
            if self.window_fullscreen != window_fullscreen {
                self.window_fullscreen = window_fullscreen;
                changed = true;
            }
        }
        changed
    }
}

fn wrapped_index(current: usize, len: usize, step: isize) -> usize {
    if len == 0 {
        return 0;
    }

    let len = len as isize;
    let next = (current as isize + step).rem_euclid(len);
    next as usize
}

fn zoom_step_factor(percent: f32) -> f32 {
    (percent.clamp(1.0, 200.0) / 100.0).max(0.01)
}

fn resolve_current_index(files: &[PathBuf], path: &Path) -> Option<usize> {
    files
        .iter()
        .position(|candidate| candidate == path)
        .or_else(|| {
            files
                .iter()
                .position(|candidate| same_path(candidate, path))
        })
        .or_else(|| {
            let target_name = path.file_name()?.to_string_lossy().to_ascii_lowercase();
            files.iter().position(|candidate| {
                candidate
                    .file_name()
                    .map(|name| name.to_string_lossy().to_ascii_lowercase() == target_name)
                    .unwrap_or(false)
            })
        })
        .or_else(|| (!files.is_empty()).then_some(0))
}

fn same_path(a: &Path, b: &Path) -> bool {
    if a == b {
        return true;
    }

    match (a.canonicalize(), b.canonicalize()) {
        (Ok(a), Ok(b)) => a == b,
        _ => false,
    }
}

fn human_file_size(bytes: u64) -> String {
    const KB: f64 = 1024.0;
    const MB: f64 = KB * 1024.0;
    const GB: f64 = MB * 1024.0;

    let value = bytes as f64;
    if value < KB {
        format!("{bytes} B")
    } else if value < MB {
        format!("{:.1} KB", value / KB)
    } else if value < GB {
        format!("{:.2} MB", value / MB)
    } else {
        format!("{:.2} GB", value / GB)
    }
}

fn normalize_recent_paths(paths: Vec<PathBuf>) -> Vec<PathBuf> {
    let mut normalized = Vec::new();
    for path in paths {
        push_recent_path(&mut normalized, path);
    }
    normalized
}

fn push_recent_path(list: &mut Vec<PathBuf>, path: PathBuf) {
    list.retain(|existing| existing != &path);
    list.insert(0, path);
    if list.len() > RECENT_ITEMS_LIMIT {
        list.truncate(RECENT_ITEMS_LIMIT);
    }
}

#[cfg(test)]
mod tests {
    use std::path::Path;

    use image::{DynamicImage, GenericImageView, Rgba, RgbaImage};
    use tempfile::tempdir;

    use super::AppState;

    fn write_test_png(path: &Path, width: u32, height: u32, color: [u8; 4]) {
        let image = RgbaImage::from_pixel(width, height, Rgba(color));
        DynamicImage::ImageRgba8(image)
            .save(path)
            .expect("failed to write test image");
    }

    #[test]
    fn navigation_wraps_between_first_and_last() {
        let dir = tempdir().expect("failed to create temp dir");
        let image_a = dir.path().join("a.png");
        let image_b = dir.path().join("b.png");
        let image_c = dir.path().join("c.png");

        write_test_png(&image_a, 16, 12, [255, 0, 0, 255]);
        write_test_png(&image_b, 16, 12, [0, 255, 0, 255]);
        write_test_png(&image_c, 16, 12, [0, 0, 255, 255]);

        let mut state = AppState::new();
        state.open_image(&image_a).expect("failed to open image_a");
        assert_eq!(state.image_counter_label(), "1/3");

        state
            .open_previous()
            .expect("failed to wrap previous to last image");
        assert_eq!(state.status_name(), "c.png");

        state
            .open_next()
            .expect("failed to wrap next to first image");
        assert_eq!(state.status_name(), "a.png");
    }

    #[test]
    fn navigation_without_wrap_stops_at_boundaries() {
        let dir = tempdir().expect("failed to create temp dir");
        let image_a = dir.path().join("a.png");
        let image_b = dir.path().join("b.png");
        write_test_png(&image_a, 16, 12, [255, 0, 0, 255]);
        write_test_png(&image_b, 16, 12, [0, 255, 0, 255]);

        let mut state = AppState::new();
        state.open_image(&image_a).expect("failed to open image_a");

        let previous = state.resolve_previous_path_with_wrap(false);
        assert!(previous.is_err());

        state.open_image(&image_b).expect("failed to open image_b");
        let next = state.resolve_next_path_with_wrap(false);
        assert!(next.is_err());
    }

    #[test]
    fn zoom_transitions_are_consistent() {
        let dir = tempdir().expect("failed to create temp dir");
        let image_a = dir.path().join("a.png");
        write_test_png(&image_a, 32, 24, [255, 128, 0, 255]);

        let mut state = AppState::new();
        state.open_image(&image_a).expect("failed to open image_a");

        assert!(state.zoom_is_fit());
        assert_eq!(state.zoom_label(), "Fit");

        state.zoom_in_by_percent(20.0);
        assert!(!state.zoom_is_fit());
        assert_ne!(state.zoom_label(), "Fit");

        state.set_zoom_actual();
        assert_eq!(state.zoom_label(), "100%");
        assert!((state.zoom_factor() - 1.0).abs() < f32::EPSILON);

        state.set_zoom_fit();
        assert!(state.zoom_is_fit());
        assert_eq!(state.zoom_label(), "Fit");
    }

    #[test]
    fn successful_open_clears_previous_error() {
        let dir = tempdir().expect("failed to create temp dir");
        let image_a = dir.path().join("a.png");
        write_test_png(&image_a, 16, 16, [128, 128, 128, 255]);

        let mut state = AppState::new();
        state.set_error("synthetic failure");
        assert!(state.status_dimensions().starts_with("Error:"));

        state.open_image(&image_a).expect("failed to open image_a");
        assert!(!state.status_dimensions().starts_with("Error:"));
    }

    #[test]
    fn save_as_writes_file_and_switches_current_file() {
        let dir = tempdir().expect("failed to create temp dir");
        let source = dir.path().join("source.png");
        let saved = dir.path().join("edited.png");

        write_test_png(&source, 16, 10, [64, 64, 255, 255]);

        let mut state = AppState::new();
        state
            .open_image(&source)
            .expect("failed to open source image");
        state
            .rotate_right()
            .expect("failed to rotate source image before save");
        state
            .save_current_as(saved.clone())
            .expect("failed to save image as edited.png");

        assert!(saved.exists());
        assert_eq!(state.status_name(), "edited.png");

        let saved_image = image::open(&saved).expect("failed to open saved file");
        assert_eq!(saved_image.dimensions(), (10, 16));
    }

    #[test]
    fn settings_roundtrip_includes_viewport_and_thumbnail_preferences() {
        let mut state = AppState::new();
        state.set_thumbnail_sidebar_width(999.0);
        state.set_thumbnail_grid_card_width(40.0);
        state.set_show_metadata_panel(true);
        state.set_slideshow_interval_secs(99.0);
        state.update_window_state(
            Some([10.5, 14.0]),
            Some([1280.0, 720.0]),
            Some(true),
            Some(false),
        );

        let settings = state.to_settings();
        assert_eq!(settings.thumbnail_sidebar_width, 420.0);
        assert_eq!(settings.thumbnail_grid_card_width, 96.0);
        assert!(settings.show_metadata_panel);
        assert_eq!(settings.slideshow_interval_secs, 30.0);
        assert_eq!(settings.window_position, Some([10.5, 14.0]));
        assert_eq!(settings.window_inner_size, Some([1280.0, 720.0]));
        assert!(settings.window_maximized);
        assert!(!settings.window_fullscreen);
    }

    #[test]
    fn opening_image_updates_recent_files_and_folders() {
        let dir = tempdir().expect("failed to create temp dir");
        let first = dir.path().join("first.png");
        let second = dir.path().join("second.png");
        write_test_png(&first, 8, 8, [255, 0, 0, 255]);
        write_test_png(&second, 8, 8, [0, 255, 0, 255]);

        let mut state = AppState::new();
        state
            .open_image(&first)
            .expect("failed to open first image");
        state
            .open_image(&second)
            .expect("failed to open second image");

        assert_eq!(state.recent_files().first(), Some(&second));
        assert_eq!(
            state.recent_directories().first(),
            Some(&dir.path().to_path_buf())
        );
    }
}
