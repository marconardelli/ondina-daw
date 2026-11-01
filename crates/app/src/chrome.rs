//! Panel chrome: the pieces the panels are built from (header strips, caps section labels, editor tabs, status
//! pills, slot buttons that open a menu, the dashed "+ Add" slot, LCD value boxes).

use eframe::egui::{
    self, pos2, vec2, Align2, Color32, InnerResponse, Rect, Response, RichText, Rounding, Sense, Shape, Stroke, Ui,
};

use crate::theme;
use crate::widgets::{self, Icon};

/// Height of a panel's header strip (side panel page, assistant uses [`ASSISTANT_HEADER`]).
pub const HEADER_H: f32 = 28.0;
/// Height of the assistant's header.
pub const ASSISTANT_HEADER: f32 = 40.0;
/// Height of the bottom panel's header with the editor tabs.
pub const EDITOR_HEADER: f32 = 32.0;
/// Height of a slot or a value row in the channel strip.
pub const ROW_H: f32 = 24.0;

/// A strip `height` tall across the panel with a hairline under it; `add` lays out its contents left to right,
/// vertically centered, with the panel's side padding.
pub fn header_strip<R>(ui: &mut Ui, height: f32, add: impl FnOnce(&mut Ui) -> R) -> InnerResponse<R> {
    let width = ui.available_width();
    let (rect, resp) = ui.allocate_exact_size(vec2(width, height), Sense::hover());
    ui.painter().hline(rect.x_range(), rect.bottom() - 0.5, Stroke::new(1.0_f32, theme::CONTROL));
    let inner = rect.shrink2(vec2(theme::SP_5 - 2.0, 0.0));
    let mut child = ui.new_child(
        egui::UiBuilder::new()
            .max_rect(inner)
            .layout(egui::Layout::left_to_right(egui::Align::Center)),
    );
    child.spacing_mut().item_spacing.x = theme::SP_3;
    let r = add(&mut child);
    InnerResponse::new(r, resp)
}

/// A caps section label (INSTRUMENT, MIDI FX, SENDS...).
pub fn section_label(ui: &mut Ui, text: &str) -> Response {
    ui.label(theme::label_text(text))
}

/// An editor tab: raised when selected, plain text otherwise.
pub fn tab(ui: &mut Ui, selected: bool, text: &str) -> Response {
    let font = if selected {
        egui::FontId::new(theme::SIZE_BODY, theme::semibold())
    } else {
        theme::body_font()
    };
    let galley = ui.painter().layout_no_wrap(text.to_owned(), font, theme::TEXT);
    let size = vec2(galley.size().x + 2.0 * 10.0, 24.0);
    let (rect, resp) = ui.allocate_exact_size(size, Sense::click());
    if ui.is_rect_visible(rect) {
        let (fill, color) = if selected {
            (theme::SEGMENT_ON, theme::TEXT)
        } else if resp.hovered() {
            (theme::RAISED, theme::TEXT)
        } else {
            (Color32::TRANSPARENT, theme::TEXT_2)
        };
        ui.painter().rect_filled(rect, Rounding::same(theme::R_MD), fill);
        ui.painter().galley(rect.center() - galley.size() / 2.0, galley, color);
    }
    resp
}

/// A rounded pill with a status dot, as in the assistant's header.
pub fn status_pill(ui: &mut Ui, text: &str, dot: Color32, fill: Color32, fg: Color32) -> Response {
    let font = egui::FontId::proportional(theme::SIZE_NUMBER);
    let galley = ui.painter().layout_no_wrap(text.to_owned(), font, fg);
    let size = vec2(7.0 + 6.0 + 5.0 + galley.size().x + 7.0, 18.0);
    let (rect, resp) = ui.allocate_exact_size(size, Sense::hover());
    if ui.is_rect_visible(rect) {
        let p = ui.painter();
        p.rect_filled(rect, Rounding::same(9.0), fill);
        p.circle_filled(pos2(rect.left() + 10.0, rect.center().y), 3.0, dot);
        p.galley(pos2(rect.left() + 18.0, rect.center().y - galley.size().y / 2.0), galley, fg);
    }
    resp
}

/// A small square of `color` (region color in the editor header).
pub fn color_dot(ui: &mut Ui, color: Color32, size: f32) -> Response {
    let (rect, resp) = ui.allocate_exact_size(egui::Vec2::splat(size), Sense::hover());
    ui.painter().rect_filled(rect, Rounding::same(2.0), color);
    resp
}

/// Colors of a slot button: fill, border, text.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct SlotLook {
    pub fill: Color32,
    pub border: Color32,
    pub text: Color32,
}

