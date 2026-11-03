//! The graph of the dynamics windows (Compressor, Limiter, Noise Gate, De-Esser): transfer curve with the threshold
//! point to drag, live input, gain reduction and output meters, and the De-Esser's band. The math is plain functions
//! the tests cover; dynamics_graph draws them and turns the mouse into edits.

use std::collections::BTreeMap;
use std::time::Duration;

use eframe::egui::{self, Rect, Vec2};
use ondina_core::model::{Id, PluginInstance};
use ondina_core::plugins;
use ondina_dsp::effects::DynamicsCurve;
use ondina_dsp::util::gain_to_db;
use ondina_engine::PluginMeter;

use crate::eq_view;
use crate::theme;
use crate::widgets::format_param_value;

/// The four dynamics kinds and what their point moves.
#[derive(Debug)]
pub struct DynSpec {
    pub kind: &'static str,
    /// Moved by dragging the point sideways.
    pub x_param: &'static str,
    /// Moved by dragging the point up and down.
    pub y_param: &'static str,
    /// Moved by the wheel over the point.
    pub wheel_param: &'static str,
    /// The input and output range of the graph (dB), the same on both axes.
    pub span: (f32, f32),
    /// The frequency the De-Esser's band marker moves.
    pub freq_param: Option<&'static str>,
}

static SPECS: [DynSpec; 4] = [
    DynSpec { kind: "compressor", x_param: "threshold", y_param: "ratio", wheel_param: "knee", span: (-60.0, 0.0), freq_param: None },
    DynSpec { kind: "limiter", x_param: "gain", y_param: "ceiling", wheel_param: "release", span: (-36.0, 0.0), freq_param: None },
    DynSpec { kind: "noise_gate", x_param: "threshold", y_param: "reduction", wheel_param: "reduction", span: (-90.0, 0.0), freq_param: None },
    DynSpec { kind: "de_esser", x_param: "threshold", y_param: "range", wheel_param: "range", span: (-60.0, 0.0), freq_param: Some("freq") },
];

/// Width of the meter column: three columns of 36 (label, bar, number).
pub const METER_COL_W: f32 = 108.0;
pub const METER_BAR_W: f32 = 14.0;
/// The gain reduction bar is full at this many dB (the number under it shows deeper reductions).
pub const GR_SPAN_DB: f32 = 24.0;
/// Below this an input or output peak counts as silence (no dot, no repaint).
pub const SILENCE_DB: f32 = -90.0;
/// Pointer points of a vertical drag that double the compressor's ratio.
pub const RATIO_DRAG_POINTS: f32 = 80.0;
/// egui scroll points per wheel notch.
pub const WHEEL_NOTCH: f32 = 40.0;
/// Wheel notches that double the limiter's release.
pub const RELEASE_NOTCHES_PER_DOUBLING: f32 = 4.0;
/// Narrowest De-Esser band beside the meters; narrower, it goes under the curve.
pub const BAND_MIN_W: f32 = 160.0;
pub const BAND_H: f32 = 64.0;
/// How often the window repaints while the meters move.
pub const METER_REPAINT: Duration = Duration::from_millis(33);

/// Whether plug-in kind `kind` has the dynamics graph.
pub fn has_dynamics_view(kind: &str) -> bool {
    spec(kind).is_some()
}

/// What the point of kind `kind` moves; None for a kind without the dynamics graph.
pub fn spec(kind: &str) -> Option<&'static DynSpec> {
    SPECS.iter().find(|s| s.kind == kind)
}

/// The transfer curve of `inst`, read from its parameters as the processor reads them.
pub fn curve_of(inst: &PluginInstance) -> Option<DynamicsCurve> {
    DynamicsCurve::of(&inst.kind, |id| inst.param(id))
}

// ---------------------------------------------------------------------------- scales

