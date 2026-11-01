//! Bottom editors: Piano Roll, Step Sequencer and audio region editor.

use std::collections::HashSet;

use ondina_core::command::Command;
use ondina_core::model::*;
use ondina_core::theory::{note_name, HarmonyInterval};
use ondina_dsp::MidiKind;
use eframe::egui::{
    self, pos2, vec2, Align2, Color32, FontId, Key, PointerButton, Rect, RichText, Rounding, Sense, Stroke,
};

use crate::app::{BottomTab, OndinaApp, Drag};
use crate::arrange::{rename_field, RenameEdit};
use crate::cc_lane::{self, CcStroke};
use crate::note_clipboard as nc;
use crate::note_drag::{self, VelocityStroke};
use crate::note_selection;
use crate::piano_roll_view::{
    follow_scroll, grid_line, grid_step, initial_view, is_black, key_label, loop_local_beat, note_colors,
    piano_roll_layout, playing_pattern_step, row_fill, row_line, scroll_pitch, velocity_colors, visible_rows,
    wheel_zoom, zoom_factor, zoom_pitch, zoom_time, GridLine, BLACK_KEY_INSET, KEY_BLACK, KEY_LABEL, KEY_LINE,
    KEY_WHITE, RULER_H, VEL_H,
};
use crate::step_paint::{self, cell_at, RowFill, RowRename, StepCell, StepStroke, DRUM_SOUNDS, PAINT_VELOCITY};
use crate::theme;
use crate::widgets::{self, Icon, StateKind};

/// Mouse and keyboard help of the piano roll (hover on the help entry of its "⋯" menu and on the editor header's
/// name).
/// Mouse help of the step sequencer (hover on "Help" in its toolbar).
const STEP_HELP: &str = "Click: on/off · Drag: paint · Right-click a step: accent → normal → ghost → off · M: mute the row · Click a name: preview · Right-click a name or the dots button: row menu · The number next to M: the row's sound";
/// Step cell size and the row name's width.
const STEP_W: f32 = 22.0;
const STEP_H: f32 = 20.0;
const ROW_NAME_W: f32 = 108.0;

pub(crate) const PIANO_ROLL_HELP: &str = "Click on empty grid: add a note and select it (Ctrl/Shift: add it to the selection) · Wheel: scroll pitch · Shift+wheel: scroll time · Ctrl+wheel: zoom time · Ctrl+Shift+wheel: zoom pitch · Ctrl+C/X/V: copy/cut/paste · Ctrl+A: select all · Ctrl+D: repeat the selection · ←/→: move by the snap step · ↑/↓: semitone · Shift+↑/↓: octave · Alt+←/→: shorten/lengthen by the snap step · Alt+drag: copy the notes · Controller lane: drag to draw, right-drag to erase";
/// The piano roll's Harmony menu: label, interval and whether the voice goes below the notes.
pub(crate) const HARMONY_MENU: [(&str, HarmonyInterval, bool); 7] = [
    ("3rd above", HarmonyInterval::Third, false),
    ("3rd below", HarmonyInterval::Third, true),
    ("5th above", HarmonyInterval::Fifth, false),
    ("6th above", HarmonyInterval::Sixth, false),
    ("6th below", HarmonyInterval::Sixth, true),
    ("Octave above", HarmonyInterval::Octave, false),
    ("Octave below", HarmonyInterval::Octave, true),
];

/// Removes the events that match `f` and tells whether there was one.
pub(crate) fn take_event(events: &mut Vec<egui::Event>, f: impl Fn(&egui::Event) -> bool) -> bool {
    let before = events.len();
    events.retain(|e| !f(e));
    events.len() != before
}

/// Consumes Alt+←/→ (the first pressed this frame) and returns its length change in grid steps. Runs before
/// `consume_arrow`, which would otherwise take Alt+← as a plain ← (egui ignores an extra Alt).
fn consume_alt_arrow(i: &mut egui::InputState) -> Option<i32> {
    for k in [Key::ArrowLeft, Key::ArrowRight] {
        if i.consume_key(egui::Modifiers::ALT, k) {
            return nc::arrow_length(k);
        }
    }
    None
}

/// Consumes the first arrow key pressed this frame (Shift+↑/↓ before the plain keys) and returns its move.
fn consume_arrow(i: &mut egui::InputState) -> Option<nc::Nudge> {
    for k in [Key::ArrowUp, Key::ArrowDown] {
        if i.consume_key(egui::Modifiers::SHIFT, k) {
            return nc::arrow_nudge(k, true);
        }
    }
    for k in [Key::ArrowUp, Key::ArrowDown, Key::ArrowLeft, Key::ArrowRight] {
        if i.consume_key(egui::Modifiers::NONE, k) {
            return nc::arrow_nudge(k, false);
        }
    }
    None
}

impl OndinaApp {
    pub fn editor_ui(&mut self, ui: &mut egui::Ui) {
        let p = self.project();
        let found = self
            .selected_region
            .and_then(|id| p.find_region(id))
            .map(|(t, r)| (t.clone(), r.clone()));
        self.editor_header(ui, found.as_ref().map(|(t, r)| (t, r)));
        egui::Frame::none()
            .inner_margin(egui::Margin { left: theme::SP_4, right: theme::SP_4, top: theme::SP_2, bottom: theme::SP_2 })
            .show(ui, |ui| self.editor_body(ui, &p, found));
    }

    fn editor_body(&mut self, ui: &mut egui::Ui, p: &std::sync::Arc<Project>, found: Option<(Track, Region)>) {
        let Some((track, region)) = found else {
            crate::chrome::empty_state(
                ui,
                Icon::Piano,
                "Select a region to edit it",
                "Click a region in the tracks, or double-click an instrument lane to create one.",
            );
            self.edits.row_rename = None;
            return;
        };
        // the row name field lives only while the step sequencer shows its region
        if self.extra.show_score || self.edits.row_rename.as_ref().is_some_and(|r| r.region != region.id) {
            self.edits.row_rename = None;
        }
        if self.extra.show_score
            && matches!(
                region.content,
                RegionContent::Midi(_) | RegionContent::Pattern(_)
            )
        {
            self.score_ui(ui, &region);
            return;
        }
        match &region.content {
            RegionContent::Midi(m) => self.piano_roll(ui, p, &track, &region, m),
            RegionContent::Pattern(pt) => self.step_sequencer(ui, &track, &region, pt),
            RegionContent::Audio(a) => self.audio_editor(ui, p, &region, a),
        }
    }

