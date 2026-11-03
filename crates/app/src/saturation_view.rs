//! The graph of the saturation windows (Overdrive, Bitcrusher, Exciter): transfer curve with the live input level on
//! it, input and output meters, drive dragged on the curve. The math is plain functions the tests cover;
//! saturation_graph draws them and turns the mouse into edits.

use std::collections::BTreeMap;

use eframe::egui::{self, Rect, Vec2};
use ondina_core::model::{Id, PluginInstance};
use ondina_core::plugins;
use ondina_dsp::effects::SaturationCurve;
use ondina_dsp::util::gain_to_db;

use crate::dynamics_view::{MeterLevels, SILENCE_DB, WHEEL_NOTCH};
use crate::eq_view;
use crate::theme;
use crate::widgets::format_param_value;

/// The three saturation kinds and what the mouse moves on their curve.
#[derive(Debug)]
pub struct SatSpec {
    pub kind: &'static str,
    /// Moved by dragging up and down on the curve.
    pub drag_param: &'static str,
    /// Whether dragging up raises drag_param (the Bitcrusher's Resolution goes down: up is always "more effect").
    pub up_raises: bool,
    /// Moved by the wheel over the curve, by wheel_step per notch.
    pub wheel_param: &'static str,
    pub wheel_step: f32,
    /// The input and output range of the graph (dB), the same on both axes.
    pub span: (f32, f32),
}

static SPECS: [SatSpec; 3] = [
    SatSpec { kind: "overdrive", drag_param: "drive", up_raises: true, wheel_param: "output", wheel_step: 1.0, span: (-48.0, 12.0) },
    SatSpec { kind: "bitcrusher", drag_param: "bits", up_raises: false, wheel_param: "downsample", wheel_step: 1.0, span: (-90.0, 6.0) },
    SatSpec { kind: "exciter", drag_param: "amount", up_raises: true, wheel_param: "mix", wheel_step: 0.05, span: (-48.0, 12.0) },
];

/// Vertical pointer points that sweep the drag parameter across its whole range.
pub const DRAG_POINTS: f32 = 240.0;
/// Width of the meter column: two columns of 36 (IN, OUT).
pub const SAT_METER_COL_W: f32 = 72.0;
/// Most Bitcrusher steps drawn with vertical risers (finer steps are drawn as the sampled curve).
pub const MAX_STEP_EDGES: usize = 64;

/// Whether plug-in kind `kind` has the saturation graph.
pub fn has_saturation_view(kind: &str) -> bool {
    spec(kind).is_some()
}

/// What the mouse moves on the curve of kind `kind`; None for a kind without the saturation graph.
pub fn spec(kind: &str) -> Option<&'static SatSpec> {
    SPECS.iter().find(|s| s.kind == kind)
}

/// The transfer curve of `inst`, read from its parameters as the processor reads them.
pub fn curve_of(inst: &PluginInstance) -> Option<SaturationCurve> {
    SaturationCurve::of(&inst.kind, |id| inst.param(id))
}

// ---------------------------------------------------------------------------- curve

/// `n` inputs evenly spaced across `span` (dB) with the level `out` gives for each.
fn sampled(span: (f32, f32), n: usize, out: impl Fn(f32) -> f32) -> Vec<(f32, f32)> {
    let n = n.max(2);
    (0..n)
        .map(|i| {
            let x = span.0 + (span.1 - span.0) * i as f32 / (n - 1) as f32;
            (x, out(x))
        })
        .collect()
}

/// The curve as (input, output) dB pairs on the positive half-wave: `n` inputs evenly spaced across `span` and, for
/// the Bitcrusher, both ends of every step inside the span, so the risers are drawn vertical.
pub fn curve_points(c: &SaturationCurve, span: (f32, f32), n: usize) -> Vec<(f32, f32)> {
    let mut pts = sampled(span, n, |x| c.out_db(x));
    let mut risers = Vec::new();
    for edge in c.step_edges(MAX_STEP_EDGES) {
        let x = gain_to_db(edge);
        if x > span.0 && x < span.1 {
            risers.push((x, gain_to_db(c.out(edge * (1.0 - 1e-6)).abs())));
            risers.push((x, gain_to_db(c.out(edge).abs())));
        }
    }
    if !risers.is_empty() {
        pts.extend(risers);
        // stable: the two ends of a riser keep their order
        pts.sort_by(|a, b| a.0.total_cmp(&b.0));
    }
    pts
}

