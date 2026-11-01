//! Tracks area: headers, ruler, marker & chord lanes, regions, playhead.

use std::borrow::Cow;

use ondina_core::command::Command;
use ondina_core::model::*;
use ondina_core::ops::{pattern_to_notes, resolve_chord_symbol};
use ondina_core::theory::Key;
use eframe::egui::{
    self, pos2, vec2, Align2, Color32, FontId, Pos2, Rect, Rounding, Sense, Stroke,
};

use crate::app::{OndinaApp, BottomTab, Drag};
use crate::automation_lane;
use crate::arrange_view::{self, apply_step, max_scroll_y, zoom_track_height, ArrangeView, ViewArea};
use crate::piano_roll_view::{wheel_zoom, zoom_factor};
use crate::region_edit::{self, region_name_field, FadeGeom, FadeHandle};
use crate::session_players::{self, Player};
use crate::theme;
use crate::widgets;

const HEADER_W: f32 = 224.0;
const RULER_H: f32 = 30.0;
/// Height of the cycle strip at the top of the ruler; the bar numbers and the playhead use the rest.
const CYCLE_STRIP_H: f32 = 11.0;
/// Grab distance around a cycle edge on the strip. Inside a narrow cycle it shrinks to a third of its width, so
/// there is always some body left to grab for moving.
const CYCLE_EDGE_PX: f32 = 6.0;
/// While a cycle drag rests, its range is sent again this often, so the whole drag stays one undo step (the
/// session merges steps with the same key only within 1.5 s).
const CYCLE_RESEND_S: f64 = 0.5;
const CYCLE_STRIP_HINT: &str = "Cycle strip: drag the cycle to move it, drag an edge to resize it, click it to turn cycle on or off, drag on empty space to draw a new cycle. Shift+drag anywhere on the ruler also draws one.";
const LANE_H: f32 = 20.0;
/// Width of the grab strip on a region's right edge. Narrow regions get a third of their width
/// instead, so there is always some body left to grab for moving.
const REGION_EDGE_PX: f32 = 6.0;
/// Below this track height the M/S/R buttons move onto the name row and the volume slider is hidden
/// (the mixer still has it).
const HEADER_FULL_H: f32 = 46.0;
const HEADER_BUTTON_H: f32 = 18.0;
const HEADER_BUTTON_W: f32 = 22.0;
const HEADER_BUTTON_GAP: f32 = 3.0;
/// Height of the name row of a track header, and the gap between it and the button row.
const HEADER_NAME_H: f32 = 16.0;
const HEADER_ROW_GAP: f32 = 6.0;
/// Width of the color bar on the left of a track header and of the meter column on its right.
const HEADER_COLOR_W: f32 = 4.0;
const HEADER_METER_W: f32 = 10.0;
/// Width kept for the volume value right of the slider.
const HEADER_VALUE_W: f32 = 30.0;
/// Lowest level the header volume slider sends; its left end.
const HEADER_VOLUME_MIN_DB: f32 = -96.0;
/// A region's rect leaves this much of its lane free above and below.
const REGION_INSET_Y: f32 = 2.0;
/// Height of the colored strip with the region name. With the inset it ends where the automation overlay begins.
const REGION_HEAD_H: f32 = 14.0;

/// A marker, track or region name being edited in place.
pub struct InPlaceRename {
    pub id: Id,
    pub text: String,
    /// Give the text field focus and select its text on the next frame it is shown.
    pub focus: bool,
}

/// A chord being typed in the chord lane.
pub struct ChordEdit {
    pub target: ChordTarget,
    pub text: String,
    /// Give the text field focus and select its text on the next frame it is shown.
    pub focus: bool,
    /// The last Enter found no chord in the text: the field stays open with a red frame.
    pub invalid: bool,
}

/// What a chord edit changes: a new chord over `beat..beat + length`, or an existing chord.
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum ChordTarget {
    New { beat: f64, length: f64 },
    Existing(Id),
}

/// The text fields open in place over the arrangement, in the inspector and in the step sequencer. They belong to the document they were
/// opened on: ids start again from 1 in a new project, so a field left open would edit a different object.
#[derive(Default)]
pub struct InPlaceEdits {
    pub marker_rename: Option<InPlaceRename>,
    pub track_rename: Option<InPlaceRename>,
    pub region_rename: Option<InPlaceRename>,
    /// The selected region's name as typed in the inspector, with the region it was typed for.
    pub region_name_edit: Option<(Id, String)>,
    pub chord_edit: Option<ChordEdit>,
    pub row_rename: Option<crate::step_paint::RowRename>,
}

impl InPlaceEdits {
    /// Closes every field without applying what was typed.
    pub fn clear(&mut self) {
        *self = InPlaceEdits::default();
    }

    /// Closes every field when the project shown (`shown`, a `Session::document`) was replaced by another one
    /// (`fresh`), whoever opened or created it.
    pub fn follow_document(&mut self, shown: u64, fresh: u64) {
        if shown != fresh {
            self.clear();
        }
    }
}

impl OndinaApp {
    pub fn beat_to_x(&self, beat: f64, origin: f32) -> f32 {
        origin + ((beat - self.scroll_beat) as f32) * self.px_per_beat
    }

    pub fn x_to_beat(&self, x: f32, origin: f32) -> f64 {
        self.scroll_beat + ((x - origin) / self.px_per_beat) as f64
    }

    /// The arrangement's scroll and zoom, as the View menu and its geometry functions see them.
    pub fn arrange_view(&self) -> ArrangeView {
        ArrangeView {
            scroll_beat: self.scroll_beat,
            px_per_beat: self.px_per_beat,
            scroll_y: self.scroll_y,
            track_height: self.track_height,
        }
    }

    fn set_arrange_view(&mut self, v: ArrangeView) {
        self.scroll_beat = v.scroll_beat;
        self.px_per_beat = v.px_per_beat;
        self.scroll_y = v.scroll_y;
        self.track_height = v.track_height;
    }