    fn piano_roll(
        &mut self,
        ui: &mut egui::Ui,
        p: &Project,
        track: &Track,
        region: &Region,
        m: &MidiContent,
    ) {
        let rid = region.id;
        // the toolbar lives in the editor header (pr_toolbar.rs)
        let full = ui.available_rect_before_wrap();
        let cc_h = cc_lane::lane_height(self.cc_lane.open, full.height(), RULER_H + VEL_H);
        let lay = piano_roll_layout(full, cc_h);
        let grid = lay.grid;
        let vel_rect = lay.vel;
        let painter = ui.painter_at(full);
        painter.rect_filled(full, Rounding::ZERO, theme::LANE_BG);
        // Frame the region's notes the first time it is shown; before the input so this frame already
        // draws the framed view.
        if self.pr_framing.take(rid) {
            let rows = visible_rows(grid.height(), self.pr_key_height).max(1);
            let (top, beat) = initial_view(&m.notes, rows);
            self.pr_top_pitch = top;
            self.pr_scroll_beat = beat;
            self.pr_pitch_carry = 0.0;
        }
        // input: scroll & zoom
        if ui.rect_contains_pointer(full) {
            let line_px = ui.ctx().options(|o| o.line_scroll_speed);
            let (scroll, hover, zoom) = ui.input(|i| {
                (
                    i.smooth_scroll_delta,
                    i.pointer.hover_pos(),
                    wheel_zoom(&i.events, line_px, i.screen_rect().height()),
                )
            });
            let hover = hover.unwrap_or(grid.center());
            let dx = (hover.x - grid.left()).clamp(0.0, grid.width());
            let dy = (hover.y - grid.top()).clamp(0.0, grid.height());
            if zoom.time != 0.0 {
                (self.pr_scroll_beat, self.pr_px_per_beat) =
                    zoom_time(self.pr_scroll_beat, self.pr_px_per_beat, dx, zoom_factor(zoom.time));
            }
            if zoom.pitch != 0.0 {
                (self.pr_top_pitch, self.pr_key_height) =
                    zoom_pitch(self.pr_top_pitch, self.pr_key_height, dy, zoom_factor(zoom.pitch));
                self.pr_pitch_carry = 0.0;
            }
            // egui already turns Shift+wheel into horizontal scrolling.
            if scroll.x != 0.0 {
                self.pr_scroll_beat =
                    (self.pr_scroll_beat - (scroll.x / self.pr_px_per_beat) as f64).max(0.0);
            }
            if scroll.y != 0.0 {
                (self.pr_top_pitch, self.pr_pitch_carry) =
                    scroll_pitch(self.pr_top_pitch, self.pr_pitch_carry, scroll.y, self.pr_key_height);
            }
        }
        // Follow the playhead while playing, inside the region's repeats, like the arrangement does.
        let rel = self.position() - region.start;
        let local = loop_local_beat(rel, region.length, region.loop_count);
        if self.follow_playhead
            && self.playing()
            && self.drag.is_none()
            && (0.0..region.end() - region.start).contains(&rel)
        {
            let visible = (grid.width() / self.pr_px_per_beat) as f64;
            let margin = (40.0 / self.pr_px_per_beat) as f64;
            if let Some(s) = follow_scroll(self.pr_scroll_beat, local, visible, margin) {
                self.pr_scroll_beat = s;
            }
        }
        let kh = self.pr_key_height;
        let pbb = self.pr_px_per_beat;
        let rows = (grid.height() / kh).ceil() as i32;
        let pitch_y = |pitch: i32, top_pitch: i32| grid.top() + (top_pitch - pitch) as f32 * kh;
        let beat_x = |b: f64, sb: f64| grid.left() + ((b - sb) as f32) * pbb;
        let top_pitch = self.pr_top_pitch;
        let sb = self.pr_scroll_beat;
        // pitch rows and the keyboard
        let rpainter = painter.with_clip_rect(grid);
        let kpainter = painter.with_clip_rect(lay.keys);
        kpainter.rect_filled(lay.keys, Rounding::ZERO, KEY_WHITE);
        for k in 0..=rows {
            let pitch = top_pitch - k;
            let y = pitch_y(pitch, top_pitch);
            let row = Rect::from_min_max(pos2(grid.left(), y), pos2(grid.right(), y + kh));
            let in_key = p.key.contains(pitch.clamp(0, 127) as u8);
            rpainter.rect_filled(row, Rounding::ZERO, row_fill(pitch, in_key));
            rpainter.hline(grid.x_range(), y + kh - 0.5, Stroke::new(1.0_f32, row_line(pitch)));
            // keyboard: white keys across, black keys shorter on top of them, the C of every octave labelled
            let key = Rect::from_min_max(pos2(lay.keys.left(), y), pos2(lay.keys.right(), y + kh));
            let kresp = ui.interact(key.intersect(lay.keys), ui.id().with(("pk", pitch)), Sense::click_and_drag());
            let black = is_black(pitch);
            let face = if black {
                Rect::from_min_max(key.min, pos2(key.right() - BLACK_KEY_INSET, key.bottom()))
            } else {
                key
            };
            if black {
                kpainter.rect_filled(face, Rounding { ne: 2.0, se: 2.0, ..Rounding::ZERO }, KEY_BLACK);
            } else {
                kpainter.hline(key.x_range(), key.bottom() - 0.5, Stroke::new(1.0_f32, KEY_LINE));
            }
            if kresp.hovered() || kresp.is_pointer_button_down_on() {
                let a = if kresp.is_pointer_button_down_on() { 120 } else { 60 };
                kpainter.rect_filled(face, Rounding::ZERO, theme::ACCENT.gamma_multiply(a as f32 / 255.0));
            }
            if let Some(label) = key_label(pitch, kh) {
                kpainter.text(
                    pos2(key.right() - 4.0, y + kh / 2.0),
                    Align2::RIGHT_CENTER,
                    label,
                    FontId::monospace(9.0),
                    KEY_LABEL,
                );
            }
            if kresp.drag_started() || kresp.clicked() {
                self.live_note(
                    track.id,
                    MidiKind::NoteOn {
                        pitch: pitch.clamp(0, 127) as u8,
                        velocity: 100,
                    },
                );
            }
            if kresp.drag_stopped() || kresp.clicked() {
                self.live_note(
                    track.id,
                    MidiKind::NoteOff {
                        pitch: pitch.clamp(0, 127) as u8,
                    },
                );
            }
        }
        painter.vline(lay.keys.right() - 0.5, lay.keys.y_range(), Stroke::new(1.0_f32, theme::SEPARATOR));
        // ruler (bar numbers of the song) and the grid lines: bars, beats and divisions
        let ruler_band = Rect::from_min_max(pos2(full.left(), full.top()), lay.ruler.max);
        painter.rect_filled(ruler_band, Rounding::ZERO, theme::PANEL_BG);
        painter.hline(ruler_band.x_range(), ruler_band.bottom() - 0.5, Stroke::new(1.0_f32, theme::HAIRLINE));
        let rulp = painter.with_clip_rect(lay.ruler);
        let bpb = p.time_signature.beats_per_bar();
        let step = grid_step(pbb, bpb);
        let visible = (grid.width() / pbb) as f64;
        // count the lines in song beats, so the bar lines land on the song's bars whatever the region's start
        let mut song_b = ((sb + region.start) / step).floor() * step;
        while song_b - region.start <= sb + visible {
            let b = song_b - region.start;
            let x = beat_x(b, sb);
            let kind = grid_line(b, region.start, bpb);
            rpainter.vline(x, grid.y_range(), Stroke::new(1.0_f32, kind.color()));
            match kind {
                GridLine::Bar => {
                    rulp.vline(x, lay.ruler.top() + 3.0..=lay.ruler.bottom(), Stroke::new(1.0_f32, theme::RULER_TICK));
                    rulp.text(
                        pos2(x + 4.0, lay.ruler.center().y),
                        Align2::LEFT_CENTER,
                        format!("{}", (song_b / bpb).round() as i64 + 1),
                        theme::number_font(),
                        theme::TEXT_2,
                    );
                }
                GridLine::Beat => {
                    rulp.vline(x, lay.ruler.bottom() - 5.0..=lay.ruler.bottom(), Stroke::new(1.0_f32, theme::RULER_BEAT_TICK));
                }
                GridLine::Division => {}
            }
            song_b += step;
        }
        // region end shading
        let end_x = beat_x(region.length, sb);
        if end_x < grid.right() {
            painter.rect_filled(
                Rect::from_min_max(
                    pos2(end_x.max(grid.left()), lay.ruler.top()),
                    pos2(grid.right(), grid.bottom()),
                ),
                Rounding::ZERO,
                Color32::from_black_alpha(110),
            );
        }
        // notes (with drag preview)
        let base = theme::rgb(region.color.unwrap_or(track.color));
        let mut notes = m.notes.clone();
        let mut previewing = false;
        let alt = ui.input(|i| i.modifiers.alt);
        let mut preview_sel: Option<HashSet<usize>> = None;
        match &self.drag {
            Some(Drag::Notes {
                region,
                orig,
                indices,
                d_beat,
                d_pitch,
                ..
            }) if *region == rid => {
                let (shown, sel) = note_drag::drag_preview(orig, indices, *d_beat, *d_pitch, alt);
                notes = shown;
                preview_sel = Some(sel);
                previewing = true;
                if alt {
                    ui.ctx().set_cursor_icon(egui::CursorIcon::Copy);
                }
            }
            Some(Drag::NoteResize {
                region,
                orig,
                indices,
                d_len,
                ..
            }) if *region == rid => {
                notes = orig.clone();
                for i in indices {
                    if let Some(n) = notes.get_mut(*i) {
                        n.length = (n.length + d_len).max(0.03);
                    }
                }
                previewing = true;
            }
            Some(Drag::Velocity { region, stroke }) if *region == rid => {
                notes = stroke.notes().to_vec();
                previewing = true;
            }
            _ => {}
        }
        let gpainter = painter.with_clip_rect(grid);
        // The background grid must be registered BEFORE the notes: egui gives clicks to the
        // last-registered (topmost) widget, so registering it after would swallow every click
        // on a note (no select/delete, and a duplicate note added on top instead).
        // click = add note / select, drag on empty space = rubber-band selection
        let gresp = ui.interact(grid, ui.id().with(("grid", rid)), Sense::click_and_drag());
        // screen <-> beat/pitch helpers for the rubber band
        let x_to_beat = |x: f32| sb + ((x - grid.left()) / pbb) as f64;
        let y_to_pitch = |y: f32| top_pitch as f32 + 1.0 - (y - grid.top()) / kh;
        let mut note_hit = false;
        let pointer = ui.input(|i| i.pointer.interact_pos());
        for (i, n) in notes.iter().enumerate() {
            let x0 = beat_x(n.start, sb);
            let x1 = beat_x(n.start + n.length, sb).max(x0 + 3.0);
            let y = pitch_y(n.pitch as i32, top_pitch);
            if x1 < grid.left() || x0 > grid.right() || y + kh < grid.top() || y > grid.bottom() {
                continue;
            }
            let r = Rect::from_min_max(pos2(x0, y + 1.0), pos2(x1, y + kh - 1.0));
            let sel = preview_sel.as_ref().map_or_else(|| self.selected_notes.contains(&i), |s| s.contains(&i));
            // track color with a darker border; selected: lighter with a white border and a soft glow, readable
            // even during playback
            let (fill, border) = note_colors(base, sel);
            if sel {
                gpainter.rect_stroke(
                    r.expand(2.0),
                    Rounding::same(3.5_f32),
                    Stroke::new(2.0_f32, Color32::from_rgba_unmultiplied(255, 255, 255, 70)),
                );
            }
            gpainter.rect_filled(r, Rounding::same(2.5_f32), fill);
            gpainter.rect_stroke(r.shrink(0.5), Rounding::same(2.0_f32), Stroke::new(if sel { 1.5 } else { 1.0_f32 }, border));
            if previewing {
                continue;
            }
            let rr = r.intersect(grid);
            if !rr.is_positive() {
                continue;
            }
            let edge = Rect::from_min_max(pos2(rr.right() - 5.0, rr.top()), rr.max);
            // body first, resize edge after so the edge sits on top and gets the drag
            let resp = ui.interact(rr, ui.id().with(("n", rid, i)), Sense::click_and_drag());
            let eresp = ui.interact(edge, ui.id().with(("ne", rid, i)), Sense::drag());
            if eresp.hovered() {
                ui.ctx().set_cursor_icon(egui::CursorIcon::ResizeHorizontal);
            }
            if resp.hovered() || eresp.hovered() {
                note_hit = true;
            }
            let ctrl = ui.input(|i| i.modifiers.command || i.modifiers.shift);
            if resp.clicked() {
                if ctrl {
                    if !self.selected_notes.remove(&i) {
                        self.selected_notes.insert(i);
                    }
                } else {
                    self.selected_notes.clear();
                    self.selected_notes.insert(i);
                }
                self.live_note(
                    track.id,
                    MidiKind::NoteOn {
                        pitch: n.pitch,
                        velocity: n.velocity,
                    },
                );
                let tx = self.engine_tx.clone();
                let (tid, pitch) = (track.id, n.pitch);
                std::thread::spawn(move || {
                    std::thread::sleep(std::time::Duration::from_millis(250));
                    let _ = tx.send(ondina_engine::EngineMsg::Live(
                        tid,
                        MidiKind::NoteOff { pitch },
                    ));
                });
            }
            if resp.double_clicked() || (resp.secondary_clicked() && self.drag.is_none()) {
                self.cmd(Command::RemoveNotes {
                    region_id: rid,
                    indices: Some(vec![i]),
                    pitch: None,
                    from_beat: None,
                    to_beat: None,
                });
                self.selected_notes.clear();
            }
            if eresp.drag_started() {
                if !self.selected_notes.contains(&i) {
                    self.selected_notes.clear();
                    self.selected_notes.insert(i);
                }
                let gb = pointer
                    .map(|pp| sb + ((pp.x - grid.left()) / pbb) as f64)
                    .unwrap_or(0.0);
                self.drag = Some(Drag::NoteResize {
                    region: rid,
                    orig: m.notes.clone(),
                    indices: self.selected_notes.iter().copied().collect(),
                    grab_beat: gb,
                    d_len: 0.0,
                });
            } else if resp.drag_started() {
                if !self.selected_notes.contains(&i) {
                    self.selected_notes.clear();
                    self.selected_notes.insert(i);
                }
                if let Some(pp) = pointer {
                    let gb = sb + ((pp.x - grid.left()) / pbb) as f64;
                    let gp = top_pitch - ((pp.y - grid.top()) / kh).floor() as i32;
                    self.drag = Some(Drag::Notes {
                        region: rid,
                        orig: m.notes.clone(),
                        indices: self.selected_notes.iter().copied().collect(),
                        grab_beat: gb,
                        grab_pitch: gp,
                        d_beat: 0.0,
                        d_pitch: 0,
                    });
                }
            }
        }
        // click on empty grid = add note
        if gresp.clicked() && !note_hit && self.drag.is_none() {
            if let Some(pp) = gresp.interact_pointer_pos() {
                // Geometric guard, independent of widget ordering: a click that lands on an
                // existing note always selects it and never inserts a new one.
                let ctrl = ui.input(|i| i.modifiers.command || i.modifiers.shift);
                let hit = notes.iter().enumerate().rev().find(|(_, n)| {
                    let x0 = beat_x(n.start, sb);
                    let x1 = beat_x(n.start + n.length, sb).max(x0 + 3.0);
                    let y = pitch_y(n.pitch as i32, top_pitch);
                    pp.x >= x0 && pp.x <= x1 && pp.y >= y && pp.y <= y + kh
                });
                if let Some((i, _)) = hit {
                    if ctrl {
                        if !self.selected_notes.remove(&i) {
                            self.selected_notes.insert(i);
                        }
                    } else {
                        self.selected_notes.clear();
                        self.selected_notes.insert(i);
                    }
                } else {
                    let raw = sb + ((pp.x - grid.left()) / pbb) as f64;
                    match note_selection::grid_click(raw, self.snap, region.length, ctrl) {
                        note_selection::GridClick::Draw { start } => {
                            let pitch = (top_pitch - ((pp.y - grid.top()) / kh).floor() as i32)
                                .clamp(0, 127) as u8;
                            let note = Note {
                                pitch,
                                velocity: 100,
                                start,
                                length: self.note_len,
                            };
                            // exec, not cmd: the result says where the note landed, so it can
                            // become the selection right away.
                            if let Some(v) = self.exec(Command::AddNotes {
                                region_id: rid,
                                notes: vec![note],
                            }) {
                                let drawn = note_selection::result_indices(&v);
                                self.selected_notes =
                                    note_selection::select_drawn(&self.selected_notes, &drawn, ctrl);
                                self.live_note(
                                    track.id,
                                    MidiKind::NoteOn {
                                        pitch,
                                        velocity: 100,
                                    },
                                );
                                let tx = self.engine_tx.clone();
                                let tid = track.id;
                                std::thread::spawn(move || {
                                    std::thread::sleep(std::time::Duration::from_millis(200));
                                    let _ = tx.send(ondina_engine::EngineMsg::Live(
                                        tid,
                                        MidiKind::NoteOff { pitch },
                                    ));
                                });
                            }
                        }
                        note_selection::GridClick::Deselect => self.selected_notes.clear(),
                        note_selection::GridClick::Keep => {}
                    }
                }
            }
        }
        // drag on empty grid = start rubber-band selection
        if gresp.drag_started() && self.drag.is_none() {
            if let Some(pp) = gresp.interact_pointer_pos() {
                let additive = ui.input(|i| i.modifiers.command || i.modifiers.shift);
                let (b, p) = (x_to_beat(pp.x), y_to_pitch(pp.y));
                self.drag = Some(Drag::Marquee { region: rid, b0: b, p0: p, b1: b, p1: p, additive });
            }
        }
        // draw the rubber band
        if let Some(Drag::Marquee { region, b0, p0, b1, p1, .. }) = &self.drag {
            if *region == rid {
                let to_y = |p: f32| grid.top() + (top_pitch as f32 + 1.0 - p) * kh;
                let r = Rect::from_two_pos(pos2(beat_x(*b0, sb), to_y(*p0)), pos2(beat_x(*b1, sb), to_y(*p1)));
                gpainter.rect_filled(r, Rounding::ZERO, Color32::from_rgba_unmultiplied(120, 170, 255, 40));
                gpainter.rect_stroke(r, Rounding::ZERO, Stroke::new(1.2_f32, Color32::from_rgb(140, 185, 255)));
            }
        }
        // velocity lane: a stem with a round head per note, in the track color
        let vel_band = Rect::from_min_max(lay.vel_label.min, vel_rect.max);
        painter.rect_filled(vel_band, Rounding::ZERO, theme::HEADER_BG);
        painter.hline(vel_band.x_range(), vel_band.top() + 0.5, Stroke::new(1.0_f32, theme::HAIRLINE));
        painter.vline(lay.vel_label.right() - 0.5, lay.vel_label.y_range(), Stroke::new(1.0_f32, theme::SEPARATOR));
        painter.text(
            lay.vel_label.left_top() + vec2(6.0, 5.0),
            Align2::LEFT_TOP,
            "Velocity",
            theme::label_font(),
            theme::TEXT_3,
        );
        let vpainter = painter.with_clip_rect(vel_rect);
        // stems stand on `vel_base` and reach `vel_span` above it at 127, leaving room for the heads
        let vel_base = vel_rect.bottom() - 4.0;
        let vel_span = (vel_rect.height() - 10.0).max(1.0);
        for (i, n) in notes.iter().enumerate() {
            let x = beat_x(n.start, sb) + 2.0;
            if x < vel_rect.left() - 4.0 || x > vel_rect.right() + 4.0 {
                continue;
            }
            let top = vel_base - (n.velocity as f32 / 127.0) * vel_span;
            let sel = preview_sel.as_ref().map_or_else(|| self.selected_notes.contains(&i), |s| s.contains(&i));
            let (stem, head) = velocity_colors(base, sel);
            vpainter.line_segment([pos2(x, vel_base), pos2(x, top)], Stroke::new(2.0_f32, stem));
            vpainter.circle_filled(pos2(x, top), 3.0, head);
        }
        let vresp = ui.interact(vel_rect, ui.id().with(("vel", rid)), Sense::drag());
        let vel_tol = f64::from(4.0 / pbb);
        let vel_at = |y: f32| note_drag::velocity_from_y(y, vel_base, vel_span);
        if vresp.drag_started() {
            let mut stroke = VelocityStroke::new(m.notes.clone());
            // egui reports the drag a few pixels after the press: start the stroke where the button went down.
            if let Some(o) = ui.input(|i| i.pointer.press_origin()) {
                stroke.add(x_to_beat(o.x), vel_at(o.y), vel_tol, &self.selected_notes);
            }
            self.drag = Some(Drag::Velocity { region: rid, stroke });
        }
        // follows the app's drag, not egui's: a right click in the middle ends egui's drag but not the stroke
        if let (Some(pp), Some(Drag::Velocity { region, stroke })) = (pointer, self.drag.as_mut()) {
            if *region == rid {
                stroke.add(x_to_beat(pp.x), vel_at(pp.y), vel_tol, &self.selected_notes);
            }
        }
        // controller lane: a header across the editor (open / close, controller, hints), the drawing area under it
        let ctrl = self.cc_lane.controller;
        let cc_band = Rect::from_min_max(lay.cc_label.min, lay.cc.max);
        let cc_head = Rect::from_min_max(cc_band.min, pos2(cc_band.right(), (cc_band.top() + cc_lane::HEADER_H).min(cc_band.bottom())));
        painter.rect_filled(cc_band, Rounding::ZERO, theme::LANE_BG);
        painter.rect_filled(cc_head, Rounding::ZERO, theme::HEADER_BG);
        painter.hline(cc_band.x_range(), cc_band.top() + 0.5, Stroke::new(1.0_f32, theme::HAIRLINE));
        painter.vline(lay.cc_label.right() - 0.5, lay.cc_label.y_range(), Stroke::new(1.0_f32, theme::SEPARATOR));
        let cc_body = if self.cc_lane.open { cc_lane::body_rect(lay.cc) } else { None };
        let events = m.cc.iter().filter(|e| e.controller == ctrl).count();
        let mut stroke_value: Option<u8> = None;
        // a stroke keeps drawing if the lane closes or shrinks under it
        let cc_area = cc_body.unwrap_or(lay.cc);
        let cc_value_at = |y: f32| cc_lane::value_from_y(y, cc_area.top() + 3.0, cc_area.bottom() - 3.0);
        if let Some(body) = cc_body {
            painter.hline(cc_band.x_range(), body.top() - 0.5, Stroke::new(1.0_f32, theme::HAIRLINE));
            let cresp = ui.interact(body, ui.id().with(("cc", rid)), Sense::click_and_drag());
            let stroke = match &self.drag {
                Some(Drag::CcStroke { region, stroke }) if *region == rid => Some(stroke.clone()),
                _ => None,
            };
            let shown = stroke.as_ref().map_or_else(|| m.cc.clone(), |s| s.preview(&m.cc));
            let segs = cc_lane::segments(&shown, ctrl, region.length);
            let cpainter = painter.with_clip_rect(body);
            cc_lane::paint(&cpainter, body, &segs, ctrl, base, &|b| beat_x(b, sb));
            match &stroke {
                Some(s) if s.erase => {
                    if let Some((from, to)) = s.range() {
                        let r = Rect::from_min_max(pos2(beat_x(from, sb), body.top()), pos2(beat_x(to, sb), body.bottom()));
                        cpainter.rect_filled(r, Rounding::ZERO, Color32::from_rgba_unmultiplied(220, 60, 60, 40));
                    }
                }
                Some(s) => stroke_value = s.last_value(),
                None => {}
            }
            if segs.is_empty() && stroke.is_none() {
                painter.text(
                    pos2(body.left() + 8.0, body.center().y),
                    Align2::LEFT_CENTER,
                    format!("No {} events: drag to draw", cc_lane::short_name(ctrl)),
                    theme::body_font(),
                    theme::TEXT_3,
                );
            }
            // the gutter shows the lane's range
            let small = theme::label_font();
            painter.text(pos2(lay.cc_label.right() - 6.0, body.top() + 2.0), Align2::RIGHT_TOP, "127", small.clone(), theme::TEXT_DISABLED);
            painter.text(pos2(lay.cc_label.right() - 6.0, body.bottom() - 2.0), Align2::RIGHT_BOTTOM, "0", small, theme::TEXT_DISABLED);
            if self.drag.is_none() {
                let erase_drag = cresp.drag_started_by(PointerButton::Secondary);
                let start = if erase_drag || cresp.drag_started_by(PointerButton::Primary) {
                    ui.input(|i| i.pointer.press_origin())
                } else {
                    None
                };
                let click = if cresp.clicked() || cresp.secondary_clicked() { cresp.interact_pointer_pos() } else { None };
                if let Some(pp) = start.or(click) {
                    let erase = if start.is_some() { erase_drag } else { cresp.secondary_clicked() };
                    let mut s = CcStroke::new(ctrl, erase, cc_lane::draw_step(self.snap), region.length);
                    s.add(x_to_beat(pp.x), cc_value_at(pp.y));
                    if start.is_some() {
                        self.drag = Some(Drag::CcStroke { region: rid, stroke: s });
                    } else if let Some((from, to, events)) = s.command_parts(&m.cc) {
                        self.cmd(Command::ReplaceCc { region_id: rid, controller: ctrl, from_beat: Some(from), to_beat: Some(to), events });
                    }
                }
            }
            let _ = cresp.on_hover_text("Controller lane · drag: draw · right-drag: erase · click: one point");
        }
        // header: the open / close toggle in the gutter, then the controller and what the lane tells
        let toggle_rect = Rect::from_min_max(cc_head.min, pos2(lay.cc_label.right(), cc_head.bottom()));
        let tresp = ui
            .interact(toggle_rect, ui.id().with(("cc-toggle", rid)), Sense::click())
            .on_hover_text(if self.cc_lane.open { "Hide the controller lane" } else { "Show the controller lane" });
        if tresp.hovered() {
            painter.rect_filled(toggle_rect.shrink(1.0), Rounding::same(theme::R_SM), theme::HOVER);
        }
        let toggle_color = if tresp.hovered() { theme::TEXT } else { theme::TEXT_2 };
        let chevron = if self.cc_lane.open { Icon::ChevronDown } else { Icon::ChevronRight };
        let chevron_rect = Rect::from_center_size(pos2(toggle_rect.left() + 10.0, toggle_rect.center().y), vec2(10.0, 10.0));
        widgets::paint_icon(&painter, chevron, chevron_rect, toggle_color);
        painter.text(
            pos2(toggle_rect.left() + 18.0, toggle_rect.center().y),
            Align2::LEFT_CENTER,
            "CC",
            theme::label_font(),
            toggle_color,
        );
        if tresp.clicked() {
            self.cc_lane.open = !self.cc_lane.open;
        }
        let head_rest = Rect::from_min_max(pos2(lay.cc.left() + theme::SP_2, cc_head.top()), pos2(cc_head.right() - theme::SP_2, cc_head.bottom()));
        let mut g = ui.new_child(egui::UiBuilder::new().max_rect(head_rest).layout(egui::Layout::left_to_right(egui::Align::Center)));
        g.spacing_mut().interact_size.y = cc_lane::HEADER_H - 4.0;
        g.spacing_mut().button_padding.y = 0.0;
        let _ = egui::ComboBox::from_id_salt("cc_ctrl")
            .width(130.0)
            .selected_text(RichText::new(cc_lane::controller_name(ctrl)).size(theme::SIZE_LABEL + 1.0))
            .show_ui(&mut g, |ui| {
                for (c, n) in cc_lane::lane_choices(&m.cc) {
                    let label = if n > 0 { format!("{} · {n}", cc_lane::controller_name(c)) } else { cc_lane::controller_name(c) };
                    ui.selectable_value(&mut self.cc_lane.controller, c, label);
                }
            })
            .response
            .on_hover_text("Controller shown and drawn in this lane. The built-in instruments respond only to Sustain (64), the Drum Synth to none; VST3 instruments receive every controller.");
        let count = match events {
            0 => "no events".to_string(),
            1 => "1 event".to_string(),
            n => format!("{n} events"),
        };
        g.label(RichText::new(count).size(theme::SIZE_LABEL).color(theme::TEXT_3));
        g.with_layout(egui::Layout::right_to_left(egui::Align::Center), |g| {
            let kind = track.instrument.as_ref().map(|pl| pl.kind.as_str());
            if let Some(v) = stroke_value {
                g.label(RichText::new(cc_lane::value_label(ctrl, v)).font(theme::number_font()).color(theme::TEXT));
            } else if let Some(hint) = cc_lane::ignored_hint(kind, ctrl) {
                g.label(RichText::new(hint).size(theme::SIZE_LABEL).color(theme::MONITOR));
            } else if self.cc_lane.open {
                g.label(RichText::new("drag: draw · right-drag: erase").size(theme::SIZE_LABEL).color(theme::TEXT_DISABLED));
            }
        });
        // playhead (inside a looped region it is drawn in the repeat being played)
        let px = beat_x(local, sb);
        if px >= grid.left() && px <= grid.right() {
            painter.line_segment(
                [pos2(px, lay.ruler.top()), pos2(px, full.bottom())],
                Stroke::new(1.0_f32, theme::PLAYHEAD),
            );
            let t = lay.ruler.top();
            painter.add(egui::Shape::convex_polygon(
                vec![pos2(px - 4.0, t), pos2(px + 4.0, t), pos2(px, t + 6.0)],
                theme::PLAYHEAD,
                Stroke::NONE,
            ));
        }
        // drag updates / commit
        let released = ui.input(|i| crate::drag_cancel::drag_finished(&i.pointer, self.drag.as_ref()));
        if let Some(pp) = pointer {
            let snap = if self.snap > 0.0 {
                self.snap.min(1.0)
            } else {
                0.0
            };
            match &mut self.drag {
                Some(Drag::Notes {
                    grab_beat,
                    grab_pitch,
                    d_beat,
                    d_pitch,
                    ..
                }) => {
                    let b = sb + ((pp.x - grid.left()) / pbb) as f64;
                    let mut d = b - *grab_beat;
                    if snap > 0.0 {
                        d = (d / snap).round() * snap;
                    }
                    *d_beat = d;
                    *d_pitch =
                        (top_pitch - ((pp.y - grid.top()) / kh).floor() as i32) - *grab_pitch;
                }
                Some(Drag::NoteResize {
                    grab_beat, d_len, ..
                }) => {
                    let b = sb + ((pp.x - grid.left()) / pbb) as f64;
                    let mut d = b - *grab_beat;
                    if snap > 0.0 {
                        d = (d / snap).round() * snap;
                    }
                    *d_len = d;
                }
                Some(Drag::Marquee { b1, p1, .. }) => {
                    *b1 = x_to_beat(pp.x.clamp(grid.left(), grid.right()));
                    *p1 = y_to_pitch(pp.y.clamp(grid.top(), grid.bottom()));
                }
                Some(Drag::CcStroke { stroke, .. }) => stroke.add(x_to_beat(pp.x), cc_value_at(pp.y)),
                _ => {}
            }
        }
        if released {
            match self.drag.take() {
                Some(Drag::Notes {
                    region,
                    orig,
                    indices,
                    d_beat,
                    d_pitch,
                    ..
                }) => {
                    let copy = ui.input(|i| i.modifiers.alt);
                    if let Some(drop) = note_drag::drop_notes(&orig, &indices, d_beat, d_pitch, copy) {
                        let (c, count) = (drop.command(region), drop.count());
                        if self.apply_note_edit(region, c, drop.into_edit()) && copy {
                            self.status = format!("Duplicated {count} notes");
                        }
                    }
                }
                Some(Drag::NoteResize {
                    region,
                    orig,
                    indices,
                    d_len,
                    ..
                }) => {
                    if d_len != 0.0 {
                        let mut nn = orig;
                        for i in &indices {
                            if let Some(n) = nn.get_mut(*i) {
                                n.length = (n.length + d_len).max(0.03);
                            }
                        }
                        self.cmd(Command::ReplaceNotes {
                            region_id: region,
                            notes: nn,
                        });
                    }
                }
                Some(Drag::Velocity { region, stroke }) => {
                    if let Some(notes) = stroke.changed() {
                        self.cmd(Command::ReplaceNotes { region_id: region, notes });
                    }
                }
                Some(Drag::Marquee { region, b0, p0, b1, p1, additive }) if region == rid => {
                    // select every note touching the rectangle, including overlapping /
                    // stacked ones and those scrolled out of view
                    let (bmin, bmax) = (b0.min(b1), b0.max(b1));
                    let (pmin, pmax) = (p0.min(p1), p0.max(p1));
                    if !additive {
                        self.selected_notes.clear();
                    }
                    for (i, n) in m.notes.iter().enumerate() {
                        let (ns, ne) = (n.start, n.start + n.length);
                        let (np0, np1) = (n.pitch as f32, n.pitch as f32 + 1.0);
                        if ns < bmax && ne > bmin && np0 < pmax && np1 > pmin {
                            self.selected_notes.insert(i);
                        }
                    }
                }
                Some(Drag::CcStroke { region, stroke }) if region == rid => {
                    if let Some((from, to, events)) = stroke.command_parts(&m.cc) {
                        self.cmd(Command::ReplaceCc { region_id: region, controller: stroke.controller, from_beat: Some(from), to_beat: Some(to), events });
                    }
                }
                Some(Drag::CcStroke { .. }) => {}
                other => self.drag = other,
            }
        }
        ui.allocate_rect(full, Sense::hover());
    }

