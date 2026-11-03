//! The drawings of the synth windows: the Analog Synth's oscillator shapes, filter curve and amp and filter
//! envelopes, the FM Synth's amp and index envelopes, and the keyboard at the bottom of every built-in instrument.
//! What is drawn comes from the same functions the synths play with (`ondina_dsp::synth`, `ondina_dsp::util`);
//! the math is plain functions the tests cover, synth_graph draws them and turns the mouse into edits.

use std::collections::BTreeMap;

use eframe::egui::{self, Color32, CursorIcon, PointerButton, Pos2, Rect, Sense, Stroke, Vec2};
use ondina_core::command::Command;
use ondina_core::model::{Id, PluginInstance, Project, MASTER_ID};
use ondina_core::plugins;
use ondina_dsp::synth::{analog_filter_db, analog_resonance_db, analog_resonance_from_db, FILTER_ENV_OCTAVES, FM_INDEX_ATTACK};
use ondina_dsp::util::{wave_shape, Wave};
use ondina_dsp::MidiKind;

use crate::app::OndinaApp;
use crate::theme;
use crate::widgets::{envelope, fill_under, format_param_value, paint_graph_readout, unit_to_value_log, value_to_unit_log, with_alpha, EnvParam, EnvSpec, KeyboardState, ParamsEdit};

pub const F_LO: f32 = 20.0;
pub const F_HI: f32 = 20000.0;
/// The filter graph shows -36..+36 dB: full resonance peaks at +34 dB.
pub const FILTER_DB_TOP: f32 = 36.0;
pub const FILTER_DB_BOTTOM: f32 = -36.0;
pub const POINT_R: f32 = 6.0;
/// Half the side of the square around the filter point that takes the mouse.
pub const HIT_R: f32 = 10.0;
/// Shift: drags and wheel turns move ten times less.
pub const FINE: f32 = 0.1;
/// Wheel points for the whole resonance range: a notch (40 points) is 0.1.
pub const RES_WHEEL_POINTS: f32 = 400.0;
/// Wheel points per wave shape: a notch is one shape.
pub const WAVE_WHEEL_POINTS: f32 = 40.0;
pub const OSC_BOX_W: f32 = 104.0;
pub const ROW_OSC_H: f32 = 92.0;
pub const ROW_ENV_H: f32 = 112.0;

/// Whether plug-in kind `kind` has the synth drawings.
pub fn has_synth_view(kind: &str) -> bool {
    matches!(kind, "analog_synth" | "fm_synth")
}

/// The sample rate the curve is computed at: the engine's, or 48 kHz before it reports one.
pub fn curve_sample_rate(sr: u32) -> f32 {
    if sr >= 8000 {
        sr as f32
    } else {
        48000.0
    }
}

/// The area the filter curve uses inside its box: the point stays whole inside at either end.
pub fn filter_plot(rect: Rect) -> Rect {
    rect.shrink(POINT_R + 2.0)
}

pub fn freq_to_x(f: f32, plot: Rect) -> f32 {
    plot.left() + value_to_unit_log(f, F_LO, F_HI) * plot.width()
}

/// The frequency at `x`, clamped to F_LO..F_HI.
pub fn x_to_freq(x: f32, plot: Rect) -> f32 {
    let u = if plot.width() > 0.0 { (x - plot.left()) / plot.width() } else { 0.0 };
    unit_to_value_log(u, F_LO, F_HI)
}

/// FILTER_DB_TOP at the top, FILTER_DB_BOTTOM at the bottom, clamped to the plot.
pub fn filter_db_to_y(db: f32, plot: Rect) -> f32 {
    let u = (db - FILTER_DB_BOTTOM) / (FILTER_DB_TOP - FILTER_DB_BOTTOM);
    (plot.bottom() - u * plot.height()).clamp(plot.top(), plot.bottom())
}

/// The gain at `y`, clamped to the graph's range.
pub fn filter_y_to_db(y: f32, plot: Rect) -> f32 {
    if plot.height() <= 0.0 {
        return 0.0;
    }
    let u = (plot.bottom() - y) / plot.height();
    (FILTER_DB_BOTTOM + u * (FILTER_DB_TOP - FILTER_DB_BOTTOM)).clamp(FILTER_DB_BOTTOM, FILTER_DB_TOP)
}

/// The wave index `step` shapes on from `v`, going round: 4 + 1 is 0, 0 - 1 is 4.
pub fn next_wave(v: f32, step: i32, count: usize) -> f32 {
    let n = count.max(1) as i32;
    (v.round() as i32 + step).rem_euclid(n) as f32
}

/// A wheel turn of `scroll` points over an oscillator added to `acc`: one shape per WAVE_WHEEL_POINTS, wheel down
/// going on. Returns what is left over and the steps.
pub fn wave_wheel(acc: f32, scroll: Vec2) -> (f32, i32) {
    let s = scroll.x + scroll.y;
    if !s.is_finite() {
        return (acc, 0);
    }
    let total = acc - s;
    let steps = (total / WAVE_WHEEL_POINTS).trunc();
    (total - steps * WAVE_WHEEL_POINTS, steps as i32)
}

/// Two cycles of wave `wave` (registry index) with pulse width `pw` across `rect`, in `n` + 1 points.
pub fn wave_points(wave: f32, pw: f32, rect: Rect, n: usize) -> Vec<Pos2> {
    let n = n.max(2);
    let w = Wave::from_index(wave);
    let half = rect.height() / 2.0;
    (0..=n)
        .map(|i| {
            let f = i as f32 / n as f32;
            let v = wave_shape(w, 2.0 * f, pw).clamp(-1.0, 1.0);
            egui::pos2(rect.left() + rect.width() * f, rect.center().y - v * half)
        })
        .collect()
}

/// The filter point: at the cutoff, at the gain the three modes have there (1/damping), so it sits on the curve.
pub fn filter_handle(inst: &PluginInstance, plot: Rect) -> Pos2 {
    egui::pos2(freq_to_x(inst.param("cutoff"), plot), filter_db_to_y(analog_resonance_db(inst.param("resonance")), plot))
}