    pub fn arrange_ui(&mut self, ui: &mut egui::Ui) {
        let p = self.project();
        let full = ui.available_rect_before_wrap();
        let bpb = p.time_signature.beats_per_bar();
        let timeline_x = full.left() + HEADER_W;
        let top = full.top();
        let tracks_top = top + RULER_H + LANE_H * 2.0;
        let painter = ui.painter_at(full);

        let view_w = (full.right() - timeline_x).max(1.0);
        let view_h = (full.bottom() - tracks_top).max(1.0);
        // zoom / scroll input
        if ui.rect_contains_pointer(full) {
            let line_px = ui.ctx().options(|o| o.line_scroll_speed);
            let (scroll, hover, zoom) = ui.input(|i| {
                (
                    i.smooth_scroll_delta,
                    i.pointer.hover_pos(),
                    wheel_zoom(&i.events, line_px, i.screen_rect().height()),
                )
            });
            let hover = hover.unwrap_or(full.center());
            let dx = (hover.x - timeline_x).clamp(0.0, view_w);
            let dy = (hover.y - tracks_top).clamp(0.0, view_h);
            // egui 0.29 gives Ctrl+wheel only to its zoom factor: read the wheel events instead.
            if zoom.time != 0.0 {
                (self.scroll_beat, self.px_per_beat) =
                    arrange_view::zoom_time(self.scroll_beat, self.px_per_beat, dx, zoom_factor(zoom.time));
            }
            // Ctrl+Shift+wheel: the piano roll's vertical zoom is the track height here.
            if zoom.pitch != 0.0 {
                (self.scroll_y, self.track_height) = zoom_track_height(
                    self.scroll_y, self.track_height, dy, zoom_factor(zoom.pitch), p.tracks.len(), view_h,
                );
            }
            // egui already turns Shift+wheel into horizontal scrolling.
            if scroll.x != 0.0 {
                self.scroll_beat = (self.scroll_beat - (scroll.x / self.px_per_beat) as f64).max(0.0);
            }
            if scroll.y != 0.0 {
                self.scroll_y -= scroll.y;
            }
        }
        if let Some(step) = self.arrange_step.take() {
            let area = ViewArea {
                width: view_w,
                height: view_h,
                tracks: p.tracks.len(),
                content_end: p.content_end(),
                beats_per_bar: bpb,
                playhead: self.position(),
            };
            self.set_arrange_view(apply_step(step, self.arrange_view(), &area));
        }
        if let Some(id) = self.reveal_track.take() {
            if let Some(i) = p.tracks.iter().position(|t| t.id == id) {
                self.scroll_y = arrange_view::scroll_to_track(self.scroll_y, self.track_height, i, view_h);
            }
        }
        // Also after tracks were deleted or made shorter elsewhere: never leave the view below the content.
        self.scroll_y = self.scroll_y.clamp(0.0, max_scroll_y(p.tracks.len(), self.track_height, view_h));
        // follow playhead
        let pos = self.position();
        let jumped = (pos - self.last_playhead).abs() > 0.5 && !self.playing();
        if (self.follow_playhead && self.playing()) || jumped {
            let px = self.beat_to_x(pos, timeline_x);
            if px > full.right() - 40.0 || px < timeline_x {
                self.scroll_beat = (pos - 2.0).max(0.0);
            }
        }
        self.last_playhead = pos;

        painter.rect_filled(full, Rounding::ZERO, theme::BG);

        // ---------------- ruler: the cycle strip on top, bar numbers and the playhead below
        let ruler = Rect::from_min_max(pos2(timeline_x, top), pos2(full.right(), top + RULER_H));
        let strip = Rect::from_min_max(ruler.min, pos2(ruler.right(), top + CYCLE_STRIP_H));
        let numbers = Rect::from_min_max(pos2(timeline_x, strip.bottom()), ruler.max);
        painter.rect_filled(ruler, Rounding::ZERO, theme::PANEL_BG);
        painter.rect_filled(strip, Rounding::ZERO, theme::CHROME);
        let visible_beats = ((full.right() - timeline_x) / self.px_per_beat) as f64;
        let bar_px = bpb as f32 * self.px_per_beat;
        let label_every = bar_label_every(bar_px);
        let first_bar = (self.scroll_beat / bpb).floor() as i64;
        let last_bar = ((self.scroll_beat + visible_beats) / bpb).ceil() as i64;
        // cycle band: amber in the strip with both edges marked, a faint tint behind the bar numbers
        let has_cycle = p.cycle.end > p.cycle.start;
        let cx0 = self.beat_to_x(p.cycle.start, timeline_x);
        let cx1 = self.beat_to_x(p.cycle.end, timeline_x);
        if has_cycle && cx1 > timeline_x {
            let x0 = cx0.max(timeline_x);
            let (col, edge_col) = if p.cycle.enabled {
                (theme::CYCLE, theme::SOLO)
            } else {
                (Color32::from_rgba_premultiplied(60, 62, 66, 90), theme::TEXT_3)
            };
            painter.rect_filled(
                Rect::from_min_max(pos2(x0, strip.top() + 2.0), pos2(cx1, strip.bottom() - 2.0)),
                Rounding::same(2.0_f32),
                col,
            );
            painter.rect_filled(
                Rect::from_min_max(pos2(x0, numbers.top()), pos2(cx1, numbers.bottom())),
                Rounding::ZERO,
                col.gamma_multiply(0.22),
            );
            for x in [cx0, cx1] {
                if x >= timeline_x {
                    painter.line_segment(
                        [pos2(x, strip.top() + 1.0), pos2(x, strip.bottom() - 1.0)],
                        Stroke::new(2.0_f32, edge_col),
                    );
                }
            }
        }
        // beat ticks when there is room for them, then the bar ticks with their numbers
        if self.px_per_beat >= 12.0 {
            let mut b = (self.scroll_beat).floor();
            while b < self.scroll_beat + visible_beats {
                let x = self.beat_to_x(b, timeline_x);
                if x >= timeline_x && (b / bpb).fract().abs() > 1e-6 {
                    painter.line_segment(
                        [pos2(x, numbers.bottom() - 5.0), pos2(x, numbers.bottom())],
                        Stroke::new(1.0_f32, theme::RULER_BEAT_TICK),
                    );
                }
                b += 1.0;
            }
        }
        for bar in first_bar..=last_bar {
            let x = self.beat_to_x(bar as f64 * bpb, timeline_x);
            if x < timeline_x {
                continue;
            }
            let labelled = bar % label_every == 0;
            let tick_top = if labelled { numbers.top() + 1.0 } else { numbers.bottom() - 7.0 };
            painter.line_segment([pos2(x, tick_top), pos2(x, numbers.bottom())], Stroke::new(1.0_f32, theme::RULER_TICK));
            if labelled {
                painter.text(
                    pos2(x + 4.0, numbers.center().y),
                    Align2::LEFT_CENTER,
                    format!("{}", bar + 1),
                    theme::number_font(),
                    theme::TEXT_2,
                );
            }
        }
        painter.line_segment(
            [pos2(timeline_x, ruler.bottom() - 0.5), pos2(full.right(), ruler.bottom() - 0.5)],
            Stroke::new(1.0_f32, theme::HAIRLINE),
        );
        let ruler_resp = ui.interact(ruler, ui.id().with("ruler"), Sense::click_and_drag());
        let shift = ui.input(|i| i.modifiers.shift);
        let zone_at = |x: f32| if has_cycle { cycle_hit_zone(x, cx0, cx1) } else { CycleZone::Outside };
        let on_strip = |pt: Pos2| pt.y < strip.bottom();
        let strip_hover = ruler_resp.hover_pos().filter(|h| on_strip(*h));
        if self.drag.is_none() {
            match strip_hover.map(|h| zone_at(h.x)) {
                Some(CycleZone::Body) => ui.ctx().set_cursor_icon(egui::CursorIcon::Grab),
                Some(CycleZone::Start | CycleZone::End) => {
                    ui.ctx().set_cursor_icon(egui::CursorIcon::ResizeHorizontal)
                }
                _ => {}
            }
        }
        let ruler_resp = if strip_hover.is_some() { ruler_resp.on_hover_text(CYCLE_STRIP_HINT) } else { ruler_resp };
        if ruler_resp.drag_started() {
            // Decide from where the press began: the drag starts a few pixels later.
            let origin = ui.input(|i| i.pointer.press_origin()).or_else(|| ruler_resp.interact_pointer_pos());
            if let Some(o) = origin {
                let gesture = next_cycle_gesture();
                match ruler_drag_action(on_strip(o), shift, zone_at(o.x)) {
                    RulerDrag::DrawCycle => {
                        let anchor = self.snap_beat(self.x_to_beat(o.x, timeline_x).max(0.0));
                        self.drag = Some(Drag::Cycle { anchor, gesture, orig: p.cycle, sent: false });
                    }
                    RulerDrag::EditCycle(zone) => {
                        self.drag = Some(Drag::CycleEdit {
                            zone,
                            grab_beat: self.x_to_beat(o.x, timeline_x),
                            start: p.cycle.start,
                            end: p.cycle.end,
                            enabled: p.cycle.enabled,
                            gesture,
                            sent: None,
                            sent_at: 0.0,
                        });
                    }
                    RulerDrag::Playhead => {}
                }
            }
        }
        if !matches!(self.drag, Some(Drag::Cycle { .. } | Drag::CycleEdit { .. })) {
            if let Some(pp) = ruler_resp.interact_pointer_pos() {
                let b = self.snap_beat(self.x_to_beat(pp.x, timeline_x).max(0.0));
                if ruler_resp.clicked() {
                    match ruler_click_action(on_strip(pp), zone_at(pp.x)) {
                        RulerClick::ToggleCycle => self.cmd(cycle_toggle(p.cycle)),
                        RulerClick::Playhead => {
                            self.exec(Command::SetPlayhead {
                                beat: Some(b),
                                bar: None,
                            });
                        }
                    }
                } else if ruler_resp.dragged() && !shift {
                    self.exec(Command::SetPlayhead {
                        beat: Some(b),
                        bar: None,
                    });
                }
            }
        }
        // Cycle drags: every frame of a gesture goes to the same undo step.
        let pointer_x = ui.input(|i| i.pointer.interact_pos().filter(|_| i.pointer.primary_down()).map(|pp| pp.x));
        let now = ui.input(|i| i.time);
        match self.drag {
            Some(Drag::Cycle { anchor, gesture, .. }) => {
                if let Some(x) = pointer_x {
                    let b = self.snap_beat(self.x_to_beat(x, timeline_x).max(0.0));
                    let (s, e) = if b < anchor { (b, anchor) } else { (anchor, b) };
                    if e - s >= MIN_CYCLE_LEN {
                        self.cmd_merge(
                            Command::SetCycle {
                                enabled: true,
                                start: Some(s),
                                end: Some(e),
                            },
                            cycle_gesture_key(gesture),
                        );
                        if let Some(Drag::Cycle { sent, .. }) = &mut self.drag {
                            *sent = true;
                        }
                    }
                }
            }
            Some(Drag::CycleEdit { zone, grab_beat, start, end, enabled, gesture, sent, sent_at }) => {
                ui.ctx().set_cursor_icon(if zone == CycleZone::Body {
                    egui::CursorIcon::Grabbing
                } else {
                    egui::CursorIcon::ResizeHorizontal
                });
                if let Some(x) = pointer_x {
                    let b = self.x_to_beat(x, timeline_x);
                    let span = match zone {
                        CycleZone::Body => moved_cycle(start, end, grab_beat, b, self.snap),
                        _ => resized_cycle(start, end, zone, b, self.snap),
                    };
                    let last = sent.unwrap_or((start, end));
                    let changed = (span.0 - last.0).abs() > 1e-9 || (span.1 - last.1).abs() > 1e-9;
                    if changed || (sent.is_some() && now - sent_at >= CYCLE_RESEND_S) {
                        self.cmd_merge(
                            Command::SetCycle {
                                enabled,
                                start: Some(span.0),
                                end: Some(span.1),
                            },
                            cycle_gesture_key(gesture),
                        );
                        if let Some(Drag::CycleEdit { sent, sent_at, .. }) = &mut self.drag {
                            *sent = Some(span);
                            *sent_at = now;
                        }
                    }
                }
                // keep frames coming while the mouse rests, for the resend above
                ui.ctx().request_repaint_after(std::time::Duration::from_secs_f64(CYCLE_RESEND_S / 2.0));
            }
            _ => {}
        }
        if matches!(self.drag, Some(Drag::Cycle { .. } | Drag::CycleEdit { .. }))
            && ui.input(|i| crate::drag_cancel::drag_finished(&i.pointer, self.drag.as_ref()))
        {
            self.drag = None;
        }

        // ---------------- marker lane
        let mlane = Rect::from_min_max(
            pos2(timeline_x, top + RULER_H),
            pos2(full.right(), top + RULER_H + LANE_H),
        );
        painter.rect_filled(mlane, Rounding::ZERO, theme::GROUND);
        painter.line_segment(
            [pos2(timeline_x, mlane.bottom() - 0.5), pos2(full.right(), mlane.bottom() - 0.5)],
            Stroke::new(1.0_f32, theme::LANE_LINE),
        );
        // Background first: markers registered after it sit on top and get clicks, drags and menus.
        let mresp = ui.interact(mlane, ui.id().with("mlane"), Sense::click());
        if mresp.double_clicked() {
            if let Some(pp) = mresp.interact_pointer_pos() {
                let b = (self.x_to_beat(pp.x, timeline_x) / bpb).floor() * bpb;
                let n = p.markers.len() + 1;
                self.cmd(Command::AddMarker {
                    beat: b.max(0.0),
                    name: format!("Section {n}"),
                    length: Some(bpb * 4.0),
                });
            }
        }
        let mut rename_rect: Option<Rect> = None;
        for (mi, m) in p.markers.iter().enumerate() {
            // a marker being dragged is drawn where it will land
            let (beat, len, ghost) = match &self.drag {
                Some(Drag::MoveMarker { id, cur_beat, .. }) if *id == m.id => {
                    (*cur_beat, m.length, true)
                }
                Some(Drag::ResizeMarker { id, cur_len, .. }) if *id == m.id => {
                    (m.beat, Some(*cur_len), true)
                }
                _ => (m.beat, m.length, false),
            };
            let x = self.beat_to_x(beat, timeline_x);
            let w = len.map(|l| l as f32 * self.px_per_beat).unwrap_or(60.0);
            if x + w < timeline_x || x > full.right() {
                continue;
            }
            let r = Rect::from_min_size(
                pos2(x.max(timeline_x), mlane.top() + 1.0),
                vec2((w - (timeline_x - x).max(0.0)).max(2.0), LANE_H - 2.0),
            );
            let (fill, ink) = marker_chip_colors(mi);
            let chip = chip_rect(r);
            painter.rect_filled(chip, Rounding::same(3.0_f32), fill);
            if m.length.is_none() {
                // a point marker: a flag line on its beat
                painter.line_segment([pos2(chip.left() + 0.5, chip.top()), pos2(chip.left() + 0.5, chip.bottom())], Stroke::new(1.0_f32, ink));
            }
            if ghost {
                painter.rect_stroke(chip, Rounding::same(3.0_f32), Stroke::new(1.5_f32, theme::SELECT));
            }
            if self.edits.marker_rename.as_ref().is_some_and(|e| e.id == m.id) {
                rename_rect = Some(r);
            } else {
                painter.with_clip_rect(chip.shrink2(vec2(2.0, 0.0)).intersect(painter.clip_rect())).text(
                    pos2(chip.left() + 6.0, chip.center().y),
                    Align2::LEFT_CENTER,
                    &m.name,
                    FontId::proportional(11.0),
                    ink,
                );
            }
            if ghost {
                // like regions: no interaction while it is being dragged
                continue;
            }
            let resp = ui.interact(r, ui.id().with(("marker", m.id)), Sense::click_and_drag());
            // Same grab strip as regions; point markers have no length to resize.
            let edge = m.length.map(|_| {
                let er = Rect::from_min_max(pos2(r.right() - region_edge_width(r.width()), r.top()), r.max);
                ui.interact(er, ui.id().with(("medge", m.id)), Sense::drag())
            });
            if self.drag.is_none() && edge.as_ref().is_some_and(|e| e.hovered()) {
                ui.ctx().set_cursor_icon(egui::CursorIcon::ResizeHorizontal);
            }
            // Decide from where the press began, as for regions.
            let press_origin = ui.input(|i| i.pointer.press_origin());
            let press_zone = match (m.length, press_origin) {
                (Some(_), Some(o)) => region_hit_zone(o.x, r.left(), r.right()),
                _ => RegionZone::Body,
            };
            let start_resize = m.length.is_some()
                && (edge.as_ref().is_some_and(|e| e.drag_started())
                    || (resp.drag_started() && press_zone == RegionZone::Edge));
            let start_move = resp.drag_started() && !start_resize;
            if start_resize {
                let l = m.length.unwrap_or(MIN_MARKER_LEN);
                self.drag = Some(Drag::ResizeMarker {
                    id: m.id,
                    start: m.beat,
                    orig_len: l,
                    cur_len: l,
                });
            } else if start_move {
                if let Some(pp) = press_origin.or_else(|| resp.interact_pointer_pos()) {
                    self.drag = Some(Drag::MoveMarker {
                        id: m.id,
                        grab_beat: self.x_to_beat(pp.x, timeline_x),
                        orig_beat: m.beat,
                        cur_beat: m.beat,
                    });
                }
            }
            if resp.clicked() {
                self.exec(Command::SetPlayhead {
                    beat: Some(m.beat),
                    bar: None,
                });
            }
            if resp.double_clicked() {
                self.edits.marker_rename = Some(InPlaceRename {
                    id: m.id,
                    text: m.name.clone(),
                    focus: true,
                });
                // open the field on this frame: rename_rect was decided before the double-click
                rename_rect = Some(r);
            }
            let mid = m.id;
            let (mb, ml) = (m.beat, m.length);
            crate::drag_cancel::menu_unless_dragging(&resp, self.drag.is_some(), |ui| {
                if let Some(l) = ml {
                    if ui.button("Set cycle to section").clicked() {
                        self.cmd(Command::SetCycle {
                            enabled: true,
                            start: Some(mb),
                            end: Some(mb + l),
                        });
                        ui.close_menu();
                    }
                    if ui.button("Repeat section after itself").clicked() {
                        self.cmd(Command::CopySection {
                            from_beat: mb,
                            length: l,
                            to_beat: mb + l,
                            insert: Some(true),
                        });
                        ui.close_menu();
                    }
                }
                if ui.button("Delete marker").clicked() {
                    self.cmd(Command::RemoveMarker { marker_id: mid });
                    ui.close_menu();
                }
            });
        }
        // Rename field, registered last so it sits on top of the lane. A marker that scrolled out of
        // view, was deleted or undone away drops the edit.
        let target = self.edits.marker_rename.take().and_then(|ren| {
            let old = p.markers.iter().find(|m| m.id == ren.id)?.name.clone();
            Some((ren, old, rename_rect?))
        });
        if let Some((mut ren, old, r)) = target {
            // computed first: the call below borrows ui mutably
            let edit_id = ui.id().with(("mrename", ren.id));
            match rename_field(ui, edit_id, r, FontId::proportional(11.0), &mut ren.text, &mut ren.focus) {
                RenameEdit::Editing => self.edits.marker_rename = Some(ren),
                RenameEdit::Commit => {
                    if let Some(name) = rename_commit(&old, &ren.text) {
                        self.cmd(Command::UpdateMarker {
                            marker_id: ren.id,
                            name: Some(name),
                            beat: None,
                            length: None,
                        });
                    }
                }
                RenameEdit::Cancel => {}
            }
        }

        // ---------------- chord lane
        let clane = Rect::from_min_max(
            pos2(timeline_x, top + RULER_H + LANE_H),
            pos2(full.right(), tracks_top),
        );
        let cgeom = ChordLaneGeom {
            timeline_x,
            right: full.right(),
            top: clane.top(),
            scroll_beat: self.scroll_beat,
            px_per_beat: self.px_per_beat,
        };
        painter.rect_filled(clane, Rounding::ZERO, theme::GROUND);
        painter.line_segment(
            [pos2(timeline_x, clane.bottom() - 0.5), pos2(full.right(), clane.bottom() - 0.5)],
            Stroke::new(1.0_f32, theme::HAIRLINE),
        );
        // Background first: chords registered after it sit on top and get clicks, drags and menus.
        let cresp = ui.interact(clane, ui.id().with("clane"), Sense::click());
        if cresp.double_clicked() {
            if let Some(pp) = cresp.interact_pointer_pos() {
                let pb = self.x_to_beat(pp.x, timeline_x);
                if let Some((beat, length)) = new_chord_span(pb, self.snap, bpb, &p.chords) {
                    self.edits.chord_edit = Some(ChordEdit {
                        target: ChordTarget::New { beat, length },
                        text: String::new(),
                        focus: true,
                        invalid: false,
                    });
                }
            }
        }
        for c in &p.chords {
            // a chord being dragged is drawn where it will land
            let (beat, len, ghost) = match &self.drag {
                Some(Drag::MoveChord { id, cur_beat, .. }) if *id == c.id => (*cur_beat, c.length, true),
                Some(Drag::ResizeChord { id, cur_len, .. }) if *id == c.id => (c.beat, *cur_len, true),
                _ => (c.beat, c.length, false),
            };
            let Some(r) = chord_span_rect(beat, len, cgeom) else { continue };
            let chip = chip_rect(r);
            painter.rect_filled(chip, Rounding::same(3.0_f32), theme::CHORD_CHIP);
            if ghost {
                painter.rect_stroke(chip, Rounding::same(3.0_f32), Stroke::new(1.5_f32, theme::SELECT));
            }
            if !self.edits.chord_edit.as_ref().is_some_and(|e| e.target == ChordTarget::Existing(c.id)) {
                painter.with_clip_rect(chip.shrink2(vec2(2.0, 0.0)).intersect(painter.clip_rect())).text(
                    pos2(chip.left() + 6.0, chip.center().y),
                    Align2::LEFT_CENTER,
                    &c.symbol,
                    FontId::proportional(11.0),
                    theme::ICON,
                );
            }
            if ghost {
                // like markers: no interaction while it is being dragged
                continue;
            }
            let resp = ui.interact(r, ui.id().with(("chord", c.id)), Sense::click_and_drag());
            let er = Rect::from_min_max(pos2(r.right() - region_edge_width(r.width()), r.top()), r.max);
            let edge = ui.interact(er, ui.id().with(("cedge", c.id)), Sense::drag());
            if self.drag.is_none() && edge.hovered() {
                ui.ctx().set_cursor_icon(egui::CursorIcon::ResizeHorizontal);
            }
            // Decide from where the press began, as for markers.
            let press_origin = ui.input(|i| i.pointer.press_origin());
            let press_zone = press_origin.map_or(RegionZone::Body, |o| region_hit_zone(o.x, r.left(), r.right()));
            let start_resize = edge.drag_started() || (resp.drag_started() && press_zone == RegionZone::Edge);
            let start_move = resp.drag_started() && !start_resize;
            if start_resize {
                let next_start = p
                    .chords
                    .iter()
                    .filter(|o| o.id != c.id && o.beat > c.beat + 1e-9)
                    .map(|o| o.beat)
                    .reduce(f64::min);
                self.drag = Some(Drag::ResizeChord {
                    id: c.id,
                    start: c.beat,
                    orig_len: c.length,
                    cur_len: c.length,
                    next_start,
                });
            } else if start_move {
                if let Some(pp) = press_origin.or_else(|| resp.interact_pointer_pos()) {
                    self.drag = Some(Drag::MoveChord {
                        id: c.id,
                        grab_beat: self.x_to_beat(pp.x, timeline_x),
                        orig_beat: c.beat,
                        cur_beat: c.beat,
                    });
                }
            }
            if resp.clicked() {
                self.exec(Command::SetPlayhead {
                    beat: Some(c.beat),
                    bar: None,
                });
            }
            // not when a drag started on this same frame
            if resp.double_clicked() && !start_move && !start_resize {
                self.edits.chord_edit = Some(ChordEdit {
                    target: ChordTarget::Existing(c.id),
                    text: c.symbol.clone(),
                    focus: true,
                    invalid: false,
                });
            }
            let cid = c.id;
            let symbol = &c.symbol;
            crate::drag_cancel::menu_unless_dragging(&resp, self.drag.is_some(), |ui| {
                if ui.button("Edit chord").clicked() {
                    self.edits.chord_edit = Some(ChordEdit {
                        target: ChordTarget::Existing(cid),
                        text: symbol.clone(),
                        focus: true,
                        invalid: false,
                    });
                    ui.close_menu();
                }
                if ui.button("Remove chord").clicked() {
                    self.cmd(Command::RemoveChord { chord_id: cid });
                    ui.close_menu();
                }
            });
        }
        // Chord field, registered last so it sits on top of the lane. A chord that scrolled out of view, was
        // deleted or undone away drops the edit, and so does a new chord whose place is out of view.
        let target = self.edits.chord_edit.take().and_then(|edit| {
            let r = chord_field_rect(edit.target, &p.chords, cgeom)?;
            let old = match edit.target {
                ChordTarget::Existing(id) => Some(p.chords.iter().find(|c| c.id == id)?.symbol.clone()),
                ChordTarget::New { .. } => None,
            };
            Some((edit, old, r))
        });
        if let Some((mut edit, old, r)) = target {
            let edit_id = match edit.target {
                ChordTarget::Existing(id) => ui.id().with(("cedit", id)),
                ChordTarget::New { .. } => {
                    // the outline shows how long the new chord will be
                    painter.rect_stroke(chip_rect(r), Rounding::same(3.0_f32), Stroke::new(1.0_f32, theme::TEXT_DIM));
                    ui.id().with(("cedit", "new"))
                }
            };
            match rename_field(ui, edit_id, r, FontId::proportional(11.0), &mut edit.text, &mut edit.focus) {
                RenameEdit::Editing => self.edits.chord_edit = Some(edit),
                RenameEdit::Commit => {
                    let enter = ui.input(|i| i.key_pressed(egui::Key::Enter));
                    match chord_commit(old.as_deref(), &edit.text, p.key) {
                        Ok(None) => {}
                        Ok(Some(symbol)) => match edit.target {
                            ChordTarget::New { beat, length } => self.cmd(Command::AddChord { beat, length, symbol }),
                            ChordTarget::Existing(id) => self.cmd(Command::UpdateChord {
                                chord_id: id,
                                symbol: Some(symbol),
                                beat: None,
                                length: None,
                            }),
                        },
                        Err(e) => {
                            self.status = format!("⚠ {e}");
                            // Enter on a wrong symbol keeps the field open with the text selected, to retype it;
                            // a click elsewhere drops it (the message stays in the status bar).
                            if enter {
                                edit.focus = true;
                                edit.invalid = true;
                                self.edits.chord_edit = Some(edit);
                            }
                        }
                    }
                }
                RenameEdit::Cancel => {}
            }
            if self.edits.chord_edit.as_ref().is_some_and(|e| e.invalid) {
                let fr = Rect::from_min_size(r.min, vec2(r.width().max(120.0), r.height()));
                painter.rect_stroke(fr, Rounding::same(2.0_f32), Stroke::new(1.5_f32, theme::REC));
            }
        }

        // ---------------- tracks
        let track_area = Rect::from_min_max(pos2(full.left(), tracks_top), full.max);
        let tpainter = ui.painter_at(track_area);
        let th = self.track_height;
        let ctrl_alt = ui.input(|i| i.modifiers.alt);
        let mut hovered_track: Option<(Id, TrackKind)> = None;
        let pointer = ui.input(|i| i.pointer.hover_pos());
        let mut track_rename_rect: Option<Rect> = None;
        let mut region_rename_rect: Option<Rect> = None;
        // the tracks area right of the headers: a region name field never covers the headers or the lanes
        let region_area = Rect::from_min_max(pos2(timeline_x, tracks_top), full.max);
        for (i, t) in p.tracks.iter().enumerate() {
            let y = tracks_top + i as f32 * th - self.scroll_y;
            if y + th < tracks_top || y > full.bottom() {
                continue;
            }
            let lane = Rect::from_min_max(pos2(timeline_x, y), pos2(full.right(), y + th));
            let sel = self.selected_track == Some(t.id);
            tpainter.rect_filled(lane, Rounding::ZERO, lane_color(i, sel));
            if let Some(pp) = pointer {
                if lane.contains(pp) {
                    hovered_track = Some((t.id, t.kind));
                }
            }
            // grid
            let grid_step = if self.px_per_beat >= 12.0 { 1.0 } else { bpb };
            let mut b = (self.scroll_beat / grid_step).floor() * grid_step;
            while b < self.scroll_beat + visible_beats {
                let x = self.beat_to_x(b, timeline_x);
                let is_bar = (b / bpb).fract().abs() < 1e-6;
                if x >= timeline_x {
                    tpainter.line_segment(
                        [pos2(x, y), pos2(x, y + th)],
                        Stroke::new(1.0_f32, if is_bar { theme::GRID_BAR_LINE } else { theme::GRID_BEAT_LINE }),
                    );
                }
                b += grid_step;
            }
            tpainter.line_segment(
                [pos2(timeline_x, y + th - 0.5), pos2(full.right(), y + th - 0.5)],
                Stroke::new(1.0_f32, theme::LANE_LINE),
            );

            // lane interactions (empty area)
            let lane_resp = ui.interact(lane, ui.id().with(("lane", t.id)), Sense::click());
            if lane_resp.clicked() {
                self.selected_track = Some(t.id);
                self.selected_region = None;
            }
            if lane_resp.double_clicked() && t.kind == TrackKind::Instrument {
                if let Some(pp) = lane_resp.interact_pointer_pos() {
                    let b = (self.x_to_beat(pp.x, timeline_x) / bpb).floor() * bpb;
                    if let Some(v) = self.exec(Command::CreateMidiRegion {
                        track_id: t.id,
                        start: b.max(0.0),
                        length: bpb,
                        name: None,
                        notes: None,
                    }) {
                        self.selected_region = v["region_id"].as_u64();
                        self.selected_track = Some(t.id);
                        self.bottom = BottomTab::Editor;
                    }
                }
            }
            let tid = t.id;
            let tkind = t.kind;
            crate::drag_cancel::menu_unless_dragging(&lane_resp, self.drag.is_some(), |ui| {
                let b = self.snap_floor(self.position());
                if tkind == TrackKind::Instrument {
                    if ui.button("New empty MIDI region at playhead").clicked() {
                        self.cmd(Command::CreateMidiRegion { track_id: tid, start: b, length: bpb, name: None, notes: None });
                        ui.close_menu();
                    }
                    if ui.button("New step-sequencer pattern at playhead").clicked() {
                        self.cmd(Command::CreatePatternRegion { track_id: tid, start: b, steps: Some(16), step_length: Some(0.25), rows: None, name: None });
                        ui.close_menu();
                    }
                    ui.separator();
                    let span = session_players::generator_span(p.cycle, self.position(), bpb, self.extra.players.bars);
                    let len = session_players::span_text(span);
                    if span.from_cycle {
                        ui.label(egui::RichText::new(format!("Length: {len}")).weak()).on_hover_text(
                            "The session players fill the cycle around the playhead. Move the playhead out of the cycle or turn the cycle off to choose a length.",
                        );
                    } else {
                        ui.horizontal(|ui| {
                            ui.label("Length");
                            for n in session_players::LENGTHS {
                                ui.selectable_value(&mut self.extra.players.bars, n, n.to_string());
                            }
                            ui.label("bars");
                        });
                    }
                    ui.menu_button(format!("Session Drummer ({len})"), |ui| {
                        ui.horizontal(|ui| {
                            ui.label("Intensity");
                            for i in session_players::Intensity::ALL {
                                ui.selectable_value(&mut self.extra.players.intensity, i, i.label());
                            }
                        });
                        ui.checkbox(&mut self.extra.players.fill, "Fill at the end")
                            .on_hover_text("The last bar ends with a fill (parts of 2 bars or more).");
                        ui.separator();
                        for st in ["rock", "pop", "funk", "hip_hop", "trap", "house", "techno", "disco", "reggaeton", "jazz", "ballad", "drum_and_bass", "lo_fi"] {
                            if ui.button(st).clicked() {
                                if let Ok(style) = serde_json::from_value(serde_json::json!(st)) {
                                    self.run_player(Player::Drums(style), tid, span);
                                }
                                ui.close_menu();
                            }
                        }
                    });
                    ui.menu_button(format!("Session Bassist ({len}, follows chord track)"), |ui| {
                        for st in ["root", "root_fifth", "eighths", "octaves", "syncopated", "walking", "sub808"] {
                            if ui.button(st).clicked() {
                                if let Ok(style) = serde_json::from_value(serde_json::json!(st)) {
                                    self.run_player(Player::Bass(style), tid, span);
                                }
                                ui.close_menu();
                            }
                        }
                    });
                    ui.menu_button(format!("Session Keys ({len}, follows chord track)"), |ui| {
                        for st in ["sustained", "pop", "offbeat", "ballad", "funk"] {
                            if ui.button(st).clicked() {
                                if let Ok(style) = serde_json::from_value(serde_json::json!(st)) {
                                    self.run_player(Player::Keys(style), tid, span);
                                }
                                ui.close_menu();
                            }
                        }
                    });
                    if ui.button(format!("Arpeggio ({len})")).clicked() {
                        self.run_player(Player::Arpeggio, tid, span);
                        ui.close_menu();
                    }
                    if ui.button(format!("Melody ({len})")).clicked() {
                        self.run_player(Player::Melody, tid, span);
                        ui.close_menu();
                    }
                    ui.separator();
                }
                if self.ai_button(ui, "✨ Ask AI to write a part for this track") {
                    self.selected_track = Some(tid);
                    ui.close_menu();
                    self.ask_ai(ondina_studio::prompts::WRITE_PART.into());
                }
            });

            // regions
            for r in &t.regions {
                let x0 = self.beat_to_x(r.start, timeline_x);
                let x1 = self.beat_to_x(r.end(), timeline_x);
                if x1 < timeline_x || x0 > full.right() {
                    continue;
                }
                let (mut rx0, mut rx1, mut ry) = (x0, x1, y);
                let mut ghost = false;
                match &self.drag {
                    Some(Drag::MoveRegion {
                        id,
                        cur_start,
                        cur_track,
                        ..
                    }) if *id == r.id => {
                        rx0 = self.beat_to_x(*cur_start, timeline_x);
                        rx1 = rx0 + (x1 - x0);
                        if let Some(ti) = p.track_index(*cur_track) {
                            ry = tracks_top + ti as f32 * th - self.scroll_y;
                        }
                        ghost = true;
                    }
                    Some(Drag::ResizeRegion { id, cur_len, .. }) if *id == r.id => {
                        rx1 = self
                            .beat_to_x(r.start + cur_len * r.loop_count.max(1) as f64, timeline_x);
                        ghost = true;
                    }
                    _ => {}
                }
                let rect = Rect::from_min_max(
                    pos2(rx0, ry + REGION_INSET_Y),
                    pos2(rx1.max(rx0 + 4.0), ry + th - REGION_INSET_Y),
                );
                let clip =
                    rect.intersect(Rect::from_min_max(pos2(timeline_x, tracks_top), full.max));
                let renaming = self.edits.region_rename.as_ref().is_some_and(|e| e.id == r.id);
                if renaming {
                    region_rename_rect = region_name_field(rect, region_area);
                }
                let base = if r.muted { theme::REGION_MUTED } else { theme::rgb(r.color.unwrap_or(t.color)) };
                let selected = self.selected_region == Some(r.id);
                let ppainter = tpainter.with_clip_rect(clip);
                // corners are rounded only where the region really ends, not where the view cuts it
                let round = region_rounding(rect, clip);
                ppainter.rect_filled(clip, round, base.gamma_multiply(if r.muted { 0.22 } else { 0.33 }));
                let header = Rect::from_min_max(clip.min, pos2(clip.right(), (clip.top() + REGION_HEAD_H).min(clip.bottom())));
                ppainter.rect_filled(header, Rounding { sw: 0.0, se: 0.0, ..round }, base);
                // the name field covers the name while it is being edited
                if !renaming {
                    let name_clip = header.shrink2(vec2(3.0, 0.0)).intersect(clip);
                    tpainter.with_clip_rect(name_clip).text(
                        pos2(clip.left() + 6.0, header.center().y),
                        Align2::LEFT_CENTER,
                        &r.name,
                        FontId::new(11.0, theme::semibold()),
                        theme::ON_TRACK,
                    );
                }
                // loop separators
                for k in 1..r.loop_count {
                    let lx = rx0 + (k as f64 * r.length) as f32 * self.px_per_beat;
                    if lx > clip.left() && lx < clip.right() {
                        ppainter.line_segment(
                            [pos2(lx, rect.top()), pos2(lx, rect.bottom())],
                            Stroke::new(1.0_f32, Color32::from_black_alpha(110)),
                        );
                    }
                }
                // content preview
                let content = Rect::from_min_max(
                    pos2(rect.left(), rect.top() + REGION_HEAD_H + 2.0),
                    pos2(rect.right(), rect.bottom() - 3.0),
                );
                let fade_drag = match &self.drag {
                    Some(Drag::RegionFade { id, handle, cur, .. }) if *id == r.id => Some((*handle, *cur)),
                    _ => None,
                };
                // while a resize is dragged the repetitions already have the new length, like the ghost
                let rep_len = match &self.drag {
                    Some(Drag::ResizeRegion { id, cur_len, .. }) if *id == r.id => *cur_len,
                    _ => r.length,
                };
                let fade_geom = region_edit::audio_fades(r).map(|(fi, fo)| {
                    let (fi, fo) = match fade_drag {
                        Some((FadeHandle::In, v)) => (v, fo),
                        Some((FadeHandle::Out, v)) => (fi, v),
                        None => (fi, fo),
                    };
                    let first_right = (rx0 + rep_len as f32 * self.px_per_beat).min(rect.right());
                    FadeGeom {
                        first: Rect::from_min_max(rect.min, pos2(first_right, rect.bottom())),
                        reps: r.loop_count.max(1),
                        fade_in_px: fi as f32 * self.px_per_beat,
                        fade_out_px: fo as f32 * self.px_per_beat,
                    }
                });
                if content.height() >= 2.0 {
                    match &r.content {
                        RegionContent::Midi(_) | RegionContent::Pattern(_) => {
                            let notes: Cow<[Note]> = match &r.content {
                                RegionContent::Midi(m) => Cow::Borrowed(&m.notes),
                                RegionContent::Pattern(pt) => Cow::Owned(pattern_to_notes(pt)),
                                RegionContent::Audio(_) => Cow::Owned(Vec::new()),
                            };
                            let geom = NoteGeom {
                                x0: rx0,
                                px_per_beat: self.px_per_beat,
                                length: r.length,
                                loops: r.loop_count.max(1),
                                content,
                            };
                            if is_drum_region(r, t) {
                                paint_drum_hits(&ppainter, &notes, geom);
                            } else {
                                paint_note_bars(&ppainter, &notes, geom);
                            }
                        }
                        RegionContent::Audio(a) => {
                            if let (Some(peaks), Some(f)) =
                                (self.waveforms.get(&a.file_id), p.audio_file(a.file_id))
                            {
                                let mid = content.center().y;
                                let h = content.height() * 0.5;
                                let start_sec = p.tempo.beat_to_sec(r.start);
                                let mut columns = Vec::new();
                                let mut x = clip.left().max(rect.left()).floor();
                                while x < clip.right() {
                                    let beat = self.x_to_beat(x, timeline_x);
                                    let in_reg = (beat - r.start) % r.length.max(1e-6);
                                    let sec = p.tempo.beat_to_sec(r.start + in_reg) - start_sec
                                        + a.offset_sec;
                                    let sec = if a.reverse { f.duration_sec - sec } else { sec };
                                    let idx = (sec * f.sample_rate as f64 / 256.0) as usize;
                                    if let Some((lo, hi)) = peaks.get(idx) {
                                        columns.push((x, mid - hi * h, mid - lo * h));
                                    }
                                    x += 1.0;
                                }
                                paint_waveform(&ppainter, &columns, theme::REGION_INK);
                            }
                        }
                    }
                }
                if let Some(g) = &fade_geom {
                    region_edit::paint_fades(&ppainter, g, fade_drag.map(|(h, _)| h));
                }
                if selected {
                    // white border and a white ring outside it, readable on every track color
                    tpainter.rect_stroke(clip.shrink(0.5), round, Stroke::new(1.0_f32, theme::SELECT));
                    tpainter.rect_stroke(clip.expand(0.5), round, Stroke::new(1.0_f32, theme::SELECT));
                } else {
                    tpainter.rect_stroke(clip.shrink(0.5), round, Stroke::new(1.0_f32, base));
                }
                if ghost {
                    continue;
                }
                // interactions: body = move, right edge = resize.
                // Body first, resize edge after so the edge sits on top and gets the drag (egui gives a press to
                // the last widget registered under the pointer); clicks on the edge still reach the body.
                let edge_w = region_edge_width(rect.width());
                let edge = Rect::from_min_max(pos2(rect.right() - edge_w, rect.top()), rect.max);
                let body_resp = ui.interact(
                    clip,
                    ui.id().with(("region", r.id)),
                    Sense::click_and_drag(),
                );
                let edge_resp = ui.interact(
                    edge.intersect(clip),
                    ui.id().with(("redge", r.id)),
                    Sense::drag(),
                );
                if edge_resp.hovered() && self.drag.is_none() {
                    ui.ctx().set_cursor_icon(egui::CursorIcon::ResizeHorizontal);
                }
                // fade handles last, over body and edge, so they get presses, clicks and double-clicks
                let handles = fade_geom.as_ref().and_then(region_edit::fade_handles);
                let fade_resps = handles.map(|[hi, ho]| {
                    [
                        ui.interact(hi.intersect(clip), ui.id().with(("rfadein", r.id)), Sense::click_and_drag()),
                        ui.interact(ho.intersect(clip), ui.id().with(("rfadeout", r.id)), Sense::click_and_drag()),
                    ]
                });
                // Decide from where the press began, not where the pointer is once egui reports the drag
                // (that comes a few pixels later), so a drag that starts on the edge never moves the region.
                let press_origin = ui.input(|i| i.pointer.press_origin());
                let press_zone = press_origin.map(|o| region_press_zone(o, rect, handles)).unwrap_or(RegionZone::Body);
                let handle_started = match &fade_resps {
                    Some([fin, _]) if fin.drag_started() => Some(FadeHandle::In),
                    Some([_, fout]) if fout.drag_started() => Some(FadeHandle::Out),
                    _ => None,
                };
                let any_started = body_resp.drag_started() || edge_resp.drag_started() || handle_started.is_some();
                let start_fade = if !any_started {
                    None
                } else {
                    match press_zone {
                        RegionZone::FadeIn => Some(FadeHandle::In),
                        RegionZone::FadeOut => Some(FadeHandle::Out),
                        _ => handle_started,
                    }
                };
                let start_resize = start_fade.is_none()
                    && (edge_resp.drag_started() || (body_resp.drag_started() && press_zone == RegionZone::Edge));
                let start_move = body_resp.drag_started() && start_fade.is_none() && !start_resize;
                if let (Some(handle), Some((fi, fo)), Some(o)) = (start_fade, region_edit::audio_fades(r), press_origin) {
                    let orig = if handle == FadeHandle::In { fi } else { fo };
                    let grab_beat = self.x_to_beat(o.x, timeline_x);
                    self.drag = Some(Drag::RegionFade { id: r.id, handle, grab_beat, orig, cur: orig });
                    self.selected_region = Some(r.id);
                    self.selected_track = Some(t.id);
                }
                if let (Some(resps), Some((fi, fo))) = (&fade_resps, region_edit::audio_fades(r)) {
                    let mut hot = fade_drag.map(|(h, _)| h);
                    for (resp, handle, v) in [(&resps[0], FadeHandle::In, fi), (&resps[1], FadeHandle::Out, fo)] {
                        // a click on a handle only selects the region
                        if resp.clicked() {
                            self.selected_region = Some(r.id);
                            self.selected_track = Some(t.id);
                        }
                        // a double-click removes that fade
                        if resp.double_clicked() {
                            if let Some(c) = region_edit::fade_command(r.id, handle, v, 0.0) {
                                self.cmd(c);
                            }
                        }
                        if resp.hovered() && self.drag.is_none() {
                            hot = Some(handle);
                            ui.ctx().set_cursor_icon(egui::CursorIcon::ResizeHorizontal);
                            resp.clone().on_hover_text(format!(
                                "{}. Drag to change it, Alt+drag ignores the grid, double-click removes it.",
                                region_edit::fade_label(handle, v)
                            ));
                        }
                    }
                    let show = selected || fade_drag.is_some() || pointer.is_some_and(|pp| clip.contains(pp));
                    if let (true, Some(h)) = (show, handles) {
                        region_edit::paint_fade_handles(&tpainter.with_clip_rect(clip), h, hot);
                    }
                }
                if start_resize {
                    self.drag = Some(Drag::ResizeRegion {
                        id: r.id,
                        orig_len: r.length,
                        cur_len: r.length,
                    });
                    self.selected_region = Some(r.id);
                }
                if body_resp.clicked() || start_move {
                    self.selected_region = Some(r.id);
                    self.selected_track = Some(t.id);
                    if self.bottom == BottomTab::Hidden {
                        self.bottom = BottomTab::Editor;
                    }
                    self.selected_notes.clear();
                }
                if body_resp.double_clicked() {
                    self.bottom = BottomTab::Editor;
                    if let RegionContent::Midi(_) = r.content {
                        self.pr_framing.reset();
                    }
                }
                if start_move {
                    if let Some(pp) = body_resp.interact_pointer_pos() {
                        self.drag = Some(Drag::MoveRegion {
                            id: r.id,
                            grab_beat: self.x_to_beat(pp.x, timeline_x),
                            orig_start: r.start,
                            orig_track: t.id,
                            cur_start: r.start,
                            cur_track: t.id,
                            copy: ctrl_alt,
                        });
                    }
                }
                let rid = r.id;
                let muted = r.muted;
                let is_pattern = matches!(r.content, RegionContent::Pattern(_));
                let loops = r.loop_count;
                let r_clone = r.clone();
                let rname = r.name.clone();
                let rcolor = r.color;
                let rtid = t.id;
                crate::drag_cancel::menu_unless_dragging(&body_resp, self.drag.is_some(), |ui| {
                    if ui.button("Rename…").clicked() {
                        self.edits.region_rename = Some(InPlaceRename { id: rid, text: rname.clone(), focus: true });
                        self.selected_region = Some(rid);
                        self.selected_track = Some(rtid);
                        // open the field on this frame: region_rename_rect was decided before the menu
                        region_rename_rect = region_name_field(rect, region_area);
                        ui.close_menu();
                    }
                    ui.menu_button("Color", |ui| {
                        if let Some(choice) = region_edit::color_swatches(ui, rcolor) {
                            if let Some(c) = region_edit::color_command(rid, rcolor, choice) {
                                self.cmd(c);
                            }
                            ui.close_menu();
                        }
                    });
                    ui.separator();
                    if ui.button("Split at playhead").clicked() {
                        let b = self.position();
                        self.cmd(Command::SplitRegion { region_id: rid, beat: b });
                        ui.close_menu();
                    }
                    if ui.button("Duplicate").clicked() {
                        self.cmd(Command::DuplicateRegion { region_id: rid, start: None, count: Some(1) });
                        ui.close_menu();
                    }
                    if ui.button("Repeat ×4").clicked() {
                        self.cmd(Command::DuplicateRegion { region_id: rid, start: None, count: Some(4) });
                        ui.close_menu();
                    }
                    if ui.button(format!("Loop count: {} → {}", loops, loops + 1)).clicked() {
                        self.cmd(Command::SetRegionProperties { region_id: rid, name: None, muted: None, loop_count: Some(loops + 1), transpose: None, velocity_offset: None, gain_db: None, fade_in: None, fade_out: None, reverse: None, color: None });
                        ui.close_menu();
                    }
                    if ui.button(if muted { "Unmute" } else { "Mute" }).clicked() {
                        self.cmd(Command::SetRegionProperties { region_id: rid, name: None, muted: Some(!muted), loop_count: None, transpose: None, velocity_offset: None, gain_db: None, fade_in: None, fade_out: None, reverse: None, color: None });
                        ui.close_menu();
                    }
                    self.takes_menu(ui, &r_clone);
                    if is_pattern && ui.button("Convert pattern to MIDI").clicked() {
                        self.cmd(Command::ConvertPatternToMidi { region_id: rid });
                        ui.close_menu();
                    }
                    if ui.button("Delete").clicked() {
                        self.cmd(Command::DeleteRegion { region_id: rid });
                        ui.close_menu();
                    }
                    ui.separator();
                    if self.ai_button(ui, "✨ Ask AI to vary this region") {
                        self.selected_region = Some(rid);
                        ui.close_menu();
                        self.ask_ai(ondina_studio::prompts::VARY_REGION.into());
                    }
                });
            }

            // automation over the regions, registered after them so it takes the presses below their name bar
            if self.automation.on && automation_lane::automatable(t.id) {
                let geom = automation_lane::LaneGeom { lane, region_area, timeline_x, alt: ctrl_alt };
                self.automation_overlay(ui, &tpainter, geom, t, &p);
            }

            // header
            let header = Rect::from_min_max(pos2(full.left(), y), pos2(timeline_x, y + th));
            if let Some(r) = self.track_header(ui, &tpainter, header, t, i, sel) {
                track_rename_rect = Some(r);
            }
        }

        // Track name field, registered after every header so it sits on top. A track that scrolled out of
        // view, was deleted or undone away drops the edit.
        let target = self.edits.track_rename.take().and_then(|ren| {
            p.track(ren.id)?;
            Some((ren, track_rename_rect?))
        });
        if let Some((mut ren, r)) = target {
            let edit_id = ui.id().with(("trename", ren.id));
            match rename_field(ui, edit_id, r, FontId::proportional(12.5), &mut ren.text, &mut ren.focus) {
                RenameEdit::Editing => self.edits.track_rename = Some(ren),
                RenameEdit::Commit => {
                    if let Some(c) = track_rename_command(&ren, &p) {
                        self.cmd(c);
                    }
                }
                RenameEdit::Cancel => {}
            }
        }

        // Region name field, registered after every region so it sits on top. A region that scrolled out of
        // view, was deleted or undone away drops the edit.
        let target = self.edits.region_rename.take().and_then(|ren| {
            let old = p.find_region(ren.id)?.1.name.clone();
            Some((ren, old, region_rename_rect?))
        });
        if let Some((mut ren, old, r)) = target {
            let edit_id = ui.id().with(("rrename", ren.id));
            match rename_field(ui, edit_id, r, FontId::proportional(10.5), &mut ren.text, &mut ren.focus) {
                RenameEdit::Editing => self.edits.region_rename = Some(ren),
                RenameEdit::Commit => {
                    if let Some(c) = region_edit::name_command(ren.id, &old, &ren.text) {
                        self.cmd(c);
                    }
                }
                RenameEdit::Cancel => {}
            }
        }

        // dragging updates
        let pointer_pos = ui.input(|i| i.pointer.interact_pos());
        let released = ui.input(|i| crate::drag_cancel::drag_finished(&i.pointer, self.drag.as_ref()));
        let pointer_beat = pointer_pos.map(|pp| self.x_to_beat(pp.x, timeline_x));
        let snap = self.snap;
        match &mut self.drag {
            Some(Drag::MoveRegion {
                grab_beat,
                orig_start,
                cur_start,
                cur_track,
                orig_track,
                ..
            }) => {
                if let Some(pp) = pointer_pos {
                    let b = self.scroll_beat + ((pp.x - timeline_x) / self.px_per_beat) as f64;
                    let ns = *orig_start + (b - *grab_beat);
                    *cur_start = if self.snap > 0.0 {
                        ((ns / self.snap).round() * self.snap).max(0.0)
                    } else {
                        ns.max(0.0)
                    };
                    let ti = ((pp.y - tracks_top + self.scroll_y) / th).floor();
                    if ti >= 0.0 {
                        if let Some(tt) = p.tracks.get(ti as usize) {
                            let src_kind = p.track(*orig_track).map(|x| x.kind);
                            if Some(tt.kind) == src_kind {
                                *cur_track = tt.id;
                            }
                        }
                    }
                }
            }
            Some(Drag::ResizeRegion { id, cur_len, .. }) => {
                ui.ctx().set_cursor_icon(egui::CursorIcon::ResizeHorizontal);
                if let (Some(pp), Some((_, r))) = (pointer_pos, p.find_region(*id)) {
                    let b = self.scroll_beat + ((pp.x - timeline_x) / self.px_per_beat) as f64;
                    let mut l = (b - r.start) / r.loop_count.max(1) as f64;
                    if self.snap > 0.0 {
                        l = (l / self.snap).round() * self.snap;
                    }
                    *cur_len = l.max(0.25);
                }
            }
            Some(Drag::RegionFade { id, handle, grab_beat, orig, cur }) => {
                ui.ctx().set_cursor_icon(egui::CursorIcon::ResizeHorizontal);
                if let (Some(b), Some((_, r))) = (pointer_beat, p.find_region(*id)) {
                    if let Some((fi, fo)) = region_edit::audio_fades(r) {
                        let other = if *handle == FadeHandle::In { fo } else { fi };
                        // Alt drags without the grid
                        let grid = if ctrl_alt { 0.0 } else { snap };
                        *cur = region_edit::fade_from_drag(*handle, *orig, b - *grab_beat, r.length, other, grid);
                    }
                }
                let text = region_edit::fade_label(*handle, *cur);
                egui::show_tooltip_at_pointer(ui.ctx(), ui.layer_id(), ui.id().with("fade_tip"), |ui| {
                    ui.label(text);
                });
            }
            Some(Drag::MoveMarker {
                grab_beat,
                orig_beat,
                cur_beat,
                ..
            }) => {
                if let Some(b) = pointer_beat {
                    *cur_beat = dragged_marker_beat(*orig_beat, *grab_beat, b, snap);
                }
            }
            Some(Drag::ResizeMarker { start, cur_len, .. }) => {
                ui.ctx().set_cursor_icon(egui::CursorIcon::ResizeHorizontal);
                if let Some(b) = pointer_beat {
                    *cur_len = resized_marker_length(*start, b, snap);
                }
            }
            Some(Drag::MoveChord { grab_beat, orig_beat, cur_beat, .. }) => {
                if let Some(b) = pointer_beat {
                    *cur_beat = dragged_marker_beat(*orig_beat, *grab_beat, b, snap);
                }
            }
            Some(Drag::ResizeChord { start, cur_len, next_start, .. }) => {
                ui.ctx().set_cursor_icon(egui::CursorIcon::ResizeHorizontal);
                if let Some(b) = pointer_beat {
                    *cur_len = resized_chord_length(*start, b, snap, *next_start);
                }
            }
            Some(Drag::MoveTrack { gap, .. }) => {
                ui.ctx().set_cursor_icon(egui::CursorIcon::Grabbing);
                if let Some(pp) = pointer_pos {
                    let n = p.tracks.len();
                    *gap = track_drop_index(pp.y, tracks_top - self.scroll_y, th, n);
                    let v = edge_scroll_speed(pp.y, tracks_top, full.bottom());
                    if v != 0.0 {
                        let dt = ui.input(|i| i.stable_dt).min(0.1);
                        // same limit as the mouse wheel
                        let max = max_scroll_y(n, th, full.bottom() - tracks_top);
                        self.scroll_y = (self.scroll_y + v * dt).clamp(0.0, max);
                        // keep scrolling while the pointer rests in the band
                        ui.ctx().request_repaint();
                    }
                }
            }
            Some(Drag::AutoPoint { drag, .. }) => {
                ui.ctx().set_cursor_icon(egui::CursorIcon::Grabbing);
                if let (Some(b), Some(pp)) = (pointer_beat, pointer_pos) {
                    let (top, bottom) = automation_lane::value_band(0.0, th);
                    // Alt moves the point off the grid
                    drag.update(b, pp.y, bottom - top, self.px_per_beat, automation_lane::grid(snap, ctrl_alt));
                }
                let text = automation_lane::value_label(&drag.spec, drag.point().value);
                egui::show_tooltip_at_pointer(ui.ctx(), ui.layer_id(), ui.id().with("auto_tip"), |ui| {
                    ui.label(text);
                });
            }
            _ => {}
        }
        if let Some(Drag::MoveTrack { gap, .. }) = &self.drag {
            let y = (tracks_top - self.scroll_y + *gap as f32 * th).clamp(tracks_top + 1.0, full.bottom() - 1.0);
            tpainter.line_segment([pos2(full.left(), y), pos2(full.right(), y)], Stroke::new(2.0_f32, theme::SELECT));
        }
        if released {
            match self.drag.take() {
                Some(Drag::MoveRegion {
                    id,
                    orig_start,
                    orig_track,
                    cur_start,
                    cur_track,
                    copy,
                    ..
                }) => {
                    if copy && (cur_start != orig_start || cur_track != orig_track) {
                        if let Some(v) = self.exec(Command::DuplicateRegion {
                            region_id: id,
                            start: Some(cur_start),
                            count: Some(1),
                        }) {
                            if let Some(nid) = v["region_ids"][0].as_u64() {
                                if cur_track != orig_track {
                                    self.cmd(Command::MoveRegion {
                                        region_id: nid,
                                        start: None,
                                        track_id: Some(cur_track),
                                    });
                                }
                                self.selected_region = Some(nid);
                            }
                        }
                    } else if cur_start != orig_start || cur_track != orig_track {
                        self.cmd(Command::MoveRegion {
                            region_id: id,
                            start: Some(cur_start),
                            track_id: if cur_track != orig_track {
                                Some(cur_track)
                            } else {
                                None
                            },
                        });
                    }
                }
                Some(Drag::ResizeRegion {
                    id,
                    orig_len,
                    cur_len,
                }) => {
                    if (cur_len - orig_len).abs() > 1e-9 {
                        self.cmd(Command::ResizeRegion {
                            region_id: id,
                            length: cur_len,
                        });
                    }
                }
                Some(Drag::MoveMarker {
                    id,
                    orig_beat,
                    cur_beat,
                    ..
                }) => {
                    if (cur_beat - orig_beat).abs() > 1e-9 {
                        self.cmd(Command::UpdateMarker {
                            marker_id: id,
                            name: None,
                            beat: Some(cur_beat),
                            length: None,
                        });
                    }
                }
                Some(Drag::ResizeMarker {
                    id,
                    orig_len,
                    cur_len,
                    ..
                }) => {
                    if (cur_len - orig_len).abs() > 1e-9 {
                        self.cmd(Command::UpdateMarker {
                            marker_id: id,
                            name: None,
                            beat: None,
                            length: Some(cur_len),
                        });
                    }
                }
                Some(Drag::MoveChord { id, orig_beat, cur_beat, .. }) => {
                    if (cur_beat - orig_beat).abs() > 1e-9 {
                        self.cmd(Command::UpdateChord {
                            chord_id: id,
                            symbol: None,
                            beat: Some(cur_beat),
                            length: None,
                        });
                    }
                }
                Some(Drag::ResizeChord { id, orig_len, cur_len, .. }) => {
                    if (cur_len - orig_len).abs() > 1e-9 {
                        self.cmd(Command::UpdateChord {
                            chord_id: id,
                            symbol: None,
                            beat: None,
                            length: Some(cur_len),
                        });
                    }
                }
                Some(Drag::MoveTrack { id, gap }) => {
                    // Found by id: the assistant or MCP may have reordered the tracks during the drag.
                    if let Some(index) = p
                        .track_index(id)
                        .and_then(|from| track_move_target(from, gap.min(p.tracks.len())))
                    {
                        self.cmd(Command::MoveTrack { track_id: id, index });
                    }
                }
                Some(Drag::RegionFade { id, handle, orig, cur, .. }) => {
                    if let Some(c) = region_edit::fade_command(id, handle, orig, cur) {
                        self.cmd(c);
                    }
                }
                Some(Drag::AutoPoint { track, drag }) => {
                    if let Some(c) = drag.command(track) {
                        self.cmd(c);
                    }
                }
                other => self.drag = other.filter(|d| !matches!(d, Drag::Cycle { .. } | Drag::CycleEdit { .. })),
            }
        }

        // add-track buttons below the last track
        let y_end = tracks_top + p.tracks.len() as f32 * th - self.scroll_y;
        if y_end < full.bottom() - 30.0 {
            let r = Rect::from_min_size(
                pos2(full.left() + 8.0, y_end + 8.0),
                vec2(HEADER_W - 16.0, 24.0),
            );
            let mut child = ui.new_child(
                egui::UiBuilder::new()
                    .max_rect(r)
                    .layout(egui::Layout::left_to_right(egui::Align::Center)),
            );
            child.menu_button("+  Add track", |ui| self.add_track_menu(ui));
            if p.tracks.is_empty() {
                let c = pos2(
                    timeline_x + (full.right() - timeline_x) / 2.0,
                    tracks_top + 120.0,
                );
                painter.text(
                    c,
                    Align2::CENTER_CENTER,
                    "Empty project — add a track, or ask the AI assistant to compose something ✨",
                    FontId::proportional(15.0),
                    theme::TEXT_DIM,
                );
            }
        }

        // drop target highlight for move between tracks
        let _ = hovered_track;

        // files dragged in from Explorer: drawn over the tracks, under the playhead
        let drop_geom = crate::file_drop::DropGeom {
            rect: full,
            timeline_x,
            tracks_top,
            track_h: th,
            scroll_y: self.scroll_y,
            scroll_beat: self.scroll_beat,
            px_per_beat: self.px_per_beat,
        };
        self.file_drop_ui(ui, drop_geom, &p);

        // playhead: white, a soft glow around it and a small triangle head under the cycle strip
        let px = self.beat_to_x(pos, timeline_x);
        if px >= timeline_x && px <= full.right() {
            for (w, a) in [(7.0_f32, 14_u8), (3.5, 34)] {
                painter.line_segment([pos2(px, top), pos2(px, full.bottom())], Stroke::new(w, Color32::from_white_alpha(a)));
            }
            painter.line_segment([pos2(px, top), pos2(px, full.bottom())], Stroke::new(1.0_f32, theme::PLAYHEAD));
            painter.add(egui::Shape::convex_polygon(
                vec![
                    pos2(px - 6.0, top + CYCLE_STRIP_H),
                    pos2(px + 6.0, top + CYCLE_STRIP_H),
                    pos2(px, top + CYCLE_STRIP_H + 8.0),
                ],
                theme::PLAYHEAD,
                Stroke::NONE,
            ));
        }
        // header column next to the ruler and the two lanes: track count with the + menu, then the lane names
        let corner = Rect::from_min_max(full.min, pos2(timeline_x, tracks_top));
        painter.rect_filled(corner, Rounding::ZERO, theme::HEADER_BG);
        let label_x = full.left() + HEADER_COLOR_W + 8.0;
        let ruler_mid = top + RULER_H / 2.0;
        painter.text(
            pos2(label_x, ruler_mid),
            Align2::LEFT_CENTER,
            match p.tracks.len() {
                1 => "1 track".to_string(),
                n => format!("{n} tracks"),
            },
            FontId::proportional(theme::SIZE_BODY),
            theme::TEXT_3,
        );
        for (row_top, label, line) in [
            (top + RULER_H, "Markers".to_string(), theme::LANE_LINE),
            (top + RULER_H + LANE_H, format!("Chords · {}", p.key.name()), theme::HAIRLINE),
        ] {
            painter.line_segment(
                [pos2(full.left(), row_top - 0.5), pos2(timeline_x, row_top - 0.5)],
                Stroke::new(1.0_f32, theme::HAIRLINE),
            );
            painter.text(pos2(label_x, row_top + LANE_H / 2.0), Align2::LEFT_CENTER, label, FontId::proportional(11.0), theme::TEXT_3);
            painter.line_segment(
                [pos2(full.left(), row_top + LANE_H - 0.5), pos2(timeline_x, row_top + LANE_H - 0.5)],
                Stroke::new(1.0_f32, line),
            );
        }
        // the dark divider between the headers and the timeline
        painter.line_segment(
            [pos2(timeline_x - 0.5, top), pos2(timeline_x - 0.5, full.bottom())],
            Stroke::new(1.0_f32, theme::SEPARATOR),
        );
        let plus = Rect::from_center_size(pos2(timeline_x - 8.0 - 11.0, ruler_mid), vec2(22.0, 20.0));
        let mut child = ui.new_child(
            egui::UiBuilder::new()
                .max_rect(plus)
                .layout(egui::Layout::left_to_right(egui::Align::Center)),
        );
        child.spacing_mut().button_padding = vec2(6.0, 1.0);
        child
            .menu_button("+", |ui| self.add_track_menu(ui))
            .response
            .on_hover_text("Add a track");
        ui.allocate_rect(full, Sense::hover());
    }

