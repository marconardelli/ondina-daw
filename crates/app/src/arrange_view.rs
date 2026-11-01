//! View geometry of the arrangement: zoom limits, track height around the pointer and zoom to fit.
//! Pure functions so they can be tested without a window.

use crate::piano_roll_view::zoom_time_between;

/// Horizontal zoom limits of the arrangement (the range the wheel zoom always allowed).
pub const PX_PER_BEAT_MIN: f32 = 2.0;
pub const PX_PER_BEAT_MAX: f32 = 400.0;
/// Track height limits for Ctrl+Shift+wheel and the View menu.
pub const TRACK_H_MIN: f32 = 32.0;
pub const TRACK_H_MAX: f32 = 160.0;
/// Empty space kept below the last track, where the add-track button sits.
const TRACKS_TAIL_PX: f32 = 80.0;
/// One View-menu step; Zoom Out and Shorter Tracks divide by it, so in then out returns.
const MENU_STEP: f32 = 1.25;
/// Space left after the end of the song by Zoom to Fit.
const FIT_MARGIN_PX: f32 = 16.0;
/// Zoom to Fit shows at least this many bars, so a short or empty song is not blown up.
const FIT_MIN_BARS: f64 = 4.0;

/// One step from the View menu.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ViewStep {
    ZoomIn,
    ZoomOut,
    Fit,
    Taller,
    Shorter,
}

/// What the arrangement shows: the four values live in `OndinaApp`.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct ArrangeView {
    pub scroll_beat: f64,
    pub px_per_beat: f32,
    pub scroll_y: f32,
    pub track_height: f32,
}

/// The part of the window the view is drawn in and what it shows.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct ViewArea {
    /// Width of the timeline (right of the track headers) and height of the track area, in points.
    pub width: f32,
    pub height: f32,
    pub tracks: usize,
    pub content_end: f64,
    pub beats_per_bar: f64,
    pub playhead: f64,
}

/// Time zoom around the pointer with the arrangement's limits.
pub fn zoom_time(scroll: f64, px_per_beat: f32, pointer_dx: f32, factor: f32) -> (f64, f32) {
    zoom_time_between(scroll, px_per_beat, pointer_dx, factor, PX_PER_BEAT_MIN, PX_PER_BEAT_MAX)
}

/// Furthest the tracks can scroll down: the last track plus the empty tail stay reachable.
pub fn max_scroll_y(tracks: usize, track_h: f32, view_h: f32) -> f32 {
    (tracks as f32 * track_h + TRACKS_TAIL_PX - view_h).max(0.0)
}

/// Scroll that shows the whole row of track `index`: unchanged when it is already in view, otherwise the least
/// movement that brings it in. A row as tall as the view or taller lines up with the top.
pub fn scroll_to_track(scroll_y: f32, track_h: f32, index: usize, view_h: f32) -> f32 {
    let top = index as f32 * track_h;
    let bottom = top + track_h;
    if top < scroll_y || track_h >= view_h {
        top
    } else if bottom > scroll_y + view_h {
        bottom - view_h
    } else {
        scroll_y
    }
}

/// Scales the track height around the pointer (`pointer_dy` points below the top of the track area):
/// the track under the pointer, and the same point inside it, stays there. When that would scroll past
/// the content (few tracks, pointer below the last one) the scroll limit wins.
/// Returns the new `(scroll_y, track_height)`.
pub fn zoom_track_height(scroll_y: f32, track_h: f32, pointer_dy: f32, factor: f32, tracks: usize, view_h: f32) -> (f32, f32) {
    let th = (track_h * factor).clamp(TRACK_H_MIN, TRACK_H_MAX);
    // Fractional track index under the pointer.
    let at = (scroll_y + pointer_dy) / track_h;
    ((at * th - pointer_dy).clamp(0.0, max_scroll_y(tracks, th, view_h)), th)
}

/// Scroll and zoom that show the song from beat 0 to `content_end` (at least a few bars) in `width`
/// points. A very long song stops at the minimum zoom and does not fit entirely.
pub fn zoom_to_fit(content_end: f64, beats_per_bar: f64, width: f32) -> (f64, f32) {
    let span = content_end.max(beats_per_bar * FIT_MIN_BARS).max(1.0);
    let usable = (width - FIT_MARGIN_PX).max(1.0);
    (0.0, (usable / span as f32).clamp(PX_PER_BEAT_MIN, PX_PER_BEAT_MAX))
}

