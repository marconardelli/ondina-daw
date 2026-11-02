//! The response graph of the EQ windows (Channel EQ, Dynamic EQ): curve, band points to drag, Q on the wheel. The
//! math is plain functions the tests cover; eq_graph draws them and turns the mouse into edits.

use std::collections::BTreeMap;

use eframe::egui::{self, Color32, CursorIcon, PointerButton, Pos2, Rect, Sense, Stroke, Vec2};
use ondina_core::command::Command;
use ondina_core::model::{Id, PluginInstance};
use ondina_core::plugins;
use ondina_dsp::effects::{channel_eq_filters, channel_eq_params, channel_eq_response_db, dynamic_eq_filters, dynamic_eq_params, dynamic_eq_response_db};
use ondina_dsp::util::Biquad;

use crate::app::OndinaApp;
use crate::theme;
use crate::widgets::{format_param_value, unit_to_value_log, value_to_unit_log};

pub const F_LO: f32 = 20.0;
pub const F_HI: f32 = 20000.0;
/// The graph shows gains from -DB_SPAN to +DB_SPAN.
pub const DB_SPAN: f32 = 24.0;
pub const POINT_R: f32 = 6.0;
/// Half the side of the square around a point that takes the mouse.
pub const HIT_R: f32 = 10.0;
/// Wheel points (egui: 40 per notch) that double the Q; with Shift ten times as many.
pub const Q_WHEEL_POINTS: f32 = 160.0;
/// Shift: drags and wheel turns move ten times less.
pub const FINE: f32 = 0.1;
/// Colors of the band points and curves, in band order (theme tokens, no new colors).
const BAND_COLORS: [Color32; 6] = [theme::MUTE, theme::PLAY, theme::SOLO, theme::MONITOR, theme::AI, theme::ACCENT];
/// Lowest and highest Q the wheel reaches (the registry range of every Q).
const Q_MIN: f32 = 0.1;
const Q_MAX: f32 = 10.0;

/// One draggable point of an EQ window and the parameters it moves.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct EqBand {
    /// Index in the DSP's filters (`channel_eq_filters` / `dynamic_eq_filters`); also picks the color.
    pub index: usize,
    pub name: &'static str,
    pub freq: &'static str,
    /// None: the point moves only sideways (High Pass, Low Pass).
    pub gain: Option<&'static str>,
    /// None: the wheel does nothing (shelves, cut filters).
    pub q: Option<&'static str>,
    /// The band's on switch; None when the band has none (it runs when its gain is not 0).
    pub on: Option<&'static str>,
    pub kind_param: Option<&'static str>,
    pub threshold: Option<&'static str>,
    pub range: Option<&'static str>,
}

const fn ceq_band(index: usize, name: &'static str, freq: &'static str, gain: Option<&'static str>, q: Option<&'static str>, on: Option<&'static str>) -> EqBand {
    EqBand { index, name, freq, gain, q, on, kind_param: None, threshold: None, range: None }
}

const CHANNEL_EQ_BANDS: [EqBand; 6] = [
    ceq_band(0, "High Pass", "hp_freq", None, None, Some("hp_on")),
    ceq_band(1, "Low Shelf", "low_freq", Some("low_gain"), None, None),
    ceq_band(2, "Mid 1", "mid1_freq", Some("mid1_gain"), Some("mid1_q"), None),
    ceq_band(3, "Mid 2", "mid2_freq", Some("mid2_gain"), Some("mid2_q"), None),
    ceq_band(4, "High Shelf", "high_freq", Some("high_gain"), None, None),
    ceq_band(5, "Low Pass", "lp_freq", None, None, Some("lp_on")),
];

const DYNAMIC_EQ_BANDS: [EqBand; 4] = [
    EqBand { index: 0, name: "Band 1", freq: "b1_freq", gain: Some("b1_gain"), q: Some("b1_q"), on: Some("b1_on"), kind_param: Some("b1_type"), threshold: Some("b1_threshold"), range: Some("b1_range") },
    EqBand { index: 1, name: "Band 2", freq: "b2_freq", gain: Some("b2_gain"), q: Some("b2_q"), on: Some("b2_on"), kind_param: Some("b2_type"), threshold: Some("b2_threshold"), range: Some("b2_range") },
    EqBand { index: 2, name: "Band 3", freq: "b3_freq", gain: Some("b3_gain"), q: Some("b3_q"), on: Some("b3_on"), kind_param: Some("b3_type"), threshold: Some("b3_threshold"), range: Some("b3_range") },
    EqBand { index: 3, name: "Band 4", freq: "b4_freq", gain: Some("b4_gain"), q: Some("b4_q"), on: Some("b4_on"), kind_param: Some("b4_type"), threshold: Some("b4_threshold"), range: Some("b4_range") },
];

/// Whether plug-in kind `kind` has the EQ graph.
pub fn has_eq_view(kind: &str) -> bool {
    matches!(kind, "channel_eq" | "dynamic_eq")
}

/// Every band of kind `kind`, unfiltered.
fn band_table(kind: &str) -> &'static [EqBand] {
    match kind {
        "channel_eq" => &CHANNEL_EQ_BANDS,
        "dynamic_eq" => &DYNAMIC_EQ_BANDS,
        _ => &[],
    }
}

/// Whether a Dynamic EQ band is a bell now (type 0, rounded like the processor).
fn is_bell(inst: &PluginInstance, band: &EqBand) -> bool {
    band.kind_param.is_none_or(|k| inst.param(k).round() == 0.0)
}

/// The points of `inst`, in band order. In the Dynamic EQ a band's `q` is None unless its type is Bell.
pub fn eq_bands(inst: &PluginInstance) -> Vec<EqBand> {
    band_table(&inst.kind).iter().map(|b| EqBand { q: if is_bell(inst, b) { b.q } else { None }, ..*b }).collect()
}

/// Whether the band is on: its switch is up, or it has no switch.
pub fn band_on(inst: &PluginInstance, band: &EqBand) -> bool {
    band.on.is_none_or(|id| inst.param(id) >= 0.5)
}