    /// Selects a track from its header: a selected region on another track is dropped, so the Inspector's region row
    /// always belongs to the selected track (see [`region_after_track_click`]).
    pub(crate) fn select_track(&mut self, id: Id) {
        let p = self.project();
        self.selected_region = region_after_track_click(&p, self.selected_region, id);
        self.selected_track = Some(id);
    }

    /// Draws one track header and handles its clicks, drag-to-reorder and double-click-to-rename.
    /// Returns where the rename field goes when this track is being renamed and its name row is fully visible.
    fn track_header(
        &mut self,
        ui: &mut egui::Ui,
        painter: &egui::Painter,
        rect: Rect,
        t: &Track,
        index: usize,
        selected: bool,
    ) -> Option<Rect> {
        // Background first: M/S/R, the volume slider and the rename field are registered after it and sit on top.
        let resp = ui.interact(rect, ui.id().with(("hdr", t.id)), Sense::click_and_drag());
        let bg = if selected {
            theme::SELECTED_BG
        } else if resp.contains_pointer() && self.drag.is_none() {
            theme::HEADER_HOVER
        } else {
            theme::HEADER_BG
        };
        painter.rect_filled(rect, Rounding::ZERO, bg);
        let color = theme::rgb(t.color);
        painter.rect_filled(
            Rect::from_min_size(rect.min, vec2(HEADER_COLOR_W, rect.height())),
            Rounding::ZERO,
            if selected { color } else { theme::darker(color, 0.85) },
        );
        painter.line_segment(
            [pos2(rect.left(), rect.bottom() - 0.5), pos2(rect.right(), rect.bottom() - 0.5)],
            Stroke::new(1.0_f32, theme::HAIRLINE),
        );
        if resp.clicked() {
            self.select_track(t.id);
        }
        // Only the left button reorders: egui turns a slow or moving right or middle press into a drag too.
        if resp.drag_started_by(egui::PointerButton::Primary) && self.drag.is_none() {
            self.select_track(t.id);
            self.drag = Some(Drag::MoveTrack { id: t.id, gap: index });
        }
        let rows = HeaderRows::new(rect.height());
        let left = rect.left() + HEADER_COLOR_W + 8.0;
        let right = rect.right() - HEADER_METER_W - 6.0;
        let name_y = rect.top() + rows.name_center;
        let buttons = header_buttons(t);
        // On short tracks the buttons sit at the right end of the name row, so the name stops before them.
        let buttons_w = buttons.len() as f32 * (HEADER_BUTTON_W + HEADER_BUTTON_GAP) - HEADER_BUTTON_GAP;
        let first_button_x = if rows.compact { right - buttons_w } else { left };
        let kind_label = track_type_label(t);
        let kind_w = if rows.compact {
            0.0
        } else {
            painter.layout_no_wrap(kind_label.to_string(), FontId::proportional(11.0), theme::TEXT_3).size().x
        };
        let name_x = left + 16.0;
        let name_right = if rows.compact { first_button_x - 6.0 } else { right - kind_w - 8.0 };
        // the name row opens the rename; the rest of the header only selects
        let name_row = Rect::from_min_max(pos2(rect.left(), name_y - 10.0), pos2(name_right, name_y + 10.0));
        if resp.double_clicked() && resp.interact_pointer_pos().is_some_and(|pp| name_row.contains(pp)) {
            self.edits.track_rename = Some(InPlaceRename {
                id: t.id,
                text: t.name.clone(),
                focus: true,
            });
        }
        // decided after the double-click above, so the field opens on the same frame
        let renaming = self.edits.track_rename.as_ref().is_some_and(|e| e.id == t.id);
        let mut field = None;
        painter.text(
            pos2(left, name_y),
            Align2::LEFT_CENTER,
            format!("{}", index + 1),
            FontId::monospace(10.0),
            theme::TEXT_DISABLED,
        );
        if renaming {
            let r = Rect::from_min_max(pos2(name_x - 3.0, name_y - 9.0), pos2(name_right, name_y + 9.0));
            // half-hidden under the lanes or the window edge: drop the edit rather than draw over them
            field = painter.clip_rect().contains_rect(r).then_some(r);
        } else {
            let name_clip = Rect::from_min_max(pos2(name_x, rect.top()), pos2(name_right, rect.bottom())).intersect(painter.clip_rect());
            painter.with_clip_rect(name_clip).text(
                pos2(name_x, name_y),
                Align2::LEFT_CENTER,
                &t.name,
                FontId::new(theme::SIZE_BODY, theme::semibold()),
                if selected { theme::PLAYHEAD } else { theme::TEXT },
            );
        }
        if !rows.compact {
            painter.text(pos2(right, name_y), Align2::RIGHT_CENTER, kind_label, FontId::proportional(11.0), theme::TEXT_3);
        }
        let tid = t.id;
        // computed before the menus, which borrow `self` mutably
        let auto_choices = if automation_lane::automatable(tid) {
            let p = self.project();
            let current = self.automation.target(&p, t);
            Some((automation_lane::lane_choices(&p, t), current))
        } else {
            None
        };
        crate::drag_cancel::menu_unless_dragging(&resp, self.drag.is_some(), |ui| {
            if let Some((choices, current)) = &auto_choices {
                ui.menu_button("Automation lane", |ui| {
                    if let Some(target) = automation_lane_menu(ui, choices, current) {
                        self.automation.choose(tid, &target);
                        self.automation.on = true;
                        ui.close_menu();
                    }
                });
                ui.separator();
            }
            if ui.button("Duplicate track").clicked() {
                self.cmd(Command::DuplicateTrack { track_id: tid });
                ui.close_menu();
            }
            if ui.button("Bounce in place").clicked() {
                ui.close_menu();
                self.exec(Command::BounceInPlace { track_id: tid });
            }
            if ui.button("Delete track").clicked() {
                self.cmd(Command::RemoveTrack { track_id: tid });
                ui.close_menu();
            }
        });
        // M / S / R
        let by = rect.top() + rows.buttons_top;
        for (k, kind) in buttons.iter().enumerate() {
            let x = first_button_x + k as f32 * (HEADER_BUTTON_W + HEADER_BUTTON_GAP);
            let r = Rect::from_min_size(pos2(x, by), vec2(HEADER_BUTTON_W, HEADER_BUTTON_H));
            let (on, tip) = match kind {
                widgets::StateKind::Mute => (t.channel.mute, "Mute"),
                widgets::StateKind::Solo => (t.channel.solo, "Solo"),
                widgets::StateKind::Rec => (t.record_armed, "Record arm"),
                widgets::StateKind::Monitor => (false, "Input monitoring"),
            };
            let b = ui.interact(r, ui.id().with((kind.letter(), tid)), Sense::click()).on_hover_text(tip);
            widgets::paint_state_button(painter, r, *kind, on, b.hovered());
            if b.clicked() {
                match kind {
                    widgets::StateKind::Mute => self.cmd(Command::SetMute { track_id: tid, mute: !on }),
                    widgets::StateKind::Solo => self.cmd(Command::SetSolo { track_id: tid, solo: !on }),
                    widgets::StateKind::Rec => self.cmd(Command::SetRecordArm { track_id: tid, armed: !on }),
                    widgets::StateKind::Monitor => {}
                }
            }
        }
        // the slim volume slider and its value; with the automation view on, a lane selector takes its place
        let row = Rect::from_min_max(
            pos2(first_button_x + buttons_w + 8.0, by),
            pos2(right, by + HEADER_BUTTON_H),
        );
        let selector = auto_choices.as_ref().filter(|_| self.automation.on);
        if let (Some((choices, current)), false, true) = (selector, rows.compact, row.width() > 30.0) {
            // the view shows a lane: the slider's place picks which one (the volume stays in the mixer)
            let mut child = ui.new_child(
                egui::UiBuilder::new()
                    .max_rect(row)
                    .layout(egui::Layout::left_to_right(egui::Align::Center)),
            );
            let short = choices.iter().find(|(s, _)| s.target == *current).map_or("Volume", |(s, _)| s.short.as_str());
            child
                .menu_button(format!("{short} ⏷"), |ui| {
                    if let Some(target) = automation_lane_menu(ui, choices, current) {
                        self.automation.choose(tid, &target);
                        ui.close_menu();
                    }
                })
                .response
                .on_hover_text("Automation lane shown over this track's regions");
        } else if !rows.compact && row.width() > HEADER_VALUE_W + 24.0 {
            let rail_x0 = row.left() + 6.0;
            let rail_x1 = row.right() - HEADER_VALUE_W - 10.0;
            let cy = row.center().y;
            let hit = Rect::from_min_max(pos2(row.left(), row.top()), pos2(rail_x1 + 6.0, row.bottom()));
            let db = t.channel.volume_db;
            let sresp = ui
                .interact(hit, ui.id().with(("hvol", tid)), Sense::click_and_drag())
                .on_hover_text(format!("Volume {} dB. Drag to change it, double-click for 0 dB.", db_text(db)));
            let new_db = if sresp.double_clicked() {
                Some(0.0)
            } else if sresp.dragged() || sresp.clicked() {
                sresp.interact_pointer_pos().map(|pp| {
                    header_volume_db(((pp.x - rail_x0) / (rail_x1 - rail_x0).max(1.0)).clamp(0.0, 1.0))
                })
            } else {
                None
            };
            if let Some(v) = new_db.filter(|v| (v - db).abs() > 1e-4) {
                self.cmd_merge(Command::SetVolume { track_id: tid, db: v }, format!("vol{tid}"));
            }
            let shown = new_db.unwrap_or(db);
            let kx = rail_x0 + header_volume_unit(shown) * (rail_x1 - rail_x0);
            let rail = Rect::from_min_max(pos2(rail_x0, cy - 2.0), pos2(rail_x1, cy + 2.0));
            painter.rect_filled(rail, Rounding::same(2.0_f32), theme::BORDER);
            painter.rect_filled(Rect::from_min_max(rail.min, pos2(kx, rail.bottom())), Rounding::same(2.0_f32), theme::SLIDER_FILL);
            let hot = sresp.hovered() || sresp.dragged();
            painter.circle(
                pos2(kx, cy),
                if hot { 6.5 } else { 6.0 },
                if hot { theme::PLAYHEAD } else { theme::ICON },
                Stroke::new(1.0_f32, theme::SEPARATOR),
            );
            painter.text(
                pos2(row.right(), cy),
                Align2::RIGHT_CENTER,
                db_text(shown),
                FontId::monospace(10.0),
                theme::TEXT_2,
            );
        }
        // meter: a slim stereo pair in the right column, barely there at rest
        let m = self.meters.get(&tid).copied().unwrap_or([0.0; 2]);
        let mr = Rect::from_min_max(
            pos2(rect.right() - HEADER_METER_W, rect.top() + 8.0),
            pos2(rect.right() - 4.0, rect.bottom() - 8.0),
        );
        draw_meter(painter, mr, m);
        if matches!(&self.drag, Some(Drag::MoveTrack { id, .. }) if *id == t.id) {
            // the dragged header stays in place, dimmed, until it is dropped
            painter.rect_filled(rect, Rounding::ZERO, Color32::from_black_alpha(110));
            painter.rect_stroke(rect.shrink(1.0), Rounding::ZERO, Stroke::new(1.5_f32, theme::SELECT));
        }
        field
    }

