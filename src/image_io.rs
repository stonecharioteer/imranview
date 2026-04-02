use std::fs::{self, File};
use std::io::BufReader;
use std::path::{Path, PathBuf};

use anyhow::{Context, Result};
use exif::{In, Tag};
use image::imageops::FilterType;
use image::{DynamicImage, GenericImageView, ImageReader};
use slint::{Image, Rgba8Pixel, SharedPixelBuffer};
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
    #[error("failed to scan images in directory {path}")]
    DirectoryScan {
        path: PathBuf,
        #[source]
        source: anyhow::Error,
    },
}

pub struct LoadedImage {
    pub image: Image,
    pub width: u32,
    pub height: u32,
    pub original_width: u32,
    pub original_height: u32,
    pub downscaled_for_preview: bool,
}

const SUPPORTED_EXTENSIONS: &[&str] = &[
    "avif", "bmp", "gif", "hdr", "heic", "heif", "ico", "jpeg", "jpg", "pbm", "pgm", "png", "pnm",
    "ppm", "qoi", "tif", "tiff", "webp",
];
const PREVIEW_MAX_DIMENSION: u32 = 4096;
const THUMBNAIL_MAX_DIMENSION: u32 = 192;

pub fn load_image(path: &Path) -> ImageIoResult<LoadedImage> {
    let oriented = decode_oriented_image(path).map_err(|source| ImageIoError::LoadImage {
        path: path.to_path_buf(),
        source,
    })?;
    let (original_width, original_height) = oriented.dimensions();
    let preview = downscale_for_preview(oriented);
    let downscaled_for_preview = preview.dimensions() != (original_width, original_height);
    let (image, width, height) = to_slint_image(preview);

    Ok(LoadedImage {
        image,
        width,
        height,
        original_width,
        original_height,
        downscaled_for_preview,
    })
}

pub fn load_thumbnail(path: &Path) -> ImageIoResult<Image> {
    let oriented = decode_oriented_image(path).map_err(|source| ImageIoError::LoadThumbnail {
        path: path.to_path_buf(),
        source,
    })?;
    let thumbnail = oriented.thumbnail(THUMBNAIL_MAX_DIMENSION, THUMBNAIL_MAX_DIMENSION);
    let (image, _, _) = to_slint_image(thumbnail);
    Ok(image)
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

fn downscale_for_preview(image: DynamicImage) -> DynamicImage {
    let (width, height) = image.dimensions();
    let max_dimension = width.max(height);
    if max_dimension <= PREVIEW_MAX_DIMENSION {
        return image;
    }

    image.resize(
        PREVIEW_MAX_DIMENSION,
        PREVIEW_MAX_DIMENSION,
        FilterType::Triangle,
    )
}

fn to_slint_image(image: DynamicImage) -> (Image, u32, u32) {
    let rgba = image.to_rgba8();
    let (width, height) = rgba.dimensions();
    let buffer = SharedPixelBuffer::<Rgba8Pixel>::clone_from_slice(rgba.as_raw(), width, height);
    (Image::from_rgba8(buffer), width, height)
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
