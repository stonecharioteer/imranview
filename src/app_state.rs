use std::collections::HashMap;
use std::fs;
use std::path::{Path, PathBuf};

use anyhow::{Context, Result};
use slint::Image;

use crate::image_io::{LoadedImage, collect_images_in_directory, load_image, load_thumbnail};

const ZOOM_MIN: f32 = 0.1;
const ZOOM_MAX: f32 = 16.0;
const ZOOM_STEP_IN: f32 = 1.2;
const ZOOM_STEP_OUT: f32 = 1.0 / ZOOM_STEP_IN;
const THUMBNAIL_WINDOW_RADIUS: usize = 90;

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
    last_error: Option<String>,
}

impl AppState {
    pub fn new() -> Self {
        Self {
            current_file: None,
            current_directory: None,
            images_in_directory: Vec::new(),
            current_index: None,
            current_image: None,
            thumbnail_cache: HashMap::new(),
            zoom_mode: ZoomMode::Fit,
            show_toolbar: true,
            show_status_bar: true,
            show_thumbnail_strip: false,
            last_error: None,
        }
    }

    pub fn open_image(&mut self, path: impl Into<PathBuf>) -> Result<()> {
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
        Ok(())
    }

    pub fn open_next(&mut self) -> Result<()> {
        self.open_adjacent(1)
    }

    pub fn open_previous(&mut self) -> Result<()> {
        self.open_adjacent(-1)
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
        if !self.show_thumbnail_strip {
            return Vec::new();
        }

        let Some(current_index) = self.current_index else {
            return Vec::new();
        };

        let total = self.images_in_directory.len();
        let start = current_index.saturating_sub(THUMBNAIL_WINDOW_RADIUS);
        let end = (current_index + THUMBNAIL_WINDOW_RADIUS + 1).min(total);
        let mut items = Vec::with_capacity(end.saturating_sub(start));

        for index in start..end {
            let path = self.images_in_directory[index].clone();
            let preview = self
                .thumbnail_cache
                .get(&path)
                .cloned()
                .or_else(|| match load_thumbnail(&path) {
                    Ok(thumbnail) => {
                        self.thumbnail_cache.insert(path.clone(), thumbnail.clone());
                        Some(thumbnail)
                    }
                    Err(_) => None,
                })
                .unwrap_or_default();

            let label = path
                .file_name()
                .map(|name| name.to_string_lossy().into_owned())
                .unwrap_or_else(|| path.display().to_string());

            items.push(ThumbnailView {
                source_index: index as i32,
                label,
                preview,
                current: index == current_index,
            });
        }

        items
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

    fn open_adjacent(&mut self, step: isize) -> Result<()> {
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

        self.open_image(next_path)
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

    fn prime_thumbnail_window(&mut self) {
        if !self.show_thumbnail_strip {
            return;
        }

        let Some(current_index) = self.current_index else {
            return;
        };

        let total = self.images_in_directory.len();
        let start = current_index.saturating_sub(THUMBNAIL_WINDOW_RADIUS);
        let end = (current_index + THUMBNAIL_WINDOW_RADIUS + 1).min(total);

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
