//! Automation values are kept in the range of their target when they are written, like the commands that set the
//! same parameter directly: a volume lane of +1000 dB used to turn into inf/NaN in the mix.

use ondina_core::command::Command;
use ondina_core::ops::{self, Outcome};
use ondina_core::{load_project_str, save_project_string, AutomationPoint, Project, TrackKind};
use serde_json::{json, Value};

fn apply(p: &mut Project, c: Command) -> Result<Value, String> {
    match ops::apply(p, &c)? {
        Outcome::Done(v) => Ok(v),
        Outcome::NotHandled => panic!("not handled"),
    }
}

fn pt(beat: f64, value: f32) -> AutomationPoint {
    AutomationPoint { beat, value }
}

fn lane(p: &Project, track: u64, target: &str) -> Vec<f32> {
    let t = p.track(track).unwrap();
    t.automation.iter().find(|l| l.target == target).unwrap().points.iter().map(|p| p.value).collect()
}

/// An instrument track with a synth, a bus and a send to it. Returns (track, synth plug-in id).
fn setup(p: &mut Project) -> (u64, u64) {
    let t = ops::add_track(p, TrackKind::Instrument, None, None, Some("analog_synth".into()), None, None).unwrap();
    let bus = ops::add_track(p, TrackKind::Bus, None, None, None, None, None).unwrap();
    apply(p, Command::AddSend { track_id: t, bus_id: bus, level_db: None, pre_fader: None }).unwrap();
    let pid = p.track(t).unwrap().instrument.as_ref().unwrap().id;
    (t, pid)
}

#[test]
fn set_automation_keeps_every_value_in_the_range_of_its_target() {
    let mut p = Project::new("t");
    let (t, pid) = setup(&mut p);
    let cases: Vec<(String, Vec<f32>, Vec<f32>)> = vec![
        ("volume".into(), vec![1000.0, -1000.0, -6.0], vec![6.0, -96.0, -6.0]),
        ("pan".into(), vec![5.0, -5.0, 0.25], vec![1.0, -1.0, 0.25]),
        ("send:0".into(), vec![100.0, -500.0], vec![6.0, -96.0]),
        ("surround:elevation".into(), vec![200.0, -90.0], vec![90.0, -30.0]),
        ("surround:azimuth".into(), vec![400.0, -400.0], vec![180.0, -180.0]),
        ("surround:spread".into(), vec![3.0, -1.0], vec![1.0, 0.0]),
        (format!("plugin:{pid}:cutoff"), vec![99999.0, 0.0], vec![20000.0, 20.0]),
    ];
    for (target, values, expected) in cases {
        let points: Vec<AutomationPoint> = values.iter().enumerate().map(|(i, v)| pt(i as f64, *v)).collect();
        let v = apply(&mut p, Command::SetAutomation { track_id: t, target: target.clone(), points }).unwrap();
        assert_eq!(lane(&p, t, &target), expected, "{target}");
        let answered: Vec<f32> = v["points"].as_array().unwrap().iter().map(|p| p["value"].as_f64().unwrap() as f32).collect();
        assert_eq!(answered, expected, "{target}: the answer reports the stored values");
    }
}

#[test]
fn add_automation_point_keeps_the_value_in_range() {
    let mut p = Project::new("t");
    let (t, _) = setup(&mut p);
    let v = apply(&mut p, Command::AddAutomationPoint { track_id: t, target: "volume".into(), beat: 0.0, value: 400.0 }).unwrap();
    assert_eq!(lane(&p, t, "volume"), vec![6.0]);
    assert_eq!(v["value"], 6.0);
}

#[test]
fn automation_values_that_are_not_numbers_are_refused() {
    let mut p = Project::new("t");
    let (t, _) = setup(&mut p);
    let before = serde_json::to_string(&p).unwrap();
    for bad in [f32::INFINITY, f32::NEG_INFINITY, f32::NAN] {
        let err = apply(&mut p, Command::SetAutomation { track_id: t, target: "volume".into(), points: vec![pt(0.0, 0.0), pt(1.0, bad)] }).unwrap_err();
        assert!(err.contains("finite"), "{err}");
        let err = apply(&mut p, Command::AddAutomationPoint { track_id: t, target: "pan".into(), beat: 0.0, value: bad }).unwrap_err();
        assert!(err.contains("finite"), "{err}");
    }
    let err = apply(&mut p, Command::SetAutomation { track_id: t, target: "volume".into(), points: vec![pt(f64::NAN, 0.0)] }).unwrap_err();
    assert!(err.contains("finite"), "{err}");
    assert_eq!(serde_json::to_string(&p).unwrap(), before);
}

#[test]
fn automation_out_of_range_in_a_project_file_is_put_back_in_range() {
    let mut p = Project::new("t");
    let (t, _) = setup(&mut p);
    apply(&mut p, Command::SetAutomation { track_id: t, target: "volume".into(), points: vec![pt(0.0, 0.0)] }).unwrap();
    let mut doc: Value = serde_json::from_str(&save_project_string(&p).unwrap()).unwrap();
    doc["project"]["tracks"][0]["automation"][0]["points"] = json!([{"beat": 0.0, "value": 1000.0}]);
    let loaded = load_project_str(&doc.to_string()).unwrap();
    assert_eq!(lane(&loaded, t, "volume"), vec![6.0]);
}
