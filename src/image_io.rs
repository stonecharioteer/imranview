use std::fs::{self, File};
use std::io::BufReader;
use std::path::{Path, PathBuf};
use std::sync::Arc;

use anyhow::{Context, Result};
use exif::{In, Tag};
use image::imageops::FilterType;
use image::{DynamicImage, GenericImageView, ImageReader};
use thiserror::Error;

pub type ImageIoResult<T> = std::result::Result<T, ImageIoError>;

#[derive(Debug, Error)]
pub enum ImageIoError {
    #[error("failed to load image from {path}")]
    LoadImage {
        path: PathBuf,
        #[source]
        source: anyhow::Error,
    },
    #[error("failed to load thumbnail from {path}")]
    LoadThumbnail {
        path: PathBuf,
        #[source]
        source: anyhow::Error,
    },
    #[error("failed to save image to {path}")]
    SaveImage {
        path: PathBuf,
        #[source]
        source: anyhow::Error,
    },
    #[error("failed to scan images in directory {path}")]
    DirectoryScan {
        path: PathBuf,
        #[source]
        source: anyhow::Error,
    },
}

pub struct LoadedImagePayload {
    pub preview_rgba: Vec<u8>,
    pub preview_width: u32,
    pub preview_height: u32,
    pub original_width: u32,
    pub original_height: u32,
    pub downscaled_for_preview: bool,
    pub working_image: Arc<DynamicImage>,
}

pub struct ThumbnailPayload {
    pub rgba: Vec<u8>,
    pub width: u32,
    pub height: u32,
}

const SUPPORTED_EXTENSIONS: &[&str] = &[
    "avif", "bmp", "gif", "hdr", "heic", "heif", "ico", "jpeg", "jpg", "pbm", "pgm", "png", "pnm",
    "ppm", "qoi", "tif", "tiff", "webp",
];
const PREVIEW_MAX_DIMENSION: u32 = 4096;
const THUMBNAIL_MAX_DIMENSION: u32 = 192;

pub fn load_image_payload(path: &Path) -> ImageIoResult<LoadedImagePayload> {
    let working_image = decode_oriented_image(path).map_err(|source| ImageIoError::LoadImage {
        path: path.to_path_buf(),
        source,
    })?;
    Ok(payload_from_working_image(Arc::new(working_image)))
}

pub fn payload_from_working_image(working_image: Arc<DynamicImage>) -> LoadedImagePayload {
    let (preview_rgba, preview_width, preview_height, downscaled_for_preview) =
        render_preview_rgba(working_image.as_ref());
    let (original_width, original_height) = working_image.dimensions();
    LoadedImagePayload {
        preview_rgba,
        preview_width,
        preview_height,
        original_width,
        original_height,
        downscaled_for_preview,
        working_image,
    }
}

pub fn load_thumbnail_payload(path: &Path) -> ImageIoResult<ThumbnailPayload> {
    let oriented = decode_oriented_image(path).map_err(|source| ImageIoError::LoadThumbnail {
        path: path.to_path_buf(),
        source,
    })?;
    let thumbnail = oriented.thumbnail(THUMBNAIL_MAX_DIMENSION, THUMBNAIL_MAX_DIMENSION);
    let (rgba, width, height) = to_rgba_bytes(&thumbnail);
    Ok(ThumbnailPayload {
        rgba,
        width,
        height,
    })
}

pub fn save_image(path: &Path, image: &DynamicImage) -> ImageIoResult<()> {
    image
        .save(path)
        .with_context(|| format!("failed to save {}", path.display()))
        .map_err(|source| ImageIoError::SaveImage {
            path: path.to_path_buf(),
            source,
        })
}

pub fn collect_images_in_directory(dir: &Path) -> ImageIoResult<Vec<PathBuf>> {
    let scanned = (|| -> Result<Vec<PathBuf>> {
        let mut files = Vec::new();

        for entry in fs::read_dir(dir)
            .with_context(|| format!("failed to read directory {}", dir.display()))?
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
    })();

    scanned.map_err(|source| ImageIoError::DirectoryScan {
        path: dir.to_path_buf(),
        source,
    })
}

pub fn is_supported_image_path(path: &Path) -> bool {
    path.extension()
        .and_then(|ext| ext.to_str())
        .map(|ext| SUPPORTED_EXTENSIONS.contains(&ext.to_ascii_lowercase().as_str()))
        .unwrap_or(false)
}

fn decode_oriented_image(path: &Path) -> Result<DynamicImage> {
    let decoded = ImageReader::open(path)
        .with_context(|| format!("failed to open {}", path.display()))?
        .with_guessed_format()
        .with_context(|| format!("failed to guess image format for {}", path.display()))?
        .decode()
        .with_context(|| format!("failed to decode {}", path.display()))?;

    Ok(apply_exif_orientation(path, decoded))
}

fn render_preview_rgba(image: &DynamicImage) -> (Vec<u8>, u32, u32, bool) {
    let (width, height) = image.dimensions();
    let max_dimension = width.max(height);
    if max_dimension > PREVIEW_MAX_DIMENSION {
        let preview = image.resize(
            PREVIEW_MAX_DIMENSION,
            PREVIEW_MAX_DIMENSION,
            FilterType::Triangle,
        );
        let (rgba, w, h) = to_rgba_bytes(&preview);
        return (rgba, w, h, true);
    }

    let (rgba, w, h) = to_rgba_bytes(image);
    (rgba, w, h, false)
}

fn to_rgba_bytes(image: &DynamicImage) -> (Vec<u8>, u32, u32) {
    let rgba = image.to_rgba8();
    let (width, height) = rgba.dimensions();
    (rgba.into_raw(), width, height)
}

fn apply_exif_orientation(path: &Path, image: DynamicImage) -> DynamicImage {
    match read_exif_orientation(path) {
        Some(2) => image.fliph(),
        Some(3) => image.rotate180(),
        Some(4) => image.flipv(),
        Some(5) => image.fliph().rotate90(),
        Some(6) => image.rotate90(),
        Some(7) => image.fliph().rotate270(),
        Some(8) => image.rotate270(),
        _ => image,
    }
}

fn read_exif_orientation(path: &Path) -> Option<u32> {
    let file = File::open(path).ok()?;
    let mut reader = BufReader::new(file);
    let exif = exif::Reader::new().read_from_container(&mut reader).ok()?;
    exif.get_field(Tag::Orientation, In::PRIMARY)
        .and_then(|field| field.value.get_uint(0))
}
