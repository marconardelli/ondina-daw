//! Hand-drawn controls of the Ondina look: knob, fader, stereo meter, state buttons, segmented control, effect
//! slots and a small vector icon set. Everything is painted with the egui painter from the tokens in
//! [`crate::theme`], so nothing depends on the glyphs of a system font.
//!
//! The math (value and dB mappings, knob angles, hit tests, peak hold, SVG paths) lives in plain functions that the
//! tests below cover; the widgets only read input and paint.

use std::f32::consts::PI;
use std::sync::OnceLock;

use eframe::egui::{
    self, epaint::PathShape, pos2, vec2, Align, Align2, Color32, FontSelection, Painter, Pos2, Rect, Response,
    RichText, Rounding, Sense, Shape, Stroke, Ui, Vec2, Widget, WidgetText,
};

use crate::theme;

// ===================================================================================== value mappings

/// Position of `v` inside `min..=max` as 0..1 (clamped). An empty range maps to 0.
pub fn value_to_unit(v: f32, min: f32, max: f32) -> f32 {
    if max <= min {
        return 0.0;
    }
    ((v - min) / (max - min)).clamp(0.0, 1.0)
}

/// The value at position `u` (0..1, clamped) of `min..=max`.
pub fn unit_to_value(u: f32, min: f32, max: f32) -> f32 {
    min + u.clamp(0.0, 1.0) * (max - min)
}

/// Pixels of vertical drag that sweep a knob over its whole range (ten times as many with Shift).
pub const KNOB_DRAG_PX: f32 = 200.0;

/// New knob position after a vertical drag of `dy` pixels (screen y grows downwards, so dragging up raises it).
pub fn drag_unit(u: f32, dy: f32, fine: bool) -> f32 {
    let px = if fine { KNOB_DRAG_PX * 10.0 } else { KNOB_DRAG_PX };
    (u - dy / px).clamp(0.0, 1.0)
}

/// Knob angle at the minimum: bottom-left, 135 degrees clockwise from "east" in screen coordinates.
pub const KNOB_START: f32 = 0.75 * PI;
/// The knob turns through 270 degrees, leaving the gap at the bottom.
pub const KNOB_SWEEP: f32 = 1.5 * PI;

/// Screen angle (radians, y down) of the knob pointer at position `u`.
pub fn knob_angle(u: f32) -> f32 {
    KNOB_START + u.clamp(0.0, 1.0) * KNOB_SWEEP
}

/// Whether a parameter is better turned on a logarithmic scale: frequencies, and times that span more than two
/// decades (attack 1 ms..5 s).
pub fn param_is_log(unit: &str, min: f32, max: f32) -> bool {
    unit == "Hz" || ((unit == "s" || unit == "ms") && max / min.max(1e-4) > 100.0)
}

/// The bottom of the log curve: the minimum, or a thousandth of the maximum when the range starts at 0 (Glide
/// 0..1 s, Hold 0..500 ms), so the curve has somewhere to start.
pub fn log_floor(min: f32, max: f32) -> f32 {
    if min > 0.0 {
        min
    } else {
        (max * 1e-3).max(1e-6)
    }
}

/// Position of `v` inside `min..=max` on a log scale, as 0..1 (clamped). An empty range maps to 0.
pub fn value_to_unit_log(v: f32, min: f32, max: f32) -> f32 {
    let lo = log_floor(min, max);
    if max <= min || max <= lo || v <= min || v <= lo {
        return 0.0;
    }
    ((v / lo).ln() / (max / lo).ln()).clamp(0.0, 1.0)
}

/// The value at position `u` (0..1, clamped) of `min..=max` on a log scale; 0 gives the minimum exactly.
pub fn unit_to_value_log(u: f32, min: f32, max: f32) -> f32 {
    let lo = log_floor(min, max);
    if max <= min || max <= lo || u <= 0.0 {
        return min;
    }
    (lo * (max / lo).powf(u.min(1.0))).clamp(min, max)
}

/// A number with a sign when it is not zero once rounded to `decimals`.
fn signed(v: f32, decimals: usize) -> String {
    let s = format!("{:.*}", decimals, v.abs());
    if s.chars().all(|c| c == '0' || c == '.') {
        s
    } else if v < 0.0 {
        format!("-{s}")
    } else {
        format!("+{s}")
    }
}

/// How a parameter value reads with its unit: "2.40 kHz", "-6.0 dB", "250 ms", "4.0:1", "+7 ct". The minus is the
/// ASCII hyphen so the text can be typed back ([`parse_param_value`]).
pub fn format_param_value(v: f32, unit: &str) -> String {
    let ms = |m: f32| if m.abs() >= 100.0 { format!("{m:.0} ms") } else { format!("{m:.1} ms") };
    match unit {
        "Hz" if v.abs() >= 10000.0 => format!("{:.1} kHz", v / 1000.0),
        "Hz" if v.abs() >= 1000.0 => format!("{:.2} kHz", v / 1000.0),
        "Hz" if v.abs() >= 100.0 => format!("{v:.0} Hz"),
        "Hz" => format!("{v:.1} Hz"),
        "dB" => format!("{} dB", signed(v, 1)),
        "ms" => ms(v),
        "s" if v.abs() < 1.0 => ms(v * 1000.0),
        "s" => format!("{v:.2} s"),
        ":1" => format!("{v:.1}:1"),
        "st" | "ct" if (v - v.round()).abs() < 1e-3 => format!("{} {unit}", signed(v.round(), 0)),
        "st" | "ct" => format!("{} {unit}", signed(v, 1)),
        "x" => format!("{v:.2}x"),
        "bit" => format!("{v:.0} bit"),
        "beats" => format!("{v:.2} beats"),
        "" => format!("{v:.2}"),
        _ => format!("{v:.2} {unit}"),
    }
}

/// A value typed for a parameter in `unit`: "2.4k", "2.4 kHz", "-6", "250ms", "0,5 s". A unit after the number is
/// optional; "k" multiplies by a thousand and seconds and milliseconds convert into the parameter's unit. None when
/// the text is not a finite number. The caller clamps it to the range.
pub fn parse_param_value(text: &str, unit: &str) -> Option<f32> {
    let lower = text.trim().to_lowercase().replace(',', ".").replace('\u{2212}', "-");
    let mut t = lower.trim();
    let mut scale = 1.0f32;
    // longer suffixes first: "beats" and "ms" before "s", "khz" before "hz"
    for suffix in ["beats", "khz", "hz", "ms", "db", "st", "ct", "bit", ":1", "x", "s"] {
        if let Some(rest) = t.strip_suffix(suffix) {
            match suffix {
                "khz" => scale = 1000.0,
                "ms" if unit == "s" => scale = 0.001,
                "s" if unit == "ms" => scale = 1000.0,
                _ => {}
            }
            t = rest.trim_end();
            break;
        }
    }
    if let Some(rest) = t.strip_suffix('k') {
        scale *= 1000.0;
        t = rest.trim_end();
    }
    let v = t.parse::<f32>().ok()? * scale;
    v.is_finite().then_some(v)
}

// ===================================================================================== fader and meter scale

/// The fader scale marks, top to bottom, evenly spaced along the travel (the mixer draws them as labels).
pub const FADER_MARKS: [f32; 7] = [6.0, 0.0, -6.0, -12.0, -24.0, -48.0, f32::NEG_INFINITY];
pub const FADER_LABELS: [&str; 7] = ["+6", "0", "\u{2212}6", "\u{2212}12", "\u{2212}24", "\u{2212}48", "\u{2212}\u{221E}"];
const SEGMENTS: f32 = (FADER_MARKS.len() - 1) as f32;

/// Fader position of a level in dB: 0 at the top (+6 dB), 1 at the bottom (silence). Linear in dB between the
/// marks; below -48 dB the last segment is linear in amplitude so it reaches the bottom at -inf.
pub fn db_to_fader(db: f32) -> f32 {
    if db.is_nan() || db <= f32::NEG_INFINITY {
        return 1.0;
    }
    if db >= FADER_MARKS[0] {
        return 0.0;
    }
    let floor = FADER_MARKS[5];
    if db <= floor {
        let g = 10f32.powf((db - floor) / 20.0);
        return 1.0 - g / SEGMENTS;
    }
    for i in 0..5 {
        let (hi, lo) = (FADER_MARKS[i], FADER_MARKS[i + 1]);
        if db > lo || i == 4 {
            let t = (hi - db) / (hi - lo);
            return (i as f32 + t) / SEGMENTS;
        }
    }
    1.0
}

/// Inverse of [`db_to_fader`].
pub fn fader_to_db(pos: f32) -> f32 {
    if pos <= 0.0 {
        return FADER_MARKS[0];
    }
    if pos >= 1.0 {
        return f32::NEG_INFINITY;
    }
    let x = pos * SEGMENTS;
    let i = x.floor() as usize;
    if i >= 5 {
        let g = (1.0 - pos) * SEGMENTS;
        return FADER_MARKS[5] + 20.0 * g.log10();
    }
    let t = x - i as f32;
    FADER_MARKS[i] + t * (FADER_MARKS[i + 1] - FADER_MARKS[i])
}

pub fn db_to_gain(db: f32) -> f32 {
    if db <= f32::NEG_INFINITY {
        0.0
    } else {
        10f32.powf(db / 20.0)
    }
}

pub fn gain_to_db(gain: f32) -> f32 {
    if gain <= 0.0 {
        f32::NEG_INFINITY
    } else {
        20.0 * gain.log10()
    }
}

/// Lit fraction of a meter bar (0 empty, 1 full), on the same scale as the fader so the two line up.
pub fn meter_fill(db: f32) -> f32 {
    1.0 - db_to_fader(db)
}

/// Whether a peak-hold marker at `db` is drawn: not when it has fallen to the bottom of the bar, where it would only
/// sit on the floor of a silent meter.
pub fn hold_marker_shown(db: f32) -> bool {
    meter_fill(db) > 0.01
}

/// How long the peak marker stays put before it falls.
pub const PEAK_HOLD_SECS: f32 = 1.5;
/// How fast the peak marker falls after the hold time.
pub const PEAK_DECAY_DB_PER_SEC: f32 = 20.0;

/// Peak-hold marker and clip latch of one meter channel.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct PeakHold {
    /// Level of the marker, dB.
    pub db: f32,
    /// Seconds since the marker was last pushed up.
    age: f32,
    /// A sample went over 0 dB since the last [`PeakHold::reset_clip`].
    pub clipped: bool,
}

impl Default for PeakHold {
    fn default() -> Self {
        Self {
            db: f32::NEG_INFINITY,
            age: 0.0,
            clipped: false,
        }
    }
}

impl PeakHold {
    /// Feed the current peak (dB) after `dt` seconds.
    pub fn update(&mut self, peak_db: f32, dt: f32) {
        if peak_db >= self.db {
            self.db = peak_db;
            self.age = 0.0;
        } else {
            let before = self.age;
            self.age += dt;
            let falling = (self.age - PEAK_HOLD_SECS.max(before)).max(0.0);
            if falling > 0.0 {
                self.db = (self.db - PEAK_DECAY_DB_PER_SEC * falling).max(peak_db);
            }
        }
        if peak_db > 0.0 {
            self.clipped = true;
        }
    }

    pub fn reset_clip(&mut self) {
        self.clipped = false;
    }
}

/// Meter color at height `t` (0 bottom, 1 top): dark green, green, then yellow and red near the top.
pub fn meter_color(t: f32) -> Color32 {
    // stops from the design: red 0%, yellow 10%, green 35%, dark green 100% (measured from the top)
    const STOPS: [(f32, Color32); 4] = [
        (0.0, theme::METER_LOW),
        (0.65, theme::PLAY),
        (0.90, theme::SOLO),
        (1.0, theme::REC),
    ];
    let t = t.clamp(0.0, 1.0);
    for w in STOPS.windows(2) {
        let ((a, ca), (b, cb)) = (w[0], w[1]);
        if t <= b {
            return lerp_color(ca, cb, (t - a) / (b - a));
        }
    }
    theme::REC
}