/// The negative half-wave of an asymmetric curve (the Overdrive's Fuzz and Tube); None when both halves match.
pub fn negative_points(c: &SaturationCurve, span: (f32, f32), n: usize) -> Option<Vec<(f32, f32)>> {
    (!c.symmetric()).then(|| sampled(span, n, |x| c.out_db_negative(x)))
}

/// The Overdrive's curves of its other three modes with the same Drive, Output and Mix, to compare the characters by
/// eye; empty for the other kinds.
pub fn ghost_curves(inst: &PluginInstance, span: (f32, f32), n: usize) -> Vec<Vec<(f32, f32)>> {
    let Some(SaturationCurve::Overdrive { mode, pre, post, mix }) = curve_of(inst) else {
        return Vec::new();
    };
    // any mode past the last reads as Soft, like the processor
    let current = if mode > 3 { 0 } else { mode };
    (0..=3u8).filter(|m| *m != current).map(|m| curve_points(&SaturationCurve::Overdrive { mode: m, pre, post, mix }, span, n)).collect()
}

/// Where the live input level sits on the curve (input, output dB); None in silence and in Bypass.
pub fn live_point(c: &SaturationCurve, levels: MeterLevels, bypass: bool) -> Option<(f32, f32)> {
    (levels.input_db > SILENCE_DB && !bypass).then(|| (levels.input_db, c.out_db(levels.input_db)))
}

/// How many dB louder (positive) or quieter the curve makes a steady input at `in_db`.
pub fn level_change_db(c: &SaturationCurve, in_db: f32) -> f32 {
    c.out_db(in_db) - in_db
}

// ---------------------------------------------------------------------------- interaction

/// The drag parameter after a vertical drag that started at `start` and has moved `moved_y` points (up is
/// negative), clamped to its registry range: DRAG_POINTS sweep the whole range, up is more effect.
pub fn drag_value(inst: &PluginInstance, start: f32, moved_y: f32) -> BTreeMap<String, f32> {
    let Some(s) = spec(&inst.kind) else {
        return BTreeMap::new();
    };
    let (lo, hi) = eq_view::range_of(&inst.kind, s.drag_param);
    let sign = if s.up_raises { 1.0 } else { -1.0 };
    let v = start + (-moved_y / DRAG_POINTS) * (hi - lo) * sign;
    BTreeMap::from([(s.drag_param.to_string(), v.clamp(lo, hi))])
}

/// The wheel parameter after a turn of `scroll` points (x and y summed: egui turns Shift+wheel into x): wheel_step
/// per notch, up raises it, Shift moves ten times less. None when nothing changes.
pub fn wheel_values(inst: &PluginInstance, scroll: Vec2, fine: bool) -> Option<BTreeMap<String, f32>> {
    let s = spec(&inst.kind)?;
    let notches = (scroll.x + scroll.y) / WHEEL_NOTCH * if fine { eq_view::FINE } else { 1.0 };
    if !notches.is_finite() {
        return None;
    }
    let id = s.wheel_param;
    let v = inst.param(id);
    let (lo, hi) = eq_view::range_of(&inst.kind, id);
    let next = (v + notches * s.wheel_step).clamp(lo, hi);
    (next != v).then(|| BTreeMap::from([(id.to_string(), next)]))
}

/// The two parameters of the gestures (drag and wheel) back to their defaults, as one change.
pub fn point_reset(inst: &PluginInstance) -> BTreeMap<String, f32> {
    let Some(s) = spec(&inst.kind) else {
        return BTreeMap::new();
    };
    let defaults = plugins::default_params(inst);
    [s.drag_param, s.wheel_param].into_iter().filter_map(|id| defaults.get(id).map(|v| (id.to_string(), *v))).collect()
}

/// The undo key of a drag on the curve in plug-in `pid`'s window: one key per gesture.
pub fn drag_key(pid: Id, gesture: u64) -> String {
    format!("pw{pid}:sat#{gesture}")
}

