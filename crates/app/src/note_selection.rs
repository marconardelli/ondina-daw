//! Keeps the piano-roll note selection on the same notes when the region's note list changes under it
//! (undo/redo, the assistant, MCP, re-sorting commands). The selection is a set of indices, so after any
//! such change it has to be translated by matching notes, not by position.

use std::collections::HashSet;

use ondina_core::command::{Command, NoteFilter};
use ondina_core::model::{Id, Note, Project, RegionContent};

/// Starts and lengths closer than this (beats) are the same position.
pub const SAME_TIME: f64 = 1e-6;
/// How far in time (beats) a selected note may have moved and still be recognised: undoing Quantize
/// moves notes by at most half a grid step, Humanize by a few hundredths.
pub const MAX_SHIFT: f64 = 1.0;
/// How far in pitch (semitones) a note at the same position may have moved: undoing ↑/↓ st and oct.
pub const MAX_PITCH_SHIFT: i32 = 12;

/// Translates a selection (indices into `old`) to the matching notes of `new`.
///
/// Every note of `old` is first paired with an identical note of `new` (velocity ignored), so a note that
/// still exists can never be taken over by a selected note that disappeared. Selected notes left without
/// a twin are then looked for nearby: same pitch moved in time (undo of Quantize, Humanize or a resize),
/// or same position moved in pitch (undo of a transpose). Notes found nowhere drop out of the selection.
pub fn remap_selection(old: &[Note], selected: &HashSet<usize>, new: &[Note]) -> HashSet<usize> {
    remap(old, selected, new, true)
}

/// Like [`remap_selection`], but only identical notes count: a queued Delete must only ever hit notes
/// identical to the ones the user saw highlighted.
pub fn remap_exact(old: &[Note], selected: &HashSet<usize>, new: &[Note]) -> HashSet<usize> {
    remap(old, selected, new, false)
}

/// The notes of a MIDI region, `None` for pattern or audio regions and for regions that do not exist.
pub fn region_notes(p: &Project, region: Id) -> Option<&[Note]> {
    match &p.find_region(region)?.1.content {
        RegionContent::Midi(m) => Some(&m.notes),
        _ => None,
    }
}

/// Moves the selection of `region` from the notes of `old` to the same notes in `new`. The GUI picks
/// indices against the view it draws, so the view being replaced is exactly what they refer to.
pub fn follow(selected: &mut HashSet<usize>, region: Option<Id>, old: &Project, new: &Project) {
    if selected.is_empty() {
        return;
    }
    let Some(region) = region else { return };
    let (Some(o), Some(n)) = (region_notes(old, region), region_notes(new, region)) else {
        return;
    };
    if o != n {
        *selected = remap_selection(o, selected, n);
    }
}

/// Rewrites the note indices of a command built against `old` so it acts on the same notes in `new`,
/// when the project changed from elsewhere (assistant, MCP, script) before the command ran. Only
/// identical notes are kept; when none survives the command gets an empty list, never `None`, because
/// `None` means every note of the region.
pub fn reindex_command(c: &mut Command, old: &Project, new: &Project) {
    let (region, indices) = match c {
        Command::RemoveNotes { region_id, indices, .. }
        | Command::TransposeNotes { region_id, indices, .. }
        | Command::QuantizeNotes { region_id, indices, .. }
        | Command::HumanizeNotes { region_id, indices, .. }
        | Command::SetNoteVelocities { region_id, indices, .. }
        | Command::SnapNotesToKey { region_id, indices, .. }
        | Command::TransformNotes { region_id, select: Some(NoteFilter { indices, .. }), .. } => {
            (*region_id, indices)
        }
        _ => return,
    };
    let Some(ix) = indices.as_ref() else { return };
    let (Some(o), Some(n)) = (region_notes(old, region), region_notes(new, region)) else {
        return;
    };
    if o == n {
        return;
    }
    let selected: HashSet<usize> = ix.iter().copied().collect();
    let mut out: Vec<usize> = remap_exact(o, &selected, n).into_iter().collect();
    out.sort_unstable();
    *indices = Some(out);
}

fn same_place(a: &Note, b: &Note) -> bool {
    (a.start - b.start).abs() <= SAME_TIME && (a.length - b.length).abs() <= SAME_TIME
}

fn identical(a: &Note, b: &Note) -> bool {
    a.pitch == b.pitch && same_place(a, b)
}

fn bucket(n: &Note) -> usize {
    usize::from(n.pitch.min(127))
}

