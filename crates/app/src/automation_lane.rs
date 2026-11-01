//! The arrangement's automation view: which lane each track shows, how its values map to the lane height, how a
//! mouse gesture changes the points and which command it sends. Pure functions, so the arrangement only wires the
//! mouse to them.

use std::collections::HashMap;

use eframe::egui::{self, pos2, Align2, Color32, FontId, Key, Modifiers, Pos2, Rect, Sense, Shape, Stroke};
use ondina_core::command::Command;
use ondina_core::model::{AutomationLane, AutomationPoint, Id, Project, Track, MASTER_ID};

use crate::app::{Drag, OndinaApp};
use crate::theme;

/// Band at the top of a track lane left to the regions while the view is on: their name bar and fade handles.
pub const TOP_BAND_PX: f32 = 16.0;
/// Margin between the value band and the lane edges.
const BAND_MARGIN_PX: f32 = 3.0;
/// A point within this distance of the pointer is the one under it.
pub const HIT_PX: f32 = 6.0;
/// A drag that moved less than this horizontally keeps the point's beat (vertical adjustment only).
pub const STILL_PX: f32 = 3.0;
pub const VOLUME_MIN_DB: f32 = -60.0;
pub const VOLUME_MAX_DB: f32 = 6.0;

/// How a lane's value is shown.
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum Unit {
    Db,
    Pan,
    Degrees,
    Plain(&'static str),
}

/// What the view needs to know about one lane of one track.
#[derive(Debug, Clone, PartialEq)]
pub struct LaneSpec {
    pub target: String,
    /// Long name for the menu: "Volume", "Pan", "Send 1 → Reverb", "Surround azimuth", "Compressor: Threshold".
    pub label: String,
    /// Short name for the header selector and the lane label: "Volume", "Pan", "Send 1", "Azimuth", "Threshold".
    pub short: String,
    pub min: f32,
    pub max: f32,
    /// Selector parameters (plug-in params with choices): values are whole numbers.
    pub stepped: bool,
    /// The names of a selector's values, by index (empty for every other lane).
    pub choices: &'static [&'static str],
    pub unit: Unit,
    /// The channel or parameter value the lane has when it has no points (drawn dashed).
    pub static_value: f32,
}

impl LaneSpec {
    fn new(target: &str, label: String, short: String, range: (f32, f32), unit: Unit, static_value: f32) -> Self {
        LaneSpec {
            target: target.to_string(),
            label,
            short,
            min: range.0,
            max: range.1,
            stepped: false,
            choices: &[],
            unit,
            static_value,
        }
    }
}

/// The master has no automation (the core refuses it); every track and bus has.
pub fn automatable(track_id: Id) -> bool {
    track_id != MASTER_ID
}

/// The spec of `target` on `track`, or None when the target is not valid for it (send index out of range,
/// plug-in or parameter gone, VST3 parameters, unknown target). Mirrors `validate_target` in core/ops.rs.
pub fn lane_spec(project: &Project, track: &Track, target: &str) -> Option<LaneSpec> {
    let ch = &track.channel;
    let volume = (VOLUME_MIN_DB, VOLUME_MAX_DB);
    let spec = match target {
        "volume" => LaneSpec::new(target, "Volume".into(), "Volume".into(), volume, Unit::Db, ch.volume_db),
        "pan" => LaneSpec::new(target, "Pan".into(), "Pan".into(), (-1.0, 1.0), Unit::Pan, ch.pan),
        "surround:azimuth" => {
            let az = ch.surround_pan.azimuth;
            LaneSpec::new(target, "Surround azimuth".into(), "Azimuth".into(), (-180.0, 180.0), Unit::Degrees, az)
        }
        "surround:elevation" => {
            let el = ch.surround_pan.elevation;
            LaneSpec::new(target, "Surround elevation".into(), "Elevation".into(), (-30.0, 90.0), Unit::Degrees, el)
        }
        "surround:spread" => {
            let sp = ch.surround_pan.spread;
            LaneSpec::new(target, "Surround spread".into(), "Spread".into(), (0.0, 1.0), Unit::Plain(""), sp)
        }
        _ => {
            if let Some(rest) = target.strip_prefix("send:") {
                let i: usize = rest.parse().ok()?;
                let send = ch.sends.get(i)?;
                let bus = project.track(send.bus_id).map_or("?", |b| b.name.as_str());
                let label = format!("Send {} → {bus}", i + 1);
                return Some(LaneSpec::new(target, label, format!("Send {}", i + 1), volume, Unit::Db, send.level_db));
            }
            let rest = target.strip_prefix("plugin:")?;
            let (pid, param) = rest.split_once(':')?;
            let plugin = project.find_plugin(pid.parse().ok()?)?;
            let info = ondina_core::plugins::find(&plugin.kind)?;
            let pi = info.params.iter().find(|x| x.id == param)?;
            let value = plugin.params.get(param).copied().unwrap_or(pi.default);
            let label = format!("{}: {}", ondina_core::plugins::plugin_label(plugin), pi.name);
            let mut spec = LaneSpec::new(target, label, pi.name.into(), (pi.min, pi.max), Unit::Plain(pi.unit), value);
            spec.stepped = !pi.choices.is_empty();
            spec.choices = pi.choices;
            spec
        }
    };
    Some(spec)
}

/// The lanes the selector offers, with how many points each has: Volume, Pan, one per send of the track, then
/// every other lane the track already has (surround, plug-in parameters) whose spec is valid, in lane order.
pub fn lane_choices(project: &Project, track: &Track) -> Vec<(LaneSpec, usize)> {
    let mut targets: Vec<String> = vec!["volume".into(), "pan".into()];
    targets.extend((0..track.channel.sends.len()).map(|i| format!("send:{i}")));
    for lane in &track.automation {
        if !targets.contains(&lane.target) {
            targets.push(lane.target.clone());
        }
    }
    targets
        .iter()
        .filter_map(|target| lane_spec(project, track, target))
        .map(|spec| {
            let n = track.automation.iter().find(|l| l.target == spec.target).map_or(0, |l| l.points.len());
            (spec, n)
        })
        .collect()
}