/// Frequency and gain of the point (0 dB for High Pass and Low Pass).
pub fn band_point(inst: &PluginInstance, band: &EqBand) -> (f32, f32) {
    (inst.param(band.freq), band.gain.map_or(0.0, |g| inst.param(g)))
}

/// The color of band `index`'s point and curve.
pub fn band_color(index: usize) -> Color32 {
    BAND_COLORS[index % BAND_COLORS.len()]
}

/// The registry range of parameter `id` of `kind` ((0, 1) if unknown).
fn range_of(kind: &str, id: &str) -> (f32, f32) {
    plugins::find(kind).and_then(|i| i.params.iter().find(|p| p.id == id)).map_or((0.0, 1.0), |p| (p.min, p.max))
}

// ---------------------------------------------------------------------------- scales

pub fn freq_to_x(f: f32, plot: Rect) -> f32 {
    plot.left() + value_to_unit_log(f, F_LO, F_HI) * plot.width()
}

/// The frequency at `x`, clamped to F_LO..F_HI.
pub fn x_to_freq(x: f32, plot: Rect) -> f32 {
    let u = if plot.width() > 0.0 { (x - plot.left()) / plot.width() } else { 0.0 };
    unit_to_value_log(u, F_LO, F_HI)
}

/// +DB_SPAN at the top, -DB_SPAN at the bottom, clamped to the plot.
pub fn db_to_y(db: f32, plot: Rect) -> f32 {
    (plot.center().y - db / DB_SPAN * plot.height() / 2.0).clamp(plot.top(), plot.bottom())
}

pub fn y_to_db(y: f32, plot: Rect) -> f32 {
    if plot.height() <= 0.0 {
        return 0.0;
    }
    (plot.center().y - y) / (plot.height() / 2.0) * DB_SPAN
}

pub fn point_pos(freq: f32, db: f32, plot: Rect) -> Pos2 {
    egui::pos2(freq_to_x(freq, plot), db_to_y(db, plot))
}

/// `n` frequencies from F_LO to F_HI (both included), evenly spaced on the log axis.
pub fn log_freqs(n: usize) -> Vec<f32> {
    match n {
        0 => Vec::new(),
        1 => vec![F_LO],
        _ => (0..n).map(|i| F_LO * (F_HI / F_LO).powf(i as f32 / (n - 1) as f32)).collect(),
    }
}

/// How tall the graph is in a window with `available` points of height left under the header.
pub fn graph_height(available: f32) -> f32 {
    (available * 0.5).clamp(150.0, 300.0)
}

/// The sample rate the curve is computed at: the engine's, or 48 kHz before it reports one.
pub fn curve_sample_rate(sr: u32) -> f32 {
    if sr >= 8000 {
        sr as f32
    } else {
        48000.0
    }
}

// ---------------------------------------------------------------------------- curves

/// The filter of every band of `inst` (None for a band that does nothing), with `extra` dB on Dynamic EQ bands.
fn filters(inst: &PluginInstance, sr: f32, extra: &[f32; 4]) -> Vec<Option<Biquad>> {
    match inst.kind.as_str() {
        "channel_eq" => channel_eq_filters(&channel_eq_params(|id| inst.param(id)), sr).to_vec(),
        "dynamic_eq" => dynamic_eq_filters(&dynamic_eq_params(|id| inst.param(id)), sr, extra).to_vec(),
        _ => Vec::new(),
    }
}

/// Gain of the whole plug-in at each of `freqs` (dB).
pub fn total_curve(inst: &PluginInstance, sr: f32, freqs: &[f32]) -> Vec<f32> {
    match inst.kind.as_str() {
        "channel_eq" => {
            let p = channel_eq_params(|id| inst.param(id));
            freqs.iter().map(|f| channel_eq_response_db(&p, *f, sr)).collect()
        }
        "dynamic_eq" => {
            let p = dynamic_eq_params(|id| inst.param(id));
            freqs.iter().map(|f| dynamic_eq_response_db(&p, *f, sr, &[0.0; 4])).collect()
        }
        _ => vec![0.0; freqs.len()],
    }
}

fn curve_of(b: &Biquad, sr: f32, freqs: &[f32]) -> Vec<f32> {
    freqs.iter().map(|f| b.magnitude_db(*f, sr)).collect()
}

/// Gain of band `band` alone at each of `freqs`; None when the band does nothing now.
pub fn band_curve(inst: &PluginInstance, band: &EqBand, sr: f32, freqs: &[f32]) -> Option<Vec<f32>> {
    let b = filters(inst, sr, &[0.0; 4]).get(band.index).copied().flatten()?;
    Some(curve_of(&b, sr, freqs))
}

/// Dynamic EQ: the band at its Gain and at Gain+Range, for the shaded area; None when the band is off, its Range is
/// 0, or the plug-in is not a Dynamic EQ.
pub fn range_area(inst: &PluginInstance, band: &EqBand, sr: f32, freqs: &[f32]) -> Option<(Vec<f32>, Vec<f32>)> {
    if inst.kind != "dynamic_eq" || band.index >= 4 {
        return None;
    }
    let p = dynamic_eq_params(|id| inst.param(id));
    let range = p[band.index * 7 + 6];
    if range == 0.0 {
        return None;
    }
    let mut extra = [0.0; 4];
    extra[band.index] = range;
    let at_gain = dynamic_eq_filters(&p, sr, &[0.0; 4])[band.index]?;
    let at_range = dynamic_eq_filters(&p, sr, &extra)[band.index]?;
    Some((curve_of(&at_gain, sr, freqs), curve_of(&at_range, sr, freqs)))
}

// ---------------------------------------------------------------------------- interaction

/// Pointer movement since the drag started, Shift moving ten times less.
pub fn accumulate(moved: Vec2, delta: Vec2, fine: bool) -> Vec2 {
    moved + if fine { delta * FINE } else { delta }
}

