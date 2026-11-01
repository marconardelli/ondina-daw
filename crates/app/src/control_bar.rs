//! The control bar under the menus: side-panel toggles, transport, the LCD (position, time, tempo, time signature,
//! key), snap, the choice of the bottom panel, the assistant toggle and the DSP meter.
//!
//! Every LCD field has a fixed width and its numbers have a fixed number of digits, so nothing in the bar moves
//! while the song plays. The formatting and the state changes are plain functions covered by the tests below.

use eframe::egui::{self, pos2, vec2, Align2, Color32, Key, Rect, Rounding, Sense, Stroke, Ui};
use ondina_core::command::Command;
use ondina_core::model::*;
use ondina_core::tempo::TempoMap;
use ondina_core::theory::ScaleKind;

use crate::app::{BottomTab, LeftTab, OndinaApp};
use crate::key_menu;
use crate::metronome_ui;
use crate::theme;
use crate::widgets::{self, Icon, ToolButton, ToolColors};

pub const OVERLOAD_HINT: &str = "Audio blocks since the last Play that took longer than the audio device allows. Each one can be heard as a click. If the count grows, raise the buffer size in the audio device's settings or remove heavy plug-ins.";

/// Height of the control bar.
pub const BAR_HEIGHT: f32 = 52.0;

// ===================================================================================== pure logic

/// The bottom panel choices, in the order of the segmented control.
pub const BOTTOM_CHOICES: [(BottomTab, &str); 4] = [
    (BottomTab::Editor, "Editor"),
    (BottomTab::Mixer, "Mixer"),
    (BottomTab::Live, "Clip Grid"),
    (BottomTab::Hidden, "Hide"),
];

impl BottomTab {
    /// Position in [`BOTTOM_CHOICES`].
    pub fn index(self) -> usize {
        BOTTOM_CHOICES.iter().position(|(t, _)| *t == self).unwrap_or(0)
    }

    pub fn from_index(i: usize) -> BottomTab {
        BOTTOM_CHOICES.get(i).map_or(BottomTab::Hidden, |(t, _)| *t)
    }
}

/// The bottom panel after picking `choice` in the segmented control: one panel at a time, whatever was open before.
pub fn pick_bottom(_current: BottomTab, choice: usize) -> BottomTab {
    BottomTab::from_index(choice)
}

/// The side panel shows the Inspector or the Library. Its button lights when the panel shows that page.
pub fn side_lit(show: bool, page: LeftTab, button: LeftTab) -> bool {
    show && page == button
}

/// Clicking the Inspector or Library button: shows that page, or closes the panel when it already shows it.
pub fn side_click(show: bool, page: LeftTab, button: LeftTab) -> (bool, LeftTab) {
    if side_lit(show, page, button) {
        (false, page)
    } else {
        (true, button)
    }
}

/// Ticks in a quarter note.
const TICKS_PER_QUARTER: i64 = 960;

/// Bar, beat, division (a quarter of the beat) and tick (1/960 of a quarter note) at `beat`, all counted from 1
/// except the tick. Negative positions read as the start.
pub fn bar_beat_div_tick(beat: f64, ts: TimeSignature) -> (i64, i64, i64, i64) {
    let den = ts.denominator.max(1) as i64;
    let num = ts.numerator.max(1) as i64;
    let beat_ticks = (TICKS_PER_QUARTER * 4 / den).max(4);
    let div_ticks = beat_ticks / 4;
    let bar_ticks = beat_ticks * num;
    let total = if beat.is_finite() { (beat.max(0.0) * TICKS_PER_QUARTER as f64 + 1e-6).floor() as i64 } else { 0 };
    let bar = total / bar_ticks;
    let in_bar = total - bar * bar_ticks;
    let b = (in_bar / beat_ticks).min(num - 1);
    let in_beat = in_bar - b * beat_ticks;
    let d = (in_beat / div_ticks).min(3);
    let t = in_beat - d * div_ticks;
    (bar + 1, b + 1, d + 1, t)
}