    /// Run a note command on the current selection and keep the same notes selected: Quantize and
    /// Humanize re-sort the region, and the result says where the selected notes went.
    pub(crate) fn exec_on_selection(&mut self, c: Command) -> Option<serde_json::Value> {
        let had_selection = !self.selected_notes.is_empty();
        let v = self.exec(c)?;
        if had_selection {
            if let Some(ix) = v.get("indices").and_then(|x| x.as_array()) {
                self.selected_notes = ix.iter().filter_map(|i| i.as_u64()).map(|i| i as usize).collect();
            }
        }
        Some(v)
    }

    /// Run add_harmony and select the notes it added, so the new voice can be moved or deleted at once.
    pub(crate) fn add_harmony(&mut self, c: Command) {
        let Some(v) = self.exec(c) else { return };
        self.selected_notes = note_selection::select_added(&self.selected_notes, &v);
        if note_selection::result_indices(&v).is_empty() {
            self.status = "Harmony: no new notes (they are already there or out of range)".into();
        }
    }

    /// Copy, cut, paste, select all and arrow moves on the piano roll's notes. Runs from `handle_keys` before the
    /// panels, so no widget sees the keys it uses. Returns whether the arrow keys belong to the selected notes this
    /// frame, so the track keys leave them alone.
    pub(crate) fn piano_roll_keys(&mut self, ctx: &egui::Context) -> bool {
        if !nc::keys_on(self.bottom == BottomTab::Editor, self.extra.show_score, ctx.wants_keyboard_input(), self.drag.is_some()) {
            return false;
        }
        let p = self.project();
        let Some(rid) = self.selected_region else { return false };
        let Some((_, region)) = p.find_region(rid) else { return false };
        let RegionContent::Midi(m) = &region.content else { return false };
        let has_sel = !self.selected_notes.is_empty();
        let has_clip = self.note_clipboard.is_some();
        let arrows = nc::arrows_on(true, self.musical_typing, has_sel);
        let (cut, copy, paste, repeat, all, length, nudge) = ctx.input_mut(|i| {
            let repeat = nc::consume_repeat(i, has_sel);
            let cut = has_sel && take_event(&mut i.events, |e| matches!(e, egui::Event::Cut));
            let copy = has_sel && take_event(&mut i.events, |e| matches!(e, egui::Event::Copy));
            let paste = has_clip && take_event(&mut i.events, |e| matches!(e, egui::Event::Paste(_)));
            let all = i.consume_key(egui::Modifiers::COMMAND, Key::A);
            let length = if arrows { consume_alt_arrow(i) } else { None };
            let nudge = if arrows && length.is_none() { consume_arrow(i) } else { None };
            (cut, copy, paste, repeat, all, length, nudge)
        });
        if cut || copy {
            let Some(clip) = nc::copy(&m.notes, &self.selected_notes) else { return arrows };
            let count = clip.notes.len();
            ctx.copy_text(nc::clip_text(&clip));
            self.note_clipboard = Some(clip);
            self.region_clipboard = None;
            if cut {
                let mut idx: Vec<usize> = self.selected_notes.iter().copied().collect();
                idx.sort_unstable();
                self.cmd(Command::RemoveNotes { region_id: rid, indices: Some(idx), pitch: None, from_beat: None, to_beat: None });
                self.selected_notes.clear();
                self.status = format!("Cut {count} notes");
            } else {
                self.status = format!("Copied {count} notes");
            }
        } else if paste {
            let Some(clip) = self.note_clipboard.clone() else { return arrows };
            let at = nc::paste_start(&clip, nc::playhead_in_region(self.position(), region), nc::grid(self.snap));
            let added = nc::pasted_notes(&clip, at, region.length);
            if added.is_empty() {
                self.status = "Nothing pasted: the notes would start past the end of the region".into();
            } else {
                let count = added.len();
                let edit = nc::paste(&m.notes, &added);
                if self.apply_note_edit(rid, Command::AddNotes { region_id: rid, notes: added }, edit) {
                    self.status = format!("Pasted {count} notes");
                }
            }
        } else if repeat {
            self.repeat_notes(rid, region.length, &m.notes);
        } else if all {
            self.selected_notes = nc::select_all(m.notes.len());
        } else if let Some(steps) = length {
            if let Some(edit) = nc::resize(&m.notes, &self.selected_notes, steps, nc::nudge_step(self.snap)) {
                self.apply_note_edit(rid, Command::ReplaceNotes { region_id: rid, notes: edit.notes.clone() }, edit);
            }
        } else if let Some(n) = nudge {
            let d_beat = f64::from(n.steps) * nc::nudge_step(self.snap);
            if let Some(edit) = nc::nudge(&m.notes, &self.selected_notes, d_beat, n.semitones) {
                self.apply_note_edit(rid, Command::ReplaceNotes { region_id: rid, notes: edit.notes.clone() }, edit);
            }
        }
        arrows
    }

