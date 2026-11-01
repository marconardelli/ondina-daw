//! Mouse edits of the piano roll's notes, worked out without the GUI: what a note drag shows and what its release
//! sends (a move, or a copy with Alt), and the velocity lane's stroke.

use std::collections::HashSet;

use ondina_core::command::Command;
use ondina_core::model::{Id, Note};

use crate::note_clipboard::{self as nc, NoteEdit};

/// What releasing a note drag sends: the moved notes in place, or (Alt held) moved copies next to the originals.
#[derive(Debug, Clone, PartialEq)]
pub enum NoteDrop {
    Move(NoteEdit),
    Copy { added: Vec<Note>, edit: NoteEdit },
}

impl NoteDrop {
    /// The command for the region: `ReplaceNotes` for a move, `AddNotes` with the copies for a copy (one undo step).
    pub fn command(&self, region_id: Id) -> Command {
        match self {
            NoteDrop::Move(edit) => Command::ReplaceNotes { region_id, notes: edit.notes.clone() },
            NoteDrop::Copy { added, .. } => Command::AddNotes { region_id, notes: added.clone() },
        }
    }

    /// The region as the session will store it and the notes to select (the moved notes or the copies).
    pub fn into_edit(self) -> NoteEdit {
        match self {
            NoteDrop::Move(edit) | NoteDrop::Copy { edit, .. } => edit,
        }
    }

    /// How many notes were moved or copied.
    pub fn count(&self) -> usize {
        match self {
            NoteDrop::Move(edit) => edit.selected.len(),
            NoteDrop::Copy { added, .. } => added.len(),
        }
    }
}

/// A note moved by `d_beat` and `d_pitch`, kept inside the region start and the MIDI range.
fn moved(n: &Note, d_beat: f64, d_pitch: i32) -> Note {
    Note { start: (n.start + d_beat).max(0.0), pitch: (i32::from(n.pitch) + d_pitch).clamp(0, 127) as u8, ..*n }
}

/// The dragged indices that point at a note of the region.
fn valid(orig: &[Note], indices: &[usize]) -> HashSet<usize> {
    indices.iter().copied().filter(|&i| i < orig.len()).collect()
}

/// The dragged notes moved (valid indices, in index order), as the copies of an Alt drag.
fn copies(orig: &[Note], set: &HashSet<usize>, d_beat: f64, d_pitch: i32) -> Vec<Note> {
    let mut ix: Vec<usize> = set.iter().copied().collect();
    ix.sort_unstable();
    ix.iter().map(|&i| moved(&orig[i], d_beat, d_pitch)).collect()
}

/// The notes the piano roll draws during a drag and which of them look selected. The move is limited like the
/// release (the block stays inside the region and the MIDI range). Without Alt the dragged notes move in their
/// place, so the indices stay those of `orig`; with Alt the originals stay put and the copies follow at the end,
/// shown as the selection.
pub fn drag_preview(orig: &[Note], indices: &[usize], d_beat: f64, d_pitch: i32, copy: bool) -> (Vec<Note>, HashSet<usize>) {
    let set = valid(orig, indices);
    let Some((db, dp)) = nc::limit_move(orig, &set, d_beat, d_pitch) else {
        return (orig.to_vec(), set);
    };
    if copy && (db.abs() >= 1e-9 || dp != 0) {
        let added = copies(orig, &set, db, dp);
        let shown_sel = (orig.len()..orig.len() + added.len()).collect();
        let shown = orig.iter().chain(&added).copied().collect();
        return (shown, shown_sel);
    }
    let shown = orig.iter().enumerate().map(|(i, n)| if set.contains(&i) { moved(n, db, dp) } else { *n }).collect();
    (shown, set)
}

/// What the release of a drag sends, `None` when the notes would not move (nothing to send, no undo step).
pub fn drop_notes(orig: &[Note], indices: &[usize], d_beat: f64, d_pitch: i32, copy: bool) -> Option<NoteDrop> {
    let set = valid(orig, indices);
    if !copy {
        return nc::nudge(orig, &set, d_beat, d_pitch).map(NoteDrop::Move);
    }
    let (db, dp) = nc::limit_move(orig, &set, d_beat, d_pitch)?;
    if db.abs() < 1e-9 && dp == 0 {
        return None;
    }
    let added = copies(orig, &set, db, dp);
    let edit = nc::paste(orig, &added);
    Some(NoteDrop::Copy { added, edit })
}

/// Velocity for a pointer at `y` in a lane whose bars stand on `bottom` and are at most `height` tall (1..127).
pub fn velocity_from_y(y: f32, bottom: f32, height: f32) -> u8 {
    (((bottom - y) / height) * 127.0).clamp(1.0, 127.0) as u8
}

