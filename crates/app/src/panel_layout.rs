//! The main window's panels: the Inspector / Library on the left, the assistant on the right and the Editor, Mixer
//! or Clip Grid at the bottom. Each one can be resized with the splitter on its edge and closed; the limits below
//! keep the arrangement usable on a 1500 px window, and the sizes the person drags to are kept in the settings.

use eframe::egui::{self, pos2, vec2, Id, LayerId, Order, Rect, Rounding};
use ondina_studio::PanelSizes;

use crate::app::{BottomTab, OndinaApp};
use crate::theme;

pub const LEFT_DEFAULT: f32 = 248.0;
pub const LEFT_MIN: f32 = 220.0;
pub const LEFT_MAX: f32 = 460.0;
pub const ASSISTANT_DEFAULT: f32 = 304.0;
pub const ASSISTANT_MIN: f32 = 260.0;
pub const ASSISTANT_MAX: f32 = 560.0;
/// Width the arrangement always keeps between the two side panels.
pub const CENTER_MIN: f32 = 520.0;
/// Height the arrangement always keeps above the bottom panel.
pub const ARRANGE_MIN_H: f32 = 150.0;
/// The bottom panel never takes more than this share of the space under the control bar.
pub const BOTTOM_MAX_SHARE: f32 = 0.72;
/// Length and thickness of the grip drawn in the middle of each splitter.
pub const GRIP: (f32, f32) = (40.0, 3.0);

const LEFT_ID: &str = "left";
const ASSISTANT_ID: &str = "assistant";
const BOTTOM_ID: &str = "bottom";

/// The smallest height that shows what a bottom panel is for: a useful piano roll, a whole mixer strip, a row of
/// clips.
pub fn bottom_min(tab: BottomTab) -> f32 {
    match tab {
        BottomTab::Editor => 220.0,
        BottomTab::Mixer => 300.0,
        BottomTab::Live => 180.0,
        BottomTab::Hidden => 0.0,
    }
}

/// How the bottom panel's choice is kept in the settings.
pub fn bottom_key(tab: BottomTab) -> &'static str {
    match tab {
        BottomTab::Editor => "editor",
        BottomTab::Mixer => "mixer",
        BottomTab::Live => "clip_grid",
        BottomTab::Hidden => "hidden",
    }
}

/// The bottom panel to open with: the one kept in the settings, the Editor when there is none or it is unknown.
pub fn bottom_from_key(key: Option<&str>) -> BottomTab {
    [BottomTab::Editor, BottomTab::Mixer, BottomTab::Live, BottomTab::Hidden]
        .into_iter()
        .find(|t| Some(bottom_key(*t)) == key)
        .unwrap_or(BottomTab::Editor)
}

/// Width range of a side panel whose own limits are `min..=max`, when `room` points are free for the two side
/// panels and the arrangement and the panel on the other side takes `other`. The arrangement keeps
/// [`CENTER_MIN`]; the panel never goes under its own minimum (egui then gives it what is left).
pub fn side_range(min: f32, max: f32, room: f32, other: f32) -> (f32, f32) {
    let free = room - other - CENTER_MIN;
    (min, max.min(free).max(min))
}

/// Height range of the bottom panel in an area `area_h` tall (everything between the control bar and the status
/// bar).
pub fn bottom_range(tab: BottomTab, area_h: f32) -> (f32, f32) {
    let min = bottom_min(tab);
    let max = (area_h * BOTTOM_MAX_SHARE).min(area_h - ARRANGE_MIN_H);
    (min, max.max(min))
}

/// The height the bottom panel opens with before the person has resized it: about two fifths of the area, so the
/// piano roll shows more than a handful of rows on a 1000 px tall window.
pub fn default_bottom(area_h: f32) -> f32 {
    (area_h * 0.42).clamp(280.0, 520.0)
}

/// The size a panel opens with: the remembered one when it is a real size, otherwise `default`.
pub fn initial(saved: Option<f32>, default: f32) -> f32 {
    saved.filter(|v| v.is_finite() && *v > 0.0).unwrap_or(default)
}

/// `v` inside `range` (NaN becomes the minimum).
pub fn clamp_to(v: f32, range: (f32, f32)) -> f32 {
    if v.is_nan() {
        range.0
    } else {
        v.clamp(range.0, range.1.max(range.0))
    }
}

/// Keeps the size the person dragged a splitter to. Only a drag changes it: a panel squeezed by a smaller window
/// opens again at the remembered size when there is room. Returns whether `saved` changed.
pub fn remember(saved: &mut Option<f32>, shown: f32, dragging: bool) -> bool {
    if !dragging || !shown.is_finite() || shown <= 0.0 {
        return false;
    }
    let rounded = shown.round();
    if *saved == Some(rounded) {
        return false;
    }
    *saved = Some(rounded);
    true
}