/// The position field: `bar beat div tick`, the tick always three digits. Drawn right-aligned, so the beat,
/// division and tick never move and a longer bar number only grows to the left.
pub fn position_text(beat: f64, ts: TimeSignature) -> String {
    let (bar, b, d, t) = bar_beat_div_tick(beat, ts);
    format!("{bar} {b} {d} {t:03}")
}

/// The time field: minutes, seconds and milliseconds, `mm:ss.mmm`.
pub fn time_text(secs: f64) -> String {
    let ms = if secs.is_finite() { (secs.max(0.0) * 1000.0).round() as u64 } else { 0 };
    format!("{:02}:{:02}.{:03}", ms / 60_000, ms / 1000 % 60, ms % 1000)
}

/// The tempo field: always two decimals.
pub fn bpm_text(bpm: f64) -> String {
    format!("{bpm:.2}")
}

/// The tempo in force at `beat`: the tempo map's value there, not only the song's first tempo.
pub fn tempo_at(map: &TempoMap, beat: f64) -> f64 {
    map.bpm_at(beat.max(0.0))
}

/// Editing the tempo field changes the tempo in force at the playhead: the song's first tempo, or the tempo change
/// the playhead is in (replaced at its own beat).
pub fn tempo_command(map: &TempoMap, beat: f64, bpm: f64) -> Command {
    let beat = beat.max(0.0);
    let idx = map.points.partition_point(|p| p.beat <= beat);
    match idx.checked_sub(1).and_then(|i| map.points.get(i)) {
        Some(p) if p.beat > 0.0 => Command::AddTempoChange { beat: p.beat, bpm },
        _ => Command::SetTempo { bpm },
    }
}

pub const BPM_MIN: f64 = 20.0;
pub const BPM_MAX: f64 = 300.0;

/// A tempo typed in the field ("128", "92.5", "92,5"), clamped to the accepted range; `None` when it is not a number.
pub fn parse_bpm(s: &str) -> Option<f64> {
    let v: f64 = s.trim().trim_end_matches("bpm").trim().replace(',', ".").parse().ok()?;
    v.is_finite().then(|| v.clamp(BPM_MIN, BPM_MAX))
}

/// Dragging the tempo field: right or up raises it, 0.1 BPM per point (0.01 with Shift), two decimals at most.
pub fn drag_bpm(bpm: f64, dx: f32, dy: f32, fine: bool) -> f64 {
    let step = if fine { 0.01 } else { 0.1 };
    let v = bpm + (dx - dy) as f64 * step;
    ((v * 100.0).round() / 100.0).clamp(BPM_MIN, BPM_MAX)
}

/// The DSP meter's color: green, amber from 60 %, red from 85 %.
pub fn load_color(load: f32) -> Color32 {
    if load >= 0.85 {
        theme::REC
    } else if load >= 0.6 {
        theme::SOLO
    } else {
        theme::PLAY
    }
}

// ===================================================================================== LCD painting

const LCD_HEIGHT: f32 = 40.0;
const FIELD_PAD: f32 = 10.0;
const FIELD_LINE: Color32 = Color32::from_rgb(0x1E, 0x22, 0x26);
const POS_W: f32 = 146.0;
const TIME_W: f32 = 100.0;
const SMPTE_W: f32 = 118.0;
const BPM_W: f32 = 76.0;
const SIG_W: f32 = 64.0;
const KEY_W: f32 = 120.0;

fn value_font(size: f32) -> egui::FontId {
    egui::FontId::monospace(size)
}

/// Where a field's value sits: right-aligned numbers or left-aligned words.
#[derive(Clone, Copy)]
enum Align {
    Right,
    Left,
}

