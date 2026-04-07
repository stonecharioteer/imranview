use super::*;

impl ImranViewApp {
    pub(super) fn poll_worker_results(&mut self, ctx: &egui::Context) {
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

    pub(super) fn poll_picker_results(&mut self) {
        loop {
            match self.picker_result_rx.try_recv() {
                Ok(result) => self.handle_picker_result(result),
                Err(mpsc::TryRecvError::Empty) => break,
                Err(mpsc::TryRecvError::Disconnected) => {
                    self.pending.picker_inflight = false;
                    log::error!(target: "imranview::ui", "picker result channel disconnected");
                    self.state
                        .set_error("picker result channel disconnected unexpectedly");
                    break;
                }
            }
        }
    }

    pub(super) fn handle_picker_result(&mut self, result: PickerResult) {
        self.pending.picker_inflight = false;

        let PickerResult {
            kind,
            picked_path,
            blocked,
        } = result;

        crate::perf::log_timing(kind.perf_label(), blocked, crate::perf::OPEN_PICKER_BUDGET);

        match (kind, picked_path) {
            (PickerRequestKind::OpenImage, Some(path)) => {
                log::debug!(
                    target: "imranview::ui",
                    "open picker selected path={} blocked={}ms",
                    path.display(),
                    blocked.as_millis()
                );
                self.dispatch_open(path, false);
            }
            (PickerRequestKind::CompareImage, Some(path)) => {
                log::debug!(
                    target: "imranview::ui",
                    "compare picker selected path={} blocked={}ms",
                    path.display(),
                    blocked.as_millis()
                );
                self.dispatch_compare_open(path);
            }
            (PickerRequestKind::OpenImage, None) => {
                log::debug!(
                    target: "imranview::ui",
                    "open picker cancelled blocked={}ms",
                    blocked.as_millis()
                );
            }
            (PickerRequestKind::CompareImage, None) => {
                log::debug!(
                    target: "imranview::ui",
                    "compare picker cancelled blocked={}ms",
                    blocked.as_millis()
                );
            }
        }
    }

    pub(super) fn handle_worker_result(&mut self, ctx: &egui::Context, result: WorkerResult) {
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
                let mut files = files;
                self.apply_browsing_sort(&mut files);
                self.state
                    .apply_open_payload(path, directory, files, loaded);
                self.current_metadata = Some(metadata);
                self.clear_folder_panel_cache();
                self.update_main_texture_from_state(ctx);
                self.schedule_preview_refine_after_idle();
                if self.preview_refine_due_at.is_some() {
                    ctx.request_repaint_after(Duration::from_millis(PREVIEW_REFINE_IDLE_DELAY_MS));
                }
                self.scroll_thumbnail_to_current = true;
                if self.pending.queued_navigation_steps == 0 {
                    self.schedule_preload_neighbors();
                }
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
                    (WorkerRequestKind::Thumbnail, _) => {
                        // Keep this low-noise for folders with unreadable files.
                    }
                    _ => {}
                }
            }
        }
    }

    pub(super) fn update_main_texture_from_state(&mut self, ctx: &egui::Context) {
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

    pub(super) fn texture_from_rgba(
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

    pub(super) fn apply_view_color_pipeline(&self, rgba: &[u8]) -> Vec<u8> {
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

    pub(super) fn open_batch_script_picker_and_dispatch(&mut self) {
        let mut dialog = rfd::FileDialog::new().set_title("Run batch automation script");
        if let Some(directory) = self.state.preferred_open_directory() {
            dialog = dialog.set_directory(directory);
        }
        dialog = dialog.add_filter("JSON", &["json"]);
        if let Some(path) = dialog.pick_file() {
            self.dispatch_batch_script(path);
        }
    }

    pub(super) fn open_command_palette(&mut self) {
        self.command_palette.open = true;
        self.command_palette.query.clear();
        self.command_palette.selected_index = 0;
        self.command_palette.request_focus = true;
    }

    pub(super) fn toggle_command_palette(&mut self) {
        if self.command_palette.open {
            self.command_palette.open = false;
            self.command_palette.query.clear();
            self.command_palette.selected_index = 0;
            self.command_palette.request_focus = false;
        } else {
            self.open_command_palette();
        }
    }

    pub(super) fn is_menu_command_enabled(&self, command: &MenuCommand) -> bool {
        match command {
            MenuCommand::FileOpen
            | MenuCommand::FileBatchConvertRename
            | MenuCommand::FileRunAutomationScript
            | MenuCommand::FileBatchScanImport
            | MenuCommand::FileScreenshotCapture
            | MenuCommand::FileExit
            | MenuCommand::ViewCommandPalette
            | MenuCommand::ViewToggleStatusBar
            | MenuCommand::ViewToggleToolbar
            | MenuCommand::ViewToggleMetadataPanel
            | MenuCommand::ViewToggleThumbnailStrip
            | MenuCommand::ViewToggleThumbnailWindow
            | MenuCommand::OptionsPerformanceCache
            | MenuCommand::OptionsClearRuntimeCaches
            | MenuCommand::OptionsPurgeFolderCatalogCache
            | MenuCommand::OptionsAdvancedSettings
            | MenuCommand::HelpAbout => true,
            MenuCommand::FileOpenRecent(path) => path.is_file(),
            MenuCommand::FileOpenRecentFolder(path) => path.is_dir(),
            MenuCommand::FileSearchFiles => !self.state.images_in_directory().is_empty(),
            MenuCommand::FileSave
            | MenuCommand::FileSaveAs
            | MenuCommand::FileSaveWithOptions
            | MenuCommand::FileLosslessJpegTransform
            | MenuCommand::FileChangeExifDateTime
            | MenuCommand::FileConvertColorProfile
            | MenuCommand::FileRenameCurrent
            | MenuCommand::FileCopyCurrentToFolder
            | MenuCommand::FileMoveCurrentToFolder
            | MenuCommand::FileDeleteCurrent
            | MenuCommand::FileOcr
            | MenuCommand::FilePrintCurrent
            | MenuCommand::FileMultipageTiff
            | MenuCommand::FileCreateMultipagePdf
            | MenuCommand::FileExportContactSheet
            | MenuCommand::FileExportHtmlGallery
            | MenuCommand::EditRotateLeft
            | MenuCommand::EditRotateRight
            | MenuCommand::EditFlipHorizontal
            | MenuCommand::EditFlipVertical
            | MenuCommand::EditResizeResample
            | MenuCommand::EditCrop
            | MenuCommand::EditColorCorrections
            | MenuCommand::EditBorderFrame
            | MenuCommand::EditCanvasSize
            | MenuCommand::EditFineRotation
            | MenuCommand::EditTextTool
            | MenuCommand::EditShapeTool
            | MenuCommand::EditOverlayWatermark
            | MenuCommand::EditSelectionWorkflows
            | MenuCommand::EditReplaceColor
            | MenuCommand::EditAlphaTools
            | MenuCommand::EditEffects
            | MenuCommand::EditPerspectiveCorrection
            | MenuCommand::ImagePrevious
            | MenuCommand::ImageNext
            | MenuCommand::ImageZoomIn
            | MenuCommand::ImageZoomOut
            | MenuCommand::ImageFitToWindow
            | MenuCommand::ImageActualSize
            | MenuCommand::ImageLoadCompare
            | MenuCommand::ImagePanoramaStitch
            | MenuCommand::ViewZoomMagnifier => self.state.has_image(),
            MenuCommand::ImageToggleCompareMode => self.compare_image.is_some(),
            MenuCommand::ImageToggleSlideshow => self.slideshow_running || self.state.has_image(),
            MenuCommand::EditUndo => self.state.can_undo(),
            MenuCommand::EditRedo => self.state.can_redo(),
        }
    }

    pub(super) fn undo_edit(&mut self, ctx: &egui::Context) {
        if self.pending.edit_inflight {
            self.state
                .set_error("wait for current edit to finish before undo");
            return;
        }
        if let Err(err) = self.state.undo_edit() {
            self.state.set_error(err.to_string());
            return;
        }
        self.update_main_texture_from_state(ctx);
    }

    pub(super) fn redo_edit(&mut self, ctx: &egui::Context) {
        if self.pending.edit_inflight {
            self.state
                .set_error("wait for current edit to finish before redo");
            return;
        }
        if let Err(err) = self.state.redo_edit() {
            self.state.set_error(err.to_string());
            return;
        }
        self.update_main_texture_from_state(ctx);
    }

    pub(super) fn open_next(&mut self) {
        self.queue_navigation_step(1);
        self.slideshow_last_tick = Instant::now();
    }

    pub(super) fn open_previous(&mut self) {
        self.queue_navigation_step(-1);
        self.slideshow_last_tick = Instant::now();
    }

    pub(super) fn launch_picker_async(&mut self, kind: PickerRequestKind) {
        if self.pending.picker_inflight {
            log::debug!(
                target: "imranview::ui",
                "picker request ignored because another picker is in-flight"
            );
            return;
        }

        let preferred_directory_started = Instant::now();
        let preferred_directory = self.state.preferred_open_directory();
        let preferred_directory_elapsed = preferred_directory_started.elapsed();
        crate::picker::log_prepare(
            kind,
            preferred_directory.as_ref(),
            preferred_directory_elapsed,
        );

        self.pending.picker_inflight = true;
        crate::picker::launch_picker_async(
            kind,
            preferred_directory,
            self.picker_result_tx.clone(),
        );
    }

    pub(super) fn open_path_dialog(&mut self) {
        self.launch_picker_async(PickerRequestKind::OpenImage);
    }

    pub(super) fn open_compare_path_dialog(&mut self) {
        self.launch_picker_async(PickerRequestKind::CompareImage);
    }

    pub(super) fn open_save_as_dialog(&mut self) {
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

    pub(super) fn build_save_options_from_dialog(&self) -> SaveImageOptions {
        SaveImageOptions {
            output_format: self.save_dialog.output_format,
            jpeg_quality: self.save_dialog.jpeg_quality,
            metadata_policy: self.save_dialog.metadata_policy,
        }
    }

    pub(super) fn open_resize_dialog(&mut self) {
        if let Some((width, height)) = self.state.original_dimensions() {
            self.resize_dialog.width = width;
            self.resize_dialog.height = height;
        }
        self.resize_dialog.open = true;
    }

    pub(super) fn open_crop_dialog(&mut self) {
        if let Some((width, height)) = self.state.original_dimensions() {
            self.crop_dialog.x = 0;
            self.crop_dialog.y = 0;
            self.crop_dialog.width = width;
            self.crop_dialog.height = height;
        }
        self.crop_dialog.open = true;
    }

    pub(super) fn open_color_dialog(&mut self) {
        self.color_dialog = ColorDialogState::default();
        self.color_dialog.open = true;
    }

    pub(super) fn open_text_tool_dialog(&mut self) {
        self.text_tool_dialog = TextToolDialogState::default();
        self.text_tool_dialog.open = true;
    }

    pub(super) fn open_shape_tool_dialog(&mut self) {
        self.shape_tool_dialog = ShapeToolDialogState::default();
        self.shape_tool_dialog.open = true;
    }

    pub(super) fn open_overlay_dialog(&mut self) {
        self.overlay_dialog = OverlayDialogState::default();
        self.overlay_dialog.open = true;
    }

    pub(super) fn open_selection_workflow_dialog(&mut self) {
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

    pub(super) fn parse_polygon_points(input: &str) -> Option<Vec<[u32; 2]>> {
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
        if points.len() >= 3 {
            Some(points)
        } else {
            None
        }
    }

    pub(super) fn open_replace_color_dialog(&mut self) {
        self.replace_color_dialog = ReplaceColorDialogState::default();
        self.replace_color_dialog.open = true;
    }

    pub(super) fn open_alpha_dialog(&mut self) {
        let mut state = AlphaDialogState::default();
        if let Some((w, h)) = self.state.original_dimensions() {
            state.region_width = w;
            state.region_height = h;
            state.brush_center_x = w / 2;
            state.brush_center_y = h / 2;
            state.brush_radius = (w.min(h) / 6).max(1);
        }
        state.open = true;
        self.alpha_dialog = state;
    }

    pub(super) fn open_effects_dialog(&mut self) {
        self.effects_dialog = EffectsDialogState::default();
        self.effects_dialog.open = true;
    }

    pub(super) fn apply_effects_preset(&mut self, preset: EffectsPreset) {
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

    pub(super) fn effects_params_from_dialog(&self) -> EffectsParams {
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

    pub(super) fn apply_effects_params_to_dialog(&mut self, params: EffectsParams) {
        self.effects_dialog.preset = EffectsPreset::Custom;
        self.effects_dialog.blur_sigma = params.blur_sigma;
        self.effects_dialog.sharpen_sigma = params.sharpen_sigma;
        self.effects_dialog.sharpen_threshold = params.sharpen_threshold;
        self.effects_dialog.invert = params.invert;
        self.effects_dialog.grayscale = params.grayscale;
        self.effects_dialog.sepia_strength = params.sepia_strength;
        self.effects_dialog.posterize_levels = params.posterize_levels;
        self.effects_dialog.vignette_strength = params.vignette_strength;
        self.effects_dialog.tilt_shift_strength = params.tilt_shift_strength;
        self.effects_dialog.stained_glass_strength = params.stained_glass_strength;
        self.effects_dialog.emboss_strength = params.emboss_strength;
        self.effects_dialog.edge_enhance_strength = params.edge_enhance_strength;
        self.effects_dialog.oil_paint_strength = params.oil_paint_strength;
    }

    pub(super) fn save_effects_preset(&mut self, path: PathBuf) {
        let params = self.effects_params_from_dialog();
        if let Some(parent) = path.parent() {
            if !parent.as_os_str().is_empty() && fs::create_dir_all(parent).is_err() {
                self.state.set_error(format!(
                    "failed to create preset directory {}",
                    parent.display()
                ));
                return;
            }
        }
        match serde_json::to_string_pretty(&params) {
            Ok(json) => match fs::write(&path, json) {
                Ok(_) => {
                    self.info_message = Some(format!("Saved effects preset {}", path.display()));
                }
                Err(err) => self
                    .state
                    .set_error(format!("failed to write preset {}: {err}", path.display())),
            },
            Err(err) => self
                .state
                .set_error(format!("failed to serialize effects preset: {err}")),
        }
    }

    pub(super) fn load_effects_preset(&mut self, path: PathBuf) {
        match fs::read_to_string(&path) {
            Ok(json) => match serde_json::from_str::<EffectsParams>(&json) {
                Ok(params) => {
                    self.apply_effects_params_to_dialog(params);
                    self.info_message = Some(format!("Loaded effects preset {}", path.display()));
                }
                Err(err) => self
                    .state
                    .set_error(format!("invalid effects preset {}: {err}", path.display())),
            },
            Err(err) => self
                .state
                .set_error(format!("failed to read preset {}: {err}", path.display())),
        }
    }

    pub(super) fn open_border_dialog(&mut self) {
        self.border_dialog = BorderDialogState::default();
        self.border_dialog.open = true;
    }

    pub(super) fn open_canvas_dialog(&mut self) {
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

    pub(super) fn open_fine_rotate_dialog(&mut self) {
        self.fine_rotate_dialog = FineRotateDialogState::default();
        self.fine_rotate_dialog.open = true;
    }

    pub(super) fn open_batch_dialog(&mut self) {
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

    pub(super) fn open_rename_dialog(&mut self) {
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

    pub(super) fn open_search_dialog(&mut self) {
        self.search_dialog.results.clear();
        self.search_dialog.open = true;
        self.run_search_files();
    }

    pub(super) fn open_screenshot_dialog(&mut self) {
        self.screenshot_dialog = ScreenshotDialogState::default();
        self.screenshot_dialog.open = true;
    }

    pub(super) fn open_tiff_dialog(&mut self) {
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

    pub(super) fn open_pdf_dialog(&mut self) {
        self.pdf_dialog = PdfDialogState::default();
        if let Some(directory) = self.state.current_directory_path() {
            self.pdf_dialog.output_path = directory.join("images.pdf").display().to_string();
        }
        self.pdf_dialog.open = true;
    }

    pub(super) fn open_batch_scan_dialog(&mut self) {
        self.batch_scan_dialog = BatchScanDialogState::default();
        if let Some(directory) = self.state.current_directory_path() {
            self.batch_scan_dialog.input_dir = directory.display().to_string();
            self.batch_scan_dialog.output_dir = directory.join("scan-output").display().to_string();
        }
        self.batch_scan_dialog.open = true;
    }

    pub(super) fn open_ocr_dialog(&mut self) {
        self.ocr_dialog.open = true;
        if self.ocr_dialog.output_path.is_empty() {
            if let Some(path) = self.state.current_file_path() {
                self.ocr_dialog.output_path = path.with_extension("txt").display().to_string();
            }
        }
    }

    pub(super) fn open_lossless_jpeg_dialog(&mut self) {
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

    pub(super) fn open_exif_date_dialog(&mut self) {
        self.exif_date_dialog = ExifDateDialogState::default();
        self.exif_date_dialog.open = true;
    }

    pub(super) fn open_color_profile_dialog(&mut self) {
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

    pub(super) fn open_panorama_dialog(&mut self) {
        self.panorama_dialog = PanoramaDialogState::default();
        if let Some(directory) = self.state.current_directory_path() {
            self.panorama_dialog.output_path = directory.join("panorama.jpg").display().to_string();
        }
        self.panorama_dialog.open = true;
    }

    pub(super) fn open_perspective_dialog(&mut self) {
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

    pub(super) fn open_magnifier_dialog(&mut self) {
        self.magnifier_dialog.open = true;
    }

    pub(super) fn open_contact_sheet_dialog(&mut self) {
        self.contact_sheet_dialog = ContactSheetDialogState::default();
        if let Some(directory) = self.state.current_directory_path() {
            self.contact_sheet_dialog.output_path =
                directory.join("contact-sheet.jpg").display().to_string();
        }
        self.contact_sheet_dialog.open = true;
    }

    pub(super) fn open_html_export_dialog(&mut self) {
        self.html_export_dialog = HtmlExportDialogState::default();
        if let Some(directory) = self.state.current_directory_path() {
            self.html_export_dialog.output_dir = directory.join("gallery").display().to_string();
        }
        self.html_export_dialog.open = true;
    }

    pub(super) fn open_advanced_options_dialog(&mut self) {
        self.advanced_options_dialog.open = true;
    }

    pub(super) fn run_search_files(&mut self) {
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

    pub(super) fn copy_current_to_dialog(&mut self) {
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

    pub(super) fn move_current_to_dialog(&mut self) {
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

    pub(super) fn delete_current_file(&mut self) {
        let Some(path) = self.state.current_file_path() else {
            self.state.set_error("no image loaded");
            return;
        };
        self.dispatch_file_operation(FileOperation::Delete { path });
    }

    pub(super) fn open_about_window(&mut self) {
        self.show_about_window = true;
    }

    pub(super) fn open_performance_dialog(&mut self) {
        self.performance_dialog.thumb_cache_entry_cap = self.state.thumb_cache_entry_cap();
        self.performance_dialog.thumb_cache_max_mb = self.state.thumb_cache_max_mb();
        self.performance_dialog.preload_cache_entry_cap = self.state.preload_cache_entry_cap();
        self.performance_dialog.preload_cache_max_mb = self.state.preload_cache_max_mb();
        self.refresh_catalog_cache_stats();
        self.performance_dialog.open = true;
    }

    pub(super) fn clear_runtime_caches(&mut self) {
        self.thumb_cache = ThumbTextureCache::new(
            self.state.thumb_cache_entry_cap(),
            self.state.thumb_cache_max_mb().saturating_mul(1024 * 1024),
        );
        self.inflight_thumbnails.clear();
        self.inflight_preloads.clear();
        self.info_message = Some("Cleared thumbnail and preload caches".to_owned());
    }

    pub(super) fn refresh_catalog_cache_stats(&mut self) {
        match crate::catalog::cache_stats() {
            Ok(stats) => {
                self.performance_dialog.catalog_cache_size_bytes = stats.database_bytes;
                self.performance_dialog.catalog_tracked_folders = stats.tracked_folders;
                self.performance_dialog.catalog_persisted_folders = stats.persisted_folders;
                self.performance_dialog.catalog_entries = stats.persisted_entries;
                self.performance_dialog.catalog_last_error = None;
            }
            Err(err) => {
                self.performance_dialog.catalog_last_error = Some(err.to_string());
            }
        }
    }

    pub(super) fn purge_folder_catalog_cache(&mut self) {
        match crate::catalog::purge_cache() {
            Ok(bytes_freed) => {
                self.refresh_catalog_cache_stats();
                self.info_message = Some(format!(
                    "Purged folder catalog cache ({})",
                    human_file_size(bytes_freed)
                ));
            }
            Err(err) => {
                self.state
                    .set_error(format!("failed to purge folder catalog cache: {err}"));
            }
        }
    }

    pub(super) fn apply_performance_settings(&mut self) {
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

    pub(super) fn clear_folder_panel_cache(&mut self) {
        self.folder_panel_cache = FolderPanelCache::default();
    }

    pub(super) fn ensure_folder_panel_cache(&mut self) {
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

    pub(super) fn open_directory_from_panel(&mut self, directory: PathBuf) {
        if self.state.current_directory_path() == Some(directory.clone()) {
            return;
        }
        self.dispatch_open_directory(directory);
    }

    pub(super) fn sync_viewport_state(&mut self, ctx: &egui::Context) {
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

    pub(super) fn capture_viewport_snapshot(ctx: &egui::Context) -> ViewportSnapshot {
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

    pub(super) fn format_worker_error(kind: WorkerRequestKind, error: &str) -> String {
        match kind {
            WorkerRequestKind::Open => format!("Unable to open image: {error}"),
            WorkerRequestKind::Save => format!("Unable to save image: {error}"),
            WorkerRequestKind::Edit => format!("Unable to apply edit: {error}"),
            WorkerRequestKind::Thumbnail => format!("Thumbnail decode failed: {error}"),
            WorkerRequestKind::Batch => format!("Batch convert failed: {error}"),
            WorkerRequestKind::File => format!("File operation failed: {error}"),
            WorkerRequestKind::Print => format!("Print failed: {error}"),
            WorkerRequestKind::Compare => format!("Compare load failed: {error}"),
            WorkerRequestKind::Utility => format!("Utility workflow failed: {error}"),
            WorkerRequestKind::Ocr => format!("OCR failed: {error}"),
        }
    }

    pub(super) fn start_slideshow(&mut self) {
        if !self.state.has_image() {
            self.state
                .set_error("open an image before starting slideshow");
            return;
        }
        self.slideshow_running = true;
        self.slideshow_last_tick = Instant::now();
    }

    pub(super) fn stop_slideshow(&mut self) {
        self.slideshow_running = false;
    }

    pub(super) fn run_slideshow_tick(&mut self) {
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

    pub(super) fn schedule_preview_refine_after_idle(&mut self) {
        if !self.state.downscaled_for_preview() {
            self.preview_refine_due_at = None;
            return;
        }
        self.preview_refine_due_at =
            Some(Instant::now() + Duration::from_millis(PREVIEW_REFINE_IDLE_DELAY_MS));
        self.preview_refined_for_path = None;
    }

    pub(super) fn maybe_refine_preview_after_idle(&mut self, ctx: &egui::Context) {
        let Some(due_at) = self.preview_refine_due_at else {
            return;
        };
        let now = Instant::now();
        if now < due_at {
            let wait = due_at.saturating_duration_since(now);
            ctx.request_repaint_after(wait.min(Duration::from_millis(32)));
            return;
        }
        if self.pending.open_inflight
            || self.pending.queued_navigation_steps != 0
            || self.slideshow_running
        {
            self.schedule_preview_refine_after_idle();
            return;
        }
        let Some(path) = self.state.current_file_path() else {
            self.preview_refine_due_at = None;
            return;
        };
        if self.preview_refined_for_path.as_ref() == Some(&path) {
            self.preview_refine_due_at = None;
            return;
        }
        if !self.state.downscaled_for_preview() {
            self.preview_refine_due_at = None;
            return;
        }
        match self
            .state
            .refresh_preview_from_working(PREVIEW_REFINE_DIMENSION)
        {
            Ok(()) => {
                self.preview_refined_for_path = Some(path);
                self.preview_refine_due_at = None;
                self.update_main_texture_from_state(ctx);
                log::debug!(target: "imranview::perf", "refined preview after idle pause");
            }
            Err(err) => {
                log::debug!(
                    target: "imranview::perf",
                    "skipped preview refine: {err:#}"
                );
                self.preview_refine_due_at = None;
            }
        }
    }

}
