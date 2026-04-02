use std::collections::{HashMap, VecDeque};
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::sync::Mutex;
use std::sync::mpsc::{Receiver, Sender};
use std::thread;
use std::time::Instant;

use anyhow::{Context, Result};
use image::DynamicImage;

use crate::image_io::{
    LoadedImagePayload, ThumbnailPayload, collect_images_in_directory, load_image_payload,
    load_thumbnail_payload, payload_from_working_image, save_image,
};
use crate::perf::{EDIT_IMAGE_BUDGET, OPEN_IMAGE_BUDGET, SAVE_IMAGE_BUDGET, log_timing};

#[derive(Clone, Copy, Debug)]
pub enum WorkerRequestKind {
    Open,
    Save,
    Preload,
    Thumbnail,
}

#[derive(Clone, Copy, Debug)]
pub enum TransformOp {
    RotateLeft,
    RotateRight,
    FlipHorizontal,
    FlipVertical,
}

pub enum WorkerCommand {
    OpenImage {
        request_id: u64,
        path: PathBuf,
    },
    SaveImage {
        request_id: u64,
        path: PathBuf,
        image: Arc<DynamicImage>,
        reopen_after_save: bool,
    },
    TransformImage {
        request_id: u64,
        op: TransformOp,
        image: Arc<DynamicImage>,
    },
    PreloadImage {
        path: PathBuf,
    },
}

pub enum WorkerResult {
    Opened {
        request_id: u64,
        path: PathBuf,
        directory: PathBuf,
        files: Vec<PathBuf>,
        loaded: LoadedImagePayload,
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
    Failed {
        request_id: Option<u64>,
        kind: WorkerRequestKind,
        error: String,
    },
}

struct PreloadCache {
    map: HashMap<PathBuf, LoadedImagePayload>,
    order: VecDeque<PathBuf>,
    capacity: usize,
}

impl PreloadCache {
    fn new(capacity: usize) -> Self {
        Self {
            map: HashMap::new(),
            order: VecDeque::new(),
            capacity,
        }
    }

    fn take(&mut self, path: &Path) -> Option<LoadedImagePayload> {
        let key = path.to_path_buf();
        let value = self.map.remove(&key)?;
        if let Some(index) = self.order.iter().position(|candidate| candidate == path) {
            let _ = self.order.remove(index);
        }
        Some(value)
    }

    fn insert(&mut self, path: PathBuf, payload: LoadedImagePayload) {
        if self.map.contains_key(&path) {
            self.map.insert(path.clone(), payload);
            self.touch(&path);
            return;
        }
        self.map.insert(path.clone(), payload);
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
        while self.map.len() > self.capacity {
            if let Some(oldest) = self.order.pop_front() {
                self.map.remove(&oldest);
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
) {
    log::debug!(target: "imranview::worker", "spawning primary worker thread");
    let _ = thread::Builder::new()
        .name("imranview-worker".to_owned())
        .spawn({
            let result_tx = result_tx.clone();
            move || run_worker(command_rx, result_tx)
        });

    spawn_thumbnail_workers(thumbnail_rx, result_tx);
}

fn spawn_thumbnail_workers(thumbnail_rx: Receiver<PathBuf>, result_tx: Sender<WorkerResult>) {
    let workers = thumbnail_worker_count();
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

fn thumbnail_worker_count() -> usize {
    let logical_cores = thread::available_parallelism()
        .map(|n| n.get())
        .unwrap_or(2);
    logical_cores.saturating_sub(1).clamp(1, 2)
}

fn run_worker(command_rx: Receiver<WorkerCommand>, result_tx: Sender<WorkerResult>) {
    log::debug!(target: "imranview::worker", "worker thread started");
    let mut preload_cache = PreloadCache::new(6);
    while let Ok(command) = command_rx.recv() {
        let result = match command {
            WorkerCommand::OpenImage { request_id, path } => {
                Some(run_open(request_id, path, &mut preload_cache))
            }
            WorkerCommand::SaveImage {
                request_id,
                path,
                image,
                reopen_after_save,
            } => Some(run_save(request_id, path, image, reopen_after_save)),
            WorkerCommand::TransformImage {
                request_id,
                op,
                image,
            } => Some(run_transform(request_id, op, image)),
            WorkerCommand::PreloadImage { path } => run_preload(path, &mut preload_cache),
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
        Ok(WorkerResult::Opened {
            request_id,
            path,
            directory,
            files,
            loaded,
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
    image: Arc<DynamicImage>,
    reopen_after_save: bool,
) -> WorkerResult {
    log::debug!(
        target: "imranview::worker",
        "save start request_id={} path={} reopen_after_save={}",
        request_id,
        path.display(),
        reopen_after_save
    );
    let started = Instant::now();
    let output = save_image(&path, image.as_ref())
        .with_context(|| format!("failed to save {}", path.display()))
        .map(|_| WorkerResult::Saved {
            request_id,
            path,
            reopen_after_save,
        });

    log_timing("save_image", started.elapsed(), SAVE_IMAGE_BUDGET);
    output.unwrap_or_else(|err| WorkerResult::Failed {
        request_id: Some(request_id),
        kind: WorkerRequestKind::Save,
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
    let transformed = match op {
        TransformOp::RotateLeft => image.rotate270(),
        TransformOp::RotateRight => image.rotate90(),
        TransformOp::FlipHorizontal => image.fliph(),
        TransformOp::FlipVertical => image.flipv(),
    };
    let loaded = payload_from_working_image(Arc::new(transformed));
    log_timing("edit_image", started.elapsed(), EDIT_IMAGE_BUDGET);
    WorkerResult::Transformed { request_id, loaded }
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