/// The undo key of the wheel over the curve: turns close together merge.
pub fn wheel_key(pid: Id) -> String {
    format!("pw{pid}:sat:wheel")
}

/// The parameters that shape the curve of `kind`, in the order the readout names them.
fn curve_params(kind: &str) -> &'static [&'static str] {
    match kind {
        "overdrive" => &["mode", "drive", "output"],
        "bitcrusher" => &["bits", "downsample"],
        "exciter" => &["freq", "amount", "mix"],
        _ => &[],
    }
}

/// "Mode Soft · Drive 0.40 · Output -3.0 dB": the curve's parameters with their registry names and units (a
/// selector by the name of its choice).
pub fn readout(inst: &PluginInstance) -> String {
    let params = plugins::find(&inst.kind).map_or(&[][..], |i| i.params);
    curve_params(&inst.kind)
        .iter()
        .filter_map(|id| params.iter().find(|p| p.id == *id))
        .map(|p| {
            let v = inst.param(p.id);
            let value = if p.choices.is_empty() {
                format_param_value(v, p.unit)
            } else {
                p.choices.get(v.round().max(0.0) as usize).copied().unwrap_or(p.choices[0]).to_string()
            };
            format!("{} {value}", p.name)
        })
        .collect::<Vec<_>>()
        .join(" \u{00B7} ")
}

/// "in -12.0 dB → out -6.5 dB (+5.5 dB)" for the input under the pointer.
pub fn hover_readout(c: &SaturationCurve, in_db: f32) -> String {
    format!(
        "in {} \u{2192} out {} ({})",
        format_param_value(in_db, "dB"),
        format_param_value(c.out_db(in_db), "dB"),
        format_param_value(level_change_db(c, in_db), "dB")
    )
}

/// The help line under the graph.
pub fn hint(kind: &str) -> &'static str {
    match kind {
        "overdrive" => "Drag up and down on the curve: drive (Shift: fine) \u{00B7} Wheel: output \u{00B7} Double-click: default \u{00B7} Faint lines: the other modes",
        "bitcrusher" => "Drag up and down on the curve: resolution, up is fewer bits (Shift: fine) \u{00B7} Wheel: downsampling \u{00B7} Double-click: default",
        "exciter" => "The curve of the band above Frequency \u{00B7} Drag up and down: harmonics (Shift: fine) \u{00B7} Wheel: mix \u{00B7} Double-click: default",
        _ => "",
    }
}

// ---------------------------------------------------------------------------- layout

