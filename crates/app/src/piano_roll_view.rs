//! View geometry of the piano roll: wheel scrolling, pointer-anchored zoom, framing a region's notes, the
//! playhead inside looped regions, and how the keyboard, rows, grid lines and lanes are laid out and shaded.
//! Pure functions so they can be tested without a window.

use eframe::egui::{pos2, Color32, Event, MouseWheelUnit, Rect};
use ondina_core::model::{Id, Note};
use ondina_core::theory::note_name;

use crate::theme;

/// Width of the keyboard on the left of the grid (and of the lane labels under it).
pub const KEYS_W: f32 = 64.0;
/// How much shorter than a white key a black key is drawn.
pub const BLACK_KEY_INSET: f32 = 22.0;
/// Height of the bar ruler over the grid.
pub const RULER_H: f32 = 18.0;
/// Height of the velocity lane.
pub const VEL_H: f32 = 44.0;
/// Row height the piano roll opens with: about fifteen rows or more in the default bottom panel.
pub const DEFAULT_KEY_H: f32 = 12.0;
/// Keyboard colors: white and black keys, the line between white keys, the C labels.
pub const KEY_WHITE: Color32 = Color32::from_rgb(0xE8, 0xEA, 0xED);
pub const KEY_BLACK: Color32 = Color32::from_rgb(0x24, 0x27, 0x2C);
pub const KEY_LINE: Color32 = Color32::from_rgb(0xB9, 0xBE, 0xC6);
pub const KEY_LABEL: Color32 = Color32::from_rgb(0x4A, 0x50, 0x59);
/// Below this row height the keys are too short for their C labels.
const KEY_LABEL_MIN_H: f32 = 8.0;

/// Highest and lowest pitch the top row can show (the range the piano roll always allowed).
const TOP_PITCH_MIN: i32 = 20;
const TOP_PITCH_MAX: i32 = 127;
/// Row height limits for vertical zoom.
const KEY_H_MIN: f32 = 6.0;
const KEY_H_MAX: f32 = 28.0;
/// Horizontal zoom limits (unchanged from before).
const PX_PER_BEAT_MIN: f32 = 10.0;
const PX_PER_BEAT_MAX: f32 = 600.0;
/// Middle C, shown when a region has no notes yet.
const C4: i32 = 60;

/// Moves the top row by a smooth wheel delta. egui spreads one wheel notch over several frames in
/// fractions of a row; the fraction is carried to the next frame instead of being truncated, so
/// scrolling up moves exactly as far as scrolling down. Returns the new top pitch and the carry.
pub fn scroll_pitch(top: i32, carry: f32, delta_px: f32, key_h: f32) -> (i32, f32) {
    let rows = carry + delta_px / key_h;
    let whole = rows.trunc();
    let moved = top + whole as i32;
    let clamped = moved.clamp(TOP_PITCH_MIN, TOP_PITCH_MAX);
    // At either end the leftover is dropped, so turning back responds at once.
    if clamped != moved {
        (clamped, 0.0)
    } else {
        (clamped, rows - whole)
    }
}

/// Scales beats per pixel around the pointer: the beat under `pointer_dx` (pixels from the grid's
/// left edge) stays under the pointer unless the view would scroll before beat 0.
pub fn zoom_time(scroll: f64, px_per_beat: f32, pointer_dx: f32, factor: f32) -> (f64, f32) {
    zoom_time_between(scroll, px_per_beat, pointer_dx, factor, PX_PER_BEAT_MIN, PX_PER_BEAT_MAX)
}

/// Same as `zoom_time` with explicit limits, for views with a different zoom range (the arrangement).
pub fn zoom_time_between(scroll: f64, px_per_beat: f32, pointer_dx: f32, factor: f32, min: f32, max: f32) -> (f64, f32) {
    let anchor = scroll + (pointer_dx / px_per_beat) as f64;
    let ppb = (px_per_beat * factor).clamp(min, max);
    ((anchor - (pointer_dx / ppb) as f64).max(0.0), ppb)
}

