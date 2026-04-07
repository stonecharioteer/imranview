use super::*;

impl ImranViewApp {
    pub(super) fn run_menu_command(&mut self, ctx: &egui::Context, command: MenuCommand) {
        match command {
            MenuCommand::FileOpen => self.open_path_dialog(),
            MenuCommand::FileSave => self.dispatch_save(None, false, self.default_save_options()),
            MenuCommand::FileSaveAs | MenuCommand::FileSaveWithOptions => {
                self.open_save_as_dialog();
            }
            MenuCommand::FileLosslessJpegTransform => self.open_lossless_jpeg_dialog(),
            MenuCommand::FileChangeExifDateTime => self.open_exif_date_dialog(),
            MenuCommand::FileConvertColorProfile => self.open_color_profile_dialog(),
            MenuCommand::FileRenameCurrent => self.open_rename_dialog(),
            MenuCommand::FileCopyCurrentToFolder => self.copy_current_to_dialog(),
            MenuCommand::FileMoveCurrentToFolder => self.move_current_to_dialog(),
            MenuCommand::FileDeleteCurrent => {
                if self.advanced_options_dialog.confirm_delete {
                    self.confirm_delete_current = true;
                } else {
                    self.delete_current_file();
                }
            }
            MenuCommand::FileBatchConvertRename => self.open_batch_dialog(),
            MenuCommand::FileRunAutomationScript => self.open_batch_script_picker_and_dispatch(),
            MenuCommand::FileBatchScanImport => self.open_batch_scan_dialog(),
            MenuCommand::FileScreenshotCapture => self.open_screenshot_dialog(),
            MenuCommand::FileOcr => self.open_ocr_dialog(),
            MenuCommand::FilePrintCurrent => self.dispatch_print_current(),
            MenuCommand::FileOpenRecent(path) => self.dispatch_open(path, false),
            MenuCommand::FileOpenRecentFolder(path) => self.dispatch_open_directory(path),
            MenuCommand::FileSearchFiles => self.open_search_dialog(),
            MenuCommand::FileMultipageTiff => self.open_tiff_dialog(),
            MenuCommand::FileCreateMultipagePdf => self.open_pdf_dialog(),
            MenuCommand::FileExportContactSheet => self.open_contact_sheet_dialog(),
            MenuCommand::FileExportHtmlGallery => self.open_html_export_dialog(),
            MenuCommand::FileExit => {
                ctx.send_viewport_cmd(egui::ViewportCommand::Close);
            }
            MenuCommand::EditUndo => self.undo_edit(ctx),
            MenuCommand::EditRedo => self.redo_edit(ctx),
            MenuCommand::EditRotateLeft => self.dispatch_transform(TransformOp::RotateLeft),
            MenuCommand::EditRotateRight => self.dispatch_transform(TransformOp::RotateRight),
            MenuCommand::EditFlipHorizontal => self.dispatch_transform(TransformOp::FlipHorizontal),
            MenuCommand::EditFlipVertical => self.dispatch_transform(TransformOp::FlipVertical),
            MenuCommand::EditResizeResample => self.open_resize_dialog(),
            MenuCommand::EditCrop => self.open_crop_dialog(),
            MenuCommand::EditColorCorrections => self.open_color_dialog(),
            MenuCommand::EditBorderFrame => self.open_border_dialog(),
            MenuCommand::EditCanvasSize => self.open_canvas_dialog(),
            MenuCommand::EditFineRotation => self.open_fine_rotate_dialog(),
            MenuCommand::EditTextTool => self.open_text_tool_dialog(),
            MenuCommand::EditShapeTool => self.open_shape_tool_dialog(),
            MenuCommand::EditOverlayWatermark => self.open_overlay_dialog(),
            MenuCommand::EditSelectionWorkflows => self.open_selection_workflow_dialog(),
            MenuCommand::EditReplaceColor => self.open_replace_color_dialog(),
            MenuCommand::EditAlphaTools => self.open_alpha_dialog(),
            MenuCommand::EditEffects => self.open_effects_dialog(),
            MenuCommand::EditPerspectiveCorrection => self.open_perspective_dialog(),
            MenuCommand::ImagePrevious => self.open_previous(),
            MenuCommand::ImageNext => self.open_next(),
            MenuCommand::ImageZoomIn => self.zoom_in(),
            MenuCommand::ImageZoomOut => self.zoom_out(),
            MenuCommand::ImageFitToWindow => self.zoom_fit(),
            MenuCommand::ImageActualSize => self.zoom_actual(),
            MenuCommand::ImageToggleSlideshow => {
                if self.slideshow_running {
                    self.stop_slideshow();
                } else {
                    self.start_slideshow();
                }
            }
            MenuCommand::ImageLoadCompare => self.open_compare_path_dialog(),
            MenuCommand::ImageToggleCompareMode => {
                self.compare_mode = !self.compare_mode;
            }
            MenuCommand::ImagePanoramaStitch => self.open_panorama_dialog(),
            MenuCommand::ViewCommandPalette => self.open_command_palette(),
            MenuCommand::ViewToggleStatusBar => {
                self.state
                    .set_show_status_bar(!self.state.show_status_bar());
                self.persist_settings();
            }
            MenuCommand::ViewToggleToolbar => {
                self.state.set_show_toolbar(!self.state.show_toolbar());
                self.persist_settings();
            }
            MenuCommand::ViewToggleMetadataPanel => {
                self.state
                    .set_show_metadata_panel(!self.state.show_metadata_panel());
                self.persist_settings();
            }
            MenuCommand::ViewToggleThumbnailStrip => {
                self.state
                    .set_show_thumbnail_strip(!self.state.show_thumbnail_strip());
                self.persist_settings();
            }
            MenuCommand::ViewToggleThumbnailWindow => {
                self.state
                    .set_thumbnails_window_mode(!self.state.thumbnails_window_mode());
                self.persist_settings();
            }
            MenuCommand::ViewZoomMagnifier => self.open_magnifier_dialog(),
            MenuCommand::OptionsPerformanceCache => self.open_performance_dialog(),
            MenuCommand::OptionsClearRuntimeCaches => self.clear_runtime_caches(),
            MenuCommand::OptionsPurgeFolderCatalogCache => self.purge_folder_catalog_cache(),
            MenuCommand::OptionsAdvancedSettings => self.open_advanced_options_dialog(),
            MenuCommand::HelpAbout => self.open_about_window(),
        }
    }