/// The band's values after a drag that started at `start` (frequency, gain) and has moved `moved` points: the
/// frequency follows x on the log axis, the gain follows y; each is clamped to its own parameter's range.
/// Only the parameters the band has (High Pass and Low Pass: frequency only).
pub fn drag_values(inst: &PluginInstance, band: &EqBand, start: (f32, f32), moved: Vec2, plot: Rect) -> BTreeMap<String, f32> {
    let mut out = BTreeMap::new();
    let (lo, hi) = range_of(&inst.kind, band.freq);
    let f = x_to_freq(freq_to_x(start.0, plot) + moved.x, plot).clamp(lo, hi);
    out.insert(band.freq.to_string(), f);
    if let Some(g) = band.gain {
        let (lo, hi) = range_of(&inst.kind, g);
        let db = y_to_db(db_to_y(start.1, plot) + moved.y, plot).clamp(lo, hi);
        out.insert(g.to_string(), db);
    }
    out
}

/// Q after a wheel turn of `scroll` points (x and y summed: egui turns Shift+wheel into x), clamped to 0.1..10;
/// wheel up (positive) narrows. A non-finite turn leaves it as it is.
pub fn wheel_q(q: f32, scroll: Vec2, fine: bool) -> f32 {
    let s = scroll.x + scroll.y;
    if !s.is_finite() {
        return q;
    }
    (q * 2f32.powf(s * if fine { FINE } else { 1.0 } / Q_WHEEL_POINTS)).clamp(Q_MIN, Q_MAX)
}

/// Frequency, gain and Q of band `index` back to their defaults (`plugins::default_params`), as one change.
pub fn band_reset(inst: &PluginInstance, index: usize) -> BTreeMap<String, f32> {
    let Some(band) = band_table(&inst.kind).get(index) else {
        return BTreeMap::new();
    };
    let defaults = plugins::default_params(inst);
    [Some(band.freq), band.gain, band.q].into_iter().flatten().filter_map(|id| defaults.get(id).map(|v| (id.to_string(), *v))).collect()
}

/// The band's switch flipped; None when the band has no switch.
pub fn band_toggle(inst: &PluginInstance, band: &EqBand) -> Option<BTreeMap<String, f32>> {
    let id = band.on?;
    let v = if band_on(inst, band) { 0.0 } else { 1.0 };
    Some(BTreeMap::from([(id.to_string(), v)]))
}

/// "Mid 1 · 400 Hz · +3.0 dB · Q 1.00"; see the tests for each kind of band.
pub fn band_readout(inst: &PluginInstance, band: &EqBand) -> String {
    let mut parts: Vec<String> = vec![band.name.to_string()];
    if let Some(k) = band.kind_param {
        let choices = plugins::find(&inst.kind).and_then(|i| i.params.iter().find(|p| p.id == k)).map_or(&[][..], |p| p.choices);
        let v = inst.param(k).round().max(0.0) as usize;
        if let Some(name) = choices.get(v) {
            parts.push(name.to_string());
        }
    }
    parts.push(format_param_value(inst.param(band.freq), "Hz"));
    if let Some(g) = band.gain {
        parts.push(format_param_value(inst.param(g), "dB"));
    }
    if let (Some(q), true) = (band.q, is_bell(inst, band)) {
        parts.push(format!("Q {:.2}", inst.param(q)));
    }
    if let (Some(r), Some(t)) = (band.range, band.threshold) {
        let range = inst.param(r);
        if range != 0.0 {
            parts.push(format!("Range {} above {}", format_param_value(range, "dB"), format_param_value(inst.param(t), "dB")));
        }
    }
    if !band_on(inst, band) {
        parts.push("off".to_string());
    }
    parts.join(" \u{00B7} ")
}

/// Where the readout of a point at `point` goes: centered above it, below when there is no room above, kept
/// inside `plot` by SP_1.
pub fn readout_rect(point: Pos2, size: Vec2, plot: Rect) -> Rect {
    let gap = POINT_R + theme::SP_3;
    let mut y = point.y - gap - size.y;
    if y < plot.top() {
        y = point.y + gap;
    }
    let inner = plot.shrink(theme::SP_1);
    let x = (point.x - size.x / 2.0).min(inner.right() - size.x).max(inner.left());
    Rect::from_min_size(egui::pos2(x, y), size)
}

/// The undo key of a drag of band `band` in plug-in `pid`'s window: one key per gesture.
pub fn drag_key(pid: Id, band: usize, gesture: u64) -> String {
    format!("pw{pid}:eq{band}#{gesture}")
}

/// The undo key of the wheel over band `band`: turns close together merge.
pub fn wheel_key(pid: Id, band: usize) -> String {
    format!("pw{pid}:eq{band}:q")
}

// ---------------------------------------------------------------------------- drawing and mouse

/// What a frame of the EQ graph asks for.
#[derive(Debug, Clone, PartialEq)]
pub enum EqEdit {
    /// Part of a drag or of a wheel turn: merged into one undo step by `key`.
    Drag { params: BTreeMap<String, f32>, key: String },
    /// A single change (double-click reset, right-click on/off).
    Set { params: BTreeMap<String, f32> },
}

pub struct EqGraphOut {
    /// The plot area (tests use it to find the points; the window has no use for it).
    #[cfg(test)]
    pub plot: Rect,
    pub edits: Vec<EqEdit>,
}

/// The drag going on in a graph, kept in egui's memory between frames.
#[derive(Clone, Copy)]
struct EqDrag {
    band: usize,
    start: (f32, f32),
    moved: Vec2,
}

/// Whether `params` would change any value of `inst`.
fn changes(inst: &PluginInstance, params: &BTreeMap<String, f32>) -> bool {
    params.iter().any(|(k, v)| inst.param(k) != *v)
}

