mod app_state;
mod image_io;
mod perf;
mod settings;
mod worker;

use std::collections::{HashMap, HashSet, VecDeque};
use std::path::PathBuf;
use std::sync::mpsc::{self, Receiver, Sender};
use std::time::Instant;

use anyhow::{Result, anyhow};
use eframe::egui;

use crate::app_state::{AppState, ThumbnailEntry};
use crate::settings::{load_settings, save_settings};
use crate::worker::{TransformOp, WorkerCommand, WorkerRequestKind, WorkerResult};

const THUMB_TEXTURE_CACHE_CAP: usize = 320;
const THUMB_CARD_WIDTH: f32 = 120.0;
const THUMB_CARD_HEIGHT: f32 = 100.0;
const TOOLBAR_ICON_SIZE: f32 = 18.0;

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
    let image = image::load_from_memory(bytes)
        .map_err(|err| anyhow!("failed to decode toolbar icon {name}: {err}"))?;
    let rgba = image.to_rgba8();
    let width = rgba.width() as usize;
    let height = rgba.height() as usize;
    let pixels = rgba.into_raw();
    let color = egui::ColorImage::from_rgba_unmultiplied([width, height], &pixels);
    Ok(ctx.load_texture(
        format!("toolbar-{name}"),
        color,
        egui::TextureOptions::LINEAR,
    ))
}

#[derive(Default)]
struct PendingRequests {
    latest_open: u64,
    latest_save: u64,
    latest_edit: u64,
    open_inflight: bool,
    save_inflight: bool,
    edit_inflight: bool,
    queued_navigation_steps: i32,
}

impl PendingRequests {
    fn has_inflight(&self) -> bool {
        self.open_inflight || self.save_inflight || self.edit_inflight
    }
}

struct ThumbTextureCache {
    map: HashMap<PathBuf, egui::TextureHandle>,
    order: VecDeque<PathBuf>,
    capacity: usize,
}

impl ThumbTextureCache {
    fn new(capacity: usize) -> Self {
        Self {
            map: HashMap::new(),
            order: VecDeque::new(),
            capacity,
        }
    }

    fn get(&mut self, path: &PathBuf) -> Option<&egui::TextureHandle> {
        if self.map.contains_key(path) {
            self.touch(path);
        }
        self.map.get(path)
    }

    fn insert(&mut self, path: PathBuf, texture: egui::TextureHandle) {
        if self.map.contains_key(&path) {
            self.map.insert(path.clone(), texture);
            self.touch(&path);
            return;
        }

        self.map.insert(path.clone(), texture);
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
        while self.map.len() > self.capacity {
            if let Some(oldest) = self.order.pop_front() {
                self.map.remove(&oldest);
            } else {
                break;
            }
        }
    }
}

struct ImranViewApp {
    state: AppState,
    worker_tx: Sender<WorkerCommand>,
    thumbnail_tx: Sender<PathBuf>,
    worker_rx: Receiver<WorkerResult>,
    request_sequence: u64,
    pending: PendingRequests,
    main_texture: Option<egui::TextureHandle>,
    main_texture_generation: u64,
    thumb_cache: ThumbTextureCache,
    inflight_thumbnails: HashSet<PathBuf>,
    inflight_preloads: HashSet<PathBuf>,
    toolbar_icons: Option<ToolbarIcons>,
    last_logged_thumb_entry_count: Option<usize>,
    scroll_thumbnail_to_current: bool,
}