fn lerp_color(a: Color32, b: Color32, t: f32) -> Color32 {
    let l = |x: u8, y: u8| (x as f32 + (y as f32 - x as f32) * t).round() as u8;
    Color32::from_rgb(l(a.r(), b.r()), l(a.g(), b.g()), l(a.b(), b.b()))
}

/// Index of the segment under `x`, given the `n + 1` segment edges from left to right.
pub fn segment_at(x: f32, edges: &[f32]) -> Option<usize> {
    edges.windows(2).position(|w| x >= w[0] && x < w[1])
}

// ===================================================================================== SVG path subset

/// One stroke of an icon: a polyline, closed or open.
#[derive(Debug, Clone, PartialEq)]
pub struct SubPath {
    pub points: Vec<Pos2>,
    pub closed: bool,
}

struct Lexer<'a> {
    s: &'a [u8],
    i: usize,
}

impl Lexer<'_> {
    fn skip(&mut self) {
        while self.i < self.s.len() && (self.s[self.i].is_ascii_whitespace() || self.s[self.i] == b',') {
            self.i += 1;
        }
    }

    fn command(&mut self) -> Option<u8> {
        self.skip();
        let c = *self.s.get(self.i)?;
        (c.is_ascii_alphabetic()).then(|| {
            self.i += 1;
            c
        })
    }

    fn at_number(&mut self) -> bool {
        self.skip();
        matches!(self.s.get(self.i), Some(c) if c.is_ascii_digit() || matches!(c, b'-' | b'+' | b'.'))
    }

    fn number(&mut self) -> Option<f32> {
        self.skip();
        let start = self.i;
        let digits = |l: &mut Self| {
            while l.i < l.s.len() && l.s[l.i].is_ascii_digit() {
                l.i += 1;
            }
        };
        if matches!(self.s.get(self.i), Some(b'-' | b'+')) {
            self.i += 1;
        }
        digits(self);
        if self.s.get(self.i) == Some(&b'.') {
            self.i += 1;
            digits(self);
        }
        std::str::from_utf8(&self.s[start..self.i]).ok()?.parse().ok()
    }

    fn pair(&mut self) -> Option<Vec2> {
        Some(vec2(self.number()?, self.number()?))
    }
}

const CURVE_STEPS: usize = 10;

fn cubic(p0: Pos2, c1: Pos2, c2: Pos2, p1: Pos2, out: &mut Vec<Pos2>) {
    for k in 1..=CURVE_STEPS {
        let t = k as f32 / CURVE_STEPS as f32;
        let u = 1.0 - t;
        let w = [u * u * u, 3.0 * u * u * t, 3.0 * u * t * t, t * t * t];
        out.push(pos2(
            w[0] * p0.x + w[1] * c1.x + w[2] * c2.x + w[3] * p1.x,
            w[0] * p0.y + w[1] * c1.y + w[2] * c2.y + w[3] * p1.y,
        ));
    }
}

/// SVG elliptical arc from `p0` to `p1` (endpoint form, SVG spec F.6.5), flattened; `p0` itself is not pushed.
/// `radii` are (rx, ry); `flags` are (large-arc, sweep).
fn arc(p0: Pos2, radii: Vec2, phi_deg: f32, flags: (bool, bool), p1: Pos2, out: &mut Vec<Pos2>) {
    let (large, sweep) = flags;
    let (mut rx, mut ry) = (radii.x.abs(), radii.y.abs());
    if rx == 0.0 || ry == 0.0 || p0 == p1 {
        out.push(p1);
        return;
    }
    let phi = phi_deg.to_radians();
    let (sin, cos) = phi.sin_cos();
    let dx = (p0.x - p1.x) / 2.0;
    let dy = (p0.y - p1.y) / 2.0;
    let x1 = cos * dx + sin * dy;
    let y1 = -sin * dx + cos * dy;
    let lambda = (x1 * x1) / (rx * rx) + (y1 * y1) / (ry * ry);
    if lambda > 1.0 {
        let s = lambda.sqrt();
        rx *= s;
        ry *= s;
    }
    let num = rx * rx * ry * ry - rx * rx * y1 * y1 - ry * ry * x1 * x1;
    let den = rx * rx * y1 * y1 + ry * ry * x1 * x1;
    let mut k = (num / den).max(0.0).sqrt();
    if large == sweep {
        k = -k;
    }
    let cx1 = k * rx * y1 / ry;
    let cy1 = -k * ry * x1 / rx;
    let cx = cos * cx1 - sin * cy1 + (p0.x + p1.x) / 2.0;
    let cy = sin * cx1 + cos * cy1 + (p0.y + p1.y) / 2.0;
    let angle = |ux: f32, uy: f32| uy.atan2(ux);
    let t1 = angle((x1 - cx1) / rx, (y1 - cy1) / ry);
    let t2 = angle((-x1 - cx1) / rx, (-y1 - cy1) / ry);
    let mut dt = t2 - t1;
    if sweep && dt < 0.0 {
        dt += 2.0 * PI;
    } else if !sweep && dt > 0.0 {
        dt -= 2.0 * PI;
    }
    let steps = ((dt.abs() / (PI / 16.0)).ceil() as usize).max(2);
    for s in 1..=steps {
        let t = t1 + dt * s as f32 / steps as f32;
        let (st, ct) = t.sin_cos();
        out.push(pos2(cos * rx * ct - sin * ry * st + cx, sin * rx * ct + cos * ry * st + cy));
    }
}

/// Parse the subset of SVG path data the icons use (M L H V A C S Z, absolute and relative, implicit repeats) into
/// polylines. Malformed input stops the parse; what was read so far is kept.
pub fn parse_path(d: &str) -> Vec<SubPath> {
    let mut lx = Lexer { s: d.as_bytes(), i: 0 };
    let mut out: Vec<SubPath> = Vec::new();
    let mut current: Option<SubPath> = None;
    let mut cur = Pos2::ZERO;
    let mut start = Pos2::ZERO;
    let mut last_ctrl: Option<Pos2> = None;
    let mut cmd = 0u8;
    loop {
        let c = match lx.command() {
            Some(c) => c,
            None if cmd != 0 && lx.at_number() => cmd,
            None => break,
        };
        cmd = c;
        let rel = c.is_ascii_lowercase();
        let base = if rel { cur.to_vec2() } else { Vec2::ZERO };
        let mut ctrl = None;
        let mut pts = Vec::new();
        match c.to_ascii_uppercase() {
            b'M' => {
                let Some(p) = lx.pair() else { break };
                if let Some(sp) = current.take() {
                    out.push(sp);
                }
                cur = (base + p).to_pos2();
                start = cur;
                current = Some(SubPath {
                    points: vec![cur],
                    closed: false,
                });
                // further pairs after a moveto are linetos
                cmd = if rel { b'l' } else { b'L' };
            }
            b'L' => {
                let Some(p) = lx.pair() else { break };
                pts.push((base + p).to_pos2());
            }
            b'H' => {
                let Some(x) = lx.number() else { break };
                pts.push(pos2(if rel { cur.x + x } else { x }, cur.y));
            }
            b'V' => {
                let Some(y) = lx.number() else { break };
                pts.push(pos2(cur.x, if rel { cur.y + y } else { y }));
            }
            b'C' => {
                let (Some(c1), Some(c2), Some(p)) = (lx.pair(), lx.pair(), lx.pair()) else { break };
                let (c1, c2, p) = ((base + c1).to_pos2(), (base + c2).to_pos2(), (base + p).to_pos2());
                cubic(cur, c1, c2, p, &mut pts);
                ctrl = Some(c2);
            }
            b'S' => {
                let (Some(c2), Some(p)) = (lx.pair(), lx.pair()) else { break };
                let c1 = last_ctrl.map_or(cur, |lc| cur + (cur - lc));
                let (c2, p) = ((base + c2).to_pos2(), (base + p).to_pos2());
                cubic(cur, c1, c2, p, &mut pts);
                ctrl = Some(c2);
            }
            b'A' => {
                let (Some(rx), Some(ry), Some(rot), Some(la), Some(sw), Some(p)) =
                    (lx.number(), lx.number(), lx.number(), lx.number(), lx.number(), lx.pair())
                else {
                    break;
                };
                arc(cur, vec2(rx, ry), rot, (la != 0.0, sw != 0.0), (base + p).to_pos2(), &mut pts);
            }
            b'Z' => {
                if let Some(mut sp) = current.take() {
                    sp.closed = true;
                    out.push(sp);
                }
                cur = start;
                cmd = 0;
            }
            _ => break,
        }
        if let Some(&last) = pts.last() {
            current
                .get_or_insert_with(|| SubPath {
                    points: vec![cur],
                    closed: false,
                })
                .points
                .extend(pts);
            cur = last;
        }
        last_ctrl = ctrl;
    }
    out.extend(current);
    out
}

// ===================================================================================== icons

/// The icons of the set, drawn on a 16-unit grid with a 1.3-unit stroke (the transport ones can also be filled).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Icon {
    Play,
    Stop,
    Cycle,
    Metronome,
    Effect,
    Send,
    Mixer,
    Piano,
    Drums,
    Audio,
    Bus,
    Mic,
    Ai,
    Library,
    Inspector,
    ClipGrid,
    ToStart,
    Rewind,
    Forward,
    Record,
    Close,
    Plus,
    Search,
    Settings,
    Undo,
    Warning,
    Check,
    Movie,
    Scissors,
    More,
    ChevronDown,
    ChevronRight,
    ChevronLeft,
}

impl Icon {
    pub const ALL: [Icon; 33] = [
        Icon::Play,
        Icon::Stop,
        Icon::Cycle,
        Icon::Metronome,
        Icon::Effect,
        Icon::Send,
        Icon::Mixer,
        Icon::Piano,
        Icon::Drums,
        Icon::Audio,
        Icon::Bus,
        Icon::Mic,
        Icon::Ai,
        Icon::Library,
        Icon::Inspector,
        Icon::ClipGrid,
        Icon::ToStart,
        Icon::Rewind,
        Icon::Forward,
        Icon::Record,
        Icon::Close,
        Icon::Plus,
        Icon::Search,
        Icon::Settings,
        Icon::Undo,
        Icon::Warning,
        Icon::Check,
        Icon::Movie,
        Icon::Scissors,
        Icon::More,
        Icon::ChevronDown,
        Icon::ChevronRight,
        Icon::ChevronLeft,
    ];

