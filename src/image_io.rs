use std::fs::{self, File};
use std::io::{BufReader, BufWriter};
use std::path::{Path, PathBuf};
use std::sync::Arc;

use anyhow::{Context, Result, anyhow};
use exif::{In, Tag};
use image::codecs::jpeg::JpegEncoder;
use image::imageops::FilterType;
use image::{DynamicImage, GenericImageView, ImageFormat, ImageReader};
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

#[derive(Clone, Debug, Default)]
pub struct MetadataSummary {
    pub exif_fields: Vec<(String, String)>,
    pub iptc_fields: Vec<(String, String)>,
    pub xmp_fields: Vec<(String, String)>,
}

#[derive(Debug, Clone, Copy)]
pub enum SaveFormat {
    Png,
    Jpeg { quality: u8 },
    Webp,
    Bmp,
    Tiff,
}

impl SaveFormat {
    pub fn extension(self) -> &'static str {
        match self {
            SaveFormat::Png => "png",
            SaveFormat::Jpeg { .. } => "jpg",
            SaveFormat::Webp => "webp",
            SaveFormat::Bmp => "bmp",
            SaveFormat::Tiff => "tiff",
        }
    }
}

const SUPPORTED_EXTENSIONS: &[&str] = &[
    "avif", "bmp", "gif", "hdr", "heic", "heif", "ico", "jpeg", "jpg", "pbm", "pgm", "png", "pnm",
    "ppm", "qoi", "tif", "tiff", "webp",
];
const PREVIEW_MAX_DIMENSION: u32 = 4096;
const THUMBNAIL_MAX_DIMENSION: u32 = 192;
const EXIF_FIELD_LIMIT: usize = 32;

pub fn load_image_payload(path: &Path) -> ImageIoResult<LoadedImagePayload> {
    let working_image = load_working_image(path)?;
    Ok(payload_from_working_image(Arc::new(working_image)))
}

pub fn load_working_image(path: &Path) -> ImageIoResult<DynamicImage> {
    decode_oriented_image(path).map_err(|source| ImageIoError::LoadImage {
        path: path.to_path_buf(),
        source,
    })
}

