//! Session players fill every bar they are asked for, and the step sequencer commands refuse input they would
//! otherwise correct without saying so.

use ondina_core::command::Command;
use ondina_core::generate::{BassStyle, CompStyle};
use ondina_core::ops::{self, resolve_chords, Outcome};
use ondina_core::{Project, RegionContent, TrackKind};
use serde_json::Value;

fn apply(p: &mut Project, c: Command) -> Result<Value, String> {
    match ops::apply(p, &c)? {
        Outcome::Done(v) => Ok(v),
        Outcome::NotHandled => panic!("not handled"),
    }
}

/// A project with a 4-bar chord track (i VI III VII in A minor, one bar each) and an instrument track.
fn with_chord_track() -> (Project, u64) {
    let mut p = Project::new("t");
    apply(&mut p, Command::SetKey { key: "A minor".into() }).unwrap();
    let chords = ["i", "VI", "III", "VII"].iter().map(|s| s.to_string()).collect();
    apply(&mut p, Command::SetChordProgression { start: 0.0, chords, beats_per_chord: Some(4.0), replace: None }).unwrap();
    let t = ops::add_track(&mut p, TrackKind::Instrument, None, None, None, None, None).unwrap();
    (p, t)
}

fn notes(p: &Project, rid: u64) -> Vec<ondina_core::Note> {
    let (_, r) = p.find_region(rid).unwrap();
    let RegionContent::Midi(m) = &r.content else { panic!("not MIDI") };
    m.notes.clone()
}

#[test]
fn the_chord_track_repeats_to_fill_the_bars_asked_for() {
    let (p, _) = with_chord_track();
    let tc = resolve_chords(&p, 0.0, 8, &None, None).unwrap();
    let roots: Vec<u8> = tc.iter().map(|c| c.chord.root).collect();
    assert_eq!(roots, vec![9, 5, 0, 7, 9, 5, 0, 7]);
    assert_eq!(tc.last().map(|c| c.start + c.length), Some(32.0));
    // starting inside the progression continues it: bar 3 is III
    let tc = resolve_chords(&p, 8.0, 4, &None, None).unwrap();
    let roots: Vec<u8> = tc.iter().map(|c| c.chord.root).collect();
    assert_eq!(roots, vec![0, 7, 9, 5]);
    assert_eq!(tc[0].start, 0.0);
}

#[test]
fn bass_and_chords_fill_every_bar_past_the_end_of_the_chord_track() {
    let (mut p, t) = with_chord_track();
    let bass = apply(&mut p, Command::GenerateBass { track_id: t, start: 0.0, bars: 8, style: BassStyle::Syncopated, chords: None, beats_per_chord: None, octave: None, seed: Some(7) }).unwrap();
    assert_eq!(bass["bars"], 8);
    let comp = apply(&mut p, Command::GenerateChords { track_id: t, start: 0.0, bars: 8, style: CompStyle::Sustained, chords: None, beats_per_chord: None, center_pitch: None, seed: Some(7) }).unwrap();
    for v in [bass, comp] {
        let ns = notes(&p, v["region_id"].as_u64().unwrap());
        for bar in 0..8 {
            let (b0, b1) = (bar as f64 * 4.0, bar as f64 * 4.0 + 4.0);
            assert!(ns.iter().any(|n| n.start >= b0 && n.start < b1), "bar {} has no notes: {v}", bar + 1);
        }
    }
}

fn pattern(p: &mut Project, t: u64) -> u64 {
    apply(p, Command::CreatePatternRegion { track_id: t, start: 0.0, steps: Some(16), step_length: None, rows: None, name: None }).unwrap()["region_id"].as_u64().unwrap()
}

#[test]
fn set_pattern_swing_refuses_a_value_out_of_range() {
    let mut p = Project::new("t");
    let t = ops::add_track(&mut p, TrackKind::Instrument, None, None, None, None, None).unwrap();
    let rid = pattern(&mut p, t);
    for bad in [3.0, -0.1, 1.01, f32::NAN] {
        let err = apply(&mut p, Command::SetPatternSwing { region_id: rid, swing: bad }).unwrap_err();
        assert!(err.contains("swing"), "{bad}: {err}");
    }
    let v = apply(&mut p, Command::SetPatternSwing { region_id: rid, swing: 0.5 }).unwrap();
    assert_eq!(v["swing"], 0.5);
}

#[test]
fn set_pattern_row_refuses_a_string_that_does_not_match_the_steps() {
    let mut p = Project::new("t");
    let t = ops::add_track(&mut p, TrackKind::Instrument, None, None, None, None, None).unwrap();
    let rid = pattern(&mut p, t);
    apply(&mut p, Command::SetPatternRow { region_id: rid, row: 0, pattern: "x...x...x...x...".into() }).unwrap();
    let before = serde_json::to_string(&p).unwrap();
    for bad in ["o.o", "x...x...x...x...x"] {
        let err = apply(&mut p, Command::SetPatternRow { region_id: rid, row: 0, pattern: bad.into() }).unwrap_err();
        assert!(err.contains("16"), "{bad}: {err}");
    }
    assert_eq!(serde_json::to_string(&p).unwrap(), before);
    // bar lines and spaces are only for reading
    apply(&mut p, Command::SetPatternRow { region_id: rid, row: 0, pattern: "o.o. o.o. | o.o. o.o.".into() }).unwrap();
}
