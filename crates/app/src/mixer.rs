//! Mixer: a header bar (channel filter, loudness of the last analysis, Analyze mix), one channel strip per track,
//! all of the same fixed size, and the Master pinned on the right.

use eframe::egui::{self, pos2, vec2, Align2, Color32, Rect, RichText, Rounding, Sense, Stroke};
use ondina_core::command::Command;
use ondina_core::model::*;
use ondina_core::plugins::{self, PluginCategory};
use serde_json::Value;

use crate::app::{LeftTab, OndinaApp};
use crate::chrome::{self, SlotLook};
use crate::inspector_strip::{clear_clip, db_text};
use crate::theme;
use crate::widgets::{self, PeakHold, StateKind};

/// Width of a track's channel strip.
pub const STRIP_W: f32 = 116.0;
/// Width of the Master column.
pub const MASTER_W: f32 = 128.0;
/// Height of the mixer's header bar.
pub const HEADER_H: f32 = 36.0;
/// Room for a section's caps label above its slots.
const LABEL_H: f32 = 14.0;
/// Height of a slot (effect, send, input, output).
const SLOT_H: f32 = widgets::SLOT_HEIGHT;
/// Space between two slots.
const ROW_GAP: f32 = 4.0;
/// Space between two sections of a strip.
const GAP: f32 = 6.0;
/// Effect slots a track strip shows before its effects scroll.
pub const FX_ROWS: usize = 3;
/// Effect slots the Master shows before its effects scroll.
pub const MASTER_FX_ROWS: usize = 4;
/// Send slots a strip shows before its sends scroll.
pub const SEND_ROWS: usize = 2;
/// The fader never gets shorter than this; a shorter panel scrolls.
pub const MIN_FADER_H: f32 = 90.0;
/// Pan knob diameter.
const KNOB_D: f32 = 34.0;
/// Rows of the Master's loudness box.
const LOUDNESS_ROWS: usize = 3;
/// Width of the volume and peak boxes.
const VALUE_BOX_W: f32 = 44.0;
/// The quietest peak the peak box shows as a number; under it, "−∞".
pub const PEAK_FLOOR_DB: f32 = -60.0;
/// Characters of a track name that fit the strip's name band; longer names get a tooltip.
const NAME_CHARS: usize = 15;
/// Data keys of the mixer meters' peak holds (apart from the Inspector's).
const HOLDS_KEY: &str = "mixer-holds";

/// The send / bus slot look of the design: teal fill and border.
const BUS_LOOK: SlotLook = SlotLook {
    fill: theme::BUS_BG,
    border: theme::BUS_BORDER,
    text: theme::TEXT,
};

/// Scroll bars of the effect and send lists inside a strip: always shown as a thin line over the slots (taking no
/// room from them), widening on hover.
fn inner_scroll() -> egui::style::ScrollStyle {
    egui::style::ScrollStyle { dormant_background_opacity: 0.5, dormant_handle_opacity: 1.0, active_background_opacity: 0.6, ..egui::style::ScrollStyle::floating() }
}

/// Height of the track strips in an area `h` tall and `w` wide: the whole height, less the horizontal scroll bar
/// (`bar` tall) when `n` strips are wider than the area, so the bar never hides the strips' bottom.
pub fn strips_height(h: f32, w: f32, n: usize, bar: f32) -> f32 {
    let needed = n as f32 * STRIP_W + n.saturating_sub(1) as f32 * 2.0;
    if needed > w {
        h - bar
    } else {
        h
    }
}

/// Height of `n` slots stacked with their gaps.
pub fn rows_h(n: usize) -> f32 {
    n as f32 * SLOT_H + n.saturating_sub(1) as f32 * ROW_GAP
}

/// The sections of a strip, top to bottom.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Part {
    /// Colored name band.
    Head,
    /// Instrument, audio input or bus input.
    Input,
    Effects,
    Sends,
    Output,
    /// The Master's loudness box.
    Loudness,
    Pan,
    /// Fader and meter: the only part that grows with the panel.
    Fader,
    /// Volume and peak boxes.
    Values,
    /// Mute / Solo / Record.
    Buttons,
    /// Colored band with number and name.
    Foot,
}

impl Part {
    /// Fixed height of the part, None for the fader that takes the height the others leave.
    pub fn height(self, master: bool) -> Option<f32> {
        Some(match self {
            Part::Head => 22.0,
            Part::Input => LABEL_H + SLOT_H,
            Part::Effects => LABEL_H + rows_h(if master { MASTER_FX_ROWS } else { FX_ROWS }),
            Part::Sends => LABEL_H + rows_h(SEND_ROWS),
            Part::Output => SLOT_H,
            Part::Loudness => LABEL_H + loudness_box_h(),
            Part::Pan => KNOB_D + theme::SIZE_NUMBER + 4.0,
            Part::Fader => return None,
            Part::Values => 18.0,
            Part::Buttons => widgets::STATE_BUTTON.y,
            Part::Foot => 24.0,
        })
    }
}

fn loudness_box_h() -> f32 {
    LOUDNESS_ROWS as f32 * 14.0 + 2.0 * theme::SP_3
}

/// A track strip's parts.
pub const CHANNEL_PARTS: [Part; 10] = [
    Part::Head,
    Part::Input,
    Part::Effects,
    Part::Sends,
    Part::Output,
    Part::Pan,
    Part::Fader,
    Part::Values,
    Part::Buttons,
    Part::Foot,
];
/// The Master's parts.
pub const MASTER_PARTS: [Part; 8] =
    [Part::Head, Part::Effects, Part::Loudness, Part::Pan, Part::Fader, Part::Values, Part::Buttons, Part::Foot];

/// Space above part `i`: none above the head and the foot (the foot sits on the bottom edge).
fn gap_before(parts: &[Part], i: usize) -> f32 {
    if i == 0 || parts[i] == Part::Foot {
        0.0
    } else {
        GAP
    }
}

/// The smallest height that fits every part with the fader at [`MIN_FADER_H`].
pub fn strip_min_height(parts: &[Part], master: bool) -> f32 {
    (0..parts.len())
        .map(|i| gap_before(parts, i) + parts[i].height(master).unwrap_or(MIN_FADER_H))
        .sum()
}