    /// Run a command whose resulting notes and selection were worked out in advance, and select the edited notes.
    /// `exec` already moved the old selection along, so setting it afterwards is not remapped twice. When the
    /// project changed from elsewhere in between, the stored notes differ and the selection is cleared instead of
    /// pointing at the wrong notes.
    fn apply_note_edit(&mut self, rid: Id, c: Command, edit: nc::NoteEdit) -> bool {
        if self.exec(c).is_none() {
            return false;
        }
        let stored = note_selection::region_notes(&self.view.project, rid);
        self.selected_notes = if stored == Some(edit.notes.as_slice()) { edit.selected } else { HashSet::new() };
        true
    }

    /// Ctrl+D with notes selected: the selected notes again right after themselves, one `AddNotes` (one undo step),
    /// and the copies become the selection so the next Ctrl+D repeats the phrase again.
    fn repeat_notes(&mut self, rid: Id, region_length: f64, notes: &[Note]) {
        let Some(added) = nc::repeat_after(notes, &self.selected_notes, nc::grid(self.snap), region_length) else { return };
        if added.is_empty() {
            self.status = "Nothing repeated: the copies would start past the end of the region".into();
            return;
        }
        let count = added.len();
        let edit = nc::paste(notes, &added);
        if self.apply_note_edit(rid, Command::AddNotes { region_id: rid, notes: added }, edit) {
            self.status = format!("Repeated {count} notes");
        }
    }

