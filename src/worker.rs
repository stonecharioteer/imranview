use std::collections::{HashMap, VecDeque};
use std::fs;
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::sync::Mutex;
use std::sync::mpsc::{Receiver, Sender};
use std::thread;
use std::time::Instant;

use anyhow::{Context, Result, anyhow};
use font8x8::UnicodeFonts;
use image::imageops::FilterType;
use image::{DynamicImage, GenericImageView, Rgba, RgbaImage};
use serde::{Deserialize, Serialize};

use crate::image_io::{
    LoadedImagePayload, MetadataSummary, SaveFormat, ThumbnailPayload, collect_images_in_directory,
    extract_metadata_summary, infer_save_format, load_image_payload, load_thumbnail_payload,
    load_working_image, payload_from_working_image, preserve_metadata_best_effort,
    save_image_with_format,
};
use crate::perf::{EDIT_IMAGE_BUDGET, OPEN_IMAGE_BUDGET, SAVE_IMAGE_BUDGET, log_timing};

const PRELOAD_CACHE_CAP: usize = 6;
const PRELOAD_CACHE_MAX_BYTES: usize = 192 * 1024 * 1024;

#[derive(Clone, Copy, Debug)]
pub struct WorkerConfig {
    pub preload_cache_cap: usize,
    pub preload_cache_max_bytes: usize,
    pub thumbnail_workers: usize,
}

impl Default for WorkerConfig {
    fn default() -> Self {
        Self {
            preload_cache_cap: PRELOAD_CACHE_CAP,
            preload_cache_max_bytes: PRELOAD_CACHE_MAX_BYTES,
            thumbnail_workers: 0,
        }
    }
}

