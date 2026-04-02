use std::collections::HashMap;
use std::fs;
use std::path::{Path, PathBuf};
use std::time::Instant;

use anyhow::{Context, Result};
use image::DynamicImage;
use slint::Image;

use crate::image_io::{
    LoadedImage, collect_images_in_directory, load_image, load_thumbnail, refresh_loaded_image,
    save_image,
};
use crate::perf::{
    EDIT_IMAGE_BUDGET, NAVIGATION_BUDGET, OPEN_IMAGE_BUDGET, SAVE_IMAGE_BUDGET, log_timing,
};
use crate::settings::{PersistedSettings, is_existing_dir};

const ZOOM_MIN: f32 = 0.1;
const ZOOM_MAX: f32 = 16.0;
const ZOOM_STEP_IN: f32 = 1.2;
const ZOOM_STEP_OUT: f32 = 1.0 / ZOOM_STEP_IN;
const THUMBNAIL_WINDOW_RADIUS_STRIP: usize = 90;
const THUMBNAIL_DECODE_RADIUS_WINDOW_MODE: usize = 180;

#[derive(Clone)]
pub struct ThumbnailView {
    pub source_index: i32,
    pub label: String,
    pub preview: Image,
    pub current: bool,
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
    current_image: Option<LoadedImage>,
    thumbnail_cache: HashMap<PathBuf, Image>,
    zoom_mode: ZoomMode,
    show_toolbar: bool,
    show_status_bar: bool,
    show_thumbnail_strip: bool,
    thumbnails_window_mode: bool,
    last_error: Option<String>,
}

