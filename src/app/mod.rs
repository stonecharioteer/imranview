mod actions;
mod bootstrap;
mod controller;
mod controller_actions;
mod model;
mod runtime;
mod runtime_panels;
mod runtime_platform;
mod ui_dialogs;
mod ui_dialogs_edit;
mod ui_shell;
mod view;

use std::collections::{HashMap, HashSet};
use std::fs;
use std::path::Path;
use std::path::PathBuf;
use std::sync::mpsc::{self, Receiver, Sender};
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};

use anyhow::{Result, anyhow};
use eframe::egui;

use self::controller::MenuCommand;
use self::model::{
    CommandPaletteEntry, CommandPaletteState, CompareImageState, FileSortFacts, FolderPanelCache,
    ThumbTextureCache, ViewportSnapshot, default_scanner_command_template,
};
use self::view::{
    ToolbarIcons, apply_native_look, centered_dialog_window, load_png_texture,
    platform_widget_corner_radius, platform_window_corner_radius,
};

use crate::app_state::{AppState, ThumbnailEntry};
use crate::image_io::MetadataSummary;
use crate::image_io::{
    PREVIEW_REFINE_DIMENSION, collect_images_in_directory, is_supported_image_path,
};
#[cfg(any(target_os = "macos", target_os = "windows", target_os = "linux"))]
use crate::native_menu::{NativeMenu, NativeMenuAction};
use crate::pending_requests::PendingRequests;
use crate::picker::{PickerRequestKind, PickerResult};
use crate::plugin::{PluginContext, PluginEvent, PluginHost};
use crate::settings::{PersistedSettings, save_settings};
use crate::shortcuts::{self, ShortcutAction, menu_item_label, shortcut_text};
use crate::worker::{
    self, AlphaBrushOp, BatchConvertOptions, BatchOutputFormat, CanvasAnchor, ColorAdjustParams,
    EffectsParams, FileOperation, LosslessJpegOp, PanoramaDirection, ResizeFilter,
    RotationInterpolation, SaveImageOptions, SaveMetadataPolicy, SaveOutputFormat, SelectionParams,
    SelectionWorkflow, ShapeKind, ShapeParams, TransformOp, WorkerCommand, WorkerRequestKind,
    WorkerResult,
};

const THUMB_TEXTURE_CACHE_CAP: usize = 320;
const THUMB_TEXTURE_CACHE_MAX_BYTES: usize = 96 * 1024 * 1024;
const THUMB_CARD_WIDTH: f32 = 120.0;
const THUMB_CARD_HEIGHT: f32 = 100.0;
const TOOLBAR_ICON_SIZE: f32 = 18.0;
const TOOLBAR_PANEL_HEIGHT: f32 = 38.0;
const STATUS_PANEL_HEIGHT: f32 = 26.0;
const APP_FAVICON_PNG: &[u8] = include_bytes!("../../assets/branding/favicon.png");
const FOLDER_PANEL_LIST_LIMIT: usize = 256;
const RECENT_MENU_LIMIT: usize = 12;
const COMMAND_PALETTE_PANEL_WIDTH: f32 = 700.0;
const COMMAND_PALETTE_PANEL_MAX_HEIGHT: f32 = 560.0;
const COMMAND_PALETTE_MAX_VISIBLE: usize = 320;
const PREVIEW_REFINE_IDLE_DELAY_MS: u64 = 220;

fn load_app_icon_data(bytes: &[u8]) -> Result<egui::IconData> {
    eframe::icon_data::from_png_bytes(bytes)
        .map_err(|err| anyhow!("failed to decode app icon PNG bytes: {err}"))
}

include!("dialog_state.rs");