/// The track's points for `target` (empty when there is no lane) and whether the lane is enabled.
pub fn lane_points(track: &Track, target: &str) -> (Vec<AutomationPoint>, bool) {
    track.automation.iter().find(|l| l.target == target).map_or((Vec::new(), true), |l| (l.points.clone(), l.enabled))
}

/// Top and bottom of the value band for a lane drawn from `lane_top` with height `h`.
pub fn value_band(lane_top: f32, h: f32) -> (f32, f32) {
    (lane_top + TOP_BAND_PX + BAND_MARGIN_PX, lane_top + h - BAND_MARGIN_PX)
}

fn rounded(spec: &LaneSpec, v: f32) -> f32 {
    let v = v.clamp(spec.min, spec.max);
    if spec.stepped {
        v.round()
    } else {
        v
    }
}

/// Value for a pointer height: `top` is `max`, `bottom` is `min`, clamped; stepped values are rounded.
pub fn value_from_y(spec: &LaneSpec, y: f32, top: f32, bottom: f32) -> f32 {
    if bottom <= top {
        return spec.min;
    }
    let t = ((bottom - y) / (bottom - top)).clamp(0.0, 1.0);
    rounded(spec, spec.min + t * (spec.max - spec.min))
}

/// The inverse, for drawing; a value outside the range is drawn on the nearest edge.
pub fn y_from_value(spec: &LaneSpec, v: f32, top: f32, bottom: f32) -> f32 {
    let span = spec.max - spec.min;
    if span <= 0.0 {
        return bottom;
    }
    let t = (v.clamp(spec.min, spec.max) - spec.min) / span;
    bottom - t * (bottom - top)
}

/// How much the value changes for a vertical move of `dy` points in a band `band_h` tall (up is positive).
pub fn value_delta(spec: &LaneSpec, dy: f32, band_h: f32) -> f32 {
    if band_h <= 0.0 {
        return 0.0;
    }
    -dy / band_h * (spec.max - spec.min)
}

/// "-6.0 dB", "Pan +32" / "Pan 0" (the mixer's scale: pan × 64), "45°", "0.35", for a selector the choice name
/// ("Saw"); `unit` of a plug-in parameter appended ("120.0 ms").
pub fn value_label(spec: &LaneSpec, v: f32) -> String {
    match spec.unit {
        Unit::Db => format!("{v:.1} dB"),
        Unit::Pan => {
            let p = (v * 64.0).round() as i32;
            if p > 0 {
                format!("Pan +{p}")
            } else {
                format!("Pan {p}")
            }
        }
        Unit::Degrees => format!("{v:.0}°"),
        Unit::Plain(_) if spec.stepped => {
            let i = v.round().max(0.0) as usize;
            spec.choices.get(i).map_or_else(|| format!("{i}"), |c| (*c).to_string())
        }
        Unit::Plain("") => format!("{v:.2}"),
        Unit::Plain(unit) => format!("{v:.1} {unit}"),
    }
}

/// The grid for automation: the snap step capped at one beat (like the fades and the controller lane), none with
/// Alt or with snap off.
pub fn grid(snap: f64, alt: bool) -> f64 {
    if alt || snap <= 0.0 {
        0.0
    } else {
        snap.min(1.0)
    }
}

/// `b` on the grid, never below 0.
pub fn snap_beat(b: f64, grid: f64) -> f64 {
    let b = if grid > 0.0 { (b / grid).round() * grid } else { b };
    b.max(0.0)
}

/// `points` with a new point at `beat`/`value`, sorted; a point at the same beat (|Δ| < 1e-6) is replaced, as
/// AddAutomationPoint does. Returns the new list and the index of the new point.
pub fn insert_point(points: &[AutomationPoint], beat: f64, value: f32) -> (Vec<AutomationPoint>, usize) {
    let mut out: Vec<AutomationPoint> = points.iter().copied().filter(|p| (p.beat - beat).abs() >= 1e-6).collect();
    let index = out.partition_point(|p| p.beat <= beat);
    out.insert(index, AutomationPoint { beat, value });
    (out, index)
}

/// `points` with point `index` moved to `beat`/`value`: the beat stays between its neighbours (equal beats allowed)
/// and at or after 0, the value inside the spec range (rounded when stepped). Out-of-range index: unchanged copy.
pub fn move_point(points: &[AutomationPoint], index: usize, beat: f64, value: f32, spec: &LaneSpec) -> Vec<AutomationPoint> {
    let mut out = points.to_vec();
    if index >= out.len() {
        return out;
    }
    let lo = if index > 0 { out[index - 1].beat.max(0.0) } else { 0.0 };
    let hi = out.get(index + 1).map_or(f64::INFINITY, |p| p.beat);
    out[index] = AutomationPoint { beat: beat.max(lo).min(hi).max(0.0), value: rounded(spec, value) };
    out
}

/// `points` without point `index`.
pub fn remove_point(points: &[AutomationPoint], index: usize) -> Vec<AutomationPoint> {
    let mut out = points.to_vec();
    if index < out.len() {
        out.remove(index);
    }
    out
}

/// The point under `pos`, the nearest one within HIT_PX. `beat_x` and `value_y` are the lane's mappings.
pub fn point_at(
    points: &[AutomationPoint],
    pos: Pos2,
    beat_x: &dyn Fn(f64) -> f32,
    value_y: &dyn Fn(f32) -> f32,
) -> Option<usize> {
    points
        .iter()
        .enumerate()
        .map(|(i, p)| (i, pos2(beat_x(p.beat), value_y(p.value)).distance(pos)))
        .filter(|(_, d)| *d <= HIT_PX)
        .min_by(|a, b| a.1.total_cmp(&b.1))
        .map(|(i, _)| i)
}

fn same_points(a: &[AutomationPoint], b: &[AutomationPoint]) -> bool {
    a.len() == b.len() && a.iter().zip(b).all(|(p, q)| (p.beat - q.beat).abs() <= 1e-9 && (p.value - q.value).abs() <= 1e-6)
}

