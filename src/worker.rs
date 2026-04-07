mod image_ops;
mod panorama;
mod tasks;

use std::collections::{HashMap, VecDeque};
use std::fs;
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::sync::Mutex;
use std::sync::mpsc::{Receiver, Sender, SyncSender, TrySendError};
use std::thread;
use std::time::Instant;

use anyhow::{Context, Result, anyhow};
use font8x8::UnicodeFonts;
use image::imageops::FilterType;
use image::{DynamicImage, GenericImageView, Rgba, RgbaImage};
use lcms2::{Intent, PixelFormat, Profile, Transform};
use serde::{Deserialize, Serialize};

use self::image_ops::{fill_rect_blend, run_transform};
use self::panorama::{blit_rgba, mix_rgba, stitch_images};
#[cfg(test)]
use self::panorama::estimate_vertical_overlap_shift;
use self::tasks::*;

use crate::catalog::list_images_in_directory;
use crate::image_io::{
    LoadedImagePayload, MetadataSummary, SaveFormat, ThumbnailPayload, collect_images_in_directory,
    embed_icc_profile_best_effort, extract_embedded_icc_profile, extract_metadata_summary,
    infer_save_format, load_image_payload, load_thumbnail_payload, load_working_image,
    payload_from_working_image, preserve_metadata_best_effort, save_image_with_format,
};
use crate::perf::{
    EDIT_IMAGE_BUDGET, OPEN_IMAGE_BUDGET, OPEN_QUEUE_BUDGET, SAVE_IMAGE_BUDGET, log_timing,
};

