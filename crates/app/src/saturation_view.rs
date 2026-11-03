//! The graph of the saturation windows (Overdrive, Bitcrusher, Exciter): transfer curve with the live input level on
//! it, input and output meters, drive dragged on the curve. The math is plain functions the tests cover;
//! saturation_graph draws them and turns the mouse into edits.

use std::collections::BTreeMap;

use eframe::egui::{self, CursorIcon, PointerButton, Pos2, Rect, Sense, Stroke, Vec2};
use ondina_core::model::{Id, PluginInstance};
use ondina_core::plugins;
use ondina_dsp::effects::SaturationCurve;
use ondina_dsp::util::gain_to_db;
use ondina_engine::PluginMeter;

use crate::dynamics_view::{db_text, db_to_x, db_to_y, hold_line, meter_levels, x_to_db, MeterLevels, METER_BAR_W, SILENCE_DB, WHEEL_NOTCH};
use crate::eq_view::{self, EqEdit};
use crate::theme;
use crate::widgets::{format_param_value, hold_marker_shown, meter_fill, paint_meter_fill, PeakHold};

/// Fill of the zone where the curve is above the diagonal (the plug-in raises the level): amber.
const RAISE_ZONE: egui::Color32 = theme::SOLO;
/// Fill of the zone under the diagonal (the plug-in squashes the level): light blue, far from the amber above.
const SQUASH_ZONE: egui::Color32 = theme::ACCENT;

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


// ---------------------------------------------------------------------------- drawing and mouse

/// What a frame of the saturation graph asks for and showed.
pub struct SatGraphOut {
    /// The plot of the curve (tests start their gestures in it).
    #[cfg(test)]
    pub plot: Rect,
    pub edits: Vec<EqEdit>,
    /// What the meters showed: the window asks for a repaint while they move.
    pub levels: MeterLevels,
}

/// The drag on the curve going on, kept in egui's memory between frames.
#[derive(Clone, Copy)]
struct SatDrag {
    start: f32,
    moved: f32,
}

/// The peak markers of the two meters.
#[derive(Clone, Copy, Default)]
struct MeterHolds {
    input: PeakHold,
    output: PeakHold,
}

/// Whether `params` would change any value of `inst`.
fn changes(inst: &PluginInstance, params: &BTreeMap<String, f32>) -> bool {
    params.iter().any(|(k, v)| inst.param(k) != *v)
}

/// The points of a curve in (input, output) dB on the plot.
fn screen_line(pts: &[(f32, f32)], span: (f32, f32), plot: Rect) -> Vec<Pos2> {
    pts.iter().map(|(i, o)| egui::pos2(db_to_x(*i, span, plot), db_to_y(*o, span, plot))).collect()
}

