//! Renaming and recoloring a region from the arrange window and the inspector. The commands are built by pure
//! functions so they can be tested without a window.

use eframe::egui::{self, pos2, vec2, Color32, Rect, Stroke};
use ondina_core::command::Command;
use ondina_core::model::Id;

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
}
