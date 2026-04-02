use anyhow::{Context, Result};
use muda::accelerator::{Accelerator, Code, Modifiers};
use muda::{CheckMenuItem, Menu, MenuEvent, MenuItem, PredefinedMenuItem, Submenu};

use crate::app_state::AppState;

const ID_FILE_OPEN: &str = "menu.file.open";
const ID_FILE_SAVE: &str = "menu.file.save";
const ID_FILE_SAVE_AS: &str = "menu.file.save_as";
const ID_FILE_EXIT: &str = "menu.file.exit";
const ID_EDIT_ROTATE_LEFT: &str = "menu.edit.rotate_left";
const ID_EDIT_ROTATE_RIGHT: &str = "menu.edit.rotate_right";
const ID_EDIT_FLIP_HORIZONTAL: &str = "menu.edit.flip_horizontal";
const ID_EDIT_FLIP_VERTICAL: &str = "menu.edit.flip_vertical";
const ID_VIEW_SHOW_TOOLBAR: &str = "menu.view.show_toolbar";
const ID_VIEW_SHOW_STATUS_BAR: &str = "menu.view.show_status_bar";
const ID_VIEW_SHOW_THUMBNAIL_STRIP: &str = "menu.view.show_thumbnail_strip";
const ID_VIEW_SHOW_THUMBNAIL_WINDOW: &str = "menu.view.show_thumbnail_window";

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum NativeMenuAction {
    Open,
    Save,
    SaveAs,
    Exit,
    RotateLeft,
    RotateRight,
    FlipHorizontal,
    FlipVertical,
    ToggleShowToolbar,
    ToggleShowStatusBar,
    ToggleThumbnailStrip,
    ToggleThumbnailWindow,
}

pub struct NativeMenu {
    _menu: Menu,
    file_save: MenuItem,
    file_save_as: MenuItem,
    edit_rotate_left: MenuItem,
    edit_rotate_right: MenuItem,
    edit_flip_horizontal: MenuItem,
    edit_flip_vertical: MenuItem,
    view_show_toolbar: CheckMenuItem,
    view_show_status_bar: CheckMenuItem,
    view_show_thumbnail_strip: CheckMenuItem,
    view_show_thumbnail_window: CheckMenuItem,
}

impl NativeMenu {
    pub fn install() -> Result<Self> {
        let menu = Menu::new();

        let app_menu = Submenu::new("App", true);
        menu.append(&app_menu)
            .context("failed to append App menu")?;
        app_menu
            .append_items(&[
                &PredefinedMenuItem::about(Some("About ImranView"), None),
                &PredefinedMenuItem::separator(),
                &PredefinedMenuItem::services(None),
                &PredefinedMenuItem::separator(),
                &PredefinedMenuItem::hide(None),
                &PredefinedMenuItem::hide_others(None),
                &PredefinedMenuItem::show_all(None),
                &PredefinedMenuItem::separator(),
                &PredefinedMenuItem::quit(Some("Quit ImranView")),
            ])
            .context("failed to populate App menu")?;

        let file_menu = Submenu::new("File", true);
        menu.append(&file_menu)
            .context("failed to append File menu")?;
        let file_open = MenuItem::with_id(
            ID_FILE_OPEN,
            "Open...",
            true,
            Some(Accelerator::new(Some(Modifiers::SUPER), Code::KeyO)),
        );
        let file_save = MenuItem::with_id(
            ID_FILE_SAVE,
            "Save",
            false,
            Some(Accelerator::new(Some(Modifiers::SUPER), Code::KeyS)),
        );
        let file_save_as = MenuItem::with_id(
            ID_FILE_SAVE_AS,
            "Save As...",
            false,
            Some(Accelerator::new(
                Some(Modifiers::SUPER | Modifiers::SHIFT),
                Code::KeyS,
            )),
        );
        let file_exit = MenuItem::with_id(
            ID_FILE_EXIT,
            "Exit",
            true,
            Some(Accelerator::new(Some(Modifiers::SUPER), Code::KeyQ)),
        );
        file_menu
            .append_items(&[
                &file_open,
                &file_save,
                &file_save_as,
                &PredefinedMenuItem::separator(),
                &file_exit,
            ])
            .context("failed to populate File menu")?;

        let edit_menu = Submenu::new("Edit", true);
        menu.append(&edit_menu)
            .context("failed to append Edit menu")?;
        let edit_rotate_left = MenuItem::with_id(ID_EDIT_ROTATE_LEFT, "Rotate Left", false, None);
        let edit_rotate_right =
            MenuItem::with_id(ID_EDIT_ROTATE_RIGHT, "Rotate Right", false, None);
        let edit_flip_horizontal =
            MenuItem::with_id(ID_EDIT_FLIP_HORIZONTAL, "Flip Horizontal", false, None);
        let edit_flip_vertical =
            MenuItem::with_id(ID_EDIT_FLIP_VERTICAL, "Flip Vertical", false, None);
        edit_menu
            .append_items(&[
                &edit_rotate_left,
                &edit_rotate_right,
                &edit_flip_horizontal,
                &edit_flip_vertical,
            ])
            .context("failed to populate Edit menu")?;

        let view_menu = Submenu::new("View", true);
        menu.append(&view_menu)
            .context("failed to append View menu")?;
        let view_show_toolbar =
            CheckMenuItem::with_id(ID_VIEW_SHOW_TOOLBAR, "Show toolbar", true, true, None);
        let view_show_status_bar =
            CheckMenuItem::with_id(ID_VIEW_SHOW_STATUS_BAR, "Show status bar", true, true, None);
        let view_show_thumbnail_strip = CheckMenuItem::with_id(
            ID_VIEW_SHOW_THUMBNAIL_STRIP,
            "Thumbnail strip",
            true,
            true,
            None,
        );
        let view_show_thumbnail_window = CheckMenuItem::with_id(
            ID_VIEW_SHOW_THUMBNAIL_WINDOW,
            "Thumbnail window",
            true,
            false,
            None,
        );
        view_menu
            .append_items(&[
                &view_show_toolbar,
                &view_show_status_bar,
                &view_show_thumbnail_strip,
                &view_show_thumbnail_window,
            ])
            .context("failed to populate View menu")?;

        let window_menu = Submenu::new("Window", true);
        menu.append(&window_menu)
            .context("failed to append Window menu")?;
        window_menu
            .append_items(&[
                &PredefinedMenuItem::minimize(None),
                &PredefinedMenuItem::separator(),
                &PredefinedMenuItem::close_window(None),
            ])
            .context("failed to populate Window menu")?;
        window_menu.set_as_windows_menu_for_nsapp();

        let help_menu = Submenu::new("Help", true);
        menu.append(&help_menu)
            .context("failed to append Help menu")?;
        help_menu
            .append_items(&[&PredefinedMenuItem::about(Some("About ImranView"), None)])
            .context("failed to populate Help menu")?;
        help_menu.set_as_help_menu_for_nsapp();

        menu.init_for_nsapp();

        Ok(Self {
            _menu: menu,
            file_save,
            file_save_as,
            edit_rotate_left,
            edit_rotate_right,
            edit_flip_horizontal,
            edit_flip_vertical,
            view_show_toolbar,
            view_show_status_bar,
            view_show_thumbnail_strip,
            view_show_thumbnail_window,
        })
    }