/// Top and bottom of every part of a strip starting at `top`, `height` tall (at least [`strip_min_height`]): the
/// fixed parts keep their height whatever the strip holds, the fader takes the rest.
pub fn strip_rows(parts: &[Part], master: bool, top: f32, height: f32) -> Vec<(Part, f32, f32)> {
    let min = strip_min_height(parts, master);
    let fader = MIN_FADER_H + (height - min).max(0.0);
    let mut y = top;
    parts
        .iter()
        .enumerate()
        .map(|(i, part)| {
            y += gap_before(parts, i);
            let h = part.height(master).unwrap_or(fader);
            let row = (*part, y, y + h);
            y += h;
            row
        })
        .collect()
}

/// Whether `n` effects plus the "+ Add" slot overflow the effect area (it scrolls then).
pub fn effects_scroll(n: usize, master: bool) -> bool {
    n + 1 > if master { MASTER_FX_ROWS } else { FX_ROWS }
}

/// Empty dashed placeholders under `used` slots so the area always shows `rows` of them.
pub fn placeholders(used: usize, rows: usize) -> usize {
    rows.saturating_sub(used)
}

/// The channel filter of the header.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum MixerFilter {
    All,
    Tracks,
    Buses,
}

impl MixerFilter {
    pub const LABELS: [&'static str; 3] = ["All", "Tracks", "Buses"];

    pub fn from_index(i: usize) -> Self {
        match i {
            1 => MixerFilter::Tracks,
            2 => MixerFilter::Buses,
            _ => MixerFilter::All,
        }
    }

    /// Whether a track of `kind` has a strip under this filter (the Master always has one).
    pub fn shows(self, kind: TrackKind) -> bool {
        match self {
            MixerFilter::All => true,
            MixerFilter::Tracks => kind != TrackKind::Bus,
            MixerFilter::Buses => kind == TrackKind::Bus,
        }
    }

    /// What the strip area says when the filter leaves no strip.
    pub fn empty_text(self) -> &'static str {
        match self {
            MixerFilter::All => "No tracks yet",
            MixerFilter::Tracks => "No instrument or audio tracks",
            MixerFilter::Buses => "No buses",
        }
    }
}

/// The peak box: "−∞" under [`PEAK_FLOOR_DB`], else the dB with one decimal.
pub fn peak_text(db: f32) -> String {
    if !db.is_finite() || db < PEAK_FLOOR_DB {
        "\u{2212}\u{221E}".into()
    } else {
        db_text(db)
    }
}

/// Color of the peak box: red after a clip, yellow with signal, dim in silence.
pub fn peak_color(db: f32, clipped: bool) -> Color32 {
    if clipped {
        theme::REC
    } else if db.is_finite() && db >= PEAK_FLOOR_DB {
        theme::SOLO
    } else {
        theme::TEXT_DISABLED
    }
}

/// The input slot of an audio track: its stereo input pair, "In 1·2".
pub fn audio_input_label(input_channel: u16) -> String {
    let first = u32::from(input_channel) + 1;
    format!("In {first}\u{00B7}{}", first + 1)
}

/// The input slot of a bus: how many tracks send or route to it.
pub fn bus_input_label(sources: usize) -> String {
    match sources {
        0 => "No sources".into(),
        1 => "1 source".into(),
        n => format!("{n} sources"),
    }
}

/// Tracks that send to bus `id` or route their output to it.
pub fn bus_sources(p: &Project, id: Id) -> usize {
    p.tracks
        .iter()
        .filter(|t| t.id != id && (t.channel.output == Output::Bus(id) || t.channel.sends.iter().any(|s| s.bus_id == id)))
        .count()
}

/// A number with `decimals` decimals, a real minus sign and its unit: "−17.8 LUFS". Values at the analysis' floor
/// (−100 and below: silence) read "−∞".
pub fn format_number(n: f64, decimals: usize, unit: &str) -> String {
    let body = if !n.is_finite() || n <= -99.95 {
        "\u{2212}\u{221E}".to_string()
    } else {
        let s = format!("{:.*}", decimals, n.abs());
        let zero = s.chars().all(|c| c == '0' || c == '.');
        if n < 0.0 && !zero {
            format!("\u{2212}{s}")
        } else {
            s
        }
    };
    if unit.is_empty() {
        body
    } else {
        format!("{body} {unit}")
    }
}

/// [`format_number`] for a value of the analysis report; "—" when it is missing.
pub fn report_number(v: &Value, decimals: usize, unit: &str) -> String {
    v.as_f64().map_or_else(|| "\u{2014}".to_string(), |n| format_number(n, decimals, unit))
}

/// The low / mid / high energy shares: "34 / 45 / 21 %".
pub fn balance_text(v: &Value) -> String {
    let part = |i: usize| v[i].as_f64().map_or_else(|| "\u{2014}".to_string(), |n| format!("{n:.0}"));
    format!("{} / {} / {} %", part(0), part(1), part(2))
}

/// Rows of the master section of the Mix Analysis window: label, report key, decimals, unit.
pub const ANALYSIS_ROWS: [(&str, &str, usize, &str); 7] = [
    ("Integrated loudness", "integrated_lufs", 1, "LUFS"),
    ("Short-term max", "short_term_max_lufs", 1, "LUFS"),
    ("Loudness range", "loudness_range_lu", 1, "LU"),
    ("True peak", "true_peak_dbtp", 1, "dBTP"),
    ("RMS", "rms_dbfs", 1, "dBFS"),
    ("Crest factor", "crest_factor_db", 1, "dB"),
    ("Stereo correlation", "stereo_correlation", 2, ""),
];

/// The master loudness of the last mix analysis, for the header and the Master strip.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct MixLoudness {
    pub integrated_lufs: f64,
    pub short_term_max_lufs: f64,
    pub true_peak_dbtp: f64,
}

impl MixLoudness {
    /// Reads the master figures of an `analyze_mix` result; None when the report has no master.
    pub fn from_report(rep: &Value) -> Option<Self> {
        let m = &rep["master"];
        Some(Self {
            integrated_lufs: m["integrated_lufs"].as_f64()?,
            short_term_max_lufs: m["short_term_max_lufs"].as_f64()?,
            true_peak_dbtp: m["true_peak_dbtp"].as_f64()?,
        })
    }
}