/// The single command a gesture sends: None when `after` equals `before` (beats within 1e-9, values within 1e-6),
/// ClearAutomation for the target when `after` is empty, otherwise SetAutomation with `after`.
pub fn gesture_command(track_id: Id, target: &str, before: &[AutomationPoint], after: &[AutomationPoint]) -> Option<Command> {
    if same_points(before, after) {
        return None;
    }
    if after.is_empty() {
        return Some(Command::ClearAutomation { track_id, target: Some(target.to_string()) });
    }
    Some(Command::SetAutomation { track_id, target: target.to_string(), points: after.to_vec() })
}

/// The vertices of the drawn curve between beats `from` and `to`: the value at `from`, the points strictly inside,
/// the value at `to`. The ends use `AutomationLane::value_at` (the renderer's interpolation) and the line between
/// vertices is straight, as the renderer interpolates it, so what is drawn is what plays. Two points on the same
/// beat give a vertical step. Empty when `points` is empty.
pub fn curve(points: &[AutomationPoint], from: f64, to: f64) -> Vec<(f64, f32)> {
    if points.is_empty() {
        return Vec::new();
    }
    let lane = AutomationLane { target: String::new(), points: points.to_vec(), enabled: true };
    let at = |b: f64| lane.value_at(b).unwrap_or(points[0].value);
    let mut out = vec![(from, at(from))];
    out.extend(points.iter().filter(|p| p.beat > from && p.beat < to).map(|p| (p.beat, p.value)));
    out.push((to, at(to)));
    out
}

/// Plain A, pressed once, toggles the view (no Ctrl, Alt, Shift or Command; key repeats ignored).
pub fn toggle_key(key: Key, repeat: bool, mods: Modifiers) -> bool {
    key == Key::A && !repeat && !mods.ctrl && !mods.command && !mods.alt && !mods.shift
}

/// The whole decision: also needs what `transport_keys::keys_on` needs (no text field, Musical Typing off, no drag).
pub fn view_toggle(text_focus: bool, musical_typing: bool, dragging: bool, key: Key, repeat: bool, mods: Modifiers) -> bool {
    crate::transport_keys::keys_on(text_focus, musical_typing, dragging) && toggle_key(key, repeat, mods)
}

/// Whether the arrangement shows automation and which lane each track shows; lives for the whole app session.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct AutomationView {
    pub on: bool,
    chosen: HashMap<Id, String>,
}

impl AutomationView {
    /// The lane shown for `track`: the chosen one while it is valid for the track, otherwise "volume".
    pub fn target(&self, project: &Project, track: &Track) -> String {
        match self.chosen.get(&track.id) {
            Some(t) if lane_spec(project, track, t).is_some() => t.clone(),
            _ => "volume".into(),
        }
    }

    pub fn choose(&mut self, track_id: Id, target: &str) {
        self.chosen.insert(track_id, target.to_string());
    }
}

/// Dragging one point of a lane (a new one when the press began on empty space). Positions are in beats and
/// values, so scrolling during the drag keeps it right.
#[derive(Debug, Clone, PartialEq)]
pub struct PointDrag {
    pub spec: LaneSpec,
    /// The lane when the press began: what Esc leaves and what the release compares with.
    pub orig: Vec<AutomationPoint>,
    /// `orig` with the new point added (equal to `orig` when an existing point is dragged).
    base: Vec<AutomationPoint>,
    index: usize,
    /// The dragged point when the press began, where the press began (beat and screen y).
    start: AutomationPoint,
    grab_beat: f64,
    grab_y: f32,
    /// The lane as the release sends it.
    pub cur: Vec<AutomationPoint>,
}

impl PointDrag {
    /// Drag of existing point `index` of `points`.
    pub fn existing(spec: LaneSpec, points: Vec<AutomationPoint>, index: usize, grab_beat: f64, grab_y: f32) -> Self {
        let index = index.min(points.len().saturating_sub(1));
        let start = points.get(index).copied().unwrap_or(AutomationPoint { beat: grab_beat.max(0.0), value: spec.static_value });
        PointDrag { spec, base: points.clone(), cur: points.clone(), orig: points, index, start, grab_beat, grab_y }
    }

    /// A new point at the press (`beat` already on the grid, `value` from the press height), then dragged.
    pub fn new_point(spec: LaneSpec, points: Vec<AutomationPoint>, beat: f64, value: f32, grab_beat: f64, grab_y: f32) -> Self {
        let value = rounded(&spec, value);
        let (base, index) = insert_point(&points, beat.max(0.0), value);
        let start = base[index];
        PointDrag { spec, cur: base.clone(), base, orig: points, index, start, grab_beat, grab_y }
    }

    /// Moves the point with the pointer: the beat follows the horizontal move on `grid` (it stays put while the
    /// pointer has moved less than STILL_PX horizontally at `px_per_beat`), the value follows the vertical move
    /// (`value_delta` over `band_h`), both limited by `move_point`.
    pub fn update(&mut self, pointer_beat: f64, pointer_y: f32, band_h: f32, px_per_beat: f32, grid: f64) {
        if self.base.is_empty() {
            return;
        }
        let dx = pointer_beat - self.grab_beat;
        let beat = if (dx.abs() as f32) * px_per_beat < STILL_PX { self.start.beat } else { snap_beat(self.start.beat + dx, grid) };
        let value = self.start.value + value_delta(&self.spec, pointer_y - self.grab_y, band_h);
        self.cur = move_point(&self.base, self.index, beat, value, &self.spec);
    }

    /// The index of the dragged point in `cur`.
    pub fn index(&self) -> usize {
        self.index
    }

    /// The dragged point as it is now, for the tooltip.
    pub fn point(&self) -> AutomationPoint {
        self.cur.get(self.index).copied().unwrap_or(self.start)
    }

    /// What the release sends (`gesture_command` of orig → cur).
    pub fn command(&self, track_id: Id) -> Option<Command> {
        gesture_command(track_id, &self.spec.target, &self.orig, &self.cur)
    }
}

/// Where a track's lane is on screen: the whole lane, the part of the window the tracks may use (right of the
/// headers, below the ruler lanes), the x of beat `scroll_beat`, and whether Alt is held.
#[derive(Debug, Clone, Copy)]
pub struct LaneGeom {
    pub lane: Rect,
    pub region_area: Rect,
    pub timeline_x: f32,
    pub alt: bool,
}

