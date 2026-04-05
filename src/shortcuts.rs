use eframe::egui;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ShortcutAction {
    Open,
    Save,
    SaveAs,
    CommandPalette,
    Undo,
    Redo,
    PreviousImage,
    NextImage,
    ZoomIn,
    ZoomOut,
    Fit,
    ActualSize,
}

fn shortcut_for(action: ShortcutAction) -> Option<egui::KeyboardShortcut> {
    match action {
        ShortcutAction::Open => Some(egui::KeyboardShortcut::new(
            egui::Modifiers::COMMAND,
            egui::Key::O,
        )),
        ShortcutAction::Save => Some(egui::KeyboardShortcut::new(
            egui::Modifiers::COMMAND,
            egui::Key::S,
        )),
        ShortcutAction::SaveAs => Some(egui::KeyboardShortcut::new(
            egui::Modifiers::COMMAND | egui::Modifiers::SHIFT,
            egui::Key::S,
        )),
        ShortcutAction::CommandPalette => Some(egui::KeyboardShortcut::new(
            egui::Modifiers::COMMAND,
            egui::Key::K,
        )),
        ShortcutAction::Undo => Some(egui::KeyboardShortcut::new(
            egui::Modifiers::COMMAND,
            egui::Key::Z,
        )),
        ShortcutAction::Redo => Some(egui::KeyboardShortcut::new(
            egui::Modifiers::COMMAND | egui::Modifiers::SHIFT,
            egui::Key::Z,
        )),
        ShortcutAction::PreviousImage => Some(egui::KeyboardShortcut::new(
            egui::Modifiers::NONE,
            egui::Key::ArrowLeft,
        )),
        ShortcutAction::NextImage => Some(egui::KeyboardShortcut::new(
            egui::Modifiers::NONE,
            egui::Key::ArrowRight,
        )),
        ShortcutAction::ZoomIn => Some(egui::KeyboardShortcut::new(
            egui::Modifiers::NONE,
            egui::Key::Plus,
        )),
        ShortcutAction::ZoomOut => Some(egui::KeyboardShortcut::new(
            egui::Modifiers::NONE,
            egui::Key::Minus,
        )),
        ShortcutAction::Fit => Some(egui::KeyboardShortcut::new(
            egui::Modifiers::NONE,
            egui::Key::Num0,
        )),
        ShortcutAction::ActualSize => Some(egui::KeyboardShortcut::new(
            egui::Modifiers::NONE,
            egui::Key::Num1,
        )),
    }
}

pub fn trigger(ctx: &egui::Context, action: ShortcutAction) -> bool {
    if action == ShortcutAction::ZoomIn {
        if let Some(shortcut) = shortcut_for(action) {
            if ctx.input_mut(|i| i.consume_shortcut(&shortcut)) {
                return true;
            }
        }
        return ctx.input_mut(|i| i.consume_key(egui::Modifiers::NONE, egui::Key::Equals));
    }

    if let Some(shortcut) = shortcut_for(action) {
        return ctx.input_mut(|i| i.consume_shortcut(&shortcut));
    }

    false
}

pub fn shortcut_text(ctx: &egui::Context, action: ShortcutAction) -> Option<String> {
    shortcut_for(action).map(|shortcut| ctx.format_shortcut(&shortcut))
}

pub fn menu_item_label(ctx: &egui::Context, action: ShortcutAction, title: &str) -> String {
    if let Some(shortcut) = shortcut_text(ctx, action) {
        format!("{title}\t{shortcut}")
    } else {
        title.to_owned()
    }
}