    pub(super) fn command_palette_group_order(group: &str) -> usize {
        match group {
            "File" => 0,
            "Edit" => 1,
            "Image" => 2,
            "View" => 3,
            "Options" => 4,
            "Help" => 5,
            _ => usize::MAX,
        }
    }

    pub(super) fn fuzzy_match_score(query: &str, haystack: &str) -> Option<i32> {
        let query_chars: Vec<char> = query
            .chars()
            .filter(|ch| !ch.is_whitespace())
            .map(|ch| ch.to_ascii_lowercase())
            .collect();
        if query_chars.is_empty() {
            return Some(0);
        }

        let haystack_chars: Vec<char> =
            haystack.chars().map(|ch| ch.to_ascii_lowercase()).collect();
        let mut query_index = 0usize;
        let mut score = 0i32;
        let mut previous_match: Option<usize> = None;

        for (index, hay_char) in haystack_chars.iter().enumerate() {
            if query_index >= query_chars.len() {
                break;
            }
            if *hay_char == query_chars[query_index] {
                score += 10;
                if let Some(previous) = previous_match {
                    if index == previous + 1 {
                        score += 14;
                    } else if index <= previous + 3 {
                        score += 4;
                    }
                }
                if index == 0 || !haystack_chars[index - 1].is_ascii_alphanumeric() {
                    score += 8;
                }
                previous_match = Some(index);
                query_index += 1;
            }
        }

        if query_index != query_chars.len() {
            return None;
        }

        let density_penalty =
            (haystack_chars.len().saturating_sub(query_chars.len()) as i32).min(40);
        Some(score - density_penalty)
    }