/// What `paint_lane` draws.
pub struct LaneLook<'a> {
    pub spec: &'a LaneSpec,
    pub points: &'a [AutomationPoint],
    pub enabled: bool,
    /// The point under the pointer or being dragged.
    pub highlight: Option<usize>,
}

/// Paints the veil over `rect`, the curve (or a dashed line at the lane's static value when it has no points), a
/// dot per point and the lane's name at the bottom left. `band` is the value band, `beat_x` the arrangement's
/// mapping and `x_beat` its inverse.
pub fn paint_lane(
    painter: &egui::Painter,
    rect: Rect,
    band: (f32, f32),
    look: &LaneLook,
    beat_x: &dyn Fn(f64) -> f32,
    x_beat: &dyn Fn(f32) -> f64,
) {
    let painter = painter.with_clip_rect(rect.intersect(painter.clip_rect()));
    painter.rect_filled(rect, 0.0, Color32::from_black_alpha(70));
    let spec = look.spec;
    let y = |v: f32| y_from_value(spec, v, band.0, band.1);
    let color = if look.enabled { theme::ACCENT } else { theme::ACCENT.gamma_multiply(0.4) };
    let mut label = spec.short.clone();
    if look.points.is_empty() {
        let yy = y(spec.static_value);
        let line = [pos2(rect.left(), yy), pos2(rect.right(), yy)];
        painter.extend(Shape::dashed_line(&line, Stroke::new(1.0_f32, theme::TEXT_DIM), 4.0, 3.0));
        label = format!("{label} {}", value_label(spec, spec.static_value));
    } else {
        let vertices: Vec<Pos2> = curve(look.points, x_beat(rect.left()), x_beat(rect.right()))
            .into_iter()
            .map(|(b, v)| pos2(beat_x(b), y(v)))
            .collect();
        painter.add(Shape::line(vertices, Stroke::new(1.5_f32, color)));
        for (i, p) in look.points.iter().enumerate() {
            let c = pos2(beat_x(p.beat), y(p.value));
            if c.x < rect.left() - 6.0 || c.x > rect.right() + 6.0 {
                continue;
            }
            if look.highlight == Some(i) {
                painter.circle(c, 4.5, color, Stroke::new(1.0_f32, theme::SELECT));
            } else {
                painter.circle_filled(c, 3.0, color);
            }
        }
        if !look.enabled {
            label.push_str(" (off)");
        }
    }
    painter.text(pos2(rect.left() + 4.0, rect.bottom() - 2.0), Align2::LEFT_BOTTOM, label, FontId::proportional(10.0), theme::TEXT_DIM);
}