/// One mouse stroke in the velocity lane. Each point sets the notes that start between the previous point and this
/// one (within `tol` beats) to the velocity along the segment, so a fast stroke reaches every note it crosses.
/// Positions are in beats, so scrolling during the stroke keeps it right.
#[derive(Debug, Clone, PartialEq)]
pub struct VelocityStroke {
    orig: Vec<Note>,
    notes: Vec<Note>,
    last: Option<(f64, u8)>,
}

impl VelocityStroke {
    pub fn new(notes: Vec<Note>) -> Self {
        Self { orig: notes.clone(), notes, last: None }
    }

    /// Adds a point of the stroke at `beat` with `value` (limited to 1..127). With `only` not empty, only those
    /// notes change.
    pub fn add(&mut self, beat: f64, value: u8, tol: f64, only: &HashSet<usize>) {
        let value = value.clamp(1, 127);
        let (b0, v0) = self.last.unwrap_or((beat, value));
        let (lo, hi) = (b0.min(beat) - tol, b0.max(beat) + tol);
        let span = beat - b0;
        for (i, n) in self.notes.iter_mut().enumerate() {
            if (!only.is_empty() && !only.contains(&i)) || n.start < lo || n.start > hi {
                continue;
            }
            let t = if span.abs() < 1e-12 { 1.0 } else { ((n.start - b0) / span).clamp(0.0, 1.0) };
            let v = f64::from(v0) + (f64::from(value) - f64::from(v0)) * t;
            n.velocity = v.round().clamp(1.0, 127.0) as u8;
        }
        self.last = Some((beat, value));
    }

    /// The notes as drawn so far.
    pub fn notes(&self) -> &[Note] {
        &self.notes
    }