/// One LCD field: allocates its fixed rect, draws the caps label at the bottom and the divider on its right.
fn lcd_field(ui: &mut Ui, width: f32, label: &str, sense: Sense, last: bool) -> egui::Response {
    let (rect, resp) = ui.allocate_exact_size(vec2(width, LCD_HEIGHT), sense);
    if ui.is_rect_visible(rect) {
        let p = ui.painter();
        if resp.hovered() && sense != Sense::hover() {
            p.rect_filled(rect.shrink(2.0), Rounding::same(theme::R_SM), Color32::from_rgb(0x14, 0x17, 0x1A));
        }
        p.text(
            pos2(rect.left() + FIELD_PAD, rect.bottom() - 5.0),
            Align2::LEFT_BOTTOM,
            label,
            theme::label_font(),
            theme::TEXT_3,
        );
        if !last {
            p.vline(rect.right(), rect.y_range().shrink(6.0), Stroke::new(1.0_f32, FIELD_LINE));
        }
    }
    resp
}

/// The value of a field, clipped to it so an unusually long one never spills into the next.
fn lcd_value(ui: &Ui, rect: Rect, text: &str, font: egui::FontId, color: Color32, align: Align) {
    let p = ui.painter().with_clip_rect(rect.shrink2(vec2(FIELD_PAD - 2.0, 0.0)));
    let y = rect.top() + 4.0;
    match align {
        Align::Right => p.text(pos2(rect.right() - FIELD_PAD, y), Align2::RIGHT_TOP, text, font, color),
        Align::Left => p.text(pos2(rect.left() + FIELD_PAD, y), Align2::LEFT_TOP, text, font, color),
    };
}

fn popup_id(ui: &Ui, name: &str) -> egui::Id {
    ui.id().with(("lcd-popup", name))
}

// ===================================================================================== the bar

