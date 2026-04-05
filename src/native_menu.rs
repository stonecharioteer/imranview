use anyhow::{Context, Result};
use muda::accelerator::{Accelerator, Code, Modifiers};
use muda::{CheckMenuItem, Menu, MenuEvent, MenuItem, PredefinedMenuItem, Submenu};

use crate::app_state::AppState;

fn primary_menu_modifier() -> Modifiers {
    #[cfg(target_os = "macos")]
    {
        Modifiers::SUPER
    }
    #[cfg(not(target_os = "macos"))]
    {
        Modifiers::CONTROL
    }
}

const ID_FILE_OPEN: &str = "menu.file.open";
const ID_FILE_SAVE: &str = "menu.file.save";
const ID_FILE_SAVE_AS: &str = "menu.file.save_as";
const ID_FILE_LOSSLESS_JPEG: &str = "menu.file.lossless_jpeg";
const ID_FILE_EXIF_DATE: &str = "menu.file.exif_date";
const ID_FILE_COLOR_PROFILE: &str = "menu.file.color_profile";
const ID_FILE_RENAME_CURRENT: &str = "menu.file.rename_current";
const ID_FILE_COPY_CURRENT: &str = "menu.file.copy_current";
const ID_FILE_MOVE_CURRENT: &str = "menu.file.move_current";
const ID_FILE_DELETE_CURRENT: &str = "menu.file.delete_current";
const ID_FILE_BATCH_CONVERT: &str = "menu.file.batch_convert";
const ID_FILE_RUN_SCRIPT: &str = "menu.file.run_script";
const ID_FILE_BATCH_SCAN: &str = "menu.file.batch_scan";
const ID_FILE_SCREENSHOT: &str = "menu.file.screenshot";
const ID_FILE_OCR: &str = "menu.file.ocr";
const ID_FILE_SEARCH_FILES: &str = "menu.file.search_files";
const ID_FILE_TIFF: &str = "menu.file.tiff";
const ID_FILE_PDF: &str = "menu.file.pdf";
const ID_FILE_CONTACT_SHEET: &str = "menu.file.contact_sheet";
const ID_FILE_HTML_GALLERY: &str = "menu.file.html_gallery";
const ID_FILE_PRINT_CURRENT: &str = "menu.file.print_current";
const ID_FILE_LOAD_COMPARE: &str = "menu.file.load_compare";
const ID_FILE_TOGGLE_COMPARE: &str = "menu.file.toggle_compare";
const ID_FILE_EXIT: &str = "menu.file.exit";
const ID_APP_ABOUT: &str = "menu.app.about";
const ID_HELP_ABOUT: &str = "menu.help.about";
const ID_EDIT_UNDO: &str = "menu.edit.undo";
const ID_EDIT_REDO: &str = "menu.edit.redo";
const ID_EDIT_ROTATE_LEFT: &str = "menu.edit.rotate_left";
const ID_EDIT_ROTATE_RIGHT: &str = "menu.edit.rotate_right";
const ID_EDIT_FLIP_HORIZONTAL: &str = "menu.edit.flip_horizontal";
const ID_EDIT_FLIP_VERTICAL: &str = "menu.edit.flip_vertical";
const ID_EDIT_RESIZE: &str = "menu.edit.resize";
const ID_EDIT_CROP: &str = "menu.edit.crop";
const ID_EDIT_COLOR: &str = "menu.edit.color";
const ID_EDIT_BORDER_FRAME: &str = "menu.edit.border_frame";
const ID_EDIT_CANVAS_SIZE: &str = "menu.edit.canvas_size";
const ID_EDIT_FINE_ROTATION: &str = "menu.edit.fine_rotation";
const ID_EDIT_TEXT_TOOL: &str = "menu.edit.text_tool";
const ID_EDIT_SHAPE_TOOL: &str = "menu.edit.shape_tool";
const ID_EDIT_OVERLAY: &str = "menu.edit.overlay";
const ID_EDIT_SELECTION: &str = "menu.edit.selection";
const ID_EDIT_REPLACE_COLOR: &str = "menu.edit.replace_color";
const ID_EDIT_ALPHA_TOOLS: &str = "menu.edit.alpha_tools";
const ID_EDIT_EFFECTS: &str = "menu.edit.effects";
const ID_EDIT_PERSPECTIVE: &str = "menu.edit.perspective";
const ID_EDIT_PANORAMA: &str = "menu.edit.panorama";
const ID_VIEW_SHOW_TOOLBAR: &str = "menu.view.show_toolbar";
const ID_VIEW_SHOW_STATUS_BAR: &str = "menu.view.show_status_bar";
const ID_VIEW_SHOW_METADATA_PANEL: &str = "menu.view.show_metadata_panel";
const ID_VIEW_SHOW_THUMBNAIL_STRIP: &str = "menu.view.show_thumbnail_strip";
const ID_VIEW_SHOW_THUMBNAIL_WINDOW: &str = "menu.view.show_thumbnail_window";
const ID_VIEW_COMMAND_PALETTE: &str = "menu.view.command_palette";
const ID_VIEW_MAGNIFIER: &str = "menu.view.magnifier";
const ID_OPTIONS_PERFORMANCE: &str = "menu.options.performance";
const ID_OPTIONS_CLEAR_CACHES: &str = "menu.options.clear_caches";
const ID_OPTIONS_ADVANCED: &str = "menu.options.advanced";

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum NativeMenuAction {
    About,
    Open,
    Save,
    SaveAs,
    LosslessJpeg,
    ChangeExifDate,
    ConvertColorProfile,
    RenameCurrent,
    CopyCurrentToFolder,
    MoveCurrentToFolder,
    DeleteCurrent,
    BatchConvert,
    RunAutomationScript,
    BatchScan,
    ScreenshotCapture,
    OcrWorkflow,
    SearchFiles,
    MultipageTiff,
    MultipagePdf,
    ExportContactSheet,
    ExportHtmlGallery,
    PrintCurrent,
    LoadCompareImage,
    ToggleCompareMode,
    Exit,
    Undo,
    Redo,
    RotateLeft,
    RotateRight,
    FlipHorizontal,
    FlipVertical,
    Resize,
    Crop,
    ColorCorrections,
    AddBorderFrame,
    CanvasSize,
    FineRotation,
    TextTool,
    ShapeTool,
    OverlayWatermark,
    SelectionWorkflows,
    ReplaceColor,
    AlphaTools,
    Effects,
    PerspectiveCorrection,
    PanoramaStitch,
    CommandPalette,
    ToggleShowToolbar,
    ToggleShowStatusBar,
    ToggleShowMetadataPanel,
    ToggleThumbnailStrip,
    ToggleThumbnailWindow,
    Magnifier,
    PerformanceSettings,
    ClearRuntimeCaches,
    AdvancedSettings,
}

