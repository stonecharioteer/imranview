mod app_state;
mod image_io;
#[cfg(target_os = "macos")]
mod native_menu;
mod perf;
mod plugin;
mod settings;
mod shortcuts;
mod worker;

use std::collections::{HashMap, HashSet, VecDeque};
use std::fs;
use std::path::Path;
use std::path::PathBuf;
use std::sync::mpsc::{self, Receiver, Sender};
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};

use anyhow::{Result, anyhow};
use eframe::egui;

use crate::app_state::{AppState, ThumbnailEntry};
use crate::image_io::MetadataSummary;
use crate::image_io::collect_images_in_directory;
#[cfg(target_os = "macos")]
use crate::native_menu::{NativeMenu, NativeMenuAction};
use crate::plugin::{PluginContext, PluginEvent, PluginHost};
use crate::settings::{PersistedSettings, load_settings, save_settings};
use crate::shortcuts::{ShortcutAction, menu_item_label};
use crate::worker::{
    BatchConvertOptions, BatchOutputFormat, ColorAdjustParams, FileOperation, ResizeFilter,
    SaveImageOptions, SaveMetadataPolicy, SaveOutputFormat, TransformOp, WorkerCommand,
    WorkerRequestKind, WorkerResult,
};

const THUMB_TEXTURE_CACHE_CAP: usize = 320;
const THUMB_TEXTURE_CACHE_MAX_BYTES: usize = 96 * 1024 * 1024;
const THUMB_CARD_WIDTH: f32 = 120.0;
const THUMB_CARD_HEIGHT: f32 = 100.0;
const TOOLBAR_ICON_SIZE: f32 = 18.0;
const APP_FAVICON_PNG: &[u8] = include_bytes!("../assets/branding/favicon.png");
const FOLDER_PANEL_LIST_LIMIT: usize = 256;
const RECENT_MENU_LIMIT: usize = 12;

#[derive(Clone)]
struct ToolbarIcons {
    open: egui::TextureHandle,
    prev: egui::TextureHandle,
    next: egui::TextureHandle,
    zoom_out: egui::TextureHandle,
    zoom_in: egui::TextureHandle,
    actual_size: egui::TextureHandle,
    fit: egui::TextureHandle,
    gallery: egui::TextureHandle,
}

impl ToolbarIcons {
    fn try_load(ctx: &egui::Context) -> Option<Self> {
        match Self::load(ctx) {
            Ok(icons) => Some(icons),
            Err(err) => {
                log::warn!(
                    target: "imranview::ui",
                    "failed to load toolbar icons: {err:#}"
                );
                None
            }
        }
    }

    fn load(ctx: &egui::Context) -> Result<Self> {
        Ok(Self {
            open: load_toolbar_icon(
                ctx,
                "open",
                include_bytes!("../assets/icons/tabler/png/folder-open.png"),
            )?,
            prev: load_toolbar_icon(
                ctx,
                "prev",
                include_bytes!("../assets/icons/tabler/png/chevron-left.png"),
            )?,
            next: load_toolbar_icon(
                ctx,
                "next",
                include_bytes!("../assets/icons/tabler/png/chevron-right.png"),
            )?,
            zoom_out: load_toolbar_icon(
                ctx,
                "zoom-out",
                include_bytes!("../assets/icons/tabler/png/zoom-out.png"),
            )?,
            zoom_in: load_toolbar_icon(
                ctx,
                "zoom-in",
                include_bytes!("../assets/icons/tabler/png/zoom-in.png"),
            )?,
            actual_size: load_toolbar_icon(
                ctx,
                "actual-size",
                include_bytes!("../assets/icons/tabler/png/maximize.png"),
            )?,
            fit: load_toolbar_icon(
                ctx,
                "fit",
                include_bytes!("../assets/icons/tabler/png/aspect-ratio.png"),
            )?,
            gallery: load_toolbar_icon(
                ctx,
                "gallery",
                include_bytes!("../assets/icons/tabler/png/photo.png"),
            )?,
        })
    }
}

fn load_toolbar_icon(ctx: &egui::Context, name: &str, bytes: &[u8]) -> Result<egui::TextureHandle> {
    load_png_texture(ctx, &format!("toolbar-{name}"), bytes)
}

fn load_png_texture(
    ctx: &egui::Context,
    texture_name: &str,
    bytes: &[u8],
) -> Result<egui::TextureHandle> {
    let image = image::load_from_memory(bytes)
        .map_err(|err| anyhow!("failed to decode texture {texture_name}: {err}"))?;
    let rgba = image.to_rgba8();
    let width = rgba.width() as usize;
    let height = rgba.height() as usize;
    let pixels = rgba.into_raw();
    let color = egui::ColorImage::from_rgba_unmultiplied([width, height], &pixels);
    Ok(ctx.load_texture(texture_name.to_owned(), color, egui::TextureOptions::LINEAR))
}

fn load_app_icon_data(bytes: &[u8]) -> Result<egui::IconData> {
    eframe::icon_data::from_png_bytes(bytes)
        .map_err(|err| anyhow!("failed to decode app icon PNG bytes: {err}"))
}

#[derive(Default)]
struct PendingRequests {
    latest_open: u64,
    latest_save: u64,
    latest_edit: u64,
    latest_batch: u64,
    latest_file: u64,
    latest_compare: u64,
    latest_print: u64,
    open_inflight: bool,
    save_inflight: bool,
    edit_inflight: bool,
    batch_inflight: bool,
    file_inflight: bool,
    compare_inflight: bool,
    print_inflight: bool,
    queued_navigation_steps: i32,
}

impl PendingRequests {
    fn has_inflight(&self) -> bool {
        self.open_inflight
            || self.save_inflight
            || self.edit_inflight
            || self.batch_inflight
            || self.file_inflight
            || self.compare_inflight
            || self.print_inflight
    }
}

#[derive(Clone, Debug, PartialEq)]
struct ViewportSnapshot {
    position: Option<[f32; 2]>,
    inner_size: Option<[f32; 2]>,
    maximized: Option<bool>,
    fullscreen: Option<bool>,
}

#[derive(Clone, Debug, Default)]
struct FolderPanelCache {
    current_directory: Option<PathBuf>,
    ancestors: Vec<PathBuf>,
    siblings: Vec<PathBuf>,
    children: Vec<PathBuf>,
}

#[derive(Clone, Debug)]
struct ResizeDialogState {
    open: bool,
    width: u32,
    height: u32,
    keep_aspect: bool,
    filter: ResizeFilter,
}

impl Default for ResizeDialogState {
    fn default() -> Self {
        Self {
            open: false,
            width: 0,
            height: 0,
            keep_aspect: true,
            filter: ResizeFilter::Lanczos3,
        }
    }
}

#[derive(Clone, Debug, Default)]
struct CropDialogState {
    open: bool,
    x: u32,
    y: u32,
    width: u32,
    height: u32,
}

#[derive(Clone, Debug)]
struct ColorDialogState {
    open: bool,
    brightness: i32,
    contrast: f32,
    gamma: f32,
    saturation: f32,
    grayscale: bool,
}

impl Default for ColorDialogState {
    fn default() -> Self {
        Self {
            open: false,
            brightness: 0,
            contrast: 0.0,
            gamma: 1.0,
            saturation: 1.0,
            grayscale: false,
        }
    }
}

#[derive(Clone, Debug)]
struct BatchDialogState {
    open: bool,
    input_dir: String,
    output_dir: String,
    output_format: BatchOutputFormat,
    rename_prefix: String,
    start_index: u32,
    jpeg_quality: u8,
    preview_count: Option<usize>,
    preview_for_input: String,
    preview_error: Option<String>,
}

impl Default for BatchDialogState {
    fn default() -> Self {
        Self {
            open: false,
            input_dir: String::new(),
            output_dir: String::new(),
            output_format: BatchOutputFormat::Jpeg,
            rename_prefix: String::new(),
            start_index: 1,
            jpeg_quality: 90,
            preview_count: None,
            preview_for_input: String::new(),
            preview_error: None,
        }
    }
}

#[derive(Clone, Debug)]
struct SaveDialogState {
    open: bool,
    path: String,
    output_format: SaveOutputFormat,
    jpeg_quality: u8,
    metadata_policy: SaveMetadataPolicy,
    reopen_after_save: bool,
}

impl Default for SaveDialogState {
    fn default() -> Self {
        Self {
            open: false,
            path: String::new(),
            output_format: SaveOutputFormat::Auto,
            jpeg_quality: 92,
            metadata_policy: SaveMetadataPolicy::PreserveIfPossible,
            reopen_after_save: true,
        }
    }
}

#[derive(Clone, Debug)]
struct PerformanceDialogState {
    open: bool,
    thumb_cache_entry_cap: usize,
    thumb_cache_max_mb: usize,
    preload_cache_entry_cap: usize,
    preload_cache_max_mb: usize,
}

impl Default for PerformanceDialogState {
    fn default() -> Self {
        Self {
            open: false,
            thumb_cache_entry_cap: THUMB_TEXTURE_CACHE_CAP,
            thumb_cache_max_mb: THUMB_TEXTURE_CACHE_MAX_BYTES / (1024 * 1024),
            preload_cache_entry_cap: 6,
            preload_cache_max_mb: 192,
        }
    }
}

#[derive(Clone, Debug, Default)]
struct RenameDialogState {
    open: bool,
    target_path: Option<PathBuf>,
    new_name: String,
}

struct ThumbTextureCache {
    map: HashMap<PathBuf, egui::TextureHandle>,
    byte_sizes: HashMap<PathBuf, usize>,
    order: VecDeque<PathBuf>,
    capacity: usize,
    max_bytes: usize,
    total_bytes: usize,
}

struct CompareImageState {
    path: PathBuf,
    texture: egui::TextureHandle,
    width: u32,
    height: u32,
    metadata: MetadataSummary,
}

impl ThumbTextureCache {
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

    fn get(&mut self, path: &PathBuf) -> Option<&egui::TextureHandle> {
        if self.map.contains_key(path) {
            self.touch(path);
        }
        self.map.get(path)
    }

    fn insert(&mut self, path: PathBuf, texture: egui::TextureHandle) {
        let bytes = Self::estimate_texture_bytes(&texture);
        if self.map.contains_key(&path) {
            if let Some(previous_bytes) = self.byte_sizes.insert(path.clone(), bytes) {
                self.total_bytes = self.total_bytes.saturating_sub(previous_bytes);
            }
            self.total_bytes = self.total_bytes.saturating_add(bytes);
            self.map.insert(path.clone(), texture);
            self.touch(&path);
            self.evict_if_needed();
            return;
        }

        self.map.insert(path.clone(), texture);
        self.byte_sizes.insert(path.clone(), bytes);
        self.total_bytes = self.total_bytes.saturating_add(bytes);
        self.order.push_back(path);
        self.evict_if_needed();
    }

    fn touch(&mut self, path: &PathBuf) {
        if let Some(index) = self.order.iter().position(|candidate| candidate == path) {
            if let Some(existing) = self.order.remove(index) {
                self.order.push_back(existing);
            }
        }
    }

    fn evict_if_needed(&mut self) {
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

    fn estimate_texture_bytes(texture: &egui::TextureHandle) -> usize {
        let [width, height] = texture.size();
        width.saturating_mul(height).saturating_mul(4)
    }
}

struct ImranViewApp {
    state: AppState,
    current_metadata: Option<MetadataSummary>,
    worker_tx: Sender<WorkerCommand>,
    thumbnail_tx: Sender<PathBuf>,
    worker_rx: Receiver<WorkerResult>,
    request_sequence: u64,
    pending: PendingRequests,
    main_texture: Option<egui::TextureHandle>,
    main_texture_generation: u64,
    main_scroll_offset: egui::Vec2,
    main_viewport_size: egui::Vec2,
    compare_image: Option<CompareImageState>,
    compare_texture_generation: u64,
    compare_mode: bool,
    plugin_host: PluginHost,
    thumb_cache: ThumbTextureCache,
    inflight_thumbnails: HashSet<PathBuf>,
    inflight_preloads: HashSet<PathBuf>,
    toolbar_icons: Option<ToolbarIcons>,
    about_icon_texture: Option<egui::TextureHandle>,
    last_logged_thumb_entry_count: Option<usize>,
    scroll_thumbnail_to_current: bool,
    folder_panel_cache: FolderPanelCache,
    last_viewport_snapshot: Option<ViewportSnapshot>,
    resize_dialog: ResizeDialogState,
    crop_dialog: CropDialogState,
    color_dialog: ColorDialogState,
    batch_dialog: BatchDialogState,
    save_dialog: SaveDialogState,
    performance_dialog: PerformanceDialogState,
    rename_dialog: RenameDialogState,
    confirm_delete_current: bool,
    info_message: Option<String>,
    slideshow_running: bool,
    slideshow_last_tick: Instant,
    show_about_window: bool,
    center_about_window_next_frame: bool,
    #[cfg(target_os = "macos")]
    native_menu: Option<NativeMenu>,
}

impl ImranViewApp {
    fn new(
        cc: &eframe::CreationContext<'_>,
        cli_path: Option<PathBuf>,
        settings: PersistedSettings,
    ) -> Self {
        let state = AppState::new_with_settings(settings);
        let thumb_cache_entry_cap = state.thumb_cache_entry_cap();
        let thumb_cache_max_bytes = state.thumb_cache_max_mb().saturating_mul(1024 * 1024);
        let (worker_tx, worker_thread_rx) = mpsc::channel::<WorkerCommand>();
        let (thumbnail_tx, thumbnail_rx) = mpsc::channel::<PathBuf>();
        let (worker_thread_tx, worker_rx) = mpsc::channel::<WorkerResult>();
        let worker_config = worker::WorkerConfig {
            preload_cache_cap: state.preload_cache_entry_cap(),
            preload_cache_max_bytes: state.preload_cache_max_mb().saturating_mul(1024 * 1024),
            thumbnail_workers: 0,
        };
        worker::spawn_workers(
            worker_thread_rx,
            thumbnail_rx,
            worker_thread_tx,
            worker_config,
        );
        #[cfg(target_os = "macos")]
        let native_menu = match NativeMenu::install() {
            Ok(menu) => Some(menu),
            Err(err) => {
                log::warn!(
                    target: "imranview::ui",
                    "failed to install native macOS menu; falling back to in-window menu: {err:#}"
                );
                None
            }
        };

        let mut app = Self {
            state,
            current_metadata: None,
            worker_tx,
            thumbnail_tx,
            worker_rx,
            request_sequence: 1,
            pending: PendingRequests::default(),
            main_texture: None,
            main_texture_generation: 1,
            main_scroll_offset: egui::Vec2::ZERO,
            main_viewport_size: egui::Vec2::ZERO,
            compare_image: None,
            compare_texture_generation: 1,
            compare_mode: false,
            plugin_host: PluginHost::new_with_builtins(),
            thumb_cache: ThumbTextureCache::new(thumb_cache_entry_cap, thumb_cache_max_bytes),
            inflight_thumbnails: HashSet::new(),
            inflight_preloads: HashSet::new(),
            toolbar_icons: ToolbarIcons::try_load(&cc.egui_ctx),
            about_icon_texture: load_png_texture(&cc.egui_ctx, "about-favicon", APP_FAVICON_PNG)
                .map_err(|err| {
                    log::warn!(target: "imranview::ui", "failed to load about favicon texture: {err:#}");
                    err
                })
                .ok(),
            last_logged_thumb_entry_count: None,
            scroll_thumbnail_to_current: false,
            folder_panel_cache: FolderPanelCache::default(),
            last_viewport_snapshot: None,
            resize_dialog: ResizeDialogState::default(),
            crop_dialog: CropDialogState::default(),
            color_dialog: ColorDialogState::default(),
            batch_dialog: BatchDialogState::default(),
            save_dialog: SaveDialogState::default(),
            performance_dialog: PerformanceDialogState::default(),
            rename_dialog: RenameDialogState::default(),
            confirm_delete_current: false,
            info_message: None,
            slideshow_running: false,
            slideshow_last_tick: Instant::now(),
            show_about_window: false,
            center_about_window_next_frame: false,
            #[cfg(target_os = "macos")]
            native_menu,
        };

        if let Some(path) = cli_path {
            log::debug!(target: "imranview::ui", "startup CLI open {}", path.display());
            app.dispatch_open(path, false);
        }

        app
    }