/// Scales the row height around the pointer: the pitch row under `pointer_dy` (pixels from the grid's
/// top edge) stays under the pointer. The top row is a whole pitch, so the row is kept, not the pixel.
pub fn zoom_pitch(top: i32, key_h: f32, pointer_dy: f32, factor: f32) -> (i32, f32) {
    let pitch = top - (pointer_dy / key_h).floor() as i32;
    let kh = (key_h * factor).clamp(KEY_H_MIN, KEY_H_MAX);
    let new_top = pitch + (pointer_dy / kh).floor() as i32;
    (new_top.clamp(TOP_PITCH_MIN, TOP_PITCH_MAX), kh)
}

/// Zoom factor for a wheel movement in points: about 8% per notch, and one notch in then one out
/// returns to the same scale.
pub fn zoom_factor(points: f32) -> f32 {
    (points * 0.002).exp().clamp(0.5, 2.0)
}

/// Wheel movement in points meant for zooming this frame.
#[derive(Debug, Default, Clone, Copy, PartialEq)]
pub struct WheelZoom {
    /// Ctrl+wheel: horizontal (time) zoom.
    pub time: f32,
    /// Ctrl+Shift+wheel: the vertical zoom (pitch rows in the piano roll, track height in the arrangement).
    pub pitch: f32,
}

/// Reads the raw wheel events, because egui 0.29 hands Ctrl+wheel only to its zoom factor and loses
/// Ctrl+Shift+wheel entirely (Shift turns the delta horizontal before the zoom takes its vertical part).
pub fn wheel_zoom(events: &[Event], line_px: f32, page_px: f32) -> WheelZoom {
    let mut z = WheelZoom::default();
    for e in events {
        let Event::MouseWheel { unit, delta, modifiers } = e else { continue };
        if !(modifiers.ctrl || modifiers.command) {
            continue;
        }
        let scale = match unit {
            MouseWheelUnit::Point => 1.0,
            MouseWheelUnit::Line => line_px,
            MouseWheelUnit::Page => page_px,
        };
        if modifiers.shift {
            // Some platforms already send Shift+wheel as horizontal: take both axes.
            z.pitch += (delta.x + delta.y) * scale;
        } else {
            z.time += delta.y * scale;
        }
    }
    z
}

/// Top pitch and first beat that frame a region's notes in `rows` fully visible rows: the whole pitch
/// range centred when it fits, otherwise centred on the average pitch; middle C for an empty region.
/// Horizontally the view starts at the beat of the first note.
pub fn initial_view(notes: &[Note], rows: i32) -> (i32, f64) {
    let rows = rows.max(1);
    let Some(lo) = notes.iter().map(|n| i32::from(n.pitch)).min() else {
        return ((C4 + rows / 2).clamp(TOP_PITCH_MIN, TOP_PITCH_MAX), 0.0);
    };
    let hi = notes.iter().map(|n| i32::from(n.pitch)).max().unwrap_or(lo);
    let span = hi - lo + 1;
    let top = if span <= rows {
        hi + (rows - span) / 2
    } else {
        let mean = notes.iter().map(|n| f64::from(n.pitch)).sum::<f64>() / notes.len() as f64;
        mean.round() as i32 + rows / 2
    };
    let first = notes.iter().map(|n| n.start).fold(f64::INFINITY, f64::min);
    (top.clamp(TOP_PITCH_MIN, TOP_PITCH_MAX), first.floor().max(0.0))
}

/// Remembers which region the piano roll last framed, so a region is framed when it is first shown
/// and not again on every frame or when the same region is selected again.
#[derive(Debug, Default)]
pub struct ViewFraming {
    framed: Option<Id>,
}

impl ViewFraming {
    /// True the first time `region` is shown since the last different region or `reset`.
    pub fn take(&mut self, region: Id) -> bool {
        if self.framed == Some(region) {
            return false;
        }
        self.framed = Some(region);
        true
    }

    /// Frame again the next time a region is shown (double-click on a region, new or opened project).
    pub fn reset(&mut self) {
        self.framed = None;
    }
}

