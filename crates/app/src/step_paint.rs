//! The step sequencer's mouse gestures: painting steps with a drag, the row fill menu and the drum sounds of
//! "+ Row". Pure functions, so the editor only wires the mouse to them.

use std::collections::BTreeSet;

use eframe::egui::{Pos2, Rect};
use ondina_core::command::Command;
use ondina_core::model::{Id, PatternRow};

/// Velocity of a step turned on with a click or a paint stroke.
pub const PAINT_VELOCITY: u8 = 100;

/// Velocity of an 'o' (normal) step in a `SetPatternRow` string: `parse_step_string` in crates/core/src/ops.rs.
/// `row_fills_match_what_set_pattern_row_stores` keeps the two in line.
const NORMAL: u8 = 90;

/// Where one step cell was laid out this frame (screen coordinates).
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct StepCell {
    pub row: usize,
    pub step: usize,
    pub rect: Rect,
}

/// The cell under `pos`, if any (the 3-point gaps between cells hit nothing).
pub fn cell_at(cells: &[StepCell], pos: Pos2) -> Option<(usize, usize)> {
    cells.iter().find(|c| c.rect.contains(pos)).map(|c| (c.row, c.step))
}

/// A paint stroke over the step grid. The cell where the press began decides the value: an off step turns the
/// stroke into "on at PAINT_VELOCITY", an on step into "off". Positions are cell indices, so scrolling during the
/// stroke keeps it right.
#[derive(Debug, Clone, PartialEq)]
pub struct StepStroke {
    value: u8,
    touched: BTreeSet<(usize, usize)>,
    last: (usize, usize),
}

impl StepStroke {
    /// Starts at (`row`, `step`), whose stored velocity is `current`.
    pub fn begin(row: usize, step: usize, current: u8) -> Self {
        let value = if current > 0 { 0 } else { PAINT_VELOCITY };
        StepStroke { value, touched: BTreeSet::from([(row, step)]), last: (row, step) }
    }

    /// The velocity the stroke writes (0 or PAINT_VELOCITY).
    pub fn value(&self) -> u8 {
        self.value
    }

    /// The pointer reached (`row`, `step`): every cell on the line from the last reached cell to this one joins
    /// the stroke, so a fast drag skips nothing.
    pub fn reach(&mut self, row: usize, step: usize) {
        let (r0, s0) = (self.last.0 as f64, self.last.1 as f64);
        let (dr, ds) = (row as f64 - r0, step as f64 - s0);
        let n = dr.abs().max(ds.abs()) as usize;
        if n == 0 {
            self.touched.insert((row, step));
        } else {
            for i in 0..=n {
                let t = i as f64 / n as f64;
                let r = (r0 + dr * t).round() as usize;
                let s = (s0 + ds * t).round() as usize;
                self.touched.insert((r, s));
            }
        }
        self.last = (row, step);
    }

    /// The velocity to show for a cell whose stored velocity is `stored`.
    pub fn shown(&self, row: usize, step: usize, stored: u8) -> u8 {
        if self.touched.contains(&(row, step)) {
            self.value()
        } else {
            stored
        }
    }

    /// One SetPatternStep per touched cell that `rows` (the pattern at release) does not already hold, in row/step
    /// order. Cells outside `rows` (the pattern changed meanwhile) are left out. Empty when nothing changes.
    pub fn commands(&self, region_id: Id, rows: &[PatternRow]) -> Vec<Command> {
        let velocity = self.value();
        self.touched
            .iter()
            .filter(|(r, s)| rows.get(*r).and_then(|x| x.steps.get(*s)).is_some_and(|v| *v != velocity))
            .map(|&(row, step)| Command::SetPatternStep { region_id, row, step, velocity })
            .collect()
    }
}

/// Undo key for one paint stroke: its commands merge into one step, and two strokes in a row stay two.
pub fn paint_gesture_key() -> String {
    use std::sync::atomic::{AtomicU64, Ordering};
    static NEXT: AtomicU64 = AtomicU64::new(1);
    format!("step-paint-{}", NEXT.fetch_add(1, Ordering::Relaxed))
}

/// The row menu's fills.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RowFill {
    Clear,
    Every,
    Every2nd,
    Every4th,
    Offbeats,
}

impl RowFill {
    pub const ALL: [RowFill; 5] = [RowFill::Clear, RowFill::Every, RowFill::Every2nd, RowFill::Every4th, RowFill::Offbeats];