impl SlotLook {
    /// A loaded plug-in.
    pub const NORMAL: SlotLook = SlotLook { fill: theme::CONTROL, border: theme::BORDER, text: theme::TEXT };
    /// The instrument slot, in the instrument blue of the design.
    pub const INSTRUMENT: SlotLook = SlotLook {
        fill: Color32::from_rgb(0x23, 0x31, 0x50),
        border: Color32::from_rgb(0x3A, 0x4E, 0x7A),
        text: Color32::from_rgb(0xDC, 0xE6, 0xFF),
    };
    /// A value box (send, output).
    pub const VALUE: SlotLook = SlotLook { fill: theme::CONTROL, border: theme::BORDER, text: theme::TEXT };
}

/// A full-width slot button that opens a menu on click. `text` is cut to the width.
pub fn slot_menu<R>(
    ui: &mut Ui,
    look: SlotLook,
    height: f32,
    text: RichText,
    add_contents: impl FnOnce(&mut Ui) -> R,
) -> InnerResponse<Option<R>> {
    let width = ui.available_width();
    let button = egui::Button::new(text.color(look.text))
        .fill(look.fill)
        .stroke(Stroke::new(1.0_f32, look.border))
        .rounding(Rounding::same(theme::R_MD))
        .truncate()
        .min_size(vec2(width, height));
    // a button lays its text out with the layout's alignment: centered like the design's slots
    ui.with_layout(egui::Layout::top_down(egui::Align::Center), |ui| {
        ui.set_max_width(width);
        egui::menu::menu_custom_button(ui, button, add_contents)
    })
    .inner
}

/// The dashed empty slot ("+ Add") that opens a menu on click.
pub fn add_slot_menu<R>(ui: &mut Ui, label: &str, add_contents: impl FnOnce(&mut Ui) -> R) -> InnerResponse<Option<R>> {
    let width = ui.available_width();
    let button = egui::Button::new(RichText::new(label).color(theme::TEXT_3))
        .frame(false)
        .min_size(vec2(width, ROW_H));
    let ir = ui
        .with_layout(egui::Layout::top_down(egui::Align::Center), |ui| {
            egui::menu::menu_custom_button(ui, button, add_contents)
        })
        .inner;
    paint_dashed(ui, ir.response.rect, ir.response.hovered());
    ir
}

/// A dashed rounded-looking border around `rect`.
pub fn paint_dashed(ui: &Ui, rect: Rect, hot: bool) {
    let color = if hot { theme::BORDER_HOVER } else { theme::TRACK_OFF };
    let r = rect.shrink(0.5);
    let corners = [r.left_top(), r.right_top(), r.right_bottom(), r.left_bottom(), r.left_top()];
    ui.painter().extend(Shape::dashed_line(&corners, Stroke::new(1.0_f32, color), 3.0, 2.0));
}

/// A number in an LCD box (the fader's dB value).
pub fn lcd_value(ui: &mut Ui, text: &str) -> Response {
    let font = theme::number_font();
    let galley = ui.painter().layout_no_wrap(text.to_owned(), font, theme::TEXT);
    let (rect, resp) = ui.allocate_exact_size(vec2(galley.size().x + 12.0, 18.0), Sense::hover());
    ui.painter().rect_filled(rect, Rounding::same(theme::R_SM), theme::LCD);
    ui.painter().galley(rect.center() - galley.size() / 2.0, galley, theme::TEXT);
    resp
}

/// A small square icon button for panel headers (close, options).
pub fn header_button(ui: &mut Ui, text: &str, tip: &str) -> Response {
    let (rect, resp) = ui.allocate_exact_size(vec2(24.0, 22.0), Sense::click());
    if ui.is_rect_visible(rect) {
        let fill = if resp.hovered() { theme::HOVER } else { theme::CONTROL };
        ui.painter().rect_filled(rect, Rounding::same(theme::R_MD), fill);
        ui.painter().text(rect.center(), Align2::CENTER_CENTER, text, theme::body_font(), theme::TEXT_2);
    }
    resp.on_hover_text(tip)
}

/// An icon followed by a title, as panel headers start.
pub fn icon_title(ui: &mut Ui, icon: Icon, color: Color32, title: RichText) {
    widgets::icon(ui, icon, 14.0, color);
    ui.label(title);
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn slot_looks_keep_text_readable_on_their_fill() {
        // contrast by luminance difference: every look has light text on a dark fill
        let lum = |c: Color32| 0.2126 * c.r() as f32 + 0.7152 * c.g() as f32 + 0.0722 * c.b() as f32;
        for look in [SlotLook::NORMAL, SlotLook::INSTRUMENT, SlotLook::VALUE] {
            assert!(lum(look.text) - lum(look.fill) > 120.0, "{look:?}");
        }
    }
}
