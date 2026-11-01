//! Bottom editors: Piano Roll, Step Sequencer and audio region editor.

use std::collections::HashSet;

use ondina_core::command::Command;
use ondina_core::model::*;
use ondina_core::theory::note_name;
use ondina_dsp::MidiKind;
use eframe::egui::{
    self, pos2, vec2, Align2, Color32, FontId, Key, PointerButton, Rect, RichText, Rounding, Sense, Stroke,
};

use crate::app::{BottomTab, OndinaApp, Drag};
use crate::cc_lane::{self, CcStroke};
use crate::note_clipboard as nc;
use crate::note_selection;
use crate::piano_roll_view::{
    follow_scroll, initial_view, loop_local_beat, playing_pattern_step, scroll_pitch, wheel_zoom, zoom_factor,
    zoom_pitch, zoom_time,
};
use crate::theme;

const KEYS_W: f32 = 54.0;
const VEL_H: f32 = 56.0;

fn is_black(p: i32) -> bool {
    matches!(p.rem_euclid(12), 1 | 3 | 6 | 8 | 10)
}

/// Removes the events that match `f` and tells whether there was one.
fn take_event(events: &mut Vec<egui::Event>, f: impl Fn(&egui::Event) -> bool) -> bool {
    let before = events.len();
    events.retain(|e| !f(e));
    events.len() != before
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
        let Some((track, region)) = self
            .selected_region
            .and_then(|id| p.find_region(id))
            .map(|(t, r)| (t.clone(), r.clone()))
        else {
            ui.centered_and_justified(|ui| {
                ui.label(RichText::new("Select a region to edit it (double-click an instrument lane to create one)").color(theme::TEXT_DIM));
            });
            return;
        };
        if matches!(
            region.content,
            RegionContent::Midi(_) | RegionContent::Pattern(_)
        ) {
            ui.horizontal(|ui| {
                ui.selectable_value(&mut self.extra.show_score, false, "🎹 Piano Roll / Step");
                ui.selectable_value(&mut self.extra.show_score, true, "𝄞 Score");
                if ui.button("MIDI Transform…").clicked() {
                    self.extra.show_transform = true;
                }
            });
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
            RegionContent::Midi(m) => self.piano_roll(ui, &p, &track, &region, m),
            RegionContent::Pattern(pt) => self.step_sequencer(ui, &track, &region, pt),
            RegionContent::Audio(a) => self.audio_editor(ui, &p, &region, a),
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
        // toolbar
        ui.horizontal(|ui| {
            ui.label(RichText::new(format!("🎹 {} — {}", track.name, region.name)).strong())
                .on_hover_text("Wheel: scroll pitch · Shift+wheel: scroll time · Ctrl+wheel: zoom time · Ctrl+Shift+wheel: zoom pitch · Ctrl+C/X/V: copy/cut/paste · Ctrl+A: select all · ←/→: move by the snap step · ↑/↓: semitone · Shift+↑/↓: octave · Controller lane: drag to draw, right-drag to erase");
            ui.separator();
            ui.label("Note length");
            egui::ComboBox::from_id_salt("notelen").width(60.0).selected_text(len_label(self.note_len)).show_ui(ui, |ui| {
                for l in [4.0, 2.0, 1.0, 0.5, 0.25, 0.125] {
                    ui.selectable_value(&mut self.note_len, l, len_label(l));
                }
            });
            let sel: Option<Vec<usize>> = if self.selected_notes.is_empty() { None } else { Some(self.selected_notes.iter().copied().collect()) };
            if ui.button("Quantize").on_hover_text("Quantize the selected notes to the snap grid (all notes when nothing is selected)").clicked() {
                let g = if self.snap > 0.0 { self.snap.min(1.0) } else { 0.25 };
                self.exec_on_selection(Command::QuantizeNotes { region_id: rid, grid: g, strength: Some(1.0), swing: None, quantize_ends: Some(false), indices: sel.clone() });
            }
            if ui.button("Humanize").on_hover_text("Humanize the selected notes (all notes when nothing is selected)").clicked() {
                self.exec_on_selection(Command::HumanizeNotes { region_id: rid, timing: Some(0.015), velocity: Some(10), seed: None, indices: sel.clone() });
            }
            if ui.button("↑ st").clicked() {
                self.cmd(Command::TransposeNotes { region_id: rid, semitones: 1, indices: sel.clone() });
            }
            if ui.button("↓ st").clicked() {
                self.cmd(Command::TransposeNotes { region_id: rid, semitones: -1, indices: sel.clone() });
            }
            if ui.button("↑ oct").clicked() {
                self.cmd(Command::TransposeNotes { region_id: rid, semitones: 12, indices: sel.clone() });
            }
            if ui.button("↓ oct").clicked() {
                self.cmd(Command::TransposeNotes { region_id: rid, semitones: -12, indices: sel.clone() });
            }
            if ui.button("Snap to key").on_hover_text("Move the selected notes to the nearest pitch in the project key (all notes when nothing is selected)").clicked() {
                self.exec_on_selection(Command::SnapNotesToKey { region_id: rid, indices: sel.clone() });
            }
            if ui.button("Select all").on_hover_text("Select every note (Ctrl+A)").clicked() {
                self.selected_notes = nc::select_all(m.notes.len());
            }
            ui.separator();
            let mut tr = m.transpose;
            if ui.add(egui::DragValue::new(&mut tr).range(-48..=48).prefix("Region transpose ")).changed() {
                self.cmd_merge(Command::SetRegionProperties { region_id: rid, name: None, muted: None, loop_count: None, transpose: Some(tr), velocity_offset: None, gain_db: None, fade_in: None, fade_out: None, reverse: None, color: None }, format!("tr{rid}"));
            }
            ui.separator();
            if ui.button(RichText::new("✨ AI: continue / improve").color(theme::AI)).clicked() {
                self.ask_ai(ondina_studio::prompts::CONTINUE_IMPROVE.into());
            }
        });
        let full = ui.available_rect_before_wrap();
        let cc_h = cc_lane::lane_height(self.cc_lane.open, full.height(), VEL_H);
        let grid = Rect::from_min_max(
            pos2(full.left() + KEYS_W, full.top()),
            pos2(full.right(), full.bottom() - VEL_H - cc_h),
        );
        let vel_rect = Rect::from_min_max(
            pos2(full.left() + KEYS_W, full.bottom() - VEL_H - cc_h + 2.0),
            pos2(full.right(), full.bottom() - cc_h),
        );
        let cc_rect = Rect::from_min_max(pos2(full.left() + KEYS_W, full.bottom() - cc_h + 2.0), full.max);
        let cc_gutter = Rect::from_min_max(pos2(full.left(), full.bottom() - cc_h + 2.0), pos2(full.left() + KEYS_W, full.bottom()));
        let painter = ui.painter_at(full);
        painter.rect_filled(full, Rounding::ZERO, theme::BG);
        // Frame the region's notes the first time it is shown; before the input so this frame already
        // draws the framed view.
        if self.pr_framing.take(rid) {
            let rows = ((grid.height() / self.pr_key_height).floor() as i32).max(1);
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
        // rows
        for k in 0..=rows {
            let pitch = top_pitch - k;
            let y = pitch_y(pitch, top_pitch);
            let row = Rect::from_min_max(pos2(grid.left(), y), pos2(grid.right(), y + kh));
            let in_key = p.key.contains(pitch.clamp(0, 127) as u8);
            painter.rect_filled(
                row,
                Rounding::ZERO,
                if is_black(pitch) {
                    theme::LANE_ALT
                } else if in_key {
                    Color32::from_rgb(46, 50, 56)
                } else {
                    theme::LANE
                },
            );
            painter.line_segment(
                [pos2(grid.left(), y + kh), pos2(grid.right(), y + kh)],
                Stroke::new(0.5_f32, theme::BG),
            );
            // keyboard
            let key = Rect::from_min_max(pos2(full.left(), y), pos2(full.left() + KEYS_W, y + kh));
            painter.rect_filled(
                key,
                Rounding::ZERO,
                if is_black(pitch) {
                    Color32::from_gray(30)
                } else {
                    Color32::from_gray(215)
                },
            );
            painter.line_segment(
                [
                    pos2(full.left(), y + kh),
                    pos2(full.left() + KEYS_W, y + kh),
                ],
                Stroke::new(0.5_f32, Color32::from_gray(120)),
            );
            if pitch.rem_euclid(12) == 0 {
                painter.text(
                    pos2(full.left() + KEYS_W - 3.0, y + kh / 2.0),
                    Align2::RIGHT_CENTER,
                    note_name(pitch.clamp(0, 127) as u8),
                    FontId::proportional(9.5),
                    Color32::from_gray(40),
                );
            }
            let kresp = ui.interact(key, ui.id().with(("pk", pitch)), Sense::click_and_drag());
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
        // beat grid
        let bpb = p.time_signature.beats_per_bar();
        let step = if pbb > 160.0 {
            0.25
        } else if pbb > 50.0 {
            0.5
        } else {
            1.0
        };
        let mut b = (sb / step).floor() * step;
        let visible = (grid.width() / pbb) as f64;
        while b <= sb + visible {
            let x = beat_x(b, sb);
            let bar = (b / bpb).fract().abs() < 1e-6;
            let beat = b.fract().abs() < 1e-6;
            painter.line_segment(
                [pos2(x, grid.top()), pos2(x, full.bottom())],
                Stroke::new(
                    if bar { 1.2 } else { 0.6_f32 },
                    if bar {
                        theme::GRID_BAR
                    } else if beat {
                        theme::GRID
                    } else {
                        Color32::from_rgb(48, 51, 58)
                    },
                ),
            );
            if bar {
                painter.text(
                    pos2(x + 2.0, grid.top() + 1.0),
                    Align2::LEFT_TOP,
                    format!("{}", ((region.start + b) / bpb).floor() as i64 + 1),
                    FontId::proportional(9.5),
                    theme::TEXT_DIM,
                );
            }
            b += step;
        }
        // region end shading
        let end_x = beat_x(region.length, sb);
        if end_x < grid.right() {
            painter.rect_filled(
                Rect::from_min_max(
                    pos2(end_x.max(grid.left()), grid.top()),
                    pos2(grid.right(), full.bottom()),
                ),
                Rounding::ZERO,
                Color32::from_black_alpha(110),
            );
        }
        // notes (with drag preview)
        let base = theme::rgb(region.color.unwrap_or(track.color));
        let mut notes = m.notes.clone();
        let mut previewing = false;
        match &self.drag {
            Some(Drag::Notes {
                region,
                orig,
                indices,
                d_beat,
                d_pitch,
                ..
            }) if *region == rid => {
                notes = orig.clone();
                for i in indices {
                    if let Some(n) = notes.get_mut(*i) {
                        n.start = (n.start + d_beat).max(0.0);
                        n.pitch = (n.pitch as i32 + d_pitch).clamp(0, 127) as u8;
                    }
                }
                previewing = true;
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
            Some(Drag::Velocity { region, notes: vn }) if *region == rid => {
                notes = vn.clone();
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
            let sel = self.selected_notes.contains(&i);
            // selected notes: light fill + thick white outline + outer glow, readable even during playback
            let fill = if sel {
                theme::velocity_color(base, n.velocity).lerp_to_gamma(Color32::WHITE, 0.55)
            } else {
                theme::velocity_color(base, n.velocity)
            };
            if sel {
                gpainter.rect_stroke(
                    r.expand(2.0),
                    Rounding::same(3.0_f32),
                    Stroke::new(2.0_f32, Color32::from_rgba_unmultiplied(255, 255, 255, 90)),
                );
            }
            gpainter.rect_filled(r, Rounding::same(2.0_f32), fill);
            gpainter.rect_stroke(
                r,
                Rounding::same(2.0_f32),
                Stroke::new(
                    if sel { 2.2 } else { 0.8_f32 },
                    if sel {
                        Color32::WHITE
                    } else {
                        Color32::from_black_alpha(170)
                    },
                ),
            );
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
            if resp.double_clicked() || resp.secondary_clicked() {
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
                    let start = if self.snap > 0.0 {
                        (raw / self.snap.min(1.0)).floor() * self.snap.min(1.0)
                    } else {
                        raw
                    };
                    let pitch =
                        (top_pitch - ((pp.y - grid.top()) / kh).floor() as i32).clamp(0, 127) as u8;
                    if start >= 0.0 && start < region.length {
                        self.cmd(Command::AddNotes {
                            region_id: rid,
                            notes: vec![Note {
                                pitch,
                                velocity: 100,
                                start,
                                length: self.note_len,
                            }],
                        });
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
                    self.selected_notes.clear();
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
        // velocity lane
        painter.rect_filled(vel_rect, Rounding::ZERO, theme::PANEL);
        painter.text(
            pos2(full.left() + 4.0, vel_rect.center().y),
            Align2::LEFT_CENTER,
            "Velocity",
            FontId::proportional(10.0),
            theme::TEXT_DIM,
        );
        let vpainter = painter.with_clip_rect(vel_rect);
        for (i, n) in notes.iter().enumerate() {
            let x = beat_x(n.start, sb);
            if x < vel_rect.left() || x > vel_rect.right() {
                continue;
            }
            let h = (n.velocity as f32 / 127.0) * (vel_rect.height() - 4.0);
            let sel = self.selected_notes.contains(&i);
            vpainter.line_segment(
                [
                    pos2(x + 1.0, vel_rect.bottom() - 2.0),
                    pos2(x + 1.0, vel_rect.bottom() - 2.0 - h),
                ],
                Stroke::new(
                    3.0_f32,
                    if sel {
                        Color32::WHITE
                    } else {
                        theme::velocity_color(base, n.velocity)
                    },
                ),
            );
        }
        let vresp = ui.interact(vel_rect, ui.id().with(("vel", rid)), Sense::drag());
        if vresp.drag_started() {
            self.drag = Some(Drag::Velocity {
                region: rid,
                notes: m.notes.clone(),
            });
        }
        if vresp.dragged() {
            if let (Some(pp), Some(Drag::Velocity { notes: vn, .. })) =
                (pointer, self.drag.as_mut())
            {
                let v = (((vel_rect.bottom() - pp.y) / (vel_rect.height() - 4.0)) * 127.0)
                    .clamp(1.0, 127.0) as u8;
                let only_sel = !self.selected_notes.is_empty();
                for (i, n) in vn.iter_mut().enumerate() {
                    let x = beat_x(n.start, sb);
                    if (x - pp.x).abs() < 4.0 && (!only_sel || self.selected_notes.contains(&i)) {
                        n.velocity = v;
                    }
                }
            }
        }
        // controller lane
        painter.rect_filled(cc_rect, Rounding::ZERO, theme::PANEL);
        let cresp = ui.interact(cc_rect, ui.id().with(("cc", rid)), Sense::click_and_drag());
        let ctrl = self.cc_lane.controller;
        let cc_value_at = |y: f32| cc_lane::value_from_y(y, cc_rect.top() + 3.0, cc_rect.bottom() - 3.0);
        if self.cc_lane.open {
            let stroke = match &self.drag {
                Some(Drag::CcStroke { region, stroke }) if *region == rid => Some(stroke.clone()),
                _ => None,
            };
            let shown = stroke.as_ref().map_or_else(|| m.cc.clone(), |s| s.preview(&m.cc));
            let segs = cc_lane::segments(&shown, ctrl, region.length);
            let cpainter = painter.with_clip_rect(cc_rect);
            cc_lane::paint(&cpainter, cc_rect, &segs, ctrl, base, &|b| beat_x(b, sb));
            let small = FontId::proportional(9.5);
            match &stroke {
                Some(s) if s.erase => {
                    if let Some((from, to)) = s.range() {
                        let r = Rect::from_min_max(pos2(beat_x(from, sb), cc_rect.top()), pos2(beat_x(to, sb), cc_rect.bottom()));
                        cpainter.rect_filled(r, Rounding::ZERO, Color32::from_rgba_unmultiplied(220, 60, 60, 40));
                    }
                }
                Some(s) => {
                    if let Some(v) = s.last_value() {
                        painter.text(pos2(cc_rect.right() - 4.0, cc_rect.top() + 2.0), Align2::RIGHT_TOP, cc_lane::value_label(ctrl, v), small.clone(), theme::TEXT_DIM);
                    }
                }
                None => {}
            }
            if segs.is_empty() && stroke.is_none() {
                painter.text(pos2(cc_rect.left() + 6.0, cc_rect.center().y), Align2::LEFT_CENTER, format!("No {} events: drag to draw", cc_lane::short_name(ctrl)), FontId::proportional(10.0), theme::TEXT_DIM);
            }
            let kind = track.instrument.as_ref().map(|pl| pl.kind.as_str());
            if let Some(hint) = cc_lane::ignored_hint(kind, ctrl) {
                painter.text(pos2(cc_rect.right() - 4.0, cc_rect.bottom() - 2.0), Align2::RIGHT_BOTTOM, hint, small, theme::TEXT_DIM);
            }
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
        let mut g = ui.new_child(egui::UiBuilder::new().max_rect(cc_gutter.shrink(2.0)).layout(egui::Layout::top_down(egui::Align::LEFT)));
        if g.selectable_label(self.cc_lane.open, "CC").on_hover_text("Show or hide the controller lane").clicked() {
            self.cc_lane.open = !self.cc_lane.open;
        }
        if self.cc_lane.open {
            let _ = egui::ComboBox::from_id_salt("cc_ctrl")
                .width(KEYS_W - 6.0)
                .selected_text(cc_lane::short_name(ctrl))
                .show_ui(&mut g, |ui| {
                    for (c, n) in cc_lane::lane_choices(&m.cc) {
                        let label = if n > 0 { format!("{} · {n}", cc_lane::controller_name(c)) } else { cc_lane::controller_name(c) };
                        ui.selectable_value(&mut self.cc_lane.controller, c, label);
                    }
                })
                .response
                .on_hover_text("Controller shown and drawn in this lane. The Analog Synth and FM Synth respond only to Sustain (64); VST3 instruments receive every controller.");
        }
        // playhead (inside a looped region it is drawn in the repeat being played)
        let px = beat_x(local, sb);
        if px >= grid.left() && px <= grid.right() {
            painter.line_segment(
                [pos2(px, grid.top()), pos2(px, full.bottom())],
                Stroke::new(1.2_f32, theme::PLAYHEAD),
            );
        }
        // drag updates / commit
        let released = ui.input(|i| i.pointer.any_released());
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
                    if d_beat != 0.0 || d_pitch != 0 {
                        let mut nn = orig;
                        for i in &indices {
                            if let Some(n) = nn.get_mut(*i) {
                                n.start = (n.start + d_beat).max(0.0);
                                n.pitch = (n.pitch as i32 + d_pitch).clamp(0, 127) as u8;
                            }
                        }
                        self.cmd(Command::ReplaceNotes {
                            region_id: region,
                            notes: nn,
                        });
                        self.selected_notes.clear();
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
                Some(Drag::Velocity { region, notes }) => {
                    self.cmd(Command::ReplaceNotes {
                        region_id: region,
                        notes,
                    });
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
    fn exec_on_selection(&mut self, c: Command) {
        let had_selection = !self.selected_notes.is_empty();
        let Some(v) = self.exec(c) else { return };
        if !had_selection {
            return;
        }
        if let Some(ix) = v.get("indices").and_then(|x| x.as_array()) {
            self.selected_notes = ix.iter().filter_map(|i| i.as_u64()).map(|i| i as usize).collect();
        }
    }

    /// Copy, cut, paste, select all and arrow moves on the piano roll's notes. Runs from `handle_keys` before the
    /// panels, so no widget sees the keys it uses.
    pub(crate) fn piano_roll_keys(&mut self, ctx: &egui::Context) {
        if !nc::keys_on(self.bottom == BottomTab::Editor, self.extra.show_score, ctx.wants_keyboard_input(), self.drag.is_some()) {
            return;
        }
        let p = self.project();
        let Some(rid) = self.selected_region else { return };
        let Some((_, region)) = p.find_region(rid) else { return };
        let RegionContent::Midi(m) = &region.content else { return };
        let has_sel = !self.selected_notes.is_empty();
        let has_clip = self.note_clipboard.is_some();
        let arrows = nc::arrows_on(true, self.musical_typing, has_sel);
        let (cut, copy, paste, all, nudge) = ctx.input_mut(|i| {
            let cut = has_sel && take_event(&mut i.events, |e| matches!(e, egui::Event::Cut));
            let copy = has_sel && take_event(&mut i.events, |e| matches!(e, egui::Event::Copy));
            let paste = has_clip && take_event(&mut i.events, |e| matches!(e, egui::Event::Paste(_)));
            let all = i.consume_key(egui::Modifiers::COMMAND, Key::A);
            let nudge = if arrows { consume_arrow(i) } else { None };
            (cut, copy, paste, all, nudge)
        });
        if cut || copy {
            let Some(clip) = nc::copy(&m.notes, &self.selected_notes) else { return };
            let count = clip.notes.len();
            ctx.copy_text(nc::clip_text(&clip));
            self.note_clipboard = Some(clip);
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
            let Some(clip) = self.note_clipboard.clone() else { return };
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
        } else if all {
            self.selected_notes = nc::select_all(m.notes.len());
        } else if let Some(n) = nudge {
            let d_beat = f64::from(n.steps) * nc::nudge_step(self.snap);
            if let Some(edit) = nc::nudge(&m.notes, &self.selected_notes, d_beat, n.semitones) {
                self.apply_note_edit(rid, Command::ReplaceNotes { region_id: rid, notes: edit.notes.clone() }, edit);
            }
        }
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

    fn step_sequencer(
        &mut self,
        ui: &mut egui::Ui,
        track: &Track,
        region: &Region,
        pt: &PatternContent,
    ) {
        let rid = region.id;
        ui.horizontal(|ui| {
            ui.label(RichText::new(format!("🥁 Step Sequencer — {} / {}", track.name, region.name)).strong());
            ui.separator();
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
            if ui.button(RichText::new("✨ AI: write a groove").color(theme::AI)).clicked() {
                self.ask_ai(ondina_studio::prompts::WRITE_GROOVE.into());
            }
        });
        ui.label(
            RichText::new("Click: on/off · Right-click: cycle accent → normal → ghost")
                .small()
                .color(theme::TEXT_DIM),
        );
        // the step under the playhead, in whichever repeat of a looped pattern it is
        let playing_step = if self.playing() {
            playing_pattern_step(self.position() - region.start, region.length, region.loop_count, pt.step_length)
        } else {
            None
        };
        egui::ScrollArea::both().show(ui, |ui| {
            egui::Grid::new(("steps", rid))
                .spacing(vec2(3.0, 3.0))
                .show(ui, |ui| {
                    for (ri, row) in pt.rows.iter().enumerate() {
                        if ui
                            .add(
                                egui::Button::new(
                                    RichText::new(format!("{:<11}", row.name)).monospace(),
                                )
                                .min_size(vec2(100.0, 20.0)),
                            )
                            .on_hover_text(format!(
                                "pitch {} ({})",
                                row.pitch,
                                note_name(row.pitch)
                            ))
                            .clicked()
                        {
                            self.live_note(
                                track.id,
                                MidiKind::NoteOn {
                                    pitch: row.pitch,
                                    velocity: 110,
                                },
                            );
                            self.live_note(track.id, MidiKind::NoteOff { pitch: row.pitch });
                        }
                        for (si, v) in row.steps.iter().enumerate() {
                            let on = *v > 0;
                            let beat_group = (si / 4) % 2 == 0;
                            let col = if on {
                                theme::velocity_color(theme::ACCENT, *v)
                            } else if beat_group {
                                theme::LANE
                            } else {
                                theme::LANE_ALT
                            };
                            let (rect, resp) =
                                ui.allocate_exact_size(vec2(22.0, 20.0), Sense::click());
                            ui.painter().rect_filled(rect, Rounding::same(3.0_f32), col);
                            if playing_step == Some(si) {
                                ui.painter().rect_stroke(
                                    rect,
                                    Rounding::same(3.0_f32),
                                    Stroke::new(1.5_f32, Color32::WHITE),
                                );
                            }
                            if resp.clicked() {
                                self.cmd(Command::SetPatternStep {
                                    region_id: rid,
                                    row: ri,
                                    step: si,
                                    velocity: if on { 0 } else { 100 },
                                });
                            }
                            if resp.secondary_clicked() {
                                let nv = match *v {
                                    0 => 120,
                                    101..=127 => 90,
                                    61..=100 => 45,
                                    _ => 0,
                                };
                                self.cmd(Command::SetPatternStep {
                                    region_id: rid,
                                    row: ri,
                                    step: si,
                                    velocity: nv,
                                });
                            }
                        }
                        ui.end_row();
                    }
                });
        });
    }

    fn audio_editor(&mut self, ui: &mut egui::Ui, p: &Project, region: &Region, a: &AudioContent) {
        let rid = region.id;
        let f = p.audio_file(a.file_id);
        ui.horizontal(|ui| {
            ui.label(RichText::new(format!("🎤 {}", region.name)).strong());
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

fn len_label(l: f64) -> &'static str {
    match l {
        x if x == 4.0 => "1/1",
        x if x == 2.0 => "1/2",
        x if x == 1.0 => "1/4",
        x if x == 0.5 => "1/8",
        x if x == 0.25 => "1/16",
        _ => "1/32",
    }
}