/// The square of the curve on the left of `rect` and the meter column beside it.
pub fn graph_layout(rect: Rect) -> (Rect, Rect) {
    let side = rect.height().min(rect.width() - SAT_METER_COL_W - theme::SP_5).max(60.0);
    let frame = Rect::from_min_size(rect.min, Vec2::splat(side));
    let meters = Rect::from_min_size(egui::pos2(frame.right() + theme::SP_5, rect.top()), egui::vec2(SAT_METER_COL_W, side));
    (frame, meters)
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

    fn near(a: f32, b: f32, tol: f32) -> bool {
        (a - b).abs() <= tol
    }

    const KINDS: [&str; 3] = ["overdrive", "bitcrusher", "exciter"];

    #[test]
    fn has_saturation_view_only_for_the_three_kinds() {
        for kind in KINDS {
            assert!(has_saturation_view(kind), "{kind}");
            assert_eq!(spec(kind).map(|s| s.kind), Some(kind));
            assert!(curve_of(&instance(kind)).is_some(), "{kind}");
        }
        for kind in ["compressor", "tape_delay", "channel_eq", "vst3", "organ"] {
            assert!(!has_saturation_view(kind), "{kind}");
            assert!(spec(kind).is_none() && curve_of(&instance(kind)).is_none(), "{kind}");
        }
    }

    #[test]
    fn specs_name_registry_parameters() {
        for kind in KINDS {
            let s = spec(kind).unwrap();
            let info = plugins::find(kind).unwrap();
            for id in [s.drag_param, s.wheel_param] {
                assert!(info.params.iter().any(|p| p.id == id), "{kind}: {id}");
            }
            assert!(s.span.0 < s.span.1, "{kind}");
            assert!(s.wheel_step > 0.0, "{kind}");
        }
    }

    #[test]
    fn curve_points_follow_the_dsp_curve() {
        let inst = with(instance("overdrive"), &[("mode", 3.0), ("drive", 0.7)]);
        let c = curve_of(&inst).unwrap();
        let span = spec("overdrive").unwrap().span;
        let pts = curve_points(&c, span, 50);
        assert_eq!(pts.len(), 50);
        assert_eq!(pts.first().unwrap().0, span.0);
        assert!(near(pts.last().unwrap().0, span.1, 1e-4));
        for (i, o) in &pts {
            assert_eq!(*o, c.out_db(*i), "at {i} dB");
        }
    }

    #[test]
    fn bitcrusher_points_have_vertical_steps() {
        let span = spec("bitcrusher").unwrap().span;
        let c = curve_of(&with(instance("bitcrusher"), &[("bits", 4.0)])).unwrap();
        let pts = curve_points(&c, span, 100);
        assert!(pts.windows(2).any(|w| w[0].0 == w[1].0 && w[0].1 != w[1].1), "no vertical step in {pts:?}");
        assert!(pts.windows(2).all(|w| w[0].0 <= w[1].0), "inputs go left to right");
        let fine = curve_of(&with(instance("bitcrusher"), &[("bits", 24.0)])).unwrap();
        assert!(curve_points(&fine, span, 100).len() <= 100 + 2 * MAX_STEP_EDGES);
    }

    #[test]
    fn asymmetric_modes_draw_the_negative_half() {
        let span = spec("overdrive").unwrap().span;
        for (mode, asymmetric) in [(0.0, false), (1.0, false), (2.0, true), (3.0, true)] {
            let c = curve_of(&with(instance("overdrive"), &[("mode", mode)])).unwrap();
            let neg = negative_points(&c, span, 40);
            assert_eq!(neg.is_some(), asymmetric, "mode {mode}");
            if let Some(neg) = neg {
                assert!(neg.iter().all(|(i, o)| *o == c.out_db_negative(*i)));
            }
        }
        for kind in ["bitcrusher", "exciter"] {
            assert!(negative_points(&curve_of(&instance(kind)).unwrap(), (-48.0, 12.0), 40).is_none(), "{kind}");
        }
    }

    #[test]
    fn the_overdrive_shows_the_other_modes() {
        let span = spec("overdrive").unwrap().span;
        let inst = with(instance("overdrive"), &[("mode", 1.0)]);
        let current = curve_points(&curve_of(&inst).unwrap(), span, 60);
        let ghosts = ghost_curves(&inst, span, 60);
        assert_eq!(ghosts.len(), 3);
        for g in &ghosts {
            assert_ne!(g, &current);
        }
        assert!(ghost_curves(&instance("bitcrusher"), span, 60).is_empty());
        assert!(ghost_curves(&instance("exciter"), span, 60).is_empty());
    }

    #[test]
    fn live_point_sits_on_the_curve() {
        let c = curve_of(&instance("overdrive")).unwrap();
        let loud = MeterLevels { input_db: -12.0, output_db: -6.0, reduction_db: 0.0 };
        assert_eq!(live_point(&c, loud, false), Some((-12.0, c.out_db(-12.0))));
        assert_eq!(live_point(&c, MeterLevels { input_db: -120.0, ..loud }, false), None);
        assert_eq!(live_point(&c, loud, true), None);
    }

    #[test]
    fn drag_value_moves_the_drag_param() {
        let od = instance("overdrive");
        let up = drag_value(&od, 0.4, -24.0);
        assert_eq!(up.len(), 1);
        assert!(near(up["drive"], 0.5, 1e-5), "{up:?}");
        assert_eq!(drag_value(&od, 0.4, -1000.0)["drive"], 1.0);
        let bc = drag_value(&instance("bitcrusher"), 8.0, -24.0);
        assert_eq!(bc.len(), 1);
        assert!(near(bc["bits"], 8.0 - 2.3, 1e-4), "up is fewer bits: {bc:?}");
        let ex = drag_value(&instance("exciter"), 0.4, -24.0);
        assert_eq!(ex.len(), 1);
        assert!(ex["amount"] > 0.4, "{ex:?}");
        assert!(drag_value(&instance("reverb"), 0.4, -24.0).is_empty());
    }

    #[test]
    fn wheel_values_step_each_kind() {
        let notch = vec2(0.0, 40.0);
        assert!(near(wheel_values(&instance("overdrive"), notch, false).unwrap()["output"], -2.0, 1e-5));
        assert!(near(wheel_values(&instance("bitcrusher"), notch, false).unwrap()["downsample"], 5.0, 1e-5));
        assert!(near(wheel_values(&instance("exciter"), notch, false).unwrap()["mix"], 0.35, 1e-5));
        assert!(near(wheel_values(&instance("overdrive"), notch, true).unwrap()["output"], -2.9, 1e-5));
        assert!(near(wheel_values(&instance("exciter"), notch, true).unwrap()["mix"], 0.305, 1e-5));
        assert!(wheel_values(&with(instance("overdrive"), &[("output", 12.0)]), notch, false).is_none());
        assert!(wheel_values(&instance("overdrive"), vec2(0.0, f32::NAN), false).is_none());
        assert!(wheel_values(&instance("reverb"), notch, false).is_none());
    }

    #[test]
    fn point_reset_uses_the_registry_defaults() {
        let inst = with(instance("overdrive"), &[("drive", 0.9), ("output", 6.0)]);
        let reset = point_reset(&inst);
        assert_eq!(reset, BTreeMap::from([("drive".to_string(), 0.4), ("output".to_string(), -3.0)]));
    }

    #[test]
    fn keys_have_the_window_format() {
        assert_eq!(drag_key(3, 1), "pw3:sat#1");
        assert_eq!(wheel_key(3), "pw3:sat:wheel");
    }

    #[test]
    fn readouts_name_values_and_units() {
        assert_eq!(readout(&instance("overdrive")), "Mode Soft \u{00B7} Drive 0.40 \u{00B7} Output -3.0 dB");
        assert!(readout(&with(instance("overdrive"), &[("mode", 3.0)])).starts_with("Mode Tube"));
        let bc = readout(&instance("bitcrusher"));
        assert!(bc.contains("Resolution 8 bit") && bc.contains("Downsampling 4.00x"), "{bc}");
        let ex = readout(&instance("exciter"));
        assert!(ex.contains("Frequency 4.00 kHz") && ex.contains("Harmonics 0.40") && ex.contains("Mix 0.30"), "{ex}");
        let c = curve_of(&instance("overdrive")).unwrap();
        let h = hover_readout(&c, -12.0);
        assert!(h.contains("in -12.0 dB") && h.contains("out "), "{h}");
        assert!(h.contains(&format!("({})", format_param_value(level_change_db(&c, -12.0), "dB"))), "{h}");
        assert!(h.contains("(+"), "the overdrive raises -12 dB: {h}");
    }

    #[test]
    fn level_change_is_out_minus_in() {
        for kind in KINDS {
            let c = curve_of(&instance(kind)).unwrap();
            for x in [-40.0, -12.0, -3.0, 0.0] {
                assert_eq!(level_change_db(&c, x), c.out_db(x) - x, "{kind} at {x}");
            }
        }
    }

    #[test]
    fn hints_say_what_the_mouse_does() {
        assert!(hint("overdrive").contains("drive") && hint("overdrive").contains("output"));
        assert!(hint("bitcrusher").contains("fewer bits"));
        assert!(hint("exciter").contains("band above Frequency"));
        assert_eq!(hint("reverb"), "");
    }

    #[test]
    fn layout_puts_the_meters_beside_the_curve() {
        let rect = Rect::from_min_size(pos2(0.0, 0.0), vec2(560.0, 200.0));
        let (frame, meters) = graph_layout(rect);
        assert_eq!(frame, Rect::from_min_size(pos2(0.0, 0.0), vec2(200.0, 200.0)));
        assert_eq!(meters.left(), 200.0 + theme::SP_5);
        assert_eq!(meters.width(), SAT_METER_COL_W);
        assert_eq!(meters.height(), 200.0);
        // never smaller than 60 points
        assert_eq!(graph_layout(Rect::from_min_size(pos2(0.0, 0.0), vec2(100.0, 150.0))).0.width(), 60.0);
    }
}