    /// Whether Edit ▸ Repeat Selected Notes acts: notes are selected in the piano roll shown in the Editor tab.
    pub(crate) fn can_repeat_notes(&self) -> bool {
        self.bottom == BottomTab::Editor && !self.extra.show_score && !self.selected_notes.is_empty() && self.drag.is_none()
    }

    /// Edit ▸ Repeat Selected Notes: what Ctrl+D does in the piano roll.
    pub(crate) fn repeat_selected_notes(&mut self) {
        let p = self.project();
        let Some(rid) = self.selected_region else { return };
        let Some((_, region)) = p.find_region(rid) else { return };
        let RegionContent::Midi(m) = &region.content else { return };
        self.repeat_notes(rid, region.length, &m.notes);
    }

    fn step_sequencer(
        &mut self,
        ui: &mut egui::Ui,
        track: &Track,
        region: &Region,
        pt: &PatternContent,
    ) {
        let rid = region.id;
        ui.horizontal(|ui| {
            let mut swing = pt.swing;
            if ui.add(egui::Slider::new(&mut swing, 0.0..=1.0).text("Swing")).changed() {
                self.cmd_merge(Command::SetPatternSwing { region_id: rid, swing }, format!("swing{rid}"));
            }
            let mut steps = pt.steps;
            if ui.add(egui::DragValue::new(&mut steps).range(1..=64).prefix("Steps ")).changed() {
                self.cmd_merge(Command::ResizeRegion { region_id: rid, length: steps as f64 * pt.step_length }, format!("steps{rid}"));
            }
            if ui.button("Convert to MIDI").clicked() {
                self.cmd(Command::ConvertPatternToMidi { region_id: rid });
            }
            if self.ai_button(ui, "AI: write a groove") {
                self.ask_ai(ondina_studio::prompts::WRITE_GROOVE.into());
            }
            ui.label(RichText::new("Help").color(theme::TEXT_3)).on_hover_text(STEP_HELP);
        });
        // the step under the playhead, in whichever repeat of a looped pattern it is
        let playing_step = if self.playing() {
            playing_pattern_step(self.position() - region.start, region.length, region.loop_count, pt.step_length)
        } else {
            None
        };
        let steps = pt.steps as usize;
        egui::ScrollArea::both().drag_to_scroll(false).auto_shrink([false, false]).show(ui, |ui| {
            // a row added or removed elsewhere shifts the indices: drop what was aimed at the old rows
            if matches!(&self.drag, Some(Drag::StepPaint { region, stroke }) if *region == rid && !stroke.fits(pt.rows.len())) {
                self.drag = None;
            }
            if self.edits.row_rename.as_ref().is_some_and(|r| !r.fits(rid, &pt.rows)) {
                self.edits.row_rename = None;
            }
            // every name button's rect: Rename… opens inside the menu, after its own row was laid out
            let mut name_rects: Vec<Rect> = Vec::with_capacity(pt.rows.len());
            let mut cells: Vec<StepCell> = Vec::with_capacity(pt.rows.iter().map(|r| r.steps.len()).sum());
            let mut started: Option<(usize, usize)> = None;
            let base = theme::rgb(region.color.unwrap_or(track.color));
            // min_col_width 0: egui's default (the interact width, 40) would spread the 22-point cells apart
            egui::Grid::new(("steps", rid))
                .spacing(vec2(3.0, 3.0))
                .min_col_width(0.0)
                .show(ui, |ui| {
                    // step numbers over the cells; the playing one lights up
                    for _ in 0..4 {
                        ui.label("");
                    }
                    for si in 0..steps {
                        let gap = step_paint::group_gap(si);
                        let (rect, _) = ui.allocate_exact_size(vec2(STEP_W + gap, 14.0), Sense::hover());
                        let cell = Rect::from_min_size(rect.min + vec2(gap, 0.0), vec2(STEP_W, rect.height()));
                        let color = if playing_step == Some(si) {
                            theme::ACCENT
                        } else if si % 4 == 0 {
                            theme::TEXT_2
                        } else {
                            theme::TEXT_DISABLED
                        };
                        ui.painter().text(cell.center(), Align2::CENTER_CENTER, format!("{}", si + 1), FontId::monospace(9.5), color);
                    }
                    ui.end_row();
                    for (ri, row) in pt.rows.iter().enumerate() {
                        // name: click previews the sound, right-click opens the row menu
                        let (rect, name) = ui.allocate_exact_size(vec2(ROW_NAME_W, STEP_H), Sense::click());
                        let fill = if name.hovered() { theme::HOVER } else { theme::RAISED };
                        ui.painter().rect_filled(rect, Rounding::same(theme::R_SM), fill);
                        let color = if row.muted { theme::TEXT_DISABLED } else { theme::TEXT };
                        ui.painter().with_clip_rect(rect.shrink(2.0)).text(
                            rect.left_center() + vec2(theme::SP_3, 0.0),
                            Align2::LEFT_CENTER,
                            &row.name,
                            theme::body_font(),
                            color,
                        );
                        let name = name.on_hover_text(format!(
                            "{} · pitch {} ({}){} · click: preview · right-click: row menu",
                            row.name,
                            row.pitch,
                            note_name(row.pitch),
                            if row.muted { " · muted" } else { "" }
                        ));
                        name_rects.push(rect);
                        if name.clicked() {
                            self.preview_row(track.id, row.pitch);
                        }
                        crate::drag_cancel::menu_unless_dragging(&name, self.drag.is_some(), |ui| {
                            self.row_menu(ui, track.id, rid, ri, row, pt);
                        });
                        // mute, sound (its pitch) and the row menu
                        let (mrect, m) = ui.allocate_exact_size(widgets::STATE_BUTTON, Sense::click());
                        widgets::paint_state_button(ui.painter(), mrect, StateKind::Mute, row.muted, m.hovered());
                        if m.on_hover_text(if row.muted { "Unmute this row" } else { "Mute this row" }).clicked() {
                            self.cmd(step_paint::mute_command(rid, ri, row));
                        }
                        let sound = egui::Button::new(RichText::new(format!("{}", row.pitch)).font(theme::number_font()).color(theme::TEXT_2))
                            .min_size(vec2(30.0, STEP_H));
                        egui::menu::menu_custom_button(ui, sound, |ui| self.sound_items(ui, rid, ri, row))
                            .response
                            .on_hover_text(format!("Sound: {} ({}) · click to choose another", row.pitch, note_name(row.pitch)));
                        widgets::more_menu_button(ui, vec2(22.0, STEP_H), None, |ui| self.row_menu(ui, track.id, rid, ri, row, pt))
                            .response
                            .on_hover_text("Row menu");
                        for (si, v) in row.steps.iter().enumerate() {
                            let on = *v > 0;
                            let gap = step_paint::group_gap(si);
                            let (alloc, resp) = ui.allocate_exact_size(vec2(STEP_W + gap, STEP_H), Sense::click_and_drag());
                            let rect = Rect::from_min_size(alloc.min + vec2(gap, 0.0), vec2(STEP_W, STEP_H));
                            cells.push(StepCell { row: ri, step: si, rect });
                            if resp.clicked() {
                                self.cmd(Command::SetPatternStep {
                                    region_id: rid,
                                    row: ri,
                                    step: si,
                                    velocity: if on { 0 } else { PAINT_VELOCITY },
                                });
                            }
                            if resp.secondary_clicked() && self.drag.is_none() {
                                self.cmd(Command::SetPatternStep {
                                    region_id: rid,
                                    row: ri,
                                    step: si,
                                    velocity: step_paint::cycle_velocity(*v),
                                });
                            }
                            if resp.drag_started_by(PointerButton::Primary) {
                                started = Some((ri, si));
                            }
                        }
                        ui.end_row();
                    }
                });
            // the press decides the stroke's value; egui gives the drag to the cell under the press
            if let Some((r, s)) = started {
                if self.drag.is_none() {
                    let cur = pt.rows[r].steps[s];
                    self.drag = Some(Drag::StepPaint { region: rid, stroke: StepStroke::begin(r, s, cur).on_rows(pt.rows.len()) });
                }
            }
            // during a drag egui hovers only the pressed cell, so the stroke follows the pointer over the rects
            if let (Some(pos), Some(Drag::StepPaint { region, stroke })) =
                (ui.input(|i| i.pointer.latest_pos()), self.drag.as_mut())
            {
                if *region == rid {
                    if let Some((r, s)) = cell_at(&cells, pos) {
                        stroke.reach(r, s);
                    }
                }
            }
            let stroke = match &self.drag {
                Some(Drag::StepPaint { region, stroke }) if *region == rid => Some(stroke.clone()),
                _ => None,
            };
            paint_step_cells(ui, &cells, pt, stroke.as_ref(), playing_step, base);
            ui.menu_button("+ Row", |ui| {
                egui::ScrollArea::vertical().max_height(320.0).show(ui, |ui| {
                    for (name, pitch) in DRUM_SOUNDS {
                        if ui.button(step_paint::sound_label(name, pitch)).clicked() {
                            self.cmd(Command::AddPatternRow { region_id: rid, name: name.into(), pitch });
                            ui.close_menu();
                        }
                    }
                });
            })
            .response
            .on_hover_text("Add a drum sound as a new row");
            // the name field goes last, so it sits on top of the name button and gets the clicks
            let target = self.edits.row_rename.take().and_then(|ren| {
                let r = *name_rects.get(ren.row)?;
                Some((ren, r))
            });
            if let Some((mut ren, r)) = target {
                let edit_id = ui.id().with(("row-rename", rid, ren.row));
                match rename_field(ui, edit_id, r, theme::body_font(), &mut ren.text, &mut ren.focus) {
                    RenameEdit::Editing => self.edits.row_rename = Some(ren),
                    RenameEdit::Commit => {
                        if let Some(c) = ren.command(rid, &pt.rows) {
                            self.cmd(c);
                        }
                    }
                    RenameEdit::Cancel => {}
                }
            }
        });
        // the stroke reaches the project on release, as one undo step
        if ui.input(|i| crate::drag_cancel::drag_finished(&i.pointer, self.drag.as_ref())) {
            match self.drag.take() {
                Some(Drag::StepPaint { region, stroke }) if region == rid => {
                    let key = step_paint::paint_gesture_key();
                    for c in stroke.commands(rid, &pt.rows) {
                        self.cmd_merge(c, key.clone());
                    }
                }
                Some(Drag::StepPaint { .. }) => {}
                other => self.drag = other,
            }
        }
    }

