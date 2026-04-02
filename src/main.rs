mod app_state;
mod image_io;

use std::cell::RefCell;
use std::path::PathBuf;
use std::rc::Rc;

use anyhow::Result;
use slint::{ModelRc, VecModel};

use crate::app_state::{AppState, ThumbnailView};

slint::include_modules!();

fn main() -> Result<()> {
    let app = AppWindow::new()?;
    let state = Rc::new(RefCell::new(AppState::new()));
    let weak = app.as_weak();

    wire_callbacks(&app, Rc::clone(&state), weak.clone());
    maybe_open_cli_path(
        Rc::clone(&state),
        weak,
        std::env::args_os().nth(1).map(PathBuf::from),
    );

    refresh_ui(&app, &state);
    app.run()?;
    Ok(())
}

fn wire_callbacks(app: &AppWindow, state: Rc<RefCell<AppState>>, weak: slint::Weak<AppWindow>) {
    let state_for_open = Rc::clone(&state);
    let weak_for_open = weak.clone();
    app.on_request_open(move || {
        if let Some(path) = pick_image_path() {
            open_path_and_refresh(&state_for_open, &weak_for_open, path);
        }
    });

    let state_for_next = Rc::clone(&state);
    let weak_for_next = weak.clone();
    app.on_request_next(move || {
        mutate_state_and_refresh(&state_for_next, &weak_for_next, |state| state.open_next());
    });

    let state_for_prev = Rc::clone(&state);
    let weak_for_prev = weak.clone();
    app.on_request_prev(move || {
        mutate_state_and_refresh(&state_for_prev, &weak_for_prev, |state| {
            state.open_previous()
        });
    });

    let state_for_index = Rc::clone(&state);
    let weak_for_index = weak.clone();
    app.on_request_open_index(move |index| {
        mutate_state_and_refresh(&state_for_index, &weak_for_index, |state| {
            state.open_at_index(index)
        });
    });

    let state_for_zoom_in = Rc::clone(&state);
    let weak_for_zoom_in = weak.clone();
    app.on_request_zoom_in(move || {
        mutate_state_and_refresh(&state_for_zoom_in, &weak_for_zoom_in, |state| {
            state.zoom_in();
            Ok(())
        });
    });

    let state_for_zoom_out = Rc::clone(&state);
    let weak_for_zoom_out = weak.clone();
    app.on_request_zoom_out(move || {
        mutate_state_and_refresh(&state_for_zoom_out, &weak_for_zoom_out, |state| {
            state.zoom_out();
            Ok(())
        });
    });

    let state_for_zoom_fit = Rc::clone(&state);
    let weak_for_zoom_fit = weak.clone();
    app.on_request_zoom_fit(move || {
        mutate_state_and_refresh(&state_for_zoom_fit, &weak_for_zoom_fit, |state| {
            state.set_zoom_fit();
            Ok(())
        });
    });

    let state_for_zoom_actual = Rc::clone(&state);
    let weak_for_zoom_actual = weak.clone();
    app.on_request_zoom_actual(move || {
        mutate_state_and_refresh(&state_for_zoom_actual, &weak_for_zoom_actual, |state| {
            state.set_zoom_actual();
            Ok(())
        });
    });

    app.on_request_wheel_zoom({
        let state = Rc::clone(&state);
        let weak = weak.clone();
        move |delta_y| {
            mutate_state_and_refresh(&state, &weak, |state| {
                state.zoom_from_wheel_delta(delta_y);
                Ok(())
            });
        }
    });

    app.on_request_toggle_toolbar({
        let state = Rc::clone(&state);
        let weak = weak.clone();
        move || {
            mutate_state_and_refresh(&state, &weak, |state| {
                state.toggle_toolbar();
                Ok(())
            });
        }
    });

    app.on_request_toggle_status_bar({
        let state = Rc::clone(&state);
        let weak = weak.clone();
        move || {
            mutate_state_and_refresh(&state, &weak, |state| {
                state.toggle_status_bar();
                Ok(())
            });
        }
    });

    app.on_request_toggle_thumbnails({
        let state = Rc::clone(&state);
        let weak = weak.clone();
        move || {
            mutate_state_and_refresh(&state, &weak, |state| {
                state.toggle_thumbnail_strip();
                Ok(())
            });
        }
    });

    app.on_request_not_implemented({
        let state = Rc::clone(&state);
        let weak = weak.clone();
        move |action| {
            let action = action.to_string();
            mutate_state_and_refresh(&state, &weak, move |state| {
                state.set_error(format!("{action} is not implemented yet"));
                Ok(())
            });
        }
    });

    app.on_request_exit(move || {
        if let Some(app) = weak.upgrade() {
            let _ = app.hide();
        }
    });
}

