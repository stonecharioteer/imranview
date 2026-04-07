use super::*;

impl ImranViewApp {
    pub(super) fn draw_menu(&mut self, ctx: &egui::Context) {
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
                            self.open_batch_script_picker_and_dispatch();
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
                        if ui
                            .button(menu_item_label(
                                ctx,
                                ShortcutAction::CommandPalette,
                                "Command palette...",
                            ))
                            .clicked()
                        {
                            self.open_command_palette();
                            ui.close_menu();
                        }
                        ui.separator();
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
                        if ui.button("Purge folder catalog cache").clicked() {
                            self.purge_folder_catalog_cache();
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

                    ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                        let label = if let Some(shortcut) =
                            shortcut_text(ctx, ShortcutAction::CommandPalette)
                        {
                            format!("Command Palette  {shortcut}")
                        } else {
                            "Command Palette".to_owned()
                        };
                        if ui
                            .button(label)
                            .on_hover_text("Search all menu commands")
                            .clicked()
                        {
                            self.open_command_palette();
                        }
                    });
                });
            });
    }

    pub(super) fn toolbar_icon_button(
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

    pub(super) fn draw_toolbar(&mut self, ctx: &egui::Context) {
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

    pub(super) fn draw_thumbnail_strip(&mut self, ctx: &egui::Context) {
        if !self.state.show_thumbnail_strip() || self.state.thumbnails_window_mode() {
            return;
        }

        let entries = self.sorted_thumbnail_entries(self.state.thumbnail_entries());
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

    pub(super) fn draw_thumbnail_window(&mut self, ctx: &egui::Context) {
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
            let mut browsing_sort_changed = false;
            let mut thumbnail_sort_changed = false;

            egui::menu::bar(ui, |ui| {
                ui.menu_button("File", |ui| {
                    if ui
                        .add_enabled(self.state.has_image(), egui::Button::new("Open current"))
                        .clicked()
                    {
                        if let Some(path) = self.state.current_file_path() {
                            self.dispatch_open(path, false);
                        }
                        ui.close_menu();
                    }
                    if ui.button("Open image...").clicked() {
                        self.open_path_dialog();
                        ui.close_menu();
                    }
                    if ui.button("Refresh folder panel").clicked() {
                        self.clear_folder_panel_cache();
                        ui.close_menu();
                    }
                });
                ui.menu_button("Options", |ui| {
                    ui.label("Browse/navigation order");
                    for mode in [
                        FileSortMode::Name,
                        FileSortMode::Extension,
                        FileSortMode::ModifiedTime,
                        FileSortMode::FileSize,
                    ] {
                        if ui
                            .selectable_label(
                                self.advanced_options_dialog.browsing_sort_mode == mode,
                                mode.as_label(),
                            )
                            .clicked()
                        {
                            self.advanced_options_dialog.browsing_sort_mode = mode;
                            browsing_sort_changed = true;
                        }
                    }
                    if ui
                        .checkbox(
                            &mut self.advanced_options_dialog.browsing_sort_descending,
                            "Descending order",
                        )
                        .changed()
                    {
                        browsing_sort_changed = true;
                    }

                    ui.separator();
                    ui.label("Thumbnail grid order");
                    for mode in [
                        FileSortMode::Name,
                        FileSortMode::Extension,
                        FileSortMode::ModifiedTime,
                        FileSortMode::FileSize,
                    ] {
                        if ui
                            .selectable_label(
                                self.advanced_options_dialog.thumbnails_sort_mode == mode,
                                mode.as_label(),
                            )
                            .clicked()
                        {
                            self.advanced_options_dialog.thumbnails_sort_mode = mode;
                            thumbnail_sort_changed = true;
                        }
                    }
                    if ui
                        .checkbox(
                            &mut self.advanced_options_dialog.thumbnails_sort_descending,
                            "Descending thumbnails",
                        )
                        .changed()
                    {
                        thumbnail_sort_changed = true;
                    }
                });
            });

            if browsing_sort_changed {
                self.resort_current_directory_listing();
            }
            if browsing_sort_changed || thumbnail_sort_changed {
                self.persist_settings();
            }

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

            let entries = self.sorted_thumbnail_entries(self.state.thumbnail_entries());
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

    pub(super) fn draw_thumbnail_card(
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
        if response.response.double_clicked() {
            self.dispatch_open(entry.path.clone(), false);
        }

        if self.thumb_cache.get(&entry.path).is_none()
            && (entry.decode_hint
                || response.response.rect.is_positive()
                    && ui.is_rect_visible(response.response.rect))
        {
            self.request_thumbnail_decode(entry.path.clone());
        }
    }

    pub(super) fn draw_thumbnail_image(
        &mut self,
        ui: &mut egui::Ui,
        entry: &ThumbnailEntry,
        card_width: f32,
    ) {
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

    pub(super) fn show_main_viewer_context_menu(
        &mut self,
        ctx: &egui::Context,
        response: &egui::Response,
    ) {
        response.context_menu(|ui| {
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
                    egui::Button::new(menu_item_label(ctx, ShortcutAction::SaveAs, "Save As...")),
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
                    egui::Button::new(menu_item_label(ctx, ShortcutAction::ZoomIn, "Zoom in")),
                )
                .clicked()
            {
                self.zoom_in();
                ui.close_menu();
            }
            if ui
                .add_enabled(
                    self.state.has_image(),
                    egui::Button::new(menu_item_label(ctx, ShortcutAction::ZoomOut, "Zoom out")),
                )
                .clicked()
            {
                self.zoom_out();
                ui.close_menu();
            }
            if ui
                .add_enabled(
                    self.state.has_image(),
                    egui::Button::new(menu_item_label(ctx, ShortcutAction::Fit, "Fit to window")),
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
            if ui
                .add_enabled(self.state.has_image(), egui::Button::new("Rotate left"))
                .clicked()
            {
                self.dispatch_transform(TransformOp::RotateLeft);
                ui.close_menu();
            }
            if ui
                .add_enabled(self.state.has_image(), egui::Button::new("Rotate right"))
                .clicked()
            {
                self.dispatch_transform(TransformOp::RotateRight);
                ui.close_menu();
            }

            ui.separator();
            if ui
                .button(menu_item_label(
                    ctx,
                    ShortcutAction::CommandPalette,
                    "Command palette...",
                ))
                .clicked()
            {
                self.open_command_palette();
                ui.close_menu();
            }
        });
    }

    pub(super) fn draw_main_viewer(&mut self, ctx: &egui::Context) {
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
            } else if let Some(texture) = self.main_texture.clone() {
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
                    self.show_main_viewer_context_menu(ctx, &response);
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
                    self.show_main_viewer_context_menu(ctx, &output.inner);
                    main_image_rect = output.inner.rect;
                }
                self.draw_magnifier_overlay(ctx, &texture, main_image_rect);
            } else {
                ui.centered_and_justified(|ui| {
                    ui.label("ImranView\n\nFile > Open...");
                });
            }
        });
    }

    pub(super) fn paint_checkerboard_background(&self, ui: &mut egui::Ui) {
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

    pub(super) fn draw_magnifier_overlay(
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

    pub(super) fn draw_about_window(&mut self, ctx: &egui::Context) {
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
                        ui.label("If you are Irfan, then I'm your brother.");
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

    pub(super) fn draw_error_banner(&mut self, ctx: &egui::Context) {
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

    pub(super) fn draw_info_banner(&mut self, ctx: &egui::Context) {
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
}
