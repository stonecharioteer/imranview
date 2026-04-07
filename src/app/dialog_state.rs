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

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum AlphaToolMode {
    Global,
    Brush,
}

#[derive(Clone, Debug)]
struct AlphaDialogState {
    open: bool,
    mode: AlphaToolMode,
    alpha_percent: f32,
    alpha_from_luma: bool,
    invert_luma: bool,
    limit_to_region: bool,
    region_x: u32,
    region_y: u32,
    region_width: u32,
    region_height: u32,
    brush_center_x: u32,
    brush_center_y: u32,
    brush_radius: u32,
    brush_strength_percent: f32,
    brush_softness: f32,
    brush_operation: AlphaBrushOp,
}

impl Default for AlphaDialogState {
    fn default() -> Self {
        Self {
            open: false,
            mode: AlphaToolMode::Global,
            alpha_percent: 100.0,
            alpha_from_luma: false,
            invert_luma: false,
            limit_to_region: false,
            region_x: 0,
            region_y: 0,
            region_width: 256,
            region_height: 256,
            brush_center_x: 128,
            brush_center_y: 128,
            brush_radius: 40,
            brush_strength_percent: 50.0,
            brush_softness: 0.4,
            brush_operation: AlphaBrushOp::Decrease,
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
    catalog_cache_size_bytes: u64,
    catalog_tracked_folders: usize,
    catalog_persisted_folders: usize,
    catalog_entries: usize,
    catalog_last_error: Option<String>,
}

impl Default for PerformanceDialogState {
    fn default() -> Self {
        Self {
            open: false,
            thumb_cache_entry_cap: THUMB_TEXTURE_CACHE_CAP,
            thumb_cache_max_mb: THUMB_TEXTURE_CACHE_MAX_BYTES / (1024 * 1024),
            preload_cache_entry_cap: 6,
            preload_cache_max_mb: 192,
            catalog_cache_size_bytes: 0,
            catalog_tracked_folders: 0,
            catalog_persisted_folders: 0,
            catalog_entries: 0,
            catalog_last_error: None,
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
    device_name: String,
    dpi: u32,
    grayscale: bool,
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
            device_name: String::new(),
            dpi: 300,
            grayscale: false,
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum BatchScanSource {
    FolderImport,
    ScannerCommand,
    NativeBackend,
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

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum FileSortMode {
    Name,
    Extension,
    ModifiedTime,
    FileSize,
}

impl FileSortMode {
    fn as_label(self) -> &'static str {
        match self {
            FileSortMode::Name => "Name",
            FileSortMode::Extension => "Type/Extension",
            FileSortMode::ModifiedTime => "Modified Time",
            FileSortMode::FileSize => "File Size",
        }
    }

    fn as_settings_value(self) -> &'static str {
        match self {
            FileSortMode::Name => "name",
            FileSortMode::Extension => "extension",
            FileSortMode::ModifiedTime => "modified",
            FileSortMode::FileSize => "size",
        }
    }

    fn from_settings_value(value: &str) -> Self {
        match value.trim().to_ascii_lowercase().as_str() {
            "extension" | "ext" | "type" => FileSortMode::Extension,
            "modified" | "modified_time" | "date" | "time" => FileSortMode::ModifiedTime,
            "size" | "file_size" => FileSortMode::FileSize,
            _ => FileSortMode::Name,
        }
    }
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
    browsing_sort_mode: FileSortMode,
    browsing_sort_descending: bool,
    thumbnails_sort_mode: FileSortMode,
    thumbnails_sort_descending: bool,
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
            browsing_sort_mode: FileSortMode::Name,
            browsing_sort_descending: false,
            thumbnails_sort_mode: FileSortMode::Name,
            thumbnails_sort_descending: false,
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