/// Cutoff and resonance after the filter point, which was at `start`, has moved `moved` points: x is the cutoff on
/// the log axis, y the gain at the cutoff, which is the resonance.
pub fn filter_drag(inst: &PluginInstance, start: Pos2, moved: Vec2, plot: Rect) -> BTreeMap<String, f32> {
    let (lo, hi) = registry_param(&inst.kind, "cutoff").map_or((F_LO, F_HI), |p| (p.min, p.max));
    let cutoff = x_to_freq(start.x + moved.x, plot).clamp(lo, hi);
    let resonance = analog_resonance_from_db(filter_y_to_db(start.y + moved.y, plot)).clamp(0.0, 1.0);
    BTreeMap::from([("cutoff".to_string(), cutoff), ("resonance".to_string(), resonance)])
}

/// Resonance after a wheel turn of `scroll` points (x and y summed: egui turns Shift+wheel into x), 0..1; wheel up
/// raises it. A non-finite turn leaves it as it is.
pub fn wheel_resonance(res: f32, scroll: Vec2, fine: bool) -> f32 {
    let s = scroll.x + scroll.y;
    if !s.is_finite() {
        return res;
    }
    (res + s / RES_WHEEL_POINTS * if fine { FINE } else { 1.0 }).clamp(0.0, 1.0)
}

fn registry_param(kind: &str, id: &str) -> Option<&'static plugins::ParamInfo> {
    plugins::find(kind).and_then(|i| i.params.iter().find(|p| p.id == id))
}

/// Cutoff and resonance back to their registry defaults, as one change.
pub fn filter_reset(inst: &PluginInstance) -> BTreeMap<String, f32> {
    ["cutoff", "resonance"].into_iter().filter_map(|id| registry_param(&inst.kind, id).map(|p| (id.to_string(), p.default))).collect()
}

/// "Lowpass · 2.50 kHz · Res 0.20".
pub fn filter_readout(inst: &PluginInstance) -> String {
    let choices = registry_param(&inst.kind, "filter_type").map_or(&[][..], |p| p.choices);
    let mode = choices.get(inst.param("filter_type").round().max(0.0) as usize).copied().unwrap_or("Filter");
    format!("{mode} \u{00B7} {} \u{00B7} Res {:.2}", format_param_value(inst.param("cutoff"), "Hz"), inst.param("resonance"))
}

/// The stage of an envelope that parameter `id` of `inst` moves, with its registry range and default.
fn stage(inst: &PluginInstance, id: &'static str) -> EnvParam {
    let p = registry_param(&inst.kind, id);
    EnvParam { id: Some(id), value: inst.param(id), min: p.map_or(0.0, |p| p.min), max: p.map_or(1.0, |p| p.max), default: p.map_or(0.0, |p| p.default) }
}

/// The amp envelope (Analog and FM Synth).
pub fn amp_env(inst: &PluginInstance, color: Color32) -> EnvSpec {
    EnvSpec { title: "AMP", attack: stage(inst, "attack"), decay: stage(inst, "decay"), sustain: stage(inst, "sustain"), release: stage(inst, "release"), color }
}

/// The Analog Synth's filter envelope.
pub fn filter_env(inst: &PluginInstance, color: Color32) -> EnvSpec {
    EnvSpec { title: "FILTER ENV", attack: stage(inst, "f_attack"), decay: stage(inst, "f_decay"), sustain: stage(inst, "f_sustain"), release: stage(inst, "f_release"), color }
}

/// The FM Synth's modulation-index envelope: its attack is fixed and its release is the amp's, so neither has a
/// point.
pub fn index_env(inst: &PluginInstance, color: Color32) -> EnvSpec {
    let attack = EnvParam { id: None, value: FM_INDEX_ATTACK, min: 0.001, max: 5.0, default: FM_INDEX_ATTACK };
    let amp_release = stage(inst, "release");
    let release = EnvParam { id: None, ..amp_release };
    EnvSpec { title: "INDEX ENV", attack, decay: stage(inst, "index_decay"), sustain: stage(inst, "index_sustain"), release, color }
}

/// The undo key of the wheel over oscillator `n`: turns close together merge.
pub fn osc_key(pid: Id, n: u8) -> String {
    format!("pw{pid}:osc{n}:wave")
}

/// The undo key of a drag of the filter point: one key per gesture.
pub fn filter_drag_key(pid: Id, gesture: u64) -> String {
    format!("pw{pid}:filter#{gesture}")
}

/// The undo key of the wheel over the filter: turns close together merge.
pub fn filter_wheel_key(pid: Id) -> String {
    format!("pw{pid}:filter:res")
}

/// The name of an envelope editor's undo steps: "pw7:amp", "pw7:fenv", "pw7:index".
pub fn env_prefix(pid: Id, which: &str) -> String {
    format!("pw{pid}:{which}")
}

// ---------------------------------------------------------------------------- drawing and mouse

pub struct SynthGraphOut {
    /// The boxes (tests use them to find the points; the window has no use for them).
    #[cfg(test)]
    pub layout: SynthLayout,
    pub edits: Vec<ParamsEdit>,
}

#[cfg(test)]
#[derive(Debug, Clone, Copy)]
pub struct SynthLayout {
    pub osc1: Rect,
    pub osc2: Rect,
    pub filter: Rect,
    pub env1: Rect,
    pub env2: Rect,
}

#[cfg(test)]
impl Default for SynthLayout {
    fn default() -> Self {
        SynthLayout { osc1: Rect::NOTHING, osc2: Rect::NOTHING, filter: Rect::NOTHING, env1: Rect::NOTHING, env2: Rect::NOTHING }
    }
}

/// The drag of the filter point, kept in egui's memory between frames.
#[derive(Clone, Copy)]
struct FilterDrag {
    start: Pos2,
    moved: Vec2,
}

fn changes(inst: &PluginInstance, params: &BTreeMap<String, f32>) -> bool {
    params.iter().any(|(k, v)| inst.param(k) != *v)
}