/// Fills the area between two curves sampled at the same `xs` (screen coordinates): two triangles per pair of
/// neighbouring samples, so the curve need not be convex.
fn fill_between(painter: &egui::Painter, xs: &[f32], top: &[f32], bottom: &[f32], color: Color32) {
    let n = xs.len().min(top.len()).min(bottom.len());
    if n < 2 {
        return;
    }
    let mut mesh = egui::Mesh::default();
    for i in 0..n {
        mesh.colored_vertex(egui::pos2(xs[i], top[i]), color);
        mesh.colored_vertex(egui::pos2(xs[i], bottom[i]), color);
    }
    for i in 0..n as u32 - 1 {
        let k = 2 * i;
        mesh.add_triangle(k, k + 1, k + 2);
        mesh.add_triangle(k + 1, k + 3, k + 2);
    }
    painter.add(egui::Shape::mesh(mesh));
}

fn faded(c: Color32, alpha: u8) -> Color32 {
    Color32::from_rgba_unmultiplied(c.r(), c.g(), c.b(), alpha)
}

/// Grid frequencies: 20, 30 ... 90, 100, 200 ... 20000 Hz.
fn grid_freqs() -> impl Iterator<Item = f32> {
    [10.0f32, 100.0, 1000.0, 10000.0].into_iter().flat_map(|d| (1..10).map(move |k| d * k as f32)).filter(|f| (F_LO..=F_HI).contains(f)).chain(std::iter::once(F_HI))
}

/// Draws the response graph of EQ plug-in `inst` across the available width, `height` points tall, and turns what
/// the mouse does on its points into edits. `gesture` is the window's drag counter (`PluginWindows::gesture`).
pub fn eq_graph(ui: &mut egui::Ui, inst: &PluginInstance, sr: f32, height: f32, gesture: &mut u64) -> EqGraphOut {
    // the background first, hover only: the points registered after it take the clicks
    let (rect, bg) = ui.allocate_exact_size(egui::vec2(ui.available_width(), height), Sense::hover());
    let plot = rect;
    let bands = eq_bands(inst);
    let drag_id = egui::Id::new(("eq-drag", inst.id));
    let mut drag: Option<EqDrag> = ui.data(|d| d.get_temp(drag_id));
    let fine = ui.input(|i| i.modifiers.shift);
    let mut edits = Vec::new();
    let mut hot: Option<usize> = None;
    let positions: Vec<Pos2> = bands.iter().map(|b| {
        let (f, g) = band_point(inst, b);
        point_pos(f, g, plot)
    }).collect();
    let responses: Vec<egui::Response> = bands
        .iter()
        .zip(&positions)
        .map(|(b, pos)| ui.interact(Rect::from_center_size(*pos, Vec2::splat(2.0 * HIT_R)), ui.id().with(("eq-point", b.index)), Sense::click_and_drag()))
        .collect();
    for (b, r) in bands.iter().zip(&responses) {
        if r.drag_started_by(PointerButton::Primary) {
            *gesture += 1;
            drag = Some(EqDrag { band: b.index, start: band_point(inst, b), moved: Vec2::ZERO });
        }
        if r.dragged_by(PointerButton::Primary) {
            if let Some(d) = drag.as_mut().filter(|d| d.band == b.index) {
                d.moved = accumulate(d.moved, r.drag_delta(), fine);
                let params = drag_values(inst, b, d.start, d.moved, plot);
                if changes(inst, &params) {
                    edits.push(EqEdit::Drag { params, key: drag_key(inst.id, b.index, *gesture) });
                }
                hot = Some(b.index);
                ui.ctx().set_cursor_icon(CursorIcon::Grabbing);
            }
        }
        if r.drag_stopped() {
            drag = None;
        }
        if r.double_clicked() {
            edits.push(EqEdit::Set { params: band_reset(inst, b.index) });
        }
        if r.secondary_clicked() {
            if let Some(params) = band_toggle(inst, b) {
                edits.push(EqEdit::Set { params });
            }
        }
        if r.hovered() {
            if hot.is_none() {
                hot = Some(b.index);
            }
            if !r.dragged() {
                ui.ctx().set_cursor_icon(CursorIcon::Grab);
            }
            if let Some(q_id) = b.q {
                let s = ui.input(|i| i.raw_scroll_delta);
                if s != Vec2::ZERO {
                    let q = wheel_q(inst.param(q_id), s, fine);
                    if q != inst.param(q_id) {
                        edits.push(EqEdit::Drag { params: BTreeMap::from([(q_id.to_string(), q)]), key: wheel_key(inst.id, b.index) });
                    }
                    // the wheel was for this point: nothing else scrolls with it
                    ui.input_mut(|i| {
                        i.raw_scroll_delta = Vec2::ZERO;
                        i.smooth_scroll_delta = Vec2::ZERO;
                    });
                }
            }
        }
    }
    if let Some(d) = drag.filter(|d| hot.is_none() && responses.iter().any(|r| r.dragged()) && d.band < bands.len()) {
        hot = Some(d.band);
    }
    ui.data_mut(|m| match drag {
        Some(d) => m.insert_temp(drag_id, d),
        None => m.remove::<EqDrag>(drag_id),
    });

    let painter = ui.painter_at(rect);
    painter.rect(rect, theme::R_LG, theme::INSET, Stroke::new(1.0_f32, theme::BORDER));
    for f in grid_freqs() {
        let x = freq_to_x(f, plot);
        let decade = [100.0, 1000.0, 10000.0].contains(&f);
        painter.vline(x, plot.y_range(), Stroke::new(1.0_f32, if decade { theme::GRID_BAR_LINE } else { theme::GRID_BEAT_LINE }));
    }
    for (f, label) in [(50.0, "50"), (100.0, "100"), (200.0, "200"), (500.0, "500"), (1000.0, "1k"), (2000.0, "2k"), (5000.0, "5k"), (10000.0, "10k")] {
        painter.text(egui::pos2(freq_to_x(f, plot), plot.bottom() - theme::SP_1), egui::Align2::CENTER_BOTTOM, label, theme::label_font(), theme::TEXT_3);
    }
    let mut db = -DB_SPAN + 6.0;
    while db < DB_SPAN {
        let y = db_to_y(db, plot);
        painter.hline(plot.x_range(), y, Stroke::new(1.0_f32, if db == 0.0 { theme::CENTER_LINE } else { theme::GRID_BEAT_LINE }));
        if [12.0, 0.0, -12.0].contains(&db) {
            let label = if db > 0.0 { format!("+{db:.0}") } else { format!("{db:.0}") };
            painter.text(egui::pos2(plot.left() + theme::SP_3, y - theme::SP_1), egui::Align2::LEFT_BOTTOM, label, theme::label_font(), theme::TEXT_3);
        }
        db += 6.0;
    }
    let n = (plot.width() / 2.0).clamp(64.0, 400.0) as usize;
    let freqs = log_freqs(n);
    let xs: Vec<f32> = freqs.iter().map(|f| freq_to_x(*f, plot)).collect();
    let to_y = |c: &[f32]| -> Vec<f32> { c.iter().map(|d| db_to_y(*d, plot)).collect() };
    let zero = vec![db_to_y(0.0, plot); n];
    for b in &bands {
        if let Some(c) = band_curve(inst, b, sr, &freqs) {
            fill_between(&painter, &xs, &to_y(&c), &zero, faded(band_color(b.index), 40));
        }
    }
    for (b, pos) in bands.iter().zip(&positions) {
        if let Some((at_gain, at_range)) = range_area(inst, b, sr, &freqs) {
            let color = band_color(b.index);
            fill_between(&painter, &xs, &to_y(&at_gain), &to_y(&at_range), faded(color, 60));
            let reach = b.gain.map_or(0.0, |g| inst.param(g)) + b.range.map_or(0.0, |r| inst.param(r));
            let y = db_to_y(reach, plot);
            painter.hline(pos.x - HIT_R..=pos.x + HIT_R, y, Stroke::new(1.0_f32, color));
        }
    }
    let total = total_curve(inst, sr, &freqs);
    let line: Vec<Pos2> = xs.iter().zip(&total).map(|(x, d)| egui::pos2(*x, db_to_y(*d, plot))).collect();
    painter.add(egui::Shape::line(line, Stroke::new(2.0_f32, if inst.bypass { theme::TEXT_DISABLED } else { theme::TEXT })));
    for (b, pos) in bands.iter().zip(&positions) {
        if band_on(inst, b) {
            painter.circle_filled(*pos, POINT_R, band_color(b.index));
        } else {
            painter.circle_stroke(*pos, POINT_R, Stroke::new(1.5_f32, theme::TEXT_DISABLED));
        }
        if hot == Some(b.index) {
            painter.circle_stroke(*pos, POINT_R + 2.0, Stroke::new(1.5_f32, theme::TEXT));
        }
    }
    if let Some(b) = hot.and_then(|i| bands.iter().find(|b| b.index == i)) {
        let text = band_readout(inst, b);
        let galley = painter.layout_no_wrap(text, theme::number_font(), theme::TEXT);
        let size = galley.size() + Vec2::splat(2.0 * theme::SP_3);
        let pos = positions[bands.iter().position(|x| x.index == b.index).unwrap_or(0)];
        let r = readout_rect(pos, size, plot);
        painter.rect(r, theme::R_LG, theme::RAISED, Stroke::new(1.0_f32, theme::BORDER));
        painter.galley(r.min + Vec2::splat(theme::SP_3), galley, theme::TEXT);
    } else if let Some(p) = bg.hover_pos() {
        let text = format_param_value(x_to_freq(p.x, plot), "Hz");
        painter.text(egui::pos2(plot.right() - theme::SP_3, plot.top() + theme::SP_3), egui::Align2::RIGHT_TOP, text, theme::number_font(), theme::TEXT_3);
    }
    ui.label(egui::RichText::new("Drag a point: frequency and gain (Shift: fine) \u{00B7} Wheel: Q \u{00B7} Double-click: default \u{00B7} Right-click: on/off").small().color(theme::TEXT_3));
    EqGraphOut {
        #[cfg(test)]
        plot,
        edits,
    }
}