    /// The entries of the add-track menus (the + in the header column and the button below the last track).
    fn add_track_menu(&mut self, ui: &mut egui::Ui) {
        if ui.button("Software Instrument").clicked() {
            self.add_track(TrackKind::Instrument, None);
            ui.close_menu();
        }
        if ui.button("Drums").clicked() {
            self.add_track(TrackKind::Instrument, Some("Pop Drum Kit"));
            ui.close_menu();
        }
        if ui.button("Audio").clicked() {
            self.add_track(TrackKind::Audio, None);
            ui.close_menu();
        }
        if ui.button("Aux Bus").clicked() {
            self.add_track(TrackKind::Bus, None);
            ui.close_menu();
        }
        ui.separator();
        for patch in ondina_core::plugins::PATCHES.iter() {
            let kind = if patch.instrument.is_some() {
                TrackKind::Instrument
            } else {
                TrackKind::Audio
            };
            if ui
                .button(format!("{} · {}", patch.category, patch.name))
                .clicked()
            {
                self.add_track(kind, Some(patch.name));
                ui.close_menu();
            }
        }
    }
}

/// The region selection after a click on the header of `track`: kept when the region is on that track, dropped when
/// it is on another one. Clicking a region already selects its track, and clicking an empty lane drops the region,
/// so the region selection always belongs to the selected track and the Inspector never shows a region of a track
/// that is not the one selected. A click on the region's own header keeps it, to mute or rename the track while
/// editing the region.
pub(crate) fn region_after_track_click(project: &Project, region: Option<Id>, track: Id) -> Option<Id> {
    region.filter(|r| project.region_track_id(*r) == Some(track))
}