pub struct NativeMenu {
    _menu: Menu,
    file_save: MenuItem,
    file_save_as: MenuItem,
    file_lossless_jpeg: MenuItem,
    file_exif_date: MenuItem,
    file_color_profile: MenuItem,
    file_rename_current: MenuItem,
    file_copy_current: MenuItem,
    file_move_current: MenuItem,
    file_delete_current: MenuItem,
    file_run_script: MenuItem,
    file_batch_scan: MenuItem,
    file_screenshot: MenuItem,
    file_ocr: MenuItem,
    file_print_current: MenuItem,
    file_search_files: MenuItem,
    file_tiff: MenuItem,
    file_pdf: MenuItem,
    file_contact_sheet: MenuItem,
    file_html_gallery: MenuItem,
    file_load_compare: MenuItem,
    file_toggle_compare: MenuItem,
    edit_undo: MenuItem,
    edit_redo: MenuItem,
    edit_rotate_left: MenuItem,
    edit_rotate_right: MenuItem,
    edit_flip_horizontal: MenuItem,
    edit_flip_vertical: MenuItem,
    edit_resize: MenuItem,
    edit_crop: MenuItem,
    edit_color: MenuItem,
    edit_border_frame: MenuItem,
    edit_canvas_size: MenuItem,
    edit_fine_rotation: MenuItem,
    edit_text_tool: MenuItem,
    edit_shape_tool: MenuItem,
    edit_overlay: MenuItem,
    edit_selection: MenuItem,
    edit_replace_color: MenuItem,
    edit_alpha_tools: MenuItem,
    edit_effects: MenuItem,
    edit_perspective: MenuItem,
    edit_panorama: MenuItem,
    view_show_toolbar: CheckMenuItem,
    view_show_status_bar: CheckMenuItem,
    view_show_metadata_panel: CheckMenuItem,
    view_show_thumbnail_strip: CheckMenuItem,
    view_show_thumbnail_window: CheckMenuItem,
    view_command_palette: MenuItem,
    view_magnifier: MenuItem,
    options_performance: MenuItem,
    options_clear_caches: MenuItem,
    options_advanced: MenuItem,
}