/// Whether `pitch` is a black key.
pub fn is_black(pitch: i32) -> bool {
    matches!(pitch.rem_euclid(12), 1 | 3 | 6 | 8 | 10)
}

/// The label on a key: the C of every octave ("C3", "C4", "C5"...), when the rows are tall enough to read it.
pub fn key_label(pitch: i32, key_h: f32) -> Option<String> {
    (pitch.rem_euclid(12) == 0 && (0..=127).contains(&pitch) && key_h >= KEY_LABEL_MIN_H).then(|| note_name(pitch as u8))
}

/// Fill of a pitch row: white keys lighter than black ones, and the pitches of the project key a step lighter
/// than the others, so the scale reads at a glance.
pub fn row_fill(pitch: i32, in_key: bool) -> Color32 {
    match (is_black(pitch), in_key) {
        (false, true) => Color32::from_rgb(0x1B, 0x1E, 0x22),
        (false, false) => Color32::from_rgb(0x17, 0x19, 0x1C),
        (true, true) => Color32::from_rgb(0x16, 0x18, 0x1B),
        (true, false) => Color32::from_rgb(0x12, 0x14, 0x17),
    }
}

/// The line under a pitch row: stronger under each C, where an octave starts.
pub fn row_line(pitch: i32) -> Color32 {
    if pitch.rem_euclid(12) == 0 {
        Color32::from_rgb(0x2C, 0x30, 0x36)
    } else {
        theme::LANE_LINE
    }
}

/// Fill and border of a note: the track color with a darker border; selected, lighter with a white border.
pub fn note_colors(track: Color32, selected: bool) -> (Color32, Color32) {
    if selected {
        (track.lerp_to_gamma(Color32::WHITE, 0.55), Color32::WHITE)
    } else {
        (track, theme::darker(track, 0.62))
    }
}

/// Stem and head of a note's velocity: the track color with a lighter head; selected, both close to white.
pub fn velocity_colors(track: Color32, selected: bool) -> (Color32, Color32) {
    if selected {
        (track.lerp_to_gamma(Color32::WHITE, 0.6), Color32::WHITE)
    } else {
        (track, track.lerp_to_gamma(Color32::WHITE, 0.45))
    }
}

/// A vertical line of the note grid.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum GridLine {
    Bar,
    Beat,
    Division,
}

impl GridLine {
    /// Color of the line (all are one point wide; the bars stand out by brightness).
    pub fn color(self) -> Color32 {
        match self {
            GridLine::Bar => Color32::from_rgb(0x34, 0x39, 0x41),
            GridLine::Beat => Color32::from_rgb(0x24, 0x28, 0x2E),
            GridLine::Division => Color32::from_rgb(0x1C, 0x1F, 0x23),
        }
    }
}

/// Beats between two grid lines at a zoom: sixteenths when zoomed in, eighths in between, beats when zoomed
/// out, and only bars when even beats would crowd.
pub fn grid_step(px_per_beat: f32, beats_per_bar: f64) -> f64 {
    if px_per_beat > 160.0 {
        0.25
    } else if px_per_beat > 50.0 {
        0.5
    } else if px_per_beat >= 8.0 {
        1.0
    } else {
        beats_per_bar.max(1.0)
    }
}

/// What kind of line sits at `beat` (beats from the region start, with the region starting `offset` beats into
/// the song, so the bar lines follow the song's bars).
pub fn grid_line(beat: f64, offset: f64, beats_per_bar: f64) -> GridLine {
    let song = beat + offset;
    let on = |x: f64, unit: f64| unit > 0.0 && ((x / unit) - (x / unit).round()).abs() < 1e-6;
    if on(song, beats_per_bar) {
        GridLine::Bar
    } else if on(song, 1.0) {
        GridLine::Beat
    } else {
        GridLine::Division
    }
}

/// Where the piano roll's parts go inside the editor area: keyboard and lane labels on the left, the ruler on
/// top, then the note grid, the velocity lane and the controller lane (`cc_h` tall, header included).
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct PianoRollLayout {
    pub ruler: Rect,
    pub grid: Rect,
    pub keys: Rect,
    pub vel: Rect,
    pub vel_label: Rect,
    pub cc: Rect,
    pub cc_label: Rect,
}