    fn next_request_id(&mut self) -> u64 {
        let next = self.request_sequence;
        self.request_sequence = self.request_sequence.saturating_add(1);
        next
    }

    fn persist_settings(&self) {
        if let Err(err) = save_settings(&self.state.to_settings()) {
            log::warn!(
                target: "imranview::settings",
                "failed to save settings: {err:#}"
            );
        }
    }

    fn dispatch_open(&mut self, path: PathBuf, from_navigation: bool) {
        if !from_navigation {
            self.pending.queued_navigation_steps = 0;
        }
        self.inflight_preloads.remove(&path);
        let request_id = self.next_request_id();
        self.pending.latest_open = request_id;
        self.pending.open_inflight = true;
        log::debug!(
            target: "imranview::ui",
            "queue open request_id={} path={}",
            request_id,
            path.display()
        );

        if self
            .worker_tx
            .send(WorkerCommand::OpenImage { request_id, path })
            .is_err()
        {
            self.pending.open_inflight = false;
            log::error!(target: "imranview::ui", "failed to queue open-image command");
            self.state.set_error("failed to queue open-image command");
        }
    }

    fn dispatch_open_directory(&mut self, directory: PathBuf) {
        let request_id = self.next_request_id();
        self.pending.latest_open = request_id;
        self.pending.open_inflight = true;
        self.pending.queued_navigation_steps = 0;
        log::debug!(
            target: "imranview::ui",
            "queue directory open request_id={} directory={}",
            request_id,
            directory.display()
        );

        if self
            .worker_tx
            .send(WorkerCommand::OpenDirectory {
                request_id,
                directory,
            })
            .is_err()
        {
            self.pending.open_inflight = false;
            log::error!(target: "imranview::ui", "failed to queue open-directory command");
            self.state
                .set_error("failed to queue open-directory command");
        }
    }

    fn dispatch_save(
        &mut self,
        path: Option<PathBuf>,
        reopen_after_save: bool,
        options: SaveImageOptions,
    ) {
        let source_path = self.state.current_file_path();
        let Some(path) = path.or_else(|| self.state.current_file_path()) else {
            self.state.set_error("no image loaded to save");
            return;
        };

        let image = match self.state.current_working_image() {
            Ok(image) => image,
            Err(err) => {
                self.state.set_error(err.to_string());
                return;
            }
        };

        let request_id = self.next_request_id();
        self.pending.latest_save = request_id;
        self.pending.save_inflight = true;
        log::debug!(
            target: "imranview::ui",
            "queue save request_id={} path={} reopen_after_save={} format={:?} metadata_policy={:?}",
            request_id,
            path.display(),
            reopen_after_save,
            options.output_format,
            options.metadata_policy
        );

        if self
            .worker_tx
            .send(WorkerCommand::SaveImage {
                request_id,
                path,
                source_path,
                image,
                reopen_after_save,
                options,
            })
            .is_err()
        {
            self.pending.save_inflight = false;
            log::error!(target: "imranview::ui", "failed to queue save-image command");
            self.state.set_error("failed to queue save-image command");
        }
    }

    fn default_save_options(&self) -> SaveImageOptions {
        SaveImageOptions {
            output_format: SaveOutputFormat::Auto,
            jpeg_quality: 92,
            metadata_policy: SaveMetadataPolicy::PreserveIfPossible,
        }
    }

    fn plugin_context(&self) -> PluginContext {
        PluginContext {
            has_image: self.state.has_image(),
            current_file: self.state.current_file_path(),
            compare_mode: self.compare_mode,
        }
    }

    fn apply_zoom_change<F>(&mut self, zoom_change: F)
    where
        F: FnOnce(&mut AppState),
    {
        let old_zoom = if self.state.zoom_is_fit() {
            None
        } else {
            Some(self.state.zoom_factor())
        };
        let old_offset = self.main_scroll_offset;
        let viewport_size = self.main_viewport_size;

        zoom_change(&mut self.state);

        let Some(old_zoom) = old_zoom else {
            if self.state.zoom_is_fit() {
                self.main_scroll_offset = egui::Vec2::ZERO;
            }
            return;
        };
        if self.state.zoom_is_fit() {
            self.main_scroll_offset = egui::Vec2::ZERO;
            return;
        }

        let new_zoom = self.state.zoom_factor();
        if (new_zoom - old_zoom).abs() < f32::EPSILON
            || viewport_size.x <= 0.0
            || viewport_size.y <= 0.0
        {
            return;
        }

        let old_center = old_offset + viewport_size * 0.5;
        let scale = new_zoom / old_zoom;
        self.main_scroll_offset = old_center * scale - viewport_size * 0.5;
        self.main_scroll_offset.x = self.main_scroll_offset.x.max(0.0);
        self.main_scroll_offset.y = self.main_scroll_offset.y.max(0.0);
    }

    fn zoom_in(&mut self) {
        self.apply_zoom_change(|state| state.zoom_in());
    }

    fn zoom_out(&mut self) {
        self.apply_zoom_change(|state| state.zoom_out());
    }

    fn zoom_fit(&mut self) {
        self.apply_zoom_change(|state| state.set_zoom_fit());
    }

    fn zoom_actual(&mut self) {
        self.apply_zoom_change(|state| state.set_zoom_actual());
    }

    fn dispatch_transform(&mut self, op: TransformOp) {
        let image = match self.state.current_working_image() {
            Ok(image) => image,
            Err(err) => {
                self.state.set_error(err.to_string());
                return;
            }
        };

        let request_id = self.next_request_id();
        self.pending.latest_edit = request_id;
        self.pending.edit_inflight = true;
        log::debug!(
            target: "imranview::ui",
            "queue transform request_id={} op={:?}",
            request_id,
            op
        );

        if self
            .worker_tx
            .send(WorkerCommand::TransformImage {
                request_id,
                op,
                image,
            })
            .is_err()
        {
            self.pending.edit_inflight = false;
            log::error!(target: "imranview::ui", "failed to queue transform-image command");
            self.state
                .set_error("failed to queue transform-image command");
        }
    }

    fn dispatch_batch_convert(&mut self, options: BatchConvertOptions) {
        let request_id = self.next_request_id();
        self.pending.latest_batch = request_id;
        self.pending.batch_inflight = true;
        log::debug!(
            target: "imranview::ui",
            "queue batch convert request_id={} input={} output={}",
            request_id,
            options.input_dir.display(),
            options.output_dir.display()
        );

        if self
            .worker_tx
            .send(WorkerCommand::BatchConvert {
                request_id,
                options,
            })
            .is_err()
        {
            self.pending.batch_inflight = false;
            self.state
                .set_error("failed to queue batch-convert command");
        }
    }

    fn dispatch_file_operation(&mut self, operation: FileOperation) {
        let request_id = self.next_request_id();
        self.pending.latest_file = request_id;
        self.pending.file_inflight = true;
        log::debug!(
            target: "imranview::ui",
            "queue file operation request_id={} op={:?}",
            request_id,
            operation
        );

        if self
            .worker_tx
            .send(WorkerCommand::FileOperation {
                request_id,
                operation,
            })
            .is_err()
        {
            self.pending.file_inflight = false;
            self.state
                .set_error("failed to queue file operation command");
        }
    }

    fn dispatch_compare_open(&mut self, path: PathBuf) {
        let request_id = self.next_request_id();
        self.pending.latest_compare = request_id;
        self.pending.compare_inflight = true;
        log::debug!(
            target: "imranview::ui",
            "queue compare load request_id={} path={}",
            request_id,
            path.display()
        );
        if self
            .worker_tx
            .send(WorkerCommand::LoadCompareImage { request_id, path })
            .is_err()
        {
            self.pending.compare_inflight = false;
            self.state
                .set_error("failed to queue compare-image command");
        }
    }

    fn dispatch_print_current(&mut self) {
        let Some(path) = self.state.current_file_path() else {
            self.state.set_error("no image loaded");
            return;
        };
        let request_id = self.next_request_id();
        self.pending.latest_print = request_id;
        self.pending.print_inflight = true;
        log::debug!(
            target: "imranview::ui",
            "queue print request_id={} path={}",
            request_id,
            path.display()
        );
        if self
            .worker_tx
            .send(WorkerCommand::PrintImage { request_id, path })
            .is_err()
        {
            self.pending.print_inflight = false;
            self.state.set_error("failed to queue print command");
        }
    }

    fn request_thumbnail_decode(&mut self, path: PathBuf) {
        if self.inflight_thumbnails.contains(&path) {
            return;
        }
        self.inflight_thumbnails.insert(path.clone());
        log::debug!(
            target: "imranview::thumb",
            "queue thumbnail decode path={}",
            path.display()
        );

        if self.thumbnail_tx.send(path.clone()).is_err() {
            self.inflight_thumbnails.remove(&path);
            log::error!(target: "imranview::thumb", "failed to queue thumbnail decode");
            self.state
                .set_error("failed to queue thumbnail decode command");
        }
    }

    fn queue_navigation_step(&mut self, step: i32) {
        if step == 0 {
            return;
        }
        if !self.state.has_image() {
            self.state.set_error("no image loaded");
            return;
        }

        self.pending.queued_navigation_steps =
            (self.pending.queued_navigation_steps + step).clamp(-256, 256);
        log::debug!(
            target: "imranview::ui",
            "queue navigation step={} backlog={}",
            step,
            self.pending.queued_navigation_steps
        );

        if !self.pending.open_inflight {
            self.dispatch_queued_navigation_step();
        }
    }

    fn dispatch_queued_navigation_step(&mut self) {
        let queued = self.pending.queued_navigation_steps;
        if queued == 0 {
            return;
        }

        let forward = queued > 0;
        let path_result = if forward {
            self.state.resolve_next_path()
        } else {
            self.state.resolve_previous_path()
        };

        match path_result {
            Ok(path) => {
                if forward {
                    self.pending.queued_navigation_steps -= 1;
                } else {
                    self.pending.queued_navigation_steps += 1;
                }
                self.dispatch_open(path, true);
            }
            Err(err) => {
                self.pending.queued_navigation_steps = 0;
                self.state.set_error(err.to_string());
            }
        }
    }

    fn schedule_preload_neighbors(&mut self) {
        let mut candidates = Vec::with_capacity(2);

        if let Ok(next) = self.state.resolve_next_path() {
            candidates.push(next);
        }
        if let Ok(previous) = self.state.resolve_previous_path() {
            if !candidates.iter().any(|candidate| candidate == &previous) {
                candidates.push(previous);
            }
        }

        for path in candidates {
            if self.inflight_preloads.contains(&path) {
                continue;
            }
            self.inflight_preloads.insert(path.clone());
            if self
                .worker_tx
                .send(WorkerCommand::PreloadImage { path: path.clone() })
                .is_err()
            {
                self.inflight_preloads.remove(&path);
                log::warn!(
                    target: "imranview::worker",
                    "failed to queue preload for {}",
                    path.display()
                );
            }
        }
    }

    fn poll_worker_results(&mut self, ctx: &egui::Context) {
        loop {
            match self.worker_rx.try_recv() {
                Ok(result) => self.handle_worker_result(ctx, result),
                Err(mpsc::TryRecvError::Empty) => break,
                Err(mpsc::TryRecvError::Disconnected) => {
                    log::error!(target: "imranview::worker", "background worker disconnected");
                    self.state
                        .set_error("background worker disconnected unexpectedly");
                    break;
                }
            }
        }
    }