impl OndinaApp {
    pub(crate) fn control_bar(&mut self, ui: &mut Ui) {
        let p = self.project();
        ui.set_height(BAR_HEIGHT - 2.0 * theme::SP_2);
        ui.horizontal_centered(|ui| {
            ui.spacing_mut().item_spacing.x = theme::SP_2;
            self.side_toggles(ui);
            bar_divider(ui);
            self.transport_buttons(ui, &p);
            ui.add_space(theme::SP_4);
            self.lcd(ui, &p);
            ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                ui.spacing_mut().item_spacing.x = theme::SP_4;
                self.dsp_meter(ui);
                let assistant = ToolButton::new(Icon::Ai)
                    .text("Assistant")
                    .lit(self.show_assistant.then_some(ToolColors::AI))
                    .icon_color(if self.show_assistant { theme::AI_TEXT } else { theme::AI });
                if ui.add(assistant).on_hover_text("Show or hide the AI assistant").clicked() {
                    self.show_assistant = !self.show_assistant;
                }
                let mut choice = self.bottom.index();
                let labels: Vec<&str> = BOTTOM_CHOICES.iter().map(|(_, l)| *l).collect();
                if widgets::segmented(ui, &mut choice, &labels)
                    .on_hover_text("Bottom panel: Editor, Mixer, Clip Grid, or hidden")
                    .changed()
                {
                    self.bottom = pick_bottom(self.bottom, choice);
                }
                bar_divider(ui);
                egui::ComboBox::from_id_salt("snap")
                    .width(64.0)
                    .selected_text(crate::app::snap_label(self.snap))
                    .show_ui(ui, |ui| {
                        for s in [0.0, 4.0, 1.0, 0.5, 0.25, 0.125] {
                            ui.selectable_value(&mut self.snap, s, crate::app::snap_label(s));
                        }
                    });
                ui.label(egui::RichText::new("Snap").color(theme::TEXT_2));
                if self.musical_typing {
                    ui.add_space(theme::SP_4);
                    ui.label(
                        egui::RichText::new(format!("Typing C{}", self.typing_octave)).color(theme::ACCENT),
                    );
                    widgets::icon(ui, Icon::Piano, 14.0, theme::ACCENT);
                }
            });
        });
    }

    fn side_toggles(&mut self, ui: &mut Ui) {
        for (page, icon, tip) in [
            (LeftTab::Library, Icon::Library, "Library: show or hide the sounds and patches browser"),
            (LeftTab::Inspector, Icon::Inspector, "Inspector: show or hide the settings of the selected track"),
        ] {
            let lit = side_lit(self.show_left, self.left, page);
            let b = ToolButton::new(icon).size(32.0, 30.0).icon_size(16.0).lit(lit.then_some(ToolColors::ACCENT));
            if ui.add(b).on_hover_text(tip).clicked() {
                (self.show_left, self.left) = side_click(self.show_left, self.left, page);
            }
        }
    }

    fn transport_buttons(&mut self, ui: &mut Ui, p: &Project) {
        ui.spacing_mut().item_spacing.x = 3.0;
        let bpb = p.time_signature.beats_per_bar();
        if ui.add(ToolButton::new(Icon::ToStart).filled()).on_hover_text("Go to start (Home)").clicked() {
            self.exec(Command::SetPlayhead { beat: Some(0.0), bar: None });
        }
        if ui.add(ToolButton::new(Icon::Rewind).filled()).on_hover_text("Back one bar").clicked() {
            let b = (self.position() - bpb).max(0.0);
            self.exec(Command::SetPlayhead { beat: Some(b), bar: None });
        }
        if ui.add(ToolButton::new(Icon::Forward).filled()).on_hover_text("Forward one bar").clicked() {
            let b = self.position() + bpb;
            self.exec(Command::SetPlayhead { beat: Some(b), bar: None });
        }
        let playing = self.playing();
        let rec = self.recording();
        if ui.add(ToolButton::new(Icon::Stop).filled().icon_size(12.0)).on_hover_text("Stop (Space)").clicked()
            && (playing || rec)
        {
            self.exec(Command::Stop {});
        }
        let play = ToolButton::new(Icon::Play)
            .filled()
            .size(42.0, 30.0)
            .lit(playing.then_some(ToolColors::PLAY))
            .icon_color(theme::PLAY);
        if ui.add(play).on_hover_text("Play / Stop (Space)").clicked() {
            self.toggle_play();
        }
        let record = ToolButton::new(Icon::Record)
            .filled()
            .icon_size(13.0)
            .lit(rec.then_some(ToolColors::REC))
            .icon_color(theme::REC);
        if ui.add(record).on_hover_text("Record (R)").clicked() {
            if rec {
                self.exec(Command::Stop {});
            } else {
                self.exec(Command::Record { from_beat: None });
            }
        }
        ui.add_space(theme::SP_3);
        let cyc = p.cycle.enabled;
        let cycle = ToolButton::new(Icon::Cycle).icon_size(16.0).lit(cyc.then_some(ToolColors::CYCLE));
        if ui.add(cycle).on_hover_text("Cycle (C)").clicked() {
            self.cmd(Command::SetCycle { enabled: !cyc, start: None, end: None });
        }
        let look = metronome_ui::look(p, rec);
        let metro = ToolButton::new(Icon::Metronome).lit(match look {
            metronome_ui::Look::On => Some(ToolColors::ACCENT),
            metronome_ui::Look::Waiting => {
                Some(ToolColors { fill: theme::RAISED, border: theme::ACCENT_BORDER, fg: theme::ACCENT })
            }
            metronome_ui::Look::Off => None,
        });
        let resp = ui.add(metro).on_hover_text(metronome_ui::tooltip(p));
        if resp.clicked() {
            self.cmd(metronome_ui::toggle(p));
        }
        crate::drag_cancel::menu_unless_dragging(&resp, self.drag.is_some(), |ui| {
            ui.label("Click level");
            let mut db = p.metronome_level_db;
            if ui
                .add(egui::Slider::new(&mut db, METRONOME_MIN_DB..=METRONOME_MAX_DB).suffix(" dB").fixed_decimals(1))
                .changed()
            {
                self.cmd_merge(metronome_ui::set_level(p, db), "metronome-level");
            }
            if ui.button("Reset level").clicked() {
                self.cmd(metronome_ui::set_level(p, METRONOME_DEFAULT_DB));
            }
            let mut only = p.metronome_only_recording;
            if ui.checkbox(&mut only, "Only while recording").changed() {
                self.cmd(metronome_ui::set_only_recording(p, only));
            }
        });
    }

    fn lcd(&mut self, ui: &mut Ui, p: &Project) {
        let pos = self.position();
        let movie = p.movie.is_some();
        let width = POS_W + TIME_W + if movie { SMPTE_W } else { 0.0 } + BPM_W + SIG_W + KEY_W;
        let (rect, _) = ui.allocate_exact_size(vec2(width, LCD_HEIGHT), Sense::hover());
        ui.painter().rect(rect, Rounding::same(theme::R_XL), theme::LCD, Stroke::new(1.0_f32, theme::BORDER));
        let layout = egui::Layout::left_to_right(egui::Align::Center);
        ui.allocate_new_ui(egui::UiBuilder::new().max_rect(rect).layout(layout), |ui| {
            ui.spacing_mut().item_spacing.x = 0.0;
            // position
            let r = lcd_field(ui, POS_W, "BAR \u{b7} BEAT \u{b7} DIV \u{b7} TICK", Sense::hover(), false);
            let r = r.on_hover_text("Bar, beat, division (a quarter of the beat) and tick");
            lcd_value(ui, r.rect, &position_text(pos, p.time_signature), theme::lcd_font(), theme::ACCENT_TEXT, Align::Right);
            // time
            let r = lcd_field(ui, TIME_W, "TIME", Sense::hover(), false);
            let secs = p.tempo.beat_to_sec(pos);
            lcd_value(ui, r.rect, &time_text(secs), value_font(14.0), theme::ICON, Align::Right);
            if movie {
                let r = lcd_field(ui, SMPTE_W, "SMPTE", Sense::hover(), false);
                lcd_value(ui, r.rect, &p.timecode(pos), value_font(14.0), theme::ICON, Align::Right);
            }
            self.tempo_field(ui, p, pos);
            self.signature_field(ui, p);
            self.key_field(ui, p);
        });
    }

    fn tempo_field(&mut self, ui: &mut Ui, p: &Project, pos: f64) {
        let bpm = tempo_at(&p.tempo, pos);
        let changes = p.tempo.points.len() > 1;
        let r = lcd_field(ui, BPM_W, "BPM", Sense::click_and_drag(), false);
        let edit_id = ui.id().with("lcd-bpm-edit");
        let editing: Option<String> = ui.data(|d| d.get_temp(edit_id));
        if let Some(mut buf) = editing {
            let inner = r.rect.shrink2(vec2(4.0, 3.0));
            let inner = Rect::from_min_size(inner.min, vec2(inner.width(), 20.0));
            // drawn in a child that the bar's layout does not see, so the next fields stay where they are
            ui.painter().rect(
                inner.expand(1.0),
                Rounding::same(theme::R_SM),
                theme::INSET,
                Stroke::new(1.0_f32, theme::ACCENT_BORDER),
            );
            let mut child = ui.new_child(egui::UiBuilder::new().max_rect(inner));
            let te = child.add(
                egui::TextEdit::singleline(&mut buf)
                    .font(value_font(14.0))
                    .horizontal_align(egui::Align::Max)
                    .frame(false)
                    .desired_width(inner.width() - 4.0),
            );
            if !te.has_focus() && !te.lost_focus() {
                te.request_focus();
            }
            if te.lost_focus() {
                ui.data_mut(|d| d.remove::<String>(edit_id));
                if !ui.input(|i| i.key_pressed(Key::Escape)) {
                    if let Some(v) = parse_bpm(&buf) {
                        self.cmd(tempo_command(&p.tempo, pos, v));
                    }
                }
            } else {
                ui.data_mut(|d| d.insert_temp(edit_id, buf));
            }
            return;
        }
        lcd_value(ui, r.rect, &bpm_text(bpm), value_font(14.0), theme::TEXT, Align::Right);
        if r.dragged() {
            let d = r.drag_delta();
            let fine = ui.input(|i| i.modifiers.shift);
            let v = drag_bpm(bpm, d.x, d.y, fine);
            if v != bpm {
                self.cmd_merge(tempo_command(&p.tempo, pos, v), "tempo");
            }
        } else if r.clicked() {
            ui.data_mut(|d| d.insert_temp(edit_id, bpm_text(bpm)));
        }
        let tip = if changes {
            "Tempo at the playhead (the song has tempo changes). Drag to change it, click to type"
        } else {
            "Tempo. Drag to change it (Shift for fine steps), click to type"
        };
        r.on_hover_text(tip);
    }

    fn signature_field(&mut self, ui: &mut Ui, p: &Project) {
        let ts = p.time_signature;
        let r = lcd_field(ui, SIG_W, "TIME SIG", Sense::click(), false);
        lcd_value(ui, r.rect, &format!("{}/{}", ts.numerator, ts.denominator), value_font(14.0), theme::TEXT, Align::Left);
        let id = popup_id(ui, "timesig");
        if r.clicked() {
            ui.memory_mut(|m| m.toggle_popup(id));
        }
        egui::popup_below_widget(ui, id, &r, egui::PopupCloseBehavior::CloseOnClick, |ui| {
            ui.set_min_width(80.0);
            for (n, d) in [(4u8, 4u8), (3, 4), (2, 4), (5, 4), (6, 8), (7, 8), (12, 8)] {
                if ui.selectable_label(ts.numerator == n && ts.denominator == d, format!("{n}/{d}")).clicked() {
                    self.cmd(Command::SetTimeSignature { numerator: n, denominator: d });
                }
            }
        });
        r.on_hover_text("Time signature: click to change");
    }

    fn key_field(&mut self, ui: &mut Ui, p: &Project) {
        let r = lcd_field(ui, KEY_W, "KEY", Sense::click(), true);
        let name = p.key.name();
        lcd_value(ui, r.rect, &name, egui::FontId::proportional(13.0), theme::TEXT, Align::Left);
        let id = popup_id(ui, "key");
        if r.clicked() {
            ui.memory_mut(|m| m.toggle_popup(id));
        }
        egui::popup_below_widget(ui, id, &r, egui::PopupCloseBehavior::CloseOnClick, |ui| {
            ui.set_min_width(150.0);
            if ui.selectable_label(false, key_menu::DETECT_LABEL).on_hover_text(key_menu::DETECT_TOOLTIP).clicked() {
                if let Some(v) = self.exec(key_menu::detect_command()) {
                    self.status = key_menu::status(&v);
                }
            }
            ui.separator();
            egui::ScrollArea::vertical().max_height(360.0).show(ui, |ui| {
                for root in 0..12u8 {
                    for scale in [ScaleKind::Major, ScaleKind::Minor, ScaleKind::Dorian, ScaleKind::Mixolydian] {
                        let k = ondina_core::theory::Key { root, scale };
                        if ui.selectable_label(p.key == k, k.name()).clicked() {
                            self.cmd(Command::SetKey { key: k.name() });
                        }
                    }
                }
            });
        });
        r.on_hover_text(format!("Key: {name}. Click to change or detect it"));
    }

    /// DSP load bar and the overload count of this playback.
    fn dsp_meter(&mut self, ui: &mut Ui) {
        let load = self.shared.load().clamp(0.0, 1.0);
        let overloads = self.shared.overloads();
        let (rect, resp) = ui.allocate_exact_size(vec2(58.0, 26.0), Sense::hover());
        if ui.is_rect_visible(rect) {
            let p = ui.painter();
            p.text(rect.left_top(), Align2::LEFT_TOP, "DSP", theme::label_font(), theme::TEXT_3);
            p.text(
                rect.right_top(),
                Align2::RIGHT_TOP,
                format!("{:.0}%", load * 100.0),
                value_font(10.0),
                if load >= 0.85 { theme::REC } else { theme::TEXT_3 },
            );
            let track = Rect::from_min_size(pos2(rect.left(), rect.top() + 14.0), vec2(rect.width(), 4.0));
            p.rect_filled(track, Rounding::same(2.0), theme::CONTROL);
            let fill = Rect::from_min_size(track.min, vec2(track.width() * load, 4.0));
            p.rect_filled(fill, Rounding::same(2.0), load_color(load));
            // one dot per overload, up to six; red when any
            let dots = overloads.min(6) as usize;
            for i in 0..6 {
                let c = pos2(rect.left() + 3.0 + i as f32 * 9.0, rect.bottom() - 2.5);
                let col = if i < dots { theme::REC } else { theme::CONTROL };
                p.circle_filled(c, 2.0, col);
            }
        }
        resp.on_hover_text(format!(
            "DSP load {:.0}% \u{b7} {overloads} overload{}\n{OVERLOAD_HINT}",
            load * 100.0,
            if overloads == 1 { "" } else { "s" }
        ));
    }
}

