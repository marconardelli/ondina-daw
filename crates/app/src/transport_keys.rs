//! Keyboard moves of the arrangement: the playhead by bars and markers, and the split at the playhead.

use eframe::egui::{Key, Modifiers};
use ondina_core::command::Command;
use ondina_core::model::{Id, Marker, Project, Region};

/// Positions closer than this are the same position (they come from `f64` sums of bar lengths).
pub const EPS: f64 = 1e-6;
/// While playing, a step back from less than this many beats past a bar or marker goes to the one before it,
/// so pressing `,` repeatedly keeps moving back instead of landing on the bar that has just started.
pub const PLAYING_GRACE: f64 = 0.5;

pub const NO_REGION: &str = "Select a region to split at the playhead";
pub const OUTSIDE: &str = "Playhead is outside the selected region";

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Dir {
    Back,
    Forward,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Jump {
    Bar(Dir),
    Marker(Dir),
}

/// The playhead and split shortcuts act only with no text field focused, with Musical Typing off (there `,` `.`
/// and `T` are note keys or switched off) and with no drag in progress (its commit would overwrite the split).
pub fn keys_on(text_focus: bool, musical_typing: bool, dragging: bool) -> bool {
    !text_focus && !musical_typing && !dragging
}

/// `,` and `.` step by a bar, with Shift by a marker. The physical key counts only with Shift: on Italian and
/// German layouts Shift+, types `;` and Shift+. types `:`, so the logical key is no longer `,` or `.`.
pub fn jump_from_key(key: Key, physical: Option<Key>, mods: Modifiers) -> Option<Jump> {
    if mods.command || mods.ctrl || mods.alt {
        return None;
    }
    let back = key == Key::Comma || (mods.shift && physical == Some(Key::Comma));
    let forward = key == Key::Period || (mods.shift && physical == Some(Key::Period));
    let dir = if back {
        Dir::Back
    } else if forward {
        Dir::Forward
    } else {
        return None;
    };
    Some(if mods.shift { Jump::Marker(dir) } else { Jump::Bar(dir) })
}

/// Ctrl+T, pressed once: the repeats of a held key must not split again.
pub fn split_key(key: Key, repeat: bool, mods: Modifiers) -> bool {
    key == Key::T && !repeat && mods.command && !mods.alt && !mods.shift
}

/// Start of the next bar, or of the bar the playhead is in (the previous one when it sits on a bar line or, while
/// playing, less than `grace` beats past it). Never below 0. A signature of 0 beats per bar counts as 4/4.
pub fn bar_step(position: f64, beats_per_bar: f64, dir: Dir, grace: f64) -> f64 {
    let bpb = if beats_per_bar > EPS { beats_per_bar } else { 4.0 };
    let pos = position.max(0.0);
    // `+ EPS`: 7.9999999 is on the bar line at 8, not in the bar before it.
    let bar_start = ((pos / bpb) + EPS).floor() * bpb;
    match dir {
        Dir::Forward => bar_start + bpb,
        Dir::Back if pos - bar_start > EPS + grace => bar_start,
        Dir::Back => (bar_start - bpb).max(0.0),
    }
}

/// Beat of the nearest marker in that direction, ordered by position (the marker list is not sorted); the song's
/// start or end when there is none.
pub fn marker_step(markers: &[Marker], song_end: f64, position: f64, dir: Dir, grace: f64) -> f64 {
    let beats = markers.iter().map(|m| m.beat);
    let to = match dir {
        Dir::Forward => {
            let next = beats.filter(|&b| b > position + EPS).fold(f64::INFINITY, f64::min);
            if next.is_finite() {
                next
            } else if song_end > position + EPS {
                song_end
            } else {
                position
            }
        }
        Dir::Back => {
            let prev = beats.filter(|&b| b < position - EPS - grace).fold(f64::NEG_INFINITY, f64::max);
            if prev.is_finite() { prev } else { 0.0 }
        }
    };
    to.max(0.0)
}

/// Where a jump takes the playhead in this project.
pub fn jump_target(project: &Project, position: f64, jump: Jump, playing: bool) -> f64 {
    let grace = if playing { PLAYING_GRACE } else { 0.0 };
    match jump {
        Jump::Bar(d) => bar_step(position, project.time_signature.beats_per_bar(), d, grace),
        Jump::Marker(d) => marker_step(&project.markers, project.content_end(), position, d, grace),
    }
}

/// The split beat when the playhead is strictly inside the region (its repeats included): on an edge there is
/// nothing to split.
pub fn split_target(region: &Region, position: f64) -> Option<f64> {
    (position > region.start + EPS && position < region.end() - EPS).then_some(position)
}

/// The command that splits the selected region at the playhead, or why there is nothing to split.
pub fn split_command(project: &Project, selected: Option<Id>, position: f64) -> Result<Command, &'static str> {
    let region = selected.and_then(|id| project.find_region(id)).map(|(_, r)| r).ok_or(NO_REGION)?;
    let beat = split_target(region, position).ok_or(OUTSIDE)?;
    Ok(Command::SplitRegion { region_id: region.id, beat })
}