    fn handle_worker_result(&mut self, ctx: &egui::Context, result: WorkerResult) {
        match result {
            WorkerResult::Opened {
                request_id,
                path,
                directory,
                files,
                loaded,
                metadata,
            } => {
                if request_id != self.pending.latest_open {
                    log::debug!(
                        target: "imranview::worker",
                        "drop stale open result request_id={} latest_open={}",
                        request_id,
                        self.pending.latest_open
                    );
                    return;
                }
                self.pending.open_inflight = false;
                self.state
                    .apply_open_payload(path, directory, files, loaded);
                self.current_metadata = Some(metadata);
                self.clear_folder_panel_cache();
                self.update_main_texture_from_state(ctx);
                self.scroll_thumbnail_to_current = true;
                self.schedule_preload_neighbors();
                let thumb_entries = self.state.thumbnail_entries().len();
                log::debug!(
                    target: "imranview::worker",
                    "open applied request_id={} thumbs={} in_window_mode={}",
                    request_id,
                    thumb_entries,
                    self.state.thumbnails_window_mode()
                );
                self.persist_settings();
                self.dispatch_queued_navigation_step();
                if let Some(current_path) = self.state.current_file_path() {
                    let context = self.plugin_context();
                    self.plugin_host
                        .emit(PluginEvent::ImageOpened(current_path), &context);
                }
            }
            WorkerResult::Saved {
                request_id,
                path,
                reopen_after_save,
            } => {
                if request_id != self.pending.latest_save {
                    log::debug!(
                        target: "imranview::worker",
                        "drop stale save result request_id={} latest_save={}",
                        request_id,
                        self.pending.latest_save
                    );
                    return;
                }
                self.pending.save_inflight = false;
                if reopen_after_save {
                    self.dispatch_open(path, false);
                } else {
                    log::debug!(target: "imranview::worker", "save applied request_id={request_id}");
                    self.state.clear_error();
                    self.persist_settings();
                    let context = self.plugin_context();
                    self.plugin_host
                        .emit(PluginEvent::ImageSaved(path), &context);
                }
            }
            WorkerResult::Transformed { request_id, loaded } => {
                if request_id != self.pending.latest_edit {
                    log::debug!(
                        target: "imranview::worker",
                        "drop stale transform result request_id={} latest_edit={}",
                        request_id,
                        self.pending.latest_edit
                    );
                    return;
                }
                self.pending.edit_inflight = false;
                if let Err(err) = self.state.apply_transform_payload(loaded) {
                    self.state.set_error(err.to_string());
                }
                self.update_main_texture_from_state(ctx);
                let context = self.plugin_context();
                self.plugin_host.emit(
                    PluginEvent::TransformApplied("image-transform".to_owned()),
                    &context,
                );
            }
            WorkerResult::BatchCompleted {
                request_id,
                processed,
                failed,
                output_dir,
            } => {
                if request_id != self.pending.latest_batch {
                    log::debug!(
                        target: "imranview::worker",
                        "drop stale batch result request_id={} latest_batch={}",
                        request_id,
                        self.pending.latest_batch
                    );
                    return;
                }
                self.pending.batch_inflight = false;
                self.info_message = Some(format!(
                    "Batch complete: {} processed, {} failed ({})",
                    processed,
                    failed,
                    output_dir.display()
                ));
                let context = self.plugin_context();
                self.plugin_host
                    .emit(PluginEvent::BatchCompleted { processed, failed }, &context);
            }
            WorkerResult::FileOperationCompleted {
                request_id,
                operation,
            } => {
                if request_id != self.pending.latest_file {
                    log::debug!(
                        target: "imranview::worker",
                        "drop stale file-op result request_id={} latest_file={}",
                        request_id,
                        self.pending.latest_file
                    );
                    return;
                }
                self.pending.file_inflight = false;
                match operation {
                    FileOperation::Rename { from, to } => {
                        if self.state.current_file_path() == Some(from.clone()) {
                            self.dispatch_open(to.clone(), false);
                        }
                        self.info_message = Some(format!("Renamed to {}", to.display()));
                        let context = self.plugin_context();
                        self.plugin_host.emit(
                            PluginEvent::FileOperation(format!(
                                "rename {} -> {}",
                                from.display(),
                                to.display()
                            )),
                            &context,
                        );
                    }
                    FileOperation::Delete { path } => {
                        self.info_message = Some(format!("Deleted {}", path.display()));
                        if self.state.current_file_path() == Some(path.clone()) {
                            if let Some(directory) = self.state.current_directory_path() {
                                self.dispatch_open_directory(directory);
                            }
                        }
                        let context = self.plugin_context();
                        self.plugin_host.emit(
                            PluginEvent::FileOperation(format!("delete {}", path.display())),
                            &context,
                        );
                    }
                    FileOperation::Copy { from: _, to } => {
                        self.info_message = Some(format!("Copied to {}", to.display()));
                        let context = self.plugin_context();
                        self.plugin_host.emit(
                            PluginEvent::FileOperation(format!("copy -> {}", to.display())),
                            &context,
                        );
                    }
                    FileOperation::Move { from, to } => {
                        if self.state.current_file_path() == Some(from.clone()) {
                            self.dispatch_open(to.clone(), false);
                        }
                        self.info_message = Some(format!("Moved to {}", to.display()));
                        let context = self.plugin_context();
                        self.plugin_host.emit(
                            PluginEvent::FileOperation(format!(
                                "move {} -> {}",
                                from.display(),
                                to.display()
                            )),
                            &context,
                        );
                    }
                }
            }
            WorkerResult::CompareLoaded {
                request_id,
                path,
                loaded,
                metadata,
            } => {
                if request_id != self.pending.latest_compare {
                    return;
                }
                self.pending.compare_inflight = false;
                let texture = Self::texture_from_rgba(
                    ctx,
                    format!("compare-image-{}", self.compare_texture_generation),
                    &loaded.preview_rgba,
                    loaded.preview_width,
                    loaded.preview_height,
                );
                self.compare_texture_generation = self.compare_texture_generation.saturating_add(1);
                self.compare_image = Some(CompareImageState {
                    path: path.clone(),
                    texture,
                    width: loaded.preview_width,
                    height: loaded.preview_height,
                    metadata,
                });
                self.compare_mode = true;
                self.info_message = Some(format!("Loaded compare image {}", path.display()));
                let context = self.plugin_context();
                self.plugin_host
                    .emit(PluginEvent::CompareLoaded(path), &context);
            }
            WorkerResult::Printed { request_id, path } => {
                if request_id != self.pending.latest_print {
                    return;
                }
                self.pending.print_inflight = false;
                self.info_message = Some(format!("Print job submitted for {}", path.display()));
                let context = self.plugin_context();
                self.plugin_host
                    .emit(PluginEvent::PrintSubmitted(path), &context);
            }
            WorkerResult::ThumbnailDecoded { path, payload } => {
                self.inflight_thumbnails.remove(&path);
                let texture = Self::texture_from_rgba(
                    ctx,
                    format!("thumb-{}", path.display()),
                    &payload.rgba,
                    payload.width,
                    payload.height,
                );
                self.thumb_cache.insert(path, texture);
                log::debug!(
                    target: "imranview::thumb",
                    "thumbnail decoded {}x{} cache_size={} cache_bytes={} inflight={}",
                    payload.width,
                    payload.height,
                    self.thumb_cache.map.len(),
                    self.thumb_cache.total_bytes,
                    self.inflight_thumbnails.len()
                );
            }
            WorkerResult::Preloaded { path } => {
                self.inflight_preloads.remove(&path);
                log::debug!(
                    target: "imranview::worker",
                    "preload ready path={} inflight_preloads={}",
                    path.display(),
                    self.inflight_preloads.len()
                );
            }
            WorkerResult::Failed {
                request_id,
                kind,
                error,
            } => {
                log::warn!(
                    target: "imranview::worker",
                    "worker failure kind={:?} request_id={:?}: {}",
                    kind,
                    request_id,
                    error
                );
                let error_message = Self::format_worker_error(kind, &error);
                match (kind, request_id) {
                    (WorkerRequestKind::Open, Some(id)) if id == self.pending.latest_open => {
                        self.pending.open_inflight = false;
                        self.pending.queued_navigation_steps = 0;
                        self.state.set_error(error_message);
                    }
                    (WorkerRequestKind::Save, Some(id)) if id == self.pending.latest_save => {
                        self.pending.save_inflight = false;
                        self.state.set_error(error_message);
                    }
                    (WorkerRequestKind::Edit, Some(id)) if id == self.pending.latest_edit => {
                        self.pending.edit_inflight = false;
                        self.state.set_error(error_message);
                    }
                    (WorkerRequestKind::Batch, Some(id)) if id == self.pending.latest_batch => {
                        self.pending.batch_inflight = false;
                        self.state.set_error(error_message);
                    }
                    (WorkerRequestKind::File, Some(id)) if id == self.pending.latest_file => {
                        self.pending.file_inflight = false;
                        self.state.set_error(error_message);
                    }
                    (WorkerRequestKind::Compare, Some(id)) if id == self.pending.latest_compare => {
                        self.pending.compare_inflight = false;
                        self.state.set_error(error_message);
                    }
                    (WorkerRequestKind::Print, Some(id)) if id == self.pending.latest_print => {
                        self.pending.print_inflight = false;
                        self.state.set_error(error_message);
                    }
                    (WorkerRequestKind::Preload, _) => {
                        // Preload failures are expected for transient/unsupported files.
                    }
                    (WorkerRequestKind::Thumbnail, _) => {
                        // Keep this low-noise for folders with unreadable files.
                    }
                    _ => {}
                }
            }
        }
    }

    fn update_main_texture_from_state(&mut self, ctx: &egui::Context) {
        let Some((rgba, width, height)) = self.state.current_preview_rgba() else {
            self.main_texture = None;
            return;
        };

        let texture = Self::texture_from_rgba(
            ctx,
            format!("main-image-{}", self.main_texture_generation),
            &rgba,
            width,
            height,
        );
        self.main_texture_generation = self.main_texture_generation.saturating_add(1);
        self.main_texture = Some(texture);
    }

    fn texture_from_rgba(
        ctx: &egui::Context,
        name: String,
        rgba: &[u8],
        width: u32,
        height: u32,
    ) -> egui::TextureHandle {
        let color =
            egui::ColorImage::from_rgba_unmultiplied([width as usize, height as usize], rgba);
        ctx.load_texture(name, color, egui::TextureOptions::LINEAR)
    }

    fn run_shortcuts(&mut self, ctx: &egui::Context) {
        if shortcuts::trigger(ctx, ShortcutAction::SaveAs) {
            self.open_save_as_dialog();
        } else if shortcuts::trigger(ctx, ShortcutAction::Save) {
            self.dispatch_save(None, false, self.default_save_options());
        }
        if shortcuts::trigger(ctx, ShortcutAction::Open) {
            self.open_path_dialog();
        }
        if shortcuts::trigger(ctx, ShortcutAction::NextImage) {
            self.open_next();
        }
        if shortcuts::trigger(ctx, ShortcutAction::PreviousImage) {
            self.open_previous();
        }
        if shortcuts::trigger(ctx, ShortcutAction::ZoomIn) {
            self.zoom_in();
        }
        if shortcuts::trigger(ctx, ShortcutAction::ZoomOut) {
            self.zoom_out();
        }
        if shortcuts::trigger(ctx, ShortcutAction::Fit) {
            self.zoom_fit();
        }
        if shortcuts::trigger(ctx, ShortcutAction::ActualSize) {
            self.zoom_actual();
        }
        if ctx.input(|i| i.key_pressed(egui::Key::Space)) {
            if self.slideshow_running {
                self.stop_slideshow();
            } else {
                self.start_slideshow();
            }
        }
        if ctx.input(|i| i.key_pressed(egui::Key::Escape)) {
            self.stop_slideshow();
        }

        let wheel_zoom = ctx.input(|i| {
            if i.modifiers.command || i.modifiers.ctrl {
                i.raw_scroll_delta.y
            } else {
                0.0
            }
        });
        if wheel_zoom != 0.0 {
            if wheel_zoom > 0.0 {
                self.zoom_in();
            } else if wheel_zoom < 0.0 {
                self.zoom_out();
            }
        }
    }

    fn open_next(&mut self) {
        self.queue_navigation_step(1);
        self.slideshow_last_tick = Instant::now();
    }

    fn open_previous(&mut self) {
        self.queue_navigation_step(-1);
        self.slideshow_last_tick = Instant::now();
    }

    fn open_path_dialog(&mut self) {
        let preferred_directory = self.state.preferred_open_directory();
        let mut dialog = rfd::FileDialog::new().set_title("Open image").add_filter(
            "Images",
            &[
                "avif", "bmp", "gif", "heic", "heif", "hdr", "ico", "jpeg", "jpg", "pbm", "pgm",
                "png", "pnm", "ppm", "qoi", "tif", "tiff", "webp",
            ],
        );

        if let Some(directory) = preferred_directory {
            dialog = dialog.set_directory(directory);
        }

        if let Some(path) = dialog.pick_file() {
            self.dispatch_open(path, false);
        }
    }

    fn open_compare_path_dialog(&mut self) {
        let preferred_directory = self.state.preferred_open_directory();
        let mut dialog = rfd::FileDialog::new()
            .set_title("Open compare image")
            .add_filter(
                "Images",
                &[
                    "avif", "bmp", "gif", "heic", "heif", "hdr", "ico", "jpeg", "jpg", "pbm",
                    "pgm", "png", "pnm", "ppm", "qoi", "tif", "tiff", "webp",
                ],
            );
        if let Some(directory) = preferred_directory {
            dialog = dialog.set_directory(directory);
        }
        if let Some(path) = dialog.pick_file() {
            self.dispatch_compare_open(path);
        }
    }

    fn open_save_as_dialog(&mut self) {
        if let Some(path) = self.state.current_file_path() {
            self.save_dialog.path = path.display().to_string();
        } else {
            let directory = self
                .state
                .preferred_open_directory()
                .unwrap_or_else(|| PathBuf::from("."));
            let suggested_name = self
                .state
                .suggested_save_name()
                .unwrap_or_else(|| "image.jpg".to_owned());
            self.save_dialog.path = directory.join(suggested_name).display().to_string();
        }
        let defaults = self.default_save_options();
        self.save_dialog.output_format = defaults.output_format;
        self.save_dialog.jpeg_quality = defaults.jpeg_quality;
        self.save_dialog.metadata_policy = defaults.metadata_policy;
        self.save_dialog.reopen_after_save = true;
        self.save_dialog.open = true;
    }

    fn build_save_options_from_dialog(&self) -> SaveImageOptions {
        SaveImageOptions {
            output_format: self.save_dialog.output_format,
            jpeg_quality: self.save_dialog.jpeg_quality,
            metadata_policy: self.save_dialog.metadata_policy,
        }
    }

    fn open_resize_dialog(&mut self) {
        if let Some((width, height)) = self.state.original_dimensions() {
            self.resize_dialog.width = width;
            self.resize_dialog.height = height;
        }
        self.resize_dialog.open = true;
    }