/// The box of oscillator `n` (1 or 2): its shape, a click for the next one, a right-click for the previous one, the
/// wheel to go through them.
fn osc_box(ui: &mut egui::Ui, inst: &PluginInstance, rect: Rect, n: u8, edits: &mut Vec<ParamsEdit>) {
    let id = if n == 1 { "osc1_wave" } else { "osc2_wave" };
    let choices = registry_param(&inst.kind, id).map_or(&[][..], |p| p.choices);
    let count = choices.len().max(1);
    let v = inst.param(id);
    let resp = ui
        .interact(rect, ui.id().with(("osc-box", n)), Sense::click())
        .on_hover_text("Click: next shape \u{00B7} Right-click: previous \u{00B7} Wheel: change");
    if resp.clicked() {
        edits.push(ParamsEdit::Set { params: BTreeMap::from([(id.to_string(), next_wave(v, 1, count))]) });
    }
    if resp.secondary_clicked() {
        edits.push(ParamsEdit::Set { params: BTreeMap::from([(id.to_string(), next_wave(v, -1, count))]) });
    }
    let acc_id = egui::Id::new(("osc-wheel", inst.id, n));
    if resp.contains_pointer() {
        let s = ui.input(|i| i.raw_scroll_delta);
        if s != Vec2::ZERO {
            let acc = ui.data(|d| d.get_temp::<f32>(acc_id)).unwrap_or(0.0);
            let (left, steps) = wave_wheel(acc, s);
            ui.data_mut(|d| d.insert_temp(acc_id, left));
            if steps != 0 {
                edits.push(ParamsEdit::Drag { params: BTreeMap::from([(id.to_string(), next_wave(v, steps, count))]), key: osc_key(inst.id, n) });
            }
            ui.input_mut(|i| {
                i.raw_scroll_delta = Vec2::ZERO;
                i.smooth_scroll_delta = Vec2::ZERO;
            });
        }
    }
    let painter = ui.painter_at(rect);
    painter.rect(rect, theme::R_LG, theme::INSET, Stroke::new(1.0_f32, if resp.hovered() { theme::BORDER_HOVER } else { theme::BORDER }));
    let name = choices.get(v.round().max(0.0) as usize).copied().unwrap_or("?");
    painter.text(rect.left_top() + egui::vec2(theme::SP_4, theme::SP_3), egui::Align2::LEFT_TOP, format!("OSC {n} \u{00B7} {name}"), theme::label_font(), theme::TEXT_3);
    let wave_rect = Rect::from_min_max(rect.left_top() + egui::vec2(theme::SP_4, 24.0), rect.right_bottom() - egui::vec2(theme::SP_4, theme::SP_5));
    painter.hline(wave_rect.x_range(), wave_rect.center().y, Stroke::new(1.0_f32, theme::CENTER_LINE));
    let pw = inst.param("pulse_width");
    let color = if inst.bypass { theme::TEXT_DISABLED } else { theme::ACCENT };
    painter.add(egui::Shape::line(wave_points(v, pw, wave_rect, 128), Stroke::new(1.5_f32, color)));
    if Wave::from_index(v) == Wave::Square {
        painter.text(rect.right_bottom() - egui::vec2(theme::SP_3, theme::SP_1), egui::Align2::RIGHT_BOTTOM, format!("PW {pw:.2}"), theme::label_font(), theme::TEXT_3);
    }
}

/// Grid frequencies: 20, 30 ... 90, 100, 200 ... 20000 Hz.
fn grid_freqs() -> impl Iterator<Item = f32> {
    [10.0f32, 100.0, 1000.0, 10000.0].into_iter().flat_map(|d| (1..10).map(move |k| d * k as f32)).filter(|f| (F_LO..=F_HI).contains(f)).chain(std::iter::once(F_HI))
}

/// `n` frequencies from F_LO to F_HI, evenly spaced on the log axis.
fn log_freqs(n: usize) -> Vec<f32> {
    let n = n.max(2);
    (0..n).map(|i| F_LO * (F_HI / F_LO).powf(i as f32 / (n - 1) as f32)).collect()
}

/// The points of the filter curve at `cutoff` (the others as the project holds them).
fn filter_curve(inst: &PluginInstance, cutoff: f32, sr: f32, plot: Rect, freqs: &[f32]) -> Vec<Pos2> {
    let (ft, res) = (inst.param("filter_type"), inst.param("resonance"));
    freqs.iter().map(|f| egui::pos2(freq_to_x(*f, plot), filter_db_to_y(analog_filter_db(ft, cutoff, res, *f, sr), plot))).collect()
}

