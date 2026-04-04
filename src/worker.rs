use std::collections::{HashMap, VecDeque};
use std::fs;
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::sync::Mutex;
use std::sync::mpsc::{Receiver, Sender};
use std::thread;
use std::time::Instant;

use anyhow::{Context, Result, anyhow};
use image::imageops::FilterType;
use image::{DynamicImage, GenericImageView};

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
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ResizeFilter {
    Nearest,
    Triangle,
    CatmullRom,
    Gaussian,
    Lanczos3,
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

#[derive(Clone, Copy, Debug)]
pub enum TransformOp {
    RotateLeft,
    RotateRight,
    FlipHorizontal,
    FlipVertical,
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

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum BatchOutputFormat {
    Png,
    Jpeg,
    Webp,
    Bmp,
    Tiff,
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

#[derive(Clone, Debug)]
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
            WorkerCommand::FileOperation {
                request_id,
                operation,
            } => Some(run_file_operation(request_id, operation)),
            WorkerCommand::LoadCompareImage { request_id, path } => {
                Some(run_load_compare(request_id, path))
            }
            WorkerCommand::PrintImage { request_id, path } => Some(run_print(request_id, path)),
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

        Ok(WorkerResult::BatchCompleted {
            request_id,
            processed,
            failed,
            output_dir: options.output_dir,
        })
    })();

    result.unwrap_or_else(|err| WorkerResult::Failed {
        request_id: Some(request_id),
        kind: WorkerRequestKind::Batch,
        error: err.to_string(),
    })
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

    use super::{ColorAdjustParams, ResizeFilter, TransformOp, apply_transform};

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
}