    fn open_crop_dialog(&mut self) {
        if let Some((width, height)) = self.state.original_dimensions() {
            self.crop_dialog.x = 0;
            self.crop_dialog.y = 0;
            self.crop_dialog.width = width;
            self.crop_dialog.height = height;
        }
        self.crop_dialog.open = true;
    }

    fn open_color_dialog(&mut self) {
        self.color_dialog = ColorDialogState::default();
        self.color_dialog.open = true;
    }

    fn open_batch_dialog(&mut self) {
        let input_dir = self
            .state
            .current_directory_path()
            .or_else(|| self.state.preferred_open_directory());
        if let Some(input_dir) = input_dir {
            self.batch_dialog.input_dir = input_dir.display().to_string();
            self.batch_dialog.output_dir = input_dir.join("output").display().to_string();
        }
        self.batch_dialog.preview_count = None;
        self.batch_dialog.preview_for_input.clear();
        self.batch_dialog.preview_error = None;
        self.batch_dialog.open = true;
    }

    fn open_rename_dialog(&mut self) {
        let Some(current_path) = self.state.current_file_path() else {
            self.state.set_error("no image loaded");
            return;
        };
        self.rename_dialog.target_path = Some(current_path.clone());
        self.rename_dialog.new_name = current_path
            .file_name()
            .map(|name| name.to_string_lossy().into_owned())
            .unwrap_or_default();
        self.rename_dialog.open = true;
    }

    fn copy_current_to_dialog(&mut self) {
        let Some(source) = self.state.current_file_path() else {
            self.state.set_error("no image loaded");
            return;
        };
        let Some(file_name) = source.file_name() else {
            self.state.set_error("failed to resolve current file name");
            return;
        };

        let mut dialog = rfd::FileDialog::new().set_title("Copy image to folder");
        if let Some(directory) = self.state.current_directory_path() {
            dialog = dialog.set_directory(directory);
        }
        if let Some(destination_dir) = dialog.pick_folder() {
            let destination = destination_dir.join(file_name);
            self.dispatch_file_operation(FileOperation::Copy {
                from: source,
                to: destination,
            });
        }
    }

    fn move_current_to_dialog(&mut self) {
        let Some(source) = self.state.current_file_path() else {
            self.state.set_error("no image loaded");
            return;
        };
        let Some(file_name) = source.file_name() else {
            self.state.set_error("failed to resolve current file name");
            return;
        };

        let mut dialog = rfd::FileDialog::new().set_title("Move image to folder");
        if let Some(directory) = self.state.current_directory_path() {
            dialog = dialog.set_directory(directory);
        }
        if let Some(destination_dir) = dialog.pick_folder() {
            let destination = destination_dir.join(file_name);
            self.dispatch_file_operation(FileOperation::Move {
                from: source,
                to: destination,
            });
        }
    }

    fn delete_current_file(&mut self) {
        let Some(path) = self.state.current_file_path() else {
            self.state.set_error("no image loaded");
            return;
        };
        self.dispatch_file_operation(FileOperation::Delete { path });
    }

    fn open_about_window(&mut self) {
        self.show_about_window = true;
        self.center_about_window_next_frame = true;
    }

    fn open_performance_dialog(&mut self) {
        self.performance_dialog.thumb_cache_entry_cap = self.state.thumb_cache_entry_cap();
        self.performance_dialog.thumb_cache_max_mb = self.state.thumb_cache_max_mb();
        self.performance_dialog.preload_cache_entry_cap = self.state.preload_cache_entry_cap();
        self.performance_dialog.preload_cache_max_mb = self.state.preload_cache_max_mb();
        self.performance_dialog.open = true;
    }

    fn clear_runtime_caches(&mut self) {
        self.thumb_cache = ThumbTextureCache::new(
            self.state.thumb_cache_entry_cap(),
            self.state.thumb_cache_max_mb().saturating_mul(1024 * 1024),
        );
        self.inflight_thumbnails.clear();
        self.inflight_preloads.clear();
        self.info_message = Some("Cleared thumbnail and preload caches".to_owned());
    }

    fn apply_performance_settings(&mut self) {
        self.state
            .set_thumb_cache_entry_cap(self.performance_dialog.thumb_cache_entry_cap);
        self.state
            .set_thumb_cache_max_mb(self.performance_dialog.thumb_cache_max_mb);
        self.state
            .set_preload_cache_entry_cap(self.performance_dialog.preload_cache_entry_cap);
        self.state
            .set_preload_cache_max_mb(self.performance_dialog.preload_cache_max_mb);

        self.clear_runtime_caches();
        let _ = self.worker_tx.send(WorkerCommand::UpdateCachePolicy {
            preload_cache_cap: self.state.preload_cache_entry_cap(),
            preload_cache_max_bytes: self
                .state
                .preload_cache_max_mb()
                .saturating_mul(1024 * 1024),
        });
        self.persist_settings();
    }

    fn clear_folder_panel_cache(&mut self) {
        self.folder_panel_cache = FolderPanelCache::default();
    }

    fn ensure_folder_panel_cache(&mut self) {
        let current_directory = self.state.current_directory_path();
        if self.folder_panel_cache.current_directory == current_directory {
            return;
        }

        let mut cache = FolderPanelCache {
            current_directory: current_directory.clone(),
            ancestors: Vec::new(),
            siblings: Vec::new(),
            children: Vec::new(),
        };

        if let Some(current) = current_directory {
            cache.ancestors = path_ancestors(&current);
            if let Some(parent) = current.parent() {
                cache.siblings = list_directories(parent, FOLDER_PANEL_LIST_LIMIT);
            }
            cache.children = list_directories(&current, FOLDER_PANEL_LIST_LIMIT);
        }

        self.folder_panel_cache = cache;
    }

    fn open_directory_from_panel(&mut self, directory: PathBuf) {
        if self.state.current_directory_path() == Some(directory.clone()) {
            return;
        }
        self.dispatch_open_directory(directory);
    }

    fn sync_viewport_state(&mut self, ctx: &egui::Context) {
        let snapshot = Self::capture_viewport_snapshot(ctx);
        if self.last_viewport_snapshot.as_ref() == Some(&snapshot) {
            return;
        }
        self.last_viewport_snapshot = Some(snapshot.clone());
        let changed = self.state.update_window_state(
            snapshot.position,
            snapshot.inner_size,
            snapshot.maximized,
            snapshot.fullscreen,
        );
        if changed && !ctx.input(|i| i.pointer.any_down()) {
            self.persist_settings();
        }
    }

    fn capture_viewport_snapshot(ctx: &egui::Context) -> ViewportSnapshot {
        let quantize = |value: f32| (value * 2.0).round() / 2.0;
        ctx.input(|input| {
            let viewport = input.viewport();
            ViewportSnapshot {
                position: viewport
                    .outer_rect
                    .map(|rect| [quantize(rect.min.x), quantize(rect.min.y)]),
                inner_size: viewport
                    .inner_rect
                    .map(|rect| [quantize(rect.width()), quantize(rect.height())]),
                maximized: viewport.maximized,
                fullscreen: viewport.fullscreen,
            }
        })
    }

    fn format_worker_error(kind: WorkerRequestKind, error: &str) -> String {
        match kind {
            WorkerRequestKind::Open => format!("Unable to open image: {error}"),
            WorkerRequestKind::Save => format!("Unable to save image: {error}"),
            WorkerRequestKind::Edit => format!("Unable to apply edit: {error}"),
            WorkerRequestKind::Preload => format!("Background preload skipped: {error}"),
            WorkerRequestKind::Thumbnail => format!("Thumbnail decode failed: {error}"),
            WorkerRequestKind::Batch => format!("Batch convert failed: {error}"),
            WorkerRequestKind::File => format!("File operation failed: {error}"),
            WorkerRequestKind::Print => format!("Print failed: {error}"),
            WorkerRequestKind::Compare => format!("Compare load failed: {error}"),
        }
    }

    fn start_slideshow(&mut self) {
        if !self.state.has_image() {
            self.state
                .set_error("open an image before starting slideshow");
            return;
        }
        self.slideshow_running = true;
        self.slideshow_last_tick = Instant::now();
    }

    fn stop_slideshow(&mut self) {
        self.slideshow_running = false;
    }

    fn run_slideshow_tick(&mut self) {
        if !self.slideshow_running || !self.state.has_image() || self.pending.open_inflight {
            return;
        }

        let interval = Duration::from_secs_f32(self.state.slideshow_interval_secs());
        if self.slideshow_last_tick.elapsed() < interval {
            return;
        }

        self.open_next();
        self.slideshow_last_tick = Instant::now();
    }

    fn should_draw_in_window_menu(&self) -> bool {
        #[cfg(target_os = "macos")]
        {
            return self.native_menu.is_none();
        }

        #[cfg(not(target_os = "macos"))]
        {
            true
        }
    }

    #[cfg(target_os = "macos")]
    fn handle_native_menu_events(&mut self, ctx: &egui::Context) {
        let Some(menu) = self.native_menu.as_ref() else {
            return;
        };

        menu.sync_state(&self.state);
        let actions = menu.drain_actions();
        for action in actions {
            match action {
                NativeMenuAction::About => self.open_about_window(),
                NativeMenuAction::Open => self.open_path_dialog(),
                NativeMenuAction::Save => {
                    self.dispatch_save(None, false, self.default_save_options())
                }
                NativeMenuAction::SaveAs => self.open_save_as_dialog(),
                NativeMenuAction::RenameCurrent => self.open_rename_dialog(),
                NativeMenuAction::CopyCurrentToFolder => self.copy_current_to_dialog(),
                NativeMenuAction::MoveCurrentToFolder => self.move_current_to_dialog(),
                NativeMenuAction::DeleteCurrent => {
                    self.confirm_delete_current = true;
                }
                NativeMenuAction::BatchConvert => self.open_batch_dialog(),
                NativeMenuAction::PrintCurrent => self.dispatch_print_current(),
                NativeMenuAction::LoadCompareImage => self.open_compare_path_dialog(),
                NativeMenuAction::ToggleCompareMode => {
                    self.compare_mode = !self.compare_mode;
                }
                NativeMenuAction::Exit => {
                    ctx.send_viewport_cmd(egui::ViewportCommand::Close);
                }
                NativeMenuAction::RotateLeft => self.dispatch_transform(TransformOp::RotateLeft),
                NativeMenuAction::RotateRight => self.dispatch_transform(TransformOp::RotateRight),
                NativeMenuAction::FlipHorizontal => {
                    self.dispatch_transform(TransformOp::FlipHorizontal);
                }
                NativeMenuAction::FlipVertical => {
                    self.dispatch_transform(TransformOp::FlipVertical);
                }
                NativeMenuAction::Resize => self.open_resize_dialog(),
                NativeMenuAction::Crop => self.open_crop_dialog(),
                NativeMenuAction::ColorCorrections => self.open_color_dialog(),
                NativeMenuAction::ToggleShowToolbar => {
                    self.state.set_show_toolbar(!self.state.show_toolbar());
                    self.persist_settings();
                }
                NativeMenuAction::ToggleShowStatusBar => {
                    self.state
                        .set_show_status_bar(!self.state.show_status_bar());
                    self.persist_settings();
                }
                NativeMenuAction::ToggleShowMetadataPanel => {
                    self.state
                        .set_show_metadata_panel(!self.state.show_metadata_panel());
                    self.persist_settings();
                }
                NativeMenuAction::ToggleThumbnailStrip => {
                    self.state
                        .set_show_thumbnail_strip(!self.state.show_thumbnail_strip());
                    self.persist_settings();
                }
                NativeMenuAction::ToggleThumbnailWindow => {
                    self.state
                        .set_thumbnails_window_mode(!self.state.thumbnails_window_mode());
                    self.persist_settings();
                }
            }
        }
    }

    #[cfg(not(target_os = "macos"))]
    fn handle_native_menu_events(&mut self, _ctx: &egui::Context) {}

