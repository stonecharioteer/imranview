use super::*;

impl ImranViewApp {
    pub(super) fn draw_delete_confirmation(&mut self, ctx: &egui::Context) {
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

    pub(super) fn draw_metadata_panel(&mut self, ctx: &egui::Context) {
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

    pub(super) fn primary_camera_metadata(&self) -> Option<String> {
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

    pub(super) fn primary_capture_metadata(&self) -> Option<String> {
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

    pub(super) fn draw_status_bar(&mut self, ctx: &egui::Context) {
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
                    if self.pending.picker_inflight {
                        ui.separator();
                        ui.label("Picker: opening…");
                    }
                    if let Some(shortcut) = shortcut_text(ctx, ShortcutAction::CommandPalette) {
                        ui.separator();
                        ui.label(format!("Commands: {shortcut}"));
                    }
                });
            });
    }
}