/// Paints a colored name band: optional number in monospace, then the name cut to the width.
fn paint_band(ui: &egui::Ui, rect: Rect, rounding: Rounding, fill: Color32, ink: Color32, num: Option<usize>, name: &str) {
    let p = ui.painter();
    p.rect_filled(rect, rounding, fill);
    let mut job = egui::text::LayoutJob::default();
    if let Some(n) = num {
        job.append(
            &format!("{n} "),
            0.0,
            egui::TextFormat::simple(theme::number_font(), ink.gamma_multiply(0.7)),
        );
    }
    job.append(name, 0.0, egui::TextFormat::simple(egui::FontId::new(theme::SIZE_NUMBER, theme::semibold()), ink));
    job.wrap = egui::text::TextWrapping::truncate_at_width(rect.width() - 2.0 * theme::SP_2);
    let galley = ui.fonts(|f| f.layout_job(job));
    p.galley(rect.center() - galley.size() / 2.0, galley, ink);
}

/// A caps label at the top of `rect`.
fn paint_label(ui: &egui::Ui, rect: Rect, text: &str) {
    ui.painter().text(
        pos2(rect.left(), rect.top() + LABEL_H / 2.0 - 1.0),
        Align2::LEFT_CENTER,
        text,
        theme::label_font(),
        theme::TEXT_3,
    );
}

/// An LCD box with `text` in `color`.
fn paint_lcd(ui: &egui::Ui, rect: Rect, text: &str, color: Color32) {
    ui.painter().rect_filled(rect, Rounding::same(theme::R_SM), theme::LCD);
    ui.painter().text(rect.center(), Align2::CENTER_CENTER, text, theme::number_font(), color);
}

/// A static slot (audio or bus input): text centered in a slot look.
fn static_slot(ui: &mut egui::Ui, look: SlotLook, text: &str) -> egui::Response {
    let (rect, resp) = ui.allocate_exact_size(vec2(ui.available_width(), SLOT_H), Sense::hover());
    ui.painter().rect(rect, Rounding::same(theme::R_SM), look.fill, Stroke::new(1.0_f32, look.border));
    ui.painter().text(rect.center(), Align2::CENTER_CENTER, text, egui::FontId::proportional(theme::SIZE_NUMBER), look.text);
    resp
}

/// Dashed empty slots that keep a section's height when it holds fewer items than its rows.
fn paint_placeholders(ui: &mut egui::Ui, n: usize) {
    for _ in 0..n {
        let (rect, _) = ui.allocate_exact_size(vec2(ui.available_width(), SLOT_H), Sense::hover());
        chrome::paint_dashed(ui, rect, false);
    }
}

impl OndinaApp {
    pub fn mixer_ui(&mut self, ui: &mut egui::Ui) {
        let p = self.project();
        let filter_id = egui::Id::new("mixer-filter");
        let mut fi: usize = ui.data(|d| d.get_temp(filter_id)).unwrap_or(0);
        self.mixer_header(ui, &mut fi);
        ui.data_mut(|d| d.insert_temp(filter_id, fi));
        let filter = MixerFilter::from_index(fi);

        let body = ui.available_rect_before_wrap();
        ui.allocate_rect(body, Sense::hover());
        let master_rect = Rect::from_min_max(pos2(body.right() - MASTER_W, body.top()), body.max);
        let tracks_rect = Rect::from_min_max(body.min, pos2(master_rect.left(), body.bottom()));
        ui.painter().rect_filled(tracks_rect, Rounding::ZERO, theme::INSET);
        ui.painter().rect_filled(master_rect, Rounding::ZERO, theme::LANE_BG);
        ui.painter().vline(master_rect.left() + 0.5, master_rect.y_range(), Stroke::new(1.0_f32, theme::BORDER));

        // tracks: same-size strips that scroll sideways (and down when the panel is too short)
        let inner = Rect::from_min_max(tracks_rect.min + vec2(theme::SP_4, theme::SP_4), tracks_rect.max - vec2(theme::SP_4, 0.0));
        let mut tui = ui.new_child(
            egui::UiBuilder::new()
                .id_salt("mixer-tracks")
                .max_rect(inner)
                .layout(egui::Layout::left_to_right(egui::Align::Min)),
        );
        // scroll bars always visible (solid, they take their own room) instead of appearing only on hover
        tui.spacing_mut().scroll = egui::style::ScrollStyle::solid();
        let bar = tui.spacing().scroll.allocated_width();
        let shown_n = p.tracks.iter().filter(|t| filter.shows(t.kind)).count();
        let strip_h = strips_height(inner.height(), inner.width(), shown_n, bar).max(strip_min_height(&CHANNEL_PARTS, false));
        let shown: Vec<(usize, &Track)> = p.tracks.iter().enumerate().filter(|(_, t)| filter.shows(t.kind)).collect();
        if shown.is_empty() {
            tui.label(RichText::new(filter.empty_text()).color(theme::TEXT_3));
        } else {
            egui::ScrollArea::both().id_salt("mixer-strips").auto_shrink([false, false]).show(&mut tui, |ui| {
                ui.horizontal_top(|ui| {
                    ui.spacing_mut().item_spacing.x = 2.0;
                    for (i, t) in shown {
                        self.channel_strip(ui, &p, Some((i + 1, t)), t.id, &t.channel, strip_h);
                    }
                });
            });
        }

        // the Master, pinned on the right
        let mut mui = ui.new_child(
            egui::UiBuilder::new()
                .id_salt("mixer-master")
                .max_rect(master_rect.shrink2(vec2(1.0, 0.0)).translate(vec2(0.5, 0.0)))
                .layout(egui::Layout::top_down(egui::Align::Min)),
        );
        mui.spacing_mut().scroll = egui::style::ScrollStyle::solid();
        let master_h = master_rect.height().max(strip_min_height(&MASTER_PARTS, true) + theme::SP_4);
        egui::ScrollArea::vertical().id_salt("mixer-master-scroll").auto_shrink([false, false]).show(&mut mui, |ui| {
            self.channel_strip(ui, &p, None, MASTER_ID, &p.master, master_h);
        });
    }

