//! The drawing of the delay windows: the Stereo Delay's left and right repeats, or the Tape Delay's, on the project's
//! tempo grid, with the first repeat of each side to drag. Times come from ondina_core::note_value and levels from
//! ondina_dsp::echoes, the same rules the delays play by; the math is plain functions the tests cover, delay_graph
//! draws them and turns the mouse into edits.

use std::collections::BTreeMap;

use eframe::egui::{self, Color32, CursorIcon, PointerButton, Pos2, Rect, Sense, Stroke, Vec2};
use ondina_core::model::{Id, PluginInstance, TimeSignature};
use ondina_core::note_value::{beats_to_ms, bpm_text, delay_times, ms_text, ms_to_beats, snap_note_value, time_label, FREE_RIGHT_RATIO};
use ondina_core::plugins;
use ondina_core::tempo::TempoMap;
use ondina_dsp::echoes::{stereo_echoes, stereo_repeats_to_floor, tape_brightness, tape_echoes, tape_repeats_to_floor, tape_sustain_db, Side, Tap};

use crate::theme;
use crate::widgets::{paint_graph_readout, with_alpha, ParamsEdit};

/// The handle on the first repeat of each side.
pub const POINT_R: f32 = 6.0;
/// Half the side of the square around a handle that takes the mouse.
pub const HIT_R: f32 = 10.0;
/// Width of a repeat's bar.
pub const TAP_W: f32 = 4.0;
/// After this many points of movement the larger of the two moves decides the drag until the button goes up.
pub const AXIS_LOCK_PX: f32 = 4.0;
/// The whole feedback range is a drag of two lane heights.
pub const FEEDBACK_DRAG_LANES: f32 = 2.0;
/// Wheel points for the whole mix: a notch (40 points) is 0.1.
pub const MIX_WHEEL_POINTS: f32 = 400.0;
/// Shift: the wheel moves ten times less.
pub const FINE: f32 = 0.1;
/// The row at the top: title, tempo and Sync.
pub const HEADER_H: f32 = 18.0;
/// The row at the bottom: bars or ms.
pub const RULER_H: f32 = 14.0;
/// Room on the left for the lanes' letters.
const LANE_LABEL_W: f32 = 16.0;
const SYNC_W: f32 = 44.0;

/// Whether plug-in kind `kind` has the delay drawing.
pub fn has_delay_view(kind: &str) -> bool {
    matches!(kind, "stereo_delay" | "tape_delay")
}

/// Height of the drawing in a window with `available` points left under the header.
pub fn graph_height(available: f32) -> f32 {
    (available * 0.42).clamp(170.0, 260.0)
}

/// The tempo the delay plays at.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct DelayTempo {
    pub bpm: f64,
    pub sig: TimeSignature,
    pub varies: bool,
}

impl DelayTempo {
    /// The tempo in force at `beat` (the playhead), the time signature, and whether the song has tempo changes.
    pub fn at(map: &TempoMap, sig: TimeSignature, beat: f64) -> Self {
        DelayTempo { bpm: crate::control_bar::tempo_at(map, beat), sig, varies: map.points.len() > 1 }
    }

    pub fn beats_per_bar(&self) -> f64 {
        self.sig.beats_per_bar()
    }
}

/// The span the grid shows, in beats: the shortest of 1, 2 or 4 bars that holds four times the longest first
/// repeat; past that, whole bars holding twice of it.
pub fn window_beats(longest_beats: f32, beats_per_bar: f32) -> f32 {
    let bar = beats_per_bar.max(0.25);
    let longest = longest_beats.max(0.0);
    for bars in [1.0, 2.0, 4.0] {
        if 4.0 * longest <= bars * bar + 1e-4 {
            return bars * bar;
        }
    }
    (2.0 * longest / bar - 1e-4).ceil().max(4.0) * bar
}

/// With Tempo Sync off: the shortest of 250, 500, 1000, 2000, 4000, 8000 ms holding four times the longest repeat.
pub fn window_ms_free(longest_ms: f32) -> f32 {
    [250.0, 500.0, 1000.0, 2000.0, 4000.0].into_iter().find(|w| 4.0 * longest_ms <= *w).unwrap_or(8000.0)
}

/// Grid step in ms with Tempo Sync off: the smallest of 10, 25, 50, 100, 250, 500, 1000 giving at most 10 lines.
pub fn free_grid_step(window_ms: f32) -> f32 {
    [10.0, 25.0, 50.0, 100.0, 250.0, 500.0].into_iter().find(|s| window_ms / s <= 10.0).unwrap_or(1000.0)
}

/// 0 ms at the plot's left edge, `window_ms` at its right edge.
pub fn ms_to_x(ms: f32, window_ms: f32, plot: Rect) -> f32 {
    plot.left() + ms / window_ms.max(1.0) * plot.width()
}

pub fn x_to_ms(x: f32, window_ms: f32, plot: Rect) -> f32 {
    if plot.width() <= 0.0 {
        return 0.0;
    }
    (x - plot.left()) / plot.width() * window_ms
}

/// Height of a repeat of gain `gain` in a lane of height `lane_h`: 0 dB fills it, -60 dB is nothing.
pub fn tap_height(gain: f32, lane_h: f32) -> f32 {
    if gain.is_nan() || gain <= 0.0 {
        return 0.0;
    }
    (1.0 + 20.0 * gain.log10() / 60.0).clamp(0.0, 1.0) * lane_h
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Handle {
    Left,
    Right,
    Tape,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DragAxis {
    Time,
    Feedback,
}

/// The axis a drag is locked to once it has moved AXIS_LOCK_PX points: the larger of the two moves.
pub fn drag_axis(moved: Vec2) -> Option<DragAxis> {
    if moved.x.abs().max(moved.y.abs()) < AXIS_LOCK_PX {
        None
    } else if moved.x.abs() >= moved.y.abs() {
        Some(DragAxis::Time)
    } else {
        Some(DragAxis::Feedback)
    }
}

/// A drag of a repeat, kept in egui's memory between frames. The window is frozen at the press, so the repeat stays
/// under the pointer even when a new time would change the span the grid shows.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct DelayDrag {
    pub handle: Handle,
    pub moved: Vec2,
    pub axis: Option<DragAxis>,
    pub window_ms: f32,
    pub plot_w: f32,
    pub lane_h: f32,
    /// The dragged time when the button went down: in beats, or in ms (Time (unsynced)) with Tempo Sync off.
    pub start_time: f32,
    pub start_feedback: f32,
}

fn registry_param(kind: &str, id: &str) -> Option<&'static plugins::ParamInfo> {
    plugins::find(kind).and_then(|i| i.params.iter().find(|p| p.id == id))
}

