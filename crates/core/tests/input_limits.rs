//! Sizes that come from a command (an MCP client, the assistant) are bounded: a number that would allocate
//! gigabytes or loop for hours is refused with a clear error, and the project stays as it was.

use ondina_core::command::Command;
use ondina_core::generate::{ArpMode, BassStyle};
use ondina_core::ops::{self, Outcome};
use ondina_core::{Project, TrackKind};
use serde_json::Value;

fn apply(p: &mut Project, c: Command) -> Result<Value, String> {
    match ops::apply(p, &c)? {
        Outcome::Done(v) => Ok(v),
        Outcome::NotHandled => panic!("not handled"),
    }
}

fn instrument(p: &mut Project) -> u64 {
    ops::add_track(p, TrackKind::Instrument, None, None, None, None, None).unwrap()
}

fn json(p: &Project) -> String {
    serde_json::to_string(p).unwrap()
}

#[test]
fn resize_region_refuses_a_pattern_longer_than_the_step_limit() {
    let mut p = Project::new("t");
    let t = instrument(&mut p);
    let rid = apply(&mut p, Command::CreatePatternRegion { track_id: t, start: 0.0, steps: Some(16), step_length: None, rows: None, name: None }).unwrap()["region_id"].as_u64().unwrap();
    let before = json(&p);
    let err = apply(&mut p, Command::ResizeRegion { region_id: rid, length: 1e9 }).unwrap_err();
    assert!(err.contains("65536"), "{err}");
    assert_eq!(json(&p), before);
    // the limit itself is fine: 65536 sixteenths
    let v = apply(&mut p, Command::ResizeRegion { region_id: rid, length: 65_536.0 * 0.25 }).unwrap();
    assert_eq!(v["length"], 16_384.0);
}

#[test]
fn create_live_cell_refuses_a_scene_past_the_scene_limit() {
    let mut p = Project::new("t");
    let t = instrument(&mut p);
    let before = json(&p);
    let err = apply(&mut p, Command::CreateLiveCell { track_id: t, scene: 4_000_000_000, length: None, notes: None, from_region_id: None, name: None }).unwrap_err();
    assert!(err.contains("scene"), "{err}");
    assert_eq!(json(&p), before);
    apply(&mut p, Command::CreateLiveCell { track_id: t, scene: ondina_core::MAX_SCENES - 1, length: None, notes: None, from_region_id: None, name: None }).unwrap();
    assert_eq!(p.live.scenes.len(), ondina_core::MAX_SCENES);
}

#[test]
fn generators_refuse_more_bars_than_the_limit() {
    let mut p = Project::new("t");
    let t = instrument(&mut p);
    let before = json(&p);
    let err = apply(&mut p, Command::GenerateBass { track_id: t, start: 0.0, bars: 4_000_000_000, style: BassStyle::Root, chords: Some(vec!["C".into()]), beats_per_chord: None, octave: None, seed: Some(1) }).unwrap_err();
    assert!(err.contains("bars"), "{err}");
    let err = apply(&mut p, Command::GenerateDrums { track_id: t, start: 0.0, bars: 513, style: ondina_core::generate::DrumStyle::Rock, intensity: None, fill: None, seed: Some(1) }).unwrap_err();
    assert!(err.contains("512"), "{err}");
    assert_eq!(json(&p), before);
}

#[test]
fn generators_refuse_a_chord_length_that_would_loop_forever() {
    let mut p = Project::new("t");
    let t = instrument(&mut p);
    for per in [0.0, -1.0, 1e-9] {
        let err = apply(&mut p, Command::GenerateBass { track_id: t, start: 0.0, bars: 1, style: BassStyle::Root, chords: Some(vec!["C".into()]), beats_per_chord: Some(per), octave: None, seed: Some(1) }).unwrap_err();
        assert!(err.contains("beats_per_chord"), "{per}: {err}");
    }
}

#[test]
fn an_arpeggio_with_a_huge_octave_count_stays_in_the_midi_range() {
    let mut p = Project::new("t");
    let t = instrument(&mut p);
    let rid = apply(&mut p, Command::GenerateArpeggio { track_id: t, start: 0.0, bars: 1, mode: ArpMode::Up, rate: None, octaves: Some(4_000_000_000), chords: Some(vec!["C".into()]), beats_per_chord: None, center_pitch: None, seed: Some(1) }).unwrap()["region_id"].as_u64().unwrap();
    let (_, r) = p.find_region(rid).unwrap();
    let ondina_core::RegionContent::Midi(m) = &r.content else { panic!() };
    assert!(!m.notes.is_empty());
    assert!(m.notes.iter().all(|n| n.pitch <= 127));
}
