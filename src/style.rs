//! A compact light Win32 desktop look for panels, toolbars, and dialogs matching MetaTrader 5.

use egui::{Color32, FontFamily, FontId, Stroke, TextStyle};

pub const PANEL: Color32 = Color32::from_rgb(240, 240, 240);
pub const PANEL_BORDER: Color32 = Color32::from_rgb(170, 170, 175);
pub const HEADER: Color32 = Color32::from_rgb(228, 232, 238);
pub const SUNKEN_DARK: Color32 = Color32::from_rgb(160, 160, 160);
pub const SUNKEN_LIGHT: Color32 = Color32::from_rgb(255, 255, 255);
pub const UP: Color32 = Color32::from_rgb(0, 85, 215);
pub const DOWN: Color32 = Color32::from_rgb(215, 25, 25);
pub const TAB_ACTIVE: Color32 = Color32::from_rgb(255, 255, 255);
pub const TAB_INACTIVE: Color32 = Color32::from_rgb(232, 232, 236);

pub fn apply(ctx: &egui::Context) {
    let mut v = egui::Visuals::light();
    v.panel_fill = PANEL;
    v.window_fill = Color32::from_rgb(244, 244, 246);
    v.extreme_bg_color = Color32::WHITE;
    v.faint_bg_color = Color32::from_rgb(236, 240, 246);
    v.selection.bg_fill = Color32::from_rgb(180, 212, 248);
    v.selection.stroke = Stroke::new(1.0_f32, Color32::from_rgb(20, 60, 120));
    v.widgets.noninteractive.bg_stroke = Stroke::new(1.0_f32, Color32::from_rgb(195, 198, 204));
    v.widgets.inactive.weak_bg_fill = Color32::from_rgb(235, 235, 238);
    v.widgets.inactive.bg_fill = Color32::from_rgb(235, 235, 238);
    v.widgets.hovered.bg_fill = Color32::from_rgb(220, 235, 252);
    v.widgets.hovered.bg_stroke = Stroke::new(1.0_f32, Color32::from_rgb(140, 190, 245));
    v.widgets.active.bg_fill = Color32::from_rgb(195, 222, 250);
    v.widgets.active.bg_stroke = Stroke::new(1.0_f32, Color32::from_rgb(100, 160, 235));
    v.window_corner_radius = 0.into();
    v.menu_corner_radius = 0.into();
    ctx.set_visuals(v);

    let mut style = (*ctx.style()).clone();
    style.text_styles = [
        (TextStyle::Heading, FontId::new(14.0, FontFamily::Proportional)),
        (TextStyle::Body, FontId::new(12.0, FontFamily::Proportional)),
        (TextStyle::Monospace, FontId::new(11.5, FontFamily::Monospace)),
        (TextStyle::Button, FontId::new(12.0, FontFamily::Proportional)),
        (TextStyle::Small, FontId::new(10.0, FontFamily::Proportional)),
    ]
    .into();
    style.spacing.item_spacing = egui::vec2(4.0, 2.0);
    style.spacing.button_padding = egui::vec2(4.0, 2.0);
    style.spacing.interact_size.y = 19.0;
    ctx.set_style(style);
}

/// Draw an authentic Win32 sunken box for status bar compartments or inset fields.
pub fn draw_sunken_box(painter: &egui::Painter, rect: egui::Rect) {
    painter.rect_filled(rect, 0.0, PANEL);
    painter.line_segment([rect.left_top(), rect.right_top()], Stroke::new(1.0_f32, SUNKEN_DARK));
    painter.line_segment([rect.left_top(), rect.left_bottom()], Stroke::new(1.0_f32, SUNKEN_DARK));
    painter.line_segment([rect.right_top(), rect.right_bottom()], Stroke::new(1.0_f32, SUNKEN_LIGHT));
    painter.line_segment([rect.left_bottom(), rect.right_bottom()], Stroke::new(1.0_f32, SUNKEN_LIGHT));
}

/// Draw an authentic Win32 raised box for tabs or push buttons.
pub fn draw_raised_box(painter: &egui::Painter, rect: egui::Rect, fill: Color32) {
    painter.rect_filled(rect, 0.0, fill);
    painter.line_segment([rect.left_top(), rect.right_top()], Stroke::new(1.0_f32, SUNKEN_LIGHT));
    painter.line_segment([rect.left_top(), rect.left_bottom()], Stroke::new(1.0_f32, SUNKEN_LIGHT));
    painter.line_segment([rect.right_top(), rect.right_bottom()], Stroke::new(1.0_f32, SUNKEN_DARK));
    painter.line_segment([rect.left_bottom(), rect.right_bottom()], Stroke::new(1.0_f32, SUNKEN_DARK));
}

/// Draw a vertical Win32 toolbar grip separator.
pub fn draw_toolbar_grip(painter: &egui::Painter, rect: egui::Rect) {
    let x = rect.left() + 2.0;
    let top = rect.top() + 2.0;
    let bot = rect.bottom() - 2.0;
    painter.line_segment([egui::pos2(x, top), egui::pos2(x, bot)], Stroke::new(1.0_f32, SUNKEN_DARK));
    painter.line_segment([egui::pos2(x + 1.0, top + 1.0), egui::pos2(x + 1.0, bot + 1.0)], Stroke::new(1.0_f32, SUNKEN_LIGHT));
    painter.line_segment([egui::pos2(x + 3.0, top), egui::pos2(x + 3.0, bot)], Stroke::new(1.0_f32, SUNKEN_DARK));
    painter.line_segment([egui::pos2(x + 4.0, top + 1.0), egui::pos2(x + 4.0, bot + 1.0)], Stroke::new(1.0_f32, SUNKEN_LIGHT));
}

/// A shortcut label as the platform writes it: "Ctrl+M" becomes "Cmd+M" on macOS.
pub fn sc(label: &str) -> String {
    if cfg!(target_os = "macos") {
        label.replace("Ctrl+", "Cmd+")
    } else {
        label.to_string()
    }
}