impl ImranViewApp {
    fn new(cc: &eframe::CreationContext<'_>, cli_path: Option<PathBuf>) -> Self {
        let state = AppState::new_with_settings(load_settings());
        let (worker_tx, worker_thread_rx) = mpsc::channel::<WorkerCommand>();
        let (thumbnail_tx, thumbnail_rx) = mpsc::channel::<PathBuf>();
        let (worker_thread_tx, worker_rx) = mpsc::channel::<WorkerResult>();
        worker::spawn_workers(worker_thread_rx, thumbnail_rx, worker_thread_tx);

        let mut app = Self {
            state,
            worker_tx,
            thumbnail_tx,
            worker_rx,
            request_sequence: 1,
            pending: PendingRequests::default(),
            main_texture: None,
            main_texture_generation: 1,
            thumb_cache: ThumbTextureCache::new(THUMB_TEXTURE_CACHE_CAP),
            inflight_thumbnails: HashSet::new(),
            inflight_preloads: HashSet::new(),
            toolbar_icons: ToolbarIcons::try_load(&cc.egui_ctx),
            last_logged_thumb_entry_count: None,
            scroll_thumbnail_to_current: false,
        };

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

    fn persist_settings(&self) {
        if let Err(err) = save_settings(&self.state.to_settings()) {
            log::warn!(
                target: "imranview::settings",
                "failed to save settings: {err:#}"
            );
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

    fn dispatch_save(&mut self, path: Option<PathBuf>, reopen_after_save: bool) {
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
            "queue save request_id={} path={} reopen_after_save={}",
            request_id,
            path.display(),
            reopen_after_save
        );

        if self
            .worker_tx
            .send(WorkerCommand::SaveImage {
                request_id,
                path,
                image,
                reopen_after_save,
            })
            .is_err()
        {
            self.pending.save_inflight = false;
            log::error!(target: "imranview::ui", "failed to queue save-image command");
            self.state.set_error("failed to queue save-image command");
        }
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
        let path_result = if forward {
            self.state.resolve_next_path()
        } else {
            self.state.resolve_previous_path()
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

        if let Ok(next) = self.state.resolve_next_path() {
            candidates.push(next);
        }
        if let Ok(previous) = self.state.resolve_previous_path() {
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
            }
            WorkerResult::ThumbnailDecoded { path, payload } => {
                self.inflight_thumbnails.remove(&path);
                let texture = Self::texture_from_rgba(
                    ctx,
                    format!("thumb-{}", path.display()),
                    &payload.rgba,
                    payload.width,
                    payload.height,
                );
                self.thumb_cache.insert(path, texture);
                log::debug!(
                    target: "imranview::thumb",
                    "thumbnail decoded {}x{} cache_size={} inflight={}",
                    payload.width,
                    payload.height,
                    self.thumb_cache.map.len(),
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
                match (kind, request_id) {
                    (WorkerRequestKind::Open, Some(id)) if id == self.pending.latest_open => {
                        self.pending.open_inflight = false;
                        self.pending.queued_navigation_steps = 0;
                        self.state.set_error(error);
                    }
                    (WorkerRequestKind::Save, Some(id)) if id == self.pending.latest_save => {
                        self.pending.save_inflight = false;
                        self.state.set_error(error);
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
            ctx,
            format!("main-image-{}", self.main_texture_generation),
            &rgba,
            width,
            height,
        );
        self.main_texture_generation = self.main_texture_generation.saturating_add(1);
        self.main_texture = Some(texture);
    }

    fn texture_from_rgba(
        ctx: &egui::Context,
        name: String,
        rgba: &[u8],
        width: u32,
        height: u32,
    ) -> egui::TextureHandle {
        let color =
            egui::ColorImage::from_rgba_unmultiplied([width as usize, height as usize], rgba);
        ctx.load_texture(name, color, egui::TextureOptions::LINEAR)
    }

    fn run_shortcuts(&mut self, ctx: &egui::Context) {
        let ctrl_s = ctx.input(|i| i.modifiers.ctrl && i.key_pressed(egui::Key::S));
        if ctrl_s {
            let shift = ctx.input(|i| i.modifiers.shift);
            if shift {
                self.open_save_as_dialog();
            } else {
                self.dispatch_save(None, false);
            }
        }

        if ctx.input(|i| i.key_pressed(egui::Key::ArrowRight)) {
            self.open_next();
        }
        if ctx.input(|i| i.key_pressed(egui::Key::ArrowLeft)) {
            self.open_previous();
        }
        if ctx.input(|i| i.key_pressed(egui::Key::Plus) || i.key_pressed(egui::Key::Equals)) {
            self.state.zoom_in();
        }
        if ctx.input(|i| i.key_pressed(egui::Key::Minus)) {
            self.state.zoom_out();
        }
        if ctx.input(|i| i.key_pressed(egui::Key::Num0)) {
            self.state.set_zoom_fit();
        }
        if ctx.input(|i| i.key_pressed(egui::Key::Num1)) {
            self.state.set_zoom_actual();
        }

        let wheel_zoom = ctx.input(|i| {
            if i.modifiers.ctrl {
                i.raw_scroll_delta.y
            } else {
                0.0
            }
        });
        if wheel_zoom != 0.0 {
            self.state.zoom_from_wheel_delta(wheel_zoom);
        }
    }

    fn open_next(&mut self) {
        self.queue_navigation_step(1);
    }

    fn open_previous(&mut self) {
        self.queue_navigation_step(-1);
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

    fn open_save_as_dialog(&mut self) {
        let preferred_directory = self.state.preferred_open_directory();
        let suggested_name = self.state.suggested_save_name();

        let mut dialog = rfd::FileDialog::new()
            .set_title("Save image as")
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
        if let Some(file_name) = suggested_name {
            dialog = dialog.set_file_name(file_name);
        }

        if let Some(path) = dialog.save_file() {
            self.dispatch_save(Some(path), true);
        }
    }

    fn draw_menu(&mut self, ctx: &egui::Context) {
        egui::TopBottomPanel::top("menu").show(ctx, |ui| {
            egui::menu::bar(ui, |ui| {
                ui.menu_button("File", |ui| {
                    if ui.button("Open...").clicked() {
                        self.open_path_dialog();
                        ui.close_menu();
                    }
                    if ui
                        .add_enabled(self.state.has_image(), egui::Button::new("Save"))
                        .clicked()
                    {
                        self.dispatch_save(None, false);
                        ui.close_menu();
                    }
                    if ui
                        .add_enabled(self.state.has_image(), egui::Button::new("Save As..."))
                        .clicked()
                    {
                        self.open_save_as_dialog();
                        ui.close_menu();
                    }
                    ui.separator();
                    if ui.button("Exit").clicked() {
                        ctx.send_viewport_cmd(egui::ViewportCommand::Close);
                    }
                });

                ui.menu_button("Edit", |ui| {
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
                        .add_enabled(self.state.has_image(), egui::Button::new("Flip Horizontal"))
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
        let mut button = egui::Button::image(image);
        if selected {
            button = button.fill(egui::Color32::from_rgb(216, 232, 251));
        }
        ui.add_enabled(enabled, button).on_hover_text(tooltip)
    }

    fn draw_toolbar(&mut self, ctx: &egui::Context) {
        if !self.state.show_toolbar() {
            return;
        }

        egui::TopBottomPanel::top("toolbar")
            .exact_height(34.0)
            .show(ctx, |ui| {
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
                            self.state.zoom_out();
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
                            self.state.zoom_in();
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
                            self.state.set_zoom_actual();
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
                            self.state.set_zoom_fit();
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
                            self.state.zoom_out();
                        }
                        if ui
                            .add_enabled(self.state.has_image(), egui::Button::new("+"))
                            .clicked()
                        {
                            self.state.zoom_in();
                        }
                        if ui
                            .add_enabled(self.state.has_image(), egui::Button::new("1:1"))
                            .clicked()
                        {
                            self.state.set_zoom_actual();
                        }
                        if ui
                            .add_enabled(self.state.has_image(), egui::Button::new("Fit"))
                            .clicked()
                        {
                            self.state.set_zoom_fit();
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
                "thumbnail strip entries={} cache_size={} inflight={}",
                entries.len(),
                self.thumb_cache.map.len(),
                self.inflight_thumbnails.len()
            );
            self.last_logged_thumb_entry_count = Some(entries.len());
        }

        egui::TopBottomPanel::bottom("thumbnail-strip")
            .resizable(true)
            .min_height(112.0)
            .default_height(146.0)
            .show(ctx, |ui| {
                egui::ScrollArea::horizontal().show(ui, |ui| {
                    ui.horizontal(|ui| {
                        for entry in entries {
                            self.draw_thumbnail_card(ui, &entry, false);
                        }
                    });
                });
            });
    }

    fn draw_thumbnail_window(&mut self, ctx: &egui::Context) {
        egui::CentralPanel::default().show(ctx, |ui| {
            ui.horizontal_wrapped(|ui| {
                ui.heading("Thumbnails");
                ui.separator();
                ui.label(self.state.folder_label());
            });
            ui.separator();

            let entries = self.state.thumbnail_entries();
            egui::ScrollArea::vertical().show(ui, |ui| {
                for entry in entries {
                    self.draw_thumbnail_card(ui, &entry, true);
                    ui.separator();
                }
            });
        });
    }

    fn draw_thumbnail_card(&mut self, ui: &mut egui::Ui, entry: &ThumbnailEntry, row_mode: bool) {
        let mut frame = egui::Frame::group(ui.style());
        if entry.current {
            frame = frame.fill(egui::Color32::from_rgb(216, 232, 251));
        }

        let response = frame.show(ui, |ui| {
            if row_mode {
                ui.horizontal(|ui| {
                    self.draw_thumbnail_image(ui, entry);
                    ui.vertical(|ui| {
                        ui.label(&entry.label);
                        if entry.current {
                            ui.label("Current image");
                        }
                    });
                });
            } else {
                ui.set_width(THUMB_CARD_WIDTH);
                self.draw_thumbnail_image(ui, entry);
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

    fn draw_thumbnail_image(&mut self, ui: &mut egui::Ui, entry: &ThumbnailEntry) {
        if let Some(texture) = self.thumb_cache.get(&entry.path) {
            let image_size = egui::vec2(THUMB_CARD_WIDTH - 8.0, THUMB_CARD_HEIGHT - 8.0);
            ui.add(egui::Image::new((texture.id(), image_size)));
        } else {
            ui.allocate_ui(
                egui::vec2(THUMB_CARD_WIDTH - 8.0, THUMB_CARD_HEIGHT - 8.0),
                |ui| {
                    ui.centered_and_justified(|ui| {
                        ui.label("...");
                    });
                },
            );
        }
    }

    fn draw_main_viewer(&mut self, ctx: &egui::Context) {
        egui::CentralPanel::default().show(ctx, |ui| {
            if let Some(texture) = &self.main_texture {
                let mut desired_size =
                    egui::vec2(self.state.image_width(), self.state.image_height());

                if self.state.zoom_is_fit() {
                    let available = ui.available_size();
                    let fit_scale = if desired_size.x > 0.0 && desired_size.y > 0.0 {
                        (available.x / desired_size.x)
                            .min(available.y / desired_size.y)
                            .max(0.01)
                    } else {
                        1.0
                    };
                    desired_size *= fit_scale;
                    ui.centered_and_justified(|ui| {
                        ui.add(egui::Image::new((texture.id(), desired_size)));
                    });
                } else {
                    desired_size *= self.state.zoom_factor();
                    egui::ScrollArea::both().show(ui, |ui| {
                        ui.add(egui::Image::new((texture.id(), desired_size)));
                    });
                }
            } else {
                ui.centered_and_justified(|ui| {
                    ui.label("ImranView\n\nFile > Open...");
                });
            }
        });
    }

    fn draw_status_bar(&mut self, ctx: &egui::Context) {
        if !self.state.show_status_bar() {
            return;
        }

        egui::TopBottomPanel::bottom("status")
            .exact_height(24.0)
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
                });
            });
    }
}

impl eframe::App for ImranViewApp {
    fn update(&mut self, ctx: &egui::Context, _frame: &mut eframe::Frame) {
        self.poll_worker_results(ctx);
        self.run_shortcuts(ctx);

        self.draw_menu(ctx);
        self.draw_toolbar(ctx);
        self.draw_thumbnail_strip(ctx);

        if self.state.thumbnails_window_mode() {
            self.draw_thumbnail_window(ctx);
        } else {
            self.draw_main_viewer(ctx);
        }

        self.draw_status_bar(ctx);

        ctx.send_viewport_cmd(egui::ViewportCommand::Title(self.state.window_title()));

        if self.pending.has_inflight() || !self.inflight_thumbnails.is_empty() {
            ctx.request_repaint_after(std::time::Duration::from_millis(16));
        }
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
    log::info!(target: "imranview::startup", "launching ImranView");

    let native_options = eframe::NativeOptions::default();
    eframe::run_native(
        "ImranView",
        native_options,
        Box::new(move |cc| {
            let startup_started = Instant::now();
            let app = ImranViewApp::new(cc, cli_path.clone());
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