    pub fn sync_state(&self, state: &AppState) {
        let has_image = state.has_image();
        self.file_save.set_enabled(has_image);
        self.file_save_as.set_enabled(has_image);
        self.edit_rotate_left.set_enabled(has_image);
        self.edit_rotate_right.set_enabled(has_image);
        self.edit_flip_horizontal.set_enabled(has_image);
        self.edit_flip_vertical.set_enabled(has_image);

        self.view_show_toolbar.set_checked(state.show_toolbar());
        self.view_show_status_bar
            .set_checked(state.show_status_bar());
        self.view_show_thumbnail_strip
            .set_checked(state.show_thumbnail_strip());
        self.view_show_thumbnail_window
            .set_checked(state.thumbnails_window_mode());
    }

    pub fn drain_actions(&self) -> Vec<NativeMenuAction> {
        let mut actions = Vec::new();
        while let Ok(event) = MenuEvent::receiver().try_recv() {
            let action = match event.id.as_ref() {
                ID_FILE_OPEN => Some(NativeMenuAction::Open),
                ID_FILE_SAVE => Some(NativeMenuAction::Save),
                ID_FILE_SAVE_AS => Some(NativeMenuAction::SaveAs),
                ID_FILE_EXIT => Some(NativeMenuAction::Exit),
                ID_EDIT_ROTATE_LEFT => Some(NativeMenuAction::RotateLeft),
                ID_EDIT_ROTATE_RIGHT => Some(NativeMenuAction::RotateRight),
                ID_EDIT_FLIP_HORIZONTAL => Some(NativeMenuAction::FlipHorizontal),
                ID_EDIT_FLIP_VERTICAL => Some(NativeMenuAction::FlipVertical),
                ID_VIEW_SHOW_TOOLBAR => Some(NativeMenuAction::ToggleShowToolbar),
                ID_VIEW_SHOW_STATUS_BAR => Some(NativeMenuAction::ToggleShowStatusBar),
                ID_VIEW_SHOW_THUMBNAIL_STRIP => Some(NativeMenuAction::ToggleThumbnailStrip),
                ID_VIEW_SHOW_THUMBNAIL_WINDOW => Some(NativeMenuAction::ToggleThumbnailWindow),
                _ => None,
            };
            if let Some(action) = action {
                actions.push(action);
            }
        }
        actions
    }
}
