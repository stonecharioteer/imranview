use std::path::{Path, PathBuf};

use anyhow::{Context, Result};
use slint::Image;

use crate::image_io::{LoadedImage, collect_images_in_directory, load_image};

pub struct AppState {
    current_file: Option<PathBuf>,
    images_in_directory: Vec<PathBuf>,
    current_index: Option<usize>,
    current_image: Option<LoadedImage>,
    last_error: Option<String>,
}

impl AppState {
    pub fn new() -> Self {
        Self {
            current_file: None,
            images_in_directory: Vec::new(),
            current_index: None,
            current_image: None,
            last_error: None,
        }
    }

    pub fn open_image(&mut self, path: impl Into<PathBuf>) -> Result<()> {
        let path = path.into();
        let loaded = load_image(&path)?;
        let directory = path.parent().unwrap_or_else(|| Path::new("."));
        let files = collect_images_in_directory(directory)?;

        let current_index = files
            .iter()
            .position(|candidate| same_path(candidate, &path));
        self.current_file = Some(path);
        self.images_in_directory = files;
        self.current_index = current_index;
        self.current_image = Some(loaded);
        self.last_error = None;
        Ok(())
    }

    pub fn open_next(&mut self) -> Result<()> {
        self.open_adjacent(1)
    }

    pub fn open_previous(&mut self) -> Result<()> {
        self.open_adjacent(-1)
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

    pub fn status_line(&self) -> String {
        if let Some(error) = &self.last_error {
            return format!("Error: {error}");
        }

        let Some(image) = &self.current_image else {
            return "Open an image to start".to_owned();
        };

        let name = self
            .current_file
            .as_ref()
            .and_then(|path| path.file_name())
            .map(|name| name.to_string_lossy().into_owned())
            .unwrap_or_else(|| "Untitled".to_owned());

        if let Some(index) = self.current_index {
            format!(
                "{name} | {}x{} | {}/{}",
                image.width,
                image.height,
                index + 1,
                self.images_in_directory.len()
            )
        } else {
            format!("{name} | {}x{}", image.width, image.height)
        }
    }

    pub fn window_title(&self) -> String {
        match self.current_file.as_ref() {
            Some(path) => {
                let name = path
                    .file_name()
                    .map(|name| name.to_string_lossy().into_owned())
                    .unwrap_or_else(|| path.display().to_string());
                format!("ImranView - {name}")
            }
            None => "ImranView".to_owned(),
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
