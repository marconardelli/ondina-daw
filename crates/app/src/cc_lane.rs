//! The piano roll's controller lane: which controller it shows, how a mouse stroke becomes controller events
//! and how they are drawn. Pure functions, so the editor only wires the mouse to them.

use std::collections::BTreeMap;

use eframe::egui::{self, pos2, Color32, Rect, Stroke};
use ondina_core::command::CcPoint;
use ondina_core::model::CcEvent;

pub const SUSTAIN: u8 = 64;
pub const PITCH_BEND: u8 = 128;
/// The controllers the lane always offers, in menu order.
pub const STANDARD: [u8; 6] = [64, 1, 11, 7, 10, 128];
/// Height of the lane's header (open / close, controller selector, hints); a closed lane is only its header.
pub const HEADER_H: f32 = 20.0;
/// Height of the open lane (header and drawing area) and of the closed one (points).
pub const OPEN_H: f32 = HEADER_H + 56.0;
pub const CLOSED_H: f32 = HEADER_H;
/// The note grid never gets shorter than this (eight 12-point rows) because of the lane.
pub const MIN_GRID_H: f32 = 96.0;
/// Drawing step with snap off: a 1/32 note.
pub const FREE_STEP: f64 = 0.125;

const PAN: u8 = 10;

/// What the lane shows; lives for the whole app session.
#[derive(Debug, Clone, PartialEq)]
pub struct CcLaneState {
    pub open: bool,
    pub controller: u8,
}

impl Default for CcLaneState {
    fn default() -> Self {
        // closed at first, so the note grid gets the room; its header stays to open it
        CcLaneState { open: false, controller: SUSTAIN }
    }
}

/// "Sustain (64)", "Mod wheel (1)", "Expression (11)", "Volume (7)", "Pan (10)", "Pitch bend", otherwise "CC 74".
pub fn controller_name(c: u8) -> String {
    match c {
        64 => "Sustain (64)".into(),
        1 => "Mod wheel (1)".into(),
        11 => "Expression (11)".into(),
        7 => "Volume (7)".into(),
        10 => "Pan (10)".into(),
        128 => "Pitch bend".into(),
        _ => format!("CC {c}"),
    }
}

/// Short label for the lane's selector: "Sustain", "Mod", "Expr", "Volume", "Pan", "Bend", "CC 74".
pub fn short_name(c: u8) -> String {
    match c {
        64 => "Sustain".into(),
        1 => "Mod".into(),
        11 => "Expr".into(),
        7 => "Volume".into(),
        10 => "Pan".into(),
        128 => "Bend".into(),
        _ => format!("CC {c}"),
    }
}

/// The lane's choices with how many events of each the region has: the STANDARD six (count may be 0), then any other
/// controller present in `cc`, by number.
pub fn lane_choices(cc: &[CcEvent]) -> Vec<(u8, usize)> {
    let mut counts: BTreeMap<u8, usize> = BTreeMap::new();
    for e in cc {
        *counts.entry(e.controller).or_default() += 1;
    }
    let mut out: Vec<(u8, usize)> = STANDARD.iter().map(|c| (*c, counts.get(c).copied().unwrap_or(0))).collect();
    out.extend(counts.into_iter().filter(|(c, _)| !STANDARD.contains(c)));
    out
}

/// Drawing step in beats: the snap step (at most a quarter note, like the rest of the piano roll), FREE_STEP with snap off.
pub fn draw_step(snap: f64) -> f64 {
    if snap > 0.0 {
        snap.min(1.0)
    } else {
        FREE_STEP
    }
}

/// Height of the lane for the space the editor has: CLOSED_H when closed; OPEN_H when open, shrunk (never below
/// CLOSED_H) so that the note grid keeps MIN_GRID_H next to the other fixed parts (ruler and velocity lane,
/// `fixed_h` together).
pub fn lane_height(open: bool, full_h: f32, fixed_h: f32) -> f32 {
    if !open {
        return CLOSED_H;
    }
    (full_h - fixed_h - MIN_GRID_H).clamp(CLOSED_H, OPEN_H)
}

/// The drawing area of a lane laid out in `lane` (its header taken off the top); None when too short to draw in.
pub fn body_rect(lane: Rect) -> Option<Rect> {
    let body = Rect::from_min_max(pos2(lane.left(), lane.top() + HEADER_H), lane.max);
    (body.height() >= 12.0).then_some(body)
}