/// Applies a View-menu step. Zooming keeps the playhead in place when it is visible, otherwise the
/// middle of the view; the track height keeps the top track at the top.
pub fn apply_step(step: ViewStep, v: ArrangeView, a: &ViewArea) -> ArrangeView {
    let mut out = v;
    match step {
        ViewStep::ZoomIn | ViewStep::ZoomOut => {
            let playhead_dx = ((a.playhead - v.scroll_beat) as f32) * v.px_per_beat;
            let dx = if (0.0..=a.width).contains(&playhead_dx) { playhead_dx } else { a.width / 2.0 };
            let factor = if step == ViewStep::ZoomIn { MENU_STEP } else { 1.0 / MENU_STEP };
            (out.scroll_beat, out.px_per_beat) = zoom_time(v.scroll_beat, v.px_per_beat, dx, factor);
        }
        ViewStep::Fit => {
            (out.scroll_beat, out.px_per_beat) = zoom_to_fit(a.content_end, a.beats_per_bar, a.width);
        }
        ViewStep::Taller | ViewStep::Shorter => {
            let factor = if step == ViewStep::Taller { MENU_STEP } else { 1.0 / MENU_STEP };
            (out.scroll_y, out.track_height) =
                zoom_track_height(v.scroll_y, v.track_height, 0.0, factor, a.tracks, a.height);
        }
    }
    out
}