impl NativeMenu {
    pub fn install(_frame: &eframe::Frame) -> Result<Self> {
        let menu = Menu::new();

        #[cfg(target_os = "macos")]
        {
            let app_menu = Submenu::new("App", true);
            menu.append(&app_menu)
                .context("failed to append App menu")?;
            let app_about = MenuItem::with_id(ID_APP_ABOUT, "About ImranView", true, None);
            app_menu
                .append_items(&[
                    &app_about,
                    &PredefinedMenuItem::separator(),
                    &PredefinedMenuItem::services(None),
                    &PredefinedMenuItem::separator(),
                    &PredefinedMenuItem::hide(None),
                    &PredefinedMenuItem::hide_others(None),
                    &PredefinedMenuItem::show_all(None),
                    &PredefinedMenuItem::separator(),
                    &PredefinedMenuItem::quit(Some("Quit ImranView")),
                ])
                .context("failed to populate App menu")?;
        }

        let file_menu = Submenu::new("File", true);
        menu.append(&file_menu)
            .context("failed to append File menu")?;
        let file_open = MenuItem::with_id(
            ID_FILE_OPEN,
            "Open...",
            true,
            Some(Accelerator::new(Some(primary_menu_modifier()), Code::KeyO)),
        );
        let file_save = MenuItem::with_id(
            ID_FILE_SAVE,
            "Save",
            false,
            Some(Accelerator::new(Some(primary_menu_modifier()), Code::KeyS)),
        );
        let file_save_as = MenuItem::with_id(
            ID_FILE_SAVE_AS,
            "Save As...",
            false,
            Some(Accelerator::new(
                Some(primary_menu_modifier() | Modifiers::SHIFT),
                Code::KeyS,
            )),
        );
        let file_rename_current =
            MenuItem::with_id(ID_FILE_RENAME_CURRENT, "Rename Current...", false, None);
        let file_lossless_jpeg = MenuItem::with_id(
            ID_FILE_LOSSLESS_JPEG,
            "Lossless JPEG Transform...",
            false,
            None,
        );
        let file_exif_date =
            MenuItem::with_id(ID_FILE_EXIF_DATE, "Change EXIF Date/Time...", false, None);
        let file_color_profile = MenuItem::with_id(
            ID_FILE_COLOR_PROFILE,
            "Convert Color Profile...",
            false,
            None,
        );
        let file_copy_current = MenuItem::with_id(
            ID_FILE_COPY_CURRENT,
            "Copy Current to Folder...",
            false,
            None,
        );
        let file_move_current = MenuItem::with_id(
            ID_FILE_MOVE_CURRENT,
            "Move Current to Folder...",
            false,
            None,
        );
        let file_delete_current =
            MenuItem::with_id(ID_FILE_DELETE_CURRENT, "Delete Current...", false, None);
        let file_batch_convert = MenuItem::with_id(
            ID_FILE_BATCH_CONVERT,
            "Batch Convert / Rename...",
            true,
            None,
        );
        let file_run_script =
            MenuItem::with_id(ID_FILE_RUN_SCRIPT, "Run Automation Script...", true, None);
        let file_batch_scan =
            MenuItem::with_id(ID_FILE_BATCH_SCAN, "Batch Scan / Import...", true, None);
        let file_screenshot =
            MenuItem::with_id(ID_FILE_SCREENSHOT, "Screenshot Capture...", true, None);
        let file_ocr = MenuItem::with_id(ID_FILE_OCR, "OCR...", false, None);
        let file_search_files =
            MenuItem::with_id(ID_FILE_SEARCH_FILES, "Search Files...", false, None);
        let file_tiff = MenuItem::with_id(ID_FILE_TIFF, "Multipage TIFF...", false, None);
        let file_pdf = MenuItem::with_id(ID_FILE_PDF, "Create Multipage PDF...", false, None);
        let file_contact_sheet = MenuItem::with_id(
            ID_FILE_CONTACT_SHEET,
            "Export Contact Sheet...",
            false,
            None,
        );
        let file_html_gallery =
            MenuItem::with_id(ID_FILE_HTML_GALLERY, "Export HTML Gallery...", false, None);
        let file_print_current =
            MenuItem::with_id(ID_FILE_PRINT_CURRENT, "Print Current...", false, None);
        let file_load_compare =
            MenuItem::with_id(ID_FILE_LOAD_COMPARE, "Load Compare Image...", true, None);
        let file_toggle_compare =
            MenuItem::with_id(ID_FILE_TOGGLE_COMPARE, "Toggle Compare Mode", true, None);
        let file_exit = MenuItem::with_id(
            ID_FILE_EXIT,
            "Exit",
            true,
            Some(Accelerator::new(Some(primary_menu_modifier()), Code::KeyQ)),
        );
        file_menu
            .append_items(&[
                &file_open,
                &file_save,
                &file_save_as,
                &file_lossless_jpeg,
                &file_exif_date,
                &file_color_profile,
                &PredefinedMenuItem::separator(),
                &file_rename_current,
                &file_copy_current,
                &file_move_current,
                &file_delete_current,
                &PredefinedMenuItem::separator(),
                &file_batch_convert,
                &file_run_script,
                &file_batch_scan,
                &file_screenshot,
                &file_ocr,
                &file_search_files,
                &file_tiff,
                &file_pdf,
                &file_contact_sheet,
                &file_html_gallery,
                &file_print_current,
                &file_load_compare,
                &file_toggle_compare,
                &PredefinedMenuItem::separator(),
                &file_exit,
            ])
            .context("failed to populate File menu")?;

        let edit_menu = Submenu::new("Edit", true);
        menu.append(&edit_menu)
            .context("failed to append Edit menu")?;
        let edit_undo = MenuItem::with_id(
            ID_EDIT_UNDO,
            "Undo",
            false,
            Some(Accelerator::new(Some(primary_menu_modifier()), Code::KeyZ)),
        );
        let edit_redo = MenuItem::with_id(
            ID_EDIT_REDO,
            "Redo",
            false,
            Some(Accelerator::new(
                Some(primary_menu_modifier() | Modifiers::SHIFT),
                Code::KeyZ,
            )),
        );
        let edit_rotate_left = MenuItem::with_id(ID_EDIT_ROTATE_LEFT, "Rotate Left", false, None);
        let edit_rotate_right =
            MenuItem::with_id(ID_EDIT_ROTATE_RIGHT, "Rotate Right", false, None);
        let edit_flip_horizontal =
            MenuItem::with_id(ID_EDIT_FLIP_HORIZONTAL, "Flip Horizontal", false, None);
        let edit_flip_vertical =
            MenuItem::with_id(ID_EDIT_FLIP_VERTICAL, "Flip Vertical", false, None);
        let edit_resize = MenuItem::with_id(ID_EDIT_RESIZE, "Resize / Resample...", false, None);
        let edit_crop = MenuItem::with_id(ID_EDIT_CROP, "Crop...", false, None);
        let edit_color = MenuItem::with_id(ID_EDIT_COLOR, "Color Corrections...", false, None);
        let edit_border_frame =
            MenuItem::with_id(ID_EDIT_BORDER_FRAME, "Add Border / Frame...", false, None);
        let edit_canvas_size =
            MenuItem::with_id(ID_EDIT_CANVAS_SIZE, "Canvas Size...", false, None);
        let edit_fine_rotation =
            MenuItem::with_id(ID_EDIT_FINE_ROTATION, "Fine Rotation...", false, None);
        let edit_text_tool = MenuItem::with_id(ID_EDIT_TEXT_TOOL, "Text Tool...", false, None);
        let edit_shape_tool = MenuItem::with_id(ID_EDIT_SHAPE_TOOL, "Shape Tool...", false, None);
        let edit_overlay =
            MenuItem::with_id(ID_EDIT_OVERLAY, "Overlay / Watermark...", false, None);
        let edit_selection =
            MenuItem::with_id(ID_EDIT_SELECTION, "Selection Workflows...", false, None);
        let edit_replace_color =
            MenuItem::with_id(ID_EDIT_REPLACE_COLOR, "Replace Color...", false, None);
        let edit_alpha_tools =
            MenuItem::with_id(ID_EDIT_ALPHA_TOOLS, "Alpha Tools...", false, None);
        let edit_effects = MenuItem::with_id(ID_EDIT_EFFECTS, "Effects...", false, None);
        let edit_perspective = MenuItem::with_id(
            ID_EDIT_PERSPECTIVE,
            "Perspective Correction...",
            false,
            None,
        );
        let edit_panorama = MenuItem::with_id(ID_EDIT_PANORAMA, "Panorama Stitch...", false, None);
        edit_menu
            .append_items(&[
                &edit_undo,
                &edit_redo,
                &PredefinedMenuItem::separator(),
                &edit_rotate_left,
                &edit_rotate_right,
                &edit_flip_horizontal,
                &edit_flip_vertical,
                &PredefinedMenuItem::separator(),
                &edit_resize,
                &edit_crop,
                &edit_color,
                &edit_border_frame,
                &edit_canvas_size,
                &edit_fine_rotation,
                &PredefinedMenuItem::separator(),
                &edit_text_tool,
                &edit_shape_tool,
                &edit_overlay,
                &edit_selection,
                &edit_replace_color,
                &edit_alpha_tools,
                &edit_effects,
                &edit_perspective,
                &edit_panorama,
            ])
            .context("failed to populate Edit menu")?;

        let view_menu = Submenu::new("View", true);
        menu.append(&view_menu)
            .context("failed to append View menu")?;
        let view_show_toolbar =
            CheckMenuItem::with_id(ID_VIEW_SHOW_TOOLBAR, "Show toolbar", true, true, None);
        let view_show_status_bar =
            CheckMenuItem::with_id(ID_VIEW_SHOW_STATUS_BAR, "Show status bar", true, true, None);
        let view_show_metadata_panel = CheckMenuItem::with_id(
            ID_VIEW_SHOW_METADATA_PANEL,
            "Metadata panel",
            true,
            false,
            None,
        );
        let view_show_thumbnail_strip = CheckMenuItem::with_id(
            ID_VIEW_SHOW_THUMBNAIL_STRIP,
            "Thumbnail strip",
            true,
            true,
            None,
        );
        let view_show_thumbnail_window = CheckMenuItem::with_id(
            ID_VIEW_SHOW_THUMBNAIL_WINDOW,
            "Thumbnail window",
            true,
            false,
            None,
        );
        let view_command_palette = MenuItem::with_id(
            ID_VIEW_COMMAND_PALETTE,
            "Command Palette...",
            true,
            Some(Accelerator::new(Some(primary_menu_modifier()), Code::KeyK)),
        );
        let view_magnifier = MenuItem::with_id(ID_VIEW_MAGNIFIER, "Zoom Magnifier...", true, None);
        view_menu
            .append_items(&[
                &view_command_palette,
                &PredefinedMenuItem::separator(),
                &view_show_toolbar,
                &view_show_status_bar,
                &view_show_metadata_panel,
                &view_show_thumbnail_strip,
                &view_show_thumbnail_window,
                &PredefinedMenuItem::separator(),
                &view_magnifier,
            ])
            .context("failed to populate View menu")?;

        let options_menu = Submenu::new("Options", true);
        menu.append(&options_menu)
            .context("failed to append Options menu")?;
        let options_performance =
            MenuItem::with_id(ID_OPTIONS_PERFORMANCE, "Performance / Cache...", true, None);
        let options_clear_caches =
            MenuItem::with_id(ID_OPTIONS_CLEAR_CACHES, "Clear Runtime Caches", true, None);
        let options_advanced =
            MenuItem::with_id(ID_OPTIONS_ADVANCED, "Advanced Settings...", true, None);
        options_menu
            .append_items(&[
                &options_performance,
                &options_clear_caches,
                &options_advanced,
            ])
            .context("failed to populate Options menu")?;

        let window_menu = Submenu::new("Window", true);
        menu.append(&window_menu)
            .context("failed to append Window menu")?;
        window_menu
            .append_items(&[
                &PredefinedMenuItem::minimize(None),
                &PredefinedMenuItem::separator(),
                &PredefinedMenuItem::close_window(None),
            ])
            .context("failed to populate Window menu")?;
        #[cfg(target_os = "macos")]
        window_menu.set_as_windows_menu_for_nsapp();

        let help_menu = Submenu::new("Help", true);
        menu.append(&help_menu)
            .context("failed to append Help menu")?;
        let help_about = MenuItem::with_id(ID_HELP_ABOUT, "About ImranView", true, None);
        help_menu
            .append_items(&[&help_about])
            .context("failed to populate Help menu")?;
        #[cfg(target_os = "macos")]
        help_menu.set_as_help_menu_for_nsapp();

        #[cfg(target_os = "macos")]
        menu.init_for_nsapp();
        #[cfg(target_os = "windows")]
        init_for_windows_hwnd(&menu, _frame)?;

        Ok(Self {
            _menu: menu,
            file_save,
            file_save_as,
            file_lossless_jpeg,
            file_exif_date,
            file_color_profile,
            file_rename_current,
            file_copy_current,
            file_move_current,
            file_delete_current,
            file_run_script,
            file_batch_scan,
            file_screenshot,
            file_ocr,
            file_print_current,
            file_search_files,
            file_tiff,
            file_pdf,
            file_contact_sheet,
            file_html_gallery,
            file_load_compare,
            file_toggle_compare,
            edit_undo,
            edit_redo,
            edit_rotate_left,
            edit_rotate_right,
            edit_flip_horizontal,
            edit_flip_vertical,
            edit_resize,
            edit_crop,
            edit_color,
            edit_border_frame,
            edit_canvas_size,
            edit_fine_rotation,
            edit_text_tool,
            edit_shape_tool,
            edit_overlay,
            edit_selection,
            edit_replace_color,
            edit_alpha_tools,
            edit_effects,
            edit_perspective,
            edit_panorama,
            view_show_toolbar,
            view_show_status_bar,
            view_show_metadata_panel,
            view_show_thumbnail_strip,
            view_show_thumbnail_window,
            view_command_palette,
            view_magnifier,
            options_performance,
            options_clear_caches,
            options_advanced,
        })
    }