/// Pairs every note of `old` with an identical, not yet used note of `new`: the same index first, so
/// notes that did not move keep their place and chords or duplicates do not swap, then the first free
/// twin in index order. Returns the pairing and the free notes of `new` grouped by pitch.
fn pair_identical(old: &[Note], new: &[Note]) -> (Vec<Option<usize>>, Vec<Vec<usize>>) {
    let mut pairs: Vec<Option<usize>> = old
        .iter()
        .zip(new)
        .enumerate()
        .map(|(i, (o, n))| identical(o, n).then_some(i))
        .collect();
    pairs.resize(old.len(), None);
    let mut free: Vec<Vec<usize>> = vec![Vec::new(); 128];
    for (j, n) in new.iter().enumerate() {
        if pairs.get(j).copied().flatten() != Some(j) {
            free[bucket(n)].push(j);
        }
    }
    for (o, pair) in old.iter().zip(pairs.iter_mut()) {
        if pair.is_some() {
            continue;
        }
        let b = &mut free[bucket(o)];
        if let Some(k) = b.iter().position(|&j| identical(o, &new[j])) {
            *pair = Some(b.remove(k));
        }
    }
    (pairs, free)
}

fn remap(old: &[Note], selected: &HashSet<usize>, new: &[Note], tolerant: bool) -> HashSet<usize> {
    let mut sel: Vec<usize> = selected.iter().copied().filter(|&i| i < old.len()).collect();
    sel.sort_unstable();
    if old == new {
        return sel.into_iter().collect();
    }
    let (pairs, mut free) = pair_identical(old, new);
    let mut out = HashSet::new();
    let mut lost = Vec::new();
    for i in sel {
        match pairs[i] {
            Some(j) => {
                out.insert(j);
            }
            None => lost.push(i),
        }
    }
    if !tolerant {
        return out;
    }
    // Moved in time at the same pitch.
    let mut still_lost = Vec::new();
    for i in lost {
        let o = &old[i];
        let b = &mut free[bucket(o)];
        let best = b
            .iter()
            .enumerate()
            .map(|(k, &j)| (k, j, (new[j].start - o.start).abs(), (new[j].length - o.length).abs()))
            .filter(|&(_, _, ds, _)| ds <= MAX_SHIFT + SAME_TIME)
            .min_by(|a, b| a.2.total_cmp(&b.2).then(a.3.total_cmp(&b.3)).then(a.1.cmp(&b.1)));
        match best {
            Some((k, j, _, _)) => {
                b.remove(k);
                out.insert(j);
            }
            None => still_lost.push(i),
        }
    }
    // Moved in pitch at the same position. A transpose moves every selected note by the same interval,
    // so the shift that explains the most lost notes wins (then the smallest one), and the rest are
    // looked for again with what is left.
    let mut lost = still_lost;
    while !lost.is_empty() {
        // Most notes explained first, then the smallest shift, then the lowest lost index.
        let mut best_key = None;
        let mut best = Vec::new();
        for d in -MAX_PITCH_SHIFT..=MAX_PITCH_SHIFT {
            let pairs = shift_pairs(old, &lost, new, &free, d);
            let Some(&(first, _)) = pairs.first() else { continue };
            let key = (std::cmp::Reverse(pairs.len()), d.abs(), first);
            if best_key.is_none_or(|k| key < k) {
                best_key = Some(key);
                best = pairs;
            }
        }
        if best.is_empty() {
            break;
        }
        for (i, j) in best {
            free[bucket(&new[j])].retain(|&x| x != j);
            lost.retain(|&x| x != i);
            out.insert(j);
        }
    }
    out
}

/// Pairs each lost note of `old` with a free note of `new` at the same position, `d` semitones away,
/// without using a note of `new` twice. Lost notes are taken in order, each with its lowest free index.
fn shift_pairs(old: &[Note], lost: &[usize], new: &[Note], free: &[Vec<usize>], d: i32) -> Vec<(usize, usize)> {
    let mut used = HashSet::new();
    let mut pairs = Vec::new();
    for &i in lost {
        let o = &old[i];
        let q = i32::from(o.pitch.min(127)) + d;
        if !(0..=127).contains(&q) {
            continue;
        }
        let found = free[q as usize].iter().copied().filter(|j| !used.contains(j) && same_place(o, &new[*j])).min();
        if let Some(j) = found {
            used.insert(j);
            pairs.push((i, j));
        }
    }
    pairs
}