impl AppState {
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
            thumbnail_cache: HashMap::new(),
            zoom_mode: ZoomMode::Fit,
            show_toolbar: settings.show_toolbar,
            show_status_bar: settings.show_status_bar,
            show_thumbnail_strip,
            thumbnails_window_mode,
            last_error: None,
        }
    }

    pub fn to_settings(&self) -> PersistedSettings {
        PersistedSettings {
            show_toolbar: self.show_toolbar,
            show_status_bar: self.show_status_bar,
            show_thumbnail_strip: self.show_thumbnail_strip,
            thumbnails_window_mode: self.thumbnails_window_mode,
            last_open_directory: self
                .current_directory
                .as_ref()
                .filter(|path| is_existing_dir(path))
                .cloned(),
        }
    }

    pub fn open_image(&mut self, path: impl Into<PathBuf>) -> Result<()> {
        let started = Instant::now();
        let path = path.into();
        let loaded = load_image(&path)?;
        let directory = path
            .parent()
            .unwrap_or_else(|| Path::new("."))
            .to_path_buf();
        let files = collect_images_in_directory(&directory)?;
        self.reconcile_thumbnail_cache(&directory, &files);

        let current_index = files
            .iter()
            .position(|candidate| same_path(candidate, &path));
        self.current_file = Some(path);
        self.current_directory = Some(directory);
        self.images_in_directory = files;
        self.current_index = current_index;
        self.current_image = Some(loaded);
        self.zoom_mode = ZoomMode::Fit;
        self.prime_thumbnail_window();
        self.last_error = None;

        log_timing("open_image", started.elapsed(), OPEN_IMAGE_BUDGET);
        Ok(())
    }

    pub fn open_next(&mut self) -> Result<()> {
        self.open_adjacent("navigate_next", 1)
    }

    pub fn open_previous(&mut self) -> Result<()> {
        self.open_adjacent("navigate_previous", -1)
    }

    pub fn open_at_index(&mut self, index: i32) -> Result<()> {
        if index < 0 {
            anyhow::bail!("invalid image index");
        }
        let index = index as usize;
        let path = self
            .images_in_directory
            .get(index)
            .cloned()
            .context("invalid image index")?;
        self.open_image(path)
    }

    pub fn save_current(&mut self) -> Result<()> {
        let path = self
            .current_file
            .clone()
            .context("no image loaded to save")?;
        self.save_to_path(path, false)
    }

    pub fn save_current_as(&mut self, path: PathBuf) -> Result<()> {
        self.save_to_path(path, true)
    }

    pub fn rotate_left(&mut self) -> Result<()> {
        self.apply_edit("rotate_left", |image| image.rotate270())
    }

    pub fn rotate_right(&mut self) -> Result<()> {
        self.apply_edit("rotate_right", |image| image.rotate90())
    }

    pub fn flip_horizontal(&mut self) -> Result<()> {
        self.apply_edit("flip_horizontal", |image| image.fliph())
    }

    pub fn flip_vertical(&mut self) -> Result<()> {
        self.apply_edit("flip_vertical", |image| image.flipv())
    }

    pub fn set_zoom_fit(&mut self) {
        self.zoom_mode = ZoomMode::Fit;
    }

    pub fn set_zoom_actual(&mut self) {
        if self.current_image.is_some() {
            self.zoom_mode = ZoomMode::Manual(1.0);
        }
    }

    pub fn zoom_in(&mut self) {
        self.apply_zoom_step(ZOOM_STEP_IN);
    }

    pub fn zoom_out(&mut self) {
        self.apply_zoom_step(ZOOM_STEP_OUT);
    }

    pub fn zoom_from_wheel_delta(&mut self, delta_y: f32) {
        if delta_y > 0.0 {
            self.zoom_in();
        } else if delta_y < 0.0 {
            self.zoom_out();
        }
    }

    pub fn toggle_toolbar(&mut self) {
        self.show_toolbar = !self.show_toolbar;
    }

    pub fn toggle_status_bar(&mut self) {
        self.show_status_bar = !self.show_status_bar;
    }

    pub fn toggle_thumbnail_strip(&mut self) {
        self.show_thumbnail_strip = !self.show_thumbnail_strip;
        if self.show_thumbnail_strip {
            self.thumbnails_window_mode = false;
            self.prime_thumbnail_window();
        }
    }

    pub fn toggle_thumbnails_window_mode(&mut self) {
        self.thumbnails_window_mode = !self.thumbnails_window_mode;
        if self.thumbnails_window_mode {
            self.show_thumbnail_strip = false;
            self.prime_thumbnail_window();
        }
    }

    pub fn show_toolbar(&self) -> bool {
        self.show_toolbar
    }

    pub fn show_status_bar(&self) -> bool {
        self.show_status_bar
    }

    pub fn show_thumbnail_strip(&self) -> bool {
        self.show_thumbnail_strip
    }

    pub fn thumbnails_window_mode(&self) -> bool {
        self.thumbnails_window_mode
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

    pub fn suggested_save_name(&self) -> Option<String> {
        self.current_file.as_ref().and_then(|path| {
            path.file_name()
                .map(|name| name.to_string_lossy().into_owned())
        })
    }

    pub fn set_error(&mut self, message: impl Into<String>) {
        self.last_error = Some(message.into());
    }

    pub fn has_image(&self) -> bool {
        self.current_image.is_some()
    }

    pub fn current_image(&self) -> Option<Image> {
        self.current_image
            .as_ref()
            .map(|loaded| loaded.image.clone())
    }

    pub fn image_width(&self) -> f32 {
        self.current_image
            .as_ref()
            .map(|image| image.width as f32)
            .unwrap_or(0.0)
    }

    pub fn image_height(&self) -> f32 {
        self.current_image
            .as_ref()
            .map(|image| image.height as f32)
            .unwrap_or(0.0)
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

    pub fn thumbnail_entries(&mut self) -> Vec<ThumbnailView> {
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
                    format!("Preview: {} x {}", image.width, image.height)
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
                let zoom_w = (image.width as f32 * scale).max(1.0).round() as u32;
                let zoom_h = (image.height as f32 * scale).max(1.0).round() as u32;
                format!("{file_name} - ImranView (Zoom: {zoom_w} x {zoom_h})")
            }
        }
    }

    fn open_adjacent(&mut self, perf_label: &str, step: isize) -> Result<()> {
        let Some(current_index) = self.current_index else {
            anyhow::bail!("no image loaded");
        };
        let total = self.images_in_directory.len();
        if total <= 1 {
            return Ok(());
        }

        let next_index = wrapped_index(current_index, total, step);
        let next_path = self
            .images_in_directory
            .get(next_index)
            .cloned()
            .context("failed to resolve adjacent image path")?;

        let started = Instant::now();
        self.open_image(next_path)?;
        log_timing(perf_label, started.elapsed(), NAVIGATION_BUDGET);
        Ok(())
    }

    fn save_to_path(&mut self, path: PathBuf, switch_current_file: bool) -> Result<()> {
        let started = Instant::now();
        let loaded = self
            .current_image
            .as_ref()
            .context("no image loaded to save")?;
        save_image(&path, &loaded.working_image)?;
        log_timing("save_image", started.elapsed(), SAVE_IMAGE_BUDGET);

        if switch_current_file {
            self.open_image(path)?;
        } else {
            self.last_error = None;
        }
        Ok(())
    }

    fn apply_edit<F>(&mut self, perf_label: &str, transform: F) -> Result<()>
    where
        F: Fn(&DynamicImage) -> DynamicImage,
    {
        let started = Instant::now();
        let loaded = self.current_image.as_mut().context("no image loaded")?;
        loaded.working_image = transform(&loaded.working_image);
        refresh_loaded_image(loaded);
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

    fn thumbnail_entries_for_strip_mode(&mut self, current_index: usize) -> Vec<ThumbnailView> {
        let total = self.images_in_directory.len();
        let start = current_index.saturating_sub(THUMBNAIL_WINDOW_RADIUS_STRIP);
        let end = (current_index + THUMBNAIL_WINDOW_RADIUS_STRIP + 1).min(total);
        let mut items = Vec::with_capacity(end.saturating_sub(start));

        for index in start..end {
            items.push(self.make_thumbnail_view(index, true, current_index));
        }
        items
    }

    fn thumbnail_entries_for_window_mode(&mut self, current_index: usize) -> Vec<ThumbnailView> {
        let mut items = Vec::with_capacity(self.images_in_directory.len());
        for index in 0..self.images_in_directory.len() {
            let decode_now = index.abs_diff(current_index) <= THUMBNAIL_DECODE_RADIUS_WINDOW_MODE;
            items.push(self.make_thumbnail_view(index, decode_now, current_index));
        }
        items
    }

    fn make_thumbnail_view(
        &mut self,
        index: usize,
        decode_now: bool,
        current_index: usize,
    ) -> ThumbnailView {
        let path = self.images_in_directory[index].clone();
        let preview = self
            .thumbnail_cache
            .get(&path)
            .cloned()
            .or_else(|| {
                if !decode_now {
                    return None;
                }
                match load_thumbnail(&path) {
                    Ok(thumbnail) => {
                        self.thumbnail_cache.insert(path.clone(), thumbnail.clone());
                        Some(thumbnail)
                    }
                    Err(_) => None,
                }
            })
            .unwrap_or_default();

        let label = path
            .file_name()
            .map(|name| name.to_string_lossy().into_owned())
            .unwrap_or_else(|| path.display().to_string());

        ThumbnailView {
            source_index: index as i32,
            label,
            preview,
            current: index == current_index,
        }
    }

    fn prime_thumbnail_window(&mut self) {
        let Some(current_index) = self.current_index else {
            return;
        };

        let decode_radius = if self.thumbnails_window_mode {
            THUMBNAIL_DECODE_RADIUS_WINDOW_MODE
        } else if self.show_thumbnail_strip {
            THUMBNAIL_WINDOW_RADIUS_STRIP
        } else {
            return;
        };

        let start = current_index.saturating_sub(decode_radius);
        let end = (current_index + decode_radius + 1).min(self.images_in_directory.len());

        for index in start..end {
            let path = self.images_in_directory[index].clone();
            if self.thumbnail_cache.contains_key(&path) {
                continue;
            }
            if let Ok(thumbnail) = load_thumbnail(&path) {
                self.thumbnail_cache.insert(path, thumbnail);
            }
        }
    }

    fn reconcile_thumbnail_cache(&mut self, directory: &Path, files: &[PathBuf]) {
        let same_directory = self
            .current_directory
            .as_ref()
            .map(|current| same_path(current, directory))
            .unwrap_or(false);

        if !same_directory {
            self.thumbnail_cache.clear();
            return;
        }

        self.thumbnail_cache
            .retain(|cached_path, _| files.iter().any(|path| same_path(cached_path, path)));
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
    fn zoom_transitions_are_consistent() {
        let dir = tempdir().expect("failed to create temp dir");
        let image_a = dir.path().join("a.png");
        write_test_png(&image_a, 32, 24, [255, 128, 0, 255]);

        let mut state = AppState::new();
        state.open_image(&image_a).expect("failed to open image_a");

        assert!(state.zoom_is_fit());
        assert_eq!(state.zoom_label(), "Fit");

        state.zoom_in();
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
}
