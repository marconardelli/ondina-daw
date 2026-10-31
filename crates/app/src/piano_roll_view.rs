//! View geometry of the piano roll: wheel scrolling, pointer-anchored zoom, framing a region's notes and
//! the playhead inside looped regions. Pure functions so they can be tested without a window.

use eframe::egui::{Event, MouseWheelUnit};
use ondina_core::model::{Id, Note};

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
    let anchor = scroll + (pointer_dx / px_per_beat) as f64;
    let ppb = (px_per_beat * factor).clamp(PX_PER_BEAT_MIN, PX_PER_BEAT_MAX);
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
    /// Ctrl+Shift+wheel: vertical (pitch) zoom.
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

/// Position of the playhead inside one iteration of a looped region. `rel` is the playhead minus the
/// region start; outside the repeats it is returned unchanged.
pub fn loop_local_beat(rel: f64, length: f64, loop_count: u32) -> f64 {
    if loop_count > 1 && length > 0.0 && (length..length * f64::from(loop_count)).contains(&rel) {
        rel.rem_euclid(length)
    } else {
        rel
    }
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
        // Fixes the egui 0.29 behaviour the piano roll input relies on.
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
    fn follow_scroll_pages_with_the_playhead() {
        assert_eq!(follow_scroll(0.0, 3.0, 8.0, 0.5), None);
        let s = follow_scroll(0.0, 7.8, 8.0, 0.5).expect("pages right");
        assert!((s - 7.3).abs() < 1e-9, "scroll {s}");
        assert_eq!(follow_scroll(8.0, 1.0, 8.0, 0.5), Some(0.5));
        assert_eq!(follow_scroll(8.0, 0.2, 8.0, 0.5), Some(0.0));
    }
}