    /// The row menu, on a right click on a row's name and on its "⋯" button.
    fn row_menu(&mut self, ui: &mut egui::Ui, track_id: Id, rid: Id, ri: usize, row: &PatternRow, pt: &PatternContent) {
        let steps = pt.steps as usize;
        ui.label(RichText::new(format!("{} · {} {}", row.name, row.pitch, note_name(row.pitch))).strong());
        ui.separator();
        if ui.button("Preview").clicked() {
            self.preview_row(track_id, row.pitch);
            ui.close_menu();
        }
        if ui.button(step_paint::mute_label(row.muted)).clicked() {
            self.cmd(step_paint::mute_command(rid, ri, row));
            ui.close_menu();
        }
        if ui.button("Rename…").clicked() {
            self.edits.row_rename = RowRename::open(rid, ri, &pt.rows);
            ui.close_menu();
        }
        ui.menu_button("Sound", |ui| self.sound_items(ui, rid, ri, row));
        ui.separator();
        for fill in RowFill::ALL {
            if ui.button(fill.label()).clicked() {
                if row.steps != fill.velocities(steps, pt.step_length) {
                    self.cmd(Command::SetPatternRow {
                        region_id: rid,
                        row: ri,
                        pattern: fill.pattern(steps, pt.step_length),
                    });
                }
                ui.close_menu();
            }
        }
        ui.separator();
        if ui.button(RichText::new("Delete row").color(theme::REC)).clicked() {
            self.cmd(Command::RemovePatternRow { region_id: rid, row: ri });
            ui.close_menu();
        }
    }