/// The filter curve with its point: drag it for cutoff and resonance, the wheel anywhere on the box for the resonance.
fn filter_box(ui: &mut egui::Ui, inst: &PluginInstance, rect: Rect, sr: f32, gesture: &mut u64, edits: &mut Vec<ParamsEdit>) {
    let plot = filter_plot(rect);
    // the background first, hover only: the point registered after it takes the clicks
    let bg = ui.interact(rect, ui.id().with("filter-bg"), Sense::hover());
    let pos = filter_handle(inst, plot);
    let r = ui.interact(Rect::from_center_size(pos, Vec2::splat(2.0 * HIT_R)), ui.id().with("filter-point"), Sense::click_and_drag());
    let drag_id = egui::Id::new(("filter-drag", inst.id));
    let mut drag: Option<FilterDrag> = ui.data(|d| d.get_temp(drag_id));
    let fine = ui.input(|i| i.modifiers.shift);
    let mut hot = false;
    if r.drag_started_by(PointerButton::Primary) {
        *gesture += 1;
        drag = Some(FilterDrag { start: pos, moved: Vec2::ZERO });
    }
    if r.dragged_by(PointerButton::Primary) {
        if let Some(d) = drag.as_mut() {
            d.moved += if fine { r.drag_delta() * FINE } else { r.drag_delta() };
            let params = filter_drag(inst, d.start, d.moved, plot);
            if changes(inst, &params) {
                edits.push(ParamsEdit::Drag { params, key: filter_drag_key(inst.id, *gesture) });
            }
            hot = true;
            ui.ctx().set_cursor_icon(CursorIcon::Grabbing);
        }
    }
    if r.drag_stopped() {
        drag = None;
    }
    if r.double_clicked() {
        edits.push(ParamsEdit::Set { params: filter_reset(inst) });
    }
    if r.hovered() {
        hot = true;
        if !r.dragged() {
            ui.ctx().set_cursor_icon(CursorIcon::Grab);
        }
    }
    ui.data_mut(|m| match drag {
        Some(d) => m.insert_temp(drag_id, d),
        None => m.remove::<FilterDrag>(drag_id),
    });
    if bg.contains_pointer() || r.contains_pointer() {
        let s = ui.input(|i| i.raw_scroll_delta);
        if s != Vec2::ZERO {
            let res = wheel_resonance(inst.param("resonance"), s, fine);
            if res != inst.param("resonance") {
                edits.push(ParamsEdit::Drag { params: BTreeMap::from([("resonance".to_string(), res)]), key: filter_wheel_key(inst.id) });
            }
            // the wheel was for the filter: nothing else scrolls with it
            ui.input_mut(|i| {
                i.raw_scroll_delta = Vec2::ZERO;
                i.smooth_scroll_delta = Vec2::ZERO;
            });
        }
    }

    let painter = ui.painter_at(rect);
    painter.rect(rect, theme::R_LG, theme::INSET, Stroke::new(1.0_f32, theme::BORDER));
    for f in grid_freqs() {
        let decade = [100.0, 1000.0, 10000.0].contains(&f);
        painter.vline(freq_to_x(f, plot), plot.y_range(), Stroke::new(1.0_f32, if decade { theme::GRID_BAR_LINE } else { theme::GRID_BEAT_LINE }));
    }
    for (f, label) in [(100.0, "100"), (1000.0, "1k"), (10000.0, "10k")] {
        painter.text(egui::pos2(freq_to_x(f, plot) + theme::SP_1, plot.bottom()), egui::Align2::LEFT_BOTTOM, label, theme::label_font(), theme::TEXT_3);
    }
    for db in [-24.0, -12.0, 0.0, 12.0, 24.0] {
        let color = if db == 0.0 { theme::CENTER_LINE } else { theme::GRID_BEAT_LINE };
        painter.hline(plot.x_range(), filter_db_to_y(db, plot), Stroke::new(1.0_f32, color));
    }
    let n = (plot.width() / 2.0).clamp(64.0, 400.0) as usize;
    let freqs = log_freqs(n);
    let cutoff = inst.param("cutoff");
    let amount = inst.param("filter_env");
    if amount != 0.0 {
        // where the filter goes with the envelope at its peak
        let reach = (cutoff * 2f32.powf(amount * FILTER_ENV_OCTAVES)).clamp(F_LO, F_HI);
        let color = if inst.bypass { theme::TEXT_DISABLED } else { theme::SOLO };
        painter.add(egui::Shape::line(filter_curve(inst, reach, sr, plot, &freqs), Stroke::new(1.0_f32, with_alpha(color, 110))));
    }
    let curve = filter_curve(inst, cutoff, sr, plot, &freqs);
    fill_under(&painter, &curve, plot.bottom(), with_alpha(theme::ACCENT, 40));
    painter.add(egui::Shape::line(curve, Stroke::new(2.0_f32, if inst.bypass { theme::TEXT_DISABLED } else { theme::TEXT })));
    painter.circle_filled(pos, POINT_R, if inst.bypass { theme::TEXT_DISABLED } else { theme::ACCENT });
    if hot {
        painter.circle_stroke(pos, POINT_R + 2.0, Stroke::new(1.5_f32, theme::TEXT));
    }
    painter.text(rect.left_top() + egui::vec2(theme::SP_4, theme::SP_3), egui::Align2::LEFT_TOP, "FILTER", theme::label_font(), theme::TEXT_3);
    painter.text(egui::pos2(plot.left() + theme::SP_3, plot.bottom() - 14.0), egui::Align2::LEFT_BOTTOM, "at C4, envelope at rest", theme::label_font(), theme::TEXT_3);
    if hot {
        paint_graph_readout(&painter, filter_readout(inst), pos, rect, POINT_R);
    } else if let Some(p) = bg.hover_pos() {
        let f = x_to_freq(p.x, plot);
        let db = analog_filter_db(inst.param("filter_type"), cutoff, inst.param("resonance"), f, sr);
        let text = format!("{} \u{00B7} {}", format_param_value(f, "Hz"), format_param_value(db, "dB"));
        painter.text(egui::pos2(plot.right() - theme::SP_3, rect.top() + theme::SP_3), egui::Align2::RIGHT_TOP, text, theme::number_font(), theme::TEXT_3);
    }
}

/// Two envelope editors side by side across the available width. Returns their boxes.
fn envelope_row(ui: &mut egui::Ui, specs: [(&EnvSpec, String); 2], gesture: &mut u64, edits: &mut Vec<ParamsEdit>) -> [Rect; 2] {
    let w = ((ui.available_width() - theme::SP_3) / 2.0).floor().max(40.0);
    let size = egui::vec2(w, ROW_ENV_H);
    let top = ui.available_rect_before_wrap().min;
    ui.horizontal(|ui| {
        ui.spacing_mut().item_spacing.x = theme::SP_3;
        for (spec, prefix) in &specs {
            edits.extend(envelope(ui, spec, prefix, size, gesture));
        }
    });
    [Rect::from_min_size(top, size), Rect::from_min_size(top + egui::vec2(w + theme::SP_3, 0.0), size)]
}