fn bar_divider(ui: &mut Ui) {
    let (rect, _) = ui.allocate_exact_size(vec2(theme::SP_5 * 2.0 + 1.0, 28.0), Sense::hover());
    ui.painter().vline(rect.center().x, rect.y_range(), Stroke::new(1.0_f32, theme::BORDER));
}

#[cfg(test)]
mod tests {
    use super::*;
    use ondina_core::tempo::TempoMap;

    fn ts(n: u8, d: u8) -> TimeSignature {
        TimeSignature { numerator: n, denominator: d }
    }

    #[test]
    fn position_counts_bar_beat_division_and_tick() {
        let four = ts(4, 4);
        assert_eq!(bar_beat_div_tick(0.0, four), (1, 1, 1, 0));
        // bar 5, beat 2, third sixteenth, half of it
        let b = 16.0 + 1.0 + 0.5 + 0.125;
        assert_eq!(bar_beat_div_tick(b, four), (5, 2, 3, 120));
        assert_eq!(position_text(b, four), "5 2 3 120");
        // in 6/8 the beat is an eighth note: beat 4 of bar 1 starts at quarter 1.5
        assert_eq!(bar_beat_div_tick(1.5, ts(6, 8)), (1, 4, 1, 0));
        assert_eq!(bar_beat_div_tick(3.0, ts(6, 8)), (2, 1, 1, 0));
        // negative and broken positions read as the start
        assert_eq!(position_text(-3.0, four), "1 1 1 000");
        assert_eq!(position_text(f64::NAN, four), "1 1 1 000");
    }

