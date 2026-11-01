//! Renaming, recoloring and fading a region from the arrange window and the inspector. The commands are built by
//! pure functions so they can be tested without a window.

use eframe::egui::{self, pos2, vec2, Color32, Pos2, Rect, Rounding, Shape, Stroke};
use ondina_core::command::Command;
use ondina_core::model::{Id, Region, RegionContent};

use crate::arrange::rename_commit;
use crate::theme;

/// Region colors offered by the arrange menu and the inspector: the ten track colors first, so a region can
/// match another track, then two quieter ones. All light enough for the black name in the region header.
pub const REGION_COLORS: [(&str, [u8; 3]); 12] = [
    ("Blue", [86, 156, 214]),
    ("Green", [106, 190, 120]),
    ("Amber", [230, 170, 70]),
    ("Red", [214, 92, 92]),
    ("Purple", [170, 120, 220]),
    ("Teal", [80, 190, 190]),
    ("Pink", [220, 120, 170]),
    ("Olive", [180, 180, 90]),
    ("Indigo", [120, 140, 230]),
    ("Orange", [230, 130, 80]),
    ("Sand", [200, 170, 130]),
    ("Grey", [150, 155, 165]),
];

/// Height of the in-place name field over a region, and its minimum width (the same as `rename_field`'s).
pub const NAME_FIELD_H: f32 = 16.0;
pub const NAME_FIELD_MIN_W: f32 = 120.0;

/// Side of a fade handle's grab square, in points; the square sits on the top edge of the region.
pub const FADE_HANDLE_PX: f32 = 10.0;
/// A repetition narrower than this shows no fade handles, so its right edge stays free for resizing.
pub const FADE_MIN_REP_PX: f32 = 24.0;
/// The fade grid is never coarser than a beat (the "Bar" snap would only allow fades of whole bars), as in the
/// piano roll.
const FADE_MAX_GRID: f64 = 1.0;
/// Veil over the part of the waveform a fade takes away.
const FADE_VEIL: Color32 = Color32::from_black_alpha(110);

/// Which fade of an audio region a handle changes.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum FadeHandle {
    In,
    Out,
}

/// Where an audio region's fades are on screen.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct FadeGeom {
    /// The first repetition, top to bottom of the region.
    pub first: Rect,
    /// How many repetitions the region plays (at least 1); each one has the same fades.
    pub reps: u32,
    pub fade_in_px: f32,
    pub fade_out_px: f32,
}

/// What the user picked in the color selector.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ColorChoice {
    Color([u8; 3]),
    /// Drop the region's own color: it shows its track's color again.
    Track,
}

/// `SetRegionProperties` changing only the name and/or the color.
fn region_props(region_id: Id, name: Option<String>, color: Option<String>) -> Command {
    Command::SetRegionProperties {
        region_id,
        name,
        muted: None,
        loop_count: None,
        transpose: None,
        velocity_offset: None,
        gain_db: None,
        fade_in: None,
        fade_out: None,
        reverse: None,
        color,
    }
}

/// The command for a typed region name, or None when it is empty or unchanged.
pub fn name_command(region_id: Id, old: &str, typed: &str) -> Option<Command> {
    rename_commit(old, typed).map(|n| region_props(region_id, Some(n), None))
}

/// The command for a color choice, or None when it would change nothing.
pub fn color_command(region_id: Id, current: Option<[u8; 3]>, choice: ColorChoice) -> Option<Command> {
    match choice {
        ColorChoice::Color(c) if current != Some(c) => {
            Some(region_props(region_id, None, Some(format!("#{:02X}{:02X}{:02X}", c[0], c[1], c[2]))))
        }
        // an empty color removes the region's own color (see SetRegionProperties)
        ColorChoice::Track if current.is_some() => Some(region_props(region_id, None, Some(String::new()))),
        _ => None,
    }
}

