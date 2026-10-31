//! Piano-roll editing from the computer keyboard: the app's note clipboard, pasting at the playhead and moving the
//! selected notes by grid steps or semitones. Pure functions, so the GUI only wires keys to them.

use std::cmp::Ordering;
use std::collections::HashSet;

use eframe::egui::Key;
use ondina_core::model::{Note, Region};

use crate::piano_roll_view::loop_local_beat;

/// Notes copied from the piano roll. Starts are relative to the earliest copied note.
#[derive(Debug, Clone, PartialEq)]
pub struct NoteClip {
    pub notes: Vec<Note>,
    /// Where the earliest copied note started in its region (beats): the paste position when the playhead is not
    /// in the target region.
    pub origin: f64,
}

/// A region's notes after an edit, exactly as the session will store them, and where the edited notes ended up.
#[derive(Debug, Clone, PartialEq)]
pub struct NoteEdit {
    pub notes: Vec<Note>,
    pub selected: HashSet<usize>,
}

/// One arrow-key move: grid steps in time and semitones in pitch.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Nudge {
    pub steps: i32,
    pub semitones: i32,
}

/// The selected notes (valid indices only, in region order) with starts relative to the earliest of them.
pub fn copy(notes: &[Note], selected: &HashSet<usize>) -> Option<NoteClip> {
    let mut ix: Vec<usize> = selected.iter().copied().filter(|&i| i < notes.len()).collect();
    if ix.is_empty() {
        return None;
    }
    ix.sort_unstable();
    let origin = ix.iter().map(|&i| notes[i].start).fold(f64::INFINITY, f64::min);
    let notes = ix.iter().map(|&i| Note { start: notes[i].start - origin, ..notes[i] }).collect();
    Some(NoteClip { notes, origin })
}

/// The copied notes as JSON, with the same fields as `add_notes`. Never empty: the system clipboard has to hold
/// text for Ctrl+V to reach the app.
pub fn clip_text(clip: &NoteClip) -> String {
    serde_json::to_string(&clip.notes).unwrap_or_else(|_| "[]".into())
}

/// Where the playhead is inside the region (beats from its start, inside one repeat of a looped region), or `None`
/// when it is outside.
pub fn playhead_in_region(position: f64, region: &Region) -> Option<f64> {
    let rel = position - region.start;
    if (0.0..region.end() - region.start).contains(&rel) {
        Some(loop_local_beat(rel, region.length, region.loop_count))
    } else {
        None
    }
}

/// Where the first pasted note goes: the playhead rounded down to the grid, or the clip's origin without one.
pub fn paste_start(clip: &NoteClip, playhead: Option<f64>, grid: f64) -> f64 {
    let at = match playhead {
        Some(p) if grid > 0.0 => (p / grid).floor() * grid,
        Some(p) => p,
        None => clip.origin,
    };
    at.max(0.0)
}

/// The clip's notes moved to `start`, without those that would begin past the end of the region.
pub fn pasted_notes(clip: &NoteClip, start: f64, region_length: f64) -> Vec<Note> {
    clip.notes
        .iter()
        .map(|n| Note { start: n.start + start, ..*n })
        .filter(|n| n.start < region_length - 1e-9)
        .collect()
}

/// The region after `AddNotes` with `added`, and where the added notes are.
pub fn paste(existing: &[Note], added: &[Note]) -> NoteEdit {
    let notes: Vec<Note> = existing.iter().chain(added).copied().collect();
    let marked: Vec<bool> = (0..notes.len()).map(|i| i >= existing.len()).collect();
    session_order(notes, &marked)
}

/// The region with the selected notes moved by `d_beat` and `d_pitch`, limited so no note starts before 0 and every
/// pitch stays in 0..127 (a chord keeps its shape). `None` when there is nothing to move.
pub fn nudge(notes: &[Note], selected: &HashSet<usize>, d_beat: f64, d_pitch: i32) -> Option<NoteEdit> {
    let marked: Vec<bool> = (0..notes.len()).map(|i| selected.contains(&i)).collect();
    let picked: Vec<&Note> = notes.iter().zip(&marked).filter(|(_, &m)| m).map(|(n, _)| n).collect();
    if picked.is_empty() {
        return None;
    }
    let min_start = picked.iter().map(|n| n.start).fold(f64::INFINITY, f64::min);
    let min_pitch = picked.iter().map(|n| i32::from(n.pitch)).min().unwrap_or(0);
    let max_pitch = picked.iter().map(|n| i32::from(n.pitch)).max().unwrap_or(127);
    let d_beat = if d_beat < 0.0 { d_beat.max(-min_start) } else { d_beat };
    let d_pitch = d_pitch.clamp(-min_pitch, 127 - max_pitch);
    if d_beat.abs() < 1e-9 && d_pitch == 0 {
        return None;
    }
    let moved = notes
        .iter()
        .zip(&marked)
        .map(|(n, &m)| {
            if m {
                Note { start: (n.start + d_beat).max(0.0), pitch: (i32::from(n.pitch) + d_pitch).clamp(0, 127) as u8, ..*n }
            } else {
                *n
            }
        })
        .collect();
    Some(session_order(moved, &marked))
}

