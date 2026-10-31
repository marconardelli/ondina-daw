//! Tracks area: headers, ruler, marker & chord lanes, regions, playhead.

use ondina_core::command::Command;
use ondina_core::model::*;
use ondina_core::ops::pattern_to_notes;
use eframe::egui::{
    self, pos2, vec2, Align2, Color32, FontId, Pos2, Rect, Rounding, Sense, Stroke,
};

use crate::app::{OndinaApp, BottomTab, Drag};
use crate::theme;

const HEADER_W: f32 = 210.0;
const RULER_H: f32 = 22.0;
const LANE_H: f32 = 18.0;
/// Width of the grab strip on a region's right edge. Narrow regions get a third of their width
/// instead, so there is always some body left to grab for moving.
const REGION_EDGE_PX: f32 = 6.0;

/// A marker or track name being edited in place.
pub struct InPlaceRename {
    pub id: Id,
    pub text: String,
    /// Give the text field focus and select its text on the next frame it is shown.
    pub focus: bool,
}

impl OndinaApp {
    pub fn beat_to_x(&self, beat: f64, origin: f32) -> f32 {
        origin + ((beat - self.scroll_beat) as f32) * self.px_per_beat
    }

    pub fn x_to_beat(&self, x: f32, origin: f32) -> f64 {
        self.scroll_beat + ((x - origin) / self.px_per_beat) as f64
    }