/// Where the in-place name field of a region goes: over the top of the region's visible part, at least
/// NAME_FIELD_MIN_W wide and kept inside `area` (the tracks area right of the headers). None when less than
/// NAME_FIELD_H of the region is visible: the edit is dropped then, as for markers and tracks.
pub fn region_name_field(region: Rect, area: Rect) -> Option<Rect> {
    let vis = region.intersect(area);
    if vis.width() <= 0.0 || vis.height() < NAME_FIELD_H {
        return None;
    }
    let w = vis.width().max(NAME_FIELD_MIN_W).min(area.width());
    let x = vis.left().min(area.right() - w).max(area.left());
    Some(Rect::from_min_size(pos2(x, vis.top()), vec2(w, NAME_FIELD_H)))
}

/// Fade in and fade out of an audio region, in beats; None for MIDI and pattern regions, which have no fades.
pub fn audio_fades(r: &Region) -> Option<(f64, f64)> {
    match &r.content {
        RegionContent::Audio(a) => Some((a.fade_in, a.fade_out)),
        _ => None,
    }
}

/// The grab squares of the fade handles, `[fade in, fade out]`: on the top edge of the first repetition, centered
/// where the fade in ends and where the fade out begins, kept inside the repetition. None when the repetition is
/// narrower than FADE_MIN_REP_PX.
pub fn fade_handles(g: &FadeGeom) -> Option<[Rect; 2]> {
    let rep = g.first;
    if rep.width() < FADE_MIN_REP_PX {
        return None;
    }
    let half = FADE_HANDLE_PX / 2.0;
    let at = |cx: f32| {
        let cx = cx.clamp(rep.left() + half, rep.right() - half);
        Rect::from_min_max(pos2(cx - half, rep.top()), pos2(cx + half, rep.top() + FADE_HANDLE_PX))
    };
    Some([at(rep.left() + g.fade_in_px), at(rep.right() - g.fade_out_px)])
}

/// The handle under `p`. When the two squares overlap (the fades touch) the one whose center is nearer in x wins,
/// the fade in on a tie.
pub fn fade_handle_at(p: Pos2, handles: [Rect; 2]) -> Option<FadeHandle> {
    let [hi, ho] = handles;
    match (hi.contains(p), ho.contains(p)) {
        (true, true) => {
            if (p.x - hi.center().x).abs() <= (p.x - ho.center().x).abs() {
                Some(FadeHandle::In)
            } else {
                Some(FadeHandle::Out)
            }
        }
        (true, false) => Some(FadeHandle::In),
        (false, true) => Some(FadeHandle::Out),
        (false, false) => None,
    }
}

/// The fade after moving its handle by `delta` beats from where the drag began. `orig` is the fade when the drag
/// began, `length` the length of one repetition, `other` the other fade and `snap` the grid in beats (0 = free;
/// the caller passes 0 while Alt is held). The fade stays between 0 and what the other fade leaves of the
/// repetition.
pub fn fade_from_drag(handle: FadeHandle, orig: f64, delta: f64, length: f64, other: f64, snap: f64) -> f64 {
    // the fade-out handle sits at the start of the fade: dragging it left makes the fade longer
    let raw = match handle {
        FadeHandle::In => orig + delta,
        FadeHandle::Out => orig - delta,
    };
    let grid = snap.min(FADE_MAX_GRID);
    let v = if grid > 0.0 { (raw / grid).round() * grid } else { raw };
    v.clamp(0.0, (length - other).max(0.0))
}

/// `SetRegionProperties` changing only one fade, or None when the value did not change.
pub fn fade_command(region_id: Id, handle: FadeHandle, old: f64, new: f64) -> Option<Command> {
    if (new - old).abs() <= 1e-9 {
        return None;
    }
    let (fade_in, fade_out) = match handle {
        FadeHandle::In => (Some(new), None),
        FadeHandle::Out => (None, Some(new)),
    };
    Some(Command::SetRegionProperties {
        region_id,
        name: None,
        muted: None,
        loop_count: None,
        transpose: None,
        velocity_offset: None,
        gain_db: None,
        fade_in,
        fade_out,
        reverse: None,
        color: None,
    })
}