/// Input level `db` across the plot: `span.0` at the left, `span.1` at the right, clamped to the plot.
pub fn db_to_x(db: f32, span: (f32, f32), plot: Rect) -> f32 {
    let u = (db - span.0) / (span.1 - span.0);
    (plot.left() + u * plot.width()).clamp(plot.left(), plot.right())
}

pub fn x_to_db(x: f32, span: (f32, f32), plot: Rect) -> f32 {
    if plot.width() <= 0.0 {
        return span.0;
    }
    span.0 + (x - plot.left()) / plot.width() * (span.1 - span.0)
}

/// Output level `db` up the plot: `span.0` at the bottom, `span.1` at the top, clamped to the plot.
pub fn db_to_y(db: f32, span: (f32, f32), plot: Rect) -> f32 {
    let u = (db - span.0) / (span.1 - span.0);
    (plot.bottom() - u * plot.height()).clamp(plot.top(), plot.bottom())
}

pub fn y_to_db(y: f32, span: (f32, f32), plot: Rect) -> f32 {
    if plot.height() <= 0.0 {
        return span.0;
    }
    span.0 + (plot.bottom() - y) / plot.height() * (span.1 - span.0)
}

// ---------------------------------------------------------------------------- curve

/// Where the threshold point sits (input, output dB). The gate's goes on the diagonal: at the threshold itself the
/// processor is still closed, and out_db would give the bottom of the step.
pub fn handle_db(c: &DynamicsCurve) -> (f32, f32) {
    let t = c.threshold_db();
    match c {
        DynamicsCurve::NoiseGate { .. } => (t, t),
        _ => (t, c.out_db(t)),
    }
}

/// The curve as (input, output) dB pairs: `n` inputs evenly spaced across `span` and, for the gate, both ends of its
/// step at the threshold, so the step is drawn vertical.
pub fn curve_points(c: &DynamicsCurve, span: (f32, f32), n: usize) -> Vec<(f32, f32)> {
    let n = n.max(2);
    let mut pts: Vec<(f32, f32)> = (0..n)
        .map(|i| {
            let x = span.0 + (span.1 - span.0) * i as f32 / (n - 1) as f32;
            (x, c.out_db(x))
        })
        .collect();
    if let DynamicsCurve::NoiseGate { threshold, reduction } = *c {
        let k = pts.iter().position(|p| p.0 > threshold).unwrap_or(pts.len());
        pts.splice(k..k, [(threshold, threshold + reduction), (threshold, threshold)]);
    }
    pts
}

// ---------------------------------------------------------------------------- interaction

/// The values of the point's two parameters when a drag starts.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct DragStart {
    pub x: f32,
    pub y: f32,
}

/// The point's two parameters after a drag that started at `start` and has moved `moved` points across `plot`, each
/// clamped to its registry range. Sideways moves the threshold (the Limiter: its input gain, so the point where it
/// starts to limit follows the pointer); up and down the ratio (down is higher), the ceiling, the gate's reduction
/// (down is deeper) or the de-esser's range (down is more).
pub fn drag_values(inst: &PluginInstance, start: DragStart, moved: Vec2, plot: Rect) -> BTreeMap<String, f32> {
    let Some(s) = spec(&inst.kind) else {
        return BTreeMap::new();
    };
    let w = s.span.1 - s.span.0;
    let dx = if plot.width() > 0.0 { moved.x / plot.width() * w } else { 0.0 };
    let dy = if plot.height() > 0.0 { -moved.y / plot.height() * w } else { 0.0 };
    let clamp = |id: &str, v: f32| {
        let (lo, hi) = eq_view::range_of(&inst.kind, id);
        v.clamp(lo, hi)
    };
    let (x, y) = match s.kind {
        "compressor" => (start.x + dx, start.y * 2f32.powf(moved.y / RATIO_DRAG_POINTS)),
        "limiter" => {
            let ceiling = clamp(s.y_param, start.y + dy);
            (ceiling - ((start.y - start.x) + dx), ceiling)
        }
        "de_esser" => (start.x + dx, start.y - dy),
        _ => (start.x + dx, start.y + dy),
    };
    BTreeMap::from([(s.x_param.to_string(), clamp(s.x_param, x)), (s.y_param.to_string(), clamp(s.y_param, y))])
}