const PRELOAD_CACHE_CAP: usize = 6;
const PRELOAD_CACHE_MAX_BYTES: usize = 192 * 1024 * 1024;
const PRELOAD_WORK_QUEUE_CAP: usize = 8;

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
        queued_at: Instant,
    },
    OpenDirectory {
        request_id: u64,
        directory: PathBuf,
        queued_at: Instant,
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
        device_name: Option<String>,
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
    let preload_cache = Arc::new(Mutex::new(PreloadCache::new(
        config.preload_cache_cap,
        config.preload_cache_max_bytes,
    )));
    let (preload_tx, preload_rx) = std::sync::mpsc::sync_channel::<PathBuf>(PRELOAD_WORK_QUEUE_CAP);

    log::debug!(target: "imranview::worker", "spawning primary worker thread");
    let _ = thread::Builder::new()
        .name("imranview-worker".to_owned())
        .spawn({
            let result_tx = result_tx.clone();
            let preload_cache = Arc::clone(&preload_cache);
            move || run_worker(command_rx, result_tx, preload_cache, preload_tx)
        });

    log::debug!(target: "imranview::worker", "spawning preload worker thread");
    let _ = thread::Builder::new()
        .name("imranview-preload".to_owned())
        .spawn({
            let result_tx = result_tx.clone();
            let preload_cache = Arc::clone(&preload_cache);
            move || run_preload_worker(preload_rx, result_tx, preload_cache)
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
    preload_cache: Arc<Mutex<PreloadCache>>,
    preload_tx: SyncSender<PathBuf>,
) {
    log::debug!(target: "imranview::worker", "worker thread started");
    while let Ok(command) = command_rx.recv() {
        let result = match command {
            WorkerCommand::OpenImage {
                request_id,
                path,
                queued_at,
            } => Some(run_open(request_id, path, queued_at, &preload_cache)),
            WorkerCommand::OpenDirectory {
                request_id,
                directory,
                queued_at,
            } => Some(run_open_directory(
                request_id,
                directory,
                queued_at,
                &preload_cache,
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
            WorkerCommand::PreloadImage { path } => match preload_tx.try_send(path.clone()) {
                Ok(()) => None,
                Err(TrySendError::Full(path)) => {
                    log::debug!(
                        target: "imranview::worker",
                        "preload queue full, skipping {}",
                        path.display()
                    );
                    Some(WorkerResult::Preloaded { path })
                }
                Err(TrySendError::Disconnected(path)) => {
                    log::warn!(
                        target: "imranview::worker",
                        "preload worker disconnected, skipping {}",
                        path.display()
                    );
                    Some(WorkerResult::Preloaded { path })
                }
            },
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
                device_name,
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
                device_name,
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
                let mut cache = preload_cache
                    .lock()
                    .unwrap_or_else(|poisoned| poisoned.into_inner());
                *cache =
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

fn run_preload_worker(
    preload_rx: Receiver<PathBuf>,
    result_tx: Sender<WorkerResult>,
    preload_cache: Arc<Mutex<PreloadCache>>,
) {
    log::debug!(target: "imranview::worker", "preload worker thread started");
    while let Ok(path) = preload_rx.recv() {
        if let Some(result) = run_preload(path, &preload_cache) {
            if result_tx.send(result).is_err() {
                log::warn!(
                    target: "imranview::worker",
                    "result channel disconnected, preload worker exiting"
                );
                break;
            }
        }
    }
    log::debug!(target: "imranview::worker", "preload worker thread stopped");
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

fn take_preloaded(
    path: &Path,
    preload_cache: &Arc<Mutex<PreloadCache>>,
) -> Option<LoadedImagePayload> {
    let mut cache = preload_cache
        .lock()
        .unwrap_or_else(|poisoned| poisoned.into_inner());
    cache.take(path)
}

fn insert_preloaded(
    path: PathBuf,
    payload: LoadedImagePayload,
    preload_cache: &Arc<Mutex<PreloadCache>>,
) {
    let mut cache = preload_cache
        .lock()
        .unwrap_or_else(|poisoned| poisoned.into_inner());
    cache.insert(path, payload);
}

fn run_open(
    request_id: u64,
    path: PathBuf,
    queued_at: Instant,
    preload_cache: &Arc<Mutex<PreloadCache>>,
) -> WorkerResult {
    log::debug!(
        target: "imranview::worker",
        "open start request_id={} path={}",
        request_id,
        path.display()
    );
    log_timing("open_queue", queued_at.elapsed(), OPEN_QUEUE_BUDGET);
    let started = Instant::now();
    let output = (|| -> Result<WorkerResult> {
        let loaded = if let Some(cached) = take_preloaded(&path, preload_cache) {
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
        let files = list_images_in_directory(&directory)?;
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
    queued_at: Instant,
    preload_cache: &Arc<Mutex<PreloadCache>>,
) -> WorkerResult {
    log::debug!(
        target: "imranview::worker",
        "open directory start request_id={} directory={}",
        request_id,
        directory.display()
    );
    log_timing("open_queue", queued_at.elapsed(), OPEN_QUEUE_BUDGET);
    let started = Instant::now();
    let output = (|| -> Result<WorkerResult> {
        let files = list_images_in_directory(&directory)?;
        let path = files
            .first()
            .cloned()
            .context("selected folder has no supported images")?;
        let loaded = if let Some(cached) = take_preloaded(&path, preload_cache) {
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

fn run_preload(path: PathBuf, preload_cache: &Arc<Mutex<PreloadCache>>) -> Option<WorkerResult> {
    log::debug!(
        target: "imranview::worker",
        "preload start path={}",
        path.display()
    );
    match load_image_payload(&path) {
        Ok(payload) => {
            insert_preloaded(path.clone(), payload, preload_cache);
            Some(WorkerResult::Preloaded { path })
        }
        Err(err) => {
            log::debug!(
                target: "imranview::worker",
                "preload skipped path={} error={}",
                path.display(),
                err
            );
            Some(WorkerResult::Preloaded { path })
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

#[cfg(test)]
fn apply_transform(op: TransformOp, image: &DynamicImage) -> Result<DynamicImage> {
    image_ops::apply_transform(op, image)
}

fn html_escape(input: &str) -> String {
    input
        .replace('&', "&amp;")
        .replace('<', "&lt;")
        .replace('>', "&gt;")
        .replace('"', "&quot;")
        .replace('\'', "&#39;")
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
        estimate_vertical_overlap_shift, stitch_images,
    };

    fn test_image(width: u32, height: u32) -> DynamicImage {
        let image = RgbaImage::from_pixel(width, height, Rgba([120, 80, 30, 255]));
        DynamicImage::ImageRgba8(image)
    }

    fn gradient_image(width: u32, height: u32) -> RgbaImage {
        let mut image = RgbaImage::from_pixel(width, height, Rgba([0, 0, 0, 255]));
        for y in 0..height {
            for x in 0..width {
                image.put_pixel(
                    x,
                    y,
                    Rgba([
                        ((x * 5 + y * 3) % 256) as u8,
                        ((x * 2 + y * 7) % 256) as u8,
                        ((x * 11 + y * 13) % 256) as u8,
                        255,
                    ]),
                );
            }
        }
        image
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

    #[test]
    fn panorama_shift_estimation_detects_vertical_offset() {
        let base = gradient_image(96, 72);
        let mut shifted = RgbaImage::from_pixel(96, 72, Rgba([0, 0, 0, 255]));
        let expected_shift = 8i32;
        for y in 0..72 {
            let src_y = y as i32 + expected_shift;
            if src_y >= 0 && src_y < 72 {
                for x in 0..96 {
                    shifted.put_pixel(x, y, *base.get_pixel(x, src_y as u32));
                }
            }
        }

        let estimated = estimate_vertical_overlap_shift(&base, &shifted, 48);
        assert!((estimated - expected_shift).abs() <= 1);
    }

    #[test]
    fn panorama_stitch_expands_height_for_vertical_alignment_shift() {
        let base = gradient_image(90, 60);
        let mut shifted = RgbaImage::from_pixel(90, 60, Rgba([0, 0, 0, 255]));
        for y in 0u32..60 {
            let src_y = y.saturating_add(10).min(59);
            for x in 0..90 {
                shifted.put_pixel(x, y, *base.get_pixel(x, src_y));
            }
        }

        let out = stitch_images(&[base, shifted], PanoramaDirection::Horizontal, 0.2);
        assert!(out.height() > 60);
    }
}