/// Every index of a region with `len` notes.
pub fn select_all(len: usize) -> HashSet<usize> {
    (0..len).collect()
}

/// The piano roll's grid for pasting: the toolbar snap, at most a quarter note; 0 (no rounding) with snap off.
pub fn grid(snap: f64) -> f64 {
    if snap > 0.0 { snap.min(1.0) } else { 0.0 }
}

/// How far ←/→ move the selected notes: the snap step, at most a quarter note; a sixteenth with snap off.
pub fn nudge_step(snap: f64) -> f64 {
    if snap > 0.0 { snap.min(1.0) } else { 0.25 }
}

/// The move of an arrow key: ←/→ one grid step, ↑/↓ a semitone, Shift+↑/↓ an octave.
pub fn arrow_nudge(key: Key, shift: bool) -> Option<Nudge> {
    let octave = if shift { 12 } else { 1 };
    match key {
        Key::ArrowLeft => Some(Nudge { steps: -1, semitones: 0 }),
        Key::ArrowRight => Some(Nudge { steps: 1, semitones: 0 }),
        Key::ArrowUp => Some(Nudge { steps: 0, semitones: octave }),
        Key::ArrowDown => Some(Nudge { steps: 0, semitones: -octave }),
        _ => None,
    }
}

/// Whether the piano roll's editing keys act: the editor tab shows the piano roll (not the score), no text field
/// has the focus and no mouse drag is under way (its commit would overwrite the edit).
pub fn keys_on(editor_tab: bool, score_shown: bool, text_focus: bool, dragging: bool) -> bool {
    editor_tab && !score_shown && !text_focus && !dragging
}

/// Whether the arrow keys move notes: the editing keys act, Musical Typing is off and some notes are selected.
pub fn arrows_on(keys_on: bool, musical_typing: bool, has_selection: bool) -> bool {
    keys_on && !musical_typing && has_selection
}

/// Mirrors `sanitize_notes` in `ondina_core::ops` (stable sort by start, then pitch), so the indices match what the
/// session stores; the session tests below check it.
fn session_order(notes: Vec<Note>, marked: &[bool]) -> NoteEdit {
    let mut order: Vec<usize> = (0..notes.len()).collect();
    order.sort_by(|&a, &b| {
        let (x, y) = (&notes[a], &notes[b]);
        x.start.partial_cmp(&y.start).unwrap_or(Ordering::Equal).then(x.pitch.cmp(&y.pitch))
    });
    let selected = order.iter().enumerate().filter(|&(_, &old)| marked.get(old).copied().unwrap_or(false)).map(|(new, _)| new).collect();
    NoteEdit { notes: order.iter().map(|&i| notes[i]).collect(), selected }
}

#[cfg(test)]
mod tests {
    use super::*;
    use ondina_core::command::Command;
    use ondina_core::model::{Id, MidiContent, Project, RegionContent};
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

    fn region(start: f64, length: f64, loop_count: u32) -> Region {
        Region {
            id: 1,
            name: "r".into(),
            start,
            length,
            loop_count,
            muted: false,
            color: None,
            content: RegionContent::Midi(MidiContent::default()),
            take: None,
        }
    }

    fn clip(notes: Vec<Note>, origin: f64) -> NoteClip {
        NoteClip { notes, origin }
    }

    #[test]
    fn copy_keeps_times_relative_to_the_first_note() {
        let mut notes = vec![n(60, 0.0, 0.5), n(62, 1.5, 0.5), n(64, 2.0, 0.5), n(65, 3.0, 0.5)];
        notes[3].velocity = 70;
        let c = copy(&notes, &set(&[3, 1])).unwrap();
        assert_eq!(c.origin, 1.5);
        let mut last = n(65, 1.5, 0.5);
        last.velocity = 70;
        assert_eq!(c.notes, vec![n(62, 0.0, 0.5), last]);
    }