    #[test]
    fn position_digits_after_the_bar_never_change_width() {
        let four = ts(4, 4);
        // right-aligned: everything after the bar number keeps its length while the song plays
        let tail = |s: String| s.split_once(' ').map(|(_, t)| t.len()).unwrap();
        let mut b = 0.0;
        while b < 64.0 {
            assert_eq!(tail(position_text(b, four)), 7, "{b}");
            b += 0.01;
        }
        assert_eq!(position_text(15.999_999_9, four), "4 4 4 239");
    }

    #[test]
    fn time_and_tempo_fields_keep_their_width() {
        assert_eq!(time_text(0.0), "00:00.000");
        assert_eq!(time_text(9.84), "00:09.840");
        assert_eq!(time_text(59.9996), "01:00.000");
        assert_eq!(time_text(-1.0), "00:00.000");
        for s in [0.0, 1.234, 59.9, 61.0, 3599.0] {
            assert_eq!(time_text(s).len(), 9, "{s}");
        }
        assert_eq!(bpm_text(100.0), "100.00");
        assert_eq!(bpm_text(92.5), "92.50");
    }

    #[test]
    fn tempo_field_follows_the_tempo_map_at_the_playhead() {
        let mut map = TempoMap::new(100.0);
        map.add_change(16.0, 140.0);
        assert_eq!(tempo_at(&map, 0.0), 100.0);
        assert_eq!(tempo_at(&map, 15.99), 100.0);
        assert_eq!(tempo_at(&map, 16.0), 140.0);
        assert_eq!(tempo_at(&map, 40.0), 140.0);
        assert_eq!(tempo_at(&map, -2.0), 100.0);
    }