/// Whether a View-menu step would change anything (false once its limit is reached).
pub fn can_step(step: ViewStep, v: ArrangeView) -> bool {
    match step {
        ViewStep::ZoomIn => v.px_per_beat < PX_PER_BEAT_MAX,
        ViewStep::ZoomOut => v.px_per_beat > PX_PER_BEAT_MIN,
        ViewStep::Fit => true,
        ViewStep::Taller => v.track_height < TRACK_H_MAX,
        ViewStep::Shorter => v.track_height > TRACK_H_MIN,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::piano_roll_view::zoom_factor;

    fn view(scroll_beat: f64, px_per_beat: f32, scroll_y: f32, track_height: f32) -> ArrangeView {
        ArrangeView { scroll_beat, px_per_beat, scroll_y, track_height }
    }

    fn area(width: f32, tracks: usize, playhead: f64) -> ViewArea {
        ViewArea { width, height: 400.0, tracks, content_end: 64.0, beats_per_bar: 4.0, playhead }
    }

    #[test]
    fn max_scroll_y_matches_the_old_limit() {
        assert_eq!(max_scroll_y(10, 58.0, 300.0), 360.0);
        assert_eq!(max_scroll_y(2, 58.0, 300.0), 0.0);
        assert_eq!(max_scroll_y(0, 58.0, 300.0), 0.0);
    }

    #[test]
    fn track_height_zoom_keeps_the_track_under_the_pointer() {
        for f in [zoom_factor(40.0), zoom_factor(-40.0), 1.5, 0.7] {
            let (s, h) = zoom_track_height(100.0, 58.0, 150.0, f, 20, 400.0);
            assert!(s > 0.0 && s < max_scroll_y(20, h, 400.0), "factor {f}: scroll {s} hit a limit");
            assert!(((s + 150.0) / h - (100.0 + 150.0) / 58.0).abs() < 1e-3, "factor {f}: s {s} h {h}");
        }
    }

    #[test]
    fn track_height_zoom_respects_the_limits() {
        assert_eq!(zoom_track_height(0.0, 58.0, 0.0, 10.0, 20, 400.0).1, TRACK_H_MAX);
        assert_eq!(zoom_track_height(0.0, 58.0, 0.0, 0.01, 20, 400.0).1, TRACK_H_MIN);
    }

    #[test]
    fn track_height_zoom_keeps_the_scroll_inside_the_content() {
        assert_eq!(zoom_track_height(0.0, 58.0, 390.0, 0.5, 3, 400.0).0, 0.0);
        let bottom = max_scroll_y(50, 58.0, 400.0);
        for f in [0.5, 2.0] {
            let (s, h) = zoom_track_height(bottom, 58.0, 0.0, f, 50, 400.0);
            assert!((0.0..=max_scroll_y(50, h, 400.0)).contains(&s), "factor {f}: s {s} h {h}");
        }
    }

    #[test]
    fn arrange_time_zoom_keeps_the_beat_under_the_pointer() {
        let (scroll, ppb, dx) = (16.0_f64, 28.0_f32, 400.0_f32);
        let anchor = scroll + (dx / ppb) as f64;
        for f in [zoom_factor(40.0), zoom_factor(-40.0), 2.0, 0.5] {
            let (s, p) = zoom_time(scroll, ppb, dx, f);
            assert!(((anchor - s) as f32 * p - dx).abs() <= 1.0, "factor {f}: s {s} p {p}");
        }
    }

    #[test]
    fn arrange_time_zoom_uses_the_arrange_limits() {
        assert_eq!(zoom_time(8.0, 300.0, 200.0, 2.0).1, 400.0);
        // the piano roll stops at 10
        assert_eq!(zoom_time(8.0, 3.0, 200.0, 0.5).1, 2.0);
    }

    #[test]
    fn zoom_to_fit_shows_the_whole_song() {
        let (s, ppb) = zoom_to_fit(128.0, 4.0, 1000.0);
        assert_eq!(s, 0.0);
        let shown = 128.0 * ppb as f64;
        assert!((983.0..=984.0 + 1e-3).contains(&shown), "song {shown} px");
        assert!((zoom_to_fit(0.0, 4.0, 1000.0).1 - 984.0 / 16.0).abs() < 1e-3);
        let short = zoom_to_fit(2.0, 4.0, 2000.0).1;
        assert!((short - 1984.0 / 16.0).abs() < 1e-3 && short <= PX_PER_BEAT_MAX, "ppb {short}");
        assert_eq!(zoom_to_fit(10000.0, 4.0, 500.0).1, PX_PER_BEAT_MIN);
    }

    #[test]
    fn menu_fit_frames_the_song() {
        let v = view(40.0, 28.0, 116.0, 58.0);
        let out = apply_step(ViewStep::Fit, v, &area(1000.0, 20, 0.0));
        assert_eq!(out.scroll_beat, 0.0);
        assert_eq!((out.scroll_y, out.track_height), (116.0, 58.0));
        assert!((64.0 * out.px_per_beat - 984.0).abs() < 1e-3, "ppb {}", out.px_per_beat);
    }

    #[test]
    fn menu_zoom_anchors_on_the_visible_playhead() {
        let v = view(8.0, 20.0, 0.0, 58.0);
        let out = apply_step(ViewStep::ZoomIn, v, &area(800.0, 4, 18.0));
        assert!(out.px_per_beat > 20.0);
        assert!((((18.0 - out.scroll_beat) as f32) * out.px_per_beat - 200.0).abs() <= 1.0, "{out:?}");
        // playhead out of view: the beat in the middle stays in the middle
        let out = apply_step(ViewStep::ZoomIn, v, &area(800.0, 4, 100.0));
        assert!((((28.0 - out.scroll_beat) as f32) * out.px_per_beat - 400.0).abs() <= 1.0, "{out:?}");
    }

    #[test]
    fn menu_zoom_in_then_out_returns() {
        let v = view(8.0, 20.0, 0.0, 58.0);
        let a = area(800.0, 4, 100.0);
        let back = apply_step(ViewStep::ZoomOut, apply_step(ViewStep::ZoomIn, v, &a), &a);
        assert!((back.px_per_beat - 20.0).abs() < 1e-3, "ppb {}", back.px_per_beat);
        assert!((back.scroll_beat - 8.0).abs() < 1e-4, "scroll {}", back.scroll_beat);
    }

    #[test]
    fn menu_taller_keeps_the_top_track() {
        let v = view(0.0, 28.0, 116.0, 58.0);
        let a = area(800.0, 20, 0.0);
        for step in [ViewStep::Taller, ViewStep::Shorter] {
            let out = apply_step(step, v, &a);
            assert!((out.track_height - 58.0).abs() > 1.0, "{step:?}");
            assert!((out.scroll_y / out.track_height - 2.0).abs() < 1e-4, "{step:?}: {out:?}");
        }
    }

    #[test]
    fn menu_steps_stop_at_the_limits() {
        assert!(!can_step(ViewStep::ZoomIn, view(0.0, PX_PER_BEAT_MAX, 0.0, 58.0)));
        assert!(!can_step(ViewStep::ZoomOut, view(0.0, PX_PER_BEAT_MIN, 0.0, 58.0)));
        assert!(!can_step(ViewStep::Taller, view(0.0, 28.0, 0.0, TRACK_H_MAX)));
        assert!(!can_step(ViewStep::Shorter, view(0.0, 28.0, 0.0, TRACK_H_MIN)));
        assert!(can_step(ViewStep::Fit, view(0.0, PX_PER_BEAT_MAX, 0.0, TRACK_H_MAX)));
        let default = view(0.0, 28.0, 0.0, 58.0);
        for step in [ViewStep::ZoomIn, ViewStep::ZoomOut, ViewStep::Fit, ViewStep::Taller, ViewStep::Shorter] {
            assert!(can_step(step, default), "{step:?}");
        }
    }

    #[test]
    fn scroll_to_track_leaves_a_visible_track_alone() {
        assert_eq!(scroll_to_track(58.0, 58.0, 2, 300.0), 58.0);
    }

    #[test]
    fn scroll_to_track_scrolls_up_to_a_track_above() {
        assert_eq!(scroll_to_track(200.0, 58.0, 1, 300.0), 58.0);
    }

    #[test]
    fn scroll_to_track_scrolls_down_just_enough() {
        assert_eq!(scroll_to_track(0.0, 58.0, 6, 300.0), 7.0 * 58.0 - 300.0);
    }

    #[test]
    fn scroll_to_track_puts_a_tall_track_at_the_top() {
        assert_eq!(scroll_to_track(0.0, 160.0, 3, 100.0), 480.0);
    }
}