    /// The drum sounds a row can play; the row's own is checked.
    fn sound_items(&mut self, ui: &mut egui::Ui, rid: Id, ri: usize, row: &PatternRow) {
        egui::ScrollArea::vertical().max_height(320.0).show(ui, |ui| {
            for (sname, spitch) in DRUM_SOUNDS {
                if ui.selectable_label(row.pitch == spitch, step_paint::sound_label(sname, spitch)).clicked() {
                    if let Some(c) = step_paint::sound_command(rid, ri, row, (sname, spitch)) {
                        self.cmd(c);
                    }
                    ui.close_menu();
                }
            }
        });
    }

    /// Plays a pattern row's sound once.
    fn preview_row(&self, track_id: Id, pitch: u8) {
        self.live_note(track_id, MidiKind::NoteOn { pitch, velocity: 110 });
        self.live_note(track_id, MidiKind::NoteOff { pitch });
    }

    fn audio_editor(&mut self, ui: &mut egui::Ui, p: &Project, region: &Region, a: &AudioContent) {
        let rid = region.id;
        let f = p.audio_file(a.file_id);
        ui.horizontal(|ui| {
            if let Some(f) = f {
                ui.label(
                    RichText::new(format!(
                        "{} · {} Hz · {} ch · {:.2}s",
                        f.path, f.sample_rate, f.channels, f.duration_sec
                    ))
                    .small()
                    .color(theme::TEXT_DIM),
                );
            }
        });
        self.audio_tools(ui, region);
        let props = |gain: Option<f32>, fi: Option<f64>, fo: Option<f64>, rev: Option<bool>| {
            Command::SetRegionProperties {
                region_id: rid,
                name: None,
                muted: None,
                loop_count: None,
                transpose: None,
                velocity_offset: None,
                gain_db: gain,
                fade_in: fi,
                fade_out: fo,
                reverse: rev,
                color: None,
            }
        };
        egui::Grid::new("audioprops").show(ui, |ui| {
            let mut g = a.gain_db;
            ui.label("Gain");
            if ui
                .add(egui::Slider::new(&mut g, -30.0..=12.0).suffix(" dB"))
                .changed()
            {
                self.cmd_merge(props(Some(g), None, None, None), format!("ag{rid}"));
            }
            ui.end_row();
            let mut fi = a.fade_in;
            ui.label("Fade in");
            if ui
                .add(egui::Slider::new(&mut fi, 0.0..=8.0).suffix(" beats").clamping(egui::SliderClamping::Edits))
                .changed()
            {
                self.cmd_merge(props(None, Some(fi), None, None), format!("fi{rid}"));
            }
            ui.end_row();
            let mut fo = a.fade_out;
            ui.label("Fade out");
            if ui
                .add(egui::Slider::new(&mut fo, 0.0..=8.0).suffix(" beats").clamping(egui::SliderClamping::Edits))
                .changed()
            {
                self.cmd_merge(props(None, None, Some(fo), None), format!("fo{rid}"));
            }
            ui.end_row();
            let mut rev = a.reverse;
            ui.label("Reverse");
            if ui.checkbox(&mut rev, "").changed() {
                self.cmd(props(None, None, None, Some(rev)));
            }
            ui.end_row();
        });
        // big waveform
        let rect = ui.available_rect_before_wrap();
        let painter = ui.painter_at(rect);
        painter.rect_filled(rect, Rounding::ZERO, theme::LANE);
        if let (Some(peaks), Some(f)) = (self.waveforms.get(&a.file_id), f) {
            let n = peaks.len().max(1);
            let mid = rect.center().y;
            let h = rect.height() * 0.45;
            let col = theme::ACCENT;
            for x in 0..rect.width() as usize {
                let idx = x * n / rect.width().max(1.0) as usize;
                if let Some((lo, hi)) = peaks.get(idx) {
                    let xx = rect.left() + x as f32;
                    painter.line_segment(
                        [pos2(xx, mid - hi * h), pos2(xx, mid - lo * h)],
                        Stroke::new(1.0_f32, col),
                    );
                }
            }
            let off = (a.offset_sec / f.duration_sec) as f32;
            let xo = rect.left() + off * rect.width();
            painter.line_segment(
                [pos2(xo, rect.top()), pos2(xo, rect.bottom())],
                Stroke::new(1.0_f32, Color32::WHITE),
            );
        }
        ui.allocate_rect(rect, Sense::hover());
    }
}