/// Whether the splitter of panel `id` is under the pointer or being dragged, and whether it is being dragged.
fn splitter_state(ctx: &egui::Context, id: Id) -> (bool, bool) {
    ctx.read_response(id.with("__resize"))
        .map(|r| (r.hovered() || r.dragged(), r.dragged()))
        .unwrap_or((false, false))
}

/// Draws the grip in the middle of a splitter: vertical on a side panel's edge at `x`, horizontal on the bottom
/// panel's top edge at `y`.
fn paint_grip(ctx: &egui::Context, edge: Edge, panel: Rect, hot: bool) {
    let painter = ctx.layer_painter(LayerId::new(Order::PanelResizeLine, Id::new("splitter-grips")));
    let color = if hot { theme::ACCENT } else { theme::TEXT_DISABLED };
    let (len, thick) = GRIP;
    let r = match edge {
        Edge::Right => Rect::from_center_size(pos2(panel.right() - 1.0, panel.center().y), vec2(thick, len)),
        Edge::Left => Rect::from_center_size(pos2(panel.left() + 1.0, panel.center().y), vec2(thick, len)),
        Edge::Top => Rect::from_center_size(pos2(panel.center().x, panel.top() + 1.0), vec2(len, thick)),
    };
    painter.rect_filled(r, Rounding::same(1.5), color);
}

#[derive(Clone, Copy)]
enum Edge {
    Left,
    Right,
    Top,
}

fn panel_frame() -> egui::Frame {
    egui::Frame::none().fill(theme::PANEL)
}