struct ImranViewApp {
    state: AppState,
    current_metadata: Option<MetadataSummary>,
    worker_tx: Sender<WorkerCommand>,
    thumbnail_tx: Sender<PathBuf>,
    worker_rx: Receiver<WorkerResult>,
    picker_result_tx: Sender<PickerResult>,
    picker_result_rx: Receiver<PickerResult>,
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
    border_dialog: BorderDialogState,
    canvas_dialog: CanvasDialogState,
    fine_rotate_dialog: FineRotateDialogState,
    text_tool_dialog: TextToolDialogState,
    shape_tool_dialog: ShapeToolDialogState,
    overlay_dialog: OverlayDialogState,
    selection_workflow_dialog: SelectionWorkflowDialogState,
    replace_color_dialog: ReplaceColorDialogState,
    alpha_dialog: AlphaDialogState,
    effects_dialog: EffectsDialogState,
    batch_dialog: BatchDialogState,
    save_dialog: SaveDialogState,
    performance_dialog: PerformanceDialogState,
    rename_dialog: RenameDialogState,
    search_dialog: SearchDialogState,
    screenshot_dialog: ScreenshotDialogState,
    tiff_dialog: TiffDialogState,
    pdf_dialog: PdfDialogState,
    batch_scan_dialog: BatchScanDialogState,
    ocr_dialog: OcrDialogState,
    lossless_jpeg_dialog: LosslessJpegDialogState,
    exif_date_dialog: ExifDateDialogState,
    color_profile_dialog: ColorProfileDialogState,
    panorama_dialog: PanoramaDialogState,
    perspective_dialog: PerspectiveDialogState,
    magnifier_dialog: MagnifierDialogState,
    contact_sheet_dialog: ContactSheetDialogState,
    html_export_dialog: HtmlExportDialogState,
    advanced_options_dialog: AdvancedOptionsDialogState,
    confirm_delete_current: bool,
    info_message: Option<String>,
    slideshow_running: bool,
    slideshow_last_tick: Instant,
    preview_refine_due_at: Option<Instant>,
    preview_refined_for_path: Option<PathBuf>,
    show_about_window: bool,
    command_palette: CommandPaletteState,
    #[cfg(any(target_os = "macos", target_os = "windows", target_os = "linux"))]
    native_menu: Option<NativeMenu>,
    #[cfg(any(target_os = "macos", target_os = "windows", target_os = "linux"))]
    native_menu_install_attempted: bool,
}

impl ImranViewApp {
    fn new(
        cc: &eframe::CreationContext<'_>,
        cli_path: Option<PathBuf>,
        settings: PersistedSettings,
    ) -> Self {
        apply_native_look(&cc.egui_ctx);
        let state = AppState::new_with_settings(settings.clone());
        let thumb_cache_entry_cap = state.thumb_cache_entry_cap();
        let thumb_cache_max_bytes = state.thumb_cache_max_mb().saturating_mul(1024 * 1024);
        let (worker_tx, worker_thread_rx) = mpsc::channel::<WorkerCommand>();
        let (thumbnail_tx, thumbnail_rx) = mpsc::channel::<PathBuf>();
        let (worker_thread_tx, worker_rx) = mpsc::channel::<WorkerResult>();
        let (picker_result_tx, picker_result_rx) = mpsc::channel::<PickerResult>();
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

        let mut app = Self {
            state,
            current_metadata: None,
            worker_tx,
            thumbnail_tx,
            worker_rx,
            picker_result_tx,
            picker_result_rx,
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
            border_dialog: BorderDialogState::default(),
            canvas_dialog: CanvasDialogState::default(),
            fine_rotate_dialog: FineRotateDialogState::default(),
            text_tool_dialog: TextToolDialogState::default(),
            shape_tool_dialog: ShapeToolDialogState::default(),
            overlay_dialog: OverlayDialogState::default(),
            selection_workflow_dialog: SelectionWorkflowDialogState::default(),
            replace_color_dialog: ReplaceColorDialogState::default(),
            alpha_dialog: AlphaDialogState::default(),
            effects_dialog: EffectsDialogState::default(),
            batch_dialog: BatchDialogState::default(),
            save_dialog: SaveDialogState::default(),
            performance_dialog: PerformanceDialogState::default(),
            rename_dialog: RenameDialogState::default(),
            search_dialog: SearchDialogState::default(),
            screenshot_dialog: ScreenshotDialogState::default(),
            tiff_dialog: TiffDialogState::default(),
            pdf_dialog: PdfDialogState::default(),
            batch_scan_dialog: BatchScanDialogState::default(),
            ocr_dialog: OcrDialogState::default(),
            lossless_jpeg_dialog: LosslessJpegDialogState::default(),
            exif_date_dialog: ExifDateDialogState::default(),
            color_profile_dialog: ColorProfileDialogState::default(),
            panorama_dialog: PanoramaDialogState::default(),
            perspective_dialog: PerspectiveDialogState::default(),
            magnifier_dialog: MagnifierDialogState::default(),
            contact_sheet_dialog: ContactSheetDialogState::default(),
            html_export_dialog: HtmlExportDialogState::default(),
            advanced_options_dialog: Self::advanced_options_from_settings(&settings),
            confirm_delete_current: false,
            info_message: None,
            slideshow_running: false,
            slideshow_last_tick: Instant::now(),
            preview_refine_due_at: None,
            preview_refined_for_path: None,
            show_about_window: false,
            command_palette: CommandPaletteState::default(),
            #[cfg(any(target_os = "macos", target_os = "windows", target_os = "linux"))]
            native_menu: None,
            #[cfg(any(target_os = "macos", target_os = "windows", target_os = "linux"))]
            native_menu_install_attempted: false,
        };

        app.apply_selected_skin(&cc.egui_ctx);

        if let Some(path) = cli_path {
            log::debug!(target: "imranview::ui", "startup CLI open {}", path.display());
            app.dispatch_open(path, false);
        }

        app
    }


}

