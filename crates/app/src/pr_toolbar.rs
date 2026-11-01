//! The piano roll's toolbar, on the right of the editor header: quantize grid and Q, Humanize, transpose, Length,
//! Velocity, Snap to key, Harmony and the note length, then a "⋯" menu with the rest. On a narrow panel the tools
//! that do not fit move into the "⋯" menu, least used first, so the toolbar always stays on one line.

use eframe::egui::{self, pos2, vec2, Align2, Rect, RichText, Rounding, Sense, Stroke};
use ondina_core::command::Command;
use ondina_core::model::{Id, MidiContent, Region};

use crate::app::OndinaApp;
use crate::editors::{len_label, HARMONY_MENU, PIANO_ROLL_HELP};
use crate::note_clipboard as nc;
use crate::note_menus::{self, VelocityChoice};
use crate::theme;
use crate::widgets;

/// A tool of the toolbar.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Tool {
    DrawLength,
    QuantizeGrid,
    Quantize,
    Humanize,
    Transpose,
    Length,
    Velocity,
    SnapToKey,
    Harmony,
}

/// The tools in toolbar order, left to right.
pub const TOOLS: [Tool; 9] = [
    Tool::DrawLength,
    Tool::QuantizeGrid,
    Tool::Quantize,
    Tool::Humanize,
    Tool::Transpose,
    Tool::Length,
    Tool::Velocity,
    Tool::SnapToKey,
    Tool::Harmony,
];

impl Tool {
    /// Which tools stay longest when the toolbar is short of room: 1 is the last to go.
    pub fn priority(self) -> u8 {
        match self {
            Tool::Quantize => 1,
            Tool::Transpose => 2,
            Tool::QuantizeGrid => 3,
            Tool::Length => 4,
            Tool::Velocity => 5,
            Tool::DrawLength => 6,
            Tool::Humanize => 7,
            Tool::SnapToKey => 8,
            Tool::Harmony => 9,
        }
    }
}

impl Tool {
    /// Tools shown as a drop-down (their label ends with a chevron).
    pub fn is_dropdown(self) -> bool {
        matches!(self, Tool::DrawLength | Tool::QuantizeGrid | Tool::Length | Tool::Velocity | Tool::Harmony)
    }
}

/// Width of the "⋯" button.
pub const MORE_W: f32 = 28.0;
/// Height of the toolbar's buttons.
const TOOL_H: f32 = 24.0;
/// Width of the − and + buttons of the transpose group.
const STEP_W: f32 = 26.0;

/// The tools that fit `avail` points, in toolbar order: with `gap` between them and the "⋯" button always at the
/// end, the least important tools are dropped until the rest fits. The dropped ones go into the "⋯" menu.
pub fn fit(widths: &[(Tool, f32)], avail: f32, gap: f32, more_w: f32) -> Vec<Tool> {
    let mut shown = widths.to_vec();
    let total = |v: &[(Tool, f32)]| v.iter().map(|(_, w)| w + gap).sum::<f32>() + more_w;
    while total(&shown) > avail {
        let Some(worst) = shown.iter().enumerate().max_by_key(|(_, (t, _))| t.priority()).map(|(i, _)| i) else {
            break;
        };
        shown.remove(worst);
    }
    shown.into_iter().map(|(t, _)| t).collect()
}

/// The quantize grid in beats: the one chosen in the toolbar, or the snap value (at most a beat; 1/16 with snap
/// off) when it follows the snap.
pub fn quantize_grid(choice: Option<f64>, snap: f64) -> f64 {
    choice.unwrap_or(if snap > 0.0 { snap.min(1.0) } else { 0.25 })
}

/// The grids the quantize menu offers besides following the snap.
const GRIDS: [f64; 4] = [1.0, 0.5, 0.25, 0.125];

/// How far the transpose buttons have moved the current selection, shown between − and +. It starts again from 0
/// when the selection or the region changes.
#[derive(Debug, Default, Clone, PartialEq)]
pub struct TransposeRun {
    target: Option<(Id, Vec<usize>)>,
    total: i32,
}