    fn draw_menu(&mut self, ctx: &egui::Context) {
        egui::TopBottomPanel::top("menu").show(ctx, |ui| {
            egui::menu::bar(ui, |ui| {
                ui.menu_button("File", |ui| {
                    if ui
                        .button(menu_item_label(ctx, ShortcutAction::Open, "Open..."))
                        .clicked()
                    {
                        self.open_path_dialog();
                        ui.close_menu();
                    }
                    if ui
                        .add_enabled(
                            self.state.has_image(),
                            egui::Button::new(menu_item_label(ctx, ShortcutAction::Save, "Save")),
                        )
                        .clicked()
                    {
                        self.dispatch_save(None, false, self.default_save_options());
                        ui.close_menu();
                    }
                    if ui
                        .add_enabled(
                            self.state.has_image(),
                            egui::Button::new(menu_item_label(
                                ctx,
                                ShortcutAction::SaveAs,
                                "Save As...",
                            )),
                        )
                        .clicked()
                    {
                        self.open_save_as_dialog();
                        ui.close_menu();
                    }
                    if ui
                        .add_enabled(
                            self.state.has_image(),
                            egui::Button::new("Save with options..."),
                        )
                        .clicked()
                    {
                        self.open_save_as_dialog();
                        ui.close_menu();
                    }
                    ui.separator();
                    if ui
                        .add_enabled(
                            self.state.has_image(),
                            egui::Button::new("Rename current..."),
                        )
                        .clicked()
                    {
                        self.open_rename_dialog();
                        ui.close_menu();
                    }
                    if ui
                        .add_enabled(
                            self.state.has_image(),
                            egui::Button::new("Copy current to folder..."),
                        )
                        .clicked()
                    {
                        self.copy_current_to_dialog();
                        ui.close_menu();
                    }
                    if ui
                        .add_enabled(
                            self.state.has_image(),
                            egui::Button::new("Move current to folder..."),
                        )
                        .clicked()
                    {
                        self.move_current_to_dialog();
                        ui.close_menu();
                    }
                    if ui
                        .add_enabled(
                            self.state.has_image(),
                            egui::Button::new("Delete current..."),
                        )
                        .clicked()
                    {
                        self.confirm_delete_current = true;
                        ui.close_menu();
                    }
                    if ui.button("Batch convert / rename...").clicked() {
                        self.open_batch_dialog();
                        ui.close_menu();
                    }
                    if ui
                        .add_enabled(
                            self.state.has_image(),
                            egui::Button::new("Print current..."),
                        )
                        .clicked()
                    {
                        self.dispatch_print_current();
                        ui.close_menu();
                    }
                    ui.separator();
                    ui.menu_button("Recent files", |ui| {
                        let recent_files: Vec<PathBuf> = self
                            .state
                            .recent_files()
                            .iter()
                            .take(RECENT_MENU_LIMIT)
                            .cloned()
                            .collect();
                        if recent_files.is_empty() {
                            ui.label("No recent files");
                            return;
                        }
                        for path in recent_files {
                            let label = format_recent_file_label(&path);
                            let enabled = path.is_file();
                            if ui.add_enabled(enabled, egui::Button::new(label)).clicked() {
                                self.dispatch_open(path, false);
                                ui.close_menu();
                            }
                        }
                    });
                    ui.menu_button("Recent folders", |ui| {
                        let recent_dirs: Vec<PathBuf> = self
                            .state
                            .recent_directories()
                            .iter()
                            .take(RECENT_MENU_LIMIT)
                            .cloned()
                            .collect();
                        if recent_dirs.is_empty() {
                            ui.label("No recent folders");
                            return;
                        }
                        for path in recent_dirs {
                            let label = format_recent_folder_label(&path);
                            let enabled = path.is_dir();
                            if ui.add_enabled(enabled, egui::Button::new(label)).clicked() {
                                self.dispatch_open_directory(path);
                                ui.close_menu();
                            }
                        }
                    });
                    ui.separator();
                    if ui.button("Exit").clicked() {
                        ctx.send_viewport_cmd(egui::ViewportCommand::Close);
                    }
                });

                ui.menu_button("Edit", |ui| {
                    if ui
                        .add_enabled(self.state.has_image(), egui::Button::new("Rotate Left"))
                        .clicked()
                    {
                        self.dispatch_transform(TransformOp::RotateLeft);
                        ui.close_menu();
                    }
                    if ui
                        .add_enabled(self.state.has_image(), egui::Button::new("Rotate Right"))
                        .clicked()
                    {
                        self.dispatch_transform(TransformOp::RotateRight);
                        ui.close_menu();
                    }
                    if ui
                        .add_enabled(self.state.has_image(), egui::Button::new("Flip Horizontal"))
                        .clicked()
                    {
                        self.dispatch_transform(TransformOp::FlipHorizontal);
                        ui.close_menu();
                    }
                    if ui
                        .add_enabled(self.state.has_image(), egui::Button::new("Flip Vertical"))
                        .clicked()
                    {
                        self.dispatch_transform(TransformOp::FlipVertical);
                        ui.close_menu();
                    }
                    ui.separator();
                    if ui
                        .add_enabled(
                            self.state.has_image(),
                            egui::Button::new("Resize / resample..."),
                        )
                        .clicked()
                    {
                        self.open_resize_dialog();
                        ui.close_menu();
                    }
                    if ui
                        .add_enabled(self.state.has_image(), egui::Button::new("Crop..."))
                        .clicked()
                    {
                        self.open_crop_dialog();
                        ui.close_menu();
                    }
                    if ui
                        .add_enabled(
                            self.state.has_image(),
                            egui::Button::new("Color corrections..."),
                        )
                        .clicked()
                    {
                        self.open_color_dialog();
                        ui.close_menu();
                    }
                });

                ui.menu_button("Image", |ui| {
                    if ui
                        .add_enabled(
                            self.state.has_image(),
                            egui::Button::new(menu_item_label(
                                ctx,
                                ShortcutAction::PreviousImage,
                                "Previous image",
                            )),
                        )
                        .clicked()
                    {
                        self.open_previous();
                        ui.close_menu();
                    }
                    if ui
                        .add_enabled(
                            self.state.has_image(),
                            egui::Button::new(menu_item_label(
                                ctx,
                                ShortcutAction::NextImage,
                                "Next image",
                            )),
                        )
                        .clicked()
                    {
                        self.open_next();
                        ui.close_menu();
                    }
                    ui.separator();
                    if ui
                        .add_enabled(
                            self.state.has_image(),
                            egui::Button::new(menu_item_label(
                                ctx,
                                ShortcutAction::ZoomIn,
                                "Zoom in",
                            )),
                        )
                        .clicked()
                    {
                        self.zoom_in();
                        ui.close_menu();
                    }
                    if ui
                        .add_enabled(
                            self.state.has_image(),
                            egui::Button::new(menu_item_label(
                                ctx,
                                ShortcutAction::ZoomOut,
                                "Zoom out",
                            )),
                        )
                        .clicked()
                    {
                        self.zoom_out();
                        ui.close_menu();
                    }
                    if ui
                        .add_enabled(
                            self.state.has_image(),
                            egui::Button::new(menu_item_label(
                                ctx,
                                ShortcutAction::Fit,
                                "Fit to window",
                            )),
                        )
                        .clicked()
                    {
                        self.zoom_fit();
                        ui.close_menu();
                    }
                    if ui
                        .add_enabled(
                            self.state.has_image(),
                            egui::Button::new(menu_item_label(
                                ctx,
                                ShortcutAction::ActualSize,
                                "Actual size",
                            )),
                        )
                        .clicked()
                    {
                        self.zoom_actual();
                        ui.close_menu();
                    }
                    ui.separator();
                    if self.slideshow_running {
                        if ui.button("Stop slideshow    Space").clicked() {
                            self.stop_slideshow();
                            ui.close_menu();
                        }
                    } else if ui
                        .add_enabled(
                            self.state.has_image(),
                            egui::Button::new("Start slideshow    Space"),
                        )
                        .clicked()
                    {
                        self.start_slideshow();
                        ui.close_menu();
                    }
                    let mut interval = self.state.slideshow_interval_secs();
                    if ui
                        .add(
                            egui::Slider::new(&mut interval, 0.5..=30.0)
                                .text("Interval (s)")
                                .fixed_decimals(1),
                        )
                        .changed()
                    {
                        self.state.set_slideshow_interval_secs(interval);
                        self.persist_settings();
                    }
                    ui.separator();
                    if ui
                        .add_enabled(
                            self.state.has_image(),
                            egui::Button::new("Load compare image..."),
                        )
                        .clicked()
                    {
                        self.open_compare_path_dialog();
                        ui.close_menu();
                    }
                    if ui
                        .add_enabled(
                            self.compare_image.is_some(),
                            egui::Button::new("Toggle compare mode"),
                        )
                        .clicked()
                    {
                        self.compare_mode = !self.compare_mode;
                        ui.close_menu();
                    }
                });

                ui.menu_button("View", |ui| {
                    let mut show_status_bar = self.state.show_status_bar();
                    if ui
                        .checkbox(&mut show_status_bar, "Show status bar")
                        .changed()
                    {
                        self.state.set_show_status_bar(show_status_bar);
                        self.persist_settings();
                    }
                    let mut show_toolbar = self.state.show_toolbar();
                    if ui.checkbox(&mut show_toolbar, "Show toolbar").changed() {
                        self.state.set_show_toolbar(show_toolbar);
                        self.persist_settings();
                    }
                    let mut show_metadata_panel = self.state.show_metadata_panel();
                    if ui
                        .checkbox(&mut show_metadata_panel, "Metadata panel")
                        .changed()
                    {
                        self.state.set_show_metadata_panel(show_metadata_panel);
                        self.persist_settings();
                    }
                    let mut show_thumbnail_strip = self.state.show_thumbnail_strip();
                    if ui
                        .checkbox(&mut show_thumbnail_strip, "Thumbnail strip")
                        .changed()
                    {
                        self.state.set_show_thumbnail_strip(show_thumbnail_strip);
                        self.persist_settings();
                    }
                    let mut show_thumbnail_window = self.state.thumbnails_window_mode();
                    if ui
                        .checkbox(&mut show_thumbnail_window, "Thumbnail window")
                        .changed()
                    {
                        self.state.set_thumbnails_window_mode(show_thumbnail_window);
                        self.persist_settings();
                    }
                });

                ui.menu_button("Options", |ui| {
                    if ui.button("Performance / cache...").clicked() {
                        self.open_performance_dialog();
                        ui.close_menu();
                    }
                    if ui.button("Clear runtime caches").clicked() {
                        self.clear_runtime_caches();
                        ui.close_menu();
                    }
                });

                ui.menu_button("Plugins", |ui| {
                    let context = self.plugin_context();
                    self.plugin_host.menu_ui(ui, &context);
                });

                ui.menu_button("Help", |ui| {
                    if ui.button("About ImranView").clicked() {
                        self.open_about_window();
                        ui.close_menu();
                    }
                });
            });
        });
    }

    fn toolbar_icon_button(
        ui: &mut egui::Ui,
        icon: &egui::TextureHandle,
        tooltip: &str,
        enabled: bool,
        selected: bool,
    ) -> egui::Response {
        let icon_size = egui::vec2(TOOLBAR_ICON_SIZE, TOOLBAR_ICON_SIZE);
        let image = egui::Image::new((icon.id(), icon_size));
        let mut button = egui::Button::image(image);
        if selected {
            button = button.fill(egui::Color32::from_rgb(216, 232, 251));
        }
        ui.add_enabled(enabled, button).on_hover_text(tooltip)
    }

    fn draw_toolbar(&mut self, ctx: &egui::Context) {
        if !self.state.show_toolbar() {
            return;
        }

        egui::TopBottomPanel::top("toolbar")
            .exact_height(34.0)
            .show(ctx, |ui| {
                ui.horizontal(|ui| {
                    let has_image = self.state.has_image();

                    if let Some(icons) = self.toolbar_icons.clone() {
                        if Self::toolbar_icon_button(ui, &icons.open, "Open image", true, false)
                            .clicked()
                        {
                            self.open_path_dialog();
                        }
                        if Self::toolbar_icon_button(
                            ui,
                            &icons.prev,
                            "Previous image",
                            has_image,
                            false,
                        )
                        .clicked()
                        {
                            self.open_previous();
                        }
                        if Self::toolbar_icon_button(
                            ui,
                            &icons.next,
                            "Next image",
                            has_image,
                            false,
                        )
                        .clicked()
                        {
                            self.open_next();
                        }

                        ui.separator();

                        if Self::toolbar_icon_button(
                            ui,
                            &icons.zoom_out,
                            "Zoom out",
                            has_image,
                            false,
                        )
                        .clicked()
                        {
                            self.zoom_out();
                        }
                        if Self::toolbar_icon_button(
                            ui,
                            &icons.zoom_in,
                            "Zoom in",
                            has_image,
                            false,
                        )
                        .clicked()
                        {
                            self.zoom_in();
                        }
                        if Self::toolbar_icon_button(
                            ui,
                            &icons.actual_size,
                            "Actual size (1:1)",
                            has_image,
                            !self.state.zoom_is_fit()
                                && (self.state.zoom_factor() - 1.0).abs() < f32::EPSILON,
                        )
                        .clicked()
                        {
                            self.zoom_actual();
                        }
                        if Self::toolbar_icon_button(
                            ui,
                            &icons.fit,
                            "Fit to window",
                            has_image,
                            self.state.zoom_is_fit(),
                        )
                        .clicked()
                        {
                            self.zoom_fit();
                        }

                        ui.separator();

                        if Self::toolbar_icon_button(
                            ui,
                            &icons.gallery,
                            "Toggle thumbnail strip",
                            has_image,
                            self.state.show_thumbnail_strip()
                                && !self.state.thumbnails_window_mode(),
                        )
                        .clicked()
                        {
                            self.state.toggle_thumbnail_strip();
                            self.persist_settings();
                        }
                        if Self::toolbar_icon_button(
                            ui,
                            &icons.gallery,
                            "Toggle thumbnail window",
                            has_image,
                            self.state.thumbnails_window_mode(),
                        )
                        .clicked()
                        {
                            self.state.toggle_thumbnails_window_mode();
                            self.persist_settings();
                        }
                    } else {
                        if ui.button("Open").clicked() {
                            self.open_path_dialog();
                        }
                        if ui
                            .add_enabled(self.state.has_image(), egui::Button::new("Prev"))
                            .clicked()
                        {
                            self.open_previous();
                        }
                        if ui
                            .add_enabled(self.state.has_image(), egui::Button::new("Next"))
                            .clicked()
                        {
                            self.open_next();
                        }

                        ui.separator();

                        if ui
                            .add_enabled(self.state.has_image(), egui::Button::new("-"))
                            .clicked()
                        {
                            self.zoom_out();
                        }
                        if ui
                            .add_enabled(self.state.has_image(), egui::Button::new("+"))
                            .clicked()
                        {
                            self.zoom_in();
                        }
                        if ui
                            .add_enabled(self.state.has_image(), egui::Button::new("1:1"))
                            .clicked()
                        {
                            self.zoom_actual();
                        }
                        if ui
                            .add_enabled(self.state.has_image(), egui::Button::new("Fit"))
                            .clicked()
                        {
                            self.zoom_fit();
                        }

                        ui.separator();

                        if ui.button("Gallery").clicked() {
                            self.state.toggle_thumbnail_strip();
                            self.persist_settings();
                        }
                        if ui.button("Thumb Window").clicked() {
                            self.state.toggle_thumbnails_window_mode();
                            self.persist_settings();
                        }
                    }

                    ui.separator();
                    ui.label(self.state.image_counter_label());
                    ui.label(self.state.zoom_label());
                    if self.slideshow_running {
                        ui.label("Slideshow");
                    }
                });
            });
    }

    fn draw_thumbnail_strip(&mut self, ctx: &egui::Context) {
        if !self.state.show_thumbnail_strip() || self.state.thumbnails_window_mode() {
            return;
        }

        let entries = self.state.thumbnail_entries();
        if self.last_logged_thumb_entry_count != Some(entries.len()) {
            log::debug!(
                target: "imranview::thumb",
                "thumbnail strip entries={} cache_size={} cache_bytes={} inflight={}",
                entries.len(),
                self.thumb_cache.map.len(),
                self.thumb_cache.total_bytes,
                self.inflight_thumbnails.len()
            );
            self.last_logged_thumb_entry_count = Some(entries.len());
        }

        egui::TopBottomPanel::bottom("thumbnail-strip")
            .resizable(true)
            .min_height(112.0)
            .default_height(146.0)
            .show(ctx, |ui| {
                egui::ScrollArea::horizontal().show(ui, |ui| {
                    ui.horizontal(|ui| {
                        for entry in entries {
                            self.draw_thumbnail_card(ui, &entry, false, THUMB_CARD_WIDTH);
                        }
                    });
                });
            });
    }

