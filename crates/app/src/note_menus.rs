//! The piano roll's Length and Velocity menus: what each entry does, as plain data and functions that build the
//! command, so they can be tested without a window.

use ondina_core::command::{Command, NoteLengthMode};
use ondina_core::model::Id;

/// An entry of the Length menu.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum LengthChoice {
    Legato,
    RemoveOverlaps,
    /// The length chosen in the toolbar's Note length box.
    Fixed,
    Half,
    Double,
}

/// The Length menu: label, entry, hover text.
pub const LENGTH_MENU: [(&str, LengthChoice, &str); 5] = [
    ("Legato", LengthChoice::Legato, "Each note lasts until the next one starts; notes that start together end together. The last notes keep their length."),
    ("Remove overlaps", LengthChoice::RemoveOverlaps, "Cut a note where the same pitch starts again"),
    ("Fixed", LengthChoice::Fixed, "Give every note the length chosen in Note length"),
    ("Halve (×½)", LengthChoice::Half, "Make every note half as long"),
    ("Double (×2)", LengthChoice::Double, "Make every note twice as long, up to the end of the region"),
];

/// An entry of the Velocity menu.
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum VelocityChoice {
    Set(u8),
    Scale(f32),
}

/// The Velocity menu's fixed entries (the number field with its Set button comes after them).
pub const VELOCITY_MENU: [(&str, VelocityChoice); 6] = [
    ("Set 64", VelocityChoice::Set(64)),
    ("Set 80", VelocityChoice::Set(80)),
    ("Set 100", VelocityChoice::Set(100)),
    ("Set 127", VelocityChoice::Set(127)),
    ("Scale 90%", VelocityChoice::Scale(0.9)),
    ("Scale 110%", VelocityChoice::Scale(1.1)),
];

/// The command of a Length entry. `note_len` is the toolbar's note length in beats (used by Fixed);
/// `indices` is the selection, `None` for every note.
pub fn length_command(region_id: Id, choice: LengthChoice, note_len: f64, indices: Option<Vec<usize>>) -> Command {
    let (mode, length, factor) = match choice {
        LengthChoice::Legato => (NoteLengthMode::Legato, None, None),
        LengthChoice::RemoveOverlaps => (NoteLengthMode::NoOverlap, None, None),
        LengthChoice::Fixed => (NoteLengthMode::Fixed, Some(note_len), None),
        LengthChoice::Half => (NoteLengthMode::Scale, None, Some(0.5)),
        LengthChoice::Double => (NoteLengthMode::Scale, None, Some(2.0)),
    };
    Command::SetNoteLengths { region_id, mode, length, factor, indices }
}

/// The command of a Velocity entry, with the same `indices` rule.
pub fn velocity_command(region_id: Id, choice: VelocityChoice, indices: Option<Vec<usize>>) -> Command {
    let (velocity, scale) = match choice {
        VelocityChoice::Set(v) => (Some(v.clamp(1, 127)), None),
        VelocityChoice::Scale(f) => (None, Some(f)),
    };
    Command::SetNoteVelocities { region_id, velocity, scale, indices }
}

/// The text of a Length entry: Fixed shows the note length it will use ("Fixed 1/16").
pub fn length_label(label: &str, choice: LengthChoice, note_len_label: &str) -> String {
    match choice {
        LengthChoice::Fixed => format!("{label} {note_len_label}"),
        _ => label.to_string(),
    }
}