pub fn extract_metadata_summary(path: &Path) -> MetadataSummary {
    let exif_fields = read_exif_fields(path);
    let blob = fs::read(path).ok();
    let (iptc_fields, xmp_fields) = if let Some(data) = blob.as_deref() {
        (extract_iptc_fields(data), extract_xmp_fields(data))
    } else {
        (Vec::new(), Vec::new())
    };

    MetadataSummary {
        exif_fields,
        iptc_fields,
        xmp_fields,
    }
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

#[cfg(test)]
pub fn save_image(path: &Path, image: &DynamicImage) -> ImageIoResult<()> {
    image
        .save(path)
        .with_context(|| format!("failed to save {}", path.display()))
        .map_err(|source| ImageIoError::SaveImage {
            path: path.to_path_buf(),
            source,
        })
}

pub fn save_image_with_format(
    path: &Path,
    image: &DynamicImage,
    format: SaveFormat,
) -> ImageIoResult<()> {
    let save_result = (|| -> Result<()> {
        match format {
            SaveFormat::Png => image
                .save_with_format(path, ImageFormat::Png)
                .with_context(|| format!("failed to save {}", path.display()))?,
            SaveFormat::Jpeg { quality } => {
                let file = File::create(path)
                    .with_context(|| format!("failed to create {}", path.display()))?;
                let mut writer = BufWriter::new(file);
                let mut encoder = JpegEncoder::new_with_quality(&mut writer, quality);
                encoder
                    .encode_image(image)
                    .with_context(|| format!("failed to encode jpeg {}", path.display()))?;
            }
            SaveFormat::Webp => image
                .save_with_format(path, ImageFormat::WebP)
                .with_context(|| format!("failed to save {}", path.display()))?,
            SaveFormat::Bmp => image
                .save_with_format(path, ImageFormat::Bmp)
                .with_context(|| format!("failed to save {}", path.display()))?,
            SaveFormat::Tiff => image
                .save_with_format(path, ImageFormat::Tiff)
                .with_context(|| format!("failed to save {}", path.display()))?,
        }
        Ok(())
    })();

    save_result.map_err(|source| ImageIoError::SaveImage {
        path: path.to_path_buf(),
        source,
    })
}

pub fn infer_save_format(path: &Path, jpeg_quality: u8) -> ImageIoResult<SaveFormat> {
    let Some(extension) = path.extension().and_then(|ext| ext.to_str()) else {
        return Err(ImageIoError::SaveImage {
            path: path.to_path_buf(),
            source: anyhow!("output path must include an extension"),
        });
    };

    let format = match extension.to_ascii_lowercase().as_str() {
        "png" => SaveFormat::Png,
        "jpg" | "jpeg" => SaveFormat::Jpeg {
            quality: jpeg_quality.clamp(1, 100),
        },
        "webp" => SaveFormat::Webp,
        "bmp" => SaveFormat::Bmp,
        "tif" | "tiff" => SaveFormat::Tiff,
        other => {
            return Err(ImageIoError::SaveImage {
                path: path.to_path_buf(),
                source: anyhow!("unsupported save extension: {other}"),
            });
        }
    };

    Ok(format)
}

pub fn preserve_metadata_best_effort(
    source: &Path,
    destination: &Path,
    format: SaveFormat,
) -> Result<()> {
    if !source.exists() {
        return Err(anyhow!(
            "cannot preserve metadata because source file is missing: {}",
            source.display()
        ));
    }
    match format {
        SaveFormat::Jpeg { .. } => copy_jpeg_metadata_segments(source, destination),
        _ => Err(anyhow!(
            "metadata preservation currently supports JPEG output only"
        )),
    }
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

fn read_exif_fields(path: &Path) -> Vec<(String, String)> {
    let file = match File::open(path) {
        Ok(file) => file,
        Err(_) => return Vec::new(),
    };
    let mut reader = BufReader::new(file);
    let exif = match exif::Reader::new().read_from_container(&mut reader) {
        Ok(exif) => exif,
        Err(_) => return Vec::new(),
    };

    exif.fields()
        .take(EXIF_FIELD_LIMIT)
        .map(|field| {
            (
                format!("{:?}", field.tag),
                field.display_value().with_unit(&exif).to_string(),
            )
        })
        .collect()
}

fn extract_xmp_fields(data: &[u8]) -> Vec<(String, String)> {
    let text = String::from_utf8_lossy(data);
    let marker_start = text
        .find("<x:xmpmeta")
        .or_else(|| text.find("<rdf:RDF"))
        .unwrap_or_default();
    if marker_start >= text.len() {
        return Vec::new();
    }

    let tail = &text[marker_start..];
    let marker_end = tail
        .find("</x:xmpmeta>")
        .map(|index| index + "</x:xmpmeta>".len())
        .or_else(|| {
            tail.find("</rdf:RDF>")
                .map(|index| index + "</rdf:RDF>".len())
        })
        .unwrap_or_else(|| tail.len().min(64 * 1024));
    let snippet = &tail[..marker_end.min(tail.len())];

    let mut fields = Vec::new();
    for tag in [
        "dc:title",
        "dc:creator",
        "dc:description",
        "xmp:CreateDate",
        "xmp:ModifyDate",
        "photoshop:Headline",
        "tiff:Make",
        "tiff:Model",
    ] {
        if let Some(value) = extract_xml_tag_value(snippet, tag) {
            fields.push((tag.to_owned(), value));
        }
    }

    if fields.is_empty() && !snippet.trim().is_empty() {
        fields.push(("raw".to_owned(), snippet.chars().take(240).collect()));
    }
    fields
}

fn extract_xml_tag_value(text: &str, tag: &str) -> Option<String> {
    let open_start = text.find(&format!("<{tag}"))?;
    let open_end = text[open_start..].find('>')? + open_start + 1;
    let close = text[open_end..].find(&format!("</{tag}>"))? + open_end;
    let value = text[open_end..close].trim();
    if value.is_empty() {
        None
    } else {
        Some(value.chars().take(240).collect())
    }
}

fn extract_iptc_fields(data: &[u8]) -> Vec<(String, String)> {
    let mut fields = Vec::new();
    let mut index = 0usize;
    while index + 12 < data.len() {
        if &data[index..index + 4] == b"8BIM" && data[index + 4] == 0x04 && data[index + 5] == 0x04
        {
            let mut cursor = index + 6;
            if cursor >= data.len() {
                break;
            }
            let name_len = data[cursor] as usize;
            cursor = cursor.saturating_add(1 + name_len);
            if cursor % 2 == 1 {
                cursor = cursor.saturating_add(1);
            }
            if cursor + 4 > data.len() {
                break;
            }
            let block_size = u32::from_be_bytes([
                data[cursor],
                data[cursor + 1],
                data[cursor + 2],
                data[cursor + 3],
            ]) as usize;
            cursor += 4;
            if cursor + block_size > data.len() {
                break;
            }
            let block = &data[cursor..cursor + block_size];
            parse_iptc_dataset(block, &mut fields);
            break;
        }
        index += 1;
    }

    fields
}

fn parse_iptc_dataset(block: &[u8], fields: &mut Vec<(String, String)>) {
    let mut cursor = 0usize;
    while cursor + 5 <= block.len() {
        if block[cursor] != 0x1c {
            cursor += 1;
            continue;
        }
        let record = block[cursor + 1];
        let dataset = block[cursor + 2];
        let length = u16::from_be_bytes([block[cursor + 3], block[cursor + 4]]) as usize;
        cursor += 5;
        if cursor + length > block.len() {
            break;
        }
        let raw_value = &block[cursor..cursor + length];
        cursor += length;

        let label = match (record, dataset) {
            (2, 5) => "ObjectName",
            (2, 55) => "DateCreated",
            (2, 80) => "Byline",
            (2, 120) => "Caption",
            (2, 116) => "CopyrightNotice",
            _ => continue,
        };
        let value = String::from_utf8_lossy(raw_value).trim().to_owned();
        if !value.is_empty() {
            fields.push((label.to_owned(), value.chars().take(240).collect()));
        }
        if fields.len() >= 16 {
            break;
        }
    }
}

#[derive(Clone)]
struct JpegSegment {
    marker: u8,
    bytes: Vec<u8>,
}

fn copy_jpeg_metadata_segments(source: &Path, destination: &Path) -> Result<()> {
    let source_bytes = fs::read(source)
        .with_context(|| format!("failed to read metadata source {}", source.display()))?;
    let destination_bytes = fs::read(destination).with_context(|| {
        format!(
            "failed to read save output before metadata merge {}",
            destination.display()
        )
    })?;

    let (source_segments, _) = split_jpeg_header_and_scan(&source_bytes)?;
    let (dest_segments, dest_scan) = split_jpeg_header_and_scan(&destination_bytes)?;
    let mut rebuilt = Vec::with_capacity(destination_bytes.len().saturating_add(16 * 1024));
    rebuilt.extend_from_slice(&[0xff, 0xd8]);

    for segment in dest_segments {
        if !is_jpeg_metadata_segment(segment.marker) {
            rebuilt.extend_from_slice(&segment.bytes);
        }
    }
    for segment in source_segments {
        if is_jpeg_metadata_segment(segment.marker) {
            rebuilt.extend_from_slice(&segment.bytes);
        }
    }
    rebuilt.extend_from_slice(&dest_scan);

    fs::write(destination, rebuilt)
        .with_context(|| format!("failed to write metadata-merged {}", destination.display()))?;
    Ok(())
}

fn is_jpeg_metadata_segment(marker: u8) -> bool {
    marker == 0xe1 || marker == 0xed
}

fn split_jpeg_header_and_scan(bytes: &[u8]) -> Result<(Vec<JpegSegment>, Vec<u8>)> {
    if bytes.len() < 4 || bytes[0] != 0xff || bytes[1] != 0xd8 {
        return Err(anyhow!("not a valid jpeg stream"));
    }

    let mut index = 2usize;
    let mut segments = Vec::new();
    while index + 1 < bytes.len() {
        if bytes[index] != 0xff {
            return Err(anyhow!("invalid jpeg marker layout"));
        }
        let marker = bytes[index + 1];
        if marker == 0xda {
            return Ok((segments, bytes[index..].to_vec()));
        }
        if marker == 0xd9 {
            return Ok((segments, vec![0xff, 0xd9]));
        }
        if (0xd0..=0xd7).contains(&marker) || marker == 0x01 {
            segments.push(JpegSegment {
                marker,
                bytes: bytes[index..index + 2].to_vec(),
            });
            index += 2;
            continue;
        }
        if index + 4 > bytes.len() {
            return Err(anyhow!("truncated jpeg segment"));
        }
        let length = u16::from_be_bytes([bytes[index + 2], bytes[index + 3]]) as usize;
        if length < 2 {
            return Err(anyhow!("invalid jpeg segment length"));
        }
        let end = index + 2 + length;
        if end > bytes.len() {
            return Err(anyhow!("truncated jpeg segment payload"));
        }
        segments.push(JpegSegment {
            marker,
            bytes: bytes[index..end].to_vec(),
        });
        index = end;
    }
    Err(anyhow!("jpeg scan data marker not found"))
}