    /// The notes to send on release, `None` when no velocity changed.
    pub fn changed(&self) -> Option<Vec<Note>> {
        (self.notes != self.orig).then(|| self.notes.clone())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use ondina_core::model::Project;
    use ondina_studio::{Session, Source};
    use serde_json::{json, Value};

    use crate::note_selection::region_notes;

    fn n(pitch: u8, start: f64, length: f64) -> Note {
        Note { pitch, velocity: 100, start, length }
    }

    fn set(ix: &[usize]) -> HashSet<usize> {
        ix.iter().copied().collect()
    }

    fn pitches_starts(notes: &[Note]) -> Vec<(u8, f64)> {
        notes.iter().map(|x| (x.pitch, x.start)).collect()
    }

    fn four() -> Vec<Note> {
        vec![n(60, 0.0, 0.5), n(62, 0.5, 0.5), n(64, 1.0, 0.5), n(65, 1.5, 0.5)]
    }

    fn picked(e: &NoteEdit) -> Vec<(u8, f64)> {
        let mut ix: Vec<usize> = e.selected.iter().copied().collect();
        ix.sort_unstable();
        ix.iter().map(|&i| (e.notes[i].pitch, e.notes[i].start)).collect()
    }

    #[test]
    fn moved_notes_stay_selected_in_session_order() {
        let Some(NoteDrop::Move(edit)) = drop_notes(&four(), &[1, 0], 1.0, 2, false) else { panic!("not a move") };
        assert_eq!(pitches_starts(&edit.notes), vec![(62, 1.0), (64, 1.0), (64, 1.5), (65, 1.5)]);
        assert_eq!(edit.selected, set(&[0, 2]));
        assert_eq!(pitches_starts(&[edit.notes[0]]), vec![(62, 1.0)]);
        assert_eq!(pitches_starts(&[edit.notes[2]]), vec![(64, 1.5)]);
    }

    #[test]
    fn alt_drop_adds_moved_copies_and_selects_them() {
        let orig = four();
        let drop = drop_notes(&orig, &[0, 1], 1.0, 2, true).unwrap();
        assert_eq!(drop.count(), 2);
        let NoteDrop::Copy { added, edit } = drop.clone() else { panic!("not a copy") };
        assert_eq!(pitches_starts(&added), vec![(62, 1.0), (64, 1.5)]);
        match drop.command(7) {
            Command::AddNotes { region_id: 7, notes } => assert_eq!(notes, added),
            _ => panic!("not add_notes"),
        }
        assert_eq!(edit.notes.len(), 6);
        for o in &orig {
            assert!(edit.notes.contains(o), "{o:?} is gone");
        }
        assert_eq!(picked(&edit), vec![(62, 1.0), (64, 1.5)]);
        let starts: Vec<f64> = edit.notes.iter().map(|x| x.start).collect();
        assert!(starts.windows(2).all(|w| w[0] <= w[1]), "{starts:?}");
        assert_eq!(drop.into_edit(), edit);
    }

    #[test]
    fn move_drop_sends_replace_notes() {
        let drop = drop_notes(&four(), &[2], 0.5, 0, false).unwrap();
        assert_eq!(drop.count(), 1);
        let NoteDrop::Move(edit) = drop.clone() else { panic!("not a move") };
        match drop.command(3) {
            Command::ReplaceNotes { region_id: 3, notes } => assert_eq!(notes, edit.notes),
            _ => panic!("not replace_notes"),
        }
    }

    #[test]
    fn drop_without_movement_sends_nothing() {
        assert_eq!(drop_notes(&four(), &[0, 1], 0.0, 0, false), None);
        assert_eq!(drop_notes(&four(), &[0, 1], 0.0, 0, true), None);
        let chord = [n(60, 0.0, 1.0), n(64, 0.0, 1.0)];
        assert_eq!(drop_notes(&chord, &[0, 1], -1.0, 0, false), None);
        assert_eq!(drop_notes(&chord, &[0, 1], -1.0, 0, true), None);
        assert_eq!(drop_notes(&chord, &[5], 1.0, 0, false), None);
    }

    fn sorted_positions(notes: &[Note]) -> Vec<(u8, u64)> {
        let mut v: Vec<(u8, u64)> = notes.iter().map(|x| (x.pitch, x.start.to_bits())).collect();
        v.sort_unstable();
        v
    }

    #[test]
    fn preview_and_drop_limit_the_move_the_same_way() {
        let chord = [n(60, 0.5, 0.5), n(64, 0.5, 0.5)];
        let (shown, _) = drag_preview(&chord, &[0, 1], -2.0, 0, false);
        let Some(NoteDrop::Move(edit)) = drop_notes(&chord, &[0, 1], -2.0, 0, false) else { panic!() };
        assert_eq!(sorted_positions(&shown), sorted_positions(&edit.notes));
        assert_eq!(pitches_starts(&shown), vec![(60, 0.0), (64, 0.0)]);

        let high = [n(120, 0.0, 1.0), n(125, 0.0, 1.0), n(50, 2.0, 1.0)];
        let (shown, _) = drag_preview(&high, &[0, 1], 0.0, 12, false);
        let Some(NoteDrop::Move(edit)) = drop_notes(&high, &[0, 1], 0.0, 12, false) else { panic!() };
        assert_eq!(sorted_positions(&shown), sorted_positions(&edit.notes));
        assert_eq!(shown.iter().map(|x| x.pitch).collect::<Vec<_>>(), vec![122, 127, 50]);

        let (shown, sel) = drag_preview(&high, &[0, 1], 0.0, 12, true);
        let Some(NoteDrop::Copy { added, .. }) = drop_notes(&high, &[0, 1], 0.0, 12, true) else { panic!() };
        assert_eq!(&shown[3..], added.as_slice());
        assert_eq!(sel, set(&[3, 4]));
    }

    #[test]
    fn preview_shows_moves_in_place_and_copies_after_the_originals() {
        let orig = four();
        let (shown, sel) = drag_preview(&orig, &[0, 9, 2], 1.0, 1, false);
        assert_eq!(pitches_starts(&shown), vec![(61, 1.0), (62, 0.5), (65, 2.0), (65, 1.5)]);
        assert_eq!(sel, set(&[0, 2]));

        let (shown, sel) = drag_preview(&orig, &[2, 0, 9], 1.0, 1, true);
        assert_eq!(shown.len(), orig.len() + 2);
        assert_eq!(&shown[..orig.len()], orig.as_slice());
        assert_eq!(pitches_starts(&shown[orig.len()..]), vec![(61, 1.0), (65, 2.0)]);
        assert_eq!(sel, set(&[4, 5]));

        let (shown, sel) = drag_preview(&orig, &[1], 0.0, 0, true);
        assert_eq!(shown, orig);
        assert_eq!(sel, set(&[1]));
        let (shown, sel) = drag_preview(&orig, &[8], 1.0, 0, true);
        assert_eq!(shown, orig);
        assert!(sel.is_empty());
    }

    // ---- velocity lane

    fn vels(notes: &[Note]) -> Vec<u8> {
        notes.iter().map(|x| x.velocity).collect()
    }

    fn nine() -> Vec<Note> {
        (0..9).map(|i| n(60, f64::from(i) * 0.25, 0.25)).collect()
    }

    #[test]
    fn velocity_stroke_sets_every_note_it_crosses() {
        let mut s = VelocityStroke::new(nine());
        s.add(0.0, 20, 0.05, &HashSet::new());
        s.add(2.0, 120, 0.05, &HashSet::new());
        let v = vels(s.notes());
        assert_eq!(v[0], 20);
        assert_eq!(v[8], 120);
        assert_eq!(v[4], 70);
        assert!(v.windows(2).all(|w| w[0] <= w[1]), "{v:?}");
        // Far from both points (more than 4 px at any zoom used here), still reached.
        for i in 2..=6 {
            assert_ne!(v[i], 100, "note {i} untouched: {v:?}");
        }
        assert!(s.changed().is_some());
    }

    #[test]
    fn velocity_stroke_backwards_and_single_point() {
        let mut s = VelocityStroke::new(nine());
        s.add(2.0, 120, 0.05, &HashSet::new());
        s.add(0.0, 20, 0.05, &HashSet::new());
        let v = vels(s.notes());
        assert_eq!((v[0], v[4], v[8]), (20, 70, 120));

        let mut one = VelocityStroke::new(nine());
        one.add(1.0, 40, 0.05, &HashSet::new());
        let v = vels(one.notes());
        assert_eq!(v[4], 40);
        assert_eq!(v.iter().filter(|&&x| x != 100).count(), 1);
    }

    #[test]
    fn velocity_stroke_respects_the_selection() {
        let mut s = VelocityStroke::new(nine());
        s.add(0.0, 20, 0.05, &set(&[1, 3]));
        s.add(2.0, 120, 0.05, &set(&[1, 3]));
        let v = vels(s.notes());
        for (i, x) in v.iter().enumerate() {
            assert_eq!(*x != 100, i == 1 || i == 3, "{v:?}");
        }
    }

    #[test]
    fn velocity_stroke_without_changes_sends_nothing() {
        assert_eq!(VelocityStroke::new(nine()).changed(), None);
        let mut same = VelocityStroke::new(nine());
        same.add(0.0, 100, 0.05, &HashSet::new());
        same.add(2.0, 100, 0.05, &HashSet::new());
        assert_eq!(same.changed(), None);
        let mut one = VelocityStroke::new(nine());
        one.add(0.5, 30, 0.05, &HashSet::new());
        let changed = one.changed().unwrap();
        let orig = nine();
        let diff: Vec<usize> = (0..orig.len()).filter(|&i| changed[i] != orig[i]).collect();
        assert_eq!(diff, vec![2]);
        assert_eq!(changed[2].velocity, 30);
    }

    #[test]
    fn velocity_from_y_clamps_to_the_midi_range() {
        assert_eq!(velocity_from_y(0.0, 100.0, 80.0), 127);
        assert_eq!(velocity_from_y(150.0, 100.0, 80.0), 1);
        assert!((63..=64).contains(&velocity_from_y(60.0, 100.0, 80.0)));
    }

    // ---- the session stores the edit, one undo step per gesture

    fn run(s: &mut Session, name: &str, args: Value) -> Value {
        let c = Command::from_tool_call(name, &args).unwrap();
        s.execute(c, Source::Script).unwrap_or_else(|e| panic!("{name} {args} failed: {e}"))
    }

    fn session_with(notes: &[Note]) -> (Session, Id) {
        let mut s = Session::new(false);
        let t = run(&mut s, "add_track", json!({"kind": "instrument"}))["track_id"].as_u64().unwrap();
        let args = json!({"track_id": t, "start": 0, "length": 8, "notes": serde_json::to_value(notes).unwrap()});
        let rid = run(&mut s, "create_midi_region", args)["region_id"].as_u64().unwrap();
        (s, rid)
    }

    fn notes_of(p: &Project, rid: Id) -> Vec<Note> {
        region_notes(p, rid).unwrap().to_vec()
    }

    fn one_undo_step(copy: bool) {
        let (mut s, rid) = session_with(&four());
        let before = notes_of(&s.project, rid);
        let undo_before = s.can_undo().map(str::to_owned);
        let drop = drop_notes(&before, &[0, 1], 1.0, 2, copy).unwrap();
        s.execute(drop.command(rid), Source::Gui).unwrap();
        assert_eq!(notes_of(&s.project, rid), drop.into_edit().notes);
        run(&mut s, "undo", json!({}));
        assert_eq!(notes_of(&s.project, rid), before);
        assert_eq!(s.can_undo().map(str::to_owned), undo_before);
    }

    #[test]
    fn note_move_is_one_undo_step() {
        one_undo_step(false);
    }

    #[test]
    fn alt_copy_is_one_undo_step() {
        one_undo_step(true);
    }
}