impl OndinaApp {
    /// The automation overlay of one track: registered after the track's regions, so in the lane below their name
    /// bar it takes the presses. A click on empty space adds a point, a drag moves one (or a new one), a double or
    /// right click deletes one. Every gesture sends one command.
    pub(crate) fn automation_overlay(&mut self, ui: &mut egui::Ui, painter: &egui::Painter, g: LaneGeom, track: &Track, project: &Project) {
        let target = self.automation.target(project, track);
        let Some(view_spec) = lane_spec(project, track, &target) else { return };
        let overlay = Rect::from_min_max(pos2(g.lane.left(), g.lane.top() + TOP_BAND_PX), g.lane.max).intersect(g.region_area);
        if !overlay.is_positive() {
            return;
        }
        let band = value_band(g.lane.top(), g.lane.height());
        let (view_points, enabled) = lane_points(track, &view_spec.target);
        let resp = ui.interact(overlay, ui.id().with(("auto", track.id)), Sense::click_and_drag());

        let (tx, scroll, ppb) = (g.timeline_x, self.scroll_beat, self.px_per_beat);
        let beat_x = move |b: f64| tx + ((b - scroll) as f32) * ppb;
        let x_beat = move |x: f32| scroll + ((x - tx) / ppb) as f64;
        let value_y = |v: f32| y_from_value(&view_spec, v, band.0, band.1);
        let step = grid(self.snap, g.alt);

        if resp.drag_started_by(egui::PointerButton::Primary) && self.drag.is_none() {
            // decided from where the press began, not from where egui noticed the drag
            if let Some(o) = ui.input(|i| i.pointer.press_origin()) {
                let press_beat = x_beat(o.x);
                let drag = match point_at(&view_points, o, &beat_x, &value_y) {
                    Some(i) => PointDrag::existing(view_spec.clone(), view_points.clone(), i, press_beat, o.y),
                    None => {
                        let v = value_from_y(&view_spec, o.y, band.0, band.1);
                        PointDrag::new_point(view_spec.clone(), view_points.clone(), snap_beat(press_beat, step), v, press_beat, o.y)
                    }
                };
                self.drag = Some(Drag::AutoPoint { track: track.id, drag });
                self.selected_track = Some(track.id);
            }
        } else if self.drag.is_none() {
            let at = resp.interact_pointer_pos();
            let hit = at.and_then(|pp| point_at(&view_points, pp, &beat_x, &value_y));
            // the second click of a double click finds the point the first one added, and deletes it
            let after = match (hit, at) {
                (Some(i), _) if resp.double_clicked() || resp.secondary_clicked() => Some(remove_point(&view_points, i)),
                (None, Some(pp)) if resp.clicked() => {
                    let v = value_from_y(&view_spec, pp.y, band.0, band.1);
                    Some(insert_point(&view_points, snap_beat(x_beat(pp.x), step), v).0)
                }
                _ => None,
            };
            if resp.clicked() {
                self.selected_track = Some(track.id);
            }
            if let Some(c) = after.and_then(|a| gesture_command(track.id, &view_spec.target, &view_points, &a)) {
                self.cmd(c);
            }
        }

        // a drag on this track draws its preview; Esc drops the drag and the lane is drawn from the project again
        let dragged = match &self.drag {
            Some(Drag::AutoPoint { track: id, drag }) if *id == track.id => Some(drag.clone()),
            _ => None,
        };
        let hover = if self.drag.is_none() { resp.hover_pos() } else { None };
        let hover_hit = hover.and_then(|pp| point_at(&view_points, pp, &beat_x, &value_y));
        let (spec, points, highlight) = match &dragged {
            Some(d) => (&d.spec, d.cur.as_slice(), Some(d.index())),
            None => (&view_spec, view_points.as_slice(), hover_hit),
        };
        let look = LaneLook { spec, points, enabled: enabled || dragged.is_some(), highlight };
        paint_lane(painter, overlay, band, &look, &beat_x, &x_beat);

        if let Some(pp) = hover {
            let tip = match hover_hit {
                Some(i) => {
                    ui.ctx().set_cursor_icon(egui::CursorIcon::Grab);
                    let v = view_points[i].value;
                    format!("{} {}: drag to move, double-click or right-click to delete", view_spec.short, value_label(&view_spec, v))
                }
                None => {
                    ui.ctx().set_cursor_icon(egui::CursorIcon::Crosshair);
                    let v = value_from_y(&view_spec, pp.y, band.0, band.1);
                    format!("{} {}: click to add a point, Alt ignores the grid", view_spec.short, value_label(&view_spec, v))
                }
            };
            resp.on_hover_text_at_pointer(tip);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use eframe::egui::{vec2, Event, PointerButton};
    use ondina_core::model::TrackKind;
    use ondina_studio::{Session, Source};

    fn pt(beat: f64, value: f32) -> AutomationPoint {
        AutomationPoint { beat, value }
    }

    fn close(a: f32, b: f32) -> bool {
        (a - b).abs() < 1e-3
    }

    fn id(v: &serde_json::Value, key: &str) -> Id {
        v[key].as_u64().unwrap()
    }

    /// A session with an Analog Synth track (volume -3, pan 0.5, a compressor, a send at -12 dB to "Reverb") and the
    /// bus. Returns (session, track id, bus id, synth id, compressor id).
    fn rig() -> (Session, Id, Id, Id, Id) {
        let mut s = Session::new(false);
        let add = |kind, name: &str, instrument: Option<&str>| Command::AddTrack {
            kind,
            name: Some(name.into()),
            patch: None,
            instrument: instrument.map(Into::into),
            preset: None,
            index: None,
        };
        let tid = id(&s.execute(add(TrackKind::Instrument, "Lead", Some("analog_synth")), Source::Gui).unwrap(), "track_id");
        let bus = id(&s.execute(add(TrackKind::Bus, "Reverb", None), Source::Gui).unwrap(), "track_id");
        s.execute(Command::AddSend { track_id: tid, bus_id: bus, level_db: Some(-12.0), pre_fader: None }, Source::Gui).unwrap();
        s.execute(Command::SetVolume { track_id: tid, db: -3.0 }, Source::Gui).unwrap();
        s.execute(Command::SetPan { track_id: tid, pan: 0.5 }, Source::Gui).unwrap();
        let comp = id(
            &s.execute(Command::AddEffect { track_id: tid, kind: "compressor".into(), preset: None, index: None }, Source::Gui).unwrap(),
            "plugin_id",
        );
        let synth = s.project.track(tid).unwrap().instrument.as_ref().unwrap().id;
        (s, tid, bus, synth, comp)
    }

    fn spec(s: &Session, tid: Id, target: &str) -> Option<LaneSpec> {
        lane_spec(&s.project, s.project.track(tid).unwrap(), target)
    }

    fn volume_spec() -> LaneSpec {
        LaneSpec::new("volume", "Volume".into(), "Volume".into(), (VOLUME_MIN_DB, VOLUME_MAX_DB), Unit::Db, 0.0)
    }

    fn pan_spec() -> LaneSpec {
        LaneSpec::new("pan", "Pan".into(), "Pan".into(), (-1.0, 1.0), Unit::Pan, 0.0)
    }

    #[test]
    fn lane_spec_gives_ranges_and_static_values() {
        let (s, tid, _, synth, comp) = rig();
        let v = spec(&s, tid, "volume").unwrap();
        assert_eq!((v.min, v.max, v.unit, v.static_value), (-60.0, 6.0, Unit::Db, -3.0));
        let p = spec(&s, tid, "pan").unwrap();
        assert_eq!((p.min, p.max, p.unit, p.static_value), (-1.0, 1.0, Unit::Pan, 0.5));
        let send = spec(&s, tid, "send:0").unwrap();
        assert_eq!((send.min, send.max, send.unit, send.static_value), (-60.0, 6.0, Unit::Db, -12.0));
        assert_eq!((send.label.as_str(), send.short.as_str()), ("Send 1 → Reverb", "Send 1"));
        assert_eq!(spec(&s, tid, "send:1"), None);
        let th = spec(&s, tid, &format!("plugin:{comp}:threshold")).unwrap();
        assert_eq!((th.min, th.max, th.static_value, th.stepped), (-60.0, 0.0, -18.0, false));
        assert_eq!((th.label.as_str(), th.short.as_str()), ("Compressor: Threshold", "Threshold"));
        let wave = spec(&s, tid, &format!("plugin:{synth}:osc1_wave")).unwrap();
        assert!(wave.stepped);
        assert_eq!(wave.min, 0.0);
        assert_eq!(spec(&s, tid, &format!("plugin:{comp}:nope")), None);
        assert_eq!(spec(&s, tid, "plugin:999:x"), None);
        let el = spec(&s, tid, "surround:elevation").unwrap();
        assert_eq!((el.min, el.max, el.unit), (-30.0, 90.0, Unit::Degrees));
        assert_eq!(spec(&s, tid, "tempo"), None);
    }

    #[test]
    fn lane_choices_lists_volume_pan_sends_then_other_lanes() {
        let (mut s, tid, _, _, comp) = rig();
        let th = format!("plugin:{comp}:threshold");
        s.execute(Command::SetAutomation { track_id: tid, target: th.clone(), points: vec![pt(0.0, -20.0), pt(4.0, -10.0)] }, Source::Gui).unwrap();
        let vol = vec![pt(0.0, -6.0), pt(2.0, 0.0), pt(4.0, -3.0)];
        s.execute(Command::SetAutomation { track_id: tid, target: "volume".into(), points: vol }, Source::Gui).unwrap();
        let mut p = s.project.clone();
        p.track_mut(tid).unwrap().automation.push(AutomationLane { target: "send:5".into(), points: vec![pt(0.0, 0.0)], enabled: true });
        let got: Vec<(String, usize)> = lane_choices(&p, p.track(tid).unwrap()).into_iter().map(|(sp, n)| (sp.target, n)).collect();
        assert_eq!(got, vec![("volume".into(), 3), ("pan".into(), 0), ("send:0".into(), 0), (th, 2)]);
    }

    #[test]
    fn the_master_is_not_automatable() {
        assert!(!automatable(MASTER_ID));
        assert!(automatable(1));
    }

    #[test]
    fn the_view_falls_back_to_volume() {
        let (mut s, tid, bus, _, _) = rig();
        let mut view = AutomationView::default();
        assert!(!view.on);
        view.choose(tid, "send:0");
        assert_eq!(view.target(&s.project, s.project.track(tid).unwrap()), "send:0");
        s.execute(Command::RemoveSend { track_id: tid, send_index: 0 }, Source::Gui).unwrap();
        assert_eq!(view.target(&s.project, s.project.track(tid).unwrap()), "volume");
        assert_eq!(view.target(&s.project, s.project.track(bus).unwrap()), "volume");
    }

    #[test]
    fn volume_and_pan_map_to_the_band_and_clamp() {
        let (v, p) = (volume_spec(), pan_spec());
        assert!(close(value_from_y(&v, 20.0, 20.0, 80.0), 6.0));
        assert!(close(value_from_y(&v, 80.0, 20.0, 80.0), -60.0));
        assert!(close(value_from_y(&v, 0.0, 20.0, 80.0), 6.0));
        assert!(close(value_from_y(&v, 200.0, 20.0, 80.0), -60.0));
        assert!(close(value_from_y(&p, 50.0, 20.0, 80.0), 0.0));
        assert!(close(value_from_y(&p, 20.0, 20.0, 80.0), 1.0));
        assert!(close(value_from_y(&p, 80.0, 20.0, 80.0), -1.0));
    }

    #[test]
    fn y_from_value_is_the_inverse_and_clamps() {
        let (v, p) = (volume_spec(), pan_spec());
        for x in [-60.0, -12.0, 0.0, 6.0] {
            assert!(close(value_from_y(&v, y_from_value(&v, x, 20.0, 80.0), 20.0, 80.0), x), "{x}");
        }
        for x in [-1.0, -0.25, 1.0] {
            assert!(close(value_from_y(&p, y_from_value(&p, x, 20.0, 80.0), 20.0, 80.0), x), "{x}");
        }
        assert!(close(y_from_value(&v, -80.0, 20.0, 80.0), 80.0));
        assert!(close(y_from_value(&v, 12.0, 20.0, 80.0), 20.0));
    }

    #[test]
    fn stepped_values_are_whole_numbers() {
        let (s, tid, _, synth, _) = rig();
        let wave = spec(&s, tid, &format!("plugin:{synth}:osc1_wave")).unwrap();
        for k in 0..=60 {
            let v = value_from_y(&wave, 20.0 + k as f32, 20.0, 80.0);
            assert_eq!(v, v.round(), "y {k}");
            assert!((wave.min..=wave.max).contains(&v));
        }
    }

    #[test]
    fn value_delta_is_linear() {
        let (v, p) = (volume_spec(), pan_spec());
        assert!(close(value_delta(&v, -66.0, 66.0), 66.0));
        assert!(close(value_delta(&v, 33.0, 66.0), -33.0));
        assert!(close(value_delta(&p, -20.0, 40.0), 1.0));
        assert_eq!(value_delta(&v, 10.0, 0.0), 0.0);
    }

    #[test]
    fn value_labels() {
        let (s, tid, _, synth, comp) = rig();
        assert_eq!(value_label(&volume_spec(), -6.0), "-6.0 dB");
        assert_eq!(value_label(&pan_spec(), 0.5), "Pan +32");
        assert_eq!(value_label(&pan_spec(), 0.0), "Pan 0");
        assert_eq!(value_label(&pan_spec(), -1.0), "Pan -64");
        assert_eq!(value_label(&spec(&s, tid, "surround:azimuth").unwrap(), 45.0), "45°");
        assert_eq!(value_label(&spec(&s, tid, "surround:spread").unwrap(), 0.35), "0.35");
        assert_eq!(value_label(&spec(&s, tid, &format!("plugin:{comp}:attack")).unwrap(), 120.0), "120.0 ms");
        assert_eq!(value_label(&spec(&s, tid, &format!("plugin:{synth}:osc1_wave")).unwrap(), 0.0), "Saw");
    }

    #[test]
    fn grid_caps_the_snap_at_a_beat_and_alt_frees_it() {
        assert_eq!(grid(0.25, false), 0.25);
        assert_eq!(grid(4.0, false), 1.0);
        assert_eq!(grid(0.25, true), 0.0);
        assert_eq!(grid(0.0, false), 0.0);
        assert_eq!(snap_beat(2.37, 0.25), 2.25);
        assert_eq!(snap_beat(2.37, 0.0), 2.37);
        assert_eq!(snap_beat(-0.4, 0.25), 0.0);
    }

    #[test]
    fn insert_point_keeps_order_and_replaces_the_same_beat() {
        let pts = [pt(0.0, -6.0), pt(4.0, 0.0)];
        let (out, i) = insert_point(&pts, 2.0, -3.0);
        assert_eq!((out, i), (vec![pt(0.0, -6.0), pt(2.0, -3.0), pt(4.0, 0.0)], 1));
        let (out, i) = insert_point(&pts, 4.000_000_1, -10.0);
        assert_eq!((out, i), (vec![pt(0.0, -6.0), pt(4.000_000_1, -10.0)], 1));
        assert_eq!(insert_point(&[], 1.0, 2.0), (vec![pt(1.0, 2.0)], 0));
        // before the first point
        assert_eq!(insert_point(&pts, 0.0, 1.0).1, 0);
    }

    #[test]
    fn move_point_stays_between_neighbours_and_in_range() {
        let v = volume_spec();
        let pts = [pt(0.0, 0.0), pt(2.0, 0.0), pt(4.0, 0.0)];
        assert_eq!(move_point(&pts, 1, 5.0, 0.0, &v)[1].beat, 4.0);
        assert_eq!(move_point(&pts, 1, -1.0, 0.0, &v)[1].beat, 0.0);
        assert_eq!(move_point(&pts, 1, 3.0, 0.0, &v)[1].beat, 3.0);
        assert_eq!(move_point(&pts, 0, -2.0, 0.0, &v)[0].beat, 0.0);
        assert_eq!(move_point(&pts, 2, 9.0, 0.0, &v)[2].beat, 9.0);
        assert_eq!(move_point(&pts, 1, 2.0, 20.0, &v)[1].value, 6.0);
        assert_eq!(move_point(&pts, 1, 2.0, -100.0, &v)[1].value, -60.0);
        assert_eq!(move_point(&pts, 1, 2.0, 3.0, &pan_spec())[1].value, 1.0);
        let mut stepped = volume_spec();
        stepped.stepped = true;
        assert_eq!(move_point(&pts, 1, 2.0, 1.6, &stepped)[1].value, 2.0);
        assert_eq!(move_point(&pts, 9, 1.0, 1.0, &v), pts.to_vec());
        // only the moved point is limited: a point written out of range by an agent stays as it is
        let wild = [pt(0.0, -80.0), pt(2.0, 0.0)];
        assert_eq!(move_point(&wild, 1, 2.0, -1.0, &v)[0].value, -80.0);
    }

    #[test]
    fn remove_point_drops_one_point() {
        let pts = [pt(0.0, 0.0), pt(2.0, 1.0), pt(4.0, 2.0)];
        assert_eq!(remove_point(&pts, 1), vec![pt(0.0, 0.0), pt(4.0, 2.0)]);
        assert_eq!(remove_point(&pts, 7), pts.to_vec());
        assert!(remove_point(&[pt(1.0, 1.0)], 0).is_empty());
    }

    #[test]
    fn gesture_command_is_one_command_or_none() {
        let before = [pt(0.0, -6.0), pt(4.0, 0.0)];
        assert!(gesture_command(3, "volume", &before, &before).is_none());
        assert!(gesture_command(3, "volume", &before, &[pt(1e-12, -6.0), pt(4.0, 0.0)]).is_none());
        let after = [pt(0.0, -6.0), pt(4.0, -1.0)];
        assert!(matches!(
            gesture_command(3, "volume", &before, &after),
            Some(Command::SetAutomation { track_id: 3, target, points }) if target == "volume" && points == after
        ));
        assert!(matches!(
            gesture_command(3, "pan", &before, &[]),
            Some(Command::ClearAutomation { track_id: 3, target: Some(t) }) if t == "pan"
        ));
        assert!(gesture_command(3, "volume", &[], &[]).is_none());
    }

    #[test]
    fn point_at_finds_the_nearest_point_within_reach() {
        let pts = [pt(1.0, 0.0), pt(2.0, 0.0)];
        let bx = |b: f64| (b * 10.0) as f32;
        let vy = |v: f32| 50.0 - v;
        assert_eq!(point_at(&pts, pos2(10.0, 54.0), &bx, &vy), Some(0));
        assert_eq!(point_at(&pts, pos2(10.0, 58.0), &bx, &vy), None);
        assert_eq!(point_at(&pts, pos2(16.0, 50.0), &bx, &vy), Some(1));
        assert_eq!(point_at(&pts, pos2(14.0, 50.0), &bx, &vy), Some(0));
        assert_eq!(point_at(&[], pos2(10.0, 50.0), &bx, &vy), None);
    }

    #[test]
    fn dragging_a_point_follows_the_pointer_on_the_grid() {
        let pts = vec![pt(0.0, -6.0), pt(4.0, -6.0), pt(8.0, 0.0)];
        let mut d = PointDrag::existing(volume_spec(), pts, 1, 4.0, 50.0);
        d.update(5.1, 50.0, 66.0, 20.0, 0.25);
        assert_eq!(d.point(), pt(5.0, -6.0));
        d.update(9.0, 50.0, 66.0, 20.0, 0.25);
        assert_eq!(d.point().beat, 8.0);
        d.update(4.05, 17.0, 66.0, 20.0, 0.25);
        assert_eq!(d.point(), pt(4.0, 6.0));
        assert_eq!(d.cur.len(), 3);
    }

    #[test]
    fn a_still_press_changes_nothing() {
        let pts = vec![pt(0.0, -6.0), pt(4.1, -6.0)];
        let mut d = PointDrag::existing(volume_spec(), pts, 1, 4.1, 50.0);
        d.update(4.1, 50.0, 66.0, 20.0, 0.25);
        assert!(d.command(3).is_none());
        assert_eq!(d.point(), pt(4.1, -6.0), "a press on a point off the grid leaves it there");
    }

    #[test]
    fn a_new_point_is_added_and_dragged() {
        let pts = vec![pt(0.0, -6.0), pt(4.0, -6.0)];
        let d = PointDrag::new_point(volume_spec(), pts.clone(), 2.0, -12.0, 2.04, 50.0);
        assert_eq!(d.cur, vec![pt(0.0, -6.0), pt(2.0, -12.0), pt(4.0, -6.0)]);
        assert_eq!(d.orig, pts);
        assert!(matches!(d.command(3), Some(Command::SetAutomation { points, .. }) if points.len() == 3));
        let mut d = d;
        d.update(2.06, 40.0, 66.0, 20.0, 0.25);
        assert_eq!(d.point(), pt(2.0, -2.0));
    }

    #[test]
    fn the_curve_is_the_renderers_interpolation() {
        let pts = vec![pt(1.0, -12.0), pt(3.0, 0.0), pt(3.0, -6.0), pt(6.0, 6.0)];
        let lane = AutomationLane { target: "volume".into(), points: pts.clone(), enabled: true };
        let c = curve(&pts, 0.0, 8.0);
        assert_eq!(c.first(), Some(&(0.0, -12.0)));
        assert_eq!(c.last(), Some(&(8.0, 6.0)));
        assert!(c.windows(2).all(|w| w[0].0 <= w[1].0));
        // the straight lines between vertices give the renderer's value everywhere (off the step at beat 3)
        for k in 0..=80 {
            let b = f64::from(k) * 0.1 + 0.05;
            if b > 8.0 {
                break;
            }
            let i = c.partition_point(|v| v.0 <= b);
            let (a, z) = (c[i - 1], c[i]);
            let drawn = a.1 + (z.1 - a.1) * ((b - a.0) / (z.0 - a.0)) as f32;
            assert!(close(drawn, lane.value_at(b).unwrap()), "beat {b}: {drawn}");
        }
        let inner = curve(&pts, 2.0, 5.0);
        assert_eq!(inner.len(), 4, "only the points strictly inside, plus the two ends");
        assert!(close(inner[0].1, lane.value_at(2.0).unwrap()) && close(inner[3].1, lane.value_at(5.0).unwrap()));
        assert!(curve(&[], 0.0, 8.0).is_empty());
    }

    #[test]
    fn only_a_plain_a_press_toggles() {
        assert!(toggle_key(Key::A, false, Modifiers::NONE));
        assert!(!toggle_key(Key::A, true, Modifiers::NONE));
        for m in [Modifiers::CTRL, Modifiers::COMMAND, Modifiers::ALT, Modifiers::SHIFT] {
            assert!(!toggle_key(Key::A, false, m), "{m:?}");
        }
        assert!(!toggle_key(Key::S, false, Modifiers::NONE));
    }

    #[test]
    fn the_toggle_is_off_while_typing_with_musical_typing_or_while_dragging() {
        assert!(view_toggle(false, false, false, Key::A, false, Modifiers::NONE));
        assert!(!view_toggle(true, false, false, Key::A, false, Modifiers::NONE));
        assert!(!view_toggle(false, true, false, Key::A, false, Modifiers::NONE));
        assert!(!view_toggle(false, false, true, Key::A, false, Modifiers::NONE));
        assert!(!view_toggle(false, false, false, Key::A, true, Modifiers::NONE));
    }

    fn volume_lane(s: &Session, tid: Id) -> Option<Vec<AutomationPoint>> {
        s.project.track(tid).unwrap().automation.iter().find(|l| l.target == "volume").map(|l| l.points.clone())
    }

    #[test]
    fn a_gesture_is_one_undo_step() {
        let (mut s, tid, _, _, _) = rig();
        let start = vec![pt(0.0, -6.0), pt(4.0, -6.0)];
        s.execute(Command::SetAutomation { track_id: tid, target: "volume".into(), points: start.clone() }, Source::Gui).unwrap();
        let spec = spec(&s, tid, "volume").unwrap();

        // (a) a moved point
        let mut d = PointDrag::existing(spec.clone(), start.clone(), 1, 4.0, 50.0);
        d.update(6.0, 40.0, 66.0, 20.0, 0.25);
        s.execute(d.command(tid).unwrap(), Source::Gui).unwrap();
        assert_eq!(volume_lane(&s, tid), Some(d.cur.clone()));
        assert_eq!(s.can_undo(), Some("set_automation"));
        s.execute(Command::Undo {}, Source::Gui).unwrap();
        assert_eq!(volume_lane(&s, tid), Some(start.clone()));
        s.execute(Command::Redo {}, Source::Gui).unwrap();
        assert_eq!(volume_lane(&s, tid), Some(d.cur.clone()));
        s.execute(Command::Undo {}, Source::Gui).unwrap();

        // (b) a click on empty space
        let added = insert_point(&start, 2.0, -20.0).0;
        s.execute(gesture_command(tid, "volume", &start, &added).unwrap(), Source::Gui).unwrap();
        assert_eq!(volume_lane(&s, tid), Some(added));
        s.execute(Command::Undo {}, Source::Gui).unwrap();
        assert_eq!(volume_lane(&s, tid), Some(start.clone()));

        // (c) deleting both points, one at a time: the last one clears the lane
        let one = remove_point(&start, 0);
        s.execute(gesture_command(tid, "volume", &start, &one).unwrap(), Source::Gui).unwrap();
        let none = remove_point(&one, 0);
        let c = gesture_command(tid, "volume", &one, &none).unwrap();
        assert!(matches!(c, Command::ClearAutomation { .. }));
        s.execute(c, Source::Gui).unwrap();
        assert_eq!(volume_lane(&s, tid), None);
        s.execute(Command::Undo {}, Source::Gui).unwrap();
        assert_eq!(volume_lane(&s, tid), Some(one));
    }

    /// Runs four frames of a click at `at` over a "region" widget and, when `overlay_on`, an "auto" widget
    /// registered after it on the region minus its top band. Returns which widget saw the click.
    fn clicked_widget(at: Pos2, overlay_on: bool) -> Vec<&'static str> {
        let ctx = egui::Context::default();
        let region = Rect::from_min_max(pos2(100.0, 100.0), pos2(300.0, 160.0));
        let overlay = Rect::from_min_max(pos2(100.0, 100.0 + TOP_BAND_PX), region.max);
        let frames = [
            vec![Event::PointerMoved(at)],
            vec![Event::PointerButton { pos: at, button: PointerButton::Primary, pressed: true, modifiers: Modifiers::NONE }],
            vec![Event::PointerButton { pos: at, button: PointerButton::Primary, pressed: false, modifiers: Modifiers::NONE }],
            vec![],
        ];
        let mut got = Vec::new();
        for (k, events) in frames.into_iter().enumerate() {
            let raw = egui::RawInput {
                screen_rect: Some(Rect::from_min_size(Pos2::ZERO, vec2(800.0, 600.0))),
                time: Some(k as f64 * 0.016),
                events,
                ..Default::default()
            };
            let _ = ctx.run(raw, |ctx| {
                egui::CentralPanel::default().show(ctx, |ui| {
                    if ui.interact(region, egui::Id::new("region"), Sense::click_and_drag()).clicked() {
                        got.push("region");
                    }
                    if overlay_on && ui.interact(overlay, egui::Id::new("auto"), Sense::click_and_drag()).clicked() {
                        got.push("auto");
                    }
                });
            });
        }
        got
    }

    #[test]
    fn egui_gives_presses_to_the_overlay_only_where_it_is_registered() {
        assert_eq!(clicked_widget(pos2(200.0, 140.0), true), vec!["auto"]);
        assert_eq!(clicked_widget(pos2(200.0, 108.0), true), vec!["region"]);
        assert_eq!(clicked_widget(pos2(200.0, 140.0), false), vec!["region"]);
    }
}
