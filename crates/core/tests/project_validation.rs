//! Time signatures, tempo maps and loaded project files are checked where they enter the project, so the audio
//! thread never sees a bar of 0 beats or a tempo map without points.

use ondina_core::command::Command;
use ondina_core::ops::{self, Outcome};
use ondina_core::tempo::TempoMap;
use ondina_core::{load_project_str, save_project_string, Project, TimeSignature};
use serde_json::{json, Value};

fn apply(p: &mut Project, c: Command) -> Result<Value, String> {
    match ops::apply(p, &c)? {
        Outcome::Done(v) => Ok(v),
        Outcome::NotHandled => panic!("not handled"),
    }
}

fn new_project(ts: &str) -> Command {
    Command::NewProject { name: None, tempo: None, time_signature: Some(ts.into()), key: None }
}

#[test]
fn new_project_refuses_the_time_signatures_set_time_signature_refuses() {
    for bad in ["0/4", "4/0", "4/3", "7/32", "3/5"] {
        let mut p = Project::new("before");
        let err = apply(&mut p, new_project(bad)).unwrap_err();
        assert!(err.contains("time signature"), "{bad}: {err}");
        assert_eq!(p.name, "before", "{bad}: the project must stay as it was");
    }
    let mut p = Project::new("t");
    apply(&mut p, new_project("6/8")).unwrap();
    assert_eq!(p.time_signature, TimeSignature { numerator: 6, denominator: 8 });
}

#[test]
fn an_empty_tempo_map_answers_the_default_tempo_instead_of_panicking() {
    let mut m = TempoMap { points: vec![] };
    assert_eq!(m.bpm_at(3.0), 120.0);
    assert_eq!(m.initial_bpm(), 120.0);
    assert_eq!(m.beat_to_sec(2.0), 1.0);
    assert_eq!(m.sec_to_beat(1.0), 2.0);
    m.set_initial_bpm(90.0);
    assert_eq!(m.points.len(), 1);
    assert_eq!(m.initial_bpm(), 90.0);
    let mut m = TempoMap { points: vec![] };
    m.add_change(4.0, 60.0);
    assert_eq!(m.points[0].beat, 0.0, "the first point is always at beat 0");
    assert_eq!(m.bpm_at(5.0), 60.0);
}

fn saved_with(edit: impl FnOnce(&mut Value)) -> String {
    let mut doc: Value = serde_json::from_str(&save_project_string(&Project::new("file")).unwrap()).unwrap();
    edit(&mut doc["project"]);
    doc.to_string()
}

#[test]
fn a_project_file_with_an_invalid_time_signature_is_refused() {
    for (n, d) in [(0, 4), (4, 0), (4, 3)] {
        let text = saved_with(|p| p["time_signature"] = json!({"numerator": n, "denominator": d}));
        let err = load_project_str(&text).unwrap_err();
        assert!(err.contains("time signature"), "{n}/{d}: {err}");
    }
}

#[test]
fn a_project_file_with_an_empty_tempo_map_is_refused() {
    let text = saved_with(|p| p["tempo"] = json!({"points": []}));
    let err = load_project_str(&text).unwrap_err();
    assert!(err.contains("tempo"), "{err}");
}

#[test]
fn a_loaded_tempo_map_is_put_back_in_range_and_order() {
    let text = saved_with(|p| p["tempo"] = json!({"points": [{"beat": 8.0, "bpm": 5000.0}, {"beat": 0.0, "bpm": 0.0}]}));
    let p = load_project_str(&text).unwrap();
    assert_eq!(p.tempo.points.len(), 2);
    assert_eq!(p.tempo.points[0].beat, 0.0);
    assert_eq!(p.tempo.points[0].bpm, 5.0);
    assert_eq!(p.tempo.points[1].bpm, 999.0);

    let text = saved_with(|p| p["tempo"] = json!({"points": [{"beat": 4.0, "bpm": 100.0}]}));
    let p = load_project_str(&text).unwrap();
    assert_eq!(p.tempo.points[0].beat, 0.0, "the first point moves to beat 0");
}

#[test]
fn a_loaded_cycle_of_no_length_gets_one_bar() {
    let text = saved_with(|p| p["cycle"] = json!({"enabled": true, "start": 8.0, "end": 8.0}));
    let p = load_project_str(&text).unwrap();
    assert_eq!((p.cycle.start, p.cycle.end), (8.0, 12.0));
}

#[test]
fn a_loaded_pattern_with_too_many_steps_is_refused() {
    let mut proj = Project::new("file");
    let t = ops::add_track(&mut proj, ondina_core::TrackKind::Instrument, None, None, None, None, None).unwrap();
    apply(&mut proj, Command::CreatePatternRegion { track_id: t, start: 0.0, steps: Some(16), step_length: None, rows: None, name: None }).unwrap();
    let mut doc: Value = serde_json::from_str(&save_project_string(&proj).unwrap()).unwrap();
    doc["project"]["tracks"][0]["regions"][0]["content"]["steps"] = json!(4_000_000_000u32);
    let err = load_project_str(&doc.to_string()).unwrap_err();
    assert!(err.contains("65536"), "{err}");
}

#[test]
fn the_example_projects_still_load() {
    let dir = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../../examples");
    let mut n = 0;
    for e in std::fs::read_dir(dir).unwrap() {
        let path = e.unwrap().path();
        if ondina_core::is_project_file(&path.display().to_string()) {
            load_project_str(&std::fs::read_to_string(&path).unwrap()).unwrap_or_else(|e| panic!("{}: {e}", path.display()));
            n += 1;
        }
    }
    assert!(n > 0, "no example project found");
}

#[test]
fn a_midi_file_with_zero_beats_per_bar_keeps_the_time_signature() {
    use midly::{Format, Header, MetaMessage, Smf, Timing, TrackEvent, TrackEventKind};
    let track = vec![
        TrackEvent { delta: 0.into(), kind: TrackEventKind::Meta(MetaMessage::TimeSignature(0, 2, 24, 8)) },
        TrackEvent { delta: 0.into(), kind: TrackEventKind::Meta(MetaMessage::EndOfTrack) },
    ];
    let smf = Smf { header: Header::new(Format::SingleTrack, Timing::Metrical(480.into())), tracks: vec![track] };
    let mut bytes = vec![];
    smf.write_std(&mut bytes).unwrap();
    let mut p = Project::new("t");
    let _ = ondina_core::midi_io::import_midi(&mut p, &bytes, 0.0);
    assert_eq!(p.time_signature, TimeSignature { numerator: 4, denominator: 4 });

    // a denominator exponent that would overflow the shift
    let track = vec![
        TrackEvent { delta: 0.into(), kind: TrackEventKind::Meta(MetaMessage::TimeSignature(3, 9, 24, 8)) },
        TrackEvent { delta: 0.into(), kind: TrackEventKind::Meta(MetaMessage::EndOfTrack) },
    ];
    let smf = Smf { header: Header::new(Format::SingleTrack, Timing::Metrical(480.into())), tracks: vec![track] };
    let mut bytes = vec![];
    smf.write_std(&mut bytes).unwrap();
    let mut p = Project::new("t");
    let _ = ondina_core::midi_io::import_midi(&mut p, &bytes, 0.0);
    assert_eq!(p.time_signature, TimeSignature { numerator: 4, denominator: 4 });
}