    #[test]
    fn copy_needs_a_valid_selection() {
        let notes = vec![n(60, 0.0, 0.5), n(62, 1.5, 0.5), n(64, 2.0, 0.5), n(65, 3.0, 0.5)];
        assert_eq!(copy(&notes, &set(&[])), None);
        assert_eq!(copy(&notes, &set(&[9])), None);
        let c = copy(&notes, &set(&[9, 0])).unwrap();
        assert_eq!(c.notes, vec![n(60, 0.0, 0.5)]);
        assert_eq!(c.origin, 0.0);
    }

    #[test]
    fn clip_text_is_the_notes_as_json() {
        let c = clip(vec![n(62, 0.0, 0.5), n(65, 1.5, 0.25)], 1.5);
        assert_eq!(serde_json::from_str::<Vec<Note>>(&clip_text(&c)).unwrap(), c.notes);
        let one = clip(vec![n(60, 0.0, 1.0)], 0.0);
        let text = clip_text(&one);
        assert!(!text.is_empty());
        assert_eq!(serde_json::from_str::<Vec<Note>>(&text).unwrap(), one.notes);
    }

    #[test]
    fn playhead_in_region_follows_the_repeats() {
        let r = region(4.0, 4.0, 2);
        assert_eq!(playhead_in_region(5.0, &r), Some(1.0));
        assert_eq!(playhead_in_region(9.5, &r), Some(1.5));
        assert_eq!(playhead_in_region(3.9, &r), None);
        assert_eq!(playhead_in_region(12.0, &r), None);
        assert_eq!(playhead_in_region(4.0, &r), Some(0.0));
    }

    #[test]
    fn paste_start_snaps_down_or_keeps_the_origin() {
        let c = clip(vec![n(60, 0.0, 1.0)], 2.5);
        assert_eq!(paste_start(&c, Some(1.3), 0.25), 1.25);
        assert_eq!(paste_start(&c, Some(1.3), 0.0), 1.3);
        assert_eq!(paste_start(&c, None, 0.25), 2.5);
    }

    #[test]
    fn grid_and_nudge_step_follow_the_snap() {
        assert_eq!(grid(0.0), 0.0);
        assert_eq!(grid(4.0), 1.0);
        assert_eq!(grid(0.5), 0.5);
        assert_eq!(nudge_step(0.0), 0.25);
        assert_eq!(nudge_step(4.0), 1.0);
        assert_eq!(nudge_step(0.125), 0.125);
    }

    #[test]
    fn pasted_notes_past_the_region_end_are_dropped() {
        let c = clip(vec![n(60, 0.0, 0.5), n(62, 1.0, 0.5), n(64, 3.5, 0.5)], 0.0);
        assert_eq!(pasted_notes(&c, 1.0, 4.0), vec![n(60, 1.0, 0.5), n(62, 2.0, 0.5)]);
        assert!(pasted_notes(&c, 4.0, 4.0).is_empty());
        let long = clip(vec![n(60, 0.0, 1.0)], 0.0);
        assert_eq!(pasted_notes(&long, 3.75, 4.0), vec![n(60, 3.75, 1.0)]);
    }

    #[test]
    fn paste_selects_the_pasted_notes_in_session_order() {
        let e = paste(&[n(60, 0.0, 1.0), n(67, 2.0, 1.0)], &[n(64, 1.0, 1.0), n(59, 2.0, 1.0)]);
        assert_eq!(e.notes, vec![n(60, 0.0, 1.0), n(64, 1.0, 1.0), n(59, 2.0, 1.0), n(67, 2.0, 1.0)]);
        assert_eq!(e.selected, set(&[1, 2]));
    }

    #[test]
    fn nudge_left_stops_at_zero() {
        let notes = [n(60, 0.0, 0.5), n(62, 0.1, 0.5)];
        let e = nudge(&notes, &set(&[1]), -0.25, 0).unwrap();
        assert_eq!(e.notes, vec![n(60, 0.0, 0.5), n(62, 0.0, 0.5)]);
        assert_eq!(e.selected, set(&[1]));
        assert_eq!(nudge(&notes, &set(&[0]), -0.25, 0), None);
    }