pub fn piano_roll_layout(full: Rect, cc_h: f32) -> PianoRollLayout {
    let (l, k, r) = (full.left(), full.left() + KEYS_W, full.right());
    let grid_top = full.top() + RULER_H;
    let cc_top = (full.bottom() - cc_h).max(grid_top);
    let vel_top = (cc_top - VEL_H).max(grid_top);
    PianoRollLayout {
        ruler: Rect::from_min_max(pos2(k, full.top()), pos2(r, grid_top)),
        grid: Rect::from_min_max(pos2(k, grid_top), pos2(r, vel_top)),
        keys: Rect::from_min_max(pos2(l, grid_top), pos2(k, vel_top)),
        vel: Rect::from_min_max(pos2(k, vel_top), pos2(r, cc_top)),
        vel_label: Rect::from_min_max(pos2(l, vel_top), pos2(k, cc_top)),
        cc: Rect::from_min_max(pos2(k, cc_top), full.max),
        cc_label: Rect::from_min_max(pos2(l, cc_top), pos2(k, full.bottom())),
    }
}

/// Whole pitch rows that fit a grid `grid_h` tall.
pub fn visible_rows(grid_h: f32, key_h: f32) -> i32 {
    if key_h <= 0.0 {
        return 0;
    }
    (grid_h / key_h).floor() as i32
}

/// Position of the playhead inside one iteration of a looped region. `rel` is the playhead minus the
/// region start; outside the repeats it is returned unchanged.
pub fn loop_local_beat(rel: f64, length: f64, loop_count: u32) -> f64 {
    if loop_count > 1 && length > 0.0 && (length..length * f64::from(loop_count)).contains(&rel) {
        rel.rem_euclid(length)
    } else {
        rel
    }
}

/// Index of the pattern step under the playhead, `rel` beats after the region start, in whichever repeat of the
/// region it falls; None before the region, after its last repeat, or with no step length.
pub fn playing_pattern_step(rel: f64, length: f64, loop_count: u32, step_length: f64) -> Option<usize> {
    let local = loop_local_beat(rel, length, loop_count);
    (step_length > 0.0 && local >= 0.0 && local < length).then(|| (local / step_length) as usize)
}