    pub fn arrange_ui(&mut self, ui: &mut egui::Ui) {
        let p = self.project();
        let full = ui.available_rect_before_wrap();
        let bpb = p.time_signature.beats_per_bar();
        let timeline_x = full.left() + HEADER_W;
        let top = full.top();
        let tracks_top = top + RULER_H + LANE_H * 2.0;
        let painter = ui.painter_at(full);

        // zoom / scroll input
        if ui.rect_contains_pointer(full) {
            let (scroll, zoom_mod, shift, hover) = ui.input(|i| {
                (
                    i.smooth_scroll_delta,
                    i.modifiers.command,
                    i.modifiers.shift,
                    i.pointer.hover_pos(),
                )
            });
            if zoom_mod && scroll.y != 0.0 {
                let anchor = hover
                    .map(|h| self.x_to_beat(h.x, timeline_x))
                    .unwrap_or(self.scroll_beat);
                let factor = (1.0 + scroll.y * 0.002).clamp(0.5, 2.0);
                self.px_per_beat = (self.px_per_beat * factor).clamp(2.0, 400.0);
                if let Some(h) = hover {
                    self.scroll_beat =
                        (anchor - ((h.x - timeline_x) / self.px_per_beat) as f64).max(0.0);
                }
            } else if shift || scroll.x != 0.0 {
                let dx = if shift { scroll.y } else { scroll.x };
                self.scroll_beat = (self.scroll_beat - (dx / self.px_per_beat) as f64).max(0.0);
            } else if scroll.y != 0.0 {
                let content_h = p.tracks.len() as f32 * self.track_height + 80.0;
                self.scroll_y = (self.scroll_y - scroll.y)
                    .clamp(0.0, (content_h - (full.bottom() - tracks_top)).max(0.0));
            }
        }
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

        // ---------------- ruler
        let ruler = Rect::from_min_max(pos2(timeline_x, top), pos2(full.right(), top + RULER_H));
        painter.rect_filled(ruler, Rounding::ZERO, theme::PANEL);
        let visible_beats = ((full.right() - timeline_x) / self.px_per_beat) as f64;
        let bar_px = bpb as f32 * self.px_per_beat;
        let label_every = if bar_px < 18.0 {
            8
        } else if bar_px < 36.0 {
            4
        } else if bar_px < 60.0 {
            2
        } else {
            1
        };
        let first_bar = (self.scroll_beat / bpb).floor() as i64;
        let last_bar = ((self.scroll_beat + visible_beats) / bpb).ceil() as i64;
        // cycle band
        if p.cycle.end > p.cycle.start {
            let x0 = self.beat_to_x(p.cycle.start, timeline_x).max(timeline_x);
            let x1 = self.beat_to_x(p.cycle.end, timeline_x);
            if x1 > timeline_x {
                let col = if p.cycle.enabled {
                    theme::CYCLE
                } else {
                    Color32::from_rgba_premultiplied(80, 80, 80, 60)
                };
                painter.rect_filled(
                    Rect::from_min_max(pos2(x0, top + 2.0), pos2(x1, top + RULER_H - 2.0)),
                    Rounding::same(3.0_f32),
                    col,
                );
            }
        }
        for bar in first_bar..=last_bar {
            let x = self.beat_to_x(bar as f64 * bpb, timeline_x);
            if x < timeline_x {
                continue;
            }
            painter.line_segment(
                [pos2(x, top + RULER_H * 0.45), pos2(x, top + RULER_H)],
                Stroke::new(1.0_f32, theme::TEXT_DIM),
            );
            if bar % label_every == 0 {
                painter.text(
                    pos2(x + 3.0, top + 2.0),
                    Align2::LEFT_TOP,
                    format!("{}", bar + 1),
                    FontId::proportional(11.0),
                    theme::TEXT_DIM,
                );
            }
        }
        let ruler_resp = ui.interact(ruler, ui.id().with("ruler"), Sense::click_and_drag());
        if let Some(pp) = ruler_resp.interact_pointer_pos() {
            let b = self.snap_beat(self.x_to_beat(pp.x, timeline_x).max(0.0));
            let shift = ui.input(|i| i.modifiers.shift);
            if ruler_resp.drag_started() && shift {
                self.drag = Some(Drag::Cycle { anchor: b });
            }
            if let Some(Drag::Cycle { anchor }) = self.drag {
                if ruler_resp.dragged() {
                    let (s, e) = if b < anchor { (b, anchor) } else { (anchor, b) };
                    if e - s >= 0.25 {
                        self.cmd_merge(
                            Command::SetCycle {
                                enabled: true,
                                start: Some(s),
                                end: Some(e),
                            },
                            "cycle-drag",
                        );
                    }
                }
            } else if ruler_resp.clicked() || (ruler_resp.dragged() && !shift) {
                self.exec(Command::SetPlayhead {
                    beat: Some(b),
                    bar: None,
                });
            }
        }
        if ruler_resp.drag_stopped() {
            if let Some(Drag::Cycle { .. }) = self.drag {
                self.drag = None;
            }
        }

        // ---------------- marker lane
        let mlane = Rect::from_min_max(
            pos2(timeline_x, top + RULER_H),
            pos2(full.right(), top + RULER_H + LANE_H),
        );
        painter.rect_filled(mlane, Rounding::ZERO, theme::LANE_ALT);
        painter.text(
            pos2(full.left() + 8.0, mlane.center().y),
            Align2::LEFT_CENTER,
            "Markers",
            FontId::proportional(11.0),
            theme::TEXT_DIM,
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
        for m in &p.markers {
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
            let col = if m.length.is_some() {
                Color32::from_rgb(70, 95, 140)
            } else {
                Color32::from_rgb(110, 90, 60)
            };
            painter.rect_filled(r, Rounding::same(2.0_f32), col);
            if ghost {
                painter.rect_stroke(r, Rounding::same(2.0_f32), Stroke::new(1.5_f32, theme::SELECT));
            }
            if self.marker_rename.as_ref().is_some_and(|e| e.id == m.id) {
                rename_rect = Some(r);
            } else {
                painter.text(
                    pos2(r.left() + 4.0, r.center().y),
                    Align2::LEFT_CENTER,
                    &m.name,
                    FontId::proportional(11.0),
                    Color32::WHITE,
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
                self.marker_rename = Some(InPlaceRename {
                    id: m.id,
                    text: m.name.clone(),
                    focus: true,
                });
                // open the field on this frame: rename_rect was decided before the double-click
                rename_rect = Some(r);
            }
            let mid = m.id;
            let (mb, ml) = (m.beat, m.length);
            resp.context_menu(|ui| {
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
        let target = self.marker_rename.take().and_then(|ren| {
            let old = p.markers.iter().find(|m| m.id == ren.id)?.name.clone();
            Some((ren, old, rename_rect?))
        });
        if let Some((mut ren, old, r)) = target {
            // computed first: the call below borrows ui mutably
            let edit_id = ui.id().with(("mrename", ren.id));
            match rename_field(ui, edit_id, r, FontId::proportional(11.0), &mut ren) {
                RenameEdit::Editing => self.marker_rename = Some(ren),
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
        painter.rect_filled(clane, Rounding::ZERO, theme::LANE);
        painter.text(
            pos2(full.left() + 8.0, clane.center().y),
            Align2::LEFT_CENTER,
            format!("Chords · {}", p.key.name()),
            FontId::proportional(11.0),
            theme::TEXT_DIM,
        );
        for c in &p.chords {
            let x = self.beat_to_x(c.beat, timeline_x);
            let w = c.length as f32 * self.px_per_beat;
            if x + w < timeline_x || x > full.right() {
                continue;
            }
            let r = Rect::from_min_size(
                pos2(x.max(timeline_x), clane.top() + 1.0),
                vec2((w - 1.0 - (timeline_x - x).max(0.0)).max(1.0), LANE_H - 2.0),
            );
            painter.rect_filled(r, Rounding::same(2.0_f32), Color32::from_rgb(60, 70, 60));
            painter.text(
                pos2(r.left() + 4.0, r.center().y),
                Align2::LEFT_CENTER,
                &c.symbol,
                FontId::proportional(11.0),
                Color32::from_rgb(200, 230, 190),
            );
            let cid = c.id;
            ui.interact(r, ui.id().with(("chord", c.id)), Sense::click())
                .context_menu(|ui| {
                    if ui.button("Remove chord").clicked() {
                        self.cmd(Command::RemoveChord { chord_id: cid });
                        ui.close_menu();
                    }
                });
        }

        // ---------------- tracks
        let track_area = Rect::from_min_max(pos2(full.left(), tracks_top), full.max);
        let tpainter = ui.painter_at(track_area);
        let th = self.track_height;
        let ctrl_alt = ui.input(|i| i.modifiers.alt);
        let mut hovered_track: Option<(Id, TrackKind)> = None;
        let pointer = ui.input(|i| i.pointer.hover_pos());
        let mut track_rename_rect: Option<Rect> = None;
        for (i, t) in p.tracks.iter().enumerate() {
            let y = tracks_top + i as f32 * th - self.scroll_y;
            if y + th < tracks_top || y > full.bottom() {
                continue;
            }
            let lane = Rect::from_min_max(pos2(timeline_x, y), pos2(full.right(), y + th));
            let sel = self.selected_track == Some(t.id);
            tpainter.rect_filled(
                lane,
                Rounding::ZERO,
                if sel {
                    Color32::from_rgb(50, 54, 62)
                } else if i % 2 == 0 {
                    theme::LANE
                } else {
                    theme::LANE_ALT
                },
            );
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
                tpainter.line_segment(
                    [pos2(x, y), pos2(x, y + th)],
                    Stroke::new(1.0_f32, if is_bar { theme::GRID_BAR } else { theme::GRID }),
                );
                b += grid_step;
            }
            tpainter.line_segment(
                [pos2(full.left(), y + th), pos2(full.right(), y + th)],
                Stroke::new(1.0_f32, theme::BG),
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
            lane_resp.context_menu(|ui| {
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
                    ui.menu_button("Session Drummer (4 bars)", |ui| {
                        for st in ["rock", "pop", "funk", "hip_hop", "trap", "house", "techno", "disco", "reggaeton", "jazz", "ballad", "drum_and_bass", "lo_fi"] {
                            if ui.button(st).clicked() {
                                if let Ok(style) = serde_json::from_value(serde_json::json!(st)) {
                                    self.cmd(Command::GenerateDrums { track_id: tid, start: b, bars: 4, style, intensity: Some(0.6), fill: Some(true), seed: None });
                                }
                                ui.close_menu();
                            }
                        }
                    });
                    ui.menu_button("Session Bassist (follows chord track)", |ui| {
                        for st in ["root", "root_fifth", "eighths", "octaves", "syncopated", "walking", "sub808"] {
                            if ui.button(st).clicked() {
                                if let Ok(style) = serde_json::from_value(serde_json::json!(st)) {
                                    self.cmd(Command::GenerateBass { track_id: tid, start: b, bars: 4, style, chords: None, beats_per_chord: None, octave: None, seed: None });
                                }
                                ui.close_menu();
                            }
                        }
                    });
                    ui.menu_button("Session Keys (follows chord track)", |ui| {
                        for st in ["sustained", "pop", "offbeat", "ballad", "funk"] {
                            if ui.button(st).clicked() {
                                if let Ok(style) = serde_json::from_value(serde_json::json!(st)) {
                                    self.cmd(Command::GenerateChords { track_id: tid, start: b, bars: 4, style, chords: None, beats_per_chord: None, center_pitch: None, seed: None });
                                }
                                ui.close_menu();
                            }
                        }
                    });
                    if ui.button("Arpeggio (4 bars)").clicked() {
                        self.cmd(Command::GenerateArpeggio { track_id: tid, start: b, bars: 4, mode: ondina_core::generate::ArpMode::UpDown, rate: Some(0.25), octaves: Some(2), chords: None, beats_per_chord: None, center_pitch: None, seed: None });
                        ui.close_menu();
                    }
                    if ui.button("Melody (4 bars)").clicked() {
                        self.cmd(Command::GenerateMelody { track_id: tid, start: b, bars: 4, density: Some(0.5), octave: Some(5), chords: None, beats_per_chord: None, seed: None });
                        ui.close_menu();
                    }
                    ui.separator();
                }
                if ui.button("✨ Ask AI to write a part for this track").clicked() {
                    self.selected_track = Some(tid);
                    ui.close_menu();
                    self.ask_ai("Write a fitting 8-bar part for the selected track that works with the rest of the project.".into());
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
                    pos2(rx0, ry + 2.0),
                    pos2(rx1.max(rx0 + 4.0), ry + th - 2.0),
                );
                let clip =
                    rect.intersect(Rect::from_min_max(pos2(timeline_x, tracks_top), full.max));
                let base = theme::rgb(r.color.unwrap_or(t.color));
                let selected = self.selected_region == Some(r.id);
                let body = if r.muted {
                    Color32::from_gray(70)
                } else {
                    theme::darker(base, 0.55)
                };
                tpainter.rect_filled(clip, Rounding::same(3.0_f32), body);
                let header = Rect::from_min_max(clip.min, pos2(clip.right(), clip.top() + 13.0));
                tpainter.rect_filled(
                    header,
                    Rounding::same(3.0_f32),
                    if r.muted {
                        Color32::from_gray(90)
                    } else {
                        base
                    },
                );
                tpainter.text(
                    pos2(clip.left() + 4.0, header.center().y),
                    Align2::LEFT_CENTER,
                    &r.name,
                    FontId::proportional(10.5),
                    Color32::BLACK,
                );
                // loop separators
                for k in 1..r.loop_count {
                    let lx = rx0 + (k as f64 * r.length) as f32 * self.px_per_beat;
                    if lx > clip.left() && lx < clip.right() {
                        tpainter.line_segment(
                            [pos2(lx, rect.top()), pos2(lx, rect.bottom())],
                            Stroke::new(1.0_f32, Color32::from_black_alpha(140)),
                        );
                    }
                }
                // content preview
                let content = Rect::from_min_max(
                    pos2(rect.left(), rect.top() + 14.0),
                    pos2(rect.right(), rect.bottom() - 2.0),
                );
                let ppainter = tpainter.with_clip_rect(clip);
                match &r.content {
                    RegionContent::Midi(_) | RegionContent::Pattern(_) => {
                        let notes: Vec<Note> = match &r.content {
                            RegionContent::Midi(m) => m.notes.clone(),
                            RegionContent::Pattern(pt) => pattern_to_notes(pt),
                            _ => vec![],
                        };
                        if !notes.is_empty() {
                            let lo = notes.iter().map(|n| n.pitch).min().unwrap() as f32;
                            let hi = notes.iter().map(|n| n.pitch).max().unwrap() as f32;
                            let span = (hi - lo).max(12.0);
                            for kk in 0..r.loop_count.max(1) {
                                let off = kk as f64 * r.length;
                                for n in notes.iter().filter(|n| n.start < r.length) {
                                    let nx0 = rx0 + ((n.start + off) as f32) * self.px_per_beat;
                                    let nx1 = nx0
                                        + (n.length.min(r.length - n.start) as f32
                                            * self.px_per_beat)
                                            .max(1.5);
                                    let ny = content.bottom()
                                        - ((n.pitch as f32 - lo) / span) * (content.height() - 3.0)
                                        - 2.0;
                                    ppainter.line_segment(
                                        [pos2(nx0, ny), pos2(nx1, ny)],
                                        Stroke::new(
                                            2.0_f32,
                                            theme::darker(base, 1.0).gamma_multiply(1.0),
                                        ),
                                    );
                                }
                            }
                        }
                    }
                    RegionContent::Audio(a) => {
                        if let (Some(peaks), Some(f)) =
                            (self.waveforms.get(&a.file_id), p.audio_file(a.file_id))
                        {
                            let mid = content.center().y;
                            let h = content.height() * 0.5;
                            let start_sec = p.tempo.beat_to_sec(r.start);
                            let mut x = clip.left().max(rect.left());
                            while x < clip.right() {
                                let beat = self.x_to_beat(x, timeline_x);
                                let in_reg = (beat - r.start) % r.length.max(1e-6);
                                let sec = p.tempo.beat_to_sec(r.start + in_reg) - start_sec
                                    + a.offset_sec;
                                let sec = if a.reverse { f.duration_sec - sec } else { sec };
                                let idx = (sec * f.sample_rate as f64 / 256.0) as usize;
                                if let Some((lo, hi)) = peaks.get(idx) {
                                    ppainter.line_segment(
                                        [pos2(x, mid - hi * h), pos2(x, mid - lo * h)],
                                        Stroke::new(1.0_f32, base),
                                    );
                                }
                                x += 1.0;
                            }
                        }
                    }
                }
                tpainter.rect_stroke(
                    clip,
                    Rounding::same(3.0_f32),
                    Stroke::new(
                        if selected { 2.0 } else { 1.0_f32 },
                        if selected {
                            theme::SELECT
                        } else {
                            Color32::from_black_alpha(160)
                        },
                    ),
                );
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
                // Decide from where the press began, not where the pointer is once egui reports the drag
                // (that comes a few pixels later), so a drag that starts on the edge never moves the region.
                let press_zone = ui
                    .input(|i| i.pointer.press_origin())
                    .map(|o| region_hit_zone(o.x, rect.left(), rect.right()))
                    .unwrap_or(RegionZone::Body);
                let start_resize = edge_resp.drag_started()
                    || (body_resp.drag_started() && press_zone == RegionZone::Edge);
                let start_move = body_resp.drag_started() && !start_resize;
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
                body_resp.context_menu(|ui| {
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
                    if ui.button("✨ Ask AI to vary this region").clicked() {
                        self.selected_region = Some(rid);
                        ui.close_menu();
                        self.ask_ai("Create a musical variation of the selected region on the same track right after it (keep the style, change rhythm or melody slightly).".into());
                    }
                });
            }

            // header
            let header = Rect::from_min_max(pos2(full.left(), y), pos2(timeline_x, y + th));
            if let Some(r) = self.track_header(ui, &tpainter, header, t, i, sel) {
                track_rename_rect = Some(r);
            }
        }

        // Track name field, registered after every header so it sits on top. A track that scrolled out of
        // view, was deleted or undone away drops the edit.
        let target = self.track_rename.take().and_then(|ren| {
            let old = p.track(ren.id)?.name.clone();
            Some((ren, old, track_rename_rect?))
        });
        if let Some((mut ren, old, r)) = target {
            let edit_id = ui.id().with(("trename", ren.id));
            match rename_field(ui, edit_id, r, FontId::proportional(12.5), &mut ren) {
                RenameEdit::Editing => self.track_rename = Some(ren),
                RenameEdit::Commit => {
                    if let Some(name) = rename_commit(&old, &ren.text) {
                        self.cmd(Command::RenameTrack {
                            track_id: ren.id,
                            name,
                        });
                    }
                }
                RenameEdit::Cancel => {}
            }
        }

        // dragging updates
        let pointer_pos = ui.input(|i| i.pointer.interact_pos());
        let released = ui.input(|i| i.pointer.any_released());
        let pointer_beat = pointer_pos.map(|pp| self.x_to_beat(pp.x, timeline_x));
        let snap = self.snap;
        if matches!(self.drag, Some(Drag::MoveTrack { .. })) && ui.input(|i| i.key_pressed(egui::Key::Escape)) {
            self.drag = None;
        }
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
            Some(Drag::MoveTrack { gap, .. }) => {
                ui.ctx().set_cursor_icon(egui::CursorIcon::Grabbing);
                if let Some(pp) = pointer_pos {
                    let n = p.tracks.len();
                    *gap = track_drop_index(pp.y, tracks_top - self.scroll_y, th, n);
                    let v = edge_scroll_speed(pp.y, tracks_top, full.bottom());
                    if v != 0.0 {
                        let dt = ui.input(|i| i.stable_dt).min(0.1);
                        // same limit as the mouse wheel above
                        let max = (n as f32 * th + 80.0 - (full.bottom() - tracks_top)).max(0.0);
                        self.scroll_y = (self.scroll_y + v * dt).clamp(0.0, max);
                        // keep scrolling while the pointer rests in the band
                        ui.ctx().request_repaint();
                    }
                }
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
                Some(Drag::MoveTrack { id, gap }) => {
                    // Found by id: the assistant or MCP may have reordered the tracks during the drag.
                    if let Some(index) = p
                        .track_index(id)
                        .and_then(|from| track_move_target(from, gap.min(p.tracks.len())))
                    {
                        self.cmd(Command::MoveTrack { track_id: id, index });
                    }
                }
                other => self.drag = other.filter(|d| !matches!(d, Drag::Cycle { .. })),
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
            child.menu_button("➕ Add track", |ui| {
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
            });
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

        // playhead
        let px = self.beat_to_x(pos, timeline_x);
        if px >= timeline_x && px <= full.right() {
            painter.line_segment(
                [pos2(px, top), pos2(px, full.bottom())],
                Stroke::new(1.5_f32, theme::PLAYHEAD),
            );
            painter.add(egui::Shape::convex_polygon(
                vec![
                    pos2(px - 5.0, top),
                    pos2(px + 5.0, top),
                    pos2(px, top + 8.0),
                ],
                theme::PLAYHEAD,
                Stroke::NONE,
            ));
        }
        // header column background for ruler/lanes
        painter.rect_filled(
            Rect::from_min_max(full.min, pos2(timeline_x, top + RULER_H)),
            Rounding::ZERO,
            theme::PANEL,
        );
        painter.text(
            pos2(full.left() + 8.0, top + RULER_H / 2.0),
            Align2::LEFT_CENTER,
            format!("{} tracks", p.tracks.len()),
            FontId::proportional(11.0),
            theme::TEXT_DIM,
        );
        ui.allocate_rect(full, Sense::hover());
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
        painter.rect_filled(
            rect,
            Rounding::ZERO,
            if selected {
                Color32::from_rgb(58, 62, 72)
            } else {
                theme::PANEL
            },
        );
        painter.rect_filled(
            Rect::from_min_size(rect.min, vec2(4.0, rect.height())),
            Rounding::ZERO,
            theme::rgb(t.color),
        );
        // Background first: M/S/R, the volume slider and the rename field are registered after it and sit on top.
        let resp = ui.interact(rect, ui.id().with(("hdr", t.id)), Sense::click_and_drag());
        if resp.clicked() {
            self.selected_track = Some(t.id);
        }
        if resp.drag_started() && self.drag.is_none() {
            self.selected_track = Some(t.id);
            self.drag = Some(Drag::MoveTrack { id: t.id, gap: index });
        }
        // the top row (icon and name) opens the rename; the rest of the header only selects
        let name_row = Rect::from_min_max(
            pos2(rect.left() + 6.0, rect.top()),
            pos2(rect.right() - 26.0, rect.top() + 22.0),
        );
        if resp.double_clicked() && resp.interact_pointer_pos().is_some_and(|pp| name_row.contains(pp)) {
            self.track_rename = Some(InPlaceRename {
                id: t.id,
                text: t.name.clone(),
                focus: true,
            });
        }
        let icon = match t.kind {
            TrackKind::Instrument => {
                if t.instrument
                    .as_ref()
                    .map(|i| i.kind == "drum_synth")
                    .unwrap_or(false)
                {
                    "🥁"
                } else {
                    "🎹"
                }
            }
            TrackKind::Audio => "🎤",
            TrackKind::Bus => "🔀",
        };
        // decided after the double-click above, so the field opens on the same frame
        let renaming = self.track_rename.as_ref().is_some_and(|e| e.id == t.id);
        let name_pos = pos2(rect.left() + 10.0, rect.top() + 12.0);
        let mut field = None;
        if renaming {
            let ir = painter.text(name_pos, Align2::LEFT_CENTER, icon, FontId::proportional(12.5), Color32::WHITE);
            let r = Rect::from_min_max(
                pos2(ir.right() + 4.0, rect.top() + 3.0),
                pos2(rect.right() - 28.0, rect.top() + 21.0),
            );
            // half-hidden under the lanes or the window edge: drop the edit rather than draw over them
            field = painter.clip_rect().contains_rect(r).then_some(r);
        } else {
            painter.text(
                name_pos,
                Align2::LEFT_CENTER,
                format!("{icon} {}", t.name),
                FontId::proportional(12.5),
                Color32::WHITE,
            );
        }
        let tid = t.id;
        resp.context_menu(|ui| {
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
        // buttons
        let by = rect.top() + 26.0;
        let mut x = rect.left() + 10.0;
        let mut button =
            |ui: &mut egui::Ui, label: &str, on: bool, color: Color32, tip: &str| -> bool {
                let r = Rect::from_min_size(pos2(x, by), vec2(20.0, 16.0));
                x += 23.0;
                let resp = ui
                    .interact(r, ui.id().with((label, tid)), Sense::click())
                    .on_hover_text(tip);
                painter.rect_filled(
                    r,
                    Rounding::same(3.0_f32),
                    if on { color } else { theme::LANE },
                );
                painter.text(
                    r.center(),
                    Align2::CENTER_CENTER,
                    label,
                    FontId::proportional(10.5),
                    if on { Color32::BLACK } else { theme::TEXT_DIM },
                );
                resp.clicked()
            };
        if button(ui, "M", t.channel.mute, theme::MUTE, "Mute") {
            self.cmd(Command::SetMute {
                track_id: tid,
                mute: !t.channel.mute,
            });
        }
        if button(ui, "S", t.channel.solo, theme::SOLO, "Solo") {
            self.cmd(Command::SetSolo {
                track_id: tid,
                solo: !t.channel.solo,
            });
        }
        if t.kind != TrackKind::Bus && button(ui, "R", t.record_armed, theme::REC, "Record arm") {
            self.cmd(Command::SetRecordArm {
                track_id: tid,
                armed: !t.record_armed,
            });
        }
        // volume mini slider
        let sr = Rect::from_min_max(
            pos2(x + 4.0, by + 2.0),
            pos2(rect.right() - 30.0, by + 14.0),
        );
        if sr.width() > 20.0 {
            let mut child = ui.new_child(
                egui::UiBuilder::new()
                    .max_rect(sr)
                    .layout(egui::Layout::left_to_right(egui::Align::Center)),
            );
            let mut v = t.channel.volume_db;
            child.spacing_mut().slider_width = sr.width();
            if child
                .add(egui::Slider::new(&mut v, -60.0..=6.0).show_value(false))
                .on_hover_text(format!("{:.1} dB", v))
                .changed()
            {
                self.cmd_merge(
                    Command::SetVolume {
                        track_id: tid,
                        db: v,
                    },
                    format!("vol{tid}"),
                );
            }
        }
        // meter
        let m = self.meters.get(&tid).copied().unwrap_or([0.0; 2]);
        let mr = Rect::from_min_max(
            pos2(rect.right() - 22.0, rect.top() + 6.0),
            pos2(rect.right() - 8.0, rect.bottom() - 6.0),
        );
        draw_meter(painter, mr, m);
        if matches!(&self.drag, Some(Drag::MoveTrack { id, .. }) if *id == t.id) {
            // the dragged header stays in place, dimmed, until it is dropped
            painter.rect_filled(rect, Rounding::ZERO, Color32::from_black_alpha(110));
            painter.rect_stroke(rect.shrink(1.0), Rounding::ZERO, Stroke::new(1.5_f32, theme::SELECT));
        }
        field
    }
}

/// Which part of a region the pointer is on: the body moves it, the right edge resizes it.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum RegionZone {
    Body,
    Edge,
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

/// Where a dragged marker lands: it was grabbed at `grab_beat` and the pointer is now at `pointer_beat`.
fn dragged_marker_beat(orig_beat: f64, grab_beat: f64, pointer_beat: f64, snap: f64) -> f64 {
    snap_to(orig_beat + (pointer_beat - grab_beat), snap).max(0.0)
}

/// Length of a section marker starting at `start` whose right edge is dragged to `pointer_beat`.
/// The end snaps to the grid (not the length), so an off-grid section still ends on a grid line.
fn resized_marker_length(start: f64, pointer_beat: f64, snap: f64) -> f64 {
    (snap_to(pointer_beat, snap) - start).max(MIN_MARKER_LEN)
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

/// The name to send after an in-place rename of a marker or a track, or None when nothing should change.
fn rename_commit(old: &str, typed: &str) -> Option<String> {
    let t = typed.trim();
    (!t.is_empty() && t != old).then(|| t.to_string())
}

/// What the user did with an in-place name field this frame.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum RenameEdit {
    /// Still typing: keep the edit open.
    Editing,
    /// Enter, or a click elsewhere.
    Commit,
    /// Esc.
    Cancel,
}

/// Draws the in-place name field over `r` (at least 120 px wide). Call it after every widget it covers,
/// so it sits on top and gets the clicks.
fn rename_field(
    ui: &mut egui::Ui,
    edit_id: egui::Id,
    r: Rect,
    font: FontId,
    ren: &mut InPlaceRename,
) -> RenameEdit {
    let er = Rect::from_min_size(r.min, vec2(r.width().max(120.0), r.height()));
    let mut child = ui.new_child(egui::UiBuilder::new().max_rect(er));
    let resp = child.add(
        egui::TextEdit::singleline(&mut ren.text)
            .id(edit_id)
            .font(font)
            .margin(vec2(3.0, 1.0))
            .desired_width(er.width() - 6.0),
    );
    if ren.focus {
        resp.request_focus();
        // select the whole name so typing replaces it
        if let Some(mut st) = egui::TextEdit::load_state(ui.ctx(), edit_id) {
            let n = ren.text.chars().count();
            st.cursor.set_char_range(Some(egui::text::CCursorRange::two(
                egui::text::CCursor::new(0),
                egui::text::CCursor::new(n),
            )));
            st.store(ui.ctx(), edit_id);
        }
        ren.focus = false;
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

pub fn draw_meter(painter: &egui::Painter, r: Rect, m: [f32; 2]) {
    painter.rect_filled(r, Rounding::same(2.0_f32), Color32::from_rgb(20, 22, 26));
    let w = (r.width() - 1.0) / 2.0;
    for c in 0..2 {
        let db = ondina_dsp::util::gain_to_db(m[c]).max(-60.0);
        let frac = ((db + 60.0) / 66.0).clamp(0.0, 1.0);
        let col = if db > -0.5 {
            theme::REC
        } else if db > -9.0 {
            Color32::from_rgb(230, 200, 60)
        } else {
            Color32::from_rgb(80, 200, 110)
        };
        let x = r.left() + c as f32 * (w + 1.0);
        let top = r.bottom() - frac * r.height();
        painter.rect_filled(
            Rect::from_min_max(pos2(x, top), pos2(x + w, r.bottom())),
            Rounding::ZERO,
            col,
        );
    }
    let _ = Pos2::ZERO;
}

#[cfg(test)]
mod tests {
    use super::{
        dragged_marker_beat, edge_scroll_speed, region_edge_width, region_hit_zone, rename_commit,
        resized_marker_length, track_drop_index, track_move_target, RegionZone, MIN_MARKER_LEN,
        REGION_EDGE_PX, TRACK_DRAG_SCROLL_PX_S,
    };
    use ondina_core::command::Command;
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
}
