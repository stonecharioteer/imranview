use super::*;

impl ImranViewApp {
    pub(super) fn next_request_id(&mut self) -> u64 {
        let next = self.request_sequence;
        self.request_sequence = self.request_sequence.saturating_add(1);
        next
    }

    pub(super) fn advanced_options_from_settings(
        settings: &PersistedSettings,
    ) -> AdvancedOptionsDialogState {
        AdvancedOptionsDialogState {
            open: false,
            active_tab: AdvancedOptionsTab::Viewing,
            checkerboard_background: settings.checkerboard_background,
            smooth_main_scaling: settings.smooth_main_scaling,
            default_jpeg_quality: settings.default_jpeg_quality.clamp(1, 100),
            auto_reopen_after_save: settings.auto_reopen_after_save,
            hide_toolbar_in_fullscreen: settings.hide_toolbar_in_fullscreen,
            browsing_wrap_navigation: settings.browsing_wrap_navigation,
            browsing_sort_mode: FileSortMode::from_settings_value(&settings.browsing_sort_mode),
            browsing_sort_descending: settings.browsing_sort_descending,
            thumbnails_sort_mode: FileSortMode::from_settings_value(&settings.thumbnails_sort_mode),
            thumbnails_sort_descending: settings.thumbnails_sort_descending,
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

    pub(super) fn persist_settings(&self) {
        let mut settings = self.state.to_settings();
        settings.checkerboard_background = self.advanced_options_dialog.checkerboard_background;
        settings.smooth_main_scaling = self.advanced_options_dialog.smooth_main_scaling;
        settings.default_jpeg_quality = self.advanced_options_dialog.default_jpeg_quality;
        settings.auto_reopen_after_save = self.advanced_options_dialog.auto_reopen_after_save;
        settings.hide_toolbar_in_fullscreen =
            self.advanced_options_dialog.hide_toolbar_in_fullscreen;
        settings.browsing_wrap_navigation = self.advanced_options_dialog.browsing_wrap_navigation;
        settings.browsing_sort_mode = self
            .advanced_options_dialog
            .browsing_sort_mode
            .as_settings_value()
            .to_owned();
        settings.browsing_sort_descending = self.advanced_options_dialog.browsing_sort_descending;
        settings.thumbnails_sort_mode = self
            .advanced_options_dialog
            .thumbnails_sort_mode
            .as_settings_value()
            .to_owned();
        settings.thumbnails_sort_descending =
            self.advanced_options_dialog.thumbnails_sort_descending;
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

    pub(super) fn apply_selected_skin(&self, ctx: &egui::Context) {
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

    pub(super) fn dispatch_open(&mut self, path: PathBuf, from_navigation: bool) {
        if !from_navigation {
            self.pending.queued_navigation_steps = 0;
        }
        self.preview_refine_due_at = None;
        self.preview_refined_for_path = None;
        self.inflight_preloads.remove(&path);
        let queued_at = Instant::now();
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
            .send(WorkerCommand::OpenImage {
                request_id,
                path,
                queued_at,
            })
            .is_err()
        {
            self.pending.open_inflight = false;
            log::error!(target: "imranview::ui", "failed to queue open-image command");
            self.state.set_error("failed to queue open-image command");
        }
    }

    pub(super) fn dispatch_open_directory(&mut self, directory: PathBuf) {
        let queued_at = Instant::now();
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
                queued_at,
            })
            .is_err()
        {
            self.pending.open_inflight = false;
            log::error!(target: "imranview::ui", "failed to queue open-directory command");
            self.state
                .set_error("failed to queue open-directory command");
        }
    }

    pub(super) fn dispatch_save(
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

    pub(super) fn default_save_options(&self) -> SaveImageOptions {
        SaveImageOptions {
            output_format: SaveOutputFormat::Auto,
            jpeg_quality: self.advanced_options_dialog.default_jpeg_quality,
            metadata_policy: SaveMetadataPolicy::PreserveIfPossible,
        }
    }

    pub(super) fn plugin_context(&self) -> PluginContext {
        PluginContext {
            has_image: self.state.has_image(),
            current_file: self.state.current_file_path(),
            compare_mode: self.compare_mode,
        }
    }

    pub(super) fn apply_zoom_change<F>(&mut self, zoom_change: F)
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

    pub(super) fn zoom_in(&mut self) {
        let step_percent = self.advanced_options_dialog.zoom_step_percent;
        self.apply_zoom_change(|state| state.zoom_in_by_percent(step_percent));
    }

    pub(super) fn zoom_out(&mut self) {
        let step_percent = self.advanced_options_dialog.zoom_step_percent;
        self.apply_zoom_change(|state| state.zoom_out_by_percent(step_percent));
    }

    pub(super) fn zoom_fit(&mut self) {
        self.apply_zoom_change(|state| state.set_zoom_fit());
    }

    pub(super) fn zoom_actual(&mut self) {
        self.apply_zoom_change(|state| state.set_zoom_actual());
    }

    pub(super) fn dispatch_transform(&mut self, op: TransformOp) {
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

    pub(super) fn dispatch_batch_convert(&mut self, options: BatchConvertOptions) {
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

    pub(super) fn dispatch_batch_script(&mut self, script_path: PathBuf) {
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

    pub(super) fn batch_options_from_dialog(&self) -> BatchConvertOptions {
        BatchConvertOptions {
            input_dir: PathBuf::from(self.batch_dialog.input_dir.trim()),
            output_dir: PathBuf::from(self.batch_dialog.output_dir.trim()),
            output_format: self.batch_dialog.output_format,
            rename_prefix: self.batch_dialog.rename_prefix.clone(),
            start_index: self.batch_dialog.start_index,
            jpeg_quality: self.batch_dialog.jpeg_quality,
        }
    }

    pub(super) fn apply_batch_options_to_dialog(&mut self, options: BatchConvertOptions) {
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

    pub(super) fn save_batch_preset(&mut self, path: PathBuf) {
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

    pub(super) fn load_batch_preset(&mut self, path: PathBuf) {
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

    pub(super) fn dispatch_file_operation(&mut self, operation: FileOperation) {
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

    pub(super) fn dispatch_compare_open(&mut self, path: PathBuf) {
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

    pub(super) fn dispatch_print_current(&mut self) {
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

    pub(super) fn queue_utility_command(&mut self, build: impl FnOnce(u64) -> WorkerCommand) {
        let request_id = self.next_request_id();
        self.pending.latest_utility = request_id;
        self.pending.utility_inflight = true;
        if self.worker_tx.send(build(request_id)).is_err() {
            self.pending.utility_inflight = false;
            self.state.set_error("failed to queue utility command");
        }
    }

    pub(super) fn collect_utility_input_paths(&self, include_folder_images: bool) -> Vec<PathBuf> {
        if include_folder_images {
            let folder_images = self.state.images_in_directory();
            if !folder_images.is_empty() {
                return folder_images.to_vec();
            }
        }
        self.state.current_file_path().into_iter().collect()
    }

    pub(super) fn dispatch_capture_screenshot(
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
    pub(super) fn dispatch_scan_to_directory(
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

    #[allow(clippy::too_many_arguments)]
    pub(super) fn dispatch_scan_native(
        &mut self,
        output_dir: PathBuf,
        output_format: BatchOutputFormat,
        rename_prefix: String,
        start_index: u32,
        page_count: u32,
        jpeg_quality: u8,
        dpi: u32,
        grayscale: bool,
        device_name: Option<String>,
    ) {
        self.queue_utility_command(|request_id| WorkerCommand::ScanNative {
            request_id,
            output_dir,
            output_format,
            rename_prefix,
            start_index,
            page_count,
            jpeg_quality,
            dpi,
            grayscale,
            device_name,
        });
    }

    pub(super) fn collect_file_sort_facts(paths: &[PathBuf]) -> HashMap<PathBuf, FileSortFacts> {
        let mut facts = HashMap::with_capacity(paths.len());
        for path in paths {
            let mut fact = FileSortFacts {
                name: path
                    .file_name()
                    .map(|name| name.to_string_lossy().to_ascii_lowercase())
                    .unwrap_or_default(),
                extension: path
                    .extension()
                    .and_then(|ext| ext.to_str())
                    .map(|ext| ext.to_ascii_lowercase())
                    .unwrap_or_default(),
                ..FileSortFacts::default()
            };
            if let Ok(metadata) = fs::metadata(path) {
                fact.size_bytes = metadata.len();
                fact.modified_epoch_secs = metadata
                    .modified()
                    .ok()
                    .and_then(|time| time.duration_since(UNIX_EPOCH).ok())
                    .map(|duration| duration.as_secs())
                    .unwrap_or(0);
            }
            facts.insert(path.clone(), fact);
        }
        facts
    }

    pub(super) fn sort_paths_in_place(paths: &mut [PathBuf], mode: FileSortMode, descending: bool) {
        let facts = Self::collect_file_sort_facts(paths);
        paths.sort_by(|left, right| {
            let left_fact = facts.get(left).cloned().unwrap_or_default();
            let right_fact = facts.get(right).cloned().unwrap_or_default();
            let ordering = match mode {
                FileSortMode::Name => left_fact.name.cmp(&right_fact.name),
                FileSortMode::Extension => left_fact
                    .extension
                    .cmp(&right_fact.extension)
                    .then_with(|| left_fact.name.cmp(&right_fact.name)),
                FileSortMode::ModifiedTime => left_fact
                    .modified_epoch_secs
                    .cmp(&right_fact.modified_epoch_secs)
                    .then_with(|| left_fact.name.cmp(&right_fact.name)),
                FileSortMode::FileSize => left_fact
                    .size_bytes
                    .cmp(&right_fact.size_bytes)
                    .then_with(|| left_fact.name.cmp(&right_fact.name)),
            };
            if descending {
                ordering.reverse()
            } else {
                ordering
            }
        });
    }

    pub(super) fn apply_browsing_sort(&self, files: &mut [PathBuf]) {
        Self::sort_paths_in_place(
            files,
            self.advanced_options_dialog.browsing_sort_mode,
            self.advanced_options_dialog.browsing_sort_descending,
        );
    }

    pub(super) fn resort_current_directory_listing(&mut self) {
        let mut files = self.state.images_in_directory().to_vec();
        if files.is_empty() {
            return;
        }
        self.apply_browsing_sort(&mut files);
        self.state.reorder_images_in_directory(files);
        self.scroll_thumbnail_to_current = true;
    }

    pub(super) fn sorted_thumbnail_entries(
        &self,
        entries: Vec<ThumbnailEntry>,
    ) -> Vec<ThumbnailEntry> {
        if entries.len() <= 1 {
            return entries;
        }

        let mut entry_by_path = HashMap::with_capacity(entries.len());
        for entry in entries {
            entry_by_path.insert(entry.path.clone(), entry);
        }

        let mut paths: Vec<PathBuf> = entry_by_path.keys().cloned().collect();
        Self::sort_paths_in_place(
            &mut paths,
            self.advanced_options_dialog.thumbnails_sort_mode,
            self.advanced_options_dialog.thumbnails_sort_descending,
        );

        let mut sorted = Vec::with_capacity(paths.len());
        for path in paths {
            if let Some(entry) = entry_by_path.remove(&path) {
                sorted.push(entry);
            }
        }
        sorted
    }

    pub(super) fn dispatch_open_tiff_page(&mut self, path: PathBuf, page_index: u32) {
        self.queue_utility_command(|request_id| WorkerCommand::OpenTiffPage {
            request_id,
            path,
            page_index,
        });
    }

    pub(super) fn dispatch_extract_tiff_pages(
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

    pub(super) fn dispatch_create_multipage_pdf(
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

    pub(super) fn dispatch_ocr(
        &mut self,
        path: PathBuf,
        language: String,
        output_path: Option<PathBuf>,
    ) {
        self.queue_utility_command(|request_id| WorkerCommand::RunOcr {
            request_id,
            path,
            language,
            output_path,
        });
    }

    pub(super) fn dispatch_lossless_jpeg(
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

    pub(super) fn dispatch_update_exif_date(&mut self, path: PathBuf, datetime: String) {
        self.queue_utility_command(|request_id| WorkerCommand::UpdateExifDate {
            request_id,
            path,
            datetime,
        });
    }

    pub(super) fn dispatch_convert_color_profile(
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

    pub(super) fn dispatch_stitch_panorama(
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
    pub(super) fn dispatch_export_contact_sheet(
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

    pub(super) fn dispatch_export_html_gallery(
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

    pub(super) fn request_thumbnail_decode(&mut self, path: PathBuf) {
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

    pub(super) fn queue_navigation_step(&mut self, step: i32) {
        if step == 0 {
            return;
        }
        if !self.state.has_image() {
            self.state.set_error("no image loaded");
            return;
        }

        self.preview_refine_due_at = None;
        self.preview_refined_for_path = None;

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

    pub(super) fn dispatch_queued_navigation_step(&mut self) {
        let queued = self.pending.queued_navigation_steps;
        if queued == 0 {
            return;
        }

        let wrap_navigation = self.advanced_options_dialog.browsing_wrap_navigation;
        let path_result = self
            .state
            .resolve_offset_path_with_wrap(queued as isize, wrap_navigation);

        match path_result {
            Ok(path) => {
                self.pending.queued_navigation_steps = 0;
                self.dispatch_open(path, true);
            }
            Err(err) => {
                self.pending.queued_navigation_steps = 0;
                self.state.set_error(err.to_string());
            }
        }
    }

    pub(super) fn schedule_preload_neighbors(&mut self) {
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
}