    /// SVG path data on the 16 grid (from the design tokens).
    pub fn path(self) -> &'static str {
        match self {
            Icon::Play => "M4 2.5L13 8 4 13.5z",
            Icon::Stop => "M3.5 3.5h9v9h-9z",
            Icon::Cycle => "M3 10V7a3 3 0 0 1 3-3h7M11 2l2 2-2 2M13 6v3a3 3 0 0 1-3 3H3M5 14l-2-2 2-2",
            Icon::Metronome => "M6 1.5h4l3 13H3zM8 10l4-7",
            Icon::Effect => "M3 13V9M3 6V3M8 13V7M8 4V3M13 13v-2M13 8V3M1.5 7.5h3M6.5 5.5h3M11.5 9.5h3",
            Icon::Send => "M2 8h8M7 4.5L10.5 8 7 11.5M13.5 3v10",
            Icon::Mixer => "M4 2v12M8 2v12M12 2v12M2.5 10h3M6.5 5h3M10.5 8h3",
            Icon::Piano => "M2 2.5h12v11H2zM6 2.5v6M10 2.5v6M5 8.5h2M9 8.5h2",
            Icon::Drums => {
                "M2 6.5c0-1.4 2.7-2.5 6-2.5s6 1.1 6 2.5S11.3 9 8 9 2 7.9 2 6.5zM2 6.5v4c0 1.4 2.7 2.5 6 2.5s6-1.1 6-2.5v-4"
            }
            Icon::Audio => "M1.5 8h1M4 5v6M6.5 3v10M9 6v4M11.5 4v8M14 7v2",
            Icon::Bus => "M2 4h5l2 4-2 4H2M9 8h5",
            Icon::Mic => "M6 2.5h4v6a2 2 0 0 1-4 0zM3.5 8a4.5 4.5 0 0 0 9 0M8 12.5V15",
            Icon::Ai => "M8 1.5l1.6 4 4 1.5-4 1.5L8 12.5 6.4 8.5l-4-1.5 4-1.5z",
            Icon::Library => "M2 2.5h3v11H2zM6.5 2.5h3v11h-3zM11 3.2l2.6-.7 2 10.6-2.7.6z",
            Icon::Inspector => "M1.5 2.5h13v11h-13zM6 2.5v11",
            Icon::ClipGrid => "M2 2h5v5H2zM9 2h5v5H9zM2 9h5v5H2zM9 9h5v5H9z",
            Icon::ToStart => "M3.5 3v10M13 3.5L6 8l7 4.5z",
            Icon::Rewind => "M8 3.5L2 8l6 4.5zM14.5 3.5L8.5 8l6 4.5z",
            Icon::Forward => "M1.5 3.5L7.5 8l-6 4.5zM8 3.5L14 8l-6 4.5z",
            Icon::Record => "M8 3a5 5 0 1 1 0 10a5 5 0 1 1 0-10z",
            Icon::Close => "M4 4l8 8M12 4l-8 8",
            Icon::Plus => "M8 3v10M3 8h10",
            Icon::Search => "M7 2.5a4.5 4.5 0 1 1 0 9a4.5 4.5 0 1 1 0-9zM10.3 10.3l3.7 3.7",
            Icon::Settings => "M8 5.5a2.5 2.5 0 1 1 0 5a2.5 2.5 0 1 1 0-5zM8 1.5v2.2M8 12.3v2.2M1.5 8h2.2M12.3 8h2.2M3.4 3.4l1.6 1.6M11 11l1.6 1.6M3.4 12.6L5 11M11 5l1.6-1.6",
            Icon::Undo => "M5.5 3L2.5 6l3 3M2.5 6H10a3.5 3.5 0 0 1 0 7H6",
            Icon::Warning => "M8 2l6.5 11.5h-13zM8 6.5v3.2M8 11.4v0.8",
            Icon::Check => "M3 8.5l3 3 7-7",
            Icon::Movie => "M2 3.5h12v9H2zM5 3.5v9M11 3.5v9M2 6.5h3M2 9.5h3M11 6.5h3M11 9.5h3",
            Icon::Scissors => "M4.5 9.5a2 2 0 1 1 0 4a2 2 0 1 1 0-4zM11.5 9.5a2 2 0 1 1 0 4a2 2 0 1 1 0-4zM5.7 9.9L12 2.5M10.3 9.9L4 2.5",
            Icon::More => "M3.5 7.2a0.8 0.8 0 1 1 0 1.6a0.8 0.8 0 1 1 0-1.6zM8 7.2a0.8 0.8 0 1 1 0 1.6a0.8 0.8 0 1 1 0-1.6zM12.5 7.2a0.8 0.8 0 1 1 0 1.6a0.8 0.8 0 1 1 0-1.6z",
            Icon::ChevronDown => "M4 6l4 4 4-4",
            Icon::ChevronRight => "M6 4l4 4-4 4",
            Icon::ChevronLeft => "M10 4L6 8l4 4",
        }
    }

    /// The parsed strokes, computed once.
    pub fn strokes(self) -> &'static [SubPath] {
        static CACHE: OnceLock<Vec<Vec<SubPath>>> = OnceLock::new();
        let all = CACHE.get_or_init(|| Icon::ALL.iter().map(|i| parse_path(i.path())).collect());
        &all[self as usize]
    }
}

/// Grid size of the icon artwork.
pub const ICON_GRID: f32 = 16.0;
/// Stroke width on the 16 grid.
pub const ICON_STROKE: f32 = 1.3;

/// Draw `icon` scaled into `rect` (kept square and centered).
pub fn paint_icon(painter: &Painter, icon: Icon, rect: Rect, color: Color32) {
    let side = rect.width().min(rect.height());
    let scale = side / ICON_GRID;
    let origin = rect.center() - Vec2::splat(side / 2.0);
    let stroke = Stroke::new(ICON_STROKE * scale, color);
    for sp in icon.strokes() {
        let points: Vec<Pos2> = sp.points.iter().map(|p| origin + p.to_vec2() * scale).collect();
        let shape = if sp.closed {
            PathShape::closed_line(points, stroke)
        } else {
            PathShape::line(points, stroke)
        };
        painter.add(shape);
    }
}

/// Draw `icon` with its closed shapes filled (the transport glyphs); open strokes stay lines.
pub fn paint_icon_filled(painter: &Painter, icon: Icon, rect: Rect, color: Color32) {
    let side = rect.width().min(rect.height());
    let scale = side / ICON_GRID;
    let origin = rect.center() - Vec2::splat(side / 2.0);
    let stroke = Stroke::new(ICON_STROKE * scale, color);
    for sp in icon.strokes() {
        let points: Vec<Pos2> = sp.points.iter().map(|p| origin + p.to_vec2() * scale).collect();
        let shape = if sp.closed {
            PathShape::convex_polygon(points, color, Stroke::new(0.5 * scale, color))
        } else {
            PathShape::line(points, stroke)
        };
        painter.add(shape);
    }
}

/// The two waves of the Ondina mark, on the 16 grid.
const LOGO_WAVES: [&str; 2] = ["M1 10c2-3 4-3 6 0s4 3 6 0", "M1 6c2-3 4-3 6 0s4 3 6 0"];

/// The Ondina mark: two accent waves, the upper one half transparent.
pub fn paint_logo(painter: &Painter, rect: Rect) {
    static WAVES: OnceLock<Vec<Vec<SubPath>>> = OnceLock::new();
    let waves = WAVES.get_or_init(|| LOGO_WAVES.iter().map(|d| parse_path(d)).collect());
    let side = rect.width().min(rect.height());
    let scale = side / ICON_GRID;
    let origin = rect.center() - Vec2::splat(side / 2.0);
    for (wave, alpha) in waves.iter().zip([1.0_f32, 0.5]) {
        let stroke = Stroke::new(1.8 * scale, theme::ACCENT.gamma_multiply(alpha));
        for sp in wave {
            let points: Vec<Pos2> = sp.points.iter().map(|p| origin + p.to_vec2() * scale).collect();
            painter.add(PathShape::line(points, stroke));
        }
    }
}

/// An icon as a widget of `size` x `size` points.
pub fn icon(ui: &mut Ui, icon: Icon, size: f32, color: Color32) -> Response {
    let (rect, resp) = ui.allocate_exact_size(Vec2::splat(size), Sense::hover());
    if ui.is_rect_visible(rect) {
        paint_icon(ui.painter(), icon, rect, color);
    }
    resp
}

/// Icon size inside text widgets.
const INLINE_ICON: f32 = 14.0;
const INLINE_GAP: f32 = 5.0;

/// `text` laid out with room on its left for an inline icon.
fn with_icon_room(ui: &Ui, text: impl Into<RichText>) -> WidgetText {
    let mut job = egui::text::LayoutJob::default();
    let text: RichText = text.into();
    text.append_to(&mut job, ui.style(), FontSelection::Default, Align::Center);
    if let Some(first) = job.sections.first_mut() {
        first.leading_space = INLINE_ICON + INLINE_GAP;
    }
    WidgetText::LayoutJob(job)
}

fn paint_inline_icon(ui: &Ui, resp: &Response, icon: Icon, color: Color32) {
    let x = resp.rect.left() + ui.spacing().button_padding.x;
    let r = Rect::from_min_size(
        pos2(x, resp.rect.center().y - INLINE_ICON / 2.0),
        Vec2::splat(INLINE_ICON),
    );
    paint_icon(ui.painter(), icon, r, color);
}

/// A selectable label with an icon in front of the text. `tint` colors the icon (otherwise it follows the text).
pub fn icon_selectable(
    ui: &mut Ui,
    selected: bool,
    icon: Icon,
    text: impl Into<RichText>,
    tint: Option<Color32>,
) -> Response {
    let resp = ui.selectable_label(selected, with_icon_room(ui, text));
    let color = tint.unwrap_or_else(|| ui.style().interact_selectable(&resp, selected).text_color());
    paint_inline_icon(ui, &resp, icon, color);
    resp
}

/// A button with an icon in front of the text.
pub fn icon_button(ui: &mut Ui, icon: Icon, text: impl Into<RichText>, tint: Option<Color32>) -> Response {
    let resp = ui.button(with_icon_room(ui, text));
    let color = tint.unwrap_or_else(|| ui.style().interact(&resp).text_color());
    paint_inline_icon(ui, &resp, icon, color);
    resp
}

/// A menu button (or a sub-menu entry inside a menu) with an icon in front of the text.
pub fn icon_menu_button<R>(
    ui: &mut Ui,
    icon: Icon,
    text: impl Into<RichText>,
    add_contents: impl FnOnce(&mut Ui) -> R,
) -> egui::InnerResponse<Option<R>> {
    let ir = ui.menu_button(with_icon_room(ui, text), add_contents);
    let color = ui.style().interact(&ir.response).text_color();
    paint_inline_icon(ui, &ir.response, icon, color);
    ir
}

/// `text` with room on its left for an inline icon; paint the icon with [`paint_leading_icon`] once the widget is
/// laid out (for widgets the helpers above do not cover, such as a disabled button or a menu entry).
pub fn leading_icon_text(ui: &Ui, text: impl Into<RichText>) -> WidgetText {
    with_icon_room(ui, text)
}

/// The icon of a widget built from [`leading_icon_text`].
pub fn paint_leading_icon(ui: &Ui, resp: &Response, icon: Icon, color: Color32) {
    paint_inline_icon(ui, resp, icon, color);
}

/// Width taken by the chevron of a drop-down label.
pub const CHEVRON_W: f32 = 12.0;

/// `text` with room on its right for the chevron of a drop-down; paint it with [`paint_trailing_chevron`].
pub fn dropdown_text(ui: &Ui, text: impl Into<RichText>) -> WidgetText {
    let mut job = egui::text::LayoutJob::default();
    let text: RichText = text.into();
    text.append_to(&mut job, ui.style(), FontSelection::Default, Align::Center);
    let format = job.sections.first().map(|s| s.format.clone()).unwrap_or_default();
    job.append(" ", CHEVRON_W, format);
    WidgetText::LayoutJob(job)
}

/// The chevron at the right end of a drop-down button built from [`dropdown_text`].
pub fn paint_trailing_chevron(ui: &Ui, resp: &Response, color: Color32) {
    let x = resp.rect.right() - ui.spacing().button_padding.x - CHEVRON_W / 2.0;
    let r = Rect::from_center_size(pos2(x, resp.rect.center().y), Vec2::splat(10.0));
    paint_icon(ui.painter(), Icon::ChevronDown, r, color);
}

/// A small frameless square button showing only `icon` (close, stop, remove), lit on hover.
pub fn icon_only_button(ui: &mut Ui, icon: Icon, side: f32, color: Color32) -> Response {
    let (rect, resp) = ui.allocate_exact_size(Vec2::splat(side), Sense::click());
    if ui.is_rect_visible(rect) {
        let fg = if resp.hovered() {
            ui.painter().rect_filled(rect, Rounding::same(theme::R_SM), theme::HOVER);
            theme::TEXT
        } else {
            color
        };
        paint_icon(ui.painter(), icon, rect.shrink(side * 0.2), fg);
    }
    resp
}

/// A menu button whose label ends with a drop-down chevron.
pub fn dropdown_menu_button<R>(
    ui: &mut Ui,
    text: impl Into<RichText>,
    add_contents: impl FnOnce(&mut Ui) -> R,
) -> egui::InnerResponse<Option<R>> {
    let ir = ui.menu_button(dropdown_text(ui, text), add_contents);
    let color = ui.style().interact(&ir.response).text_color();
    paint_trailing_chevron(ui, &ir.response, color);
    ir
}