/// The synth's drawings above its knobs. `gesture` is the window's drag counter (`PluginWindows::gesture`).
pub fn synth_graph(ui: &mut egui::Ui, inst: &PluginInstance, sr: f32, gesture: &mut u64) -> SynthGraphOut {
    let mut edits = Vec::new();
    #[cfg(test)]
    let layout;
    let tint = |c: Color32| if inst.bypass { theme::TEXT_DISABLED } else { c };
    let help = if inst.kind == "analog_synth" {
        let (row, _) = ui.allocate_exact_size(egui::vec2(ui.available_width(), ROW_OSC_H), Sense::hover());
        let osc1 = Rect::from_min_size(row.min, egui::vec2(OSC_BOX_W, ROW_OSC_H));
        let osc2 = Rect::from_min_size(egui::pos2(osc1.right() + theme::SP_3, row.top()), egui::vec2(OSC_BOX_W, ROW_OSC_H));
        let filter = Rect::from_min_max(egui::pos2(osc2.right() + theme::SP_3, row.top()), row.right_bottom());
        osc_box(ui, inst, osc1, 1, &mut edits);
        osc_box(ui, inst, osc2, 2, &mut edits);
        filter_box(ui, inst, filter, sr, gesture, &mut edits);
        ui.add_space(theme::SP_3);
        let amp = amp_env(inst, tint(theme::ACCENT));
        let fenv = filter_env(inst, tint(theme::SOLO));
        let [env1, env2] = envelope_row(ui, [(&amp, env_prefix(inst.id, "amp")), (&fenv, env_prefix(inst.id, "fenv"))], gesture, &mut edits);
        let amount = format!("Amount {:+.2}", inst.param("filter_env"));
        ui.painter_at(env2).text(env2.right_top() + egui::vec2(-theme::SP_4, theme::SP_3), egui::Align2::RIGHT_TOP, amount, theme::label_font(), theme::TEXT_3);
        #[cfg(test)]
        {
            layout = SynthLayout { osc1, osc2, filter, env1, env2 };
        }
        let _ = env1;
        "Drag a point (Shift: fine) \u{00B7} Double-click: default \u{00B7} Wheel on the filter: resonance \u{00B7} Click a wave: next shape"
    } else {
        let amp = amp_env(inst, tint(theme::ACCENT));
        let index = index_env(inst, tint(theme::MONITOR));
        let [env1, env2] = envelope_row(ui, [(&amp, env_prefix(inst.id, "amp")), (&index, env_prefix(inst.id, "index"))], gesture, &mut edits);
        ui.painter_at(env2).text(env2.right_top() + egui::vec2(-theme::SP_4, theme::SP_3), egui::Align2::RIGHT_TOP, "Release follows Amp", theme::label_font(), theme::TEXT_3);
        #[cfg(test)]
        {
            layout = SynthLayout { env1, env2, ..Default::default() };
        }
        let _ = env1;
        "Drag a point (Shift: fine) \u{00B7} Double-click: default \u{00B7} The index envelope starts at once"
    };
    ui.label(egui::RichText::new(help).small().color(theme::TEXT_3));
    SynthGraphOut {
        #[cfg(test)]
        layout,
        edits,
    }
}

/// The keys held on an instrument window's keyboard and the track their notes went to.
pub struct InstrumentKeys {
    pub track: Id,
    pub keys: KeyboardState,
}

impl OndinaApp {
    /// What the synth drawings asked for, as commands: a drag or a wheel turn merges into one undo step per key.
    pub(crate) fn apply_params_edits(&mut self, pid: Id, edits: Vec<ParamsEdit>) {
        for e in edits {
            match e {
                ParamsEdit::Drag { params, key } => self.cmd_merge(Command::SetPluginParams { plugin_id: pid, params }, key),
                ParamsEdit::Set { params } => self.cmd(Command::SetPluginParams { plugin_id: pid, params }),
            }
        }
    }

    /// The keyboard at the bottom of a built-in instrument's window: live notes on the instrument's track.
    pub(crate) fn instrument_keyboard(&mut self, ui: &mut egui::Ui, p: &Project, pid: Id) {
        let Some(track) = p.plugin_owner(pid).filter(|t| *t != MASTER_ID) else {
            return;
        };
        let own_keys = ui.ctx().viewport_id() != egui::ViewportId::ROOT;
        let entry = self.plugin_windows.keyboards.entry(pid).or_insert(InstrumentKeys { track, keys: KeyboardState::default() });
        // a NoteOff always goes where its NoteOn went
        if entry.keys.held.is_none() {
            entry.track = track;
        }
        let events = instrument_keys_frame(ui, &mut entry.keys, own_keys);
        let to = entry.track;
        for kind in events {
            self.live_note(to, kind);
        }
    }
}

