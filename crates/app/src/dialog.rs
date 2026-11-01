//! Dialogs and tool windows in one style: a panel-colored frame with the window radius, no collapse arrow, a
//! dimmed backdrop under modal ones, a filled accent button for the main action, section titles and hints.

use eframe::egui::{self, vec2, Color32, Response, RichText, Rounding, Stroke, Ui};

use crate::theme;

/// Alpha of the black veil under a modal dialog.
pub const BACKDROP_ALPHA: u8 = 150;
/// Height of the buttons in a dialog's button row.
pub const BUTTON_H: f32 = 26.0;

/// The frame of every dialog and tool window.
pub fn frame() -> egui::Frame {
    egui::Frame::window(&egui::Style { visuals: theme::visuals(), ..Default::default() })
        .fill(theme::PANEL_BG)
        .stroke(Stroke::new(1.0_f32, theme::BORDER))
        .rounding(Rounding::same(theme::R_XL))
        .inner_margin(egui::Margin::same(theme::SP_6))
}

/// A tool window (Settings, Mix Analysis, MIDI Transform...): framed, no collapse arrow, closed with its × button.
pub fn window(title: &str) -> egui::Window<'static> {
    egui::Window::new(RichText::new(title).font(theme::title_font()).color(theme::TEXT))
        .id(egui::Id::new(("tool-window", title)))
        .frame(frame())
        .collapsible(false)
}

/// A modal dialog centered on the window, over [`backdrop`].
pub fn modal(title: &str, id: &str) -> egui::Window<'static> {
    egui::Window::new(RichText::new(title).font(theme::title_font()).color(theme::TEXT))
        .id(egui::Id::new(id))
        .frame(frame())
        .order(egui::Order::Foreground)
        .anchor(egui::Align2::CENTER_CENTER, [0.0, 0.0])
        .collapsible(false)
        .resizable(false)
        .movable(false)
}

/// Dims the whole window and swallows clicks, under a modal dialog.
pub fn backdrop(ctx: &egui::Context, id: &str) {
    let screen = ctx.screen_rect();
    egui::Area::new(egui::Id::new(id))
        .order(egui::Order::Foreground)
        .fixed_pos(screen.min)
        .show(ctx, |ui| {
            ui.painter().rect_filled(screen, 0.0, Color32::from_black_alpha(BACKDROP_ALPHA));
            ui.allocate_rect(screen, egui::Sense::click_and_drag());
        });
}

/// Colors of the primary button: fill, text, and the fill under the pointer.
pub fn primary_colors(hovered: bool) -> (Color32, Color32) {
    (if hovered { theme::ACCENT_HOVER } else { theme::ACCENT }, theme::ON_ACCENT)
}

/// The main action of a dialog: filled with the accent.
pub fn primary_button(ui: &mut Ui, text: &str) -> Response {
    primary_button_enabled(ui, true, text)
}

/// [`primary_button`], greyed out when `enabled` is false.
pub fn primary_button_enabled(ui: &mut Ui, enabled: bool, text: &str) -> Response {
    let id = ui.next_auto_id();
    let hovered = enabled && ui.ctx().read_response(id).is_some_and(|r| r.hovered());
    let (fill, fg) = if enabled { primary_colors(hovered) } else { (theme::RAISED, theme::TEXT_DISABLED) };
    let b = egui::Button::new(RichText::new(text).font(egui::FontId::new(theme::SIZE_BODY, theme::semibold())).color(fg))
        .fill(fill)
        .stroke(Stroke::NONE)
        .rounding(Rounding::same(theme::R_LG))
        .min_size(vec2(72.0, BUTTON_H));
    ui.add_enabled(enabled, b)
}

/// A secondary action of a dialog (Cancel, Don't Save).
pub fn secondary_button(ui: &mut Ui, text: &str) -> Response {
    let b = egui::Button::new(RichText::new(text).color(theme::TEXT))
        .fill(theme::RAISED)
        .stroke(Stroke::new(1.0_f32, theme::BORDER))
        .rounding(Rounding::same(theme::R_LG))
        .min_size(vec2(72.0, BUTTON_H));
    ui.add(b)
}

/// A section title inside a dialog, with the space above it that separates it from the previous section.
pub fn section(ui: &mut Ui, title: &str, first: bool) {
    if !first {
        ui.add_space(theme::SP_4);
        ui.separator();
        ui.add_space(theme::SP_2);
    }
    ui.label(RichText::new(title).font(egui::FontId::new(13.0, theme::semibold())).color(theme::TEXT));
    ui.add_space(theme::SP_1);
}

/// An explanation under a setting: small, tertiary text.
pub fn hint(ui: &mut Ui, text: impl Into<String>) -> Response {
    ui.label(RichText::new(text.into()).size(theme::SIZE_NUMBER).color(theme::TEXT_3))
}

/// A row with a fixed-width caption on the left and the control after it, so the controls of a dialog line up.
pub fn row<R>(ui: &mut Ui, caption: &str, add: impl FnOnce(&mut Ui) -> R) -> R {
    ui.horizontal(|ui| {
        let (rect, _) = ui.allocate_exact_size(vec2(CAPTION_W, BUTTON_H), egui::Sense::hover());
        ui.painter().text(
            rect.left_center(),
            egui::Align2::LEFT_CENTER,
            caption,
            theme::body_font(),
            theme::TEXT_2,
        );
        add(ui)
    })
    .inner
}

/// Width of the captions of [`row`].
pub const CAPTION_W: f32 = 132.0;

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn primary_button_reads_on_its_fill_and_lightens_on_hover() {
        let lum = |c: Color32| 0.2126 * c.r() as f32 + 0.7152 * c.g() as f32 + 0.0722 * c.b() as f32;
        for hovered in [false, true] {
            let (fill, fg) = primary_colors(hovered);
            assert!(lum(fill) - lum(fg) > 100.0, "hovered {hovered}");
        }
        assert!(lum(primary_colors(true).0) > lum(primary_colors(false).0));
    }

    #[test]
    fn dialogs_use_the_panel_tokens() {
        let f = frame();
        assert_eq!(f.fill, theme::PANEL_BG);
        assert_eq!(f.stroke.color, theme::BORDER);
        assert_eq!(f.rounding, Rounding::same(theme::R_XL));
    }
}