/// The note selection to copy into the session for `get_selection`, or `None` when the session already has it.
/// While `same_region` is false the GUI selection still belongs to the previous region (it is cleared later in the
/// frame), so the session gets an empty selection instead of indices into the wrong region.
pub fn session_notes_update(current: &[usize], selected: &HashSet<usize>, same_region: bool) -> Option<Vec<usize>> {
    let wanted = if same_region { selected.len() } else { 0 };
    if current.len() == wanted && (wanted == 0 || current.iter().all(|i| selected.contains(i))) {
        return None;
    }
    let mut v: Vec<usize> = if same_region { selected.iter().copied().collect() } else { Vec::new() };
    v.sort_unstable();
    Some(v)
}

#[cfg(test)]
mod tests {
    use super::*;
    use ondina_studio::{Session, Source};
    use serde_json::{json, Value};

    fn n(pitch: u8, start: f64, length: f64) -> Note {
        Note { pitch, velocity: 100, start, length }
    }

    fn set(ix: &[usize]) -> HashSet<usize> {
        ix.iter().copied().collect()
    }

    #[test]
    fn session_notes_update_only_when_the_selection_changes() {
        assert_eq!(session_notes_update(&[], &set(&[]), true), None);
        assert_eq!(session_notes_update(&[0, 2], &set(&[2, 0]), true), None);
        assert_eq!(session_notes_update(&[0, 2], &set(&[0, 1, 2]), true), Some(vec![0, 1, 2]));
        assert_eq!(session_notes_update(&[0, 1, 2], &set(&[5]), true), Some(vec![5]));
        assert_eq!(session_notes_update(&[0, 2], &set(&[0, 2]), false), Some(vec![]));
        assert_eq!(session_notes_update(&[], &set(&[0, 2]), false), None);
        assert_eq!(session_notes_update(&[3], &set(&[]), true), Some(vec![]));
    }

    fn sorted(s: &HashSet<usize>) -> Vec<usize> {
        let mut v: Vec<usize> = s.iter().copied().collect();
        v.sort_unstable();
        v
    }

    #[test]
    fn unchanged_notes_keep_the_selection() {
        let a = [n(60, 0.0, 1.0), n(62, 1.0, 1.0), n(64, 2.0, 1.0)];
        assert_eq!(remap_selection(&a, &set(&[0, 2]), &a), set(&[0, 2]));
    }

    #[test]
    fn out_of_range_indices_are_dropped() {
        let a = [n(60, 0.0, 1.0), n(62, 1.0, 1.0), n(64, 2.0, 1.0)];
        assert_eq!(remap_selection(&a, &set(&[1, 7]), &a), set(&[1]));
        let b = [n(64, 2.0, 1.0), n(60, 0.0, 1.0), n(62, 1.0, 1.0)];
        assert_eq!(remap_selection(&a, &set(&[1, 7]), &b), set(&[2]));
    }

    #[test]
    fn undo_of_quantize_follows_the_notes() {
        let quantized = [n(60, 0.1, 0.5), n(65, 0.95, 0.5), n(62, 1.0, 0.5), n(64, 1.0, 0.5)];
        let original = [n(60, 0.1, 0.5), n(62, 0.8, 0.5), n(64, 0.9, 0.5), n(65, 0.95, 0.5)];
        assert_eq!(remap_selection(&quantized, &set(&[2, 3]), &original), set(&[1, 2]));
    }

    #[test]
    fn deleted_note_is_dropped() {
        let old = [n(60, 0.0, 0.5), n(60, 1.5, 0.5), n(64, 2.0, 0.5)];
        let new = [n(60, 1.5, 0.5), n(64, 2.0, 0.5)];
        assert!(remap_selection(&old, &set(&[0]), &new).is_empty());
    }

    #[test]
    fn duplicates_are_paired_without_reuse() {
        let old = [n(60, 0.0, 1.0), n(60, 0.0, 1.0), n(62, 1.0, 1.0)];
        let new = [n(59, 0.0, 1.0), n(60, 0.0, 1.0), n(60, 0.0, 1.0), n(62, 1.0, 1.0)];
        assert_eq!(remap_selection(&old, &set(&[0, 1]), &new), set(&[1, 2]));
        let fewer = [n(60, 0.0, 1.0), n(62, 1.0, 1.0)];
        assert_eq!(remap_selection(&old, &set(&[0, 1]), &fewer), set(&[0]));
    }

    #[test]
    fn velocity_changes_keep_the_selection() {
        let old = [n(60, 0.0, 1.0), n(62, 1.0, 1.0), n(64, 2.0, 1.0)];
        let mut new = old;
        new[0].velocity = 30;
        new[2].velocity = 127;
        assert_eq!(remap_selection(&old, &set(&[0, 2]), &new), set(&[0, 2]));
    }