/// "Fade in: 1.50 beats", for the tooltips of the handles.
pub fn fade_label(handle: FadeHandle, beats: f64) -> String {
    match handle {
        FadeHandle::In => format!("Fade in: {beats:.2} beats"),
        FadeHandle::Out => format!("Fade out: {beats:.2} beats"),
    }
}

/// The linear fade curves over every visible repetition, with a veil on what each fade takes away. The side being
/// dragged (`active`) is drawn in the accent color. A fade longer than the repetition is cut at its edge, as the
/// gain is.
pub fn paint_fades(painter: &egui::Painter, g: &FadeGeom, active: Option<FadeHandle>) {
    let w = g.first.width();
    if w <= 0.0 || (g.fade_in_px <= 0.0 && g.fade_out_px <= 0.0) {
        return;
    }
    let clip = painter.clip_rect();
    let line = |h: FadeHandle| {
        let c = if active == Some(h) { theme::ACCENT } else { Color32::from_white_alpha(190) };
        Stroke::new(1.0_f32, c)
    };
    for k in 0..g.reps.max(1) {
        let rep = g.first.translate(vec2(k as f32 * w, 0.0));
        if rep.right() < clip.left() {
            continue;
        }
        if rep.left() > clip.right() {
            break;
        }
        let p = painter.with_clip_rect(rep.intersect(clip));
        let (l, r, top, bottom) = (rep.left(), rep.right(), rep.top(), rep.bottom());
        if g.fade_in_px > 0.0 {
            let e = l + g.fade_in_px;
            p.add(Shape::convex_polygon(vec![pos2(l, top), pos2(e, top), pos2(l, bottom)], FADE_VEIL, Stroke::NONE));
            p.line_segment([pos2(l, bottom), pos2(e, top)], line(FadeHandle::In));
        }
        if g.fade_out_px > 0.0 {
            let s = r - g.fade_out_px;
            p.add(Shape::convex_polygon(vec![pos2(s, top), pos2(r, top), pos2(r, bottom)], FADE_VEIL, Stroke::NONE));
            p.line_segment([pos2(s, top), pos2(r, bottom)], line(FadeHandle::Out));
        }
    }
}

/// The two grab squares; the `hot` one (under the pointer or being dragged) in the accent color.
pub fn paint_fade_handles(painter: &egui::Painter, handles: [Rect; 2], hot: Option<FadeHandle>) {
    for (rect, h) in handles.into_iter().zip([FadeHandle::In, FadeHandle::Out]) {
        let fill = if hot == Some(h) { theme::ACCENT } else { Color32::from_gray(235) };
        let rect = rect.shrink(1.5);
        painter.rect_filled(rect, Rounding::same(1.0_f32), fill);
        painter.rect_stroke(rect, Rounding::same(1.0_f32), Stroke::new(1.0_f32, Color32::from_black_alpha(200)));
    }
}

/// Two rows of swatches and a "Track color" button; the current color has a white frame. Returns what was
/// clicked this frame.
pub fn color_swatches(ui: &mut egui::Ui, current: Option<[u8; 3]>) -> Option<ColorChoice> {
    let mut picked = None;
    for row in REGION_COLORS.chunks(6) {
        ui.horizontal(|ui| {
            for &(name, c) in row {
                let stroke = if current == Some(c) {
                    Stroke::new(2.0_f32, theme::SELECT)
                } else {
                    Stroke::new(1.0_f32, Color32::from_black_alpha(160))
                };
                let b = egui::Button::new("").fill(theme::rgb(c)).stroke(stroke).min_size(vec2(18.0, 18.0));
                if ui.add(b).on_hover_text(name).clicked() {
                    picked = Some(ColorChoice::Color(c));
                }
            }
        });
    }
    if ui
        .add_enabled(current.is_some(), egui::Button::new("Track color"))
        .on_hover_text("Use the track's color again")
        .clicked()
    {
        picked = Some(ColorChoice::Track);
    }
    picked
}

#[cfg(test)]
mod tests {
    use super::*;
    use ondina_core::model::TRACK_COLORS;
    use ondina_studio::{Session, Source};
    use serde_json::{json, Value};

