//! Colors and visual style.

use eframe::egui::{self, Color32, Stroke};

pub const BG: Color32 = Color32::from_rgb(28, 30, 34);
pub const PANEL: Color32 = Color32::from_rgb(36, 39, 44);
pub const LANE: Color32 = Color32::from_rgb(42, 45, 51);
pub const LANE_ALT: Color32 = Color32::from_rgb(38, 41, 46);
pub const GRID: Color32 = Color32::from_rgb(55, 58, 66);
pub const GRID_BAR: Color32 = Color32::from_rgb(78, 82, 92);
pub const TEXT_DIM: Color32 = Color32::from_rgb(150, 155, 165);
pub const ACCENT: Color32 = Color32::from_rgb(255, 170, 60);
pub const PLAYHEAD: Color32 = Color32::from_rgb(255, 255, 255);
pub const CYCLE: Color32 = Color32::from_rgba_premultiplied(140, 110, 20, 120);
pub const SELECT: Color32 = Color32::from_rgb(255, 255, 255);
pub const MUTE: Color32 = Color32::from_rgb(70, 140, 220);
pub const SOLO: Color32 = Color32::from_rgb(230, 200, 60);
pub const REC: Color32 = Color32::from_rgb(220, 60, 60);
pub const AI: Color32 = Color32::from_rgb(170, 130, 255);

pub fn rgb(c: [u8; 3]) -> Color32 {
    Color32::from_rgb(c[0], c[1], c[2])
}

pub fn darker(c: Color32, f: f32) -> Color32 {
    Color32::from_rgb(
        (c.r() as f32 * f) as u8,
        (c.g() as f32 * f) as u8,
        (c.b() as f32 * f) as u8,
    )
}

/// Add Windows symbol/emoji fonts as fallbacks so icons render (egui's defaults lack many glyphs).
fn install_fonts(ctx: &egui::Context) {
    let mut fonts = egui::FontDefinitions::default();
    let dir = std::env::var("WINDIR")
        .map(|w| std::path::PathBuf::from(w).join("Fonts"))
        .unwrap_or_default();
    for (name, file) in [
        ("segoe_symbol", "seguisym.ttf"),
        ("segoe_emoji", "seguiemj.ttf"),
    ] {
        if let Ok(bytes) = std::fs::read(dir.join(file)) {
            fonts
                .font_data
                .insert(name.to_owned(), egui::FontData::from_owned(bytes));
            for fam in [egui::FontFamily::Proportional, egui::FontFamily::Monospace] {
                fonts.families.entry(fam).or_default().push(name.to_owned());
            }
        }
    }
    ctx.set_fonts(fonts);
}

pub fn apply(ctx: &egui::Context) {
    install_fonts(ctx);
    let mut v = egui::Visuals::dark();
    v.panel_fill = PANEL;
    v.window_fill = PANEL;
    v.extreme_bg_color = BG;
    v.selection.bg_fill = Color32::from_rgb(200, 120, 30);
    v.selection.stroke = Stroke::new(1.0_f32, ACCENT);
    v.widgets.hovered.bg_stroke = Stroke::new(1.0_f32, ACCENT);
    ctx.set_visuals(v);
    let mut style = (*ctx.style()).clone();
    style.spacing.item_spacing = egui::vec2(6.0, 4.0);
    style.spacing.button_padding = egui::vec2(6.0, 2.0);
    ctx.set_style(style);
}

pub fn velocity_color(base: Color32, velocity: u8) -> Color32 {
    let f = 0.45 + 0.55 * (velocity as f32 / 127.0);
    Color32::from_rgb(
        (base.r() as f32 * f) as u8,
        (base.g() as f32 * f) as u8,
        (base.b() as f32 * f) as u8,
    )
}