    #[test]
    fn undo_of_resize_keeps_the_selection() {
        let old = [n(60, 0.0, 2.0), n(62, 1.0, 1.0)];
        let new = [n(60, 0.0, 0.5), n(62, 1.0, 1.0)];
        assert_eq!(remap_selection(&old, &set(&[0]), &new), set(&[0]));
    }

    #[test]
    fn undo_of_transpose_keeps_the_selection() {
        let old = [n(61, 0.0, 1.0), n(64, 0.0, 1.0), n(67, 0.0, 1.0)];
        let new = [n(60, 0.0, 1.0), n(64, 0.0, 1.0), n(67, 0.0, 1.0)];
        assert_eq!(remap_selection(&old, &set(&[0]), &new), set(&[0]));
        let up = [n(72, 1.0, 1.0)];
        let down = [n(60, 1.0, 1.0)];
        assert_eq!(remap_selection(&up, &set(&[0]), &down), set(&[0]));
    }

    #[test]
    fn octave_transpose_of_a_chord_keeps_every_note() {
        let chord = [n(60, 0.0, 1.0), n(64, 0.0, 1.0)];
        let down = [n(48, 0.0, 1.0), n(52, 0.0, 1.0)];
        assert_eq!(remap_selection(&chord, &set(&[0, 1]), &down), set(&[0, 1]));
        let up = [n(72, 0.0, 1.0), n(76, 0.0, 1.0)];
        assert_eq!(remap_selection(&up, &set(&[0, 1]), &chord), set(&[0, 1]));
        let triad = [n(60, 0.0, 1.0), n(64, 0.0, 1.0), n(67, 0.0, 1.0)];
        let low = [n(48, 0.0, 1.0), n(52, 0.0, 1.0), n(55, 0.0, 1.0)];
        assert_eq!(remap_selection(&triad, &set(&[0, 1, 2]), &low), set(&[0, 1, 2]));
    }

    #[test]
    fn far_moves_are_dropped() {
        let old = [n(60, 0.0, 1.0)];
        assert!(remap_selection(&old, &set(&[0]), &[n(60, 2.0, 1.0)]).is_empty());
        assert!(remap_selection(&old, &set(&[0]), &[n(73, 0.0, 1.0)]).is_empty());
    }

    #[test]
    fn remap_exact_ignores_moved_notes() {
        let quantized = [n(60, 0.1, 0.5), n(65, 0.95, 0.5), n(62, 1.0, 0.5), n(64, 1.0, 0.5)];
        let original = [n(60, 0.1, 0.5), n(62, 0.8, 0.5), n(64, 0.9, 0.5), n(65, 0.95, 0.5)];
        assert!(remap_exact(&quantized, &set(&[2, 3]), &original).is_empty());
        assert_eq!(remap_exact(&quantized, &set(&[0, 1, 2]), &original), set(&[0, 3]));
    }

    fn run(s: &mut Session, name: &str, args: Value, src: Source) -> Value {
        let c = Command::from_tool_call(name, &args).unwrap();
        s.execute(c, src).unwrap_or_else(|e| panic!("{name} {args} failed: {e}"))
    }

    fn notes_json(notes: &[Note]) -> Value {
        serde_json::to_value(notes).unwrap()
    }

    /// A session with one instrument track and one MIDI region holding `notes`.
    fn session_with(notes: &[Note]) -> (Session, Id) {
        let mut s = Session::new(false);
        let t = run(&mut s, "add_track", json!({"kind": "instrument"}), Source::Script)["track_id"].as_u64().unwrap();
        let args = json!({"track_id": t, "start": 0, "length": 8, "notes": notes_json(notes)});
        let rid = run(&mut s, "create_midi_region", args, Source::Script)["region_id"].as_u64().unwrap();
        (s, rid)
    }

    fn notes_of(p: &Project, rid: Id) -> Vec<Note> {
        region_notes(p, rid).unwrap().to_vec()
    }

    fn at(p: &Project, rid: Id, ix: &HashSet<usize>) -> Vec<(u8, f64)> {
        let notes = notes_of(p, rid);
        sorted(ix).into_iter().map(|i| (notes[i].pitch, notes[i].start)).collect()
    }

    fn remove(rid: Id, indices: Option<Vec<usize>>) -> Command {
        Command::RemoveNotes { region_id: rid, indices, pitch: None, from_beat: None, to_beat: None }
    }