    #[test]
    fn editing_the_tempo_changes_the_tempo_in_force_at_the_playhead() {
        let mut map = TempoMap::new(100.0);
        assert!(matches!(tempo_command(&map, 8.0, 90.0), Command::SetTempo { bpm } if bpm == 90.0));
        map.add_change(16.0, 140.0);
        assert!(matches!(tempo_command(&map, 4.0, 90.0), Command::SetTempo { bpm } if bpm == 90.0));
        assert!(matches!(
            tempo_command(&map, 20.0, 150.0),
            Command::AddTempoChange { beat, bpm } if beat == 16.0 && bpm == 150.0
        ));
    }

    #[test]
    fn typed_and_dragged_tempos_stay_in_range() {
        assert_eq!(parse_bpm(" 128 "), Some(128.0));
        assert_eq!(parse_bpm("92,5"), Some(92.5));
        assert_eq!(parse_bpm("120 bpm"), Some(120.0));
        assert_eq!(parse_bpm("999"), Some(BPM_MAX));
        assert_eq!(parse_bpm("abc"), None);
        assert_eq!(parse_bpm("NaN"), None);
        assert_eq!(drag_bpm(100.0, 10.0, 0.0, false), 101.0);
        assert_eq!(drag_bpm(100.0, 0.0, 10.0, false), 99.0);
        assert_eq!(drag_bpm(100.0, 10.0, 0.0, true), 100.1);
        assert_eq!(drag_bpm(25.0, 0.0, 1000.0, false), BPM_MIN);
    }