    /// Menu text.
    pub fn label(self) -> &'static str {
        match self {
            RowFill::Clear => "Clear row",
            RowFill::Every => "Every step",
            RowFill::Every2nd => "Every 2nd step",
            RowFill::Every4th => "Every 4th step",
            RowFill::Offbeats => "Offbeats",
        }
    }

    /// Whether step `i` is on. Offbeats are the step halfway through each beat ("and"): with 1/16 steps 2, 6, 10,
    /// 14; with 1/8 steps every odd one; with steps of a beat or longer every second step.
    fn on(self, i: usize, step_length: f64) -> bool {
        match self {
            RowFill::Clear => false,
            RowFill::Every => true,
            RowFill::Every2nd => i.is_multiple_of(2),
            RowFill::Every4th => i.is_multiple_of(4),
            RowFill::Offbeats => {
                let len = if step_length > 0.0 { step_length } else { 0.25 };
                let per_beat = ((1.0 / len).round() as usize).max(1);
                if per_beat >= 2 {
                    i % per_beat == per_beat / 2
                } else {
                    i % 2 == 1
                }
            }
        }
    }

    /// The SetPatternRow string: 'o' (normal) for an on step, '.' for an off one, `steps` characters.
    pub fn pattern(self, steps: usize, step_length: f64) -> String {
        (0..steps).map(|i| if self.on(i, step_length) { 'o' } else { '.' }).collect()
    }

    /// The row that string gives (0 or 90, the velocity of 'o'), to skip a fill that changes nothing.
    pub fn velocities(self, steps: usize, step_length: f64) -> Vec<u8> {
        (0..steps).map(|i| if self.on(i, step_length) { NORMAL } else { 0 }).collect()
    }
}

/// The drum sounds "+ Row" offers: every note the Drum Synth plays (`kind_for` in crates/dsp/src/drums.rs), with
/// its General MIDI name; the default pattern rows use the same names.
pub const DRUM_SOUNDS: [(&str, u8); 25] = [
    ("Kick 2", 35),
    ("Kick", 36),
    ("Rim", 37),
    ("Snare", 38),
    ("Clap", 39),
    ("Snare 2", 40),
    ("Low Floor Tom", 41),
    ("Closed Hat", 42),
    ("High Floor Tom", 43),
    ("Pedal Hat", 44),
    ("Low Tom", 45),
    ("Open Hat", 46),
    ("Low-Mid Tom", 47),
    ("Hi-Mid Tom", 48),
    ("Crash", 49),
    ("Hi Tom", 50),
    ("Ride", 51),
    ("China", 52),
    ("Ride Bell", 53),
    ("Tambourine", 54),
    ("Splash", 55),
    ("Cowbell", 56),
    ("Crash 2", 57),
    ("Ride 2", 59),
    ("Claves", 75),
];

#[cfg(test)]
mod tests {
    use super::*;
    use eframe::egui::{pos2, vec2};
    use ondina_core::model::RegionContent;
    use ondina_studio::{Session, Source};
    use serde_json::{json, Value};

    fn run(s: &mut Session, name: &str, args: Value) -> Value {
        let c = Command::from_tool_call(name, &args).unwrap();
        s.execute(c, Source::Gui).unwrap_or_else(|e| panic!("{name} {args} failed: {e}"))
    }

    fn row(name: &str, pitch: u8, steps: Vec<u8>) -> PatternRow {
        PatternRow { name: name.into(), pitch, steps, muted: false }
    }

    fn sent(cmds: &[Command]) -> Vec<(usize, usize, u8)> {
        cmds.iter()
            .map(|c| match c {
                Command::SetPatternStep { row, step, velocity, .. } => (*row, *step, *velocity),
                other => panic!("unexpected command {other:?}"),
            })
            .collect()
    }

    /// A Pop Drum Kit track with a 16-step pattern; returns the session and the region id.
    fn drum_session() -> (Session, Id) {
        let mut s = Session::new(false);
        let tid = run(&mut s, "add_track", json!({"kind": "instrument", "patch": "Pop Drum Kit"}))["track_id"].as_u64().unwrap();
        let rid = run(&mut s, "create_pattern_region", json!({"track_id": tid, "start": 0, "steps": 16}))["region_id"]
            .as_u64()
            .unwrap();
        (s, rid)
    }

    fn rows_of(s: &Session, rid: Id) -> Vec<PatternRow> {
        match &s.project.find_region(rid).unwrap().1.content {
            RegionContent::Pattern(p) => p.rows.clone(),
            _ => panic!("not a pattern"),
        }
    }

    fn velocities(rows: &[PatternRow]) -> Vec<Vec<u8>> {
        rows.iter().map(|r| r.steps.clone()).collect()
    }