    fn draw_thumbnail_window(&mut self, ctx: &egui::Context) {
        self.ensure_folder_panel_cache();
        let current_directory = self.folder_panel_cache.current_directory.clone();
        let ancestors = self.folder_panel_cache.ancestors.clone();
        let siblings = self.folder_panel_cache.siblings.clone();
        let children = self.folder_panel_cache.children.clone();

        let side_panel = egui::SidePanel::left("thumb-window-folders")
            .resizable(true)
            .default_width(self.state.thumbnail_sidebar_width())
            .width_range(160.0..=420.0)
            .show(ctx, |ui| {
                ui.horizontal(|ui| {
                    ui.heading("Folders");
                    if ui.small_button("Refresh").clicked() {
                        self.clear_folder_panel_cache();
                    }
                });
                ui.separator();

                let Some(current_directory) = current_directory.as_ref() else {
                    ui.label("Open an image to browse folders.");
                    return;
                };

                egui::ScrollArea::vertical().show(ui, |ui| {
                    ui.label("Current path");
                    for path in &ancestors {
                        let is_current = path == current_directory;
                        let label = path
                            .file_name()
                            .map(|name| name.to_string_lossy().into_owned())
                            .unwrap_or_else(|| path.display().to_string());
                        if ui.selectable_label(is_current, label).clicked() {
                            self.open_directory_from_panel(path.clone());
                        }
                    }
                    ui.separator();

                    ui.label("Sibling folders");
                    if siblings.is_empty() {
                        ui.label("No sibling folders");
                    }
                    for path in &siblings {
                        let is_current = path == current_directory;
                        let label = path
                            .file_name()
                            .map(|name| name.to_string_lossy().into_owned())
                            .unwrap_or_else(|| path.display().to_string());
                        if ui.selectable_label(is_current, label).clicked() {
                            self.open_directory_from_panel(path.clone());
                        }
                    }
                    ui.separator();

                    ui.label("Subfolders");
                    if children.is_empty() {
                        ui.label("No subfolders");
                    }
                    for path in &children {
                        let label = path
                            .file_name()
                            .map(|name| name.to_string_lossy().into_owned())
                            .unwrap_or_else(|| path.display().to_string());
                        if ui.selectable_label(false, label).clicked() {
                            self.open_directory_from_panel(path.clone());
                        }
                    }
                });
            });

        let panel_width = side_panel.response.rect.width();
        if (panel_width - self.state.thumbnail_sidebar_width()).abs() > 0.5 {
            self.state.set_thumbnail_sidebar_width(panel_width);
            if !ctx.input(|i| i.pointer.any_down()) {
                self.persist_settings();
            }
        }

        egui::CentralPanel::default().show(ctx, |ui| {
            ui.horizontal_wrapped(|ui| {
                ui.heading("Thumbnails");
                ui.separator();
                ui.label(self.state.folder_label());
            });
            ui.add_space(4.0);
            let mut card_width = self.state.thumbnail_grid_card_width();
            if ui
                .add(egui::Slider::new(&mut card_width, 96.0..=240.0).text("Thumbnail size"))
                .changed()
            {
                self.state.set_thumbnail_grid_card_width(card_width);
                self.persist_settings();
            }
            ui.separator();

            let entries = self.state.thumbnail_entries();
            if entries.is_empty() {
                ui.label("No images in this folder.");
                return;
            }

            egui::ScrollArea::vertical().show(ui, |ui| {
                let spacing = ui.spacing().item_spacing.x.max(6.0);
                let usable_width = ui.available_width().max(card_width);
                let columns = ((usable_width + spacing) / (card_width + spacing))
                    .floor()
                    .max(1.0) as usize;

                for row in entries.chunks(columns) {
                    ui.horizontal_top(|ui| {
                        for entry in row {
                            ui.allocate_ui_with_layout(
                                egui::vec2(card_width, THUMB_CARD_HEIGHT + 56.0),
                                egui::Layout::top_down(egui::Align::Center),
                                |ui| self.draw_thumbnail_card(ui, entry, false, card_width),
                            );
                        }
                    });
                    ui.add_space(6.0);
                }
            });
        });
    }

    fn draw_thumbnail_card(
        &mut self,
        ui: &mut egui::Ui,
        entry: &ThumbnailEntry,
        row_mode: bool,
        card_width: f32,
    ) {
        let mut frame = egui::Frame::group(ui.style());
        if entry.current {
            frame = frame.fill(egui::Color32::from_rgb(216, 232, 251));
        }

        let response = frame.show(ui, |ui| {
            if row_mode {
                ui.horizontal(|ui| {
                    self.draw_thumbnail_image(ui, entry, THUMB_CARD_WIDTH);
                    ui.vertical(|ui| {
                        ui.label(&entry.label);
                        if entry.current {
                            ui.label("Current image");
                        }
                    });
                });
            } else {
                ui.set_width(card_width);
                self.draw_thumbnail_image(ui, entry, card_width);
                ui.label(egui::RichText::new(&entry.label).small());
            }

            if ui.button("Open").clicked() {
                self.dispatch_open(entry.path.clone(), false);
            }
        });

        if entry.current && self.scroll_thumbnail_to_current && !row_mode {
            response.response.scroll_to_me(Some(egui::Align::Center));
            self.scroll_thumbnail_to_current = false;
        }

        if self.thumb_cache.get(&entry.path).is_none()
            && (entry.decode_hint
                || response.response.rect.is_positive()
                    && ui.is_rect_visible(response.response.rect))
        {
            self.request_thumbnail_decode(entry.path.clone());
        }
    }

    fn draw_thumbnail_image(&mut self, ui: &mut egui::Ui, entry: &ThumbnailEntry, card_width: f32) {
        let max_size = egui::vec2(card_width.max(36.0) - 8.0, THUMB_CARD_HEIGHT - 8.0);
        if let Some(texture) = self.thumb_cache.get(&entry.path) {
            let [tex_w, tex_h] = texture.size();
            let tex_w = tex_w as f32;
            let tex_h = tex_h as f32;
            let scale = if tex_w > 0.0 && tex_h > 0.0 {
                (max_size.x / tex_w).min(max_size.y / tex_h).max(0.01)
            } else {
                1.0
            };
            let image_size = egui::vec2((tex_w * scale).max(1.0), (tex_h * scale).max(1.0));

            ui.allocate_ui(max_size, |ui| {
                ui.centered_and_justified(|ui| {
                    ui.add(egui::Image::new((texture.id(), image_size)));
                });
            });
        } else {
            ui.allocate_ui(max_size, |ui| {
                ui.centered_and_justified(|ui| {
                    ui.label("...");
                });
            });
        }
    }

    fn draw_main_viewer(&mut self, ctx: &egui::Context) {
        egui::CentralPanel::default().show(ctx, |ui| {
            if self.compare_mode {
                if let (Some(primary), Some(compare)) = (&self.main_texture, &self.compare_image) {
                    ui.columns(2, |columns| {
                        columns[0].heading("Primary");
                        columns[0].small(
                            self.state
                                .current_file_path_ref()
                                .map(|path| path.display().to_string())
                                .unwrap_or_else(|| "-".to_owned()),
                        );
                        columns[0].separator();

                        columns[1].heading("Compare");
                        columns[1].small(compare.path.display().to_string());
                        if let Some((_, model)) = compare
                            .metadata
                            .exif_fields
                            .iter()
                            .find(|(key, _)| key == "Model")
                        {
                            columns[1].small(format!("Camera: {model}"));
                        }
                        columns[1].separator();

                        if self.state.zoom_is_fit() {
                            let available_left = columns[0].available_size();
                            let available_right = columns[1].available_size();
                            let base_left =
                                egui::vec2(self.state.image_width(), self.state.image_height());
                            let base_right =
                                egui::vec2(compare.width as f32, compare.height as f32);

                            let scale_left = if base_left.x > 0.0 && base_left.y > 0.0 {
                                (available_left.x / base_left.x)
                                    .min(available_left.y / base_left.y)
                                    .max(0.01)
                            } else {
                                1.0
                            };
                            let scale_right = if base_right.x > 0.0 && base_right.y > 0.0 {
                                (available_right.x / base_right.x)
                                    .min(available_right.y / base_right.y)
                                    .max(0.01)
                            } else {
                                1.0
                            };

                            let left_size = base_left * scale_left;
                            let right_size = base_right * scale_right;

                            columns[0].centered_and_justified(|ui| {
                                ui.add(egui::Image::new((primary.id(), left_size)));
                            });
                            columns[1].centered_and_justified(|ui| {
                                ui.add(egui::Image::new((compare.texture.id(), right_size)));
                            });
                        } else {
                            let zoom = self.state.zoom_factor();
                            let left_size =
                                egui::vec2(self.state.image_width(), self.state.image_height())
                                    * zoom;
                            let right_size =
                                egui::vec2(compare.width as f32, compare.height as f32) * zoom;
                            egui::ScrollArea::both()
                                .id_salt("compare-scroll-left")
                                .show(&mut columns[0], |ui| {
                                    ui.add(egui::Image::new((primary.id(), left_size)));
                                });
                            egui::ScrollArea::both()
                                .id_salt("compare-scroll-right")
                                .show(&mut columns[1], |ui| {
                                    ui.add(egui::Image::new((compare.texture.id(), right_size)));
                                });
                        }
                    });
                } else {
                    ui.centered_and_justified(|ui| {
                        ui.label("Load a compare image from Image > Load compare image...");
                    });
                }
            } else if let Some(texture) = &self.main_texture {
                let mut desired_size =
                    egui::vec2(self.state.image_width(), self.state.image_height());

                if self.state.zoom_is_fit() {
                    self.main_scroll_offset = egui::Vec2::ZERO;
                    let available = ui.available_size();
                    self.main_viewport_size = available;
                    let fit_scale = if desired_size.x > 0.0 && desired_size.y > 0.0 {
                        (available.x / desired_size.x)
                            .min(available.y / desired_size.y)
                            .max(0.01)
                    } else {
                        1.0
                    };
                    desired_size *= fit_scale;
                    ui.centered_and_justified(|ui| {
                        ui.add(egui::Image::new((texture.id(), desired_size)));
                    });
                } else {
                    desired_size *= self.state.zoom_factor();
                    let output = egui::ScrollArea::both()
                        .id_salt("main-viewer-scroll")
                        .scroll_offset(self.main_scroll_offset)
                        .show(ui, |ui| {
                            ui.add(egui::Image::new((texture.id(), desired_size)));
                        });
                    self.main_scroll_offset = output.state.offset;
                    self.main_viewport_size = output.inner_rect.size();
                }
            } else {
                ui.centered_and_justified(|ui| {
                    ui.label("ImranView\n\nFile > Open...");
                });
            }
        });
    }

    fn draw_about_window(&mut self, ctx: &egui::Context) {
        if !self.show_about_window {
            return;
        }

        let mut open = self.show_about_window;
        let mut window = egui::Window::new("About ImranView")
            .open(&mut open)
            .collapsible(false)
            .resizable(false);

        if self.center_about_window_next_frame {
            window = window.anchor(egui::Align2::CENTER_CENTER, egui::Vec2::ZERO);
            self.center_about_window_next_frame = false;
        }

        #[cfg(target_os = "macos")]
        {
            // Keep the popup frame subtle on macOS to avoid heavy non-native looking borders.
            let frame = egui::Frame::window(&ctx.style())
                .stroke(egui::Stroke::new(0.5, egui::Color32::from_gray(110)));
            window = window.frame(frame);
        }

        window.show(ctx, |ui| {
            ui.horizontal(|ui| {
                if let Some(icon) = &self.about_icon_texture {
                    ui.add(egui::Image::new((icon.id(), egui::vec2(64.0, 64.0))));
                }
                ui.vertical(|ui| {
                    ui.heading("ImranView");
                    ui.label("Imran, brother of Irfan");
                    ui.label(format!("Version {}", env!("CARGO_PKG_VERSION")));
                });
            });
            ui.separator();
            ui.horizontal_wrapped(|ui| {
                ui.label("Twitter:");
                ui.hyperlink_to("@stonecharioteer", "https://twitter.com/stonecharioteer");
            });
            ui.horizontal_wrapped(|ui| {
                ui.label("Website:");
                ui.hyperlink_to(
                    "tech.stonecharioteer.com",
                    "https://tech.stonecharioteer.com",
                );
            });
            ui.horizontal_wrapped(|ui| {
                ui.label("Source:");
                ui.hyperlink_to(
                    "github.com/stonecharioteer/imranview",
                    "https://github.com/stonecharioteer/imranview",
                );
            });
        });

        self.show_about_window = open;
    }

    fn draw_error_banner(&mut self, ctx: &egui::Context) {
        let Some(message) = self.state.error_message().map(str::to_owned) else {
            return;
        };

        egui::TopBottomPanel::top("error-banner")
            .exact_height(30.0)
            .show(ctx, |ui| {
                egui::Frame::new()
                    .fill(egui::Color32::from_rgb(71, 18, 18))
                    .show(ui, |ui| {
                        ui.horizontal(|ui| {
                            ui.colored_label(egui::Color32::from_rgb(255, 214, 214), message);
                            ui.with_layout(
                                egui::Layout::right_to_left(egui::Align::Center),
                                |ui| {
                                    if ui.small_button("Dismiss").clicked() {
                                        self.state.clear_error();
                                    }
                                    if ui.small_button("Open...").clicked() {
                                        self.open_path_dialog();
                                    }
                                },
                            );
                        });
                    });
            });
    }