pub(crate) fn len_label(l: f64) -> &'static str {
    match l {
        x if x == 4.0 => "1/1",
        x if x == 2.0 => "1/2",
        x if x == 1.0 => "1/4",
        x if x == 0.5 => "1/8",
        x if x == 0.25 => "1/16",
        _ => "1/32",
    }
}

/// Paints the step sequencer's cells after the grid laid them out: a paint stroke in progress shows its value
/// in the cells it touched, accents get a dark bar on top, and the playing step's column is lit with a white
/// border on its cells.
fn paint_step_cells(
    ui: &egui::Ui,
    cells: &[StepCell],
    pt: &PatternContent,
    stroke: Option<&StepStroke>,
    playing_step: Option<usize>,
    track: Color32,
) {
    let column = cells
        .iter()
        .filter(|c| playing_step == Some(c.step))
        .map(|c| c.rect)
        .reduce(|a, b| a.union(b));
    if let Some(col) = column {
        ui.painter().rect_filled(col.expand2(vec2(2.0, 3.0)), Rounding::same(theme::R_SM), Color32::from_white_alpha(28));
    }
    for c in cells {
        let stored = pt.rows[c.row].steps[c.step];
        let v = stroke.map_or(stored, |st| st.shown(c.row, c.step, stored));
        let muted = pt.rows[c.row].muted;
        let col = step_paint::cell_color(v, c.step, muted, track);
        ui.painter().rect_filled(c.rect, Rounding::same(3.0_f32), col);
        if step_paint::step_level(v) == step_paint::StepLevel::Accent {
            let bar = Rect::from_min_size(c.rect.min + vec2(4.0, 3.0), vec2(c.rect.width() - 8.0, 3.0));
            let ink = theme::darker(col, 0.5);
            ui.painter().rect_filled(bar, Rounding::same(1.0_f32), ink);
        }
        if playing_step == Some(c.step) {
            ui.painter().rect_stroke(c.rect, Rounding::same(3.0_f32), Stroke::new(1.5_f32, Color32::WHITE));
        }
    }
}
