//! A compact light desktop look for panels and dialogs.

use egui::{Color32, FontFamily, FontId, Stroke, TextStyle};

pub const PANEL: Color32 = Color32::from_rgb(240, 240, 240);
pub const HEADER: Color32 = Color32::from_rgb(225, 228, 234);
pub const UP: Color32 = Color32::from_rgb(0, 90, 220);
pub const DOWN: Color32 = Color32::from_rgb(210, 30, 30);

pub fn apply(ctx: &egui::Context) {
    let mut v = egui::Visuals::light();
    v.panel_fill = PANEL;
    v.window_fill = Color32::from_rgb(246, 246, 246);
    v.extreme_bg_color = Color32::WHITE;
    v.faint_bg_color = Color32::from_rgb(236, 240, 246);
    v.selection.bg_fill = Color32::from_rgb(190, 215, 245);
    v.selection.stroke = Stroke::new(1.0_f32, Color32::from_rgb(20, 60, 120));
    v.widgets.noninteractive.bg_stroke = Stroke::new(1.0_f32, Color32::from_rgb(200, 200, 205));
    v.widgets.inactive.weak_bg_fill = Color32::from_rgb(232, 232, 236);
    v.widgets.inactive.bg_fill = Color32::from_rgb(232, 232, 236);
    v.window_corner_radius = 2.into();
    v.menu_corner_radius = 2.into();
    ctx.set_visuals(v);
    let mut style = (*ctx.style()).clone();
    style.text_styles = [
        (TextStyle::Heading, FontId::new(15.0, FontFamily::Proportional)),
        (TextStyle::Body, FontId::new(12.5, FontFamily::Proportional)),
        (TextStyle::Monospace, FontId::new(12.0, FontFamily::Monospace)),
        (TextStyle::Button, FontId::new(12.5, FontFamily::Proportional)),
        (TextStyle::Small, FontId::new(10.5, FontFamily::Proportional)),
    ]
    .into();
    style.spacing.item_spacing = egui::vec2(6.0, 3.0);
    style.spacing.button_padding = egui::vec2(5.0, 2.0);
    style.spacing.interact_size.y = 18.0;
    ctx.set_style(style);
}