impl eframe::App for ImranViewApp {
    fn update(&mut self, ctx: &egui::Context, frame: &mut eframe::Frame) {
        self.maybe_install_native_menu(frame);
        self.poll_worker_results(ctx);
        self.poll_picker_results();
        self.handle_native_menu_events(ctx);
        self.run_shortcuts(ctx);
        self.run_slideshow_tick();
        self.maybe_refine_preview_after_idle(ctx);
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
        self.draw_border_dialog(ctx);
        self.draw_canvas_dialog(ctx);
        self.draw_fine_rotate_dialog(ctx);
        self.draw_text_tool_dialog(ctx);
        self.draw_shape_tool_dialog(ctx);
        self.draw_overlay_dialog(ctx);
        self.draw_selection_workflow_dialog(ctx);
        self.draw_replace_color_dialog(ctx);
        self.draw_alpha_dialog(ctx);
        self.draw_effects_dialog(ctx);
        self.draw_batch_dialog(ctx);
        self.draw_save_dialog(ctx);
        self.draw_performance_dialog(ctx);
        self.draw_rename_dialog(ctx);
        self.draw_search_dialog(ctx);
        self.draw_screenshot_dialog(ctx);
        self.draw_tiff_dialog(ctx);
        self.draw_pdf_dialog(ctx);
        self.draw_batch_scan_dialog(ctx);
        self.draw_ocr_dialog(ctx);
        self.draw_lossless_jpeg_dialog(ctx);
        self.draw_exif_date_dialog(ctx);
        self.draw_color_profile_dialog(ctx);
        self.draw_panorama_dialog(ctx);
        self.draw_perspective_dialog(ctx);
        self.draw_magnifier_dialog(ctx);
        self.draw_contact_sheet_dialog(ctx);
        self.draw_html_export_dialog(ctx);
        self.draw_advanced_options_dialog(ctx);
        self.draw_delete_confirmation(ctx);
        self.draw_about_window(ctx);
        self.draw_command_palette(ctx);

        ctx.send_viewport_cmd(egui::ViewportCommand::Title(self.state.window_title()));

        if self.pending.has_inflight()
            || !self.inflight_thumbnails.is_empty()
            || self.preview_refine_due_at.is_some()
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

pub fn run() -> Result<()> {
    bootstrap::run()
}