/// Value 0..127 for a pointer height: `bottom` is 0, `top` is 127, clamped.
pub fn value_from_y(y: f32, top: f32, bottom: f32) -> u8 {
    if bottom <= top {
        return 0;
    }
    let t = ((bottom - y) / (bottom - top)).clamp(0.0, 1.0);
    (t * 127.0).round() as u8
}

/// The inverse, for drawing.
pub fn y_from_value(v: u8, top: f32, bottom: f32) -> f32 {
    bottom - f32::from(v.min(127)) / 127.0 * (bottom - top)
}

/// The sustain pedal is on or off: values from 64 become 127, the rest 0. Other controllers keep the value.
pub fn lane_value(controller: u8, raw: u8) -> u8 {
    if controller == SUSTAIN {
        if raw >= 64 {
            127
        } else {
            0
        }
    } else {
        raw.min(127)
    }
}

fn signed(v: u8) -> String {
    let d = i32::from(v) - 64;
    if d > 0 {
        format!("+{d}")
    } else {
        d.to_string()
    }
}

/// Text shown while drawing: "Sustain on"/"Sustain off", "Bend +12"/"Bend -3"/"Bend 0", "Pan -20"/"Pan 0"/"Pan +20",
/// otherwise "<short name> <value>".
pub fn value_label(controller: u8, value: u8) -> String {
    match controller {
        SUSTAIN => if value >= 64 { "Sustain on" } else { "Sustain off" }.into(),
        PITCH_BEND => format!("Bend {}", signed(value)),
        PAN => format!("Pan {}", signed(value)),
        c => format!("{} {value}", short_name(c)),
    }
}

/// Value in force at `beat`: the last event of `controller` with beat < `beat` (ties: the later one in the list).
pub fn value_before(cc: &[CcEvent], controller: u8, beat: f64) -> Option<u8> {
    let mut best: Option<&CcEvent> = None;
    for e in cc.iter().filter(|e| e.controller == controller && e.beat < beat) {
        if best.is_none_or(|b| e.beat >= b.beat) {
            best = Some(e);
        }
    }
    best.map(|e| e.value)
}

/// A stretch of constant value, for drawing: [start, end) in region beats.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Segment {
    pub start: f64,
    pub end: f64,
    pub value: u8,
}

/// The controller's events as constant stretches inside [0, length): each event lasts until the next one or the region
/// end; events at or past `length` are skipped (the renderer does not play them), zero-width stretches are dropped.
pub fn segments(cc: &[CcEvent], controller: u8, length: f64) -> Vec<Segment> {
    let mut ev: Vec<&CcEvent> = cc.iter().filter(|e| e.controller == controller && e.beat < length).collect();
    ev.sort_by(|a, b| a.beat.total_cmp(&b.beat));
    let mut out = Vec::with_capacity(ev.len());
    for (i, e) in ev.iter().enumerate() {
        let start = e.beat.max(0.0);
        let end = ev.get(i + 1).map_or(length, |n| n.beat);
        if end > start {
            out.push(Segment { start, end, value: e.value });
        }
    }
    out
}

/// Why the lane may not change the sound: None for VST3 instruments, and for Sustain on the built-in keyboard
/// instruments (Analog Synth, FM Synth, Organ, Plucked String, Sampler); otherwise a short sentence for the lane.
/// `kind` is the track instrument's plug-in kind.
pub fn ignored_hint(kind: Option<&str>, controller: u8) -> Option<&'static str> {
    match kind {
        None | Some("vst3") => None,
        Some("analog_synth" | "fm_synth" | "organ" | "pluck_synth" | "sampler") if controller == SUSTAIN => None,
        Some(_) => Some("This instrument ignores this controller: the built-in instruments use only Sustain (the Drum Synth not even that), VST3 instruments get every controller"),
    }
}

/// One mouse stroke in the lane: drawing (left button) or erasing (right button) on a grid of `step` beats.
#[derive(Debug, Clone, PartialEq)]
pub struct CcStroke {
    pub controller: u8,
    pub erase: bool,
    step: f64,
    length: f64,
    /// Grid slot -> value (0 for an erase stroke).
    slots: BTreeMap<i64, u8>,
    /// Last slot and raw value added, to fill the slots a fast mouse skips.
    last: Option<(i64, u8)>,
}