    pub fn sync_state(&self, state: &AppState) {
        let has_image = state.has_image();
        self.file_save.set_enabled(has_image);
        self.file_save_as.set_enabled(has_image);
        self.file_lossless_jpeg.set_enabled(has_image);
        self.file_exif_date.set_enabled(has_image);
        self.file_color_profile.set_enabled(has_image);
        self.file_rename_current.set_enabled(has_image);
        self.file_copy_current.set_enabled(has_image);
        self.file_move_current.set_enabled(has_image);
        self.file_delete_current.set_enabled(has_image);
        self.file_run_script.set_enabled(true);
        self.file_batch_scan.set_enabled(true);
        self.file_screenshot.set_enabled(true);
        self.file_ocr.set_enabled(has_image);
        self.file_print_current.set_enabled(has_image);
        self.file_search_files
            .set_enabled(!state.images_in_directory().is_empty());
        self.file_tiff.set_enabled(has_image);
        self.file_pdf.set_enabled(has_image);
        self.file_contact_sheet.set_enabled(has_image);
        self.file_html_gallery.set_enabled(has_image);
        self.file_load_compare.set_enabled(has_image);
        self.file_toggle_compare.set_enabled(has_image);
        self.edit_undo.set_enabled(state.can_undo());
        self.edit_redo.set_enabled(state.can_redo());
        self.edit_rotate_left.set_enabled(has_image);
        self.edit_rotate_right.set_enabled(has_image);
        self.edit_flip_horizontal.set_enabled(has_image);
        self.edit_flip_vertical.set_enabled(has_image);
        self.edit_resize.set_enabled(has_image);
        self.edit_crop.set_enabled(has_image);
        self.edit_color.set_enabled(has_image);
        self.edit_border_frame.set_enabled(has_image);
        self.edit_canvas_size.set_enabled(has_image);
        self.edit_fine_rotation.set_enabled(has_image);
        self.edit_text_tool.set_enabled(has_image);
        self.edit_shape_tool.set_enabled(has_image);
        self.edit_overlay.set_enabled(has_image);
        self.edit_selection.set_enabled(has_image);
        self.edit_replace_color.set_enabled(has_image);
        self.edit_alpha_tools.set_enabled(has_image);
        self.edit_effects.set_enabled(has_image);
        self.edit_perspective.set_enabled(has_image);
        self.edit_panorama.set_enabled(has_image);
        self.view_magnifier.set_enabled(has_image);
        self.options_performance.set_enabled(true);
        self.options_clear_caches.set_enabled(true);
        self.options_advanced.set_enabled(true);

        self.view_show_toolbar.set_checked(state.show_toolbar());
        self.view_show_status_bar
            .set_checked(state.show_status_bar());
        self.view_show_metadata_panel
            .set_checked(state.show_metadata_panel());
        self.view_show_thumbnail_strip
            .set_checked(state.show_thumbnail_strip());
        self.view_show_thumbnail_window
            .set_checked(state.thumbnails_window_mode());
        self.view_command_palette.set_enabled(true);
    }