    #[test]
    fn a_stroke_from_an_off_step_paints_on_and_from_an_on_step_erases() {
        assert_eq!(StepStroke::begin(0, 3, 0).value(), PAINT_VELOCITY);
        assert_eq!(StepStroke::begin(0, 3, 90).value(), 0);
    }

    #[test]
    fn reach_fills_the_cells_between_two_points() {
        let mut st = StepStroke::begin(1, 0, 0);
        st.reach(1, 7);
        for s in 0..=7 {
            assert_eq!(st.shown(1, s, 0), 100, "step {s}");
        }
        assert_eq!(st.shown(1, 8, 0), 0);
        assert_eq!(st.shown(0, 3, 45), 45);
        st.reach(4, 10);
        for cell in [(2, 8), (3, 9), (4, 10)] {
            assert_eq!(st.shown(cell.0, cell.1, 0), 100, "cell {cell:?}");
        }
        let before = st.clone();
        st.reach(4, 10);
        st.reach(4, 10);
        assert_eq!(st, before);
    }

    #[test]
    fn commands_change_only_the_touched_steps_that_differ() {
        let mut steps = vec![0u8; 16];
        steps[..7].copy_from_slice(&[120, 0, 100, 0, 45, 0, 77]);
        let rows = vec![row("Kick", 36, steps), row("Snare", 38, vec![0; 16])];
        let mut st = StepStroke::begin(0, 1, rows[0].steps[1]);
        assert_eq!(st.value(), 100);
        st.reach(0, 5);
        let cmds = st.commands(7, &rows);
        assert!(cmds.iter().all(|c| matches!(c, Command::SetPatternStep { region_id: 7, .. })));
        assert_eq!(sent(&cmds), vec![(0, 1, 100), (0, 3, 100), (0, 4, 100), (0, 5, 100)]);
    }

    #[test]
    fn a_stroke_that_changes_nothing_sends_nothing() {
        let mut st = StepStroke::begin(0, 0, 90);
        st.reach(0, 3);
        let rows = vec![row("Kick", 36, vec![0; 16])];
        assert!(st.commands(1, &rows).is_empty());
    }

    #[test]
    fn commands_skip_cells_outside_the_pattern() {
        let rows = vec![row("Kick", 36, vec![0; 16]), row("Snare", 38, vec![0; 16])];
        let mut st = StepStroke::begin(0, 0, 0);
        st.reach(9, 20);
        let cmds = sent(&st.commands(1, &rows));
        assert!(!cmds.is_empty());
        assert!(cmds.iter().all(|(r, s, _)| *r < 2 && *s < 16), "{cmds:?}");
    }

    #[test]
    fn cell_at_finds_the_cell_under_the_pointer_and_nothing_in_the_gaps() {
        let cells: Vec<StepCell> = (0..3)
            .map(|i| StepCell { row: 0, step: i, rect: Rect::from_min_size(pos2(i as f32 * 25.0, 0.0), vec2(22.0, 20.0)) })
            .collect();
        assert_eq!(cell_at(&cells, pos2(36.0, 10.0)), Some((0, 1)));
        assert_eq!(cell_at(&cells, pos2(23.5, 10.0)), None);
        assert_eq!(cell_at(&cells, pos2(10.0, 30.0)), None);
    }

    #[test]
    fn paint_gesture_keys_are_unique() {
        assert_ne!(paint_gesture_key(), paint_gesture_key());
    }

    #[test]
    fn a_stroke_over_eight_steps_is_one_undo_step() {
        let (mut s, rid) = drum_session();
        run(&mut s, "set_pattern_step", json!({"region_id": rid, "row": 3, "step": 0, "velocity": 120}));
        run(&mut s, "set_pattern_step", json!({"region_id": rid, "row": 3, "step": 12, "velocity": 45}));
        let before = rows_of(&s, rid);
        let undo_before = s.can_undo().map(String::from);

        let mut st = StepStroke::begin(3, 1, before[3].steps[1]);
        st.reach(3, 8);
        let cmds = st.commands(rid, &before);
        assert_eq!(cmds.len(), 8);
        let key = paint_gesture_key();
        for c in cmds {
            s.execute_coalesced(c, &key).unwrap();
        }
        let after = rows_of(&s, rid);
        for (si, v) in after[3].steps.iter().enumerate() {
            let want = match si {
                0 => 120,
                1..=8 => 100,
                12 => 45,
                _ => 0,
            };
            assert_eq!(*v, want, "step {si}");
        }
        for (ri, r) in after.iter().enumerate().filter(|(ri, _)| *ri != 3) {
            assert_eq!(r.steps, before[ri].steps);
        }
        run(&mut s, "undo", json!({}));
        assert_eq!(velocities(&rows_of(&s, rid)), velocities(&before));
        assert_eq!(s.can_undo().map(String::from), undo_before);

        // two strokes in a row are two undo steps
        for (r, key) in [(0, paint_gesture_key()), (1, paint_gesture_key())] {
            let rows = rows_of(&s, rid);
            let mut st = StepStroke::begin(r, 0, 0);
            st.reach(r, 3);
            for c in st.commands(rid, &rows) {
                s.execute_coalesced(c, &key).unwrap();
            }
        }
        run(&mut s, "undo", json!({}));
        let one_back = rows_of(&s, rid);
        assert_eq!(&one_back[0].steps[..4], &[100, 100, 100, 100]);
        assert_eq!(&one_back[1].steps[..4], &[0, 0, 0, 0]);
        run(&mut s, "undo", json!({}));
        assert_eq!(velocities(&rows_of(&s, rid)), velocities(&before));
    }