#[derive(Clone, Copy, Debug)]
pub enum WorkerRequestKind {
    Open,
    Save,
    Edit,
    Preload,
    Thumbnail,
    Batch,
    File,
    Print,
    Compare,
    Utility,
    Ocr,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ResizeFilter {
    Nearest,
    Triangle,
    CatmullRom,
    Gaussian,
    Lanczos3,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum CanvasAnchor {
    TopLeft,
    TopCenter,
    TopRight,
    CenterLeft,
    Center,
    CenterRight,
    BottomLeft,
    BottomCenter,
    BottomRight,
}

impl CanvasAnchor {
    fn factors(self) -> (f32, f32) {
        match self {
            CanvasAnchor::TopLeft => (0.0, 0.0),
            CanvasAnchor::TopCenter => (0.5, 0.0),
            CanvasAnchor::TopRight => (1.0, 0.0),
            CanvasAnchor::CenterLeft => (0.0, 0.5),
            CanvasAnchor::Center => (0.5, 0.5),
            CanvasAnchor::CenterRight => (1.0, 0.5),
            CanvasAnchor::BottomLeft => (0.0, 1.0),
            CanvasAnchor::BottomCenter => (0.5, 1.0),
            CanvasAnchor::BottomRight => (1.0, 1.0),
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum RotationInterpolation {
    Nearest,
    Bilinear,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ShapeKind {
    Line,
    Rectangle,
    Ellipse,
    Arrow,
    RoundedRectangleShadow,
    SpeechBubble,
}

#[derive(Clone, Copy, Debug)]
pub struct ShapeParams {
    pub kind: ShapeKind,
    pub start_x: i32,
    pub start_y: i32,
    pub end_x: i32,
    pub end_y: i32,
    pub thickness: u32,
    pub filled: bool,
    pub color: [u8; 4],
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum SelectionWorkflow {
    CropRect,
    CropCircle,
    CutOutsideRect,
    CutOutsideCircle,
    CropPolygon,
    CutOutsidePolygon,
}

#[derive(Clone, Debug)]
pub struct SelectionParams {
    pub workflow: SelectionWorkflow,
    pub x: u32,
    pub y: u32,
    pub width: u32,
    pub height: u32,
    pub radius: u32,
    pub polygon_points: Vec<[u32; 2]>,
    pub fill: [u8; 4],
}

#[derive(Clone, Copy, Debug, Serialize, Deserialize)]
pub struct EffectsParams {
    pub blur_sigma: f32,
    pub sharpen_sigma: f32,
    pub sharpen_threshold: i32,
    pub invert: bool,
    pub grayscale: bool,
    pub sepia_strength: f32,
    pub posterize_levels: u8,
    pub vignette_strength: f32,
    pub tilt_shift_strength: f32,
    pub stained_glass_strength: f32,
    pub emboss_strength: f32,
    pub edge_enhance_strength: f32,
    pub oil_paint_strength: f32,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum AlphaBrushOp {
    Increase,
    Decrease,
    SetOpaque,
    SetTransparent,
}

impl ResizeFilter {
    fn to_image_filter(self) -> FilterType {
        match self {
            ResizeFilter::Nearest => FilterType::Nearest,
            ResizeFilter::Triangle => FilterType::Triangle,
            ResizeFilter::CatmullRom => FilterType::CatmullRom,
            ResizeFilter::Gaussian => FilterType::Gaussian,
            ResizeFilter::Lanczos3 => FilterType::Lanczos3,
        }
    }
}

#[derive(Clone, Copy, Debug)]
pub struct ColorAdjustParams {
    pub brightness: i32,
    pub contrast: f32,
    pub gamma: f32,
    pub saturation: f32,
    pub grayscale: bool,
}

#[derive(Clone, Debug)]
pub enum TransformOp {
    RotateLeft,
    RotateRight,
    FlipHorizontal,
    FlipVertical,
    AddBorder {
        left: u32,
        right: u32,
        top: u32,
        bottom: u32,
        color: [u8; 4],
    },
    CanvasSize {
        width: u32,
        height: u32,
        anchor: CanvasAnchor,
        fill: [u8; 4],
    },
    RotateFine {
        angle_degrees: f32,
        interpolation: RotationInterpolation,
        expand_canvas: bool,
        fill: [u8; 4],
    },
    AddText {
        text: String,
        x: i32,
        y: i32,
        scale: u32,
        color: [u8; 4],
    },
    DrawShape(ShapeParams),
    OverlayImage {
        overlay_path: PathBuf,
        opacity: f32,
        anchor: CanvasAnchor,
    },
    SelectionWorkflow(SelectionParams),
    ReplaceColor {
        source: [u8; 4],
        target: [u8; 4],
        tolerance: u8,
        preserve_alpha: bool,
    },
    AlphaAdjust {
        alpha_percent: f32,
        alpha_from_luma: bool,
        invert_luma: bool,
        region: Option<(u32, u32, u32, u32)>,
    },
    AlphaBrush {
        center_x: u32,
        center_y: u32,
        radius: u32,
        strength_percent: f32,
        softness: f32,
        operation: AlphaBrushOp,
    },
    Effects(EffectsParams),
    PerspectiveCorrect {
        top_left: [f32; 2],
        top_right: [f32; 2],
        bottom_right: [f32; 2],
        bottom_left: [f32; 2],
        output_width: u32,
        output_height: u32,
        interpolation: RotationInterpolation,
        fill: [u8; 4],
    },
    Resize {
        width: u32,
        height: u32,
        filter: ResizeFilter,
    },
    Crop {
        x: u32,
        y: u32,
        width: u32,
        height: u32,
    },
    ColorAdjust(ColorAdjustParams),
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub enum BatchOutputFormat {
    Png,
    Jpeg,
    Webp,
    Bmp,
    Tiff,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum PanoramaDirection {
    Horizontal,
    Vertical,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum LosslessJpegOp {
    Rotate90,
    Rotate180,
    Rotate270,
    FlipHorizontal,
    FlipVertical,
}

impl BatchOutputFormat {
    fn to_save_format(self, jpeg_quality: u8) -> SaveFormat {
        match self {
            BatchOutputFormat::Png => SaveFormat::Png,
            BatchOutputFormat::Jpeg => SaveFormat::Jpeg {
                quality: jpeg_quality,
            },
            BatchOutputFormat::Webp => SaveFormat::Webp,
            BatchOutputFormat::Bmp => SaveFormat::Bmp,
            BatchOutputFormat::Tiff => SaveFormat::Tiff,
        }
    }
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct BatchConvertOptions {
    pub input_dir: PathBuf,
    pub output_dir: PathBuf,
    pub output_format: BatchOutputFormat,
    pub rename_prefix: String,
    pub start_index: u32,
    pub jpeg_quality: u8,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum SaveOutputFormat {
    Auto,
    Png,
    Jpeg,
    Webp,
    Bmp,
    Tiff,
}

impl SaveOutputFormat {
    fn to_save_format(self, path: &Path, jpeg_quality: u8) -> Result<SaveFormat> {
        match self {
            SaveOutputFormat::Auto => {
                infer_save_format(path, jpeg_quality).map_err(|err| anyhow!(err.to_string()))
            }
            SaveOutputFormat::Png => Ok(SaveFormat::Png),
            SaveOutputFormat::Jpeg => Ok(SaveFormat::Jpeg {
                quality: jpeg_quality.clamp(1, 100),
            }),
            SaveOutputFormat::Webp => Ok(SaveFormat::Webp),
            SaveOutputFormat::Bmp => Ok(SaveFormat::Bmp),
            SaveOutputFormat::Tiff => Ok(SaveFormat::Tiff),
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum SaveMetadataPolicy {
    Strip,
    PreserveIfPossible,
}

#[derive(Clone, Copy, Debug)]
pub struct SaveImageOptions {
    pub output_format: SaveOutputFormat,
    pub jpeg_quality: u8,
    pub metadata_policy: SaveMetadataPolicy,
}

impl Default for SaveImageOptions {
    fn default() -> Self {
        Self {
            output_format: SaveOutputFormat::Auto,
            jpeg_quality: 92,
            metadata_policy: SaveMetadataPolicy::PreserveIfPossible,
        }
    }
}

#[derive(Clone, Debug)]
pub enum FileOperation {
    Rename { from: PathBuf, to: PathBuf },
    Delete { path: PathBuf },
    Copy { from: PathBuf, to: PathBuf },
    Move { from: PathBuf, to: PathBuf },
}

pub enum WorkerCommand {
    OpenImage {
        request_id: u64,
        path: PathBuf,
    },
    OpenDirectory {
        request_id: u64,
        directory: PathBuf,
    },
    SaveImage {
        request_id: u64,
        path: PathBuf,
        source_path: Option<PathBuf>,
        image: Arc<DynamicImage>,
        reopen_after_save: bool,
        options: SaveImageOptions,
    },
    TransformImage {
        request_id: u64,
        op: TransformOp,
        image: Arc<DynamicImage>,
    },
    PreloadImage {
        path: PathBuf,
    },
    BatchConvert {
        request_id: u64,
        options: BatchConvertOptions,
    },
    RunBatchScript {
        request_id: u64,
        script_path: PathBuf,
    },
    FileOperation {
        request_id: u64,
        operation: FileOperation,
    },
    LoadCompareImage {
        request_id: u64,
        path: PathBuf,
    },
    PrintImage {
        request_id: u64,
        path: PathBuf,
    },
    CaptureScreenshot {
        request_id: u64,
        delay_ms: u64,
        region: Option<(u32, u32, u32, u32)>,
        output_path: Option<PathBuf>,
    },
    RunLosslessJpeg {
        request_id: u64,
        path: PathBuf,
        op: LosslessJpegOp,
        output_path: Option<PathBuf>,
    },
    UpdateExifDate {
        request_id: u64,
        path: PathBuf,
        datetime: String,
    },
    ConvertColorProfile {
        request_id: u64,
        path: PathBuf,
        output_path: PathBuf,
        source_profile: Option<PathBuf>,
        target_profile: PathBuf,
        rendering_intent: String,
    },
    ScanToDirectory {
        request_id: u64,
        output_dir: PathBuf,
        output_format: BatchOutputFormat,
        rename_prefix: String,
        start_index: u32,
        page_count: u32,
        jpeg_quality: u8,
        command_template: String,
    },
    ScanNative {
        request_id: u64,
        output_dir: PathBuf,
        output_format: BatchOutputFormat,
        rename_prefix: String,
        start_index: u32,
        page_count: u32,
        jpeg_quality: u8,
        dpi: u32,
        grayscale: bool,
    },
    OpenTiffPage {
        request_id: u64,
        path: PathBuf,
        page_index: u32,
    },
    ExtractTiffPages {
        request_id: u64,
        path: PathBuf,
        output_dir: PathBuf,
        output_format: BatchOutputFormat,
        jpeg_quality: u8,
    },
    CreateMultipagePdf {
        request_id: u64,
        input_paths: Vec<PathBuf>,
        output_path: PathBuf,
        jpeg_quality: u8,
    },
    RunOcr {
        request_id: u64,
        path: PathBuf,
        language: String,
        output_path: Option<PathBuf>,
    },
    StitchPanorama {
        request_id: u64,
        input_paths: Vec<PathBuf>,
        output_path: PathBuf,
        direction: PanoramaDirection,
        overlap_percent: f32,
    },
    ExportContactSheet {
        request_id: u64,
        input_paths: Vec<PathBuf>,
        output_path: PathBuf,
        columns: u32,
        thumb_size: u32,
        include_labels: bool,
        background: [u8; 4],
        label_color: [u8; 4],
        jpeg_quality: u8,
    },
    ExportHtmlGallery {
        request_id: u64,
        input_paths: Vec<PathBuf>,
        output_dir: PathBuf,
        title: String,
        thumb_width: u32,
    },
    UpdateCachePolicy {
        preload_cache_cap: usize,
        preload_cache_max_bytes: usize,
    },
}

pub enum WorkerResult {
    Opened {
        request_id: u64,
        path: PathBuf,
        directory: PathBuf,
        files: Vec<PathBuf>,
        loaded: LoadedImagePayload,
        metadata: MetadataSummary,
    },
    Saved {
        request_id: u64,
        path: PathBuf,
        reopen_after_save: bool,
    },
    Transformed {
        request_id: u64,
        loaded: LoadedImagePayload,
    },
    ThumbnailDecoded {
        path: PathBuf,
        payload: ThumbnailPayload,
    },
    Preloaded {
        path: PathBuf,
    },
    BatchCompleted {
        request_id: u64,
        processed: usize,
        failed: usize,
        output_dir: PathBuf,
    },
    FileOperationCompleted {
        request_id: u64,
        operation: FileOperation,
    },
    CompareLoaded {
        request_id: u64,
        path: PathBuf,
        loaded: LoadedImagePayload,
        metadata: MetadataSummary,
    },
    Printed {
        request_id: u64,
        path: PathBuf,
    },
    TiffPageLoaded {
        request_id: u64,
        page_index: u32,
        page_count: u32,
        loaded: LoadedImagePayload,
    },
    UtilityCompleted {
        request_id: u64,
        message: String,
        open_path: Option<PathBuf>,
    },
    OcrCompleted {
        request_id: u64,
        output_path: Option<PathBuf>,
        text: String,
    },
    Failed {
        request_id: Option<u64>,
        kind: WorkerRequestKind,
        error: String,
    },
}

struct PreloadCache {
    map: HashMap<PathBuf, LoadedImagePayload>,
    byte_sizes: HashMap<PathBuf, usize>,
    order: VecDeque<PathBuf>,
    capacity: usize,
    max_bytes: usize,
    total_bytes: usize,
}

impl PreloadCache {
    fn new(capacity: usize, max_bytes: usize) -> Self {
        Self {
            map: HashMap::new(),
            byte_sizes: HashMap::new(),
            order: VecDeque::new(),
            capacity,
            max_bytes,
            total_bytes: 0,
        }
    }

    fn take(&mut self, path: &Path) -> Option<LoadedImagePayload> {
        let key = path.to_path_buf();
        let value = self.map.remove(&key)?;
        if let Some(bytes) = self.byte_sizes.remove(&key) {
            self.total_bytes = self.total_bytes.saturating_sub(bytes);
        }
        if let Some(index) = self.order.iter().position(|candidate| candidate == path) {
            let _ = self.order.remove(index);
        }
        Some(value)
    }

    fn insert(&mut self, path: PathBuf, payload: LoadedImagePayload) {
        let payload_bytes = estimate_payload_bytes(&payload);
        if self.map.contains_key(&path) {
            if let Some(previous_bytes) = self.byte_sizes.insert(path.clone(), payload_bytes) {
                self.total_bytes = self.total_bytes.saturating_sub(previous_bytes);
            }
            self.total_bytes = self.total_bytes.saturating_add(payload_bytes);
            self.map.insert(path.clone(), payload);
            self.touch(&path);
            self.evict();
            return;
        }
        self.map.insert(path.clone(), payload);
        self.byte_sizes.insert(path.clone(), payload_bytes);
        self.total_bytes = self.total_bytes.saturating_add(payload_bytes);
        self.order.push_back(path);
        self.evict();
    }

    fn touch(&mut self, path: &Path) {
        if let Some(index) = self.order.iter().position(|candidate| candidate == path) {
            if let Some(existing) = self.order.remove(index) {
                self.order.push_back(existing);
            }
        }
    }

    fn evict(&mut self) {
        while self.map.len() > self.capacity || self.total_bytes > self.max_bytes {
            if let Some(oldest) = self.order.pop_front() {
                self.map.remove(&oldest);
                if let Some(bytes) = self.byte_sizes.remove(&oldest) {
                    self.total_bytes = self.total_bytes.saturating_sub(bytes);
                }
            } else {
                break;
            }
        }
    }
}

pub fn spawn_workers(
    command_rx: Receiver<WorkerCommand>,
    thumbnail_rx: Receiver<PathBuf>,
    result_tx: Sender<WorkerResult>,
    config: WorkerConfig,
) {
    log::debug!(target: "imranview::worker", "spawning primary worker thread");
    let _ = thread::Builder::new()
        .name("imranview-worker".to_owned())
        .spawn({
            let result_tx = result_tx.clone();
            move || run_worker(command_rx, result_tx, config)
        });

    spawn_thumbnail_workers(thumbnail_rx, result_tx, config.thumbnail_workers);
}

fn spawn_thumbnail_workers(
    thumbnail_rx: Receiver<PathBuf>,
    result_tx: Sender<WorkerResult>,
    configured_workers: usize,
) {
    let workers = thumbnail_worker_count(configured_workers);
    let shared_rx = Arc::new(Mutex::new(thumbnail_rx));
    log::debug!(
        target: "imranview::thumb",
        "spawning thumbnail workers: {}",
        workers
    );

    for worker_index in 0..workers {
        let rx = Arc::clone(&shared_rx);
        let tx = result_tx.clone();
        let _ = thread::Builder::new()
            .name(format!("imranview-thumb-{worker_index}"))
            .spawn(move || run_thumbnail_worker(worker_index, rx, tx));
    }
}

fn thumbnail_worker_count(configured_workers: usize) -> usize {
    if configured_workers > 0 {
        return configured_workers.clamp(1, 4);
    }
    let logical_cores = thread::available_parallelism()
        .map(|n| n.get())
        .unwrap_or(2);
    logical_cores.saturating_sub(1).clamp(1, 2)
}

fn run_worker(
    command_rx: Receiver<WorkerCommand>,
    result_tx: Sender<WorkerResult>,
    config: WorkerConfig,
) {
    log::debug!(target: "imranview::worker", "worker thread started");
    let mut preload_cache =
        PreloadCache::new(config.preload_cache_cap, config.preload_cache_max_bytes);
    while let Ok(command) = command_rx.recv() {
        let result = match command {
            WorkerCommand::OpenImage { request_id, path } => {
                Some(run_open(request_id, path, &mut preload_cache))
            }
            WorkerCommand::OpenDirectory {
                request_id,
                directory,
            } => Some(run_open_directory(
                request_id,
                directory,
                &mut preload_cache,
            )),
            WorkerCommand::SaveImage {
                request_id,
                path,
                source_path,
                image,
                reopen_after_save,
                options,
            } => Some(run_save(
                request_id,
                path,
                source_path,
                image,
                reopen_after_save,
                options,
            )),
            WorkerCommand::TransformImage {
                request_id,
                op,
                image,
            } => Some(run_transform(request_id, op, image)),
            WorkerCommand::PreloadImage { path } => run_preload(path, &mut preload_cache),
            WorkerCommand::BatchConvert {
                request_id,
                options,
            } => Some(run_batch_convert(request_id, options)),
            WorkerCommand::RunBatchScript {
                request_id,
                script_path,
            } => Some(run_batch_script(request_id, script_path)),
            WorkerCommand::FileOperation {
                request_id,
                operation,
            } => Some(run_file_operation(request_id, operation)),
            WorkerCommand::LoadCompareImage { request_id, path } => {
                Some(run_load_compare(request_id, path))
            }
            WorkerCommand::PrintImage { request_id, path } => Some(run_print(request_id, path)),
            WorkerCommand::CaptureScreenshot {
                request_id,
                delay_ms,
                region,
                output_path,
            } => Some(run_capture_screenshot(
                request_id,
                delay_ms,
                region,
                output_path,
            )),
            WorkerCommand::RunLosslessJpeg {
                request_id,
                path,
                op,
                output_path,
            } => Some(run_lossless_jpeg(request_id, path, op, output_path)),
            WorkerCommand::UpdateExifDate {
                request_id,
                path,
                datetime,
            } => Some(run_update_exif_date(request_id, path, datetime)),
            WorkerCommand::ConvertColorProfile {
                request_id,
                path,
                output_path,
                source_profile,
                target_profile,
                rendering_intent,
            } => Some(run_convert_color_profile(
                request_id,
                path,
                output_path,
                source_profile,
                target_profile,
                rendering_intent,
            )),
            WorkerCommand::ScanToDirectory {
                request_id,
                output_dir,
                output_format,
                rename_prefix,
                start_index,
                page_count,
                jpeg_quality,
                command_template,
            } => Some(run_scan_to_directory(
                request_id,
                output_dir,
                output_format,
                rename_prefix,
                start_index,
                page_count,
                jpeg_quality,
                command_template,
            )),
            WorkerCommand::ScanNative {
                request_id,
                output_dir,
                output_format,
                rename_prefix,
                start_index,
                page_count,
                jpeg_quality,
                dpi,
                grayscale,
            } => Some(run_scan_native(
                request_id,
                output_dir,
                output_format,
                rename_prefix,
                start_index,
                page_count,
                jpeg_quality,
                dpi,
                grayscale,
            )),
            WorkerCommand::OpenTiffPage {
                request_id,
                path,
                page_index,
            } => Some(run_open_tiff_page(request_id, path, page_index)),
            WorkerCommand::ExtractTiffPages {
                request_id,
                path,
                output_dir,
                output_format,
                jpeg_quality,
            } => Some(run_extract_tiff_pages(
                request_id,
                path,
                output_dir,
                output_format,
                jpeg_quality,
            )),
            WorkerCommand::CreateMultipagePdf {
                request_id,
                input_paths,
                output_path,
                jpeg_quality,
            } => Some(run_create_multipage_pdf(
                request_id,
                input_paths,
                output_path,
                jpeg_quality,
            )),
            WorkerCommand::RunOcr {
                request_id,
                path,
                language,
                output_path,
            } => Some(run_ocr(request_id, path, language, output_path)),
            WorkerCommand::StitchPanorama {
                request_id,
                input_paths,
                output_path,
                direction,
                overlap_percent,
            } => Some(run_stitch_panorama(
                request_id,
                input_paths,
                output_path,
                direction,
                overlap_percent,
            )),
            WorkerCommand::ExportContactSheet {
                request_id,
                input_paths,
                output_path,
                columns,
                thumb_size,
                include_labels,
                background,
                label_color,
                jpeg_quality,
            } => Some(run_export_contact_sheet(
                request_id,
                input_paths,
                output_path,
                columns,
                thumb_size,
                include_labels,
                background,
                label_color,
                jpeg_quality,
            )),
            WorkerCommand::ExportHtmlGallery {
                request_id,
                input_paths,
                output_dir,
                title,
                thumb_width,
            } => Some(run_export_html_gallery(
                request_id,
                input_paths,
                output_dir,
                title,
                thumb_width,
            )),
            WorkerCommand::UpdateCachePolicy {
                preload_cache_cap,
                preload_cache_max_bytes,
            } => {
                preload_cache =
                    PreloadCache::new(preload_cache_cap.max(1), preload_cache_max_bytes.max(1));
                log::debug!(
                    target: "imranview::worker",
                    "updated preload cache policy cap={} max_bytes={}",
                    preload_cache_cap,
                    preload_cache_max_bytes
                );
                None
            }
        };

        if let Some(result) = result {
            if result_tx.send(result).is_err() {
                log::warn!(
                    target: "imranview::worker",
                    "result channel disconnected, worker exiting"
                );
                break;
            }
        }
    }
    log::debug!(target: "imranview::worker", "worker thread stopped");
}

fn run_thumbnail_worker(
    worker_index: usize,
    thumbnail_rx: Arc<Mutex<Receiver<PathBuf>>>,
    result_tx: Sender<WorkerResult>,
) {
    log::debug!(
        target: "imranview::thumb",
        "thumbnail worker {} started",
        worker_index
    );
    loop {
        let path = {
            let guard = thumbnail_rx
                .lock()
                .expect("thumbnail receiver lock poisoned");
            match guard.recv() {
                Ok(path) => path,
                Err(_) => break,
            }
        };

        let result = run_thumbnail(path);
        if result_tx.send(result).is_err() {
            log::warn!(
                target: "imranview::thumb",
                "result channel disconnected, thumbnail worker {} exiting",
                worker_index
            );
            break;
        }
    }
    log::debug!(
        target: "imranview::thumb",
        "thumbnail worker {} stopped",
        worker_index
    );
}

fn run_open(request_id: u64, path: PathBuf, preload_cache: &mut PreloadCache) -> WorkerResult {
    log::debug!(
        target: "imranview::worker",
        "open start request_id={} path={}",
        request_id,
        path.display()
    );
    let started = Instant::now();
    let output = (|| -> Result<WorkerResult> {
        let loaded = if let Some(cached) = preload_cache.take(&path) {
            log::debug!(
                target: "imranview::worker",
                "open cache hit request_id={} path={}",
                request_id,
                path.display()
            );
            cached
        } else {
            load_image_payload(&path)?
        };
        let directory = path
            .parent()
            .unwrap_or_else(|| Path::new("."))
            .to_path_buf();
        let files = collect_images_in_directory(&directory)?;
        let metadata = extract_metadata_summary(&path);
        Ok(WorkerResult::Opened {
            request_id,
            path,
            directory,
            files,
            loaded,
            metadata,
        })
    })();

    log_timing("open_image", started.elapsed(), OPEN_IMAGE_BUDGET);
    output.unwrap_or_else(|err| WorkerResult::Failed {
        request_id: Some(request_id),
        kind: WorkerRequestKind::Open,
        error: err.to_string(),
    })
}

fn run_open_directory(
    request_id: u64,
    directory: PathBuf,
    preload_cache: &mut PreloadCache,
) -> WorkerResult {
    log::debug!(
        target: "imranview::worker",
        "open directory start request_id={} directory={}",
        request_id,
        directory.display()
    );
    let started = Instant::now();
    let output = (|| -> Result<WorkerResult> {
        let files = collect_images_in_directory(&directory)?;
        let path = files
            .first()
            .cloned()
            .context("selected folder has no supported images")?;
        let loaded = if let Some(cached) = preload_cache.take(&path) {
            log::debug!(
                target: "imranview::worker",
                "open directory cache hit request_id={} path={}",
                request_id,
                path.display()
            );
            cached
        } else {
            load_image_payload(&path)?
        };
        let metadata = extract_metadata_summary(&path);
        Ok(WorkerResult::Opened {
            request_id,
            path,
            directory,
            files,
            loaded,
            metadata,
        })
    })();

    log_timing("open_image", started.elapsed(), OPEN_IMAGE_BUDGET);
    output.unwrap_or_else(|err| WorkerResult::Failed {
        request_id: Some(request_id),
        kind: WorkerRequestKind::Open,
        error: err.to_string(),
    })
}

fn run_preload(path: PathBuf, preload_cache: &mut PreloadCache) -> Option<WorkerResult> {
    log::debug!(
        target: "imranview::worker",
        "preload start path={}",
        path.display()
    );
    match load_image_payload(&path) {
        Ok(payload) => {
            preload_cache.insert(path.clone(), payload);
            Some(WorkerResult::Preloaded { path })
        }
        Err(err) => {
            log::debug!(
                target: "imranview::worker",
                "preload skipped path={} error={}",
                path.display(),
                err
            );
            Some(WorkerResult::Failed {
                request_id: None,
                kind: WorkerRequestKind::Preload,
                error: err.to_string(),
            })
        }
    }
}

fn run_save(
    request_id: u64,
    path: PathBuf,
    source_path: Option<PathBuf>,
    image: Arc<DynamicImage>,
    reopen_after_save: bool,
    options: SaveImageOptions,
) -> WorkerResult {
    log::debug!(
        target: "imranview::worker",
        "save start request_id={} path={} reopen_after_save={}",
        request_id,
        path.display(),
        reopen_after_save
    );
    let started = Instant::now();
    let output = (|| -> Result<WorkerResult> {
        let save_format = options
            .output_format
            .to_save_format(&path, options.jpeg_quality)?;
        let preserve_metadata = matches!(
            options.metadata_policy,
            SaveMetadataPolicy::PreserveIfPossible
        );
        if preserve_metadata {
            let source = source_path
                .as_ref()
                .context("metadata preservation requires a source image path")?;
            if source == &path {
                let temp_output = temporary_save_path(&path);
                save_image_with_format(&temp_output, image.as_ref(), save_format)
                    .with_context(|| format!("failed to save {}", temp_output.display()))?;
                preserve_metadata_best_effort(source, &temp_output, save_format).with_context(
                    || {
                        format!(
                            "metadata preservation failed for {} (choose metadata policy: Strip to bypass)",
                            path.display()
                        )
                    },
                )?;
                fs::rename(&temp_output, &path).with_context(|| {
                    format!(
                        "failed to replace original file after metadata-preserving save {}",
                        path.display()
                    )
                })?;
            } else {
                save_image_with_format(&path, image.as_ref(), save_format)
                    .with_context(|| format!("failed to save {}", path.display()))?;
                preserve_metadata_best_effort(source, &path, save_format).with_context(|| {
                    format!(
                        "metadata preservation failed for {} (choose metadata policy: Strip to bypass)",
                        path.display()
                    )
                })?;
            }
        } else {
            save_image_with_format(&path, image.as_ref(), save_format)
                .with_context(|| format!("failed to save {}", path.display()))?;
        }

        Ok(WorkerResult::Saved {
            request_id,
            path,
            reopen_after_save,
        })
    })();

    log_timing("save_image", started.elapsed(), SAVE_IMAGE_BUDGET);
    output.unwrap_or_else(|err| WorkerResult::Failed {
        request_id: Some(request_id),
        kind: WorkerRequestKind::Save,
        error: err.to_string(),
    })
}

fn temporary_save_path(path: &Path) -> PathBuf {
    let file_name = path
        .file_name()
        .map(|name| name.to_string_lossy().into_owned())
        .unwrap_or_else(|| "image".to_owned());
    let temp_name = format!("{file_name}.imranview.tmp");
    path.with_file_name(temp_name)
}

fn run_load_compare(request_id: u64, path: PathBuf) -> WorkerResult {
    let output = (|| -> Result<WorkerResult> {
        let loaded = load_image_payload(&path)?;
        let metadata = extract_metadata_summary(&path);
        Ok(WorkerResult::CompareLoaded {
            request_id,
            path,
            loaded,
            metadata,
        })
    })();

    output.unwrap_or_else(|err| WorkerResult::Failed {
        request_id: Some(request_id),
        kind: WorkerRequestKind::Compare,
        error: err.to_string(),
    })
}

fn run_print(request_id: u64, path: PathBuf) -> WorkerResult {
    let output = (|| -> Result<WorkerResult> {
        if !path.exists() {
            return Err(anyhow!("cannot print missing file {}", path.display()));
        }

        #[cfg(target_os = "macos")]
        {
            let status = std::process::Command::new("lp")
                .arg(&path)
                .status()
                .context("failed to execute lp print command")?;
            if !status.success() {
                return Err(anyhow!("lp print command failed with status {}", status));
            }
        }

        #[cfg(target_os = "linux")]
        {
            let status = std::process::Command::new("lp")
                .arg(&path)
                .status()
                .context("failed to execute lp print command")?;
            if !status.success() {
                return Err(anyhow!("lp print command failed with status {}", status));
            }
        }

        #[cfg(target_os = "windows")]
        {
            let status = std::process::Command::new("rundll32.exe")
                .args([
                    "C:\\Windows\\System32\\shimgvw.dll,ImageView_PrintTo",
                    "/pt",
                ])
                .arg(&path)
                .status()
                .context("failed to execute Windows print command")?;
            if !status.success() {
                return Err(anyhow!(
                    "Windows print command failed with status {}",
                    status
                ));
            }
        }

        Ok(WorkerResult::Printed { request_id, path })
    })();

    output.unwrap_or_else(|err| WorkerResult::Failed {
        request_id: Some(request_id),
        kind: WorkerRequestKind::Print,
        error: err.to_string(),
    })
}

fn run_capture_screenshot(
    request_id: u64,
    delay_ms: u64,
    region: Option<(u32, u32, u32, u32)>,
    output_path: Option<PathBuf>,
) -> WorkerResult {
    let output = (|| -> Result<WorkerResult> {
        let path = output_path.unwrap_or_else(default_screenshot_path);
        capture_screenshot_to_path(&path, delay_ms, region)?;
        Ok(WorkerResult::UtilityCompleted {
            request_id,
            message: format!("Captured screenshot {}", path.display()),
            open_path: Some(path),
        })
    })();

    output.unwrap_or_else(|err| WorkerResult::Failed {
        request_id: Some(request_id),
        kind: WorkerRequestKind::Utility,
        error: err.to_string(),
    })
}

fn run_open_tiff_page(request_id: u64, path: PathBuf, page_index: u32) -> WorkerResult {
    let output = (|| -> Result<WorkerResult> {
        let (actual_page, page_count, image) = decode_tiff_page(&path, page_index)?;
        let loaded = payload_from_working_image(Arc::new(image));
        Ok(WorkerResult::TiffPageLoaded {
            request_id,
            page_index: actual_page,
            page_count,
            loaded,
        })
    })();

    output.unwrap_or_else(|err| WorkerResult::Failed {
        request_id: Some(request_id),
        kind: WorkerRequestKind::Utility,
        error: err.to_string(),
    })
}

fn run_lossless_jpeg(
    request_id: u64,
    path: PathBuf,
    op: LosslessJpegOp,
    output_path: Option<PathBuf>,
) -> WorkerResult {
    let output = (|| -> Result<WorkerResult> {
        let ext = path
            .extension()
            .map(|e| e.to_string_lossy().to_ascii_lowercase())
            .unwrap_or_default();
        if ext != "jpg" && ext != "jpeg" {
            return Err(anyhow!("lossless JPEG operations require .jpg/.jpeg input"));
        }
        if !path.is_file() {
            return Err(anyhow!("missing input file {}", path.display()));
        }

        let final_output = output_path.unwrap_or_else(|| path.clone());
        if let Some(parent) = final_output.parent() {
            fs::create_dir_all(parent)
                .with_context(|| format!("failed to create {}", parent.display()))?;
        }
        let temp_output = if final_output == path {
            final_output.with_extension("jpg.tmp")
        } else {
            final_output.clone()
        };

        let mut command = std::process::Command::new("jpegtran");
        command.arg("-copy").arg("all");
        match op {
            LosslessJpegOp::Rotate90 => {
                command.arg("-rotate").arg("90");
            }
            LosslessJpegOp::Rotate180 => {
                command.arg("-rotate").arg("180");
            }
            LosslessJpegOp::Rotate270 => {
                command.arg("-rotate").arg("270");
            }
            LosslessJpegOp::FlipHorizontal => {
                command.arg("-flip").arg("horizontal");
            }
            LosslessJpegOp::FlipVertical => {
                command.arg("-flip").arg("vertical");
            }
        }
        command.arg("-outfile").arg(&temp_output).arg(&path);
        let status = command
            .status()
            .with_context(|| "failed to execute `jpegtran` (install libjpeg-turbo tools)")?;
        if !status.success() {
            return Err(anyhow!("jpegtran failed with status {status}"));
        }

        if final_output == path {
            fs::rename(&temp_output, &path)
                .with_context(|| format!("failed to replace {}", path.display()))?;
        }

        Ok(WorkerResult::UtilityCompleted {
            request_id,
            message: format!("Lossless JPEG transform complete: {}", final_output.display()),
            open_path: Some(final_output),
        })
    })();

    output.unwrap_or_else(|err| WorkerResult::Failed {
        request_id: Some(request_id),
        kind: WorkerRequestKind::Utility,
        error: err.to_string(),
    })
}

fn run_update_exif_date(request_id: u64, path: PathBuf, datetime: String) -> WorkerResult {
    let output = (|| -> Result<WorkerResult> {
        if !path.is_file() {
            return Err(anyhow!("missing input file {}", path.display()));
        }
        let datetime = datetime.trim();
        if datetime.is_empty() {
            return Err(anyhow!("datetime is required"));
        }
        let normalized = datetime.replace('T', " ");

        let status = std::process::Command::new("exiftool")
            .arg("-overwrite_original")
            .arg(format!("-DateTimeOriginal={normalized}"))
            .arg(format!("-CreateDate={normalized}"))
            .arg(format!("-ModifyDate={normalized}"))
            .arg(&path)
            .status()
            .with_context(|| "failed to execute `exiftool` (install exiftool and ensure PATH)")?;
        if !status.success() {
            return Err(anyhow!("exiftool failed with status {status}"));
        }
        Ok(WorkerResult::UtilityCompleted {
            request_id,
            message: format!("Updated EXIF date/time for {}", path.display()),
            open_path: Some(path),
        })
    })();

    output.unwrap_or_else(|err| WorkerResult::Failed {
        request_id: Some(request_id),
        kind: WorkerRequestKind::Utility,
        error: err.to_string(),
    })
}

fn run_convert_color_profile(
    request_id: u64,
    path: PathBuf,
    output_path: PathBuf,
    source_profile: Option<PathBuf>,
    target_profile: PathBuf,
    rendering_intent: String,
) -> WorkerResult {
    let output = (|| -> Result<WorkerResult> {
        if !path.is_file() {
            return Err(anyhow!("missing input image {}", path.display()));
        }
        if !target_profile.is_file() {
            return Err(anyhow!("missing target profile {}", target_profile.display()));
        }
        if let Some(source_profile) = source_profile.as_ref() {
            if !source_profile.is_file() {
                return Err(anyhow!("missing source profile {}", source_profile.display()));
            }
        }
        if let Some(parent) = output_path.parent() {
            fs::create_dir_all(parent)
                .with_context(|| format!("failed to create {}", parent.display()))?;
        }

        let final_output = output_path;
        let temp_output = if final_output == path {
            temporary_icc_output_path(&path)
        } else {
            final_output.clone()
        };

        let intent = match rendering_intent.trim().to_ascii_lowercase().as_str() {
            "perceptual" => "perceptual",
            "saturation" => "saturation",
            "absolute" | "absolutecolorimetric" | "absolute-colorimetric" => {
                "absolute"
            }
            _ => "relative",
        };

        let mut command = std::process::Command::new("magick");
        command.arg(&path).arg("-intent").arg(intent);
        if let Some(source_profile) = source_profile.as_ref() {
            command.arg("-profile").arg(source_profile);
        }
        command
            .arg("-profile")
            .arg(&target_profile)
            .arg(&temp_output);

        let status = command
            .status()
            .with_context(|| "failed to execute `magick` (install ImageMagick with ICC support)")?;
        if !status.success() {
            return Err(anyhow!("magick color profile conversion failed with status {status}"));
        }

        if final_output == path {
            fs::rename(&temp_output, &path)
                .with_context(|| format!("failed to replace {}", path.display()))?;
        }

        Ok(WorkerResult::UtilityCompleted {
            request_id,
            message: format!("Color profile conversion complete: {}", final_output.display()),
            open_path: Some(final_output),
        })
    })();

    output.unwrap_or_else(|err| WorkerResult::Failed {
        request_id: Some(request_id),
        kind: WorkerRequestKind::Utility,
        error: err.to_string(),
    })
}

fn temporary_icc_output_path(path: &Path) -> PathBuf {
    let ext = path
        .extension()
        .map(|value| value.to_string_lossy().into_owned())
        .unwrap_or_else(|| "png".to_owned());
    let stem = path
        .file_stem()
        .map(|value| value.to_string_lossy().into_owned())
        .unwrap_or_else(|| "image".to_owned());
    path.with_file_name(format!("{stem}.imranview.icc.tmp.{ext}"))
}

#[allow(clippy::too_many_arguments)]
fn run_scan_to_directory(
    request_id: u64,
    output_dir: PathBuf,
    output_format: BatchOutputFormat,
    rename_prefix: String,
    start_index: u32,
    page_count: u32,
    jpeg_quality: u8,
    command_template: String,
) -> WorkerResult {
    let output = (|| -> Result<WorkerResult> {
        let template = command_template.trim();
        if template.is_empty() {
            return Err(anyhow!(
                "scanner command template is required (use {{output}} placeholder)"
            ));
        }
        let page_count = page_count.clamp(1, 10_000);
        fs::create_dir_all(&output_dir)
            .with_context(|| format!("failed to create {}", output_dir.display()))?;
        let extension = output_format
            .to_save_format(jpeg_quality.clamp(1, 100))
            .extension();

        let mut first_path: Option<PathBuf> = None;
        for page_offset in 0..page_count {
            let index = start_index.saturating_add(page_offset);
            let name = format!("{rename_prefix}{index:04}.{extension}");
            let output_path = output_dir.join(name);
            let command = template
                .replace("{output}", &shell_escape_path(&output_path))
                .replace("{index}", &index.to_string());
            run_shell_capture_command(&command).with_context(|| {
                format!(
                    "scanner command failed for output {}",
                    output_path.display()
                )
            })?;
            if !output_path.is_file() {
                return Err(anyhow!(
                    "scanner command completed but did not create {}",
                    output_path.display()
                ));
            }
            if first_path.is_none() {
                first_path = Some(output_path);
            }
        }

        Ok(WorkerResult::UtilityCompleted {
            request_id,
            message: format!(
                "Captured {} scan page(s) to {}",
                page_count,
                output_dir.display()
            ),
            open_path: first_path,
        })
    })();

    output.unwrap_or_else(|err| WorkerResult::Failed {
        request_id: Some(request_id),
        kind: WorkerRequestKind::Utility,
        error: err.to_string(),
    })
}

#[allow(clippy::too_many_arguments)]
fn run_scan_native(
    request_id: u64,
    output_dir: PathBuf,
    output_format: BatchOutputFormat,
    rename_prefix: String,
    start_index: u32,
    page_count: u32,
    jpeg_quality: u8,
    dpi: u32,
    grayscale: bool,
) -> WorkerResult {
    let output = (|| -> Result<WorkerResult> {
        let page_count = page_count.clamp(1, 10_000);
        let dpi = dpi.clamp(75, 1200);
        fs::create_dir_all(&output_dir)
            .with_context(|| format!("failed to create {}", output_dir.display()))?;

        let save_format = output_format.to_save_format(jpeg_quality.clamp(1, 100));
        let extension = save_format.extension();
        let mut first_path: Option<PathBuf> = None;

        for page_offset in 0..page_count {
            let index = start_index.saturating_add(page_offset);
            let final_name = format!("{rename_prefix}{index:04}.{extension}");
            let final_path = output_dir.join(final_name);
            let temp_capture = output_dir.join(format!(".imranview-scan-{index:04}.png"));

            scan_native_capture_to_png(&temp_capture, dpi, grayscale)
                .with_context(|| format!("native scan capture failed for page {}", page_offset + 1))?;

            let image = load_working_image(&temp_capture)
                .with_context(|| format!("failed to decode scanned image {}", temp_capture.display()))?;
            save_image_with_format(&final_path, &image, save_format)
                .with_context(|| format!("failed to save {}", final_path.display()))?;
            let _ = fs::remove_file(&temp_capture);

            if first_path.is_none() {
                first_path = Some(final_path.clone());
            }
        }

        Ok(WorkerResult::UtilityCompleted {
            request_id,
            message: format!(
                "Captured {} page(s) via native scanner backend to {}",
                page_count,
                output_dir.display()
            ),
            open_path: first_path,
        })
    })();

    output.unwrap_or_else(|err| WorkerResult::Failed {
        request_id: Some(request_id),
        kind: WorkerRequestKind::Utility,
        error: err.to_string(),
    })
}

fn scan_native_capture_to_png(output_path: &Path, dpi: u32, grayscale: bool) -> Result<()> {
    #[cfg(any(target_os = "linux", target_os = "macos"))]
    {
        if let Some(parent) = output_path.parent() {
            fs::create_dir_all(parent)
                .with_context(|| format!("failed to create {}", parent.display()))?;
        }
        let mode = if grayscale { "Gray" } else { "Color" };
        let status = std::process::Command::new("scanimage")
            .arg("--format=png")
            .arg("--mode")
            .arg(mode)
            .arg("--resolution")
            .arg(dpi.to_string())
            .arg("--output-file")
            .arg(output_path)
            .status()
            .with_context(|| "failed to execute `scanimage` (install SANE tools)")?;
        if !status.success() {
            return Err(anyhow!("scanimage failed with status {status}"));
        }
        if !output_path.is_file() {
            return Err(anyhow!(
                "scanimage completed but did not produce {}",
                output_path.display()
            ));
        }
        return Ok(());
    }

    #[cfg(target_os = "windows")]
    {
        if let Some(parent) = output_path.parent() {
            fs::create_dir_all(parent)
                .with_context(|| format!("failed to create {}", parent.display()))?;
        }
        let escaped = output_path.display().to_string().replace('\'', "''");
        let wia_format = if grayscale {
            "{B96B3CAA-0728-11D3-9D7B-0000F81EF32E}" // PNG
        } else {
            "{B96B3CAF-0728-11D3-9D7B-0000F81EF32E}" // JPEG
        };
        let script = format!(
            "$dialog=New-Object -ComObject WIA.CommonDialog; \
             $device=$dialog.ShowSelectDevice(1,$true,$false); \
             if($null -eq $device){{exit 2}}; \
             $item=$device.Items.Item(1); \
             $item.Properties.Item('6147').Value={dpi}; \
             $item.Properties.Item('6148').Value={dpi}; \
             $img=$dialog.ShowTransfer($item,'{wia_format}',$false); \
             if($null -eq $img){{exit 3}}; \
             $img.SaveFile('{escaped}');"
        );
        let status = std::process::Command::new("powershell")
            .args(["-NoProfile", "-Command", &script])
            .status()
            .with_context(|| "failed to execute Windows WIA scanner command")?;
        if !status.success() {
            return Err(anyhow!("Windows WIA scanner command failed with status {status}"));
        }
        if !output_path.is_file() {
            return Err(anyhow!(
                "WIA command completed but did not produce {}",
                output_path.display()
            ));
        }
        return Ok(());
    }

    #[cfg(not(any(target_os = "linux", target_os = "macos", target_os = "windows")))]
    {
        let _ = (output_path, dpi, grayscale);
        Err(anyhow!("native scanner backend is not supported on this platform"))
    }
}

fn shell_escape_path(path: &Path) -> String {
    #[cfg(target_os = "windows")]
    {
        return format!("\"{}\"", path.display().to_string().replace('"', "\"\""));
    }
    #[cfg(not(target_os = "windows"))]
    {
        format!("'{}'", path.display().to_string().replace('\'', "'\"'\"'"))
    }
}

fn run_shell_capture_command(command: &str) -> Result<()> {
    #[cfg(target_os = "windows")]
    let status = std::process::Command::new("cmd")
        .args(["/C", command])
        .status()
        .with_context(|| format!("failed to launch command: {command}"))?;

    #[cfg(not(target_os = "windows"))]
    let status = std::process::Command::new("sh")
        .args(["-c", command])
        .status()
        .with_context(|| format!("failed to launch command: {command}"))?;

    if status.success() {
        Ok(())
    } else {
        Err(anyhow!("command exited with status {status}: {command}"))
    }
}

fn run_extract_tiff_pages(
    request_id: u64,
    path: PathBuf,
    output_dir: PathBuf,
    output_format: BatchOutputFormat,
    jpeg_quality: u8,
) -> WorkerResult {
    let output = (|| -> Result<WorkerResult> {
        let pages = decode_tiff_pages(&path)?;
        if pages.is_empty() {
            return Err(anyhow!("no pages found in {}", path.display()));
        }
        fs::create_dir_all(&output_dir)
            .with_context(|| format!("failed to create {}", output_dir.display()))?;

        let save_format = output_format.to_save_format(jpeg_quality.clamp(1, 100));
        let extension = save_format.extension();
        for (index, image) in pages.iter().enumerate() {
            let file_name = format!("page-{:04}.{extension}", index + 1);
            let output_path = output_dir.join(file_name);
            save_image_with_format(&output_path, image, save_format)
                .with_context(|| format!("failed to save {}", output_path.display()))?;
        }
        Ok(WorkerResult::UtilityCompleted {
            request_id,
            message: format!(
                "Extracted {} TIFF page(s) to {}",
                pages.len(),
                output_dir.display()
            ),
            open_path: None,
        })
    })();

    output.unwrap_or_else(|err| WorkerResult::Failed {
        request_id: Some(request_id),
        kind: WorkerRequestKind::Utility,
        error: err.to_string(),
    })
}

fn run_create_multipage_pdf(
    request_id: u64,
    input_paths: Vec<PathBuf>,
    output_path: PathBuf,
    jpeg_quality: u8,
) -> WorkerResult {
    let output = (|| -> Result<WorkerResult> {
        if input_paths.is_empty() {
            return Err(anyhow!("select at least one image for PDF creation"));
        }
        if let Some(parent) = output_path.parent() {
            fs::create_dir_all(parent)
                .with_context(|| format!("failed to create {}", parent.display()))?;
        }

        let mut pages = Vec::with_capacity(input_paths.len());
        for path in &input_paths {
            let image = load_working_image(path)
                .map_err(|err| anyhow!("failed to load {}: {err}", path.display()))?;
            pages.push(pdf_page_from_image(&image, jpeg_quality.clamp(1, 100))?);
        }
        let pdf = build_simple_pdf(&pages)?;
        fs::write(&output_path, pdf)
            .with_context(|| format!("failed to write {}", output_path.display()))?;

        Ok(WorkerResult::UtilityCompleted {
            request_id,
            message: format!(
                "Created PDF with {} page(s): {}",
                input_paths.len(),
                output_path.display()
            ),
            open_path: None,
        })
    })();

    output.unwrap_or_else(|err| WorkerResult::Failed {
        request_id: Some(request_id),
        kind: WorkerRequestKind::Utility,
        error: err.to_string(),
    })
}

fn run_ocr(
    request_id: u64,
    path: PathBuf,
    language: String,
    output_path: Option<PathBuf>,
) -> WorkerResult {
    let output = (|| -> Result<WorkerResult> {
        if !path.exists() {
            return Err(anyhow!("cannot OCR missing file {}", path.display()));
        }
        let language = if language.trim().is_empty() {
            "eng".to_owned()
        } else {
            language.trim().to_owned()
        };
        let command = std::process::Command::new("tesseract")
            .arg(&path)
            .arg("stdout")
            .arg("-l")
            .arg(&language)
            .output()
            .with_context(
                || "failed to execute `tesseract` (install it and ensure it is on PATH)",
            )?;
        if !command.status.success() {
            let stderr = String::from_utf8_lossy(&command.stderr);
            return Err(anyhow!("tesseract failed: {}", stderr.trim()));
        }
        let text = String::from_utf8(command.stdout).context("OCR output is not valid UTF-8")?;
        if let Some(path) = output_path.as_ref() {
            if let Some(parent) = path.parent() {
                fs::create_dir_all(parent)
                    .with_context(|| format!("failed to create {}", parent.display()))?;
            }
            fs::write(path, &text)
                .with_context(|| format!("failed to write {}", path.display()))?;
        }
        Ok(WorkerResult::OcrCompleted {
            request_id,
            output_path,
            text,
        })
    })();

    output.unwrap_or_else(|err| WorkerResult::Failed {
        request_id: Some(request_id),
        kind: WorkerRequestKind::Ocr,
        error: err.to_string(),
    })
}

fn run_stitch_panorama(
    request_id: u64,
    input_paths: Vec<PathBuf>,
    output_path: PathBuf,
    direction: PanoramaDirection,
    overlap_percent: f32,
) -> WorkerResult {
    let output = (|| -> Result<WorkerResult> {
        if input_paths.len() < 2 {
            return Err(anyhow!("select at least two images for panorama"));
        }
        if let Some(parent) = output_path.parent() {
            fs::create_dir_all(parent)
                .with_context(|| format!("failed to create {}", parent.display()))?;
        }
        let mut images = Vec::with_capacity(input_paths.len());
        for path in &input_paths {
            let image = load_working_image(path)
                .map_err(|err| anyhow!("failed to load {}: {err}", path.display()))?;
            images.push(image.to_rgba8());
        }
        let stitched = stitch_images(&images, direction, overlap_percent.clamp(0.0, 0.9));
        let stitched = DynamicImage::ImageRgba8(stitched);
        let save_format = infer_save_format(&output_path, 90).unwrap_or(SaveFormat::Png);
        save_image_with_format(&output_path, &stitched, save_format)
            .with_context(|| format!("failed to save {}", output_path.display()))?;
        Ok(WorkerResult::UtilityCompleted {
            request_id,
            message: format!("Panorama written to {}", output_path.display()),
            open_path: Some(output_path),
        })
    })();

    output.unwrap_or_else(|err| WorkerResult::Failed {
        request_id: Some(request_id),
        kind: WorkerRequestKind::Utility,
        error: err.to_string(),
    })
}

#[allow(clippy::too_many_arguments)]
fn run_export_contact_sheet(
    request_id: u64,
    input_paths: Vec<PathBuf>,
    output_path: PathBuf,
    columns: u32,
    thumb_size: u32,
    include_labels: bool,
    background: [u8; 4],
    label_color: [u8; 4],
    jpeg_quality: u8,
) -> WorkerResult {
    let output = (|| -> Result<WorkerResult> {
        if input_paths.is_empty() {
            return Err(anyhow!("no input images selected for contact sheet"));
        }
        if let Some(parent) = output_path.parent() {
            fs::create_dir_all(parent)
                .with_context(|| format!("failed to create {}", parent.display()))?;
        }

        let columns = columns.max(1);
        let thumb_size = thumb_size.clamp(32, 1024);
        let rows = ((input_paths.len() as f32) / columns as f32).ceil() as u32;
        let label_height = if include_labels { 18 } else { 0 };
        let card_w = thumb_size + 16;
        let card_h = thumb_size + 16 + label_height;
        let sheet_w = (columns * card_w).max(1);
        let sheet_h = (rows * card_h).max(1);
        let mut sheet = RgbaImage::from_pixel(sheet_w, sheet_h, Rgba(background));

        for (index, path) in input_paths.iter().enumerate() {
            let image = load_working_image(path)
                .map_err(|err| anyhow!("failed to load {}: {err}", path.display()))?;
            let thumb = image.thumbnail(thumb_size, thumb_size).to_rgba8();
            let col = index as u32 % columns;
            let row = index as u32 / columns;
            let base_x = col * card_w;
            let base_y = row * card_h;
            let x = base_x + (card_w.saturating_sub(thumb.width())) / 2;
            let y = base_y + 8 + (thumb_size.saturating_sub(thumb.height())) / 2;
            blit_rgba(&thumb, &mut sheet, x, y);

            if include_labels {
                let label = path
                    .file_name()
                    .map(|name| name.to_string_lossy().into_owned())
                    .unwrap_or_else(|| path.display().to_string());
                draw_bitmap_text(
                    &mut sheet,
                    &label,
                    (base_x + 6) as i32,
                    (base_y + thumb_size + 10) as i32,
                    1,
                    Rgba(label_color),
                );
            }
        }

        let save_format =
            infer_save_format(&output_path, jpeg_quality.clamp(1, 100)).unwrap_or(SaveFormat::Png);
        let output = DynamicImage::ImageRgba8(sheet);
        save_image_with_format(&output_path, &output, save_format)
            .with_context(|| format!("failed to save {}", output_path.display()))?;

        Ok(WorkerResult::UtilityCompleted {
            request_id,
            message: format!("Contact sheet exported to {}", output_path.display()),
            open_path: Some(output_path),
        })
    })();

    output.unwrap_or_else(|err| WorkerResult::Failed {
        request_id: Some(request_id),
        kind: WorkerRequestKind::Utility,
        error: err.to_string(),
    })
}

fn run_export_html_gallery(
    request_id: u64,
    input_paths: Vec<PathBuf>,
    output_dir: PathBuf,
    title: String,
    thumb_width: u32,
) -> WorkerResult {
    let output = (|| -> Result<WorkerResult> {
        if input_paths.is_empty() {
            return Err(anyhow!("no input images selected for HTML export"));
        }
        fs::create_dir_all(&output_dir)
            .with_context(|| format!("failed to create {}", output_dir.display()))?;
        let thumbs_dir = output_dir.join("thumbs");
        let images_dir = output_dir.join("images");
        fs::create_dir_all(&thumbs_dir)
            .with_context(|| format!("failed to create {}", thumbs_dir.display()))?;
        fs::create_dir_all(&images_dir)
            .with_context(|| format!("failed to create {}", images_dir.display()))?;

        let mut items = Vec::with_capacity(input_paths.len());
        for (index, path) in input_paths.iter().enumerate() {
            let source_name = path
                .file_name()
                .map(|name| name.to_string_lossy().into_owned())
                .unwrap_or_else(|| format!("image-{index:04}.png"));
            let image_target = images_dir.join(format!("{index:04}-{source_name}"));
            fs::copy(path, &image_target).with_context(|| {
                format!(
                    "failed to copy source image {} to {}",
                    path.display(),
                    image_target.display()
                )
            })?;

            let image = load_working_image(path)
                .map_err(|err| anyhow!("failed to load {}: {err}", path.display()))?;
            let thumb = image.thumbnail(thumb_width.max(32), thumb_width.max(32));
            let thumb_name = format!("{index:04}.jpg");
            let thumb_target = thumbs_dir.join(&thumb_name);
            save_image_with_format(&thumb_target, &thumb, SaveFormat::Jpeg { quality: 90 })
                .with_context(|| format!("failed to save {}", thumb_target.display()))?;

            items.push((
                source_name,
                format!(
                    "images/{}",
                    image_target.file_name().unwrap().to_string_lossy()
                ),
                format!("thumbs/{thumb_name}"),
            ));
        }

        let safe_title = if title.trim().is_empty() {
            "ImranView Gallery".to_owned()
        } else {
            title.trim().to_owned()
        };
        let mut html = String::new();
        html.push_str("<!doctype html><html><head><meta charset=\"utf-8\">");
        html.push_str(&format!("<title>{}</title>", html_escape(&safe_title)));
        html.push_str(
            "<style>body{font-family:system-ui,sans-serif;margin:24px;background:#111;color:#f3f3f3}h1{font-size:20px;margin:0 0 16px}\
            .grid{display:grid;grid-template-columns:repeat(auto-fill,minmax(180px,1fr));gap:14px}\
            .card{background:#1d1d1d;border:1px solid #303030;border-radius:8px;padding:8px}\
            .card img{width:100%;height:auto;display:block;border-radius:4px}\
            .name{font-size:12px;overflow-wrap:anywhere;margin-top:8px;color:#ddd}</style></head><body>",
        );
        html.push_str(&format!(
            "<h1>{}</h1><div class=\"grid\">",
            html_escape(&safe_title)
        ));
        for (name, image_href, thumb_href) in items {
            html.push_str("<a class=\"card\" href=\"");
            html.push_str(&html_escape(&image_href));
            html.push_str("\"><img src=\"");
            html.push_str(&html_escape(&thumb_href));
            html.push_str("\" alt=\"");
            html.push_str(&html_escape(&name));
            html.push_str("\"><div class=\"name\">");
            html.push_str(&html_escape(&name));
            html.push_str("</div></a>");
        }
        html.push_str("</div></body></html>");

        let index_path = output_dir.join("index.html");
        fs::write(&index_path, html)
            .with_context(|| format!("failed to write {}", index_path.display()))?;

        Ok(WorkerResult::UtilityCompleted {
            request_id,
            message: format!("HTML gallery exported to {}", index_path.display()),
            open_path: None,
        })
    })();

    output.unwrap_or_else(|err| WorkerResult::Failed {
        request_id: Some(request_id),
        kind: WorkerRequestKind::Utility,
        error: err.to_string(),
    })
}

fn default_screenshot_path() -> PathBuf {
    let timestamp_ms = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|duration| duration.as_millis())
        .unwrap_or(0);
    std::env::temp_dir().join(format!("imranview-screenshot-{timestamp_ms}.png"))
}

fn capture_screenshot_to_path(
    path: &Path,
    delay_ms: u64,
    region: Option<(u32, u32, u32, u32)>,
) -> Result<()> {
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent)
            .with_context(|| format!("failed to create {}", parent.display()))?;
    }
    if delay_ms > 0 {
        thread::sleep(std::time::Duration::from_millis(delay_ms));
    }

    #[cfg(target_os = "macos")]
    {
        let mut command = std::process::Command::new("screencapture");
        command.arg("-x");
        if let Some((x, y, width, height)) = region {
            command.arg(format!("-R{x},{y},{width},{height}"));
        }
        command.arg(path);
        let status = command
            .status()
            .context("failed to execute macOS screencapture command")?;
        if !status.success() {
            return Err(anyhow!("screencapture failed with status {}", status));
        }
        return Ok(());
    }

    #[cfg(target_os = "linux")]
    {
        let run_and_check = |mut command: std::process::Command,
                             label: &str|
         -> Result<Option<std::process::ExitStatus>> {
            match command.status() {
                Ok(status) => Ok(Some(status)),
                Err(err) if err.kind() == std::io::ErrorKind::NotFound => Ok(None),
                Err(err) => Err(anyhow!("failed to execute {label}: {err}")),
            }
        };

        if let Some((x, y, width, height)) = region {
            let mut grim = std::process::Command::new("grim");
            grim.arg("-g")
                .arg(format!("{x},{y} {width}x{height}"))
                .arg(path);
            if let Some(status) = run_and_check(grim, "grim")? {
                if status.success() {
                    return Ok(());
                }
                return Err(anyhow!("grim failed with status {}", status));
            }
        } else {
            let mut grim = std::process::Command::new("grim");
            grim.arg(path);
            if let Some(status) = run_and_check(grim, "grim")? {
                if status.success() {
                    return Ok(());
                }
                return Err(anyhow!("grim failed with status {}", status));
            }
        }

        let mut gnome = std::process::Command::new("gnome-screenshot");
        gnome.arg("-f").arg(path);
        if delay_ms > 0 {
            gnome
                .arg("-d")
                .arg((delay_ms as f32 / 1000.0).ceil().to_string());
        }
        if let Some(status) = run_and_check(gnome, "gnome-screenshot")? {
            if status.success() {
                if let Some(region) = region {
                    crop_image_file_in_place(path, region)?;
                }
                return Ok(());
            }
            return Err(anyhow!("gnome-screenshot failed with status {}", status));
        }

        let mut import = std::process::Command::new("import");
        import.arg("-window").arg("root").arg(path);
        if let Some(status) = run_and_check(import, "import")? {
            if status.success() {
                if let Some(region) = region {
                    crop_image_file_in_place(path, region)?;
                }
                return Ok(());
            }
            return Err(anyhow!("import failed with status {}", status));
        }

        let mut scrot = std::process::Command::new("scrot");
        scrot.arg(path);
        if delay_ms > 0 {
            scrot
                .arg("-d")
                .arg((delay_ms as f32 / 1000.0).ceil().to_string());
        }
        if let Some(status) = run_and_check(scrot, "scrot")? {
            if status.success() {
                if let Some(region) = region {
                    crop_image_file_in_place(path, region)?;
                }
                return Ok(());
            }
            return Err(anyhow!("scrot failed with status {}", status));
        }

        return Err(anyhow!(
            "no screenshot backend found (install grim, gnome-screenshot, import, or scrot)"
        ));
    }

    #[cfg(target_os = "windows")]
    {
        let escaped_path = path.display().to_string().replace('\'', "''");
        let script = format!(
            "Add-Type -AssemblyName System.Windows.Forms; \
             Add-Type -AssemblyName System.Drawing; \
             $bounds=[System.Windows.Forms.Screen]::PrimaryScreen.Bounds; \
             $bitmap=New-Object System.Drawing.Bitmap($bounds.Width,$bounds.Height); \
             $graphics=[System.Drawing.Graphics]::FromImage($bitmap); \
             $graphics.CopyFromScreen($bounds.Location,[System.Drawing.Point]::Empty,$bounds.Size); \
             $bitmap.Save('{escaped_path}', [System.Drawing.Imaging.ImageFormat]::Png); \
             $graphics.Dispose(); \
             $bitmap.Dispose();"
        );
        let status = std::process::Command::new("powershell")
            .args(["-NoProfile", "-Command", &script])
            .status()
            .context("failed to execute PowerShell screenshot command")?;
        if !status.success() {
            return Err(anyhow!(
                "PowerShell screenshot failed with status {}",
                status
            ));
        }
        if let Some(region) = region {
            crop_image_file_in_place(path, region)?;
        }
        return Ok(());
    }

    #[cfg(not(any(target_os = "macos", target_os = "linux", target_os = "windows")))]
    {
        let _ = (path, region);
        return Err(anyhow!(
            "screenshot capture is not supported on this platform"
        ));
    }
}

#[cfg(any(target_os = "linux", target_os = "windows"))]
fn crop_image_file_in_place(path: &Path, region: (u32, u32, u32, u32)) -> Result<()> {
    let (x, y, width, height) = region;
    if width == 0 || height == 0 {
        return Ok(());
    }
    let image = image::open(path).with_context(|| format!("failed to open {}", path.display()))?;
    let (source_w, source_h) = image.dimensions();
    if x >= source_w || y >= source_h {
        return Ok(());
    }
    let crop_w = width.min(source_w - x);
    let crop_h = height.min(source_h - y);
    let cropped = image.crop_imm(x, y, crop_w, crop_h);
    cropped
        .save(path)
        .with_context(|| format!("failed to save cropped screenshot {}", path.display()))
}

fn decode_tiff_page(path: &Path, requested_page: u32) -> Result<(u32, u32, DynamicImage)> {
    let pages = decode_tiff_pages(path)?;
    if pages.is_empty() {
        return Err(anyhow!("TIFF has no decodable pages"));
    }
    let page_count = pages.len() as u32;
    let actual_page = requested_page.min(page_count.saturating_sub(1));
    let image = pages
        .into_iter()
        .nth(actual_page as usize)
        .context("failed to fetch requested TIFF page")?;
    Ok((actual_page, page_count, image))
}

fn decode_tiff_pages(path: &Path) -> Result<Vec<DynamicImage>> {
    let file =
        std::fs::File::open(path).with_context(|| format!("failed to open {}", path.display()))?;
    let mut decoder = tiff::decoder::Decoder::new(std::io::BufReader::new(file))
        .with_context(|| format!("failed to decode TIFF headers {}", path.display()))?;
    let mut pages = Vec::new();

    loop {
        let (width, height) = decoder
            .dimensions()
            .with_context(|| format!("failed to read dimensions for {}", path.display()))?;
        let color_type = decoder
            .colortype()
            .with_context(|| format!("failed to read color type for {}", path.display()))?;
        let decoded = decoder
            .read_image()
            .with_context(|| format!("failed to decode TIFF page for {}", path.display()))?;
        let rgba = tiff_decoding_to_rgba(decoded, color_type, width, height)?;
        pages.push(DynamicImage::ImageRgba8(rgba));

        if !decoder.more_images() {
            break;
        }
        decoder
            .next_image()
            .with_context(|| format!("failed to read next TIFF page in {}", path.display()))?;
    }

    Ok(pages)
}

fn tiff_decoding_to_rgba(
    decoded: tiff::decoder::DecodingResult,
    color_type: tiff::ColorType,
    width: u32,
    height: u32,
) -> Result<RgbaImage> {
    let pixel_count = width as usize * height as usize;
    let mut rgba = vec![0u8; pixel_count * 4];
    match decoded {
        tiff::decoder::DecodingResult::U8(buffer) => match color_type {
            tiff::ColorType::Gray(_) => {
                if buffer.len() < pixel_count {
                    return Err(anyhow!("invalid TIFF grayscale buffer length"));
                }
                for index in 0..pixel_count {
                    let g = buffer[index];
                    rgba[index * 4] = g;
                    rgba[index * 4 + 1] = g;
                    rgba[index * 4 + 2] = g;
                    rgba[index * 4 + 3] = 255;
                }
            }
            tiff::ColorType::GrayA(_) => {
                if buffer.len() < pixel_count * 2 {
                    return Err(anyhow!("invalid TIFF gray+alpha buffer length"));
                }
                for index in 0..pixel_count {
                    let base = index * 2;
                    let g = buffer[base];
                    rgba[index * 4] = g;
                    rgba[index * 4 + 1] = g;
                    rgba[index * 4 + 2] = g;
                    rgba[index * 4 + 3] = buffer[base + 1];
                }
            }
            tiff::ColorType::RGB(_) => {
                if buffer.len() < pixel_count * 3 {
                    return Err(anyhow!("invalid TIFF RGB buffer length"));
                }
                for index in 0..pixel_count {
                    let base = index * 3;
                    rgba[index * 4] = buffer[base];
                    rgba[index * 4 + 1] = buffer[base + 1];
                    rgba[index * 4 + 2] = buffer[base + 2];
                    rgba[index * 4 + 3] = 255;
                }
            }
            tiff::ColorType::RGBA(_) => {
                if buffer.len() < pixel_count * 4 {
                    return Err(anyhow!("invalid TIFF RGBA buffer length"));
                }
                rgba.copy_from_slice(&buffer[..pixel_count * 4]);
            }
            other => {
                return Err(anyhow!("unsupported TIFF color type: {:?}", other));
            }
        },
        tiff::decoder::DecodingResult::U16(buffer) => match color_type {
            tiff::ColorType::Gray(_) => {
                if buffer.len() < pixel_count {
                    return Err(anyhow!("invalid TIFF grayscale16 buffer length"));
                }
                for index in 0..pixel_count {
                    let g = (buffer[index] >> 8) as u8;
                    rgba[index * 4] = g;
                    rgba[index * 4 + 1] = g;
                    rgba[index * 4 + 2] = g;
                    rgba[index * 4 + 3] = 255;
                }
            }
            tiff::ColorType::GrayA(_) => {
                if buffer.len() < pixel_count * 2 {
                    return Err(anyhow!("invalid TIFF gray16+alpha16 buffer length"));
                }
                for index in 0..pixel_count {
                    let base = index * 2;
                    let g = (buffer[base] >> 8) as u8;
                    rgba[index * 4] = g;
                    rgba[index * 4 + 1] = g;
                    rgba[index * 4 + 2] = g;
                    rgba[index * 4 + 3] = (buffer[base + 1] >> 8) as u8;
                }
            }
            tiff::ColorType::RGB(_) => {
                if buffer.len() < pixel_count * 3 {
                    return Err(anyhow!("invalid TIFF RGB16 buffer length"));
                }
                for index in 0..pixel_count {
                    let base = index * 3;
                    rgba[index * 4] = (buffer[base] >> 8) as u8;
                    rgba[index * 4 + 1] = (buffer[base + 1] >> 8) as u8;
                    rgba[index * 4 + 2] = (buffer[base + 2] >> 8) as u8;
                    rgba[index * 4 + 3] = 255;
                }
            }
            tiff::ColorType::RGBA(_) => {
                if buffer.len() < pixel_count * 4 {
                    return Err(anyhow!("invalid TIFF RGBA16 buffer length"));
                }
                for index in 0..pixel_count {
                    let base = index * 4;
                    rgba[index * 4] = (buffer[base] >> 8) as u8;
                    rgba[index * 4 + 1] = (buffer[base + 1] >> 8) as u8;
                    rgba[index * 4 + 2] = (buffer[base + 2] >> 8) as u8;
                    rgba[index * 4 + 3] = (buffer[base + 3] >> 8) as u8;
                }
            }
            other => {
                return Err(anyhow!("unsupported TIFF color type: {:?}", other));
            }
        },
        other => {
            return Err(anyhow!(
                "unsupported TIFF sample type for decode: {:?}",
                other
            ));
        }
    }

    RgbaImage::from_raw(width, height, rgba).context("failed to construct RGBA TIFF page")
}

struct PdfPageData {
    width: u32,
    height: u32,
    jpeg_bytes: Vec<u8>,
}

fn pdf_page_from_image(image: &DynamicImage, jpeg_quality: u8) -> Result<PdfPageData> {
    let rgb = image.to_rgb8();
    let (width, height) = rgb.dimensions();
    let mut jpeg = Vec::new();
    let mut encoder =
        image::codecs::jpeg::JpegEncoder::new_with_quality(&mut jpeg, jpeg_quality.clamp(1, 100));
    encoder
        .encode_image(&DynamicImage::ImageRgb8(rgb))
        .context("failed to encode JPEG page for PDF")?;
    Ok(PdfPageData {
        width,
        height,
        jpeg_bytes: jpeg,
    })
}

fn build_simple_pdf(pages: &[PdfPageData]) -> Result<Vec<u8>> {
    if pages.is_empty() {
        return Err(anyhow!("cannot build PDF without pages"));
    }

    let mut objects: Vec<(u32, Vec<u8>)> = Vec::new();
    let mut kids = String::new();
    let mut next_id = 3u32;

    for (index, page) in pages.iter().enumerate() {
        let page_id = next_id;
        let content_id = next_id + 1;
        let image_id = next_id + 2;
        next_id += 3;

        if !kids.is_empty() {
            kids.push(' ');
        }
        kids.push_str(&format!("{page_id} 0 R"));

        let width_pt = ((page.width as f32) * 72.0 / 96.0).max(1.0);
        let height_pt = ((page.height as f32) * 72.0 / 96.0).max(1.0);
        let image_name = format!("Im{}", index + 1);
        let content_stream = format!(
            "q\n{} 0 0 {} 0 0 cm\n/{} Do\nQ\n",
            format_pdf_num(width_pt),
            format_pdf_num(height_pt),
            image_name
        );
        let mut content_obj =
            format!("<< /Length {} >>\nstream\n", content_stream.len()).into_bytes();
        content_obj.extend_from_slice(content_stream.as_bytes());
        content_obj.extend_from_slice(b"endstream\n");
        objects.push((content_id, content_obj));

        let mut image_obj = format!(
            "<< /Type /XObject /Subtype /Image /Width {} /Height {} /ColorSpace /DeviceRGB /BitsPerComponent 8 /Filter /DCTDecode /Length {} >>\nstream\n",
            page.width,
            page.height,
            page.jpeg_bytes.len()
        )
        .into_bytes();
        image_obj.extend_from_slice(&page.jpeg_bytes);
        image_obj.extend_from_slice(b"\nendstream\n");
        objects.push((image_id, image_obj));

        let page_obj = format!(
            "<< /Type /Page /Parent 2 0 R /MediaBox [0 0 {} {}] /Resources << /XObject << /{} {} 0 R >> >> /Contents {} 0 R >>\n",
            format_pdf_num(width_pt),
            format_pdf_num(height_pt),
            image_name,
            image_id,
            content_id
        )
        .into_bytes();
        objects.push((page_id, page_obj));
    }

    objects.push((
        2,
        format!(
            "<< /Type /Pages /Count {} /Kids [{}] >>\n",
            pages.len(),
            kids
        )
        .into_bytes(),
    ));
    objects.push((1, b"<< /Type /Catalog /Pages 2 0 R >>\n".to_vec()));

    objects.sort_by_key(|(id, _)| *id);
    let max_id = objects
        .iter()
        .map(|(id, _)| *id)
        .max()
        .context("missing PDF objects")?;

    let mut pdf = Vec::new();
    pdf.extend_from_slice(b"%PDF-1.4\n%\xE2\xE3\xCF\xD3\n");
    let mut offsets = vec![0usize; max_id as usize + 1];
    for (id, object) in objects {
        offsets[id as usize] = pdf.len();
        pdf.extend_from_slice(format!("{id} 0 obj\n").as_bytes());
        pdf.extend_from_slice(&object);
        pdf.extend_from_slice(b"endobj\n");
    }

    let xref_offset = pdf.len();
    pdf.extend_from_slice(format!("xref\n0 {}\n", max_id + 1).as_bytes());
    pdf.extend_from_slice(b"0000000000 65535 f \n");
    for object_id in 1..=max_id {
        pdf.extend_from_slice(format!("{:010} 00000 n \n", offsets[object_id as usize]).as_bytes());
    }
    pdf.extend_from_slice(
        format!(
            "trailer\n<< /Size {} /Root 1 0 R >>\nstartxref\n{}\n%%EOF\n",
            max_id + 1,
            xref_offset
        )
        .as_bytes(),
    );
    Ok(pdf)
}

fn format_pdf_num(value: f32) -> String {
    format!("{:.3}", value)
        .trim_end_matches('0')
        .trim_end_matches('.')
        .to_owned()
}

fn stitch_images(
    images: &[RgbaImage],
    direction: PanoramaDirection,
    overlap_percent: f32,
) -> RgbaImage {
    let mut iter = images.iter();
    let Some(first) = iter.next() else {
        return RgbaImage::from_pixel(1, 1, Rgba([0, 0, 0, 255]));
    };
    let mut canvas = first.clone();

    for image in iter {
        match direction {
            PanoramaDirection::Horizontal => {
                let overlap =
                    ((canvas.width().min(image.width()) as f32) * overlap_percent).round() as u32;
                let overlap = overlap.min(canvas.width().min(image.width()).saturating_sub(1));
                let mut next = RgbaImage::from_pixel(
                    canvas.width() + image.width() - overlap,
                    canvas.height().max(image.height()),
                    Rgba([0, 0, 0, 255]),
                );
                blit_rgba(&canvas, &mut next, 0, 0);
                let x_start = canvas.width() - overlap;
                blend_with_vertical_seam(&canvas, image, &mut next, x_start, 0, overlap);
                canvas = next;
            }
            PanoramaDirection::Vertical => {
                let overlap =
                    ((canvas.height().min(image.height()) as f32) * overlap_percent).round() as u32;
                let overlap = overlap.min(canvas.height().min(image.height()).saturating_sub(1));
                let mut next = RgbaImage::from_pixel(
                    canvas.width().max(image.width()),
                    canvas.height() + image.height() - overlap,
                    Rgba([0, 0, 0, 255]),
                );
                blit_rgba(&canvas, &mut next, 0, 0);
                let y_start = canvas.height() - overlap;
                blend_with_horizontal_seam(&canvas, image, &mut next, 0, y_start, overlap);
                canvas = next;
            }
        }
    }
    canvas
}

fn blend_with_vertical_seam(
    existing: &RgbaImage,
    incoming: &RgbaImage,
    out: &mut RgbaImage,
    offset_x: u32,
    offset_y: u32,
    overlap: u32,
) {
    if overlap == 0 {
        blit_rgba(incoming, out, offset_x, offset_y);
        return;
    }

    let overlap_w = overlap
        .min(incoming.width())
        .min(existing.width().saturating_sub(offset_x))
        .min(out.width().saturating_sub(offset_x));
    let overlap_h = incoming
        .height()
        .min(existing.height().saturating_sub(offset_y))
        .min(out.height().saturating_sub(offset_y));

    if overlap_w == 0 || overlap_h == 0 {
        blit_rgba(incoming, out, offset_x, offset_y);
        return;
    }

    let mut cost = vec![vec![0u32; overlap_w as usize]; overlap_h as usize];
    for y in 0..overlap_h {
        for x in 0..overlap_w {
            let existing_px = existing.get_pixel(offset_x + x, offset_y + y);
            let incoming_px = incoming.get_pixel(x, y);
            cost[y as usize][x as usize] = rgb_distance_sq(*existing_px, *incoming_px);
        }
    }
    let seam = compute_vertical_seam(&cost);
    let blend_band = ((overlap_w as i32) / 24).clamp(1, 6);

    for y in 0..incoming.height() {
        for x in 0..incoming.width() {
            let dx = offset_x + x;
            let dy = offset_y + y;
            if dx >= out.width() || dy >= out.height() {
                continue;
            }

            let src_px = *incoming.get_pixel(x, y);
            if x >= overlap_w || y >= overlap_h {
                out.put_pixel(dx, dy, src_px);
                continue;
            }

            let seam_x = seam[y as usize] as i32;
            let x_i = x as i32;
            if x_i < seam_x - blend_band {
                continue;
            }
            if x_i > seam_x + blend_band {
                out.put_pixel(dx, dy, src_px);
                continue;
            }

            let blend_t = if blend_band <= 0 {
                0.5
            } else {
                let left = seam_x - blend_band;
                ((x_i - left) as f32 / (blend_band * 2) as f32).clamp(0.0, 1.0)
            };
            let dst_px = *out.get_pixel(dx, dy);
            out.put_pixel(dx, dy, mix_rgba(dst_px, src_px, blend_t));
        }
    }
}

fn blend_with_horizontal_seam(
    existing: &RgbaImage,
    incoming: &RgbaImage,
    out: &mut RgbaImage,
    offset_x: u32,
    offset_y: u32,
    overlap: u32,
) {
    if overlap == 0 {
        blit_rgba(incoming, out, offset_x, offset_y);
        return;
    }

    let overlap_h = overlap
        .min(incoming.height())
        .min(existing.height().saturating_sub(offset_y))
        .min(out.height().saturating_sub(offset_y));
    let overlap_w = incoming
        .width()
        .min(existing.width().saturating_sub(offset_x))
        .min(out.width().saturating_sub(offset_x));

    if overlap_w == 0 || overlap_h == 0 {
        blit_rgba(incoming, out, offset_x, offset_y);
        return;
    }

    let mut cost = vec![vec![0u32; overlap_h as usize]; overlap_w as usize];
    for x in 0..overlap_w {
        for y in 0..overlap_h {
            let existing_px = existing.get_pixel(offset_x + x, offset_y + y);
            let incoming_px = incoming.get_pixel(x, y);
            cost[x as usize][y as usize] = rgb_distance_sq(*existing_px, *incoming_px);
        }
    }
    let seam = compute_horizontal_seam(&cost);
    let blend_band = ((overlap_h as i32) / 24).clamp(1, 6);

    for y in 0..incoming.height() {
        for x in 0..incoming.width() {
            let dx = offset_x + x;
            let dy = offset_y + y;
            if dx >= out.width() || dy >= out.height() {
                continue;
            }

            let src_px = *incoming.get_pixel(x, y);
            if y >= overlap_h || x >= overlap_w {
                out.put_pixel(dx, dy, src_px);
                continue;
            }

            let seam_y = seam[x as usize] as i32;
            let y_i = y as i32;
            if y_i < seam_y - blend_band {
                continue;
            }
            if y_i > seam_y + blend_band {
                out.put_pixel(dx, dy, src_px);
                continue;
            }

            let blend_t = if blend_band <= 0 {
                0.5
            } else {
                let top = seam_y - blend_band;
                ((y_i - top) as f32 / (blend_band * 2) as f32).clamp(0.0, 1.0)
            };
            let dst_px = *out.get_pixel(dx, dy);
            out.put_pixel(dx, dy, mix_rgba(dst_px, src_px, blend_t));
        }
    }
}

fn compute_vertical_seam(cost: &[Vec<u32>]) -> Vec<usize> {
    let rows = cost.len();
    let cols = cost.first().map(|row| row.len()).unwrap_or(0);
    if rows == 0 || cols == 0 {
        return Vec::new();
    }

    let mut dp = vec![vec![0u64; cols]; rows];
    let mut parent = vec![vec![0usize; cols]; rows];
    for x in 0..cols {
        dp[0][x] = cost[0][x] as u64;
        parent[0][x] = x;
    }

    for y in 1..rows {
        for x in 0..cols {
            let mut best_prev = x;
            let mut best_cost = dp[y - 1][x];
            if x > 0 && dp[y - 1][x - 1] < best_cost {
                best_cost = dp[y - 1][x - 1];
                best_prev = x - 1;
            }
            if x + 1 < cols && dp[y - 1][x + 1] < best_cost {
                best_cost = dp[y - 1][x + 1];
                best_prev = x + 1;
            }
            dp[y][x] = best_cost + cost[y][x] as u64;
            parent[y][x] = best_prev;
        }
    }

    let mut end_x = 0usize;
    let mut end_cost = dp[rows - 1][0];
    for x in 1..cols {
        if dp[rows - 1][x] < end_cost {
            end_cost = dp[rows - 1][x];
            end_x = x;
        }
    }

    let mut seam = vec![0usize; rows];
    let mut x = end_x;
    for y in (0..rows).rev() {
        seam[y] = x;
        if y > 0 {
            x = parent[y][x];
        }
    }
    seam
}

fn compute_horizontal_seam(cost: &[Vec<u32>]) -> Vec<usize> {
    let cols = cost.len();
    let rows = cost.first().map(|column| column.len()).unwrap_or(0);
    if cols == 0 || rows == 0 {
        return Vec::new();
    }

    let mut dp = vec![vec![0u64; rows]; cols];
    let mut parent = vec![vec![0usize; rows]; cols];
    for y in 0..rows {
        dp[0][y] = cost[0][y] as u64;
        parent[0][y] = y;
    }

    for x in 1..cols {
        for y in 0..rows {
            let mut best_prev = y;
            let mut best_cost = dp[x - 1][y];
            if y > 0 && dp[x - 1][y - 1] < best_cost {
                best_cost = dp[x - 1][y - 1];
                best_prev = y - 1;
            }
            if y + 1 < rows && dp[x - 1][y + 1] < best_cost {
                best_cost = dp[x - 1][y + 1];
                best_prev = y + 1;
            }
            dp[x][y] = best_cost + cost[x][y] as u64;
            parent[x][y] = best_prev;
        }
    }

    let mut end_y = 0usize;
    let mut end_cost = dp[cols - 1][0];
    for y in 1..rows {
        if dp[cols - 1][y] < end_cost {
            end_cost = dp[cols - 1][y];
            end_y = y;
        }
    }

    let mut seam = vec![0usize; cols];
    let mut y = end_y;
    for x in (0..cols).rev() {
        seam[x] = y;
        if x > 0 {
            y = parent[x][y];
        }
    }
    seam
}

fn rgb_distance_sq(a: Rgba<u8>, b: Rgba<u8>) -> u32 {
    let dr = a[0] as i32 - b[0] as i32;
    let dg = a[1] as i32 - b[1] as i32;
    let db = a[2] as i32 - b[2] as i32;
    (dr * dr + dg * dg + db * db) as u32
}

fn mix_rgba(a: Rgba<u8>, b: Rgba<u8>, t: f32) -> Rgba<u8> {
    let t = t.clamp(0.0, 1.0);
    let mix = |lhs: u8, rhs: u8| -> u8 {
        (lhs as f32 * (1.0 - t) + rhs as f32 * t)
            .round()
            .clamp(0.0, 255.0) as u8
    };
    Rgba([
        mix(a[0], b[0]),
        mix(a[1], b[1]),
        mix(a[2], b[2]),
        mix(a[3], b[3]),
    ])
}

fn blit_rgba(src: &RgbaImage, dst: &mut RgbaImage, offset_x: u32, offset_y: u32) {
    for y in 0..src.height() {
        let dy = offset_y + y;
        if dy >= dst.height() {
            break;
        }
        for x in 0..src.width() {
            let dx = offset_x + x;
            if dx >= dst.width() {
                break;
            }
            dst.put_pixel(dx, dy, *src.get_pixel(x, y));
        }
    }
}

fn draw_bitmap_text(
    image: &mut RgbaImage,
    text: &str,
    x: i32,
    y: i32,
    scale: u32,
    color: Rgba<u8>,
) {
    let scale = scale.clamp(1, 8);
    let mut cursor_x = x;
    let mut cursor_y = y;
    for ch in text.chars() {
        if ch == '\n' {
            cursor_x = x;
            cursor_y += (8 * scale + scale) as i32;
            continue;
        }
        let Some(glyph) = font8x8::BASIC_FONTS.get(ch) else {
            cursor_x += (8 * scale + scale) as i32;
            continue;
        };
        for (row, bits) in glyph.iter().enumerate() {
            for col in 0..8 {
                if (bits >> col) & 1 == 1 {
                    let px = cursor_x + (col as i32 * scale as i32);
                    let py = cursor_y + (row as i32 * scale as i32);
                    fill_rect_blend(image, px, py, scale, scale, color);
                }
            }
        }
        cursor_x += (8 * scale + scale) as i32;
    }
}

fn html_escape(input: &str) -> String {
    input
        .replace('&', "&amp;")
        .replace('<', "&lt;")
        .replace('>', "&gt;")
        .replace('"', "&quot;")
        .replace('\'', "&#39;")
}

fn run_transform(request_id: u64, op: TransformOp, image: Arc<DynamicImage>) -> WorkerResult {
    log::debug!(
        target: "imranview::worker",
        "transform start request_id={} op={:?}",
        request_id,
        op
    );
    let started = Instant::now();
    let transformed = apply_transform(op, image.as_ref());

    match transformed {
        Ok(transformed) => {
            let loaded = payload_from_working_image(Arc::new(transformed));
            log_timing("edit_image", started.elapsed(), EDIT_IMAGE_BUDGET);
            WorkerResult::Transformed { request_id, loaded }
        }
        Err(err) => WorkerResult::Failed {
            request_id: Some(request_id),
            kind: WorkerRequestKind::Edit,
            error: err.to_string(),
        },
    }
}

fn apply_transform(op: TransformOp, image: &DynamicImage) -> Result<DynamicImage> {
    match op {
        TransformOp::RotateLeft => Ok(image.rotate270()),
        TransformOp::RotateRight => Ok(image.rotate90()),
        TransformOp::FlipHorizontal => Ok(image.fliph()),
        TransformOp::FlipVertical => Ok(image.flipv()),
        TransformOp::AddBorder {
            left,
            right,
            top,
            bottom,
            color,
        } => apply_border(image, left, right, top, bottom, color),
        TransformOp::CanvasSize {
            width,
            height,
            anchor,
            fill,
        } => apply_canvas_size(image, width, height, anchor, fill),
        TransformOp::RotateFine {
            angle_degrees,
            interpolation,
            expand_canvas,
            fill,
        } => apply_rotate_fine(image, angle_degrees, interpolation, expand_canvas, fill),
        TransformOp::AddText {
            text,
            x,
            y,
            scale,
            color,
        } => Ok(apply_text(image, &text, x, y, scale, color)),
        TransformOp::DrawShape(params) => Ok(apply_shape(image, params)),
        TransformOp::OverlayImage {
            overlay_path,
            opacity,
            anchor,
        } => apply_overlay_image(image, &overlay_path, opacity, anchor),
        TransformOp::SelectionWorkflow(params) => apply_selection_workflow(image, params),
        TransformOp::ReplaceColor {
            source,
            target,
            tolerance,
            preserve_alpha,
        } => Ok(apply_replace_color(
            image,
            source,
            target,
            tolerance,
            preserve_alpha,
        )),
        TransformOp::AlphaAdjust {
            alpha_percent,
            alpha_from_luma,
            invert_luma,
            region,
        } => Ok(apply_alpha_adjust(
            image,
            alpha_percent,
            alpha_from_luma,
            invert_luma,
            region,
        )),
        TransformOp::AlphaBrush {
            center_x,
            center_y,
            radius,
            strength_percent,
            softness,
            operation,
        } => Ok(apply_alpha_brush(
            image,
            center_x,
            center_y,
            radius,
            strength_percent,
            softness,
            operation,
        )),
        TransformOp::Effects(params) => Ok(apply_effects(image, params)),
        TransformOp::PerspectiveCorrect {
            top_left,
            top_right,
            bottom_right,
            bottom_left,
            output_width,
            output_height,
            interpolation,
            fill,
        } => apply_perspective_correct(
            image,
            top_left,
            top_right,
            bottom_right,
            bottom_left,
            output_width,
            output_height,
            interpolation,
            fill,
        ),
        TransformOp::Resize {
            width,
            height,
            filter,
        } => {
            if width == 0 || height == 0 {
                return Err(anyhow!("resize dimensions must be greater than zero"));
            }
            Ok(image.resize_exact(width, height, filter.to_image_filter()))
        }
        TransformOp::Crop {
            x,
            y,
            width,
            height,
        } => {
            let (source_width, source_height) = image.dimensions();
            if width == 0 || height == 0 {
                return Err(anyhow!("crop dimensions must be greater than zero"));
            }
            if x >= source_width || y >= source_height {
                return Err(anyhow!("crop origin is outside image bounds"));
            }
            let end_x = x.saturating_add(width);
            let end_y = y.saturating_add(height);
            if end_x > source_width || end_y > source_height {
                return Err(anyhow!("crop rectangle exceeds image bounds"));
            }
            Ok(image.crop_imm(x, y, width, height))
        }
        TransformOp::ColorAdjust(params) => Ok(apply_color_adjustments(image, params)),
    }
}

fn apply_text(
    image: &DynamicImage,
    text: &str,
    x: i32,
    y: i32,
    scale: u32,
    color: [u8; 4],
) -> DynamicImage {
    let mut output = image.to_rgba8();
    if text.trim().is_empty() {
        return DynamicImage::ImageRgba8(output);
    }

    let scale = scale.clamp(1, 16);
    let draw_color = Rgba(color);
    let mut cursor_x = x;
    let mut cursor_y = y;
    for ch in text.chars() {
        if ch == '\n' {
            cursor_x = x;
            cursor_y += (8 * scale + scale) as i32;
            continue;
        }
        let Some(glyph) = font8x8::BASIC_FONTS.get(ch) else {
            cursor_x += (8 * scale) as i32;
            continue;
        };

        for (row, bits) in glyph.iter().enumerate() {
            for col in 0..8 {
                if (bits >> col) & 1 == 1 {
                    let px = cursor_x + (col as i32 * scale as i32);
                    let py = cursor_y + (row as i32 * scale as i32);
                    fill_rect_blend(&mut output, px, py, scale, scale, draw_color);
                }
            }
        }

        cursor_x += (8 * scale + scale) as i32;
    }

    DynamicImage::ImageRgba8(output)
}

fn apply_shape(image: &DynamicImage, params: ShapeParams) -> DynamicImage {
    let mut output = image.to_rgba8();
    let thickness = params.thickness.clamp(1, 128);
    let color = Rgba(params.color);
    match params.kind {
        ShapeKind::Line => draw_line(
            &mut output,
            params.start_x,
            params.start_y,
            params.end_x,
            params.end_y,
            thickness,
            color,
        ),
        ShapeKind::Rectangle => draw_rectangle(
            &mut output,
            params.start_x,
            params.start_y,
            params.end_x,
            params.end_y,
            thickness,
            params.filled,
            color,
        ),
        ShapeKind::Ellipse => draw_ellipse(
            &mut output,
            params.start_x,
            params.start_y,
            params.end_x,
            params.end_y,
            thickness,
            params.filled,
            color,
        ),
        ShapeKind::Arrow => draw_arrow(
            &mut output,
            params.start_x,
            params.start_y,
            params.end_x,
            params.end_y,
            thickness,
            color,
        ),
        ShapeKind::RoundedRectangleShadow => draw_rounded_rect_shadow(
            &mut output,
            params.start_x,
            params.start_y,
            params.end_x,
            params.end_y,
            thickness,
            params.filled,
            color,
        ),
        ShapeKind::SpeechBubble => draw_speech_bubble(
            &mut output,
            params.start_x,
            params.start_y,
            params.end_x,
            params.end_y,
            thickness,
            params.filled,
            color,
        ),
    }

    DynamicImage::ImageRgba8(output)
}

fn apply_overlay_image(
    image: &DynamicImage,
    overlay_path: &Path,
    opacity: f32,
    anchor: CanvasAnchor,
) -> Result<DynamicImage> {
    let overlay = image::open(overlay_path)
        .with_context(|| format!("failed to open overlay image {}", overlay_path.display()))?
        .to_rgba8();
    let mut output = image.to_rgba8();
    let (base_w, base_h) = output.dimensions();
    let (ov_w, ov_h) = overlay.dimensions();
    if ov_w == 0 || ov_h == 0 {
        return Err(anyhow!("overlay image is empty"));
    }

    let (factor_x, factor_y) = anchor.factors();
    let dx_max = base_w.saturating_sub(ov_w);
    let dy_max = base_h.saturating_sub(ov_h);
    let offset_x = ((dx_max as f32 * factor_x).round() as u32).min(dx_max);
    let offset_y = ((dy_max as f32 * factor_y).round() as u32).min(dy_max);
    let opacity = opacity.clamp(0.0, 1.0);

    for y in 0..ov_h.min(base_h) {
        for x in 0..ov_w.min(base_w) {
            let target_x = offset_x.saturating_add(x);
            let target_y = offset_y.saturating_add(y);
            if target_x >= base_w || target_y >= base_h {
                continue;
            }
            let mut src = *overlay.get_pixel(x, y);
            src.0[3] = (src.0[3] as f32 * opacity).round().clamp(0.0, 255.0) as u8;
            blend_pixel(output.get_pixel_mut(target_x, target_y), src);
        }
    }
    Ok(DynamicImage::ImageRgba8(output))
}

fn apply_selection_workflow(image: &DynamicImage, params: SelectionParams) -> Result<DynamicImage> {
    let source = image.to_rgba8();
    let (w, h) = source.dimensions();
    if w == 0 || h == 0 {
        return Err(anyhow!("no image pixels to process"));
    }

    match params.workflow {
        SelectionWorkflow::CropRect => {
            if params.width == 0 || params.height == 0 {
                return Err(anyhow!(
                    "selection crop dimensions must be greater than zero"
                ));
            }
            if params.x >= w || params.y >= h {
                return Err(anyhow!("selection origin is outside image bounds"));
            }
            let crop_w = params.width.min(w - params.x);
            let crop_h = params.height.min(h - params.y);
            Ok(DynamicImage::ImageRgba8(
                image::imageops::crop_imm(&source, params.x, params.y, crop_w, crop_h).to_image(),
            ))
        }
        SelectionWorkflow::CutOutsideRect => {
            let mut output = RgbaImage::from_pixel(w, h, Rgba(params.fill));
            if params.width == 0 || params.height == 0 {
                return Ok(DynamicImage::ImageRgba8(output));
            }
            if params.x >= w || params.y >= h {
                return Ok(DynamicImage::ImageRgba8(output));
            }
            let copy_w = params.width.min(w - params.x);
            let copy_h = params.height.min(h - params.y);
            for y in 0..copy_h {
                for x in 0..copy_w {
                    output.put_pixel(
                        params.x + x,
                        params.y + y,
                        *source.get_pixel(params.x + x, params.y + y),
                    );
                }
            }
            Ok(DynamicImage::ImageRgba8(output))
        }
        SelectionWorkflow::CropCircle => {
            if params.radius == 0 {
                return Err(anyhow!("circle radius must be greater than zero"));
            }
            let diameter = params.radius.saturating_mul(2);
            let mut output = RgbaImage::from_pixel(diameter, diameter, Rgba([0, 0, 0, 0]));
            let cx = params.x as i32;
            let cy = params.y as i32;
            let radius = params.radius as i32;
            for oy in 0..diameter {
                for ox in 0..diameter {
                    let dx = ox as i32 - radius;
                    let dy = oy as i32 - radius;
                    if dx * dx + dy * dy <= radius * radius {
                        let sx = cx + dx;
                        let sy = cy + dy;
                        if sx >= 0 && sy >= 0 && sx < w as i32 && sy < h as i32 {
                            output.put_pixel(ox, oy, *source.get_pixel(sx as u32, sy as u32));
                        }
                    }
                }
            }
            Ok(DynamicImage::ImageRgba8(output))
        }
        SelectionWorkflow::CutOutsideCircle => {
            if params.radius == 0 {
                return Err(anyhow!("circle radius must be greater than zero"));
            }
            let mut output = RgbaImage::from_pixel(w, h, Rgba(params.fill));
            let cx = params.x as i32;
            let cy = params.y as i32;
            let radius = params.radius as i32;
            for y in 0..h {
                for x in 0..w {
                    let dx = x as i32 - cx;
                    let dy = y as i32 - cy;
                    if dx * dx + dy * dy <= radius * radius {
                        output.put_pixel(x, y, *source.get_pixel(x, y));
                    }
                }
            }
            Ok(DynamicImage::ImageRgba8(output))
        }
        SelectionWorkflow::CropPolygon => {
            if params.polygon_points.len() < 3 {
                return Err(anyhow!("polygon crop requires at least 3 points"));
            }
            let points: Vec<(i32, i32)> = params
                .polygon_points
                .iter()
                .map(|p| (p[0] as i32, p[1] as i32))
                .collect();
            let (min_x, max_x, min_y, max_y) = polygon_bounds(&points)?;
            if min_x < 0 || min_y < 0 || min_x as u32 >= w || min_y as u32 >= h {
                return Err(anyhow!("polygon origin is outside image bounds"));
            }
            let out_w = (max_x - min_x + 1).max(1) as u32;
            let out_h = (max_y - min_y + 1).max(1) as u32;
            let mut output = RgbaImage::from_pixel(out_w, out_h, Rgba([0, 0, 0, 0]));
            for y in min_y..=max_y {
                for x in min_x..=max_x {
                    if x < 0 || y < 0 || x as u32 >= w || y as u32 >= h {
                        continue;
                    }
                    if point_in_polygon(x, y, &points) {
                        output.put_pixel(
                            (x - min_x) as u32,
                            (y - min_y) as u32,
                            *source.get_pixel(x as u32, y as u32),
                        );
                    }
                }
            }
            Ok(DynamicImage::ImageRgba8(output))
        }
        SelectionWorkflow::CutOutsidePolygon => {
            if params.polygon_points.len() < 3 {
                return Err(anyhow!("polygon cut-outside requires at least 3 points"));
            }
            let points: Vec<(i32, i32)> = params
                .polygon_points
                .iter()
                .map(|p| (p[0] as i32, p[1] as i32))
                .collect();
            let mut output = RgbaImage::from_pixel(w, h, Rgba(params.fill));
            for y in 0..h {
                for x in 0..w {
                    if point_in_polygon(x as i32, y as i32, &points) {
                        output.put_pixel(x, y, *source.get_pixel(x, y));
                    }
                }
            }
            Ok(DynamicImage::ImageRgba8(output))
        }
    }
}

fn polygon_bounds(points: &[(i32, i32)]) -> Result<(i32, i32, i32, i32)> {
    let mut min_x = i32::MAX;
    let mut max_x = i32::MIN;
    let mut min_y = i32::MAX;
    let mut max_y = i32::MIN;
    for &(x, y) in points {
        min_x = min_x.min(x);
        max_x = max_x.max(x);
        min_y = min_y.min(y);
        max_y = max_y.max(y);
    }
    if min_x > max_x || min_y > max_y {
        return Err(anyhow!("invalid polygon bounds"));
    }
    Ok((min_x, max_x, min_y, max_y))
}

fn point_in_polygon(x: i32, y: i32, points: &[(i32, i32)]) -> bool {
    let mut inside = false;
    let mut j = points.len() - 1;
    for i in 0..points.len() {
        let (xi, yi) = points[i];
        let (xj, yj) = points[j];
        if (yi > y) != (yj > y) {
            let x_intersect = ((xj - xi) as f32 * (y - yi) as f32) / (yj - yi) as f32 + xi as f32;
            if (x as f32) < x_intersect {
                inside = !inside;
            }
        }
        j = i;
    }
    inside
}

fn apply_replace_color(
    image: &DynamicImage,
    source: [u8; 4],
    target: [u8; 4],
    tolerance: u8,
    preserve_alpha: bool,
) -> DynamicImage {
    let mut output = image.to_rgba8();
    let src = source;
    let distance_max = tolerance as i32 * tolerance as i32 * 3;
    for px in output.pixels_mut() {
        let dr = px[0] as i32 - src[0] as i32;
        let dg = px[1] as i32 - src[1] as i32;
        let db = px[2] as i32 - src[2] as i32;
        let dist = dr * dr + dg * dg + db * db;
        if dist <= distance_max {
            px[0] = target[0];
            px[1] = target[1];
            px[2] = target[2];
            px[3] = if preserve_alpha { px[3] } else { target[3] };
        }
    }
    DynamicImage::ImageRgba8(output)
}

fn apply_alpha_adjust(
    image: &DynamicImage,
    alpha_percent: f32,
    alpha_from_luma: bool,
    invert_luma: bool,
    region: Option<(u32, u32, u32, u32)>,
) -> DynamicImage {
    let mut output = image.to_rgba8();
    let (w, h) = output.dimensions();
    let bounds = region.map(|(x, y, width, height)| {
        let x0 = x.min(w);
        let y0 = y.min(h);
        let x1 = x0.saturating_add(width).min(w);
        let y1 = y0.saturating_add(height).min(h);
        (x0, y0, x1, y1)
    });
    let factor = (alpha_percent / 100.0).clamp(0.0, 1.0);
    for y in 0..h {
        for x in 0..w {
            if let Some((x0, y0, x1, y1)) = bounds {
                if x < x0 || x >= x1 || y < y0 || y >= y1 {
                    continue;
                }
            }
            let px = output.get_pixel_mut(x, y);
            let mut alpha = px[3] as f32 * factor;
            if alpha_from_luma {
                let luma =
                    0.2126 * px[0] as f32 + 0.7152 * px[1] as f32 + 0.0722 * px[2] as f32;
                alpha = if invert_luma { 255.0 - luma } else { luma };
            }
            px[3] = alpha.round().clamp(0.0, 255.0) as u8;
        }
    }
    DynamicImage::ImageRgba8(output)
}

fn apply_alpha_brush(
    image: &DynamicImage,
    center_x: u32,
    center_y: u32,
    radius: u32,
    strength_percent: f32,
    softness: f32,
    operation: AlphaBrushOp,
) -> DynamicImage {
    let mut output = image.to_rgba8();
    let (w, h) = output.dimensions();
    if w == 0 || h == 0 {
        return DynamicImage::ImageRgba8(output);
    }

    let radius = radius.max(1);
    let radius_f = radius as f32;
    let strength = (strength_percent / 100.0).clamp(0.0, 1.0);
    if strength <= 0.0 {
        return DynamicImage::ImageRgba8(output);
    }
    let softness = softness.clamp(0.0, 1.0);
    let inner_radius = radius_f * (1.0 - softness);

    let min_x = center_x.saturating_sub(radius);
    let min_y = center_y.saturating_sub(radius);
    let max_x = center_x.saturating_add(radius).min(w.saturating_sub(1));
    let max_y = center_y.saturating_add(radius).min(h.saturating_sub(1));

    for y in min_y..=max_y {
        for x in min_x..=max_x {
            let dx = x as f32 - center_x as f32;
            let dy = y as f32 - center_y as f32;
            let distance = (dx * dx + dy * dy).sqrt();
            if distance > radius_f {
                continue;
            }
            let falloff = if distance <= inner_radius || inner_radius >= radius_f {
                1.0
            } else {
                let t = ((distance - inner_radius) / (radius_f - inner_radius)).clamp(0.0, 1.0);
                (1.0 - t).powf(1.4)
            };
            let weight = (strength * falloff).clamp(0.0, 1.0);
            if weight <= 0.0 {
                continue;
            }

            let px = output.get_pixel_mut(x, y);
            let current_alpha = px[3] as f32;
            let next_alpha = match operation {
                AlphaBrushOp::Increase | AlphaBrushOp::SetOpaque => {
                    current_alpha + (255.0 - current_alpha) * weight
                }
                AlphaBrushOp::Decrease | AlphaBrushOp::SetTransparent => {
                    current_alpha * (1.0 - weight)
                }
            };
            px[3] = next_alpha.round().clamp(0.0, 255.0) as u8;
        }
    }

    DynamicImage::ImageRgba8(output)
}

fn apply_effects(image: &DynamicImage, params: EffectsParams) -> DynamicImage {
    let mut current = image.clone();
    if params.blur_sigma > 0.01 {
        current = current.blur(params.blur_sigma.clamp(0.0, 30.0));
    }
    if params.sharpen_sigma > 0.01 {
        current = current.unsharpen(
            params.sharpen_sigma.clamp(0.0, 30.0),
            params.sharpen_threshold.clamp(-255, 255),
        );
    }
    if params.grayscale {
        current = current.grayscale();
    }

    let mut rgba = current.to_rgba8();
    if params.invert {
        for px in rgba.pixels_mut() {
            px[0] = 255 - px[0];
            px[1] = 255 - px[1];
            px[2] = 255 - px[2];
        }
    }

    if params.sepia_strength > 0.001 {
        let strength = params.sepia_strength.clamp(0.0, 1.0);
        for px in rgba.pixels_mut() {
            let r = px[0] as f32;
            let g = px[1] as f32;
            let b = px[2] as f32;
            let sepia_r = (0.393 * r + 0.769 * g + 0.189 * b).clamp(0.0, 255.0);
            let sepia_g = (0.349 * r + 0.686 * g + 0.168 * b).clamp(0.0, 255.0);
            let sepia_b = (0.272 * r + 0.534 * g + 0.131 * b).clamp(0.0, 255.0);
            px[0] = (r * (1.0 - strength) + sepia_r * strength).round() as u8;
            px[1] = (g * (1.0 - strength) + sepia_g * strength).round() as u8;
            px[2] = (b * (1.0 - strength) + sepia_b * strength).round() as u8;
        }
    }

    if params.posterize_levels >= 2 {
        let levels = params.posterize_levels.clamp(2, 64) as f32;
        let step = 255.0 / (levels - 1.0);
        for px in rgba.pixels_mut() {
            px[0] = ((px[0] as f32 / step).round() * step).clamp(0.0, 255.0) as u8;
            px[1] = ((px[1] as f32 / step).round() * step).clamp(0.0, 255.0) as u8;
            px[2] = ((px[2] as f32 / step).round() * step).clamp(0.0, 255.0) as u8;
        }
    }

    if params.vignette_strength > 0.001 {
        apply_vignette_in_place(&mut rgba, params.vignette_strength.clamp(0.0, 1.0));
    }
    if params.emboss_strength > 0.001 {
        apply_emboss_in_place(&mut rgba, params.emboss_strength.clamp(0.0, 1.0));
    }
    if params.edge_enhance_strength > 0.001 {
        apply_edge_enhance_in_place(&mut rgba, params.edge_enhance_strength.clamp(0.0, 1.0));
    }
    if params.stained_glass_strength > 0.001 {
        apply_stained_glass_in_place(&mut rgba, params.stained_glass_strength.clamp(0.0, 1.0));
    }
    if params.tilt_shift_strength > 0.001 {
        apply_tilt_shift_in_place(&mut rgba, params.tilt_shift_strength.clamp(0.0, 1.0));
    }
    if params.oil_paint_strength > 0.001 {
        apply_oil_paint_in_place(&mut rgba, params.oil_paint_strength.clamp(0.0, 1.0));
    }

    DynamicImage::ImageRgba8(rgba)
}

fn apply_vignette_in_place(image: &mut RgbaImage, strength: f32) {
    let (w, h) = image.dimensions();
    if w == 0 || h == 0 {
        return;
    }
    let cx = (w as f32 - 1.0) * 0.5;
    let cy = (h as f32 - 1.0) * 0.5;
    let max_dist = (cx * cx + cy * cy).sqrt().max(1.0);
    let keep = 1.0 - strength * 0.85;
    for y in 0..h {
        for x in 0..w {
            let dx = x as f32 - cx;
            let dy = y as f32 - cy;
            let dist = (dx * dx + dy * dy).sqrt() / max_dist;
            let edge = dist.powf(1.6);
            let factor = (keep + (1.0 - keep) * (1.0 - edge)).clamp(0.0, 1.0);
            let px = image.get_pixel_mut(x, y);
            px[0] = (px[0] as f32 * factor).round().clamp(0.0, 255.0) as u8;
            px[1] = (px[1] as f32 * factor).round().clamp(0.0, 255.0) as u8;
            px[2] = (px[2] as f32 * factor).round().clamp(0.0, 255.0) as u8;
        }
    }
}

fn apply_stained_glass_in_place(image: &mut RgbaImage, strength: f32) {
    let source = image.clone();
    let (w, h) = source.dimensions();
    if w == 0 || h == 0 {
        return;
    }
    let block_size = ((4.0 + strength * 28.0).round() as u32).clamp(2, 48);
    for by in (0..h).step_by(block_size as usize) {
        for bx in (0..w).step_by(block_size as usize) {
            let max_x = (bx + block_size).min(w);
            let max_y = (by + block_size).min(h);
            let mut sum = [0u64; 4];
            let mut count = 0u64;
            for y in by..max_y {
                for x in bx..max_x {
                    let px = source.get_pixel(x, y);
                    sum[0] += px[0] as u64;
                    sum[1] += px[1] as u64;
                    sum[2] += px[2] as u64;
                    sum[3] += px[3] as u64;
                    count += 1;
                }
            }
            if count == 0 {
                continue;
            }
            let avg = [
                (sum[0] / count) as u8,
                (sum[1] / count) as u8,
                (sum[2] / count) as u8,
                (sum[3] / count) as u8,
            ];
            for y in by..max_y {
                for x in bx..max_x {
                    let edge = x == bx || y == by || x + 1 == max_x || y + 1 == max_y;
                    if edge {
                        image.put_pixel(
                            x,
                            y,
                            Rgba([
                                ((avg[0] as f32 * 0.35).round() as u8),
                                ((avg[1] as f32 * 0.35).round() as u8),
                                ((avg[2] as f32 * 0.35).round() as u8),
                                avg[3],
                            ]),
                        );
                    } else {
                        image.put_pixel(x, y, Rgba(avg));
                    }
                }
            }
        }
    }
}

fn apply_tilt_shift_in_place(image: &mut RgbaImage, strength: f32) {
    let source = image.clone();
    let blurred = DynamicImage::ImageRgba8(source.clone())
        .blur((2.5 + strength * 10.0).clamp(0.0, 20.0))
        .to_rgba8();
    let (w, h) = source.dimensions();
    if w == 0 || h == 0 {
        return;
    }
    let center_y = (h as f32 - 1.0) * 0.5;
    let focus_half_band = (h as f32 * (0.09 + (1.0 - strength) * 0.20)).max(6.0);
    let transition = (h as f32 * (0.10 + strength * 0.22)).max(8.0);

    for y in 0..h {
        let dist = (y as f32 - center_y).abs();
        let blur_mix = if dist <= focus_half_band {
            0.0
        } else {
            ((dist - focus_half_band) / transition).clamp(0.0, 1.0)
        };
        for x in 0..w {
            let src = source.get_pixel(x, y);
            let blur = blurred.get_pixel(x, y);
            image.put_pixel(x, y, mix_rgba(*src, *blur, blur_mix));
        }
    }
}

fn apply_emboss_in_place(image: &mut RgbaImage, strength: f32) {
    let source = image.clone();
    let (w, h) = source.dimensions();
    if w < 3 || h < 3 {
        return;
    }
    let kernel = [[-2.0f32, -1.0, 0.0], [-1.0, 1.0, 1.0], [0.0, 1.0, 2.0]];
    for y in 1..(h - 1) {
        for x in 1..(w - 1) {
            let mut channel = [0.0f32; 3];
            for ky in 0..3 {
                for kx in 0..3 {
                    let weight = kernel[ky as usize][kx as usize];
                    let px = source.get_pixel(x + kx - 1, y + ky - 1);
                    channel[0] += px[0] as f32 * weight;
                    channel[1] += px[1] as f32 * weight;
                    channel[2] += px[2] as f32 * weight;
                }
            }
            let orig = source.get_pixel(x, y);
            let embossed = [
                (channel[0] + 128.0).clamp(0.0, 255.0) as u8,
                (channel[1] + 128.0).clamp(0.0, 255.0) as u8,
                (channel[2] + 128.0).clamp(0.0, 255.0) as u8,
                orig[3],
            ];
            image.put_pixel(
                x,
                y,
                mix_rgba(*orig, Rgba(embossed), (strength * 0.95).clamp(0.0, 1.0)),
            );
        }
    }
}

fn apply_edge_enhance_in_place(image: &mut RgbaImage, strength: f32) {
    let source = image.clone();
    let (w, h) = source.dimensions();
    if w < 3 || h < 3 {
        return;
    }
    for y in 1..(h - 1) {
        for x in 1..(w - 1) {
            let center = source.get_pixel(x, y);
            let mut neighbor_sum = [0i32; 3];
            let mut neighbor_count = 0i32;
            for ny in (y - 1)..=(y + 1) {
                for nx in (x - 1)..=(x + 1) {
                    if nx == x && ny == y {
                        continue;
                    }
                    let px = source.get_pixel(nx, ny);
                    neighbor_sum[0] += px[0] as i32;
                    neighbor_sum[1] += px[1] as i32;
                    neighbor_sum[2] += px[2] as i32;
                    neighbor_count += 1;
                }
            }
            let mut out = [0u8; 4];
            for channel in 0..3 {
                let avg = neighbor_sum[channel] as f32 / neighbor_count as f32;
                let edge = (center[channel] as f32 - avg).abs();
                let boosted = center[channel] as f32 + edge * (strength * 1.6);
                out[channel] = boosted.clamp(0.0, 255.0) as u8;
            }
            out[3] = center[3];
            image.put_pixel(x, y, Rgba(out));
        }
    }
}

fn apply_oil_paint_in_place(image: &mut RgbaImage, strength: f32) {
    let source = image.clone();
    let (w, h) = source.dimensions();
    if w == 0 || h == 0 {
        return;
    }
    let radius = ((1.0 + strength * 6.0).round() as i32).clamp(1, 8);
    for y in 0..h {
        for x in 0..w {
            let mut sum = [0u32; 4];
            let mut count = 0u32;
            let sample_points = [
                (x as i32, y as i32),
                (x as i32 - radius, y as i32),
                (x as i32 + radius, y as i32),
                (x as i32, y as i32 - radius),
                (x as i32, y as i32 + radius),
                (x as i32 - radius, y as i32 - radius),
                (x as i32 + radius, y as i32 + radius),
            ];
            for (sx, sy) in sample_points {
                if sx < 0 || sy < 0 || sx >= w as i32 || sy >= h as i32 {
                    continue;
                }
                let px = source.get_pixel(sx as u32, sy as u32);
                sum[0] += px[0] as u32;
                sum[1] += px[1] as u32;
                sum[2] += px[2] as u32;
                sum[3] += px[3] as u32;
                count += 1;
            }
            if count == 0 {
                continue;
            }
            let avg = Rgba([
                (sum[0] / count) as u8,
                (sum[1] / count) as u8,
                (sum[2] / count) as u8,
                (sum[3] / count) as u8,
            ]);
            let original = source.get_pixel(x, y);
            image.put_pixel(x, y, mix_rgba(*original, avg, strength.clamp(0.0, 1.0)));
        }
    }
}

#[allow(clippy::too_many_arguments)]
fn apply_perspective_correct(
    image: &DynamicImage,
    top_left: [f32; 2],
    top_right: [f32; 2],
    bottom_right: [f32; 2],
    bottom_left: [f32; 2],
    output_width: u32,
    output_height: u32,
    interpolation: RotationInterpolation,
    fill: [u8; 4],
) -> Result<DynamicImage> {
    if output_width == 0 || output_height == 0 {
        return Err(anyhow!(
            "perspective output dimensions must be greater than zero"
        ));
    }
    let source = image.to_rgba8();
    let mut output = RgbaImage::from_pixel(output_width, output_height, Rgba(fill));
    let denom_x = output_width.saturating_sub(1).max(1) as f32;
    let denom_y = output_height.saturating_sub(1).max(1) as f32;

    for y in 0..output_height {
        let v = y as f32 / denom_y;
        for x in 0..output_width {
            let u = x as f32 / denom_x;
            let source_x = bilinear_quad_value(
                top_left[0],
                top_right[0],
                bottom_right[0],
                bottom_left[0],
                u,
                v,
            );
            let source_y = bilinear_quad_value(
                top_left[1],
                top_right[1],
                bottom_right[1],
                bottom_left[1],
                u,
                v,
            );
            let sampled = match interpolation {
                RotationInterpolation::Nearest => {
                    sample_nearest(&source, source_x, source_y, Rgba(fill))
                }
                RotationInterpolation::Bilinear => {
                    sample_bilinear(&source, source_x, source_y, Rgba(fill))
                }
            };
            output.put_pixel(x, y, sampled);
        }
    }

    Ok(DynamicImage::ImageRgba8(output))
}

fn bilinear_quad_value(tl: f32, tr: f32, br: f32, bl: f32, u: f32, v: f32) -> f32 {
    tl * (1.0 - u) * (1.0 - v) + tr * u * (1.0 - v) + br * u * v + bl * (1.0 - u) * v
}

fn draw_line(
    image: &mut RgbaImage,
    x0: i32,
    y0: i32,
    x1: i32,
    y1: i32,
    thickness: u32,
    color: Rgba<u8>,
) {
    let dx = x1 - x0;
    let dy = y1 - y0;
    let steps = dx.abs().max(dy.abs()).max(1);
    for step in 0..=steps {
        let t = step as f32 / steps as f32;
        let x = x0 as f32 + dx as f32 * t;
        let y = y0 as f32 + dy as f32 * t;
        draw_disc(
            image,
            x.round() as i32,
            y.round() as i32,
            (thickness as i32 / 2).max(1),
            color,
        );
    }
}

fn draw_rectangle(
    image: &mut RgbaImage,
    start_x: i32,
    start_y: i32,
    end_x: i32,
    end_y: i32,
    thickness: u32,
    filled: bool,
    color: Rgba<u8>,
) {
    let min_x = start_x.min(end_x);
    let max_x = start_x.max(end_x);
    let min_y = start_y.min(end_y);
    let max_y = start_y.max(end_y);
    if filled {
        fill_rect_blend(
            image,
            min_x,
            min_y,
            (max_x - min_x + 1).max(0) as u32,
            (max_y - min_y + 1).max(0) as u32,
            color,
        );
    } else {
        let t = thickness.max(1) as i32;
        fill_rect_blend(
            image,
            min_x,
            min_y,
            (max_x - min_x + 1).max(0) as u32,
            t as u32,
            color,
        );
        fill_rect_blend(
            image,
            min_x,
            max_y - t + 1,
            (max_x - min_x + 1).max(0) as u32,
            t as u32,
            color,
        );
        fill_rect_blend(
            image,
            min_x,
            min_y,
            t as u32,
            (max_y - min_y + 1).max(0) as u32,
            color,
        );
        fill_rect_blend(
            image,
            max_x - t + 1,
            min_y,
            t as u32,
            (max_y - min_y + 1).max(0) as u32,
            color,
        );
    }
}

fn draw_ellipse(
    image: &mut RgbaImage,
    start_x: i32,
    start_y: i32,
    end_x: i32,
    end_y: i32,
    thickness: u32,
    filled: bool,
    color: Rgba<u8>,
) {
    let min_x = start_x.min(end_x);
    let max_x = start_x.max(end_x);
    let min_y = start_y.min(end_y);
    let max_y = start_y.max(end_y);
    let cx = (min_x + max_x) as f32 * 0.5;
    let cy = (min_y + max_y) as f32 * 0.5;
    let rx = ((max_x - min_x).max(1) as f32) * 0.5;
    let ry = ((max_y - min_y).max(1) as f32) * 0.5;
    let stroke = (thickness as f32 / 2.0).max(1.0);
    for y in min_y..=max_y {
        for x in min_x..=max_x {
            let nx = (x as f32 - cx) / rx;
            let ny = (y as f32 - cy) / ry;
            let d = nx * nx + ny * ny;
            let inside = d <= 1.0;
            let on_edge = (d - 1.0).abs() <= (stroke / rx.max(ry)).max(0.02);
            if (filled && inside) || (!filled && on_edge) {
                blend_pixel_safe(image, x, y, color);
            }
        }
    }
}

fn draw_arrow(
    image: &mut RgbaImage,
    start_x: i32,
    start_y: i32,
    end_x: i32,
    end_y: i32,
    thickness: u32,
    color: Rgba<u8>,
) {
    draw_line(image, start_x, start_y, end_x, end_y, thickness, color);
    let dx = (end_x - start_x) as f32;
    let dy = (end_y - start_y) as f32;
    let len = (dx * dx + dy * dy).sqrt().max(1.0);
    let ux = dx / len;
    let uy = dy / len;
    let head_len = (thickness.max(2) as f32 * 4.0).max(8.0);
    let side = (thickness.max(2) as f32 * 2.0).max(5.0);
    let bx = end_x as f32 - ux * head_len;
    let by = end_y as f32 - uy * head_len;
    let px = -uy;
    let py = ux;
    let left_x = (bx + px * side).round() as i32;
    let left_y = (by + py * side).round() as i32;
    let right_x = (bx - px * side).round() as i32;
    let right_y = (by - py * side).round() as i32;
    draw_line(image, end_x, end_y, left_x, left_y, thickness, color);
    draw_line(image, end_x, end_y, right_x, right_y, thickness, color);
}

fn draw_rounded_rect_shadow(
    image: &mut RgbaImage,
    start_x: i32,
    start_y: i32,
    end_x: i32,
    end_y: i32,
    thickness: u32,
    filled: bool,
    color: Rgba<u8>,
) {
    let shadow = Rgba([0, 0, 0, 110]);
    let offset = (thickness.max(2) / 2) as i32 + 2;
    draw_rounded_rect(
        image,
        start_x + offset,
        start_y + offset,
        end_x + offset,
        end_y + offset,
        thickness.max(2),
        true,
        shadow,
    );
    draw_rounded_rect(
        image,
        start_x,
        start_y,
        end_x,
        end_y,
        thickness,
        filled,
        color,
    );
}

fn draw_rounded_rect(
    image: &mut RgbaImage,
    start_x: i32,
    start_y: i32,
    end_x: i32,
    end_y: i32,
    thickness: u32,
    filled: bool,
    color: Rgba<u8>,
) {
    let min_x = start_x.min(end_x);
    let max_x = start_x.max(end_x);
    let min_y = start_y.min(end_y);
    let max_y = start_y.max(end_y);
    let width = (max_x - min_x + 1).max(1);
    let height = (max_y - min_y + 1).max(1);
    let radius = ((width.min(height) as f32) * 0.16).round() as i32;
    let radius = radius.clamp(2, 48);
    if filled {
        for y in min_y..=max_y {
            for x in min_x..=max_x {
                if rounded_rect_contains(x, y, min_x, min_y, max_x, max_y, radius) {
                    blend_pixel_safe(image, x, y, color);
                }
            }
        }
    } else {
        let t = thickness.max(1) as i32;
        for y in min_y..=max_y {
            for x in min_x..=max_x {
                let inside = rounded_rect_contains(x, y, min_x, min_y, max_x, max_y, radius);
                if !inside {
                    continue;
                }
                let inner = rounded_rect_contains(
                    x,
                    y,
                    min_x + t,
                    min_y + t,
                    max_x - t,
                    max_y - t,
                    (radius - t).max(0),
                );
                if !inner {
                    blend_pixel_safe(image, x, y, color);
                }
            }
        }
    }
}

fn rounded_rect_contains(
    x: i32,
    y: i32,
    min_x: i32,
    min_y: i32,
    max_x: i32,
    max_y: i32,
    radius: i32,
) -> bool {
    if x < min_x || x > max_x || y < min_y || y > max_y {
        return false;
    }
    if radius <= 0 {
        return true;
    }
    let corner_x = if x < min_x + radius {
        min_x + radius
    } else if x > max_x - radius {
        max_x - radius
    } else {
        x
    };
    let corner_y = if y < min_y + radius {
        min_y + radius
    } else if y > max_y - radius {
        max_y - radius
    } else {
        y
    };
    let dx = x - corner_x;
    let dy = y - corner_y;
    dx * dx + dy * dy <= radius * radius
}

fn draw_speech_bubble(
    image: &mut RgbaImage,
    start_x: i32,
    start_y: i32,
    end_x: i32,
    end_y: i32,
    thickness: u32,
    filled: bool,
    color: Rgba<u8>,
) {
    draw_rounded_rect(
        image,
        start_x,
        start_y,
        end_x,
        end_y,
        thickness,
        filled,
        color,
    );
    let min_x = start_x.min(end_x);
    let max_x = start_x.max(end_x);
    let max_y = start_y.max(end_y);
    let tail_w = ((max_x - min_x).abs() as f32 * 0.16).round() as i32;
    let tail_w = tail_w.clamp(10, 72);
    let tail_h = (tail_w as f32 * 0.8).round() as i32;
    let tip_x = min_x + ((max_x - min_x) as f32 * 0.25).round() as i32;
    let tip_y = max_y + tail_h;
    let left_x = tip_x - tail_w / 2;
    let left_y = max_y - 1;
    let right_x = tip_x + tail_w / 2;
    let right_y = max_y - 1;
    draw_line(image, left_x, left_y, tip_x, tip_y, thickness, color);
    draw_line(image, tip_x, tip_y, right_x, right_y, thickness, color);
    if filled {
        for y in max_y..=tip_y {
            let t = (y - max_y) as f32 / (tail_h.max(1) as f32);
            let row_left = (left_x as f32 + (tip_x - left_x) as f32 * t).round() as i32;
            let row_right = (right_x as f32 + (tip_x - right_x) as f32 * t).round() as i32;
            for x in row_left.min(row_right)..=row_left.max(row_right) {
                blend_pixel_safe(image, x, y, color);
            }
        }
    }
}

fn draw_disc(image: &mut RgbaImage, cx: i32, cy: i32, radius: i32, color: Rgba<u8>) {
    let radius = radius.max(1);
    for y in -radius..=radius {
        for x in -radius..=radius {
            if x * x + y * y <= radius * radius {
                blend_pixel_safe(image, cx + x, cy + y, color);
            }
        }
    }
}

fn fill_rect_blend(
    image: &mut RgbaImage,
    x: i32,
    y: i32,
    width: u32,
    height: u32,
    color: Rgba<u8>,
) {
    if width == 0 || height == 0 {
        return;
    }
    for row in 0..height {
        for col in 0..width {
            blend_pixel_safe(image, x + col as i32, y + row as i32, color);
        }
    }
}

fn blend_pixel_safe(image: &mut RgbaImage, x: i32, y: i32, src: Rgba<u8>) {
    if x < 0 || y < 0 || x >= image.width() as i32 || y >= image.height() as i32 {
        return;
    }
    let dst = image.get_pixel_mut(x as u32, y as u32);
    blend_pixel(dst, src);
}

fn blend_pixel(dst: &mut Rgba<u8>, src: Rgba<u8>) {
    let src_a = src[3] as f32 / 255.0;
    if src_a <= 0.0 {
        return;
    }
    let dst_a = dst[3] as f32 / 255.0;
    let out_a = src_a + dst_a * (1.0 - src_a);
    if out_a <= 0.0 {
        return;
    }
    for channel in 0..3 {
        let src_v = src[channel] as f32 / 255.0;
        let dst_v = dst[channel] as f32 / 255.0;
        let out_v = (src_v * src_a + dst_v * dst_a * (1.0 - src_a)) / out_a;
        dst[channel] = (out_v * 255.0).round().clamp(0.0, 255.0) as u8;
    }
    dst[3] = (out_a * 255.0).round().clamp(0.0, 255.0) as u8;
}

fn apply_border(
    image: &DynamicImage,
    left: u32,
    right: u32,
    top: u32,
    bottom: u32,
    color: [u8; 4],
) -> Result<DynamicImage> {
    if left == 0 && right == 0 && top == 0 && bottom == 0 {
        return Ok(image.clone());
    }

    let source = image.to_rgba8();
    let (source_width, source_height) = source.dimensions();
    let width = source_width
        .checked_add(left)
        .and_then(|v| v.checked_add(right))
        .context("border width overflows image dimensions")?;
    let height = source_height
        .checked_add(top)
        .and_then(|v| v.checked_add(bottom))
        .context("border height overflows image dimensions")?;

    let mut output = RgbaImage::from_pixel(width, height, Rgba(color));
    for y in 0..source_height {
        for x in 0..source_width {
            output.put_pixel(x + left, y + top, *source.get_pixel(x, y));
        }
    }
    Ok(DynamicImage::ImageRgba8(output))
}

fn apply_canvas_size(
    image: &DynamicImage,
    width: u32,
    height: u32,
    anchor: CanvasAnchor,
    fill: [u8; 4],
) -> Result<DynamicImage> {
    if width == 0 || height == 0 {
        return Err(anyhow!("canvas dimensions must be greater than zero"));
    }

    let source = image.to_rgba8();
    let (source_width, source_height) = source.dimensions();
    if width == source_width && height == source_height {
        return Ok(DynamicImage::ImageRgba8(source));
    }

    let (factor_x, factor_y) = anchor.factors();
    let (src_x, dst_x, copy_width) = axis_mapping(source_width, width, factor_x);
    let (src_y, dst_y, copy_height) = axis_mapping(source_height, height, factor_y);

    let mut output = RgbaImage::from_pixel(width, height, Rgba(fill));
    for y in 0..copy_height {
        for x in 0..copy_width {
            output.put_pixel(
                dst_x + x,
                dst_y + y,
                *source.get_pixel(src_x + x, src_y + y),
            );
        }
    }
    Ok(DynamicImage::ImageRgba8(output))
}

fn axis_mapping(source: u32, target: u32, factor: f32) -> (u32, u32, u32) {
    let copy_len = source.min(target);
    if target >= source {
        let pad = target - source;
        let dst = ((pad as f32 * factor).round() as u32).min(pad);
        (0, dst, copy_len)
    } else {
        let trim = source - target;
        let src = ((trim as f32 * factor).round() as u32).min(trim);
        (src, 0, copy_len)
    }
}

fn apply_rotate_fine(
    image: &DynamicImage,
    angle_degrees: f32,
    interpolation: RotationInterpolation,
    expand_canvas: bool,
    fill: [u8; 4],
) -> Result<DynamicImage> {
    if !angle_degrees.is_finite() {
        return Err(anyhow!("rotation angle must be finite"));
    }

    let source = image.to_rgba8();
    let (source_width, source_height) = source.dimensions();
    if source_width == 0 || source_height == 0 {
        return Err(anyhow!("cannot rotate empty image"));
    }

    if angle_degrees.abs() < f32::EPSILON {
        return Ok(DynamicImage::ImageRgba8(source));
    }

    let radians = angle_degrees.to_radians();
    let (sin, cos) = radians.sin_cos();
    let (target_width, target_height) = if expand_canvas {
        let abs_cos = cos.abs();
        let abs_sin = sin.abs();
        (
            ((source_width as f32 * abs_cos + source_height as f32 * abs_sin)
                .ceil()
                .max(1.0)) as u32,
            ((source_width as f32 * abs_sin + source_height as f32 * abs_cos)
                .ceil()
                .max(1.0)) as u32,
        )
    } else {
        (source_width, source_height)
    };

    let source_cx = (source_width as f32 - 1.0) * 0.5;
    let source_cy = (source_height as f32 - 1.0) * 0.5;
    let target_cx = (target_width as f32 - 1.0) * 0.5;
    let target_cy = (target_height as f32 - 1.0) * 0.5;

    let mut output = RgbaImage::from_pixel(target_width, target_height, Rgba(fill));
    for y in 0..target_height {
        let dy = y as f32 - target_cy;
        for x in 0..target_width {
            let dx = x as f32 - target_cx;
            let source_x = cos * dx + sin * dy + source_cx;
            let source_y = -sin * dx + cos * dy + source_cy;

            let sampled = match interpolation {
                RotationInterpolation::Nearest => {
                    sample_nearest(&source, source_x, source_y, Rgba(fill))
                }
                RotationInterpolation::Bilinear => {
                    sample_bilinear(&source, source_x, source_y, Rgba(fill))
                }
            };
            output.put_pixel(x, y, sampled);
        }
    }

    Ok(DynamicImage::ImageRgba8(output))
}

fn sample_nearest(
    source: &RgbaImage,
    source_x: f32,
    source_y: f32,
    fallback: Rgba<u8>,
) -> Rgba<u8> {
    let x = source_x.round() as i32;
    let y = source_y.round() as i32;
    if x < 0 || y < 0 || x >= source.width() as i32 || y >= source.height() as i32 {
        return fallback;
    }
    *source.get_pixel(x as u32, y as u32)
}

fn sample_bilinear(
    source: &RgbaImage,
    source_x: f32,
    source_y: f32,
    fallback: Rgba<u8>,
) -> Rgba<u8> {
    if source_x < 0.0
        || source_y < 0.0
        || source_x > (source.width() - 1) as f32
        || source_y > (source.height() - 1) as f32
    {
        return fallback;
    }

    let x0 = source_x.floor() as u32;
    let y0 = source_y.floor() as u32;
    let x1 = (x0 + 1).min(source.width() - 1);
    let y1 = (y0 + 1).min(source.height() - 1);

    let tx = source_x - x0 as f32;
    let ty = source_y - y0 as f32;
    let p00 = source.get_pixel(x0, y0).0;
    let p10 = source.get_pixel(x1, y0).0;
    let p01 = source.get_pixel(x0, y1).0;
    let p11 = source.get_pixel(x1, y1).0;

    let mut output = [0u8; 4];
    for channel in 0..4 {
        let top = p00[channel] as f32 * (1.0 - tx) + p10[channel] as f32 * tx;
        let bottom = p01[channel] as f32 * (1.0 - tx) + p11[channel] as f32 * tx;
        let value = top * (1.0 - ty) + bottom * ty;
        output[channel] = value.round().clamp(0.0, 255.0) as u8;
    }

    Rgba(output)
}

fn apply_color_adjustments(image: &DynamicImage, params: ColorAdjustParams) -> DynamicImage {
    let mut rgba = image.to_rgba8();
    let brightness = params.brightness.clamp(-255, 255) as f32 / 255.0;
    let contrast = 1.0 + (params.contrast.clamp(-100.0, 100.0) / 100.0);
    let gamma = params.gamma.clamp(0.1, 5.0);
    let saturation = params.saturation.clamp(0.0, 3.0);

    for pixel in rgba.pixels_mut() {
        let alpha = pixel[3];
        let mut r = pixel[0] as f32 / 255.0;
        let mut g = pixel[1] as f32 / 255.0;
        let mut b = pixel[2] as f32 / 255.0;

        r = ((r + brightness - 0.5) * contrast + 0.5).clamp(0.0, 1.0);
        g = ((g + brightness - 0.5) * contrast + 0.5).clamp(0.0, 1.0);
        b = ((b + brightness - 0.5) * contrast + 0.5).clamp(0.0, 1.0);

        let gray = (0.2126 * r + 0.7152 * g + 0.0722 * b).clamp(0.0, 1.0);
        r = (gray + (r - gray) * saturation).clamp(0.0, 1.0);
        g = (gray + (g - gray) * saturation).clamp(0.0, 1.0);
        b = (gray + (b - gray) * saturation).clamp(0.0, 1.0);

        if params.grayscale {
            r = gray;
            g = gray;
            b = gray;
        }

        r = r.powf(1.0 / gamma).clamp(0.0, 1.0);
        g = g.powf(1.0 / gamma).clamp(0.0, 1.0);
        b = b.powf(1.0 / gamma).clamp(0.0, 1.0);

        pixel[0] = (r * 255.0).round() as u8;
        pixel[1] = (g * 255.0).round() as u8;
        pixel[2] = (b * 255.0).round() as u8;
        pixel[3] = alpha;
    }

    DynamicImage::ImageRgba8(rgba)
}

fn run_batch_convert(request_id: u64, options: BatchConvertOptions) -> WorkerResult {
    log::debug!(
        target: "imranview::worker",
        "batch convert start request_id={} input={} output={} format={:?}",
        request_id,
        options.input_dir.display(),
        options.output_dir.display(),
        options.output_format
    );

    let result = (|| -> Result<WorkerResult> {
        let (processed, failed, output_dir) = execute_batch_convert(&options)?;

        Ok(WorkerResult::BatchCompleted {
            request_id,
            processed,
            failed,
            output_dir,
        })
    })();

    result.unwrap_or_else(|err| WorkerResult::Failed {
        request_id: Some(request_id),
        kind: WorkerRequestKind::Batch,
        error: err.to_string(),
    })
}

#[derive(Debug, Deserialize)]
struct BatchScriptFile {
    jobs: Vec<BatchConvertOptions>,
    #[serde(default = "default_continue_on_error")]
    continue_on_error: bool,
}

const fn default_continue_on_error() -> bool {
    true
}

fn run_batch_script(request_id: u64, script_path: PathBuf) -> WorkerResult {
    log::debug!(
        target: "imranview::worker",
        "batch script start request_id={} script={}",
        request_id,
        script_path.display()
    );
    let result = (|| -> Result<WorkerResult> {
        let json = fs::read_to_string(&script_path)
            .with_context(|| format!("failed to read {}", script_path.display()))?;
        let script: BatchScriptFile = serde_json::from_str(&json)
            .with_context(|| format!("invalid JSON in {}", script_path.display()))?;
        if script.jobs.is_empty() {
            return Err(anyhow!("batch script has no jobs"));
        }

        let mut processed_total = 0usize;
        let mut failed_total = 0usize;
        let mut last_output_dir = PathBuf::from(".");
        let mut hard_failures = 0usize;

        for (job_index, job) in script.jobs.iter().enumerate() {
            match execute_batch_convert(job) {
                Ok((processed, failed, output_dir)) => {
                    processed_total = processed_total.saturating_add(processed);
                    failed_total = failed_total.saturating_add(failed);
                    last_output_dir = output_dir;
                }
                Err(err) => {
                    hard_failures = hard_failures.saturating_add(1);
                    if !script.continue_on_error {
                        return Err(anyhow!("script job {} failed: {err:#}", job_index + 1));
                    }
                    log::warn!(
                        target: "imranview::worker",
                        "batch script job {} failed and was skipped: {err:#}",
                        job_index + 1
                    );
                }
            }
        }

        failed_total = failed_total.saturating_add(hard_failures);
        Ok(WorkerResult::BatchCompleted {
            request_id,
            processed: processed_total,
            failed: failed_total,
            output_dir: last_output_dir,
        })
    })();

    result.unwrap_or_else(|err| WorkerResult::Failed {
        request_id: Some(request_id),
        kind: WorkerRequestKind::Batch,
        error: err.to_string(),
    })
}

fn execute_batch_convert(options: &BatchConvertOptions) -> Result<(usize, usize, PathBuf)> {
    fs::create_dir_all(&options.output_dir)
        .with_context(|| format!("failed to create {}", options.output_dir.display()))?;

    let files = collect_images_in_directory(&options.input_dir)?;
    let mut processed = 0usize;
    let mut failed = 0usize;
    let save_format = options
        .output_format
        .to_save_format(options.jpeg_quality.clamp(1, 100));
    let extension = save_format.extension();

    for (offset, path) in files.iter().enumerate() {
        let stem = path
            .file_stem()
            .map(|stem| stem.to_string_lossy().into_owned())
            .unwrap_or_else(|| "image".to_owned());
        let index = options.start_index.saturating_add(offset as u32);
        let name = if options.rename_prefix.trim().is_empty() {
            format!("{stem}.{extension}")
        } else {
            format!("{}{:05}.{extension}", options.rename_prefix.trim(), index)
        };
        let output_path = options.output_dir.join(name);

        let one = (|| -> Result<()> {
            let image = load_working_image(path)?;
            save_image_with_format(&output_path, &image, save_format)?;
            Ok(())
        })();

        if one.is_ok() {
            processed += 1;
        } else {
            failed += 1;
        }
    }

    Ok((processed, failed, options.output_dir.clone()))
}

fn run_file_operation(request_id: u64, operation: FileOperation) -> WorkerResult {
    let operation_for_result = operation.clone();
    let result = (|| -> Result<()> {
        match operation {
            FileOperation::Rename { from, to } => {
                if let Some(parent) = to.parent() {
                    fs::create_dir_all(parent)
                        .with_context(|| format!("failed to create {}", parent.display()))?;
                }
                fs::rename(&from, &to)
                    .with_context(|| format!("failed to rename {}", from.display()))?;
            }
            FileOperation::Delete { path } => {
                fs::remove_file(&path)
                    .with_context(|| format!("failed to delete {}", path.display()))?;
            }
            FileOperation::Copy { from, to } => {
                if let Some(parent) = to.parent() {
                    fs::create_dir_all(parent)
                        .with_context(|| format!("failed to create {}", parent.display()))?;
                }
                fs::copy(&from, &to)
                    .with_context(|| format!("failed to copy {}", from.display()))?;
            }
            FileOperation::Move { from, to } => {
                if let Some(parent) = to.parent() {
                    fs::create_dir_all(parent)
                        .with_context(|| format!("failed to create {}", parent.display()))?;
                }
                match fs::rename(&from, &to) {
                    Ok(()) => {}
                    Err(_) => {
                        fs::copy(&from, &to)
                            .with_context(|| format!("failed to move {}", from.display()))?;
                        fs::remove_file(&from).with_context(|| {
                            format!("failed to remove source {}", from.display())
                        })?;
                    }
                }
            }
        }
        Ok(())
    })();

    match result {
        Ok(()) => WorkerResult::FileOperationCompleted {
            request_id,
            operation: operation_for_result,
        },
        Err(err) => WorkerResult::Failed {
            request_id: Some(request_id),
            kind: WorkerRequestKind::File,
            error: err.to_string(),
        },
    }
}

fn run_thumbnail(path: PathBuf) -> WorkerResult {
    log::debug!(
        target: "imranview::thumb",
        "thumbnail decode start path={}",
        path.display()
    );
    match load_thumbnail_payload(&path) {
        Ok(payload) => WorkerResult::ThumbnailDecoded { path, payload },
        Err(err) => WorkerResult::Failed {
            request_id: None,
            kind: WorkerRequestKind::Thumbnail,
            error: err.to_string(),
        },
    }
}

fn estimate_payload_bytes(payload: &LoadedImagePayload) -> usize {
    let preview_bytes = payload.preview_rgba.len();
    let working_bytes = (payload.original_width as usize)
        .saturating_mul(payload.original_height as usize)
        .saturating_mul(4);
    preview_bytes.saturating_add(working_bytes)
}

#[cfg(test)]
mod tests {
    use image::{DynamicImage, GenericImageView, Rgba, RgbaImage};

    use super::{
        AlphaBrushOp, CanvasAnchor, ColorAdjustParams, EffectsParams, PanoramaDirection,
        ResizeFilter, RotationInterpolation, ShapeKind, ShapeParams, TransformOp, apply_transform,
        stitch_images,
    };

    fn test_image(width: u32, height: u32) -> DynamicImage {
        let image = RgbaImage::from_pixel(width, height, Rgba([120, 80, 30, 255]));
        DynamicImage::ImageRgba8(image)
    }

    #[test]
    fn resize_transform_changes_dimensions() {
        let source = test_image(32, 20);
        let resized = apply_transform(
            TransformOp::Resize {
                width: 10,
                height: 8,
                filter: ResizeFilter::Triangle,
            },
            &source,
        )
        .expect("resize should succeed");
        assert_eq!(resized.dimensions(), (10, 8));
    }

    #[test]
    fn crop_transform_changes_dimensions() {
        let source = test_image(64, 40);
        let cropped = apply_transform(
            TransformOp::Crop {
                x: 4,
                y: 6,
                width: 20,
                height: 10,
            },
            &source,
        )
        .expect("crop should succeed");
        assert_eq!(cropped.dimensions(), (20, 10));
    }

    #[test]
    fn color_adjust_transform_preserves_dimensions() {
        let source = test_image(16, 12);
        let adjusted = apply_transform(
            TransformOp::ColorAdjust(ColorAdjustParams {
                brightness: 12,
                contrast: 18.0,
                gamma: 1.4,
                saturation: 1.2,
                grayscale: false,
            }),
            &source,
        )
        .expect("color adjust should succeed");
        assert_eq!(adjusted.dimensions(), (16, 12));
    }

    #[test]
    fn add_border_transform_expands_dimensions() {
        let source = test_image(20, 10);
        let bordered = apply_transform(
            TransformOp::AddBorder {
                left: 3,
                right: 4,
                top: 5,
                bottom: 2,
                color: [0, 0, 0, 255],
            },
            &source,
        )
        .expect("add border should succeed");
        assert_eq!(bordered.dimensions(), (27, 17));
    }

    #[test]
    fn canvas_size_transform_changes_dimensions() {
        let source = test_image(30, 20);
        let resized_canvas = apply_transform(
            TransformOp::CanvasSize {
                width: 40,
                height: 16,
                anchor: CanvasAnchor::Center,
                fill: [255, 255, 255, 255],
            },
            &source,
        )
        .expect("canvas size should succeed");
        assert_eq!(resized_canvas.dimensions(), (40, 16));
    }

    #[test]
    fn fine_rotation_transform_expands_canvas_when_requested() {
        let source = test_image(30, 20);
        let rotated = apply_transform(
            TransformOp::RotateFine {
                angle_degrees: 45.0,
                interpolation: RotationInterpolation::Bilinear,
                expand_canvas: true,
                fill: [0, 0, 0, 0],
            },
            &source,
        )
        .expect("fine rotation should succeed");
        let (w, h) = rotated.dimensions();
        assert!(w > 30);
        assert!(h > 20);
    }

    #[test]
    fn text_transform_preserves_dimensions() {
        let source = test_image(30, 20);
        let transformed = apply_transform(
            TransformOp::AddText {
                text: "Hi".to_owned(),
                x: 2,
                y: 2,
                scale: 2,
                color: [255, 255, 255, 255],
            },
            &source,
        )
        .expect("text transform should succeed");
        assert_eq!(transformed.dimensions(), (30, 20));
    }

    #[test]
    fn shape_transform_preserves_dimensions() {
        let source = test_image(30, 20);
        let transformed = apply_transform(
            TransformOp::DrawShape(ShapeParams {
                kind: ShapeKind::Rectangle,
                start_x: 2,
                start_y: 2,
                end_x: 20,
                end_y: 16,
                thickness: 2,
                filled: false,
                color: [255, 0, 0, 255],
            }),
            &source,
        )
        .expect("shape transform should succeed");
        assert_eq!(transformed.dimensions(), (30, 20));
    }

    #[test]
    fn effects_transform_preserves_dimensions() {
        let source = test_image(20, 20);
        let transformed = apply_transform(
            TransformOp::Effects(EffectsParams {
                blur_sigma: 1.0,
                sharpen_sigma: 0.5,
                sharpen_threshold: 1,
                invert: true,
                grayscale: false,
                sepia_strength: 0.4,
                posterize_levels: 8,
                vignette_strength: 0.2,
                tilt_shift_strength: 0.3,
                stained_glass_strength: 0.0,
                emboss_strength: 0.1,
                edge_enhance_strength: 0.2,
                oil_paint_strength: 0.0,
            }),
            &source,
        )
        .expect("effects transform should succeed");
        assert_eq!(transformed.dimensions(), (20, 20));
    }

    #[test]
    fn alpha_brush_transform_preserves_dimensions() {
        let source = test_image(64, 48);
        let transformed = apply_transform(
            TransformOp::AlphaBrush {
                center_x: 32,
                center_y: 24,
                radius: 14,
                strength_percent: 60.0,
                softness: 0.5,
                operation: AlphaBrushOp::SetTransparent,
            },
            &source,
        )
        .expect("alpha brush transform should succeed");
        assert_eq!(transformed.dimensions(), (64, 48));
    }

    #[test]
    fn panorama_stitch_preserves_expected_canvas_size() {
        let a = RgbaImage::from_pixel(80, 32, Rgba([10, 20, 30, 255]));
        let b = RgbaImage::from_pixel(90, 32, Rgba([12, 22, 32, 255]));
        let out = stitch_images(&[a, b], PanoramaDirection::Horizontal, 0.1);
        assert_eq!(out.height(), 32);
        assert_eq!(out.width(), 80 + 90 - 8);
    }
}
