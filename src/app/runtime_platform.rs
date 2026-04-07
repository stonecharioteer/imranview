use super::*;

impl ImranViewApp {
    pub(super) fn maybe_install_native_menu(&mut self, frame: &eframe::Frame) {
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
    pub(super) fn maybe_install_native_menu(&mut self, _frame: &eframe::Frame) {}

    pub(super) fn should_draw_in_window_menu(&self) -> bool {
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
    pub(super) fn handle_native_menu_events(&mut self, ctx: &egui::Context) {
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
                    self.open_batch_script_picker_and_dispatch()
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
                NativeMenuAction::CommandPalette => self.open_command_palette(),
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
    pub(super) fn handle_native_menu_events(&mut self, _ctx: &egui::Context) {}

    pub(super) fn native_selected_surface(visuals: &egui::Visuals) -> egui::Color32 {
        let accent = visuals.selection.bg_fill;
        let alpha = if visuals.dark_mode { 112 } else { 70 };
        egui::Color32::from_rgba_unmultiplied(accent.r(), accent.g(), accent.b(), alpha)
    }

    pub(super) fn native_bar_frame(ctx: &egui::Context) -> egui::Frame {
        egui::Frame::new()
            .fill(ctx.style().visuals.panel_fill)
            .inner_margin(egui::Margin::symmetric(8, 2))
    }

    pub(super) fn dialog_viewport_builder(
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

    pub(super) fn show_popup_window(
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
}