/// Draws the saturation graph of `inst` across the available width, `height` points tall, reading the insert's live
/// `meter`, and turns what the mouse does into edits. `gesture` is the window's drag counter
/// (`PluginWindows::gesture`).
pub fn saturation_graph(ui: &mut egui::Ui, inst: &PluginInstance, meter: Option<PluginMeter>, height: f32, gesture: &mut u64) -> SatGraphOut {
    let levels = meter_levels(meter, inst.bypass);
    let (Some(spec), Some(curve)) = (spec(&inst.kind), curve_of(inst)) else {
        return SatGraphOut {
            #[cfg(test)]
            plot: Rect::NOTHING,
            edits: Vec::new(),
            levels,
        };
    };
    let span = spec.span;
    // the background first, hover only: the gesture surface registered after it takes the clicks
    let width = ui.available_width();
    let (rect, _) = ui.allocate_exact_size(egui::vec2(width, height), Sense::hover());
    let (frame, meters) = graph_layout(rect);
    let plot = frame.shrink(eq_view::PLOT_INSET);
    ui.interact(meters, ui.id().with("sat-meters"), Sense::hover()).on_hover_text("Input peak \u{00B7} Output peak");

    // the whole curve square is the surface of the gesture
    let fine = ui.input(|i| i.modifiers.shift);
    let mut edits = Vec::new();
    let drag_id = egui::Id::new(("sat-drag", inst.id));
    let mut drag: Option<SatDrag> = ui.data(|d| d.get_temp(drag_id));
    let r = ui.interact(frame, ui.id().with("sat-curve"), Sense::click_and_drag());
    if r.drag_started_by(PointerButton::Primary) {
        *gesture += 1;
        drag = Some(SatDrag { start: inst.param(spec.drag_param), moved: 0.0 });
    }
    let mut dragging = false;
    if r.dragged_by(PointerButton::Primary) {
        if let Some(d) = drag.as_mut() {
            d.moved = eq_view::accumulate(Vec2::new(0.0, d.moved), r.drag_delta(), fine).y;
            let params = drag_value(inst, d.start, d.moved);
            if changes(inst, &params) {
                edits.push(EqEdit::Drag { params, key: drag_key(inst.id, *gesture) });
            }
            dragging = true;
        }
    }
    if r.drag_stopped() {
        drag = None;
    }
    if r.double_clicked() {
        edits.push(EqEdit::Set { params: point_reset(inst) });
    }
    if r.hovered() || dragging {
        ui.ctx().set_cursor_icon(CursorIcon::ResizeVertical);
    }
    if r.hovered() {
        let s = ui.input(|i| i.raw_scroll_delta);
        if s != Vec2::ZERO {
            if let Some(params) = wheel_values(inst, s, fine) {
                edits.push(EqEdit::Drag { params, key: wheel_key(inst.id) });
            }
            // the wheel was for this curve: the knobs under the graph do not scroll with it
            ui.input_mut(|i| {
                i.raw_scroll_delta = Vec2::ZERO;
                i.smooth_scroll_delta = Vec2::ZERO;
            });
        }
    }
    ui.data_mut(|m| match drag {
        Some(d) => m.insert_temp(drag_id, d),
        None => m.remove::<SatDrag>(drag_id),
    });

    let painter = ui.painter_at(rect);
    // grid, 0 dBFS in, diagonal, the other modes, what the plug-in adds and takes away, the curve, the live level
    painter.rect(frame, theme::R_LG, theme::INSET, Stroke::new(1.0_f32, theme::BORDER));
    let label_every = if span.1 - span.0 > 60.0 { 30 } else { 12 };
    let steps = ((span.1 - span.0) / 6.0).round() as i32;
    for k in 0..=steps {
        let db = span.1 - 6.0 * k as f32;
        let color = if k % 2 == 0 { theme::GRID_BAR_LINE } else { theme::GRID_BEAT_LINE };
        let (x, y) = (db_to_x(db, span, plot), db_to_y(db, span, plot));
        painter.vline(x, plot.y_range(), Stroke::new(1.0_f32, color));
        painter.hline(plot.x_range(), y, Stroke::new(1.0_f32, color));
        if (k * 6) % label_every == 0 && k < steps {
            let text = format!("{db:.0}");
            painter.text(egui::pos2(plot.left() + theme::SP_1, y + theme::SP_1), egui::Align2::LEFT_TOP, &text, theme::label_font(), theme::TEXT_3);
            if k > 0 {
                painter.text(egui::pos2(x - theme::SP_1, plot.bottom() - theme::SP_1), egui::Align2::RIGHT_BOTTOM, text, theme::label_font(), theme::TEXT_3);
            }
        }
    }
    if span.0 < 0.0 && span.1 > 0.0 {
        let zx = db_to_x(0.0, span, plot);
        painter.extend(egui::Shape::dashed_line(&[egui::pos2(zx, plot.top()), egui::pos2(zx, plot.bottom())], Stroke::new(1.0_f32, theme::GRID_BAR_LINE), 4.0, 3.0));
    }
    painter.line_segment([egui::pos2(plot.left(), plot.bottom()), egui::pos2(plot.right(), plot.top())], Stroke::new(1.0_f32, theme::CENTER_LINE));
    let n = (plot.width() / 2.0).clamp(48.0, 300.0) as usize;
    for ghost in ghost_curves(inst, span, n) {
        painter.add(egui::Shape::line(screen_line(&ghost, span, plot), Stroke::new(1.0_f32, eq_view::faded(theme::TEXT_3, 70))));
    }
    let pts = curve_points(&curve, span, n);
    if !inst.bypass {
        let xs: Vec<f32> = pts.iter().map(|(i, _)| db_to_x(*i, span, plot)).collect();
        let diag: Vec<f32> = pts.iter().map(|(i, _)| db_to_y(*i, span, plot)).collect();
        let ys: Vec<f32> = pts.iter().map(|(_, o)| db_to_y(*o, span, plot)).collect();
        // above the diagonal: how much the plug-in raises the level; under it: how much it squashes
        let above: Vec<f32> = ys.iter().zip(&diag).map(|(y, d)| y.min(*d)).collect();
        let under: Vec<f32> = ys.iter().zip(&diag).map(|(y, d)| y.max(*d)).collect();
        eq_view::fill_between(&painter, &xs, &above, &diag, eq_view::faded(RAISE_ZONE, 40));
        eq_view::fill_between(&painter, &xs, &diag, &under, eq_view::faded(SQUASH_ZONE, 40));
    }
    let curve_color = if inst.bypass { theme::TEXT_DISABLED } else { theme::TEXT };
    painter.add(egui::Shape::line(screen_line(&pts, span, plot), Stroke::new(2.0_f32, curve_color)));
    if let Some(neg) = negative_points(&curve, span, n) {
        painter.extend(egui::Shape::dashed_line(&screen_line(&neg, span, plot), Stroke::new(1.5_f32, curve_color), 6.0, 4.0));
    }
    if let Some((i, o)) = live_point(&curve, levels, inst.bypass) {
        painter.circle_filled(egui::pos2(db_to_x(i, span, plot), db_to_y(o, span, plot)), 4.0, theme::PLAY);
    }
    if let Some(p) = r.hover_pos().or_else(|| dragging.then(|| r.interact_pointer_pos()).flatten()) {
        let in_db = x_to_db(p.x, span, plot);
        let on_curve = (db_to_y(curve.out_db(in_db), span, plot) - p.y).abs() <= eq_view::HIT_R;
        if dragging || on_curve {
            // the curve's parameters beside the pointer; they may run over the meters, wider than a small curve
            let galley = painter.layout_no_wrap(readout(inst), theme::number_font(), theme::TEXT);
            let size = galley.size() + Vec2::splat(2.0 * theme::SP_3);
            let room = Rect::from_min_max(plot.min, egui::pos2(rect.right(), plot.bottom()));
            let b = eq_view::readout_rect(p, size, room);
            painter.rect(b, theme::R_LG, theme::RAISED, Stroke::new(1.0_f32, theme::BORDER));
            painter.galley(b.min + Vec2::splat(theme::SP_3), galley, theme::TEXT);
        } else {
            let text = hover_readout(&curve, in_db);
            painter.text(egui::pos2(plot.left() + theme::SP_3, plot.top() + theme::SP_3), egui::Align2::LEFT_TOP, text, theme::number_font(), theme::TEXT_3);
        }
    }

    // the meters: input and output rise from the bottom; under them, how much the output differs from the input
    let holds_id = egui::Id::new(("sat-holds", inst.id));
    let mut holds: MeterHolds = ui.data(|d| d.get_temp(holds_id)).unwrap_or_default();
    let dt = ui.input(|i| i.stable_dt).min(0.1);
    holds.input.update(levels.input_db, dt);
    holds.output.update(levels.output_db, dt);
    ui.data_mut(|d| d.insert_temp(holds_id, holds));
    let col_w = SAT_METER_COL_W / 2.0;
    let text_h = 14.0;
    for (k, (label, db, hold)) in [("IN", levels.input_db, holds.input), ("OUT", levels.output_db, holds.output)].into_iter().enumerate() {
        let cx = meters.left() + col_w * (k as f32 + 0.5);
        painter.text(egui::pos2(cx, meters.top()), egui::Align2::CENTER_TOP, label, theme::label_font(), theme::TEXT_3);
        let bar = Rect::from_min_max(egui::pos2(cx - METER_BAR_W / 2.0, meters.top() + text_h), egui::pos2(cx + METER_BAR_W / 2.0, meters.bottom() - 2.0 * text_h));
        painter.rect_filled(bar, theme::R_SM, theme::LCD);
        paint_meter_fill(&painter, bar, meter_fill(db));
        if hold_marker_shown(hold.db) {
            hold_line(&painter, bar, bar.bottom() - meter_fill(hold.db) * bar.height(), if hold.db > 0.0 { theme::REC } else { theme::SOLO });
        }
        painter.text(egui::pos2(cx, meters.bottom() - text_h), egui::Align2::CENTER_BOTTOM, db_text(db), theme::number_font(), theme::TEXT_2);
    }
    if levels.input_db > SILENCE_DB {
        let change = format_param_value(levels.output_db - levels.input_db, "dB");
        painter.text(egui::pos2(meters.center().x, meters.bottom()), egui::Align2::CENTER_BOTTOM, change, theme::number_font(), theme::TEXT_2);
    }

    let mut help = hint(spec.kind).to_string();
    if !curve.symmetric() {
        help.push_str(" \u{00B7} Dashed: the negative half");
    }
    ui.label(egui::RichText::new(help).small().color(theme::TEXT_3));
    SatGraphOut {
        #[cfg(test)]
        plot,
        edits,
        levels,
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

    /// What the harness saw over all its frames.
    struct Run {
        plot: Rect,
        edits: Vec<EqEdit>,
        levels: Option<MeterLevels>,
    }

    /// Frames of the saturation graph in a headless 700x500 context, one per entry of `frames` (the events of that
    /// frame), with `modifiers` held throughout.
    fn run_frames(inst: &PluginInstance, meter: Option<PluginMeter>, gesture: &mut u64, frames: &[Vec<Event>], modifiers: Modifiers) -> Run {
        let ctx = egui::Context::default();
        theme::apply(&ctx);
        let mut run = Run { plot: Rect::NOTHING, edits: Vec::new(), levels: None };
        for (k, events) in frames.iter().enumerate() {
            let raw = egui::RawInput {
                screen_rect: Some(Rect::from_min_size(Pos2::ZERO, vec2(700.0, 500.0))),
                time: Some(k as f64 / 60.0),
                events: events.clone(),
                modifiers,
                ..Default::default()
            };
            let _ = ctx.run(raw, |ctx| {
                egui::CentralPanel::default().show(ctx, |ui| {
                    let out = saturation_graph(ui, inst, meter, 240.0, gesture);
                    run.plot = out.plot;
                    run.edits.extend(out.edits);
                    run.levels = Some(out.levels);
                });
            });
        }
        run
    }

    /// The middle of the plot of `inst` in the harness: where the gestures start.
    fn center(inst: &PluginInstance) -> Pos2 {
        run_frames(inst, None, &mut 0, &[vec![]], Modifiers::NONE).plot.center()
    }

    fn press(pos: Pos2, pressed: bool, modifiers: Modifiers) -> Event {
        Event::PointerButton { pos, button: PointerButton::Primary, pressed, modifiers }
    }

    /// A drag from `from` in three steps of `step`, then the release.
    fn drag(from: Pos2, step: Vec2, modifiers: Modifiers) -> Vec<Vec<Event>> {
        let mut frames = vec![vec![], vec![Event::PointerMoved(from)], vec![press(from, true, modifiers)]];
        for k in 1..=3 {
            frames.push(vec![Event::PointerMoved(from + step * k as f32)]);
        }
        frames.push(vec![press(from + step * 3.0, false, modifiers)]);
        frames
    }

    /// The parameters of every drag edit with undo key `want`.
    fn drags(edits: &[EqEdit], want: &str) -> Vec<BTreeMap<String, f32>> {
        edits.iter().filter_map(|e| match e {
            EqEdit::Drag { params, key } if key == want => Some(params.clone()),
            _ => None,
        }).collect()
    }

    #[test]
    fn saturation_graph_draws_every_kind_without_edits() {
        let extremes: [(&str, &[(&str, f32)]); 3] = [
            ("overdrive", &[("mode", 2.0), ("drive", 1.0), ("output", 12.0)]),
            ("bitcrusher", &[("bits", 1.0), ("downsample", 64.0)]),
            ("exciter", &[("amount", 1.0), ("mix", 1.0)]),
        ];
        let loud = PluginMeter { input_peak: 0.9, output_peak: 0.5, gain_reduction_db: None };
        for (kind, extreme) in extremes {
            let base = instance(kind);
            let bypassed = PluginInstance { bypass: true, ..base.clone() };
            for inst in [base.clone(), with(base.clone(), extreme), bypassed] {
                for meter in [None, Some(loud)] {
                    let mut g = 0;
                    let run = run_frames(&inst, meter, &mut g, &[vec![], vec![]], Modifiers::NONE);
                    assert!(run.edits.is_empty(), "{kind}: {:?}", run.edits);
                    assert_eq!(g, 0);
                    assert!(run.plot.width() > 0.0 && run.plot.height() == 240.0 - 2.0 * eq_view::PLOT_INSET, "{kind}: {:?}", run.plot);
                    let input = run.levels.unwrap().input_db;
                    let want = if meter.is_some() { gain_to_db(0.9) } else { -120.0 };
                    assert!(near(input, want, 1e-3), "{kind} meter {meter:?}: {input}");
                }
            }
        }
        let run = run_frames(&instance("reverb"), None, &mut 0, &[vec![], vec![]], Modifiers::NONE);
        assert!(run.edits.is_empty());
        assert_eq!(run.plot, Rect::NOTHING);
    }

    #[test]
    fn dragging_on_the_curve_is_one_gesture_on_drive() {
        let inst = instance("overdrive");
        let mut g = 0;
        let run = run_frames(&inst, None, &mut g, &drag(center(&inst), vec2(0.0, -8.0), Modifiers::NONE), Modifiers::NONE);
        assert!(!run.edits.is_empty());
        assert_eq!(g, 1);
        for e in &run.edits {
            assert!(matches!(e, EqEdit::Drag { key, .. } if key == "pw3:sat#1"), "{e:?}");
        }
        let EqEdit::Drag { params, .. } = run.edits.last().unwrap() else { unreachable!() };
        assert_eq!(params.keys().collect::<Vec<_>>(), ["drive"]);
        assert!(params["drive"] > 0.4, "{params:?}");
    }

    #[test]
    fn two_drags_are_two_gestures() {
        let inst = instance("overdrive");
        let p = center(&inst);
        let mut frames = drag(p, vec2(0.0, -8.0), Modifiers::NONE);
        frames.extend(drag(p, vec2(0.0, 8.0), Modifiers::NONE));
        let mut g = 0;
        let run = run_frames(&inst, None, &mut g, &frames, Modifiers::NONE);
        assert_eq!(g, 2);
        assert!(!drags(&run.edits, "pw3:sat#1").is_empty() && !drags(&run.edits, "pw3:sat#2").is_empty(), "{:?}", run.edits);
    }

    #[test]
    fn shift_drag_moves_ten_times_less() {
        let inst = instance("overdrive");
        let p = center(&inst);
        let moved = |m: Modifiers| -> f32 {
            let run = run_frames(&inst, None, &mut 0, &drag(p, vec2(0.0, -10.0), m), m);
            drags(&run.edits, "pw3:sat#1").last().expect("a drag")["drive"] - 0.4
        };
        let (coarse, fine) = (moved(Modifiers::NONE), moved(Modifiers::SHIFT));
        assert!(coarse > 0.05, "{coarse}");
        assert!(near(fine, coarse * 0.1, 1e-3), "{fine} vs {coarse}");
    }

    #[test]
    fn the_bitcrusher_drag_lowers_the_bits() {
        let inst = instance("bitcrusher");
        let run = run_frames(&inst, None, &mut 0, &drag(center(&inst), vec2(0.0, -8.0), Modifiers::NONE), Modifiers::NONE);
        let last = drags(&run.edits, "pw3:sat#1").last().cloned().expect("a drag");
        assert_eq!(last.keys().collect::<Vec<_>>(), ["bits"]);
        assert!(last["bits"] < 8.0, "{last:?}");
    }

    #[test]
    fn wheel_over_the_curve_changes_output() {
        let wheel = Event::MouseWheel { unit: MouseWheelUnit::Line, delta: vec2(0.0, 1.0), modifiers: Modifiers::NONE };
        for (kind, id, want) in [("overdrive", "output", -2.0), ("bitcrusher", "downsample", 5.0)] {
            let inst = instance(kind);
            let p = center(&inst);
            let mut g = 0;
            let run = run_frames(&inst, None, &mut g, &[vec![], vec![Event::PointerMoved(p)], vec![wheel.clone()]], Modifiers::NONE);
            assert_eq!(run.edits.len(), 1, "{kind}: {:?}", run.edits);
            let EqEdit::Drag { params, key } = &run.edits[0] else { panic!("{:?}", run.edits) };
            assert_eq!(key, "pw3:sat:wheel");
            assert!(near(params[id], want, 1e-4), "{kind}: {params:?}");
            assert_eq!(g, 0);
        }
        // over the meter column the wheel changes nothing
        let inst = instance("overdrive");
        let plot = run_frames(&inst, None, &mut 0, &[vec![]], Modifiers::NONE).plot;
        let meters = pos2(plot.right() + eq_view::PLOT_INSET + theme::SP_5 + SAT_METER_COL_W / 2.0, plot.center().y);
        let run = run_frames(&inst, None, &mut 0, &[vec![], vec![Event::PointerMoved(meters)], vec![wheel]], Modifiers::NONE);
        assert!(run.edits.is_empty(), "{:?}", run.edits);
    }

    #[test]
    fn double_click_resets_the_curve_params() {
        let inst = with(instance("overdrive"), &[("drive", 0.9), ("output", 6.0)]);
        let p = center(&inst);
        let n = Modifiers::NONE;
        let frames = [vec![], vec![Event::PointerMoved(p)], vec![press(p, true, n)], vec![press(p, false, n)], vec![press(p, true, n)], vec![press(p, false, n)]];
        let run = run_frames(&inst, None, &mut 0, &frames, n);
        assert!(run.edits.contains(&EqEdit::Set { params: point_reset(&inst) }), "{:?}", run.edits);
    }

    #[test]
    fn the_saturation_view_reuses_the_common_header() {
        let src = include_str!("saturation_view.rs");
        assert!(!src.contains(concat!("plugin_ai_bar", "(")));
        assert!(!src.contains(concat!("plugin_header", "(")));
    }

    fn hue(c: egui::Color32) -> f32 {
        let (r, g, b) = (c.r() as f32 / 255.0, c.g() as f32 / 255.0, c.b() as f32 / 255.0);
        let (max, min) = (r.max(g).max(b), r.min(g).min(b));
        let d = max - min;
        if d <= 0.0 {
            return 0.0;
        }
        let h = if max == r { ((g - b) / d).rem_euclid(6.0) } else if max == g { (b - r) / d + 2.0 } else { (r - g) / d + 4.0 };
        h * 60.0
    }

    #[test]
    fn the_raise_and_squash_zones_have_clearly_different_hues() {
        let diff = (hue(RAISE_ZONE) - hue(SQUASH_ZONE)).abs();
        let diff = diff.min(360.0 - diff);
        assert!(diff >= 90.0, "raise {:.0} vs squash {:.0}", hue(RAISE_ZONE), hue(SQUASH_ZONE));
        // the guide calls the squash zone light blue
        assert!((170.0..=210.0).contains(&hue(SQUASH_ZONE)), "{:.0}", hue(SQUASH_ZONE));
        let guide = include_str!("../../../docs/GUIDA.md");
        assert!(guide.contains("quella azzurra sotto quanto lo schiaccia"));
    }

    #[test]
    fn the_saturation_view_uses_theme_tokens() {
        let src = include_str!("saturation_view.rs");
        assert!(!src.contains(concat!("Color32::from_rgb", "(")));
        assert!(!src.contains(concat!("FontId::new", "(")));
    }
}