/// The wheel parameter after a turn of `scroll` points (x and y summed: egui turns Shift+wheel into x): 1 dB per
/// notch, the Limiter's release doubling every RELEASE_NOTCHES_PER_DOUBLING notches; up raises it, Shift moves ten
/// times less. None when nothing changes.
pub fn wheel_values(inst: &PluginInstance, scroll: Vec2, fine: bool) -> Option<BTreeMap<String, f32>> {
    let s = spec(&inst.kind)?;
    let notches = (scroll.x + scroll.y) / WHEEL_NOTCH * if fine { eq_view::FINE } else { 1.0 };
    if !notches.is_finite() {
        return None;
    }
    let id = s.wheel_param;
    let v = inst.param(id);
    let next = if id == "release" { v * 2f32.powf(notches / RELEASE_NOTCHES_PER_DOUBLING) } else { v + notches };
    let (lo, hi) = eq_view::range_of(&inst.kind, id);
    let next = next.clamp(lo, hi);
    (next != v).then(|| BTreeMap::from([(id.to_string(), next)]))
}

/// The point's parameters (sideways, up and down, wheel) back to their defaults, as one change.
pub fn point_reset(inst: &PluginInstance) -> BTreeMap<String, f32> {
    let Some(s) = spec(&inst.kind) else {
        return BTreeMap::new();
    };
    let defaults = plugins::default_params(inst);
    [s.x_param, s.y_param, s.wheel_param].into_iter().filter_map(|id| defaults.get(id).map(|v| (id.to_string(), *v))).collect()
}

/// The De-Esser's frequency after its marker moved `moved_x` points across `band` (log axis), clamped to its range.
pub fn freq_drag(inst: &PluginInstance, start_freq: f32, moved_x: f32, band: Rect) -> BTreeMap<String, f32> {
    let Some(id) = spec(&inst.kind).and_then(|s| s.freq_param) else {
        return BTreeMap::new();
    };
    let (lo, hi) = eq_view::range_of(&inst.kind, id);
    let f = eq_view::x_to_freq(eq_view::freq_to_x(start_freq, band) + moved_x, band).clamp(lo, hi);
    BTreeMap::from([(id.to_string(), f)])
}

/// The undo key of a drag of the point in plug-in `pid`'s window: one key per gesture.
pub fn drag_key(pid: Id, gesture: u64) -> String {
    format!("pw{pid}:dyn#{gesture}")
}

/// The undo key of the wheel over the point: turns close together merge.
pub fn wheel_key(pid: Id) -> String {
    format!("pw{pid}:dyn:wheel")
}

/// The undo key of a drag of the De-Esser's frequency marker.
pub fn freq_key(pid: Id, gesture: u64) -> String {
    format!("pw{pid}:dyn:freq#{gesture}")
}

/// "Threshold -18.0 dB · Ratio 4.0:1 · Knee +6.0 dB": the point's parameters with their registry names and units
/// (the Limiter adds where it starts to limit).
pub fn point_readout(inst: &PluginInstance) -> String {
    let Some(s) = spec(&inst.kind) else {
        return String::new();
    };
    let params = plugins::find(&inst.kind).map_or(&[][..], |i| i.params);
    let mut ids = vec![s.x_param, s.y_param];
    if !ids.contains(&s.wheel_param) {
        ids.push(s.wheel_param);
    }
    let mut parts: Vec<String> = ids
        .iter()
        .filter_map(|id| params.iter().find(|p| p.id == *id))
        .map(|p| format!("{} {}", p.name, format_param_value(inst.param(p.id), p.unit)))
        .collect();
    if s.kind == "limiter" {
        parts.push(format!("starts at {}", format_param_value(inst.param("ceiling") - inst.param("gain"), "dB")));
    }
    parts.join(" \u{00B7} ")
}