/// New horizontal scroll when the playhead leaves the visible beats (or gets within `margin` beats of
/// the right edge): the view jumps so the playhead sits `margin` beats from the left edge.
pub fn follow_scroll(scroll: f64, beat: f64, visible: f64, margin: f64) -> Option<f64> {
    if beat < scroll || beat > scroll + visible - margin {
        Some((beat - margin).max(0.0))
    } else {
        None
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use eframe::egui::{self, vec2, Modifiers};

    #[test]
    fn wheel_scroll_is_symmetric() {
        // With the old truncation (`as i32`) the eight small upward steps moved 0 rows.
        let (mut top, mut carry) = (84, 0.0);
        for _ in 0..8 {
            (top, carry) = scroll_pitch(top, carry, 3.0, 12.0);
        }
        assert_eq!(top, 86);
        for _ in 0..8 {
            (top, carry) = scroll_pitch(top, carry, -3.0, 12.0);
        }
        assert_eq!(top, 84);
    }

    #[test]
    fn wheel_scroll_stops_at_the_ends_without_debt() {
        let (top, carry) = scroll_pitch(126, 0.0, 60.0, 12.0);
        assert_eq!((top, carry), (127, 0.0));
        assert_eq!(scroll_pitch(top, carry, -12.0, 12.0).0, 126);
        let (top, carry) = scroll_pitch(21, 0.0, -60.0, 12.0);
        assert_eq!((top, carry), (20, 0.0));
        assert_eq!(scroll_pitch(top, carry, 12.0, 12.0).0, 21);
    }

    #[test]
    fn time_zoom_keeps_the_beat_under_the_pointer() {
        let (scroll, ppb, dx) = (8.0_f64, 80.0_f32, 300.0_f32);
        let anchor = scroll + (dx / ppb) as f64;
        for f in [zoom_factor(40.0), zoom_factor(-40.0), 2.0, 0.5] {
            let (s, p) = zoom_time(scroll, ppb, dx, f);
            assert!(((anchor - s) as f32 * p - dx).abs() <= 1.0, "factor {f}: s {s} p {p}");
        }
        let anchor = 8.0 + (300.0_f32 / 590.0) as f64;
        let (s, p) = zoom_time(8.0, 590.0, 300.0, 2.0);
        assert_eq!(p, 600.0);
        assert!(((anchor - s) as f32 * p - 300.0).abs() <= 1.0);
    }

    #[test]
    fn time_zoom_between_uses_the_given_limits() {
        assert_eq!(zoom_time_between(8.0, 300.0, 200.0, 2.0, 2.0, 400.0).1, 400.0);
        assert_eq!(zoom_time_between(8.0, 3.0, 200.0, 0.5, 2.0, 400.0).1, 2.0);
        let (scroll, ppb, dx) = (8.0_f64, 80.0_f32, 300.0_f32);
        let anchor = scroll + (dx / ppb) as f64;
        let (s, p) = zoom_time_between(scroll, ppb, dx, 2.0, 2.0, 400.0);
        assert!(((anchor - s) as f32 * p - dx).abs() <= 1.0, "s {s} p {p}");
    }

    #[test]
    fn time_zoom_in_then_out_returns() {
        let (s1, p1) = zoom_time(8.0, 80.0, 300.0, zoom_factor(40.0));
        let (s2, p2) = zoom_time(s1, p1, 300.0, zoom_factor(-40.0));
        assert!((p2 - 80.0).abs() < 1e-3, "ppb {p2}");
        assert!((s2 - 8.0).abs() < 1e-4, "scroll {s2}");
    }

    #[test]
    fn pitch_zoom_keeps_the_pitch_under_the_pointer() {
        let pitch_at = |top: i32, kh: f32, dy: f32| top - (dy / kh).floor() as i32;
        for dy in [0.0, 5.0, 11.9, 12.0, 100.0, 239.0] {
            for f in [zoom_factor(40.0), zoom_factor(-40.0), 3.0, 0.1] {
                let (t, k) = zoom_pitch(84, 12.0, dy, f);
                assert_eq!(pitch_at(t, k, dy), pitch_at(84, 12.0, dy), "dy {dy} factor {f}");
            }
        }
    }

    #[test]
    fn pitch_zoom_respects_key_height_limits() {
        assert_eq!(zoom_pitch(84, 12.0, 0.0, 10.0).1, 28.0);
        assert_eq!(zoom_pitch(84, 12.0, 0.0, 0.01).1, 6.0);
    }

    #[test]
    fn zoom_factor_is_bounded() {
        assert_eq!(zoom_factor(0.0), 1.0);
        assert_eq!(zoom_factor(1e6), 2.0);
        assert_eq!(zoom_factor(-1e6), 0.5);
        assert!(zoom_factor(40.0) > 1.0);
    }

    fn wheel(unit: MouseWheelUnit, delta: egui::Vec2, modifiers: Modifiers) -> Event {
        Event::MouseWheel { unit, delta, modifiers }
    }

    #[test]
    fn wheel_zoom_reads_ctrl_and_ctrl_shift() {
        let line = |m| wheel(MouseWheelUnit::Line, vec2(0.0, 1.0), m);
        assert_eq!(wheel_zoom(&[line(Modifiers::CTRL)], 40.0, 600.0), WheelZoom { time: 40.0, pitch: 0.0 });
        assert_eq!(
            wheel_zoom(&[line(Modifiers::CTRL | Modifiers::SHIFT)], 40.0, 600.0),
            WheelZoom { time: 0.0, pitch: 40.0 }
        );
        assert_eq!(wheel_zoom(&[line(Modifiers::COMMAND)], 40.0, 600.0), WheelZoom { time: 40.0, pitch: 0.0 });
        assert_eq!(wheel_zoom(&[line(Modifiers::NONE)], 40.0, 600.0), WheelZoom::default());
        assert_eq!(wheel_zoom(&[line(Modifiers::SHIFT)], 40.0, 600.0), WheelZoom::default());
        let point = wheel(MouseWheelUnit::Point, vec2(0.0, 3.0), Modifiers::CTRL);
        assert_eq!(wheel_zoom(&[point], 40.0, 600.0).time, 3.0);
        assert_eq!(wheel_zoom(&[Event::Copy], 40.0, 600.0), WheelZoom::default());
    }

    fn frame(m: Modifiers) -> (egui::Vec2, WheelZoom) {
        let ctx = egui::Context::default();
        let mut raw = egui::RawInput::default();
        raw.events.push(wheel(MouseWheelUnit::Point, vec2(0.0, 3.0), m));
        let mut out = (egui::Vec2::ZERO, WheelZoom::default());
        let _ = ctx.run(raw, |ctx| {
            out = ctx.input(|i| (i.smooth_scroll_delta, wheel_zoom(&i.events, 40.0, 600.0)));
        });
        out
    }

    #[test]
    fn egui_routes_modified_wheel_as_expected() {
        // Fixes the egui 0.29 behaviour the piano roll input and the arrangement rely on.
        assert_eq!(frame(Modifiers::NONE), (vec2(0.0, 3.0), WheelZoom::default()));
        assert_eq!(frame(Modifiers::SHIFT), (vec2(3.0, 0.0), WheelZoom::default()));
        assert_eq!(frame(Modifiers::CTRL), (egui::Vec2::ZERO, WheelZoom { time: 3.0, pitch: 0.0 }));
        assert_eq!(
            frame(Modifiers::CTRL | Modifiers::SHIFT),
            (egui::Vec2::ZERO, WheelZoom { time: 0.0, pitch: 3.0 })
        );
    }

    fn n(pitch: u8, start: f64) -> Note {
        Note { pitch, velocity: 100, start, length: 0.5 }
    }

    #[test]
    fn framing_contains_low_notes() {
        let notes: Vec<Note> =
            [36, 38, 41, 43, 45, 48].iter().enumerate().map(|(i, &p)| n(p, i as f64)).collect();
        for rows in [13, 20, 40] {
            let (top, _) = initial_view(&notes, rows);
            assert!(top >= 48 && top - rows < 36, "rows {rows}: top {top}");
        }
        let notes = vec![n(40, 9.0), n(36, 8.25), n(48, 12.0)];
        assert_eq!(initial_view(&notes, 20).1, 8.0);
    }

    #[test]
    fn framing_empty_region_shows_c4() {
        for rows in [1, 13, 20, 40] {
            let (top, scroll) = initial_view(&[], rows);
            assert!(top >= 60 && top - rows < 60, "rows {rows}: top {top}");
            assert_eq!(scroll, 0.0);
        }
    }

    #[test]
    fn framing_wide_range_centers_on_the_average() {
        let notes = vec![n(24, 0.0), n(24, 1.0), n(24, 2.0), n(100, 3.0)];
        let (top, _) = initial_view(&notes, 20);
        assert!(top >= 43 && top - 19 <= 43, "top {top}");
    }

    #[test]
    fn framing_stays_in_range() {
        assert_eq!(initial_view(&[n(127, 0.0)], 40).0, 127);
        assert_eq!(initial_view(&[n(0, 0.0)], 10).0, 20);
    }

    #[test]
    fn framing_happens_once_per_region() {
        let mut f = ViewFraming::default();
        assert!(f.take(5));
        assert!(!f.take(5));
        assert!(f.take(6));
        assert!(f.take(5));
        assert!(!f.take(5));
        f.reset();
        assert!(f.take(5));
    }

    #[test]
    fn loop_local_beat_wraps_inside_the_repeats() {
        assert_eq!(loop_local_beat(9.5, 4.0, 4), 1.5);
        assert_eq!(loop_local_beat(2.0, 4.0, 4), 2.0);
        assert_eq!(loop_local_beat(17.0, 4.0, 4), 17.0);
        assert_eq!(loop_local_beat(-1.0, 4.0, 4), -1.0);
        assert_eq!(loop_local_beat(5.0, 4.0, 1), 5.0);
        assert_eq!(loop_local_beat(5.0, 0.0, 4), 5.0);
    }

    #[test]
    fn playing_pattern_step_follows_the_repeats() {
        // 4-beat pattern looped 4 times, sixteenth steps: 9.5 beats in is beat 1.5 of the third repeat
        assert_eq!(playing_pattern_step(9.5, 4.0, 4, 0.25), Some(6));
        assert_eq!(playing_pattern_step(0.0, 4.0, 4, 0.25), Some(0));
        assert_eq!(playing_pattern_step(4.0, 4.0, 4, 0.25), Some(0));
        assert_eq!(playing_pattern_step(15.9, 4.0, 4, 0.25), Some(15));
        // after the last repeat and before the start there is no step
        assert_eq!(playing_pattern_step(16.0, 4.0, 4, 0.25), None);
        assert_eq!(playing_pattern_step(-0.1, 4.0, 4, 0.25), None);
        // a region that does not loop works as before
        assert_eq!(playing_pattern_step(3.99, 4.0, 1, 0.25), Some(15));
        assert_eq!(playing_pattern_step(5.0, 4.0, 1, 0.25), None);
        assert_eq!(playing_pattern_step(1.0, 4.0, 4, 0.0), None);
    }

    #[test]
    fn keys_are_black_and_white_like_a_piano() {
        let blacks: Vec<i32> = (60..72).filter(|p| is_black(*p)).collect();
        assert_eq!(blacks, [61, 63, 66, 68, 70]);
        assert!(!is_black(0) && is_black(-11), "works below zero");
    }

    #[test]
    fn every_octave_gets_its_c_label() {
        let labels: Vec<String> = (36..=84).filter_map(|p| key_label(p, DEFAULT_KEY_H)).collect();
        assert_eq!(labels, ["C2", "C3", "C4", "C5", "C6"]);
        assert_eq!(key_label(61, DEFAULT_KEY_H), None);
        assert_eq!(key_label(60, 6.0), None, "rows too short to read a label");
        assert_eq!(key_label(132, DEFAULT_KEY_H), None, "outside MIDI");
    }

    fn lum(c: Color32) -> u32 {
        u32::from(c.r()) + u32::from(c.g()) + u32::from(c.b())
    }

    #[test]
    fn white_rows_are_lighter_than_black_rows() {
        for in_key in [false, true] {
            assert!(lum(row_fill(60, in_key)) > lum(row_fill(61, in_key)), "in key {in_key}");
        }
        assert!(lum(row_fill(62, true)) > lum(row_fill(62, false)), "the scale is lighter");
        assert!(lum(row_fill(61, true)) > lum(row_fill(61, false)), "the scale is lighter");
        assert!(lum(row_line(60)) > lum(row_line(62)), "a stronger line at each C");
    }

    #[test]
    fn selected_notes_are_lighter_with_a_white_border() {
        let track = Color32::from_rgb(0xE0, 0x70, 0x5F);
        let (fill, border) = note_colors(track, false);
        assert_eq!(fill, track);
        assert!(lum(border) < lum(fill), "a darker border");
        let (sfill, sborder) = note_colors(track, true);
        assert!(lum(sfill) > lum(fill));
        assert_eq!(sborder, Color32::WHITE);
        let (stem, head) = velocity_colors(track, false);
        assert_eq!(stem, track);
        assert!(lum(head) > lum(stem), "a lighter head");
        assert_eq!(velocity_colors(track, true).1, Color32::WHITE);
        assert!(lum(KEY_WHITE) > lum(KEY_LINE) && lum(KEY_LINE) > lum(KEY_BLACK), "keys read as a piano");
    }

    #[test]
    fn grid_lines_follow_bars_beats_and_divisions() {
        assert_eq!(grid_line(0.0, 0.0, 4.0), GridLine::Bar);
        assert_eq!(grid_line(1.0, 0.0, 4.0), GridLine::Beat);
        assert_eq!(grid_line(0.5, 0.0, 4.0), GridLine::Division);
        assert_eq!(grid_line(8.0, 0.0, 4.0), GridLine::Bar);
        // a region starting on beat 2 of the song: its beat 2 is the song's bar line
        assert_eq!(grid_line(2.0, 2.0, 4.0), GridLine::Bar);
        assert_eq!(grid_line(0.0, 2.0, 4.0), GridLine::Beat);
        assert_eq!(grid_line(3.0, 0.0, 3.0), GridLine::Bar, "3/4");
        assert!(lum(GridLine::Bar.color()) > lum(GridLine::Beat.color()));
        assert!(lum(GridLine::Beat.color()) > lum(GridLine::Division.color()));
    }

    #[test]
    fn grid_step_thins_out_when_zooming_out() {
        assert_eq!(grid_step(200.0, 4.0), 0.25);
        assert_eq!(grid_step(80.0, 4.0), 0.5);
        assert_eq!(grid_step(20.0, 4.0), 1.0);
        assert_eq!(grid_step(5.0, 4.0), 4.0);
        assert_eq!(grid_step(5.0, 0.0), 1.0);
    }

    #[test]
    fn layout_stacks_ruler_grid_velocity_and_controller_lane() {
        let full = Rect::from_min_size(pos2(10.0, 20.0), vec2(800.0, 300.0));
        let l = piano_roll_layout(full, 80.0);
        assert_eq!(l.ruler.height(), RULER_H);
        assert_eq!(l.grid.top(), l.ruler.bottom());
        assert_eq!(l.vel.top(), l.grid.bottom());
        assert_eq!(l.vel.height(), VEL_H);
        assert_eq!(l.cc.top(), l.vel.bottom());
        assert_eq!(l.cc.bottom(), full.bottom());
        assert_eq!(l.cc.height(), 80.0);
        assert_eq!(l.keys.width(), KEYS_W);
        assert_eq!(l.keys.y_range(), l.grid.y_range());
        assert_eq!(l.grid.left(), full.left() + KEYS_W);
        assert_eq!(l.cc_label.y_range(), l.cc.y_range());
        assert_eq!(l.vel_label.y_range(), l.vel.y_range());
        // too short for everything: the grid shrinks to nothing, never upside down
        let tiny = piano_roll_layout(Rect::from_min_size(full.min, vec2(800.0, 60.0)), 80.0);
        assert!(tiny.grid.height() >= 0.0 && tiny.vel.height() >= 0.0);
    }

    #[test]
    fn default_bottom_panel_shows_fifteen_rows_or_more() {
        use crate::cc_lane;
        use crate::panel_layout::default_bottom;
        // a 1000 px tall window leaves about 880 px between the control bar and the status bar; the editor's
        // header (32) and its margins (8) come off the panel
        let panel = default_bottom(880.0);
        let full = Rect::from_min_size(pos2(0.0, 0.0), vec2(1000.0, panel - 32.0 - 8.0));
        for open in [false, true] {
            let cc_h = cc_lane::lane_height(open, full.height(), RULER_H + VEL_H);
            let rows = visible_rows(piano_roll_layout(full, cc_h).grid.height(), DEFAULT_KEY_H);
            assert!(rows >= 15, "controller lane open {open}: {rows} rows");
        }
        assert_eq!(visible_rows(100.0, 0.0), 0);
    }

    #[test]
    fn follow_scroll_pages_with_the_playhead() {
        assert_eq!(follow_scroll(0.0, 3.0, 8.0, 0.5), None);
        let s = follow_scroll(0.0, 7.8, 8.0, 0.5).expect("pages right");
        assert!((s - 7.3).abs() < 1e-9, "scroll {s}");
        assert_eq!(follow_scroll(8.0, 1.0, 8.0, 0.5), Some(0.5));
        assert_eq!(follow_scroll(8.0, 0.2, 8.0, 0.5), Some(0.0));
    }
}