    /// (region_id, name, color) of a SetRegionProperties that changes nothing else.
    fn props(c: &Command) -> (Id, Option<String>, Option<String>) {
        match c {
            Command::SetRegionProperties {
                region_id,
                name,
                muted,
                loop_count,
                transpose,
                velocity_offset,
                gain_db,
                fade_in,
                fade_out,
                reverse,
                color,
            } => {
                assert!(muted.is_none() && loop_count.is_none() && transpose.is_none() && velocity_offset.is_none());
                assert!(gain_db.is_none() && fade_in.is_none() && fade_out.is_none() && reverse.is_none());
                (*region_id, name.clone(), color.clone())
            }
            other => panic!("unexpected command {other:?}"),
        }
    }

    #[test]
    fn name_command_trims_and_ignores_empty_or_unchanged() {
        let c = name_command(7, "Keys", "  Verse 1 ").unwrap();
        assert_eq!(props(&c), (7, Some("Verse 1".to_string()), None));
        for typed in ["Keys", " Keys ", "", "   "] {
            assert!(name_command(7, "Keys", typed).is_none(), "{typed:?}");
        }
        let c = name_command(7, "Keys", "keys").unwrap();
        assert_eq!(props(&c).1.as_deref(), Some("keys"));
    }

    #[test]
    fn color_command_sets_clears_and_skips_no_ops() {
        let sand = [200, 170, 130];
        let c = color_command(3, None, ColorChoice::Color(sand)).unwrap();
        assert_eq!(props(&c), (3, None, Some("#C8AA82".to_string())));
        assert!(color_command(3, Some(sand), ColorChoice::Color(sand)).is_none());
        let c = color_command(3, Some(sand), ColorChoice::Color([86, 156, 214])).unwrap();
        assert_eq!(props(&c), (3, None, Some("#569CD6".to_string())));
        let c = color_command(3, Some(sand), ColorChoice::Track).unwrap();
        assert_eq!(props(&c), (3, None, Some(String::new())));
        assert!(color_command(3, None, ColorChoice::Track).is_none());
    }

    #[test]
    fn palette_has_twelve_distinct_readable_colors() {
        for (i, c) in TRACK_COLORS.iter().enumerate() {
            assert_eq!(REGION_COLORS[i].1, *c, "color {i}");
        }
        for (i, (na, ca)) in REGION_COLORS.iter().enumerate() {
            for (nb, cb) in &REGION_COLORS[i + 1..] {
                assert_ne!(na, nb);
                assert_ne!(ca, cb, "{na} and {nb}");
            }
            let luma = 0.299 * ca[0] as f32 + 0.587 * ca[1] as f32 + 0.114 * ca[2] as f32;
            assert!(luma >= 110.0, "{na} is too dark for the black region name: {luma}");
        }
    }

    #[test]
    fn region_name_field_follows_the_visible_part() {
        let area = Rect::from_min_max(pos2(210.0, 50.0), pos2(1000.0, 600.0));
        let rect = |x0: f32, y0: f32, x1: f32, y1: f32| Rect::from_min_max(pos2(x0, y0), pos2(x1, y1));
        assert_eq!(region_name_field(rect(300.0, 100.0, 500.0, 128.0), area), Some(rect(300.0, 100.0, 500.0, 116.0)));
        // short regions get the minimum width, kept inside the area at the right edge
        let f = region_name_field(rect(300.0, 100.0, 330.0, 128.0), area).unwrap();
        assert_eq!((f.left(), f.width()), (300.0, 120.0));
        let f = region_name_field(rect(950.0, 100.0, 980.0, 128.0), area).unwrap();
        assert_eq!((f.left(), f.width()), (880.0, 120.0));
        // a region starting under the headers only covers its visible part
        let f = region_name_field(rect(100.0, 100.0, 400.0, 128.0), area).unwrap();
        assert_eq!((f.left(), f.width()), (210.0, 190.0));
        // the header scrolled above the area, 20 px of the body still visible
        let f = region_name_field(rect(300.0, 30.0, 500.0, 70.0), area).unwrap();
        assert_eq!(f.top(), 50.0);
        // too little visible, or out of view
        assert_eq!(region_name_field(rect(300.0, 40.0, 500.0, 60.0), area), None);
        assert_eq!(region_name_field(rect(0.0, 100.0, 200.0, 128.0), area), None);
        assert_eq!(region_name_field(rect(1100.0, 100.0, 1200.0, 128.0), area), None);
        assert_eq!(region_name_field(rect(300.0, 700.0, 500.0, 728.0), area), None);
    }