    #[test]
    fn nudge_keeps_chords_inside_the_midi_range() {
        let high = [n(120, 0.0, 1.0), n(125, 0.0, 1.0)];
        let e = nudge(&high, &set(&[0, 1]), 0.0, 12).unwrap();
        assert_eq!(e.notes.iter().map(|x| x.pitch).collect::<Vec<_>>(), vec![122, 127]);
        let low = [n(0, 0.0, 1.0), n(5, 0.0, 1.0)];
        assert_eq!(nudge(&low, &set(&[0, 1]), 0.0, -12), None);
        let near = [n(1, 0.0, 1.0), n(5, 0.0, 1.0)];
        let e = nudge(&near, &set(&[0, 1]), 0.0, -1).unwrap();
        assert_eq!(e.notes.iter().map(|x| x.pitch).collect::<Vec<_>>(), vec![0, 4]);
    }

    #[test]
    fn nudge_resorts_and_follows_the_selected_notes() {
        let notes = [n(60, 0.0, 0.5), n(62, 0.5, 0.5), n(64, 1.0, 0.5)];
        let e = nudge(&notes, &set(&[0]), 0.75, 0).unwrap();
        assert_eq!(e.notes, vec![n(62, 0.5, 0.5), n(60, 0.75, 0.5), n(64, 1.0, 0.5)]);
        assert_eq!(e.selected, set(&[1]));
    }

    #[test]
    fn nudge_leaves_unselected_notes_alone() {
        let notes = [n(60, 0.0, 0.5), n(62, 1.0, 0.5), n(64, 2.0, 0.5)];
        let e = nudge(&notes, &set(&[1]), 0.0, 3).unwrap();
        assert_eq!(e.notes[0], notes[0]);
        assert_eq!(e.notes[2], notes[2]);
        assert_eq!(e.notes[1], n(65, 1.0, 0.5));
        assert_eq!(nudge(&notes, &set(&[]), 0.25, 0), None);
        assert_eq!(nudge(&notes, &set(&[7, 9]), 0.25, 1), None);
    }

    #[test]
    fn select_all_lists_every_index() {
        assert!(select_all(0).is_empty());
        assert_eq!(select_all(3), set(&[0, 1, 2]));
    }

    #[test]
    fn arrow_keys_map_to_steps_and_semitones() {
        let nd = |steps, semitones| Some(Nudge { steps, semitones });
        assert_eq!(arrow_nudge(Key::ArrowLeft, false), nd(-1, 0));
        assert_eq!(arrow_nudge(Key::ArrowRight, false), nd(1, 0));
        assert_eq!(arrow_nudge(Key::ArrowUp, false), nd(0, 1));
        assert_eq!(arrow_nudge(Key::ArrowUp, true), nd(0, 12));
        assert_eq!(arrow_nudge(Key::ArrowDown, false), nd(0, -1));
        assert_eq!(arrow_nudge(Key::ArrowDown, true), nd(0, -12));
        assert_eq!(arrow_nudge(Key::ArrowLeft, true), nd(-1, 0));
        assert_eq!(arrow_nudge(Key::A, false), None);
    }

    #[test]
    fn editing_keys_are_off_while_typing_or_dragging() {
        assert!(keys_on(true, false, false, false));
        assert!(!keys_on(false, false, false, false));
        assert!(!keys_on(true, true, false, false));
        assert!(!keys_on(true, false, true, false));
        assert!(!keys_on(true, false, false, true));
    }

    #[test]
    fn arrows_are_off_with_musical_typing_or_no_selection() {
        assert!(arrows_on(true, false, true));
        assert!(!arrows_on(true, true, true));
        assert!(!arrows_on(true, false, false));
        assert!(!arrows_on(false, false, true));
    }

    // ---- the pure functions and the session agree

    fn run(s: &mut Session, name: &str, args: Value, src: Source) -> Value {
        let c = Command::from_tool_call(name, &args).unwrap();
        s.execute(c, src).unwrap_or_else(|e| panic!("{name} {args} failed: {e}"))
    }

    fn notes_json(notes: &[Note]) -> Value {
        serde_json::to_value(notes).unwrap()
    }

    fn add_region(s: &mut Session, track: u64, start: f64, notes: &[Note]) -> Id {
        let args = json!({"track_id": track, "start": start, "length": 8, "notes": notes_json(notes)});
        run(s, "create_midi_region", args, Source::Script)["region_id"].as_u64().unwrap()
    }

    /// A session with one instrument track and one MIDI region at `start`, eight beats long, holding `notes`.
    fn session_with(start: f64, notes: &[Note]) -> (Session, u64, Id) {
        let mut s = Session::new(false);
        let t = run(&mut s, "add_track", json!({"kind": "instrument"}), Source::Script)["track_id"].as_u64().unwrap();
        let rid = add_region(&mut s, t, start, notes);
        (s, t, rid)
    }