    fn indices_of(c: &Command) -> Option<Vec<usize>> {
        match c {
            Command::RemoveNotes { indices, .. }
            | Command::TransposeNotes { indices, .. }
            | Command::QuantizeNotes { indices, .. }
            | Command::HumanizeNotes { indices, .. }
            | Command::SetNoteVelocities { indices, .. }
            | Command::SnapNotesToKey { indices, .. }
            | Command::TransformNotes { select: Some(NoteFilter { indices, .. }), .. } => indices.clone(),
            _ => None,
        }
    }

    fn four() -> Vec<Note> {
        vec![n(60, 0.0, 0.5), n(62, 1.0, 0.5), n(64, 2.0, 0.5), n(65, 3.0, 0.5)]
    }

    #[test]
    fn follow_needs_a_selected_midi_region() {
        let (mut s, rid) = session_with(&four());
        let old = s.project.clone();
        run(&mut s, "replace_notes", json!({"region_id": rid, "notes": notes_json(&[n(65, 3.0, 0.5), n(60, 0.0, 0.5)])}), Source::Mcp);
        let new = s.project.clone();

        let mut empty = HashSet::new();
        follow(&mut empty, Some(rid), &old, &new);
        assert!(empty.is_empty());

        let mut sel = set(&[1, 3]);
        follow(&mut sel, None, &old, &new);
        assert_eq!(sel, set(&[1, 3]));

        run(&mut s, "delete_region", json!({"region_id": rid}), Source::Mcp);
        let gone = s.project.clone();
        follow(&mut sel, Some(rid), &old, &gone);
        assert_eq!(sel, set(&[1, 3]));

        follow(&mut sel, Some(rid), &old, &new);
        assert_eq!(sel, set(&[1]));
    }

    #[test]
    fn reindex_translates_queued_note_commands() {
        let (mut s, rid) = session_with(&four());
        let old = s.project.clone();
        let mut shuffled = four();
        shuffled.insert(0, n(59, 0.0, 0.5));
        run(&mut s, "replace_notes", json!({"region_id": rid, "notes": notes_json(&shuffled)}), Source::Mcp);
        let new = s.project.clone();
        // 59@0 sorts before 60@0, so every old note moved one place down.
        let ix = Some(vec![0, 2]);
        let mut cmds = [
            remove(rid, ix.clone()),
            Command::TransposeNotes { region_id: rid, semitones: 1, indices: ix.clone() },
            Command::QuantizeNotes { region_id: rid, grid: 0.5, strength: None, swing: None, quantize_ends: None, indices: ix.clone() },
            Command::HumanizeNotes { region_id: rid, timing: None, velocity: None, seed: None, indices: ix.clone() },
            Command::SetNoteVelocities { region_id: rid, velocity: Some(10), scale: None, indices: ix.clone() },
            Command::SnapNotesToKey { region_id: rid, indices: ix.clone() },
            Command::TransformNotes {
                region_id: rid,
                select: Some(NoteFilter { indices: ix.clone(), ..Default::default() }),
                operations: Default::default(),
            },
        ];
        for c in cmds.iter_mut() {
            reindex_command(c, &old, &new);
            assert_eq!(indices_of(c), Some(vec![1, 3]), "{c:?}");
        }
    }

    #[test]
    fn reindex_keeps_none_and_empties_to_some() {
        let (mut s, rid) = session_with(&four());
        let old = s.project.clone();
        run(&mut s, "replace_notes", json!({"region_id": rid, "notes": notes_json(&[n(62, 1.0, 0.5), n(65, 3.0, 0.5)])}), Source::Mcp);
        let new = s.project.clone();
        let mut all = remove(rid, None);
        reindex_command(&mut all, &old, &new);
        assert_eq!(indices_of(&all), None);
        let mut gone = remove(rid, Some(vec![0, 2]));
        reindex_command(&mut gone, &old, &new);
        assert_eq!(indices_of(&gone), Some(vec![]));
    }