    fn run(s: &mut Session, name: &str, args: Value) -> Value {
        let c = Command::from_tool_call(name, &args).unwrap();
        s.execute(c, Source::Gui).unwrap_or_else(|e| panic!("{name} {args} failed: {e}"))
    }

    #[test]
    fn region_name_and_color_edits_are_one_undo_step_each() {
        let mut s = Session::new(false);
        let tid = run(&mut s, "add_track", json!({"kind": "instrument", "name": "Keys"}))["track_id"].as_u64().unwrap();
        let rid = run(&mut s, "create_midi_region", json!({"track_id": tid, "start": 0, "length": 4}))["region_id"]
            .as_u64()
            .unwrap();
        let state = |s: &Session| {
            let r = s.project.find_region(rid).unwrap().1;
            (r.name.clone(), r.color)
        };
        let amber = REGION_COLORS[2].1;
        assert_eq!(state(&s), ("Keys".to_string(), None));

        s.execute(name_command(rid, "Keys", " Verse 1 ").unwrap(), Source::Gui).unwrap();
        assert_eq!(state(&s), ("Verse 1".to_string(), None));

        s.execute(color_command(rid, None, ColorChoice::Color(amber)).unwrap(), Source::Gui).unwrap();
        assert_eq!(state(&s), ("Verse 1".to_string(), Some([230, 170, 70])));
        assert!(color_command(rid, Some(amber), ColorChoice::Color(amber)).is_none());

        s.execute(color_command(rid, Some(amber), ColorChoice::Track).unwrap(), Source::Gui).unwrap();
        assert_eq!(state(&s), ("Verse 1".to_string(), None));
        assert!(color_command(rid, None, ColorChoice::Track).is_none());

        run(&mut s, "undo", json!({}));
        assert_eq!(state(&s), ("Verse 1".to_string(), Some(amber)));
        run(&mut s, "undo", json!({}));
        assert_eq!(state(&s), ("Verse 1".to_string(), None));
        run(&mut s, "undo", json!({}));
        assert_eq!(state(&s), ("Keys".to_string(), None));
    }

    #[test]
    fn fade_drag_stays_between_zero_and_the_region_length() {
        use FadeHandle::{In, Out};
        assert_eq!(fade_from_drag(In, 0.0, -1.0, 4.0, 0.0, 0.0), 0.0);
        assert_eq!(fade_from_drag(Out, 0.5, 2.0, 4.0, 0.0, 0.0), 0.0);
        assert_eq!(fade_from_drag(In, 0.0, 3.0, 4.0, 2.0, 0.0), 2.0);
        assert_eq!(fade_from_drag(Out, 1.0, -5.0, 4.0, 0.5, 0.0), 3.5);
        // the other fade already takes more than the repetition
        assert_eq!(fade_from_drag(In, 0.0, 1.0, 4.0, 6.0, 0.0), 0.0);
        for handle in [In, Out] {
            for orig in [0.0, 0.5, 2.0] {
                for delta in [-6.0, -1.3, 0.0, 0.7, 2.2, 9.0] {
                    for other in [0.0, 1.0, 3.5] {
                        for snap in [0.0, 0.25, 1.0, 4.0] {
                            let v = fade_from_drag(handle, orig, delta, 4.0, other, snap);
                            assert!(v >= 0.0, "{handle:?} {orig} {delta} {other} {snap}: {v}");
                            assert!(v + other <= 4.0 + 1e-9, "{handle:?} {orig} {delta} {other} {snap}: {v}");
                        }
                    }
                }
            }
        }
    }