/// The "more" button (three dots) that opens a menu; `fill` overrides the resting button fill.
pub fn more_menu_button<R>(
    ui: &mut Ui,
    size: Vec2,
    fill: Option<Color32>,
    add_contents: impl FnOnce(&mut Ui) -> R,
) -> egui::InnerResponse<Option<R>> {
    let mut button = egui::Button::new("").min_size(size);
    if let Some(f) = fill {
        button = button.fill(f).stroke(Stroke::NONE);
    }
    let ir = egui::menu::menu_custom_button(ui, button, add_contents);
    let color = if ir.response.hovered() { theme::TEXT } else { theme::TEXT_2 };
    let side = size.y.min(size.x) * 0.6;
    paint_icon(ui.painter(), Icon::More, Rect::from_center_size(ir.response.rect.center(), Vec2::splat(side)), color);
    ir
}

// ===================================================================================== tool buttons

/// Fill, border and foreground of a tool button.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ToolColors {
    pub fill: Color32,
    pub border: Color32,
    pub fg: Color32,
}

impl ToolColors {
    /// Resting, not switched on.
    pub const OFF: ToolColors = ToolColors { fill: theme::RAISED, border: theme::BORDER, fg: theme::ICON };
    /// Switched on: panels, metronome.
    pub const ACCENT: ToolColors =
        ToolColors { fill: theme::ACCENT_BG, border: theme::ACCENT_BORDER, fg: theme::ACCENT };
    pub const PLAY: ToolColors = ToolColors { fill: theme::PLAY_BG, border: theme::PLAY_BORDER, fg: theme::PLAY };
    pub const REC: ToolColors = ToolColors { fill: theme::REC_BG, border: theme::REC_BORDER, fg: theme::REC };
    pub const CYCLE: ToolColors = ToolColors { fill: theme::CYCLE_BG, border: theme::CYCLE_BORDER, fg: theme::SOLO };
    pub const AI: ToolColors = ToolColors { fill: theme::AI_BG, border: theme::AI_BORDER, fg: theme::AI_TEXT };

    /// The look under the pointer: a resting button lifts to the hover neutrals, a lit one keeps its tint and
    /// brightens its border toward the foreground.
    pub fn hovered(self) -> ToolColors {
        if self == ToolColors::OFF {
            ToolColors { fill: theme::HOVER, border: theme::BORDER_HOVER, fg: theme::TEXT }
        } else {
            ToolColors { border: mix(self.border, self.fg, 0.35), ..self }
        }
    }
}

/// `a` moved toward `b` by `t` (0..1), channel by channel.
pub fn mix(a: Color32, b: Color32, t: f32) -> Color32 {
    let t = t.clamp(0.0, 1.0);
    let ch = |x: u8, y: u8| (x as f32 + (y as f32 - x as f32) * t).round() as u8;
    Color32::from_rgb(ch(a.r(), b.r()), ch(a.g(), b.g()), ch(a.b(), b.b()))
}

/// A fixed-size button with a vector icon (and optional text after it), in the transport-bar style.
pub struct ToolButton {
    icon: Icon,
    size: Vec2,
    icon_size: f32,
    colors: ToolColors,
    /// Icon color when it should differ from the foreground (record stays red while off).
    icon_color: Option<Color32>,
    filled: bool,
    text: Option<String>,
}

impl ToolButton {
    pub fn new(icon: Icon) -> Self {
        Self {
            icon,
            size: vec2(34.0, 30.0),
            icon_size: 14.0,
            colors: ToolColors::OFF,
            icon_color: None,
            filled: false,
            text: None,
        }
    }

    pub fn size(mut self, w: f32, h: f32) -> Self {
        self.size = vec2(w, h);
        self
    }

    pub fn icon_size(mut self, s: f32) -> Self {
        self.icon_size = s;
        self
    }

    /// `Some(colors)` when the button is switched on.
    pub fn lit(mut self, colors: Option<ToolColors>) -> Self {
        self.colors = colors.unwrap_or(ToolColors::OFF);
        self
    }

    pub fn icon_color(mut self, c: Color32) -> Self {
        self.icon_color = Some(c);
        self
    }

    /// Fill the icon's closed shapes (play, stop, record...).
    pub fn filled(mut self) -> Self {
        self.filled = true;
        self
    }

    /// Text after the icon; the button grows to fit it.
    pub fn text(mut self, t: impl Into<String>) -> Self {
        self.text = Some(t.into());
        self
    }
}

impl Widget for ToolButton {
    fn ui(self, ui: &mut Ui) -> Response {
        let font = theme::body_font();
        let galley = self.text.map(|t| ui.painter().layout_no_wrap(t, font, theme::TEXT));
        let gap = theme::SP_3;
        let pad = 10.0;
        let size = match &galley {
            Some(g) => vec2(pad + self.icon_size + gap + g.size().x + pad, self.size.y),
            None => self.size,
        };
        let (rect, resp) = ui.allocate_exact_size(size, Sense::click());
        if ui.is_rect_visible(rect) {
            let c = if resp.hovered() { self.colors.hovered() } else { self.colors };
            let p = ui.painter();
            p.rect(rect, Rounding::same(theme::R_LG), c.fill, Stroke::new(1.0_f32, c.border));
            let icon_color = self.icon_color.unwrap_or(c.fg);
            let icon_rect = match &galley {
                Some(_) => Rect::from_center_size(
                    pos2(rect.left() + pad + self.icon_size / 2.0, rect.center().y),
                    Vec2::splat(self.icon_size),
                ),
                None => Rect::from_center_size(rect.center(), Vec2::splat(self.icon_size)),
            };
            if self.filled {
                paint_icon_filled(p, self.icon, icon_rect, icon_color);
            } else {
                paint_icon(p, self.icon, icon_rect, icon_color);
            }
            if let Some(g) = galley {
                let at = pos2(icon_rect.right() + gap, rect.center().y - g.size().y / 2.0);
                p.galley(at, g, c.fg);
            }
        }
        resp
    }
}

// ===================================================================================== knob

/// Rotary knob: arc and pointer, vertical drag (Shift for fine steps), double-click to reset, value text below.
/// With a unit it shows the value with its unit (lit while dragging) and in a tooltip; on a log scale it turns
/// like the ear hears; when editable a right-click opens a field to type the value (Enter sets it, Esc cancels).
pub struct Knob<'a> {
    value: &'a mut f32,
    min: f32,
    max: f32,
    default: f32,
    bipolar: bool,
    diameter: f32,
    text: Option<String>,
    log: bool,
    unit: Option<&'a str>,
    label: Option<String>,
    note: Option<String>,
    editable: bool,
    width: Option<f32>,
}

/// The text being typed into a knob, kept in egui's memory between frames.
#[derive(Debug, Clone, PartialEq)]
struct KnobEdit {
    text: String,
    /// Opened this frame: focus the field and select its text.
    fresh: bool,
}

fn knob_edit_id(knob: egui::Id) -> egui::Id {
    knob.with("knob-edit")
}

impl<'a> Knob<'a> {
    pub fn new(value: &'a mut f32, range: std::ops::RangeInclusive<f32>) -> Self {
        let (min, max) = (*range.start(), *range.end());
        Self {
            value,
            min,
            max,
            default: min,
            bipolar: false,
            diameter: 34.0,
            text: None,
            log: false,
            unit: None,
            label: None,
            note: None,
            editable: false,
            width: None,
        }
    }

    /// Value restored by a double-click.
    pub fn default_value(mut self, v: f32) -> Self {
        self.default = v;
        self
    }

    /// Light the arc from the top center (pan, detune) instead of from the minimum.
    pub fn bipolar(mut self, on: bool) -> Self {
        self.bipolar = on;
        self
    }

    pub fn diameter(mut self, d: f32) -> Self {
        self.diameter = d;
        self
    }

    /// Text shown under the knob (already formatted, e.g. "2.4 kHz").
    pub fn text(mut self, t: impl Into<String>) -> Self {
        self.text = Some(t.into());
        self
    }

    /// Turn on a logarithmic scale (frequencies, long times): see [`param_is_log`].
    pub fn log(mut self, on: bool) -> Self {
        self.log = on;
        self
    }

    /// Show the value under the knob and in its tooltip with this unit ("Hz", "dB", "" for a plain number).
    pub fn unit(mut self, unit: &'a str) -> Self {
        self.unit = Some(unit);
        self
    }

    /// A caption above the knob (the parameter's name).
    pub fn label(mut self, label: impl Into<String>) -> Self {
        self.label = Some(label.into());
        self
    }

    /// A line added to the tooltip, under the value.
    pub fn note(mut self, note: impl Into<String>) -> Self {
        self.note = Some(note.into());
        self
    }

    /// A right-click opens a field to type the value.
    pub fn editable(mut self, on: bool) -> Self {
        self.editable = on;
        self
    }

    /// Width of the cell the knob sits in (at least its diameter), so the label and the value have room.
    pub fn width(mut self, w: f32) -> Self {
        self.width = Some(w);
        self
    }

    fn to_unit(&self, v: f32) -> f32 {
        if self.log {
            value_to_unit_log(v, self.min, self.max)
        } else {
            value_to_unit(v, self.min, self.max)
        }
    }

    fn to_value(&self, u: f32) -> f32 {
        if self.log {
            unit_to_value_log(u, self.min, self.max)
        } else {
            unit_to_value(u, self.min, self.max)
        }
    }
}

/// A knob's tooltip: its name and value, an optional note, then the gestures.
pub fn knob_tooltip(label: Option<&str>, value: &str, note: Option<&str>, editable: bool) -> String {
    let head = match label {
        Some(l) => format!("{l}: {value}"),
        None => value.to_string(),
    };
    let note = note.map(|n| format!("\n{n}")).unwrap_or_default();
    let typing = if editable { " · Right-click: type a value" } else { "" };
    format!("{head}{note}\nDrag up or down · Shift: fine · Double-click: default{typing}")
}

fn arc_points(c: Pos2, r: f32, a0: f32, a1: f32) -> Vec<Pos2> {
    let steps = (((a1 - a0).abs() / (PI / 24.0)).ceil() as usize).max(1);
    (0..=steps)
        .map(|s| {
            let a = a0 + (a1 - a0) * s as f32 / steps as f32;
            c + vec2(a.cos(), a.sin()) * r
        })
        .collect()
}