    fn notes_of(p: &Project, rid: Id) -> Vec<Note> {
        region_notes(p, rid).unwrap().to_vec()
    }

    fn region_of(p: &Project, rid: Id) -> Region {
        p.find_region(rid).unwrap().1.clone()
    }

    fn four() -> Vec<Note> {
        vec![n(60, 0.0, 0.5), n(62, 1.0, 0.5), n(64, 2.0, 0.5), n(65, 3.0, 0.5)]
    }

    #[test]
    fn paste_into_another_region_is_one_undo_step() {
        let (mut s, t, a) = session_with(0.0, &four());
        let b = add_region(&mut s, t, 8.0, &[n(48, 0.0, 0.5)]);
        let before_a = notes_of(&s.project, a);
        let before_b = notes_of(&s.project, b);
        let c = copy(&before_a, &set(&[0, 1, 2])).unwrap();
        let rb = region_of(&s.project, b);
        let at = paste_start(&c, playhead_in_region(0.0, &rb), 0.25);
        assert_eq!(at, 0.0);
        let added = pasted_notes(&c, at, rb.length);
        let edit = paste(&before_b, &added);
        s.execute(Command::AddNotes { region_id: b, notes: added }, Source::Gui).unwrap();
        let stored = notes_of(&s.project, b);
        assert_eq!(stored, edit.notes);
        let mut picked: Vec<(u8, f64)> = edit.selected.iter().map(|&i| (stored[i].pitch, stored[i].start)).collect();
        picked.sort_by(|x, y| x.1.partial_cmp(&y.1).unwrap());
        assert_eq!(picked, vec![(60, 0.0), (62, 1.0), (64, 2.0)]);
        run(&mut s, "undo", json!({}), Source::Gui);
        assert_eq!(notes_of(&s.project, b), before_b);
        assert_eq!(notes_of(&s.project, a), before_a);
    }

    #[test]
    fn cut_then_paste_at_the_original_playhead_restores_the_notes() {
        let (mut s, _, rid) = session_with(4.0, &four());
        let before = notes_of(&s.project, rid);
        let c = copy(&before, &set(&[1, 2])).unwrap();
        let cut = Command::RemoveNotes { region_id: rid, indices: Some(vec![1, 2]), pitch: None, from_beat: None, to_beat: None };
        s.execute(cut, Source::Gui).unwrap();
        let r = region_of(&s.project, rid);
        let at = paste_start(&c, playhead_in_region(4.0 + 1.0, &r), 0.25);
        assert_eq!(at, 1.0);
        let added = pasted_notes(&c, at, r.length);
        let edit = paste(&notes_of(&s.project, rid), &added);
        s.execute(Command::AddNotes { region_id: rid, notes: added }, Source::Gui).unwrap();
        assert_eq!(notes_of(&s.project, rid), before);
        assert_eq!(edit.notes, before);
        assert_eq!(edit.selected, set(&[1, 2]));
    }

    #[test]
    fn nudge_matches_what_the_session_stores() {
        let notes = [n(60, 0.0, 0.5), n(62, 0.5, 0.5), n(64, 1.0, 0.5)];
        let (mut s, _, rid) = session_with(0.0, &notes);
        let edit = nudge(&notes_of(&s.project, rid), &set(&[0]), 0.75, 0).unwrap();
        s.execute(Command::ReplaceNotes { region_id: rid, notes: edit.notes.clone() }, Source::Gui).unwrap();
        let stored = notes_of(&s.project, rid);
        assert_eq!(stored, edit.notes);
        assert_eq!(pitches_starts(&[stored[1]]), vec![(60, 0.75)]);
        assert_eq!(edit.selected, set(&[1]));

        // An unsorted region (a transpose leaves the chord out of order) and a paste on top of it.
        let (mut s, _, rid) = session_with(0.0, &[n(60, 0.0, 1.0), n(64, 0.0, 1.0)]);
        s.execute(Command::TransposeNotes { region_id: rid, semitones: 12, indices: Some(vec![0]) }, Source::Gui).unwrap();
        let unsorted = notes_of(&s.project, rid);
        assert_eq!(pitches_starts(&unsorted), vec![(72, 0.0), (64, 0.0)]);
        let added = vec![n(67, 0.0, 1.0)];
        let edit = paste(&unsorted, &added);
        s.execute(Command::AddNotes { region_id: rid, notes: added }, Source::Gui).unwrap();
        assert_eq!(notes_of(&s.project, rid), edit.notes);
        assert_eq!(edit.selected, set(&[1]));
    }
}