/// Where the rows of a track header `height` points tall sit, measured from its top.
#[derive(Debug, Clone, Copy, PartialEq)]
struct HeaderRows {
    /// One row: name and buttons side by side, no slider.
    compact: bool,
    /// Vertical center of the name row.
    name_center: f32,
    /// Top of the M/S/R buttons.
    buttons_top: f32,
}

impl HeaderRows {
    fn new(height: f32) -> Self {
        if height < HEADER_FULL_H {
            let top = button_row_top(height);
            HeaderRows { compact: true, name_center: top + HEADER_BUTTON_H / 2.0, buttons_top: top }
        } else {
            let block_top = button_row_top(height) - HEADER_NAME_H - HEADER_ROW_GAP;
            HeaderRows { compact: false, name_center: block_top + HEADER_NAME_H / 2.0, buttons_top: button_row_top(height) }
        }
    }
}

/// The state buttons of a track header, left to right. A bus records nothing, so it has no R; there is no input
/// monitoring switch in the model yet, so no I.
fn header_buttons(t: &Track) -> Vec<widgets::StateKind> {
    let mut v = vec![widgets::StateKind::Mute, widgets::StateKind::Solo];
    if t.kind != TrackKind::Bus {
        v.push(widgets::StateKind::Rec);
    }
    v
}

/// The short type label at the right of a track header's name row.
fn track_type_label(t: &Track) -> &'static str {
    match t.kind {
        TrackKind::Audio => "Audio",
        TrackKind::Bus => "Bus",
        TrackKind::Instrument => match t.instrument.as_ref().map(|i| i.kind.as_str()) {
            Some("drum_synth") => "Drums",
            Some("analog_synth") => "Synth",
            Some("fm_synth") => "FM",
            Some("sampler") => "Sampler",
            Some("organ") => "Organ",
            Some("pluck_synth") => "Pluck",
            _ => "Inst",
        },
    }
}

/// Position of a level on the header volume slider, 0 at the left end and 1 at +6 dB, on the mixer fader's scale.
fn header_volume_unit(db: f32) -> f32 {
    1.0 - widgets::db_to_fader(db.max(HEADER_VOLUME_MIN_DB))
}

/// Inverse of [`header_volume_unit`]; the left end sends the lowest level the model keeps instead of silence.
fn header_volume_db(unit: f32) -> f32 {
    widgets::fader_to_db(1.0 - unit.clamp(0.0, 1.0)).max(HEADER_VOLUME_MIN_DB)
}

/// A level as the track header writes it: one decimal near 0 dB, whole dB from -10 down, a real minus sign, and
/// "−∞" at the bottom of the scale.
fn db_text(db: f32) -> String {
    if db <= HEADER_VOLUME_MIN_DB + 0.05 {
        return "\u{2212}\u{221E}".into();
    }
    let s = if db.abs() >= 9.95 { format!("{:.0}", db.abs()) } else { format!("{:.1}", db.abs()) };
    if db < -0.05 {
        format!("\u{2212}{s}")
    } else {
        s
    }
}

/// The automation lanes of a track as menu entries ("Volume · 3" when the lane has points); the one picked.
fn automation_lane_menu(ui: &mut egui::Ui, choices: &[(automation_lane::LaneSpec, usize)], current: &str) -> Option<String> {
    let mut picked = None;
    for (spec, n) in choices {
        let label = if *n > 0 { format!("{} · {n}", spec.label) } else { spec.label.clone() };
        if ui.selectable_label(spec.target == current, label).clicked() {
            picked = Some(spec.target.clone());
        }
    }
    picked
}

/// Which part of a region the pointer is on: the body moves it, the right edge resizes it, the fade handles on
/// the top edge of an audio region change its fades.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum RegionZone {
    Body,
    Edge,
    FadeIn,
    FadeOut,
}

/// Distance of the M/S/R buttons from the top of a track header `height` points tall: centered on short tracks,
/// under the name row on taller ones, the two rows centered together.
fn button_row_top(height: f32) -> f32 {
    if height < HEADER_FULL_H {
        ((height - HEADER_BUTTON_H) / 2.0).max(2.0)
    } else {
        let block = HEADER_NAME_H + HEADER_ROW_GAP + HEADER_BUTTON_H;
        ((height - block) / 2.0).max(2.0) + HEADER_NAME_H + HEADER_ROW_GAP
    }
}

/// Every how many bars the ruler writes a number when a bar is `bar_px` pixels wide, so the numbers never touch.
fn bar_label_every(bar_px: f32) -> i64 {
    if bar_px < 18.0 {
        8
    } else if bar_px < 36.0 {
        4
    } else if bar_px < 60.0 {
        2
    } else {
        1
    }
}

/// Width of the resize strip for a region drawn `region_w` pixels wide.
fn region_edge_width(region_w: f32) -> f32 {
    REGION_EDGE_PX.min(region_w.max(0.0) / 3.0)
}

/// Zone under screen x for a region spanning `left..right` (x is expected inside that span).
fn region_hit_zone(x: f32, left: f32, right: f32) -> RegionZone {
    if x >= right - region_edge_width(right - left) {
        RegionZone::Edge
    } else {
        RegionZone::Body
    }
}

/// Zone under `p` for a region drawn in `rect`, whose fade handles (if any) are `handles`. The handles sit over the
/// body and the right edge, in the band at the top only, so the rest of the edge still resizes.
fn region_press_zone(p: Pos2, rect: Rect, handles: Option<[Rect; 2]>) -> RegionZone {
    match handles.and_then(|h| region_edit::fade_handle_at(p, h)) {
        Some(FadeHandle::In) => RegionZone::FadeIn,
        Some(FadeHandle::Out) => RegionZone::FadeOut,
        None => region_hit_zone(p.x, rect.left(), rect.right()),
    }
}

/// The chip drawn for a marker or chord whose grab area is `r`: two points shorter at the top and bottom, so the
/// chips float in the lane while the whole lane height stays easy to grab.
fn chip_rect(r: Rect) -> Rect {
    r.shrink2(vec2(0.0, 2.0))
}

/// (fill, text) of the `index`-th marker chip: the marker tints take turns, so neighbouring sections stand apart.
fn marker_chip_colors(index: usize) -> (Color32, Color32) {
    theme::MARKER_CHIPS[index % theme::MARKER_CHIPS.len()]
}

/// Background of the lane of the `index`-th track: rows alternate subtly, the selected track's lane is lighter.
fn lane_color(index: usize, selected: bool) -> Color32 {
    if selected {
        theme::LANE_SELECTED
    } else if index % 2 == 0 {
        theme::LANE_EVEN
    } else {
        theme::LANE_ODD
    }
}

/// Where the notes of a region are drawn: its left edge, the zoom, one repetition's length, the repetitions and the
/// content area under the name strip.
#[derive(Debug, Clone, Copy)]
struct NoteGeom {
    x0: f32,
    px_per_beat: f32,
    length: f64,
    loops: u32,
    content: Rect,
}

impl NoteGeom {
    /// Screen x span of `n` in repetition `k`; notes past the end of the region are cut there.
    fn span(&self, n: &Note, k: u32) -> (f32, f32) {
        let off = k as f64 * self.length;
        let x0 = self.x0 + ((n.start + off) as f32) * self.px_per_beat;
        let w = (n.length.min(self.length - n.start) as f32 * self.px_per_beat).max(2.0);
        (x0, x0 + w)
    }
}

/// MIDI notes as small bars, the lowest pitch at the bottom of the content area.
fn paint_note_bars(painter: &egui::Painter, notes: &[Note], g: NoteGeom) {
    let (Some(lo), Some(hi)) = (notes.iter().map(|n| n.pitch).min(), notes.iter().map(|n| n.pitch).max()) else {
        return;
    };
    let span = (hi - lo).max(12) as f32;
    let bar_h = (g.content.height() / (span + 1.0)).clamp(2.0, 4.0);
    for k in 0..g.loops {
        for n in notes.iter().filter(|n| n.start < g.length) {
            let (x0, x1) = g.span(n, k);
            let y = g.content.bottom() - ((n.pitch - lo) as f32 / span) * (g.content.height() - bar_h) - bar_h;
            let r = Rect::from_min_max(pos2(x0, y), pos2((x1 - 1.0).max(x0 + 2.0), y + bar_h));
            painter.rect_filled(r, Rounding::same(1.0_f32), theme::REGION_INK);
        }
    }
}