    pub(super) fn collect_command_palette_entries(
        &self,
        ctx: &egui::Context,
    ) -> Vec<CommandPaletteEntry> {
        // Keep this list in sync with all menu-bar commands so new items automatically show up in
        // the command palette.
        let mut entries = Vec::with_capacity(96);
        let mut push = |group: &'static str,
                        title: &str,
                        shortcut: Option<ShortcutAction>,
                        command: MenuCommand,
                        aliases: &[&str]| {
            let mut search_blob = format!(
                "{} {}",
                group.to_ascii_lowercase(),
                title.to_ascii_lowercase()
            );
            for alias in aliases {
                search_blob.push(' ');
                search_blob.push_str(alias);
            }
            let enabled = self.is_menu_command_enabled(&command);
            entries.push(CommandPaletteEntry {
                group,
                title: title.to_owned(),
                shortcut: shortcut.and_then(|action| shortcut_text(ctx, action)),
                search_blob,
                enabled,
                command,
            });
        };

        push(
            "File",
            "Open...",
            Some(ShortcutAction::Open),
            MenuCommand::FileOpen,
            &["load", "file", "browse", "quick", "quick open"],
        );
        push(
            "File",
            "Save",
            Some(ShortcutAction::Save),
            MenuCommand::FileSave,
            &["write", "export"],
        );
        push(
            "File",
            "Save As...",
            Some(ShortcutAction::SaveAs),
            MenuCommand::FileSaveAs,
            &["duplicate", "export"],
        );
        push(
            "File",
            "Save with options...",
            None,
            MenuCommand::FileSaveWithOptions,
            &["save", "format", "metadata"],
        );
        push(
            "File",
            "Lossless JPEG transform...",
            None,
            MenuCommand::FileLosslessJpegTransform,
            &["jpegtran", "rotate", "flip"],
        );
        push(
            "File",
            "Change EXIF date/time...",
            None,
            MenuCommand::FileChangeExifDateTime,
            &["metadata", "timestamp", "capture"],
        );
        push(
            "File",
            "Convert color profile...",
            None,
            MenuCommand::FileConvertColorProfile,
            &["icc", "icm", "color"],
        );
        push(
            "File",
            "Rename current...",
            None,
            MenuCommand::FileRenameCurrent,
            &["rename", "file"],
        );
        push(
            "File",
            "Copy current to folder...",
            None,
            MenuCommand::FileCopyCurrentToFolder,
            &["copy", "file"],
        );
        push(
            "File",
            "Move current to folder...",
            None,
            MenuCommand::FileMoveCurrentToFolder,
            &["move", "file"],
        );
        push(
            "File",
            "Delete current...",
            None,
            MenuCommand::FileDeleteCurrent,
            &["delete", "remove"],
        );
        push(
            "File",
            "Batch convert / rename...",
            None,
            MenuCommand::FileBatchConvertRename,
            &["batch", "convert", "rename"],
        );
        push(
            "File",
            "Run automation script...",
            None,
            MenuCommand::FileRunAutomationScript,
            &["batch", "script", "json"],
        );
        push(
            "File",
            "Batch scan/import...",
            None,
            MenuCommand::FileBatchScanImport,
            &["scan", "import"],
        );
        push(
            "File",
            "Screenshot capture...",
            None,
            MenuCommand::FileScreenshotCapture,
            &["capture", "screen"],
        );
        push(
            "File",
            "OCR...",
            None,
            MenuCommand::FileOcr,
            &["text", "recognition"],
        );
        push(
            "File",
            "Print current...",
            None,
            MenuCommand::FilePrintCurrent,
            &["print", "paper"],
        );
        push(
            "File",
            "Search files...",
            None,
            MenuCommand::FileSearchFiles,
            &["search", "find", "folder"],
        );
        push(
            "File",
            "Multipage TIFF...",
            None,
            MenuCommand::FileMultipageTiff,
            &["tiff", "pages"],
        );
        push(
            "File",
            "Create multipage PDF...",
            None,
            MenuCommand::FileCreateMultipagePdf,
            &["pdf", "pages", "export"],
        );
        push(
            "File",
            "Export contact sheet...",
            None,
            MenuCommand::FileExportContactSheet,
            &["sheet", "thumbnails", "export"],
        );
        push(
            "File",
            "Export HTML gallery...",
            None,
            MenuCommand::FileExportHtmlGallery,
            &["html", "gallery", "export"],
        );
        push(
            "File",
            "Exit",
            None,
            MenuCommand::FileExit,
            &["quit", "close"],
        );

        push(
            "Edit",
            "Undo",
            Some(ShortcutAction::Undo),
            MenuCommand::EditUndo,
            &["history", "revert"],
        );
        push(
            "Edit",
            "Redo",
            Some(ShortcutAction::Redo),
            MenuCommand::EditRedo,
            &["history", "repeat"],
        );
        push(
            "Edit",
            "Rotate left",
            None,
            MenuCommand::EditRotateLeft,
            &["rotate"],
        );
        push(
            "Edit",
            "Rotate right",
            None,
            MenuCommand::EditRotateRight,
            &["rotate"],
        );
        push(
            "Edit",
            "Flip horizontal",
            None,
            MenuCommand::EditFlipHorizontal,
            &["mirror"],
        );
        push(
            "Edit",
            "Flip vertical",
            None,
            MenuCommand::EditFlipVertical,
            &["mirror"],
        );
        push(
            "Edit",
            "Resize / resample...",
            None,
            MenuCommand::EditResizeResample,
            &["scale", "dimensions"],
        );
        push(
            "Edit",
            "Crop...",
            None,
            MenuCommand::EditCrop,
            &["crop", "selection"],
        );
        push(
            "Edit",
            "Color corrections...",
            None,
            MenuCommand::EditColorCorrections,
            &["color", "brightness", "contrast"],
        );
        push(
            "Edit",
            "Add border / frame...",
            None,
            MenuCommand::EditBorderFrame,
            &["border", "frame", "canvas"],
        );
        push(
            "Edit",
            "Canvas size...",
            None,
            MenuCommand::EditCanvasSize,
            &["canvas", "resize"],
        );
        push(
            "Edit",
            "Fine rotation...",
            None,
            MenuCommand::EditFineRotation,
            &["rotation", "angle"],
        );
        push(
            "Edit",
            "Text tool...",
            None,
            MenuCommand::EditTextTool,
            &["text", "annotate"],
        );
        push(
            "Edit",
            "Shape tool...",
            None,
            MenuCommand::EditShapeTool,
            &["shape", "annotate"],
        );
        push(
            "Edit",
            "Overlay / watermark...",
            None,
            MenuCommand::EditOverlayWatermark,
            &["overlay", "watermark"],
        );
        push(
            "Edit",
            "Selection workflows...",
            None,
            MenuCommand::EditSelectionWorkflows,
            &["selection", "mask", "workflow"],
        );
        push(
            "Edit",
            "Replace color...",
            None,
            MenuCommand::EditReplaceColor,
            &["replace", "color"],
        );
        push(
            "Edit",
            "Alpha tools...",
            None,
            MenuCommand::EditAlphaTools,
            &["alpha", "transparency"],
        );
        push(
            "Edit",
            "Effects...",
            None,
            MenuCommand::EditEffects,
            &["filters", "effects"],
        );
        push(
            "Edit",
            "Perspective correction...",
            None,
            MenuCommand::EditPerspectiveCorrection,
            &["perspective", "keystone"],
        );

        push(
            "Image",
            "Previous image",
            Some(ShortcutAction::PreviousImage),
            MenuCommand::ImagePrevious,
            &["prev", "back"],
        );
        push(
            "Image",
            "Next image",
            Some(ShortcutAction::NextImage),
            MenuCommand::ImageNext,
            &["next", "forward"],
        );
        push(
            "Image",
            "Zoom in",
            Some(ShortcutAction::ZoomIn),
            MenuCommand::ImageZoomIn,
            &["scale", "magnify"],
        );
        push(
            "Image",
            "Zoom out",
            Some(ShortcutAction::ZoomOut),
            MenuCommand::ImageZoomOut,
            &["scale", "shrink"],
        );
        push(
            "Image",
            "Fit to window",
            Some(ShortcutAction::Fit),
            MenuCommand::ImageFitToWindow,
            &["fit", "autosize"],
        );
        push(
            "Image",
            "Actual size",
            Some(ShortcutAction::ActualSize),
            MenuCommand::ImageActualSize,
            &["100%", "native"],
        );
        let slideshow_label = if self.slideshow_running {
            "Stop slideshow"
        } else {
            "Start slideshow"
        };
        push(
            "Image",
            slideshow_label,
            None,
            MenuCommand::ImageToggleSlideshow,
            &["slideshow", "space", "playback"],
        );
        push(
            "Image",
            "Load compare image...",
            None,
            MenuCommand::ImageLoadCompare,
            &["compare", "diff"],
        );
        push(
            "Image",
            "Toggle compare mode",
            None,
            MenuCommand::ImageToggleCompareMode,
            &["compare", "toggle"],
        );
        push(
            "Image",
            "Panorama stitch...",
            None,
            MenuCommand::ImagePanoramaStitch,
            &["panorama", "stitch"],
        );

        let status_bar_label = if self.state.show_status_bar() {
            "Show status bar (on)"
        } else {
            "Show status bar (off)"
        };
        let toolbar_label = if self.state.show_toolbar() {
            "Show toolbar (on)"
        } else {
            "Show toolbar (off)"
        };
        let metadata_label = if self.state.show_metadata_panel() {
            "Metadata panel (on)"
        } else {
            "Metadata panel (off)"
        };
        let strip_label = if self.state.show_thumbnail_strip() {
            "Thumbnail strip (on)"
        } else {
            "Thumbnail strip (off)"
        };
        let thumbnail_window_label = if self.state.thumbnails_window_mode() {
            "Thumbnail window (on)"
        } else {
            "Thumbnail window (off)"
        };
        push(
            "View",
            "Command palette...",
            Some(ShortcutAction::CommandPalette),
            MenuCommand::ViewCommandPalette,
            &["commands", "actions", "search"],
        );
        push(
            "View",
            status_bar_label,
            None,
            MenuCommand::ViewToggleStatusBar,
            &["toggle", "status", "bar"],
        );
        push(
            "View",
            toolbar_label,
            None,
            MenuCommand::ViewToggleToolbar,
            &["toggle", "toolbar"],
        );
        push(
            "View",
            metadata_label,
            None,
            MenuCommand::ViewToggleMetadataPanel,
            &["toggle", "metadata"],
        );
        push(
            "View",
            strip_label,
            None,
            MenuCommand::ViewToggleThumbnailStrip,
            &["toggle", "thumbnails", "strip"],
        );
        push(
            "View",
            thumbnail_window_label,
            None,
            MenuCommand::ViewToggleThumbnailWindow,
            &["toggle", "thumbnails", "window"],
        );
        push(
            "View",
            "Zoom magnifier...",
            None,
            MenuCommand::ViewZoomMagnifier,
            &["magnifier", "loupe", "zoom"],
        );

        push(
            "Options",
            "Performance / cache...",
            None,
            MenuCommand::OptionsPerformanceCache,
            &["performance", "cache", "memory"],
        );
        push(
            "Options",
            "Clear runtime caches",
            None,
            MenuCommand::OptionsClearRuntimeCaches,
            &["clear", "cache", "memory"],
        );
        push(
            "Options",
            "Purge folder catalog cache",
            None,
            MenuCommand::OptionsPurgeFolderCatalogCache,
            &[
                "purge", "catalog", "folder", "cache", "disk", "sqlite", "database",
            ],
        );
        push(
            "Options",
            "Advanced settings...",
            None,
            MenuCommand::OptionsAdvancedSettings,
            &["advanced", "settings", "preferences"],
        );

        push(
            "Help",
            "About ImranView",
            None,
            MenuCommand::HelpAbout,
            &["about", "version", "credits"],
        );

        drop(push);

        for path in self.state.recent_files().iter().take(RECENT_MENU_LIMIT) {
            let label = format!("Recent file: {}", format_recent_file_label(path));
            let mut search_blob = format!("file recent {}", label.to_ascii_lowercase());
            search_blob.push(' ');
            search_blob.push_str(&path.display().to_string().to_ascii_lowercase());
            let command = MenuCommand::FileOpenRecent(path.clone());
            let enabled = self.is_menu_command_enabled(&command);
            entries.push(CommandPaletteEntry {
                group: "File",
                title: label,
                shortcut: None,
                search_blob,
                enabled,
                command,
            });
        }
        for path in self
            .state
            .recent_directories()
            .iter()
            .take(RECENT_MENU_LIMIT)
        {
            let label = format!("Recent folder: {}", format_recent_folder_label(path));
            let mut search_blob = format!("file recent folder {}", label.to_ascii_lowercase());
            search_blob.push(' ');
            search_blob.push_str(&path.display().to_string().to_ascii_lowercase());
            let command = MenuCommand::FileOpenRecentFolder(path.clone());
            let enabled = self.is_menu_command_enabled(&command);
            entries.push(CommandPaletteEntry {
                group: "File",
                title: label,
                shortcut: None,
                search_blob,
                enabled,
                command,
            });
        }

        entries
    }

