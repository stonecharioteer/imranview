mod app_state;
mod image_io;
#[cfg(any(target_os = "macos", target_os = "windows", target_os = "linux"))]
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
use crate::image_io::{collect_images_in_directory, is_supported_image_path};
#[cfg(any(target_os = "macos", target_os = "windows", target_os = "linux"))]
use crate::native_menu::{NativeMenu, NativeMenuAction};
use crate::plugin::{PluginContext, PluginEvent, PluginHost};
use crate::settings::{PersistedSettings, load_settings, save_settings};
use crate::shortcuts::{ShortcutAction, menu_item_label};
use crate::worker::{
    BatchConvertOptions, BatchOutputFormat, CanvasAnchor, ColorAdjustParams, EffectsParams,
    FileOperation, LosslessJpegOp, PanoramaDirection, ResizeFilter, RotationInterpolation, SaveImageOptions,
    SaveMetadataPolicy, SaveOutputFormat, SelectionParams, SelectionWorkflow, ShapeKind,
    ShapeParams, TransformOp, WorkerCommand, WorkerRequestKind, WorkerResult,
};

const THUMB_TEXTURE_CACHE_CAP: usize = 320;
const THUMB_TEXTURE_CACHE_MAX_BYTES: usize = 96 * 1024 * 1024;
const THUMB_CARD_WIDTH: f32 = 120.0;
const THUMB_CARD_HEIGHT: f32 = 100.0;
const TOOLBAR_ICON_SIZE: f32 = 18.0;
const TOOLBAR_PANEL_HEIGHT: f32 = 38.0;
const STATUS_PANEL_HEIGHT: f32 = 26.0;
const APP_FAVICON_PNG: &[u8] = include_bytes!("../assets/branding/favicon.png");
const FOLDER_PANEL_LIST_LIMIT: usize = 256;
const RECENT_MENU_LIMIT: usize = 12;

const fn platform_window_corner_radius() -> u8 {
    #[cfg(target_os = "macos")]
    {
        12
    }
    #[cfg(target_os = "windows")]
    {
        8
    }
    #[cfg(all(not(target_os = "macos"), not(target_os = "windows")))]
    {
        7
    }
}

const fn platform_widget_corner_radius() -> u8 {
    #[cfg(target_os = "macos")]
    {
        9
    }
    #[cfg(target_os = "windows")]
    {
        6
    }
    #[cfg(all(not(target_os = "macos"), not(target_os = "windows")))]
    {
        6
    }
}

fn apply_native_look(ctx: &egui::Context) {
    ctx.set_theme(egui::ThemePreference::System);
    ctx.all_styles_mut(|style| {
        style.spacing.item_spacing = egui::vec2(8.0, 6.0);
        style.spacing.button_padding = egui::vec2(8.0, 4.0);
        style.spacing.interact_size.y = 24.0;
        style.spacing.menu_margin = egui::Margin::symmetric(8, 6);
        style.spacing.window_margin = egui::Margin::symmetric(10, 8);
        style.spacing.combo_width = style.spacing.combo_width.max(120.0);
        style.spacing.slider_width = style.spacing.slider_width.max(140.0);

        if let Some(body) = style.text_styles.get_mut(&egui::TextStyle::Body) {
            body.size = body.size.clamp(13.0, 15.0);
        }
        if let Some(button) = style.text_styles.get_mut(&egui::TextStyle::Button) {
            button.size = button.size.clamp(13.0, 15.0);
        }
        if let Some(small) = style.text_styles.get_mut(&egui::TextStyle::Small) {
            small.size = small.size.clamp(11.0, 13.0);
        }

        let visuals = &mut style.visuals;
        let (window_fill, panel_fill, extreme_bg, border_color, shadow_color) = if visuals.dark_mode
        {
            (
                egui::Color32::from_rgb(30, 31, 34),
                egui::Color32::from_rgb(37, 38, 42),
                egui::Color32::from_rgb(24, 25, 27),
                egui::Color32::from_gray(72),
                egui::Color32::from_black_alpha(110),
            )
        } else {
            (
                egui::Color32::from_rgb(250, 250, 252),
                egui::Color32::from_rgb(242, 242, 246),
                egui::Color32::from_rgb(255, 255, 255),
                egui::Color32::from_rgb(198, 198, 205),
                egui::Color32::from_black_alpha(44),
            )
        };

        let window_radius = platform_window_corner_radius();
        let widget_radius = platform_widget_corner_radius();

        visuals.window_corner_radius = egui::CornerRadius::same(window_radius);
        visuals.menu_corner_radius = egui::CornerRadius::same(widget_radius);
        visuals.window_fill = window_fill;
        visuals.panel_fill = panel_fill;
        visuals.faint_bg_color = panel_fill;
        visuals.extreme_bg_color = extreme_bg;
        visuals.window_stroke = egui::Stroke::new(1.0, border_color);
        visuals.window_shadow = egui::Shadow {
            offset: [0, 2],
            blur: 16,
            spread: 0,
            color: shadow_color,
        };
        visuals.popup_shadow = visuals.window_shadow;
        visuals.interact_cursor = None;
        visuals.button_frame = true;
        visuals.collapsing_header_frame = false;
        visuals.widgets.noninteractive.bg_stroke = egui::Stroke::new(1.0, border_color);
        visuals.widgets.noninteractive.corner_radius = egui::CornerRadius::same(widget_radius);
        visuals.widgets.inactive.corner_radius = egui::CornerRadius::same(widget_radius);
        visuals.widgets.hovered.corner_radius = egui::CornerRadius::same(widget_radius);
        visuals.widgets.active.corner_radius = egui::CornerRadius::same(widget_radius);
        visuals.widgets.open.corner_radius = egui::CornerRadius::same(widget_radius);
    });
}

