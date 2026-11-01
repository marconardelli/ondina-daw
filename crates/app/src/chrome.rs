//! Panel chrome: the pieces the panels are built from (header strips, caps section labels, editor tabs, status
//! pills, slot buttons that open a menu, the dashed "+ Add" slot, LCD value boxes).

use eframe::egui::{
    self, pos2, vec2, Color32, InnerResponse, Rect, Response, RichText, Rounding, Sense, Shape, Stroke, Ui,
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
        fill: theme::INSTRUMENT_BG,
        border: theme::INSTRUMENT_BORDER,
        text: theme::INSTRUMENT_TEXT,
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
    add_slot_menu_sized(ui, label, ROW_H, add_contents)
}

/// [`add_slot_menu`] `height` tall (the mixer's slots are shorter than the Inspector's rows).
pub fn add_slot_menu_sized<R>(
    ui: &mut Ui,
    label: &str,
    height: f32,
    add_contents: impl FnOnce(&mut Ui) -> R,
) -> InnerResponse<Option<R>> {
    let width = ui.available_width();
    let button = egui::Button::new(RichText::new(label).color(theme::TEXT_3))
        .frame(false)
        .min_size(vec2(width, height));
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
pub fn header_button(ui: &mut Ui, icon: Icon, tip: &str) -> Response {
    let (rect, resp) = ui.allocate_exact_size(vec2(24.0, 22.0), Sense::click());
    if ui.is_rect_visible(rect) {
        let (fill, fg) = if resp.hovered() { (theme::HOVER, theme::TEXT) } else { (theme::CONTROL, theme::TEXT_2) };
        ui.painter().rect_filled(rect, Rounding::same(theme::R_MD), fill);
        widgets::paint_icon(ui.painter(), icon, Rect::from_center_size(rect.center(), vec2(12.0, 12.0)), fg);
    }
    resp.on_hover_text(tip)
}

/// An icon followed by a title, as panel headers start.
pub fn icon_title(ui: &mut Ui, icon: Icon, color: Color32, title: RichText) {
    widgets::icon(ui, icon, 14.0, color);
    ui.label(title);
}

/// What an empty view says: an icon, a short title and a hint on what to do, centered in the space it is given.
pub fn empty_state(ui: &mut Ui, icon: Icon, title: &str, hint: &str) -> Response {
    let rect = ui.available_rect_before_wrap();
    // inside a scroll area the space below is unbounded: center in the part that shows
    let height = rect.intersect(ui.clip_rect()).height().max(EMPTY_STATE_MIN_H);
    let (rect, resp) = ui.allocate_exact_size(vec2(rect.width(), height), Sense::hover());
    paint_empty_state(ui, rect, icon, title, hint);
    resp
}

/// Least height [`empty_state`] takes.
pub const EMPTY_STATE_MIN_H: f32 = 120.0;

/// [`empty_state`] painted into `rect` without taking space (over a canvas such as the arrangement).
pub fn paint_empty_state(ui: &Ui, rect: Rect, icon: Icon, title: &str, hint: &str) {
    if !ui.is_rect_visible(rect) {
        return;
    }
    let p = ui.painter().with_clip_rect(rect);
    let wrap = (rect.width() - 2.0 * theme::SP_6).clamp(80.0, 360.0);
    let title_g = p.layout(title.to_owned(), egui::FontId::new(13.0, theme::semibold()), theme::TEXT_2, wrap);
    let hint_g = p.layout(hint.to_owned(), egui::FontId::proportional(theme::SIZE_NUMBER), theme::TEXT_3, wrap);
    let badge = 40.0;
    let total = badge + theme::SP_5 + title_g.size().y + theme::SP_2 + hint_g.size().y;
    let top = rect.center().y - total / 2.0;
    let badge_rect = Rect::from_center_size(pos2(rect.center().x, top + badge / 2.0), vec2(badge, badge));
    p.rect(badge_rect, Rounding::same(theme::R_XL), theme::RAISED, Stroke::new(1.0_f32, theme::BORDER));
    widgets::paint_icon(&p, icon, badge_rect.shrink(11.0), theme::TEXT_3);
    let y = badge_rect.bottom() + theme::SP_5;
    let title_w = title_g.size().x;
    let title_h = title_g.size().y;
    p.galley(pos2(rect.center().x - title_w / 2.0, y), title_g, theme::TEXT_2);
    let y = y + title_h + theme::SP_2;
    p.galley(pos2(rect.center().x - hint_g.size().x / 2.0, y), hint_g, theme::TEXT_3);
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