impl Widget for Knob<'_> {
    fn ui(self, ui: &mut Ui) -> Response {
        let d = self.diameter;
        let w = self.width.unwrap_or(d).max(d);
        let label_h = if self.label.is_some() { theme::SIZE_LABEL + 4.0 } else { 0.0 };
        let text_h = if self.text.is_some() || self.unit.is_some() { theme::SIZE_NUMBER + 4.0 } else { 0.0 };
        let (rect, mut resp) = ui.allocate_exact_size(vec2(w, label_h + d + text_h), Sense::click_and_drag());
        let mut u = self.to_unit(*self.value);
        if resp.double_clicked() {
            *self.value = self.default;
            u = self.to_unit(self.default);
            resp.mark_changed();
        } else if resp.dragged() {
            let fine = ui.input(|i| i.modifiers.shift);
            let nu = drag_unit(u, resp.drag_delta().y, fine);
            if nu != u {
                u = nu;
                *self.value = self.to_value(u);
                resp.mark_changed();
            }
        }

        // typing a value: a right-click opens the field over the value row
        let unit = self.unit.unwrap_or("");
        let edit_id = knob_edit_id(resp.id);
        if self.editable && resp.secondary_clicked() {
            let text = format_param_value(*self.value, unit);
            ui.data_mut(|m| m.insert_temp(edit_id, KnobEdit { text, fresh: true }));
        }
        let editing = if self.editable { ui.data(|m| m.get_temp::<KnobEdit>(edit_id)) } else { None };
        let value_row = Rect::from_min_size(
            pos2(rect.left(), rect.bottom() - text_h.max(theme::SIZE_NUMBER + 6.0)),
            vec2(w, text_h.max(theme::SIZE_NUMBER + 6.0)),
        );
        if let Some(mut edit) = editing.clone() {
            // a child at the value row, so the field does not move the parent's layout cursor
            let mut child = ui.new_child(egui::UiBuilder::new().max_rect(value_row));
            let field_id = resp.id.with("knob-field");
            let field = child.add(
                egui::TextEdit::singleline(&mut edit.text)
                    .id(field_id)
                    .font(theme::number_font())
                    .horizontal_align(Align::Center)
                    .margin(vec2(2.0, 1.0))
                    .desired_width(w - 4.0),
            );
            if edit.fresh {
                field.request_focus();
                // select the whole value so typing replaces it
                if let Some(mut st) = egui::TextEdit::load_state(ui.ctx(), field_id) {
                    let n = edit.text.chars().count();
                    st.cursor.set_char_range(Some(egui::text::CCursorRange::two(
                        egui::text::CCursor::new(0),
                        egui::text::CCursor::new(n),
                    )));
                    st.store(ui.ctx(), field_id);
                }
                edit.fresh = false;
                ui.data_mut(|m| m.insert_temp(edit_id, edit));
            } else if field.lost_focus() || !field.has_focus() {
                // egui drops the focus on Esc before the field sees it: check the keys to tell cancel from commit
                let (esc, enter) = ui.input(|i| (i.key_pressed(egui::Key::Escape), i.key_pressed(egui::Key::Enter)));
                if !esc && enter {
                    if let Some(x) = parse_param_value(&edit.text, unit) {
                        *self.value = x.clamp(self.min.min(self.max), self.max.max(self.min));
                        u = self.to_unit(*self.value);
                        resp.mark_changed();
                    }
                }
                ui.data_mut(|m| m.remove::<KnobEdit>(edit_id));
            } else {
                ui.data_mut(|m| m.insert_temp(edit_id, edit));
            }
        }

        if ui.is_rect_visible(rect) {
            let p = ui.painter();
            if let Some(label) = &self.label {
                let mut job = egui::text::LayoutJob::simple_singleline(
                    label.clone(),
                    egui::FontId::proportional(theme::SIZE_LABEL),
                    theme::TEXT_2,
                );
                job.wrap = egui::text::TextWrapping::truncate_at_width(w);
                let g = ui.fonts(|f| f.layout_job(job));
                p.galley(pos2(rect.center().x - g.size().x / 2.0, rect.top()), g, theme::TEXT_2);
            }
            let s = d / 40.0; // the artwork is drawn on a 40 grid
            let c = pos2(rect.center().x, rect.top() + label_h + d / 2.0);
            let r = 15.0 * s;
            let body = if resp.hovered() || resp.dragged() { theme::HOVER } else { theme::BORDER };
            p.circle(c, r, body, Stroke::new(1.0_f32, theme::SEPARATOR));
            let ring = Stroke::new(3.0 * s, theme::TRACK_OFF);
            p.add(PathShape::line(arc_points(c, r, KNOB_START, KNOB_START + KNOB_SWEEP), ring));
            let a = knob_angle(u);
            let from = if self.bipolar { knob_angle(0.5) } else { KNOB_START };
            if (a - from).abs() > 0.01 {
                let lit = Stroke::new(3.0 * s, theme::ACCENT);
                p.add(PathShape::line(arc_points(c, r, from.min(a), from.max(a)), lit));
            }
            p.line_segment([c, c + vec2(a.cos(), a.sin()) * 11.0 * s], Stroke::new(2.0 * s, theme::TEXT));
            let shown = match (&self.text, self.unit) {
                (Some(t), _) => Some(t.clone()),
                (None, Some(unit)) => Some(format_param_value(*self.value, unit)),
                (None, None) => None,
            };
            let typing = self.editable && ui.data(|m| m.get_temp::<KnobEdit>(edit_id)).is_some();
            if let (Some(t), false) = (shown, typing) {
                let color = if resp.dragged() { theme::ACCENT_TEXT } else { theme::TEXT };
                p.text(pos2(rect.center().x, rect.bottom() - text_h / 2.0), Align2::CENTER_CENTER, t, theme::number_font(), color);
            }
        }
        match self.unit {
            Some(unit) => {
                let value = format_param_value(*self.value, unit);
                resp.on_hover_text(knob_tooltip(self.label.as_deref(), &value, self.note.as_deref(), self.editable))
            }
            None => resp,
        }
    }
}

// ===================================================================================== fader

/// Vertical volume fader in dB (+6 to -inf): drag the cap (Shift for fine steps), double-click for 0 dB, with an
/// optional dB scale on its left.
pub struct Fader<'a> {
    db: &'a mut f32,
    height: f32,
    scale: bool,
}

impl<'a> Fader<'a> {
    pub fn new(db: &'a mut f32) -> Self {
        Self {
            db,
            height: 140.0,
            scale: true,
        }
    }

    pub fn height(mut self, h: f32) -> Self {
        self.height = h;
        self
    }

    pub fn scale(mut self, on: bool) -> Self {
        self.scale = on;
        self
    }
}

const CAP: Vec2 = vec2(26.0, 22.0);
const SCALE_W: f32 = 22.0;

/// Cap center y for a fader position, with the cap kept inside `top..top + height`.
pub fn fader_cap_y(pos: f32, top: f32, height: f32) -> f32 {
    let travel = (height - CAP.y).max(1.0);
    top + CAP.y / 2.0 + pos.clamp(0.0, 1.0) * travel
}

/// Whether `p` falls on the cap of a fader whose track is centered at `track_x`.
pub fn fader_cap_hit(p: Pos2, track_x: f32, pos: f32, top: f32, height: f32) -> bool {
    let c = pos2(track_x, fader_cap_y(pos, top, height));
    Rect::from_center_size(c, CAP).contains(p)
}

impl Widget for Fader<'_> {
    fn ui(self, ui: &mut Ui) -> Response {
        let w = CAP.x + if self.scale { SCALE_W + 4.0 } else { 0.0 };
        let (rect, mut resp) = ui.allocate_exact_size(vec2(w, self.height), Sense::click_and_drag());
        let track_x = rect.right() - CAP.x / 2.0;
        let mut pos = db_to_fader(*self.db);
        if resp.double_clicked() {
            *self.db = 0.0;
            pos = db_to_fader(0.0);
            resp.mark_changed();
        } else if resp.dragged() {
            let fine = ui.input(|i| i.modifiers.shift);
            let travel = (self.height - CAP.y).max(1.0) * if fine { 10.0 } else { 1.0 };
            let np = (pos + resp.drag_delta().y / travel).clamp(0.0, 1.0);
            if np != pos {
                pos = np;
                *self.db = fader_to_db(pos);
                resp.mark_changed();
            }
        }
        if ui.is_rect_visible(rect) {
            let p = ui.painter();
            if self.scale {
                for (i, label) in FADER_LABELS.iter().enumerate() {
                    let y = fader_cap_y(i as f32 / SEGMENTS, rect.top(), self.height);
                    p.text(
                        pos2(rect.left() + SCALE_W, y),
                        Align2::RIGHT_CENTER,
                        label,
                        egui::FontId::monospace(9.0),
                        theme::TEXT_DISABLED,
                    );
                }
            }
            let track = Rect::from_center_size(
                pos2(track_x, rect.center().y),
                vec2(6.0, self.height - CAP.y + 6.0),
            );
            p.rect_filled(track, Rounding::same(3.0), theme::LCD);
            let cap = Rect::from_center_size(pos2(track_x, fader_cap_y(pos, rect.top(), self.height)), CAP);
            let hot = resp.hovered() || resp.dragged();
            let (top_c, bottom_c) = if hot {
                (Color32::from_rgb(0x66, 0x6E, 0x79), Color32::from_rgb(0x46, 0x4C, 0x55))
            } else {
                (Color32::from_rgb(0x5A, 0x61, 0x6B), Color32::from_rgb(0x3C, 0x42, 0x4A))
            };
            p.rect_filled(cap, Rounding::same(theme::R_SM), bottom_c);
            let upper = Rect::from_min_max(cap.min, pos2(cap.max.x, cap.center().y));
            p.rect_filled(
                upper,
                Rounding { nw: theme::R_SM, ne: theme::R_SM, sw: 0.0, se: 0.0 },
                top_c,
            );
            p.rect_stroke(cap, Rounding::same(theme::R_SM), Stroke::new(1.0_f32, theme::SEPARATOR));
            let y = cap.center().y;
            p.line_segment(
                [pos2(cap.left() + 2.0, y), pos2(cap.right() - 2.0, y)],
                Stroke::new(1.0_f32, theme::TEXT),
            );
        }
        resp
    }
}

// ===================================================================================== meter

/// Stereo meter: two bars on the fader scale, a peak-hold marker per channel and a clip light on top that a click
/// clears. `levels` are the current peaks in dB; `holds` keeps the markers between frames.
pub fn stereo_meter(ui: &mut Ui, size: Vec2, levels: [f32; 2], holds: &mut [PeakHold; 2]) -> Response {
    let (rect, resp) = ui.allocate_exact_size(size, Sense::click());
    let dt = ui.input(|i| i.stable_dt).min(0.1);
    for (h, l) in holds.iter_mut().zip(levels) {
        h.update(l, dt);
    }
    if resp.clicked() {
        holds.iter_mut().for_each(PeakHold::reset_clip);
    }
    if !ui.is_rect_visible(rect) {
        return resp;
    }
    let p = ui.painter();
    let gap = 2.0;
    let clip_h = 4.0;
    let bar_w = (rect.width() - gap) / 2.0;
    for (ch, (level, hold)) in levels.iter().zip(holds.iter()).enumerate() {
        let x0 = rect.left() + ch as f32 * (bar_w + gap);
        let clip = Rect::from_min_size(pos2(x0, rect.top()), vec2(bar_w, clip_h));
        p.rect_filled(
            clip,
            Rounding::same(1.0),
            if hold.clipped { theme::REC } else { theme::LCD },
        );
        let bar = Rect::from_min_max(pos2(x0, rect.top() + clip_h + 2.0), pos2(x0 + bar_w, rect.bottom()));
        p.rect_filled(bar, Rounding::same(2.0), theme::LCD);
        paint_meter_fill(p, bar, meter_fill(*level));
        if hold_marker_shown(hold.db) {
            let y = bar.bottom() - meter_fill(hold.db) * bar.height();
            let c = if hold.db > 0.0 { theme::REC } else { theme::SOLO };
            p.rect_filled(
                Rect::from_min_max(pos2(bar.left(), y - 1.0), pos2(bar.right(), y + 1.0)),
                Rounding::ZERO,
                c,
            );
        }
    }
    resp
}

/// The lit part of a meter bar as a vertical gradient mesh.
pub fn paint_meter_fill(p: &Painter, bar: Rect, fill: f32) {
    if fill <= 0.0 {
        return;
    }
    let top = bar.bottom() - fill * bar.height();
    let mut mesh = egui::Mesh::default();
    let mut add_quad = |y0: f32, y1: f32| {
        let t0 = (bar.bottom() - y0) / bar.height();
        let t1 = (bar.bottom() - y1) / bar.height();
        let (c0, c1) = (meter_color(t0), meter_color(t1));
        let i = mesh.vertices.len() as u32;
        mesh.colored_vertex(pos2(bar.left(), y0), c0);
        mesh.colored_vertex(pos2(bar.right(), y0), c0);
        mesh.colored_vertex(pos2(bar.right(), y1), c1);
        mesh.colored_vertex(pos2(bar.left(), y1), c1);
        mesh.add_triangle(i, i + 1, i + 2);
        mesh.add_triangle(i, i + 2, i + 3);
    };
    // quads between the gradient stops, cut at the top of the lit part
    let mut y = bar.bottom();
    for stop in [0.65, 0.90, 1.0] {
        let ys = bar.bottom() - stop * bar.height();
        let y1 = ys.max(top);
        if y1 < y {
            add_quad(y, y1);
            y = y1;
        }
        if y <= top {
            break;
        }
    }
    p.add(Shape::mesh(mesh));
}

// ===================================================================================== state buttons

/// The per-track state buttons and their lit colors.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum StateKind {
    Mute,
    Solo,
    Rec,
    Monitor,
}