/// One frame of an instrument keyboard: always drawn and registered, so the first click after the window gets its
/// focus back already lands on the keys; a window without focus releases the held key.
fn instrument_keys_frame(ui: &mut egui::Ui, keys: &mut KeyboardState, own_keys: bool) -> Vec<MidiKind> {
    let focused = ui.ctx().input(|i| i.viewport().focused) != Some(false);
    let mut events = crate::widgets::keyboard(ui, keys, own_keys);
    if !focused {
        events.extend(keys.release_all());
    }
    events
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::widgets::{env_handle_pos, env_plot, EnvHandle};
    use eframe::egui::{pos2, vec2, Event, Modifiers, MouseWheelUnit};

    fn instance(kind: &str) -> PluginInstance {
        PluginInstance { id: 3, kind: kind.into(), params: Default::default(), bypass: false, preset: None, sample: None, vst3_path: None, vst3_params: vec![], vst3_state: None }
    }

    fn with(mut i: PluginInstance, kv: &[(&str, f32)]) -> PluginInstance {
        for (k, v) in kv {
            i.params.insert(k.to_string(), *v);
        }
        i
    }

    fn near(a: f32, b: f32, tol: f32) -> bool {
        (a - b).abs() <= tol
    }

    fn plot() -> Rect {
        Rect::from_min_size(pos2(0.0, 0.0), vec2(400.0, 120.0))
    }

    #[test]
    fn envelope_specs_name_registry_parameters() {
        let analog = instance("analog_synth");
        let fm = instance("fm_synth");
        for (kind, spec) in [("analog_synth", amp_env(&analog, theme::ACCENT)), ("analog_synth", filter_env(&analog, theme::SOLO)), ("fm_synth", amp_env(&fm, theme::ACCENT)), ("fm_synth", index_env(&fm, theme::MONITOR))] {
            let info = plugins::find(kind).unwrap();
            for st in [spec.attack, spec.decay, spec.sustain, spec.release] {
                if let Some(id) = st.id {
                    let p = info.params.iter().find(|p| p.id == id).unwrap_or_else(|| panic!("{kind}: {id}"));
                    assert_eq!((st.min, st.max, st.default), (p.min, p.max, p.default), "{kind}: {id}");
                    assert_eq!(st.value, p.default, "{kind}: {id}");
                }
            }
        }
        let fm = with(fm, &[("release", 1.7)]);
        let idx = index_env(&fm, theme::MONITOR);
        assert_eq!((idx.attack.id, idx.attack.value), (None, FM_INDEX_ATTACK));
        assert_eq!((idx.release.id, idx.release.value), (None, 1.7));
        assert_eq!((idx.decay.id, idx.sustain.id), (Some("index_decay"), Some("index_sustain")));
    }

    #[test]
    fn waves_cycle_both_ways() {
        assert_eq!(next_wave(4.0, 1, 5), 0.0);
        assert_eq!(next_wave(0.0, -1, 5), 4.0);
        assert_eq!(next_wave(2.0, 7, 5), 4.0);
        assert_eq!(wave_wheel(0.0, vec2(0.0, -40.0)), (0.0, 1));
        assert_eq!(wave_wheel(0.0, vec2(0.0, 40.0)), (0.0, -1));
        let (acc, steps) = wave_wheel(0.0, vec2(0.0, -25.0));
        assert_eq!(steps, 0);
        assert_eq!(wave_wheel(acc, vec2(0.0, -25.0)), (10.0, 1));
        assert_eq!(wave_wheel(5.0, vec2(f32::NAN, 0.0)), (5.0, 0));
        let r = Rect::from_min_size(pos2(10.0, 10.0), vec2(80.0, 40.0));
        for w in 0..5 {
            let pts = wave_points(w as f32, 0.3, r, 64);
            assert_eq!(pts.len(), 65);
            assert!(pts.iter().all(|p| r.expand(0.01).contains(*p)), "wave {w}");
        }
    }

    #[test]
    fn filter_handle_sits_on_the_curve() {
        let p = plot();
        for ft in 0..3 {
            for (cutoff, res) in [(2500.0, 0.2), (300.0, 0.9), (8000.0, 0.0)] {
                let inst = with(instance("analog_synth"), &[("filter_type", ft as f32), ("cutoff", cutoff), ("resonance", res)]);
                let h = filter_handle(&inst, p);
                let y = filter_db_to_y(analog_filter_db(ft as f32, cutoff, res, cutoff, 48000.0), p);
                assert!(near(h.y, y, 0.5), "type {ft} {cutoff} {res}: {} vs {y}", h.y);
                assert!(near(h.x, freq_to_x(cutoff, p), 1e-3));
            }
        }
        assert_eq!(filter_db_to_y(36.0, p), 0.0);
        assert_eq!(filter_db_to_y(-120.0, p), 120.0);
        for d in [-30.0, 0.0, 12.5] {
            assert!(near(filter_y_to_db(filter_db_to_y(d, p), p), d, 1e-3));
        }
        assert_eq!(curve_sample_rate(0), 48000.0);
        assert_eq!(curve_sample_rate(96000), 96000.0);
        assert_eq!(filter_plot(p), p.shrink(POINT_R + 2.0));
    }

    #[test]
    fn filter_drag_maps_x_to_cutoff_and_y_to_resonance() {
        let p = plot();
        let inst = instance("analog_synth");
        let start = filter_handle(&inst, p);
        let still = filter_drag(&inst, start, Vec2::ZERO, p);
        assert!(near(still["cutoff"], 2500.0, 2.0) && near(still["resonance"], 0.2, 1e-3), "{still:?}");
        assert!(filter_drag(&inst, start, vec2(40.0, 0.0), p)["cutoff"] > 2600.0);
        assert_eq!(filter_drag(&inst, start, vec2(1e5, 0.0), p)["cutoff"], 20000.0);
        assert_eq!(filter_drag(&inst, start, vec2(-1e5, 0.0), p)["cutoff"], 20.0);
        assert!(filter_drag(&inst, start, vec2(0.0, -10.0), p)["resonance"] > 0.2);
        assert_eq!(filter_drag(&inst, start, vec2(0.0, -1e5), p)["resonance"], 1.0);
        assert_eq!(filter_drag(&inst, start, vec2(0.0, 1e5), p)["resonance"], 0.0);
    }

    #[test]
    fn filter_wheel_changes_resonance() {
        assert!(near(wheel_resonance(0.2, vec2(0.0, 40.0), false), 0.3, 1e-5));
        assert!(near(wheel_resonance(0.2, vec2(0.0, 40.0), true), 0.21, 1e-5));
        assert!(near(wheel_resonance(0.2, vec2(40.0, 0.0), true), 0.21, 1e-5));
        assert_eq!(wheel_resonance(0.95, vec2(0.0, 400.0), false), 1.0);
        assert_eq!(wheel_resonance(0.05, vec2(0.0, -400.0), false), 0.0);
        assert_eq!(wheel_resonance(0.4, vec2(f32::NAN, 0.0), false), 0.4);
        let inst = with(instance("analog_synth"), &[("cutoff", 900.0), ("resonance", 0.7), ("filter_type", 1.0)]);
        assert_eq!(filter_reset(&inst), BTreeMap::from([("cutoff".to_string(), 2500.0), ("resonance".to_string(), 0.2)]));
        assert_eq!(filter_readout(&inst), "Highpass \u{00B7} 900 Hz \u{00B7} Res 0.70");
        assert_eq!(filter_readout(&instance("analog_synth")), "Lowpass \u{00B7} 2.50 kHz \u{00B7} Res 0.20");
    }

    #[test]
    fn keys_have_the_window_format() {
        assert_eq!(osc_key(7, 1), "pw7:osc1:wave");
        assert_eq!(filter_drag_key(7, 3), "pw7:filter#3");
        assert_eq!(filter_wheel_key(7), "pw7:filter:res");
        assert_eq!(env_prefix(7, "amp"), "pw7:amp");
    }

    /// Frames of the synth graph in a headless 700x600 context, one per entry of `frames` (the events of that
    /// frame). Returns the boxes and every edit asked for.
    fn run_frames(inst: &PluginInstance, gesture: &mut u64, frames: &[Vec<Event>]) -> (SynthLayout, Vec<ParamsEdit>) {
        let ctx = egui::Context::default();
        theme::apply(&ctx);
        let mut layout = SynthLayout::default();
        let mut all = Vec::new();
        for (k, events) in frames.iter().enumerate() {
            let raw = egui::RawInput {
                screen_rect: Some(Rect::from_min_size(Pos2::ZERO, vec2(700.0, 600.0))),
                time: Some(k as f64 / 60.0),
                events: events.clone(),
                ..Default::default()
            };
            let _ = ctx.run(raw, |ctx| {
                egui::CentralPanel::default().show(ctx, |ui| {
                    let out = synth_graph(ui, inst, 48000.0, gesture);
                    layout = out.layout;
                    all.extend(out.edits);
                });
            });
        }
        (layout, all)
    }

    fn harness_layout(inst: &PluginInstance) -> SynthLayout {
        run_frames(inst, &mut 0, &[vec![]]).0
    }

    fn press(pos: Pos2, button: PointerButton, pressed: bool) -> Event {
        Event::PointerButton { pos, button, pressed, modifiers: Modifiers::NONE }
    }

    /// Press at `p`, three moves of `step`, release.
    fn drag_frames(p: Pos2, step: Vec2) -> Vec<Vec<Event>> {
        let mut frames = vec![vec![], vec![Event::PointerMoved(p)], vec![press(p, PointerButton::Primary, true)]];
        for k in 1..=3 {
            frames.push(vec![Event::PointerMoved(p + step * k as f32)]);
        }
        frames.push(vec![press(p + step * 3.0, PointerButton::Primary, false)]);
        frames
    }

    #[test]
    fn synth_graph_draws_both_synths_without_edits() {
        let extremes: [&[(&str, f32)]; 3] = [
            &[],
            &[("cutoff", 20.0), ("resonance", 1.0), ("attack", 0.001), ("decay", 0.001), ("sustain", 0.0), ("release", 0.001), ("f_attack", 0.001), ("f_sustain", 0.0), ("filter_env", -1.0), ("index_decay", 0.01), ("index_sustain", 0.0)],
            &[("cutoff", 20000.0), ("resonance", 1.0), ("attack", 5.0), ("decay", 8.0), ("sustain", 1.0), ("release", 8.0), ("f_release", 5.0), ("f_sustain", 1.0), ("filter_env", 1.0), ("osc1_wave", 4.0), ("osc2_wave", 1.0), ("index_decay", 5.0), ("index_sustain", 1.0)],
        ];
        for kind in ["analog_synth", "fm_synth"] {
            for kv in extremes {
                let inst = with(instance(kind), kv);
                let mut g = 0;
                let (layout, edits) = run_frames(&inst, &mut g, &[vec![], vec![]]);
                assert!(edits.is_empty(), "{kind}: {edits:?}");
                assert_eq!(g, 0);
                assert!(layout.env1.is_positive() && layout.env2.is_positive(), "{kind}: {layout:?}");
                if kind == "analog_synth" {
                    assert!(layout.osc1.is_positive() && layout.osc2.is_positive() && layout.filter.is_positive(), "{layout:?}");
                } else {
                    assert!(!layout.filter.is_positive());
                }
            }
        }
    }

    #[test]
    fn dragging_the_amp_decay_point_sends_one_gesture() {
        let inst = instance("analog_synth");
        let layout = harness_layout(&inst);
        let p = env_handle_pos(&amp_env(&inst, theme::ACCENT), EnvHandle::Decay, env_plot(layout.env1));
        let mut g = 0;
        let (_, edits) = run_frames(&inst, &mut g, &drag_frames(p, vec2(6.0, -5.0)));
        assert!(!edits.is_empty());
        assert_eq!(g, 1);
        for e in &edits {
            assert!(matches!(e, ParamsEdit::Drag { key, .. } if key == "pw3:amp:D#1"), "{e:?}");
        }
        let ParamsEdit::Drag { params, .. } = edits.last().unwrap() else { unreachable!() };
        assert_eq!(params.keys().collect::<Vec<_>>(), ["decay", "sustain"]);
    }

    #[test]
    fn dragging_the_filter_point_sends_one_gesture() {
        let inst = instance("analog_synth");
        let layout = harness_layout(&inst);
        let p = filter_handle(&inst, filter_plot(layout.filter));
        let mut g = 0;
        let (_, edits) = run_frames(&inst, &mut g, &drag_frames(p, vec2(-8.0, -4.0)));
        assert!(!edits.is_empty());
        assert_eq!(g, 1);
        for e in &edits {
            assert!(matches!(e, ParamsEdit::Drag { key, .. } if key == "pw3:filter#1"), "{e:?}");
        }
        let ParamsEdit::Drag { params, .. } = edits.last().unwrap() else { unreachable!() };
        assert_eq!(params.keys().collect::<Vec<_>>(), ["cutoff", "resonance"]);
        assert!(params["cutoff"] < 2500.0 && params["resonance"] > 0.2, "{params:?}");
    }

    #[test]
    fn wheel_over_the_filter_changes_resonance() {
        let inst = instance("analog_synth");
        let layout = harness_layout(&inst);
        let wheel = Event::MouseWheel { unit: MouseWheelUnit::Line, delta: vec2(0.0, 1.0), modifiers: Modifiers::NONE };
        let somewhere = layout.filter.left_top() + vec2(20.0, 20.0);
        let mut g = 0;
        let (_, edits) = run_frames(&inst, &mut g, &[vec![], vec![Event::PointerMoved(somewhere)], vec![wheel]]);
        assert_eq!(edits.len(), 1, "{edits:?}");
        let ParamsEdit::Drag { params, key } = &edits[0] else { panic!("{edits:?}") };
        assert_eq!(key, "pw3:filter:res");
        assert!(near(params["resonance"], 0.3, 1e-3), "{params:?}");
        assert_eq!(g, 0);
    }

    #[test]
    fn clicking_an_oscillator_box_changes_its_shape() {
        let inst = instance("analog_synth");
        let layout = harness_layout(&inst);
        let c1 = layout.osc1.center();
        let mut g = 0;
        let (_, edits) = run_frames(&inst, &mut g, &[vec![], vec![Event::PointerMoved(c1)], vec![press(c1, PointerButton::Primary, true)], vec![press(c1, PointerButton::Primary, false)]]);
        assert_eq!(edits, [ParamsEdit::Set { params: BTreeMap::from([("osc1_wave".to_string(), 1.0)]) }]);
        let c2 = layout.osc2.center();
        let (_, edits) = run_frames(&inst, &mut g, &[vec![], vec![Event::PointerMoved(c2)], vec![press(c2, PointerButton::Secondary, true)], vec![press(c2, PointerButton::Secondary, false)]]);
        assert_eq!(edits, [ParamsEdit::Set { params: BTreeMap::from([("osc2_wave".to_string(), 4.0)]) }]);
        let wheel = Event::MouseWheel { unit: MouseWheelUnit::Line, delta: vec2(0.0, -1.0), modifiers: Modifiers::NONE };
        let (_, edits) = run_frames(&inst, &mut g, &[vec![], vec![Event::PointerMoved(c2)], vec![wheel]]);
        assert_eq!(edits, [ParamsEdit::Drag { params: BTreeMap::from([("osc2_wave".to_string(), 1.0)]), key: "pw3:osc2:wave".into() }]);
        assert_eq!(g, 0);
    }

    #[test]
    fn fm_index_envelope_has_no_attack_handle() {
        let inst = instance("fm_synth");
        let layout = harness_layout(&inst);
        let spec = index_env(&inst, theme::MONITOR);
        let p = env_handle_pos(&spec, EnvHandle::Attack, env_plot(layout.env2));
        let mut g = 0;
        let (_, edits) = run_frames(&inst, &mut g, &drag_frames(p, vec2(10.0, 0.0)));
        assert!(edits.is_empty(), "{edits:?}");
        assert_eq!(g, 0);
        let d = env_handle_pos(&spec, EnvHandle::Decay, env_plot(layout.env2));
        let (_, edits) = run_frames(&inst, &mut g, &drag_frames(d, vec2(10.0, -5.0)));
        assert!(edits.iter().all(|e| matches!(e, ParamsEdit::Drag { key, params } if key == "pw3:index:D#1" && params.keys().all(|k| k == "index_decay" || k == "index_sustain"))), "{edits:?}");
        assert!(!edits.is_empty());
    }

    #[test]
    fn a_change_in_the_project_changes_the_drawing() {
        let p = plot();
        let before = instance("analog_synth");
        let after = with(before.clone(), &[("attack", 1.2), ("cutoff", 600.0), ("resonance", 0.7)]);
        let freqs = log_freqs(64);
        assert_ne!(filter_handle(&before, p), filter_handle(&after, p));
        assert_ne!(filter_curve(&before, 2500.0, 48000.0, p, &freqs), filter_curve(&after, 600.0, 48000.0, p, &freqs));
        let (a, b) = (amp_env(&before, theme::ACCENT), amp_env(&after, theme::ACCENT));
        assert!(env_handle_pos(&b, EnvHandle::Attack, p).x > env_handle_pos(&a, EnvHandle::Attack, p).x);
        assert_ne!(crate::widgets::env_curve(&a, p, 16), crate::widgets::env_curve(&b, p, 16));
        let k = freqs.iter().position(|f| *f > 590.0).unwrap();
        let drawn = filter_curve(&after, 600.0, 48000.0, p, &freqs)[k].y;
        assert!(near(drawn, filter_db_to_y(analog_filter_db(0.0, 600.0, 0.7, freqs[k], 48000.0), p), 1e-3));
    }

    #[test]
    fn the_synth_view_reuses_the_common_header() {
        let src = include_str!("synth_view.rs");
        assert!(!src.contains(concat!("plugin_ai_bar", "(")));
        assert!(!src.contains(concat!("plugin_header", "(")));
    }

    /// Frames of the instrument keyboard in a headless 800x600 context; `focused` is the window's focus in each
    /// frame. Returns where the keyboard started, the bottom of what the panel reserved, and every event sent.
    fn keys_frames(keys: &mut KeyboardState, frames: &[(bool, Vec<Event>)]) -> (Pos2, f32, Vec<MidiKind>) {
        let ctx = egui::Context::default();
        theme::apply(&ctx);
        let (mut start, mut after, mut all) = (Pos2::ZERO, 0.0, Vec::new());
        for (k, (focused, events)) in frames.iter().enumerate() {
            let mut raw = egui::RawInput {
                screen_rect: Some(Rect::from_min_size(Pos2::ZERO, vec2(800.0, 600.0))),
                time: Some(k as f64 / 60.0),
                events: events.clone(),
                ..Default::default()
            };
            raw.viewports.entry(egui::ViewportId::ROOT).or_default().focused = Some(*focused);
            let _ = ctx.run(raw, |ctx| {
                egui::CentralPanel::default().show(ctx, |ui| {
                    start = ui.available_rect_before_wrap().min;
                    all.extend(instrument_keys_frame(ui, keys, true));
                    after = ui.min_rect().bottom();
                });
            });
        }
        (start, after, all)
    }

    #[test]
    fn keyboard_stays_drawn_without_focus_and_releases_the_held_key() {
        use crate::widgets::{keyboard_keys_rect, KEYBOARD_H, KEYBOARD_ROW_H};
        let mut keys = KeyboardState { octave: 3, held: Some((60, 100)) };
        let (start, after, events) = keys_frames(&mut keys, &[(false, vec![])]);
        assert_eq!(events, [MidiKind::NoteOff { pitch: 60 }]);
        assert_eq!(keys.held, None);
        assert!(after - start.y >= KEYBOARD_ROW_H + KEYBOARD_H, "the keyboard keeps its place without focus");

        // the first click after the window gets its focus back plays
        let key = keyboard_keys_rect(start, 800.0 - 2.0 * start.x).left_bottom() + vec2(10.0, -5.0);
        let mut keys = KeyboardState { octave: 3, held: None };
        let frames = [(false, vec![]), (false, vec![Event::PointerMoved(key)]), (true, vec![press(key, PointerButton::Primary, true)]), (true, vec![press(key, PointerButton::Primary, false)])];
        let (_, _, events) = keys_frames(&mut keys, &frames);
        assert!(matches!(events.first(), Some(MidiKind::NoteOn { pitch: 48, .. })), "{events:?}");
        assert_eq!(events.last(), Some(&MidiKind::NoteOff { pitch: 48 }));
    }
}