/// "in -12.0 dB → out -15.0 dB" for the input under the pointer.
pub fn hover_readout(c: &DynamicsCurve, in_db: f32) -> String {
    format!("in {} \u{2192} out {}", format_param_value(in_db, "dB"), format_param_value(c.out_db(in_db), "dB"))
}

/// The help line under the graph.
pub fn hint(kind: &str) -> &'static str {
    match kind {
        "compressor" => "Drag the point: threshold sideways, ratio up and down (Shift: fine) \u{00B7} Wheel: knee \u{00B7} Double-click: default",
        "limiter" => "Drag the point: input gain sideways, ceiling up and down (Shift: fine) \u{00B7} Wheel: release \u{00B7} Double-click: default",
        "noise_gate" => "Drag the point: threshold sideways, reduction up and down (Shift: fine) \u{00B7} Wheel: reduction \u{00B7} Double-click: default",
        "de_esser" => "Drag the point: threshold sideways, range up and down (Shift: fine) \u{00B7} Wheel: range \u{00B7} Double-click: default \u{00B7} Drag the marker: frequency",
        _ => "",
    }
}

// ---------------------------------------------------------------------------- meters

/// What the three meters show, in dB: input and output peaks (-120 is silence) and the gain reduction (<= 0).
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct MeterLevels {
    pub input_db: f32,
    pub output_db: f32,
    pub reduction_db: f32,
}

/// The engine's meter of the insert as the view shows it: no meter is silence, and a bypassed plug-in reduces nothing.
pub fn meter_levels(meter: Option<PluginMeter>, bypass: bool) -> MeterLevels {
    let Some(m) = meter else {
        return MeterLevels { input_db: -120.0, output_db: -120.0, reduction_db: 0.0 };
    };
    let reduction_db = if bypass { 0.0 } else { m.gain_reduction_db.unwrap_or(0.0).min(0.0) };
    MeterLevels { input_db: gain_to_db(m.input_peak), output_db: gain_to_db(m.output_peak), reduction_db }
}

/// Lit fraction of the gain reduction bar, which fills from the top.
pub fn gr_fill(reduction_db: f32) -> f32 {
    (-reduction_db / GR_SPAN_DB).clamp(0.0, 1.0)
}

/// The number under a meter: one decimal, "-inf" for silence, never "-0.0".
pub fn db_text(db: f32) -> String {
    if db < -119.5 {
        return "-inf".to_string();
    }
    let text = format!("{db:.1}");
    if text == "-0.0" {
        "0.0".to_string()
    } else {
        text
    }
}

/// When the window should be drawn again for its meters: only while it is visible and something moves (the song
/// plays, or the levels have not fallen to rest yet).
pub fn meter_repaint(visible: bool, playing: bool, l: MeterLevels) -> Option<Duration> {
    let moving = playing || l.input_db > SILENCE_DB || l.output_db > SILENCE_DB || l.reduction_db < -0.05;
    (visible && moving).then_some(METER_REPAINT)
}

// ---------------------------------------------------------------------------- layout

/// The square of the curve on the left of `rect`, the meter column beside it and, for the De-Esser, its band in the
/// rest of the width when it is at least BAND_MIN_W (None otherwise: it goes under, see total_height).
pub fn graph_layout(rect: Rect, has_band: bool) -> (Rect, Rect, Option<Rect>) {
    let side = rect.height().min(rect.width() - METER_COL_W - theme::SP_5).max(60.0);
    let frame = Rect::from_min_size(rect.min, Vec2::splat(side));
    let meters = Rect::from_min_size(egui::pos2(frame.right() + theme::SP_5, rect.top()), egui::vec2(METER_COL_W, side));
    let left = meters.right() + theme::SP_5;
    let band = (has_band && rect.right() - left >= BAND_MIN_W).then(|| Rect::from_min_max(egui::pos2(left, rect.top()), egui::pos2(rect.right(), rect.top() + side)));
    (frame, meters, band)
}