impl StateKind {
    pub fn letter(self) -> &'static str {
        match self {
            StateKind::Mute => "M",
            StateKind::Solo => "S",
            StateKind::Rec => "R",
            StateKind::Monitor => "I",
        }
    }

    /// (fill, text) when lit.
    pub fn colors(self) -> (Color32, Color32) {
        match self {
            StateKind::Mute => (theme::MUTE, theme::ON_MUTE),
            StateKind::Solo => (theme::SOLO, theme::ON_SOLO),
            StateKind::Rec => (theme::REC, theme::ON_REC),
            StateKind::Monitor => (theme::MONITOR, theme::ON_MONITOR),
        }
    }
}

pub const STATE_BUTTON: Vec2 = vec2(24.0, 20.0);

/// A small M / S / R / I toggle; a click flips `on`.
pub fn state_toggle(ui: &mut Ui, on: &mut bool, kind: StateKind) -> Response {
    let (rect, mut resp) = ui.allocate_exact_size(STATE_BUTTON, Sense::click());
    if resp.clicked() {
        *on = !*on;
        resp.mark_changed();
    }
    if ui.is_rect_visible(rect) {
        paint_state_button(ui.painter(), rect, kind, *on, resp.hovered());
    }
    resp
}

/// (fill, border, text) of a state button: the state's own color when lit, a raised neutral at rest.
pub fn state_button_colors(kind: StateKind, on: bool, hovered: bool) -> (Color32, Color32, Color32) {
    if on {
        let (bg, fg) = kind.colors();
        (bg, bg, fg)
    } else if hovered {
        (theme::HOVER, theme::BORDER_HOVER, theme::TEXT)
    } else {
        (theme::RAISED, STATE_OFF_BORDER, theme::TEXT_2)
    }
}

/// Border of a state button at rest.
const STATE_OFF_BORDER: Color32 = Color32::from_rgb(0x2E, 0x32, 0x38);

/// Paints a state button in `rect`, for views that place their own buttons (the track headers).
pub fn paint_state_button(painter: &Painter, rect: Rect, kind: StateKind, on: bool, hovered: bool) {
    let (fill, border, text) = state_button_colors(kind, on, hovered);
    painter.rect(rect, Rounding::same(theme::R_SM), fill, Stroke::new(1.0_f32, border));
    painter.text(
        rect.center(),
        Align2::CENTER_CENTER,
        kind.letter(),
        egui::FontId::new(theme::SIZE_LABEL, theme::semibold()),
        text,
    );
}

// ===================================================================================== segmented control

/// Exclusive choice between a few options, drawn as one sunken well with the chosen segment raised.
pub fn segmented(ui: &mut Ui, selected: &mut usize, options: &[&str]) -> Response {
    let pad = 2.0;
    let seg_h = 22.0;
    let font = theme::body_font();
    let galleys: Vec<_> = options
        .iter()
        .map(|o| ui.painter().layout_no_wrap((*o).to_owned(), font.clone(), theme::TEXT))
        .collect();
    let widths: Vec<f32> = galleys.iter().map(|g| g.size().x + 2.0 * theme::SP_5).collect();
    let total = widths.iter().sum::<f32>() + 2.0 * pad;
    let (rect, mut resp) = ui.allocate_exact_size(vec2(total, seg_h + 2.0 * pad), Sense::click());
    let mut edges = vec![rect.left() + pad];
    for w in &widths {
        edges.push(edges.last().copied().unwrap_or_default() + w);
    }
    let hover = resp.hover_pos().and_then(|p| segment_at(p.x, &edges));
    if resp.clicked() {
        if let Some(i) = resp.interact_pointer_pos().and_then(|p| segment_at(p.x, &edges)) {
            if *selected != i {
                *selected = i;
                resp.mark_changed();
            }
        }
    }
    if ui.is_rect_visible(rect) {
        let p = ui.painter();
        p.rect(rect, Rounding::same(theme::R_LG + 1.0), theme::INSET, Stroke::new(1.0_f32, theme::BORDER));
        for (i, (opt, w)) in options.iter().zip(&widths).enumerate() {
            let seg = Rect::from_min_size(pos2(edges[i], rect.top() + pad), vec2(*w, seg_h));
            let (fill, color) = if i == *selected {
                (theme::SEGMENT_ON, theme::TEXT)
            } else if hover == Some(i) {
                (theme::RAISED, theme::TEXT)
            } else {
                (Color32::TRANSPARENT, theme::TEXT_2)
            };
            p.rect_filled(seg, Rounding::same(theme::R_MD), fill);
            p.text(seg.center(), Align2::CENTER_CENTER, *opt, font.clone(), color);
        }
    }
    resp
}

/// Width of one segment of [`segmented_icons`].
pub const ICON_SEGMENT_W: f32 = 32.0;

/// [`segmented`] with an icon in each segment instead of a word (for a narrow bar).
pub fn segmented_icons(ui: &mut Ui, selected: &mut usize, icons: &[Icon]) -> Response {
    let pad = 2.0;
    let seg_h = 22.0;
    let total = icons.len() as f32 * ICON_SEGMENT_W + 2.0 * pad;
    let (rect, mut resp) = ui.allocate_exact_size(vec2(total, seg_h + 2.0 * pad), Sense::click());
    let edges: Vec<f32> = (0..=icons.len()).map(|i| rect.left() + pad + i as f32 * ICON_SEGMENT_W).collect();
    let hover = resp.hover_pos().and_then(|p| segment_at(p.x, &edges));
    if resp.clicked() {
        if let Some(i) = resp.interact_pointer_pos().and_then(|p| segment_at(p.x, &edges)) {
            if *selected != i {
                *selected = i;
                resp.mark_changed();
            }
        }
    }
    if ui.is_rect_visible(rect) {
        let p = ui.painter();
        p.rect(rect, Rounding::same(theme::R_LG + 1.0), theme::INSET, Stroke::new(1.0_f32, theme::BORDER));
        for (i, icon) in icons.iter().enumerate() {
            let seg = Rect::from_min_size(pos2(edges[i], rect.top() + pad), vec2(ICON_SEGMENT_W, seg_h));
            let (fill, color) = if i == *selected {
                (theme::SEGMENT_ON, theme::TEXT)
            } else if hover == Some(i) {
                (theme::RAISED, theme::TEXT)
            } else {
                (Color32::TRANSPARENT, theme::TEXT_2)
            };
            p.rect_filled(seg, Rounding::same(theme::R_MD), fill);
            paint_icon(p, *icon, Rect::from_center_size(seg.center(), Vec2::splat(13.0)), color);
        }
    }
    resp
}

// ===================================================================================== effect slot