    pub fn drain_actions(&self) -> Vec<NativeMenuAction> {
        let mut actions = Vec::new();
        while let Ok(event) = MenuEvent::receiver().try_recv() {
            let action = match event.id.as_ref() {
                ID_APP_ABOUT | ID_HELP_ABOUT => Some(NativeMenuAction::About),
                ID_FILE_OPEN => Some(NativeMenuAction::Open),
                ID_FILE_SAVE => Some(NativeMenuAction::Save),
                ID_FILE_SAVE_AS => Some(NativeMenuAction::SaveAs),
                ID_FILE_LOSSLESS_JPEG => Some(NativeMenuAction::LosslessJpeg),
                ID_FILE_EXIF_DATE => Some(NativeMenuAction::ChangeExifDate),
                ID_FILE_COLOR_PROFILE => Some(NativeMenuAction::ConvertColorProfile),
                ID_FILE_RENAME_CURRENT => Some(NativeMenuAction::RenameCurrent),
                ID_FILE_COPY_CURRENT => Some(NativeMenuAction::CopyCurrentToFolder),
                ID_FILE_MOVE_CURRENT => Some(NativeMenuAction::MoveCurrentToFolder),
                ID_FILE_DELETE_CURRENT => Some(NativeMenuAction::DeleteCurrent),
                ID_FILE_BATCH_CONVERT => Some(NativeMenuAction::BatchConvert),
                ID_FILE_RUN_SCRIPT => Some(NativeMenuAction::RunAutomationScript),
                ID_FILE_BATCH_SCAN => Some(NativeMenuAction::BatchScan),
                ID_FILE_SCREENSHOT => Some(NativeMenuAction::ScreenshotCapture),
                ID_FILE_OCR => Some(NativeMenuAction::OcrWorkflow),
                ID_FILE_SEARCH_FILES => Some(NativeMenuAction::SearchFiles),
                ID_FILE_TIFF => Some(NativeMenuAction::MultipageTiff),
                ID_FILE_PDF => Some(NativeMenuAction::MultipagePdf),
                ID_FILE_CONTACT_SHEET => Some(NativeMenuAction::ExportContactSheet),
                ID_FILE_HTML_GALLERY => Some(NativeMenuAction::ExportHtmlGallery),
                ID_FILE_PRINT_CURRENT => Some(NativeMenuAction::PrintCurrent),
                ID_FILE_LOAD_COMPARE => Some(NativeMenuAction::LoadCompareImage),
                ID_FILE_TOGGLE_COMPARE => Some(NativeMenuAction::ToggleCompareMode),
                ID_FILE_EXIT => Some(NativeMenuAction::Exit),
                ID_EDIT_UNDO => Some(NativeMenuAction::Undo),
                ID_EDIT_REDO => Some(NativeMenuAction::Redo),
                ID_EDIT_ROTATE_LEFT => Some(NativeMenuAction::RotateLeft),
                ID_EDIT_ROTATE_RIGHT => Some(NativeMenuAction::RotateRight),
                ID_EDIT_FLIP_HORIZONTAL => Some(NativeMenuAction::FlipHorizontal),
                ID_EDIT_FLIP_VERTICAL => Some(NativeMenuAction::FlipVertical),
                ID_EDIT_RESIZE => Some(NativeMenuAction::Resize),
                ID_EDIT_CROP => Some(NativeMenuAction::Crop),
                ID_EDIT_COLOR => Some(NativeMenuAction::ColorCorrections),
                ID_EDIT_BORDER_FRAME => Some(NativeMenuAction::AddBorderFrame),
                ID_EDIT_CANVAS_SIZE => Some(NativeMenuAction::CanvasSize),
                ID_EDIT_FINE_ROTATION => Some(NativeMenuAction::FineRotation),
                ID_EDIT_TEXT_TOOL => Some(NativeMenuAction::TextTool),
                ID_EDIT_SHAPE_TOOL => Some(NativeMenuAction::ShapeTool),
                ID_EDIT_OVERLAY => Some(NativeMenuAction::OverlayWatermark),
                ID_EDIT_SELECTION => Some(NativeMenuAction::SelectionWorkflows),
                ID_EDIT_REPLACE_COLOR => Some(NativeMenuAction::ReplaceColor),
                ID_EDIT_ALPHA_TOOLS => Some(NativeMenuAction::AlphaTools),
                ID_EDIT_EFFECTS => Some(NativeMenuAction::Effects),
                ID_EDIT_PERSPECTIVE => Some(NativeMenuAction::PerspectiveCorrection),
                ID_EDIT_PANORAMA => Some(NativeMenuAction::PanoramaStitch),
                ID_VIEW_SHOW_TOOLBAR => Some(NativeMenuAction::ToggleShowToolbar),
                ID_VIEW_SHOW_STATUS_BAR => Some(NativeMenuAction::ToggleShowStatusBar),
                ID_VIEW_SHOW_METADATA_PANEL => Some(NativeMenuAction::ToggleShowMetadataPanel),
                ID_VIEW_SHOW_THUMBNAIL_STRIP => Some(NativeMenuAction::ToggleThumbnailStrip),
                ID_VIEW_SHOW_THUMBNAIL_WINDOW => Some(NativeMenuAction::ToggleThumbnailWindow),
                ID_VIEW_COMMAND_PALETTE => Some(NativeMenuAction::CommandPalette),
                ID_VIEW_MAGNIFIER => Some(NativeMenuAction::Magnifier),
                ID_OPTIONS_PERFORMANCE => Some(NativeMenuAction::PerformanceSettings),
                ID_OPTIONS_CLEAR_CACHES => Some(NativeMenuAction::ClearRuntimeCaches),
                ID_OPTIONS_ADVANCED => Some(NativeMenuAction::AdvancedSettings),
                _ => None,
            };
            if let Some(action) = action {
                actions.push(action);
            }
        }
        actions
    }
}

#[cfg(target_os = "windows")]
fn init_for_windows_hwnd(menu: &Menu, frame: &eframe::Frame) -> Result<()> {
    use raw_window_handle::{HasWindowHandle, RawWindowHandle};

    let window_handle = frame
        .window_handle()
        .context("failed to fetch native window handle")?;
    let RawWindowHandle::Win32(handle) = window_handle.as_raw() else {
        anyhow::bail!("unsupported window handle for native menu integration");
    };
    let hwnd = handle.hwnd.get();
    // SAFETY: hwnd originates from eframe's current root viewport window handle.
    unsafe { menu.init_for_hwnd(hwnd) }.context("failed to attach native menu to window")?;
    Ok(())
}