    fn draw_info_banner(&mut self, ctx: &egui::Context) {
        let Some(message) = self.info_message.clone() else {
            return;
        };

        egui::TopBottomPanel::top("info-banner")
            .exact_height(28.0)
            .show(ctx, |ui| {
                egui::Frame::new()
                    .fill(egui::Color32::from_rgb(20, 49, 28))
                    .show(ui, |ui| {
                        ui.horizontal(|ui| {
                            ui.colored_label(egui::Color32::from_rgb(219, 255, 227), message);
                            ui.with_layout(
                                egui::Layout::right_to_left(egui::Align::Center),
                                |ui| {
                                    if ui.small_button("Dismiss").clicked() {
                                        self.info_message = None;
                                    }
                                },
                            );
                        });
                    });
            });
    }

    fn draw_resize_dialog(&mut self, ctx: &egui::Context) {
        if !self.resize_dialog.open {
            return;
        }

        let mut open = self.resize_dialog.open;
        egui::Window::new("Resize / Resample")
            .open(&mut open)
            .collapsible(false)
            .resizable(false)
            .show(ctx, |ui| {
                let aspect = if let Some((width, height)) = self.state.original_dimensions() {
                    if height > 0 {
                        width as f32 / height as f32
                    } else {
                        1.0
                    }
                } else {
                    1.0
                };

                let before_width = self.resize_dialog.width;
                let before_height = self.resize_dialog.height;
                ui.horizontal(|ui| {
                    ui.label("Width:");
                    ui.add(egui::DragValue::new(&mut self.resize_dialog.width).range(1..=65535));
                    ui.label("Height:");
                    ui.add(egui::DragValue::new(&mut self.resize_dialog.height).range(1..=65535));
                });
                ui.checkbox(&mut self.resize_dialog.keep_aspect, "Keep aspect ratio");
                if self.resize_dialog.keep_aspect {
                    if self.resize_dialog.width != before_width && aspect > 0.0 {
                        self.resize_dialog.height =
                            ((self.resize_dialog.width as f32 / aspect).round().max(1.0)) as u32;
                    } else if self.resize_dialog.height != before_height && aspect > 0.0 {
                        self.resize_dialog.width =
                            ((self.resize_dialog.height as f32 * aspect).round().max(1.0)) as u32;
                    }
                }

                ui.separator();
                ui.label("Filter:");
                ui.horizontal_wrapped(|ui| {
                    ui.selectable_value(
                        &mut self.resize_dialog.filter,
                        ResizeFilter::Nearest,
                        "Nearest",
                    );
                    ui.selectable_value(
                        &mut self.resize_dialog.filter,
                        ResizeFilter::Triangle,
                        "Triangle",
                    );
                    ui.selectable_value(
                        &mut self.resize_dialog.filter,
                        ResizeFilter::CatmullRom,
                        "CatmullRom",
                    );
                    ui.selectable_value(
                        &mut self.resize_dialog.filter,
                        ResizeFilter::Gaussian,
                        "Gaussian",
                    );
                    ui.selectable_value(
                        &mut self.resize_dialog.filter,
                        ResizeFilter::Lanczos3,
                        "Lanczos3",
                    );
                });

                ui.separator();
                if ui.button("Apply").clicked() {
                    self.dispatch_transform(TransformOp::Resize {
                        width: self.resize_dialog.width.max(1),
                        height: self.resize_dialog.height.max(1),
                        filter: self.resize_dialog.filter,
                    });
                    self.resize_dialog.open = false;
                }
            });
        self.resize_dialog.open = open;
    }

    fn draw_crop_dialog(&mut self, ctx: &egui::Context) {
        if !self.crop_dialog.open {
            return;
        }

        let mut open = self.crop_dialog.open;
        egui::Window::new("Crop")
            .open(&mut open)
            .collapsible(false)
            .resizable(false)
            .show(ctx, |ui| {
                ui.horizontal(|ui| {
                    ui.label("X:");
                    ui.add(egui::DragValue::new(&mut self.crop_dialog.x).range(0..=65535));
                    ui.label("Y:");
                    ui.add(egui::DragValue::new(&mut self.crop_dialog.y).range(0..=65535));
                });
                ui.horizontal(|ui| {
                    ui.label("Width:");
                    ui.add(egui::DragValue::new(&mut self.crop_dialog.width).range(1..=65535));
                    ui.label("Height:");
                    ui.add(egui::DragValue::new(&mut self.crop_dialog.height).range(1..=65535));
                });

                if ui.button("Apply").clicked() {
                    self.dispatch_transform(TransformOp::Crop {
                        x: self.crop_dialog.x,
                        y: self.crop_dialog.y,
                        width: self.crop_dialog.width.max(1),
                        height: self.crop_dialog.height.max(1),
                    });
                    self.crop_dialog.open = false;
                }
            });
        self.crop_dialog.open = open;
    }

    fn draw_color_dialog(&mut self, ctx: &egui::Context) {
        if !self.color_dialog.open {
            return;
        }

        let mut open = self.color_dialog.open;
        egui::Window::new("Color Corrections")
            .open(&mut open)
            .collapsible(false)
            .resizable(false)
            .show(ctx, |ui| {
                ui.add(
                    egui::Slider::new(&mut self.color_dialog.brightness, -255..=255)
                        .text("Brightness"),
                );
                ui.add(
                    egui::Slider::new(&mut self.color_dialog.contrast, -100.0..=100.0)
                        .text("Contrast"),
                );
                ui.add(
                    egui::Slider::new(&mut self.color_dialog.gamma, 0.1..=5.0)
                        .text("Gamma")
                        .fixed_decimals(2),
                );
                ui.add(
                    egui::Slider::new(&mut self.color_dialog.saturation, 0.0..=3.0)
                        .text("Saturation")
                        .fixed_decimals(2),
                );
                ui.checkbox(&mut self.color_dialog.grayscale, "Grayscale");

                if ui.button("Apply").clicked() {
                    self.dispatch_transform(TransformOp::ColorAdjust(ColorAdjustParams {
                        brightness: self.color_dialog.brightness,
                        contrast: self.color_dialog.contrast,
                        gamma: self.color_dialog.gamma,
                        saturation: self.color_dialog.saturation,
                        grayscale: self.color_dialog.grayscale,
                    }));
                    self.color_dialog.open = false;
                }
            });
        self.color_dialog.open = open;
    }

    fn draw_batch_dialog(&mut self, ctx: &egui::Context) {
        if !self.batch_dialog.open {
            return;
        }

        let mut open = self.batch_dialog.open;
        egui::Window::new("Batch Convert / Rename")
            .open(&mut open)
            .collapsible(false)
            .resizable(false)
            .show(ctx, |ui| {
                ui.label("Input directory");
                ui.horizontal(|ui| {
                    ui.text_edit_singleline(&mut self.batch_dialog.input_dir);
                    if ui.button("Pick...").clicked() {
                        let dialog = rfd::FileDialog::new().set_title("Batch input directory");
                        if let Some(path) = dialog.pick_folder() {
                            self.batch_dialog.input_dir = path.display().to_string();
                        }
                    }
                });

                ui.label("Output directory");
                ui.horizontal(|ui| {
                    ui.text_edit_singleline(&mut self.batch_dialog.output_dir);
                    if ui.button("Pick...").clicked() {
                        let dialog = rfd::FileDialog::new().set_title("Batch output directory");
                        if let Some(path) = dialog.pick_folder() {
                            self.batch_dialog.output_dir = path.display().to_string();
                        }
                    }
                });

                ui.separator();
                ui.label("Output format");
                ui.horizontal_wrapped(|ui| {
                    ui.selectable_value(
                        &mut self.batch_dialog.output_format,
                        BatchOutputFormat::Jpeg,
                        "JPEG",
                    );
                    ui.selectable_value(
                        &mut self.batch_dialog.output_format,
                        BatchOutputFormat::Png,
                        "PNG",
                    );
                    ui.selectable_value(
                        &mut self.batch_dialog.output_format,
                        BatchOutputFormat::Webp,
                        "WEBP",
                    );
                    ui.selectable_value(
                        &mut self.batch_dialog.output_format,
                        BatchOutputFormat::Bmp,
                        "BMP",
                    );
                    ui.selectable_value(
                        &mut self.batch_dialog.output_format,
                        BatchOutputFormat::Tiff,
                        "TIFF",
                    );
                });
                if matches!(self.batch_dialog.output_format, BatchOutputFormat::Jpeg) {
                    ui.add(
                        egui::Slider::new(&mut self.batch_dialog.jpeg_quality, 1..=100)
                            .text("JPEG quality"),
                    );
                }

                ui.horizontal(|ui| {
                    ui.label("Rename prefix");
                    ui.text_edit_singleline(&mut self.batch_dialog.rename_prefix);
                });
                ui.horizontal(|ui| {
                    ui.label("Start index");
                    ui.add(
                        egui::DragValue::new(&mut self.batch_dialog.start_index).range(0..=999999),
                    );
                });

                ui.separator();
                if ui.button("Preview summary").clicked() {
                    let input_dir = PathBuf::from(self.batch_dialog.input_dir.trim());
                    if input_dir.as_os_str().is_empty() {
                        self.batch_dialog.preview_error =
                            Some("input directory is required for preview".to_owned());
                        self.batch_dialog.preview_count = None;
                    } else {
                        match collect_images_in_directory(&input_dir) {
                            Ok(files) => {
                                self.batch_dialog.preview_count = Some(files.len());
                                self.batch_dialog.preview_for_input =
                                    self.batch_dialog.input_dir.trim().to_owned();
                                self.batch_dialog.preview_error = None;
                            }
                            Err(err) => {
                                self.batch_dialog.preview_count = None;
                                self.batch_dialog.preview_for_input.clear();
                                self.batch_dialog.preview_error = Some(err.to_string());
                            }
                        }
                    }
                }

                if let Some(error) = &self.batch_dialog.preview_error {
                    ui.colored_label(egui::Color32::from_rgb(255, 190, 190), error);
                } else if let Some(count) = self.batch_dialog.preview_count {
                    ui.label(format!(
                        "Preview: {} image(s), output format {:?}, start index {}",
                        count, self.batch_dialog.output_format, self.batch_dialog.start_index
                    ));
                } else {
                    ui.label("Preview required before running batch.");
                }

                let preview_ready = self.batch_dialog.preview_count.is_some()
                    && self.batch_dialog.preview_for_input == self.batch_dialog.input_dir.trim();
                if ui
                    .add_enabled(preview_ready, egui::Button::new("Run batch"))
                    .clicked()
                {
                    let input_dir = PathBuf::from(self.batch_dialog.input_dir.trim());
                    let output_dir = PathBuf::from(self.batch_dialog.output_dir.trim());
                    if input_dir.as_os_str().is_empty() || output_dir.as_os_str().is_empty() {
                        self.state
                            .set_error("input and output directories are required");
                    } else {
                        self.dispatch_batch_convert(BatchConvertOptions {
                            input_dir,
                            output_dir,
                            output_format: self.batch_dialog.output_format,
                            rename_prefix: self.batch_dialog.rename_prefix.clone(),
                            start_index: self.batch_dialog.start_index,
                            jpeg_quality: self.batch_dialog.jpeg_quality,
                        });
                        self.batch_dialog.open = false;
                    }
                }
            });
        self.batch_dialog.open = open;
    }

    fn draw_save_dialog(&mut self, ctx: &egui::Context) {
        if !self.save_dialog.open {
            return;
        }

        let mut open = self.save_dialog.open;
        egui::Window::new("Save Image")
            .open(&mut open)
            .collapsible(false)
            .resizable(false)
            .show(ctx, |ui| {
                ui.label("Output path");
                ui.horizontal(|ui| {
                    ui.text_edit_singleline(&mut self.save_dialog.path);
                    if ui.button("Pick...").clicked() {
                        let mut dialog = rfd::FileDialog::new().set_title("Save image as");
                        if let Some(directory) = self.state.preferred_open_directory() {
                            dialog = dialog.set_directory(directory);
                        }
                        if let Some(file_name) = self.state.suggested_save_name() {
                            dialog = dialog.set_file_name(file_name);
                        }
                        if let Some(path) = dialog.save_file() {
                            self.save_dialog.path = path.display().to_string();
                        }
                    }
                });

                ui.separator();
                ui.label("Format");
                ui.horizontal_wrapped(|ui| {
                    ui.selectable_value(
                        &mut self.save_dialog.output_format,
                        SaveOutputFormat::Auto,
                        "Auto (from extension)",
                    );
                    ui.selectable_value(
                        &mut self.save_dialog.output_format,
                        SaveOutputFormat::Jpeg,
                        "JPEG",
                    );
                    ui.selectable_value(
                        &mut self.save_dialog.output_format,
                        SaveOutputFormat::Png,
                        "PNG",
                    );
                    ui.selectable_value(
                        &mut self.save_dialog.output_format,
                        SaveOutputFormat::Webp,
                        "WEBP",
                    );
                    ui.selectable_value(
                        &mut self.save_dialog.output_format,
                        SaveOutputFormat::Bmp,
                        "BMP",
                    );
                    ui.selectable_value(
                        &mut self.save_dialog.output_format,
                        SaveOutputFormat::Tiff,
                        "TIFF",
                    );
                });
                if matches!(self.save_dialog.output_format, SaveOutputFormat::Jpeg) {
                    ui.add(
                        egui::Slider::new(&mut self.save_dialog.jpeg_quality, 1..=100)
                            .text("JPEG quality"),
                    );
                }

                ui.separator();
                ui.label("Metadata policy");
                ui.radio_value(
                    &mut self.save_dialog.metadata_policy,
                    SaveMetadataPolicy::PreserveIfPossible,
                    "Preserve if possible",
                );
                ui.radio_value(
                    &mut self.save_dialog.metadata_policy,
                    SaveMetadataPolicy::Strip,
                    "Strip metadata",
                );
                if matches!(
                    self.save_dialog.metadata_policy,
                    SaveMetadataPolicy::PreserveIfPossible
                ) {
                    ui.small("Current best-effort preservation supports JPEG output.");
                }

                ui.separator();
                if ui.button("Save").clicked() {
                    let path = PathBuf::from(self.save_dialog.path.trim());
                    if path.as_os_str().is_empty() {
                        self.state.set_error("save path is required");
                    } else {
                        let options = self.build_save_options_from_dialog();
                        self.dispatch_save(Some(path), self.save_dialog.reopen_after_save, options);
                        self.save_dialog.open = false;
                    }
                }
            });
        self.save_dialog.open = open;
    }