/// Drum hits as short marks on one row per kind of drum (cymbals and hats on top, kick at the bottom), so a beat
/// reads as a pattern instead of a band of dashes. Louder hits are brighter.
fn paint_drum_hits(painter: &egui::Painter, notes: &[Note], g: NoteGeom) {
    let rows = drum_rows(notes.iter().map(|n| n.pitch));
    if rows.is_empty() {
        return;
    }
    let row_h = g.content.height() / rows.len() as f32;
    let hit_h = (row_h - 2.0).clamp(1.5, 10.0);
    let hit_w = (g.px_per_beat * 0.25 * 0.6).clamp(2.0, 4.0);
    for k in 0..g.loops {
        for n in notes.iter().filter(|n| n.start < g.length) {
            let Some(row) = drum_row_index(&rows, n.pitch) else { continue };
            let (x0, _) = g.span(n, k);
            let cy = g.content.top() + (row as f32 + 0.5) * row_h;
            let r = Rect::from_center_size(pos2(x0 + hit_w / 2.0, cy), vec2(hit_w, hit_h));
            let a = 0.45 + 0.55 * (n.velocity as f32 / 127.0);
            painter.rect_filled(r, Rounding::same(1.0_f32), theme::REGION_INK.gamma_multiply(a));
        }
    }
}

/// A filled waveform from one `(x, top, bottom)` column per pixel.
fn paint_waveform(painter: &egui::Painter, columns: &[(f32, f32, f32)], color: Color32) {
    if columns.is_empty() {
        return;
    }
    let mut mesh = egui::Mesh::default();
    for (i, &(x, top, bottom)) in columns.iter().enumerate() {
        // at least a hairline where the audio is silent
        let (top, bottom) = if bottom - top < 1.0 { ((top + bottom) / 2.0 - 0.5, (top + bottom) / 2.0 + 0.5) } else { (top, bottom) };
        mesh.colored_vertex(pos2(x, top), color);
        mesh.colored_vertex(pos2(x + 1.0, bottom), color);
        mesh.colored_vertex(pos2(x + 1.0, top), color);
        mesh.colored_vertex(pos2(x, bottom), color);
        let b = (i * 4) as u32;
        mesh.add_triangle(b, b + 2, b + 1);
        mesh.add_triangle(b, b + 1, b + 3);
    }
    painter.add(egui::Shape::mesh(mesh));
}

/// Corner rounding of a region drawn in `rect` and cut to `clip`: square where the view cuts it.
fn region_rounding(rect: Rect, clip: Rect) -> Rounding {
    let r = theme::R_SM;
    let left = if clip.left() > rect.left() + 0.5 { 0.0 } else { r };
    let right = if clip.right() < rect.right() - 0.5 { 0.0 } else { r };
    let top = if clip.top() > rect.top() + 0.5 { 0.0 } else { 1.0 };
    let bottom = if clip.bottom() < rect.bottom() - 0.5 { 0.0 } else { 1.0 };
    Rounding { nw: left * top, ne: right * top, sw: left * bottom, se: right * bottom }
}

/// Kind of drum a General MIDI drum note belongs to, as a row from the top: 0 cymbals and hats, 1 snares, claps
/// and rims, 2 toms and other percussion, 3 kicks. Pitches outside the drum map count as percussion.
fn drum_kind(pitch: u8) -> u8 {
    match pitch {
        35 | 36 => 3,
        37..=40 => 1,
        42 | 44 | 46 | 49 | 51 | 52 | 53 | 55 | 57 | 59 => 0,
        _ => 2,
    }
}

/// The rows a drum region shows, top to bottom: only the kinds of drum it plays, so a kick-and-hat beat uses two
/// rows and each row stays tall.
fn drum_rows(pitches: impl IntoIterator<Item = u8>) -> Vec<u8> {
    let mut kinds: Vec<u8> = pitches.into_iter().map(drum_kind).collect();
    kinds.sort_unstable();
    kinds.dedup();
    kinds
}

/// Row of `pitch` among `rows` (from [`drum_rows`]).
fn drum_row_index(rows: &[u8], pitch: u8) -> Option<usize> {
    rows.iter().position(|k| *k == drum_kind(pitch))
}

/// Whether a region's notes are drawn as drum hits: step-sequencer patterns, and MIDI on a drum-kit track.
fn is_drum_region(r: &Region, t: &Track) -> bool {
    match &r.content {
        RegionContent::Pattern(_) => true,
        RegionContent::Midi(_) => is_drum_track(t),
        RegionContent::Audio(_) => false,
    }
}

fn is_drum_track(t: &Track) -> bool {
    t.instrument.as_ref().is_some_and(|i| i.kind == "drum_synth")
}

/// Shortest section a marker can be resized to, in beats.
const MIN_MARKER_LEN: f64 = 1.0;

/// `b` rounded to a grid of `snap` beats; 0 turns the grid off.
fn snap_to(b: f64, snap: f64) -> f64 {
    if snap > 0.0 {
        (b / snap).round() * snap
    } else {
        b
    }
}

/// Where a dragged marker or chord lands: it was grabbed at `grab_beat` and the pointer is now at `pointer_beat`.
fn dragged_marker_beat(orig_beat: f64, grab_beat: f64, pointer_beat: f64, snap: f64) -> f64 {
    snap_to(orig_beat + (pointer_beat - grab_beat), snap).max(0.0)
}

/// Length of a section marker starting at `start` whose right edge is dragged to `pointer_beat`.
/// The end snaps to the grid (not the length), so an off-grid section still ends on a grid line.
fn resized_marker_length(start: f64, pointer_beat: f64, snap: f64) -> f64 {
    (snap_to(pointer_beat, snap) - start).max(MIN_MARKER_LEN)
}

/// Shortest chord the right edge can be dragged to, in beats.
const MIN_CHORD_LEN: f64 = 0.25;

/// Where the chord lane sits on screen, and how the arrangement is scrolled and zoomed.
#[derive(Debug, Clone, Copy)]
struct ChordLaneGeom {
    /// Screen x of the timeline's left edge, where beat `scroll_beat` is drawn.
    timeline_x: f32,
    /// Screen x of the lane's right edge.
    right: f32,
    /// Screen y of the lane's top edge.
    top: f32,
    scroll_beat: f64,
    px_per_beat: f32,
}

/// Screen rect of the chord-lane span `beat..beat + len`, cut at the timeline's left edge; None when no part of it
/// is in view.
fn chord_span_rect(beat: f64, len: f64, g: ChordLaneGeom) -> Option<Rect> {
    let x = g.timeline_x + ((beat - g.scroll_beat) as f32) * g.px_per_beat;
    let w = len as f32 * g.px_per_beat;
    if x + w < g.timeline_x || x > g.right {
        return None;
    }
    Some(Rect::from_min_size(
        pos2(x.max(g.timeline_x), g.top + 1.0),
        vec2((w - 1.0 - (g.timeline_x - x).max(0.0)).max(1.0), LANE_H - 2.0),
    ))
}

/// Where the chord field goes this frame. It comes from the project, not from the chords drawn earlier in the
/// frame, so a field opened by the menu or a double-click shows on the same frame. None drops the edit: the chord
/// was deleted or undone away, or its span is out of view.
fn chord_field_rect(target: ChordTarget, chords: &[ChordEvent], g: ChordLaneGeom) -> Option<Rect> {
    match target {
        ChordTarget::Existing(id) => {
            let c = chords.iter().find(|c| c.id == id)?;
            chord_span_rect(c.beat, c.length, g)
        }
        ChordTarget::New { beat, length } => chord_span_rect(beat, length, g),
    }
}

/// Where a chord typed on the empty chord lane at `pointer_beat` goes: on the grid line at or before the pointer
/// (`snap` beats, 0 = no grid), `bar` beats long, but never over the chords around it. None when there is no room.
fn new_chord_span(pointer_beat: f64, snap: f64, bar: f64, chords: &[ChordEvent]) -> Option<(f64, f64)> {
    let floor = if snap > 0.0 { (pointer_beat / snap).floor() * snap } else { pointer_beat };
    let prev_end = chords.iter().filter(|c| c.beat <= pointer_beat).map(|c| c.beat + c.length).fold(0.0, f64::max);
    let next = chords.iter().filter(|c| c.beat > pointer_beat).map(|c| c.beat).fold(f64::INFINITY, f64::min);
    let beat = floor.max(prev_end).max(0.0);
    let len = bar.min(next - beat);
    (len > 1e-6).then_some((beat, len))
}

/// Length of a chord starting at `start` whose right edge is dragged to `pointer_beat`. The end snaps to the grid
/// and stops at `next_start` (the next chord), so lengthening a chord never covers the one after it.
fn resized_chord_length(start: f64, pointer_beat: f64, snap: f64, next_start: Option<f64>) -> f64 {
    let mut end = snap_to(pointer_beat, snap);
    if let Some(n) = next_start.filter(|n| *n > start) {
        end = end.min(n);
    }
    (end - start).max(MIN_CHORD_LEN)
}

/// Shortest cycle a drag on the ruler can make, in beats (a sixteenth note).
const MIN_CYCLE_LEN: f64 = 0.25;

/// Where the pointer is on the ruler's cycle strip.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CycleZone {
    Outside,
    Body,
    Start,
    End,
}

/// Zone under screen x for a cycle drawn from `x0` to `x1`. The edges can be grabbed from just outside the band too.
fn cycle_hit_zone(x: f32, x0: f32, x1: f32) -> CycleZone {
    let inner = CYCLE_EDGE_PX.min((x1 - x0).max(0.0) / 3.0);
    if x < x0 - CYCLE_EDGE_PX || x > x1 + CYCLE_EDGE_PX {
        CycleZone::Outside
    } else if x <= x0 + inner {
        CycleZone::Start
    } else if x >= x1 - inner {
        CycleZone::End
    } else {
        CycleZone::Body
    }
}

/// The cycle `start..end` grabbed at `grab_beat` with the pointer now at `pointer_beat`: same length, the start on
/// the grid, never before the song start.
fn moved_cycle(start: f64, end: f64, grab_beat: f64, pointer_beat: f64, snap: f64) -> (f64, f64) {
    let s = dragged_marker_beat(start, grab_beat, pointer_beat, snap);
    (s, s + (end - start))
}

/// The cycle after dragging its `edge` (Start or End) to `pointer_beat`. The dragged edge snaps to the grid and stays
/// at least MIN_CYCLE_LEN from the other one, so the edges never cross; the start never goes below 0.
fn resized_cycle(start: f64, end: f64, edge: CycleZone, pointer_beat: f64, snap: f64) -> (f64, f64) {
    let b = snap_to(pointer_beat, snap);
    match edge {
        CycleZone::Start => (b.min(end - MIN_CYCLE_LEN).max(0.0), end),
        CycleZone::End => (start, b.max(start + MIN_CYCLE_LEN)),
        CycleZone::Body | CycleZone::Outside => (start, end),
    }
}

/// What a drag on the ruler does, decided where the press began.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum RulerDrag {
    Playhead,
    DrawCycle,
    EditCycle(CycleZone),
}

fn ruler_drag_action(on_strip: bool, shift: bool, zone: CycleZone) -> RulerDrag {
    if shift {
        RulerDrag::DrawCycle
    } else if !on_strip {
        RulerDrag::Playhead
    } else if zone == CycleZone::Outside {
        RulerDrag::DrawCycle
    } else {
        RulerDrag::EditCycle(zone)
    }
}

/// What a click on the ruler does.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum RulerClick {
    Playhead,
    ToggleCycle,
}

fn ruler_click_action(on_strip: bool, zone: CycleZone) -> RulerClick {
    if on_strip && zone != CycleZone::Outside {
        RulerClick::ToggleCycle
    } else {
        RulerClick::Playhead
    }
}

/// The command a click on the cycle sends, the same as the C key.
fn cycle_toggle(c: Cycle) -> Command {
    Command::SetCycle {
        enabled: !c.enabled,
        start: None,
        end: None,
    }
}

/// Undo key for one cycle gesture: every frame of a drag merges into one step, and two drags in a row stay two.
pub(crate) fn cycle_gesture_key(gesture: u64) -> String {
    format!("cycle-{gesture}")
}

fn next_cycle_gesture() -> u64 {
    use std::sync::atomic::{AtomicU64, Ordering};
    static NEXT: AtomicU64 = AtomicU64::new(1);
    NEXT.fetch_add(1, Ordering::Relaxed)
}

/// The symbol to send after typing `typed` in the chord field, where `old` is the chord's current symbol (None for
/// a new chord). Ok(None): nothing to do (empty, or the same chord). Err: not a chord symbol or roman numeral.
fn chord_commit(old: Option<&str>, typed: &str, key: Key) -> Result<Option<String>, String> {
    let t = typed.trim();
    if t.is_empty() {
        return Ok(None);
    }
    let sym = resolve_chord_symbol(t, key)?;
    Ok((old != Some(sym.as_str())).then_some(sym))
}

/// Height of the band at the top and bottom of the track area where a dragged header scrolls the view.
const TRACK_DRAG_EDGE_PX: f32 = 24.0;
/// Fastest scroll speed of that band, in pixels per second.
const TRACK_DRAG_SCROLL_PX_S: f32 = 600.0;

/// Gap a dragged track header drops into: 0 is above the first track, `count` below the last.
/// `first_top` is the screen y of the first track's top edge (above the view when scrolled down);
/// over a track the pointer picks its nearer edge.
fn track_drop_index(pointer_y: f32, first_top: f32, height: f32, count: usize) -> usize {
    let g = ((pointer_y - first_top) / height + 0.5).floor();
    // max() also turns NaN into 0
    (g.max(0.0) as usize).min(count)
}

/// The `index` for `MoveTrack` (the track's final position) when the track at `from` is dropped into
/// `gap`, or None when it would stay where it is: the gaps just above and below it change nothing.
fn track_move_target(from: usize, gap: usize) -> Option<usize> {
    let to = if gap > from { gap - 1 } else { gap };
    (to != from).then_some(to)
}

/// Scroll speed (pixels per second, negative = up) while a track header is dragged at screen y
/// `pointer_y` over a track area spanning `top..bottom`: faster the deeper it is in the edge band.
fn edge_scroll_speed(pointer_y: f32, top: f32, bottom: f32) -> f32 {
    let up = (top + TRACK_DRAG_EDGE_PX - pointer_y) / TRACK_DRAG_EDGE_PX;
    let down = (pointer_y - (bottom - TRACK_DRAG_EDGE_PX)) / TRACK_DRAG_EDGE_PX;
    if up > 0.0 {
        -up.min(1.0) * TRACK_DRAG_SCROLL_PX_S
    } else if down > 0.0 {
        down.min(1.0) * TRACK_DRAG_SCROLL_PX_S
    } else {
        0.0
    }
}

/// The name to send after an in-place rename of a marker, a track or a region, or None when nothing should change.
pub(crate) fn rename_commit(old: &str, typed: &str) -> Option<String> {
    let t = typed.trim();
    (!t.is_empty() && t != old).then(|| t.to_string())
}

/// The command Enter (or a click elsewhere) sends from the track name field: None when the track is gone or the
/// name is empty or unchanged.
fn track_rename_command(ren: &InPlaceRename, project: &Project) -> Option<Command> {
    let old = &project.track(ren.id)?.name;
    rename_commit(old, &ren.text).map(|name| Command::RenameTrack { track_id: ren.id, name })
}

/// What the user did with an in-place name field this frame.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum RenameEdit {
    /// Still typing: keep the edit open.
    Editing,
    /// Enter, or a click elsewhere.
    Commit,
    /// Esc.
    Cancel,
}

/// Draws the in-place name field over `r` (at least 120 px wide). Call it after every widget it covers,
/// so it sits on top and gets the clicks.
pub(crate) fn rename_field(
    ui: &mut egui::Ui,
    edit_id: egui::Id,
    r: Rect,
    font: FontId,
    text: &mut String,
    focus: &mut bool,
) -> RenameEdit {
    let er = Rect::from_min_size(r.min, vec2(r.width().max(120.0), r.height()));
    let mut child = ui.new_child(egui::UiBuilder::new().max_rect(er));
    let resp = child.add(
        egui::TextEdit::singleline(text)
            .id(edit_id)
            .font(font)
            .margin(vec2(3.0, 1.0))
            .desired_width(er.width() - 6.0),
    );
    if *focus {
        resp.request_focus();
        // select the whole name so typing replaces it
        if let Some(mut st) = egui::TextEdit::load_state(ui.ctx(), edit_id) {
            let n = text.chars().count();
            st.cursor.set_char_range(Some(egui::text::CCursorRange::two(
                egui::text::CCursor::new(0),
                egui::text::CCursor::new(n),
            )));
            st.store(ui.ctx(), edit_id);
        }
        *focus = false;
    }
    if !resp.lost_focus() {
        return RenameEdit::Editing;
    }
    // egui drops the focus on Esc before the field sees it: check the key to tell cancel from commit
    if ui.input(|i| i.key_pressed(egui::Key::Escape)) {
        RenameEdit::Cancel
    } else {
        RenameEdit::Commit
    }
}

/// A stereo meter from two linear peak levels: two slim bars on the mixer fader's scale with the meter gradient. At
/// rest only the faint bar tracks show.
pub fn draw_meter(painter: &egui::Painter, r: Rect, m: [f32; 2]) {
    let w = ((r.width() - 1.0) / 2.0).max(1.0);
    for (c, level) in m.iter().enumerate() {
        let x = r.left() + c as f32 * (w + 1.0);
        let bar = Rect::from_min_max(pos2(x, r.top()), pos2(x + w, r.bottom()));
        painter.rect_filled(bar, Rounding::same(1.0_f32), theme::RAISED);
        let db = ondina_dsp::util::gain_to_db(*level);
        widgets::paint_meter_fill(painter, bar, widgets::meter_fill(db));
    }
}

#[cfg(test)]
mod tests {
    use crate::step_paint::RowRename;
    use super::{
        bar_label_every, db_text, drum_kind, drum_row_index, drum_rows, header_buttons, header_volume_db,
        header_volume_unit, is_drum_region, lane_color, region_after_track_click, region_rounding, track_type_label,
        HeaderRows, HEADER_NAME_H, HEADER_VOLUME_MIN_DB, REGION_HEAD_H, REGION_INSET_Y,
        button_row_top, chord_commit, chord_field_rect, chord_span_rect, cycle_gesture_key, track_rename_command, cycle_hit_zone, cycle_toggle, dragged_marker_beat,
        edge_scroll_speed, moved_cycle, new_chord_span, next_cycle_gesture, region_edge_width, region_hit_zone,
        region_press_zone, rename_commit, resized_chord_length, resized_cycle, resized_marker_length, ruler_click_action,
        ruler_drag_action, track_drop_index, track_move_target, CycleZone, RegionZone, RulerClick, RulerDrag,
        CYCLE_EDGE_PX, HEADER_BUTTON_H, HEADER_FULL_H, MIN_CHORD_LEN, MIN_CYCLE_LEN, MIN_MARKER_LEN, REGION_EDGE_PX,
        TRACK_DRAG_SCROLL_PX_S, ChordEdit, ChordLaneGeom, ChordTarget, InPlaceEdits, InPlaceRename, LANE_H,
    };
    use crate::arrange_view::{TRACK_H_MAX, TRACK_H_MIN};
    use crate::region_edit::{fade_handles, FadeGeom};
    use eframe::egui::{pos2, vec2, Rect};
    use ondina_core::command::Command;
    use ondina_core::model::ChordEvent;
    use ondina_core::theory::{Key, ScaleKind};
    use ondina_studio::{Session, Source};
    use serde_json::{json, Value};