/// Height of the graph `width` points wide: `height`, plus room for the band underneath when it does not fit beside.
pub fn total_height(width: f32, height: f32, has_band: bool) -> f32 {
    let side = height.min(width - METER_COL_W - theme::SP_5).max(60.0);
    if has_band && width - side - METER_COL_W - 2.0 * theme::SP_5 < BAND_MIN_W {
        height + theme::SP_3 + BAND_H
    } else {
        height
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use eframe::egui::{pos2, vec2};

    fn instance(kind: &str) -> PluginInstance {
        PluginInstance { id: 3, kind: kind.into(), params: Default::default(), bypass: false, preset: None, sample: None, vst3_path: None, vst3_params: vec![], vst3_state: None }
    }

    fn with(mut i: PluginInstance, kv: &[(&str, f32)]) -> PluginInstance {
        for (k, v) in kv {
            i.params.insert(k.to_string(), *v);
        }
        i
    }

    fn plot() -> Rect {
        Rect::from_min_size(pos2(0.0, 0.0), vec2(300.0, 300.0))
    }

    fn near(a: f32, b: f32, tol: f32) -> bool {
        (a - b).abs() <= tol
    }

    fn map(kv: &[(&str, f32)]) -> BTreeMap<String, f32> {
        kv.iter().map(|(k, v)| (k.to_string(), *v)).collect()
    }

    const KINDS: [&str; 4] = ["compressor", "limiter", "noise_gate", "de_esser"];

    #[test]
    fn has_dynamics_view_only_for_the_four_kinds() {
        for kind in KINDS {
            assert!(has_dynamics_view(kind), "{kind}");
            assert_eq!(spec(kind).map(|s| s.kind), Some(kind));
            assert!(curve_of(&instance(kind)).is_some(), "{kind}");
        }
        for kind in ["channel_eq", "dynamic_eq", "reverb", "vst3", "analog_synth"] {
            assert!(!has_dynamics_view(kind), "{kind}");
            assert!(spec(kind).is_none() && curve_of(&instance(kind)).is_none(), "{kind}");
        }
    }

    #[test]
    fn specs_name_registry_parameters() {
        for s in &SPECS {
            let info = plugins::find(s.kind).unwrap();
            for id in [Some(s.x_param), Some(s.y_param), Some(s.wheel_param), s.freq_param].into_iter().flatten() {
                assert!(info.params.iter().any(|p| p.id == id), "{}: {id} not in the registry", s.kind);
            }
            assert!(s.span.0 < s.span.1 && s.span.1 <= 0.0, "{}", s.kind);
        }
    }

    #[test]
    fn scales_round_trip() {
        let p = plot();
        for s in &SPECS {
            let span = s.span;
            assert_eq!(db_to_x(span.0, span, p), p.left());
            assert_eq!(db_to_x(span.1, span, p), p.right());
            assert_eq!(db_to_y(span.1, span, p), p.top());
            assert_eq!(db_to_y(span.0, span, p), p.bottom());
            for d in [-30.0, -18.0, -3.0] {
                assert!(near(x_to_db(db_to_x(d, span, p), span, p), d, 1e-3), "{} {d}", s.kind);
                assert!(near(y_to_db(db_to_y(d, span, p), span, p), d, 1e-3), "{} {d}", s.kind);
            }
            assert_eq!(db_to_x(-200.0, span, p), p.left());
            assert_eq!(db_to_x(20.0, span, p), p.right());
            assert_eq!(db_to_y(-200.0, span, p), p.bottom());
            assert_eq!(db_to_y(20.0, span, p), p.top());
        }
    }

    #[test]
    fn handle_sits_where_the_curve_bends() {
        let c = curve_of(&instance("compressor")).unwrap();
        assert_eq!(handle_db(&c), (-18.0, c.out_db(-18.0)));
        let l = curve_of(&with(instance("limiter"), &[("gain", 6.0), ("ceiling", -1.0)])).unwrap();
        assert_eq!(handle_db(&l), (-7.0, -1.0));
        assert_eq!(handle_db(&curve_of(&instance("noise_gate")).unwrap()), (-50.0, -50.0));
        assert_eq!(handle_db(&curve_of(&instance("de_esser")).unwrap()), (-24.0, -24.0));
    }

    #[test]
    fn gate_curve_has_a_vertical_step() {
        let gate = curve_of(&with(instance("noise_gate"), &[("threshold", -40.0), ("reduction", -30.0)])).unwrap();
        let pts = curve_points(&gate, (-90.0, 0.0), 64);
        assert!(pts.windows(2).any(|w| w == [(-40.0, -70.0), (-40.0, -40.0)]), "no vertical step in {pts:?}");
        assert!(pts.windows(2).all(|w| w[0].0 <= w[1].0), "{pts:?}");
        let comp = curve_of(&instance("compressor")).unwrap();
        let pts = curve_points(&comp, (-60.0, 0.0), 64);
        assert_eq!(pts.len(), 64);
        assert!(pts.windows(2).all(|w| w[0].0 < w[1].0));
        assert_eq!((pts[0].0, pts[63].0), (-60.0, 0.0));
        for (x, y) in &pts {
            assert_eq!(*y, comp.out_db(*x));
        }
    }

    #[test]
    fn drag_values_move_each_kind() {
        let p = plot();
        let comp = instance("compressor");
        let c = |m: Vec2| drag_values(&comp, DragStart { x: -18.0, y: 4.0 }, m, p);
        assert_eq!(c(vec2(30.0, 0.0)), map(&[("threshold", -12.0), ("ratio", 4.0)]));
        assert!(near(c(vec2(0.0, 80.0))["ratio"], 8.0, 1e-4));
        assert_eq!(c(vec2(0.0, -1e4))["ratio"], 1.0);
        let lim = instance("limiter");
        let l = |m: Vec2| drag_values(&lim, DragStart { x: 0.0, y: -1.0 }, m, p);
        assert_eq!(l(vec2(0.0, -1e4)), map(&[("gain", 1.0), ("ceiling", 0.0)]), "the point stays at -1 dB of input");
        let moved = l(vec2(-60.0, 0.0));
        assert!(near(moved["gain"], 7.2, 1e-4) && moved["ceiling"] == -1.0, "{moved:?}");
        let gate = instance("noise_gate");
        assert!(near(drag_values(&gate, DragStart { x: -50.0, y: -60.0 }, vec2(0.0, 30.0), p)["reduction"], -69.0, 1e-4));
        let de = instance("de_esser");
        assert!(near(drag_values(&de, DragStart { x: -24.0, y: 8.0 }, vec2(0.0, 30.0), p)["range"], 14.0, 1e-4));
        for kind in KINDS {
            let inst = instance(kind);
            let s = spec(kind).unwrap();
            let start = DragStart { x: inst.param(s.x_param), y: inst.param(s.y_param) };
            for m in [vec2(1e4, 1e4), vec2(-1e4, -1e4), vec2(1e4, -1e4), vec2(-1e4, 1e4)] {
                let v = drag_values(&inst, start, m, p);
                assert_eq!(v.keys().map(String::as_str).collect::<Vec<_>>().len(), 2, "{kind}");
                assert!(v.contains_key(s.x_param) && v.contains_key(s.y_param), "{kind}: {v:?}");
                let info = plugins::find(kind).unwrap();
                for (k, x) in &v {
                    let pi = info.params.iter().find(|q| q.id == k).unwrap();
                    assert!((pi.min..=pi.max).contains(x), "{kind}: {k} = {x}");
                }
            }
        }
    }

    #[test]
    fn wheel_values_step_each_kind() {
        let comp = instance("compressor");
        assert_eq!(wheel_values(&comp, vec2(0.0, 40.0), false), Some(map(&[("knee", 7.0)])));
        assert!(near(wheel_values(&comp, vec2(0.0, 40.0), true).unwrap()["knee"], 6.1, 1e-4));
        assert_eq!(wheel_values(&comp, vec2(0.0, -1e6), false), Some(map(&[("knee", 0.0)])));
        assert!(near(wheel_values(&instance("limiter"), vec2(0.0, 160.0), false).unwrap()["release"], 120.0, 1e-3));
        assert_eq!(wheel_values(&instance("noise_gate"), vec2(0.0, 40.0), false), Some(map(&[("reduction", -59.0)])));
        assert_eq!(wheel_values(&instance("de_esser"), vec2(0.0, 40.0), false), Some(map(&[("range", 9.0)])));
        assert_eq!(wheel_values(&comp, vec2(f32::NAN, 0.0), false), None);
        assert_eq!(wheel_values(&with(comp, &[("knee", 24.0)]), vec2(0.0, 40.0), false), None);
        assert_eq!(wheel_values(&instance("reverb"), vec2(0.0, 40.0), false), None);
    }

    #[test]
    fn point_reset_uses_the_registry_defaults() {
        let comp = with(instance("compressor"), &[("threshold", -30.0), ("ratio", 8.0), ("knee", 0.0), ("attack", 2.0)]);
        assert_eq!(point_reset(&comp), map(&[("threshold", -18.0), ("ratio", 4.0), ("knee", 6.0)]));
        assert_eq!(point_reset(&instance("noise_gate")), map(&[("threshold", -50.0), ("reduction", -60.0)]));
        assert_eq!(point_reset(&instance("limiter")), map(&[("gain", 0.0), ("ceiling", -1.0), ("release", 60.0)]));
        assert!(point_reset(&instance("reverb")).is_empty());
    }

    #[test]
    fn freq_drag_clamps_to_the_de_esser_range() {
        let de = instance("de_esser");
        let band = Rect::from_min_size(pos2(0.0, 0.0), vec2(200.0, 64.0));
        let still = freq_drag(&de, 6000.0, 0.0, band);
        assert!(near(still["freq"], 6000.0, 1.0), "{still:?}");
        assert!(freq_drag(&de, 6000.0, 10.0, band)["freq"] > 6000.0);
        assert_eq!(freq_drag(&de, 6000.0, 1e4, band), map(&[("freq", 12000.0)]));
        assert_eq!(freq_drag(&de, 6000.0, -1e4, band), map(&[("freq", 2000.0)]));
        assert!(freq_drag(&instance("compressor"), 6000.0, 10.0, band).is_empty());
    }

    #[test]
    fn keys_have_the_window_format() {
        assert_eq!(drag_key(7, 3), "pw7:dyn#3");
        assert_eq!(wheel_key(7), "pw7:dyn:wheel");
        assert_eq!(freq_key(7, 2), "pw7:dyn:freq#2");
    }

    #[test]
    fn readouts_name_values_and_units() {
        let comp = instance("compressor");
        let r = point_readout(&comp);
        for want in ["Threshold", "Ratio", "Knee", &format_param_value(-18.0, "dB"), &format_param_value(4.0, ":1"), &format_param_value(6.0, "dB")] {
            assert!(r.contains(want), "missing {want:?} in {r}");
        }
        let gate = point_readout(&instance("noise_gate"));
        assert_eq!(gate.matches("Reduction").count(), 1, "{gate}");
        let lim = point_readout(&with(instance("limiter"), &[("gain", 6.0), ("ceiling", -1.0)]));
        assert!(lim.contains("starts at") && lim.contains(&format_param_value(-7.0, "dB")), "{lim}");
        let h = hover_readout(&curve_of(&comp).unwrap(), -12.0);
        assert!(h.contains('\u{2192}') && h.contains(&format_param_value(-12.0, "dB")), "{h}");
        for kind in KINDS {
            assert!(hint(kind).contains("Double-click"), "{kind}");
        }
        assert!(hint("de_esser").contains("frequency"));
    }

    #[test]
    fn meter_levels_read_the_engine_meter() {
        assert_eq!(meter_levels(None, false), MeterLevels { input_db: -120.0, output_db: -120.0, reduction_db: 0.0 });
        let m = PluginMeter { input_peak: 0.5, output_peak: 0.25, gain_reduction_db: Some(-6.0) };
        let l = meter_levels(Some(m), false);
        assert!(near(l.input_db, -6.02, 0.01) && near(l.output_db, -12.04, 0.01), "{l:?}");
        assert_eq!(l.reduction_db, -6.0);
        assert_eq!(meter_levels(Some(m), true).reduction_db, 0.0);
        assert_eq!(meter_levels(Some(PluginMeter { gain_reduction_db: None, ..m }), false).reduction_db, 0.0);
        assert_eq!(meter_levels(Some(PluginMeter { gain_reduction_db: Some(0.5), ..m }), false).reduction_db, 0.0);
        assert_eq!(gr_fill(-12.0), 0.5);
        assert_eq!(gr_fill(-60.0), 1.0);
        assert_eq!(gr_fill(0.0), 0.0);
        assert_eq!(db_text(-130.0), "-inf");
        assert_eq!(db_text(-0.01), "0.0");
        assert_eq!(db_text(-6.04), "-6.0");
    }

    #[test]
    fn meter_repaint_only_when_visible_and_moving() {
        let silent = MeterLevels { input_db: -120.0, output_db: -120.0, reduction_db: 0.0 };
        assert_eq!(meter_repaint(false, true, silent), None);
        assert_eq!(meter_repaint(true, true, silent), Some(METER_REPAINT));
        assert_eq!(meter_repaint(true, false, silent), None);
        assert_eq!(meter_repaint(true, false, MeterLevels { input_db: -40.0, ..silent }), Some(METER_REPAINT));
        assert_eq!(meter_repaint(true, false, MeterLevels { output_db: -40.0, ..silent }), Some(METER_REPAINT));
        assert_eq!(meter_repaint(true, false, MeterLevels { reduction_db: -3.0, ..silent }), Some(METER_REPAINT));
        assert_eq!(meter_repaint(false, false, MeterLevels { reduction_db: -3.0, ..silent }), None);
    }

    #[test]
    fn layout_puts_the_band_beside_or_below() {
        let rect = Rect::from_min_size(pos2(0.0, 0.0), vec2(560.0, 150.0));
        let (frame, meters, band) = graph_layout(rect, true);
        assert_eq!(frame, Rect::from_min_size(pos2(0.0, 0.0), vec2(150.0, 150.0)));
        assert_eq!(meters.left(), 150.0 + theme::SP_5);
        assert_eq!(meters.width(), METER_COL_W);
        let band = band.expect("the band fits beside");
        assert_eq!(band.left(), meters.right() + theme::SP_5);
        assert_eq!(band.right(), 560.0);
        assert_eq!(total_height(560.0, 150.0, true), 150.0);
        let narrow = Rect::from_min_size(pos2(0.0, 0.0), vec2(420.0, 150.0));
        assert_eq!(graph_layout(narrow, true).2, None);
        assert_eq!(total_height(420.0, 150.0, true), 150.0 + theme::SP_3 + BAND_H);
        assert_eq!(graph_layout(rect, false).2, None);
        assert_eq!(total_height(420.0, 150.0, false), 150.0);
        // never smaller than 60 points
        assert_eq!(graph_layout(Rect::from_min_size(pos2(0.0, 0.0), vec2(100.0, 150.0)), false).0.width(), 60.0);
    }
}