fn range(kind: &str, id: &str, lo: f32, hi: f32) -> (f32, f32) {
    registry_param(kind, id).map_or((lo, hi), |p| (p.min, p.max))
}

fn stereo(inst: &PluginInstance) -> bool {
    inst.kind == "stereo_delay"
}

/// The Stereo Delay with Tempo Sync off plays its times in ms.
fn free(inst: &PluginInstance) -> bool {
    stereo(inst) && inst.param("sync") < 0.5
}

/// The time parameter a handle moves: "time_l", "time_r", "time", or "time_ms" for both stereo handles with Sync off.
pub fn handle_param(inst: &PluginInstance, h: Handle) -> &'static str {
    match h {
        _ if !stereo(inst) => "time",
        _ if free(inst) => "time_ms",
        Handle::Right => "time_r",
        _ => "time_l",
    }
}

/// The parameters a drag sets: the time of the dragged side (snapped to note values unless `free`), or the
/// feedback; empty until the axis is locked.
pub fn drag_params(inst: &PluginInstance, d: &DelayDrag, bpm: f64, free_time: bool) -> BTreeMap<String, f32> {
    let mut out = BTreeMap::new();
    match d.axis {
        None => {}
        Some(DragAxis::Time) if free(inst) => {
            let (lo, hi) = range(&inst.kind, "time_ms", 1.0, 2000.0);
            let px_per_ms = d.plot_w / d.window_ms.max(1.0);
            let ms = if d.handle == Handle::Right {
                (d.start_time * FREE_RIGHT_RATIO + d.moved.x / px_per_ms) / FREE_RIGHT_RATIO
            } else {
                d.start_time + d.moved.x / px_per_ms
            };
            out.insert("time_ms".to_string(), ms.round().clamp(lo, hi));
        }
        Some(DragAxis::Time) => {
            let id = handle_param(inst, d.handle);
            let (lo, hi) = range(&inst.kind, id, 0.0625, 4.0);
            let px_per_beat = d.plot_w / (d.window_ms / beats_to_ms(1.0, bpm) as f32).max(1e-3);
            let t = d.start_time + d.moved.x / px_per_beat;
            let t = if free_time { t.clamp(lo, hi) } else { snap_note_value(t, lo, hi) };
            out.insert(id.to_string(), t);
        }
        Some(DragAxis::Feedback) => {
            let (_, hi) = range(&inst.kind, "feedback", 0.0, 0.95);
            let lanes = (FEEDBACK_DRAG_LANES * d.lane_h).max(1.0);
            // outward from the lane raises it: up for the left lane and the tape, down for the right lane
            let dy = if d.handle == Handle::Right { d.moved.y } else { -d.moved.y };
            out.insert("feedback".to_string(), (d.start_feedback + dy / lanes * hi).clamp(0.0, hi));
        }
    }
    out
}

/// Mix after a wheel turn of `scroll` points (x and y summed: egui turns Shift+wheel into x), 0..1; wheel up raises
/// it. A non-finite turn leaves it as it is.
pub fn wheel_mix(mix: f32, scroll: Vec2, fine: bool) -> f32 {
    let s = scroll.x + scroll.y;
    if !s.is_finite() {
        return mix;
    }
    (mix + s / MIX_WHEEL_POINTS * if fine { FINE } else { 1.0 }).clamp(0.0, 1.0)
}

/// Height of a lane: half the plot for the Stereo Delay (left above the middle, right below), all of it for the tape.
fn lane_h(inst: &PluginInstance, plot: Rect) -> f32 {
    if stereo(inst) {
        plot.height() / 2.0
    } else {
        plot.height()
    }
}

/// The handle positions: the top of the Left lane's first repeat, the bottom of the Right lane's first repeat, the
/// top of the tape's first repeat.
pub fn handle_pos(inst: &PluginInstance, h: Handle, tempo: DelayTempo, window_ms: f32, plot: Rect) -> Pos2 {
    let t = delay_times(inst, tempo.bpm);
    let left = t.map_or(0.0, |t| t.left_ms);
    match h {
        Handle::Right => egui::pos2(ms_to_x(t.and_then(|t| t.right_ms).unwrap_or(left), window_ms, plot), plot.bottom()),
        Handle::Left | Handle::Tape => egui::pos2(ms_to_x(left, window_ms, plot), plot.top()),
    }
}

/// Undo key of a drag of a repeat: one per gesture.
pub fn drag_key(pid: Id, h: Handle, gesture: u64) -> String {
    let side = match h {
        Handle::Left => "L",
        Handle::Right => "R",
        Handle::Tape => "T",
    };
    format!("pw{pid}:delay:{side}#{gesture}")
}

/// Undo key of the wheel over the drawing: turns close together merge.
pub fn mix_wheel_key(pid: Id) -> String {
    format!("pw{pid}:delay:mix")
}

/// "L 1/8 dotted · 375 ms" (Sync off: "L 350 ms"), the readout of a handle.
pub fn handle_readout(inst: &PluginInstance, h: Handle, tempo: DelayTempo) -> String {
    let prefix = match h {
        Handle::Left => "L ",
        Handle::Right => "R ",
        Handle::Tape => "",
    };
    if free(inst) {
        let t = delay_times(inst, tempo.bpm);
        let ms = match h {
            Handle::Right => t.and_then(|t| t.right_ms),
            _ => t.map(|t| t.left_ms),
        };
        return format!("{prefix}{}", ms_text(ms.unwrap_or(0.0) as f64));
    }
    format!("{prefix}{}", time_label(inst.param(handle_param(inst, h)), tempo.bpm))
}

/// How many repeats there are above -60 dB, or the level the tape holds them at.
enum Repeats {
    Count(u32),
    Holds(f32),
}

fn repeats(inst: &PluginInstance) -> Repeats {
    let fb = inst.param("feedback");
    if stereo(inst) {
        return Repeats::Count(stereo_repeats_to_floor(fb));
    }
    let sat = inst.param("saturation");
    match tape_repeats_to_floor(fb, sat) {
        Some(n) => Repeats::Count(n),
        None => Repeats::Holds(tape_sustain_db(fb, sat).unwrap_or(0.0)),
    }
}

fn plural(n: u32) -> &'static str {
    if n == 1 {
        "repeat"
    } else {
        "repeats"
    }
}