impl CcStroke {
    pub fn new(controller: u8, erase: bool, step: f64, length: f64) -> Self {
        let step = if step > 0.0 { step } else { FREE_STEP };
        CcStroke { controller, erase, step, length, slots: BTreeMap::new(), last: None }
    }

    fn put(&mut self, slot: i64, raw: u8) {
        let v = if self.erase { 0 } else { lane_value(self.controller, raw) };
        self.slots.insert(slot, v);
    }

    /// Adds the pointer at `beat` (clamped into the region) with value `raw` (0..127, before `lane_value`). The slots
    /// between the previous point and this one get values interpolated linearly, so a fast drag leaves no gaps.
    /// A later point in the same slot replaces the earlier one. Does nothing when length <= 0.
    pub fn add(&mut self, beat: f64, raw: u8) {
        if self.length <= 0.0 {
            return;
        }
        let raw = raw.min(127);
        let slot = (beat.clamp(0.0, self.length - 1e-9) / self.step).floor() as i64;
        match self.last {
            Some((ls, lv)) if ls != slot => {
                let n = (slot - ls).abs();
                let dir = (slot - ls).signum();
                for k in 1..=n {
                    let v = f64::from(lv) + (f64::from(raw) - f64::from(lv)) * k as f64 / n as f64;
                    self.put(ls + dir * k, v.round() as u8);
                }
            }
            _ => self.put(slot, raw),
        }
        self.last = Some((slot, raw));
    }

    /// The touched range [first slot start, end of last slot), the end clamped to the region length; None if empty.
    pub fn range(&self) -> Option<(f64, f64)> {
        let first = *self.slots.keys().next()?;
        let last = *self.slots.keys().next_back()?;
        Some((first as f64 * self.step, ((last + 1) as f64 * self.step).min(self.length)))
    }

    /// The drawn events: one per slot at the slot start, value after `lane_value`, a value equal to the previous one
    /// dropped (the first always kept). Empty for an erase stroke.
    pub fn points(&self) -> Vec<CcPoint> {
        if self.erase {
            return Vec::new();
        }
        let mut out: Vec<CcPoint> = Vec::new();
        for (slot, v) in &self.slots {
            if out.last().is_none_or(|p| p.value != *v) {
                out.push(CcPoint { beat: *slot as f64 * self.step, value: *v });
            }
        }
        out
    }

    /// What `replace_cc` gets: (from, to, events). For a drawing stroke, when `to` is before the region end, no event
    /// of the controller sits exactly at `to` (|beat - to| < 1e-9) and `value_before(current, controller, to)` is some
    /// value different from the last drawn one, an event with that value is added at `to`: the controller goes back
    /// to what it was, so nothing changes outside the touched range. An erase stroke never adds events.
    pub fn command_parts(&self, current: &[CcEvent]) -> Option<(f64, f64, Vec<CcPoint>)> {
        let (from, to) = self.range()?;
        let mut events = self.points();
        if let Some(last) = events.last().map(|p| p.value) {
            let at_to = current.iter().any(|e| e.controller == self.controller && (e.beat - to).abs() < 1e-9);
            if to < self.length - 1e-9 && !at_to {
                if let Some(v) = value_before(current, self.controller, to).filter(|v| *v != last) {
                    events.push(CcPoint { beat: to, value: v });
                }
            }
        }
        Some((from, to, events))
    }

    /// `current` as it will be after the stroke (uses `ondina_core::ops::replace_cc_events`).
    pub fn preview(&self, current: &[CcEvent]) -> Vec<CcEvent> {
        let mut out = current.to_vec();
        if let Some((from, to, events)) = self.command_parts(current) {
            ondina_core::ops::replace_cc_events(&mut out, self.controller, from, to, &events);
        }
        out
    }

    /// The value of the last point added (after `lane_value`), for the label shown while drawing.
    pub fn last_value(&self) -> Option<u8> {
        self.last.map(|(_, raw)| lane_value(self.controller, raw))
    }
}