    /// Title, channel filter, the loudness of the last analysis and the Analyze mix button.
    fn mixer_header(&mut self, ui: &mut egui::Ui, filter: &mut usize) {
        let (rect, _) = ui.allocate_exact_size(vec2(ui.available_width(), HEADER_H), Sense::hover());
        ui.painter().rect_filled(rect, Rounding::ZERO, theme::PANEL_BG);
        ui.painter().hline(rect.x_range(), rect.bottom() - 0.5, Stroke::new(1.0_f32, theme::SEPARATOR));
        let mut child = ui.new_child(
            egui::UiBuilder::new()
                .id_salt("mixer-header")
                .max_rect(rect.shrink2(vec2(14.0, 0.0)))
                .layout(egui::Layout::left_to_right(egui::Align::Center)),
        );
        let ui = &mut child;
        ui.spacing_mut().item_spacing.x = 10.0;
        ui.label(RichText::new("Mixer").font(egui::FontId::new(13.0, theme::semibold())).color(theme::TEXT));
        ui.add_space(theme::SP_2);
        widgets::segmented(ui, filter, &MixerFilter::LABELS).on_hover_text("Which channel strips to show");
        ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
            ui.spacing_mut().item_spacing.x = theme::SP_4;
            let analyze = egui::Button::new(RichText::new("Analyze mix").color(theme::TEXT))
                .fill(theme::RAISED)
                .stroke(Stroke::new(1.0_f32, theme::BORDER))
                .rounding(Rounding::same(theme::R_LG))
                .min_size(vec2(0.0, 26.0));
            if ui.add(analyze).on_hover_text("Render the song offline and measure loudness, peaks and balance").clicked() {
                self.analyze_mix();
            }
            let (lufs, tp, tip) = match self.mix_loudness {
                Some(l) => (
                    format_number(l.integrated_lufs, 1, "LUFS"),
                    format_number(l.true_peak_dbtp, 1, "dBTP"),
                    "Master, from the last Analyze mix",
                ),
                None => ("\u{2014} LUFS".into(), "\u{2014} dBTP".into(), "Click Analyze mix to measure the master"),
            };
            let ink = if self.mix_loudness.is_some() { theme::TEXT } else { theme::TEXT_DISABLED };
            ui.label(RichText::new(tp).monospace().color(ink)).on_hover_text(tip);
            ui.label(RichText::new("True peak").color(theme::TEXT_3));
            ui.label(RichText::new(lufs).monospace().color(ink)).on_hover_text(tip);
            ui.label(RichText::new("Loudness").color(theme::TEXT_3));
        });
    }

    /// Runs the mix analysis: the report opens the Mix Analysis window, its master loudness goes to the mixer.
    pub(crate) fn analyze_mix(&mut self) {
        if let Some(v) = self.exec(Command::AnalyzeMix { start: None, end: None }) {
            self.mix_loudness = MixLoudness::from_report(&v);
            self.mix_report = Some(v);
        }
    }

    /// A click on a plug-in of the strip selected it: show its parameters in the Inspector.
    fn reveal_plugin(&mut self, before: Option<Id>, id: Id) {
        if self.selected_plugin != before && self.selected_plugin.is_some() {
            if id != MASTER_ID {
                self.selected_track = Some(id);
            }
            self.left = LeftTab::Inspector;
            self.show_left = true;
        }
    }

    /// One strip, `height` tall: a track's (with its 1-based number) or the Master's when `track` is None.
    fn channel_strip(&mut self, ui: &mut egui::Ui, p: &Project, track: Option<(usize, &Track)>, id: Id, ch: &Channel, height: f32) {
        let master = track.is_none();
        let w = if master { MASTER_W - 1.0 } else { STRIP_W };
        let parts: &[Part] = if master { &MASTER_PARTS } else { &CHANNEL_PARTS };
        let (rect, bg) = ui.allocate_exact_size(vec2(w, height), Sense::click());
        if bg.clicked() && !master {
            self.select_track(id);
        }
        if !ui.is_rect_visible(rect) {
            return;
        }
        let selected = !master && self.selected_track == Some(id);
        let top_round = Rounding { nw: theme::R_LG, ne: theme::R_LG, sw: 0.0, se: 0.0 };
        if !master {
            let (fill, border) =
                if selected { (theme::SELECTED_LANE, theme::BORDER_HOVER) } else { (theme::HEADER_BG, theme::RAISED) };
            ui.painter().rect(rect, top_round, fill, Stroke::new(1.0_f32, border));
        }
        let color = track.map(|(_, t)| theme::rgb(t.color)).unwrap_or(theme::TEXT);
        let pad = if master { theme::SP_4 } else { theme::SP_3 };
        let top = rect.top() + if master { theme::SP_4 } else { 0.0 };
        let rows = strip_rows(parts, master, top, height - (top - rect.top()));
        for (part, y0, y1) in rows {
            let full = Rect::from_x_y_ranges(rect.x_range(), y0..=y1);
            let r = full.shrink2(vec2(pad, 0.0));
            let mut child = ui.new_child(
                egui::UiBuilder::new()
                    .id_salt(("mixer-part", id, part as u8))
                    .max_rect(r)
                    .layout(egui::Layout::top_down(egui::Align::Min)),
            );
            let ui = &mut child;
            ui.spacing_mut().item_spacing.y = ROW_GAP;
            match part {
                Part::Head => {
                    let name = track.map_or("Master", |(_, t)| t.name.as_str());
                    let (band, round) = if master {
                        (r, Rounding::same(theme::R_SM))
                    } else {
                        (full.shrink(1.0), Rounding { nw: theme::R_LG - 1.0, ne: theme::R_LG - 1.0, sw: 0.0, se: 0.0 })
                    };
                    paint_band(ui, band, round, color, theme::ON_TRACK, None, name);
                    if !master {
                        let mut resp = ui.interact(band, egui::Id::new(("mixer-head", id)), Sense::click());
                        if let Some(tip) = slot_tooltip(name, NAME_CHARS, None) {
                            resp = resp.on_hover_text(tip);
                        }
                        if resp.clicked() {
                            self.select_track(id);
                        }
                    }
                }
                Part::Input => {
                    paint_label(ui, r, "INPUT");
                    ui.add_space(LABEL_H);
                    if let Some((_, t)) = track {
                        match t.kind {
                            TrackKind::Instrument => {
                                let before = self.selected_plugin;
                                self.instrument_slot(ui, p, t, SLOT_H);
                                self.reveal_plugin(before, id);
                            }
                            TrackKind::Audio => {
                                static_slot(ui, SlotLook::INSTRUMENT, &audio_input_label(t.input_channel))
                                    .on_hover_text("Audio input pair used when recording");
                            }
                            TrackKind::Bus => {
                                static_slot(ui, BUS_LOOK, &bus_input_label(bus_sources(p, id)))
                                    .on_hover_text("Tracks sending or routed to this bus");
                            }
                        }
                    }
                }
                Part::Effects => {
                    let n = ch.inserts.len();
                    // when the effects overflow their rows the label counts them: the rest is a scroll away
                    let label =
                        if effects_scroll(n, master) { format!("EFFECTS \u{00B7} {n}") } else { "EFFECTS".into() };
                    paint_label(ui, r, &label);
                    ui.add_space(LABEL_H);
                    let rows = if master { MASTER_FX_ROWS } else { FX_ROWS };
                    ui.spacing_mut().scroll = inner_scroll();
                    egui::ScrollArea::vertical()
                        .id_salt(("mixer-fx", id))
                        .max_height(rows_h(rows))
                        .auto_shrink([false, true])
                        .show(ui, |ui| {
                            ui.spacing_mut().item_spacing.y = ROW_GAP;
                            let before = self.selected_plugin;
                            self.insert_slots(ui, id, &ch.inserts, SLOT_H);
                            self.reveal_plugin(before, id);
                            paint_placeholders(ui, placeholders(n + 1, rows));
                        });
                }
                Part::Sends => {
                    paint_label(ui, r, "SENDS");
                    ui.add_space(LABEL_H);
                    let buses = Self::buses_for(p, id);
                    ui.spacing_mut().scroll = inner_scroll();
                    egui::ScrollArea::vertical()
                        .id_salt(("mixer-sends", id))
                        .max_height(rows_h(SEND_ROWS))
                        .auto_shrink([false, true])
                        .show(ui, |ui| {
                            ui.spacing_mut().item_spacing.y = ROW_GAP;
                            for (si, s) in ch.sends.iter().enumerate() {
                                self.send_row(ui, p, id, si, s);
                            }
                            chrome::add_slot_menu_sized(ui, "+ Send", SLOT_H, |ui| self.add_send_items(ui, id, &buses))
                                .response
                                .on_hover_text("Send to a bus, or create a reverb or delay bus");
                            paint_placeholders(ui, placeholders(ch.sends.len() + 1, SEND_ROWS));
                        });
                }
                Part::Output => {
                    let buses = Self::buses_for(p, id);
                    let out = match ch.output {
                        Output::Master => "Master".to_string(),
                        Output::Bus(b) => p.track(b).map(|x| x.name.clone()).unwrap_or("?".into()),
                    };
                    chrome::slot_menu(ui, SlotLook::VALUE, SLOT_H, RichText::new(out).size(theme::SIZE_NUMBER), |ui| {
                        self.output_items(ui, id, &buses)
                    })
                    .response
                    .on_hover_text("Output: the Master or a bus");
                }
                Part::Loudness => {
                    paint_label(ui, r, "LOUDNESS");
                    let bx = Rect::from_min_max(pos2(r.left(), r.top() + LABEL_H), r.max);
                    ui.painter().rect_filled(bx, Rounding::same(theme::R_LG), theme::LCD);
                    let rows: [(&str, Option<f64>, Color32); LOUDNESS_ROWS] = match self.mix_loudness {
                        Some(l) => [
                            ("I", Some(l.integrated_lufs), theme::ACCENT_TEXT),
                            ("S max", Some(l.short_term_max_lufs), theme::TEXT),
                            ("TP", Some(l.true_peak_dbtp), theme::TEXT),
                        ],
                        None => [("I", None, theme::TEXT), ("S max", None, theme::TEXT), ("TP", None, theme::TEXT)],
                    };
                    let font = egui::FontId::monospace(theme::SIZE_LABEL);
                    for (k, (label, v, ink)) in rows.iter().enumerate() {
                        let y = bx.top() + theme::SP_3 + 7.0 + k as f32 * 14.0;
                        ui.painter().text(pos2(bx.left() + theme::SP_4, y), Align2::LEFT_CENTER, *label, font.clone(), theme::TEXT_3);
                        let text = v.map_or_else(|| "\u{2014}".to_string(), |n| format_number(n, 1, ""));
                        ui.painter().text(pos2(bx.right() - theme::SP_4, y), Align2::RIGHT_CENTER, text, font.clone(), *ink);
                    }
                    let tip = if self.mix_loudness.is_some() {
                        "Integrated, short-term max (LUFS) and true peak (dBTP) of the last Analyze mix"
                    } else {
                        "Click Analyze mix in the header to measure the master"
                    };
                    ui.interact(bx, egui::Id::new("mixer-loudness"), Sense::hover()).on_hover_text(tip);
                }
                Part::Pan => {
                    ui.vertical_centered(|ui| self.pan_knob(ui, id, ch.pan, KNOB_D));
                }
                Part::Fader => {
                    let meter_w = if master { 19.0 } else { 12.0 };
                    let fw = 26.0 + 22.0 + 4.0 + 4.0 + meter_w;
                    let fr = Rect::from_center_size(r.center(), vec2(fw, r.height()));
                    let mut fui = ui.new_child(
                        egui::UiBuilder::new()
                            .id_salt(("mixer-fader", id))
                            .max_rect(fr)
                            .layout(egui::Layout::left_to_right(egui::Align::Min)),
                    );
                    let holds = self.fader_and_meter(&mut fui, id, ch, r.height(), meter_w, HOLDS_KEY);
                    ui.data_mut(|d| d.insert_temp(egui::Id::new(("mixer-peak", id)), holds));
                }
                Part::Values => {
                    let holds: [PeakHold; 2] =
                        ui.data(|d| d.get_temp(egui::Id::new(("mixer-peak", id)))).unwrap_or_default();
                    let peak = holds[0].db.max(holds[1].db);
                    let clipped = holds.iter().any(|h| h.clipped);
                    let c = r.center();
                    let vol = Rect::from_min_size(pos2(c.x - VALUE_BOX_W - 2.0, r.top()), vec2(VALUE_BOX_W, r.height()));
                    let pk = Rect::from_min_size(pos2(c.x + 2.0, r.top()), vec2(VALUE_BOX_W, r.height()));
                    paint_lcd(ui, vol, &db_text(ch.volume_db), theme::TEXT);
                    ui.interact(vol, egui::Id::new(("mixer-vol", id)), Sense::hover()).on_hover_text("Volume, dB");
                    paint_lcd(ui, pk, &peak_text(peak), peak_color(peak, clipped));
                    let resp = ui
                        .interact(pk, egui::Id::new(("mixer-pk", id)), Sense::click())
                        .on_hover_text("Peak, dB · click to clear the clip lights");
                    if resp.clicked() {
                        clear_clip(ui.ctx(), HOLDS_KEY, id);
                    }
                }
                Part::Buttons => {
                    let kinds: &[StateKind] = match track {
                        None => &[StateKind::Mute],
                        Some((_, t)) if t.kind == TrackKind::Bus => &[StateKind::Mute, StateKind::Solo],
                        Some(_) => &[StateKind::Mute, StateKind::Solo, StateKind::Rec],
                    };
                    let gap = 3.0;
                    let bw = kinds.len() as f32 * widgets::STATE_BUTTON.x + (kinds.len() as f32 - 1.0) * gap;
                    let br = Rect::from_center_size(r.center(), vec2(bw, r.height()));
                    let mut bui = ui.new_child(
                        egui::UiBuilder::new()
                            .id_salt(("mixer-buttons", id))
                            .max_rect(br)
                            .layout(egui::Layout::left_to_right(egui::Align::Min)),
                    );
                    bui.spacing_mut().item_spacing.x = gap;
                    for kind in kinds {
                        match kind {
                            StateKind::Mute => {
                                let mut on = ch.mute;
                                if widgets::state_toggle(&mut bui, &mut on, *kind).on_hover_text("Mute").changed() {
                                    self.cmd(Command::SetMute { track_id: id, mute: on });
                                }
                            }
                            StateKind::Solo => {
                                let mut on = ch.solo;
                                if widgets::state_toggle(&mut bui, &mut on, *kind).on_hover_text("Solo").changed() {
                                    self.cmd(Command::SetSolo { track_id: id, solo: on });
                                }
                            }
                            StateKind::Rec => {
                                let mut on = track.is_some_and(|(_, t)| t.record_armed);
                                if widgets::state_toggle(&mut bui, &mut on, *kind).on_hover_text("Record arm").changed() {
                                    self.cmd(Command::SetRecordArm { track_id: id, armed: on });
                                }
                            }
                            StateKind::Monitor => {}
                        }
                    }
                }
                Part::Foot => {
                    let (num, name) = match track {
                        Some((n, t)) => (Some(n), t.name.as_str()),
                        None => (None, "Stereo Out"),
                    };
                    let band = if master { full } else { full.shrink2(vec2(1.0, 0.0)) };
                    paint_band(ui, band, Rounding::ZERO, color, theme::ON_TRACK, num, name);
                    if !master && ui.interact(band, egui::Id::new(("mixer-foot", id)), Sense::click()).clicked() {
                        self.select_track(id);
                    }
                }
            }
        }
    }

    /// The Mix Analysis window with the last `analyze_mix` report, numbers rounded to their unit.
    pub(crate) fn mix_analysis_ui(&mut self, ctx: &egui::Context) {
        let Some(rep) = self.mix_report.clone() else {
            return;
        };
        let mut open = true;
        crate::dialog::window("Mix Analysis").open(&mut open).default_width(520.0).show(ctx, |ui| {
            let m = &rep["master"];
            chrome::section_label(ui, "Master");
            egui::Grid::new("mixrep").striped(true).spacing(vec2(theme::SP_6, theme::SP_2)).show(ui, |ui| {
                for (label, key, decimals, unit) in ANALYSIS_ROWS {
                    ui.label(RichText::new(label).color(theme::TEXT_2));
                    ui.label(RichText::new(report_number(&m[key], decimals, unit)).font(theme::number_font()).color(theme::TEXT));
                    ui.end_row();
                }
                ui.label(RichText::new("Spectral balance low/mid/high").color(theme::TEXT_2));
                ui.label(RichText::new(balance_text(&m["spectral_balance_pct"])).font(theme::number_font()).color(theme::TEXT));
                ui.end_row();
            });
            let advice = rep["advice"].as_array().cloned().unwrap_or_default();
            if !advice.is_empty() {
                ui.add_space(theme::SP_4);
                chrome::section_label(ui, "Advice");
                for a in advice {
                    ui.horizontal_top(|ui| {
                        let (dot, _) = ui.allocate_exact_size(vec2(8.0, 14.0), Sense::hover());
                        ui.painter().circle_filled(dot.center(), 2.0, theme::ACCENT);
                        ui.label(RichText::new(a.as_str().unwrap_or("")).color(theme::TEXT));
                    });
                }
            }
            ui.add_space(theme::SP_4);
            chrome::section_label(ui, "Tracks");
            egui::ScrollArea::vertical().max_height(200.0).show(ui, |ui| {
                egui::Grid::new("trk").striped(true).spacing(vec2(theme::SP_6, theme::SP_2)).show(ui, |ui| {
                    for head in ["Track", "Peak", "Loudness", "Low/Mid/High"] {
                        ui.label(theme::label_text(head));
                    }
                    ui.end_row();
                    for t in rep["tracks"].as_array().cloned().unwrap_or_default() {
                        let a = &t["analysis"];
                        ui.label(t["name"].as_str().unwrap_or(""));
                        ui.label(RichText::new(report_number(&a["peak_dbfs"], 1, "dBFS")).font(theme::number_font()));
                        ui.label(RichText::new(report_number(&a["integrated_lufs"], 1, "LUFS")).font(theme::number_font()));
                        ui.label(RichText::new(balance_text(&a["spectral_balance_pct"])).font(theme::number_font()));
                        ui.end_row();
                    }
                });
            });
            ui.add_space(theme::SP_4);
            if self.ai_button(ui, "Ask the AI to fix the mix") {
                self.ask_ai(ondina_studio::prompts::FIX_MIX.into());
            }
        });
        if !open {
            self.mix_report = None;
        }
    }

    /// Buses a track can send or route to (not itself).
    pub(crate) fn buses_for(p: &Project, id: Id) -> Vec<(Id, String)> {
        p.tracks
            .iter()
            .filter(|b| b.kind == TrackKind::Bus && b.id != id)
            .map(|b| (b.id, b.name.clone()))
            .collect()
    }

    /// Entries of a "send" menu: the existing buses, then a new reverb or delay bus with the send to it.
    pub(crate) fn add_send_items(&mut self, ui: &mut egui::Ui, id: Id, buses: &[(Id, String)]) {
        for (bid, bname) in buses {
            if ui.button(bname).clicked() {
                self.cmd(Command::AddSend {
                    track_id: id,
                    bus_id: *bid,
                    level_db: Some(-10.0),
                    pre_fader: None,
                });
                ui.close_menu();
            }
        }
        if ui.button("New reverb bus…").clicked() {
            if let Some(v) = self.exec(Command::AddTrack {
                kind: TrackKind::Bus,
                name: Some("Reverb".into()),
                patch: None,
                instrument: None,
                preset: None,
                index: None,
            }) {
                if let Some(bid) = v["track_id"].as_u64() {
                    self.cmd(Command::AddEffect {
                        track_id: bid,
                        kind: "reverb".into(),
                        preset: Some("Reverb Return 100%".into()),
                        index: None,
                    });
                    self.cmd(Command::AddSend {
                        track_id: id,
                        bus_id: bid,
                        level_db: Some(-10.0),
                        pre_fader: None,
                    });
                }
            }
            ui.close_menu();
        }
        if ui.button("New delay bus…").clicked() {
            if let Some(v) = self.exec(Command::AddTrack {
                kind: TrackKind::Bus,
                name: Some("Delay".into()),
                patch: None,
                instrument: None,
                preset: None,
                index: None,
            }) {
                if let Some(bid) = v["track_id"].as_u64() {
                    self.cmd(Command::AddEffect {
                        track_id: bid,
                        kind: "stereo_delay".into(),
                        preset: Some("Delay Return 100%".into()),
                        index: None,
                    });
                    self.cmd(Command::AddSend {
                        track_id: id,
                        bus_id: bid,
                        level_db: Some(-12.0),
                        pre_fader: None,
                    });
                }
            }
            ui.close_menu();
        }
    }

    /// Entries of the output menu: Master or one of the buses.
    pub(crate) fn output_items(&mut self, ui: &mut egui::Ui, id: Id, buses: &[(Id, String)]) {
        if ui.button("Master").clicked() {
            self.cmd(Command::SetTrackOutput {
                track_id: id,
                bus_id: None,
            });
            ui.close_menu();
        }
        for (bid, bname) in buses {
            if ui.button(bname).clicked() {
                self.cmd(Command::SetTrackOutput {
                    track_id: id,
                    bus_id: Some(*bid),
                });
                ui.close_menu();
            }
        }
    }
}