impl OndinaApp {
    /// What the EQ graph asked for, as commands: a drag or a wheel turn merges into one undo step per key.
    pub(crate) fn apply_eq_edits(&mut self, pid: Id, edits: Vec<EqEdit>) {
        for e in edits {
            match e {
                EqEdit::Drag { params, key } => self.cmd_merge(Command::SetPluginParams { plugin_id: pid, params }, key),
                EqEdit::Set { params } => self.cmd(Command::SetPluginParams { plugin_id: pid, params }),
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
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

    fn band(inst: &PluginInstance, name: &str) -> EqBand {
        eq_bands(inst).into_iter().find(|b| b.name == name).unwrap()
    }

    fn plot() -> Rect {
        Rect::from_min_size(pos2(0.0, 0.0), vec2(600.0, 240.0))
    }

    fn near(a: f32, b: f32, tol: f32) -> bool {
        (a - b).abs() <= tol
    }

    fn map(kv: &[(&str, f32)]) -> BTreeMap<String, f32> {
        kv.iter().map(|(k, v)| (k.to_string(), *v)).collect()
    }

    #[test]
    fn band_tables_name_registry_parameters() {
        for (kind, table) in [("channel_eq", &CHANNEL_EQ_BANDS[..]), ("dynamic_eq", &DYNAMIC_EQ_BANDS[..])] {
            let info = plugins::find(kind).unwrap();
            for (i, b) in table.iter().enumerate() {
                assert_eq!(b.index, i, "{kind}");
                for id in [Some(b.freq), b.gain, b.q, b.on, b.kind_param, b.threshold, b.range].into_iter().flatten() {
                    assert!(info.params.iter().any(|p| p.id == id), "{kind}: {id} not in the registry");
                }
            }
        }
        assert_eq!(CHANNEL_EQ_BANDS.len(), 6);
        assert_eq!(DYNAMIC_EQ_BANDS.len(), 4);
        assert!(has_eq_view("channel_eq") && has_eq_view("dynamic_eq"));
        assert!(!has_eq_view("compressor") && !has_eq_view("vst3"));
    }

    #[test]
    fn points_follow_the_bands() {
        let ceq = instance("channel_eq");
        let bands = eq_bands(&ceq);
        assert_eq!(bands.len(), 6);
        let on: Vec<bool> = bands.iter().map(|b| band_on(&ceq, b)).collect();
        assert_eq!(on, [false, true, true, true, true, false]);
        assert_eq!(band_point(&ceq, &band(&ceq, "Mid 1")), (400.0, 0.0));
        assert_eq!(band_point(&ceq, &band(&ceq, "High Pass")), (80.0, 0.0));
        assert!(band_on(&with(ceq.clone(), &[("hp_on", 1.0)]), &band(&ceq, "High Pass")));
        // every band has its own color
        let colors: Vec<Color32> = bands.iter().map(|b| band_color(b.index)).collect();
        for (i, c) in colors.iter().enumerate() {
            assert!(!colors[..i].contains(c), "band {i} shares a color");
        }
        let deq = instance("dynamic_eq");
        let bands = eq_bands(&deq);
        assert_eq!(bands.len(), 4);
        assert!(bands.iter().all(|b| band_on(&deq, b)));
        assert_eq!(bands[0].q, None);
        assert_eq!(bands[3].q, None);
        assert!(bands[1].q.is_some() && bands[2].q.is_some());
        assert_eq!(eq_bands(&with(deq.clone(), &[("b2_type", 1.0)]))[1].q, None);
        assert!(!band_on(&with(deq, &[("b3_on", 0.0)]), &bands[2]));
        assert!(eq_bands(&instance("compressor")).is_empty());
    }

    #[test]
    fn scales_round_trip() {
        let p = plot();
        assert!(near(freq_to_x(20.0, p), 0.0, 1e-3));
        assert!(near(freq_to_x(20000.0, p), 600.0, 1e-3));
        assert!(near(freq_to_x(632.46, p), 300.0, 0.5));
        for f in [25.0, 400.0, 15000.0] {
            assert!(near(x_to_freq(freq_to_x(f, p), p), f, f * 1e-3), "{f}");
        }
        assert_eq!(db_to_y(24.0, p), 0.0);
        assert_eq!(db_to_y(-24.0, p), 240.0);
        assert_eq!(db_to_y(0.0, p), 120.0);
        assert_eq!(db_to_y(-120.0, p), 240.0);
        for d in [-18.0, -3.0, 0.0, 6.5, 20.0] {
            assert!(near(y_to_db(db_to_y(d, p), p), d, 1e-3), "{d}");
        }
        let f = log_freqs(4);
        assert_eq!(f.len(), 4);
        for (a, b) in f.iter().zip([20.0, 200.0, 2000.0, 20000.0]) {
            assert!(near(*a, b, b * 1e-3), "{a} {b}");
        }
    }

    #[test]
    fn drag_values_clamp_to_each_parameter_range() {
        let p = plot();
        let ceq = instance("channel_eq");
        let mid = band(&ceq, "Mid 1");
        let v = |m: Vec2| drag_values(&ceq, &mid, (400.0, 0.0), m, p);
        let still = v(Vec2::ZERO);
        assert!(near(still["mid1_freq"], 400.0, 0.5) && near(still["mid1_gain"], 0.0, 1e-3));
        assert!(near(v(vec2(200.0, 0.0))["mid1_freq"], 4000.0, 40.0), "{:?}", v(vec2(200.0, 0.0)));
        assert_eq!(v(vec2(10000.0, 0.0))["mid1_freq"], 8000.0);
        assert_eq!(v(vec2(-10000.0, 0.0))["mid1_freq"], 80.0);
        assert_eq!(v(vec2(0.0, -10000.0))["mid1_gain"], 24.0);
        assert_eq!(v(vec2(0.0, 10000.0))["mid1_gain"], -24.0);
        assert!(near(v(vec2(0.0, -30.0))["mid1_gain"], 6.0, 1e-3));
        let hp = drag_values(&ceq, &band(&ceq, "High Pass"), (80.0, 0.0), vec2(10000.0, -50.0), p);
        assert_eq!(hp, map(&[("hp_freq", 1000.0)]));
        let low = drag_values(&ceq, &band(&ceq, "Low Shelf"), (120.0, 0.0), vec2(-10000.0, 0.0), p);
        assert_eq!(low["low_freq"], 30.0);
        let deq = instance("dynamic_eq");
        let b3 = drag_values(&deq, &band(&deq, "Band 3"), (2500.0, 0.0), vec2(10000.0, 0.0), p);
        assert_eq!(b3["b3_freq"], 20000.0);
        assert_eq!(accumulate(Vec2::ZERO, vec2(100.0, -40.0), true), vec2(10.0, -4.0));
        assert_eq!(accumulate(Vec2::ZERO, vec2(100.0, -40.0), false), vec2(100.0, -40.0));
    }

    #[test]
    fn wheel_q_doubles_and_clamps() {
        assert!(near(wheel_q(1.0, vec2(0.0, 160.0), false), 2.0, 1e-4));
        assert!(near(wheel_q(1.0, vec2(0.0, -160.0), false), 0.5, 1e-4));
        assert_eq!(wheel_q(1.0, vec2(0.0, 1e6), false), 10.0);
        assert_eq!(wheel_q(1.0, vec2(0.0, -1e6), false), 0.1);
        let fine = wheel_q(1.0, vec2(0.0, 160.0), true);
        assert!(near(fine, 2f32.powf(0.1), 1e-5));
        assert_eq!(fine, wheel_q(1.0, vec2(160.0, 0.0), true));
        assert_eq!(wheel_q(2.0, vec2(f32::NAN, 0.0), false), 2.0);
    }

    #[test]
    fn band_reset_uses_the_registry_defaults() {
        let ceq = with(instance("channel_eq"), &[("mid1_freq", 900.0), ("mid1_gain", 5.0), ("mid1_q", 4.0), ("hp_on", 1.0)]);
        assert_eq!(band_reset(&ceq, 2), map(&[("mid1_freq", 400.0), ("mid1_gain", 0.0), ("mid1_q", 1.0)]));
        assert_eq!(band_reset(&ceq, 0), map(&[("hp_freq", 80.0)]));
        let deq = instance("dynamic_eq");
        assert_eq!(band_reset(&deq, 3), map(&[("b4_freq", 8000.0), ("b4_gain", 0.0), ("b4_q", 0.7)]));
        assert!(band_reset(&deq, 9).is_empty());
    }

    #[test]
    fn band_toggle_flips_only_real_switches() {
        let ceq = instance("channel_eq");
        assert_eq!(band_toggle(&ceq, &band(&ceq, "High Pass")), Some(map(&[("hp_on", 1.0)])));
        let hp_on = with(ceq.clone(), &[("hp_on", 1.0)]);
        assert_eq!(band_toggle(&hp_on, &band(&ceq, "High Pass")), Some(map(&[("hp_on", 0.0)])));
        assert_eq!(band_toggle(&ceq, &band(&ceq, "Mid 1")), None);
        assert_eq!(band_toggle(&ceq, &band(&ceq, "Low Pass")), Some(map(&[("lp_on", 1.0)])));
        let deq = instance("dynamic_eq");
        assert_eq!(band_toggle(&deq, &band(&deq, "Band 2")), Some(map(&[("b2_on", 0.0)])));
    }

    #[test]
    fn band_readout_names_values_and_units() {
        let ceq = instance("channel_eq");
        assert_eq!(band_readout(&ceq, &band(&ceq, "Mid 1")), "Mid 1 \u{00B7} 400 Hz \u{00B7} 0.0 dB \u{00B7} Q 1.00");
        assert_eq!(band_readout(&ceq, &band(&ceq, "High Pass")), "High Pass \u{00B7} 80.0 Hz \u{00B7} off");
        let hp_on = with(ceq.clone(), &[("hp_on", 1.0)]);
        assert_eq!(band_readout(&hp_on, &band(&ceq, "High Pass")), "High Pass \u{00B7} 80.0 Hz");
        let low = with(ceq.clone(), &[("low_gain", 3.0)]);
        assert_eq!(band_readout(&low, &band(&ceq, "Low Shelf")), "Low Shelf \u{00B7} 120 Hz \u{00B7} +3.0 dB");
        let deq = with(instance("dynamic_eq"), &[("b2_range", -6.0)]);
        assert_eq!(band_readout(&deq, &band(&deq, "Band 2")), "Band 2 \u{00B7} Bell \u{00B7} 300 Hz \u{00B7} 0.0 dB \u{00B7} Q 1.00 \u{00B7} Range -6.0 dB above -24.0 dB");
        assert_eq!(band_readout(&deq, &band(&deq, "Band 1")), "Band 1 \u{00B7} Low Shelf \u{00B7} 100 Hz \u{00B7} 0.0 dB");
        let off = with(instance("dynamic_eq"), &[("b3_on", 0.0)]);
        assert!(band_readout(&off, &band(&off, "Band 3")).ends_with(" \u{00B7} off"));
    }

    #[test]
    fn readout_rect_stays_inside_the_graph() {
        let p = plot();
        let size = vec2(120.0, 18.0);
        let r = readout_rect(pos2(300.0, 120.0), size, p);
        assert!(near(r.center().x, 300.0, 1e-3));
        assert_eq!(r.max.y, 120.0 - POINT_R - theme::SP_3);
        assert_eq!(readout_rect(pos2(300.0, 10.0), size, p).min.y, 10.0 + POINT_R + theme::SP_3);
        assert_eq!(readout_rect(pos2(595.0, 120.0), size, p).max.x, 600.0 - theme::SP_1);
        assert_eq!(readout_rect(pos2(5.0, 120.0), size, p).min.x, theme::SP_1);
    }

    #[test]
    fn range_area_only_where_range_is_set() {
        let freqs = log_freqs(64);
        let deq = instance("dynamic_eq");
        for b in eq_bands(&deq) {
            assert!(range_area(&deq, &b, 48000.0, &freqs).is_none(), "{}", b.name);
        }
        let set = with(deq.clone(), &[("b2_freq", 1000.0), ("b2_q", 1.0), ("b2_range", -6.0)]);
        let (at_gain, at_range) = range_area(&set, &band(&set, "Band 2"), 48000.0, &freqs).expect("an area for Band 2");
        let k = freqs.iter().enumerate().min_by(|a, b| (a.1 - 1000.0).abs().total_cmp(&(b.1 - 1000.0).abs())).unwrap().0;
        assert!(near(at_gain[k], 0.0, 0.01), "{}", at_gain[k]);
        assert!(near(at_range[k], -6.0, 0.3), "{}", at_range[k]);
        assert!(range_area(&set, &band(&set, "Band 3"), 48000.0, &freqs).is_none());
        let off = with(set, &[("b2_on", 0.0)]);
        assert!(range_area(&off, &band(&off, "Band 2"), 48000.0, &freqs).is_none());
        let ceq = with(instance("channel_eq"), &[("mid1_gain", 6.0)]);
        for b in eq_bands(&ceq) {
            assert!(range_area(&ceq, &b, 48000.0, &freqs).is_none());
        }
    }

    #[test]
    fn curves_match_the_dsp() {
        let ceq = with(instance("channel_eq"), &[("mid1_gain", -6.0), ("mid1_freq", 400.0)]);
        let total = total_curve(&ceq, 48000.0, &[400.0]);
        let p = channel_eq_params(|id| ceq.param(id));
        assert_eq!(total[0], channel_eq_response_db(&p, 400.0, 48000.0));
        assert!(near(total[0], -6.0, 0.05));
        assert!(band_curve(&ceq, &band(&ceq, "Mid 1"), 48000.0, &[400.0]).is_some());
        assert!(band_curve(&ceq, &band(&ceq, "Mid 2"), 48000.0, &[400.0]).is_none());
    }

    #[test]
    fn keys_have_the_window_format() {
        assert_eq!(drag_key(7, 2, 3), "pw7:eq2#3");
        assert_eq!(wheel_key(7, 2), "pw7:eq2:q");
    }

    #[test]
    fn graph_sizes() {
        assert_eq!(graph_height(100.0), 150.0);
        assert_eq!(graph_height(400.0), 200.0);
        assert_eq!(graph_height(2000.0), 300.0);
        assert_eq!(curve_sample_rate(0), 48000.0);
        assert_eq!(curve_sample_rate(44100), 44100.0);
        assert_eq!(crate::plugin_window::default_size(&instance("channel_eq")), vec2(680.0, 640.0));
        assert_eq!(crate::plugin_window::default_size(&instance("dynamic_eq")), vec2(680.0, 640.0));
        assert_eq!(crate::plugin_window::default_size(&instance("compressor")), vec2(560.0, 440.0));
    }

    /// Frames of the EQ graph in a headless 700x500 context, one per entry of `frames` (the events of that frame).
    /// Returns the plot and every edit asked for.
    fn run_frames(inst: &PluginInstance, gesture: &mut u64, frames: &[Vec<Event>]) -> (Rect, Vec<EqEdit>) {
        let ctx = egui::Context::default();
        theme::apply(&ctx);
        let mut plot = Rect::NOTHING;
        let mut all = Vec::new();
        for (k, events) in frames.iter().enumerate() {
            let raw = egui::RawInput {
                screen_rect: Some(Rect::from_min_size(Pos2::ZERO, vec2(700.0, 500.0))),
                time: Some(k as f64 / 60.0),
                events: events.clone(),
                ..Default::default()
            };
            let _ = ctx.run(raw, |ctx| {
                egui::CentralPanel::default().show(ctx, |ui| {
                    let out = eq_graph(ui, inst, 48000.0, 240.0, gesture);
                    plot = out.plot;
                    all.extend(out.edits);
                });
            });
        }
        (plot, all)
    }

    /// The plot of the graph in the harness (the same in every frame).
    fn harness_plot(inst: &PluginInstance) -> Rect {
        run_frames(inst, &mut 0, &[vec![]]).0
    }

    fn press(pos: Pos2, button: PointerButton, pressed: bool) -> Event {
        Event::PointerButton { pos, button, pressed, modifiers: Modifiers::NONE }
    }

    #[test]
    fn eq_graph_draws_both_eqs_without_edits() {
        let extreme = [("mid1_gain", 24.0), ("mid1_q", 10.0), ("b2_range", -24.0), ("b3_freq", 20000.0)];
        for kind in ["channel_eq", "dynamic_eq"] {
            for inst in [instance(kind), with(instance(kind), &extreme)] {
                let mut g = 0;
                let (plot, edits) = run_frames(&inst, &mut g, &[vec![], vec![]]);
                assert!(edits.is_empty(), "{kind}: {edits:?}");
                assert_eq!(g, 0);
                assert!(plot.width() > 0.0 && plot.height() == 240.0);
            }
        }
    }

    #[test]
    fn dragging_a_point_sends_one_gesture() {
        let inst = instance("channel_eq");
        let p = point_pos(400.0, 0.0, harness_plot(&inst));
        let mut frames = vec![vec![], vec![Event::PointerMoved(p)], vec![press(p, PointerButton::Primary, true)]];
        for k in 1..=3 {
            frames.push(vec![Event::PointerMoved(p + vec2(10.0, -8.0) * k as f32)]);
        }
        frames.push(vec![press(p + vec2(30.0, -24.0), PointerButton::Primary, false)]);
        let mut g = 0;
        let (_, edits) = run_frames(&inst, &mut g, &frames);
        assert!(!edits.is_empty());
        assert_eq!(g, 1);
        for e in &edits {
            assert!(matches!(e, EqEdit::Drag { key, .. } if key == "pw3:eq2#1"), "{e:?}");
        }
        let EqEdit::Drag { params, .. } = edits.last().unwrap() else { unreachable!() };
        assert_eq!(params.keys().collect::<Vec<_>>(), ["mid1_freq", "mid1_gain"]);
        assert!(params["mid1_freq"] > 400.0 && params["mid1_gain"] > 0.0, "{params:?}");
    }

    #[test]
    fn wheel_over_a_bell_changes_q() {
        let inst = instance("channel_eq");
        let plot = harness_plot(&inst);
        let wheel = Event::MouseWheel { unit: MouseWheelUnit::Line, delta: vec2(0.0, 1.0), modifiers: Modifiers::NONE };
        let mid = point_pos(400.0, 0.0, plot);
        let mut g = 0;
        let (_, edits) = run_frames(&inst, &mut g, &[vec![], vec![Event::PointerMoved(mid)], vec![wheel.clone()]]);
        assert_eq!(edits.len(), 1, "{edits:?}");
        let EqEdit::Drag { params, key } = &edits[0] else { panic!("{edits:?}") };
        assert_eq!(key, "pw3:eq2:q");
        assert!(near(params["mid1_q"], 2f32.powf(40.0 / 160.0), 0.01), "{params:?}");
        let shelf = point_pos(120.0, 0.0, plot);
        let (_, edits) = run_frames(&inst, &mut g, &[vec![], vec![Event::PointerMoved(shelf)], vec![wheel]]);
        assert!(edits.is_empty(), "{edits:?}");
        assert_eq!(g, 0);
    }

    #[test]
    fn right_click_turns_the_high_pass_on() {
        let inst = instance("channel_eq");
        let hp = point_pos(80.0, 0.0, harness_plot(&inst));
        let mut g = 0;
        let frames = [vec![], vec![Event::PointerMoved(hp)], vec![press(hp, PointerButton::Secondary, true)], vec![press(hp, PointerButton::Secondary, false)]];
        let (_, edits) = run_frames(&inst, &mut g, &frames);
        assert_eq!(edits, [EqEdit::Set { params: map(&[("hp_on", 1.0)]) }]);
    }

    #[test]
    fn the_eq_view_reuses_the_common_header() {
        let src = include_str!("eq_view.rs");
        assert!(!src.contains(concat!("plugin_ai_bar", "(")));
        assert!(!src.contains(concat!("plugin_header", "(")));
    }
}