    #[test]
    fn reindex_leaves_other_regions_and_commands_alone() {
        let (mut s, rid) = session_with(&four());
        let t = s.project.tracks.last().unwrap().id;
        let args = json!({"track_id": t, "start": 8, "length": 4, "notes": notes_json(&four())});
        let other = run(&mut s, "create_midi_region", args, Source::Script)["region_id"].as_u64().unwrap();
        let old = s.project.clone();
        run(&mut s, "replace_notes", json!({"region_id": rid, "notes": notes_json(&[n(59, 0.0, 0.5)])}), Source::Mcp);
        let new = s.project.clone();
        let mut c = remove(other, Some(vec![2, 0]));
        let before = format!("{c:?}");
        reindex_command(&mut c, &old, &new);
        assert_eq!(format!("{c:?}"), before);
        let mut play = Command::Play { from_beat: None };
        let before = format!("{play:?}");
        reindex_command(&mut play, &old, &new);
        assert_eq!(format!("{play:?}"), before);
    }

    #[test]
    fn quantize_then_undo_keeps_the_same_notes_selected() {
        let original = [n(60, 0.1, 0.5), n(62, 0.8, 0.5), n(64, 0.9, 0.5), n(65, 0.95, 0.5)];
        let (mut s, rid) = session_with(&original);
        let p0 = s.project.clone();
        let r = run(&mut s, "quantize_notes", json!({"region_id": rid, "grid": 0.5, "indices": [1, 2]}), Source::Gui);
        assert_eq!(r["indices"], json!([2, 3]));
        let p1 = s.project.clone();
        // The general rule agrees with the indices the command returns.
        let mut sel = set(&[1, 2]);
        follow(&mut sel, Some(rid), &p0, &p1);
        assert_eq!(sel, set(&[2, 3]));

        run(&mut s, "undo", json!({}), Source::Gui);
        let p2 = s.project.clone();
        // Without following, the old indices now point at other notes.
        assert_eq!(at(&p2, rid, &sel), vec![(64, 0.9), (65, 0.95)]);
        follow(&mut sel, Some(rid), &p1, &p2);
        assert_eq!(sel, set(&[1, 2]));
        assert_eq!(at(&p2, rid, &sel), vec![(62, 0.8), (64, 0.9)]);

        run(&mut s, "redo", json!({}), Source::Gui);
        let p3 = s.project.clone();
        follow(&mut sel, Some(rid), &p2, &p3);
        assert_eq!(sel, set(&[2, 3]));
        assert_eq!(at(&p3, rid, &sel), vec![(62, 1.0), (64, 1.0)]);
    }

    #[test]
    fn mcp_replace_then_delete_removes_only_selected_notes() {
        let (mut s, rid) = session_with(&four());
        let old = s.project.clone();
        let mut sel = set(&[0, 2]);
        let replacement = [n(65, 3.0, 0.5), n(59, 0.0, 0.5), n(64, 2.0, 0.5), n(60, 0.0, 0.5), n(67, 3.5, 0.5)];
        run(&mut s, "replace_notes", json!({"region_id": rid, "notes": notes_json(&replacement)}), Source::Mcp);
        let new = s.project.clone();
        follow(&mut sel, Some(rid), &old, &new);
        assert_eq!(at(&new, rid, &sel), vec![(60, 0.0), (64, 2.0)]);
        s.execute(remove(rid, Some(sorted(&sel))), Source::Gui).unwrap();
        let left: Vec<(u8, f64)> = notes_of(&s.project, rid).iter().map(|x| (x.pitch, x.start)).collect();
        assert_eq!(left, vec![(59, 0.0), (65, 3.0), (67, 3.5)]);
    }

    #[test]
    fn queued_delete_follows_an_external_edit() {
        let (mut s, rid) = session_with(&four());
        let view = s.project.clone();
        let mut c = remove(rid, Some(vec![0, 2]));
        let mut more = four();
        more.push(n(59, 0.0, 0.5));
        run(&mut s, "replace_notes", json!({"region_id": rid, "notes": notes_json(&more)}), Source::Mcp);
        reindex_command(&mut c, &view, &s.project);
        s.execute(c, Source::Gui).unwrap();
        let left: Vec<(u8, f64)> = notes_of(&s.project, rid).iter().map(|x| (x.pitch, x.start)).collect();
        assert_eq!(left, vec![(59, 0.0), (62, 1.0), (65, 3.0)]);

        // The selected notes vanish before the queued Delete runs: it must remove nothing.
        let view = s.project.clone();
        let mut c = remove(rid, Some(vec![0, 1]));
        run(&mut s, "replace_notes", json!({"region_id": rid, "notes": notes_json(&[n(65, 3.0, 0.5)])}), Source::Mcp);
        reindex_command(&mut c, &view, &s.project);
        assert_eq!(indices_of(&c), Some(vec![]));
        s.execute(c, Source::Gui).unwrap();
        assert_eq!(notes_of(&s.project, rid).len(), 1);
    }
}
