use super::*;

impl ImranViewApp {

    pub(super) fn draw_batch_dialog(&mut self, ctx: &egui::Context) {
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

    pub(super) fn draw_save_dialog(&mut self, ctx: &egui::Context) {
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

    pub(super) fn draw_performance_dialog(&mut self, ctx: &egui::Context) {
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
                ui.label("Folder catalog cache (disk)");
                ui.label(format!(
                    "Database size: {}",
                    human_file_size(app.performance_dialog.catalog_cache_size_bytes)
                ));
                ui.label(format!(
                    "Tracked folders: {} | Persisted folders: {} | Indexed entries: {}",
                    app.performance_dialog.catalog_tracked_folders,
                    app.performance_dialog.catalog_persisted_folders,
                    app.performance_dialog.catalog_entries
                ));
                if let Some(error) = app.performance_dialog.catalog_last_error.as_ref() {
                    ui.colored_label(
                        ui.visuals().warn_fg_color,
                        format!("Catalog error: {error}"),
                    );
                }
                ui.horizontal(|ui| {
                    if ui.button("Refresh catalog stats").clicked() {
                        app.refresh_catalog_cache_stats();
                    }
                    if ui.button("Purge folder catalog cache").clicked() {
                        app.purge_folder_catalog_cache();
                    }
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

    pub(super) fn draw_rename_dialog(&mut self, ctx: &egui::Context) {
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

    pub(super) fn draw_search_dialog(&mut self, ctx: &egui::Context) {
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

    pub(super) fn draw_screenshot_dialog(&mut self, ctx: &egui::Context) {
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

    pub(super) fn draw_tiff_dialog(&mut self, ctx: &egui::Context) {
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

    pub(super) fn draw_pdf_dialog(&mut self, ctx: &egui::Context) {
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

    pub(super) fn draw_batch_scan_dialog(&mut self, ctx: &egui::Context) {
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
                        BatchScanSource::NativeBackend,
                        "Native scanner",
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
                } else if app.batch_scan_dialog.source == BatchScanSource::ScannerCommand {
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
                } else {
                    ui.label("Native scanner settings");
                    ui.horizontal(|ui| {
                        ui.label("Pages");
                        ui.add(
                            egui::DragValue::new(&mut app.batch_scan_dialog.page_count)
                                .range(1..=1024),
                        );
                        ui.label("DPI");
                        ui.add(
                            egui::DragValue::new(&mut app.batch_scan_dialog.dpi).range(75..=1200),
                        );
                    });
                    ui.horizontal(|ui| {
                        ui.label("Device (optional)");
                        ui.text_edit_singleline(&mut app.batch_scan_dialog.device_name);
                    });
                    ui.checkbox(&mut app.batch_scan_dialog.grayscale, "Capture grayscale");
                    ui.small(
                        "Uses platform scanner backend (SANE on Linux/macOS, WIA on Windows). Use scanner command mode for advanced custom pipelines.",
                    );
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
                } else if app.batch_scan_dialog.source == BatchScanSource::NativeBackend {
                    "Run native scan"
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
                    } else if app.batch_scan_dialog.source == BatchScanSource::ScannerCommand {
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
                    } else {
                        app.dispatch_scan_native(
                            output_dir,
                            app.batch_scan_dialog.output_format,
                            app.batch_scan_dialog.rename_prefix.clone(),
                            app.batch_scan_dialog.start_index,
                            app.batch_scan_dialog.page_count,
                            app.batch_scan_dialog.jpeg_quality,
                            app.batch_scan_dialog.dpi,
                            app.batch_scan_dialog.grayscale,
                            if app.batch_scan_dialog.device_name.trim().is_empty() {
                                None
                            } else {
                                Some(app.batch_scan_dialog.device_name.trim().to_owned())
                            },
                        );
                        *open_state = false;
                    }
                }
            },
        );
        self.batch_scan_dialog.open = open;
    }

    pub(super) fn draw_ocr_dialog(&mut self, ctx: &egui::Context) {
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

    pub(super) fn draw_lossless_jpeg_dialog(&mut self, ctx: &egui::Context) {
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
                            app.state
                                .set_error("output path is required when not in-place");
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

    pub(super) fn draw_exif_date_dialog(&mut self, ctx: &egui::Context) {
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

    pub(super) fn draw_color_profile_dialog(&mut self, ctx: &egui::Context) {
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
                            let mut dialog =
                                rfd::FileDialog::new().set_title("Color profile output");
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
                ui.small(
                    "Runs in-process using Little CMS. Source profile is optional; defaults to embedded profile (JPEG/PNG) or sRGB.",
                );
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
                            app.state
                                .set_error("output path is required when not in-place");
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

    pub(super) fn draw_panorama_dialog(&mut self, ctx: &egui::Context) {
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

    pub(super) fn draw_perspective_dialog(&mut self, ctx: &egui::Context) {
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

    pub(super) fn draw_magnifier_dialog(&mut self, ctx: &egui::Context) {
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

    pub(super) fn draw_contact_sheet_dialog(&mut self, ctx: &egui::Context) {
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

    pub(super) fn draw_html_export_dialog(&mut self, ctx: &egui::Context) {
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

    pub(super) fn draw_advanced_options_dialog(&mut self, ctx: &egui::Context) {
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
                        ui.separator();
                        ui.label("Browse/navigation order");
                        egui::ComboBox::from_id_salt("advanced-browsing-sort")
                            .selected_text(
                                app.advanced_options_dialog
                                    .browsing_sort_mode
                                    .as_label(),
                            )
                            .show_ui(ui, |ui| {
                                for mode in [
                                    FileSortMode::Name,
                                    FileSortMode::Extension,
                                    FileSortMode::ModifiedTime,
                                    FileSortMode::FileSize,
                                ] {
                                    ui.selectable_value(
                                        &mut app.advanced_options_dialog.browsing_sort_mode,
                                        mode,
                                        mode.as_label(),
                                    );
                                }
                            });
                        ui.checkbox(
                            &mut app.advanced_options_dialog.browsing_sort_descending,
                            "Descending browse/navigation order",
                        );

                        ui.separator();
                        ui.label("Thumbnail order");
                        egui::ComboBox::from_id_salt("advanced-thumbnails-sort")
                            .selected_text(
                                app.advanced_options_dialog
                                    .thumbnails_sort_mode
                                    .as_label(),
                            )
                            .show_ui(ui, |ui| {
                                for mode in [
                                    FileSortMode::Name,
                                    FileSortMode::Extension,
                                    FileSortMode::ModifiedTime,
                                    FileSortMode::FileSize,
                                ] {
                                    ui.selectable_value(
                                        &mut app.advanced_options_dialog.thumbnails_sort_mode,
                                        mode,
                                        mode.as_label(),
                                    );
                                }
                            });
                        ui.checkbox(
                            &mut app.advanced_options_dialog.thumbnails_sort_descending,
                            "Descending thumbnail order",
                        );
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
        let sort_changed = self.advanced_options_dialog.browsing_sort_mode
            != before.browsing_sort_mode
            || self.advanced_options_dialog.browsing_sort_descending
                != before.browsing_sort_descending;
        if self.advanced_options_dialog != before {
            if sort_changed {
                self.resort_current_directory_listing();
            }
            self.apply_selected_skin(ctx);
            self.update_main_texture_from_state(ctx);
            self.persist_settings();
        }
    }
}