/// "Feedback 0.45 · 9 repeats" (tape that holds: "Feedback 0.50 · holds at -11 dB"), the readout while the feedback
/// is dragged.
pub fn feedback_readout(inst: &PluginInstance) -> String {
    let fb = inst.param("feedback");
    match repeats(inst) {
        Repeats::Count(n) => format!("Feedback {fb:.2} \u{00B7} {n} {}", plural(n)),
        Repeats::Holds(db) => format!("Feedback {fb:.2} \u{00B7} holds at {db:.0} dB"),
    }
}

/// "Feedback 0.35: 7 repeats to -60 dB"; tape: "... 53 repeats to -60 dB" or "Feedback 0.50: the tape holds the
/// repeats at -11 dB".
pub fn repeats_label(inst: &PluginInstance) -> String {
    let fb = inst.param("feedback");
    match repeats(inst) {
        Repeats::Count(n) => format!("Feedback {fb:.2}: {n} {} to -60 dB", plural(n)),
        Repeats::Holds(db) => format!("Feedback {fb:.2}: the tape holds the repeats at {db:.0} dB"),
    }
}

/// "120 BPM · 4/4", "118.5 BPM at the playhead · 4/4" when the tempo changes, "Tempo Sync off · ms".
pub fn tempo_label(inst: &PluginInstance, tempo: DelayTempo) -> String {
    if free(inst) {
        return "Tempo Sync off \u{00B7} ms".to_string();
    }
    let at = if tempo.varies { " at the playhead" } else { "" };
    format!("{} BPM{at} \u{00B7} {}/{}", bpm_text(tempo.bpm), tempo.sig.numerator, tempo.sig.denominator)
}

/// The line under the drawing: both times, the repeats and the mix (the tape's Tone too).
pub fn summary_line(inst: &PluginInstance, tempo: DelayTempo) -> String {
    let gap = "   \u{00B7}   ";
    let mix = format!("Mix {:.0}%", inst.param("mix") * 100.0);
    if stereo(inst) {
        let times = format!("{}   {}", handle_readout(inst, Handle::Left, tempo), handle_readout(inst, Handle::Right, tempo));
        format!("{times}{gap}{}{gap}{mix}", repeats_label(inst))
    } else {
        format!("{}{gap}{}{gap}Tone {:.2}{gap}{mix}", handle_readout(inst, Handle::Tape, tempo), repeats_label(inst), inst.param("tone"))
    }
}

/// The span the grid shows, in ms at the tempo.
fn window_ms(inst: &PluginInstance, tempo: DelayTempo) -> f32 {
    if free(inst) {
        let t = delay_times(inst, tempo.bpm);
        let longest = t.map_or(0.0, |t| t.left_ms.max(t.right_ms.unwrap_or(0.0)));
        return window_ms_free(longest);
    }
    let longest = if stereo(inst) { inst.param("time_l").max(inst.param("time_r")) } else { inst.param("time") };
    beats_to_ms(window_beats(longest, tempo.beats_per_bar() as f32) as f64, tempo.bpm) as f32
}

/// The repeats the drawing shows inside `window_ms`.
fn echoes(inst: &PluginInstance, tempo: DelayTempo, window_ms: f32) -> Vec<Tap> {
    let Some(t) = delay_times(inst, tempo.bpm) else {
        return Vec::new();
    };
    let fb = inst.param("feedback");
    match t.right_ms {
        Some(right) => stereo_echoes(t.left_ms, right, fb, inst.param("crossfeed"), window_ms),
        None => tape_echoes(t.left_ms, fb, inst.param("saturation"), window_ms),
    }
}

fn changes(inst: &PluginInstance, params: &BTreeMap<String, f32>) -> bool {
    params.iter().any(|(k, v)| inst.param(k) != *v)
}

/// A color `t` of the way from `a` to `b`.
fn blend(a: Color32, b: Color32, t: f32) -> Color32 {
    let t = t.clamp(0.0, 1.0);
    let ch = |x: u8, y: u8| (x as f32 + (y as f32 - x as f32) * t).round() as u8;
    Color32::from_rgb(ch(a.r(), b.r()), ch(a.g(), b.g()), ch(a.b(), b.b()))
}

// ---------------------------------------------------------------------------- drawing and mouse

pub struct DelayGraphOut {
    /// Where things are (tests use it to find the handles; the window has no use for it).
    #[cfg(test)]
    pub layout: DelayLayout,
    pub edits: Vec<ParamsEdit>,
}

#[cfg(test)]
#[derive(Debug, Clone, Copy)]
pub struct DelayLayout {
    pub rect: Rect,
    pub plot: Rect,
    pub left: Option<Pos2>,
    pub right: Option<Pos2>,
    pub tape: Option<Pos2>,
    pub sync: Option<Rect>,
    pub taps: usize,
}