/// What an insert slot shows.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SlotState<'a> {
    Normal(&'a str),
    Bypassed(&'a str),
    Empty,
}

pub const SLOT_HEIGHT: f32 = 22.0;

/// An insert slot button: the effect name, struck through when bypassed, or a dashed "+" when empty.
pub fn effect_slot(ui: &mut Ui, state: SlotState<'_>, width: f32) -> Response {
    let (rect, resp) = ui.allocate_exact_size(vec2(width, SLOT_HEIGHT), Sense::click());
    if !ui.is_rect_visible(rect) {
        return resp;
    }
    let p = ui.painter();
    let hot = resp.hovered();
    let r = Rounding::same(theme::R_SM);
    match state {
        SlotState::Normal(name) | SlotState::Bypassed(name) => {
            let bypassed = matches!(state, SlotState::Bypassed(_));
            let (fill, border) = if hot {
                (theme::HOVER, theme::BORDER_HOVER)
            } else {
                (theme::CONTROL, theme::BORDER)
            };
            p.rect(rect, r, fill, Stroke::new(1.0_f32, border));
            let color = if bypassed { theme::TEXT_DISABLED } else { theme::TEXT };
            let mut job = egui::text::LayoutJob::simple_singleline(
                name.to_owned(),
                egui::FontId::proportional(theme::SIZE_NUMBER),
                color,
            );
            job.wrap = egui::text::TextWrapping::truncate_at_width(width - 2.0 * theme::SP_2);
            if bypassed {
                if let Some(s) = job.sections.first_mut() {
                    s.format.strikethrough = Stroke::new(1.0_f32, color);
                }
            }
            let galley = ui.fonts(|f| f.layout_job(job));
            let at = rect.center() - galley.size() / 2.0;
            p.galley(at, galley, color);
        }
        SlotState::Empty => {
            let border = if hot { theme::BORDER_HOVER } else { theme::TRACK_OFF };
            let corners = [rect.left_top(), rect.right_top(), rect.right_bottom(), rect.left_bottom(), rect.left_top()];
            p.extend(Shape::dashed_line(&corners, Stroke::new(1.0_f32, border), 3.0, 2.0));
            let c = rect.center();
            let color = if hot { theme::TEXT } else { theme::TEXT_DISABLED };
            let st = Stroke::new(1.3_f32, color);
            p.line_segment([c - vec2(4.0, 0.0), c + vec2(4.0, 0.0)], st);
            p.line_segment([c - vec2(0.0, 4.0), c + vec2(0.0, 4.0)], st);
        }
    }
    resp
}

#[cfg(test)]
mod tests {
    use super::*;

    fn close(a: f32, b: f32, eps: f32) -> bool {
        (a - b).abs() <= eps
    }

    #[test]
    fn values_map_to_unit_and_back_clamped() {
        assert_eq!(value_to_unit(5.0, 0.0, 10.0), 0.5);
        assert_eq!(value_to_unit(-3.0, 0.0, 10.0), 0.0);
        assert_eq!(value_to_unit(30.0, 0.0, 10.0), 1.0);
        assert_eq!(value_to_unit(1.0, 2.0, 2.0), 0.0);
        assert_eq!(unit_to_value(0.25, -1.0, 1.0), -0.5);
        assert_eq!(unit_to_value(2.0, -1.0, 1.0), 1.0);
        for v in [-1.0, -0.3, 0.0, 0.8, 1.0] {
            assert!(close(unit_to_value(value_to_unit(v, -1.0, 1.0), -1.0, 1.0), v, 1e-6));
        }
    }

    #[test]
    fn dragging_up_raises_the_knob_and_shift_is_finer() {
        assert!(close(drag_unit(0.5, -100.0, false), 1.0, 1e-6));
        assert!(close(drag_unit(0.5, 20.0, false), 0.4, 1e-6));
        assert!(close(drag_unit(0.5, -100.0, true), 0.55, 1e-6));
        assert_eq!(drag_unit(0.9, -500.0, false), 1.0);
        assert_eq!(drag_unit(0.1, 500.0, false), 0.0);
    }

    #[test]
    fn knob_turns_270_degrees_with_the_gap_at_the_bottom() {
        assert!(close(knob_angle(0.0).to_degrees(), 135.0, 1e-3));
        assert!(close(knob_angle(0.5).to_degrees(), 270.0, 1e-3)); // straight up on screen
        assert!(close(knob_angle(1.0).to_degrees(), 405.0, 1e-3));
        assert_eq!(knob_angle(-1.0), knob_angle(0.0));
        // the design's example: pointer towards the top-left at a third of the range
        assert!(close(knob_angle(1.0 / 3.0).to_degrees(), 225.0, 1e-3));
    }

    #[test]
    fn fader_marks_sit_on_even_steps_of_the_travel() {
        for (i, db) in FADER_MARKS.iter().enumerate() {
            assert!(close(db_to_fader(*db), i as f32 / 6.0, 1e-5), "{db} dB");
        }
        assert_eq!(db_to_fader(12.0), 0.0);
        assert_eq!(db_to_fader(f32::NAN), 1.0);
        // the mixer mockup: -6 dB at a third, -18 dB a little past half
        assert!(close(db_to_fader(-18.0), 0.5833, 1e-3));
        assert!(close(db_to_fader(-4.0), 0.2778, 1e-3));
    }

    #[test]
    fn fader_position_and_db_round_trip() {
        for db in [6.0, 3.0, 0.0, -1.5, -6.0, -9.0, -20.0, -40.0, -48.0, -60.0, -90.0] {
            let back = fader_to_db(db_to_fader(db));
            assert!(close(back, db, 0.01), "{db} -> {back}");
        }
        assert_eq!(fader_to_db(1.0), f32::NEG_INFINITY);
        assert_eq!(fader_to_db(-0.2), 6.0);
        // monotonic: lower on the travel is always quieter
        let mut last = f32::INFINITY;
        for k in 0..=100 {
            let db = fader_to_db(k as f32 / 100.0);
            assert!(db < last, "step {k}");
            last = db;
        }
    }

    #[test]
    fn gain_and_db_convert_both_ways() {
        assert!(close(db_to_gain(-6.0206), 0.5, 1e-4));
        assert_eq!(db_to_gain(f32::NEG_INFINITY), 0.0);
        assert_eq!(gain_to_db(0.0), f32::NEG_INFINITY);
        assert!(close(gain_to_db(1.0), 0.0, 1e-6));
    }

    #[test]
    fn meter_follows_the_fader_scale() {
        assert_eq!(meter_fill(f32::NEG_INFINITY), 0.0);
        assert_eq!(meter_fill(6.0), 1.0);
        assert!(close(meter_fill(0.0), 5.0 / 6.0, 1e-5));
        assert_eq!(meter_color(0.0), theme::METER_LOW);
        assert_eq!(meter_color(1.0), theme::REC);
        assert_eq!(meter_color(0.9), theme::SOLO);
    }

    #[test]
    fn hold_marker_hides_on_the_floor() {
        assert!(!hold_marker_shown(f32::NEG_INFINITY));
        assert!(!hold_marker_shown(-200.0));
        assert!(hold_marker_shown(-48.0));
        assert!(hold_marker_shown(0.0));
    }

    #[test]
    fn peak_hold_waits_then_falls_and_never_goes_below_the_signal() {
        let mut h = PeakHold::default();
        h.update(-6.0, 0.016);
        assert_eq!(h.db, -6.0);
        // held for the hold time
        h.update(-30.0, 1.0);
        assert_eq!(h.db, -6.0);
        // 0.5 s past the hold time: falls 10 dB
        h.update(-30.0, 1.0);
        assert!(close(h.db, -16.0, 1e-4), "{}", h.db);
        // stops at the current signal
        h.update(-30.0, 5.0);
        assert_eq!(h.db, -30.0);
        // a louder peak pushes it up again and restarts the hold
        h.update(-3.0, 0.016);
        assert_eq!(h.db, -3.0);
        h.update(-50.0, 1.0);
        assert_eq!(h.db, -3.0);
    }

    #[test]
    fn clip_latches_until_reset() {
        let mut h = PeakHold::default();
        h.update(-0.1, 0.016);
        assert!(!h.clipped);
        h.update(0.4, 0.016);
        assert!(h.clipped);
        h.update(-20.0, 3.0);
        assert!(h.clipped);
        h.reset_clip();
        assert!(!h.clipped);
    }

    #[test]
    fn segment_hit_test_uses_the_edges() {
        let edges = [10.0, 50.0, 80.0, 130.0];
        assert_eq!(segment_at(9.9, &edges), None);
        assert_eq!(segment_at(10.0, &edges), Some(0));
        assert_eq!(segment_at(50.0, &edges), Some(1));
        assert_eq!(segment_at(129.0, &edges), Some(2));
        assert_eq!(segment_at(130.0, &edges), None);
    }

    #[test]
    fn fader_cap_stays_inside_the_widget_and_is_hit_where_drawn() {
        assert_eq!(fader_cap_y(0.0, 100.0, 200.0), 111.0);
        assert_eq!(fader_cap_y(1.0, 100.0, 200.0), 289.0);
        assert!(fader_cap_hit(pos2(50.0, 111.0), 50.0, 0.0, 100.0, 200.0));
        assert!(fader_cap_hit(pos2(62.0, 120.0), 50.0, 0.0, 100.0, 200.0));
        assert!(!fader_cap_hit(pos2(50.0, 140.0), 50.0, 0.0, 100.0, 200.0));
        assert!(!fader_cap_hit(pos2(70.0, 111.0), 50.0, 0.0, 100.0, 200.0));
    }

    #[test]
    fn path_parses_relative_lines_and_closes() {
        let p = parse_path("M3.5 3.5h9v9h-9z");
        assert_eq!(p.len(), 1);
        assert!(p[0].closed);
        assert_eq!(
            p[0].points,
            vec![pos2(3.5, 3.5), pos2(12.5, 3.5), pos2(12.5, 12.5), pos2(3.5, 12.5)]
        );
    }

    #[test]
    fn path_repeats_implicit_coordinates_and_splits_subpaths() {
        // "L13 8 4 13.5": two line-tos; compact numbers like "2.6-.7"
        let p = parse_path("M4 2.5L13 8 4 13.5z");
        assert_eq!(p[0].points, vec![pos2(4.0, 2.5), pos2(13.0, 8.0), pos2(4.0, 13.5)]);
        let lib = parse_path("M11 3.2l2.6-.7 2 10.6-2.7.6z");
        assert_eq!(lib.len(), 1);
        assert!(close(lib[0].points[1].x, 13.6, 1e-5) && close(lib[0].points[1].y, 2.5, 1e-5));
        assert!(close(lib[0].points[3].x, 12.9, 1e-5) && close(lib[0].points[3].y, 13.7, 1e-5));
        let mixer = parse_path(Icon::Mixer.path());
        assert_eq!(mixer.len(), 6);
        assert!(mixer.iter().all(|s| !s.closed && s.points.len() == 2));
    }

    #[test]
    fn arcs_follow_the_circle_the_long_way_round() {
        // the knob ring: from bottom-left to bottom-right over the top of a circle of radius 15 around (20, 20)
        let p = parse_path("M9.4 30.6A15 15 0 1 1 30.6 30.6");
        let pts = &p[0].points;
        let last = pts.last().copied().unwrap();
        assert!(close(last.x, 30.6, 1e-3) && close(last.y, 30.6, 1e-3));
        let top = pts.iter().map(|q| q.y).fold(f32::INFINITY, f32::min);
        assert!(close(top, 5.0, 0.2), "top of the arc at {top}");
        for q in pts {
            assert!(close(q.distance(pos2(20.0, 20.0)), 15.0, 0.1), "{q:?}");
        }
        // relative small arc of the cycle icon: a quarter circle of radius 3
        let c = parse_path("M3 7a3 3 0 0 1 3-3");
        let end = *c[0].points.last().unwrap();
        assert!(close(end.x, 6.0, 1e-4) && close(end.y, 4.0, 1e-4));
        for q in &c[0].points {
            assert!(close(q.distance(pos2(6.0, 7.0)), 3.0, 0.05), "{q:?}");
        }
    }

    #[test]
    fn smooth_cubics_reflect_the_previous_control_point() {
        let p = parse_path("M0 0C0 4 4 4 4 0S8 -4 8 0");
        let pts = &p[0].points;
        assert_eq!(*pts.last().unwrap(), pos2(8.0, 0.0));
        // the S half mirrors the C half: below the axis where the first went above
        let mid_second = pts[CURVE_STEPS + CURVE_STEPS / 2];
        assert!(close(mid_second.x, 6.0, 1e-4) && close(mid_second.y, -3.0, 1e-4), "{mid_second:?}");
    }

    #[test]
    fn every_icon_parses_inside_its_16_grid() {
        for icon in Icon::ALL {
            let strokes = icon.strokes();
            assert!(!strokes.is_empty(), "{icon:?}");
            for s in strokes {
                assert!(s.points.len() >= 2, "{icon:?}");
                for q in &s.points {
                    assert!((0.0..=16.0).contains(&q.x) && (0.0..=16.0).contains(&q.y), "{icon:?} {q:?}");
                }
            }
        }
        assert_eq!(
            Icon::ALL.iter().map(|i| i.path()).collect::<std::collections::HashSet<_>>().len(),
            Icon::ALL.len()
        );
        // the enum order is the cache order
        for (i, icon) in Icon::ALL.iter().enumerate() {
            assert_eq!(*icon as usize, i);
        }
    }

    #[test]
    fn interface_icons_have_the_expected_strokes() {
        let count = |i: Icon| i.strokes().len();
        assert_eq!(count(Icon::Close), 2);
        assert_eq!(count(Icon::Plus), 2);
        assert_eq!(count(Icon::Settings), 9);
        assert_eq!(count(Icon::Warning), 3);
        // the three dots of "more" are closed circles in a row
        let more = Icon::More.strokes();
        assert_eq!(more.len(), 3);
        assert!(more.iter().all(|s| s.closed));
        let centers: Vec<f32> =
            more.iter().map(|s| s.points.iter().map(|p| p.x).sum::<f32>() / s.points.len() as f32).collect();
        assert!(centers.windows(2).all(|w| w[0] < w[1]), "{centers:?}");
        // chevrons are single open strokes pointing their way
        let down = &Icon::ChevronDown.strokes()[0];
        assert!(!down.closed && down.points.len() == 3 && down.points[1].y > down.points[0].y);
        let left = &Icon::ChevronLeft.strokes()[0];
        assert!(left.points[1].x < left.points[0].x);
        let right = &Icon::ChevronRight.strokes()[0];
        assert!(right.points[1].x > right.points[0].x);
        // the search lens is a circle with a handle toward the lower right
        let search = Icon::Search.strokes();
        assert!(search[0].closed && !search[1].closed);
        assert!(search[1].points[1].x > search[1].points[0].x && search[1].points[1].y > search[1].points[0].y);
    }

    #[test]
    fn tool_colors_hover_lifts_rest_and_keeps_the_tint_of_lit_buttons() {
        let rest = ToolColors::OFF.hovered();
        assert_eq!(rest.fill, theme::HOVER);
        assert_eq!(rest.border, theme::BORDER_HOVER);
        let play = ToolColors::PLAY.hovered();
        assert_eq!(play.fill, ToolColors::PLAY.fill);
        assert_eq!(play.fg, ToolColors::PLAY.fg);
        assert_ne!(play.border, ToolColors::PLAY.border);
        assert_eq!(mix(Color32::BLACK, Color32::WHITE, 0.5), Color32::from_rgb(128, 128, 128));
        assert_eq!(mix(Color32::BLACK, Color32::WHITE, 2.0), Color32::WHITE);
    }

    #[test]
    fn malformed_paths_stop_without_panicking() {
        assert!(parse_path("").is_empty());
        assert_eq!(parse_path("M1 1L").len(), 1);
        assert!(parse_path("Q1 2 3 4").is_empty());
        assert_eq!(parse_path("M1 1 2 2 3 3")[0].points.len(), 3);
    }

    #[test]
    fn log_rule_matches_the_old_parameter_editor() {
        assert!(param_is_log("Hz", 20.0, 20000.0));
        assert!(param_is_log("s", 0.001, 5.0));
        assert!(param_is_log("ms", 0.0, 500.0));
        assert!(!param_is_log("dB", -24.0, 24.0));
        assert!(!param_is_log("s", 0.3, 4.0));
        assert!(!param_is_log("ms", 1.0, 50.0));
        assert!(!param_is_log("", 0.0, 1.0));
    }

    #[test]
    fn log_mapping_round_trips_and_hits_the_ends() {
        assert_eq!(value_to_unit_log(20.0, 20.0, 20000.0), 0.0);
        assert_eq!(value_to_unit_log(20000.0, 20.0, 20000.0), 1.0);
        assert!(close(value_to_unit_log(632.5, 20.0, 20000.0), 0.5, 0.01));
        for v in [50.0, 440.0, 2500.0, 12000.0] {
            let back = unit_to_value_log(value_to_unit_log(v, 20.0, 20000.0), 20.0, 20000.0);
            assert!((back - v).abs() / v < 1e-3, "{v} -> {back}");
        }
        let mut last = -1.0;
        for k in 0..=100 {
            let v = unit_to_value_log(k as f32 / 100.0, 20.0, 20000.0);
            assert!(v > last, "not increasing at {k}");
            last = v;
        }
        // a range starting at 0 (Hold 0..500 ms): the ends are exact, the curve still rises
        assert_eq!(unit_to_value_log(0.0, 0.0, 500.0), 0.0);
        assert_eq!(unit_to_value_log(1.0, 0.0, 500.0), 500.0);
        assert_eq!(value_to_unit_log(0.0, 0.0, 500.0), 0.0);
        let mut last = -1.0;
        for k in 0..=100 {
            let v = unit_to_value_log(k as f32 / 100.0, 0.0, 500.0);
            assert!(v > last, "not increasing at {k}");
            last = v;
        }
        assert_eq!(value_to_unit_log(5.0, 3.0, 3.0), 0.0);
        assert_eq!(unit_to_value_log(0.5, 3.0, 3.0), 3.0);
    }

    const SHOWN: [(f32, &str, &str); 15] = [
        (2400.0, "Hz", "2.40 kHz"),
        (12500.0, "Hz", "12.5 kHz"),
        (440.0, "Hz", "440 Hz"),
        (52.0, "Hz", "52.0 Hz"),
        (-6.0, "dB", "-6.0 dB"),
        (2.5, "dB", "+2.5 dB"),
        (0.0, "dB", "0.0 dB"),
        (0.005, "s", "5.0 ms"),
        (1.2, "s", "1.20 s"),
        (350.0, "ms", "350 ms"),
        (4.0, ":1", "4.0:1"),
        (7.0, "ct", "+7 ct"),
        (0.5, "", "0.50"),
        (1.0, "x", "1.00x"),
        (8.0, "bit", "8 bit"),
    ];

    #[test]
    fn values_read_with_their_unit() {
        for (v, unit, text) in SHOWN {
            assert_eq!(format_param_value(v, unit), text, "{v} {unit}");
        }
        assert_eq!(format_param_value(-12.0, "st"), "-12 st");
        assert_eq!(format_param_value(0.0, "st"), "0 st");
        assert_eq!(format_param_value(0.75, "beats"), "0.75 beats");
        assert_eq!(format_param_value(-0.01, "dB"), "0.0 dB");
    }

    #[test]
    fn typed_values_parse_with_or_without_units() {
        let cases: [(&str, &str, f32); 8] = [
            ("2.4k", "Hz", 2400.0),
            ("2.4 kHz", "Hz", 2400.0),
            ("880", "Hz", 880.0),
            ("-6", "dB", -6.0),
            ("\u{2212}3,5 dB", "dB", -3.5),
            ("250ms", "s", 0.25),
            ("0.5s", "ms", 500.0),
            ("4:1", ":1", 4.0),
        ];
        for (text, unit, v) in cases {
            let got = parse_param_value(text, unit).unwrap_or_else(|| panic!("{text} did not parse"));
            assert!(close(got, v, 1e-4), "{text}: {got}");
        }
        assert_eq!(parse_param_value("abc", "Hz"), None);
        assert_eq!(parse_param_value("", "dB"), None);
        assert_eq!(parse_param_value("inf", ""), None);
        for (v, unit, text) in SHOWN {
            let back = parse_param_value(text, unit).unwrap_or_else(|| panic!("{text} did not parse"));
            assert!((back - v).abs() <= v.abs() * 0.01 + 1e-3, "{text} -> {back}, not {v}");
        }
    }

    use egui::{Event, Key, Modifiers, PointerButton};

    /// A knob drawn alone in a context fed with synthetic input, one frame at a time.
    struct KnobRig {
        ctx: egui::Context,
        frame: usize,
        v: f32,
        range: (f32, f32),
        log: bool,
        unit: &'static str,
        rect: Rect,
        id: egui::Id,
    }

    const RIG_D: f32 = 40.0;

    impl KnobRig {
        fn new(v: f32, range: (f32, f32), log: bool, unit: &'static str) -> Self {
            let mut rig = KnobRig { ctx: egui::Context::default(), frame: 0, v, range, log, unit, rect: Rect::NOTHING, id: egui::Id::NULL };
            rig.run(vec![], Modifiers::NONE);
            rig
        }

        /// The center of the knob's dial, under its label.
        fn grab(&self) -> Pos2 {
            pos2(self.rect.center().x, self.rect.top() + theme::SIZE_LABEL + 4.0 + RIG_D / 2.0)
        }

        /// One frame: (changed, dragged, double-clicked).
        fn run(&mut self, events: Vec<Event>, modifiers: Modifiers) -> (bool, bool, bool) {
            let raw = egui::RawInput {
                screen_rect: Some(Rect::from_min_size(Pos2::ZERO, vec2(800.0, 600.0))),
                time: Some(self.frame as f64 * 0.016),
                modifiers,
                events,
                ..Default::default()
            };
            self.frame += 1;
            let mut out = (false, false, false);
            let (range, log, unit) = (self.range, self.log, self.unit);
            let v = &mut self.v;
            let mut rect = Rect::NOTHING;
            let mut id = egui::Id::NULL;
            let _ = self.ctx.run(raw, |ctx| {
                egui::CentralPanel::default().show(ctx, |ui| {
                    let knob = Knob::new(v, range.0..=range.1)
                        .log(log)
                        .unit(unit)
                        .label("Freq")
                        .editable(true)
                        .default_value(range.0)
                        .diameter(RIG_D)
                        .width(76.0);
                    let r = ui.put(Rect::from_min_size(pos2(100.0, 100.0), vec2(76.0, 96.0)), knob);
                    out = (r.changed(), r.dragged(), r.double_clicked());
                    rect = r.rect;
                    id = r.id;
                });
            });
            self.rect = rect;
            self.id = id;
            out
        }

        fn editing(&self) -> Option<KnobEdit> {
            self.ctx.data(|d| d.get_temp::<KnobEdit>(knob_edit_id(self.id)))
        }

        /// A vertical drag from the dial: 10 px, then `rest` more (negative is up). The flags of the drag frames.
        fn drag(&mut self, rest: f32, modifiers: Modifiers) -> Vec<(bool, bool, bool)> {
            let g = self.grab();
            let button = |pos, pressed| Event::PointerButton { pos, button: PointerButton::Primary, pressed, modifiers };
            let dir = rest.signum();
            self.run(vec![Event::PointerMoved(g)], modifiers);
            self.run(vec![button(g, true)], modifiers);
            let first = g + vec2(0.0, 10.0 * dir);
            let last = first + vec2(0.0, rest);
            let a = self.run(vec![Event::PointerMoved(first)], modifiers);
            let b = self.run(vec![Event::PointerMoved(last)], modifiers);
            self.run(vec![button(last, false)], modifiers);
            vec![a, b]
        }

        fn click(&mut self, button: PointerButton) -> (bool, bool, bool) {
            let g = self.grab();
            let ev = |pressed| Event::PointerButton { pos: g, button, pressed, modifiers: Modifiers::NONE };
            self.run(vec![Event::PointerMoved(g)], Modifiers::NONE);
            self.run(vec![ev(true)], Modifiers::NONE);
            self.run(vec![ev(false)], Modifiers::NONE)
        }

        fn key(&mut self, key: Key) -> (bool, bool, bool) {
            self.run(vec![Event::Key { key, physical_key: None, pressed: true, repeat: false, modifiers: Modifiers::NONE }], Modifiers::NONE)
        }

        fn type_text(&mut self, text: &str) {
            self.run(vec![Event::Text(text.into())], Modifiers::NONE);
        }
    }

    #[test]
    fn knob_drag_up_raises_and_shift_is_finer() {
        let mut rig = KnobRig::new(0.5, (0.0, 1.0), false, "");
        let flags = rig.drag(-50.0, Modifiers::NONE);
        assert!((0.74..=0.80).contains(&rig.v), "{}", rig.v);
        assert!(flags.iter().any(|f| f.0 && f.1), "{flags:?}");
        let mut rig = KnobRig::new(0.5, (0.0, 1.0), false, "");
        rig.drag(-50.0, Modifiers::SHIFT);
        assert!((0.52..=0.5301).contains(&rig.v), "{}", rig.v);
    }

    #[test]
    fn knob_drag_follows_the_log_scale() {
        let mut rig = KnobRig::new(632.5, (20.0, 20000.0), true, "Hz");
        rig.drag(-90.0, Modifiers::NONE);
        assert!(rig.v >= 15000.0, "{}", rig.v);
        let mut rig = KnobRig::new(632.5, (20.0, 20000.0), true, "Hz");
        rig.drag(90.0, Modifiers::NONE);
        assert!(rig.v <= 30.0, "{}", rig.v);
    }

    #[test]
    fn knob_double_click_restores_the_default() {
        let mut rig = KnobRig::new(0.7, (0.1, 1.0), false, "");
        rig.click(PointerButton::Primary);
        let second = rig.click(PointerButton::Primary);
        assert_eq!(rig.v, 0.1);
        assert!(second.2 && second.0, "{second:?}");
    }

    #[test]
    fn knob_right_click_types_a_value() {
        let mut rig = KnobRig::new(632.5, (20.0, 20000.0), true, "Hz");
        rig.click(PointerButton::Secondary);
        assert!(rig.editing().is_some());
        rig.type_text("2.5k");
        let (changed, dragged, _) = rig.key(Key::Enter);
        assert_eq!(rig.v, 2500.0);
        assert!(changed && !dragged);
        assert!(rig.editing().is_none());
    }

    #[test]
    fn knob_typed_values_are_clamped_and_bad_text_is_ignored() {
        let mut rig = KnobRig::new(632.5, (20.0, 20000.0), true, "Hz");
        rig.click(PointerButton::Secondary);
        rig.type_text("50k");
        rig.key(Key::Enter);
        assert_eq!(rig.v, 20000.0);
        rig.click(PointerButton::Secondary);
        rig.type_text("abc");
        let (changed, _, _) = rig.key(Key::Enter);
        assert_eq!(rig.v, 20000.0);
        assert!(!changed);
    }

    #[test]
    fn knob_escape_cancels_the_typed_value() {
        let mut rig = KnobRig::new(632.5, (20.0, 20000.0), true, "Hz");
        rig.click(PointerButton::Secondary);
        rig.type_text("999");
        let (changed, _, _) = rig.key(Key::Escape);
        assert!(!changed);
        assert_eq!(rig.v, 632.5);
        rig.run(vec![], Modifiers::NONE);
        assert!(rig.editing().is_none());
        assert_eq!(rig.ctx.memory(|m| m.focused()), None);
    }

    #[test]
    fn knob_without_options_keeps_the_pan_behaviour() {
        let ctx = egui::Context::default();
        let mut v = 0.25f32;
        let g = pos2(100.0 + 17.0, 100.0 + 17.0);
        let press = |pressed| Event::PointerButton { pos: g, button: PointerButton::Secondary, pressed, modifiers: Modifiers::NONE };
        for (frame, events) in [vec![Event::PointerMoved(g)], vec![press(true)], vec![press(false)], vec![]].into_iter().enumerate() {
            let raw = egui::RawInput {
                screen_rect: Some(Rect::from_min_size(Pos2::ZERO, vec2(800.0, 600.0))),
                time: Some(frame as f64 * 0.016),
                events,
                ..Default::default()
            };
            let _ = ctx.run(raw, |ctx| {
                egui::CentralPanel::default().show(ctx, |ui| {
                    ui.put(
                        Rect::from_min_size(pos2(100.0, 100.0), vec2(34.0, 34.0 + theme::SIZE_NUMBER + 4.0)),
                        Knob::new(&mut v, -1.0..=1.0).bipolar(true).text("C"),
                    );
                });
            });
        }
        assert_eq!(ctx.memory(|m| m.focused()), None);
        assert_eq!(v, 0.25);
    }

    #[test]
    fn knob_tooltip_lists_value_note_and_gestures() {
        let note = "Normalized 0..1 \u{00B7} the plug-in shows it in dB";
        let t = knob_tooltip(Some("Gain (dB)"), "0.53", Some(note), true);
        let lines: Vec<&str> = t.lines().collect();
        assert_eq!(lines[0], "Gain (dB): 0.53");
        assert_eq!(lines[1], note);
        assert!(lines[2].contains("Double-click: default"));
        assert!(lines[2].contains("Right-click: type a value"));
        assert_eq!(
            knob_tooltip(Some("Cutoff"), "2.4 kHz", None, false),
            "Cutoff: 2.4 kHz\nDrag up or down \u{00B7} Shift: fine \u{00B7} Double-click: default"
        );
        assert!(knob_tooltip(None, "0.53", None, true).starts_with("0.53\n"));
    }
}