    fn draw_performance_dialog(&mut self, ctx: &egui::Context) {
        if !self.performance_dialog.open {
            return;
        }

        let mut open = self.performance_dialog.open;
        egui::Window::new("Performance / Cache Settings")
            .open(&mut open)
            .collapsible(false)
            .resizable(false)
            .show(ctx, |ui| {
                ui.label("Thumbnail texture cache");
                ui.horizontal(|ui| {
                    ui.label("Entry cap");
                    ui.add(
                        egui::DragValue::new(&mut self.performance_dialog.thumb_cache_entry_cap)
                            .range(64..=4096),
                    );
                });
                ui.horizontal(|ui| {
                    ui.label("Memory cap (MB)");
                    ui.add(
                        egui::DragValue::new(&mut self.performance_dialog.thumb_cache_max_mb)
                            .range(16..=1024),
                    );
                });

                ui.separator();
                ui.label("Preload cache");
                ui.horizontal(|ui| {
                    ui.label("Entry cap");
                    ui.add(
                        egui::DragValue::new(&mut self.performance_dialog.preload_cache_entry_cap)
                            .range(1..=64),
                    );
                });
                ui.horizontal(|ui| {
                    ui.label("Memory cap (MB)");
                    ui.add(
                        egui::DragValue::new(&mut self.performance_dialog.preload_cache_max_mb)
                            .range(32..=2048),
                    );
                });

                ui.separator();
                if ui.button("Apply").clicked() {
                    self.apply_performance_settings();
                    self.performance_dialog.open = false;
                }
            });
        self.performance_dialog.open = open;
    }

    fn draw_rename_dialog(&mut self, ctx: &egui::Context) {
        if !self.rename_dialog.open {
            return;
        }

        let mut open = self.rename_dialog.open;
        egui::Window::new("Rename Current File")
            .open(&mut open)
            .collapsible(false)
            .resizable(false)
            .show(ctx, |ui| {
                ui.label("New file name");
                ui.text_edit_singleline(&mut self.rename_dialog.new_name);
                if ui.button("Rename").clicked() {
                    if let Some(from) = self.rename_dialog.target_path.clone() {
                        if let Some(parent) = from.parent() {
                            let to = parent.join(self.rename_dialog.new_name.trim());
                            self.dispatch_file_operation(FileOperation::Rename { from, to });
                            self.rename_dialog.open = false;
                        }
                    }
                }
            });
        self.rename_dialog.open = open;
    }

    fn draw_delete_confirmation(&mut self, ctx: &egui::Context) {
        if !self.confirm_delete_current {
            return;
        }

        let mut open = self.confirm_delete_current;
        egui::Window::new("Delete Current File")
            .open(&mut open)
            .collapsible(false)
            .resizable(false)
            .show(ctx, |ui| {
                ui.label("Delete the current image from disk?");
                ui.horizontal(|ui| {
                    if ui.button("Delete").clicked() {
                        self.delete_current_file();
                        self.confirm_delete_current = false;
                    }
                    if ui.button("Cancel").clicked() {
                        self.confirm_delete_current = false;
                    }
                });
            });
        self.confirm_delete_current = open;
    }

    fn draw_metadata_panel(&mut self, ctx: &egui::Context) {
        if !self.state.show_metadata_panel() {
            return;
        }

        egui::SidePanel::right("metadata-panel")
            .resizable(true)
            .default_width(270.0)
            .width_range(220.0..=460.0)
            .show(ctx, |ui| {
                ui.horizontal(|ui| {
                    ui.heading("Metadata");
                    if self.slideshow_running {
                        ui.separator();
                        ui.label("Slideshow: running");
                    }
                });
                ui.separator();

                let Some(path) = self.state.current_file_path_ref() else {
                    ui.label("Open an image to inspect metadata.");
                    return;
                };

                let original = self
                    .state
                    .original_dimensions()
                    .map(|(w, h)| format!("{w} x {h}"))
                    .unwrap_or_else(|| "-".to_owned());
                let preview = self
                    .state
                    .preview_dimensions()
                    .map(|(w, h)| format!("{w} x {h}"))
                    .unwrap_or_else(|| "-".to_owned());
                let preview_mode = if self.state.downscaled_for_preview() {
                    "Downscaled preview"
                } else {
                    "Original pixels"
                };
                let file_size = fs::metadata(path)
                    .ok()
                    .map(|meta| human_file_size(meta.len()))
                    .unwrap_or_else(|| "-".to_owned());
                let modified = fs::metadata(path)
                    .ok()
                    .and_then(|meta| meta.modified().ok())
                    .map(format_system_time)
                    .unwrap_or_else(|| "-".to_owned());

                egui::Grid::new("metadata-grid")
                    .spacing(egui::vec2(12.0, 6.0))
                    .show(ui, |ui| {
                        ui.label("File");
                        ui.label(path.display().to_string());
                        ui.end_row();

                        ui.label("Folder");
                        ui.label(
                            self.state
                                .current_directory_path()
                                .map(|directory| directory.display().to_string())
                                .unwrap_or_else(|| "-".to_owned()),
                        );
                        ui.end_row();

                        ui.label("Original");
                        ui.label(original);
                        ui.end_row();

                        ui.label("Preview");
                        ui.label(preview);
                        ui.end_row();

                        ui.label("Preview mode");
                        ui.label(preview_mode);
                        ui.end_row();

                        ui.label("Zoom");
                        ui.label(self.state.zoom_label());
                        ui.end_row();

                        ui.label("File size");
                        ui.label(file_size);
                        ui.end_row();

                        ui.label("Modified");
                        ui.label(modified);
                        ui.end_row();
                    });

                if let Some(metadata) = &self.current_metadata {
                    ui.separator();
                    if metadata.exif_fields.is_empty()
                        && metadata.iptc_fields.is_empty()
                        && metadata.xmp_fields.is_empty()
                    {
                        ui.label("No EXIF/IPTC/XMP fields detected.");
                    } else {
                        egui::CollapsingHeader::new(format!(
                            "EXIF ({})",
                            metadata.exif_fields.len()
                        ))
                        .default_open(true)
                        .show(ui, |ui| {
                            for (key, value) in &metadata.exif_fields {
                                ui.horizontal_wrapped(|ui| {
                                    ui.label(format!("{key}:"));
                                    ui.label(value);
                                });
                            }
                        });
                        egui::CollapsingHeader::new(format!(
                            "IPTC ({})",
                            metadata.iptc_fields.len()
                        ))
                        .default_open(false)
                        .show(ui, |ui| {
                            for (key, value) in &metadata.iptc_fields {
                                ui.horizontal_wrapped(|ui| {
                                    ui.label(format!("{key}:"));
                                    ui.label(value);
                                });
                            }
                        });
                        egui::CollapsingHeader::new(format!("XMP ({})", metadata.xmp_fields.len()))
                            .default_open(false)
                            .show(ui, |ui| {
                                for (key, value) in &metadata.xmp_fields {
                                    ui.horizontal_wrapped(|ui| {
                                        ui.label(format!("{key}:"));
                                        ui.label(value);
                                    });
                                }
                            });
                    }
                }
            });
    }

    fn primary_camera_metadata(&self) -> Option<String> {
        let metadata = self.current_metadata.as_ref()?;
        for key in ["Model", "LensModel", "Make"] {
            if let Some(value) = metadata
                .exif_fields
                .iter()
                .find(|(field, _)| field == key)
                .map(|(_, value)| value.clone())
            {
                return Some(value);
            }
        }
        None
    }

    fn primary_capture_metadata(&self) -> Option<String> {
        let metadata = self.current_metadata.as_ref()?;
        for key in ["DateTimeOriginal", "DateTime", "CreateDate"] {
            if let Some(value) = metadata
                .exif_fields
                .iter()
                .find(|(field, _)| field == key)
                .map(|(_, value)| value.clone())
            {
                return Some(value);
            }
        }
        None
    }

    fn draw_status_bar(&mut self, ctx: &egui::Context) {
        if !self.state.show_status_bar() {
            return;
        }

        egui::TopBottomPanel::bottom("status")
            .exact_height(24.0)
            .show(ctx, |ui| {
                ui.horizontal_wrapped(|ui| {
                    ui.label(self.state.status_dimensions());
                    ui.separator();
                    ui.label(self.state.status_index());
                    ui.separator();
                    ui.label(self.state.status_zoom());
                    ui.separator();
                    ui.label(self.state.status_size());
                    ui.separator();
                    ui.label(self.state.status_preview());
                    ui.separator();
                    ui.label(self.state.status_name());
                    if let Some(camera) = self.primary_camera_metadata() {
                        ui.separator();
                        ui.label(format!("Camera: {camera}"));
                    }
                    if let Some(captured) = self.primary_capture_metadata() {
                        ui.separator();
                        ui.label(format!("Captured: {captured}"));
                    }
                });
            });
    }
}

impl eframe::App for ImranViewApp {
    fn update(&mut self, ctx: &egui::Context, _frame: &mut eframe::Frame) {
        self.poll_worker_results(ctx);
        self.handle_native_menu_events(ctx);
        self.run_shortcuts(ctx);
        self.run_slideshow_tick();
        self.sync_viewport_state(ctx);

        if self.should_draw_in_window_menu() {
            self.draw_menu(ctx);
        }
        self.draw_toolbar(ctx);
        self.draw_error_banner(ctx);
        self.draw_info_banner(ctx);
        self.draw_thumbnail_strip(ctx);
        self.draw_metadata_panel(ctx);

        if self.state.thumbnails_window_mode() {
            self.draw_thumbnail_window(ctx);
        } else {
            self.draw_main_viewer(ctx);
        }

        self.draw_status_bar(ctx);
        self.draw_resize_dialog(ctx);
        self.draw_crop_dialog(ctx);
        self.draw_color_dialog(ctx);
        self.draw_batch_dialog(ctx);
        self.draw_save_dialog(ctx);
        self.draw_performance_dialog(ctx);
        self.draw_rename_dialog(ctx);
        self.draw_delete_confirmation(ctx);
        self.draw_about_window(ctx);

        ctx.send_viewport_cmd(egui::ViewportCommand::Title(self.state.window_title()));

        if self.pending.has_inflight()
            || !self.inflight_thumbnails.is_empty()
            || self.slideshow_running
        {
            ctx.request_repaint_after(std::time::Duration::from_millis(16));
        }
    }
}

fn path_ancestors(path: &Path) -> Vec<PathBuf> {
    let mut result = Vec::new();
    for ancestor in path.ancestors() {
        result.push(ancestor.to_path_buf());
    }
    result.reverse();
    result
}

fn list_directories(path: &Path, limit: usize) -> Vec<PathBuf> {
    let mut directories = Vec::new();
    let Ok(read_dir) = fs::read_dir(path) else {
        return directories;
    };

    for entry in read_dir.flatten() {
        let candidate = entry.path();
        if candidate.is_dir() {
            directories.push(candidate);
            if directories.len() >= limit {
                break;
            }
        }
    }

    directories.sort_by_key(|candidate| {
        candidate
            .file_name()
            .map(|name| name.to_string_lossy().to_ascii_lowercase())
    });
    directories
}

fn format_recent_file_label(path: &Path) -> String {
    let file_name = path
        .file_name()
        .map(|name| name.to_string_lossy().into_owned())
        .unwrap_or_else(|| path.display().to_string());
    let parent = path
        .parent()
        .map(|parent| parent.display().to_string())
        .unwrap_or_default();
    if parent.is_empty() {
        file_name
    } else {
        format!("{file_name}   ({parent})")
    }
}

fn format_recent_folder_label(path: &Path) -> String {
    let name = path
        .file_name()
        .map(|name| name.to_string_lossy().into_owned())
        .unwrap_or_else(|| path.display().to_string());
    format!("{name}   ({})", path.display())
}

fn format_system_time(value: SystemTime) -> String {
    match value.duration_since(UNIX_EPOCH) {
        Ok(duration) => format!("unix: {}", duration.as_secs()),
        Err(_) => "unix: <invalid>".to_owned(),
    }
}

fn human_file_size(bytes: u64) -> String {
    const KB: f64 = 1024.0;
    const MB: f64 = KB * 1024.0;
    const GB: f64 = MB * 1024.0;

    let value = bytes as f64;
    if value < KB {
        format!("{bytes} B")
    } else if value < MB {
        format!("{:.1} KB", value / KB)
    } else if value < GB {
        format!("{:.2} MB", value / MB)
    } else {
        format!("{:.2} GB", value / GB)
    }
}

fn init_logging() {
    let env = env_logger::Env::default().default_filter_or("info");
    let mut builder = env_logger::Builder::from_env(env);
    builder.format_timestamp_millis();
    let _ = builder.try_init();
}

fn main() -> Result<()> {
    init_logging();
    let cli_path = std::env::args_os().nth(1).map(PathBuf::from);
    let startup_settings = load_settings();
    log::info!(target: "imranview::startup", "launching ImranView");

    let mut native_options = eframe::NativeOptions::default();
    if let Some([width, height]) = startup_settings.window_inner_size {
        if width > 0.0 && height > 0.0 {
            native_options.viewport = native_options.viewport.with_inner_size([width, height]);
        }
    }
    if let Some([x, y]) = startup_settings.window_position {
        native_options.viewport = native_options.viewport.with_position([x, y]);
    }
    if startup_settings.window_maximized {
        native_options.viewport = native_options.viewport.with_maximized(true);
    }
    if startup_settings.window_fullscreen {
        native_options.viewport = native_options.viewport.with_fullscreen(true);
    }
    match load_app_icon_data(APP_FAVICON_PNG) {
        Ok(icon_data) => {
            native_options.viewport = native_options.viewport.with_icon(icon_data);
        }
        Err(err) => {
            log::warn!(target: "imranview::startup", "failed to load app icon: {err:#}");
        }
    }
    eframe::run_native(
        "ImranView",
        native_options,
        Box::new(move |cc| {
            let startup_started = Instant::now();
            let app = ImranViewApp::new(cc, cli_path.clone(), startup_settings.clone());
            crate::perf::log_timing(
                "startup",
                startup_started.elapsed(),
                crate::perf::STARTUP_BUDGET,
            );
            Ok(Box::new(app))
        }),
    )
    .map_err(|err| anyhow!("failed to run egui app: {err}"))?;
    log::info!(target: "imranview::startup", "application exited");
    Ok(())
}