impl TransposeRun {
    /// Semitones moved so far for these notes (`indices` empty: the whole region).
    pub fn shown(&self, region: Id, indices: &[usize]) -> i32 {
        match &self.target {
            Some((r, ix)) if *r == region && ix == indices => self.total,
            _ => 0,
        }
    }

    /// Count a move of `semitones` for these notes.
    pub fn add(&mut self, region: Id, indices: &[usize], semitones: i32) {
        self.total = self.shown(region, indices) + semitones;
        self.target = Some((region, indices.to_vec()));
    }
}

/// "+2 st", "0 st", "−12 st".
pub fn transpose_label(semitones: i32) -> String {
    match semitones {
        0 => "0 st".into(),
        s if s > 0 => format!("+{s} st"),
        s => format!("\u{2212}{} st", -s),
    }
}

impl OndinaApp {
    /// The selected notes as the commands take them (None: every note) and as a sorted list.
    fn pr_selection(&self) -> (Option<Vec<usize>>, Vec<usize>) {
        let mut v: Vec<usize> = self.selected_notes.iter().copied().collect();
        v.sort_unstable();
        ((!v.is_empty()).then(|| v.clone()), v)
    }

    /// Lays the toolbar out in what is left of the editor header, right-aligned.
    pub(crate) fn piano_roll_toolbar(&mut self, ui: &mut egui::Ui, region: &Region, m: &MidiContent) {
        ui.spacing_mut().interact_size.y = TOOL_H;
        let gap = theme::SP_3;
        let widths: Vec<(Tool, f32)> = TOOLS.iter().map(|t| (*t, self.tool_width(ui, *t, region.id))).collect();
        // a few points of slack, so a measuring error never pushes a tool over the region name
        let shown = fit(&widths, ui.available_width() - theme::SP_4, gap, MORE_W);
        let hidden: Vec<Tool> = TOOLS.iter().copied().filter(|t| !shown.contains(t)).collect();
        ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
            ui.spacing_mut().item_spacing.x = gap;
            self.more_menu(ui, region, m, &hidden);
            for t in shown.iter().rev() {
                self.tool_ui(ui, *t, region.id);
            }
        });
    }

    fn tool_text(&self, t: Tool, rid: Id) -> String {
        match t {
            Tool::DrawLength => format!("Note {}", len_label(self.note_len)),
            Tool::QuantizeGrid => len_label(quantize_grid(self.pr_quantize, self.snap)).to_string(),
            Tool::Quantize => "Q".into(),
            Tool::Humanize => "Humanize".into(),
            Tool::Transpose => transpose_label(self.pr_transpose_run.shown(rid, &self.pr_selection().1)),
            Tool::Length => "Length".into(),
            Tool::Velocity => "Velocity".into(),
            Tool::SnapToKey => "Snap to key".into(),
            Tool::Harmony => "Harmony".into(),
        }
    }

    /// The width a tool takes in the toolbar.
    fn tool_width(&self, ui: &egui::Ui, t: Tool, rid: Id) -> f32 {
        let text_w = |s: &str| ui.painter().layout_no_wrap(s.to_owned(), theme::body_font(), theme::TEXT).size().x;
        let pad = 2.0 * ui.spacing().button_padding.x;
        let text = self.tool_text(t, rid);
        let chevron = if t.is_dropdown() { widgets::CHEVRON_W + text_w(" ") } else { 0.0 };
        match t {
            Tool::QuantizeGrid => text_w("Quantize") + ui.spacing().item_spacing.x + text_w(&text) + chevron + pad,
            Tool::Transpose => 2.0 * STEP_W + self.transpose_mid_w(ui, &text),
            _ => (text_w(&text) + chevron).max(TOOL_H - pad) + pad,
        }
    }

    fn transpose_mid_w(&self, ui: &egui::Ui, text: &str) -> f32 {
        let w = ui.painter().layout_no_wrap(text.to_owned(), theme::number_font(), theme::TEXT).size().x;
        w.max(30.0) + 12.0
    }

    /// One tool in the toolbar (called right to left).
    fn tool_ui(&mut self, ui: &mut egui::Ui, t: Tool, rid: Id) {
        let (sel, sorted) = self.pr_selection();
        let text = self.tool_text(t, rid);
        match t {
            Tool::DrawLength => {
                widgets::dropdown_menu_button(ui, text, |ui| self.draw_length_items(ui))
                    .response
                    .on_hover_text("Note length: how long the notes you draw are (and Length, Fixed)");
            }
            Tool::QuantizeGrid => {
                widgets::dropdown_menu_button(ui, text, |ui| self.grid_items(ui))
                    .response
                    .on_hover_text("Quantize grid used by Q (Follow snap: the snap value)");
                ui.label(RichText::new("Quantize").color(theme::TEXT_3));
            }
            Tool::Quantize => {
                let grid = len_label(quantize_grid(self.pr_quantize, self.snap));
                let tip = format!("Quantize the selected notes to {grid} (all notes when nothing is selected)");
                if ui.button(text).on_hover_text(tip).clicked() {
                    self.pr_quantize_notes(rid, sel);
                }
            }
            Tool::Humanize => {
                if ui.button(text).on_hover_text("Humanize the selected notes (all notes when nothing is selected)").clicked() {
                    self.pr_humanize(rid, sel);
                }
            }
            Tool::Transpose => self.transpose_group(ui, rid, sel, &sorted, &text),
            Tool::Length => {
                widgets::dropdown_menu_button(ui, text, |ui| self.length_items(ui, rid, sel))
                    .response
                    .on_hover_text("Change the length of the selected notes (all notes when nothing is selected). Starts and pitches stay.");
            }
            Tool::Velocity => {
                widgets::dropdown_menu_button(ui, text, |ui| self.velocity_items(ui, rid, sel))
                    .response
                    .on_hover_text("Set or scale the velocity of the selected notes (all notes when nothing is selected)");
            }
            Tool::SnapToKey => {
                if ui
                    .button(text)
                    .on_hover_text("Move the selected notes to the nearest pitch in the project key (all notes when nothing is selected)")
                    .clicked()
                {
                    self.exec_on_selection(Command::SnapNotesToKey { region_id: rid, indices: sel });
                }
            }
            Tool::Harmony => {
                widgets::dropdown_menu_button(ui, text, |ui| self.harmony_items(ui, rid, sel))
                    .response
                    .on_hover_text("Add a voice in the project key to the selected notes (all notes when nothing is selected). The new notes become the selection.");
            }
        }
    }

    /// − [moved so far] + as one bordered group: a semitone per click, an octave with Shift.
    fn transpose_group(&mut self, ui: &mut egui::Ui, rid: Id, sel: Option<Vec<usize>>, sorted: &[usize], text: &str) {
        let mid_w = self.transpose_mid_w(ui, text);
        let (rect, _) = ui.allocate_exact_size(vec2(2.0 * STEP_W + mid_w, TOOL_H - 2.0), Sense::hover());
        let minus = Rect::from_min_size(rect.min, vec2(STEP_W, rect.height()));
        let plus = Rect::from_min_max(pos2(rect.right() - STEP_W, rect.top()), rect.max);
        let mid = Rect::from_min_max(pos2(minus.right(), rect.top()), pos2(plus.left(), rect.bottom()));
        let r = Rounding::same(theme::R_MD);
        let painter = ui.painter();
        painter.rect_filled(rect, r, theme::RAISED);
        painter.rect_filled(mid, Rounding::ZERO, theme::CHROME);
        let mut moved = 0;
        for (part, glyph, dir, tip) in [
            (minus, "\u{2212}", -1, "Transpose the selected notes down a semitone (Shift: an octave; all notes when nothing is selected)"),
            (plus, "+", 1, "Transpose the selected notes up a semitone (Shift: an octave; all notes when nothing is selected)"),
        ] {
            let resp = ui.interact(part, ui.id().with(("pr-transpose", dir)), Sense::click()).on_hover_text(tip);
            if resp.hovered() {
                let corners = if dir < 0 { Rounding { nw: theme::R_MD, sw: theme::R_MD, ..Rounding::ZERO } } else { Rounding { ne: theme::R_MD, se: theme::R_MD, ..Rounding::ZERO } };
                ui.painter().rect_filled(part, corners, theme::HOVER);
            }
            ui.painter().text(part.center(), Align2::CENTER_CENTER, glyph, theme::body_font(), theme::ICON);
            if resp.clicked() {
                let octave = ui.input(|i| i.modifiers.shift);
                moved = dir * if octave { 12 } else { 1 };
            }
        }
        ui.painter().text(mid.center(), Align2::CENTER_CENTER, text, theme::number_font(), theme::TEXT_2);
        ui.painter().rect_stroke(rect, r, Stroke::new(1.0_f32, theme::BORDER));
        ui.interact(mid, ui.id().with("pr-transpose-mid"), Sense::hover())
            .on_hover_text("Semitones the − and + buttons have moved the selected notes (starts again from 0 with a new selection)");
        if moved != 0 {
            self.pr_transpose(rid, sel, sorted, moved);
        }
    }

    /// The "⋯" menu: the tools that did not fit, then the ones that only live here.
    fn more_menu(&mut self, ui: &mut egui::Ui, region: &Region, m: &MidiContent, hidden: &[Tool]) {
        let rid = region.id;
        let menu = widgets::more_menu_button(ui, vec2(MORE_W, TOOL_H), None, |ui| {
            ui.set_min_width(200.0);
            let (sel, sorted) = self.pr_selection();
            let mut close = false;
            for t in hidden {
                close |= self.tool_menu_entry(ui, *t, rid, sel.clone(), &sorted);
            }
            if !hidden.is_empty() {
                ui.separator();
            }
            if ui.button("Transpose up an octave").clicked() {
                self.pr_transpose(rid, sel.clone(), &sorted, 12);
                close = true;
            }
            if ui.button("Transpose down an octave").clicked() {
                self.pr_transpose(rid, sel.clone(), &sorted, -12);
                close = true;
            }
            if ui.button("Select all").on_hover_text("Select every note (Ctrl+A)").clicked() {
                self.selected_notes = nc::select_all(m.notes.len());
                close = true;
            }
            ui.horizontal(|ui| {
                ui.label("Region transpose");
                let mut tr = m.transpose;
                if ui
                    .add(egui::DragValue::new(&mut tr).range(-48..=48).suffix(" st"))
                    .on_hover_text("Play the whole region shifted, without changing its notes")
                    .changed()
                {
                    self.cmd_merge(Command::SetRegionProperties { region_id: rid, name: None, muted: None, loop_count: None, transpose: Some(tr), velocity_offset: None, gain_db: None, fade_in: None, fade_out: None, reverse: None, color: None }, format!("tr{rid}"));
                }
            });
            ui.separator();
            if ui.button("MIDI Transform…").clicked() {
                self.extra.show_transform = true;
                close = true;
            }
            if self.ai_button(ui, "AI: continue / improve") {
                self.ask_ai(ondina_studio::prompts::CONTINUE_IMPROVE.into());
                close = true;
            }
            ui.separator();
            ui.label(RichText::new("Mouse and keyboard help").color(theme::TEXT_3)).on_hover_text(PIANO_ROLL_HELP);
            if close {
                ui.close_menu();
            }
        });
        menu.response.on_hover_text("More piano roll tools");
    }

    /// A tool that did not fit the toolbar, as an entry of the "⋯" menu. Returns whether the menu should close.
    fn tool_menu_entry(&mut self, ui: &mut egui::Ui, t: Tool, rid: Id, sel: Option<Vec<usize>>, sorted: &[usize]) -> bool {
        let mut close = false;
        match t {
            Tool::DrawLength => {
                ui.menu_button(format!("Note length: {}", len_label(self.note_len)), |ui| close |= self.draw_length_items(ui));
            }
            Tool::QuantizeGrid => {
                let grid = len_label(quantize_grid(self.pr_quantize, self.snap));
                ui.menu_button(format!("Quantize grid: {grid}"), |ui| close |= self.grid_items(ui));
            }
            Tool::Quantize => {
                if ui.button("Quantize").clicked() {
                    self.pr_quantize_notes(rid, sel);
                    close = true;
                }
            }
            Tool::Humanize => {
                if ui.button("Humanize").clicked() {
                    self.pr_humanize(rid, sel);
                    close = true;
                }
            }
            Tool::Transpose => {
                if ui.button("Transpose up a semitone").clicked() {
                    self.pr_transpose(rid, sel.clone(), sorted, 1);
                    close = true;
                }
                if ui.button("Transpose down a semitone").clicked() {
                    self.pr_transpose(rid, sel, sorted, -1);
                    close = true;
                }
            }
            Tool::Length => {
                ui.menu_button("Length", |ui| close |= self.length_items(ui, rid, sel));
            }
            Tool::Velocity => {
                ui.menu_button("Velocity", |ui| close |= self.velocity_items(ui, rid, sel));
            }
            Tool::SnapToKey => {
                if ui.button("Snap to key").clicked() {
                    self.exec_on_selection(Command::SnapNotesToKey { region_id: rid, indices: sel });
                    close = true;
                }
            }
            Tool::Harmony => {
                ui.menu_button("Harmony", |ui| close |= self.harmony_items(ui, rid, sel));
            }
        }
        close
    }

    fn pr_quantize_notes(&mut self, rid: Id, sel: Option<Vec<usize>>) {
        let grid = quantize_grid(self.pr_quantize, self.snap);
        self.exec_on_selection(Command::QuantizeNotes { region_id: rid, grid, strength: Some(1.0), swing: None, quantize_ends: Some(false), indices: sel });
    }

    fn pr_humanize(&mut self, rid: Id, sel: Option<Vec<usize>>) {
        self.exec_on_selection(Command::HumanizeNotes { region_id: rid, timing: Some(0.015), velocity: Some(10), seed: None, indices: sel });
    }

    fn pr_transpose(&mut self, rid: Id, sel: Option<Vec<usize>>, sorted: &[usize], semitones: i32) {
        if self.exec(Command::TransposeNotes { region_id: rid, semitones, indices: sel }).is_some() {
            self.pr_transpose_run.add(rid, sorted, semitones);
        }
    }

    // Menu contents shared by the toolbar and the "⋯" menu; each returns whether an entry was chosen (the caller
    // closes the menu), and closes egui's own menus itself.

    fn draw_length_items(&mut self, ui: &mut egui::Ui) -> bool {
        for l in [4.0, 2.0, 1.0, 0.5, 0.25, 0.125] {
            if ui.selectable_value(&mut self.note_len, l, len_label(l)).clicked() {
                ui.close_menu();
                return true;
            }
        }
        false
    }

    fn grid_items(&mut self, ui: &mut egui::Ui) -> bool {
        let follow = format!("Follow snap ({})", len_label(quantize_grid(None, self.snap)));
        let mut chosen = ui.selectable_value(&mut self.pr_quantize, None, follow).clicked();
        for g in GRIDS {
            chosen |= ui.selectable_value(&mut self.pr_quantize, Some(g), len_label(g)).clicked();
        }
        if chosen {
            ui.close_menu();
        }
        chosen
    }

    fn length_items(&mut self, ui: &mut egui::Ui, rid: Id, sel: Option<Vec<usize>>) -> bool {
        for (label, choice, hint) in note_menus::LENGTH_MENU {
            let text = note_menus::length_label(label, choice, len_label(self.note_len));
            if ui.button(text).on_hover_text(hint).clicked() {
                let c = note_menus::length_command(rid, choice, self.note_len, sel.clone());
                if let Some(s) = self.exec_on_selection(c).as_ref().and_then(note_menus::length_status) {
                    self.status = s;
                }
                ui.close_menu();
                return true;
            }
        }
        false
    }

    fn velocity_items(&mut self, ui: &mut egui::Ui, rid: Id, sel: Option<Vec<usize>>) -> bool {
        for (label, choice) in note_menus::VELOCITY_MENU {
            if ui.button(label).clicked() {
                self.exec_on_selection(note_menus::velocity_command(rid, choice, sel.clone()));
                ui.close_menu();
                return true;
            }
        }
        ui.separator();
        let mut chosen = false;
        ui.horizontal(|ui| {
            ui.add(egui::DragValue::new(&mut self.pr_velocity).range(1..=127));
            if ui.button("Set").clicked() {
                let c = note_menus::velocity_command(rid, VelocityChoice::Set(self.pr_velocity), sel.clone());
                self.exec_on_selection(c);
                ui.close_menu();
                chosen = true;
            }
        });
        chosen
    }

    fn harmony_items(&mut self, ui: &mut egui::Ui, rid: Id, sel: Option<Vec<usize>>) -> bool {
        for (label, interval, below) in HARMONY_MENU {
            if ui.button(label).clicked() {
                self.add_harmony(Command::AddHarmony { region_id: rid, interval, below: Some(below), indices: sel.clone() });
                ui.close_menu();
                return true;
            }
        }
        false
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn widths(w: f32) -> Vec<(Tool, f32)> {
        TOOLS.iter().map(|t| (*t, w)).collect()
    }

    #[test]
    fn a_wide_toolbar_shows_every_tool() {
        assert_eq!(fit(&widths(60.0), 2000.0, 6.0, MORE_W), TOOLS);
    }

    #[test]
    fn a_narrow_toolbar_drops_the_least_used_tools_first() {
        // room for four 60-point tools and the "⋯" button
        let avail = 4.0 * 66.0 + MORE_W;
        let shown = fit(&widths(60.0), avail, 6.0, MORE_W);
        assert_eq!(shown, [Tool::QuantizeGrid, Tool::Quantize, Tool::Transpose, Tool::Length]);
        // what is shown keeps the toolbar order and always fits
        let total: f32 = shown.len() as f32 * 66.0 + MORE_W;
        assert!(total <= avail);
        // shrinking never brings a dropped tool back
        let mut prev = TOOLS.to_vec();
        for avail in (0..1000).rev().step_by(25) {
            let now = fit(&widths(60.0), avail as f32, 6.0, MORE_W);
            assert!(now.iter().all(|t| prev.contains(t)), "avail {avail}");
            prev = now;
        }
    }

    #[test]
    fn no_room_leaves_only_the_more_button() {
        assert!(fit(&widths(60.0), 10.0, 6.0, MORE_W).is_empty());
        assert_eq!(fit(&widths(60.0), 66.0 + MORE_W, 6.0, MORE_W), [Tool::Quantize]);
    }

    #[test]
    fn priorities_are_all_different() {
        let mut p: Vec<u8> = TOOLS.iter().map(|t| t.priority()).collect();
        p.sort_unstable();
        p.dedup();
        assert_eq!(p.len(), TOOLS.len());
    }

    #[test]
    fn quantize_grid_follows_the_snap_unless_chosen() {
        assert_eq!(quantize_grid(None, 0.5), 0.5);
        assert_eq!(quantize_grid(None, 4.0), 1.0, "a bar snap quantizes to beats");
        assert_eq!(quantize_grid(None, 0.0), 0.25, "snap off: sixteenths");
        assert_eq!(quantize_grid(Some(0.125), 0.5), 0.125);
    }

    #[test]
    fn the_transpose_count_belongs_to_one_selection() {
        let mut run = TransposeRun::default();
        assert_eq!(run.shown(1, &[0, 2]), 0);
        run.add(1, &[0, 2], 1);
        run.add(1, &[0, 2], 1);
        assert_eq!(run.shown(1, &[0, 2]), 2);
        run.add(1, &[0, 2], -12);
        assert_eq!(run.shown(1, &[0, 2]), -10);
        // another selection or region starts from zero
        assert_eq!(run.shown(1, &[0]), 0);
        assert_eq!(run.shown(2, &[0, 2]), 0);
        run.add(1, &[0], 1);
        assert_eq!(run.shown(1, &[0]), 1);
        assert_eq!(run.shown(1, &[0, 2]), 0);
        assert_eq!(transpose_label(0), "0 st");
        assert_eq!(transpose_label(2), "+2 st");
        assert_eq!(transpose_label(-12), "\u{2212}12 st");
    }
}