fn maybe_open_cli_path(
    state: Rc<RefCell<AppState>>,
    weak: slint::Weak<AppWindow>,
    cli_path: Option<PathBuf>,
) {
    let Some(path) = cli_path else {
        return;
    };
    open_path_and_refresh(&state, &weak, path);
}

fn open_path_and_refresh(
    state: &Rc<RefCell<AppState>>,
    weak: &slint::Weak<AppWindow>,
    path: PathBuf,
) {
    mutate_state_and_refresh(state, weak, move |state| state.open_image(path));
}

fn pick_image_path() -> Option<PathBuf> {
    rfd::FileDialog::new()
        .set_title("Open image")
        .add_filter(
            "Images",
            &[
                "avif", "bmp", "gif", "heic", "heif", "hdr", "ico", "jpeg", "jpg", "pbm", "pgm",
                "png", "pnm", "ppm", "qoi", "tif", "tiff", "webp",
            ],
        )
        .pick_file()
}

fn refresh_ui_from_weak(weak: &slint::Weak<AppWindow>, state: &Rc<RefCell<AppState>>) {
    if let Some(app) = weak.upgrade() {
        refresh_ui(&app, state);
    }
}

fn refresh_ui(app: &AppWindow, state: &Rc<RefCell<AppState>>) {
    let snapshot = {
        let mut state = state.borrow_mut();
        let thumbnails = state
            .thumbnail_entries()
            .into_iter()
            .map(ThumbnailItem::from)
            .collect::<Vec<_>>();

        UiSnapshot {
            window_title: state.window_title(),
            has_image: state.has_image(),
            current_image: state.current_image().unwrap_or_default(),
            image_width: state.image_width(),
            image_height: state.image_height(),
            zoom_fit: state.zoom_is_fit(),
            zoom_factor: state.zoom_factor(),
            zoom_label: state.zoom_label(),
            image_counter_label: state.image_counter_label(),
            show_toolbar: state.show_toolbar(),
            show_status_bar: state.show_status_bar(),
            show_thumbnail_strip: state.show_thumbnail_strip(),
            status_dimensions: state.status_dimensions(),
            status_index: state.status_index(),
            status_zoom: state.status_zoom(),
            status_size: state.status_size(),
            status_preview: state.status_preview(),
            status_name: state.status_name(),
            thumbnail_model: thumbnails,
        }
    };

    app.set_window_title(snapshot.window_title.into());
    app.set_has_image(snapshot.has_image);
    app.set_current_image(snapshot.current_image);
    app.set_image_width(snapshot.image_width);
    app.set_image_height(snapshot.image_height);
    app.set_zoom_fit(snapshot.zoom_fit);
    app.set_zoom_factor(snapshot.zoom_factor);
    app.set_zoom_label(snapshot.zoom_label.into());
    app.set_image_counter_label(snapshot.image_counter_label.into());
    app.set_show_toolbar(snapshot.show_toolbar);
    app.set_show_status_bar(snapshot.show_status_bar);
    app.set_show_thumbnail_strip(snapshot.show_thumbnail_strip);
    app.set_status_dimensions(snapshot.status_dimensions.into());
    app.set_status_index(snapshot.status_index.into());
    app.set_status_zoom(snapshot.status_zoom.into());
    app.set_status_size(snapshot.status_size.into());
    app.set_status_preview(snapshot.status_preview.into());
    app.set_status_name(snapshot.status_name.into());
    app.set_thumbnail_model(ModelRc::new(VecModel::from(snapshot.thumbnail_model)));
}

fn mutate_state_and_refresh<F>(
    state: &Rc<RefCell<AppState>>,
    weak: &slint::Weak<AppWindow>,
    mutator: F,
) where
    F: FnOnce(&mut AppState) -> Result<()>,
{
    {
        let mut state = state.borrow_mut();
        if let Err(err) = mutator(&mut state) {
            state.set_error(err.to_string());
        }
    }
    refresh_ui_from_weak(weak, state);
}

struct UiSnapshot {
    window_title: String,
    has_image: bool,
    current_image: slint::Image,
    image_width: f32,
    image_height: f32,
    zoom_fit: bool,
    zoom_factor: f32,
    zoom_label: String,
    image_counter_label: String,
    show_toolbar: bool,
    show_status_bar: bool,
    show_thumbnail_strip: bool,
    status_dimensions: String,
    status_index: String,
    status_zoom: String,
    status_size: String,
    status_preview: String,
    status_name: String,
    thumbnail_model: Vec<ThumbnailItem>,
}

impl From<ThumbnailView> for ThumbnailItem {
    fn from(value: ThumbnailView) -> Self {
        Self {
            source_index: value.source_index,
            label: value.label.into(),
            preview: value.preview,
            current: value.current,
        }
    }
}