    #[test]
    fn row_fills_give_the_expected_strings() {
        assert_eq!(RowFill::Clear.pattern(16, 0.25), "................");
        assert_eq!(RowFill::Every.pattern(16, 0.25), "o".repeat(16));
        assert_eq!(RowFill::Every2nd.pattern(16, 0.25), "o.o.o.o.o.o.o.o.");
        assert_eq!(RowFill::Every4th.pattern(16, 0.25), "o...o...o...o...");
        assert_eq!(RowFill::Offbeats.pattern(16, 0.25), "..o...o...o...o.");
        assert_eq!(RowFill::Offbeats.pattern(8, 0.5), ".o.o.o.o");
        assert_eq!(RowFill::Offbeats.pattern(6, 1.0 / 6.0), "...o..");
        assert_eq!(RowFill::Offbeats.pattern(4, 1.0), ".o.o");
        assert_eq!(RowFill::Offbeats.pattern(4, 0.0), "..o.");
        for fill in RowFill::ALL {
            for (steps, len) in [(16, 0.25), (7, 0.5), (1, 1.0), (64, 0.125)] {
                assert_eq!(fill.pattern(steps, len).chars().count(), steps);
                assert_eq!(fill.velocities(steps, len).len(), steps);
            }
        }
    }

    #[test]
    fn row_fills_match_what_set_pattern_row_stores() {
        let (mut s, rid) = drum_session();
        for fill in RowFill::ALL {
            run(&mut s, "set_pattern_row", json!({"region_id": rid, "row": 2, "pattern": fill.pattern(16, 0.25)}));
            assert_eq!(rows_of(&s, rid)[2].steps, fill.velocities(16, 0.25), "{fill:?}");
        }
    }

    #[test]
    fn row_fill_labels_are_the_menu_texts() {
        let labels: Vec<&str> = RowFill::ALL.iter().map(|f| f.label()).collect();
        assert_eq!(labels, ["Clear row", "Every step", "Every 2nd step", "Every 4th step", "Offbeats"]);
    }

    #[test]
    fn drum_sounds_are_distinct_and_cover_the_default_rows() {
        for w in DRUM_SOUNDS.windows(2) {
            assert!(w[0].1 < w[1].1, "{:?} then {:?}", w[0], w[1]);
        }
        let names: std::collections::HashSet<&str> = DRUM_SOUNDS.iter().map(|(n, _)| *n).collect();
        assert_eq!(names.len(), DRUM_SOUNDS.len());
        assert!(DRUM_SOUNDS.iter().all(|(n, p)| !n.is_empty() && (35..=81).contains(p)));
        for r in ondina_core::ops::default_drum_rows(16) {
            assert!(DRUM_SOUNDS.contains(&(r.name.as_str(), r.pitch)), "{} {}", r.name, r.pitch);
        }
    }

    #[test]
    fn add_pattern_row_takes_the_chosen_sound() {
        let (mut s, rid) = drum_session();
        let (name, pitch) = *DRUM_SOUNDS.iter().find(|(n, _)| *n == "Ride").unwrap();
        assert_eq!(pitch, 51);
        let c = Command::AddPatternRow { region_id: rid, name: name.into(), pitch };
        s.execute(c, Source::Gui).unwrap();
        let rows = rows_of(&s, rid);
        assert_eq!(rows.len(), 9);
        let last = rows.last().unwrap();
        assert_eq!((last.name.as_str(), last.pitch), ("Ride", 51));
        assert!(last.steps.iter().all(|v| *v == 0));
        assert_eq!(last.steps.len(), 16);
        run(&mut s, "undo", json!({}));
        assert_eq!(rows_of(&s, rid).len(), 8);
    }
}