    fn run(s: &mut Session, name: &str, args: Value) -> Value {
        let c = Command::from_tool_call(name, &args).unwrap();
        s.execute(c, Source::Gui).unwrap_or_else(|e| panic!("{name} {args} failed: {e}"))
    }

    fn track_names(s: &Session) -> Vec<String> {
        s.project.tracks.iter().map(|t| t.name.clone()).collect()
    }

    fn track_id(s: &Session, name: &str) -> u64 {
        s.project.tracks.iter().find(|t| t.name == name).unwrap().id
    }

    #[test]
    fn region_edge_hit_zones() {
        // 100 px wide region from x = 200 to x = 300
        assert_eq!(region_hit_zone(298.0, 200.0, 300.0), RegionZone::Edge);
        assert_eq!(region_hit_zone(300.0 - REGION_EDGE_PX, 200.0, 300.0), RegionZone::Edge);
        assert_eq!(region_hit_zone(250.0, 200.0, 300.0), RegionZone::Body);
        assert_eq!(region_hit_zone(200.0, 200.0, 300.0), RegionZone::Body);
        assert!((region_edge_width(100.0) - REGION_EDGE_PX).abs() < 1e-6);
    }

    #[test]
    fn narrow_region_keeps_a_body_to_grab() {
        let w = region_edge_width(10.0);
        assert!(w > 0.0 && w < 10.0, "edge strip {w} px on a 10 px region");
        assert_eq!(region_hit_zone(10.0, 0.0, 10.0), RegionZone::Edge);
        assert_eq!(region_hit_zone(0.0, 0.0, 10.0), RegionZone::Body);
        assert_eq!(region_hit_zone(5.0, 0.0, 10.0), RegionZone::Body);
        // the smallest region drawn is 4 px wide
        assert!(region_edge_width(4.0) < 4.0);
    }

    #[test]
    fn fade_handles_take_the_top_band_and_leave_the_edge_below() {
        let r = |x0: f32, x1: f32| Rect::from_min_max(pos2(x0, 100.0), pos2(x1, 140.0));
        let geom = |first: Rect, fade_in_px: f32| FadeGeom { first, reps: 1, fade_in_px, fade_out_px: 0.0 };
        let rect = r(200.0, 400.0);
        let handles = fade_handles(&geom(rect, 0.0));
        assert!(handles.is_some());
        let zone = |x: f32, y: f32, rect: Rect, h| region_press_zone(pos2(x, y), rect, h);
        assert_eq!(zone(202.0, 103.0, rect, handles), RegionZone::FadeIn);
        assert_eq!(zone(398.0, 103.0, rect, handles), RegionZone::FadeOut);
        assert_eq!(zone(398.0, 125.0, rect, handles), RegionZone::Edge);
        assert_eq!(zone(300.0, 103.0, rect, handles), RegionZone::Body);
        assert_eq!(zone(300.0, 125.0, rect, handles), RegionZone::Body);
        // a 40 px fade in moves its handle, and the corner is body again
        let handles = fade_handles(&geom(rect, 40.0));
        assert_eq!(zone(240.0, 104.0, rect, handles), RegionZone::FadeIn);
        assert_eq!(zone(202.0, 103.0, rect, handles), RegionZone::Body);
        // a region looped twice: the handles stay on the first repetition, the end of the region only resizes
        let looped = r(200.0, 600.0);
        let handles = fade_handles(&geom(rect, 0.0));
        assert_eq!(zone(398.0, 103.0, looped, handles), RegionZone::FadeOut);
        assert_eq!(zone(598.0, 125.0, looped, handles), RegionZone::Edge);
        assert_eq!(zone(598.0, 103.0, looped, handles), RegionZone::Edge);
        // a narrow region has no handles and keeps body and edge as before
        let narrow = r(200.0, 220.0);
        assert_eq!(fade_handles(&geom(narrow, 0.0)), None);
        assert_eq!(zone(202.0, 103.0, narrow, None), RegionZone::Body);
        assert_eq!(zone(219.0, 103.0, narrow, None), RegionZone::Edge);
    }

    #[test]
    fn marker_drag_snaps_and_stops_at_zero() {
        // grabbed at beat 10, pointer moved 3.2 beats to the right
        assert_eq!(dragged_marker_beat(8.0, 10.0, 13.2, 1.0), 11.0);
        assert!((dragged_marker_beat(8.0, 10.0, 13.2, 0.0) - 11.2).abs() < 1e-9);
        assert_eq!(dragged_marker_beat(8.0, 10.0, 11.9, 4.0), 8.0);
        // would land on beat -5
        assert_eq!(dragged_marker_beat(2.0, 10.0, 3.0, 1.0), 0.0);
        assert_eq!(dragged_marker_beat(2.0, 10.0, 3.0, 0.0), 0.0);
    }

    #[test]
    fn marker_resize_snaps_end_and_keeps_one_beat() {
        assert_eq!(resized_marker_length(16.0, 23.6, 1.0), 8.0);
        assert_eq!(resized_marker_length(16.0, 23.6, 4.0), 8.0);
        // off-grid start, the end still lands on the grid
        assert_eq!(resized_marker_length(1.5, 7.9, 4.0), 6.5);
        assert_eq!(resized_marker_length(16.0, 16.2, 1.0), MIN_MARKER_LEN);
        assert_eq!(resized_marker_length(16.0, 10.0, 1.0), MIN_MARKER_LEN);
        assert!((resized_marker_length(16.0, 19.3, 0.0) - 3.3).abs() < 1e-9);
    }

    fn chord(id: u64, beat: f64, length: f64) -> ChordEvent {
        ChordEvent { id, beat, length, symbol: "C".into() }
    }

    fn close(a: Option<(f64, f64)>, b: (f64, f64)) -> bool {
        a.is_some_and(|(x, y)| (x - b.0).abs() < 1e-9 && (y - b.1).abs() < 1e-9)
    }

    #[test]
    fn new_chord_snaps_to_the_grid_and_stops_at_the_next_chord() {
        assert_eq!(new_chord_span(5.7, 1.0, 4.0, &[]), Some((5.0, 4.0)));
        assert!(close(new_chord_span(5.7, 0.0, 4.0, &[]), (5.7, 4.0)));
        assert_eq!(new_chord_span(5.7, 4.0, 4.0, &[]), Some((4.0, 4.0)));
        assert_eq!(new_chord_span(-0.5, 1.0, 4.0, &[]), Some((0.0, 4.0)));
        // shortened so it ends where the next chord begins
        assert_eq!(new_chord_span(6.2, 1.0, 4.0, &[chord(1, 8.0, 4.0)]), Some((6.0, 2.0)));
        // starts where the chord before it ends, even off the grid
        assert_eq!(new_chord_span(2.7, 1.0, 4.0, &[chord(1, 0.0, 2.5)]), Some((2.5, 4.0)));
        // no room between overlapping chords
        assert_eq!(new_chord_span(3.9, 1.0, 4.0, &[chord(1, 0.0, 6.0), chord(2, 4.0, 4.0)]), None);
    }

    #[test]
    fn chord_resize_snaps_the_end_and_stops_at_the_next_chord() {
        assert_eq!(resized_chord_length(0.0, 5.6, 1.0, None), 6.0);
        assert_eq!(resized_chord_length(0.0, 5.6, 1.0, Some(4.0)), 4.0);
        assert_eq!(resized_chord_length(1.5, 7.9, 4.0, None), 6.5);
        assert_eq!(resized_chord_length(8.0, 8.1, 1.0, None), MIN_CHORD_LEN);
        assert_eq!(resized_chord_length(8.0, 3.0, 1.0, None), MIN_CHORD_LEN);
        assert!((resized_chord_length(0.0, 3.3, 0.0, None) - 3.3).abs() < 1e-9);
        // a next chord before the start is not in the way
        assert_eq!(resized_chord_length(8.0, 11.6, 1.0, Some(2.0)), 4.0);
    }

    #[test]
    fn chord_commit_resolves_roman_numerals_and_rejects_unknown_symbols() {
        let key = Key { root: 0, scale: ScaleKind::Major };
        assert_eq!(chord_commit(None, "IV", key), Ok(Some("F".into())));
        assert_eq!(chord_commit(None, " Am7 ", key), Ok(Some("Am7".into())));
        assert_eq!(chord_commit(None, "   ", key), Ok(None));
        assert_eq!(chord_commit(Some("G"), "V", key), Ok(None));
        assert_eq!(chord_commit(Some("G"), "G", key), Ok(None));
        assert_eq!(chord_commit(Some("G"), "G7", key), Ok(Some("G7".into())));
        let e = chord_commit(None, "H9x", key).unwrap_err();
        assert!(e.contains("cannot parse chord"), "{e}");
    }

    #[test]
    fn marker_rename_trims_and_ignores_empty_or_unchanged() {
        assert_eq!(rename_commit("Verse", "  Chorus "), Some("Chorus".into()));
        assert_eq!(rename_commit("Verse", "Verse"), None);
        assert_eq!(rename_commit("Verse", " Verse "), None);
        assert_eq!(rename_commit("Verse", "   "), None);
        assert_eq!(rename_commit("Verse", ""), None);
    }

    #[test]
    fn rename_commit_keeps_inner_spaces_and_case_changes() {
        assert_eq!(rename_commit("Bass", "Lead  Vox"), Some("Lead  Vox".into()));
        assert_eq!(rename_commit("bass", "Bass"), Some("Bass".into()));
        assert_eq!(rename_commit("Drums", "  Batteria 🥁 "), Some("Batteria 🥁".into()));
    }

    #[test]
    fn track_drop_index_picks_the_nearest_gap() {
        // four tracks of 50 px from y = 100: 100-150, 150-200, 200-250, 250-300
        assert_eq!(track_drop_index(80.0, 100.0, 50.0, 4), 0);
        assert_eq!(track_drop_index(110.0, 100.0, 50.0, 4), 0);
        assert_eq!(track_drop_index(140.0, 100.0, 50.0, 4), 1);
        assert_eq!(track_drop_index(190.0, 100.0, 50.0, 4), 2);
        assert_eq!(track_drop_index(210.0, 100.0, 50.0, 4), 2);
        assert_eq!(track_drop_index(290.0, 100.0, 50.0, 4), 4);
        assert_eq!(track_drop_index(500.0, 100.0, 50.0, 4), 4);
        assert_eq!(track_drop_index(-1000.0, 100.0, 50.0, 4), 0);
        // scrolled down: the first track starts above the view
        assert_eq!(track_drop_index(110.0, -50.0, 50.0, 4), 3);
        assert_eq!(track_drop_index(110.0, -400.0, 50.0, 4), 4);
        // no tracks
        assert_eq!(track_drop_index(300.0, 100.0, 50.0, 0), 0);
    }

    #[test]
    fn track_move_target_is_none_when_the_track_stays() {
        assert_eq!(track_move_target(2, 2), None);
        assert_eq!(track_move_target(2, 3), None);
        assert_eq!(track_move_target(3, 4), None);
        assert_eq!(track_move_target(2, 0), Some(0));
        assert_eq!(track_move_target(2, 1), Some(1));
        assert_eq!(track_move_target(0, 2), Some(1));
        assert_eq!(track_move_target(0, 4), Some(3));
        assert_eq!(track_move_target(3, 0), Some(0));
    }

    #[test]
    fn dropping_a_header_moves_the_track_to_the_line_in_one_undo_step() {
        let mut s = Session::new(false);
        for name in ["Drums", "Bass", "Keys", "Vox"] {
            run(&mut s, "add_track", json!({"kind": "instrument", "name": name}));
        }
        let start = track_names(&s);
        assert_eq!(start, ["Drums", "Bass", "Keys", "Vox"]);
        let cases = [
            ("Vox", 0, ["Vox", "Drums", "Bass", "Keys"]),
            ("Drums", 4, ["Bass", "Keys", "Vox", "Drums"]),
            ("Bass", 3, ["Drums", "Keys", "Bass", "Vox"]),
            ("Keys", 1, ["Drums", "Keys", "Bass", "Vox"]),
        ];
        for (name, gap, expected) in cases {
            let id = track_id(&s, name);
            let from = s.project.track_index(id).unwrap();
            let index = track_move_target(from, gap).unwrap();
            run(&mut s, "move_track", json!({"track_id": id, "index": index}));
            assert_eq!(track_names(&s), expected, "{name} dropped into gap {gap}");
            run(&mut s, "undo", json!({}));
            assert_eq!(track_names(&s), start, "undo after moving {name}");
        }
    }

    #[test]
    fn drag_near_the_edges_scrolls_the_tracks() {
        // track area from y = 100 to y = 500, 24 px bands
        assert_eq!(edge_scroll_speed(300.0, 100.0, 500.0), 0.0);
        assert_eq!(edge_scroll_speed(124.0, 100.0, 500.0), 0.0);
        assert_eq!(edge_scroll_speed(476.0, 100.0, 500.0), 0.0);
        assert_eq!(edge_scroll_speed(100.0, 100.0, 500.0), -TRACK_DRAG_SCROLL_PX_S);
        assert_eq!(edge_scroll_speed(50.0, 100.0, 500.0), -TRACK_DRAG_SCROLL_PX_S);
        assert_eq!(edge_scroll_speed(500.0, 100.0, 500.0), TRACK_DRAG_SCROLL_PX_S);
        assert_eq!(edge_scroll_speed(600.0, 100.0, 500.0), TRACK_DRAG_SCROLL_PX_S);
        assert!((edge_scroll_speed(112.0, 100.0, 500.0) + TRACK_DRAG_SCROLL_PX_S / 2.0).abs() < 1e-3);
        assert!((edge_scroll_speed(488.0, 100.0, 500.0) - TRACK_DRAG_SCROLL_PX_S / 2.0).abs() < 1e-3);
    }

    #[test]
    fn track_rename_is_one_undo_step() {
        let mut s = Session::new(false);
        run(&mut s, "add_track", json!({"kind": "instrument", "name": "Bass"}));
        let id = track_id(&s, "Bass");
        let name = rename_commit("Bass", "  Bass DI ").unwrap();
        assert_eq!(name, "Bass DI");
        run(&mut s, "rename_track", json!({"track_id": id, "name": name}));
        assert_eq!(track_names(&s), ["Bass DI"]);
        run(&mut s, "undo", json!({}));
        assert_eq!(track_names(&s), ["Bass"]);
        assert_eq!(rename_commit("Bass", " Bass "), None);
        assert_eq!(rename_commit("Bass", ""), None);
    }

    #[test]
    fn chord_lane_edits_are_one_undo_step_each() {
        let mut s = Session::new(false);
        let (beat, length) = new_chord_span(5.7, 1.0, 4.0, &s.project.chords).unwrap();
        let sym = chord_commit(None, "vi", s.project.key).unwrap().unwrap();
        let id = run(&mut s, "add_chord", json!({"beat": beat, "length": length, "symbol": sym}))["chord_id"].as_u64().unwrap();
        let state = |s: &Session| {
            let c = &s.project.chords[0];
            (c.symbol.clone(), c.beat, c.length)
        };
        assert_eq!(state(&s), ("Am".into(), 5.0, 4.0));

        let b = dragged_marker_beat(5.0, 6.0, 9.4, 1.0);
        assert_eq!(b, 8.0);
        run(&mut s, "update_chord", json!({"chord_id": id, "beat": b}));
        let l = resized_chord_length(8.0, 13.7, 1.0, None);
        assert_eq!(l, 6.0);
        run(&mut s, "update_chord", json!({"chord_id": id, "length": l}));
        let sym = chord_commit(Some("Am"), "IV", s.project.key).unwrap().unwrap();
        run(&mut s, "update_chord", json!({"chord_id": id, "symbol": sym}));
        assert_eq!(state(&s), ("F".into(), 8.0, 6.0));

        run(&mut s, "undo", json!({}));
        assert_eq!(state(&s), ("Am".into(), 8.0, 6.0));
        run(&mut s, "undo", json!({}));
        assert_eq!(state(&s), ("Am".into(), 8.0, 4.0));
        run(&mut s, "undo", json!({}));
        assert_eq!(state(&s), ("Am".into(), 5.0, 4.0));
        run(&mut s, "undo", json!({}));
        assert!(s.project.chords.is_empty());
    }

    #[test]
    fn chord_field_opens_on_the_same_frame_and_drops_when_the_chord_is_gone() {
        let g = ChordLaneGeom { timeline_x: 210.0, right: 1010.0, top: 48.0, scroll_beat: 0.0, px_per_beat: 20.0 };
        let chords = [chord(7, 4.0, 4.0), chord(8, 100.0, 4.0)];
        // opened from the menu on a visible chord: the rect comes from the project alone
        let r = chord_field_rect(ChordTarget::Existing(7), &chords, g);
        assert_eq!(r, Some(Rect::from_min_size(pos2(290.0, 49.0), vec2(79.0, LANE_H - 2.0))));
        // the same rect the lane draws for that chord
        assert_eq!(r, chord_span_rect(4.0, 4.0, g));
        // out of view, or deleted / undone away: no field
        assert_eq!(chord_field_rect(ChordTarget::Existing(8), &chords, g), None);
        assert_eq!(chord_field_rect(ChordTarget::Existing(9), &chords, g), None);
        // scrolled so the chord starts left of the timeline: cut at the edge
        let scrolled = ChordLaneGeom { scroll_beat: 6.0, ..g };
        assert_eq!(
            chord_field_rect(ChordTarget::Existing(7), &chords, scrolled),
            Some(Rect::from_min_size(pos2(210.0, 49.0), vec2(39.0, LANE_H - 2.0)))
        );
        let gone = ChordLaneGeom { scroll_beat: 9.0, ..g };
        assert_eq!(chord_field_rect(ChordTarget::Existing(7), &chords, gone), None);
        // a new chord: its span, or nothing when it is out of view
        assert_eq!(
            chord_field_rect(ChordTarget::New { beat: 0.0, length: 4.0 }, &chords, g),
            Some(Rect::from_min_size(pos2(210.0, 49.0), vec2(79.0, LANE_H - 2.0)))
        );
        assert_eq!(chord_field_rect(ChordTarget::New { beat: 60.0, length: 4.0 }, &chords, g), None);
    }