/// The delay's repeats above its knobs. `gesture` is the window's drag counter (`PluginWindows::gesture`).
pub fn delay_graph(ui: &mut egui::Ui, inst: &PluginInstance, tempo: DelayTempo, gesture: &mut u64) -> DelayGraphOut {
    let mut edits = Vec::new();
    let (rect, _) = ui.allocate_exact_size(egui::vec2(ui.available_width(), graph_height(ui.available_height())), Sense::hover());
    let top = Rect::from_min_max(rect.min, egui::pos2(rect.right(), rect.top() + HEADER_H + theme::SP_2));
    let ruler = Rect::from_min_max(egui::pos2(rect.left(), rect.bottom() - RULER_H - theme::SP_1), rect.max);
    let body = Rect::from_min_max(egui::pos2(rect.left() + LANE_LABEL_W, top.bottom()), egui::pos2(rect.right() - theme::SP_3, ruler.top()));
    let plot = body.shrink(POINT_R + 2.0);
    let lane = lane_h(inst, plot);
    let is_stereo = stereo(inst);
    let is_free = free(inst);

    let drag_id = egui::Id::new(("delay-drag", inst.id));
    let mut drag: Option<DelayDrag> = ui.data(|d| d.get_temp(drag_id));
    let window = drag.map_or_else(|| window_ms(inst, tempo), |d| d.window_ms);
    let shift = ui.input(|i| i.modifiers.shift);

    // the background first, hover only: the handles registered after it take the clicks
    let bg = ui.interact(rect, ui.id().with("delay-bg"), Sense::hover());
    let handles: &[Handle] = if is_stereo { &[Handle::Right, Handle::Left] } else { &[Handle::Tape] };
    let mut hot: Option<Handle> = None;
    let mut over_handle = false;
    for &h in handles {
        let pos = handle_pos(inst, h, tempo, window, plot);
        let r = ui.interact(Rect::from_center_size(pos, Vec2::splat(2.0 * HIT_R)), ui.id().with(("delay-handle", h as u8)), Sense::click_and_drag());
        over_handle |= r.contains_pointer();
        if r.drag_started_by(PointerButton::Primary) {
            *gesture += 1;
            let start_time = inst.param(handle_param(inst, h));
            drag = Some(DelayDrag { handle: h, moved: Vec2::ZERO, axis: None, window_ms: window, plot_w: plot.width(), lane_h: lane, start_time, start_feedback: inst.param("feedback") });
        }
        if r.dragged_by(PointerButton::Primary) {
            if let Some(d) = drag.as_mut().filter(|d| d.handle == h) {
                d.moved += r.drag_delta();
                if d.axis.is_none() {
                    d.axis = drag_axis(d.moved);
                }
                let params = drag_params(inst, d, tempo.bpm, shift);
                if changes(inst, &params) {
                    edits.push(ParamsEdit::Drag { params, key: drag_key(inst.id, h, *gesture) });
                }
                hot = Some(h);
                ui.ctx().set_cursor_icon(CursorIcon::Grabbing);
            }
        }
        if r.drag_stopped() && drag.is_some_and(|d| d.handle == h) {
            drag = None;
        }
        if r.double_clicked() {
            let id = handle_param(inst, h);
            if let Some(p) = registry_param(&inst.kind, id) {
                edits.push(ParamsEdit::Set { params: BTreeMap::from([(id.to_string(), p.default)]) });
            }
        }
        if r.hovered() && hot.is_none() {
            hot = Some(h);
            if !r.dragged() {
                ui.ctx().set_cursor_icon(CursorIcon::Grab);
            }
        }
    }
    ui.data_mut(|m| match drag {
        Some(d) => m.insert_temp(drag_id, d),
        None => m.remove::<DelayDrag>(drag_id),
    });
    let sync_rect = is_stereo.then(|| Rect::from_min_size(egui::pos2(rect.right() - theme::SP_3 - SYNC_W, rect.top() + theme::SP_2), egui::vec2(SYNC_W, HEADER_H)));
    if let Some(sr) = sync_rect {
        let synced = !is_free;
        // a child that leaves the parent's cursor alone: the lines under the drawing stay under it
        let mut child = ui.new_child(egui::UiBuilder::new().max_rect(sr).layout(egui::Layout::centered_and_justified(egui::Direction::TopDown)));
        let resp = child
            .add(egui::Button::new("Sync").selected(synced).small())
            .on_hover_text("Tempo Sync: times in note values at the project tempo; off: Time (unsynced) in ms, the right side at 3/4 of it");
        if resp.clicked() {
            edits.push(ParamsEdit::Set { params: BTreeMap::from([("sync".to_string(), if synced { 0.0 } else { 1.0 })]) });
        }
    }
    if bg.contains_pointer() || over_handle {
        let s = ui.input(|i| i.raw_scroll_delta);
        if s != Vec2::ZERO {
            let mix = wheel_mix(inst.param("mix"), s, shift);
            if mix != inst.param("mix") {
                edits.push(ParamsEdit::Drag { params: BTreeMap::from([("mix".to_string(), mix)]), key: mix_wheel_key(inst.id) });
            }
            // the wheel was for the delay: nothing else scrolls with it
            ui.input_mut(|i| {
                i.raw_scroll_delta = Vec2::ZERO;
                i.smooth_scroll_delta = Vec2::ZERO;
            });
        }
    }

    let taps = echoes(inst, tempo, window);
    let painter = ui.painter_at(rect);
    painter.rect(rect, theme::R_LG, theme::INSET, Stroke::new(1.0_f32, theme::BORDER));
    // the grid: bars and beats (sixteenths when there is room) with Sync on, ms steps with it off
    let label_y = ruler.center().y;
    if is_free {
        let step = free_grid_step(window);
        let mut k = 1;
        while k as f32 * step <= window + 0.5 {
            let ms = k as f32 * step;
            let x = ms_to_x(ms, window, plot);
            painter.vline(x, plot.y_range(), Stroke::new(1.0_f32, theme::GRID_BEAT_LINE));
            let text = if k == 1 { ms_text(ms as f64) } else { format!("{ms}") };
            painter.text(egui::pos2(x, label_y), egui::Align2::CENTER_CENTER, text, theme::label_font(), theme::TEXT_3);
            k += 1;
        }
    } else {
        let beat_ms = beats_to_ms(1.0, tempo.bpm) as f32;
        let beats = window / beat_ms.max(1e-3);
        let px_per_beat = plot.width() / beats.max(1e-3);
        if px_per_beat * 0.25 >= 6.0 {
            for k in 1..(beats * 4.0).round() as i32 {
                if k % 4 != 0 {
                    painter.vline(plot.left() + k as f32 * 0.25 * px_per_beat, plot.y_range(), Stroke::new(1.0_f32, with_alpha(theme::GRID_BEAT_LINE, 140)));
                }
            }
        }
        for k in 1..=beats.round() as i32 {
            painter.vline(plot.left() + k as f32 * px_per_beat, plot.y_range(), Stroke::new(1.0_f32, theme::GRID_BEAT_LINE));
        }
        let bar = tempo.beats_per_bar().max(0.25) as f32;
        let mut m = 0;
        while m as f32 * bar <= beats + 1e-3 {
            let x = plot.left() + m as f32 * bar * px_per_beat;
            if m > 0 {
                painter.vline(x, plot.y_range(), Stroke::new(1.0_f32, theme::GRID_BAR_LINE));
            }
            if m as f32 * bar < beats - 1e-3 {
                painter.text(egui::pos2(x + theme::SP_1, label_y), egui::Align2::LEFT_CENTER, format!("{}", m + 1), theme::label_font(), theme::TEXT_3);
            }
            m += 1;
        }
    }
    // the hit
    painter.vline(plot.left(), plot.y_range(), Stroke::new(1.0_f32, theme::TEXT_3));
    painter.text(egui::pos2(plot.left() + theme::SP_2, plot.top()), egui::Align2::LEFT_TOP, "dry", theme::label_font(), theme::TEXT_3);
    let centre = plot.center().y;
    if is_stereo {
        painter.hline(plot.x_range(), centre, Stroke::new(1.0_f32, theme::CENTER_LINE));
        painter.text(egui::pos2(rect.left() + theme::SP_3, (plot.top() + centre) / 2.0), egui::Align2::LEFT_CENTER, "L", theme::label_font(), theme::TEXT_3);
        painter.text(egui::pos2(rect.left() + theme::SP_3, (centre + plot.bottom()) / 2.0), egui::Align2::LEFT_CENTER, "R", theme::label_font(), theme::TEXT_3);
    }
    // the repeats, and a faint line over their tops: the decay
    let tint = |c: Color32| if inst.bypass { theme::TEXT_DISABLED } else { c };
    let side_color = |s: Side| match s {
        Side::Right => tint(theme::MONITOR),
        _ => tint(theme::ACCENT),
    };
    let tone = inst.param("tone");
    let mut tops: [Vec<Pos2>; 3] = [Vec::new(), Vec::new(), Vec::new()];
    for t in &taps {
        let x = ms_to_x(t.ms, window, plot);
        let h = tap_height(t.gain, lane);
        let (bar, top_pt, color, k) = match t.side {
            Side::Left => (Rect::from_min_max(egui::pos2(x - TAP_W / 2.0, centre - h), egui::pos2(x + TAP_W / 2.0, centre)), egui::pos2(x, centre - h), side_color(Side::Left), 0),
            Side::Right => (Rect::from_min_max(egui::pos2(x - TAP_W / 2.0, centre), egui::pos2(x + TAP_W / 2.0, centre + h)), egui::pos2(x, centre + h), side_color(Side::Right), 1),
            Side::Mono => {
                let c = tint(blend(theme::TEXT_DISABLED, theme::ACCENT, tape_brightness(tone, t.pass)));
                (Rect::from_min_max(egui::pos2(x - TAP_W / 2.0, plot.bottom() - h), egui::pos2(x + TAP_W / 2.0, plot.bottom())), egui::pos2(x, plot.bottom() - h), c, 2)
            }
        };
        if h > 0.0 {
            painter.rect_filled(bar, 1.0, color);
        }
        tops[k].push(top_pt);
    }
    for (k, pts) in tops.into_iter().enumerate() {
        if pts.len() >= 2 {
            let color = if k == 1 { side_color(Side::Right) } else { side_color(Side::Left) };
            painter.add(egui::Shape::line(pts, Stroke::new(1.0_f32, with_alpha(color, 90))));
        }
    }
    // the handles, the hot one with a ring and its value
    let mut positions = [None; 3];
    for &h in handles {
        let pos = handle_pos(inst, h, tempo, window, plot);
        positions[h as usize] = Some(pos);
        let color = if h == Handle::Right { side_color(Side::Right) } else { side_color(Side::Left) };
        painter.circle_filled(pos, POINT_R, color);
        if hot == Some(h) {
            painter.circle_stroke(pos, POINT_R + 2.0, Stroke::new(1.5_f32, theme::TEXT));
            let feedback = drag.is_some_and(|d| d.handle == h && d.axis == Some(DragAxis::Feedback));
            let text = if feedback { feedback_readout(inst) } else { handle_readout(inst, h, tempo) };
            paint_graph_readout(&painter, text, pos, rect, POINT_R);
        }
    }
    if let Some(p) = bg.hover_pos().filter(|p| hot.is_none() && plot.x_range().contains(p.x)) {
        // the time under the pointer, in the units the grid shows
        let ms = x_to_ms(p.x, window, plot);
        let text = if is_free { ms_text(ms as f64) } else { time_label(ms_to_beats(ms as f64, tempo.bpm) as f32, tempo.bpm) };
        painter.text(egui::pos2(plot.right() - theme::SP_2, plot.top()), egui::Align2::RIGHT_TOP, text, theme::number_font(), theme::TEXT_3);
    }
    painter.text(rect.left_top() + egui::vec2(theme::SP_4, theme::SP_2 + HEADER_H / 2.0), egui::Align2::LEFT_CENTER, "ECHOES", theme::label_font(), theme::TEXT_3);
    let label_right = sync_rect.map_or(rect.right() - theme::SP_4, |r| r.left() - theme::SP_3);
    painter.text(egui::pos2(label_right, rect.top() + theme::SP_2 + HEADER_H / 2.0), egui::Align2::RIGHT_CENTER, tempo_label(inst, tempo), theme::label_font(), theme::TEXT_3);

    ui.label(egui::RichText::new(summary_line(inst, tempo)).small().color(theme::TEXT_2));
    let help = if is_free {
        "Drag a repeat sideways: Time (unsynced) in ms \u{00B7} Up/down: feedback \u{00B7} Wheel: mix \u{00B7} Double-click: default time"
    } else {
        "Drag a repeat sideways: time (snaps to note values, Shift: free) \u{00B7} Up/down: feedback \u{00B7} Wheel: mix \u{00B7} Double-click: default time"
    };
    ui.label(egui::RichText::new(help).small().color(theme::TEXT_3));
    #[cfg(test)]
    let layout = DelayLayout { rect, plot, left: positions[Handle::Left as usize], right: positions[Handle::Right as usize], tape: positions[Handle::Tape as usize], sync: sync_rect, taps: taps.len() };
    let _ = positions;
    DelayGraphOut {
        #[cfg(test)]
        layout,
        edits,
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

    fn tempo() -> DelayTempo {
        DelayTempo { bpm: 120.0, sig: TimeSignature { numerator: 4, denominator: 4 }, varies: false }
    }

    fn sig(numerator: u8, denominator: u8) -> f32 {
        TimeSignature { numerator, denominator }.beats_per_bar() as f32
    }

    fn drag(handle: Handle, moved: Vec2, window_ms: f32, plot_w: f32, start_time: f32) -> DelayDrag {
        DelayDrag { handle, moved, axis: drag_axis(moved), window_ms, plot_w, lane_h: 80.0, start_time, start_feedback: 0.35 }
    }

    #[test]
    fn delay_kinds_only() {
        assert!(has_delay_view("stereo_delay") && has_delay_view("tape_delay"));
        for kind in ["reverb", "chorus", "channel_eq", "vst3"] {
            assert!(!has_delay_view(kind), "{kind}");
        }
        assert_eq!(graph_height(100.0), 170.0);
        assert_eq!(graph_height(1000.0), 260.0);
    }

    #[test]
    fn window_fits_four_repeats_up_to_four_bars() {
        let four = sig(4, 4);
        assert_eq!(window_beats(0.75, four), 4.0);
        assert_eq!(window_beats(1.0, four), 4.0);
        assert_eq!(window_beats(1.5, four), 8.0);
        assert_eq!(window_beats(4.0, four), 16.0);
        let six = sig(6, 8);
        assert_eq!(six, 3.0);
        assert_eq!(window_beats(0.75, six), 3.0);
        assert_eq!(window_beats(4.0, six), 12.0);
        assert_eq!(window_beats(4.0, sig(1, 4)), 8.0);
    }

    #[test]
    fn free_window_steps_in_ms() {
        assert_eq!(window_ms_free(100.0), 500.0);
        assert_eq!(window_ms_free(350.0), 2000.0);
        assert_eq!(window_ms_free(2000.0), 8000.0);
        assert_eq!(free_grid_step(2000.0), 250.0);
        assert_eq!(free_grid_step(250.0), 25.0);
        assert_eq!(free_grid_step(8000.0), 1000.0);
        let plot = Rect::from_min_size(pos2(10.0, 0.0), vec2(400.0, 100.0));
        assert_eq!(ms_to_x(0.0, 2000.0, plot), 10.0);
        assert_eq!(ms_to_x(1000.0, 2000.0, plot), 210.0);
        assert_eq!(x_to_ms(210.0, 2000.0, plot), 1000.0);
    }

    #[test]
    fn tap_heights_follow_the_level() {
        assert_eq!(tap_height(1.0, 80.0), 80.0);
        assert!(near(tap_height(0.5, 80.0), 0.8997 * 80.0, 0.05));
        assert!(near(tap_height(0.001, 80.0), 0.0, 1e-3));
        assert_eq!(tap_height(0.0005, 80.0), 0.0);
        assert_eq!(tap_height(0.0, 80.0), 0.0);
        assert_eq!(tap_height(f32::NAN, 80.0), 0.0);
    }

    #[test]
    fn axis_locks_after_a_few_points() {
        assert_eq!(drag_axis(vec2(2.0, 1.0)), None);
        assert_eq!(drag_axis(vec2(5.0, 2.0)), Some(DragAxis::Time));
        assert_eq!(drag_axis(vec2(1.0, -6.0)), Some(DragAxis::Feedback));
    }

    #[test]
    fn time_drag_snaps_to_note_values_and_shift_frees_it() {
        let inst = instance("stereo_delay");
        // 2000 ms is 4 beats at 120 BPM: 100 points a beat
        let t = |dx: f32, free: bool| drag_params(&inst, &drag(Handle::Left, vec2(dx, 0.0), 2000.0, 400.0, 0.75), 120.0, free);
        assert_eq!(t(-27.0, false), BTreeMap::from([("time_l".to_string(), 0.5)]));
        assert!(near(t(-40.0, false)["time_l"], 1.0 / 3.0, 1e-6));
        assert!(near(t(-40.0, true)["time_l"], 0.35, 1e-5));
        assert_eq!(t(400.0, false)["time_l"], 4.0);
        assert_eq!(t(400.0, true)["time_l"], 4.0);
        let still = drag(Handle::Left, vec2(2.0, 1.0), 2000.0, 400.0, 0.75);
        assert!(drag_params(&inst, &still, 120.0, false).is_empty());
        let right = drag_params(&inst, &drag(Handle::Right, vec2(50.0, 0.0), 2000.0, 400.0, 0.5), 120.0, false);
        assert_eq!(right, BTreeMap::from([("time_r".to_string(), 1.0)]));
        let tape = drag_params(&instance("tape_delay"), &drag(Handle::Tape, vec2(25.0, 0.0), 2000.0, 400.0, 0.5), 120.0, false);
        assert_eq!(tape, BTreeMap::from([("time".to_string(), 0.75)]));
    }

    #[test]
    fn free_time_drag_moves_time_ms_from_either_handle() {
        let inst = with(instance("stereo_delay"), &[("sync", 0.0), ("time_ms", 200.0)]);
        assert_eq!(handle_param(&inst, Handle::Left), "time_ms");
        assert_eq!(handle_param(&inst, Handle::Right), "time_ms");
        let l = drag_params(&inst, &drag(Handle::Left, vec2(50.0, 0.0), 1000.0, 500.0, 200.0), 120.0, false);
        assert_eq!(l, BTreeMap::from([("time_ms".to_string(), 300.0)]));
        let r = drag_params(&inst, &drag(Handle::Right, vec2(30.0, 0.0), 1000.0, 500.0, 200.0), 120.0, false);
        assert_eq!(r, BTreeMap::from([("time_ms".to_string(), 280.0)]));
        let low = drag_params(&inst, &drag(Handle::Left, vec2(-500.0, 0.0), 1000.0, 500.0, 200.0), 120.0, true);
        assert_eq!(low["time_ms"], 1.0);
    }

    #[test]
    fn vertical_drag_changes_feedback_outward_raises_it() {
        let inst = instance("stereo_delay");
        let up = drag_params(&inst, &drag(Handle::Left, vec2(0.0, -40.0), 2000.0, 400.0, 0.75), 120.0, false);
        assert!(near(up["feedback"], 0.35 + 0.95 / 4.0, 1e-5), "{up:?}");
        let down = drag_params(&inst, &drag(Handle::Right, vec2(0.0, 40.0), 2000.0, 400.0, 0.5), 120.0, false);
        assert!(near(down["feedback"], 0.35 + 0.95 / 4.0, 1e-5), "{down:?}");
        assert_eq!(drag_params(&inst, &drag(Handle::Left, vec2(0.0, -1e4), 2000.0, 400.0, 0.75), 120.0, false)["feedback"], 0.95);
        assert_eq!(drag_params(&inst, &drag(Handle::Left, vec2(0.0, 1e4), 2000.0, 400.0, 0.75), 120.0, false)["feedback"], 0.0);
        let tape = drag_params(&instance("tape_delay"), &drag(Handle::Tape, vec2(0.0, -1e4), 2000.0, 400.0, 0.5), 120.0, false);
        assert_eq!(tape["feedback"], 1.0);
    }

    #[test]
    fn wheel_changes_mix() {
        assert!(near(wheel_mix(0.25, vec2(0.0, 40.0), false), 0.35, 1e-5));
        assert!(near(wheel_mix(0.25, vec2(0.0, 40.0), true), 0.26, 1e-5));
        assert!(near(wheel_mix(0.25, vec2(40.0, 0.0), true), 0.26, 1e-5));
        assert_eq!(wheel_mix(0.95, vec2(0.0, 400.0), false), 1.0);
        assert_eq!(wheel_mix(0.05, vec2(0.0, -400.0), false), 0.0);
        assert_eq!(wheel_mix(0.4, vec2(f32::NAN, 0.0), false), 0.4);
    }

    #[test]
    fn keys_have_the_window_format() {
        assert_eq!(drag_key(7, Handle::Left, 3), "pw7:delay:L#3");
        assert_eq!(drag_key(7, Handle::Right, 3), "pw7:delay:R#3");
        assert_eq!(drag_key(7, Handle::Tape, 1), "pw7:delay:T#1");
        assert_eq!(mix_wheel_key(7), "pw7:delay:mix");
    }

    #[test]
    fn readouts_name_note_values() {
        let st = instance("stereo_delay");
        assert_eq!(handle_readout(&st, Handle::Left, tempo()), "L 1/8 dotted \u{00B7} 375 ms");
        assert_eq!(handle_readout(&st, Handle::Right, tempo()), "R 1/8 \u{00B7} 250 ms");
        assert_eq!(handle_readout(&instance("tape_delay"), Handle::Tape, tempo()), "1/8 \u{00B7} 250 ms");
        assert_eq!(repeats_label(&st), "Feedback 0.35: 7 repeats to -60 dB");
        assert_eq!(feedback_readout(&st), "Feedback 0.35 \u{00B7} 7 repeats");
        assert_eq!(repeats_label(&with(st.clone(), &[("feedback", 0.0)])), "Feedback 0.00: 1 repeat to -60 dB");
        let held = with(instance("tape_delay"), &[("feedback", 0.5), ("saturation", 0.3)]);
        assert!(repeats_label(&held).contains("holds the repeats"), "{}", repeats_label(&held));
        assert_eq!(feedback_readout(&held), "Feedback 0.50 \u{00B7} holds at -11 dB");
        assert_eq!(repeats_label(&instance("tape_delay")), "Feedback 0.40: 53 repeats to -60 dB");
        assert_eq!(tempo_label(&st, tempo()), "120 BPM \u{00B7} 4/4");
        let varies = DelayTempo { bpm: 118.5, varies: true, ..tempo() };
        assert!(tempo_label(&st, varies).contains("at the playhead"), "{}", tempo_label(&st, varies));
        let free = with(st.clone(), &[("sync", 0.0)]);
        assert!(tempo_label(&free, tempo()).contains("ms"));
        assert_eq!(handle_readout(&free, Handle::Left, tempo()), "L 350 ms");
        assert_eq!(handle_readout(&free, Handle::Right, tempo()), "R 263 ms");
        let line = summary_line(&st, tempo());
        assert!(line.contains("L 1/8 dotted \u{00B7} 375 ms") && line.contains("7 repeats") && line.contains("Mix 25%"), "{line}");
        assert!(summary_line(&instance("tape_delay"), tempo()).contains("Tone 0.40"));
    }

    #[test]
    fn handles_sit_on_the_first_repeats() {
        let plot = Rect::from_min_size(pos2(0.0, 0.0), vec2(400.0, 160.0));
        let st = instance("stereo_delay");
        assert_eq!(handle_pos(&st, Handle::Left, tempo(), 2000.0, plot), pos2(75.0, 0.0));
        assert_eq!(handle_pos(&st, Handle::Right, tempo(), 2000.0, plot), pos2(50.0, 160.0));
        assert_eq!(handle_pos(&instance("tape_delay"), Handle::Tape, tempo(), 2000.0, plot), pos2(50.0, 0.0));
        assert_eq!(window_ms(&st, tempo()), 2000.0);
        assert_eq!(window_ms(&with(st, &[("sync", 0.0)]), tempo()), 2000.0);
    }

    /// Frames of the delay graph in a headless 700x600 context, one per entry of `frames` (the events of that
    /// frame). Returns the layout, every edit asked for and the scroll left over after the graph.
    fn run(inst: &PluginInstance, t: DelayTempo, gesture: &mut u64, frames: &[Vec<Event>]) -> (DelayLayout, Vec<ParamsEdit>, Vec2) {
        let ctx = egui::Context::default();
        theme::apply(&ctx);
        let mut layout = None;
        let mut all = Vec::new();
        let mut scroll = Vec2::ZERO;
        for (k, events) in frames.iter().enumerate() {
            let raw = egui::RawInput {
                screen_rect: Some(Rect::from_min_size(Pos2::ZERO, vec2(700.0, 600.0))),
                time: Some(k as f64 / 60.0),
                events: events.clone(),
                ..Default::default()
            };
            let _ = ctx.run(raw, |ctx| {
                egui::CentralPanel::default().show(ctx, |ui| {
                    let out = delay_graph(ui, inst, t, gesture);
                    layout = Some(out.layout);
                    all.extend(out.edits);
                    scroll = ui.input(|i| i.raw_scroll_delta);
                });
            });
        }
        (layout.unwrap(), all, scroll)
    }

    fn run_frames(inst: &PluginInstance, gesture: &mut u64, frames: &[Vec<Event>]) -> (DelayLayout, Vec<ParamsEdit>) {
        let (layout, edits, _) = run(inst, tempo(), gesture, frames);
        (layout, edits)
    }

    fn harness_layout(inst: &PluginInstance) -> DelayLayout {
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

    fn click_frames(p: Pos2) -> Vec<Vec<Event>> {
        vec![vec![], vec![Event::PointerMoved(p)], vec![press(p, PointerButton::Primary, true)], vec![press(p, PointerButton::Primary, false)]]
    }

    fn last_params(edits: &[ParamsEdit]) -> &BTreeMap<String, f32> {
        match edits.last().unwrap() {
            ParamsEdit::Drag { params, .. } | ParamsEdit::Set { params } => params,
        }
    }

    #[test]
    fn delay_graph_draws_both_delays_without_edits() {
        let cases: [(&str, &[(&str, f32)]); 6] = [
            ("stereo_delay", &[]),
            ("stereo_delay", &[("time_l", 0.0625), ("time_r", 4.0), ("feedback", 0.95), ("crossfeed", 1.0)]),
            ("stereo_delay", &[("sync", 0.0), ("time_ms", 1.0)]),
            ("stereo_delay", &[("sync", 0.0), ("time_ms", 2000.0), ("feedback", 0.95)]),
            ("tape_delay", &[]),
            ("tape_delay", &[("feedback", 1.0), ("saturation", 1.0), ("tone", 0.0), ("time", 0.0625)]),
        ];
        for (kind, kv) in cases {
            for varies in [false, true] {
                let inst = with(instance(kind), kv);
                let t = DelayTempo { varies, ..tempo() };
                let mut g = 0;
                let (layout, edits, _) = run(&inst, t, &mut g, &[vec![], vec![]]);
                assert!(edits.is_empty(), "{kind} {kv:?}: {edits:?}");
                assert_eq!(g, 0);
                assert!(layout.rect.is_positive() && layout.plot.is_positive(), "{layout:?}");
                assert!(layout.taps > 0, "{kind} {kv:?}");
                if kind == "stereo_delay" {
                    assert!(layout.left.is_some() && layout.right.is_some() && layout.sync.is_some() && layout.tape.is_none());
                } else {
                    assert!(layout.tape.is_some() && layout.left.is_none() && layout.sync.is_none());
                }
            }
        }
    }

    #[test]
    fn dragging_the_left_repeat_sends_one_gesture() {
        let inst = instance("stereo_delay");
        let p = harness_layout(&inst).left.unwrap();
        let mut g = 0;
        let (_, edits) = run_frames(&inst, &mut g, &drag_frames(p, vec2(-9.0, 0.0)));
        assert!(!edits.is_empty());
        assert_eq!(g, 1);
        for e in &edits {
            assert!(matches!(e, ParamsEdit::Drag { key, params } if key == "pw3:delay:L#1" && params.keys().all(|k| k == "time_l")), "{e:?}");
        }
        let t = last_params(&edits)["time_l"];
        assert!(ondina_core::note_value::note_value_name(t).is_some() && t < 0.75, "{t}");
    }

    #[test]
    fn dragging_the_right_repeat_moves_only_the_right_time() {
        let inst = instance("stereo_delay");
        let p = harness_layout(&inst).right.unwrap();
        let mut g = 0;
        let (_, edits) = run_frames(&inst, &mut g, &drag_frames(p, vec2(12.0, 0.0)));
        assert!(!edits.is_empty());
        for e in &edits {
            assert!(matches!(e, ParamsEdit::Drag { key, params } if key == "pw3:delay:R#1" && params.keys().all(|k| k == "time_r")), "{e:?}");
        }
        assert!(last_params(&edits)["time_r"] > 0.5);
    }

    #[test]
    fn close_left_and_right_handles_stay_grabbable() {
        let inst = with(instance("stereo_delay"), &[("time_l", 0.5), ("time_r", 0.5)]);
        let layout = harness_layout(&inst);
        let (l, r) = (layout.left.unwrap(), layout.right.unwrap());
        assert_eq!(l.x, r.x);
        let (_, edits) = run_frames(&inst, &mut 0, &drag_frames(l, vec2(15.0, 0.0)));
        assert!(!edits.is_empty() && edits.iter().all(|e| matches!(e, ParamsEdit::Drag { params, .. } if params.contains_key("time_l"))), "{edits:?}");
        let (_, edits) = run_frames(&inst, &mut 0, &drag_frames(r, vec2(15.0, 0.0)));
        assert!(!edits.is_empty() && edits.iter().all(|e| matches!(e, ParamsEdit::Drag { params, .. } if params.contains_key("time_r"))), "{edits:?}");
    }

    #[test]
    fn vertical_drag_on_the_graph_sends_feedback() {
        let inst = instance("stereo_delay");
        let p = harness_layout(&inst).left.unwrap();
        let (_, edits) = run_frames(&inst, &mut 0, &drag_frames(p, vec2(0.0, -8.0)));
        assert!(!edits.is_empty());
        let values: Vec<f32> = edits
            .iter()
            .map(|e| match e {
                ParamsEdit::Drag { params, .. } => {
                    assert_eq!(params.keys().collect::<Vec<_>>(), ["feedback"]);
                    params["feedback"]
                }
                ParamsEdit::Set { .. } => panic!("{e:?}"),
            })
            .collect();
        assert!(values.windows(2).all(|w| w[1] >= w[0]) && values[0] > 0.35, "{values:?}");
    }

    #[test]
    fn wheel_over_the_graph_changes_the_mix_and_scrolls_nothing() {
        let inst = instance("stereo_delay");
        let layout = harness_layout(&inst);
        let wheel = Event::MouseWheel { unit: MouseWheelUnit::Line, delta: vec2(0.0, 1.0), modifiers: Modifiers::NONE };
        let somewhere = layout.plot.center() + vec2(120.0, 10.0);
        let mut g = 0;
        let (_, edits, scroll) = run(&inst, tempo(), &mut g, &[vec![], vec![Event::PointerMoved(somewhere)], vec![wheel]]);
        assert_eq!(edits.len(), 1, "{edits:?}");
        let ParamsEdit::Drag { params, key } = &edits[0] else { panic!("{edits:?}") };
        assert_eq!(key, "pw3:delay:mix");
        assert!(params["mix"] > 0.25, "{params:?}");
        assert_eq!(scroll, Vec2::ZERO);
        assert_eq!(g, 0);
    }

    #[test]
    fn double_click_resets_the_time() {
        let inst = with(instance("stereo_delay"), &[("time_l", 1.5)]);
        let p = harness_layout(&inst).left.unwrap();
        let mut frames = vec![vec![], vec![Event::PointerMoved(p)]];
        for _ in 0..2 {
            frames.push(vec![press(p, PointerButton::Primary, true)]);
            frames.push(vec![press(p, PointerButton::Primary, false)]);
        }
        let (_, edits) = run_frames(&inst, &mut 0, &frames);
        assert!(edits.contains(&ParamsEdit::Set { params: BTreeMap::from([("time_l".to_string(), 0.75)]) }), "{edits:?}");
    }

    #[test]
    fn sync_button_switches_the_grid() {
        let inst = instance("stereo_delay");
        let c = harness_layout(&inst).sync.unwrap().center();
        let (_, edits) = run_frames(&inst, &mut 0, &click_frames(c));
        assert_eq!(edits, [ParamsEdit::Set { params: BTreeMap::from([("sync".to_string(), 0.0)]) }]);
        let off = with(inst, &[("sync", 0.0)]);
        let (_, edits) = run_frames(&off, &mut 0, &click_frames(c));
        assert_eq!(edits, [ParamsEdit::Set { params: BTreeMap::from([("sync".to_string(), 1.0)]) }]);
    }

    #[test]
    fn a_change_in_the_project_changes_the_drawing() {
        let low = with(instance("stereo_delay"), &[("feedback", 0.2)]);
        let high = with(instance("stereo_delay"), &[("feedback", 0.8)]);
        let (a, b) = (harness_layout(&low), harness_layout(&high));
        assert_eq!((a.left, a.right), (b.left, b.right));
        assert!(b.taps > a.taps, "{} vs {}", a.taps, b.taps);
        let later = harness_layout(&with(instance("stereo_delay"), &[("time_l", 1.0)]));
        assert_ne!(later.left, a.left);
    }

    #[test]
    fn the_delay_view_reuses_the_common_header() {
        let src = include_str!("delay_view.rs");
        assert!(!src.contains(concat!("plugin_ai_bar", "(")));
        assert!(!src.contains(concat!("plugin_header", "(")));
        assert!(!src.contains(concat!("knob", "(")));
    }
}