impl OndinaApp {
    /// Shows the assistant, the side panel and the bottom panel (in that order, so the bottom panel spans only the
    /// arrangement's width), each sized within its limits, and remembers the sizes dragged to.
    pub(crate) fn show_panels(&mut self, ctx: &egui::Context) {
        let area = ctx.available_rect();
        let left_now = if self.show_left {
            egui::panel::PanelState::load(ctx, Id::new(LEFT_ID))
                .map(|s| s.rect.width())
                .unwrap_or_else(|| initial(self.settings.panels.left, LEFT_DEFAULT))
        } else {
            0.0
        };
        let mut sizes: PanelSizes = self.settings.panels.clone();
        let mut assistant_now = 0.0;
        if self.show_assistant {
            let range = side_range(ASSISTANT_MIN, ASSISTANT_MAX, area.width(), left_now);
            let start = clamp_to(initial(sizes.assistant, ASSISTANT_DEFAULT), range);
            let resp = egui::SidePanel::right(ASSISTANT_ID)
                .resizable(true)
                .frame(panel_frame())
                .default_width(start)
                .width_range(range.0..=range.1)
                .show(ctx, |ui| self.assistant_ui(ui));
            let (hot, dragging) = splitter_state(ctx, Id::new(ASSISTANT_ID));
            paint_grip(ctx, Edge::Left, resp.response.rect, hot);
            assistant_now = resp.response.rect.width();
            remember(&mut sizes.assistant, assistant_now, dragging);
        }
        if self.show_left {
            let range = side_range(LEFT_MIN, LEFT_MAX, area.width(), assistant_now);
            let start = clamp_to(initial(sizes.left, LEFT_DEFAULT), range);
            let resp = egui::SidePanel::left(LEFT_ID)
                .resizable(true)
                .frame(panel_frame())
                .default_width(start)
                .width_range(range.0..=range.1)
                .show(ctx, |ui| self.left_ui(ui));
            let (hot, dragging) = splitter_state(ctx, Id::new(LEFT_ID));
            paint_grip(ctx, Edge::Right, resp.response.rect, hot);
            remember(&mut sizes.left, resp.response.rect.width(), dragging);
        }
        if self.bottom != BottomTab::Hidden {
            let range = bottom_range(self.bottom, area.height());
            let start = clamp_to(initial(sizes.bottom, default_bottom(area.height())), range);
            let resp = egui::TopBottomPanel::bottom(BOTTOM_ID)
                .resizable(true)
                .frame(panel_frame())
                .default_height(start)
                .height_range(range.0..=range.1)
                .show(ctx, |ui| match self.bottom {
                    BottomTab::Editor => self.editor_ui(ui),
                    BottomTab::Mixer => self.mixer_ui(ui),
                    BottomTab::Live => {
                        egui::Frame::none().inner_margin(theme::SP_4).show(ui, |ui| self.live_ui(ui));
                    }
                    BottomTab::Hidden => {}
                });
            let (hot, dragging) = splitter_state(ctx, Id::new(BOTTOM_ID));
            paint_grip(ctx, Edge::Top, resp.response.rect, hot);
            remember(&mut sizes.bottom, resp.response.rect.height(), dragging);
        }
        sizes.left_open = Some(self.show_left);
        sizes.assistant_open = Some(self.show_assistant);
        sizes.bottom_panel = Some(bottom_key(self.bottom).to_string());
        self.settings.panels = sizes;
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn side_panels_leave_the_arrangement_its_minimum_width() {
        // 1500 px window, assistant at its default: the side panel can grow until the arrangement has CENTER_MIN
        let (lo, hi) = side_range(LEFT_MIN, LEFT_MAX, 1500.0, ASSISTANT_DEFAULT);
        assert_eq!(lo, LEFT_MIN);
        assert_eq!(hi, LEFT_MAX.min(1500.0 - ASSISTANT_DEFAULT - CENTER_MIN));
        assert!(1500.0 - hi - ASSISTANT_DEFAULT >= CENTER_MIN);
        // a wide window: the panel's own maximum wins
        assert_eq!(side_range(LEFT_MIN, LEFT_MAX, 2560.0, ASSISTANT_DEFAULT).1, LEFT_MAX);
        // no room at all: never under the minimum
        assert_eq!(side_range(LEFT_MIN, LEFT_MAX, 900.0, ASSISTANT_MAX), (LEFT_MIN, LEFT_MIN));
        // the other panel closed gives the room back
        assert!(side_range(ASSISTANT_MIN, ASSISTANT_MAX, 1200.0, 0.0).1 > side_range(ASSISTANT_MIN, ASSISTANT_MAX, 1200.0, 300.0).1);
    }

    #[test]
    fn defaults_fit_a_1500_px_window_with_room_for_the_arrangement() {
        let room = 1500.0;
        let right = clamp_to(ASSISTANT_DEFAULT, side_range(ASSISTANT_MIN, ASSISTANT_MAX, room, LEFT_DEFAULT));
        let left = clamp_to(LEFT_DEFAULT, side_range(LEFT_MIN, LEFT_MAX, room, right));
        assert_eq!((left, right), (LEFT_DEFAULT, ASSISTANT_DEFAULT));
        assert!(room - left - right >= 900.0, "arrangement gets {}", room - left - right);
    }

    #[test]
    fn bottom_panel_height_keeps_the_arrangement_visible() {
        let (lo, hi) = bottom_range(BottomTab::Editor, 840.0);
        assert_eq!(lo, bottom_min(BottomTab::Editor));
        assert!(840.0 - hi >= ARRANGE_MIN_H);
        assert!(hi <= 840.0 * BOTTOM_MAX_SHARE + 0.01);
        // the mixer needs a whole strip
        assert!(bottom_range(BottomTab::Mixer, 840.0).0 > bottom_range(BottomTab::Live, 840.0).0);
        // a tiny area: the minimum still holds
        assert_eq!(bottom_range(BottomTab::Mixer, 300.0), (300.0, 300.0));
    }

    #[test]
    fn bottom_panel_opens_taller_than_before_on_a_common_window() {
        // the old default was 300 px whatever the window; 1000 px tall leaves ~840 under the control bar
        assert!(default_bottom(840.0) > 340.0);
        assert_eq!(default_bottom(400.0), 280.0);
        assert_eq!(default_bottom(2000.0), 520.0);
        let h = clamp_to(default_bottom(840.0), bottom_range(BottomTab::Editor, 840.0));
        assert!(h >= bottom_min(BottomTab::Editor));
    }

    #[test]
    fn remembered_sizes_are_used_only_when_they_are_real() {
        assert_eq!(initial(Some(300.0), 248.0), 300.0);
        assert_eq!(initial(None, 248.0), 248.0);
        assert_eq!(initial(Some(f32::NAN), 248.0), 248.0);
        assert_eq!(initial(Some(-5.0), 248.0), 248.0);
        assert_eq!(initial(Some(f32::INFINITY), 248.0), 248.0);
        assert_eq!(clamp_to(1000.0, (220.0, 460.0)), 460.0);
        assert_eq!(clamp_to(10.0, (220.0, 460.0)), 220.0);
        assert_eq!(clamp_to(f32::NAN, (220.0, 460.0)), 220.0);
        assert_eq!(clamp_to(300.0, (220.0, 100.0)), 220.0);
    }

    #[test]
    fn the_bottom_panel_choice_round_trips_through_the_settings() {
        for t in [BottomTab::Editor, BottomTab::Mixer, BottomTab::Live, BottomTab::Hidden] {
            assert_eq!(bottom_from_key(Some(bottom_key(t))), t);
        }
        assert_eq!(bottom_from_key(None), BottomTab::Editor);
        assert_eq!(bottom_from_key(Some("live_loops")), BottomTab::Editor);
    }

    #[test]
    fn only_a_drag_changes_the_remembered_size() {
        let mut saved = Some(300.0);
        // squeezed by a smaller window: the preference stays
        assert!(!remember(&mut saved, 240.0, false));
        assert_eq!(saved, Some(300.0));
        assert!(remember(&mut saved, 352.4, true));
        assert_eq!(saved, Some(352.0));
        assert!(!remember(&mut saved, 352.2, true));
        assert!(!remember(&mut saved, f32::NAN, true));
        let mut none = None;
        assert!(remember(&mut none, 260.0, true));
        assert_eq!(none, Some(260.0));
    }
}