    #[test]
    fn in_place_edits_close_when_the_project_is_replaced() {
        let mut s = Session::new(false);
        run(&mut s, "add_track", json!({"kind": "instrument", "name": "Bass"}));
        let id = track_id(&s, "Bass");
        let field = |text: &str| InPlaceRename { id, text: text.into(), focus: false };
        let mut edits = InPlaceEdits {
            marker_rename: Some(field("Verse")),
            track_rename: Some(field("Lead")),
            region_rename: Some(field("Take 2")),
            region_name_edit: Some((id, "Take 2".into())),
            chord_edit: Some(ChordEdit {
                target: ChordTarget::New { beat: 0.0, length: 4.0 },
                text: "Am".into(),
                focus: false,
                invalid: false,
            }),
            row_rename: Some(RowRename { region: id, row: 0, rows: 8, text: "Kick".into(), focus: false }),
        };
        // an ordinary edit is the same document: the fields stay open
        let shown = s.document;
        run(&mut s, "set_tempo", json!({"bpm": 100}));
        edits.follow_document(shown, s.document);
        assert!(edits.track_rename.is_some() && edits.chord_edit.is_some());
        assert!(edits.row_rename.is_some());

        let shown = s.document;
        run(&mut s, "new_project", json!({}));
        run(&mut s, "add_track", json!({"kind": "instrument", "name": "Drums"}));
        // ids start again in the new project: the old field would rename the new track
        assert_eq!(track_id(&s, "Drums"), id);
        assert!(track_rename_command(edits.track_rename.as_ref().unwrap(), &s.project).is_some());

        edits.follow_document(shown, s.document);
        assert!(edits.marker_rename.is_none());
        assert!(edits.track_rename.is_none());
        assert!(edits.region_rename.is_none());
        assert!(edits.region_name_edit.is_none());
        assert!(edits.chord_edit.is_none());
        assert!(edits.row_rename.is_none());
        // Enter now has nothing to send, and the new track keeps its name
        assert!(edits.track_rename.as_ref().and_then(|r| track_rename_command(r, &s.project)).is_none());
        assert_eq!(track_names(&s), ["Drums"]);
    }

    #[test]
    fn header_buttons_fit_at_every_track_height() {
        for h in [TRACK_H_MIN, 40.0, HEADER_FULL_H - 0.5, HEADER_FULL_H, 58.0, TRACK_H_MAX] {
            assert!(button_row_top(h) + HEADER_BUTTON_H <= h - 2.0, "height {h}");
        }
        // the default height keeps the full layout: name row and buttons centered together
        assert_eq!(button_row_top(58.0), 31.0);
        const { assert!(TRACK_H_MIN < HEADER_FULL_H) };
    }

    #[test]
    fn cycle_strip_hit_zones() {
        // cycle drawn from x = 200 to x = 300
        assert_eq!(cycle_hit_zone(150.0, 200.0, 300.0), CycleZone::Outside);
        assert_eq!(cycle_hit_zone(200.0 - CYCLE_EDGE_PX - 1.0, 200.0, 300.0), CycleZone::Outside);
        assert_eq!(cycle_hit_zone(195.0, 200.0, 300.0), CycleZone::Start);
        assert_eq!(cycle_hit_zone(200.0, 200.0, 300.0), CycleZone::Start);
        assert_eq!(cycle_hit_zone(200.0 + CYCLE_EDGE_PX, 200.0, 300.0), CycleZone::Start);
        assert_eq!(cycle_hit_zone(210.0, 200.0, 300.0), CycleZone::Body);
        assert_eq!(cycle_hit_zone(250.0, 200.0, 300.0), CycleZone::Body);
        assert_eq!(cycle_hit_zone(300.0 - CYCLE_EDGE_PX, 200.0, 300.0), CycleZone::End);
        assert_eq!(cycle_hit_zone(300.0, 200.0, 300.0), CycleZone::End);
        assert_eq!(cycle_hit_zone(305.0, 200.0, 300.0), CycleZone::End);
        assert_eq!(cycle_hit_zone(300.0 + CYCLE_EDGE_PX + 1.0, 200.0, 300.0), CycleZone::Outside);
        // a narrow cycle keeps a body to grab: 9 px wide, 3 px edges inside
        assert_eq!(cycle_hit_zone(202.0, 200.0, 209.0), CycleZone::Start);
        assert_eq!(cycle_hit_zone(204.5, 200.0, 209.0), CycleZone::Body);
        assert_eq!(cycle_hit_zone(207.0, 200.0, 209.0), CycleZone::End);
        // zero width: left of it grabs the start, right of it the end
        assert_eq!(cycle_hit_zone(198.0, 200.0, 200.0), CycleZone::Start);
        assert_eq!(cycle_hit_zone(202.0, 200.0, 200.0), CycleZone::End);
    }

    #[test]
    fn cycle_move_snaps_keeps_its_length_and_stops_at_zero() {
        // grabbed at beat 10, pointer moved 3.2 beats to the right
        assert_eq!(moved_cycle(8.0, 16.0, 10.0, 13.2, 1.0), (11.0, 19.0));
        let (s, e) = moved_cycle(8.0, 16.0, 10.0, 13.2, 0.0);
        assert!((s - 11.2).abs() < 1e-9 && (e - 19.2).abs() < 1e-9);
        // a length that is not a multiple of the grid stays the same
        assert_eq!(moved_cycle(1.0, 3.5, 2.0, 6.2, 1.0), (5.0, 7.5));
        // would start at beat -5: stops at 0 and keeps 4 beats
        assert_eq!(moved_cycle(2.0, 6.0, 10.0, 3.0, 1.0), (0.0, 4.0));
    }

    #[test]
    fn cycle_resize_keeps_a_sixteenth_and_edges_never_cross() {
        assert_eq!(resized_cycle(8.0, 16.0, CycleZone::End, 19.6, 1.0), (8.0, 20.0));
        assert_eq!(resized_cycle(8.0, 16.0, CycleZone::End, 17.9, 4.0), (8.0, 16.0));
        assert_eq!(resized_cycle(8.0, 16.0, CycleZone::End, 2.0, 1.0), (8.0, 8.0 + MIN_CYCLE_LEN));
        assert_eq!(resized_cycle(8.0, 16.0, CycleZone::Start, 5.4, 1.0), (5.0, 16.0));
        assert_eq!(resized_cycle(8.0, 16.0, CycleZone::Start, 30.0, 1.0), (16.0 - MIN_CYCLE_LEN, 16.0));
        assert_eq!(resized_cycle(8.0, 16.0, CycleZone::Start, -3.0, 1.0), (0.0, 16.0));
        let (s, e) = resized_cycle(8.0, 16.0, CycleZone::Start, 9.3, 0.0);
        assert!((s - 9.3).abs() < 1e-9);
        assert_eq!(e, 16.0);
        // the body is not an edge: nothing changes
        assert_eq!(resized_cycle(8.0, 16.0, CycleZone::Body, 12.0, 1.0), (8.0, 16.0));
    }

    #[test]
    fn ruler_lower_half_and_shift_drag_work_as_before() {
        use CycleZone::*;
        for z in [Outside, Body, Start, End] {
            assert_eq!(ruler_drag_action(false, false, z), RulerDrag::Playhead, "lower half {z:?}");
            assert_eq!(ruler_click_action(false, z), RulerClick::Playhead, "lower half {z:?}");
            assert_eq!(ruler_drag_action(false, true, z), RulerDrag::DrawCycle, "Shift on the lower half {z:?}");
            assert_eq!(ruler_drag_action(true, true, z), RulerDrag::DrawCycle, "Shift on the strip {z:?}");
        }
        assert_eq!(ruler_drag_action(true, false, Outside), RulerDrag::DrawCycle);
        for z in [Body, Start, End] {
            assert_eq!(ruler_drag_action(true, false, z), RulerDrag::EditCycle(z));
            assert_eq!(ruler_click_action(true, z), RulerClick::ToggleCycle);
        }
        assert_eq!(ruler_click_action(true, Outside), RulerClick::Playhead);
        assert_ne!(cycle_gesture_key(next_cycle_gesture()), cycle_gesture_key(next_cycle_gesture()));
    }

    #[test]
    fn cycle_strip_gestures_are_one_undo_step_each() {
        let mut s = Session::new(false);
        run(&mut s, "set_cycle", json!({"enabled": true, "start": 8.0, "end": 16.0}));
        let cycle = |s: &Session| (s.project.cycle.enabled, s.project.cycle.start, s.project.cycle.end);
        let send = |s: &mut Session, (start, end): (f64, f64), key: &str| {
            let c = Command::SetCycle { enabled: true, start: Some(start), end: Some(end) };
            s.execute_coalesced(c, key).unwrap();
        };
        // body drag grabbed at beat 10: the frames of one gesture share its key
        let key = cycle_gesture_key(next_cycle_gesture());
        for b in [11.3, 13.8, 14.2] {
            send(&mut s, moved_cycle(8.0, 16.0, 10.0, b, 1.0), &key);
        }
        assert_eq!(cycle(&s), (true, 12.0, 20.0));
        // right edge dragged right after: a gesture of its own
        let key = cycle_gesture_key(next_cycle_gesture());
        for b in [21.6, 23.9] {
            send(&mut s, resized_cycle(12.0, 20.0, CycleZone::End, b, 1.0), &key);
        }
        assert_eq!(cycle(&s), (true, 12.0, 24.0));
        // a click on the band
        let c = cycle_toggle(s.project.cycle);
        s.execute(c, Source::Gui).unwrap();
        assert_eq!(cycle(&s), (false, 12.0, 24.0));

        run(&mut s, "undo", json!({}));
        assert_eq!(cycle(&s), (true, 12.0, 24.0));
        run(&mut s, "undo", json!({}));
        assert_eq!(cycle(&s), (true, 12.0, 20.0));
        run(&mut s, "undo", json!({}));
        assert_eq!(cycle(&s), (true, 8.0, 16.0));
    }

    #[test]
    fn header_rows_never_overlap_and_stay_inside_the_header() {
        for h in [TRACK_H_MIN, 40.0, HEADER_FULL_H - 0.5, HEADER_FULL_H, 58.0, 72.0, TRACK_H_MAX] {
            let r = HeaderRows::new(h);
            assert_eq!(r.compact, h < HEADER_FULL_H, "height {h}");
            assert!(r.buttons_top >= 2.0 && r.buttons_top + HEADER_BUTTON_H <= h - 2.0, "height {h}: {r:?}");
            if r.compact {
                // one row: the name is centered on the buttons
                assert!((r.name_center - (r.buttons_top + HEADER_BUTTON_H / 2.0)).abs() < 1e-3, "height {h}");
            } else {
                assert!(r.name_center + HEADER_NAME_H / 2.0 <= r.buttons_top, "height {h}: {r:?}");
                assert!(r.name_center - HEADER_NAME_H / 2.0 >= 2.0, "height {h}: {r:?}");
                // centered: the space above the name row equals the space below the buttons
                let above = r.name_center - HEADER_NAME_H / 2.0;
                let below = h - r.buttons_top - HEADER_BUTTON_H;
                assert!((above - below).abs() < 1e-3, "height {h}: {r:?}");
            }
        }
    }

    #[test]
    fn region_name_strip_ends_where_the_automation_overlay_begins() {
        // presses below the name strip go to the automation overlay when the view is on
        assert_eq!(REGION_INSET_Y + REGION_HEAD_H, crate::automation_lane::TOP_BAND_PX);
    }

    #[test]
    fn header_volume_follows_the_fader_scale_and_round_trips() {
        assert_eq!(header_volume_unit(6.0), 1.0);
        assert_eq!(header_volume_unit(12.0), 1.0);
        assert_eq!(header_volume_unit(f32::NEG_INFINITY), header_volume_unit(HEADER_VOLUME_MIN_DB));
        assert!(header_volume_unit(HEADER_VOLUME_MIN_DB) < 0.01);
        for db in [6.0, 0.0, -4.0, -6.0, -12.0, -18.0, -30.0, -48.0, -60.0] {
            let back = header_volume_db(header_volume_unit(db));
            assert!((back - db).abs() < 0.05, "{db} came back as {back}");
        }
        // the left end sends a level the model keeps, never silence (-inf is not valid JSON)
        assert_eq!(header_volume_db(0.0), HEADER_VOLUME_MIN_DB);
        assert!(header_volume_db(-1.0).is_finite());
        assert_eq!(header_volume_db(2.0), 6.0);
        // louder is further right
        assert!(header_volume_unit(-6.0) < header_volume_unit(-4.0));
    }

    #[test]
    fn header_volume_text_matches_the_design() {
        assert_eq!(db_text(-4.0), "\u{2212}4.0");
        assert_eq!(db_text(-18.0), "\u{2212}18");
        assert_eq!(db_text(-9.96), "\u{2212}10");
        assert_eq!(db_text(0.0), "0.0");
        assert_eq!(db_text(-0.04), "0.0");
        assert_eq!(db_text(3.5), "3.5");
        assert_eq!(db_text(HEADER_VOLUME_MIN_DB), "\u{2212}\u{221E}");
    }

    #[test]
    fn drum_notes_get_one_row_per_kind_of_drum() {
        // cymbals and hats on top, then snares and claps, toms and percussion, kick at the bottom
        assert_eq!([drum_kind(42), drum_kind(46), drum_kind(49)], [0, 0, 0]);
        assert_eq!([drum_kind(38), drum_kind(39), drum_kind(40)], [1, 1, 1]);
        assert_eq!([drum_kind(45), drum_kind(50), drum_kind(56)], [2, 2, 2]);
        assert_eq!([drum_kind(35), drum_kind(36)], [3, 3]);
        // outside the drum map: percussion
        assert_eq!([drum_kind(20), drum_kind(100)], [2, 2]);
        // a kick, snare and hat beat uses three rows, top to bottom
        let beat = [36, 42, 38, 42, 36, 42, 38, 46];
        let rows = drum_rows(beat);
        assert_eq!(rows, [0, 1, 3]);
        assert_eq!(drum_row_index(&rows, 42), Some(0));
        assert_eq!(drum_row_index(&rows, 46), Some(0));
        assert_eq!(drum_row_index(&rows, 38), Some(1));
        assert_eq!(drum_row_index(&rows, 36), Some(2));
        assert_eq!(drum_row_index(&rows, 45), None);
        // kick and hats only: two rows, each twice as tall
        assert_eq!(drum_rows([36, 42]).len(), 2);
        assert!(drum_rows([]).is_empty());
    }

    #[test]
    fn patterns_and_drum_kit_midi_draw_as_hits() {
        let mut s = Session::new(false);
        let drums = run(&mut s, "add_track", json!({"kind": "instrument", "name": "Drums", "patch": "Pop Drum Kit"}))["track_id"].as_u64().unwrap();
        let keys = run(&mut s, "add_track", json!({"kind": "instrument", "name": "Keys"}))["track_id"].as_u64().unwrap();
        let bus = run(&mut s, "add_track", json!({"kind": "bus", "name": "Verb"}))["track_id"].as_u64().unwrap();
        run(&mut s, "create_midi_region", json!({"track_id": drums, "start": 0.0, "length": 4.0}));
        run(&mut s, "create_midi_region", json!({"track_id": keys, "start": 0.0, "length": 4.0}));
        run(&mut s, "create_pattern_region", json!({"track_id": keys, "start": 4.0}));
        let p = &s.project;
        let t = |id: u64| p.track(id).unwrap();
        assert!(is_drum_region(&t(drums).regions[0], t(drums)));
        assert!(!is_drum_region(&t(keys).regions[0], t(keys)));
        assert!(is_drum_region(&t(keys).regions[1], t(keys)), "a pattern always draws as hits");
        assert_eq!(track_type_label(t(drums)), "Drums");
        assert_eq!(track_type_label(t(bus)), "Bus");
        assert!(!track_type_label(t(keys)).is_empty());
        // a bus has no record arm
        assert_eq!(header_buttons(t(bus)).len(), 2);
        assert_eq!(header_buttons(t(keys)).len(), 3);
    }

    #[test]
    fn clicking_a_header_drops_a_region_selected_on_another_track() {
        let mut s = Session::new(false);
        let a = run(&mut s, "add_track", json!({"kind": "instrument", "name": "Bass"}))["track_id"].as_u64().unwrap();
        let b = run(&mut s, "add_track", json!({"kind": "instrument", "name": "Keys"}))["track_id"].as_u64().unwrap();
        let r = run(&mut s, "create_midi_region", json!({"track_id": a, "start": 0.0, "length": 4.0}))["region_id"].as_u64().unwrap();
        // another track's header: the region row would describe a track that is not selected
        assert_eq!(region_after_track_click(&s.project, Some(r), b), None);
        // the region's own header keeps it
        assert_eq!(region_after_track_click(&s.project, Some(r), a), Some(r));
        assert_eq!(region_after_track_click(&s.project, None, a), None);
        // a region that is gone is dropped too
        assert_eq!(region_after_track_click(&s.project, Some(9999), a), None);
    }

    #[test]
    fn ruler_numbers_thin_out_when_bars_are_narrow() {
        assert_eq!(bar_label_every(100.0), 1);
        assert_eq!(bar_label_every(40.0), 2);
        assert_eq!(bar_label_every(20.0), 4);
        assert_eq!(bar_label_every(8.0), 8);
    }

    #[test]
    fn lanes_alternate_and_the_selected_lane_stands_out() {
        assert_ne!(lane_color(0, false), lane_color(1, false));
        assert_eq!(lane_color(0, false), lane_color(2, false));
        assert_ne!(lane_color(0, true), lane_color(0, false));
        assert_ne!(lane_color(1, true), lane_color(1, false));
    }

    #[test]
    fn regions_cut_by_the_view_have_square_corners_there() {
        let rect = Rect::from_min_max(pos2(100.0, 50.0), pos2(300.0, 100.0));
        let whole = region_rounding(rect, rect);
        assert!(whole.nw > 0.0 && whole.ne > 0.0 && whole.sw > 0.0 && whole.se > 0.0);
        let cut_left = region_rounding(rect, Rect::from_min_max(pos2(224.0, 50.0), rect.max));
        assert_eq!((cut_left.nw, cut_left.sw), (0.0, 0.0));
        assert!(cut_left.ne > 0.0 && cut_left.se > 0.0);
    }

}