#[cfg(test)]
mod tests {
    use super::*;
    use ondina_core::model::{MidiContent, Note, RegionContent, TimeSignature};
    use ondina_studio::{Session, Source};
    use serde_json::{json, Value};

    fn marker(id: Id, beat: f64) -> Marker {
        Marker { id, beat, name: format!("m{id}"), length: None }
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

    fn run(s: &mut Session, name: &str, args: Value) -> Value {
        let c = Command::from_tool_call(name, &args).unwrap();
        s.execute(c, Source::Gui).unwrap_or_else(|e| panic!("{name} {args} failed: {e}"))
    }

    #[test]
    fn bar_step_moves_by_whole_bars_in_four_four() {
        assert_eq!(bar_step(5.5, 4.0, Dir::Forward, 0.0), 8.0);
        assert_eq!(bar_step(5.5, 4.0, Dir::Back, 0.0), 4.0);
        assert_eq!(bar_step(4.0, 4.0, Dir::Back, 0.0), 0.0);
        assert_eq!(bar_step(4.0, 4.0, Dir::Forward, 0.0), 8.0);
        assert_eq!(bar_step(0.0, 4.0, Dir::Back, 0.0), 0.0);
        assert_eq!(bar_step(0.0, 4.0, Dir::Forward, 0.0), 4.0);
        assert_eq!(bar_step(1.0, 4.0, Dir::Back, 0.0), 0.0);
    }

    #[test]
    fn bar_step_never_goes_below_zero() {
        assert_eq!(bar_step(0.0, 4.0, Dir::Back, 0.0), 0.0);
        assert_eq!(bar_step(-3.0, 4.0, Dir::Back, 0.0), 0.0);
        assert_eq!(bar_step(2.0, 4.0, Dir::Back, 0.5), 0.0);
    }

    #[test]
    fn bar_step_follows_the_bar_length() {
        assert_eq!(bar_step(0.0, 3.0, Dir::Forward, 0.0), 3.0);
        assert_eq!(bar_step(3.0, 3.0, Dir::Forward, 0.0), 6.0);
        assert_eq!(bar_step(4.5, 3.0, Dir::Back, 0.0), 3.0);
        assert_eq!(bar_step(3.0, 3.0, Dir::Back, 0.0), 0.0);
        assert_eq!(bar_step(1.0, 0.0, Dir::Forward, 0.0), 4.0);
    }

    #[test]
    fn bar_step_ignores_float_noise_on_bar_lines() {
        assert_eq!(bar_step(7.9999999, 4.0, Dir::Forward, 0.0), 12.0);
        assert_eq!(bar_step(8.0000001, 4.0, Dir::Back, 0.0), 4.0);
    }

    #[test]
    fn bar_step_while_playing_skips_a_bar_just_started() {
        assert_eq!(bar_step(4.3, 4.0, Dir::Back, PLAYING_GRACE), 0.0);
        assert_eq!(bar_step(4.6, 4.0, Dir::Back, PLAYING_GRACE), 4.0);
        assert_eq!(bar_step(4.3, 4.0, Dir::Forward, PLAYING_GRACE), 8.0);
    }

    #[test]
    fn marker_step_orders_markers_by_position() {
        let mut ms = vec![marker(1, 16.0), marker(2, 4.0), marker(3, 32.0)];
        ms[0].length = Some(8.0);
        let step = |pos: f64, dir: Dir| marker_step(&ms, 48.0, pos, dir, 0.0);
        assert_eq!(step(0.0, Dir::Forward), 4.0);
        assert_eq!(step(4.0, Dir::Forward), 16.0);
        assert_eq!(step(10.0, Dir::Back), 4.0);
        assert_eq!(step(16.0, Dir::Back), 4.0);
        assert_eq!(step(33.0, Dir::Forward), 48.0);
        assert_eq!(step(2.0, Dir::Back), 0.0);
        assert_eq!(step(48.0, Dir::Forward), 48.0);
        assert_eq!(step(60.0, Dir::Forward), 60.0);
    }

    #[test]
    fn marker_step_without_markers_goes_to_start_and_end() {
        assert_eq!(marker_step(&[], 24.0, 5.0, Dir::Forward, 0.0), 24.0);
        assert_eq!(marker_step(&[], 24.0, 5.0, Dir::Back, 0.0), 0.0);
        assert_eq!(marker_step(&[], 0.0, 0.0, Dir::Forward, 0.0), 0.0);
    }

    #[test]
    fn marker_step_while_playing_skips_a_marker_just_passed() {
        let ms = [marker(1, 4.0), marker(2, 16.0)];
        assert_eq!(marker_step(&ms, 32.0, 16.3, Dir::Back, 0.5), 4.0);
        assert_eq!(marker_step(&ms, 32.0, 16.8, Dir::Back, 0.5), 16.0);
    }

    #[test]
    fn jump_target_reads_the_project() {
        let mut p = Project::new("t");
        p.time_signature = TimeSignature { numerator: 3, denominator: 4 };
        p.markers = vec![marker(1, 9.0), marker(2, 6.25)];
        assert_eq!(jump_target(&p, 4.0, Jump::Bar(Dir::Forward), false), 6.0);
        assert_eq!(jump_target(&p, 4.0, Jump::Marker(Dir::Forward), false), 6.25);
        assert_eq!(jump_target(&p, 6.25, Jump::Bar(Dir::Back), false), 6.0);
        assert_eq!(jump_target(&p, 6.25, Jump::Bar(Dir::Back), true), 3.0);
        assert_eq!(jump_target(&p, 6.5, Jump::Marker(Dir::Back), false), 6.25);
        assert_eq!(jump_target(&p, 6.5, Jump::Marker(Dir::Back), true), 0.0);
    }

    #[test]
    fn comma_and_period_step_by_bar() {
        assert_eq!(jump_from_key(Key::Comma, Some(Key::Comma), Modifiers::NONE), Some(Jump::Bar(Dir::Back)));
        assert_eq!(jump_from_key(Key::Period, Some(Key::Period), Modifiers::NONE), Some(Jump::Bar(Dir::Forward)));
    }

    #[test]
    fn shift_steps_by_marker_on_us_and_italian_layouts() {
        let s = Modifiers::SHIFT;
        assert_eq!(jump_from_key(Key::Comma, Some(Key::Comma), s), Some(Jump::Marker(Dir::Back)));
        assert_eq!(jump_from_key(Key::Semicolon, Some(Key::Comma), s), Some(Jump::Marker(Dir::Back)));
        assert_eq!(jump_from_key(Key::Colon, Some(Key::Period), s), Some(Jump::Marker(Dir::Forward)));
        assert_eq!(jump_from_key(Key::Period, Some(Key::Period), s), Some(Jump::Marker(Dir::Forward)));
    }

    #[test]
    fn other_keys_and_modifiers_do_not_jump() {
        assert_eq!(jump_from_key(Key::Comma, Some(Key::Comma), Modifiers::CTRL | Modifiers::COMMAND), None);
        assert_eq!(jump_from_key(Key::Comma, Some(Key::Comma), Modifiers::ALT), None);
        assert_eq!(jump_from_key(Key::Semicolon, Some(Key::Comma), Modifiers::NONE), None);
        assert_eq!(jump_from_key(Key::T, Some(Key::T), Modifiers::NONE), None);
    }

    #[test]
    fn ctrl_t_splits_once() {
        let ctrl = Modifiers::CTRL | Modifiers::COMMAND;
        assert!(split_key(Key::T, false, ctrl));
        assert!(!split_key(Key::T, true, ctrl));
        assert!(!split_key(Key::T, false, ctrl | Modifiers::SHIFT));
        assert!(!split_key(Key::T, false, ctrl | Modifiers::ALT));
        assert!(!split_key(Key::T, false, Modifiers::NONE));
        assert!(!split_key(Key::R, false, ctrl));
    }

    #[test]
    fn keys_need_no_text_field_no_musical_typing_no_drag() {
        assert!(keys_on(false, false, false));
        assert!(!keys_on(true, false, false));
        assert!(!keys_on(false, true, false));
        assert!(!keys_on(false, false, true));
    }

    #[test]
    fn split_target_needs_the_playhead_strictly_inside() {
        let r = region(4.0, 8.0, 1);
        assert_eq!(split_target(&r, 6.0), Some(6.0));
        assert_eq!(split_target(&r, 4.0), None);
        assert_eq!(split_target(&r, 12.0), None);
        assert_eq!(split_target(&r, 2.0), None);
        assert_eq!(split_target(&r, 13.0), None);
        assert_eq!(split_target(&r, 4.0000001), None);
    }

    #[test]
    fn split_target_counts_loop_repeats() {
        let r = region(4.0, 4.0, 2);
        assert_eq!(split_target(&r, 10.0), Some(10.0));
        assert_eq!(split_target(&r, 12.0), None);
    }

    #[test]
    fn split_command_explains_why_it_cannot_split() {
        let p = Project::new("t");
        assert_eq!(split_command(&p, None, 1.0).unwrap_err(), NO_REGION);
        assert_eq!(split_command(&p, Some(99), 1.0).unwrap_err(), NO_REGION);
    }

    #[test]
    fn ctrl_t_split_is_one_undo_step() {
        let mut s = Session::new(false);
        let tid = run(&mut s, "add_track", json!({"kind": "instrument", "name": "Keys"}))["track_id"].as_u64().unwrap();
        let notes = [
            Note { pitch: 60, velocity: 100, start: 1.0, length: 1.0 },
            Note { pitch: 64, velocity: 100, start: 5.0, length: 1.0 },
        ];
        let args = json!({"track_id": tid, "start": 0, "length": 8, "notes": serde_json::to_value(notes).unwrap()});
        let rid = run(&mut s, "create_midi_region", args)["region_id"].as_u64().unwrap();
        let spans = |s: &Session| {
            let t = s.project.tracks.iter().find(|t| t.id == tid).expect("the track is still there");
            t.regions.iter().map(|r| (r.id, r.start, r.length)).collect::<Vec<_>>()
        };

        assert_eq!(split_command(&s.project, Some(rid), 8.0).unwrap_err(), OUTSIDE);
        assert_eq!(split_command(&s.project, Some(rid), 0.0).unwrap_err(), OUTSIDE);
        let c = split_command(&s.project, Some(rid), 3.0).unwrap();
        s.execute(c, Source::Gui).unwrap();
        let after: Vec<_> = spans(&s).iter().map(|&(_, start, length)| (start, length)).collect();
        assert_eq!(after, vec![(0.0, 3.0), (3.0, 5.0)]);

        run(&mut s, "undo", json!({}));
        assert_eq!(spans(&s), vec![(rid, 0.0, 8.0)]);
    }

    #[test]
    fn bar_and_marker_jumps_move_the_playhead_without_an_undo_step() {
        let mut s = Session::new(false);
        run(&mut s, "set_time_signature", json!({"numerator": 3, "denominator": 4}));
        run(&mut s, "add_marker", json!({"beat": 12, "name": "Chorus"}));
        let before = s.can_undo().map(str::to_owned);

        let to = jump_target(&s.project, 0.0, Jump::Bar(Dir::Forward), false);
        assert_eq!(to, 3.0);
        s.execute(Command::SetPlayhead { beat: Some(to), bar: None }, Source::Gui).unwrap();
        assert_eq!(s.transport_json()["beat"], 3.0);

        let to = jump_target(&s.project, 3.0, Jump::Marker(Dir::Forward), false);
        assert_eq!(to, 12.0);
        s.execute(Command::SetPlayhead { beat: Some(to), bar: None }, Source::Gui).unwrap();
        assert_eq!(s.transport_json()["beat"], 12.0);
        assert_eq!(s.can_undo().map(str::to_owned), before);
    }
}
