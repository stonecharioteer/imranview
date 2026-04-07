use anyhow::{Result, anyhow};
use eframe::egui;

pub(super) fn apply_native_look(ctx: &egui::Context) {
    ctx.set_theme(egui::ThemePreference::System);
    ctx.all_styles_mut(|style| {
        style.spacing.item_spacing = egui::vec2(8.0, 6.0);
        style.spacing.button_padding = egui::vec2(8.0, 4.0);
        style.spacing.interact_size.y = 24.0;
        style.spacing.menu_margin = egui::Margin::symmetric(8, 6);
        style.spacing.window_margin = egui::Margin::symmetric(10, 8);
        style.spacing.combo_width = style.spacing.combo_width.max(120.0);
        style.spacing.slider_width = style.spacing.slider_width.max(140.0);

        if let Some(body) = style.text_styles.get_mut(&egui::TextStyle::Body) {
            body.size = body.size.clamp(13.0, 15.0);
        }
        if let Some(button) = style.text_styles.get_mut(&egui::TextStyle::Button) {
            button.size = button.size.clamp(13.0, 15.0);
        }
        if let Some(small) = style.text_styles.get_mut(&egui::TextStyle::Small) {
            small.size = small.size.clamp(11.0, 13.0);
        }

        let visuals = &mut style.visuals;
        let (window_fill, panel_fill, extreme_bg, border_color, shadow_color) = if visuals.dark_mode
        {
            (
                egui::Color32::from_rgb(30, 31, 34),
                egui::Color32::from_rgb(37, 38, 42),
                egui::Color32::from_rgb(24, 25, 27),
                egui::Color32::from_gray(72),
                egui::Color32::from_black_alpha(110),
            )
        } else {
            (
                egui::Color32::from_rgb(250, 250, 252),
                egui::Color32::from_rgb(242, 242, 246),
                egui::Color32::from_rgb(255, 255, 255),
                egui::Color32::from_rgb(198, 198, 205),
                egui::Color32::from_black_alpha(44),
            )
        };

        let window_radius = platform_window_corner_radius();
        let widget_radius = platform_widget_corner_radius();

        visuals.window_corner_radius = egui::CornerRadius::same(window_radius);
        visuals.menu_corner_radius = egui::CornerRadius::same(widget_radius);
        visuals.window_fill = window_fill;
        visuals.panel_fill = panel_fill;
        visuals.faint_bg_color = panel_fill;
        visuals.extreme_bg_color = extreme_bg;
        visuals.window_stroke = egui::Stroke::new(1.0, border_color);
        visuals.window_shadow = egui::Shadow {
            offset: [0, 2],
            blur: 16,
            spread: 0,
            color: shadow_color,
        };
        visuals.popup_shadow = visuals.window_shadow;
        visuals.interact_cursor = None;
        visuals.button_frame = true;
        visuals.collapsing_header_frame = false;
        visuals.widgets.noninteractive.bg_stroke = egui::Stroke::new(1.0, border_color);
        visuals.widgets.noninteractive.corner_radius = egui::CornerRadius::same(widget_radius);
        visuals.widgets.inactive.corner_radius = egui::CornerRadius::same(widget_radius);
        visuals.widgets.hovered.corner_radius = egui::CornerRadius::same(widget_radius);
        visuals.widgets.active.corner_radius = egui::CornerRadius::same(widget_radius);
        visuals.widgets.open.corner_radius = egui::CornerRadius::same(widget_radius);
    });
}

pub(super) fn centered_dialog_window(title: &'static str) -> egui::Window<'static> {
    egui::Window::new(title)
        .collapsible(false)
        .resizable(false)
        .anchor(egui::Align2::CENTER_CENTER, egui::Vec2::ZERO)
}

#[derive(Clone)]
pub(super) struct ToolbarIcons {
    pub(super) open: egui::TextureHandle,
    pub(super) prev: egui::TextureHandle,
    pub(super) next: egui::TextureHandle,
    pub(super) zoom_out: egui::TextureHandle,
    pub(super) zoom_in: egui::TextureHandle,
    pub(super) actual_size: egui::TextureHandle,
    pub(super) fit: egui::TextureHandle,
    pub(super) gallery: egui::TextureHandle,
}

impl ToolbarIcons {
    pub(super) fn try_load(ctx: &egui::Context) -> Option<Self> {
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
                include_bytes!("../../assets/icons/tabler/png/folder-open.png"),
            )?,
            prev: load_toolbar_icon(
                ctx,
                "prev",
                include_bytes!("../../assets/icons/tabler/png/chevron-left.png"),
            )?,
            next: load_toolbar_icon(
                ctx,
                "next",
                include_bytes!("../../assets/icons/tabler/png/chevron-right.png"),
            )?,
            zoom_out: load_toolbar_icon(
                ctx,
                "zoom-out",
                include_bytes!("../../assets/icons/tabler/png/zoom-out.png"),
            )?,
            zoom_in: load_toolbar_icon(
                ctx,
                "zoom-in",
                include_bytes!("../../assets/icons/tabler/png/zoom-in.png"),
            )?,
            actual_size: load_toolbar_icon(
                ctx,
                "actual-size",
                include_bytes!("../../assets/icons/tabler/png/maximize.png"),
            )?,
            fit: load_toolbar_icon(
                ctx,
                "fit",
                include_bytes!("../../assets/icons/tabler/png/aspect-ratio.png"),
            )?,
            gallery: load_toolbar_icon(
                ctx,
                "gallery",
                include_bytes!("../../assets/icons/tabler/png/photo.png"),
            )?,
        })
    }
}

fn load_toolbar_icon(ctx: &egui::Context, name: &str, bytes: &[u8]) -> Result<egui::TextureHandle> {
    load_png_texture(ctx, &format!("toolbar-{name}"), bytes)
}

pub(super) fn load_png_texture(
    ctx: &egui::Context,
    texture_name: &str,
    bytes: &[u8],
) -> Result<egui::TextureHandle> {
    let image = image::load_from_memory(bytes)
        .map_err(|err| anyhow!("failed to decode texture {texture_name}: {err}"))?;
    let rgba = image.to_rgba8();
    let width = rgba.width() as usize;
    let height = rgba.height() as usize;
    let pixels = rgba.into_raw();
    let color = egui::ColorImage::from_rgba_unmultiplied([width, height], &pixels);
    Ok(ctx.load_texture(texture_name.to_owned(), color, egui::TextureOptions::LINEAR))
}

pub(super) const fn platform_window_corner_radius() -> u8 {
    #[cfg(target_os = "macos")]
    {
        12
    }
    #[cfg(target_os = "windows")]
    {
        8
    }
    #[cfg(all(not(target_os = "macos"), not(target_os = "windows")))]
    {
        7
    }
}

pub(super) const fn platform_widget_corner_radius() -> u8 {
    #[cfg(target_os = "macos")]
    {
        9
    }
    #[cfg(target_os = "windows")]
    {
        6
    }
    #[cfg(all(not(target_os = "macos"), not(target_os = "windows")))]
    {
        6
    }
}
