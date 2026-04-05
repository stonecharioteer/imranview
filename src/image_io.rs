use std::fs::{self, File};
use std::io::{BufReader, BufWriter, Read, Write};
use std::path::{Path, PathBuf};
use std::sync::Arc;

use anyhow::{Context, Result, anyhow};
use exif::{In, Tag};
use flate2::Compression;
use flate2::read::ZlibDecoder;
use flate2::write::ZlibEncoder;
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

pub fn extract_embedded_icc_profile(path: &Path) -> Result<Option<Vec<u8>>> {
    let extension = path
        .extension()
        .and_then(|value| value.to_str())
        .map(|value| value.to_ascii_lowercase())
        .unwrap_or_default();
    match extension.as_str() {
        "jpg" | "jpeg" => extract_jpeg_icc_profile(path),
        "png" => extract_png_icc_profile(path),
        _ => Ok(None),
    }
}

pub fn embed_icc_profile_best_effort(path: &Path, format: SaveFormat, icc: &[u8]) -> Result<()> {
    match format {
        SaveFormat::Jpeg { .. } => embed_jpeg_icc_profile(path, icc),
        SaveFormat::Png => embed_png_icc_profile(path, icc),
        _ => Err(anyhow!(
            "ICC embedding currently supports JPEG and PNG output only"
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

#[derive(Clone)]
struct PngChunk {
    kind: [u8; 4],
    data: Vec<u8>,
}

fn extract_png_icc_profile(path: &Path) -> Result<Option<Vec<u8>>> {
    let bytes = fs::read(path)
        .with_context(|| format!("failed to read PNG for ICC extraction {}", path.display()))?;
    let chunks = parse_png_chunks(&bytes)?;
    for chunk in chunks {
        if &chunk.kind != b"iCCP" {
            continue;
        }
        let Some(name_end) = chunk.data.iter().position(|&byte| byte == 0) else {
            continue;
        };
        let method_index = name_end + 1;
        if method_index >= chunk.data.len() {
            continue;
        }
        let method = chunk.data[method_index];
        if method != 0 {
            return Err(anyhow!(
                "unsupported PNG iCCP compression method {} in {}",
                method,
                path.display()
            ));
        }
        let compressed = &chunk.data[method_index + 1..];
        let mut decoder = ZlibDecoder::new(compressed);
        let mut icc = Vec::new();
        decoder
            .read_to_end(&mut icc)
            .with_context(|| format!("failed to decompress PNG iCCP in {}", path.display()))?;
        return Ok(Some(icc));
    }
    Ok(None)
}

fn embed_png_icc_profile(path: &Path, icc: &[u8]) -> Result<()> {
    if icc.is_empty() {
        return Err(anyhow!("cannot embed empty ICC profile"));
    }

    let bytes = fs::read(path)
        .with_context(|| format!("failed to read PNG for ICC embedding {}", path.display()))?;
    let chunks = parse_png_chunks(&bytes)?;

    let mut encoder = ZlibEncoder::new(Vec::new(), Compression::default());
    encoder
        .write_all(icc)
        .with_context(|| "failed to compress ICC profile for PNG iCCP")?;
    let compressed = encoder
        .finish()
        .with_context(|| "failed to finalize PNG iCCP compression")?;

    let mut iccp_data = Vec::with_capacity(16 + compressed.len());
    iccp_data.extend_from_slice(b"ImranViewICC");
    iccp_data.push(0);
    iccp_data.push(0);
    iccp_data.extend_from_slice(&compressed);

    let mut rebuilt = Vec::with_capacity(bytes.len().saturating_add(iccp_data.len() + 32));
    rebuilt.extend_from_slice(PNG_SIGNATURE);
    let mut inserted = false;
    for chunk in chunks {
        if &chunk.kind == b"iCCP" {
            continue;
        }
        if !inserted && &chunk.kind == b"IDAT" {
            rebuilt.extend_from_slice(&encode_png_chunk(*b"iCCP", &iccp_data));
            inserted = true;
        }
        rebuilt.extend_from_slice(&encode_png_chunk(chunk.kind, &chunk.data));
    }
    if !inserted {
        return Err(anyhow!(
            "cannot embed ICC profile because PNG is missing IDAT chunk"
        ));
    }

    fs::write(path, rebuilt)
        .with_context(|| format!("failed to write ICC-embedded PNG {}", path.display()))?;
    Ok(())
}

const PNG_SIGNATURE: &[u8; 8] = b"\x89PNG\r\n\x1a\n";

fn parse_png_chunks(bytes: &[u8]) -> Result<Vec<PngChunk>> {
    if bytes.len() < PNG_SIGNATURE.len() || &bytes[..PNG_SIGNATURE.len()] != PNG_SIGNATURE {
        return Err(anyhow!("not a valid PNG stream"));
    }

    let mut chunks = Vec::new();
    let mut index = PNG_SIGNATURE.len();
    while index + 12 <= bytes.len() {
        let length = u32::from_be_bytes([
            bytes[index],
            bytes[index + 1],
            bytes[index + 2],
            bytes[index + 3],
        ]) as usize;
        let kind_start = index + 4;
        let data_start = kind_start + 4;
        let data_end = data_start.saturating_add(length);
        let crc_end = data_end.saturating_add(4);
        if crc_end > bytes.len() {
            return Err(anyhow!("truncated PNG chunk stream"));
        }

        let mut kind = [0u8; 4];
        kind.copy_from_slice(&bytes[kind_start..kind_start + 4]);
        let data = bytes[data_start..data_end].to_vec();
        chunks.push(PngChunk { kind, data });
        index = crc_end;
        if &kind == b"IEND" {
            return Ok(chunks);
        }
    }

    Err(anyhow!("PNG IEND chunk not found"))
}

fn encode_png_chunk(kind: [u8; 4], data: &[u8]) -> Vec<u8> {
    let mut bytes = Vec::with_capacity(data.len() + 12);
    bytes.extend_from_slice(&(data.len() as u32).to_be_bytes());
    bytes.extend_from_slice(&kind);
    bytes.extend_from_slice(data);
    let mut hasher = crc32fast::Hasher::new();
    hasher.update(&kind);
    hasher.update(data);
    bytes.extend_from_slice(&hasher.finalize().to_be_bytes());
    bytes
}

fn extract_jpeg_icc_profile(path: &Path) -> Result<Option<Vec<u8>>> {
    let bytes = fs::read(path)
        .with_context(|| format!("failed to read JPEG for ICC extraction {}", path.display()))?;
    let (segments, _) = split_jpeg_header_and_scan(&bytes)?;

    let mut declared_total: Option<usize> = None;
    let mut parts: Vec<Option<Vec<u8>>> = Vec::new();
    for segment in segments {
        let Some((sequence, total, payload)) = parse_jpeg_icc_app2_segment(&segment.bytes) else {
            continue;
        };
        let total = total as usize;
        let sequence = sequence as usize;
        if sequence == 0 {
            continue;
        }

        match declared_total {
            Some(existing) if existing != total => {
                return Err(anyhow!(
                    "inconsistent ICC segment count in {}",
                    path.display()
                ));
            }
            None => {
                declared_total = Some(total);
                parts = vec![None; total];
            }
            _ => {}
        }

        if sequence > parts.len() {
            return Err(anyhow!(
                "invalid ICC segment sequence in {}",
                path.display()
            ));
        }
        parts[sequence - 1] = Some(payload.to_vec());
    }

    if parts.is_empty() {
        return Ok(None);
    }
    if parts.iter().any(|part| part.is_none()) {
        return Err(anyhow!(
            "incomplete ICC profile segments in {}",
            path.display()
        ));
    }

    let total_size = parts
        .iter()
        .map(|part| part.as_ref().map_or(0usize, Vec::len))
        .sum();
    let mut icc = Vec::with_capacity(total_size);
    for part in parts.into_iter().flatten() {
        icc.extend_from_slice(&part);
    }
    Ok(Some(icc))
}

fn embed_jpeg_icc_profile(path: &Path, icc: &[u8]) -> Result<()> {
    if icc.is_empty() {
        return Err(anyhow!("cannot embed empty ICC profile"));
    }

    let bytes = fs::read(path)
        .with_context(|| format!("failed to read JPEG for ICC embedding {}", path.display()))?;
    let (segments, scan_data) = split_jpeg_header_and_scan(&bytes)?;
    let icc_segments = build_jpeg_icc_app2_segments(icc)?;

    let mut app_prefix_end = 0usize;
    for segment in &segments {
        if (0xe0..=0xef).contains(&segment.marker) || segment.marker == 0xfe {
            app_prefix_end += 1;
        } else {
            break;
        }
    }

    let mut rebuilt = Vec::with_capacity(
        bytes
            .len()
            .saturating_add(icc_segments.iter().map(Vec::len).sum::<usize>())
            .saturating_add(1024),
    );
    rebuilt.extend_from_slice(&[0xff, 0xd8]);

    for segment in segments.iter().take(app_prefix_end) {
        if parse_jpeg_icc_app2_segment(&segment.bytes).is_none() {
            rebuilt.extend_from_slice(&segment.bytes);
        }
    }
    for icc_segment in &icc_segments {
        rebuilt.extend_from_slice(icc_segment);
    }
    for segment in segments.into_iter().skip(app_prefix_end) {
        if parse_jpeg_icc_app2_segment(&segment.bytes).is_none() {
            rebuilt.extend_from_slice(&segment.bytes);
        }
    }
    rebuilt.extend_from_slice(&scan_data);

    fs::write(path, rebuilt)
        .with_context(|| format!("failed to write ICC-embedded JPEG {}", path.display()))?;
    Ok(())
}

fn build_jpeg_icc_app2_segments(icc: &[u8]) -> Result<Vec<Vec<u8>>> {
    const ICC_HEADER: &[u8] = b"ICC_PROFILE\0";
    const MAX_CHUNK_SIZE: usize = 65_519;

    let total_segments = icc.len().div_ceil(MAX_CHUNK_SIZE);
    if total_segments == 0 || total_segments > u8::MAX as usize {
        return Err(anyhow!(
            "ICC profile requires unsupported segment count: {}",
            total_segments
        ));
    }

    let mut segments = Vec::with_capacity(total_segments);
    for (index, chunk) in icc.chunks(MAX_CHUNK_SIZE).enumerate() {
        let sequence = (index + 1) as u8;
        let total = total_segments as u8;
        let length = 2usize
            .saturating_add(ICC_HEADER.len())
            .saturating_add(2)
            .saturating_add(chunk.len());
        if length > u16::MAX as usize {
            return Err(anyhow!("ICC segment too large"));
        }

        let mut bytes = Vec::with_capacity(2 + length);
        bytes.extend_from_slice(&[0xff, 0xe2]);
        bytes.extend_from_slice(&(length as u16).to_be_bytes());
        bytes.extend_from_slice(ICC_HEADER);
        bytes.push(sequence);
        bytes.push(total);
        bytes.extend_from_slice(chunk);
        segments.push(bytes);
    }

    Ok(segments)
}

fn parse_jpeg_icc_app2_segment(bytes: &[u8]) -> Option<(u8, u8, &[u8])> {
    const ICC_HEADER: &[u8] = b"ICC_PROFILE\0";

    if bytes.len() < 4 + ICC_HEADER.len() + 2 {
        return None;
    }
    if bytes[0] != 0xff || bytes[1] != 0xe2 {
        return None;
    }
    let declared_length = u16::from_be_bytes([bytes[2], bytes[3]]) as usize;
    if declared_length + 2 != bytes.len() {
        return None;
    }
    let payload = &bytes[4..];
    if !payload.starts_with(ICC_HEADER) {
        return None;
    }
    let sequence = payload[ICC_HEADER.len()];
    let total = payload[ICC_HEADER.len() + 1];
    if sequence == 0 || total == 0 {
        return None;
    }
    let profile_payload = &payload[ICC_HEADER.len() + 2..];
    Some((sequence, total, profile_payload))
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

#[cfg(test)]
mod tests {
    use super::{
        SaveFormat, build_jpeg_icc_app2_segments, embed_jpeg_icc_profile, embed_png_icc_profile,
        extract_jpeg_icc_profile, extract_png_icc_profile, parse_jpeg_icc_app2_segment,
        save_image_with_format,
    };
    use image::{DynamicImage, Rgba, RgbaImage};
    use tempfile::tempdir;

    #[test]
    fn jpeg_icc_segments_roundtrip_payload() {
        let payload: Vec<u8> = (0..140_000).map(|index| (index % 251) as u8).collect();
        let segments = build_jpeg_icc_app2_segments(&payload).expect("segments should build");
        assert!(segments.len() >= 3);

        let mut assembled = Vec::new();
        for segment in segments {
            let parsed = parse_jpeg_icc_app2_segment(&segment).expect("must parse ICC segment");
            assembled.extend_from_slice(parsed.2);
        }
        assert_eq!(assembled, payload);
    }

    #[test]
    fn jpeg_embed_and_extract_icc_profile_roundtrip() {
        let temp = tempdir().expect("tempdir should create");
        let path = temp.path().join("icc-roundtrip.jpg");
        let source = DynamicImage::ImageRgba8(RgbaImage::from_pixel(24, 18, Rgba([3, 4, 5, 255])));
        save_image_with_format(&path, &source, SaveFormat::Jpeg { quality: 90 })
            .expect("jpeg should save");

        let icc_payload: Vec<u8> = (0..88_000).map(|index| (index % 233) as u8).collect();
        embed_jpeg_icc_profile(&path, &icc_payload).expect("embedding should succeed");
        let extracted = extract_jpeg_icc_profile(&path)
            .expect("extraction should succeed")
            .expect("profile should exist");
        assert_eq!(extracted, icc_payload);
    }

    #[test]
    fn png_embed_and_extract_icc_profile_roundtrip() {
        let temp = tempdir().expect("tempdir should create");
        let path = temp.path().join("icc-roundtrip.png");
        let source =
            DynamicImage::ImageRgba8(RgbaImage::from_pixel(28, 20, Rgba([21, 22, 23, 255])));
        save_image_with_format(&path, &source, SaveFormat::Png).expect("png should save");

        let icc_payload: Vec<u8> = (0..24_000).map(|index| (index % 239) as u8).collect();
        embed_png_icc_profile(&path, &icc_payload).expect("PNG ICC embedding should succeed");
        let extracted = extract_png_icc_profile(&path)
            .expect("PNG ICC extraction should succeed")
            .expect("PNG profile should exist");
        assert_eq!(extracted, icc_payload);
    }
}