    #[test]
    fn fade_drag_snaps_to_the_grid_unless_free() {
        use FadeHandle::{In, Out};
        assert_eq!(fade_from_drag(In, 0.0, 1.4, 8.0, 0.0, 1.0), 1.0);
        assert_eq!(fade_from_drag(In, 0.0, 1.6, 8.0, 0.0, 1.0), 2.0);
        assert_eq!(fade_from_drag(In, 0.0, 1.3, 8.0, 0.0, 0.25), 1.25);
        assert_eq!(fade_from_drag(Out, 1.0, -0.9, 8.0, 0.0, 0.5), 2.0);
        // the "Bar" snap uses a grid of one beat
        assert_eq!(fade_from_drag(In, 0.0, 2.6, 8.0, 0.0, 4.0), 3.0);
        // without snap (or with Alt) the fade follows the drag
        assert!((fade_from_drag(In, 0.0, 1.37, 8.0, 0.0, 0.0) - 1.37).abs() < 1e-9);
        // the length limit wins over the grid
        assert_eq!(fade_from_drag(In, 0.0, 3.6, 4.0, 0.5, 1.0), 3.5);
    }

    /// Two seconds of quiet audio in the temp folder: 4 beats at 120 bpm.
    fn test_wav(name: &str) -> std::path::PathBuf {
        let dir = std::env::temp_dir().join("ondina_app_tests");
        std::fs::create_dir_all(&dir).unwrap();
        let wav = dir.join(name);
        ondina_engine::write_wav(&wav, &vec![0.1; 88200], &vec![0.1; 88200], 44100, 16).unwrap();
        wav
    }

    #[test]
    fn only_audio_regions_have_fades() {
        let mut s = Session::new(false);
        let tid = run(&mut s, "add_track", json!({"kind": "instrument", "name": "Keys"}))["track_id"].as_u64().unwrap();
        let midi = run(&mut s, "create_midi_region", json!({"track_id": tid, "start": 0, "length": 4}))["region_id"]
            .as_u64()
            .unwrap();
        let pat = run(&mut s, "create_pattern_region", json!({"track_id": tid, "start": 8}))["region_id"]
            .as_u64()
            .unwrap();
        assert_eq!(audio_fades(s.project.find_region(midi).unwrap().1), None);
        assert_eq!(audio_fades(s.project.find_region(pat).unwrap().1), None);
        let wav = test_wav("fade_regions.wav");
        let aud = run(&mut s, "import_audio", json!({"path": wav.display().to_string(), "start": 0}))["region_id"]
            .as_u64()
            .unwrap();
        assert_eq!(audio_fades(s.project.find_region(aud).unwrap().1), Some((0.0, 0.0)));
    }

    #[test]
    fn fade_handle_drag_is_one_undo_step_and_changes_only_that_fade() {
        let mut s = Session::new(false);
        let wav = test_wav("fade_handles.wav");
        let rid = run(&mut s, "import_audio", json!({"path": wav.display().to_string(), "start": 0}))["region_id"]
            .as_u64()
            .unwrap();
        run(&mut s, "set_region_properties", json!({"region_id": rid, "fade_out": 1.0, "gain_db": -3}));
        let state = |s: &Session| {
            let r = s.project.find_region(rid).unwrap().1;
            let RegionContent::Audio(a) = &r.content else { panic!("not audio") };
            (a.fade_in, a.fade_out, a.gain_db, a.reverse, r.start, r.length, r.loop_count, r.name.clone())
        };
        let before = state(&s);
        let len = before.5;
        assert!((len - 4.0).abs() < 1e-6, "length {len}");
        assert_eq!((before.0, before.1, before.2), (0.0, 1.0, -3.0));

        let v = fade_from_drag(FadeHandle::In, 0.0, 1.6, len, 1.0, 0.5);
        assert_eq!(v, 1.5);
        let c = fade_command(rid, FadeHandle::In, 0.0, v).unwrap();
        match &c {
            Command::SetRegionProperties {
                region_id,
                name,
                muted,
                loop_count,
                transpose,
                velocity_offset,
                gain_db,
                fade_in,
                fade_out,
                reverse,
                color,
            } => {
                assert_eq!((*region_id, *fade_in), (rid, Some(1.5)));
                assert!(name.is_none() && muted.is_none() && loop_count.is_none() && transpose.is_none());
                assert!(velocity_offset.is_none() && gain_db.is_none() && fade_out.is_none());
                assert!(reverse.is_none() && color.is_none());
            }
            other => panic!("unexpected command {other:?}"),
        }
        s.execute(c, Source::Gui).unwrap();
        let mut expected = before.clone();
        expected.0 = 1.5;
        assert_eq!(state(&s), expected);
        assert!(fade_command(rid, FadeHandle::In, 1.5, 1.5).is_none());

        // one undo takes the whole drag back
        run(&mut s, "undo", json!({}));
        assert_eq!(state(&s), before);

        // a double-click on the fade-out handle removes that fade only
        s.execute(fade_command(rid, FadeHandle::Out, 1.0, 0.0).unwrap(), Source::Gui).unwrap();
        let mut expected = before.clone();
        expected.1 = 0.0;
        assert_eq!(state(&s), expected);
        run(&mut s, "undo", json!({}));
        assert_eq!(state(&s), before);
    }