/// Paints the controller's stretches in `rect`: the sustain as filled bars where it is on (nothing where it is off),
/// other controllers as a stepped line with a dot at each event, plus a dim centre line for Pan and Pitch bend.
/// `beat_x` maps a region beat to screen x (the piano roll's mapping, so the lane follows zoom and scroll).
pub fn paint(painter: &egui::Painter, rect: Rect, segs: &[Segment], controller: u8, color: Color32, beat_x: &dyn Fn(f64) -> f32) {
    let visible = |s: &Segment| beat_x(s.end) >= rect.left() && beat_x(s.start) <= rect.right();
    if controller == SUSTAIN {
        let fill = color.gamma_multiply(0.75);
        for s in segs.iter().filter(|s| s.value >= 64 && visible(s)) {
            let r = Rect::from_min_max(pos2(beat_x(s.start), rect.top() + 4.0), pos2(beat_x(s.end), rect.bottom() - 2.0));
            painter.rect_filled(r, 0.0, fill);
        }
        return;
    }
    let (top, bottom) = (rect.top() + 3.0, rect.bottom() - 3.0);
    if controller == PAN || controller == PITCH_BEND {
        let y = y_from_value(64, top, bottom);
        painter.line_segment([pos2(rect.left(), y), pos2(rect.right(), y)], Stroke::new(1.0_f32, crate::theme::CENTER_LINE));
    }
    let line = Stroke::new(1.5_f32, color);
    for (i, s) in segs.iter().enumerate() {
        if !visible(s) {
            continue;
        }
        let y = y_from_value(s.value, top, bottom);
        let (x0, x1) = (beat_x(s.start), beat_x(s.end));
        painter.line_segment([pos2(x0, y), pos2(x1, y)], line);
        if let Some(next) = segs.get(i + 1).filter(|n| (n.start - s.end).abs() < 1e-9) {
            painter.line_segment([pos2(x1, y), pos2(x1, y_from_value(next.value, top, bottom))], line);
        }
        painter.circle_filled(pos2(x0, y), 2.0, color);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use ondina_core::command::Command;
    use ondina_core::model::{Id, RegionContent, TrackKind};
    use ondina_studio::{Session, Source};

    fn cc(beat: f64, controller: u8, value: u8) -> CcEvent {
        CcEvent { beat, controller, value }
    }

    fn pts(p: &[CcPoint]) -> Vec<(f64, u8)> {
        p.iter().map(|p| (p.beat, p.value)).collect()
    }

    /// Value in force at `b`: the last event of `controller` with beat <= b.
    fn value_at(cc: &[CcEvent], controller: u8, b: f64) -> Option<u8> {
        value_before(cc, controller, b + 1e-12)
    }

    #[test]
    fn draw_step_follows_snap() {
        assert_eq!(draw_step(0.0), 0.125);
        assert_eq!(draw_step(0.25), 0.25);
        assert_eq!(draw_step(4.0), 1.0);
        assert_eq!(draw_step(0.125), 0.125);
    }

    #[test]
    fn lane_height_shrinks_for_small_editors() {
        assert_eq!(lane_height(false, 600.0, 56.0), CLOSED_H);
        assert_eq!(lane_height(true, 600.0, 56.0), OPEN_H);
        let h = lane_height(true, 200.0, 56.0);
        assert!((CLOSED_H..OPEN_H).contains(&h));
        assert!(200.0 - 56.0 - h >= MIN_GRID_H - 1e-3, "the grid keeps its minimum height");
        assert_eq!(lane_height(true, 100.0, 56.0), CLOSED_H);
    }

    #[test]
    fn the_header_is_never_drawn_on() {
        let lane = Rect::from_min_size(pos2(0.0, 100.0), egui::vec2(400.0, OPEN_H));
        let body = body_rect(lane).expect("an open lane has room to draw");
        assert_eq!(body.top(), lane.top() + HEADER_H);
        assert_eq!(body.bottom(), lane.bottom());
        assert_eq!(body_rect(Rect::from_min_size(lane.min, egui::vec2(400.0, CLOSED_H))), None);
    }

    #[test]
    fn value_from_y_maps_top_to_127_and_clamps() {
        assert_eq!(value_from_y(10.0, 10.0, 137.0), 127);
        assert_eq!(value_from_y(137.0, 10.0, 137.0), 0);
        assert_eq!(value_from_y(0.0, 10.0, 137.0), 127);
        assert_eq!(value_from_y(500.0, 10.0, 137.0), 0);
        assert_eq!(value_from_y(73.5, 10.0, 137.0), 64);
    }

    #[test]
    fn y_from_value_is_the_inverse() {
        for v in [0u8, 1, 30, 64, 100, 127] {
            let back = value_from_y(y_from_value(v, 20.0, 74.0), 20.0, 74.0);
            assert!((i32::from(back) - i32::from(v)).abs() <= 1, "{v} -> {back}");
        }
    }

    #[test]
    fn sustain_rounds_to_on_or_off() {
        assert_eq!(lane_value(SUSTAIN, 63), 0);
        assert_eq!(lane_value(SUSTAIN, 64), 127);
        assert_eq!(lane_value(SUSTAIN, 0), 0);
        assert_eq!(lane_value(SUSTAIN, 127), 127);
        assert_eq!(lane_value(1, 63), 63);
    }

    #[test]
    fn value_label_names_the_value() {
        assert_eq!(value_label(SUSTAIN, 127), "Sustain on");
        assert_eq!(value_label(SUSTAIN, 0), "Sustain off");
        assert_eq!(value_label(PITCH_BEND, 76), "Bend +12");
        assert_eq!(value_label(PITCH_BEND, 64), "Bend 0");
        assert_eq!(value_label(PITCH_BEND, 61), "Bend -3");
        assert_eq!(value_label(10, 44), "Pan -20");
        assert_eq!(value_label(10, 64), "Pan 0");
        assert_eq!(value_label(10, 84), "Pan +20");
        assert_eq!(value_label(1, 87), "Mod 87");
    }

    #[test]
    fn lane_choices_lists_standard_then_present() {
        let v = [cc(0.0, 64, 127), cc(0.0, 74, 1), cc(1.0, 64, 0), cc(1.0, 1, 5), cc(2.0, 74, 3), cc(3.0, 64, 127)];
        assert_eq!(lane_choices(&v), vec![(64, 3), (1, 1), (11, 0), (7, 0), (10, 0), (128, 0), (74, 2)]);
        assert_eq!(lane_choices(&[]).len(), 6);
    }

    #[test]
    fn controller_names() {
        let names: Vec<String> = STANDARD.iter().map(|c| controller_name(*c)).collect();
        assert_eq!(names, ["Sustain (64)", "Mod wheel (1)", "Expression (11)", "Volume (7)", "Pan (10)", "Pitch bend"]);
        assert_eq!(controller_name(74), "CC 74");
        let short: Vec<String> = STANDARD.iter().map(|c| short_name(*c)).collect();
        assert_eq!(short, ["Sustain", "Mod", "Expr", "Volume", "Pan", "Bend"]);
        assert_eq!(short_name(74), "CC 74");
    }

    #[test]
    fn segments_hold_until_the_next_event() {
        let v = [cc(0.0, 64, 127), cc(0.5, 1, 9), cc(1.0, 64, 0), cc(2.0, 64, 127), cc(5.0, 64, 0)];
        let s = segments(&v, 64, 4.0);
        assert_eq!(s, vec![Segment { start: 0.0, end: 1.0, value: 127 }, Segment { start: 1.0, end: 2.0, value: 0 }, Segment { start: 2.0, end: 4.0, value: 127 }]);
        // two events at the same beat: the second one holds
        let v = [cc(0.0, 1, 10), cc(0.0, 1, 20)];
        assert_eq!(segments(&v, 1, 2.0), vec![Segment { start: 0.0, end: 2.0, value: 20 }]);
        assert!(segments(&v, 64, 2.0).is_empty());
    }

    #[test]
    fn ignored_hint_cases() {
        assert_eq!(ignored_hint(Some("vst3"), 1), None);
        assert_eq!(ignored_hint(Some("vst3"), 64), None);
        for k in ["analog_synth", "fm_synth", "organ", "pluck_synth", "sampler"] {
            assert_eq!(ignored_hint(Some(k), 64), None);
            assert!(ignored_hint(Some(k), 1).is_some());
        }
        assert!(ignored_hint(Some("drum_synth"), 64).is_some());
        assert_eq!(ignored_hint(None, 1), None);
    }

    #[test]
    fn value_before_takes_the_last_earlier_event() {
        let v = [cc(0.0, 64, 127), cc(1.0, 1, 5), cc(1.0, 64, 0), cc(1.0, 64, 100), cc(2.0, 64, 30)];
        assert_eq!(value_before(&v, 64, 0.0), None);
        assert_eq!(value_before(&v, 64, 0.5), Some(127));
        assert_eq!(value_before(&v, 64, 1.0), Some(127), "an event at the beat itself is not before it");
        assert_eq!(value_before(&v, 64, 1.5), Some(100), "ties: the later one in the list");
        assert_eq!(value_before(&v, 64, 9.0), Some(30));
        assert_eq!(value_before(&v, 11, 9.0), None);
    }

    #[test]
    fn stroke_makes_one_event_per_step() {
        let mut s = CcStroke::new(1, false, 0.25, 8.0);
        s.add(0.1, 10);
        s.add(0.3, 20);
        s.add(0.6, 30);
        assert_eq!(pts(&s.points()), vec![(0.0, 10), (0.25, 20), (0.5, 30)]);
        assert_eq!(s.range(), Some((0.0, 0.75)));
        assert_eq!(s.last_value(), Some(30));
    }

    #[test]
    fn fast_stroke_fills_the_skipped_steps() {
        let mut s = CcStroke::new(1, false, 0.25, 8.0);
        s.add(0.0, 0);
        s.add(1.0, 100);
        assert_eq!(pts(&s.points()), vec![(0.0, 0), (0.25, 25), (0.5, 50), (0.75, 75), (1.0, 100)]);
    }

    #[test]
    fn stroke_backwards_and_over_itself() {
        let mut s = CcStroke::new(1, false, 0.25, 8.0);
        s.add(2.0, 10);
        s.add(1.0, 50);
        s.add(1.5, 99);
        assert_eq!(s.range(), Some((1.0, 2.25)));
        let p = s.points();
        assert_eq!(p.iter().find(|p| p.beat == 1.5).map(|p| p.value), Some(99));
        assert!(p.windows(2).all(|w| w[0].beat < w[1].beat));
    }

    #[test]
    fn repeated_values_are_dropped() {
        let mut s = CcStroke::new(SUSTAIN, false, 0.25, 8.0);
        for k in 0..8 {
            s.add(f64::from(k) * 0.25, 120 - k as u8);
        }
        assert_eq!(pts(&s.points()), vec![(0.0, 127)]);
        assert_eq!(s.range(), Some((0.0, 2.0)));
    }

    #[test]
    fn stroke_is_clamped_to_the_region() {
        let mut s = CcStroke::new(1, false, 0.25, 4.0);
        s.add(-1.0, 10);
        s.add(99.0, 20);
        let (from, to) = s.range().unwrap();
        assert!(from >= 0.0 && to <= 4.0 && from < to, "({from}, {to})");
        assert_eq!((from, to), (0.0, 4.0));
        assert!(s.points().iter().all(|p| p.beat < 4.0));
        // a region of length 0 takes nothing
        let mut z = CcStroke::new(1, false, 0.25, 0.0);
        z.add(0.0, 10);
        assert_eq!(z.range(), None);
        assert_eq!(z.command_parts(&[]), None);
    }

    #[test]
    fn erase_stroke_has_a_range_and_no_events() {
        let mut s = CcStroke::new(SUSTAIN, true, 0.5, 8.0);
        s.add(1.1, 127);
        s.add(2.2, 0);
        assert!(s.points().is_empty());
        let current = [cc(0.0, 64, 127), cc(4.0, 64, 0)];
        assert_eq!(s.command_parts(&current), Some((1.0, 2.5, vec![])));
    }

    #[test]
    fn drawing_restores_the_value_after_the_range() {
        let current = [cc(0.0, 64, 127), cc(4.0, 64, 0)];
        let mut s = CcStroke::new(SUSTAIN, false, 0.5, 8.0);
        s.add(1.0, 10);
        s.add(1.6, 10);
        let (from, to, ev) = s.command_parts(&current).unwrap();
        assert_eq!((from, to), (1.0, 2.0));
        assert_eq!(pts(&ev), vec![(1.0, 0), (2.0, 127)]);

        // an original event exactly at `to`: no restore
        let with_event = [cc(0.0, 64, 127), cc(2.0, 64, 127), cc(4.0, 64, 0)];
        assert_eq!(pts(&s.command_parts(&with_event).unwrap().2), vec![(1.0, 0)]);

        // the last drawn value is already the one in force: no restore
        let mut up = CcStroke::new(SUSTAIN, false, 0.5, 8.0);
        up.add(1.0, 120);
        assert_eq!(pts(&up.command_parts(&current).unwrap().2), vec![(1.0, 127)]);

        // `to` is the region end: no restore
        let mut end = CcStroke::new(SUSTAIN, false, 0.5, 8.0);
        end.add(7.6, 0);
        let (_, to, ev) = end.command_parts(&current).unwrap();
        assert_eq!(to, 8.0);
        assert_eq!(pts(&ev), vec![(7.5, 0)]);
    }

    #[test]
    fn stroke_changes_nothing_outside_the_range() {
        let current = [cc(0.0, 1, 20), cc(1.0, 1, 40), cc(2.0, 1, 60), cc(3.0, 1, 80), cc(5.0, 1, 100), cc(0.5, 64, 127)];
        let mut draw = CcStroke::new(1, false, 0.25, 8.0);
        draw.add(1.3, 5);
        draw.add(3.4, 120);
        let (from, to) = draw.range().unwrap();
        let after = draw.preview(&current);
        let mut erase = CcStroke::new(1, true, 0.25, 8.0);
        erase.add(1.3, 0);
        erase.add(2.6, 0);
        let (efrom, _) = erase.range().unwrap();
        let erased = erase.preview(&current);
        for k in 0..64 {
            let b = f64::from(k) * 0.125;
            if b < from || b >= to {
                assert_eq!(value_at(&after, 1, b), value_at(&current, 1, b), "draw changed beat {b}");
            }
            if b < efrom {
                assert_eq!(value_at(&erased, 1, b), value_at(&current, 1, b), "erase changed beat {b}");
            }
        }
    }

    #[test]
    fn preview_keeps_other_controllers() {
        let current = [cc(0.0, 64, 127), cc(0.5, 1, 10), cc(1.0, 64, 0), cc(1.5, 1, 50)];
        let mut s = CcStroke::new(1, false, 0.25, 8.0);
        s.add(0.0, 90);
        s.add(2.0, 90);
        let after = s.preview(&current);
        let sustain = |v: &[CcEvent]| v.iter().filter(|e| e.controller == 64).copied().collect::<Vec<_>>();
        assert_eq!(sustain(&after), sustain(&current));
        assert!(after.iter().filter(|e| e.controller == 1).all(|e| e.value == 90 || e.beat >= 2.25));
    }

    fn region_cc(s: &Session, rid: Id) -> Vec<CcEvent> {
        match &s.project.find_region(rid).unwrap().1.content {
            RegionContent::Midi(m) => m.cc.clone(),
            _ => panic!("not a MIDI region"),
        }
    }

    #[test]
    fn stroke_command_matches_the_preview_and_undoes_in_one_step() {
        let mut s = Session::new(false);
        let t = s.execute(Command::AddTrack { kind: TrackKind::Instrument, name: None, patch: None, instrument: None, preset: None, index: None }, Source::Gui).unwrap();
        let tid = t["track_id"].as_u64().unwrap();
        let r = s.execute(Command::CreateMidiRegion { track_id: tid, start: 0.0, length: 8.0, name: None, notes: None }, Source::Gui).unwrap();
        let rid = r["region_id"].as_u64().unwrap();
        let events = vec![cc(0.0, 64, 127), cc(2.0, 64, 0), cc(4.0, 64, 127), cc(6.0, 64, 0), cc(0.5, 1, 10), cc(3.5, 1, 90)];
        s.execute(Command::AddCc { region_id: rid, events }, Source::Gui).unwrap();

        let mut draw = CcStroke::new(SUSTAIN, false, 0.25, 8.0);
        draw.add(2.0, 127);
        draw.add(3.9, 127);
        let mut erase = CcStroke::new(SUSTAIN, true, 0.25, 8.0);
        erase.add(0.0, 0);
        erase.add(1.9, 0);
        for stroke in [draw, erase] {
            let before = region_cc(&s, rid);
            let (from, to, events) = stroke.command_parts(&before).unwrap();
            let c = Command::ReplaceCc { region_id: rid, controller: stroke.controller, from_beat: Some(from), to_beat: Some(to), events };
            s.execute(c, Source::Gui).unwrap();
            assert_eq!(region_cc(&s, rid), stroke.preview(&before));
            assert_ne!(region_cc(&s, rid), before);
            assert_eq!(s.can_undo(), Some("replace_cc"));
            s.execute(Command::Undo {}, Source::Gui).unwrap();
            assert_eq!(region_cc(&s, rid), before);
            s.execute(Command::Redo {}, Source::Gui).unwrap();
        }
    }
}