fn centered_dialog_window(title: &'static str) -> egui::Window<'static> {
    egui::Window::new(title)
        .collapsible(false)
        .resizable(false)
        .anchor(egui::Align2::CENTER_CENTER, egui::Vec2::ZERO)
}

fn default_scanner_command_template() -> String {
    #[cfg(target_os = "windows")]
    {
        String::new()
    }
    #[cfg(not(target_os = "windows"))]
    {
        "scanimage --format=png --output-file {output}".to_owned()
    }
}

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
    latest_utility: u64,
    open_inflight: bool,
    save_inflight: bool,
    edit_inflight: bool,
    batch_inflight: bool,
    file_inflight: bool,
    compare_inflight: bool,
    print_inflight: bool,
    utility_inflight: bool,
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
            || self.utility_inflight
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
struct BorderDialogState {
    open: bool,
    left: u32,
    right: u32,
    top: u32,
    bottom: u32,
    color: egui::Color32,
}

impl Default for BorderDialogState {
    fn default() -> Self {
        Self {
            open: false,
            left: 8,
            right: 8,
            top: 8,
            bottom: 8,
            color: egui::Color32::WHITE,
        }
    }
}

#[derive(Clone, Debug)]
struct CanvasDialogState {
    open: bool,
    width: u32,
    height: u32,
    anchor: CanvasAnchor,
    fill: egui::Color32,
}

impl Default for CanvasDialogState {
    fn default() -> Self {
        Self {
            open: false,
            width: 0,
            height: 0,
            anchor: CanvasAnchor::Center,
            fill: egui::Color32::BLACK,
        }
    }
}

#[derive(Clone, Debug)]
struct FineRotateDialogState {
    open: bool,
    angle_degrees: f32,
    interpolation: RotationInterpolation,
    expand_canvas: bool,
    fill: egui::Color32,
}

impl Default for FineRotateDialogState {
    fn default() -> Self {
        Self {
            open: false,
            angle_degrees: 0.0,
            interpolation: RotationInterpolation::Bilinear,
            expand_canvas: true,
            fill: egui::Color32::BLACK,
        }
    }
}

#[derive(Clone, Debug)]
struct TextToolDialogState {
    open: bool,
    text: String,
    x: i32,
    y: i32,
    scale: u32,
    color: egui::Color32,
}

impl Default for TextToolDialogState {
    fn default() -> Self {
        Self {
            open: false,
            text: String::new(),
            x: 10,
            y: 10,
            scale: 2,
            color: egui::Color32::WHITE,
        }
    }
}

#[derive(Clone, Debug)]
struct ShapeToolDialogState {
    open: bool,
    kind: ShapeKind,
    start_x: i32,
    start_y: i32,
    end_x: i32,
    end_y: i32,
    thickness: u32,
    filled: bool,
    color: egui::Color32,
}

impl Default for ShapeToolDialogState {
    fn default() -> Self {
        Self {
            open: false,
            kind: ShapeKind::Rectangle,
            start_x: 10,
            start_y: 10,
            end_x: 200,
            end_y: 150,
            thickness: 2,
            filled: false,
            color: egui::Color32::from_rgb(255, 0, 0),
        }
    }
}

#[derive(Clone, Debug)]
struct OverlayDialogState {
    open: bool,
    overlay_path: String,
    opacity: f32,
    anchor: CanvasAnchor,
}

impl Default for OverlayDialogState {
    fn default() -> Self {
        Self {
            open: false,
            overlay_path: String::new(),
            opacity: 0.7,
            anchor: CanvasAnchor::BottomRight,
        }
    }
}

#[derive(Clone, Debug)]
struct SelectionWorkflowDialogState {
    open: bool,
    workflow: SelectionWorkflow,
    x: u32,
    y: u32,
    width: u32,
    height: u32,
    radius: u32,
    polygon_points: String,
    fill: egui::Color32,
}

impl Default for SelectionWorkflowDialogState {
    fn default() -> Self {
        Self {
            open: false,
            workflow: SelectionWorkflow::CropRect,
            x: 0,
            y: 0,
            width: 100,
            height: 100,
            radius: 50,
            polygon_points: "10,10;120,10;140,100;20,120".to_owned(),
            fill: egui::Color32::BLACK,
        }
    }
}

#[derive(Clone, Debug)]
struct ReplaceColorDialogState {
    open: bool,
    source: egui::Color32,
    target: egui::Color32,
    tolerance: u8,
    preserve_alpha: bool,
}

impl Default for ReplaceColorDialogState {
    fn default() -> Self {
        Self {
            open: false,
            source: egui::Color32::WHITE,
            target: egui::Color32::BLACK,
            tolerance: 24,
            preserve_alpha: true,
        }
    }
}

#[derive(Clone, Debug)]
struct AlphaDialogState {
    open: bool,
    alpha_percent: f32,
    alpha_from_luma: bool,
    invert_luma: bool,
    limit_to_region: bool,
    region_x: u32,
    region_y: u32,
    region_width: u32,
    region_height: u32,
}

impl Default for AlphaDialogState {
    fn default() -> Self {
        Self {
            open: false,
            alpha_percent: 100.0,
            alpha_from_luma: false,
            invert_luma: false,
            limit_to_region: false,
            region_x: 0,
            region_y: 0,
            region_width: 256,
            region_height: 256,
        }
    }
}

#[derive(Clone, Debug)]
struct EffectsDialogState {
    open: bool,
    preset: EffectsPreset,
    blur_sigma: f32,
    sharpen_sigma: f32,
    sharpen_threshold: i32,
    invert: bool,
    grayscale: bool,
    sepia_strength: f32,
    posterize_levels: u8,
    vignette_strength: f32,
    tilt_shift_strength: f32,
    stained_glass_strength: f32,
    emboss_strength: f32,
    edge_enhance_strength: f32,
    oil_paint_strength: f32,
}

impl Default for EffectsDialogState {
    fn default() -> Self {
        Self {
            open: false,
            preset: EffectsPreset::Custom,
            blur_sigma: 0.0,
            sharpen_sigma: 0.0,
            sharpen_threshold: 1,
            invert: false,
            grayscale: false,
            sepia_strength: 0.0,
            posterize_levels: 0,
            vignette_strength: 0.0,
            tilt_shift_strength: 0.0,
            stained_glass_strength: 0.0,
            emboss_strength: 0.0,
            edge_enhance_strength: 0.0,
            oil_paint_strength: 0.0,
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum EffectsPreset {
    Custom,
    Natural,
    Vintage,
    Dramatic,
    Noir,
    StainedGlass,
    TiltShift,
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

#[derive(Clone, Debug, Default)]
struct SearchDialogState {
    open: bool,
    query: String,
    extension_filter: String,
    case_sensitive: bool,
    results: Vec<PathBuf>,
}

#[derive(Clone, Debug)]
struct ScreenshotDialogState {
    open: bool,
    delay_ms: u64,
    region_enabled: bool,
    region_x: u32,
    region_y: u32,
    region_width: u32,
    region_height: u32,
    output_path: String,
}

impl Default for ScreenshotDialogState {
    fn default() -> Self {
        Self {
            open: false,
            delay_ms: 0,
            region_enabled: false,
            region_x: 0,
            region_y: 0,
            region_width: 1280,
            region_height: 720,
            output_path: String::new(),
        }
    }
}

#[derive(Clone, Debug)]
struct TiffDialogState {
    open: bool,
    path: String,
    page_index: u32,
    page_count: Option<u32>,
    extract_output_dir: String,
    extract_format: BatchOutputFormat,
    jpeg_quality: u8,
}

impl Default for TiffDialogState {
    fn default() -> Self {
        Self {
            open: false,
            path: String::new(),
            page_index: 0,
            page_count: None,
            extract_output_dir: String::new(),
            extract_format: BatchOutputFormat::Png,
            jpeg_quality: 90,
        }
    }
}

#[derive(Clone, Debug)]
struct PdfDialogState {
    open: bool,
    output_path: String,
    include_folder_images: bool,
    jpeg_quality: u8,
}

impl Default for PdfDialogState {
    fn default() -> Self {
        Self {
            open: false,
            output_path: "output.pdf".to_owned(),
            include_folder_images: true,
            jpeg_quality: 88,
        }
    }
}

#[derive(Clone, Debug)]
struct BatchScanDialogState {
    open: bool,
    source: BatchScanSource,
    input_dir: String,
    output_dir: String,
    output_format: BatchOutputFormat,
    rename_prefix: String,
    start_index: u32,
    jpeg_quality: u8,
    page_count: u32,
    command_template: String,
}

impl Default for BatchScanDialogState {
    fn default() -> Self {
        Self {
            open: false,
            source: BatchScanSource::FolderImport,
            input_dir: String::new(),
            output_dir: String::new(),
            output_format: BatchOutputFormat::Jpeg,
            rename_prefix: "scan_".to_owned(),
            start_index: 1,
            jpeg_quality: 90,
            page_count: 1,
            command_template: default_scanner_command_template(),
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum BatchScanSource {
    FolderImport,
    ScannerCommand,
}

#[derive(Clone, Debug)]
struct OcrDialogState {
    open: bool,
    language: String,
    output_path: String,
    preview_text: String,
}

impl Default for OcrDialogState {
    fn default() -> Self {
        Self {
            open: false,
            language: "eng".to_owned(),
            output_path: String::new(),
            preview_text: String::new(),
        }
    }
}

#[derive(Clone, Debug)]
struct LosslessJpegDialogState {
    open: bool,
    output_path: String,
    in_place: bool,
    op: LosslessJpegOp,
}

impl Default for LosslessJpegDialogState {
    fn default() -> Self {
        Self {
            open: false,
            output_path: String::new(),
            in_place: true,
            op: LosslessJpegOp::Rotate90,
        }
    }
}

#[derive(Clone, Debug)]
struct ExifDateDialogState {
    open: bool,
    datetime: String,
}

impl Default for ExifDateDialogState {
    fn default() -> Self {
        Self {
            open: false,
            datetime: "2026:01:01 12:00:00".to_owned(),
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum ColorRenderingIntent {
    RelativeColorimetric,
    Perceptual,
    Saturation,
    AbsoluteColorimetric,
}

impl ColorRenderingIntent {
    fn as_worker_value(self) -> &'static str {
        match self {
            ColorRenderingIntent::RelativeColorimetric => "relative",
            ColorRenderingIntent::Perceptual => "perceptual",
            ColorRenderingIntent::Saturation => "saturation",
            ColorRenderingIntent::AbsoluteColorimetric => "absolute",
        }
    }
}

#[derive(Clone, Debug)]
struct ColorProfileDialogState {
    open: bool,
    source_profile_path: String,
    target_profile_path: String,
    output_path: String,
    in_place: bool,
    rendering_intent: ColorRenderingIntent,
}

impl Default for ColorProfileDialogState {
    fn default() -> Self {
        Self {
            open: false,
            source_profile_path: String::new(),
            target_profile_path: String::new(),
            output_path: String::new(),
            in_place: true,
            rendering_intent: ColorRenderingIntent::RelativeColorimetric,
        }
    }
}

#[derive(Clone, Debug)]
struct PanoramaDialogState {
    open: bool,
    output_path: String,
    include_folder_images: bool,
    direction: PanoramaDirection,
    overlap_percent: f32,
}

impl Default for PanoramaDialogState {
    fn default() -> Self {
        Self {
            open: false,
            output_path: "panorama.jpg".to_owned(),
            include_folder_images: true,
            direction: PanoramaDirection::Horizontal,
            overlap_percent: 0.08,
        }
    }
}

#[derive(Clone, Debug)]
struct PerspectiveDialogState {
    open: bool,
    top_left: [f32; 2],
    top_right: [f32; 2],
    bottom_right: [f32; 2],
    bottom_left: [f32; 2],
    output_width: u32,
    output_height: u32,
    interpolation: RotationInterpolation,
    fill: egui::Color32,
}

impl Default for PerspectiveDialogState {
    fn default() -> Self {
        Self {
            open: false,
            top_left: [0.0, 0.0],
            top_right: [0.0, 0.0],
            bottom_right: [0.0, 0.0],
            bottom_left: [0.0, 0.0],
            output_width: 0,
            output_height: 0,
            interpolation: RotationInterpolation::Bilinear,
            fill: egui::Color32::BLACK,
        }
    }
}

#[derive(Clone, Debug)]
struct MagnifierDialogState {
    open: bool,
    enabled: bool,
    zoom: f32,
    size: f32,
}

impl Default for MagnifierDialogState {
    fn default() -> Self {
        Self {
            open: false,
            enabled: false,
            zoom: 3.0,
            size: 180.0,
        }
    }
}

#[derive(Clone, Debug)]
struct ContactSheetDialogState {
    open: bool,
    output_path: String,
    include_folder_images: bool,
    columns: u32,
    thumb_size: u32,
    include_labels: bool,
    background: egui::Color32,
    label_color: egui::Color32,
    jpeg_quality: u8,
}

impl Default for ContactSheetDialogState {
    fn default() -> Self {
        Self {
            open: false,
            output_path: "contact-sheet.jpg".to_owned(),
            include_folder_images: true,
            columns: 6,
            thumb_size: 180,
            include_labels: true,
            background: egui::Color32::from_gray(18),
            label_color: egui::Color32::WHITE,
            jpeg_quality: 90,
        }
    }
}

#[derive(Clone, Debug)]
struct HtmlExportDialogState {
    open: bool,
    output_dir: String,
    include_folder_images: bool,
    title: String,
    thumb_width: u32,
}

impl Default for HtmlExportDialogState {
    fn default() -> Self {
        Self {
            open: false,
            output_dir: "gallery".to_owned(),
            include_folder_images: true,
            title: "ImranView Gallery".to_owned(),
            thumb_width: 360,
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum AdvancedOptionsTab {
    Viewing,
    Browsing,
    Editing,
    Fullscreen,
    Zoom,
    ColorManagement,
    Video,
    Language,
    Skins,
    Plugins,
    Misc,
    FileHandling,
}

#[derive(Clone, Debug, PartialEq)]
struct AdvancedOptionsDialogState {
    open: bool,
    active_tab: AdvancedOptionsTab,
    checkerboard_background: bool,
    smooth_main_scaling: bool,
    default_jpeg_quality: u8,
    auto_reopen_after_save: bool,
    hide_toolbar_in_fullscreen: bool,
    browsing_wrap_navigation: bool,
    zoom_step_percent: f32,
    enable_color_management: bool,
    simulate_srgb_output: bool,
    display_gamma: f32,
    video_frame_step_ms: u32,
    ui_language: String,
    skin_name: String,
    plugin_search_path: String,
    keep_single_instance: bool,
    confirm_delete: bool,
    confirm_overwrite: bool,
}

impl Default for AdvancedOptionsDialogState {
    fn default() -> Self {
        Self {
            open: false,
            active_tab: AdvancedOptionsTab::Viewing,
            checkerboard_background: false,
            smooth_main_scaling: true,
            default_jpeg_quality: 92,
            auto_reopen_after_save: true,
            hide_toolbar_in_fullscreen: false,
            browsing_wrap_navigation: true,
            zoom_step_percent: 20.0,
            enable_color_management: false,
            simulate_srgb_output: true,
            display_gamma: 2.2,
            video_frame_step_ms: 40,
            ui_language: "System".to_owned(),
            skin_name: "Classic".to_owned(),
            plugin_search_path: String::new(),
            keep_single_instance: true,
            confirm_delete: true,
            confirm_overwrite: true,
        }
    }
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
    show_about_window: bool,
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
            show_about_window: false,
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

    fn next_request_id(&mut self) -> u64 {
        let next = self.request_sequence;
        self.request_sequence = self.request_sequence.saturating_add(1);
        next
    }

    fn advanced_options_from_settings(settings: &PersistedSettings) -> AdvancedOptionsDialogState {
        AdvancedOptionsDialogState {
            open: false,
            active_tab: AdvancedOptionsTab::Viewing,
            checkerboard_background: settings.checkerboard_background,
            smooth_main_scaling: settings.smooth_main_scaling,
            default_jpeg_quality: settings.default_jpeg_quality.clamp(1, 100),
            auto_reopen_after_save: settings.auto_reopen_after_save,
            hide_toolbar_in_fullscreen: settings.hide_toolbar_in_fullscreen,
            browsing_wrap_navigation: settings.browsing_wrap_navigation,
            zoom_step_percent: settings.zoom_step_percent.clamp(5.0, 200.0),
            enable_color_management: settings.enable_color_management,
            simulate_srgb_output: settings.simulate_srgb_output,
            display_gamma: settings.display_gamma.clamp(1.6, 3.0),
            video_frame_step_ms: settings.video_frame_step_ms.clamp(10, 1000),
            ui_language: if settings.ui_language.trim().is_empty() {
                "System".to_owned()
            } else {
                settings.ui_language.clone()
            },
            skin_name: if settings.skin_name.trim().is_empty() {
                "Classic".to_owned()
            } else {
                settings.skin_name.clone()
            },
            plugin_search_path: settings.plugin_search_path.clone(),
            keep_single_instance: settings.keep_single_instance,
            confirm_delete: settings.confirm_delete,
            confirm_overwrite: settings.confirm_overwrite,
        }
    }

    fn persist_settings(&self) {
        let mut settings = self.state.to_settings();
        settings.checkerboard_background = self.advanced_options_dialog.checkerboard_background;
        settings.smooth_main_scaling = self.advanced_options_dialog.smooth_main_scaling;
        settings.default_jpeg_quality = self.advanced_options_dialog.default_jpeg_quality;
        settings.auto_reopen_after_save = self.advanced_options_dialog.auto_reopen_after_save;
        settings.hide_toolbar_in_fullscreen =
            self.advanced_options_dialog.hide_toolbar_in_fullscreen;
        settings.browsing_wrap_navigation = self.advanced_options_dialog.browsing_wrap_navigation;
        settings.zoom_step_percent = self.advanced_options_dialog.zoom_step_percent;
        settings.enable_color_management = self.advanced_options_dialog.enable_color_management;
        settings.simulate_srgb_output = self.advanced_options_dialog.simulate_srgb_output;
        settings.display_gamma = self.advanced_options_dialog.display_gamma;
        settings.video_frame_step_ms = self.advanced_options_dialog.video_frame_step_ms;
        settings.ui_language = self.advanced_options_dialog.ui_language.clone();
        settings.skin_name = self.advanced_options_dialog.skin_name.clone();
        settings.plugin_search_path = self.advanced_options_dialog.plugin_search_path.clone();
        settings.keep_single_instance = self.advanced_options_dialog.keep_single_instance;
        settings.confirm_delete = self.advanced_options_dialog.confirm_delete;
        settings.confirm_overwrite = self.advanced_options_dialog.confirm_overwrite;
        if let Err(err) = save_settings(&settings) {
            log::warn!(
                target: "imranview::settings",
                "failed to save settings: {err:#}"
            );
        }
    }

    fn apply_selected_skin(&self, ctx: &egui::Context) {
        apply_native_look(ctx);
        match self.advanced_options_dialog.skin_name.as_str() {
            "Graphite" => {
                ctx.style_mut(|style| {
                    let visuals = &mut style.visuals;
                    visuals.panel_fill = egui::Color32::from_rgb(34, 36, 40);
                    visuals.window_fill = egui::Color32::from_rgb(28, 30, 34);
                    visuals.faint_bg_color = egui::Color32::from_rgb(41, 44, 49);
                    visuals.extreme_bg_color = egui::Color32::from_rgb(22, 24, 27);
                    visuals.window_stroke =
                        egui::Stroke::new(1.0, egui::Color32::from_rgb(76, 80, 88));
                });
            }
            "Mist" => {
                ctx.style_mut(|style| {
                    let visuals = &mut style.visuals;
                    visuals.panel_fill = egui::Color32::from_rgb(236, 240, 245);
                    visuals.window_fill = egui::Color32::from_rgb(246, 248, 251);
                    visuals.faint_bg_color = egui::Color32::from_rgb(228, 233, 240);
                    visuals.extreme_bg_color = egui::Color32::from_rgb(255, 255, 255);
                    visuals.window_stroke =
                        egui::Stroke::new(1.0, egui::Color32::from_rgb(178, 186, 196));
                });
            }
            _ => {}
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
            jpeg_quality: self.advanced_options_dialog.default_jpeg_quality,
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
        let step_percent = self.advanced_options_dialog.zoom_step_percent;
        self.apply_zoom_change(|state| state.zoom_in_by_percent(step_percent));
    }

    fn zoom_out(&mut self) {
        let step_percent = self.advanced_options_dialog.zoom_step_percent;
        self.apply_zoom_change(|state| state.zoom_out_by_percent(step_percent));
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

    fn dispatch_batch_script(&mut self, script_path: PathBuf) {
        let request_id = self.next_request_id();
        self.pending.latest_batch = request_id;
        self.pending.batch_inflight = true;
        log::debug!(
            target: "imranview::ui",
            "queue batch script request_id={} script={}",
            request_id,
            script_path.display()
        );

        if self
            .worker_tx
            .send(WorkerCommand::RunBatchScript {
                request_id,
                script_path,
            })
            .is_err()
        {
            self.pending.batch_inflight = false;
            self.state.set_error("failed to queue batch-script command");
        }
    }

    fn batch_options_from_dialog(&self) -> BatchConvertOptions {
        BatchConvertOptions {
            input_dir: PathBuf::from(self.batch_dialog.input_dir.trim()),
            output_dir: PathBuf::from(self.batch_dialog.output_dir.trim()),
            output_format: self.batch_dialog.output_format,
            rename_prefix: self.batch_dialog.rename_prefix.clone(),
            start_index: self.batch_dialog.start_index,
            jpeg_quality: self.batch_dialog.jpeg_quality,
        }
    }

    fn apply_batch_options_to_dialog(&mut self, options: BatchConvertOptions) {
        self.batch_dialog.input_dir = options.input_dir.display().to_string();
        self.batch_dialog.output_dir = options.output_dir.display().to_string();
        self.batch_dialog.output_format = options.output_format;
        self.batch_dialog.rename_prefix = options.rename_prefix;
        self.batch_dialog.start_index = options.start_index;
        self.batch_dialog.jpeg_quality = options.jpeg_quality;
        self.batch_dialog.preview_count = None;
        self.batch_dialog.preview_for_input.clear();
        self.batch_dialog.preview_error = None;
    }

    fn save_batch_preset(&mut self, path: PathBuf) {
        let options = self.batch_options_from_dialog();
        if let Some(parent) = path.parent() {
            if !parent.as_os_str().is_empty() && fs::create_dir_all(parent).is_err() {
                self.state.set_error(format!(
                    "failed to create preset directory {}",
                    parent.display()
                ));
                return;
            }
        }
        match serde_json::to_string_pretty(&options) {
            Ok(json) => match fs::write(&path, json) {
                Ok(_) => {
                    self.info_message = Some(format!("Saved batch preset {}", path.display()));
                }
                Err(err) => self
                    .state
                    .set_error(format!("failed to write preset {}: {err}", path.display())),
            },
            Err(err) => self
                .state
                .set_error(format!("failed to serialize batch preset: {err}")),
        }
    }

    fn load_batch_preset(&mut self, path: PathBuf) {
        match fs::read_to_string(&path) {
            Ok(json) => match serde_json::from_str::<BatchConvertOptions>(&json) {
                Ok(options) => {
                    self.apply_batch_options_to_dialog(options);
                    self.info_message = Some(format!("Loaded batch preset {}", path.display()));
                }
                Err(err) => self
                    .state
                    .set_error(format!("invalid batch preset {}: {err}", path.display())),
            },
            Err(err) => self
                .state
                .set_error(format!("failed to read preset {}: {err}", path.display())),
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

    fn queue_utility_command(&mut self, build: impl FnOnce(u64) -> WorkerCommand) {
        let request_id = self.next_request_id();
        self.pending.latest_utility = request_id;
        self.pending.utility_inflight = true;
        if self.worker_tx.send(build(request_id)).is_err() {
            self.pending.utility_inflight = false;
            self.state.set_error("failed to queue utility command");
        }
    }

    fn collect_utility_input_paths(&self, include_folder_images: bool) -> Vec<PathBuf> {
        if include_folder_images {
            let folder_images = self.state.images_in_directory();
            if !folder_images.is_empty() {
                return folder_images.to_vec();
            }
        }
        self.state.current_file_path().into_iter().collect()
    }

    fn dispatch_capture_screenshot(
        &mut self,
        delay_ms: u64,
        region: Option<(u32, u32, u32, u32)>,
        output_path: Option<PathBuf>,
    ) {
        self.queue_utility_command(|request_id| WorkerCommand::CaptureScreenshot {
            request_id,
            delay_ms,
            region,
            output_path,
        });
    }

    #[allow(clippy::too_many_arguments)]
    fn dispatch_scan_to_directory(
        &mut self,
        output_dir: PathBuf,
        output_format: BatchOutputFormat,
        rename_prefix: String,
        start_index: u32,
        page_count: u32,
        jpeg_quality: u8,
        command_template: String,
    ) {
        self.queue_utility_command(|request_id| WorkerCommand::ScanToDirectory {
            request_id,
            output_dir,
            output_format,
            rename_prefix,
            start_index,
            page_count,
            jpeg_quality,
            command_template,
        });
    }

    fn dispatch_open_tiff_page(&mut self, path: PathBuf, page_index: u32) {
        self.queue_utility_command(|request_id| WorkerCommand::OpenTiffPage {
            request_id,
            path,
            page_index,
        });
    }

    fn dispatch_extract_tiff_pages(
        &mut self,
        path: PathBuf,
        output_dir: PathBuf,
        output_format: BatchOutputFormat,
        jpeg_quality: u8,
    ) {
        self.queue_utility_command(|request_id| WorkerCommand::ExtractTiffPages {
            request_id,
            path,
            output_dir,
            output_format,
            jpeg_quality,
        });
    }

    fn dispatch_create_multipage_pdf(
        &mut self,
        input_paths: Vec<PathBuf>,
        output_path: PathBuf,
        jpeg_quality: u8,
    ) {
        self.queue_utility_command(|request_id| WorkerCommand::CreateMultipagePdf {
            request_id,
            input_paths,
            output_path,
            jpeg_quality,
        });
    }

    fn dispatch_ocr(&mut self, path: PathBuf, language: String, output_path: Option<PathBuf>) {
        self.queue_utility_command(|request_id| WorkerCommand::RunOcr {
            request_id,
            path,
            language,
            output_path,
        });
    }

    fn dispatch_lossless_jpeg(
        &mut self,
        path: PathBuf,
        op: LosslessJpegOp,
        output_path: Option<PathBuf>,
    ) {
        self.queue_utility_command(|request_id| WorkerCommand::RunLosslessJpeg {
            request_id,
            path,
            op,
            output_path,
        });
    }

    fn dispatch_update_exif_date(&mut self, path: PathBuf, datetime: String) {
        self.queue_utility_command(|request_id| WorkerCommand::UpdateExifDate {
            request_id,
            path,
            datetime,
        });
    }

    fn dispatch_convert_color_profile(
        &mut self,
        path: PathBuf,
        output_path: PathBuf,
        source_profile: Option<PathBuf>,
        target_profile: PathBuf,
        rendering_intent: ColorRenderingIntent,
    ) {
        self.queue_utility_command(|request_id| WorkerCommand::ConvertColorProfile {
            request_id,
            path,
            output_path,
            source_profile,
            target_profile,
            rendering_intent: rendering_intent.as_worker_value().to_owned(),
        });
    }

    fn dispatch_stitch_panorama(
        &mut self,
        input_paths: Vec<PathBuf>,
        output_path: PathBuf,
        direction: PanoramaDirection,
        overlap_percent: f32,
    ) {
        self.queue_utility_command(|request_id| WorkerCommand::StitchPanorama {
            request_id,
            input_paths,
            output_path,
            direction,
            overlap_percent,
        });
    }

    #[allow(clippy::too_many_arguments)]
    fn dispatch_export_contact_sheet(
        &mut self,
        input_paths: Vec<PathBuf>,
        output_path: PathBuf,
        columns: u32,
        thumb_size: u32,
        include_labels: bool,
        background: [u8; 4],
        label_color: [u8; 4],
        jpeg_quality: u8,
    ) {
        self.queue_utility_command(|request_id| WorkerCommand::ExportContactSheet {
            request_id,
            input_paths,
            output_path,
            columns,
            thumb_size,
            include_labels,
            background,
            label_color,
            jpeg_quality,
        });
    }

    fn dispatch_export_html_gallery(
        &mut self,
        input_paths: Vec<PathBuf>,
        output_dir: PathBuf,
        title: String,
        thumb_width: u32,
    ) {
        self.queue_utility_command(|request_id| WorkerCommand::ExportHtmlGallery {
            request_id,
            input_paths,
            output_dir,
            title,
            thumb_width,
        });
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
        let wrap_navigation = self.advanced_options_dialog.browsing_wrap_navigation;
        let path_result = if forward {
            self.state.resolve_next_path_with_wrap(wrap_navigation)
        } else {
            self.state.resolve_previous_path_with_wrap(wrap_navigation)
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

        let wrap_navigation = self.advanced_options_dialog.browsing_wrap_navigation;
        if let Ok(next) = self.state.resolve_next_path_with_wrap(wrap_navigation) {
            candidates.push(next);
        }
        if let Ok(previous) = self.state.resolve_previous_path_with_wrap(wrap_navigation) {
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
                    self,
                    ctx,
                    format!("compare-image-{}", self.compare_texture_generation),
                    &loaded.preview_rgba,
                    loaded.preview_width,
                    loaded.preview_height,
                    true,
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
            WorkerResult::TiffPageLoaded {
                request_id,
                page_index,
                page_count,
                loaded,
            } => {
                if request_id != self.pending.latest_utility {
                    return;
                }
                self.pending.utility_inflight = false;
                self.tiff_dialog.page_count = Some(page_count);
                self.tiff_dialog.page_index = page_index;
                if let Err(err) = self.state.apply_transform_payload(loaded) {
                    self.state.set_error(err.to_string());
                }
                self.update_main_texture_from_state(ctx);
                self.info_message = Some(format!(
                    "Loaded TIFF page {} of {}",
                    page_index.saturating_add(1),
                    page_count
                ));
            }
            WorkerResult::UtilityCompleted {
                request_id,
                message,
                open_path,
            } => {
                if request_id != self.pending.latest_utility {
                    return;
                }
                self.pending.utility_inflight = false;
                self.info_message = Some(message);
                if let Some(path) = open_path {
                    if is_supported_image_path(&path) {
                        self.dispatch_open(path, false);
                    }
                }
            }
            WorkerResult::OcrCompleted {
                request_id,
                output_path,
                text,
            } => {
                if request_id != self.pending.latest_utility {
                    return;
                }
                self.pending.utility_inflight = false;
                self.ocr_dialog.preview_text = text.chars().take(16_384).collect();
                self.info_message = Some(match output_path {
                    Some(path) => format!("OCR complete. Text written to {}", path.display()),
                    None => "OCR complete.".to_owned(),
                });
            }
            WorkerResult::ThumbnailDecoded { path, payload } => {
                self.inflight_thumbnails.remove(&path);
                let texture = Self::texture_from_rgba(
                    self,
                    ctx,
                    format!("thumb-{}", path.display()),
                    &payload.rgba,
                    payload.width,
                    payload.height,
                    false,
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
                    (WorkerRequestKind::Utility, Some(id)) if id == self.pending.latest_utility => {
                        self.pending.utility_inflight = false;
                        self.state.set_error(error_message);
                    }
                    (WorkerRequestKind::Ocr, Some(id)) if id == self.pending.latest_utility => {
                        self.pending.utility_inflight = false;
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
            self,
            ctx,
            format!("main-image-{}", self.main_texture_generation),
            &rgba,
            width,
            height,
            true,
        );
        self.main_texture_generation = self.main_texture_generation.saturating_add(1);
        self.main_texture = Some(texture);
    }

    fn texture_from_rgba(
        &self,
        ctx: &egui::Context,
        name: String,
        rgba: &[u8],
        width: u32,
        height: u32,
        apply_color_pipeline: bool,
    ) -> egui::TextureHandle {
        let pixels = if apply_color_pipeline {
            self.apply_view_color_pipeline(rgba)
        } else {
            rgba.to_vec()
        };
        let color =
            egui::ColorImage::from_rgba_unmultiplied([width as usize, height as usize], &pixels);
        let options = if self.advanced_options_dialog.smooth_main_scaling {
            egui::TextureOptions::LINEAR
        } else {
            egui::TextureOptions::NEAREST
        };
        ctx.load_texture(name, color, options)
    }

    fn apply_view_color_pipeline(&self, rgba: &[u8]) -> Vec<u8> {
        if !self.advanced_options_dialog.enable_color_management {
            return rgba.to_vec();
        }

        let gamma = if self.advanced_options_dialog.simulate_srgb_output {
            self.advanced_options_dialog.display_gamma.clamp(1.6, 3.0)
        } else {
            1.0
        };
        if (gamma - 1.0).abs() < f32::EPSILON {
            return rgba.to_vec();
        }

        let inv_gamma = 1.0 / gamma;
        let mut lut = [0u8; 256];
        for (index, value) in lut.iter_mut().enumerate() {
            let normalized = index as f32 / 255.0;
            *value = (normalized.powf(inv_gamma) * 255.0)
                .round()
                .clamp(0.0, 255.0) as u8;
        }

        let mut output = rgba.to_vec();
        for pixel in output.chunks_exact_mut(4) {
            pixel[0] = lut[pixel[0] as usize];
            pixel[1] = lut[pixel[1] as usize];
            pixel[2] = lut[pixel[2] as usize];
        }
        output
    }

    fn run_shortcuts(&mut self, ctx: &egui::Context) {
        if shortcuts::trigger(ctx, ShortcutAction::SaveAs) {
            self.open_save_as_dialog();
        } else if shortcuts::trigger(ctx, ShortcutAction::Save) {
            self.dispatch_save(None, false, self.default_save_options());
        }
        if shortcuts::trigger(ctx, ShortcutAction::Undo) {
            self.undo_edit(ctx);
        } else if shortcuts::trigger(ctx, ShortcutAction::Redo) {
            self.redo_edit(ctx);
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

    fn undo_edit(&mut self, ctx: &egui::Context) {
        if self.pending.edit_inflight {
            self.state.set_error("wait for current edit to finish before undo");
            return;
        }
        if let Err(err) = self.state.undo_edit() {
            self.state.set_error(err.to_string());
            return;
        }
        self.update_main_texture_from_state(ctx);
    }

    fn redo_edit(&mut self, ctx: &egui::Context) {
        if self.pending.edit_inflight {
            self.state.set_error("wait for current edit to finish before redo");
            return;
        }
        if let Err(err) = self.state.redo_edit() {
            self.state.set_error(err.to_string());
            return;
        }
        self.update_main_texture_from_state(ctx);
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
        self.save_dialog.reopen_after_save = self.advanced_options_dialog.auto_reopen_after_save;
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

    fn open_text_tool_dialog(&mut self) {
        self.text_tool_dialog = TextToolDialogState::default();
        self.text_tool_dialog.open = true;
    }

    fn open_shape_tool_dialog(&mut self) {
        self.shape_tool_dialog = ShapeToolDialogState::default();
        self.shape_tool_dialog.open = true;
    }

    fn open_overlay_dialog(&mut self) {
        self.overlay_dialog = OverlayDialogState::default();
        self.overlay_dialog.open = true;
    }

    fn open_selection_workflow_dialog(&mut self) {
        let mut state = SelectionWorkflowDialogState::default();
        if let Some((w, h)) = self.state.original_dimensions() {
            state.width = w;
            state.height = h;
            state.radius = (w.min(h) / 4).max(1);
            state.polygon_points = format!(
                "{},{};{},{};{},{};{},{}",
                w / 4,
                h / 4,
                w.saturating_mul(3) / 4,
                h / 4,
                w.saturating_mul(7) / 8,
                h.saturating_mul(3) / 4,
                w / 8,
                h.saturating_mul(7) / 8
            );
        }
        state.open = true;
        self.selection_workflow_dialog = state;
    }

    fn parse_polygon_points(input: &str) -> Option<Vec<[u32; 2]>> {
        let mut points = Vec::new();
        for pair in input.split(';') {
            let trimmed = pair.trim();
            if trimmed.is_empty() {
                continue;
            }
            let mut parts = trimmed.split(',');
            let x = parts.next()?.trim().parse::<u32>().ok()?;
            let y = parts.next()?.trim().parse::<u32>().ok()?;
            if parts.next().is_some() {
                return None;
            }
            points.push([x, y]);
        }
        if points.len() >= 3 { Some(points) } else { None }
    }

    fn open_replace_color_dialog(&mut self) {
        self.replace_color_dialog = ReplaceColorDialogState::default();
        self.replace_color_dialog.open = true;
    }

    fn open_alpha_dialog(&mut self) {
        let mut state = AlphaDialogState::default();
        if let Some((w, h)) = self.state.original_dimensions() {
            state.region_width = w;
            state.region_height = h;
        }
        state.open = true;
        self.alpha_dialog = state;
    }

    fn open_effects_dialog(&mut self) {
        self.effects_dialog = EffectsDialogState::default();
        self.effects_dialog.open = true;
    }

    fn apply_effects_preset(&mut self, preset: EffectsPreset) {
        self.effects_dialog.preset = preset;
        match preset {
            EffectsPreset::Custom => {}
            EffectsPreset::Natural => {
                self.effects_dialog.blur_sigma = 0.0;
                self.effects_dialog.sharpen_sigma = 1.1;
                self.effects_dialog.sharpen_threshold = 1;
                self.effects_dialog.invert = false;
                self.effects_dialog.grayscale = false;
                self.effects_dialog.sepia_strength = 0.0;
                self.effects_dialog.posterize_levels = 0;
                self.effects_dialog.vignette_strength = 0.12;
                self.effects_dialog.tilt_shift_strength = 0.0;
                self.effects_dialog.stained_glass_strength = 0.0;
                self.effects_dialog.emboss_strength = 0.0;
                self.effects_dialog.edge_enhance_strength = 0.18;
                self.effects_dialog.oil_paint_strength = 0.0;
            }
            EffectsPreset::Vintage => {
                self.effects_dialog.blur_sigma = 0.3;
                self.effects_dialog.sharpen_sigma = 0.0;
                self.effects_dialog.sharpen_threshold = 1;
                self.effects_dialog.invert = false;
                self.effects_dialog.grayscale = false;
                self.effects_dialog.sepia_strength = 0.55;
                self.effects_dialog.posterize_levels = 12;
                self.effects_dialog.vignette_strength = 0.38;
                self.effects_dialog.tilt_shift_strength = 0.0;
                self.effects_dialog.stained_glass_strength = 0.0;
                self.effects_dialog.emboss_strength = 0.0;
                self.effects_dialog.edge_enhance_strength = 0.0;
                self.effects_dialog.oil_paint_strength = 0.0;
            }
            EffectsPreset::Dramatic => {
                self.effects_dialog.blur_sigma = 0.2;
                self.effects_dialog.sharpen_sigma = 1.8;
                self.effects_dialog.sharpen_threshold = 1;
                self.effects_dialog.invert = false;
                self.effects_dialog.grayscale = false;
                self.effects_dialog.sepia_strength = 0.0;
                self.effects_dialog.posterize_levels = 0;
                self.effects_dialog.vignette_strength = 0.44;
                self.effects_dialog.tilt_shift_strength = 0.0;
                self.effects_dialog.stained_glass_strength = 0.0;
                self.effects_dialog.emboss_strength = 0.18;
                self.effects_dialog.edge_enhance_strength = 0.65;
                self.effects_dialog.oil_paint_strength = 0.0;
            }
            EffectsPreset::Noir => {
                self.effects_dialog.blur_sigma = 0.0;
                self.effects_dialog.sharpen_sigma = 0.9;
                self.effects_dialog.sharpen_threshold = 1;
                self.effects_dialog.invert = false;
                self.effects_dialog.grayscale = true;
                self.effects_dialog.sepia_strength = 0.0;
                self.effects_dialog.posterize_levels = 0;
                self.effects_dialog.vignette_strength = 0.33;
                self.effects_dialog.tilt_shift_strength = 0.0;
                self.effects_dialog.stained_glass_strength = 0.0;
                self.effects_dialog.emboss_strength = 0.0;
                self.effects_dialog.edge_enhance_strength = 0.58;
                self.effects_dialog.oil_paint_strength = 0.0;
            }
            EffectsPreset::StainedGlass => {
                self.effects_dialog.blur_sigma = 0.0;
                self.effects_dialog.sharpen_sigma = 0.0;
                self.effects_dialog.sharpen_threshold = 1;
                self.effects_dialog.invert = false;
                self.effects_dialog.grayscale = false;
                self.effects_dialog.sepia_strength = 0.0;
                self.effects_dialog.posterize_levels = 8;
                self.effects_dialog.vignette_strength = 0.0;
                self.effects_dialog.tilt_shift_strength = 0.0;
                self.effects_dialog.stained_glass_strength = 0.75;
                self.effects_dialog.emboss_strength = 0.0;
                self.effects_dialog.edge_enhance_strength = 0.12;
                self.effects_dialog.oil_paint_strength = 0.0;
            }
            EffectsPreset::TiltShift => {
                self.effects_dialog.blur_sigma = 0.0;
                self.effects_dialog.sharpen_sigma = 0.7;
                self.effects_dialog.sharpen_threshold = 1;
                self.effects_dialog.invert = false;
                self.effects_dialog.grayscale = false;
                self.effects_dialog.sepia_strength = 0.0;
                self.effects_dialog.posterize_levels = 0;
                self.effects_dialog.vignette_strength = 0.28;
                self.effects_dialog.tilt_shift_strength = 0.72;
                self.effects_dialog.stained_glass_strength = 0.0;
                self.effects_dialog.emboss_strength = 0.0;
                self.effects_dialog.edge_enhance_strength = 0.2;
                self.effects_dialog.oil_paint_strength = 0.0;
            }
        }
    }

    fn effects_params_from_dialog(&self) -> EffectsParams {
        EffectsParams {
            blur_sigma: self.effects_dialog.blur_sigma,
            sharpen_sigma: self.effects_dialog.sharpen_sigma,
            sharpen_threshold: self.effects_dialog.sharpen_threshold,
            invert: self.effects_dialog.invert,
            grayscale: self.effects_dialog.grayscale,
            sepia_strength: self.effects_dialog.sepia_strength,
            posterize_levels: self.effects_dialog.posterize_levels,
            vignette_strength: self.effects_dialog.vignette_strength,
            tilt_shift_strength: self.effects_dialog.tilt_shift_strength,
            stained_glass_strength: self.effects_dialog.stained_glass_strength,
            emboss_strength: self.effects_dialog.emboss_strength,
            edge_enhance_strength: self.effects_dialog.edge_enhance_strength,
            oil_paint_strength: self.effects_dialog.oil_paint_strength,
        }
    }

    fn open_border_dialog(&mut self) {
        self.border_dialog = BorderDialogState::default();
        self.border_dialog.open = true;
    }

    fn open_canvas_dialog(&mut self) {
        if let Some((width, height)) = self.state.original_dimensions() {
            self.canvas_dialog.width = width;
            self.canvas_dialog.height = height;
        } else {
            self.canvas_dialog.width = 0;
            self.canvas_dialog.height = 0;
        }
        self.canvas_dialog.anchor = CanvasAnchor::Center;
        self.canvas_dialog.fill = egui::Color32::BLACK;
        self.canvas_dialog.open = true;
    }

    fn open_fine_rotate_dialog(&mut self) {
        self.fine_rotate_dialog = FineRotateDialogState::default();
        self.fine_rotate_dialog.open = true;
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

    fn open_search_dialog(&mut self) {
        self.search_dialog.results.clear();
        self.search_dialog.open = true;
        self.run_search_files();
    }

    fn open_screenshot_dialog(&mut self) {
        self.screenshot_dialog = ScreenshotDialogState::default();
        self.screenshot_dialog.open = true;
    }

    fn open_tiff_dialog(&mut self) {
        self.tiff_dialog = TiffDialogState::default();
        if let Some(path) = self.state.current_file_path() {
            self.tiff_dialog.path = path.display().to_string();
            if let Some(parent) = path.parent() {
                self.tiff_dialog.extract_output_dir =
                    parent.join("tiff-pages").display().to_string();
            }
        }
        self.tiff_dialog.open = true;
    }

    fn open_pdf_dialog(&mut self) {
        self.pdf_dialog = PdfDialogState::default();
        if let Some(directory) = self.state.current_directory_path() {
            self.pdf_dialog.output_path = directory.join("images.pdf").display().to_string();
        }
        self.pdf_dialog.open = true;
    }

    fn open_batch_scan_dialog(&mut self) {
        self.batch_scan_dialog = BatchScanDialogState::default();
        if let Some(directory) = self.state.current_directory_path() {
            self.batch_scan_dialog.input_dir = directory.display().to_string();
            self.batch_scan_dialog.output_dir = directory.join("scan-output").display().to_string();
        }
        self.batch_scan_dialog.open = true;
    }

    fn open_ocr_dialog(&mut self) {
        self.ocr_dialog.open = true;
        if self.ocr_dialog.output_path.is_empty() {
            if let Some(path) = self.state.current_file_path() {
                self.ocr_dialog.output_path = path.with_extension("txt").display().to_string();
            }
        }
    }

    fn open_lossless_jpeg_dialog(&mut self) {
        self.lossless_jpeg_dialog = LosslessJpegDialogState::default();
        if let Some(path) = self.state.current_file_path() {
            let suggested = path.with_file_name(format!(
                "{}-lossless.jpg",
                path.file_stem()
                    .map(|s| s.to_string_lossy().into_owned())
                    .unwrap_or_else(|| "image".to_owned())
            ));
            self.lossless_jpeg_dialog.output_path = suggested.display().to_string();
        }
        self.lossless_jpeg_dialog.open = true;
    }

    fn open_exif_date_dialog(&mut self) {
        self.exif_date_dialog = ExifDateDialogState::default();
        self.exif_date_dialog.open = true;
    }

    fn open_color_profile_dialog(&mut self) {
        let mut state = ColorProfileDialogState::default();
        if let Some(path) = self.state.current_file_path() {
            let suggested = path.with_file_name(format!(
                "{}-icc{}",
                path.file_stem()
                    .map(|s| s.to_string_lossy().into_owned())
                    .unwrap_or_else(|| "image".to_owned()),
                path.extension()
                    .map(|s| format!(".{}", s.to_string_lossy()))
                    .unwrap_or_else(|| ".png".to_owned())
            ));
            state.output_path = suggested.display().to_string();
        }
        state.open = true;
        self.color_profile_dialog = state;
    }

    fn open_panorama_dialog(&mut self) {
        self.panorama_dialog = PanoramaDialogState::default();
        if let Some(directory) = self.state.current_directory_path() {
            self.panorama_dialog.output_path = directory.join("panorama.jpg").display().to_string();
        }
        self.panorama_dialog.open = true;
    }

    fn open_perspective_dialog(&mut self) {
        let mut state = PerspectiveDialogState::default();
        if let Some((width, height)) = self.state.preview_dimensions() {
            let max_x = width.saturating_sub(1) as f32;
            let max_y = height.saturating_sub(1) as f32;
            state.top_left = [0.0, 0.0];
            state.top_right = [max_x, 0.0];
            state.bottom_right = [max_x, max_y];
            state.bottom_left = [0.0, max_y];
            state.output_width = width;
            state.output_height = height;
        }
        state.open = true;
        self.perspective_dialog = state;
    }

    fn open_magnifier_dialog(&mut self) {
        self.magnifier_dialog.open = true;
    }

    fn open_contact_sheet_dialog(&mut self) {
        self.contact_sheet_dialog = ContactSheetDialogState::default();
        if let Some(directory) = self.state.current_directory_path() {
            self.contact_sheet_dialog.output_path =
                directory.join("contact-sheet.jpg").display().to_string();
        }
        self.contact_sheet_dialog.open = true;
    }

    fn open_html_export_dialog(&mut self) {
        self.html_export_dialog = HtmlExportDialogState::default();
        if let Some(directory) = self.state.current_directory_path() {
            self.html_export_dialog.output_dir = directory.join("gallery").display().to_string();
        }
        self.html_export_dialog.open = true;
    }

    fn open_advanced_options_dialog(&mut self) {
        self.advanced_options_dialog.open = true;
    }

    fn run_search_files(&mut self) {
        let query = self.search_dialog.query.trim();
        let query_lower = query.to_ascii_lowercase();
        let extension_filters: Vec<String> = self
            .search_dialog
            .extension_filter
            .split(',')
            .map(str::trim)
            .filter(|value| !value.is_empty())
            .map(|value| value.trim_start_matches('.').to_ascii_lowercase())
            .collect();

        self.search_dialog.results = self
            .state
            .images_in_directory()
            .iter()
            .filter(|path| {
                if extension_filters.is_empty() {
                    true
                } else {
                    path.extension()
                        .map(|ext| ext.to_string_lossy().to_ascii_lowercase())
                        .is_some_and(|ext| extension_filters.iter().any(|item| item == &ext))
                }
            })
            .filter(|path| {
                if query.is_empty() {
                    return true;
                }
                let file_name = path
                    .file_name()
                    .map(|name| name.to_string_lossy().into_owned())
                    .unwrap_or_else(|| path.display().to_string());
                if self.search_dialog.case_sensitive {
                    file_name.contains(query)
                } else {
                    file_name.to_ascii_lowercase().contains(&query_lower)
                }
            })
            .cloned()
            .collect();
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
            WorkerRequestKind::Utility => format!("Utility workflow failed: {error}"),
            WorkerRequestKind::Ocr => format!("OCR failed: {error}"),
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

    #[cfg(any(target_os = "macos", target_os = "windows", target_os = "linux"))]
    fn maybe_install_native_menu(&mut self, frame: &eframe::Frame) {
        if self.native_menu_install_attempted {
            return;
        }
        self.native_menu_install_attempted = true;
        match NativeMenu::install(frame) {
            Ok(menu) => {
                self.native_menu = Some(menu);
                log::info!(
                    target: "imranview::ui",
                    "installed native menu integration"
                );
            }
            Err(err) => {
                log::warn!(
                    target: "imranview::ui",
                    "failed to install native menu; falling back to in-window menu: {err:#}"
                );
            }
        }
    }

    #[cfg(not(any(target_os = "macos", target_os = "windows", target_os = "linux")))]
    fn maybe_install_native_menu(&mut self, _frame: &eframe::Frame) {}

    fn should_draw_in_window_menu(&self) -> bool {
        #[cfg(any(target_os = "macos", target_os = "windows", target_os = "linux"))]
        {
            return self.native_menu.is_none();
        }

        #[cfg(not(any(target_os = "macos", target_os = "windows", target_os = "linux")))]
        {
            true
        }
    }

    #[cfg(any(target_os = "macos", target_os = "windows", target_os = "linux"))]
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
                NativeMenuAction::LosslessJpeg => self.open_lossless_jpeg_dialog(),
                NativeMenuAction::ChangeExifDate => self.open_exif_date_dialog(),
                NativeMenuAction::ConvertColorProfile => self.open_color_profile_dialog(),
                NativeMenuAction::RenameCurrent => self.open_rename_dialog(),
                NativeMenuAction::CopyCurrentToFolder => self.copy_current_to_dialog(),
                NativeMenuAction::MoveCurrentToFolder => self.move_current_to_dialog(),
                NativeMenuAction::DeleteCurrent => {
                    if self.advanced_options_dialog.confirm_delete {
                        self.confirm_delete_current = true;
                    } else {
                        self.delete_current_file();
                    }
                }
                NativeMenuAction::BatchConvert => self.open_batch_dialog(),
                NativeMenuAction::RunAutomationScript => {
                    let mut dialog =
                        rfd::FileDialog::new().set_title("Run batch automation script");
                    if let Some(directory) = self.state.preferred_open_directory() {
                        dialog = dialog.set_directory(directory);
                    }
                    dialog = dialog.add_filter("JSON", &["json"]);
                    if let Some(path) = dialog.pick_file() {
                        self.dispatch_batch_script(path);
                    }
                }
                NativeMenuAction::BatchScan => self.open_batch_scan_dialog(),
                NativeMenuAction::ScreenshotCapture => self.open_screenshot_dialog(),
                NativeMenuAction::OcrWorkflow => self.open_ocr_dialog(),
                NativeMenuAction::SearchFiles => self.open_search_dialog(),
                NativeMenuAction::MultipageTiff => self.open_tiff_dialog(),
                NativeMenuAction::MultipagePdf => self.open_pdf_dialog(),
                NativeMenuAction::ExportContactSheet => self.open_contact_sheet_dialog(),
                NativeMenuAction::ExportHtmlGallery => self.open_html_export_dialog(),
                NativeMenuAction::PrintCurrent => self.dispatch_print_current(),
                NativeMenuAction::LoadCompareImage => self.open_compare_path_dialog(),
                NativeMenuAction::ToggleCompareMode => {
                    self.compare_mode = !self.compare_mode;
                }
                NativeMenuAction::Exit => {
                    ctx.send_viewport_cmd(egui::ViewportCommand::Close);
                }
                NativeMenuAction::Undo => self.undo_edit(ctx),
                NativeMenuAction::Redo => self.redo_edit(ctx),
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
                NativeMenuAction::AddBorderFrame => self.open_border_dialog(),
                NativeMenuAction::CanvasSize => self.open_canvas_dialog(),
                NativeMenuAction::FineRotation => self.open_fine_rotate_dialog(),
                NativeMenuAction::TextTool => self.open_text_tool_dialog(),
                NativeMenuAction::ShapeTool => self.open_shape_tool_dialog(),
                NativeMenuAction::OverlayWatermark => self.open_overlay_dialog(),
                NativeMenuAction::SelectionWorkflows => self.open_selection_workflow_dialog(),
                NativeMenuAction::ReplaceColor => self.open_replace_color_dialog(),
                NativeMenuAction::AlphaTools => self.open_alpha_dialog(),
                NativeMenuAction::Effects => self.open_effects_dialog(),
                NativeMenuAction::PerspectiveCorrection => self.open_perspective_dialog(),
                NativeMenuAction::PanoramaStitch => self.open_panorama_dialog(),
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
                NativeMenuAction::Magnifier => self.open_magnifier_dialog(),
                NativeMenuAction::PerformanceSettings => self.open_performance_dialog(),
                NativeMenuAction::ClearRuntimeCaches => self.clear_runtime_caches(),
                NativeMenuAction::AdvancedSettings => self.open_advanced_options_dialog(),
            }
        }
    }

    #[cfg(not(any(target_os = "macos", target_os = "windows", target_os = "linux")))]
    fn handle_native_menu_events(&mut self, _ctx: &egui::Context) {}

    fn native_selected_surface(visuals: &egui::Visuals) -> egui::Color32 {
        let accent = visuals.selection.bg_fill;
        let alpha = if visuals.dark_mode { 112 } else { 70 };
        egui::Color32::from_rgba_unmultiplied(accent.r(), accent.g(), accent.b(), alpha)
    }

    fn native_bar_frame(ctx: &egui::Context) -> egui::Frame {
        egui::Frame::new()
            .fill(ctx.style().visuals.panel_fill)
            .inner_margin(egui::Margin::symmetric(8, 2))
    }

    fn dialog_viewport_builder(
        ctx: &egui::Context,
        title: &'static str,
        size: egui::Vec2,
    ) -> egui::ViewportBuilder {
        let mut builder = egui::ViewportBuilder::default()
            .with_title(title)
            .with_inner_size(size)
            .with_min_inner_size(size)
            .with_resizable(false)
            .with_minimize_button(false)
            .with_maximize_button(false)
            .with_close_button(true)
            .with_active(true);

        if let Some(root_outer) = ctx.input(|i| i.viewport().outer_rect) {
            let centered = egui::pos2(
                root_outer.center().x - size.x * 0.5,
                root_outer.center().y - size.y * 0.5,
            );
            builder = builder.with_position(centered);
        }

        builder
    }

    fn show_popup_window(
        &mut self,
        ctx: &egui::Context,
        id_source: &'static str,
        title: &'static str,
        size: egui::Vec2,
        open: &mut bool,
        mut add_contents: impl FnMut(&mut Self, &egui::Context, &mut egui::Ui, &mut bool),
    ) {
        let viewport_id = egui::ViewportId::from_hash_of(id_source);

        if !*open {
            ctx.send_viewport_cmd_to(viewport_id, egui::ViewportCommand::Close);
            return;
        }

        let builder = Self::dialog_viewport_builder(ctx, title, size);
        ctx.show_viewport_immediate(viewport_id, builder, |viewport_ctx, class| {
            if viewport_ctx.input(|i| i.viewport().close_requested()) {
                *open = false;
                return;
            }

            match class {
                egui::ViewportClass::Embedded => {
                    let mut embedded_open = *open;
                    let mut requested_close = false;
                    centered_dialog_window(title)
                        .open(&mut embedded_open)
                        .default_size(size)
                        .show(viewport_ctx, |ui| {
                            let mut content_open = true;
                            add_contents(self, viewport_ctx, ui, &mut content_open);
                            if !content_open {
                                requested_close = true;
                            }
                        });
                    *open = embedded_open && !requested_close;
                }
                egui::ViewportClass::Root
                | egui::ViewportClass::Deferred
                | egui::ViewportClass::Immediate => {
                    egui::CentralPanel::default()
                        .frame(egui::Frame::new().inner_margin(egui::Margin::symmetric(10, 8)))
                        .show(viewport_ctx, |ui| {
                            add_contents(self, viewport_ctx, ui, open);
                        });
                }
            }
        });

        if !*open {
            ctx.send_viewport_cmd_to(viewport_id, egui::ViewportCommand::Close);
        }
    }

    fn draw_menu(&mut self, ctx: &egui::Context) {
        egui::TopBottomPanel::top("menu")
            .frame(Self::native_bar_frame(ctx))
            .show(ctx, |ui| {
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
                                egui::Button::new(menu_item_label(
                                    ctx,
                                    ShortcutAction::Save,
                                    "Save",
                                )),
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
                        if ui
                            .add_enabled(
                                self.state.has_image(),
                                egui::Button::new("Lossless JPEG transform..."),
                            )
                            .clicked()
                        {
                            self.open_lossless_jpeg_dialog();
                            ui.close_menu();
                        }
                        if ui
                            .add_enabled(
                                self.state.has_image(),
                                egui::Button::new("Change EXIF date/time..."),
                            )
                            .clicked()
                        {
                            self.open_exif_date_dialog();
                            ui.close_menu();
                        }
                        if ui
                            .add_enabled(
                                self.state.has_image(),
                                egui::Button::new("Convert color profile..."),
                            )
                            .clicked()
                        {
                            self.open_color_profile_dialog();
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
                            if self.advanced_options_dialog.confirm_delete {
                                self.confirm_delete_current = true;
                            } else {
                                self.delete_current_file();
                            }
                            ui.close_menu();
                        }
                        if ui.button("Batch convert / rename...").clicked() {
                            self.open_batch_dialog();
                            ui.close_menu();
                        }
                        if ui.button("Run automation script...").clicked() {
                            let mut dialog =
                                rfd::FileDialog::new().set_title("Run batch automation script");
                            if let Some(directory) = self.state.preferred_open_directory() {
                                dialog = dialog.set_directory(directory);
                            }
                            dialog = dialog.add_filter("JSON", &["json"]);
                            if let Some(path) = dialog.pick_file() {
                                self.dispatch_batch_script(path);
                            }
                            ui.close_menu();
                        }
                        if ui.button("Batch scan/import...").clicked() {
                            self.open_batch_scan_dialog();
                            ui.close_menu();
                        }
                        if ui.button("Screenshot capture...").clicked() {
                            self.open_screenshot_dialog();
                            ui.close_menu();
                        }
                        if ui
                            .add_enabled(self.state.has_image(), egui::Button::new("OCR..."))
                            .clicked()
                        {
                            self.open_ocr_dialog();
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
                        if ui
                            .add_enabled(
                                !self.state.images_in_directory().is_empty(),
                                egui::Button::new("Search files..."),
                            )
                            .clicked()
                        {
                            self.open_search_dialog();
                            ui.close_menu();
                        }
                        if ui
                            .add_enabled(
                                self.state.has_image(),
                                egui::Button::new("Multipage TIFF..."),
                            )
                            .clicked()
                        {
                            self.open_tiff_dialog();
                            ui.close_menu();
                        }
                        if ui
                            .add_enabled(
                                self.state.has_image(),
                                egui::Button::new("Create multipage PDF..."),
                            )
                            .clicked()
                        {
                            self.open_pdf_dialog();
                            ui.close_menu();
                        }
                        if ui
                            .add_enabled(
                                self.state.has_image(),
                                egui::Button::new("Export contact sheet..."),
                            )
                            .clicked()
                        {
                            self.open_contact_sheet_dialog();
                            ui.close_menu();
                        }
                        if ui
                            .add_enabled(
                                self.state.has_image(),
                                egui::Button::new("Export HTML gallery..."),
                            )
                            .clicked()
                        {
                            self.open_html_export_dialog();
                            ui.close_menu();
                        }
                        ui.separator();
                        if ui.button("Exit").clicked() {
                            ctx.send_viewport_cmd(egui::ViewportCommand::Close);
                        }
                    });

                    ui.menu_button("Edit", |ui| {
                        if ui
                            .add_enabled(
                                self.state.can_undo(),
                                egui::Button::new(menu_item_label(
                                    ctx,
                                    ShortcutAction::Undo,
                                    "Undo",
                                )),
                            )
                            .clicked()
                        {
                            self.undo_edit(ctx);
                            ui.close_menu();
                        }
                        if ui
                            .add_enabled(
                                self.state.can_redo(),
                                egui::Button::new(menu_item_label(
                                    ctx,
                                    ShortcutAction::Redo,
                                    "Redo",
                                )),
                            )
                            .clicked()
                        {
                            self.redo_edit(ctx);
                            ui.close_menu();
                        }
                        ui.separator();
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
                            .add_enabled(
                                self.state.has_image(),
                                egui::Button::new("Flip Horizontal"),
                            )
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
                        if ui
                            .add_enabled(
                                self.state.has_image(),
                                egui::Button::new("Add border / frame..."),
                            )
                            .clicked()
                        {
                            self.open_border_dialog();
                            ui.close_menu();
                        }
                        if ui
                            .add_enabled(
                                self.state.has_image(),
                                egui::Button::new("Canvas size..."),
                            )
                            .clicked()
                        {
                            self.open_canvas_dialog();
                            ui.close_menu();
                        }
                        if ui
                            .add_enabled(
                                self.state.has_image(),
                                egui::Button::new("Fine rotation..."),
                            )
                            .clicked()
                        {
                            self.open_fine_rotate_dialog();
                            ui.close_menu();
                        }
                        ui.separator();
                        if ui
                            .add_enabled(self.state.has_image(), egui::Button::new("Text tool..."))
                            .clicked()
                        {
                            self.open_text_tool_dialog();
                            ui.close_menu();
                        }
                        if ui
                            .add_enabled(self.state.has_image(), egui::Button::new("Shape tool..."))
                            .clicked()
                        {
                            self.open_shape_tool_dialog();
                            ui.close_menu();
                        }
                        if ui
                            .add_enabled(
                                self.state.has_image(),
                                egui::Button::new("Overlay / watermark..."),
                            )
                            .clicked()
                        {
                            self.open_overlay_dialog();
                            ui.close_menu();
                        }
                        if ui
                            .add_enabled(
                                self.state.has_image(),
                                egui::Button::new("Selection workflows..."),
                            )
                            .clicked()
                        {
                            self.open_selection_workflow_dialog();
                            ui.close_menu();
                        }
                        if ui
                            .add_enabled(
                                self.state.has_image(),
                                egui::Button::new("Replace color..."),
                            )
                            .clicked()
                        {
                            self.open_replace_color_dialog();
                            ui.close_menu();
                        }
                        if ui
                            .add_enabled(
                                self.state.has_image(),
                                egui::Button::new("Alpha tools..."),
                            )
                            .clicked()
                        {
                            self.open_alpha_dialog();
                            ui.close_menu();
                        }
                        if ui
                            .add_enabled(self.state.has_image(), egui::Button::new("Effects..."))
                            .clicked()
                        {
                            self.open_effects_dialog();
                            ui.close_menu();
                        }
                        if ui
                            .add_enabled(
                                self.state.has_image(),
                                egui::Button::new("Perspective correction..."),
                            )
                            .clicked()
                        {
                            self.open_perspective_dialog();
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
                        if ui
                            .add_enabled(
                                self.state.has_image(),
                                egui::Button::new("Panorama stitch..."),
                            )
                            .clicked()
                        {
                            self.open_panorama_dialog();
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
                        if ui.button("Zoom magnifier...").clicked() {
                            self.open_magnifier_dialog();
                            ui.close_menu();
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
                        if ui.button("Advanced settings...").clicked() {
                            self.open_advanced_options_dialog();
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
        let mut button = egui::Button::image(image).min_size(egui::vec2(28.0, 24.0));
        if selected {
            button = button.fill(Self::native_selected_surface(ui.visuals()));
        }
        ui.add_enabled(enabled, button).on_hover_text(tooltip)
    }

    fn draw_toolbar(&mut self, ctx: &egui::Context) {
        if !self.state.show_toolbar() {
            return;
        }
        if self.advanced_options_dialog.hide_toolbar_in_fullscreen
            && ctx.input(|i| i.viewport().fullscreen).unwrap_or(false)
        {
            return;
        }

        egui::TopBottomPanel::top("toolbar")
            .frame(Self::native_bar_frame(ctx))
            .exact_height(TOOLBAR_PANEL_HEIGHT)
            .show(ctx, |ui| {
                ui.spacing_mut().item_spacing = egui::vec2(6.0, 0.0);
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
            .frame(Self::native_bar_frame(ctx))
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
        let mut frame = egui::Frame::new()
            .inner_margin(egui::Margin::symmetric(6, 5))
            .fill(ui.visuals().extreme_bg_color)
            .stroke(ui.visuals().widgets.noninteractive.bg_stroke)
            .corner_radius(egui::CornerRadius::same(platform_widget_corner_radius()));
        if entry.current {
            frame = frame.fill(Self::native_selected_surface(ui.visuals()));
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
            if self.advanced_options_dialog.checkerboard_background {
                self.paint_checkerboard_background(ui);
            }
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
                let main_image_rect: egui::Rect;

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
                    let response = ui
                        .centered_and_justified(|ui| {
                            ui.add(egui::Image::new((texture.id(), desired_size)))
                        })
                        .inner;
                    main_image_rect = response.rect;
                } else {
                    desired_size *= self.state.zoom_factor();
                    let output = egui::ScrollArea::both()
                        .id_salt("main-viewer-scroll")
                        .scroll_offset(self.main_scroll_offset)
                        .show(ui, |ui| {
                            ui.add(egui::Image::new((texture.id(), desired_size)))
                        });
                    self.main_scroll_offset = output.state.offset;
                    self.main_viewport_size = output.inner_rect.size();
                    main_image_rect = output.inner.rect;
                }
                self.draw_magnifier_overlay(ctx, texture, main_image_rect);
            } else {
                ui.centered_and_justified(|ui| {
                    ui.label("ImranView\n\nFile > Open...");
                });
            }
        });
    }

    fn paint_checkerboard_background(&self, ui: &mut egui::Ui) {
        let rect = ui.max_rect();
        let tile = 18.0;
        let dark = ui.visuals().faint_bg_color;
        let light = ui.visuals().extreme_bg_color;
        let painter = ui.painter();
        let start_x = (rect.min.x / tile).floor() as i32;
        let end_x = (rect.max.x / tile).ceil() as i32;
        let start_y = (rect.min.y / tile).floor() as i32;
        let end_y = (rect.max.y / tile).ceil() as i32;
        for ty in start_y..end_y {
            for tx in start_x..end_x {
                let color = if (tx + ty) % 2 == 0 { dark } else { light };
                let min = egui::pos2(tx as f32 * tile, ty as f32 * tile);
                let tile_rect = egui::Rect::from_min_size(min, egui::vec2(tile, tile));
                painter.rect_filled(tile_rect.intersect(rect), 0.0, color);
            }
        }
    }

    fn draw_magnifier_overlay(
        &self,
        ctx: &egui::Context,
        texture: &egui::TextureHandle,
        image_rect: egui::Rect,
    ) {
        if !self.magnifier_dialog.enabled || !image_rect.is_positive() {
            return;
        }
        let Some(pointer) = ctx.input(|input| input.pointer.hover_pos()) else {
            return;
        };
        if !image_rect.contains(pointer) {
            return;
        }

        let zoom = self.magnifier_dialog.zoom.clamp(1.1, 16.0);
        let lens_size = self.magnifier_dialog.size.clamp(80.0, 360.0);
        let mut lens_rect = egui::Rect::from_min_size(
            pointer + egui::vec2(20.0, 20.0),
            egui::vec2(lens_size, lens_size),
        );

        if let Some(viewport_rect) = ctx.input(|input| input.viewport().inner_rect) {
            if lens_rect.max.x > viewport_rect.max.x {
                lens_rect =
                    lens_rect.translate(egui::vec2(viewport_rect.max.x - lens_rect.max.x, 0.0));
            }
            if lens_rect.max.y > viewport_rect.max.y {
                lens_rect =
                    lens_rect.translate(egui::vec2(0.0, viewport_rect.max.y - lens_rect.max.y));
            }
        }

        let rel_x = ((pointer.x - image_rect.min.x) / image_rect.width()).clamp(0.0, 1.0);
        let rel_y = ((pointer.y - image_rect.min.y) / image_rect.height()).clamp(0.0, 1.0);
        let src_w = (1.0 / zoom).clamp(0.01, 1.0);
        let src_h = (1.0 / zoom).clamp(0.01, 1.0);
        let min_u = (rel_x - src_w * 0.5).clamp(0.0, 1.0 - src_w);
        let min_v = (rel_y - src_h * 0.5).clamp(0.0, 1.0 - src_h);
        let max_u = (min_u + src_w).clamp(0.0, 1.0);
        let max_v = (min_v + src_h).clamp(0.0, 1.0);

        let painter = ctx.layer_painter(egui::LayerId::new(
            egui::Order::Foreground,
            egui::Id::new("magnifier-overlay"),
        ));
        painter.rect(
            lens_rect.expand(2.0),
            egui::CornerRadius::same(8),
            egui::Color32::from_black_alpha(180),
            egui::Stroke::new(1.0, egui::Color32::from_gray(220)),
            egui::StrokeKind::Outside,
        );
        painter.image(
            texture.id(),
            lens_rect,
            egui::Rect::from_min_max(egui::pos2(min_u, min_v), egui::pos2(max_u, max_v)),
            egui::Color32::WHITE,
        );
        painter.text(
            lens_rect.left_top() + egui::vec2(8.0, 8.0),
            egui::Align2::LEFT_TOP,
            format!("{:.1}x", zoom),
            egui::FontId::proportional(11.0),
            egui::Color32::from_gray(245),
        );
    }

    fn draw_about_window(&mut self, ctx: &egui::Context) {
        if !self.show_about_window {
            return;
        }

        let mut open = self.show_about_window;
        self.show_popup_window(
            ctx,
            "popup.about",
            "About ImranView",
            egui::vec2(440.0, 260.0),
            &mut open,
            |app, _ctx, ui, _open| {
                ui.horizontal(|ui| {
                    if let Some(icon) = &app.about_icon_texture {
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
            },
        );

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
        self.show_popup_window(
            ctx,
            "popup.resize",
            "Resize / Resample",
            egui::vec2(460.0, 280.0),
            &mut open,
            |app, _ctx, ui, open_state| {
                let aspect = if let Some((width, height)) = app.state.original_dimensions() {
                    if height > 0 {
                        width as f32 / height as f32
                    } else {
                        1.0
                    }
                } else {
                    1.0
                };

                let before_width = app.resize_dialog.width;
                let before_height = app.resize_dialog.height;
                ui.horizontal(|ui| {
                    ui.label("Width:");
                    ui.add(egui::DragValue::new(&mut app.resize_dialog.width).range(1..=65535));
                    ui.label("Height:");
                    ui.add(egui::DragValue::new(&mut app.resize_dialog.height).range(1..=65535));
                });
                ui.checkbox(&mut app.resize_dialog.keep_aspect, "Keep aspect ratio");
                if app.resize_dialog.keep_aspect {
                    if app.resize_dialog.width != before_width && aspect > 0.0 {
                        app.resize_dialog.height =
                            ((app.resize_dialog.width as f32 / aspect).round().max(1.0)) as u32;
                    } else if app.resize_dialog.height != before_height && aspect > 0.0 {
                        app.resize_dialog.width =
                            ((app.resize_dialog.height as f32 * aspect).round().max(1.0)) as u32;
                    }
                }

                ui.separator();
                ui.label("Filter:");
                ui.horizontal_wrapped(|ui| {
                    ui.selectable_value(
                        &mut app.resize_dialog.filter,
                        ResizeFilter::Nearest,
                        "Nearest",
                    );
                    ui.selectable_value(
                        &mut app.resize_dialog.filter,
                        ResizeFilter::Triangle,
                        "Triangle",
                    );
                    ui.selectable_value(
                        &mut app.resize_dialog.filter,
                        ResizeFilter::CatmullRom,
                        "CatmullRom",
                    );
                    ui.selectable_value(
                        &mut app.resize_dialog.filter,
                        ResizeFilter::Gaussian,
                        "Gaussian",
                    );
                    ui.selectable_value(
                        &mut app.resize_dialog.filter,
                        ResizeFilter::Lanczos3,
                        "Lanczos3",
                    );
                });

                ui.separator();
                if ui.button("Apply").clicked() {
                    app.dispatch_transform(TransformOp::Resize {
                        width: app.resize_dialog.width.max(1),
                        height: app.resize_dialog.height.max(1),
                        filter: app.resize_dialog.filter,
                    });
                    *open_state = false;
                }
            },
        );
        self.resize_dialog.open = open;
    }

    fn draw_crop_dialog(&mut self, ctx: &egui::Context) {
        if !self.crop_dialog.open {
            return;
        }

        let mut open = self.crop_dialog.open;
        self.show_popup_window(
            ctx,
            "popup.crop",
            "Crop",
            egui::vec2(420.0, 180.0),
            &mut open,
            |app, _ctx, ui, open_state| {
                ui.horizontal(|ui| {
                    ui.label("X:");
                    ui.add(egui::DragValue::new(&mut app.crop_dialog.x).range(0..=65535));
                    ui.label("Y:");
                    ui.add(egui::DragValue::new(&mut app.crop_dialog.y).range(0..=65535));
                });
                ui.horizontal(|ui| {
                    ui.label("Width:");
                    ui.add(egui::DragValue::new(&mut app.crop_dialog.width).range(1..=65535));
                    ui.label("Height:");
                    ui.add(egui::DragValue::new(&mut app.crop_dialog.height).range(1..=65535));
                });

                if ui.button("Apply").clicked() {
                    app.dispatch_transform(TransformOp::Crop {
                        x: app.crop_dialog.x,
                        y: app.crop_dialog.y,
                        width: app.crop_dialog.width.max(1),
                        height: app.crop_dialog.height.max(1),
                    });
                    *open_state = false;
                }
            },
        );
        self.crop_dialog.open = open;
    }

    fn draw_color_dialog(&mut self, ctx: &egui::Context) {
        if !self.color_dialog.open {
            return;
        }

        let mut open = self.color_dialog.open;
        self.show_popup_window(
            ctx,
            "popup.color",
            "Color Corrections",
            egui::vec2(460.0, 260.0),
            &mut open,
            |app, _ctx, ui, open_state| {
                ui.add(
                    egui::Slider::new(&mut app.color_dialog.brightness, -255..=255)
                        .text("Brightness"),
                );
                ui.add(
                    egui::Slider::new(&mut app.color_dialog.contrast, -100.0..=100.0)
                        .text("Contrast"),
                );
                ui.add(
                    egui::Slider::new(&mut app.color_dialog.gamma, 0.1..=5.0)
                        .text("Gamma")
                        .fixed_decimals(2),
                );
                ui.add(
                    egui::Slider::new(&mut app.color_dialog.saturation, 0.0..=3.0)
                        .text("Saturation")
                        .fixed_decimals(2),
                );
                ui.checkbox(&mut app.color_dialog.grayscale, "Grayscale");

                if ui.button("Apply").clicked() {
                    app.dispatch_transform(TransformOp::ColorAdjust(ColorAdjustParams {
                        brightness: app.color_dialog.brightness,
                        contrast: app.color_dialog.contrast,
                        gamma: app.color_dialog.gamma,
                        saturation: app.color_dialog.saturation,
                        grayscale: app.color_dialog.grayscale,
                    }));
                    *open_state = false;
                }
            },
        );
        self.color_dialog.open = open;
    }

    fn draw_border_dialog(&mut self, ctx: &egui::Context) {
        if !self.border_dialog.open {
            return;
        }

        let mut open = self.border_dialog.open;
        self.show_popup_window(
            ctx,
            "popup.border",
            "Add Border / Frame",
            egui::vec2(430.0, 250.0),
            &mut open,
            |app, _ctx, ui, open_state| {
                ui.horizontal(|ui| {
                    ui.label("Left");
                    ui.add(egui::DragValue::new(&mut app.border_dialog.left).range(0..=65535));
                    ui.label("Right");
                    ui.add(egui::DragValue::new(&mut app.border_dialog.right).range(0..=65535));
                });
                ui.horizontal(|ui| {
                    ui.label("Top");
                    ui.add(egui::DragValue::new(&mut app.border_dialog.top).range(0..=65535));
                    ui.label("Bottom");
                    ui.add(egui::DragValue::new(&mut app.border_dialog.bottom).range(0..=65535));
                });
                ui.separator();
                ui.horizontal(|ui| {
                    ui.label("Border color");
                    ui.color_edit_button_srgba(&mut app.border_dialog.color);
                });
                ui.separator();
                if ui.button("Apply").clicked() {
                    if app.border_dialog.left == 0
                        && app.border_dialog.right == 0
                        && app.border_dialog.top == 0
                        && app.border_dialog.bottom == 0
                    {
                        app.state
                            .set_error("set at least one border side above zero");
                    } else {
                        app.dispatch_transform(TransformOp::AddBorder {
                            left: app.border_dialog.left,
                            right: app.border_dialog.right,
                            top: app.border_dialog.top,
                            bottom: app.border_dialog.bottom,
                            color: app.border_dialog.color.to_array(),
                        });
                        *open_state = false;
                    }
                }
            },
        );
        self.border_dialog.open = open;
    }

    fn draw_canvas_dialog(&mut self, ctx: &egui::Context) {
        if !self.canvas_dialog.open {
            return;
        }

        let mut open = self.canvas_dialog.open;
        self.show_popup_window(
            ctx,
            "popup.canvas",
            "Canvas Size",
            egui::vec2(520.0, 330.0),
            &mut open,
            |app, _ctx, ui, open_state| {
                ui.horizontal(|ui| {
                    ui.label("Width");
                    ui.add(egui::DragValue::new(&mut app.canvas_dialog.width).range(1..=65535));
                    ui.label("Height");
                    ui.add(egui::DragValue::new(&mut app.canvas_dialog.height).range(1..=65535));
                });
                ui.separator();
                ui.label("Anchor");
                ui.horizontal_wrapped(|ui| {
                    ui.selectable_value(
                        &mut app.canvas_dialog.anchor,
                        CanvasAnchor::TopLeft,
                        "Top-left",
                    );
                    ui.selectable_value(
                        &mut app.canvas_dialog.anchor,
                        CanvasAnchor::TopCenter,
                        "Top-center",
                    );
                    ui.selectable_value(
                        &mut app.canvas_dialog.anchor,
                        CanvasAnchor::TopRight,
                        "Top-right",
                    );
                    ui.selectable_value(
                        &mut app.canvas_dialog.anchor,
                        CanvasAnchor::CenterLeft,
                        "Center-left",
                    );
                    ui.selectable_value(
                        &mut app.canvas_dialog.anchor,
                        CanvasAnchor::Center,
                        "Center",
                    );
                    ui.selectable_value(
                        &mut app.canvas_dialog.anchor,
                        CanvasAnchor::CenterRight,
                        "Center-right",
                    );
                    ui.selectable_value(
                        &mut app.canvas_dialog.anchor,
                        CanvasAnchor::BottomLeft,
                        "Bottom-left",
                    );
                    ui.selectable_value(
                        &mut app.canvas_dialog.anchor,
                        CanvasAnchor::BottomCenter,
                        "Bottom-center",
                    );
                    ui.selectable_value(
                        &mut app.canvas_dialog.anchor,
                        CanvasAnchor::BottomRight,
                        "Bottom-right",
                    );
                });
                ui.separator();
                ui.horizontal(|ui| {
                    ui.label("Fill color");
                    ui.color_edit_button_srgba(&mut app.canvas_dialog.fill);
                });
                ui.separator();
                if ui.button("Apply").clicked() {
                    if app.canvas_dialog.width == 0 || app.canvas_dialog.height == 0 {
                        app.state
                            .set_error("canvas dimensions must be greater than zero");
                    } else {
                        app.dispatch_transform(TransformOp::CanvasSize {
                            width: app.canvas_dialog.width,
                            height: app.canvas_dialog.height,
                            anchor: app.canvas_dialog.anchor,
                            fill: app.canvas_dialog.fill.to_array(),
                        });
                        *open_state = false;
                    }
                }
            },
        );
        self.canvas_dialog.open = open;
    }

    fn draw_fine_rotate_dialog(&mut self, ctx: &egui::Context) {
        if !self.fine_rotate_dialog.open {
            return;
        }

        let mut open = self.fine_rotate_dialog.open;
        self.show_popup_window(
            ctx,
            "popup.fine-rotate",
            "Fine Rotation",
            egui::vec2(470.0, 280.0),
            &mut open,
            |app, _ctx, ui, open_state| {
                ui.add(
                    egui::Slider::new(&mut app.fine_rotate_dialog.angle_degrees, -180.0..=180.0)
                        .text("Angle (degrees)")
                        .fixed_decimals(2),
                );
                ui.horizontal(|ui| {
                    ui.label("Interpolation");
                    ui.selectable_value(
                        &mut app.fine_rotate_dialog.interpolation,
                        RotationInterpolation::Bilinear,
                        "Bilinear",
                    );
                    ui.selectable_value(
                        &mut app.fine_rotate_dialog.interpolation,
                        RotationInterpolation::Nearest,
                        "Nearest",
                    );
                });
                ui.checkbox(
                    &mut app.fine_rotate_dialog.expand_canvas,
                    "Expand canvas to fit",
                );
                ui.horizontal(|ui| {
                    ui.label("Background fill");
                    ui.color_edit_button_srgba(&mut app.fine_rotate_dialog.fill);
                });
                ui.separator();
                if ui.button("Apply").clicked() {
                    app.dispatch_transform(TransformOp::RotateFine {
                        angle_degrees: app.fine_rotate_dialog.angle_degrees,
                        interpolation: app.fine_rotate_dialog.interpolation,
                        expand_canvas: app.fine_rotate_dialog.expand_canvas,
                        fill: app.fine_rotate_dialog.fill.to_array(),
                    });
                    *open_state = false;
                }
            },
        );
        self.fine_rotate_dialog.open = open;
    }

    fn draw_text_tool_dialog(&mut self, ctx: &egui::Context) {
        if !self.text_tool_dialog.open {
            return;
        }

        let mut open = self.text_tool_dialog.open;
        self.show_popup_window(
            ctx,
            "popup.text-tool",
            "Text Tool",
            egui::vec2(500.0, 280.0),
            &mut open,
            |app, _ctx, ui, open_state| {
                ui.label("Text");
                ui.text_edit_multiline(&mut app.text_tool_dialog.text);
                ui.horizontal(|ui| {
                    ui.label("X");
                    ui.add(egui::DragValue::new(&mut app.text_tool_dialog.x).range(-65535..=65535));
                    ui.label("Y");
                    ui.add(egui::DragValue::new(&mut app.text_tool_dialog.y).range(-65535..=65535));
                });
                ui.horizontal(|ui| {
                    ui.label("Scale");
                    ui.add(egui::Slider::new(&mut app.text_tool_dialog.scale, 1..=16));
                    ui.label("Color");
                    ui.color_edit_button_srgba(&mut app.text_tool_dialog.color);
                });
                ui.separator();
                if ui.button("Apply").clicked() {
                    if app.text_tool_dialog.text.trim().is_empty() {
                        app.state.set_error("text content is required");
                    } else {
                        app.dispatch_transform(TransformOp::AddText {
                            text: app.text_tool_dialog.text.clone(),
                            x: app.text_tool_dialog.x,
                            y: app.text_tool_dialog.y,
                            scale: app.text_tool_dialog.scale,
                            color: app.text_tool_dialog.color.to_array(),
                        });
                        *open_state = false;
                    }
                }
            },
        );
        self.text_tool_dialog.open = open;
    }

    fn draw_shape_tool_dialog(&mut self, ctx: &egui::Context) {
        if !self.shape_tool_dialog.open {
            return;
        }

        let mut open = self.shape_tool_dialog.open;
        self.show_popup_window(
            ctx,
            "popup.shape-tool",
            "Shape Tool",
            egui::vec2(560.0, 320.0),
            &mut open,
            |app, _ctx, ui, open_state| {
                ui.horizontal_wrapped(|ui| {
                    ui.selectable_value(&mut app.shape_tool_dialog.kind, ShapeKind::Line, "Line");
                    ui.selectable_value(
                        &mut app.shape_tool_dialog.kind,
                        ShapeKind::Rectangle,
                        "Rectangle",
                    );
                    ui.selectable_value(
                        &mut app.shape_tool_dialog.kind,
                        ShapeKind::Ellipse,
                        "Ellipse",
                    );
                    ui.selectable_value(&mut app.shape_tool_dialog.kind, ShapeKind::Arrow, "Arrow");
                    ui.selectable_value(
                        &mut app.shape_tool_dialog.kind,
                        ShapeKind::RoundedRectangleShadow,
                        "Round Rect + Shadow",
                    );
                    ui.selectable_value(
                        &mut app.shape_tool_dialog.kind,
                        ShapeKind::SpeechBubble,
                        "Speech Bubble",
                    );
                });
                ui.horizontal(|ui| {
                    ui.label("Start X");
                    ui.add(
                        egui::DragValue::new(&mut app.shape_tool_dialog.start_x)
                            .range(-65535..=65535),
                    );
                    ui.label("Start Y");
                    ui.add(
                        egui::DragValue::new(&mut app.shape_tool_dialog.start_y)
                            .range(-65535..=65535),
                    );
                });
                ui.horizontal(|ui| {
                    ui.label("End X");
                    ui.add(
                        egui::DragValue::new(&mut app.shape_tool_dialog.end_x)
                            .range(-65535..=65535),
                    );
                    ui.label("End Y");
                    ui.add(
                        egui::DragValue::new(&mut app.shape_tool_dialog.end_y)
                            .range(-65535..=65535),
                    );
                });
                ui.horizontal(|ui| {
                    ui.label("Thickness");
                    ui.add(egui::Slider::new(
                        &mut app.shape_tool_dialog.thickness,
                        1..=64,
                    ));
                    ui.checkbox(&mut app.shape_tool_dialog.filled, "Filled");
                });
                ui.horizontal(|ui| {
                    ui.label("Color");
                    ui.color_edit_button_srgba(&mut app.shape_tool_dialog.color);
                });
                ui.separator();
                if ui.button("Apply").clicked() {
                    app.dispatch_transform(TransformOp::DrawShape(ShapeParams {
                        kind: app.shape_tool_dialog.kind,
                        start_x: app.shape_tool_dialog.start_x,
                        start_y: app.shape_tool_dialog.start_y,
                        end_x: app.shape_tool_dialog.end_x,
                        end_y: app.shape_tool_dialog.end_y,
                        thickness: app.shape_tool_dialog.thickness,
                        filled: app.shape_tool_dialog.filled,
                        color: app.shape_tool_dialog.color.to_array(),
                    }));
                    *open_state = false;
                }
            },
        );
        self.shape_tool_dialog.open = open;
    }

    fn draw_overlay_dialog(&mut self, ctx: &egui::Context) {
        if !self.overlay_dialog.open {
            return;
        }

        let mut open = self.overlay_dialog.open;
        self.show_popup_window(
            ctx,
            "popup.overlay",
            "Overlay / Watermark",
            egui::vec2(620.0, 280.0),
            &mut open,
            |app, _ctx, ui, open_state| {
                ui.horizontal(|ui| {
                    ui.label("Overlay image");
                    ui.text_edit_singleline(&mut app.overlay_dialog.overlay_path);
                    if ui.button("Pick...").clicked() {
                        let dialog = rfd::FileDialog::new().set_title("Choose overlay image");
                        if let Some(path) = dialog.pick_file() {
                            app.overlay_dialog.overlay_path = path.display().to_string();
                        }
                    }
                });
                ui.add(
                    egui::Slider::new(&mut app.overlay_dialog.opacity, 0.0..=1.0)
                        .text("Opacity")
                        .fixed_decimals(2),
                );
                ui.label("Anchor");
                ui.horizontal_wrapped(|ui| {
                    ui.selectable_value(
                        &mut app.overlay_dialog.anchor,
                        CanvasAnchor::TopLeft,
                        "Top-left",
                    );
                    ui.selectable_value(
                        &mut app.overlay_dialog.anchor,
                        CanvasAnchor::TopCenter,
                        "Top-center",
                    );
                    ui.selectable_value(
                        &mut app.overlay_dialog.anchor,
                        CanvasAnchor::TopRight,
                        "Top-right",
                    );
                    ui.selectable_value(
                        &mut app.overlay_dialog.anchor,
                        CanvasAnchor::CenterLeft,
                        "Center-left",
                    );
                    ui.selectable_value(
                        &mut app.overlay_dialog.anchor,
                        CanvasAnchor::Center,
                        "Center",
                    );
                    ui.selectable_value(
                        &mut app.overlay_dialog.anchor,
                        CanvasAnchor::CenterRight,
                        "Center-right",
                    );
                    ui.selectable_value(
                        &mut app.overlay_dialog.anchor,
                        CanvasAnchor::BottomLeft,
                        "Bottom-left",
                    );
                    ui.selectable_value(
                        &mut app.overlay_dialog.anchor,
                        CanvasAnchor::BottomCenter,
                        "Bottom-center",
                    );
                    ui.selectable_value(
                        &mut app.overlay_dialog.anchor,
                        CanvasAnchor::BottomRight,
                        "Bottom-right",
                    );
                });
                ui.separator();
                if ui.button("Apply").clicked() {
                    let path = PathBuf::from(app.overlay_dialog.overlay_path.trim());
                    if path.as_os_str().is_empty() {
                        app.state.set_error("overlay image path is required");
                    } else {
                        app.dispatch_transform(TransformOp::OverlayImage {
                            overlay_path: path,
                            opacity: app.overlay_dialog.opacity,
                            anchor: app.overlay_dialog.anchor,
                        });
                        *open_state = false;
                    }
                }
            },
        );
        self.overlay_dialog.open = open;
    }

    fn draw_selection_workflow_dialog(&mut self, ctx: &egui::Context) {
        if !self.selection_workflow_dialog.open {
            return;
        }

        let mut open = self.selection_workflow_dialog.open;
        self.show_popup_window(
            ctx,
            "popup.selection-workflows",
            "Selection Workflows",
            egui::vec2(620.0, 340.0),
            &mut open,
            |app, _ctx, ui, open_state| {
                ui.horizontal_wrapped(|ui| {
                    ui.selectable_value(
                        &mut app.selection_workflow_dialog.workflow,
                        SelectionWorkflow::CropRect,
                        "Crop Rect",
                    );
                    ui.selectable_value(
                        &mut app.selection_workflow_dialog.workflow,
                        SelectionWorkflow::CropCircle,
                        "Crop Circle",
                    );
                    ui.selectable_value(
                        &mut app.selection_workflow_dialog.workflow,
                        SelectionWorkflow::CutOutsideRect,
                        "Cut Outside Rect",
                    );
                    ui.selectable_value(
                        &mut app.selection_workflow_dialog.workflow,
                        SelectionWorkflow::CutOutsideCircle,
                        "Cut Outside Circle",
                    );
                    ui.selectable_value(
                        &mut app.selection_workflow_dialog.workflow,
                        SelectionWorkflow::CropPolygon,
                        "Crop Polygon",
                    );
                    ui.selectable_value(
                        &mut app.selection_workflow_dialog.workflow,
                        SelectionWorkflow::CutOutsidePolygon,
                        "Cut Outside Polygon",
                    );
                });
                ui.separator();
                ui.horizontal(|ui| {
                    ui.label("X");
                    ui.add(
                        egui::DragValue::new(&mut app.selection_workflow_dialog.x).range(0..=65535),
                    );
                    ui.label("Y");
                    ui.add(
                        egui::DragValue::new(&mut app.selection_workflow_dialog.y).range(0..=65535),
                    );
                });
                ui.horizontal(|ui| {
                    ui.label("Width");
                    ui.add(
                        egui::DragValue::new(&mut app.selection_workflow_dialog.width)
                            .range(1..=65535),
                    );
                    ui.label("Height");
                    ui.add(
                        egui::DragValue::new(&mut app.selection_workflow_dialog.height)
                            .range(1..=65535),
                    );
                });
                ui.horizontal(|ui| {
                    ui.label("Radius");
                    ui.add(
                        egui::DragValue::new(&mut app.selection_workflow_dialog.radius)
                            .range(1..=65535),
                    );
                });
                if matches!(
                    app.selection_workflow_dialog.workflow,
                    SelectionWorkflow::CropPolygon | SelectionWorkflow::CutOutsidePolygon
                ) {
                    ui.label("Polygon points (x,y; x,y; x,y)");
                    ui.text_edit_singleline(&mut app.selection_workflow_dialog.polygon_points);
                }
                ui.horizontal(|ui| {
                    ui.label("Fill (for cut-outside)");
                    ui.color_edit_button_srgba(&mut app.selection_workflow_dialog.fill);
                });
                ui.separator();
                if ui.button("Apply").clicked() {
                    let polygon_points = if matches!(
                        app.selection_workflow_dialog.workflow,
                        SelectionWorkflow::CropPolygon | SelectionWorkflow::CutOutsidePolygon
                    ) {
                        match Self::parse_polygon_points(&app.selection_workflow_dialog.polygon_points)
                        {
                            Some(points) => points,
                            None => {
                                app.state.set_error(
                                    "invalid polygon points (expected: x,y; x,y; x,y)",
                                );
                                return;
                            }
                        }
                    } else {
                        Vec::new()
                    };
                    app.dispatch_transform(TransformOp::SelectionWorkflow(SelectionParams {
                        workflow: app.selection_workflow_dialog.workflow,
                        x: app.selection_workflow_dialog.x,
                        y: app.selection_workflow_dialog.y,
                        width: app.selection_workflow_dialog.width,
                        height: app.selection_workflow_dialog.height,
                        radius: app.selection_workflow_dialog.radius,
                        polygon_points,
                        fill: app.selection_workflow_dialog.fill.to_array(),
                    }));
                    *open_state = false;
                }
            },
        );
        self.selection_workflow_dialog.open = open;
    }

    fn draw_replace_color_dialog(&mut self, ctx: &egui::Context) {
        if !self.replace_color_dialog.open {
            return;
        }

        let mut open = self.replace_color_dialog.open;
        self.show_popup_window(
            ctx,
            "popup.replace-color",
            "Replace Color",
            egui::vec2(480.0, 260.0),
            &mut open,
            |app, _ctx, ui, open_state| {
                ui.horizontal(|ui| {
                    ui.label("Source");
                    ui.color_edit_button_srgba(&mut app.replace_color_dialog.source);
                    ui.label("Target");
                    ui.color_edit_button_srgba(&mut app.replace_color_dialog.target);
                });
                ui.add(
                    egui::Slider::new(&mut app.replace_color_dialog.tolerance, 0..=255)
                        .text("Tolerance"),
                );
                ui.checkbox(
                    &mut app.replace_color_dialog.preserve_alpha,
                    "Preserve original alpha",
                );
                ui.separator();
                if ui.button("Apply").clicked() {
                    app.dispatch_transform(TransformOp::ReplaceColor {
                        source: app.replace_color_dialog.source.to_array(),
                        target: app.replace_color_dialog.target.to_array(),
                        tolerance: app.replace_color_dialog.tolerance,
                        preserve_alpha: app.replace_color_dialog.preserve_alpha,
                    });
                    *open_state = false;
                }
            },
        );
        self.replace_color_dialog.open = open;
    }

    fn draw_alpha_dialog(&mut self, ctx: &egui::Context) {
        if !self.alpha_dialog.open {
            return;
        }

        let mut open = self.alpha_dialog.open;
        self.show_popup_window(
            ctx,
            "popup.alpha",
            "Alpha Tools",
            egui::vec2(460.0, 230.0),
            &mut open,
            |app, _ctx, ui, open_state| {
                ui.add(
                    egui::Slider::new(&mut app.alpha_dialog.alpha_percent, 0.0..=100.0)
                        .text("Global alpha (%)")
                        .fixed_decimals(1),
                );
                ui.checkbox(
                    &mut app.alpha_dialog.alpha_from_luma,
                    "Derive alpha from luminance",
                );
                if app.alpha_dialog.alpha_from_luma {
                    ui.checkbox(&mut app.alpha_dialog.invert_luma, "Invert luminance alpha");
                }
                ui.checkbox(
                    &mut app.alpha_dialog.limit_to_region,
                    "Apply only to rectangular region",
                );
                if app.alpha_dialog.limit_to_region {
                    ui.horizontal(|ui| {
                        ui.label("X");
                        ui.add(egui::DragValue::new(&mut app.alpha_dialog.region_x).range(0..=1_000_000));
                        ui.label("Y");
                        ui.add(egui::DragValue::new(&mut app.alpha_dialog.region_y).range(0..=1_000_000));
                    });
                    ui.horizontal(|ui| {
                        ui.label("Width");
                        ui.add(
                            egui::DragValue::new(&mut app.alpha_dialog.region_width)
                                .range(1..=1_000_000),
                        );
                        ui.label("Height");
                        ui.add(
                            egui::DragValue::new(&mut app.alpha_dialog.region_height)
                                .range(1..=1_000_000),
                        );
                    });
                }
                ui.separator();
                if ui.button("Apply").clicked() {
                    app.dispatch_transform(TransformOp::AlphaAdjust {
                        alpha_percent: app.alpha_dialog.alpha_percent,
                        alpha_from_luma: app.alpha_dialog.alpha_from_luma,
                        invert_luma: app.alpha_dialog.invert_luma,
                        region: if app.alpha_dialog.limit_to_region {
                            Some((
                                app.alpha_dialog.region_x,
                                app.alpha_dialog.region_y,
                                app.alpha_dialog.region_width.max(1),
                                app.alpha_dialog.region_height.max(1),
                            ))
                        } else {
                            None
                        },
                    });
                    *open_state = false;
                }
            },
        );
        self.alpha_dialog.open = open;
    }

    fn draw_effects_dialog(&mut self, ctx: &egui::Context) {
        if !self.effects_dialog.open {
            return;
        }

        let mut open = self.effects_dialog.open;
        self.show_popup_window(
            ctx,
            "popup.effects",
            "Effects",
            egui::vec2(560.0, 360.0),
            &mut open,
            |app, _ctx, ui, open_state| {
                ui.horizontal_wrapped(|ui| {
                    ui.label("Presets");
                    if ui
                        .selectable_label(
                            app.effects_dialog.preset == EffectsPreset::Natural,
                            "Natural",
                        )
                        .clicked()
                    {
                        app.apply_effects_preset(EffectsPreset::Natural);
                    }
                    if ui
                        .selectable_label(
                            app.effects_dialog.preset == EffectsPreset::Vintage,
                            "Vintage",
                        )
                        .clicked()
                    {
                        app.apply_effects_preset(EffectsPreset::Vintage);
                    }
                    if ui
                        .selectable_label(
                            app.effects_dialog.preset == EffectsPreset::Dramatic,
                            "Dramatic",
                        )
                        .clicked()
                    {
                        app.apply_effects_preset(EffectsPreset::Dramatic);
                    }
                    if ui
                        .selectable_label(app.effects_dialog.preset == EffectsPreset::Noir, "Noir")
                        .clicked()
                    {
                        app.apply_effects_preset(EffectsPreset::Noir);
                    }
                    if ui
                        .selectable_label(
                            app.effects_dialog.preset == EffectsPreset::StainedGlass,
                            "Stained Glass",
                        )
                        .clicked()
                    {
                        app.apply_effects_preset(EffectsPreset::StainedGlass);
                    }
                    if ui
                        .selectable_label(
                            app.effects_dialog.preset == EffectsPreset::TiltShift,
                            "Tilt Shift",
                        )
                        .clicked()
                    {
                        app.apply_effects_preset(EffectsPreset::TiltShift);
                    }
                    if ui.button("Reset").clicked() {
                        app.apply_effects_preset(EffectsPreset::Custom);
                        app.effects_dialog = EffectsDialogState::default();
                        app.effects_dialog.open = true;
                    }
                });
                ui.separator();
                ui.add(
                    egui::Slider::new(&mut app.effects_dialog.blur_sigma, 0.0..=20.0)
                        .text("Blur sigma")
                        .fixed_decimals(2),
                );
                ui.add(
                    egui::Slider::new(&mut app.effects_dialog.sharpen_sigma, 0.0..=20.0)
                        .text("Sharpen sigma")
                        .fixed_decimals(2),
                );
                ui.add(
                    egui::Slider::new(&mut app.effects_dialog.sharpen_threshold, -255..=255)
                        .text("Sharpen threshold"),
                );
                ui.checkbox(&mut app.effects_dialog.invert, "Invert");
                ui.checkbox(&mut app.effects_dialog.grayscale, "Grayscale");
                ui.add(
                    egui::Slider::new(&mut app.effects_dialog.sepia_strength, 0.0..=1.0)
                        .text("Sepia strength")
                        .fixed_decimals(2),
                );
                ui.add(
                    egui::Slider::new(&mut app.effects_dialog.posterize_levels, 0..=64)
                        .text("Posterize levels (0 = off)"),
                );
                ui.add(
                    egui::Slider::new(&mut app.effects_dialog.vignette_strength, 0.0..=1.0)
                        .text("Vignette")
                        .fixed_decimals(2),
                );
                ui.add(
                    egui::Slider::new(&mut app.effects_dialog.tilt_shift_strength, 0.0..=1.0)
                        .text("Tilt-shift")
                        .fixed_decimals(2),
                );
                ui.add(
                    egui::Slider::new(&mut app.effects_dialog.stained_glass_strength, 0.0..=1.0)
                        .text("Stained-glass")
                        .fixed_decimals(2),
                );
                ui.add(
                    egui::Slider::new(&mut app.effects_dialog.emboss_strength, 0.0..=1.0)
                        .text("Emboss")
                        .fixed_decimals(2),
                );
                ui.add(
                    egui::Slider::new(&mut app.effects_dialog.edge_enhance_strength, 0.0..=1.0)
                        .text("Edge enhance")
                        .fixed_decimals(2),
                );
                ui.add(
                    egui::Slider::new(&mut app.effects_dialog.oil_paint_strength, 0.0..=1.0)
                        .text("Oil paint")
                        .fixed_decimals(2),
                );
                ui.separator();
                if ui.button("Apply").clicked() {
                    app.dispatch_transform(TransformOp::Effects(app.effects_params_from_dialog()));
                    *open_state = false;
                }
            },
        );
        self.effects_dialog.open = open;
    }

    fn draw_batch_dialog(&mut self, ctx: &egui::Context) {
        if !self.batch_dialog.open {
            return;
        }

        let mut open = self.batch_dialog.open;
        self.show_popup_window(
            ctx,
            "popup.batch",
            "Batch Convert / Rename",
            egui::vec2(760.0, 620.0),
            &mut open,
            |app, _ctx, ui, open_state| {
                ui.label("Input directory");
                ui.horizontal(|ui| {
                    ui.text_edit_singleline(&mut app.batch_dialog.input_dir);
                    if ui.button("Pick...").clicked() {
                        let dialog = rfd::FileDialog::new().set_title("Batch input directory");
                        if let Some(path) = dialog.pick_folder() {
                            app.batch_dialog.input_dir = path.display().to_string();
                        }
                    }
                });

                ui.label("Output directory");
                ui.horizontal(|ui| {
                    ui.text_edit_singleline(&mut app.batch_dialog.output_dir);
                    if ui.button("Pick...").clicked() {
                        let dialog = rfd::FileDialog::new().set_title("Batch output directory");
                        if let Some(path) = dialog.pick_folder() {
                            app.batch_dialog.output_dir = path.display().to_string();
                        }
                    }
                });

                ui.separator();
                ui.label("Output format");
                ui.horizontal_wrapped(|ui| {
                    ui.selectable_value(
                        &mut app.batch_dialog.output_format,
                        BatchOutputFormat::Jpeg,
                        "JPEG",
                    );
                    ui.selectable_value(
                        &mut app.batch_dialog.output_format,
                        BatchOutputFormat::Png,
                        "PNG",
                    );
                    ui.selectable_value(
                        &mut app.batch_dialog.output_format,
                        BatchOutputFormat::Webp,
                        "WEBP",
                    );
                    ui.selectable_value(
                        &mut app.batch_dialog.output_format,
                        BatchOutputFormat::Bmp,
                        "BMP",
                    );
                    ui.selectable_value(
                        &mut app.batch_dialog.output_format,
                        BatchOutputFormat::Tiff,
                        "TIFF",
                    );
                });
                if matches!(app.batch_dialog.output_format, BatchOutputFormat::Jpeg) {
                    ui.add(
                        egui::Slider::new(&mut app.batch_dialog.jpeg_quality, 1..=100)
                            .text("JPEG quality"),
                    );
                }

                ui.horizontal(|ui| {
                    ui.label("Rename prefix");
                    ui.text_edit_singleline(&mut app.batch_dialog.rename_prefix);
                });
                ui.horizontal(|ui| {
                    ui.label("Start index");
                    ui.add(
                        egui::DragValue::new(&mut app.batch_dialog.start_index).range(0..=999999),
                    );
                });
                ui.horizontal_wrapped(|ui| {
                    if ui.button("Save preset...").clicked() {
                        let mut dialog =
                            rfd::FileDialog::new().set_title("Save batch preset (JSON)");
                        if let Some(directory) = app.state.preferred_open_directory() {
                            dialog = dialog.set_directory(directory);
                        }
                        dialog = dialog.set_file_name("batch-preset.json");
                        if let Some(path) = dialog.save_file() {
                            app.save_batch_preset(path);
                        }
                    }
                    if ui.button("Load preset...").clicked() {
                        let mut dialog =
                            rfd::FileDialog::new().set_title("Load batch preset (JSON)");
                        if let Some(directory) = app.state.preferred_open_directory() {
                            dialog = dialog.set_directory(directory);
                        }
                        if let Some(path) = dialog.pick_file() {
                            app.load_batch_preset(path);
                        }
                    }
                });

                ui.separator();
                if ui.button("Preview summary").clicked() {
                    let input_dir = PathBuf::from(app.batch_dialog.input_dir.trim());
                    if input_dir.as_os_str().is_empty() {
                        app.batch_dialog.preview_error =
                            Some("input directory is required for preview".to_owned());
                        app.batch_dialog.preview_count = None;
                    } else {
                        match collect_images_in_directory(&input_dir) {
                            Ok(files) => {
                                app.batch_dialog.preview_count = Some(files.len());
                                app.batch_dialog.preview_for_input =
                                    app.batch_dialog.input_dir.trim().to_owned();
                                app.batch_dialog.preview_error = None;
                            }
                            Err(err) => {
                                app.batch_dialog.preview_count = None;
                                app.batch_dialog.preview_for_input.clear();
                                app.batch_dialog.preview_error = Some(err.to_string());
                            }
                        }
                    }
                }

                if let Some(error) = &app.batch_dialog.preview_error {
                    ui.colored_label(egui::Color32::from_rgb(255, 190, 190), error);
                } else if let Some(count) = app.batch_dialog.preview_count {
                    ui.label(format!(
                        "Preview: {} image(s), output format {:?}, start index {}",
                        count, app.batch_dialog.output_format, app.batch_dialog.start_index
                    ));
                } else {
                    ui.label("Preview required before running batch.");
                }

                let preview_ready = app.batch_dialog.preview_count.is_some()
                    && app.batch_dialog.preview_for_input == app.batch_dialog.input_dir.trim();
                if ui
                    .add_enabled(preview_ready, egui::Button::new("Run batch"))
                    .clicked()
                {
                    let input_dir = PathBuf::from(app.batch_dialog.input_dir.trim());
                    let output_dir = PathBuf::from(app.batch_dialog.output_dir.trim());
                    if input_dir.as_os_str().is_empty() || output_dir.as_os_str().is_empty() {
                        app.state
                            .set_error("input and output directories are required");
                    } else {
                        app.dispatch_batch_convert(BatchConvertOptions {
                            input_dir,
                            output_dir,
                            output_format: app.batch_dialog.output_format,
                            rename_prefix: app.batch_dialog.rename_prefix.clone(),
                            start_index: app.batch_dialog.start_index,
                            jpeg_quality: app.batch_dialog.jpeg_quality,
                        });
                        *open_state = false;
                    }
                }
            },
        );
        self.batch_dialog.open = open;
    }

    fn draw_save_dialog(&mut self, ctx: &egui::Context) {
        if !self.save_dialog.open {
            return;
        }

        let mut open = self.save_dialog.open;
        self.show_popup_window(
            ctx,
            "popup.save",
            "Save Image",
            egui::vec2(760.0, 480.0),
            &mut open,
            |app, _ctx, ui, open_state| {
                ui.label("Output path");
                ui.horizontal(|ui| {
                    ui.text_edit_singleline(&mut app.save_dialog.path);
                    if ui.button("Pick...").clicked() {
                        let mut dialog = rfd::FileDialog::new().set_title("Save image as");
                        if let Some(directory) = app.state.preferred_open_directory() {
                            dialog = dialog.set_directory(directory);
                        }
                        if let Some(file_name) = app.state.suggested_save_name() {
                            dialog = dialog.set_file_name(file_name);
                        }
                        if let Some(path) = dialog.save_file() {
                            app.save_dialog.path = path.display().to_string();
                        }
                    }
                });

                ui.separator();
                ui.label("Format");
                ui.horizontal_wrapped(|ui| {
                    ui.selectable_value(
                        &mut app.save_dialog.output_format,
                        SaveOutputFormat::Auto,
                        "Auto (from extension)",
                    );
                    ui.selectable_value(
                        &mut app.save_dialog.output_format,
                        SaveOutputFormat::Jpeg,
                        "JPEG",
                    );
                    ui.selectable_value(
                        &mut app.save_dialog.output_format,
                        SaveOutputFormat::Png,
                        "PNG",
                    );
                    ui.selectable_value(
                        &mut app.save_dialog.output_format,
                        SaveOutputFormat::Webp,
                        "WEBP",
                    );
                    ui.selectable_value(
                        &mut app.save_dialog.output_format,
                        SaveOutputFormat::Bmp,
                        "BMP",
                    );
                    ui.selectable_value(
                        &mut app.save_dialog.output_format,
                        SaveOutputFormat::Tiff,
                        "TIFF",
                    );
                });
                if matches!(app.save_dialog.output_format, SaveOutputFormat::Jpeg) {
                    ui.add(
                        egui::Slider::new(&mut app.save_dialog.jpeg_quality, 1..=100)
                            .text("JPEG quality"),
                    );
                }

                ui.separator();
                ui.label("Metadata policy");
                ui.radio_value(
                    &mut app.save_dialog.metadata_policy,
                    SaveMetadataPolicy::PreserveIfPossible,
                    "Preserve if possible",
                );
                ui.radio_value(
                    &mut app.save_dialog.metadata_policy,
                    SaveMetadataPolicy::Strip,
                    "Strip metadata",
                );
                if matches!(
                    app.save_dialog.metadata_policy,
                    SaveMetadataPolicy::PreserveIfPossible
                ) {
                    ui.small("Current best-effort preservation supports JPEG output.");
                }

                ui.separator();
                if ui.button("Save").clicked() {
                    let path = PathBuf::from(app.save_dialog.path.trim());
                    if path.as_os_str().is_empty() {
                        app.state.set_error("save path is required");
                    } else {
                        if path.exists() && app.advanced_options_dialog.confirm_overwrite {
                            let approved = matches!(
                                rfd::MessageDialog::new()
                                    .set_title("Confirm overwrite")
                                    .set_description(format!(
                                        "Overwrite existing file?\n{}",
                                        path.display()
                                    ))
                                    .set_level(rfd::MessageLevel::Warning)
                                    .set_buttons(rfd::MessageButtons::YesNo)
                                    .show(),
                                rfd::MessageDialogResult::Yes
                            );
                            if !approved {
                                return;
                            }
                        }
                        let options = app.build_save_options_from_dialog();
                        app.dispatch_save(Some(path), app.save_dialog.reopen_after_save, options);
                        *open_state = false;
                    }
                }
            },
        );
        self.save_dialog.open = open;
    }

    fn draw_performance_dialog(&mut self, ctx: &egui::Context) {
        if !self.performance_dialog.open {
            return;
        }

        let mut open = self.performance_dialog.open;
        self.show_popup_window(
            ctx,
            "popup.performance",
            "Performance / Cache Settings",
            egui::vec2(520.0, 300.0),
            &mut open,
            |app, _ctx, ui, open_state| {
                ui.label("Thumbnail texture cache");
                ui.horizontal(|ui| {
                    ui.label("Entry cap");
                    ui.add(
                        egui::DragValue::new(&mut app.performance_dialog.thumb_cache_entry_cap)
                            .range(64..=4096),
                    );
                });
                ui.horizontal(|ui| {
                    ui.label("Memory cap (MB)");
                    ui.add(
                        egui::DragValue::new(&mut app.performance_dialog.thumb_cache_max_mb)
                            .range(16..=1024),
                    );
                });

                ui.separator();
                ui.label("Preload cache");
                ui.horizontal(|ui| {
                    ui.label("Entry cap");
                    ui.add(
                        egui::DragValue::new(&mut app.performance_dialog.preload_cache_entry_cap)
                            .range(1..=64),
                    );
                });
                ui.horizontal(|ui| {
                    ui.label("Memory cap (MB)");
                    ui.add(
                        egui::DragValue::new(&mut app.performance_dialog.preload_cache_max_mb)
                            .range(32..=2048),
                    );
                });

                ui.separator();
                if ui.button("Apply").clicked() {
                    app.apply_performance_settings();
                    *open_state = false;
                }
            },
        );
        self.performance_dialog.open = open;
    }

    fn draw_rename_dialog(&mut self, ctx: &egui::Context) {
        if !self.rename_dialog.open {
            return;
        }

        let mut open = self.rename_dialog.open;
        self.show_popup_window(
            ctx,
            "popup.rename",
            "Rename Current File",
            egui::vec2(420.0, 160.0),
            &mut open,
            |app, _ctx, ui, open_state| {
                ui.label("New file name");
                ui.text_edit_singleline(&mut app.rename_dialog.new_name);
                if ui.button("Rename").clicked() {
                    if let Some(from) = app.rename_dialog.target_path.clone() {
                        if let Some(parent) = from.parent() {
                            let to = parent.join(app.rename_dialog.new_name.trim());
                            app.dispatch_file_operation(FileOperation::Rename { from, to });
                            *open_state = false;
                        }
                    }
                }
            },
        );
        self.rename_dialog.open = open;
    }

    fn draw_search_dialog(&mut self, ctx: &egui::Context) {
        if !self.search_dialog.open {
            return;
        }

        let mut open = self.search_dialog.open;
        self.show_popup_window(
            ctx,
            "popup.search",
            "Search Files",
            egui::vec2(780.0, 560.0),
            &mut open,
            |app, _ctx, ui, open_state| {
                if app.state.images_in_directory().is_empty() {
                    ui.label("Open an image folder before searching.");
                    return;
                }

                let mut refresh = false;
                ui.horizontal(|ui| {
                    ui.label("Name contains");
                    refresh |= ui
                        .text_edit_singleline(&mut app.search_dialog.query)
                        .changed();
                });
                ui.horizontal(|ui| {
                    ui.label("Extensions");
                    refresh |= ui
                        .text_edit_singleline(&mut app.search_dialog.extension_filter)
                        .changed();
                    ui.small("comma-separated, e.g. jpg,png,webp");
                });
                refresh |= ui
                    .checkbox(&mut app.search_dialog.case_sensitive, "Case sensitive")
                    .changed();
                if ui.button("Search").clicked() || refresh {
                    app.run_search_files();
                }

                ui.separator();
                ui.label(format!(
                    "Found {} result(s) in current folder",
                    app.search_dialog.results.len()
                ));

                let results = app.search_dialog.results.clone();
                let mut open_path: Option<PathBuf> = None;
                egui::ScrollArea::vertical().show(ui, |ui| {
                    for path in &results {
                        let label = path
                            .file_name()
                            .map(|name| name.to_string_lossy().into_owned())
                            .unwrap_or_else(|| path.display().to_string());
                        ui.horizontal(|ui| {
                            if ui.button("Open").clicked() {
                                open_path = Some(path.clone());
                            }
                            ui.label(label);
                        });
                    }
                });

                if let Some(path) = open_path {
                    app.dispatch_open(path, false);
                    *open_state = false;
                }
            },
        );
        self.search_dialog.open = open;
    }

    fn draw_screenshot_dialog(&mut self, ctx: &egui::Context) {
        if !self.screenshot_dialog.open {
            return;
        }

        let mut open = self.screenshot_dialog.open;
        self.show_popup_window(
            ctx,
            "popup.screenshot",
            "Screenshot Capture",
            egui::vec2(620.0, 320.0),
            &mut open,
            |app, _ctx, ui, open_state| {
                ui.horizontal(|ui| {
                    ui.label("Delay (ms)");
                    ui.add(
                        egui::DragValue::new(&mut app.screenshot_dialog.delay_ms).range(0..=60_000),
                    );
                });
                ui.checkbox(&mut app.screenshot_dialog.region_enabled, "Capture region");
                if app.screenshot_dialog.region_enabled {
                    ui.horizontal(|ui| {
                        ui.label("X");
                        ui.add(
                            egui::DragValue::new(&mut app.screenshot_dialog.region_x)
                                .range(0..=20_000),
                        );
                        ui.label("Y");
                        ui.add(
                            egui::DragValue::new(&mut app.screenshot_dialog.region_y)
                                .range(0..=20_000),
                        );
                    });
                    ui.horizontal(|ui| {
                        ui.label("Width");
                        ui.add(
                            egui::DragValue::new(&mut app.screenshot_dialog.region_width)
                                .range(1..=20_000),
                        );
                        ui.label("Height");
                        ui.add(
                            egui::DragValue::new(&mut app.screenshot_dialog.region_height)
                                .range(1..=20_000),
                        );
                    });
                }
                ui.separator();
                ui.label("Optional output file (leave empty for temporary capture)");
                ui.horizontal(|ui| {
                    ui.text_edit_singleline(&mut app.screenshot_dialog.output_path);
                    if ui.button("Pick...").clicked() {
                        let mut dialog = rfd::FileDialog::new().set_title("Save screenshot to");
                        if let Some(directory) = app.state.current_directory_path() {
                            dialog = dialog.set_directory(directory);
                        }
                        dialog = dialog.set_file_name("screenshot.png");
                        if let Some(path) = dialog.save_file() {
                            app.screenshot_dialog.output_path = path.display().to_string();
                        }
                    }
                });
                ui.separator();
                if ui.button("Capture").clicked() {
                    let output_path = if app.screenshot_dialog.output_path.trim().is_empty() {
                        None
                    } else {
                        Some(PathBuf::from(app.screenshot_dialog.output_path.trim()))
                    };
                    let region = if app.screenshot_dialog.region_enabled {
                        Some((
                            app.screenshot_dialog.region_x,
                            app.screenshot_dialog.region_y,
                            app.screenshot_dialog.region_width.max(1),
                            app.screenshot_dialog.region_height.max(1),
                        ))
                    } else {
                        None
                    };
                    app.dispatch_capture_screenshot(
                        app.screenshot_dialog.delay_ms,
                        region,
                        output_path,
                    );
                    *open_state = false;
                }
            },
        );
        self.screenshot_dialog.open = open;
    }

    fn draw_tiff_dialog(&mut self, ctx: &egui::Context) {
        if !self.tiff_dialog.open {
            return;
        }

        let mut open = self.tiff_dialog.open;
        self.show_popup_window(
            ctx,
            "popup.tiff",
            "Multipage TIFF",
            egui::vec2(680.0, 360.0),
            &mut open,
            |app, _ctx, ui, open_state| {
                ui.label("TIFF file");
                ui.horizontal(|ui| {
                    ui.text_edit_singleline(&mut app.tiff_dialog.path);
                    if ui.button("Pick...").clicked() {
                        let mut dialog = rfd::FileDialog::new().set_title("Select TIFF file");
                        dialog = dialog.add_filter("TIFF", &["tif", "tiff"]);
                        if let Some(directory) = app.state.current_directory_path() {
                            dialog = dialog.set_directory(directory);
                        }
                        if let Some(path) = dialog.pick_file() {
                            app.tiff_dialog.path = path.display().to_string();
                        }
                    }
                });
                ui.horizontal(|ui| {
                    ui.label("Page index");
                    ui.add(egui::DragValue::new(&mut app.tiff_dialog.page_index).range(0..=9999));
                    if let Some(count) = app.tiff_dialog.page_count {
                        ui.label(format!("Pages detected: {count}"));
                    }
                    if ui.button("Load page").clicked() {
                        let path = PathBuf::from(app.tiff_dialog.path.trim());
                        if path.as_os_str().is_empty() {
                            app.state.set_error("TIFF path is required");
                        } else {
                            app.dispatch_open_tiff_page(path, app.tiff_dialog.page_index);
                        }
                    }
                });
                ui.separator();
                ui.label("Extract all pages");
                ui.horizontal(|ui| {
                    ui.text_edit_singleline(&mut app.tiff_dialog.extract_output_dir);
                    if ui.button("Folder...").clicked() {
                        let mut dialog =
                            rfd::FileDialog::new().set_title("TIFF page export folder");
                        if let Some(directory) = app.state.current_directory_path() {
                            dialog = dialog.set_directory(directory);
                        }
                        if let Some(path) = dialog.pick_folder() {
                            app.tiff_dialog.extract_output_dir = path.display().to_string();
                        }
                    }
                });
                ui.horizontal_wrapped(|ui| {
                    ui.label("Format");
                    ui.selectable_value(
                        &mut app.tiff_dialog.extract_format,
                        BatchOutputFormat::Png,
                        "PNG",
                    );
                    ui.selectable_value(
                        &mut app.tiff_dialog.extract_format,
                        BatchOutputFormat::Jpeg,
                        "JPEG",
                    );
                    ui.selectable_value(
                        &mut app.tiff_dialog.extract_format,
                        BatchOutputFormat::Webp,
                        "WEBP",
                    );
                    ui.selectable_value(
                        &mut app.tiff_dialog.extract_format,
                        BatchOutputFormat::Bmp,
                        "BMP",
                    );
                    ui.selectable_value(
                        &mut app.tiff_dialog.extract_format,
                        BatchOutputFormat::Tiff,
                        "TIFF",
                    );
                });
                if matches!(app.tiff_dialog.extract_format, BatchOutputFormat::Jpeg) {
                    ui.add(
                        egui::Slider::new(&mut app.tiff_dialog.jpeg_quality, 1..=100)
                            .text("JPEG quality"),
                    );
                }
                if ui.button("Extract pages").clicked() {
                    let path = PathBuf::from(app.tiff_dialog.path.trim());
                    let output_dir = PathBuf::from(app.tiff_dialog.extract_output_dir.trim());
                    if path.as_os_str().is_empty() || output_dir.as_os_str().is_empty() {
                        app.state
                            .set_error("both TIFF file and output directory are required");
                    } else {
                        app.dispatch_extract_tiff_pages(
                            path,
                            output_dir,
                            app.tiff_dialog.extract_format,
                            app.tiff_dialog.jpeg_quality,
                        );
                        *open_state = false;
                    }
                }
            },
        );
        self.tiff_dialog.open = open;
    }

    fn draw_pdf_dialog(&mut self, ctx: &egui::Context) {
        if !self.pdf_dialog.open {
            return;
        }

        let mut open = self.pdf_dialog.open;
        self.show_popup_window(
            ctx,
            "popup.pdf",
            "Create Multipage PDF",
            egui::vec2(640.0, 260.0),
            &mut open,
            |app, _ctx, ui, open_state| {
                ui.horizontal(|ui| {
                    ui.label("Output PDF");
                    ui.text_edit_singleline(&mut app.pdf_dialog.output_path);
                    if ui.button("Pick...").clicked() {
                        let mut dialog = rfd::FileDialog::new().set_title("Save PDF as");
                        if let Some(directory) = app.state.current_directory_path() {
                            dialog = dialog.set_directory(directory);
                        }
                        dialog = dialog.set_file_name("images.pdf");
                        if let Some(path) = dialog.save_file() {
                            app.pdf_dialog.output_path = path.display().to_string();
                        }
                    }
                });
                ui.checkbox(
                    &mut app.pdf_dialog.include_folder_images,
                    "Use all images in current folder",
                );
                let input_paths =
                    app.collect_utility_input_paths(app.pdf_dialog.include_folder_images);
                ui.label(format!("Input images: {}", input_paths.len()));
                ui.add(
                    egui::Slider::new(&mut app.pdf_dialog.jpeg_quality, 1..=100)
                        .text("Embedded JPEG quality"),
                );
                if ui.button("Create PDF").clicked() {
                    let output_path = PathBuf::from(app.pdf_dialog.output_path.trim());
                    if output_path.as_os_str().is_empty() {
                        app.state.set_error("output PDF path is required");
                    } else if input_paths.is_empty() {
                        app.state.set_error("no images available for PDF export");
                    } else {
                        app.dispatch_create_multipage_pdf(
                            input_paths,
                            output_path,
                            app.pdf_dialog.jpeg_quality,
                        );
                        *open_state = false;
                    }
                }
            },
        );
        self.pdf_dialog.open = open;
    }

    fn draw_batch_scan_dialog(&mut self, ctx: &egui::Context) {
        if !self.batch_scan_dialog.open {
            return;
        }
        let mut open = self.batch_scan_dialog.open;
        self.show_popup_window(
            ctx,
            "popup.batch-scan",
            "Batch Scan / Import",
            egui::vec2(700.0, 380.0),
            &mut open,
            |app, _ctx, ui, open_state| {
                ui.horizontal_wrapped(|ui| {
                    ui.label("Source mode");
                    ui.selectable_value(
                        &mut app.batch_scan_dialog.source,
                        BatchScanSource::FolderImport,
                        "Folder import",
                    );
                    ui.selectable_value(
                        &mut app.batch_scan_dialog.source,
                        BatchScanSource::ScannerCommand,
                        "Scanner command",
                    );
                });
                ui.separator();
                if app.batch_scan_dialog.source == BatchScanSource::FolderImport {
                    ui.label("Source folder (scanner drop folder)");
                    ui.horizontal(|ui| {
                        ui.text_edit_singleline(&mut app.batch_scan_dialog.input_dir);
                        if ui.button("Pick...").clicked() {
                            if let Some(path) = rfd::FileDialog::new().pick_folder() {
                                app.batch_scan_dialog.input_dir = path.display().to_string();
                            }
                        }
                    });
                } else {
                    ui.label("Scanner command template");
                    ui.text_edit_singleline(&mut app.batch_scan_dialog.command_template);
                    ui.small(
                        "Use {output} for output path and optional {index}. Example: scanimage --format=png --output-file {output}",
                    );
                    ui.horizontal(|ui| {
                        ui.label("Pages");
                        ui.add(
                            egui::DragValue::new(&mut app.batch_scan_dialog.page_count)
                                .range(1..=1024),
                        );
                    });
                }
                ui.label("Output folder");
                ui.horizontal(|ui| {
                    ui.text_edit_singleline(&mut app.batch_scan_dialog.output_dir);
                    if ui.button("Pick...").clicked() {
                        if let Some(path) = rfd::FileDialog::new().pick_folder() {
                            app.batch_scan_dialog.output_dir = path.display().to_string();
                        }
                    }
                });
                ui.horizontal(|ui| {
                    ui.label("Prefix");
                    ui.text_edit_singleline(&mut app.batch_scan_dialog.rename_prefix);
                    ui.label("Start index");
                    ui.add(egui::DragValue::new(&mut app.batch_scan_dialog.start_index).range(0..=999_999));
                });
                ui.horizontal_wrapped(|ui| {
                    ui.label("Output format");
                    ui.selectable_value(
                        &mut app.batch_scan_dialog.output_format,
                        BatchOutputFormat::Jpeg,
                        "JPEG",
                    );
                    ui.selectable_value(
                        &mut app.batch_scan_dialog.output_format,
                        BatchOutputFormat::Png,
                        "PNG",
                    );
                    ui.selectable_value(
                        &mut app.batch_scan_dialog.output_format,
                        BatchOutputFormat::Webp,
                        "WEBP",
                    );
                    ui.selectable_value(
                        &mut app.batch_scan_dialog.output_format,
                        BatchOutputFormat::Bmp,
                        "BMP",
                    );
                    ui.selectable_value(
                        &mut app.batch_scan_dialog.output_format,
                        BatchOutputFormat::Tiff,
                        "TIFF",
                    );
                });
                if matches!(app.batch_scan_dialog.output_format, BatchOutputFormat::Jpeg) {
                    ui.add(
                        egui::Slider::new(&mut app.batch_scan_dialog.jpeg_quality, 1..=100)
                            .text("JPEG quality"),
                    );
                }
                let action_label = if app.batch_scan_dialog.source == BatchScanSource::FolderImport {
                    "Run import"
                } else {
                    "Run scan capture"
                };
                if ui.button(action_label).clicked() {
                    let output_dir = PathBuf::from(app.batch_scan_dialog.output_dir.trim());
                    if output_dir.as_os_str().is_empty() {
                        app.state.set_error("output folder is required");
                        return;
                    }
                    if app.batch_scan_dialog.source == BatchScanSource::FolderImport {
                        let input_dir = PathBuf::from(app.batch_scan_dialog.input_dir.trim());
                        if input_dir.as_os_str().is_empty() {
                            app.state.set_error("source folder is required");
                            return;
                        }
                        app.dispatch_batch_convert(BatchConvertOptions {
                            input_dir,
                            output_dir,
                            output_format: app.batch_scan_dialog.output_format,
                            rename_prefix: app.batch_scan_dialog.rename_prefix.clone(),
                            start_index: app.batch_scan_dialog.start_index,
                            jpeg_quality: app.batch_scan_dialog.jpeg_quality,
                        });
                        *open_state = false;
                    } else {
                        let template = app.batch_scan_dialog.command_template.trim().to_owned();
                        if template.is_empty() || !template.contains("{output}") {
                            app.state.set_error(
                                "scanner command template must include {output} placeholder",
                            );
                            return;
                        }
                        app.dispatch_scan_to_directory(
                            output_dir,
                            app.batch_scan_dialog.output_format,
                            app.batch_scan_dialog.rename_prefix.clone(),
                            app.batch_scan_dialog.start_index,
                            app.batch_scan_dialog.page_count,
                            app.batch_scan_dialog.jpeg_quality,
                            template,
                        );
                        *open_state = false;
                    }
                }
            },
        );
        self.batch_scan_dialog.open = open;
    }

    fn draw_ocr_dialog(&mut self, ctx: &egui::Context) {
        if !self.ocr_dialog.open {
            return;
        }
        let mut open = self.ocr_dialog.open;
        self.show_popup_window(
            ctx,
            "popup.ocr",
            "OCR",
            egui::vec2(720.0, 420.0),
            &mut open,
            |app, _ctx, ui, _open_state| {
                let has_image = app.state.has_image();
                ui.horizontal(|ui| {
                    ui.label("Language");
                    ui.text_edit_singleline(&mut app.ocr_dialog.language);
                    ui.small("e.g. eng, deu, fra");
                });
                ui.horizontal(|ui| {
                    ui.label("Output text file (optional)");
                    ui.text_edit_singleline(&mut app.ocr_dialog.output_path);
                    if ui.button("Pick...").clicked() {
                        let mut dialog = rfd::FileDialog::new().set_title("Save OCR text");
                        if let Some(directory) = app.state.current_directory_path() {
                            dialog = dialog.set_directory(directory);
                        }
                        dialog = dialog.set_file_name("ocr.txt");
                        if let Some(path) = dialog.save_file() {
                            app.ocr_dialog.output_path = path.display().to_string();
                        }
                    }
                });
                if ui
                    .add_enabled(has_image, egui::Button::new("Run OCR"))
                    .clicked()
                {
                    if let Some(path) = app.state.current_file_path() {
                        let output_path = if app.ocr_dialog.output_path.trim().is_empty() {
                            None
                        } else {
                            Some(PathBuf::from(app.ocr_dialog.output_path.trim()))
                        };
                        app.dispatch_ocr(path, app.ocr_dialog.language.clone(), output_path);
                    }
                }
                ui.separator();
                ui.label("Latest OCR output");
                egui::ScrollArea::vertical().show(ui, |ui| {
                    if app.ocr_dialog.preview_text.trim().is_empty() {
                        ui.label("No OCR output yet.");
                    } else {
                        ui.monospace(&app.ocr_dialog.preview_text);
                    }
                });
            },
        );
        self.ocr_dialog.open = open;
    }

    fn draw_lossless_jpeg_dialog(&mut self, ctx: &egui::Context) {
        if !self.lossless_jpeg_dialog.open {
            return;
        }
        let mut open = self.lossless_jpeg_dialog.open;
        self.show_popup_window(
            ctx,
            "popup.lossless-jpeg",
            "Lossless JPEG Transform",
            egui::vec2(680.0, 300.0),
            &mut open,
            |app, _ctx, ui, open_state| {
                let Some(path) = app.state.current_file_path() else {
                    ui.label("Open a JPEG image first.");
                    return;
                };
                ui.label(format!("Input: {}", path.display()));
                ui.horizontal_wrapped(|ui| {
                    ui.selectable_value(
                        &mut app.lossless_jpeg_dialog.op,
                        LosslessJpegOp::Rotate90,
                        "Rotate 90",
                    );
                    ui.selectable_value(
                        &mut app.lossless_jpeg_dialog.op,
                        LosslessJpegOp::Rotate180,
                        "Rotate 180",
                    );
                    ui.selectable_value(
                        &mut app.lossless_jpeg_dialog.op,
                        LosslessJpegOp::Rotate270,
                        "Rotate 270",
                    );
                    ui.selectable_value(
                        &mut app.lossless_jpeg_dialog.op,
                        LosslessJpegOp::FlipHorizontal,
                        "Flip horizontal",
                    );
                    ui.selectable_value(
                        &mut app.lossless_jpeg_dialog.op,
                        LosslessJpegOp::FlipVertical,
                        "Flip vertical",
                    );
                });
                ui.checkbox(
                    &mut app.lossless_jpeg_dialog.in_place,
                    "Modify input file in place",
                );
                if !app.lossless_jpeg_dialog.in_place {
                    ui.horizontal(|ui| {
                        ui.label("Output");
                        ui.text_edit_singleline(&mut app.lossless_jpeg_dialog.output_path);
                        if ui.button("Pick...").clicked() {
                            let mut dialog = rfd::FileDialog::new()
                                .set_title("Lossless JPEG output")
                                .add_filter("JPEG", &["jpg", "jpeg"]);
                            if let Some(directory) = app.state.preferred_open_directory() {
                                dialog = dialog.set_directory(directory);
                            }
                            if let Some(picked) = dialog.save_file() {
                                app.lossless_jpeg_dialog.output_path = picked.display().to_string();
                            }
                        }
                    });
                }
                ui.small("Requires external `jpegtran` to be installed.");
                ui.separator();
                if ui.button("Run lossless transform").clicked() {
                    let output_path = if app.lossless_jpeg_dialog.in_place {
                        None
                    } else {
                        let path = PathBuf::from(app.lossless_jpeg_dialog.output_path.trim());
                        if path.as_os_str().is_empty() {
                            app.state.set_error("output path is required when not in-place");
                            return;
                        }
                        Some(path)
                    };
                    app.dispatch_lossless_jpeg(path, app.lossless_jpeg_dialog.op, output_path);
                    *open_state = false;
                }
            },
        );
        self.lossless_jpeg_dialog.open = open;
    }

    fn draw_exif_date_dialog(&mut self, ctx: &egui::Context) {
        if !self.exif_date_dialog.open {
            return;
        }
        let mut open = self.exif_date_dialog.open;
        self.show_popup_window(
            ctx,
            "popup.exif-date",
            "Change EXIF Date/Time",
            egui::vec2(640.0, 220.0),
            &mut open,
            |app, _ctx, ui, open_state| {
                let Some(path) = app.state.current_file_path() else {
                    ui.label("Open an image first.");
                    return;
                };
                ui.label(format!("Target: {}", path.display()));
                ui.label("Date/time format: YYYY:MM:DD HH:MM:SS");
                ui.text_edit_singleline(&mut app.exif_date_dialog.datetime);
                ui.small("Requires external `exiftool` to be installed.");
                ui.separator();
                if ui.button("Apply EXIF date/time").clicked() {
                    let value = app.exif_date_dialog.datetime.trim().to_owned();
                    if value.is_empty() {
                        app.state.set_error("datetime is required");
                        return;
                    }
                    app.dispatch_update_exif_date(path, value);
                    *open_state = false;
                }
            },
        );
        self.exif_date_dialog.open = open;
    }

    fn draw_color_profile_dialog(&mut self, ctx: &egui::Context) {
        if !self.color_profile_dialog.open {
            return;
        }
        let mut open = self.color_profile_dialog.open;
        self.show_popup_window(
            ctx,
            "popup.color-profile",
            "Convert Color Profile",
            egui::vec2(760.0, 320.0),
            &mut open,
            |app, _ctx, ui, open_state| {
                let Some(path) = app.state.current_file_path() else {
                    ui.label("Open an image first.");
                    return;
                };
                ui.label(format!("Input: {}", path.display()));
                ui.horizontal(|ui| {
                    ui.label("Source profile (optional)");
                    ui.text_edit_singleline(&mut app.color_profile_dialog.source_profile_path);
                    if ui.button("Pick...").clicked() {
                        let mut dialog =
                            rfd::FileDialog::new().set_title("Select source ICC profile");
                        if let Some(directory) = app.state.preferred_open_directory() {
                            dialog = dialog.set_directory(directory);
                        }
                        dialog = dialog.add_filter("ICC Profiles", &["icc", "icm"]);
                        if let Some(profile) = dialog.pick_file() {
                            app.color_profile_dialog.source_profile_path =
                                profile.display().to_string();
                        }
                    }
                });
                ui.horizontal(|ui| {
                    ui.label("Target profile");
                    ui.text_edit_singleline(&mut app.color_profile_dialog.target_profile_path);
                    if ui.button("Pick...").clicked() {
                        let mut dialog =
                            rfd::FileDialog::new().set_title("Select target ICC profile");
                        if let Some(directory) = app.state.preferred_open_directory() {
                            dialog = dialog.set_directory(directory);
                        }
                        dialog = dialog.add_filter("ICC Profiles", &["icc", "icm"]);
                        if let Some(profile) = dialog.pick_file() {
                            app.color_profile_dialog.target_profile_path =
                                profile.display().to_string();
                        }
                    }
                });
                ui.horizontal_wrapped(|ui| {
                    ui.label("Rendering intent");
                    ui.selectable_value(
                        &mut app.color_profile_dialog.rendering_intent,
                        ColorRenderingIntent::RelativeColorimetric,
                        "Relative",
                    );
                    ui.selectable_value(
                        &mut app.color_profile_dialog.rendering_intent,
                        ColorRenderingIntent::Perceptual,
                        "Perceptual",
                    );
                    ui.selectable_value(
                        &mut app.color_profile_dialog.rendering_intent,
                        ColorRenderingIntent::Saturation,
                        "Saturation",
                    );
                    ui.selectable_value(
                        &mut app.color_profile_dialog.rendering_intent,
                        ColorRenderingIntent::AbsoluteColorimetric,
                        "Absolute",
                    );
                });
                ui.checkbox(
                    &mut app.color_profile_dialog.in_place,
                    "Modify input file in place",
                );
                if !app.color_profile_dialog.in_place {
                    ui.horizontal(|ui| {
                        ui.label("Output");
                        ui.text_edit_singleline(&mut app.color_profile_dialog.output_path);
                        if ui.button("Pick...").clicked() {
                            let mut dialog = rfd::FileDialog::new().set_title("Color profile output");
                            if let Some(directory) = app.state.preferred_open_directory() {
                                dialog = dialog.set_directory(directory);
                            }
                            if let Some(file_name) = app.state.suggested_save_name() {
                                dialog = dialog.set_file_name(file_name);
                            }
                            if let Some(output) = dialog.save_file() {
                                app.color_profile_dialog.output_path = output.display().to_string();
                            }
                        }
                    });
                }
                ui.small("Requires external `magick` (ImageMagick) with ICC profile support.");
                ui.separator();
                if ui.button("Run profile conversion").clicked() {
                    let target_profile =
                        PathBuf::from(app.color_profile_dialog.target_profile_path.trim());
                    if target_profile.as_os_str().is_empty() {
                        app.state.set_error("target profile is required");
                        return;
                    }
                    let output_path = if app.color_profile_dialog.in_place {
                        path.clone()
                    } else {
                        let output = PathBuf::from(app.color_profile_dialog.output_path.trim());
                        if output.as_os_str().is_empty() {
                            app.state.set_error("output path is required when not in-place");
                            return;
                        }
                        output
                    };
                    let source_profile =
                        PathBuf::from(app.color_profile_dialog.source_profile_path.trim());
                    let source_profile = if source_profile.as_os_str().is_empty() {
                        None
                    } else {
                        Some(source_profile)
                    };

                    app.dispatch_convert_color_profile(
                        path,
                        output_path,
                        source_profile,
                        target_profile,
                        app.color_profile_dialog.rendering_intent,
                    );
                    *open_state = false;
                }
            },
        );
        self.color_profile_dialog.open = open;
    }

    fn draw_panorama_dialog(&mut self, ctx: &egui::Context) {
        if !self.panorama_dialog.open {
            return;
        }
        let mut open = self.panorama_dialog.open;
        self.show_popup_window(
            ctx,
            "popup.panorama",
            "Panorama Stitch",
            egui::vec2(660.0, 300.0),
            &mut open,
            |app, _ctx, ui, open_state| {
                ui.horizontal(|ui| {
                    ui.label("Output image");
                    ui.text_edit_singleline(&mut app.panorama_dialog.output_path);
                    if ui.button("Pick...").clicked() {
                        let mut dialog = rfd::FileDialog::new().set_title("Panorama output file");
                        if let Some(directory) = app.state.current_directory_path() {
                            dialog = dialog.set_directory(directory);
                        }
                        dialog = dialog.set_file_name("panorama.jpg");
                        if let Some(path) = dialog.save_file() {
                            app.panorama_dialog.output_path = path.display().to_string();
                        }
                    }
                });
                ui.horizontal_wrapped(|ui| {
                    ui.label("Direction");
                    ui.selectable_value(
                        &mut app.panorama_dialog.direction,
                        PanoramaDirection::Horizontal,
                        "Horizontal",
                    );
                    ui.selectable_value(
                        &mut app.panorama_dialog.direction,
                        PanoramaDirection::Vertical,
                        "Vertical",
                    );
                });
                ui.add(
                    egui::Slider::new(&mut app.panorama_dialog.overlap_percent, 0.0..=0.5)
                        .text("Blend overlap"),
                );
                ui.checkbox(
                    &mut app.panorama_dialog.include_folder_images,
                    "Use all images in current folder",
                );
                let input_paths =
                    app.collect_utility_input_paths(app.panorama_dialog.include_folder_images);
                ui.label(format!("Input images: {}", input_paths.len()));
                if ui.button("Stitch panorama").clicked() {
                    let output_path = PathBuf::from(app.panorama_dialog.output_path.trim());
                    if output_path.as_os_str().is_empty() {
                        app.state.set_error("output path is required");
                    } else if input_paths.len() < 2 {
                        app.state
                            .set_error("need at least two images for panorama stitching");
                    } else {
                        app.dispatch_stitch_panorama(
                            input_paths,
                            output_path,
                            app.panorama_dialog.direction,
                            app.panorama_dialog.overlap_percent,
                        );
                        *open_state = false;
                    }
                }
            },
        );
        self.panorama_dialog.open = open;
    }

    fn draw_perspective_dialog(&mut self, ctx: &egui::Context) {
        if !self.perspective_dialog.open {
            return;
        }

        let mut open = self.perspective_dialog.open;
        self.show_popup_window(
            ctx,
            "popup.perspective",
            "Perspective Correction",
            egui::vec2(700.0, 380.0),
            &mut open,
            |app, _ctx, ui, open_state| {
                ui.label("Source quad points (in preview pixel coordinates)");
                ui.horizontal(|ui| {
                    ui.label("Top-left");
                    ui.add(
                        egui::DragValue::new(&mut app.perspective_dialog.top_left[0]).speed(1.0),
                    );
                    ui.add(
                        egui::DragValue::new(&mut app.perspective_dialog.top_left[1]).speed(1.0),
                    );
                });
                ui.horizontal(|ui| {
                    ui.label("Top-right");
                    ui.add(
                        egui::DragValue::new(&mut app.perspective_dialog.top_right[0]).speed(1.0),
                    );
                    ui.add(
                        egui::DragValue::new(&mut app.perspective_dialog.top_right[1]).speed(1.0),
                    );
                });
                ui.horizontal(|ui| {
                    ui.label("Bottom-right");
                    ui.add(
                        egui::DragValue::new(&mut app.perspective_dialog.bottom_right[0])
                            .speed(1.0),
                    );
                    ui.add(
                        egui::DragValue::new(&mut app.perspective_dialog.bottom_right[1])
                            .speed(1.0),
                    );
                });
                ui.horizontal(|ui| {
                    ui.label("Bottom-left");
                    ui.add(
                        egui::DragValue::new(&mut app.perspective_dialog.bottom_left[0]).speed(1.0),
                    );
                    ui.add(
                        egui::DragValue::new(&mut app.perspective_dialog.bottom_left[1]).speed(1.0),
                    );
                });
                ui.separator();
                ui.horizontal(|ui| {
                    ui.label("Output width");
                    ui.add(
                        egui::DragValue::new(&mut app.perspective_dialog.output_width)
                            .range(1..=20_000),
                    );
                    ui.label("Output height");
                    ui.add(
                        egui::DragValue::new(&mut app.perspective_dialog.output_height)
                            .range(1..=20_000),
                    );
                });
                ui.horizontal_wrapped(|ui| {
                    ui.label("Interpolation");
                    ui.selectable_value(
                        &mut app.perspective_dialog.interpolation,
                        RotationInterpolation::Bilinear,
                        "Bilinear",
                    );
                    ui.selectable_value(
                        &mut app.perspective_dialog.interpolation,
                        RotationInterpolation::Nearest,
                        "Nearest",
                    );
                });
                ui.horizontal(|ui| {
                    ui.label("Fill");
                    ui.color_edit_button_srgba(&mut app.perspective_dialog.fill);
                });
                if ui.button("Apply correction").clicked() {
                    app.dispatch_transform(TransformOp::PerspectiveCorrect {
                        top_left: app.perspective_dialog.top_left,
                        top_right: app.perspective_dialog.top_right,
                        bottom_right: app.perspective_dialog.bottom_right,
                        bottom_left: app.perspective_dialog.bottom_left,
                        output_width: app.perspective_dialog.output_width.max(1),
                        output_height: app.perspective_dialog.output_height.max(1),
                        interpolation: app.perspective_dialog.interpolation,
                        fill: app.perspective_dialog.fill.to_array(),
                    });
                    *open_state = false;
                }
            },
        );
        self.perspective_dialog.open = open;
    }

    fn draw_magnifier_dialog(&mut self, ctx: &egui::Context) {
        if !self.magnifier_dialog.open {
            return;
        }
        let mut open = self.magnifier_dialog.open;
        self.show_popup_window(
            ctx,
            "popup.magnifier",
            "Zoom Magnifier",
            egui::vec2(460.0, 220.0),
            &mut open,
            |app, _ctx, ui, _open_state| {
                ui.checkbox(&mut app.magnifier_dialog.enabled, "Enable magnifier");
                ui.add(
                    egui::Slider::new(&mut app.magnifier_dialog.zoom, 1.1..=12.0)
                        .text("Lens zoom")
                        .fixed_decimals(1),
                );
                ui.add(
                    egui::Slider::new(&mut app.magnifier_dialog.size, 80.0..=320.0)
                        .text("Lens size")
                        .fixed_decimals(0),
                );
                ui.small("When enabled, hover over the image to inspect details.");
            },
        );
        self.magnifier_dialog.open = open;
    }

    fn draw_contact_sheet_dialog(&mut self, ctx: &egui::Context) {
        if !self.contact_sheet_dialog.open {
            return;
        }
        let mut open = self.contact_sheet_dialog.open;
        self.show_popup_window(
            ctx,
            "popup.contact-sheet",
            "Export Contact Sheet",
            egui::vec2(700.0, 360.0),
            &mut open,
            |app, _ctx, ui, open_state| {
                ui.horizontal(|ui| {
                    ui.label("Output file");
                    ui.text_edit_singleline(&mut app.contact_sheet_dialog.output_path);
                    if ui.button("Pick...").clicked() {
                        let mut dialog = rfd::FileDialog::new().set_title("Contact sheet output");
                        if let Some(directory) = app.state.current_directory_path() {
                            dialog = dialog.set_directory(directory);
                        }
                        dialog = dialog.set_file_name("contact-sheet.jpg");
                        if let Some(path) = dialog.save_file() {
                            app.contact_sheet_dialog.output_path = path.display().to_string();
                        }
                    }
                });
                ui.checkbox(
                    &mut app.contact_sheet_dialog.include_folder_images,
                    "Use all images in current folder",
                );
                ui.horizontal(|ui| {
                    ui.label("Columns");
                    ui.add(
                        egui::DragValue::new(&mut app.contact_sheet_dialog.columns).range(1..=32),
                    );
                    ui.label("Thumbnail size");
                    ui.add(
                        egui::DragValue::new(&mut app.contact_sheet_dialog.thumb_size)
                            .range(32..=1024),
                    );
                });
                ui.checkbox(
                    &mut app.contact_sheet_dialog.include_labels,
                    "Include file labels",
                );
                ui.horizontal(|ui| {
                    ui.label("Background");
                    ui.color_edit_button_srgba(&mut app.contact_sheet_dialog.background);
                    ui.label("Label color");
                    ui.color_edit_button_srgba(&mut app.contact_sheet_dialog.label_color);
                });
                ui.add(
                    egui::Slider::new(&mut app.contact_sheet_dialog.jpeg_quality, 1..=100)
                        .text("JPEG quality (if JPEG output)"),
                );
                let input_paths =
                    app.collect_utility_input_paths(app.contact_sheet_dialog.include_folder_images);
                ui.label(format!("Input images: {}", input_paths.len()));
                if ui.button("Export contact sheet").clicked() {
                    let output_path = PathBuf::from(app.contact_sheet_dialog.output_path.trim());
                    if output_path.as_os_str().is_empty() {
                        app.state.set_error("output path is required");
                    } else if input_paths.is_empty() {
                        app.state.set_error("no images available for contact sheet");
                    } else {
                        app.dispatch_export_contact_sheet(
                            input_paths,
                            output_path,
                            app.contact_sheet_dialog.columns.max(1),
                            app.contact_sheet_dialog.thumb_size.max(32),
                            app.contact_sheet_dialog.include_labels,
                            app.contact_sheet_dialog.background.to_array(),
                            app.contact_sheet_dialog.label_color.to_array(),
                            app.contact_sheet_dialog.jpeg_quality,
                        );
                        *open_state = false;
                    }
                }
            },
        );
        self.contact_sheet_dialog.open = open;
    }

    fn draw_html_export_dialog(&mut self, ctx: &egui::Context) {
        if !self.html_export_dialog.open {
            return;
        }
        let mut open = self.html_export_dialog.open;
        self.show_popup_window(
            ctx,
            "popup.html-export",
            "Export HTML Gallery",
            egui::vec2(680.0, 320.0),
            &mut open,
            |app, _ctx, ui, open_state| {
                ui.horizontal(|ui| {
                    ui.label("Output directory");
                    ui.text_edit_singleline(&mut app.html_export_dialog.output_dir);
                    if ui.button("Pick...").clicked() {
                        if let Some(path) = rfd::FileDialog::new().pick_folder() {
                            app.html_export_dialog.output_dir = path.display().to_string();
                        }
                    }
                });
                ui.horizontal(|ui| {
                    ui.label("Gallery title");
                    ui.text_edit_singleline(&mut app.html_export_dialog.title);
                });
                ui.add(
                    egui::Slider::new(&mut app.html_export_dialog.thumb_width, 64..=1024)
                        .text("Thumbnail width"),
                );
                ui.checkbox(
                    &mut app.html_export_dialog.include_folder_images,
                    "Use all images in current folder",
                );
                let input_paths =
                    app.collect_utility_input_paths(app.html_export_dialog.include_folder_images);
                ui.label(format!("Input images: {}", input_paths.len()));
                if ui.button("Export gallery").clicked() {
                    let output_dir = PathBuf::from(app.html_export_dialog.output_dir.trim());
                    if output_dir.as_os_str().is_empty() {
                        app.state.set_error("output directory is required");
                    } else if input_paths.is_empty() {
                        app.state.set_error("no images available for HTML export");
                    } else {
                        app.dispatch_export_html_gallery(
                            input_paths,
                            output_dir,
                            app.html_export_dialog.title.clone(),
                            app.html_export_dialog.thumb_width.max(64),
                        );
                        *open_state = false;
                    }
                }
            },
        );
        self.html_export_dialog.open = open;
    }

    fn draw_advanced_options_dialog(&mut self, ctx: &egui::Context) {
        if !self.advanced_options_dialog.open {
            return;
        }
        let before = self.advanced_options_dialog.clone();
        let mut open = self.advanced_options_dialog.open;
        self.show_popup_window(
            ctx,
            "popup.advanced-options",
            "Advanced Settings",
            egui::vec2(760.0, 420.0),
            &mut open,
            |app, _ctx, ui, _open_state| {
                ui.horizontal_wrapped(|ui| {
                    ui.selectable_value(
                        &mut app.advanced_options_dialog.active_tab,
                        AdvancedOptionsTab::Viewing,
                        "Viewing",
                    );
                    ui.selectable_value(
                        &mut app.advanced_options_dialog.active_tab,
                        AdvancedOptionsTab::Browsing,
                        "Browsing",
                    );
                    ui.selectable_value(
                        &mut app.advanced_options_dialog.active_tab,
                        AdvancedOptionsTab::Editing,
                        "Editing",
                    );
                    ui.selectable_value(
                        &mut app.advanced_options_dialog.active_tab,
                        AdvancedOptionsTab::Fullscreen,
                        "Fullscreen",
                    );
                    ui.selectable_value(
                        &mut app.advanced_options_dialog.active_tab,
                        AdvancedOptionsTab::Zoom,
                        "Zoom",
                    );
                    ui.selectable_value(
                        &mut app.advanced_options_dialog.active_tab,
                        AdvancedOptionsTab::ColorManagement,
                        "Color Management",
                    );
                    ui.selectable_value(
                        &mut app.advanced_options_dialog.active_tab,
                        AdvancedOptionsTab::Video,
                        "Video",
                    );
                    ui.selectable_value(
                        &mut app.advanced_options_dialog.active_tab,
                        AdvancedOptionsTab::Language,
                        "Language",
                    );
                    ui.selectable_value(
                        &mut app.advanced_options_dialog.active_tab,
                        AdvancedOptionsTab::Skins,
                        "Skins",
                    );
                    ui.selectable_value(
                        &mut app.advanced_options_dialog.active_tab,
                        AdvancedOptionsTab::Plugins,
                        "Plugins",
                    );
                    ui.selectable_value(
                        &mut app.advanced_options_dialog.active_tab,
                        AdvancedOptionsTab::Misc,
                        "Misc",
                    );
                    ui.selectable_value(
                        &mut app.advanced_options_dialog.active_tab,
                        AdvancedOptionsTab::FileHandling,
                        "File Handling",
                    );
                });
                ui.separator();
                match app.advanced_options_dialog.active_tab {
                    AdvancedOptionsTab::Viewing => {
                        ui.checkbox(
                            &mut app.advanced_options_dialog.checkerboard_background,
                            "Checkerboard image background",
                        );
                        ui.checkbox(
                            &mut app.advanced_options_dialog.smooth_main_scaling,
                            "Smooth texture scaling",
                        );
                    }
                    AdvancedOptionsTab::Browsing => {
                        ui.checkbox(
                            &mut app.advanced_options_dialog.browsing_wrap_navigation,
                            "Wrap next/previous navigation at folder boundaries",
                        );
                        ui.small("When disabled, left/right navigation stops at first/last image.");
                    }
                    AdvancedOptionsTab::Editing => {
                        ui.add(
                            egui::Slider::new(
                                &mut app.advanced_options_dialog.default_jpeg_quality,
                                1..=100,
                            )
                            .text("Default JPEG quality"),
                        );
                        ui.checkbox(
                            &mut app.advanced_options_dialog.auto_reopen_after_save,
                            "Reopen image after Save As",
                        );
                    }
                    AdvancedOptionsTab::Fullscreen => {
                        ui.checkbox(
                            &mut app.advanced_options_dialog.hide_toolbar_in_fullscreen,
                            "Hide toolbar when fullscreen",
                        );
                    }
                    AdvancedOptionsTab::Zoom => {
                        ui.add(
                            egui::Slider::new(
                                &mut app.advanced_options_dialog.zoom_step_percent,
                                5.0..=200.0,
                            )
                            .text("Zoom step percent")
                            .fixed_decimals(0),
                        );
                        ui.small("Used for +/- buttons, wheel+Ctrl zoom, and keyboard +/- shortcuts.");
                    }
                    AdvancedOptionsTab::ColorManagement => {
                        ui.checkbox(
                            &mut app.advanced_options_dialog.enable_color_management,
                            "Enable color-management pipeline (preview)",
                        );
                        ui.checkbox(
                            &mut app.advanced_options_dialog.simulate_srgb_output,
                            "Assume sRGB output profile",
                        );
                        ui.add(
                            egui::Slider::new(
                                &mut app.advanced_options_dialog.display_gamma,
                                1.6..=3.0,
                            )
                            .text("Display gamma")
                            .fixed_decimals(2),
                        );
                    }
                    AdvancedOptionsTab::Video => {
                        ui.add(
                            egui::Slider::new(
                                &mut app.advanced_options_dialog.video_frame_step_ms,
                                10..=1000,
                            )
                            .text("Frame-step interval (ms)"),
                        );
                        ui.small("Used by frame-step timing controls in media-like workflows.");
                    }
                    AdvancedOptionsTab::Language => {
                        ui.label("UI language");
                        egui::ComboBox::from_id_salt("advanced-language")
                            .selected_text(app.advanced_options_dialog.ui_language.as_str())
                            .show_ui(ui, |ui| {
                                for candidate in
                                    ["System", "English", "Hindi", "German", "French", "Spanish"]
                                {
                                    ui.selectable_value(
                                        &mut app.advanced_options_dialog.ui_language,
                                        candidate.to_owned(),
                                        candidate,
                                    );
                                }
                            });
                    }
                    AdvancedOptionsTab::Skins => {
                        ui.label("Skin");
                        egui::ComboBox::from_id_salt("advanced-skin")
                            .selected_text(app.advanced_options_dialog.skin_name.as_str())
                            .show_ui(ui, |ui| {
                                for candidate in ["Classic", "Graphite", "Mist"] {
                                    ui.selectable_value(
                                        &mut app.advanced_options_dialog.skin_name,
                                        candidate.to_owned(),
                                        candidate,
                                    );
                                }
                            });
                        ui.small("Skins tune window/panel colors while preserving lightweight rendering.");
                    }
                    AdvancedOptionsTab::Plugins => {
                        ui.horizontal(|ui| {
                            ui.label("Plugin search path");
                            ui.text_edit_singleline(
                                &mut app.advanced_options_dialog.plugin_search_path,
                            );
                            if ui.button("Pick...").clicked() {
                                if let Some(path) = rfd::FileDialog::new()
                                    .set_title("Plugin search directory")
                                    .pick_folder()
                                {
                                    app.advanced_options_dialog.plugin_search_path =
                                        path.display().to_string();
                                }
                            }
                        });
                        ui.small(
                            "Built-in plugins stay active. This path is reserved for external plugin discovery.",
                        );
                    }
                    AdvancedOptionsTab::Misc => {
                        ui.checkbox(
                            &mut app.advanced_options_dialog.keep_single_instance,
                            "Keep single instance (launch requests reuse current app)",
                        );
                    }
                    AdvancedOptionsTab::FileHandling => {
                        ui.checkbox(
                            &mut app.advanced_options_dialog.confirm_delete,
                            "Confirm before deleting files",
                        );
                        ui.checkbox(
                            &mut app.advanced_options_dialog.confirm_overwrite,
                            "Confirm before overwrite in save flows",
                        );
                    }
                }
                app.advanced_options_dialog.default_jpeg_quality = app
                    .advanced_options_dialog
                    .default_jpeg_quality
                    .clamp(1, 100);
                app.advanced_options_dialog.zoom_step_percent =
                    app.advanced_options_dialog.zoom_step_percent.clamp(5.0, 200.0);
                app.advanced_options_dialog.display_gamma =
                    app.advanced_options_dialog.display_gamma.clamp(1.6, 3.0);
                app.advanced_options_dialog.video_frame_step_ms =
                    app.advanced_options_dialog.video_frame_step_ms.clamp(10, 1000);
                if app.advanced_options_dialog.ui_language.trim().is_empty() {
                    app.advanced_options_dialog.ui_language = "System".to_owned();
                }
                if app.advanced_options_dialog.skin_name.trim().is_empty() {
                    app.advanced_options_dialog.skin_name = "Classic".to_owned();
                }
                app.advanced_options_dialog.plugin_search_path = app
                    .advanced_options_dialog
                    .plugin_search_path
                    .trim()
                    .to_owned();
            },
        );
        self.advanced_options_dialog.open = open;
        if self.advanced_options_dialog != before {
            self.apply_selected_skin(ctx);
            self.update_main_texture_from_state(ctx);
            self.persist_settings();
        }
    }

    fn draw_delete_confirmation(&mut self, ctx: &egui::Context) {
        if !self.confirm_delete_current {
            return;
        }

        let mut open = self.confirm_delete_current;
        self.show_popup_window(
            ctx,
            "popup.delete",
            "Delete Current File",
            egui::vec2(420.0, 150.0),
            &mut open,
            |app, _ctx, ui, open_state| {
                ui.label("Delete the current image from disk?");
                ui.horizontal(|ui| {
                    if ui.button("Delete").clicked() {
                        app.delete_current_file();
                        *open_state = false;
                    }
                    if ui.button("Cancel").clicked() {
                        *open_state = false;
                    }
                });
            },
        );
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
            .frame(Self::native_bar_frame(ctx))
            .exact_height(STATUS_PANEL_HEIGHT)
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
    fn update(&mut self, ctx: &egui::Context, frame: &mut eframe::Frame) {
        self.maybe_install_native_menu(frame);
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