    #[test]
    fn fade_labels_say_which_fade() {
        assert_eq!(fade_label(FadeHandle::In, 1.5), "Fade in: 1.50 beats");
        assert_eq!(fade_label(FadeHandle::Out, 0.0), "Fade out: 0.00 beats");
    }

    fn geom(x0: f32, x1: f32, fade_in_px: f32, fade_out_px: f32) -> FadeGeom {
        FadeGeom { first: Rect::from_min_max(pos2(x0, 100.0), pos2(x1, 140.0)), reps: 1, fade_in_px, fade_out_px }
    }

    #[test]
    fn fade_handles_sit_on_the_top_edge_inside_the_first_repetition() {
        let r = |x0: f32, x1: f32| Rect::from_min_max(pos2(x0, 100.0), pos2(x1, 110.0));
        assert_eq!(fade_handles(&geom(200.0, 400.0, 0.0, 0.0)), Some([r(200.0, 210.0), r(390.0, 400.0)]));
        let [i, _] = fade_handles(&geom(200.0, 400.0, 40.0, 0.0)).unwrap();
        assert_eq!(i, r(235.0, 245.0));
        let [i, _] = fade_handles(&geom(200.0, 400.0, 1000.0, 0.0)).unwrap();
        assert_eq!(i, r(390.0, 400.0));
        let [_, o] = fade_handles(&geom(200.0, 400.0, 0.0, 60.0)).unwrap();
        assert_eq!(o, r(335.0, 345.0));
        assert_eq!(fade_handles(&geom(200.0, 223.9, 0.0, 0.0)), None);
        assert!(fade_handles(&geom(200.0, 224.0, 0.0, 0.0)).is_some());
    }

    #[test]
    fn fade_handle_at_picks_the_nearest_handle() {
        let h = fade_handles(&geom(200.0, 400.0, 0.0, 0.0)).unwrap();
        assert_eq!(fade_handle_at(pos2(205.0, 105.0), h), Some(FadeHandle::In));
        assert_eq!(fade_handle_at(pos2(395.0, 105.0), h), Some(FadeHandle::Out));
        assert_eq!(fade_handle_at(pos2(300.0, 105.0), h), None);
        // below the top band, and above the region
        assert_eq!(fade_handle_at(pos2(205.0, 120.0), h), None);
        assert_eq!(fade_handle_at(pos2(205.0, 95.0), h), None);
        // fades that touch: the handles overlap, the nearer center wins and In wins a tie
        let h = fade_handles(&geom(200.0, 400.0, 96.0, 100.0)).unwrap();
        assert_eq!((h[0].center().x, h[1].center().x), (296.0, 300.0));
        assert_eq!(fade_handle_at(pos2(297.0, 105.0), h), Some(FadeHandle::In));
        assert_eq!(fade_handle_at(pos2(299.0, 105.0), h), Some(FadeHandle::Out));
        assert_eq!(fade_handle_at(pos2(298.0, 105.0), h), Some(FadeHandle::In));
    }
}