    pub(super) fn draw_command_palette(&mut self, ctx: &egui::Context) {
        if !self.command_palette.open {
            return;
        }

        if ctx.input_mut(|i| i.consume_key(egui::Modifiers::NONE, egui::Key::Escape)) {
            self.command_palette.open = false;
            self.command_palette.query.clear();
            self.command_palette.selected_index = 0;
            return;
        }

        let entries = self.collect_command_palette_entries(ctx);
        let query = self.command_palette.query.trim().to_ascii_lowercase();
        let mut visible = Vec::with_capacity(entries.len());
        if query.is_empty() {
            for index in 0..entries.len() {
                visible.push((index, 0i32));
            }
        } else {
            for (index, entry) in entries.iter().enumerate() {
                if let Some(score) = Self::fuzzy_match_score(&query, &entry.search_blob) {
                    visible.push((index, score));
                }
            }
        }
        visible.sort_by(|left, right| {
            let left_entry = &entries[left.0];
            let right_entry = &entries[right.0];
            Self::command_palette_group_order(left_entry.group)
                .cmp(&Self::command_palette_group_order(right_entry.group))
                .then_with(|| right.1.cmp(&left.1))
                .then_with(|| left.0.cmp(&right.0))
        });
        if visible.len() > COMMAND_PALETTE_MAX_VISIBLE {
            visible.truncate(COMMAND_PALETTE_MAX_VISIBLE);
        }
        if visible.is_empty() {
            self.command_palette.selected_index = 0;
        } else if self.command_palette.selected_index >= visible.len() {
            self.command_palette.selected_index = visible.len().saturating_sub(1);
        }

        if !visible.is_empty()
            && ctx.input_mut(|i| i.consume_key(egui::Modifiers::NONE, egui::Key::ArrowDown))
        {
            self.command_palette.selected_index =
                (self.command_palette.selected_index + 1).min(visible.len().saturating_sub(1));
        }
        if !visible.is_empty()
            && ctx.input_mut(|i| i.consume_key(egui::Modifiers::NONE, egui::Key::ArrowUp))
        {
            self.command_palette.selected_index =
                self.command_palette.selected_index.saturating_sub(1);
        }
        if !visible.is_empty()
            && ctx.input_mut(|i| i.consume_key(egui::Modifiers::NONE, egui::Key::PageDown))
        {
            self.command_palette.selected_index =
                (self.command_palette.selected_index + 10).min(visible.len().saturating_sub(1));
        }
        if !visible.is_empty()
            && ctx.input_mut(|i| i.consume_key(egui::Modifiers::NONE, egui::Key::PageUp))
        {
            self.command_palette.selected_index =
                self.command_palette.selected_index.saturating_sub(10);
        }

        let mut execute_command = if !visible.is_empty()
            && ctx.input_mut(|i| i.consume_key(egui::Modifiers::NONE, egui::Key::Enter))
        {
            let entry_index = visible[self.command_palette.selected_index].0;
            let entry = &entries[entry_index];
            if entry.enabled {
                Some(entry.command.clone())
            } else {
                None
            }
        } else {
            None
        };

        let viewport_rect = ctx
            .input(|i| i.viewport().inner_rect)
            .unwrap_or_else(|| ctx.input(|i| i.screen_rect()));
        let painter = ctx.layer_painter(egui::LayerId::new(
            egui::Order::Foreground,
            egui::Id::new("command-palette-backdrop"),
        ));
        painter.rect_filled(viewport_rect, 0.0, egui::Color32::from_black_alpha(72));

        let panel_width = COMMAND_PALETTE_PANEL_WIDTH
            .min((viewport_rect.width() - 28.0).max(420.0))
            .max(420.0);
        let list_height = COMMAND_PALETTE_PANEL_MAX_HEIGHT
            .min((viewport_rect.height() * 0.68).max(220.0))
            .max(220.0);
        egui::Area::new(egui::Id::new("command-palette-panel"))
            .order(egui::Order::Foreground)
            .anchor(egui::Align2::CENTER_CENTER, egui::vec2(0.0, 0.0))
            .show(ctx, |ui| {
                let frame = egui::Frame::new()
                    .fill(ui.visuals().window_fill)
                    .stroke(ui.visuals().window_stroke)
                    .corner_radius(egui::CornerRadius::same(platform_window_corner_radius()))
                    .shadow(ui.visuals().window_shadow)
                    .inner_margin(egui::Margin::symmetric(10, 8));
                frame.show(ui, |ui| {
                    ui.set_min_width(panel_width);
                    ui.set_max_width(panel_width);
                    Self::native_bar_frame(ctx)
                        .inner_margin(egui::Margin::symmetric(10, 6))
                        .show(ui, |ui| {
                            ui.horizontal(|ui| {
                                ui.label(egui::RichText::new("Command Palette").small().strong());
                                ui.with_layout(
                                    egui::Layout::right_to_left(egui::Align::Center),
                                    |ui| {
                                        if let Some(shortcut) =
                                            shortcut_text(ctx, ShortcutAction::CommandPalette)
                                        {
                                            ui.label(egui::RichText::new(shortcut).small().weak());
                                        }
                                    },
                                );
                            });
                        });
                    ui.add_space(6.0);
                    let search_frame = egui::Frame::new()
                        .fill(ui.visuals().extreme_bg_color)
                        .stroke(ui.visuals().widgets.noninteractive.bg_stroke)
                        .corner_radius(egui::CornerRadius::same(platform_widget_corner_radius()))
                        .inner_margin(egui::Margin::symmetric(8, 6));
                    search_frame.show(ui, |ui| {
                        let query_response = ui.add(
                            egui::TextEdit::singleline(&mut self.command_palette.query)
                                .hint_text("Type a command...")
                                .desired_width(f32::INFINITY)
                                .frame(false),
                        );
                        if self.command_palette.request_focus {
                            query_response.request_focus();
                            self.command_palette.request_focus = false;
                        }
                        if query_response.changed() {
                            self.command_palette.selected_index = 0;
                        }
                    });
                    ui.separator();

                    egui::ScrollArea::vertical()
                        .auto_shrink([false; 2])
                        .max_height(list_height)
                        .show(ui, |ui| {
                            if visible.is_empty() {
                                ui.label("No matching commands");
                                return;
                            }

                            let mut last_group: Option<&str> = None;
                            for (visible_index, (entry_index, _score)) in visible.iter().enumerate()
                            {
                                let entry = &entries[*entry_index];
                                if last_group != Some(entry.group) {
                                    if last_group.is_some() {
                                        ui.add_space(6.0);
                                    }
                                    ui.label(
                                        egui::RichText::new(entry.group)
                                            .small()
                                            .strong()
                                            .color(ui.visuals().weak_text_color()),
                                    );
                                    last_group = Some(entry.group);
                                }

                                let selected = visible_index == self.command_palette.selected_index;
                                let mut title = entry.title.clone();
                                if let Some(shortcut) = &entry.shortcut {
                                    title.push_str("    ");
                                    title.push_str(shortcut);
                                }
                                if !entry.enabled {
                                    title.push_str(" (Unavailable)");
                                }
                                let mut button = egui::Button::new(title)
                                    .min_size(egui::vec2(ui.available_width(), 24.0))
                                    .frame(false);
                                if selected {
                                    button =
                                        button.fill(Self::native_selected_surface(ui.visuals()));
                                }
                                let response = ui.add_enabled(entry.enabled, button);

                                if response.hovered() {
                                    self.command_palette.selected_index = visible_index;
                                }
                                if response.clicked() && entry.enabled {
                                    execute_command = Some(entry.command.clone());
                                }
                                if selected {
                                    ui.scroll_to_rect(response.rect, Some(egui::Align::Center));
                                }
                            }
                        });
                });
            });

        if let Some(command) = execute_command {
            self.run_menu_command(ctx, command.clone());
            if self.command_palette.open && !matches!(command, MenuCommand::ViewCommandPalette) {
                self.command_palette.open = false;
                self.command_palette.query.clear();
                self.command_palette.selected_index = 0;
                self.command_palette.request_focus = false;
            }
        }
    }

    pub(super) fn run_shortcuts(&mut self, ctx: &egui::Context) {
        if shortcuts::trigger(ctx, ShortcutAction::CommandPalette) {
            self.toggle_command_palette();
        }
        if self.command_palette.open {
            return;
        }

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
}