pub fn effect_menu(ui: &mut egui::Ui, mut on_pick: impl FnMut(&'static str)) {
    let mut groups: Vec<&str> = plugins::registry()
        .iter()
        .filter(|p| p.category == PluginCategory::Effect)
        .map(|p| p.group)
        .collect();
    groups.dedup();
    let mut seen = std::collections::HashSet::new();
    for g in groups {
        if !seen.insert(g) {
            continue;
        }
        ui.menu_button(g, |ui| {
            for p in plugins::registry()
                .iter()
                .filter(|p| p.category == PluginCategory::Effect && p.group == g)
            {
                if ui.button(p.name).on_hover_text(p.description).clicked() {
                    on_pick(p.kind);
                    ui.close_menu();
                }
            }
        });
    }
}

/// Hover text for a plug-in slot of the strip: the full name when `short` cut it to `max_chars`, and why the
/// plug-in is not loaded. None when there is nothing to add.
pub fn slot_tooltip(label: &str, max_chars: usize, not_loaded: Option<&str>) -> Option<String> {
    let cut = label.chars().count() > max_chars;
    match (cut, not_loaded) {
        (false, None) => None,
        (true, None) => Some(label.to_string()),
        (false, Some(reason)) => Some(format!("Not loaded: {reason}")),
        (true, Some(reason)) => Some(format!("{label}\nNot loaded: {reason}")),
    }
}

pub fn short(s: &str, n: usize) -> String {
    if s.chars().count() > n {
        format!("{}…", s.chars().take(n - 1).collect::<String>())
    } else {
        s.to_string()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_horizontal_scroll_bar_takes_its_room_only_when_strips_overflow() {
        assert_eq!(strips_height(400.0, 1000.0, 3, 8.0), 400.0);
        assert_eq!(strips_height(400.0, 300.0, 3, 8.0), 392.0);
        assert_eq!(strips_height(400.0, 300.0, 0, 8.0), 400.0);
        assert!(inner_scroll().dormant_handle_opacity > 0.0, "inner bars show without hovering");
        assert!(egui::style::ScrollStyle::solid().allocated_width() > 0.0);
    }
    use serde_json::json;

    #[test]
    fn strips_keep_one_height_and_the_fader_takes_the_rest() {
        for (parts, master) in [(&CHANNEL_PARTS[..], false), (&MASTER_PARTS[..], true)] {
            let min = strip_min_height(parts, master);
            for h in [min, min + 37.5, 900.0] {
                let rows = strip_rows(parts, master, 10.0, h);
                assert_eq!(rows.len(), parts.len());
                assert!((rows.last().unwrap().2 - (10.0 + h)).abs() < 1e-3, "{master} {h}");
                // parts in order, never overlapping
                for w in rows.windows(2) {
                    assert!(w[1].1 >= w[0].2 - 1e-3);
                }
                let fader = rows.iter().find(|r| r.0 == Part::Fader).unwrap();
                assert!((fader.2 - fader.1 - (MIN_FADER_H + h - min)).abs() < 1e-3);
                // the head and the foot sit on the strip's edges
                assert_eq!(rows[0].1, 10.0);
                assert_eq!(rows.last().unwrap().0, Part::Foot);
            }
            // a panel shorter than the minimum keeps the fader at its minimum (the mixer scrolls)
            let rows = strip_rows(parts, master, 0.0, 50.0);
            let fader = rows.iter().find(|r| r.0 == Part::Fader).unwrap();
            assert_eq!(fader.2 - fader.1, MIN_FADER_H);
        }
    }

    #[test]
    fn effect_and_send_areas_have_fixed_rows() {
        assert_eq!(rows_h(1), SLOT_H);
        assert_eq!(rows_h(3), 3.0 * SLOT_H + 2.0 * ROW_GAP);
        assert_eq!(rows_h(0), 0.0);
        assert!(!effects_scroll(2, false));
        assert!(effects_scroll(3, false));
        assert!(!effects_scroll(3, true));
        assert_eq!(placeholders(1, 3), 2);
        assert_eq!(placeholders(5, 3), 0);
        // the effect area is the same height whatever the strip holds
        assert_eq!(Part::Effects.height(false), Some(LABEL_H + rows_h(FX_ROWS)));
        assert_eq!(Part::Effects.height(true), Some(LABEL_H + rows_h(MASTER_FX_ROWS)));
    }

    #[test]
    fn filter_picks_tracks_or_buses() {
        use TrackKind::*;
        let f = MixerFilter::from_index;
        assert!([Instrument, Audio, Bus].iter().all(|k| f(0).shows(*k)));
        assert!(f(1).shows(Instrument) && f(1).shows(Audio) && !f(1).shows(Bus));
        assert!(!f(2).shows(Instrument) && !f(2).shows(Audio) && f(2).shows(Bus));
        assert_eq!(f(9), MixerFilter::All);
        assert_eq!(MixerFilter::LABELS.len(), 3);
        assert_eq!(f(2).empty_text(), "No buses");
    }

    #[test]
    fn peak_box_reads_and_colors_the_peak() {
        assert_eq!(peak_text(f32::NEG_INFINITY), "\u{2212}\u{221E}");
        assert_eq!(peak_text(-61.0), "\u{2212}\u{221E}");
        assert_eq!(peak_text(-3.16), "\u{2212}3.2");
        assert_eq!(peak_text(0.4), "+0.4");
        assert_eq!(peak_color(-3.0, true), theme::REC);
        assert_eq!(peak_color(-3.0, false), theme::SOLO);
        assert_eq!(peak_color(f32::NEG_INFINITY, false), theme::TEXT_DISABLED);
    }

    #[test]
    fn input_slots_name_the_source() {
        assert_eq!(audio_input_label(0), "In 1\u{00B7}2");
        assert_eq!(audio_input_label(2), "In 3\u{00B7}4");
        assert_eq!(bus_input_label(0), "No sources");
        assert_eq!(bus_input_label(1), "1 source");
        assert_eq!(bus_input_label(3), "3 sources");
    }

    #[test]
    fn analysis_numbers_are_rounded_with_units() {
        assert_eq!(format_number(-17.799999237060547, 1, "LUFS"), "\u{2212}17.8 LUFS");
        assert_eq!(format_number(-1.25, 1, "dBTP"), "\u{2212}1.2 dBTP");
        assert_eq!(format_number(0.8700000047683716, 2, ""), "0.87");
        assert_eq!(format_number(-0.01, 1, "dB"), "0.0 dB");
        assert_eq!(format_number(6.0, 1, "LU"), "6.0 LU");
        assert_eq!(format_number(-120.0, 1, "LUFS"), "\u{2212}\u{221E} LUFS");
        assert_eq!(report_number(&json!(-17.799999237060547), 1, "LUFS"), "\u{2212}17.8 LUFS");
        assert_eq!(report_number(&Value::Null, 1, "LUFS"), "\u{2014}");
        assert_eq!(balance_text(&json!([34.0, 45.000001, 21.0])), "34 / 45 / 21 %");
        assert_eq!(balance_text(&Value::Null), "\u{2014} / \u{2014} / \u{2014} %");
        for (_, _, decimals, _) in ANALYSIS_ROWS {
            assert!(decimals <= 2);
        }
    }

    #[test]
    fn loudness_comes_from_the_report_master() {
        let rep = json!({"master": {"integrated_lufs": -14.199999809265137, "short_term_max_lufs": -12.0, "true_peak_dbtp": -1.3}});
        let l = MixLoudness::from_report(&rep).unwrap();
        assert_eq!(format_number(l.integrated_lufs, 1, "LUFS"), "\u{2212}14.2 LUFS");
        assert_eq!(format_number(l.true_peak_dbtp, 1, "dBTP"), "\u{2212}1.3 dBTP");
        assert_eq!(MixLoudness::from_report(&json!({"master": null})), None);
    }

    #[test]
    fn bus_sources_count_sends_and_routing() {
        let mut p = Project::default();
        let mk = |id: Id, kind: TrackKind| Track {
            id,
            name: format!("t{id}"),
            kind,
            color: [0; 3],
            channel: Channel::default(),
            instrument: None,
            midi_fx: vec![],
            regions: vec![],
            automation: vec![],
            record_armed: false,
            input_channel: 0,
            height: 0.0,
        };
        let bus = mk(10, TrackKind::Bus);
        let mut a = mk(1, TrackKind::Audio);
        a.channel.output = Output::Bus(10);
        let mut b = mk(2, TrackKind::Instrument);
        b.channel.sends.push(Send { bus_id: 10, level_db: -10.0, pre_fader: false });
        let c = mk(3, TrackKind::Audio);
        p.tracks = vec![a, b, c, bus];
        assert_eq!(bus_sources(&p, 10), 2);
        assert_eq!(bus_sources(&p, 3), 0);
    }

    #[test]
    fn slot_tooltip_is_empty_for_a_short_loaded_plugin() {
        assert_eq!(slot_tooltip("Compressor", 15, None), None);
    }

    #[test]
    fn slot_tooltip_shows_the_full_name_when_cut() {
        assert_eq!(slot_tooltip("Valhalla VintageVerb", 15, None), Some("Valhalla VintageVerb".into()));
        assert_eq!(slot_tooltip("Fifteen chars!!", 15, None), None);
        assert_eq!(short("Fifteen chars!!", 15), "Fifteen chars!!");
    }

    #[test]
    fn slot_tooltip_says_why_a_plugin_is_not_loaded() {
        assert_eq!(
            slot_tooltip("Foo", 15, Some("X is not a .vst3 plug-in bundle")),
            Some("Not loaded: X is not a .vst3 plug-in bundle".into())
        );
        assert_eq!(
            slot_tooltip("Valhalla VintageVerb", 15, Some("X is not a .vst3 plug-in bundle")),
            Some("Valhalla VintageVerb\nNot loaded: X is not a .vst3 plug-in bundle".into())
        );
    }

    #[test]
    fn short_cuts_long_names() {
        assert_eq!(short("Fifteen chars!!", 15), "Fifteen chars!!");
        assert_eq!(short("Valhalla VintageVerb", 15), "Valhalla Vinta…");
    }
}
