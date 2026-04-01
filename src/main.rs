mod app_state;
mod image_io;

use std::cell::RefCell;
use std::path::PathBuf;
use std::rc::Rc;

use anyhow::Result;

use crate::app_state::AppState;

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
        {
            let mut state = state_for_next.borrow_mut();
            if let Err(err) = state.open_next() {
                state.set_error(err.to_string());
            }
        }
        refresh_ui_from_weak(&weak_for_next, &state_for_next);
    });

    let state_for_prev = Rc::clone(&state);
    app.on_request_prev(move || {
        {
            let mut state = state_for_prev.borrow_mut();
            if let Err(err) = state.open_previous() {
                state.set_error(err.to_string());
            }
        }
        refresh_ui_from_weak(&weak, &state_for_prev);
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
    {
        let mut state = state.borrow_mut();
        if let Err(err) = state.open_image(path) {
            state.set_error(err.to_string());
        }
    }
    refresh_ui_from_weak(weak, state);
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
    let state = state.borrow();
    app.set_window_title(state.window_title().into());
    app.set_status_line(state.status_line().into());
    app.set_has_image(state.has_image());
    app.set_current_image(state.current_image().unwrap_or_default());
}