/// The status line after a Length entry: something to say only when no note changed.
pub fn length_status(result: &serde_json::Value) -> Option<String> {
    (result.get("changed").and_then(|c| c.as_u64()) == Some(0)).then(|| "Length: no note changed".to_string())
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::HashSet;

    use ondina_core::model::{Note, Project, RegionContent};
    use ondina_studio::{Session, Source};
    use serde_json::{json, Value};

    fn same(c: Command, tool: &str, args: Value) {
        let expected = Command::from_tool_call(tool, &args).unwrap();
        assert_eq!(serde_json::to_value(c).unwrap(), serde_json::to_value(expected).unwrap(), "{tool} {args}");
    }

    #[test]
    fn length_menu_builds_the_same_command_as_the_tool() {
        let t = "set_note_lengths";
        same(length_command(5, LengthChoice::Legato, 0.25, Some(vec![1, 2])), t, json!({"region_id": 5, "mode": "legato", "indices": [1, 2]}));
        same(length_command(5, LengthChoice::RemoveOverlaps, 0.25, None), t, json!({"region_id": 5, "mode": "no_overlap"}));
        same(length_command(5, LengthChoice::Fixed, 0.5, None), t, json!({"region_id": 5, "mode": "fixed", "length": 0.5}));
        same(length_command(5, LengthChoice::Half, 0.25, None), t, json!({"region_id": 5, "mode": "scale", "factor": 0.5}));
        same(length_command(5, LengthChoice::Double, 0.25, None), t, json!({"region_id": 5, "mode": "scale", "factor": 2.0}));
        let order: Vec<LengthChoice> = LENGTH_MENU.iter().map(|e| e.1).collect();
        use LengthChoice::*;
        assert_eq!(order, vec![Legato, RemoveOverlaps, Fixed, Half, Double]);
    }

    #[test]
    fn velocity_menu_builds_set_note_velocities() {
        let t = "set_note_velocities";
        same(velocity_command(5, VelocityChoice::Set(64), Some(vec![0])), t, json!({"region_id": 5, "velocity": 64, "indices": [0]}));
        same(velocity_command(5, VelocityChoice::Scale(0.9), None), t, json!({"region_id": 5, "scale": 0.9}));
        same(velocity_command(5, VelocityChoice::Set(0), None), t, json!({"region_id": 5, "velocity": 1}));
        same(velocity_command(5, VelocityChoice::Set(200), None), t, json!({"region_id": 5, "velocity": 127}));
        let entries: Vec<VelocityChoice> = VELOCITY_MENU.iter().map(|e| e.1).collect();
        use VelocityChoice::*;
        assert_eq!(entries, vec![Set(64), Set(80), Set(100), Set(127), Scale(0.9), Scale(1.1)]);
    }

    #[test]
    fn fixed_label_names_the_note_length() {
        assert_eq!(length_label("Fixed", LengthChoice::Fixed, "1/16"), "Fixed 1/16");
        assert_eq!(length_label("Legato", LengthChoice::Legato, "1/16"), "Legato");
    }

    #[test]
    fn length_status_speaks_only_when_nothing_changed() {
        assert_eq!(length_status(&json!({"changed": 0})), Some("Length: no note changed".to_string()));
        assert_eq!(length_status(&json!({"changed": 2})), None);
        assert_eq!(length_status(&json!({"ok": true})), None);
    }

    fn run(s: &mut Session, name: &str, args: Value) -> Value {
        let c = Command::from_tool_call(name, &args).unwrap();
        s.execute(c, Source::Script).unwrap_or_else(|e| panic!("{name} {args} failed: {e}"))
    }

    /// A session with one instrument track and one MIDI region (8 beats) holding four notes.
    fn session() -> (Session, Id) {
        let notes = [(40, 0.0, 1.5), (40, 1.0, 0.5), (43, 2.0, 0.25), (45, 3.0, 0.25)]
            .map(|(pitch, start, length)| Note { pitch, velocity: 100, start, length });
        let mut s = Session::new(false);
        let t = run(&mut s, "add_track", json!({"kind": "instrument"}))["track_id"].as_u64().unwrap();
        let args = json!({"track_id": t, "start": 0, "length": 8, "notes": serde_json::to_value(notes).unwrap()});
        let rid = run(&mut s, "create_midi_region", args)["region_id"].as_u64().unwrap();
        (s, rid)
    }

    fn notes_of(p: &Project, rid: Id) -> Vec<Note> {
        match &p.find_region(rid).unwrap().1.content {
            RegionContent::Midi(m) => m.notes.clone(),
            _ => panic!("not a MIDI region"),
        }
    }

    #[test]
    fn length_choice_is_one_undo_step_and_keeps_the_selection() {
        let (mut s, rid) = session();
        let mut sel: HashSet<usize> = [0, 1].into_iter().collect();
        let p0 = s.project.clone();
        let v = s.execute(length_command(rid, LengthChoice::Legato, 0.25, Some(vec![0, 1])), Source::Gui).unwrap();
        assert_eq!(v["indices"], json!([0, 1]), "{v}");
        let lens: Vec<f64> = notes_of(&s.project, rid).iter().map(|n| n.length).collect();
        assert_eq!(lens, vec![1.0, 0.5, 0.25, 0.25]);
        crate::note_selection::follow(&mut sel, Some(rid), &p0, &s.project);
        assert_eq!(sel, [0, 1].into_iter().collect());
        s.execute(Command::Undo {}, Source::Gui).unwrap();
        assert_eq!(notes_of(&s.project, rid), notes_of(&p0, rid));
    }

    #[test]
    fn velocity_choice_is_one_undo_step_and_keeps_the_selection() {
        let (mut s, rid) = session();
        let mut sel: HashSet<usize> = [2, 3].into_iter().collect();
        let p0 = s.project.clone();
        s.execute(velocity_command(rid, VelocityChoice::Set(64), Some(vec![2, 3])), Source::Gui).unwrap();
        let vels: Vec<u8> = notes_of(&s.project, rid).iter().map(|n| n.velocity).collect();
        assert_eq!(vels, vec![100, 100, 64, 64]);
        crate::note_selection::follow(&mut sel, Some(rid), &p0, &s.project);
        assert_eq!(sel, [2, 3].into_iter().collect());
        s.execute(Command::Undo {}, Source::Gui).unwrap();
        assert!(notes_of(&s.project, rid).iter().all(|n| n.velocity == 100));
    }
}