    #[test]
    fn bottom_panel_is_one_choice_and_clip_grid_hands_over_to_the_editor() {
        // the old toggles left the Editor dead while the grid was open and closed the panel on the way back
        assert_eq!(pick_bottom(BottomTab::Live, BottomTab::Editor.index()), BottomTab::Editor);
        assert_eq!(pick_bottom(BottomTab::Live, BottomTab::Mixer.index()), BottomTab::Mixer);
        assert_eq!(pick_bottom(BottomTab::Editor, BottomTab::Live.index()), BottomTab::Live);
        assert_eq!(pick_bottom(BottomTab::Live, BottomTab::Live.index()), BottomTab::Live);
        assert_eq!(pick_bottom(BottomTab::Hidden, BottomTab::Live.index()), BottomTab::Live);
        assert_eq!(pick_bottom(BottomTab::Mixer, BottomTab::Hidden.index()), BottomTab::Hidden);
        for (i, (t, _)) in BOTTOM_CHOICES.iter().enumerate() {
            assert_eq!(t.index(), i);
            assert_eq!(BottomTab::from_index(i), *t);
        }
        assert_eq!(BOTTOM_CHOICES[BottomTab::Live.index()].1, "Clip Grid");
        assert_eq!(BottomTab::from_index(99), BottomTab::Hidden);
    }

    #[test]
    fn side_buttons_switch_page_or_close_the_panel() {
        use LeftTab::*;
        assert_eq!(side_click(true, Inspector, Library), (true, Library));
        assert_eq!(side_click(true, Inspector, Inspector), (false, Inspector));
        assert_eq!(side_click(false, Inspector, Library), (true, Library));
        assert_eq!(side_click(false, Library, Library), (true, Library));
        assert!(side_lit(true, Library, Library));
        assert!(!side_lit(false, Library, Library));
        assert!(!side_lit(true, Inspector, Library));
    }

    #[test]
    fn dsp_meter_turns_amber_then_red() {
        assert_eq!(load_color(0.1), theme::PLAY);
        assert_eq!(load_color(0.7), theme::SOLO);
        assert_eq!(load_color(0.9), theme::REC);
    }
}
