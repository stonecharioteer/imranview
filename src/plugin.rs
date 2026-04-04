use std::path::PathBuf;

use eframe::egui;

#[derive(Clone, Debug)]
pub struct PluginContext {
    pub has_image: bool,
    pub current_file: Option<PathBuf>,
    pub compare_mode: bool,
}

#[derive(Clone, Debug)]
pub enum PluginEvent {
    ImageOpened(PathBuf),
    ImageSaved(PathBuf),
    TransformApplied(String),
    BatchCompleted { processed: usize, failed: usize },
    FileOperation(String),
    CompareLoaded(PathBuf),
    PrintSubmitted(PathBuf),
}

pub trait ViewerPlugin: Send {
    fn id(&self) -> &'static str;
    fn on_event(&mut self, event: &PluginEvent, context: &PluginContext);
    fn menu_ui(&mut self, ui: &mut egui::Ui, context: &PluginContext);
}

pub struct PluginHost {
    plugins: Vec<Box<dyn ViewerPlugin>>,
}

impl PluginHost {
    pub fn new_with_builtins() -> Self {
        Self {
            plugins: vec![Box::new(EventCounterPlugin::default())],
        }
    }

    pub fn emit(&mut self, event: PluginEvent, context: &PluginContext) {
        for plugin in &mut self.plugins {
            plugin.on_event(&event, context);
        }
    }

    pub fn menu_ui(&mut self, ui: &mut egui::Ui, context: &PluginContext) {
        ui.label(format!("Registered plugins: {}", self.plugins.len()));
        for plugin in &mut self.plugins {
            ui.separator();
            ui.collapsing(plugin.id(), |ui| {
                plugin.menu_ui(ui, context);
            });
        }
    }
}

#[derive(Default)]
struct EventCounterPlugin {
    event_count: u64,
    last_event: Option<String>,
}

impl ViewerPlugin for EventCounterPlugin {
    fn id(&self) -> &'static str {
        "event-counter"
    }

    fn on_event(&mut self, event: &PluginEvent, _context: &PluginContext) {
        self.event_count = self.event_count.saturating_add(1);
        self.last_event = Some(match event {
            PluginEvent::ImageOpened(path) => format!("opened {}", path.display()),
            PluginEvent::ImageSaved(path) => format!("saved {}", path.display()),
            PluginEvent::TransformApplied(name) => format!("transform {name}"),
            PluginEvent::BatchCompleted { processed, failed } => {
                format!("batch completed: {processed} processed / {failed} failed")
            }
            PluginEvent::FileOperation(name) => format!("file operation {name}"),
            PluginEvent::CompareLoaded(path) => format!("compare {}", path.display()),
            PluginEvent::PrintSubmitted(path) => format!("print {}", path.display()),
        });
    }

    fn menu_ui(&mut self, ui: &mut egui::Ui, context: &PluginContext) {
        ui.label(format!("Events seen: {}", self.event_count));
        if let Some(last_event) = &self.last_event {
            ui.label(format!("Last event: {last_event}"));
        }
        ui.label(format!("Has image: {}", context.has_image));
        ui.label(format!("Compare mode: {}", context.compare_mode));
        if let Some(path) = &context.current_file {
            ui.label(format!("Current file: {}", path.display()));
        }
    }
}
