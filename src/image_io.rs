use std::fs;
use std::path::{Path, PathBuf};

use anyhow::{Context, Result};
use image::ImageReader;
use slint::{Image, Rgba8Pixel, SharedPixelBuffer};

pub struct LoadedImage {
    pub image: Image,
    pub width: u32,
    pub height: u32,
}

const SUPPORTED_EXTENSIONS: &[&str] = &[
    "avif", "bmp", "gif", "hdr", "heic", "heif", "ico", "jpeg", "jpg", "pbm", "pgm", "png", "pnm",
    "ppm", "qoi", "tif", "tiff", "webp",
];

pub fn load_image(path: &Path) -> Result<LoadedImage> {
    let decoded = ImageReader::open(path)
        .with_context(|| format!("failed to open {}", path.display()))?
        .with_guessed_format()
        .with_context(|| format!("failed to guess image format for {}", path.display()))?
        .decode()
        .with_context(|| format!("failed to decode {}", path.display()))?;

    let rgba = decoded.to_rgba8();
    let (width, height) = rgba.dimensions();
    let buffer = SharedPixelBuffer::<Rgba8Pixel>::clone_from_slice(rgba.as_raw(), width, height);

    Ok(LoadedImage {
        image: Image::from_rgba8(buffer),
        width,
        height,
    })
}

pub fn collect_images_in_directory(dir: &Path) -> Result<Vec<PathBuf>> {
    let mut files = Vec::new();

    for entry in
        fs::read_dir(dir).with_context(|| format!("failed to read directory {}", dir.display()))?
    {
        let entry = entry.with_context(|| format!("failed to inspect {}", dir.display()))?;
        let path = entry.path();

        if path.is_file() && is_supported_image_path(&path) {
            files.push(path);
        }
    }

    files.sort_by_key(|path| {
        path.file_name()
            .map(|name| name.to_string_lossy().to_ascii_lowercase())
    });

    Ok(files)
}

pub fn is_supported_image_path(path: &Path) -> bool {
    path.extension()
        .and_then(|ext| ext.to_str())
        .map(|ext| SUPPORTED_EXTENSIONS.contains(&ext.to_ascii_lowercase().as_str()))
        .unwrap_or(false)
}
