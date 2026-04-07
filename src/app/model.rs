use std::collections::{HashMap, VecDeque};
use std::path::PathBuf;

use eframe::egui;

use crate::image_io::MetadataSummary;

use super::controller::MenuCommand;

#[derive(Clone, Debug)]
pub(super) struct CommandPaletteEntry {
    pub(super) group: &'static str,
    pub(super) title: String,
    pub(super) shortcut: Option<String>,
    pub(super) search_blob: String,
    pub(super) enabled: bool,
    pub(super) command: MenuCommand,
}

#[derive(Clone, Debug, Default)]
pub(super) struct CommandPaletteState {
    pub(super) open: bool,
    pub(super) query: String,
    pub(super) selected_index: usize,
    pub(super) request_focus: bool,
}

#[derive(Clone, Debug, Default)]
pub(super) struct FileSortFacts {
    pub(super) name: String,
    pub(super) extension: String,
    pub(super) modified_epoch_secs: u64,
    pub(super) size_bytes: u64,
}

pub(super) fn default_scanner_command_template() -> String {
    #[cfg(target_os = "windows")]
    {
        String::new()
    }
    #[cfg(not(target_os = "windows"))]
    {
        "scanimage --format=png --output-file {output}".to_owned()
    }
}

#[derive(Clone, Debug, PartialEq)]
pub(super) struct ViewportSnapshot {
    pub(super) position: Option<[f32; 2]>,
    pub(super) inner_size: Option<[f32; 2]>,
    pub(super) maximized: Option<bool>,
    pub(super) fullscreen: Option<bool>,
}

#[derive(Clone, Debug, Default)]
pub(super) struct FolderPanelCache {
    pub(super) current_directory: Option<PathBuf>,
    pub(super) ancestors: Vec<PathBuf>,
    pub(super) siblings: Vec<PathBuf>,
    pub(super) children: Vec<PathBuf>,
}

pub(super) struct ThumbTextureCache {
    pub(super) map: HashMap<PathBuf, egui::TextureHandle>,
    byte_sizes: HashMap<PathBuf, usize>,
    order: VecDeque<PathBuf>,
    capacity: usize,
    max_bytes: usize,
    pub(super) total_bytes: usize,
}

pub(super) struct CompareImageState {
    pub(super) path: PathBuf,
    pub(super) texture: egui::TextureHandle,
    pub(super) width: u32,
    pub(super) height: u32,
    pub(super) metadata: MetadataSummary,
}

impl ThumbTextureCache {
    pub(super) fn new(capacity: usize, max_bytes: usize) -> Self {
        Self {
            map: HashMap::new(),
            byte_sizes: HashMap::new(),
            order: VecDeque::new(),
            capacity,
            max_bytes,
            total_bytes: 0,
        }
    }

    pub(super) fn get(&mut self, path: &PathBuf) -> Option<&egui::TextureHandle> {
        if self.map.contains_key(path) {
            self.touch(path);
        }
        self.map.get(path)
    }

    pub(super) fn insert(&mut self, path: PathBuf, texture: egui::TextureHandle) {
        let bytes = Self::estimate_texture_bytes(&texture);
        if self.map.contains_key(&path) {
            if let Some(previous_bytes) = self.byte_sizes.insert(path.clone(), bytes) {
                self.total_bytes = self.total_bytes.saturating_sub(previous_bytes);
            }
            self.total_bytes = self.total_bytes.saturating_add(bytes);
            self.map.insert(path.clone(), texture);
            self.touch(&path);
            self.evict_if_needed();
            return;
        }

        self.map.insert(path.clone(), texture);
        self.byte_sizes.insert(path.clone(), bytes);
        self.total_bytes = self.total_bytes.saturating_add(bytes);
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
        while self.map.len() > self.capacity || self.total_bytes > self.max_bytes {
            if let Some(oldest) = self.order.pop_front() {
                self.map.remove(&oldest);
                if let Some(bytes) = self.byte_sizes.remove(&oldest) {
                    self.total_bytes = self.total_bytes.saturating_sub(bytes);
                }
            } else {
                break;
            }
        }
    }

    fn estimate_texture_bytes(texture: &egui::TextureHandle) -> usize {
        let [width, height] = texture.size();
        width.saturating_mul(height).saturating_mul(4)
    }
}
