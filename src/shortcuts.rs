use eframe::egui;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ShortcutAction {
    Open,
    Save,
    SaveAs,
    PreviousImage,
    NextImage,
    ZoomIn,
    ZoomOut,
    Fit,
    ActualSize,
}

pub fn trigger(ctx: &egui::Context, action: ShortcutAction) -> bool {
    match action {
        ShortcutAction::Open => ctx.input_mut(|i| {
            i.consume_shortcut(&egui::KeyboardShortcut::new(
                egui::Modifiers::COMMAND,
                egui::Key::O,
            ))
        }),
        ShortcutAction::Save => ctx.input_mut(|i| {
            i.consume_shortcut(&egui::KeyboardShortcut::new(
                egui::Modifiers::COMMAND,
                egui::Key::S,
            ))
        }),
        ShortcutAction::SaveAs => ctx.input_mut(|i| {
            i.consume_shortcut(&egui::KeyboardShortcut::new(
                egui::Modifiers::COMMAND | egui::Modifiers::SHIFT,
                egui::Key::S,
            ))
        }),
        ShortcutAction::PreviousImage => ctx.input(|i| i.key_pressed(egui::Key::ArrowLeft)),
        ShortcutAction::NextImage => ctx.input(|i| i.key_pressed(egui::Key::ArrowRight)),
        ShortcutAction::ZoomIn => {
            ctx.input(|i| i.key_pressed(egui::Key::Plus) || i.key_pressed(egui::Key::Equals))
        }
        ShortcutAction::ZoomOut => ctx.input(|i| i.key_pressed(egui::Key::Minus)),
        ShortcutAction::Fit => ctx.input(|i| i.key_pressed(egui::Key::Num0)),
        ShortcutAction::ActualSize => ctx.input(|i| i.key_pressed(egui::Key::Num1)),
    }
}

pub fn menu_item_label(ctx: &egui::Context, action: ShortcutAction, title: &str) -> String {
    let shortcut = match action {
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
    };

    if let Some(shortcut) = shortcut {
        format!("{title}\t{}", ctx.format_shortcut(&shortcut))
    } else {
        title.to_owned()
    }
}
