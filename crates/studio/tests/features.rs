//! End-to-end tests of the extended feature set, driven exactly like an MCP client would
//! (tool name + JSON arguments).

use std::path::{Path, PathBuf};

use ondina_core::command::{tool_specs, Command};
use ondina_studio::{Session, Source};
use serde_json::{json, Value};

fn call(s: &mut Session, name: &str, args: Value) -> Result<Value, String> {
    let cmd = Command::from_tool_call(name, &args)?;
    s.execute(cmd, Source::Mcp)
}

fn ok(s: &mut Session, name: &str, args: Value) -> Value {
    call(s, name, args.clone()).unwrap_or_else(|e| panic!("{name} {args} failed: {e}"))
}

fn tmp(name: &str) -> PathBuf {
    let d = std::env::temp_dir().join("ondina_features");
    std::fs::create_dir_all(&d).unwrap();
    d.join(name)
}

fn write_wav(path: &Path, ch: &[Vec<f32>], sr: u32) {
    let spec = hound::WavSpec { channels: ch.len() as u16, sample_rate: sr, bits_per_sample: 32, sample_format: hound::SampleFormat::Float };
    let mut w = hound::WavWriter::create(path, spec).unwrap();
    for i in 0..ch[0].len() {
        for c in ch {
            w.write_sample(c[i]).unwrap();
        }
    }
    w.finalize().unwrap();
}

/// 8 s at 44.1 kHz: melody (C4 E4 G4 C5 per beat) + bass at 82 Hz + clicks at 100 bpm.
fn song_wav() -> PathBuf {
    let sr = 44100u32;
    let n = sr as usize * 8;
    let beat = 0.6;
    let melody = [261.63f32, 329.63, 392.0, 523.25];
    let mut l = vec![0.0f32; n];
    let mut mel = vec![0.0f32; n];
    for i in 0..n {
        let t = i as f32 / sr as f32;
        let k = (t / beat) as usize;
        let ph = t - k as f32 * beat;
        let f = melody[k % 4];
        let env = if ph < beat * 0.85 { 1.0 } else { 0.0 };
        mel[i] = (2.0 * std::f32::consts::PI * f * t).sin() * 0.3 * env;
        let bass = (2.0 * std::f32::consts::PI * 82.0 * t).sin() * 0.25;
        let click = if ph < 0.004 { (1.0 - ph / 0.004) * if (i % 2) == 0 { 0.8 } else { -0.8 } } else { 0.0 };
        l[i] = mel[i] + bass + click;
    }
    let p = tmp("song.wav");
    write_wav(&p, &[l.clone(), l], sr);
    let m = tmp("melody.wav");
    write_wav(&m, &[mel], sr);
    p
}

#[test]
fn every_tool_is_implemented() {
    let mut s = Session::new(false);
    for spec in tool_specs() {
        // minimal arguments: fill required fields with placeholders
        let mut args = serde_json::Map::new();
        if let (Some(req), Some(props)) = (spec.input_schema.get("required").and_then(|r| r.as_array()), spec.input_schema.get("properties")) {
            for r in req {
                let k = r.as_str().unwrap();
                let p = &props[k];
                let v = if let Some(e) = p.get("enum").and_then(|e| e.as_array()).and_then(|a| a.first()) {
                    e.clone()
                } else if let Some(e) = p.get("oneOf").or(p.get("anyOf")).and_then(|a| a.as_array()).and_then(|a| a.first()).and_then(|x| x.get("enum")).and_then(|e| e.as_array()).and_then(|a| a.first()) {
                    e.clone()
                } else {
                    match p.get("type").and_then(|t| t.as_str()).unwrap_or("string") {
                        "integer" | "number" => json!(999_999),
                        "boolean" => json!(false),
                        "array" => json!([]),
                        "object" => json!({}),
                        _ => json!(""),
                    }
                };
                args.insert(k.to_string(), v);
            }
        }
        if matches!(spec.name.as_str(), "new_project" | "open_project" | "record" | "connect_midi_input" | "set_audio_device" | "scan_vst3" | "set_ai_settings" | "play") {
            continue; // covered elsewhere / environment dependent
        }
        if let Err(e) = call(&mut s, &spec.name, Value::Object(args.clone())) {
            assert!(!e.contains("not implemented"), "tool {} is not implemented", spec.name);
        }
    }
    assert!(tool_specs().len() >= 150, "expected the full API, got {}", tool_specs().len());
}

#[test]
fn open_project_starts_a_fresh_history() {
    let mut s = Session::new(false);
    ok(&mut s, "add_track", json!({"kind": "instrument", "patch": "Lush Pad"}));
    let path = tmp("fresh_history_open.ondina");
    ok(&mut s, "save_project", json!({"path": path.display().to_string()}));
    ok(&mut s, "add_track", json!({"kind": "instrument", "patch": "Lush Pad"}));
    assert_eq!(s.project.tracks.len(), 2);
    assert!(s.dirty);
    let rev = s.revision;

    ok(&mut s, "open_project", json!({"path": path.display().to_string(), "discard_unsaved": true}));
    assert_eq!(s.project.tracks.len(), 1);
    assert!(s.revision > rev, "the GUI must see a new revision");
    assert_eq!(s.engine.project().tracks.len(), 1, "the engine must play the opened project");
    assert_eq!(s.can_undo(), None, "opening a project must not be undoable");
    assert_eq!(s.can_redo(), None);
    assert!(!s.dirty, "a freshly opened project has no unsaved changes");
    assert_eq!(call(&mut s, "undo", json!({})).unwrap_err(), "nothing to undo");
    assert_eq!(s.project.tracks.len(), 1);

    // editing after the open is undoable as usual
    ok(&mut s, "add_track", json!({"kind": "instrument", "patch": "Lush Pad"}));
    assert!(s.dirty);
    ok(&mut s, "undo", json!({}));
    assert_eq!(s.project.tracks.len(), 1);
    assert_eq!(call(&mut s, "undo", json!({})).unwrap_err(), "nothing to undo");
}

#[test]
fn new_project_starts_a_fresh_history() {
    let mut s = Session::new(false);
    ok(&mut s, "add_track", json!({"kind": "instrument", "patch": "Lush Pad"}));
    assert!(s.can_undo().is_some());
    assert!(s.dirty);
    let rev = s.revision;

    ok(&mut s, "new_project", json!({"name": "Second", "tempo": 90, "discard_unsaved": true}));
    assert!(s.project.tracks.is_empty());
    assert!(s.revision > rev);
    assert_eq!(s.engine.project().name, "Second", "the engine must play the new project");
    assert!(s.engine.project().tracks.is_empty());
    assert_eq!(s.can_undo(), None, "a new project must not be undoable");
    assert_eq!(s.can_redo(), None);
    assert!(!s.dirty);
    assert!(s.path.is_none());
    assert_eq!(call(&mut s, "undo", json!({})).unwrap_err(), "nothing to undo");
}

#[test]
fn failed_new_project_keeps_the_current_project() {
    let wav = tmp("fresh_history_keep.wav");
    write_wav(&wav, &[vec![0.1f32; 4410]], 44100);
    let mut s = Session::new(false);
    ok(&mut s, "import_audio", json!({"path": wav.display().to_string(), "start": 0}));
    let path = tmp("fresh_history_keep.ondina");
    ok(&mut s, "save_project", json!({"path": path.display().to_string()}));
    assert_eq!(s.pool.len(), 1);

    let e = call(&mut s, "new_project", json!({"time_signature": "4-4"})).unwrap_err();
    assert!(e.contains("time signature"), "{e}");
    assert_eq!(s.path.as_deref(), Some(path.as_path()), "a failed new_project must keep the file path");
    assert_eq!(s.pool.len(), 1, "a failed new_project must keep the audio");
    assert_eq!(s.project.tracks.len(), 1);
    assert_eq!(s.can_undo(), Some("import_audio"));
    assert!(!s.dirty);
}

#[test]
fn open_and_new_are_refused_while_recording_outside_the_gui() {
    let mut s = Session::new(false);
    ok(&mut s, "set_tempo", json!({"bpm": 240}));
    ok(&mut s, "set_count_in", json!({"bars": 0}));
    let t = ok(&mut s, "add_track", json!({"kind": "instrument", "patch": "Lush Pad"}))["track_id"].as_u64().unwrap();
    ok(&mut s, "set_record_arm", json!({"track_id": t, "armed": true}));
    let other = tmp("refused_while_recording.ondina");
    ok(&mut s, "save_project", json!({"path": other.display().to_string()}));
    ok(&mut s, "add_track", json!({"kind": "instrument", "patch": "Lush Pad"}));
    ok(&mut s, "record", json!({"from_beat": 0}));
    ok(&mut s, "send_live_midi", json!({"track_id": t, "events": [{"type": "note_on", "number": 60, "value": 100}, {"type": "note_off", "number": 60, "delay_ms": 200}]}));
    std::thread::sleep(std::time::Duration::from_millis(400));
    let (tracks, path, document, revision) = (s.project.tracks.len(), s.path.clone(), s.document, s.revision);

    let e = call(&mut s, "open_project", json!({"path": other.display().to_string()})).unwrap_err();
    assert!(e.contains("recording in progress"), "{e}");
    for src in [Source::Mcp, Source::Ai, Source::Script] {
        let c = Command::from_tool_call("new_project", &json!({"name": "Elsewhere"})).unwrap();
        let e = s.execute(c, src).unwrap_err();
        assert!(e.contains("recording in progress"), "{src:?}: {e}");
    }
    // nothing changed and the recording is still running
    assert_eq!(s.project.tracks.len(), tracks);
    assert_ne!(s.project.name, "Elsewhere");
    assert_eq!(s.path, path);
    assert_eq!(s.document, document);
    assert_eq!(s.revision, revision);
    assert_eq!(ok(&mut s, "get_transport", json!({}))["recording"], true);

    // stop keeps the take in the project it was recorded for
    let r = ok(&mut s, "stop", json!({}));
    let created = r["recorded_regions"].as_array().unwrap();
    assert!(!created.is_empty(), "{r}");
    let rid = created[0].as_u64().unwrap();
    assert!(s.project.track(t).unwrap().regions.iter().any(|g| g.id == rid));
}

#[test]
fn new_project_stops_the_transport_and_returns_to_the_start() {
    let mut s = Session::new(false);
    let t = ok(&mut s, "add_track", json!({"kind": "instrument", "patch": "Lush Pad"}))["track_id"].as_u64().unwrap();
    let c = ok(&mut s, "create_live_cell", json!({"track_id": t, "scene": 0, "length": 4, "notes": [{"pitch": 60, "velocity": 100, "start": 0, "length": 1}]}))["cell_id"].as_u64().unwrap();
    ok(&mut s, "set_live_quantize", json!({"beats": 0}));
    ok(&mut s, "play", json!({"from_beat": 8}));
    ok(&mut s, "launch_cell", json!({"cell_id": c}));
    std::thread::sleep(std::time::Duration::from_millis(300));
    let live = ok(&mut s, "get_live_loops", json!({}));
    assert!(!live["playing"].as_array().unwrap().is_empty(), "{live}");

    ok(&mut s, "new_project", json!({"name": "Fresh", "discard_unsaved": true}));
    std::thread::sleep(std::time::Duration::from_millis(100));
    let tr = ok(&mut s, "get_transport", json!({}));
    assert_eq!(tr["playing"], false, "{tr}");
    assert_eq!(tr["beat"].as_f64().unwrap(), 0.0, "{tr}");
    let live = ok(&mut s, "get_live_loops", json!({}));
    assert!(live["playing"].as_array().unwrap().is_empty(), "{live}");
}

#[test]
fn transport_changes_are_visible_in_get_transport_at_once() {
    let mut s = Session::new(false);
    for i in 0..100 {
        let beat = (i % 9 * 4 + 1) as f64;
        ok(&mut s, "set_playhead", json!({"beat": beat}));
        let tr = ok(&mut s, "get_transport", json!({}));
        assert_eq!(tr["beat"].as_f64().unwrap(), beat, "{i}: {tr}");
    }
    for i in 0..50 {
        ok(&mut s, "play", json!({}));
        assert_eq!(ok(&mut s, "get_transport", json!({}))["playing"], true, "play {i}");
        ok(&mut s, "stop", json!({}));
        assert_eq!(ok(&mut s, "get_transport", json!({}))["playing"], false, "stop {i}");
    }
    ok(&mut s, "play", json!({"from_beat": 8}));
    let tr = ok(&mut s, "get_transport", json!({}));
    assert_eq!(tr["playing"], true, "{tr}");
    let b = tr["beat"].as_f64().unwrap();
    assert!((8.0..9.0).contains(&b), "{tr}");
    ok(&mut s, "stop", json!({}));
}

#[test]
fn gui_open_during_a_recording_drops_the_take_instead_of_moving_it() {
    let mut s = Session::new(false);
    ok(&mut s, "set_count_in", json!({"bars": 0}));
    let t = ok(&mut s, "add_track", json!({"kind": "instrument", "patch": "Lush Pad"}))["track_id"].as_u64().unwrap();
    ok(&mut s, "set_record_arm", json!({"track_id": t, "armed": true}));
    let path = tmp("gui_open_during_recording.ondina");
    ok(&mut s, "save_project", json!({"path": path.display().to_string()}));
    ok(&mut s, "record", json!({"from_beat": 0}));
    ok(&mut s, "send_live_midi", json!({"track_id": t, "events": [{"type": "note_on", "number": 64, "value": 100}, {"type": "note_off", "number": 64, "delay_ms": 100}]}));
    std::thread::sleep(std::time::Duration::from_millis(300));

    s.execute(Command::OpenProject { path: path.display().to_string(), discard_unsaved: None }, Source::Gui).unwrap();
    assert!(!s.engine.is_recording(), "the recording must not survive the open");
    // the reopened project has the same armed track: a take written into it would show up here
    assert!(s.project.tracks.iter().all(|tr| tr.regions.is_empty()));
    let r = ok(&mut s, "stop", json!({}));
    assert!(r.get("recorded_regions").is_none(), "{r}");
    assert!(s.project.tracks.iter().all(|tr| tr.regions.is_empty()));

    // the same for New
    ok(&mut s, "record", json!({"from_beat": 0}));
    s.execute(Command::NewProject { name: None, tempo: None, time_signature: None, key: None, discard_unsaved: None }, Source::Gui).unwrap();
    assert!(!s.engine.is_recording());
}

#[test]
fn flex_stems_tempo_transcription() {
    let wav = song_wav();
    let mut s = Session::new(false);
    ok(&mut s, "set_tempo", json!({"bpm": 100}));
    let imp = ok(&mut s, "import_audio", json!({"path": wav.display().to_string(), "start": 0}));
    let rid = imp["region_id"].as_u64().unwrap();
    let t = ok(&mut s, "detect_tempo", json!({"region_id": rid}));
    let bpm = t["bpm"].as_f64().unwrap();
    assert!((bpm - 100.0).abs() < 3.0 || (bpm - 200.0).abs() < 5.0 || (bpm - 50.0).abs() < 2.0, "tempo {bpm}");
    let before = s.project.find_region(rid).unwrap().1.length;
    ok(&mut s, "time_stretch_region", json!({"region_id": rid, "ratio": 1.25}));
    let after = s.project.find_region(rid).unwrap().1.length;
    assert!((after / before - 1.25).abs() < 0.02, "{before} -> {after}");
    ok(&mut s, "pitch_shift_region", json!({"region_id": rid, "semitones": -2}));
    assert!((s.project.find_region(rid).unwrap().1.length - after).abs() < 0.02);
    ok(&mut s, "varispeed_region", json!({"region_id": rid, "percent": 200}));
    assert!((s.project.find_region(rid).unwrap().1.length - after / 2.0).abs() < 0.05);
    let n = ok(&mut s, "normalize_region", json!({"region_id": rid, "target_db": -3}));
    assert!(n["gain_db"].as_f64().is_some());
    // stems
    let imp2 = ok(&mut s, "import_audio", json!({"path": wav.display().to_string(), "start": 0}));
    let st = ok(&mut s, "split_stems", json!({"region_id": imp2["region_id"]}));
    let stems = st["stems"].as_array().unwrap();
    assert_eq!(stems.len(), 4);
    let rms = |name: &str| stems.iter().find(|x| x["stem"] == name).unwrap()["rms_db"].as_f64().unwrap();
    assert!(rms("Bass") > -40.0 && rms("Drums") > -60.0, "{st}");
    // transcription + pitch correction on the clean melody
    let mel = tmp("melody.wav");
    let imp3 = ok(&mut s, "import_audio", json!({"path": mel.display().to_string(), "start": 0}));
    let m = ok(&mut s, "audio_to_midi", json!({"region_id": imp3["region_id"]}));
    let region = ok(&mut s, "get_region", json!({"region_id": m["region_id"]}));
    let pitches: Vec<u64> = region["notes"].as_array().unwrap().iter().map(|n| n["pitch"].as_u64().unwrap()).take(4).collect();
    assert_eq!(pitches, vec![60, 64, 67, 72], "{region}");
    ok(&mut s, "set_key", json!({"key": "C major"}));
    ok(&mut s, "pitch_correct_region", json!({"region_id": imp3["region_id"], "strength": 1.0}));
}

#[test]
fn live_loops_play_and_commit() {
    let mut s = Session::new(false); // null device clock runs in real time
    ok(&mut s, "set_tempo", json!({"bpm": 240}));
    let t = ok(&mut s, "add_track", json!({"kind": "instrument", "patch": "Pop Drum Kit"}))["track_id"].as_u64().unwrap();
    let r = ok(&mut s, "generate_drums", json!({"track_id": t, "start": 0, "bars": 1, "style": "house", "seed": 1}));
    let c = ok(&mut s, "create_live_cell", json!({"track_id": t, "scene": 0, "from_region_id": r["region_id"]}))["cell_id"].as_u64().unwrap();
    ok(&mut s, "set_live_quantize", json!({"beats": 0}));
    ok(&mut s, "launch_cell", json!({"cell_id": c}));
    std::thread::sleep(std::time::Duration::from_millis(1500));
    let st = ok(&mut s, "get_live_loops", json!({}));
    assert!(st["playing"].as_array().unwrap().iter().any(|p| p["cell_id"] == c), "{st}");
    let meters = ok(&mut s, "get_meters", json!({}));
    assert!(meters.to_string().contains("peak_db"), "{meters}");
    let commit = ok(&mut s, "commit_live_performance", json!({}));
    assert!(!commit["created_regions"].as_array().unwrap().is_empty(), "{commit}");
    ok(&mut s, "stop_live_cells", json!({}));
    ok(&mut s, "stop", json!({}));
}

#[test]
fn cycle_recording_creates_takes_and_comp() {
    let mut s = Session::new(false);
    ok(&mut s, "set_tempo", json!({"bpm": 240})); // 1 s per 4/4 bar
    ok(&mut s, "set_count_in", json!({"bars": 0}));
    let t = ok(&mut s, "add_track", json!({"kind": "instrument", "patch": "Lush Pad"}))["track_id"].as_u64().unwrap();
    ok(&mut s, "set_record_arm", json!({"track_id": t, "armed": true}));
    ok(&mut s, "set_cycle", json!({"enabled": true, "start": 0, "end": 4}));
    ok(&mut s, "record", json!({"from_beat": 0}));
    // play a note in each of three cycle passes
    for pitch in [60, 64, 67] {
        std::thread::sleep(std::time::Duration::from_millis(200));
        ok(&mut s, "send_live_midi", json!({"track_id": t, "events": [{"type": "note_on", "number": pitch, "value": 100}, {"type": "note_off", "number": pitch, "delay_ms": 300}]}));
        std::thread::sleep(std::time::Duration::from_millis(800));
    }
    let r = ok(&mut s, "stop", json!({}));
    let created = r["recorded_regions"].as_array().unwrap();
    assert!(created.len() >= 2, "expected several takes: {r}");
    let takes = ok(&mut s, "list_takes", json!({"track_id": t}));
    let group = takes["take_groups"][0].as_array().unwrap().clone();
    assert!(group.len() >= 2, "{takes}");
    let a = group[0]["region_id"].as_u64().unwrap();
    let b = group[1]["region_id"].as_u64().unwrap();
    let comp = ok(&mut s, "comp_takes", json!({"track_id": t, "segments": [{"region_id": a, "from_beat": 0, "to_beat": 2}, {"region_id": b, "from_beat": 2, "to_beat": 4}]}));
    assert_eq!(comp["comp_region_ids"].as_array().unwrap().len(), 2);
}

#[test]
fn surround_binaural_score_routes() {
    let mut s = Session::new(false);
    let t = ok(&mut s, "add_track", json!({"kind": "instrument", "patch": "Lush Pad"}))["track_id"].as_u64().unwrap();
    let r = ok(&mut s, "create_midi_region", json!({"track_id": t, "start": 0, "length": 8, "notes": [{"pitch": 60, "velocity": 100, "start": 0, "length": 4}, {"pitch": 64, "velocity": 100, "start": 4, "length": 4}]}))["region_id"].as_u64().unwrap();
    ok(&mut s, "set_surround_format", json!({"format": "7.1.4"}));
    ok(&mut s, "set_surround_pan", json!({"track_id": t, "azimuth": 120, "elevation": 40, "spread": 0.1}));
    for (fmt, n) in [("5.1", 6u16), ("7.1", 8), ("7.1.4", 12)] {
        let p = tmp(&format!("surround_{fmt}.wav"));
        let v = ok(&mut s, "bounce_surround", json!({"path": p.display().to_string(), "format": fmt}));
        let spec = hound::WavReader::open(&p).unwrap().spec();
        assert_eq!(spec.channels, n, "{v}");
    }
    // the rear-right height source must be loudest in a rear/height-right channel
    let v = ok(&mut s, "bounce_surround", json!({"path": tmp("s714.wav").display().to_string(), "format": "7.1.4"}));
    let peaks = v["channel_peaks"].as_array().unwrap();
    let loudest = peaks.iter().max_by(|a, b| a["peak_db"].as_f64().unwrap().partial_cmp(&b["peak_db"].as_f64().unwrap()).unwrap()).unwrap();
    assert!(["Rs", "Rrs", "Rtr", "Rtf"].contains(&loudest["channel"].as_str().unwrap()), "{v}");
    let b = tmp("binaural.wav");
    ok(&mut s, "bounce_binaural", json!({"path": b.display().to_string()}));
    assert!(b.exists());
    // score
    ok(&mut s, "set_lyrics", json!({"region_id": r, "text": "la la"}));
    let x = tmp("score.musicxml");
    ok(&mut s, "export_music_xml", json!({"path": x.display().to_string()}));
    let xml = std::fs::read_to_string(&x).unwrap();
    assert!(xml.contains("<score-partwise") && xml.contains("<text>la</text>"));
    // environment routing
    ok(&mut s, "set_midi_routes", json!({"routes": [{"name": "split", "note_max": 59, "transpose": 12, "track_id": t}]}));
    assert_eq!(ok(&mut s, "get_midi_routes", json!({}))["routes"].as_array().unwrap().len(), 1);
}

#[test]
fn movie_scoring_with_ffmpeg() {
    let Some(ff) = ondina_studio::session_ext::ffmpeg_path() else {
        eprintln!("ffmpeg not available: skipping");
        return;
    };
    let mp4 = tmp("test_movie.mp4");
    let st = std::process::Command::new(&ff)
        .args(["-hide_banner", "-loglevel", "error", "-y", "-f", "lavfi", "-i", "testsrc=duration=4:size=320x240:rate=25", "-f", "lavfi", "-i", "sine=frequency=440:duration=4", "-c:v", "libx264", "-pix_fmt", "yuv420p", "-c:a", "aac", "-shortest"])
        .arg(&mp4)
        .status()
        .unwrap();
    assert!(st.success());
    let mut s = Session::new(false);
    let m = ok(&mut s, "load_movie", json!({"path": mp4.display().to_string(), "offset_sec": 0}));
    assert!(m["duration_sec"].as_f64().unwrap() > 3.5, "{m}");
    ok(&mut s, "set_frame_rate", json!({"fps": 25}));
    let tr = ok(&mut s, "get_transport", json!({}));
    let _ = tr;
    let png = tmp("frame.png");
    ok(&mut s, "extract_movie_frame", json!({"path": png.display().to_string(), "beat": 2}));
    assert!(std::fs::metadata(&png).unwrap().len() > 1000);
    let t = ok(&mut s, "add_track", json!({"kind": "instrument", "patch": "Lush Pad"}))["track_id"].as_u64().unwrap();
    ok(&mut s, "create_midi_region", json!({"track_id": t, "start": 0, "length": 8, "notes": [{"pitch": 57, "velocity": 100, "start": 0, "length": 8}]}));
    let out = tmp("scored.mp4");
    ok(&mut s, "export_movie", json!({"path": out.display().to_string()}));
    assert!(std::fs::metadata(&out).unwrap().len() > 10_000);
}

#[test]
fn third_party_vst3_instrument_and_effect() {
    let root = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../tools/vst3");
    let dexed = root.join("Dexed.vst3");
    let aw = root.join("Airwindows Consolidated.vst3");
    if !dexed.exists() || !aw.exists() {
        eprintln!("test VST3 plug-ins not present: skipping");
        return;
    }
    let mut s = Session::new(false);
    s.settings.vst3_folders = vec![root.display().to_string()];
    let scan = ok(&mut s, "scan_vst3", json!({"extra_paths": [root.display().to_string()]}));
    assert!(scan["plugins"].as_array().unwrap().len() >= 2, "{scan}");
    let t = ok(&mut s, "add_track", json!({"kind": "instrument", "name": "Dexed"}))["track_id"].as_u64().unwrap();
    let v = ok(&mut s, "add_vst3", json!({"track_id": t, "path": dexed.display().to_string()}));
    assert_eq!(v["as_instrument"], true);
    let pid = v["plugin_id"].as_u64().unwrap();
    let first = v["parameters"][0]["id"].as_str().unwrap().to_string();
    ok(&mut s, "set_plugin_param", json!({"plugin_id": pid, "param": first, "value": 0.7}));
    let fx = ok(&mut s, "add_vst3", json!({"track_id": t, "path": aw.display().to_string(), "as_instrument": false}));
    assert!(fx["parameter_count"].as_u64().unwrap() > 0);
    ok(&mut s, "create_midi_region", json!({"track_id": t, "start": 0, "length": 4, "notes": [{"pitch": 60, "velocity": 110, "start": 0, "length": 2}, {"pitch": 67, "velocity": 110, "start": 2, "length": 2}]}));
    let a = ok(&mut s, "analyze_mix", json!({}));
    let peak = a["master"]["peak_dbfs"].as_f64().unwrap();
    assert!(peak > -60.0, "VST3 chain rendered silence: {a}");
    // save/reload keeps the plug-ins
    let p = tmp("vst.ondina");
    ok(&mut s, "save_project", json!({"path": p.display().to_string()}));
    let mut s2 = Session::new(false);
    s2.settings.vst3_folders = vec![root.display().to_string()];
    ok(&mut s2, "open_project", json!({"path": p.display().to_string()}));
    let a2 = ok(&mut s2, "analyze_mix", json!({}));
    assert!(a2["master"]["peak_dbfs"].as_f64().unwrap() > -60.0);
}

#[test]
fn object_based_adm_export() {
    let mut s = Session::new(false);
    ok(&mut s, "set_surround_format", json!({"format": "7.1.4"}));
    let bed = ok(&mut s, "add_track", json!({"kind": "instrument", "patch": "Lush Pad"}))["track_id"].as_u64().unwrap();
    ok(&mut s, "create_midi_region", json!({"track_id": bed, "start": 0, "length": 8, "notes": [{"pitch": 57, "velocity": 90, "start": 0, "length": 8}]}));
    let obj = ok(&mut s, "add_track", json!({"kind": "instrument", "patch": "Supersaw Lead"}))["track_id"].as_u64().unwrap();
    ok(&mut s, "create_midi_region", json!({"track_id": obj, "start": 0, "length": 8, "notes": [{"pitch": 69, "velocity": 100, "start": 0, "length": 8}]}));
    ok(&mut s, "set_surround_pan", json!({"track_id": obj, "azimuth": -90, "elevation": 30, "object": true}));
    ok(&mut s, "set_automation", json!({"track_id": obj, "target": "surround:azimuth", "points": [{"beat": 0, "value": -90}, {"beat": 8, "value": 90}]}));
    let p = tmp("scene.adm.wav");
    let v = ok(&mut s, "export_adm", json!({"path": p.display().to_string()}));
    assert_eq!(v["bed_channels"], 10);
    assert_eq!(v["objects"].as_array().unwrap().len(), 1);
    assert!(v["objects"][0]["position_blocks"].as_u64().unwrap() > 10, "automation must create time-varying blocks");
    let bytes = std::fs::read(&p).unwrap();
    assert_eq!(&bytes[0..4], b"RIFF");
    let find = |id: &[u8]| bytes.windows(4).position(|w| w == id);
    assert!(find(b"chna").is_some() && find(b"axml").is_some() && find(b"data").is_some());
    let spec = hound::WavReader::open(&p).unwrap().spec();
    assert_eq!((spec.channels, spec.sample_rate, spec.bits_per_sample), (11, 48000, 24));
    let xml = String::from_utf8_lossy(&bytes);
    assert!(xml.contains("typeDefinition=\"Objects\"") && xml.contains("typeDefinition=\"DirectSpeakers\"") && xml.contains("<speakerLabel>U+090</speakerLabel>"));
    // ADM azimuth is positive to the left: the first block (Ondina -90 = left) must be +90
    assert!(xml.contains("<position coordinate=\"azimuth\">90.00</position>"));
}

#[test]
fn ml_stem_splitter() {
    if ondina_dsp::ml_stems::find_model_dir().is_none() && !Path::new(env!("CARGO_MANIFEST_DIR")).join("../../tools/models").exists() {
        eprintln!("ML model not installed: skipping");
        return;
    }
    std::env::set_var("ONDINA_MODELS", Path::new(env!("CARGO_MANIFEST_DIR")).join("../../tools/models"));
    let example = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../tools/models/audio_example.wav");
    if !example.exists() {
        return;
    }
    let mut s = Session::new(false);
    let imp = ok(&mut s, "import_audio", json!({"path": example.display().to_string(), "start": 0}));
    let st = ok(&mut s, "split_stems", json!({"region_id": imp["region_id"], "engine": "ml"}));
    assert!(st["engine"].as_str().unwrap().starts_with("ml"), "{st}");
    let stems = st["stems"].as_array().unwrap();
    assert_eq!(stems.len(), 4);
    let voc = stems.iter().find(|x| x["stem"] == "Vocals").unwrap()["rms_db"].as_f64().unwrap();
    assert!(voc > -40.0, "vocal stem should carry the singer: {st}");
}

#[test]
fn note_tools_follow_the_selection() {
    let mut s = Session::new(false);
    let t = ok(&mut s, "add_track", json!({"kind": "instrument"}))["track_id"].as_u64().unwrap();
    let notes = json!([
        {"pitch": 60, "velocity": 100, "start": 0.1, "length": 0.5},
        {"pitch": 62, "velocity": 100, "start": 0.8, "length": 0.5},
        {"pitch": 64, "velocity": 100, "start": 0.9, "length": 0.5},
        {"pitch": 65, "velocity": 100, "start": 0.95, "length": 0.5}
    ]);
    let rid = ok(&mut s, "create_midi_region", json!({"track_id": t, "start": 0, "length": 4, "notes": notes}))["region_id"].as_u64().unwrap();
    let before = ok(&mut s, "get_region", json!({"region_id": rid}))["notes"].clone();
    let r = ok(&mut s, "quantize_notes", json!({"region_id": rid, "grid": 0.5, "indices": [1, 2]}));
    assert_eq!(r["indices"], json!([2, 3]));
    ok(&mut s, "undo", json!({}));
    assert_eq!(ok(&mut s, "get_region", json!({"region_id": rid}))["notes"], before);
    let r = ok(&mut s, "humanize_notes", json!({"region_id": rid, "seed": 1, "indices": [0]}));
    assert_eq!(r["indices"], json!([0]));
    let r = ok(&mut s, "quantize_notes", json!({"region_id": rid, "grid": 0.5}));
    assert!(r.get("indices").is_none(), "{r}");
    for name in ["quantize_notes", "humanize_notes", "snap_notes_to_key"] {
        let spec = tool_specs().into_iter().find(|t| t.name == name).unwrap();
        assert!(spec.input_schema["properties"].get("indices").is_some(), "{name} has no indices");
        let req = spec.input_schema.get("required").and_then(|r| r.as_array()).cloned().unwrap_or_default();
        assert!(!req.iter().any(|x| x == "indices"), "{name}: indices must stay optional");
    }
}

#[test]
fn add_harmony_is_one_undo_step_and_reports_the_added_notes() {
    let mut s = Session::new(false);
    let t = ok(&mut s, "add_track", json!({"kind": "instrument"}))["track_id"].as_u64().unwrap();
    let notes = json!([
        {"pitch": 60, "velocity": 100, "start": 0.0, "length": 0.5},
        {"pitch": 64, "velocity": 100, "start": 1.0, "length": 0.5},
        {"pitch": 71, "velocity": 100, "start": 2.0, "length": 0.5}
    ]);
    let rid = ok(&mut s, "create_midi_region", json!({"track_id": t, "start": 0, "length": 4, "notes": notes}))["region_id"].as_u64().unwrap();
    let before = ok(&mut s, "get_region", json!({"region_id": rid}))["notes"].clone();
    let r = ok(&mut s, "add_harmony", json!({"region_id": rid, "interval": "3rd"}));
    assert_eq!(r["indices"], json!([1, 3, 5]), "{r}");
    let after = ok(&mut s, "get_region", json!({"region_id": rid}))["notes"].clone();
    let pitches: Vec<u64> = [1, 3, 5].iter().map(|&i| after[i]["pitch"].as_u64().unwrap()).collect();
    assert_eq!(pitches, vec![64, 67, 74]);
    // one undo removes all three copies
    ok(&mut s, "undo", json!({}));
    assert_eq!(ok(&mut s, "get_region", json!({"region_id": rid}))["notes"], before);
    ok(&mut s, "redo", json!({}));
    assert_eq!(ok(&mut s, "get_region", json!({"region_id": rid}))["notes"].as_array().unwrap().len(), 6);
    let r = ok(&mut s, "add_harmony", json!({"region_id": rid, "interval": "6th", "below": true, "indices": [0]}));
    let ix = r["indices"].as_array().unwrap();
    assert_eq!(ix.len(), 1, "{r}");
    let added = &ok(&mut s, "get_region", json!({"region_id": rid}))["notes"][ix[0].as_u64().unwrap() as usize];
    assert_eq!(added["pitch"], json!(52), "{added}");
    assert_eq!(added["start"].as_f64(), Some(0.0), "{added}");
    assert!(call(&mut s, "add_harmony", json!({"region_id": rid, "interval": "2nd"})).is_err());
    let spec = tool_specs().into_iter().find(|t| t.name == "add_harmony").expect("add_harmony is a tool");
    let interval = spec.input_schema["properties"]["interval"].to_string();
    for v in ["3rd", "5th", "6th", "octave"] {
        assert!(interval.contains(&format!("\"{v}\"")), "interval schema lacks {v}: {interval}");
    }
    let req = spec.input_schema.get("required").and_then(|r| r.as_array()).cloned().unwrap_or_default();
    for k in ["region_id", "interval"] {
        assert!(req.iter().any(|x| x == k), "{k} must be required");
    }
    for k in ["below", "indices"] {
        assert!(!req.iter().any(|x| x == k), "{k} must stay optional");
    }
}

#[test]
fn set_note_lengths_is_one_undo_step_and_keeps_the_indices() {
    let mut s = Session::new(false);
    let t = ok(&mut s, "add_track", json!({"kind": "instrument"}))["track_id"].as_u64().unwrap();
    let notes = json!([
        {"pitch": 40, "velocity": 100, "start": 0.0, "length": 1.5},
        {"pitch": 40, "velocity": 100, "start": 1.0, "length": 0.5},
        {"pitch": 43, "velocity": 100, "start": 2.0, "length": 0.25},
        {"pitch": 45, "velocity": 100, "start": 3.0, "length": 0.25}
    ]);
    let rid = ok(&mut s, "create_midi_region", json!({"track_id": t, "start": 0, "length": 4, "notes": notes}))["region_id"].as_u64().unwrap();
    let before = ok(&mut s, "get_region", json!({"region_id": rid}))["notes"].clone();
    let lengths = |s: &mut Session| -> Vec<f64> {
        let n = ok(s, "get_region", json!({"region_id": rid}))["notes"].clone();
        n.as_array().unwrap().iter().map(|x| x["length"].as_f64().unwrap()).collect()
    };
    let r = ok(&mut s, "set_note_lengths", json!({"region_id": rid, "mode": "legato"}));
    assert_eq!(r["changed"], json!(3), "{r}");
    assert!(r.get("indices").is_none(), "{r}");
    assert_eq!(lengths(&mut s), vec![1.0, 1.0, 1.0, 0.25]);
    // one undo brings every length back
    ok(&mut s, "undo", json!({}));
    assert_eq!(ok(&mut s, "get_region", json!({"region_id": rid}))["notes"], before);
    let r = ok(&mut s, "set_note_lengths", json!({"region_id": rid, "mode": "no_overlap"}));
    assert_eq!(r["changed"], json!(1), "{r}");
    assert_eq!(lengths(&mut s), vec![1.0, 0.5, 0.25, 0.25]);
    ok(&mut s, "undo", json!({}));
    let r = ok(&mut s, "set_note_lengths", json!({"region_id": rid, "mode": "fixed", "length": 0.5, "indices": [2, 3]}));
    assert_eq!(r["indices"], json!([2, 3]), "{r}");
    assert_eq!(lengths(&mut s), vec![1.5, 0.5, 0.5, 0.5]);
    let after = ok(&mut s, "get_region", json!({"region_id": rid}))["notes"].clone();
    for i in 0..4 {
        assert_eq!(after[i]["pitch"], before[i]["pitch"]);
        assert_eq!(after[i]["start"], before[i]["start"]);
    }
    // doubling stops at the end of the region
    ok(&mut s, "set_note_lengths", json!({"region_id": rid, "mode": "scale", "factor": 2, "indices": [3]}));
    assert_eq!(lengths(&mut s)[3], 1.0);
    assert!(call(&mut s, "set_note_lengths", json!({"region_id": rid, "mode": "fixed"})).is_err());
    assert!(call(&mut s, "set_note_lengths", json!({"region_id": rid, "mode": "staccato"})).is_err());
    assert!(call(&mut s, "set_note_lengths", json!({"region_id": rid, "mode": "scale", "factor": 0})).is_err());
    let spec = tool_specs().into_iter().find(|t| t.name == "set_note_lengths").expect("set_note_lengths is a tool");
    let mode = spec.input_schema["properties"]["mode"].to_string();
    for v in ["legato", "no_overlap", "fixed", "scale"] {
        assert!(mode.contains(&format!("\"{v}\"")), "mode schema lacks {v}: {mode}");
    }
    let req = spec.input_schema.get("required").and_then(|r| r.as_array()).cloned().unwrap_or_default();
    for k in ["region_id", "mode"] {
        assert!(req.iter().any(|x| x == k), "{k} must be required");
    }
    for k in ["length", "factor", "indices"] {
        assert!(!req.iter().any(|x| x == k), "{k} must stay optional");
    }
}

#[test]
fn resize_region_is_one_undo_step() {
    let mut s = Session::new(false);
    let t = ok(&mut s, "add_track", json!({"kind": "instrument", "patch": "Lush Pad"}))["track_id"].as_u64().unwrap();
    let r = ok(&mut s, "create_midi_region", json!({"track_id": t, "start": 2.0, "length": 4.0}))["region_id"].as_u64().unwrap();
    ok(&mut s, "resize_region", json!({"region_id": r, "length": 6.0}));
    let g = ok(&mut s, "get_region", json!({"region_id": r}));
    assert_eq!(g["length"].as_f64(), Some(6.0), "{g}");
    assert_eq!(g["start"].as_f64(), Some(2.0), "{g}");
    ok(&mut s, "undo", json!({}));
    let g = ok(&mut s, "get_region", json!({"region_id": r}));
    assert_eq!(g["length"].as_f64(), Some(4.0), "one undo restores the previous length: {g}");
    assert_eq!(g["start"].as_f64(), Some(2.0), "{g}");
    ok(&mut s, "redo", json!({}));
    let g = ok(&mut s, "get_region", json!({"region_id": r}));
    assert_eq!(g["length"].as_f64(), Some(6.0), "{g}");
}

/// A 120 bpm session with one instrument track and a short note in a 4-beat region. Returns (session, track id).
fn short_note_session(reverb: bool) -> (Session, u64) {
    let mut s = Session::new(false);
    let t = ok(&mut s, "add_track", json!({"kind": "instrument"}))["track_id"].as_u64().unwrap();
    ok(&mut s, "create_midi_region", json!({"track_id": t, "start": 0, "length": 4, "notes": [{"pitch": 60, "velocity": 110, "start": 0, "length": 0.5}]}));
    if reverb {
        let fx = ok(&mut s, "add_effect", json!({"track_id": t, "kind": "reverb"}))["plugin_id"].as_u64().unwrap();
        ok(&mut s, "set_plugin_params", json!({"plugin_id": fx, "params": {"size": 0.95, "damping": 0.0, "mix": 0.5}}));
    }
    (s, t)
}

#[test]
fn bounce_keeps_the_reverb_tail() {
    let (mut s, t) = short_note_session(true);
    let path = tmp("tail_auto.wav");
    let r = ok(&mut s, "bounce", json!({"path": path.display().to_string(), "bit_depth": 32}));
    let dur = r["duration_sec"].as_f64().unwrap();
    assert!(dur > 2.0 + 2.5, "{r}");
    assert!(dur < 2.0 + 19.0, "{r}");
    assert!(r["tail_sec"].as_f64().unwrap() > 2.5, "{r}");
    let sr = r["sample_rate"].as_u64().unwrap() as usize;
    let samples: Vec<f32> = hound::WavReader::open(&path).unwrap().into_samples::<f32>().map(|x| x.unwrap()).collect();
    let last = (sr / 100) * 2;
    let peak = samples[samples.len() - last..].iter().fold(0.0f32, |a, b| a.max(b.abs()));
    assert!(peak < 1e-4, "last 10 ms peak {peak}");
    let b = ok(&mut s, "bounce_in_place", json!({"track_id": t}));
    let g = ok(&mut s, "get_region", json!({"region_id": b["region_id"]}));
    assert!(g["length"].as_f64().unwrap() > 4.0 + 5.0, "{g}");
}

#[test]
fn bounce_without_tails_keeps_two_seconds() {
    let (mut s, _) = short_note_session(false);
    let path = tmp("tail_dry.wav");
    let r = ok(&mut s, "bounce", json!({"path": path.display().to_string()}));
    let sr = r["sample_rate"].as_u64().unwrap() as u32;
    assert_eq!(hound::WavReader::open(&path).unwrap().duration(), 4 * sr, "{r}");
    assert_eq!(r["tail_sec"].as_f64(), Some(2.0), "{r}");
}

#[test]
fn bounce_tail_sec_zero_is_sample_exact() {
    let mut s = Session::new(false);
    ok(&mut s, "set_tempo", json!({"bpm": 100}));
    let t = ok(&mut s, "add_track", json!({"kind": "instrument"}))["track_id"].as_u64().unwrap();
    ok(&mut s, "create_midi_region", json!({"track_id": t, "start": 0, "length": 24, "notes": [
        {"pitch": 60, "velocity": 100, "start": 0, "length": 1},
        {"pitch": 64, "velocity": 100, "start": 8, "length": 1},
        {"pitch": 67, "velocity": 100, "start": 19, "length": 2}
    ]}));
    let path = tmp("tail_zero.wav");
    let p = path.display().to_string();
    let r = ok(&mut s, "bounce", json!({"path": p, "start": 4, "end": 20, "tail_sec": 0}));
    let sr = r["sample_rate"].as_u64().unwrap() as f64;
    assert_eq!(hound::WavReader::open(&path).unwrap().duration() as f64, (9.6 * sr).round(), "{r}");
    assert_eq!(r["tail_sec"].as_f64(), Some(0.0), "{r}");
    let r = ok(&mut s, "bounce", json!({"path": p, "start": 4, "end": 20, "tail_sec": 1.5}));
    assert_eq!(hound::WavReader::open(&path).unwrap().duration() as f64, ((9.6 + 1.5) * sr).round(), "{r}");
    for bad in [-1.0, 61.0] {
        let e = call(&mut s, "bounce", json!({"path": p, "tail_sec": bad})).unwrap_err();
        assert!(e.contains("tail_sec"), "{e}");
    }
    let spec = tool_specs().into_iter().find(|t| t.name == "bounce").unwrap();
    assert!(spec.input_schema["properties"].get("tail_sec").is_some());
    let req = spec.input_schema.get("required").and_then(|r| r.as_array()).cloned().unwrap_or_default();
    assert!(!req.iter().any(|v| v == "tail_sec"));
}

#[test]
fn load_movie_refuses_options_and_urls() {
    let mut s = Session::new(false);
    let tracks = s.project.tracks.len();
    for bad in ["-f lavfi -i testsrc", "http://x/m.mp4", "concat:a.mp4|b.mp4", "\\\\server\\share\\m.mp4"] {
        let e = call(&mut s, "load_movie", json!({"path": bad})).unwrap_err();
        assert!(e.contains("movie path refused"), "{bad}: {e}");
    }
    assert!(s.project.movie.is_none());
    assert_eq!(s.project.tracks.len(), tracks);
}

#[test]
fn opened_project_with_url_movie_cannot_reach_ffmpeg() {
    let mut s = Session::new(false);
    s.project.movie = Some(ondina_core::model::MovieRef { path: "http://x/m.mp4".into(), offset_sec: 0.0, duration_sec: 4.0, audio_file_id: None });
    let file = tmp("url_movie.ondina");
    std::fs::write(&file, ondina_core::save_project_string(&s.project).unwrap()).unwrap();
    let mut s2 = Session::new(false);
    ok(&mut s2, "open_project", json!({"path": file.display().to_string()}));
    let png = tmp("url_movie_frame.png");
    let mp4 = tmp("url_movie_export.mp4");
    let _ = std::fs::remove_file(&png);
    let _ = std::fs::remove_file(&mp4);
    let e = call(&mut s2, "extract_movie_frame", json!({"path": png.display().to_string(), "beat": 0})).unwrap_err();
    assert!(e.contains("movie path is refused"), "{e}");
    let e = call(&mut s2, "export_movie", json!({"path": mp4.display().to_string()})).unwrap_err();
    assert!(e.contains("movie path is refused"), "{e}");
    assert!(!png.exists() && !mp4.exists());
}

#[test]
fn movie_output_paths_are_checked() {
    let mut s = Session::new(false);
    // a syntactically valid local movie: the output check comes before ffmpeg is looked for
    s.project.movie = Some(ondina_core::model::MovieRef { path: tmp("local_movie.mp4").display().to_string(), offset_sec: 0.0, duration_sec: 4.0, audio_file_id: None });
    for (tool, path) in [("extract_movie_frame", "-y.png".to_string()), ("extract_movie_frame", tmp("frame.bmp").display().to_string()), ("export_movie", tmp("scored.avi").display().to_string())] {
        let e = call(&mut s, tool, json!({"path": path})).unwrap_err();
        assert!(e.contains("output path refused"), "{tool} {path}: {e}");
    }
}

#[test]
fn movie_frame_preview_uses_local_files_only() {
    assert!(ondina_studio::session_ext::movie_frame_rgb("http://127.0.0.1:9/m.mp4", 0.0, 320, 180).is_err());
    let Some(ff) = ondina_studio::session_ext::ffmpeg_path() else {
        eprintln!("ffmpeg not available: skipping");
        return;
    };
    let mp4 = tmp("preview_movie.mp4");
    let st = std::process::Command::new(&ff)
        .args(["-hide_banner", "-loglevel", "error", "-y", "-f", "lavfi", "-i", "testsrc=duration=2:size=320x240:rate=25", "-c:v", "libx264", "-pix_fmt", "yuv420p"])
        .arg(&mp4)
        .status()
        .unwrap();
    assert!(st.success());
    let px = ondina_studio::session_ext::movie_frame_rgb(&mp4.display().to_string(), 1.0, 320, 180).unwrap();
    assert_eq!(px.len(), 320 * 180 * 3);
}

#[test]
fn marker_update_moves_renames_and_resizes_one_undo_step_each() {
    let mut s = Session::new(false);
    let id = ok(&mut s, "add_marker", json!({"beat": 0, "name": "Verse", "length": 16}))["marker_id"].as_u64().unwrap();
    ok(&mut s, "add_marker", json!({"beat": 32, "name": "Chorus", "length": 16}));
    let pos = |s: &Session| s.project.markers.iter().position(|m| m.id == id).unwrap();
    let chorus = |s: &Session| s.project.markers.iter().position(|m| m.name == "Chorus").unwrap();

    ok(&mut s, "update_marker", json!({"marker_id": id, "beat": 48}));
    assert_eq!(s.project.markers[pos(&s)].beat, 48.0);
    assert!(pos(&s) > chorus(&s), "markers stay sorted by beat");
    ok(&mut s, "undo", json!({}));
    assert_eq!(s.project.markers[pos(&s)].beat, 0.0);
    assert_eq!(pos(&s), 0);

    ok(&mut s, "update_marker", json!({"marker_id": id, "beat": -3}));
    assert_eq!(s.project.markers[pos(&s)].beat, 0.0);

    ok(&mut s, "update_marker", json!({"marker_id": id, "name": "Intro"}));
    assert_eq!(s.project.markers[pos(&s)].name, "Intro");
    ok(&mut s, "undo", json!({}));
    assert_eq!(s.project.markers[pos(&s)].name, "Verse");

    ok(&mut s, "update_marker", json!({"marker_id": id, "length": 4}));
    assert_eq!(s.project.markers[pos(&s)].length, Some(4.0));
    ok(&mut s, "undo", json!({}));
    assert_eq!(s.project.markers[pos(&s)].length, Some(16.0));
}

#[test]
fn add_chord_stores_roman_numerals_as_symbols_and_the_bassist_follows() {
    use ondina_core::model::RegionContent;
    let mut s = Session::new(false);
    ok(&mut s, "set_key", json!({"key": "C major"}));
    let t = ok(&mut s, "add_track", json!({"kind": "instrument"}))["track_id"].as_u64().unwrap();

    let v = ok(&mut s, "add_chord", json!({"beat": 0, "length": 4, "symbol": "IV"}));
    assert_eq!(v["symbol"], "F");
    assert!(v["chord_id"].as_u64().is_some());
    assert_eq!(s.project.chords[0].symbol, "F");

    let rid = ok(&mut s, "generate_bass", json!({"track_id": t, "start": 0, "bars": 1, "style": "root", "seed": 1}))["region_id"].as_u64().unwrap();
    let (_, region) = s.project.find_region(rid).unwrap();
    let RegionContent::Midi(m) = &region.content else { panic!("bass region is not MIDI") };
    assert!(!m.notes.is_empty());
    assert!(m.notes.iter().all(|n| n.pitch % 12 == 5), "the bassist plays F, not the tonic");

    let v = ok(&mut s, "add_chord", json!({"beat": 4, "length": 4, "symbol": " Am7 "}));
    assert_eq!(v["symbol"], "Am7");

    let e = call(&mut s, "add_chord", json!({"beat": 8, "length": 4, "symbol": "H9x"})).unwrap_err();
    assert!(e.contains("cannot parse chord"), "{e}");
    assert_eq!(s.project.chords.len(), 2);
}

#[test]
fn update_chord_edits_symbol_position_and_length_in_one_undo_step() {
    let mut s = Session::new(false);
    ok(&mut s, "set_key", json!({"key": "C major"}));
    let id = ok(&mut s, "add_chord", json!({"beat": 0, "length": 4, "symbol": "C"}))["chord_id"].as_u64().unwrap();
    ok(&mut s, "add_chord", json!({"beat": 8, "length": 4, "symbol": "G"}));
    let pos = |s: &Session| s.project.chords.iter().position(|c| c.id == id).unwrap();
    let g = |s: &Session| s.project.chords.iter().position(|c| c.symbol == "G").unwrap();

    let v = ok(&mut s, "update_chord", json!({"chord_id": id, "symbol": "vi", "beat": 12, "length": 2}));
    assert_eq!(v["symbol"], "Am");
    assert_eq!(v["beat"], 12.0);
    assert_eq!(v["length"], 2.0);
    assert!(pos(&s) > g(&s), "the chord track stays sorted by beat");

    ok(&mut s, "undo", json!({}));
    let c = &s.project.chords[pos(&s)];
    assert_eq!((c.symbol.as_str(), c.beat, c.length), ("C", 0.0, 4.0));
    assert_eq!(pos(&s), 0);

    ok(&mut s, "update_chord", json!({"chord_id": id, "beat": -2}));
    assert_eq!(s.project.chords[pos(&s)].beat, 0.0);

    let track = |s: &Session| s.project.chords.iter().map(|c| (c.symbol.clone(), c.beat, c.length)).collect::<Vec<_>>();
    let before = track(&s);
    let undo_before = s.can_undo().map(String::from);
    for args in [
        json!({"chord_id": id, "symbol": "H9x"}),
        json!({"chord_id": id, "length": 0}),
        json!({"chord_id": id, "length": -1}),
        json!({"chord_id": 999999, "beat": 4}),
    ] {
        assert!(call(&mut s, "update_chord", args.clone()).is_err(), "{args} should fail");
    }
    assert_eq!(track(&s), before);
    assert_eq!(s.can_undo().map(String::from), undo_before);

    ok(&mut s, "update_chord", json!({"chord_id": id}));
    assert_eq!(track(&s), before);
}

fn midi_session() -> Session {
    let mut s = Session::new(false);
    let t = ok(&mut s, "add_track", json!({"kind": "instrument"}))["track_id"].as_u64().unwrap();
    ok(&mut s, "create_midi_region", json!({"track_id": t, "start": 0, "length": 4, "notes": [{"pitch": 60, "velocity": 100, "start": 0, "length": 1}]}));
    s
}

#[test]
fn mcp_writes_refuse_network_paths_and_foreign_extensions() {
    let mut s = midi_session();
    let bat = tmp("refused_export.bat");
    let _ = std::fs::remove_file(&bat);
    let e = call(&mut s, "export_midi", json!({"path": bat.display().to_string()})).unwrap_err();
    assert!(e.contains("output path refused"), "{e}");
    assert!(!bat.exists());
    let e = call(&mut s, "bounce", json!({"path": "\\\\server\\share\\a.wav", "tail_sec": 0})).unwrap_err();
    assert!(e.contains("output path refused"), "{e}");
    for (tool, file) in [
        ("bounce", "refused_bounce.dll"),
        ("bounce_surround", "refused_surround.exe"),
        ("bounce_binaural", "refused_binaural.wav.exe"),
        ("export_adm", "refused_adm.bat"),
        ("export_music_xml", "refused_score.exe"),
    ] {
        let p = tmp(file);
        let _ = std::fs::remove_file(&p);
        let e = call(&mut s, tool, json!({"path": p.display().to_string()})).unwrap_err();
        assert!(e.contains("output path refused"), "{tool}: {e}");
        assert!(!p.exists(), "{tool} wrote {}", p.display());
    }
    let ai = tmp("refused_ai.bat");
    let _ = std::fs::remove_file(&ai);
    let e = s.execute(Command::ExportMidi { path: ai.display().to_string(), track_ids: None }, Source::Ai).unwrap_err();
    assert!(e.contains("output path refused"), "{e}");
    assert!(!ai.exists());
}

#[test]
fn mcp_writes_to_local_files_still_work() {
    let mut s = midi_session();
    let wav = tmp("mcp_local.wav");
    let _ = std::fs::remove_file(&wav);
    ok(&mut s, "bounce", json!({"path": wav.display().to_string(), "tail_sec": 0}));
    assert!(wav.exists());
    let noext = tmp("mcp_noext.wav");
    let _ = std::fs::remove_file(&noext);
    let v = ok(&mut s, "bounce", json!({"path": tmp("mcp_noext").display().to_string(), "tail_sec": 0}));
    assert!(v["path"].as_str().unwrap().ends_with("mcp_noext.wav"), "{v}");
    assert!(noext.exists());
    let mid = tmp("mcp_local.mid");
    let _ = std::fs::remove_file(&mid);
    ok(&mut s, "export_midi", json!({"path": mid.display().to_string()}));
    assert!(mid.exists());
    let score = tmp("mcp_noext_score.musicxml");
    let _ = std::fs::remove_file(&score);
    ok(&mut s, "export_music_xml", json!({"path": tmp("mcp_noext_score").display().to_string()}));
    assert!(score.exists());
    let proj = tmp("mcp_local.ondina");
    let _ = std::fs::remove_file(&proj);
    ok(&mut s, "save_project", json!({"path": proj.display().to_string()}));
    let mut s2 = Session::new(false);
    ok(&mut s2, "open_project", json!({"path": proj.display().to_string()}));
    assert_eq!(s2.project.tracks.len(), 1);
}

#[test]
fn mcp_save_project_does_not_overwrite_other_files() {
    let mut s = midi_session();
    let fake = tmp("not_a_project.ondina");
    std::fs::write(&fake, "my notes, not a project").unwrap();
    let e = call(&mut s, "save_project", json!({"path": fake.display().to_string()})).unwrap_err();
    assert!(e.contains("not an Ondina project"), "{e}");
    assert_eq!(std::fs::read_to_string(&fake).unwrap(), "my notes, not a project");
    let notes = tmp("notes.txt");
    std::fs::write(&notes, "keep me").unwrap();
    let e = call(&mut s, "save_project", json!({"path": notes.display().to_string()})).unwrap_err();
    assert!(e.contains("output path refused"), "{e}");
    assert_eq!(std::fs::read_to_string(&notes).unwrap(), "keep me");
    let real = tmp("overwrite_real.ondina");
    let _ = std::fs::remove_file(&real);
    ok(&mut s, "save_project", json!({"path": real.display().to_string()}));
    ok(&mut s, "add_track", json!({"kind": "instrument"}));
    ok(&mut s, "save_project", json!({"path": real.display().to_string()}));
    ok(&mut s, "save_project", json!({}));
    let mut s2 = Session::new(false);
    ok(&mut s2, "open_project", json!({"path": real.display().to_string()}));
    assert_eq!(s2.project.tracks.len(), 2);
}

#[test]
fn gui_and_script_writes_are_not_restricted() {
    let mut s = midi_session();
    let txt = tmp("gui_any_name.txt");
    let _ = std::fs::remove_file(&txt);
    s.execute(Command::SaveProject { path: Some(txt.display().to_string()) }, Source::Gui).unwrap();
    assert!(txt.exists());
    let over = tmp("gui_overwrite.ondina");
    std::fs::write(&over, "plain text").unwrap();
    s.execute(Command::SaveProject { path: Some(over.display().to_string()) }, Source::Gui).unwrap();
    assert!(ondina_studio::session_ext::is_ondina_project_file(&over).unwrap());
    let bin = tmp("script_any.bin");
    let _ = std::fs::remove_file(&bin);
    s.execute(Command::ExportMidi { path: bin.display().to_string(), track_ids: None }, Source::Script).unwrap();
    assert!(bin.exists());
}
#[test]
fn projects_saved_before_the_rename_still_open() {
    assert!(ondina_core::is_project_file("C:/songs/Old Song.AURORA"));
    assert!(ondina_core::is_project_file("new.ondina"));
    assert!(!ondina_core::is_project_file("take.wav"));

    let example = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../examples/lofi-beat.aurora");
    let mut s = Session::new(false);
    ok(&mut s, "open_project", json!({"path": example.display().to_string()}));
    assert!(!s.project.tracks.is_empty());

    let saved = tmp("renamed.ondina");
    ok(&mut s, "save_project", json!({"path": saved.display().to_string()}));
    let mut again = Session::new(false);
    ok(&mut again, "open_project", json!({"path": saved.display().to_string()}));
    assert_eq!(again.project.tracks.len(), s.project.tracks.len());
}

#[test]
fn environment_variables_from_before_the_rename_still_work() {
    std::env::set_var("AURORA_RENAME_PROBE", "old");
    assert_eq!(ondina_core::env_var("RENAME_PROBE").as_deref(), Some("old"));
    std::env::set_var("ONDINA_RENAME_PROBE", "new");
    assert_eq!(ondina_core::env_var("RENAME_PROBE").as_deref(), Some("new"));
}

#[test]
fn mcp_can_save_over_a_project_from_before_the_rename() {
    let example = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../examples/lofi-beat.aurora");
    let copy = tmp("resave_old.aurora");
    std::fs::copy(&example, &copy).unwrap();
    let mut s = Session::new(false);
    ok(&mut s, "open_project", json!({"path": copy.display().to_string()}));
    ok(&mut s, "set_tempo", json!({"bpm": 77}));
    ok(&mut s, "save_project", json!({"path": copy.display().to_string()}));
    let mut again = Session::new(false);
    ok(&mut again, "open_project", json!({"path": copy.display().to_string()}));
    assert_eq!(again.project.tempo.initial_bpm(), 77.0);
}

#[test]
fn removing_tracks_sends_and_plugins_cleans_references_in_one_undo_step() {
    let mut s = Session::new(false);
    let add = |s: &mut Session, kind: &str| ok(s, "add_track", json!({"kind": kind}))["track_id"].as_u64().unwrap();
    let src = add(&mut s, "instrument");
    let t2 = add(&mut s, "instrument");
    let buses = [add(&mut s, "bus"), add(&mut s, "bus"), add(&mut s, "bus")];
    for (i, bus) in buses.iter().enumerate() {
        ok(&mut s, "add_send", json!({"track_id": src, "bus_id": bus}));
        ok(&mut s, "set_automation", json!({"track_id": src, "target": format!("send:{i}"), "points": [{"beat": 0, "value": -(i as f32) - 1.0}]}));
    }
    let fx = ok(&mut s, "add_effect", json!({"track_id": src, "kind": "reverb"}))["plugin_id"].as_u64().unwrap();
    for t in [src, t2] {
        ok(&mut s, "set_automation", json!({"track_id": t, "target": format!("plugin:{fx}:mix"), "points": [{"beat": 0, "value": 0.5}]}));
    }
    ok(&mut s, "create_live_cell", json!({"track_id": t2, "scene": 0, "length": 4}));
    ok(&mut s, "create_live_cell", json!({"track_id": src, "scene": 0, "length": 4}));
    ok(&mut s, "set_midi_routes", json!({"routes": [{"name": "upper", "note_min": 60, "track_id": t2}, {"name": "lower", "note_max": 59, "track_id": src}]}));
    let send_lanes = |s: &Session| -> Vec<(String, f32)> {
        s.project.track(src).unwrap().automation.iter().filter(|l| l.target.starts_with("send:")).map(|l| (l.target.clone(), l.points[0].value)).collect()
    };

    let cases = [
        ("remove_send", json!({"track_id": src, "send_index": 0}), json!({"ok": true, "removed_automation_lanes": 1})),
        ("remove_plugin", json!({"plugin_id": fx}), json!({"ok": true, "removed_automation_lanes": 2})),
        ("remove_track", json!({"track_id": buses[1]}), json!({"ok": true, "removed_automation_lanes": 1, "removed_live_cells": 0, "removed_midi_routes": 0})),
        ("remove_track", json!({"track_id": t2}), json!({"ok": true, "removed_automation_lanes": 0, "removed_live_cells": 1, "removed_midi_routes": 1})),
    ];
    for (tool, args, expected) in cases {
        let before = ok(&mut s, "get_project_json", json!({}));
        assert_eq!(ok(&mut s, tool, args.clone()), expected, "{tool} {args}");
        match (tool, args["track_id"].as_u64()) {
            ("remove_send", _) => assert_eq!(send_lanes(&s), vec![("send:0".to_string(), -2.0), ("send:1".to_string(), -3.0)]),
            ("remove_track", Some(id)) if id == buses[1] => {
                assert_eq!(send_lanes(&s), vec![("send:0".to_string(), -1.0), ("send:1".to_string(), -3.0)]);
                let sends: Vec<u64> = s.project.track(src).unwrap().channel.sends.iter().map(|x| x.bus_id).collect();
                assert_eq!(sends, vec![buses[0], buses[2]]);
            }
            ("remove_track", Some(id)) if id == t2 => {
                let live = ok(&mut s, "get_live_loops", json!({}));
                let cells = live["cells"].as_array().unwrap();
                assert_eq!(cells.len(), 1, "{live}");
                assert!(cells.iter().all(|c| c["track_id"] != t2), "{live}");
                let routes = ok(&mut s, "get_midi_routes", json!({}));
                let routes = routes["routes"].as_array().unwrap();
                assert_eq!(routes.len(), 1);
                assert!(routes.iter().all(|r| r["track_id"] != t2));
            }
            _ => {}
        }
        ok(&mut s, "undo", json!({}));
        assert_eq!(ok(&mut s, "get_project_json", json!({})), before, "one undo after {tool} {args}");
    }
}

#[test]
fn duplicate_patch_and_instrument_keep_automation_consistent_in_one_undo_step() {
    let mut s = Session::new(false);
    let src = ok(&mut s, "add_track", json!({"kind": "instrument"}))["track_id"].as_u64().unwrap();
    let bus = ok(&mut s, "add_track", json!({"kind": "bus"}))["track_id"].as_u64().unwrap();
    ok(&mut s, "add_send", json!({"track_id": src, "bus_id": bus}));
    let inst = ok(&mut s, "get_track", json!({"track_id": src}))["instrument"]["id"].as_u64().unwrap();
    let fx = ok(&mut s, "add_effect", json!({"track_id": src, "kind": "gain"}))["plugin_id"].as_u64().unwrap();
    let lanes = [("volume".to_string(), -6.0), ("pan".to_string(), 0.5), ("send:0".to_string(), -12.0), (format!("plugin:{inst}:cutoff"), 800.0), (format!("plugin:{fx}:gain"), -60.0)];
    for (target, value) in &lanes {
        ok(&mut s, "set_automation", json!({"track_id": src, "target": target, "points": [{"beat": 0, "value": value}]}));
    }
    let targets = |v: &Value| -> Vec<String> { v["automation"].as_array().unwrap().iter().map(|l| l["target"].as_str().unwrap().to_string()).collect() };

    // duplicate_track: the copy has the same lanes, on its own plug-ins
    let before = ok(&mut s, "get_project_json", json!({}));
    let copy = ok(&mut s, "duplicate_track", json!({"track_id": src}))["track_id"].as_u64().unwrap();
    let t = ok(&mut s, "get_track", json!({"track_id": copy}));
    let (inst2, fx2) = (t["instrument"]["id"].as_u64().unwrap(), t["channel"]["inserts"][0]["id"].as_u64().unwrap());
    assert_eq!(targets(&t), vec!["volume".to_string(), "pan".into(), "send:0".into(), format!("plugin:{inst2}:cutoff"), format!("plugin:{fx2}:gain")]);
    ok(&mut s, "undo", json!({}));
    assert_eq!(ok(&mut s, "get_project_json", json!({})), before, "one undo after duplicate_track");

    // apply_patch: instrument and insert lanes go, volume, pan and send stay
    let r = ok(&mut s, "apply_patch", json!({"track_id": src, "patch": "Lush Pad"}));
    assert_eq!(r["removed_automation_lanes"], json!(2));
    assert_eq!(targets(&r), vec!["volume".to_string(), "pan".into(), "send:0".into()]);
    ok(&mut s, "undo", json!({}));
    assert_eq!(ok(&mut s, "get_project_json", json!({})), before, "one undo after apply_patch");

    // set_instrument: only the old instrument's lane goes
    let r = ok(&mut s, "set_instrument", json!({"track_id": src, "kind": "fm_synth"}));
    assert_eq!(r["removed_automation_lanes"], json!(1));
    let t = ok(&mut s, "get_track", json!({"track_id": src}));
    assert_eq!(targets(&t), vec!["volume".to_string(), "pan".into(), "send:0".into(), format!("plugin:{fx}:gain")]);
    ok(&mut s, "undo", json!({}));
    assert_eq!(ok(&mut s, "get_project_json", json!({})), before, "one undo after set_instrument");
}

fn selection_session() -> (Session, u64, u64) {
    let mut s = Session::new(false);
    let t = ok(&mut s, "add_track", json!({"kind": "instrument", "patch": "Lush Pad"}))["track_id"].as_u64().unwrap();
    let r = ok(&mut s, "create_midi_region", json!({"track_id": t, "start": 4, "length": 4, "notes": [
        {"pitch": 60, "velocity": 100, "start": 0, "length": 1},
        {"pitch": 64, "velocity": 90, "start": 1, "length": 1},
        {"pitch": 67, "velocity": 80, "start": 2, "length": 0.5}]}))["region_id"].as_u64().unwrap();
    (s, t, r)
}

#[test]
fn get_selection_reports_track_region_and_selected_notes() {
    let (mut s, t, r) = selection_session();
    s.selected_track = Some(t);
    s.selected_region = Some(r);
    s.selected_notes = vec![2, 0];
    let v = ok(&mut s, "get_selection", json!({}));
    assert_eq!(v["track"]["id"], json!(t), "{v}");
    assert_eq!(v["track"]["kind"], json!("instrument"), "{v}");
    assert_eq!(v["region"]["id"], json!(r), "{v}");
    assert_eq!(v["region"]["track_id"], json!(t), "{v}");
    assert_eq!(v["region"]["start"].as_f64(), Some(4.0), "{v}");
    assert_eq!(
        v["notes"],
        json!([
            {"index": 0, "pitch": 60, "name": "C4", "start": 0.0, "length": 1.0, "velocity": 100},
            {"index": 2, "pitch": 67, "name": "G4", "start": 2.0, "length": 0.5, "velocity": 80}
        ]),
        "{v}"
    );
    assert_eq!(v["selected_note_count"], json!(2), "{v}");
    assert!(v["playhead"]["beat"].is_number(), "{v}");
    assert!(v["playhead"]["position"].is_string(), "{v}");
    assert!(v["cycle"]["enabled"].is_boolean(), "{v}");
}

#[test]
fn get_selection_is_read_only() {
    assert!(!Command::GetSelection {}.is_mutating());
    assert!(tool_specs().iter().any(|t| t.name == "get_selection" && t.read_only));
    let mut s = Session::new(false);
    let (undo, rev) = (s.can_undo().map(String::from), s.revision);
    ok(&mut s, "get_selection", json!({}));
    assert_eq!(s.can_undo().map(String::from), undo);
    assert_eq!(s.revision, rev);
    assert!(!s.dirty);

    // With a saved project and a real selection: still nothing to undo, still clean.
    let (mut s, t, r) = selection_session();
    let path = tmp("selection_ro.ondina");
    ok(&mut s, "save_project", json!({"path": path.display().to_string()}));
    assert!(!s.dirty);
    s.selected_track = Some(t);
    s.selected_region = Some(r);
    s.selected_notes = vec![0, 1];
    let (undo, rev) = (s.can_undo().map(String::from), s.revision);
    ok(&mut s, "get_selection", json!({}));
    assert_eq!(s.can_undo().map(String::from), undo);
    assert_eq!(s.revision, rev);
    assert!(!s.dirty);
}

#[test]
fn get_selection_without_selection_is_empty() {
    let mut s = Session::new(false);
    let v = ok(&mut s, "get_selection", json!({}));
    assert!(v["track"].is_null(), "{v}");
    assert!(v["region"].is_null(), "{v}");
    assert_eq!(v["notes"], json!([]), "{v}");
    assert_eq!(v["selected_note_count"], json!(0), "{v}");

    // A stale index past the end is skipped, as in the assistant's context.
    let (mut s, t, r) = selection_session();
    s.selected_track = Some(t);
    s.selected_region = Some(r);
    s.selected_notes = vec![1, 99];
    let v = ok(&mut s, "get_selection", json!({}));
    assert_eq!(v["notes"].as_array().map(|a| a.len()), Some(1), "{v}");
    assert_eq!(v["notes"][0]["index"], json!(1), "{v}");
    assert_eq!(v["selected_note_count"], json!(1), "{v}");

    // A region that no longer exists reports null.
    s.selected_region = Some(999_999);
    let v = ok(&mut s, "get_selection", json!({}));
    assert!(v["region"].is_null(), "{v}");
    assert_eq!(v["notes"], json!([]), "{v}");
    assert_eq!(v["selected_note_count"], json!(0), "{v}");

    // Pattern regions have no notes to select.
    let p = ok(&mut s, "create_pattern_region", json!({"track_id": t, "start": 12}))["region_id"].as_u64().unwrap();
    s.selected_region = Some(p);
    s.selected_notes = vec![0];
    let v = ok(&mut s, "get_selection", json!({}));
    assert_eq!(v["region"]["id"], json!(p), "{v}");
    assert_eq!(v["region"]["type"], json!("pattern"), "{v}");
    assert_eq!(v["notes"], json!([]), "{v}");
    assert_eq!(v["selected_note_count"], json!(0), "{v}");

    // A new project clears the selection.
    ok(&mut s, "new_project", json!({"discard_unsaved": true}));
    assert!(s.selected_notes.is_empty());
    assert_eq!(s.selected_region, None);
    let v = ok(&mut s, "get_selection", json!({}));
    assert!(v["track"].is_null() && v["region"].is_null(), "{v}");
}

#[test]
fn get_selection_lists_at_most_256_notes() {
    let mut s = Session::new(false);
    let t = ok(&mut s, "add_track", json!({"kind": "instrument"}))["track_id"].as_u64().unwrap();
    let notes: Vec<Value> = (0..300).map(|i| json!({"pitch": 48 + i % 24, "velocity": 100, "start": i as f64 * 0.25, "length": 0.25})).collect();
    let r = ok(&mut s, "create_midi_region", json!({"track_id": t, "start": 0, "length": 80, "notes": notes}))["region_id"].as_u64().unwrap();
    s.selected_track = Some(t);
    s.selected_region = Some(r);
    s.selected_notes = (0..300).collect();
    let v = ok(&mut s, "get_selection", json!({}));
    let listed = v["notes"].as_array().unwrap();
    assert_eq!(listed.len(), 256);
    assert_eq!(listed[0]["index"], json!(0));
    assert_eq!(listed[255]["index"], json!(255));
    assert_eq!(v["selected_note_count"], json!(300));
}

fn run(s: &mut Session, name: &str, args: Value, source: Source) -> Value {
    let cmd = Command::from_tool_call(name, &args).unwrap_or_else(|e| panic!("{name}: {e}"));
    s.execute(cmd, source).unwrap_or_else(|e| panic!("{name} {args} failed: {e}"))
}

fn bpm(s: &Session) -> f64 {
    s.project.tempo.initial_bpm()
}

#[test]
fn assistant_turn_is_one_undo_step() {
    let mut s = Session::new(false);
    s.begin_undo_group("assistant: make a beat");
    run(&mut s, "add_track", json!({"kind": "instrument"}), Source::Ai);
    run(&mut s, "set_tempo", json!({"bpm": 90}), Source::Ai);
    run(&mut s, "set_volume", json!({"track_id": 0, "db": -6}), Source::Ai);
    s.end_undo_group();
    assert_eq!(s.undo_group(), None);
    assert_eq!(s.can_undo(), Some("assistant: make a beat"));
    assert!(s.last_undo_is_ai());

    run(&mut s, "undo", json!({}), Source::Gui);
    assert!(s.project.tracks.is_empty());
    assert_eq!(bpm(&s), 120.0);
    assert_eq!(s.project.master.volume_db, 0.0);
    assert_eq!(s.can_undo(), None);

    run(&mut s, "redo", json!({}), Source::Gui);
    assert_eq!(s.project.tracks.len(), 1);
    assert_eq!(bpm(&s), 90.0);
    assert_eq!(s.project.master.volume_db, -6.0);
    assert_eq!(s.can_redo(), None);
    assert!(s.last_undo_is_ai());
}

#[test]
fn user_edit_during_an_assistant_turn_stays_its_own_step() {
    let mut s = Session::new(false);
    s.begin_undo_group("assistant: x");
    run(&mut s, "add_track", json!({"kind": "instrument"}), Source::Ai);
    run(&mut s, "set_tempo", json!({"bpm": 100}), Source::Gui);
    run(&mut s, "add_track", json!({"kind": "instrument"}), Source::Ai);
    run(&mut s, "set_volume", json!({"track_id": 0, "db": -3}), Source::Mcp);
    run(&mut s, "add_track", json!({"kind": "instrument"}), Source::Ai);
    s.end_undo_group();

    assert_eq!(s.can_undo(), Some("assistant: x"));
    assert!(s.last_undo_is_ai());
    run(&mut s, "undo", json!({}), Source::Gui);
    assert_eq!(s.project.tracks.len(), 2);

    assert_eq!(s.can_undo(), Some("set_volume"));
    assert!(!s.last_undo_is_ai());
    run(&mut s, "undo", json!({}), Source::Gui);
    assert_eq!(s.project.master.volume_db, 0.0);

    assert_eq!(s.can_undo(), Some("assistant: x"));
    run(&mut s, "undo", json!({}), Source::Gui);
    assert_eq!(s.project.tracks.len(), 1);

    assert_eq!(s.can_undo(), Some("set_tempo"));
    assert!(!s.last_undo_is_ai());
    run(&mut s, "undo", json!({}), Source::Gui);
    assert_eq!(bpm(&s), 120.0);

    assert_eq!(s.can_undo(), Some("assistant: x"));
    run(&mut s, "undo", json!({}), Source::Gui);
    assert!(s.project.tracks.is_empty());
    assert_eq!(s.can_undo(), None);
}

#[test]
fn assistant_undo_inside_a_turn_reverts_the_turn_so_far() {
    let mut s = Session::new(false);
    run(&mut s, "set_volume", json!({"track_id": 0, "db": -1}), Source::Gui);
    s.begin_undo_group("assistant: y");
    run(&mut s, "add_track", json!({"kind": "instrument"}), Source::Ai);
    run(&mut s, "set_tempo", json!({"bpm": 90}), Source::Ai);
    let v = run(&mut s, "undo", json!({}), Source::Ai);
    assert_eq!(v, json!({"undone": "assistant: y"}));
    assert!(s.project.tracks.is_empty());
    assert_eq!(bpm(&s), 120.0);
    assert_eq!(s.project.master.volume_db, -1.0);
    assert_eq!(s.can_redo(), Some("assistant: y"));

    run(&mut s, "set_tempo", json!({"bpm": 100}), Source::Ai);
    assert_eq!(s.can_redo(), None);
    assert_eq!(s.can_undo(), Some("assistant: y"));
    s.end_undo_group();

    run(&mut s, "undo", json!({}), Source::Gui);
    assert_eq!(bpm(&s), 120.0);
    assert_eq!(s.can_undo(), Some("set_volume"));
}

#[test]
fn new_project_inside_a_turn_starts_a_new_step() {
    let mut s = Session::new(false);
    s.begin_undo_group("assistant: z");
    run(&mut s, "add_track", json!({"kind": "instrument"}), Source::Ai);
    run(&mut s, "new_project", json!({"name": "Fresh", "discard_unsaved": true}), Source::Ai);
    assert_eq!(s.can_undo(), None);
    run(&mut s, "add_track", json!({"kind": "instrument"}), Source::Ai);
    assert_eq!(s.can_undo(), Some("assistant: z"));
    s.end_undo_group();

    run(&mut s, "undo", json!({}), Source::Gui);
    assert!(s.project.tracks.is_empty());
    assert_eq!(s.can_undo(), None);
}

#[test]
fn ai_commands_outside_a_group_keep_one_step_each() {
    let mut s = Session::new(false);
    run(&mut s, "set_tempo", json!({"bpm": 90}), Source::Ai);
    run(&mut s, "set_tempo", json!({"bpm": 100}), Source::Ai);
    run(&mut s, "undo", json!({}), Source::Gui);
    assert_eq!(bpm(&s), 90.0);
    assert_eq!(s.can_undo(), Some("set_tempo"));
    assert!(s.last_undo_is_ai());

    run(&mut s, "set_volume", json!({"track_id": 0, "db": -2}), Source::Gui);
    assert!(!s.last_undo_is_ai());
}

#[test]
fn fader_coalescing_still_merges_around_an_assistant_turn() {
    let mut s = Session::new(false);
    s.execute_coalesced(Command::SetVolume { track_id: 0, db: -3.0 }, "vol0").unwrap();
    s.execute_coalesced(Command::SetVolume { track_id: 0, db: -6.0 }, "vol0").unwrap();
    run(&mut s, "undo", json!({}), Source::Gui);
    assert_eq!(s.project.master.volume_db, 0.0);
    assert_eq!(s.can_undo(), None);
    run(&mut s, "redo", json!({}), Source::Gui);
    assert_eq!(s.project.master.volume_db, -6.0);

    s.begin_undo_group("assistant: pan");
    run(&mut s, "set_pan", json!({"track_id": 0, "pan": 0.5}), Source::Ai);
    s.execute_coalesced(Command::SetVolume { track_id: 0, db: -9.0 }, "vol0").unwrap();
    assert_eq!(s.can_undo(), Some("set_volume"));
    assert!(!s.last_undo_is_ai());
    run(&mut s, "set_pan", json!({"track_id": 0, "pan": 0.25}), Source::Ai);
    assert_eq!(s.can_undo(), Some("assistant: pan"));
    s.end_undo_group();

    run(&mut s, "undo", json!({}), Source::Gui);
    assert_eq!(s.project.master.pan, 0.5);
    run(&mut s, "undo", json!({}), Source::Gui);
    assert_eq!(s.project.master.volume_db, -6.0);
    run(&mut s, "undo", json!({}), Source::Gui);
    assert_eq!(s.project.master.pan, 0.0);
    run(&mut s, "undo", json!({}), Source::Gui);
    assert_eq!(s.project.master.volume_db, 0.0);
    assert_eq!(s.can_undo(), None);
}

#[test]
fn assistant_undo_group_closes_when_the_guard_drops() {
    use ondina_studio::session::AssistantUndoGroup;
    let shared = Session::shared(false);
    {
        let _g = AssistantUndoGroup::open(&shared, "assistant: a");
        assert_eq!(shared.lock().undo_group(), Some("assistant: a"));
    }
    assert_eq!(shared.lock().undo_group(), None);

    let r = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
        let _g = AssistantUndoGroup::open(&shared, "assistant: b");
        shared.lock().execute(Command::SetTempo { bpm: 95.0 }, Source::Ai).unwrap();
        panic!("boom");
    }));
    assert!(r.is_err());
    let mut s = shared.lock();
    assert_eq!(s.undo_group(), None);
    assert_eq!(s.can_undo(), Some("assistant: b"));
    s.execute(Command::SetTempo { bpm: 100.0 }, Source::Ai).unwrap();
    assert_eq!(s.can_undo(), Some("set_tempo"));
}

/// A MIDI track with an 8-beat region holding sustain 127@0, 0@1, 127@2, 0@3 and mod wheel 10@0.5, 50@1.5, 90@2.5.
fn region_with_pedal_and_mod_wheel(s: &mut Session) -> (u64, u64) {
    let t = ok(s, "add_track", json!({"kind": "instrument", "patch": "Lush Pad"}));
    let tid = t["track_id"].as_u64().unwrap();
    let r = ok(s, "create_midi_region", json!({"track_id": tid, "start": 0, "length": 8}));
    let rid = r["region_id"].as_u64().unwrap();
    let sustain = [(0.0, 127), (1.0, 0), (2.0, 127), (3.0, 0)].map(|(b, v)| json!({"beat": b, "controller": 64, "value": v}));
    let wheel = [(0.5, 10), (1.5, 50), (2.5, 90)].map(|(b, v)| json!({"beat": b, "controller": 1, "value": v}));
    let events: Vec<Value> = sustain.into_iter().chain(wheel).collect();
    ok(s, "add_cc", json!({"region_id": rid, "events": events}));
    (tid, rid)
}

/// The region's controller events as (beat, controller, value), read through get_region.
fn cc_list(s: &mut Session, rid: u64) -> Vec<(f64, u64, u64)> {
    let r = ok(s, "get_region", json!({"region_id": rid}));
    r["cc"].as_array().unwrap().iter().map(|e| (e["beat"].as_f64().unwrap(), e["controller"].as_u64().unwrap(), e["value"].as_u64().unwrap())).collect()
}

fn only(cc: &[(f64, u64, u64)], controller: u64) -> Vec<(f64, u64)> {
    cc.iter().filter(|e| e.1 == controller).map(|e| (e.0, e.2)).collect()
}

#[test]
fn replace_cc_replaces_one_controller_in_a_range() {
    let mut s = Session::new(false);
    let (_, rid) = region_with_pedal_and_mod_wheel(&mut s);
    let before = cc_list(&mut s, rid);

    let v = ok(&mut s, "replace_cc", json!({"region_id": rid, "controller": 64, "from_beat": 1, "to_beat": 3, "events": [{"beat": 1.5, "value": 127}, {"beat": 2.5, "value": 0}]}));
    assert_eq!((v["removed"].as_u64(), v["added"].as_u64(), v["cc_count"].as_u64()), (Some(2), Some(2), Some(7)));
    let after = cc_list(&mut s, rid);
    assert_eq!(only(&after, 64), vec![(0.0, 127), (1.5, 127), (2.5, 0), (3.0, 0)]);
    assert_eq!(only(&after, 1), only(&before, 1));
    assert!(after.windows(2).all(|w| w[0].0 <= w[1].0), "cc must stay sorted by beat: {after:?}");
    assert_eq!(s.can_undo(), Some("replace_cc"));

    ok(&mut s, "undo", json!({}));
    assert_eq!(cc_list(&mut s, rid), before);
    ok(&mut s, "redo", json!({}));
    assert_eq!(cc_list(&mut s, rid), after);
}

#[test]
fn replace_cc_without_range_clears_the_controller() {
    let mut s = Session::new(false);
    let (_, rid) = region_with_pedal_and_mod_wheel(&mut s);
    let before = cc_list(&mut s, rid);
    let v = ok(&mut s, "replace_cc", json!({"region_id": rid, "controller": 1, "events": []}));
    assert_eq!((v["removed"].as_u64(), v["added"].as_u64()), (Some(3), Some(0)));
    let after = cc_list(&mut s, rid);
    assert!(only(&after, 1).is_empty());
    assert_eq!(only(&after, 64), only(&before, 64));
    assert_eq!(after.len(), 4);
}

#[test]
fn replace_cc_rejects_bad_input_and_keeps_the_project() {
    let wav = tmp("replace_cc_audio.wav");
    write_wav(&wav, &[vec![0.1f32; 4410]], 44100);
    let mut s = Session::new(false);
    let (_, rid) = region_with_pedal_and_mod_wheel(&mut s);
    let a = ok(&mut s, "import_audio", json!({"path": wav.display().to_string(), "start": 0}));
    let audio = a["region_id"].as_u64().unwrap();
    let pt = ok(&mut s, "add_track", json!({"kind": "instrument", "patch": "Lush Pad"}));
    let pr = ok(&mut s, "create_pattern_region", json!({"track_id": pt["track_id"], "start": 0}));
    let pattern = pr["region_id"].as_u64().unwrap();

    let before = cc_list(&mut s, rid);
    let undo_before = s.can_undo().map(String::from);
    let ev = json!([{"beat": 1, "value": 10}]);
    for args in [
        json!({"region_id": audio, "controller": 64, "events": ev}),
        json!({"region_id": pattern, "controller": 64, "events": ev}),
        json!({"region_id": 999999, "controller": 64, "events": ev}),
        json!({"region_id": rid, "controller": 129, "events": ev}),
        json!({"region_id": rid, "controller": 64, "events": [{"beat": 1, "value": 128}]}),
        json!({"region_id": rid, "controller": 64, "from_beat": 3, "to_beat": 1, "events": ev}),
        json!({"region_id": rid, "controller": 64, "events": [{"beat": -1, "value": 10}]}),
    ] {
        assert!(call(&mut s, "replace_cc", args.clone()).is_err(), "{args} should fail");
        assert_eq!(cc_list(&mut s, rid), before);
        assert_eq!(s.can_undo().map(String::from), undo_before);
    }
}

#[test]
fn replace_cc_is_a_mutating_tool_with_optional_range() {
    let spec = tool_specs().into_iter().find(|t| t.name == "replace_cc").expect("replace_cc tool");
    assert!(!spec.read_only);
    let req: Vec<String> = spec.input_schema.get("required").and_then(|r| r.as_array()).cloned().unwrap_or_default().iter().filter_map(|v| v.as_str().map(String::from)).collect();
    for name in ["region_id", "controller", "events"] {
        assert!(req.iter().any(|r| r == name), "{name} must be required: {req:?}");
    }
    for name in ["from_beat", "to_beat"] {
        assert!(!req.iter().any(|r| r == name), "{name} must be optional: {req:?}");
    }
}

#[test]
fn session_counts_documents_replaced_by_open_and_new() {
    let mut s = Session::new(false);
    let first = s.document;
    // edits, undo, redo and save keep the same document
    ok(&mut s, "add_track", json!({"kind": "instrument", "patch": "Lush Pad"}));
    ok(&mut s, "undo", json!({}));
    ok(&mut s, "redo", json!({}));
    let path = tmp("document_counter.ondina");
    ok(&mut s, "save_project", json!({"path": path.display().to_string()}));
    assert_eq!(s.document, first);
    // New and Open replace it, from MCP and from the assistant alike
    ok(&mut s, "new_project", json!({"name": "Second", "discard_unsaved": true}));
    assert_eq!(s.document, first + 1);
    ok(&mut s, "open_project", json!({"path": path.display().to_string(), "discard_unsaved": true}));
    assert_eq!(s.document, first + 2);
    let c = Command::from_tool_call("new_project", &json!({})).unwrap();
    s.execute(c, Source::Ai).unwrap();
    assert_eq!(s.document, first + 3);
    // a failed open leaves the document as it was
    let missing = tmp("document_counter_missing.ondina");
    let _ = std::fs::remove_file(&missing);
    assert!(call(&mut s, "open_project", json!({"path": missing.display().to_string()})).is_err());
    assert_eq!(s.document, first + 3);
}

#[test]
fn add_vst3_refuses_network_and_non_vst3_paths() {
    let mut s = Session::new(false);
    let t = ok(&mut s, "add_track", json!({"kind": "instrument", "patch": "Lush Pad"}))["track_id"].as_u64().unwrap();
    let before = ok(&mut s, "get_project_json", json!({}));
    let undo = s.can_undo().map(str::to_string);
    for path in ["\\\\server\\share\\Evil.vst3", "C:\\plugins\\evil.dll"] {
        let e = call(&mut s, "add_vst3", json!({"track_id": t, "path": path})).unwrap_err();
        assert!(e.contains("plug-in path refused"), "{e}");
        let e = s.execute(Command::AddVst3 { track_id: t, path: path.into(), as_instrument: None, index: None }, Source::Script).unwrap_err();
        assert!(e.contains("plug-in path refused"), "{e}");
    }
    assert_eq!(ok(&mut s, "get_project_json", json!({})), before);
    assert_eq!(s.can_undo().map(str::to_string), undo);
}

#[test]
fn opened_project_with_network_vst3_reports_blocked_plugins_and_still_renders() {
    let mut s = Session::new(false);
    let t = ok(&mut s, "add_track", json!({"kind": "instrument", "patch": "Lush Pad"}))["track_id"].as_u64().unwrap();
    ok(&mut s, "create_midi_region", json!({"track_id": t, "start": 0, "length": 4, "notes": [{"pitch": 60, "velocity": 100, "start": 0, "length": 2}]}));
    let clean = tmp("blocked_vst3_clean.ondina");
    ok(&mut s, "save_project", json!({"path": clean.display().to_string()}));
    let v = ok(&mut s, "open_project", json!({"path": clean.display().to_string()}));
    assert!(v.get("blocked_plugins").is_none(), "{v}");

    // the same project edited by hand: a VST3 instrument on a share and a .dll insert on the master
    let mut doc: Value = serde_json::from_str(&std::fs::read_to_string(&clean).unwrap()).unwrap();
    let track = doc["project"]["tracks"].as_array_mut().unwrap().iter_mut().find(|x| x["id"] == json!(t)).unwrap();
    track["instrument"] = json!({"id": 900001, "kind": "vst3", "params": {}, "bypass": false, "vst3_path": "\\\\server\\share\\Evil.vst3"});
    doc["project"]["master"]["inserts"].as_array_mut().unwrap().push(json!({"id": 900002, "kind": "vst3", "params": {}, "bypass": false, "vst3_path": "C:\\plugins\\evil.dll"}));
    let evil = tmp("blocked_vst3.ondina");
    std::fs::write(&evil, serde_json::to_string_pretty(&doc).unwrap()).unwrap();

    let mut s2 = Session::new(false);
    let v = ok(&mut s2, "open_project", json!({"path": evil.display().to_string()}));
    let blocked = v["blocked_plugins"].as_array().unwrap_or_else(|| panic!("{v}"));
    assert_eq!(blocked.len(), 2, "{v}");
    // master first, then tracks
    assert_eq!(blocked[0]["track_id"], 0);
    assert_eq!(blocked[0]["plugin_id"], 900002);
    assert_eq!(blocked[0]["path"], "C:\\plugins\\evil.dll");
    assert_eq!(blocked[1]["track_id"], json!(t));
    assert_eq!(blocked[1]["plugin_id"], 900001);
    assert_eq!(blocked[1]["path"], "\\\\server\\share\\Evil.vst3");
    assert!(blocked.iter().all(|b| !b["reason"].as_str().unwrap().is_empty()));
    // the plug-ins stay in the project
    assert!(s2.project.find_plugin(900001).is_some() && s2.project.find_plugin(900002).is_some());
    let wav = tmp("blocked_vst3.wav");
    let _ = std::fs::remove_file(&wav);
    ok(&mut s2, "bounce", json!({"path": wav.display().to_string(), "start": 0, "end": 4, "tail_sec": 0}));
    assert!(std::fs::metadata(&wav).unwrap().len() > 44);
}

#[test]
fn scan_vst3_skips_network_folders() {
    let mut s = Session::new(false);
    let v = ok(&mut s, "scan_vst3", json!({"extra_paths": ["\\\\server\\vst"]}));
    let skipped = v["skipped_paths"].as_array().unwrap_or_else(|| panic!("{v}"));
    assert_eq!(skipped.len(), 1);
    assert_eq!(skipped[0]["path"], "\\\\server\\vst");
    assert!(!skipped[0]["reason"].as_str().unwrap().is_empty());
    assert!(v["plugins"].is_array());
}

/// A folder outside the VST3 folders with an (empty, never loaded) Evil.vst3 and a Missing.vst3 that does not exist.
fn untrusted_vst3_paths() -> (String, String) {
    let dir = tmp("untrusted_vst3");
    std::fs::create_dir_all(&dir).unwrap();
    let evil = dir.join("Evil.vst3");
    std::fs::write(&evil, b"").unwrap();
    let missing = dir.join("Missing.vst3");
    let _ = std::fs::remove_file(&missing);
    (evil.display().to_string(), missing.display().to_string())
}

#[test]
fn add_vst3_from_mcp_and_ai_only_loads_from_vst3_folders() {
    let mut s = Session::new(false);
    s.settings.vst3_folders = vec![];
    let t = ok(&mut s, "add_track", json!({"kind": "instrument", "patch": "Lush Pad"}))["track_id"].as_u64().unwrap();
    let (evil, missing) = untrusted_vst3_paths();
    let before = ok(&mut s, "get_project_json", json!({}));
    let undo = s.can_undo().map(str::to_string);
    for path in [&evil, &missing] {
        let e = call(&mut s, "add_vst3", json!({"track_id": t, "path": path})).unwrap_err();
        assert!(e.starts_with("plug-in path refused") && e.contains("VST3 folder added in Ondina"), "{e}");
        let e = s.execute(Command::AddVst3 { track_id: t, path: path.clone(), as_instrument: None, index: None }, Source::Ai).unwrap_err();
        assert!(e.starts_with("plug-in path refused") && e.contains("VST3 folder added in Ondina"), "{e}");
    }
    assert_eq!(ok(&mut s, "get_project_json", json!({})), before);
    assert_eq!(s.can_undo().map(str::to_string), undo);
}

#[test]
fn add_vst3_from_the_app_is_not_limited_to_vst3_folders() {
    let mut s = Session::new(false);
    s.settings.vst3_folders = vec![];
    let t = ok(&mut s, "add_track", json!({"kind": "instrument", "patch": "Lush Pad"}))["track_id"].as_u64().unwrap();
    let (_, missing) = untrusted_vst3_paths();
    for source in [Source::Gui, Source::Script] {
        let e = s.execute(Command::AddVst3 { track_id: t, path: missing.clone(), as_instrument: None, index: None }, source).unwrap_err();
        assert!(!e.starts_with("plug-in path refused"), "{source:?}: {e}");
    }
}

#[test]
fn add_vst3_from_mcp_accepts_a_folder_added_by_the_user() {
    let mut s = Session::new(false);
    let dir = tmp("user_vst3_folder");
    std::fs::create_dir_all(&dir).unwrap();
    s.settings.vst3_folders = vec![dir.display().to_string()];
    let t = ok(&mut s, "add_track", json!({"kind": "instrument", "patch": "Lush Pad"}))["track_id"].as_u64().unwrap();
    let missing = dir.join("Missing.vst3");
    let _ = std::fs::remove_file(&missing);
    let e = call(&mut s, "add_vst3", json!({"track_id": t, "path": missing.display().to_string()})).unwrap_err();
    assert!(!e.starts_with("plug-in path refused"), "{e}");
}

#[test]
fn scan_vst3_from_mcp_skips_folders_outside_vst3_folders() {
    let mut s = Session::new(false);
    s.settings.vst3_folders = vec![];
    let dir = tmp("scan_untrusted");
    std::fs::create_dir_all(&dir).unwrap();
    let dir = dir.display().to_string();
    let v = ok(&mut s, "scan_vst3", json!({"extra_paths": [dir]}));
    let skipped = v["skipped_paths"].as_array().unwrap_or_else(|| panic!("{v}"));
    assert!(skipped.iter().any(|x| x["path"] == json!(dir) && x["reason"].as_str().unwrap().contains("VST3 folder added in Ondina")), "{v}");
    let scanned = v["scanned_folders"].as_array().unwrap_or_else(|| panic!("{v}"));
    assert!(scanned.iter().all(|f| f.as_str().unwrap().to_lowercase() != dir.to_lowercase()), "{v}");
    let v = s.execute(Command::ScanVst3 { extra_paths: Some(vec![dir.clone()]) }, Source::Gui).unwrap();
    assert!(v.get("skipped_paths").is_none(), "{v}");
    assert_eq!(v["scanned_folders"].as_array().unwrap().len(), 1, "{v}");
}

#[test]
fn scan_vst3_uses_the_folders_added_by_the_user() {
    let mut s = Session::new(false);
    let dir = tmp("scan_user_folder");
    std::fs::create_dir_all(&dir).unwrap();
    s.settings.vst3_folders = vec![dir.display().to_string()];
    let v = ok(&mut s, "scan_vst3", json!({}));
    let scanned = v["scanned_folders"].as_array().unwrap_or_else(|| panic!("{v}"));
    assert_eq!(scanned.len(), 1, "{v}");
    assert!(scanned[0].as_str().unwrap().to_lowercase().ends_with("scan_user_folder"), "{v}");
    assert!(v.get("skipped_paths").is_none(), "{v}");
}

#[test]
fn get_settings_lists_the_vst3_folders() {
    let mut s = Session::new(false);
    s.settings.vst3_folders = vec!["C:\\Plugins".into()];
    assert_eq!(ok(&mut s, "get_settings", json!({}))["vst3_folders"], json!(["C:\\Plugins"]));
}

#[test]
fn transport_reports_dsp_overloads_since_the_last_play() {
    let mut s = Session::new(false);
    ok(&mut s, "play", json!({}));
    std::thread::sleep(std::time::Duration::from_millis(100));
    let tr = ok(&mut s, "get_transport", json!({}));
    assert_eq!(tr["dsp_overloads"], 0, "{tr}");
    s.engine.shared.record_block_load(2.0);
    assert_eq!(ok(&mut s, "get_transport", json!({}))["dsp_overloads"], 1);
    ok(&mut s, "play", json!({}));
    assert_eq!(ok(&mut s, "get_transport", json!({}))["dsp_overloads"], 0);
    ok(&mut s, "stop", json!({}));
}

#[test]
fn transport_reports_the_output_device_and_no_device_notice_while_nothing_went_wrong() {
    let mut s = Session::new(false);
    std::thread::sleep(std::time::Duration::from_millis(50));
    let tr = ok(&mut s, "get_transport", json!({}));
    assert_eq!(tr["output_device"], "Null device (no audio output)", "{tr}");
    assert!(tr["device_notice"].is_null(), "{tr}");
}

// ---- get_activity

#[test]
fn get_activity_is_read_only() {
    assert!(!Command::GetActivity { since: None, limit: None }.is_mutating());
    assert!(tool_specs().iter().any(|t| t.name == "get_activity" && t.read_only));
    let mut s = Session::new(false);
    ok(&mut s, "set_tempo", json!({"bpm": 100}));
    let (undo, rev, dirty) = (s.can_undo().map(String::from), s.revision, s.dirty);
    let a = ok(&mut s, "get_activity", json!({}));
    let (seq, len) = (a["latest_seq"].as_u64().unwrap(), s.log.len());
    let b = ok(&mut s, "get_activity", json!({"since": seq}));
    assert_eq!(s.can_undo().map(String::from), undo);
    assert_eq!(s.revision, rev);
    assert_eq!(s.dirty, dirty);
    // reading the activity is not itself activity
    assert_eq!(b["latest_seq"].as_u64().unwrap(), seq);
    assert!(b["entries"].as_array().unwrap().is_empty(), "{b}");
    assert_eq!(s.log.len(), len);
}

#[test]
fn get_activity_lists_who_did_what_and_folds_a_fader_drag() {
    let mut s = Session::new(false);
    s.execute_coalesced(Command::SetVolume { track_id: 0, db: -3.0 }, "vol:0").unwrap();
    s.execute_coalesced(Command::SetVolume { track_id: 0, db: -4.0 }, "vol:0").unwrap();
    s.execute(Command::from_tool_call("add_track", &json!({"kind": "instrument", "patch": "Lush Pad"})).unwrap(), Source::Gui).unwrap();
    ok(&mut s, "set_tempo", json!({"bpm": 98}));
    let a = ok(&mut s, "get_activity", json!({}));
    let e = a["entries"].as_array().unwrap();
    let rows: Vec<(&str, &str, u64)> = e.iter().map(|x| (x["source"].as_str().unwrap(), x["command"].as_str().unwrap(), x["count"].as_u64().unwrap())).collect();
    assert_eq!(rows, vec![("gui", "set_volume", 2), ("gui", "add_track", 1), ("mcp", "set_tempo", 1)], "{a}");
    assert!(e[0]["message"].as_str().unwrap().contains("-4"), "{a}");
    assert!(e.iter().all(|x| x["ok"] == true && x["mutating"] == true), "{a}");
    let seqs: Vec<u64> = e.iter().map(|x| x["seq"].as_u64().unwrap()).collect();
    assert!(seqs.windows(2).all(|w| w[0] < w[1]), "{seqs:?}");
    assert_eq!(*seqs.last().unwrap(), a["latest_seq"].as_u64().unwrap());

    // a long drag stays one entry
    let len = s.log.len();
    for i in 0..10 {
        s.execute_coalesced(Command::SetVolume { track_id: 0, db: -5.0 - i as f32 }, "vol:0").unwrap();
    }
    assert_eq!(s.log.len(), len + 1);
    assert_eq!(s.log.back().unwrap().count, 10);

    // another source in between splits the gesture; a failed step is not folded into the good ones
    ok(&mut s, "set_tempo", json!({"bpm": 99}));
    s.execute_coalesced(Command::SetVolume { track_id: 0, db: -1.0 }, "vol:0").unwrap();
    assert!(s.execute_coalesced(Command::SetVolume { track_id: 999_999, db: -1.0 }, "vol:0").is_err());
    let tail: Vec<(String, bool, u32)> = s.log.iter().rev().take(3).map(|l| (l.command.clone(), l.ok, l.count)).collect();
    assert_eq!(tail, vec![("set_volume".to_string(), false, 1), ("set_volume".to_string(), true, 1), ("set_tempo".to_string(), true, 1)]);
}

#[test]
fn get_activity_since_returns_only_newer_entries() {
    let mut s = Session::new(false);
    ok(&mut s, "set_tempo", json!({"bpm": 100}));
    let a = ok(&mut s, "get_activity", json!({}));
    let seq = a["latest_seq"].as_u64().unwrap();
    ok(&mut s, "set_key", json!({"key": "A minor"}));
    ok(&mut s, "get_transport", json!({}));
    let b = ok(&mut s, "get_activity", json!({"since": seq}));
    let names: Vec<&str> = b["entries"].as_array().unwrap().iter().map(|x| x["command"].as_str().unwrap()).collect();
    assert_eq!(names, vec!["set_key", "get_transport"], "{b}");
    assert_eq!(b["entries"][1]["mutating"], false);
    assert_eq!(b["truncated"], false);
    let c = ok(&mut s, "get_activity", json!({"since": b["latest_seq"]}));
    assert!(c["entries"].as_array().unwrap().is_empty(), "{c}");
    assert_eq!(c["truncated"], false);
}

#[test]
fn get_activity_reports_truncation() {
    let mut s = Session::new(false);
    ok(&mut s, "set_tempo", json!({"bpm": 100}));
    let first = ok(&mut s, "get_activity", json!({}))["latest_seq"].as_u64().unwrap();
    for _ in 0..510 {
        ok(&mut s, "get_transport", json!({}));
    }
    assert_eq!(s.log.len(), 500);
    let a = ok(&mut s, "get_activity", json!({"since": first}));
    assert_eq!(a["truncated"], true, "the entry after `first` fell out of the log");
    assert_eq!(a["entries"].as_array().unwrap().len(), 50);
    let latest = a["latest_seq"].as_u64().unwrap();
    assert_eq!(a["entries"][49]["seq"].as_u64().unwrap(), latest);
    assert_eq!(ok(&mut s, "get_activity", json!({"limit": 1000}))["entries"].as_array().unwrap().len(), 200);
    let z = ok(&mut s, "get_activity", json!({"since": latest, "limit": 0}));
    assert!(z["entries"].as_array().unwrap().is_empty());
    assert_eq!(z["truncated"], false);
    // a cursor from an earlier run of Ondina
    let r = ok(&mut s, "get_activity", json!({"since": latest + 1000}));
    assert_eq!(r["truncated"], true);
    assert!(r["entries"].as_array().unwrap().is_empty());
    // limit cuts the list: truncated too
    let few = ok(&mut s, "get_activity", json!({"since": latest - 5, "limit": 2}));
    assert_eq!(few["entries"].as_array().unwrap().len(), 2);
    assert_eq!(few["truncated"], true);
}

#[test]
fn get_activity_shows_when_the_project_was_replaced() {
    let mut s = Session::new(false);
    let a = ok(&mut s, "get_activity", json!({}));
    assert!(a["session_id"].as_u64().unwrap() > 0, "{a}");
    assert_eq!(a["revision"].as_u64().unwrap(), s.revision);
    ok(&mut s, "new_project", json!({"name": "Second", "discard_unsaved": true}));
    let b = ok(&mut s, "get_activity", json!({"since": a["latest_seq"]}));
    assert_ne!(b["document"], a["document"]);
    assert_eq!(b["session_id"], a["session_id"]);
    assert!(b["revision"].as_u64().unwrap() > a["revision"].as_u64().unwrap());
    let e = b["entries"].as_array().unwrap();
    assert_eq!(e.len(), 1, "{b}");
    assert_eq!((e[0]["command"].as_str(), e[0]["source"].as_str()), (Some("new_project"), Some("mcp")));
}

/// A saved project with one instrument track whose instrument is a vst3 at `plugin` and a vst3 insert at
/// `plugin` on the master. Returns the file, the track id, the instrument id and the master insert id.
fn vst3_project(name: &str, plugin: &Path) -> (PathBuf, u64, u64, u64) {
    std::fs::create_dir_all(plugin.parent().unwrap()).unwrap();
    let mut s = Session::new(false);
    let t = ok(&mut s, "add_track", json!({"kind": "instrument", "patch": "Lush Pad"}))["track_id"].as_u64().unwrap();
    ok(&mut s, "create_midi_region", json!({"track_id": t, "start": 0, "length": 4, "notes": [{"pitch": 60, "velocity": 100, "start": 0, "length": 2}]}));
    let file = tmp(name);
    ok(&mut s, "save_project", json!({"path": file.display().to_string()}));
    let mut doc: Value = serde_json::from_str(&std::fs::read_to_string(&file).unwrap()).unwrap();
    let path = plugin.display().to_string();
    let track = doc["project"]["tracks"].as_array_mut().unwrap().iter_mut().find(|x| x["id"] == json!(t)).unwrap();
    track["instrument"] = json!({"id": 900101, "kind": "vst3", "params": {}, "bypass": false, "vst3_path": path});
    doc["project"]["master"]["inserts"].as_array_mut().unwrap().push(json!({"id": 900102, "kind": "vst3", "params": {}, "bypass": false, "vst3_path": path}));
    std::fs::write(&file, serde_json::to_string_pretty(&doc).unwrap()).unwrap();
    (file, t, 900101, 900102)
}

/// Every vst3 path of a project (master inserts, then each track's instrument, MIDI effects and inserts).
fn vst3_paths(p: &ondina_core::model::Project) -> Vec<String> {
    let mut all: Vec<&ondina_core::model::PluginInstance> = p.master.inserts.iter().collect();
    for t in &p.tracks {
        all.extend(t.instrument.iter().chain(&t.midi_fx).chain(&t.channel.inserts));
    }
    all.into_iter().filter(|pi| pi.kind == "vst3").filter_map(|pi| pi.vst3_path.clone()).collect()
}

/// A .vst3 that does not exist, in a folder outside every VST3 folder.
fn c018_untrusted_plugin() -> PathBuf {
    tmp("c018_untrusted_vst3").join("Evil.vst3")
}

fn open_from(s: &mut Session, file: &Path, source: Source) -> Value {
    s.execute(Command::OpenProject { path: file.display().to_string(), discard_unsaved: None }, source).unwrap_or_else(|e| panic!("open from {source:?} failed: {e}"))
}

#[test]
fn mcp_opened_project_keeps_vst3_outside_the_vst3_folders_unloaded() {
    let plugin = c018_untrusted_plugin();
    let path = plugin.display().to_string();
    let (file, t, inst, fx) = vst3_project("c018_untrusted.ondina", &plugin);
    for source in [Source::Mcp, Source::Ai] {
        let mut s = Session::new(false);
        s.settings.vst3_folders = vec![];
        let v = open_from(&mut s, &file, source);
        let blocked = v["blocked_plugins"].as_array().unwrap_or_else(|| panic!("{source:?}: {v}"));
        assert_eq!(blocked.len(), 2, "{source:?}: {v}");
        // master first, then tracks
        assert_eq!(blocked[0]["track_id"], 0);
        assert_eq!(blocked[0]["plugin_id"], json!(fx));
        assert_eq!(blocked[1]["track_id"], json!(t));
        assert_eq!(blocked[1]["plugin_id"], json!(inst));
        for b in blocked {
            assert_eq!(b["path"], json!(path));
            let reason = b["reason"].as_str().unwrap();
            assert!(reason.contains("open the project from the app") && reason.contains("VST3 folder added in Ondina"), "{reason}");
        }
        for id in [inst, fx] {
            assert!(s.project.find_plugin(id).is_some(), "{source:?}: {id} left the project");
            assert!(s.engine.project().find_plugin(id).is_none(), "{source:?}: {id} handed to the engine");
            assert!(s.engine_project().find_plugin(id).is_none(), "{source:?}: {id} in the render snapshot");
        }
        let wav = tmp("c018_untrusted.wav");
        let _ = std::fs::remove_file(&wav);
        ok(&mut s, "bounce", json!({"path": wav.display().to_string(), "start": 0, "end": 4, "tail_sec": 0}));
        assert!(std::fs::metadata(&wav).unwrap().len() > 44);
        // the saved file keeps the plug-ins
        let saved = tmp("c018_untrusted_saved.ondina");
        ok(&mut s, "save_project", json!({"path": saved.display().to_string()}));
        let reloaded = ondina_core::load_project_str(&std::fs::read_to_string(&saved).unwrap()).unwrap();
        assert_eq!(vst3_paths(&reloaded), vec![path.clone(), path.clone()], "{source:?}");
    }
}

#[test]
fn gui_opened_project_still_loads_vst3_outside_the_vst3_folders() {
    let plugin = c018_untrusted_plugin();
    let (file, _, inst, fx) = vst3_project("c018_gui.ondina", &plugin);
    for source in [Source::Gui, Source::Script] {
        let mut s = Session::new(false);
        s.settings.vst3_folders = vec![];
        let v = open_from(&mut s, &file, source);
        assert!(v.get("blocked_plugins").is_none(), "{source:?}: {v}");
        assert!(s.engine.project().find_plugin(inst).is_some() && s.engine.project().find_plugin(fx).is_some(), "{source:?}");
    }
    // an MCP open, then an open of the same file from the app
    let mut s = Session::new(false);
    s.settings.vst3_folders = vec![];
    open_from(&mut s, &file, Source::Mcp);
    assert!(s.engine.project().find_plugin(inst).is_none());
    let v = open_from(&mut s, &file, Source::Gui);
    assert!(v.get("blocked_plugins").is_none(), "{v}");
    assert!(s.engine.project().find_plugin(inst).is_some() && s.engine.project().find_plugin(fx).is_some());
}

#[test]
fn blocked_vst3_stays_unloaded_after_duplicate_undo_and_redo() {
    let plugin = c018_untrusted_plugin();
    let path = plugin.display().to_string();
    let (file, t, _, _) = vst3_project("c018_duplicate.ondina", &plugin);
    let mut s = Session::new(false);
    s.settings.vst3_folders = vec![];
    open_from(&mut s, &file, Source::Mcp);
    ok(&mut s, "duplicate_track", json!({"track_id": t}));
    let instruments = |p: &ondina_core::model::Project| p.tracks.iter().filter(|x| x.instrument.as_ref().and_then(|i| i.vst3_path.as_deref()) == Some(path.as_str())).count();
    assert_eq!(instruments(&s.project), 2, "duplicate_track did not copy the instrument");
    assert!(!vst3_paths(s.engine.project()).contains(&path), "the copy was handed to the engine");
    ok(&mut s, "undo", json!({}));
    assert_eq!(instruments(&s.project), 1);
    assert!(!vst3_paths(s.engine.project()).contains(&path), "after undo");
    ok(&mut s, "redo", json!({}));
    assert_eq!(instruments(&s.project), 2);
    assert!(!vst3_paths(s.engine.project()).contains(&path), "after redo");
    ok(&mut s, "new_project", json!({"discard_unsaved": true}));
    assert!(call(&mut s, "undo", json!({})).is_err());
    assert!(vst3_paths(s.engine.project()).is_empty());
}

#[test]
fn mcp_opened_project_loads_vst3_from_a_user_vst3_folder() {
    let dir = tmp("c018_trusted_vst3");
    std::fs::create_dir_all(&dir).unwrap();
    let (file, _, inst, fx) = vst3_project("c018_trusted.ondina", &dir.join("Good.vst3"));
    let mut s = Session::new(false);
    s.settings.vst3_folders = vec![dir.display().to_string()];
    let v = open_from(&mut s, &file, Source::Mcp);
    assert!(v.get("blocked_plugins").is_none(), "{v}");
    assert!(s.engine.project().find_plugin(inst).is_some() && s.engine.project().find_plugin(fx).is_some());
}

#[test]
fn open_plugin_editor_refuses_a_vst3_blocked_by_an_mcp_open() {
    let plugin = c018_untrusted_plugin();
    let (file, _, inst, _) = vst3_project("c018_editor.ondina", &plugin);
    let mut s = Session::new(false);
    s.settings.vst3_folders = vec![];
    open_from(&mut s, &file, Source::Mcp);
    let e = call(&mut s, "open_plugin_editor", json!({"plugin_id": inst})).unwrap_err();
    assert!(e.contains("open the project from the app"), "{e}");
    let e = s.execute(Command::OpenPluginEditor { plugin_id: inst }, Source::Gui).unwrap_err();
    assert!(e.contains("open the project from the app"), "{e}");
}

#[test]
fn unloaded_vst3_lists_refused_and_kept_out_plugins_with_the_reason() {
    // every key is a vst3 instance of the current project (never a built-in such as the patch's inserts)
    let only_vst3 = |s: &Session| {
        for id in s.unloaded_vst3().keys() {
            assert_eq!(s.project.find_plugin(*id).map(|p| p.kind.as_str()), Some("vst3"), "{id}");
        }
    };
    let blocked_reason = |v: &Value, id: u64| -> String {
        let b = v["blocked_plugins"].as_array().unwrap_or_else(|| panic!("{v}"));
        b.iter().find(|x| x["plugin_id"] == json!(id)).unwrap_or_else(|| panic!("{id} not in {v}"))["reason"].as_str().unwrap().to_string()
    };
    let (file, t, inst, fx) = vst3_project("c024_unloaded.ondina", &c018_untrusted_plugin());
    let mut s = Session::new(false);
    s.settings.vst3_folders = vec![];
    // opened from the app: everything loads
    open_from(&mut s, &file, Source::Gui);
    assert!(s.unloaded_vst3().is_empty(), "{:?}", s.unloaded_vst3());
    // opened by an MCP client: both instances stay out, with the reason open_project gave
    let v = open_from(&mut s, &file, Source::Mcp);
    let u = s.unloaded_vst3();
    assert_eq!(u.len(), 2, "{u:?}");
    for id in [inst, fx] {
        let reason = u.get(&id).unwrap_or_else(|| panic!("{id} missing from {u:?}"));
        assert!(reason.contains("open the project from the app"), "{reason}");
        assert_eq!(*reason, blocked_reason(&v, id));
    }
    only_vst3(&s);
    // the copy of the instrument has a new id and the same path
    ok(&mut s, "duplicate_track", json!({"track_id": t}));
    assert_eq!(s.unloaded_vst3().len(), 3, "{:?}", s.unloaded_vst3());
    only_vst3(&s);
    ok(&mut s, "new_project", json!({"discard_unsaved": true}));
    assert!(s.unloaded_vst3().is_empty());
    // a path the plug-in check refuses, even when the app opens the project
    let dll = tmp("c024_unloaded_dll").join("evil.dll");
    let (file2, _, inst2, fx2) = vst3_project("c024_unloaded_dll.ondina", &dll);
    let v = open_from(&mut s, &file2, Source::Gui);
    let u = s.unloaded_vst3();
    assert_eq!(u.len(), 2, "{u:?}");
    for id in [inst2, fx2] {
        let reason = u.get(&id).unwrap_or_else(|| panic!("{id} missing from {u:?}"));
        assert!(reason.contains("not a .vst3"), "{reason}");
        assert_eq!(*reason, blocked_reason(&v, id));
    }
    only_vst3(&s);
}

#[test]
fn session_players_phrase_in_bars_of_a_three_four_project() {
    use ondina_core::model::RegionContent;
    let mut s = Session::new(false);
    ok(&mut s, "set_time_signature", json!({"numerator": 3, "denominator": 4}));
    ok(&mut s, "set_chord_progression", json!({"start": 0, "chords": ["C", "Am", "F", "G"]}));
    let t = ok(&mut s, "add_track", json!({"kind": "instrument"}))["track_id"].as_u64().unwrap();

    let bass = ok(&mut s, "generate_bass", json!({"track_id": t, "start": 0, "bars": 4, "style": "syncopated", "seed": 1}))["region_id"].as_u64().unwrap();
    let keys = ok(&mut s, "generate_chords", json!({"track_id": t, "start": 0, "bars": 4, "style": "pop", "seed": 2}))["region_id"].as_u64().unwrap();
    let mel = ok(&mut s, "generate_melody", json!({"track_id": t, "start": 0, "bars": 4, "density": 0.5, "seed": 3}))["region_id"].as_u64().unwrap();

    for (what, rid) in [("bass", bass), ("chords", keys), ("melody", mel)] {
        let (_, region) = s.project.find_region(rid).unwrap();
        assert_eq!(region.length, 12.0, "{what}: four bars of 3/4");
        let RegionContent::Midi(m) = &region.content else { panic!("{what} region is not MIDI") };
        for k in 0..4 {
            let downbeat = 3.0 * k as f64;
            assert!(m.notes.iter().any(|n| (n.start - downbeat).abs() < 1e-9), "{what}: bar {k} has no note on its first beat");
        }
        for n in &m.notes {
            let bar_end = ((n.start / 3.0 + 1e-9).floor() + 1.0) * 3.0;
            assert!(n.start + n.length <= bar_end + 1e-9, "{what}: {n:?} crosses a 3/4 bar line");
        }
        if what == "bass" {
            for n in &m.notes {
                let off = n.start - (n.start / 3.0 + 1e-9).floor() * 3.0;
                assert!([0.0, 0.75, 1.5, 2.0, 2.5].iter().any(|o| (off - o).abs() < 1e-9), "bass offset {off} is not in the 3/4 cell");
            }
        }
    }

    ok(&mut s, "undo", json!({}));
    assert!(s.project.find_region(mel).is_none(), "one undo removes the melody region");
    assert!(s.project.find_region(keys).is_some());
}

#[test]
fn add_notes_reports_where_the_added_notes_landed() {
    let mut s = Session::new(false);
    let t = ok(&mut s, "add_track", json!({"kind": "instrument"}))["track_id"].as_u64().unwrap();
    let start = json!([
        {"pitch": 60, "velocity": 100, "start": 0.0, "length": 0.5},
        {"pitch": 64, "velocity": 100, "start": 2.0, "length": 0.5}
    ]);
    let rid = ok(&mut s, "create_midi_region", json!({"track_id": t, "start": 0, "length": 8, "notes": start}))["region_id"].as_u64().unwrap();
    let before = ok(&mut s, "get_region", json!({"region_id": rid}))["notes"].clone();
    // three notes in no particular order, one identical to a note already there
    let added = [(62u64, 3.0f64), (60, 0.0), (67, 1.0)];
    let notes: Vec<Value> = added.iter().map(|&(p, st)| json!({"pitch": p, "velocity": 100, "start": st, "length": 0.5})).collect();
    let r = ok(&mut s, "add_notes", json!({"region_id": rid, "notes": notes}));
    let indices: Vec<usize> = r["indices"].as_array().expect("indices").iter().map(|x| x.as_u64().unwrap() as usize).collect();
    assert_eq!(indices, vec![1, 2, 4], "{r}");
    assert_eq!(r["note_count"], json!(5));
    let got = ok(&mut s, "get_region", json!({"region_id": rid}))["notes"].clone();
    let key = |n: &Value| (n["pitch"].as_u64().unwrap(), n["start"].as_f64().unwrap());
    let mut read: Vec<(u64, f64)> = indices.iter().map(|&i| key(&got[i])).collect();
    let mut want = added.to_vec();
    read.sort_by(|a, b| a.partial_cmp(b).unwrap());
    want.sort_by(|a, b| a.partial_cmp(b).unwrap());
    assert_eq!(read, want);
    // the existing 60 keeps index 0, the added one comes right after it
    assert_eq!(key(&got[0]), (60, 0.0));
    assert!(!indices.contains(&0));

    ok(&mut s, "transpose_notes", json!({"region_id": rid, "semitones": 12, "indices": r["indices"]}));
    let moved = ok(&mut s, "get_region", json!({"region_id": rid}))["notes"].clone();
    assert_eq!(key(&moved[0]), (60, 0.0), "the existing 60 stays");
    assert_eq!(key(&moved[3]), (64, 2.0), "the existing 64 stays");
    for &i in &indices {
        assert_eq!(moved[i]["pitch"].as_u64().unwrap(), got[i]["pitch"].as_u64().unwrap() + 12, "note {i} moved up an octave");
    }

    ok(&mut s, "undo", json!({}));
    ok(&mut s, "undo", json!({}));
    assert_eq!(ok(&mut s, "get_region", json!({"region_id": rid}))["notes"], before);

    let spec = tool_specs().into_iter().find(|t| t.name == "add_notes").unwrap();
    assert!(spec.description.contains("indices"), "{}", spec.description);
    let props = &spec.input_schema["properties"];
    assert!(props.get("region_id").is_some() && props.get("notes").is_some());
    let req = spec.input_schema.get("required").and_then(|r| r.as_array()).cloned().unwrap_or_default();
    assert!(req.iter().all(|x| x == "region_id" || x == "notes"), "no new required parameter: {req:?}");
}

/// The region object exactly as get_project_json lists it under tracks[].regions.
fn region_object(s: &mut Session, rid: u64) -> Value {
    let p = ok(s, "get_project_json", json!({}));
    p["tracks"].as_array().unwrap().iter().flat_map(|t| t["regions"].as_array().unwrap().iter()).find(|r| r["id"] == json!(rid)).cloned().unwrap_or(Value::Null)
}

fn without(mut v: Value, keys: &[&str]) -> Value {
    for k in keys {
        v.as_object_mut().unwrap().remove(*k);
    }
    v
}

/// Ids of the regions on one track, from get_project_json.
fn track_region_ids(s: &mut Session, track_id: u64) -> Vec<u64> {
    let p = ok(s, "get_project_json", json!({}));
    let t = p["tracks"].as_array().unwrap().iter().find(|t| t["id"] == json!(track_id)).cloned().unwrap();
    t["regions"].as_array().unwrap().iter().map(|r| r["id"].as_u64().unwrap()).collect()
}

#[test]
fn paste_region_copies_a_midi_region_to_another_track_in_one_undo_step() {
    let mut s = Session::new(false);
    let a = ok(&mut s, "add_track", json!({"kind": "instrument", "patch": "Lush Pad"}))["track_id"].as_u64().unwrap();
    let b = ok(&mut s, "add_track", json!({"kind": "instrument", "patch": "Lush Pad"}))["track_id"].as_u64().unwrap();
    let notes = json!([
        {"pitch": 60, "velocity": 100, "start": 0.0, "length": 1.0},
        {"pitch": 64, "velocity": 90, "start": 1.0, "length": 1.0},
        {"pitch": 67, "velocity": 80, "start": 2.0, "length": 1.0},
        {"pitch": 72, "velocity": 70, "start": 3.0, "length": 2.0}
    ]);
    let rid = ok(&mut s, "create_midi_region", json!({"track_id": a, "start": 0, "length": 8, "notes": notes}))["region_id"].as_u64().unwrap();
    ok(&mut s, "add_cc", json!({"region_id": rid, "events": [{"beat": 0.0, "controller": 64, "value": 127}, {"beat": 2.0, "controller": 1, "value": 40}]}));
    ok(&mut s, "set_lyrics", json!({"region_id": rid, "text": "la la"}));
    ok(&mut s, "set_region_properties", json!({"region_id": rid, "name": "Chorus", "velocity_offset": 12, "transpose": 3, "loop_count": 2, "color": "#FF8800"}));
    let orig = region_object(&mut s, rid);
    assert_eq!(orig["content"]["cc"].as_array().unwrap().len(), 2);
    assert_eq!(orig["content"]["lyrics"].as_array().unwrap().len(), 2);
    assert_eq!(orig["content"]["velocity_offset"], json!(12));
    let before = ok(&mut s, "get_project_json", json!({}));

    let r = ok(&mut s, "paste_region", json!({"track_id": b, "start": 16, "region": orig}));
    let new = r["region_id"].as_u64().unwrap();
    assert_ne!(new, rid);
    assert_eq!(r["track_id"], json!(b));
    assert_eq!(r["start"].as_f64(), Some(16.0));

    let pasted = region_object(&mut s, new);
    assert_eq!(pasted["start"].as_f64(), Some(16.0));
    assert_eq!(without(pasted, &["id", "start"]), without(orig.clone(), &["id", "start"]));
    assert!(track_region_ids(&mut s, b).contains(&new));
    assert!(!track_region_ids(&mut s, a).contains(&new));
    assert_eq!(region_object(&mut s, rid), orig);

    assert_eq!(s.can_undo(), Some("paste_region"));
    ok(&mut s, "undo", json!({}));
    assert_eq!(ok(&mut s, "get_project_json", json!({})), before);
}

#[test]
fn paste_region_copies_pattern_and_audio_regions_and_drops_the_take() {
    let mut s = Session::new(false);
    let a = ok(&mut s, "add_track", json!({"kind": "instrument", "patch": "Lush Pad"}))["track_id"].as_u64().unwrap();
    let b = ok(&mut s, "add_track", json!({"kind": "instrument", "patch": "Lush Pad"}))["track_id"].as_u64().unwrap();
    let pid = ok(&mut s, "create_pattern_region", json!({"track_id": a, "start": 0, "steps": 16}))["region_id"].as_u64().unwrap();
    let pat = region_object(&mut s, pid);
    let new = ok(&mut s, "paste_region", json!({"track_id": b, "start": 8, "region": pat}))["region_id"].as_u64().unwrap();
    let pasted = region_object(&mut s, new);
    assert_eq!(pasted["start"].as_f64(), Some(8.0));
    assert_eq!(without(pasted, &["id", "start"]), without(pat.clone(), &["id", "start"]));
    assert!(track_region_ids(&mut s, b).contains(&new));

    // A pattern stretched past 128 steps keeps every step: 16th notes over 16 bars are 256 steps.
    let long = ok(&mut s, "create_pattern_region", json!({"track_id": a, "start": 64, "steps": 16}))["region_id"].as_u64().unwrap();
    ok(&mut s, "set_pattern_step", json!({"region_id": long, "row": 0, "step": 3, "velocity": 90}));
    assert_eq!(ok(&mut s, "resize_region", json!({"region_id": long, "length": 64}))["length"].as_f64(), Some(64.0));
    ok(&mut s, "set_pattern_step", json!({"region_id": long, "row": 0, "step": 250, "velocity": 100}));
    let long_pat = region_object(&mut s, long);
    assert_eq!(long_pat["content"]["steps"].as_u64(), Some(256));
    let new = ok(&mut s, "paste_region", json!({"track_id": b, "start": 128, "region": long_pat}))["region_id"].as_u64().unwrap();
    let pasted = region_object(&mut s, new);
    assert_eq!(pasted["length"].as_f64(), Some(64.0));
    assert_eq!(without(pasted, &["id", "start"]), without(long_pat, &["id", "start"]));

    let wav = tmp("paste_region.wav");
    write_wav(&wav, &[vec![0.1f32; 44100]], 44100);
    let aid = ok(&mut s, "import_audio", json!({"path": wav.display().to_string(), "start": 0}))["region_id"].as_u64().unwrap();
    let t2 = ok(&mut s, "add_track", json!({"kind": "audio"}))["track_id"].as_u64().unwrap();
    let audio = region_object(&mut s, aid);
    let new = ok(&mut s, "paste_region", json!({"track_id": t2, "start": 4, "region": audio}))["region_id"].as_u64().unwrap();
    let pasted = region_object(&mut s, new);
    assert_eq!(pasted["start"].as_f64(), Some(4.0));
    assert_eq!(pasted["content"]["file_id"], audio["content"]["file_id"]);
    assert_eq!(without(pasted, &["id", "start"]), without(audio, &["id", "start"]));
    assert!(track_region_ids(&mut s, t2).contains(&new));

    let mut taken = pat;
    taken["take"] = json!(3);
    let new = ok(&mut s, "paste_region", json!({"track_id": b, "start": 32, "region": taken}))["region_id"].as_u64().unwrap();
    assert_eq!(region_object(&mut s, new)["take"], Value::Null);
}

#[test]
fn pattern_steps_sent_under_one_key_are_one_undo_step_and_keep_the_other_velocities() {
    let mut s = Session::new(false);
    let tid = ok(&mut s, "add_track", json!({"kind": "instrument", "patch": "Pop Drum Kit"}))["track_id"].as_u64().unwrap();
    let rid = ok(&mut s, "create_pattern_region", json!({"track_id": tid, "start": 0, "steps": 16}))["region_id"].as_u64().unwrap();
    for (row, step, velocity) in [(3, 0, 120), (3, 10, 100), (3, 12, 77), (1, 4, 45)] {
        ok(&mut s, "set_pattern_step", json!({"region_id": rid, "row": row, "step": step, "velocity": velocity}));
    }
    let before = ok(&mut s, "get_project_json", json!({}));

    // a paint stroke over steps 1-8 of row 3, sent the way the editor sends it: one key for the whole gesture
    for step in 1..=8 {
        let c = Command::SetPatternStep { region_id: rid, row: 3, step, velocity: 100 };
        s.execute_coalesced(c, "step-paint-1").unwrap();
    }
    let rows = region_object(&mut s, rid)["content"]["rows"].clone();
    let row3: Vec<u64> = rows[3]["steps"].as_array().unwrap().iter().map(|v| v.as_u64().unwrap()).collect();
    assert_eq!(row3, [120, 100, 100, 100, 100, 100, 100, 100, 100, 0, 100, 0, 77, 0, 0, 0]);
    assert_eq!(rows[1]["steps"][4].as_u64(), Some(45));

    ok(&mut s, "undo", json!({}));
    assert_eq!(ok(&mut s, "get_project_json", json!({})), before);
}

#[test]
fn paste_region_refuses_a_region_that_does_not_fit_and_keeps_the_project() {
    let mut s = Session::new(false);
    let inst = ok(&mut s, "add_track", json!({"kind": "instrument", "patch": "Lush Pad"}))["track_id"].as_u64().unwrap();
    let notes = json!([{"pitch": 60, "velocity": 100, "start": 0.0, "length": 1.0}]);
    let mid = ok(&mut s, "create_midi_region", json!({"track_id": inst, "start": 0, "length": 4, "notes": notes}))["region_id"].as_u64().unwrap();
    let pid = ok(&mut s, "create_pattern_region", json!({"track_id": inst, "start": 8}))["region_id"].as_u64().unwrap();
    let wav = tmp("paste_region_refused.wav");
    write_wav(&wav, &[vec![0.1f32; 44100]], 44100);
    let aid = ok(&mut s, "import_audio", json!({"path": wav.display().to_string(), "start": 0}))["region_id"].as_u64().unwrap();
    let audio_track = s.project.find_region(aid).unwrap().0.id;
    let bus = ok(&mut s, "add_track", json!({"kind": "bus"}))["track_id"].as_u64().unwrap();
    let midi = region_object(&mut s, mid);
    let pat = region_object(&mut s, pid);
    let audio = region_object(&mut s, aid);

    let before = ok(&mut s, "get_project_json", json!({}));
    let undo_before = s.can_undo().map(String::from);
    let mut missing_file = audio.clone();
    missing_file["content"]["file_id"] = json!(999_999);
    let mut empty = midi.clone();
    empty["length"] = json!(0);
    let mut huge = pat.clone();
    huge["content"]["steps"] = json!(100_000);
    let cases = [
        ("audio on an instrument track", json!({"track_id": inst, "start": 4, "region": audio})),
        ("MIDI on an audio track", json!({"track_id": audio_track, "start": 4, "region": midi})),
        ("pattern on a bus", json!({"track_id": bus, "start": 4, "region": pat})),
        ("missing audio file", json!({"track_id": audio_track, "start": 4, "region": missing_file})),
        ("missing track", json!({"track_id": 999_999, "start": 4, "region": midi})),
        ("zero length", json!({"track_id": inst, "start": 4, "region": empty})),
        ("pattern with too many steps", json!({"track_id": inst, "start": 4, "region": huge})),
    ];
    for (what, args) in cases {
        let r = call(&mut s, "paste_region", args);
        assert!(r.is_err(), "{what}: {r:?}");
        if what == "missing audio file" {
            assert!(r.unwrap_err().contains("not in this project"), "{what}");
        }
        assert_eq!(ok(&mut s, "get_project_json", json!({})), before, "{what}");
        assert_eq!(s.can_undo().map(String::from), undo_before, "{what}");
    }
}

#[test]
fn paste_region_tool_takes_a_region_object() {
    let specs = tool_specs();
    let spec = specs.iter().find(|t| t.name == "paste_region").expect("paste_region tool");
    assert!(!spec.read_only);
    assert!(spec.description.contains("get_project_json"));
    let mut required: Vec<&str> = spec.input_schema["required"].as_array().unwrap().iter().map(|v| v.as_str().unwrap()).collect();
    required.sort_unstable();
    assert_eq!(required, ["region", "start", "track_id"]);
    let region = &spec.input_schema["properties"]["region"];
    assert_eq!(region["type"], json!("object"), "{region}");
    assert!(region["properties"].get("content").is_some(), "{region}");
    assert!(!region.to_string().contains("$ref"), "{region}");
}

#[test]
fn recovered_copy_opens_as_unsaved_work_without_a_path() {
    let mut s = Session::new(false);
    ok(&mut s, "add_track", json!({"kind": "instrument", "patch": "Lush Pad"}));
    ok(&mut s, "set_project_name", json!({"name": "Night Drive"}));
    let copy = tmp("recovered_copy.ondina");
    std::fs::write(&copy, ondina_core::save_project_string(&s.project).unwrap()).unwrap();
    let before = std::fs::read_to_string(&copy).unwrap();

    let mut r = Session::new(false);
    r.open_recovered(&copy).unwrap();
    assert_eq!(r.project.name, "Night Drive");
    assert_eq!(r.project.tracks.len(), 1);
    assert_eq!(r.path, None, "the next save must ask where to write");
    assert!(r.dirty, "recovered work is unsaved work");
    assert_eq!(r.can_undo(), None);
    let err = r.execute(Command::SaveProject { path: None }, Source::Gui).unwrap_err();
    assert!(err.contains("never been saved"), "{err}");
    assert_eq!(std::fs::read_to_string(&copy).unwrap(), before, "the copy is never written over");
}

fn safe_save_dir(name: &str) -> PathBuf {
    let dir = tmp(&format!("safe_save_{name}"));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).unwrap();
    dir
}

fn listing(dir: &Path) -> Vec<String> {
    let mut v: Vec<String> = std::fs::read_dir(dir).unwrap().map(|e| e.unwrap().file_name().to_string_lossy().into_owned()).collect();
    v.sort();
    v
}

#[test]
fn failed_save_keeps_the_previous_project_byte_for_byte() {
    let dir = safe_save_dir("failed");
    let song = dir.join("song.ondina");
    let mut s = Session::new(false);
    s.settings.keep_save_backup = true;
    ok(&mut s, "add_track", json!({"kind": "instrument"}));
    ok(&mut s, "save_project", json!({"path": song.display().to_string()}));
    let before = std::fs::read(&song).unwrap();
    ok(&mut s, "add_track", json!({"kind": "instrument"}));
    std::fs::create_dir_all(dir.join("song.ondina.tmp")).unwrap();
    ondina_studio::fs_safe::fail_next_temp_file_for_test();
    let e = call(&mut s, "save_project", json!({"path": song.display().to_string()})).unwrap_err();
    assert!(e.contains("cannot save"), "{e}");
    assert!(e.contains("nothing was overwritten"), "{e}");
    assert_eq!(std::fs::read(&song).unwrap(), before);
    assert!(!dir.join("song.ondina.bak").exists());
    assert!(s.dirty, "a failed save leaves the work unsaved");
}

#[test]
fn save_over_a_project_keeps_the_previous_version_as_bak() {
    let dir = safe_save_dir("bak");
    let song = dir.join("song.ondina");
    let bak = dir.join("song.ondina.bak");
    let mut s = Session::new(false);
    s.settings.keep_save_backup = true;
    ok(&mut s, "add_track", json!({"kind": "instrument"}));
    let first = ok(&mut s, "save_project", json!({"path": song.display().to_string()}));
    assert!(first.get("backup").is_none(), "{first}");
    assert!(!bak.exists());
    let first_bytes = std::fs::read(&song).unwrap();
    ok(&mut s, "add_track", json!({"kind": "audio"}));
    let second = ok(&mut s, "save_project", json!({"path": song.display().to_string()}));
    assert_eq!(second["backup"], json!(bak.display().to_string()), "{second}");
    assert_eq!(std::fs::read(&bak).unwrap(), first_bytes);

    let old = dir.join("old.ondina");
    std::fs::copy(&bak, &old).unwrap();
    let mut new_session = Session::new(false);
    ok(&mut new_session, "open_project", json!({"path": song.display().to_string()}));
    let mut old_session = Session::new(false);
    ok(&mut old_session, "open_project", json!({"path": old.display().to_string()}));
    assert_eq!(new_session.project.tracks.len(), old_session.project.tracks.len() + 1);
    assert_eq!(listing(&dir), vec!["old.ondina", "song.ondina", "song.ondina.bak"]);
}

#[test]
fn save_without_the_backup_setting_writes_no_bak() {
    let dir = safe_save_dir("no_bak");
    let song = dir.join("song.ondina");
    let mut s = Session::new(false);
    s.settings.keep_save_backup = false;
    ok(&mut s, "save_project", json!({"path": song.display().to_string()}));
    ok(&mut s, "add_track", json!({"kind": "instrument"}));
    let second = ok(&mut s, "save_project", json!({"path": song.display().to_string()}));
    assert!(second.get("backup").is_none(), "{second}");
    assert!(!dir.join("song.ondina.bak").exists());
}

#[test]
fn save_leaves_a_bak_that_is_not_a_project_alone() {
    let dir = safe_save_dir("foreign_bak");
    let song = dir.join("song.ondina");
    let bak = dir.join("song.ondina.bak");
    let mut s = Session::new(false);
    s.settings.keep_save_backup = true;
    ok(&mut s, "add_track", json!({"kind": "instrument"}));
    ok(&mut s, "save_project", json!({"path": song.display().to_string()}));
    std::fs::write(&bak, b"my notes").unwrap();
    ok(&mut s, "add_track", json!({"kind": "instrument"}));
    let r = ok(&mut s, "save_project", json!({"path": song.display().to_string()}));
    assert!(r.get("backup").is_none(), "{r}");
    assert!(r["backup_error"].as_str().unwrap().contains("song.ondina.bak"), "{r}");
    assert_eq!(std::fs::read(&bak).unwrap(), b"my notes");
    let mut reopened = Session::new(false);
    ok(&mut reopened, "open_project", json!({"path": song.display().to_string()}));
    assert_eq!(reopened.project.tracks.len(), 2);

    std::fs::copy(&song, &bak).unwrap();
    let prev = std::fs::read(&song).unwrap();
    ok(&mut s, "add_track", json!({"kind": "audio"}));
    let r = ok(&mut s, "save_project", json!({"path": song.display().to_string()}));
    assert_eq!(r["backup"], json!(bak.display().to_string()), "{r}");
    assert_eq!(std::fs::read(&bak).unwrap(), prev);
    assert_eq!(listing(&dir), vec!["song.ondina", "song.ondina.bak"]);
}

#[test]
fn save_leaves_files_named_like_its_temporary_files_alone() {
    let dir = safe_save_dir("named_like_tmp");
    let song = dir.join("song.ondina");
    let victim = dir.join("victim.txt");
    std::fs::write(&victim, b"keep me").unwrap();
    std::fs::hard_link(&victim, dir.join("song.ondina.tmp")).unwrap();
    std::fs::write(dir.join("song.ondina.bak.tmp"), b"keep me too").unwrap();
    let mut s = Session::new(false);
    s.settings.keep_save_backup = true;
    ok(&mut s, "save_project", json!({"path": song.display().to_string()}));
    ok(&mut s, "add_track", json!({"kind": "instrument"}));
    ok(&mut s, "save_project", json!({"path": song.display().to_string()}));
    assert_eq!(std::fs::read(&victim).unwrap(), b"keep me");
    assert_eq!(std::fs::read(dir.join("song.ondina.tmp")).unwrap(), b"keep me");
    assert_eq!(std::fs::read(dir.join("song.ondina.bak.tmp")).unwrap(), b"keep me too");
    assert_eq!(listing(&dir), vec!["song.ondina", "song.ondina.bak", "song.ondina.bak.tmp", "song.ondina.tmp", "victim.txt"]);
}

#[test]
fn metronome_level_and_only_while_recording_are_undoable_steps() {
    let default_db = ondina_core::model::METRONOME_DEFAULT_DB;
    let mut s = Session::new(false);
    let t = ok(&mut s, "get_transport", json!({}));
    assert!((t["metronome_level_db"].as_f64().unwrap() + 9.12).abs() < 0.01, "{t}");
    assert_eq!(t["metronome_only_while_recording"], json!(false));

    // the old way of calling it still works and leaves the level alone
    ok(&mut s, "set_metronome", json!({"enabled": true}));
    assert!(s.project.metronome);
    assert_eq!(s.project.metronome_level_db, default_db);

    let r = ok(&mut s, "set_metronome", json!({"enabled": true, "level_db": -20}));
    assert_eq!(r["ok"], json!(true), "{r}");
    assert_eq!(r["level_db"], json!(-20.0), "{r}");
    assert_eq!(s.project.metronome_level_db, -20.0);
    assert!(s.project.metronome);
    assert_eq!(ok(&mut s, "get_transport", json!({}))["metronome_level_db"], json!(-20.0));

    ok(&mut s, "undo", json!({}));
    assert!((s.project.metronome_level_db - default_db).abs() < 1e-4);
    assert!(s.project.metronome);

    ok(&mut s, "set_metronome", json!({"enabled": true, "only_while_recording": true}));
    assert!(s.project.metronome_only_recording);
    assert!((s.project.metronome_level_db - default_db).abs() < 1e-4);
    assert_eq!(ok(&mut s, "get_transport", json!({}))["metronome_only_while_recording"], json!(true));
    ok(&mut s, "undo", json!({}));
    assert!(!s.project.metronome_only_recording);

    ok(&mut s, "set_metronome", json!({"enabled": true, "level_db": -100}));
    assert_eq!(s.project.metronome_level_db, -60.0);
    ok(&mut s, "set_metronome", json!({"enabled": true, "level_db": 6}));
    assert_eq!(s.project.metronome_level_db, 0.0);

    // a level that is not a number changes nothing at all
    let undo_before = s.can_undo().map(String::from);
    let bad = Command::SetMetronome { enabled: false, level_db: Some(f32::NAN), only_while_recording: Some(true) };
    assert!(s.execute(bad, Source::Mcp).is_err());
    assert!(s.project.metronome);
    assert_eq!(s.project.metronome_level_db, 0.0);
    assert!(!s.project.metronome_only_recording);
    assert_eq!(s.can_undo().map(String::from), undo_before);
}

#[test]
fn get_project_reports_the_metronome_level() {
    let mut s = Session::new(false);
    ok(&mut s, "set_metronome", json!({"enabled": false, "level_db": -12.5, "only_while_recording": true}));
    let p = ok(&mut s, "get_project", json!({}));
    assert_eq!(p["metronome_level_db"], json!(-12.5), "{p}");
    assert_eq!(p["metronome_only_while_recording"], json!(true), "{p}");
    assert_eq!(p["metronome"], json!(false), "{p}");
}

#[test]
fn old_projects_keep_the_metronome_level_they_had() {
    let example = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../examples/lofi-beat.aurora");
    let text = std::fs::read_to_string(&example).unwrap();
    assert!(!text.contains("metronome_level_db"), "the example must be a file saved before the level existed");
    let mut s = Session::new(false);
    ok(&mut s, "open_project", json!({"path": example.display().to_string()}));
    assert!((s.project.metronome_gain() - 0.35).abs() < 0.001);
    assert!(!s.project.metronome_only_recording);

    ok(&mut s, "set_metronome", json!({"enabled": true, "level_db": -18, "only_while_recording": true}));
    let saved = tmp("metronome_level.ondina");
    ok(&mut s, "save_project", json!({"path": saved.display().to_string()}));
    let mut again = Session::new(false);
    ok(&mut again, "open_project", json!({"path": saved.display().to_string()}));
    assert_eq!(again.project.metronome_level_db, -18.0);
    assert!(again.project.metronome_only_recording);
}

/// Amplitude of the `freq` component of `x` (single-bin DFT over the whole slice).
fn tone_level(x: &[f32], freq: f64, sr: f64) -> f64 {
    let (mut re, mut im) = (0.0f64, 0.0f64);
    for (n, v) in x.iter().enumerate() {
        let ph = 2.0 * std::f64::consts::PI * freq * n as f64 / sr;
        re += *v as f64 * ph.cos();
        im -= *v as f64 * ph.sin();
    }
    2.0 / x.len() as f64 * (re * re + im * im).sqrt()
}

#[test]
fn de_esser_is_listed_added_and_applied_in_a_bounce() {
    let mut s = Session::new(false);
    let effects = ok(&mut s, "list_plugins", json!({"category": "effect"}));
    let de = effects.as_array().unwrap().iter().find(|p| p["kind"] == "de_esser").expect("de_esser in list_plugins");
    assert_eq!(de["group"], "Dynamics");
    let ids: Vec<&str> = de["params"].as_array().unwrap().iter().map(|p| p["id"].as_str().unwrap()).collect();
    assert_eq!(ids, ["freq", "threshold", "range", "mode", "listen"]);
    assert_eq!(de["params"][3]["choices"], json!(["Split", "Wideband"]));
    let presets = ok(&mut s, "list_presets", json!({"kind": "de_esser"}));
    let names: Vec<&str> = presets.as_array().unwrap().iter().map(|p| p["name"].as_str().unwrap()).collect();
    assert!(names.contains(&"Vocal De-Ess") && names.contains(&"Soft De-Ess"), "{presets}");
    let patches = ok(&mut s, "list_patches", json!({}));
    let lead = patches.as_array().unwrap().iter().find(|p| p["name"] == "Lead Vocal").unwrap();
    assert_eq!(lead["effects"], json!(["channel_eq", "compressor", "reverb"]));

    // Three seconds of a "voice" at 200 Hz plus an "s" at 7 kHz.
    let sr_in = 48000usize;
    let x: Vec<f32> = (0..3 * sr_in)
        .map(|n| {
            let t = n as f64 / sr_in as f64;
            (0.3 * (2.0 * std::f64::consts::PI * 200.0 * t).sin() + 0.3 * (2.0 * std::f64::consts::PI * 7000.0 * t).sin()) as f32
        })
        .collect();
    let wav = tmp("de_esser.wav");
    write_wav(&wav, &[x], sr_in as u32);
    let rid = ok(&mut s, "import_audio", json!({"path": wav.display().to_string(), "start": 0}))["region_id"].as_u64().unwrap();
    let t = s.project.find_region(rid).unwrap().0.id;
    let fx = ok(&mut s, "add_effect", json!({"track_id": t, "kind": "de_esser"}))["plugin_id"].as_u64().unwrap();
    assert_eq!(s.can_undo(), Some("add_effect"));
    let info = ok(&mut s, "get_plugin", json!({"plugin_id": fx}));
    let param = |id: &str| info["params"].as_array().unwrap().iter().find(|p| p["id"] == id).unwrap().clone();
    assert_eq!(param("mode")["selected"], "Split");
    assert_eq!(param("freq")["value"].as_f64(), Some(6000.0));

    let bounce = |s: &mut Session, name: &str| {
        let path = tmp(name);
        let r = ok(s, "bounce", json!({"path": path.display().to_string(), "bit_depth": 32, "tail_sec": 0}));
        let sr = r["sample_rate"].as_u64().unwrap() as usize;
        let reader = hound::WavReader::open(&path).unwrap();
        let channels = reader.spec().channels as usize;
        let left: Vec<f32> = reader.into_samples::<f32>().map(|x| x.unwrap()).step_by(channels).collect();
        let window = &left[sr..2 * sr];
        (tone_level(window, 200.0, sr as f64), tone_level(window, 7000.0, sr as f64))
    };
    let (low_on, high_on) = bounce(&mut s, "de_esser_on.wav");
    ok(&mut s, "set_plugin_bypass", json!({"plugin_id": fx, "bypass": true}));
    let (low_off, high_off) = bounce(&mut s, "de_esser_off.wav");
    let high = 20.0 * (high_on / high_off).log10();
    let low = 20.0 * (low_on / low_off).log10();
    assert!((-10.0..=-6.0).contains(&high), "7 kHz changed by {high} dB");
    assert!(low.abs() < 0.5, "200 Hz changed by {low} dB");
}

fn a_minor_melody_json() -> Value {
    let melody = [(69, 1.0), (72, 1.0), (71, 1.0), (69, 1.0), (76, 1.0), (74, 1.0), (72, 1.0), (71, 1.0), (68, 1.0), (69, 1.0), (71, 1.0), (72, 1.0), (71, 1.0), (68, 1.0), (69, 2.0)];
    let mut start = 0.0;
    let notes: Vec<Value> = melody
        .iter()
        .map(|&(pitch, length)| {
            let n = json!({"pitch": pitch, "velocity": 100, "start": start, "length": length});
            start += length;
            n
        })
        .collect();
    Value::Array(notes)
}

fn drum_notes_json() -> Value {
    let mut notes = vec![];
    for i in 0..16 {
        notes.push(json!({"pitch": 42, "velocity": 100, "start": i, "length": 0.5}));
    }
    for bar in 0..4 {
        let b = bar as f64 * 4.0;
        notes.push(json!({"pitch": 36, "velocity": 100, "start": b, "length": 0.5}));
        notes.push(json!({"pitch": 38, "velocity": 100, "start": b + 2.0, "length": 0.5}));
        notes.push(json!({"pitch": 37, "velocity": 100, "start": b + 3.5, "length": 0.5}));
    }
    notes.push(json!({"pitch": 49, "velocity": 100, "start": 0, "length": 2}));
    notes.push(json!({"pitch": 49, "velocity": 100, "start": 8, "length": 2}));
    Value::Array(notes)
}

#[test]
fn detect_key_sets_the_project_key_in_one_undo_step() {
    let mut s = Session::new(false);
    let t = ok(&mut s, "add_track", json!({"kind": "instrument", "patch": "Lush Pad"}))["track_id"].as_u64().unwrap();
    ok(&mut s, "create_midi_region", json!({"track_id": t, "start": 0, "length": 16, "notes": a_minor_melody_json()}));
    let d = ok(&mut s, "add_track", json!({"kind": "instrument", "patch": "Pop Drum Kit"}))["track_id"].as_u64().unwrap();
    ok(&mut s, "create_midi_region", json!({"track_id": d, "start": 0, "length": 16, "notes": drum_notes_json()}));
    ok(&mut s, "create_pattern_region", json!({"track_id": t, "start": 32}));

    let (undo, rev) = (s.can_undo().map(String::from), s.revision);
    for args in [json!({}), json!({"apply": false})] {
        let r = ok(&mut s, "detect_key", args);
        assert_eq!(r["key"], "A Minor", "{r}");
        assert_eq!(r["applied"], false);
        assert_eq!(s.can_undo().map(String::from), undo);
        assert_eq!(s.revision, rev);
        assert_eq!(s.project.key.name(), "C Major");
    }

    let r = ok(&mut s, "detect_key", json!({"apply": true}));
    assert_eq!(r["applied"], true);
    assert_eq!(s.project.key.name(), "A Minor");
    assert_eq!(s.can_undo(), Some("detect_key"));
    ok(&mut s, "undo", json!({}));
    assert_eq!(s.project.key.name(), "C Major");
    assert!(tool_specs().iter().any(|t| t.name == "detect_key" && t.read_only));
}

#[test]
fn detect_key_explains_what_it_cannot_read() {
    let mut s = Session::new(false);
    let d = ok(&mut s, "add_track", json!({"kind": "instrument", "patch": "Pop Drum Kit"}))["track_id"].as_u64().unwrap();
    let drum_region = ok(&mut s, "create_midi_region", json!({"track_id": d, "start": 0, "length": 16, "notes": drum_notes_json()}))["region_id"].as_u64().unwrap();
    let pattern = ok(&mut s, "create_pattern_region", json!({"track_id": d, "start": 16}))["region_id"].as_u64().unwrap();
    let err = call(&mut s, "detect_key", json!({})).unwrap_err();
    assert!(err.contains("no MIDI notes"), "{err}");
    let err = call(&mut s, "detect_key", json!({"region_id": drum_region})).unwrap_err();
    assert!(err.contains("Drum Kit"), "{err}");
    let undo = s.can_undo().map(String::from);
    let err = call(&mut s, "detect_key", json!({"region_id": pattern, "apply": true})).unwrap_err();
    assert!(err.contains("step pattern"), "{err}");
    assert_eq!(s.can_undo().map(String::from), undo);
    assert_eq!(s.project.key.name(), "C Major");
}

/// A Pop Drum Kit with a 16-step pattern: Kick, Closed Hat and Open Hat have different steps.
fn pattern_row_session() -> (Session, u64) {
    let mut s = Session::new(false);
    let t = ok(&mut s, "add_track", json!({"kind": "instrument", "patch": "Pop Drum Kit"}))["track_id"].as_u64().unwrap();
    let rid = ok(&mut s, "create_pattern_region", json!({"track_id": t, "start": 0, "steps": 16}))["region_id"].as_u64().unwrap();
    for (row, pattern) in [(0, "x...x...x...x..."), (3, "o.o.o.o.o.o.o.o."), (4, "..x...x...x...x.")] {
        ok(&mut s, "set_pattern_row", json!({"region_id": rid, "row": row, "pattern": pattern}));
    }
    (s, rid)
}

#[test]
fn update_pattern_row_mutes_renames_and_changes_the_sound_in_one_undo_step_each() {
    let (mut s, rid) = pattern_row_session();
    let before = region_object(&mut s, rid);
    let undo_before = s.can_undo().map(String::from);

    ok(&mut s, "update_pattern_row", json!({"region_id": rid, "row": 3, "muted": true}));
    assert_eq!(s.can_undo(), Some("update_pattern_row"));
    let r = ok(&mut s, "get_region", json!({"region_id": rid}));
    assert_eq!(r["rows"][3]["muted"], true);
    assert_eq!(r["rows"][3]["pattern"], "o.o.o.o.o.o.o.o.");
    assert_eq!(r["rows"][0]["muted"], false);

    ok(&mut s, "convert_pattern_to_midi", json!({"region_id": rid}));
    let notes = ok(&mut s, "get_region", json!({"region_id": rid}))["notes"].as_array().unwrap().clone();
    assert!(!notes.iter().any(|n| n["pitch"] == 42), "a muted row is not converted");
    assert!(notes.iter().any(|n| n["pitch"] == 36));
    ok(&mut s, "undo", json!({}));

    ok(&mut s, "undo", json!({}));
    assert_eq!(region_object(&mut s, rid), before);
    assert_eq!(s.can_undo().map(String::from), undo_before);

    ok(&mut s, "update_pattern_row", json!({"region_id": rid, "row": 2, "name": " Snare 2 ", "pitch": 40}));
    let row = ok(&mut s, "get_region", json!({"region_id": rid}))["rows"][2].clone();
    assert_eq!(row["name"], "Snare 2");
    assert_eq!(row["pitch"], 40);
    assert_eq!(row["muted"], false);
    assert_eq!(row["pattern"], "................");
    ok(&mut s, "undo", json!({}));
    assert_eq!(region_object(&mut s, rid), before);
}

#[test]
fn remove_pattern_row_moves_the_later_rows_up_in_one_undo_step() {
    let (mut s, rid) = pattern_row_session();
    let before = region_object(&mut s, rid);

    let v = ok(&mut s, "remove_pattern_row", json!({"region_id": rid, "row": 3}));
    assert_eq!(v, json!({"removed": {"name": "Closed Hat", "pitch": 42}, "rows": 7}));
    let r = ok(&mut s, "get_region", json!({"region_id": rid}));
    assert_eq!(r["rows"].as_array().unwrap().len(), 7);
    assert_eq!(r["rows"][3]["name"], "Open Hat");
    assert_eq!(r["rows"][3]["pattern"], "..x...x...x...x.");
    assert_eq!(r["rows"][0]["pattern"], "x...x...x...x...");
    assert_eq!(region_object(&mut s, rid)["length"], before["length"]);
    assert_eq!(s.can_undo(), Some("remove_pattern_row"));

    ok(&mut s, "undo", json!({}));
    assert_eq!(region_object(&mut s, rid), before);

    for _ in 0..8 {
        ok(&mut s, "remove_pattern_row", json!({"region_id": rid, "row": 0}));
    }
    let r = ok(&mut s, "get_region", json!({"region_id": rid}));
    assert_eq!(r["rows"], json!([]));
    assert_eq!(r["type"], "pattern");
    assert_eq!(ok(&mut s, "add_pattern_row", json!({"region_id": rid, "name": "Kick", "pitch": 36})), json!({"row": 0}));
}

#[test]
fn pattern_row_edits_explain_bad_arguments_and_change_nothing() {
    let (mut s, rid) = pattern_row_session();
    let t = ok(&mut s, "get_region", json!({"region_id": rid}))["track_id"].clone();
    let midi = ok(&mut s, "create_midi_region", json!({"track_id": t, "start": 8, "length": 4}))["region_id"].as_u64().unwrap();
    let before = ok(&mut s, "get_project_json", json!({}));
    let undo_before = s.can_undo().map(String::from);

    let err = call(&mut s, "update_pattern_row", json!({"region_id": rid, "row": 8, "muted": true})).unwrap_err();
    assert!(err.contains("row 8") && err.contains("8 rows"), "{err}");
    let err = call(&mut s, "remove_pattern_row", json!({"region_id": rid, "row": 8})).unwrap_err();
    assert!(err.contains("row 8") && err.contains("8 rows"), "{err}");
    let err = call(&mut s, "update_pattern_row", json!({"region_id": rid, "row": 0, "pitch": 128})).unwrap_err();
    assert!(err.contains("pitch"), "{err}");
    let err = call(&mut s, "update_pattern_row", json!({"region_id": rid, "row": 0, "name": ""})).unwrap_err();
    assert!(err.contains("name"), "{err}");
    let err = call(&mut s, "update_pattern_row", json!({"region_id": rid, "row": 0})).unwrap_err();
    assert!(err.contains("nothing to change"), "{err}");
    let err = call(&mut s, "update_pattern_row", json!({"region_id": midi, "row": 0, "muted": true})).unwrap_err();
    assert!(err.contains("pattern"), "{err}");

    assert_eq!(ok(&mut s, "get_project_json", json!({})), before);
    assert_eq!(s.can_undo().map(String::from), undo_before);
    for name in ["update_pattern_row", "remove_pattern_row"] {
        assert!(tool_specs().iter().any(|t| t.name == name && !t.read_only), "{name}");
    }
}

/// The stereo mix of `song_wav` (melody, bass, clicks; 8 s at 44.1 kHz) in its own file. Measured: about -9.9 LUFS
/// integrated with peaks at +2.4 dBFS.
fn mix_wav(name: &str) -> PathBuf {
    let sr = 44100u32;
    let n = sr as usize * 8;
    let beat = 0.6;
    let melody = [261.63f32, 329.63, 392.0, 523.25];
    let mut l = vec![0.0f32; n];
    for (i, x) in l.iter_mut().enumerate() {
        let t = i as f32 / sr as f32;
        let k = (t / beat) as usize;
        let ph = t - k as f32 * beat;
        let env = if ph < beat * 0.85 { 1.0 } else { 0.0 };
        let mel = (2.0 * std::f32::consts::PI * melody[k % 4] * t).sin() * 0.3 * env;
        let bass = (2.0 * std::f32::consts::PI * 82.0 * t).sin() * 0.25;
        let click = if ph < 0.004 { (1.0 - ph / 0.004) * if (i % 2) == 0 { 0.8 } else { -0.8 } } else { 0.0 };
        *x = mel + bass + click;
    }
    let p = tmp(name);
    write_wav(&p, &[l.clone(), l], sr);
    p
}

/// A session with that mix on one audio track at 100 bpm. Returns the session and the track id.
fn loudness_session(name: &str) -> (Session, u64) {
    let wav = mix_wav(name);
    let mut s = Session::new(false);
    ok(&mut s, "set_tempo", json!({"bpm": 100}));
    let imp = ok(&mut s, "import_audio", json!({"path": wav.display().to_string(), "start": 0}));
    (s, imp["track_id"].as_u64().unwrap())
}

/// The samples of a stereo WAV file as left and right channels (float or integer samples).
fn read_stereo(path: &Path) -> (Vec<f32>, Vec<f32>, u32) {
    let reader = hound::WavReader::open(path).unwrap();
    let spec = reader.spec();
    let samples: Vec<f32> = match spec.sample_format {
        hound::SampleFormat::Float => reader.into_samples::<f32>().map(|x| x.unwrap()).collect(),
        hound::SampleFormat::Int => {
            let max = ((1i64 << (spec.bits_per_sample - 1)) - 1) as f32;
            reader.into_samples::<i32>().map(|x| x.unwrap() as f32 / max).collect()
        }
    };
    let left: Vec<f32> = samples.iter().step_by(2).copied().collect();
    let right: Vec<f32> = samples.iter().skip(1).step_by(2).copied().collect();
    (left, right, spec.sample_rate)
}

/// Loudness and peak of a WAV file, measured like analyze_mix.
fn measure_wav(path: &Path) -> ondina_engine::AnalysisReport {
    let (left, right, sr) = read_stereo(path);
    let mut a = ondina_engine::Analyzer::new(sr as f32);
    a.push(&left, &right);
    a.report()
}

/// Largest absolute sample of a WAV file (linear).
fn file_peak(path: &Path) -> f32 {
    let (left, right, _) = read_stereo(path);
    left.iter().chain(&right).fold(0.0f32, |a, b| a.max(b.abs()))
}

fn num(v: &Value) -> f64 {
    v.as_f64().unwrap_or_else(|| panic!("not a number: {v}"))
}

#[test]
fn bounce_to_a_loudness_target_measures_on_target() {
    let (mut s, _) = loudness_session("loud_target_src.wav");
    let out = tmp("loud_target.wav");
    for bits in [32, 24] {
        let r = ok(&mut s, "bounce", json!({"path": out.display().to_string(), "bit_depth": bits, "tail_sec": 0, "normalize_lufs": -14}));
        let l = &r["loudness"];
        assert_eq!(l["target_reached"], json!(true), "{r}");
        let m = measure_wav(&out);
        assert!((m.integrated_lufs + 14.0).abs() <= 0.5, "{bits} bit: {} LUFS {r}", m.integrated_lufs);
        assert!(m.true_peak_dbtp <= -1.0 + 0.1, "{bits} bit: true peak {} {r}", m.true_peak_dbtp);
        assert!((num(&l["lufs_after"]) - m.integrated_lufs as f64).abs() <= 0.2, "{bits} bit: {} {r}", m.integrated_lufs);
        // master_analysis stays the measure of the mix before the gain.
        assert!((num(&r["master_analysis"]["integrated_lufs"]) + 9.9).abs() <= 1.0, "{r}");
        assert_eq!(r.get("warnings"), None, "{r}");
    }
}

#[test]
fn bounce_loudness_target_stops_at_the_peak_ceiling() {
    let (mut s, track) = loudness_session("loud_ceiling_src.wav");
    let out = tmp("loud_ceiling.wav");
    let p = out.display().to_string();
    let r = ok(&mut s, "bounce", json!({"path": p, "bit_depth": 32, "tail_sec": 0, "normalize_lufs": -8, "true_peak_ceiling_db": -3}));
    assert_eq!(r["loudness"]["target_reached"], json!(false), "{r}");
    assert!(num(&r["loudness"]["gain_db"]) < 0.0, "{r}");
    let tp = measure_wav(&out).true_peak_dbtp;
    assert!(tp <= -3.0 + 0.1, "true peak {tp} {r}");
    // A quiet, dynamic mix: the boost stops at the default ceiling of -1 dBTP.
    ok(&mut s, "set_volume", json!({"track_id": track, "db": -20}));
    let r = ok(&mut s, "bounce", json!({"path": p, "bit_depth": 32, "tail_sec": 0, "normalize_lufs": -6}));
    assert_eq!(r["loudness"]["target_reached"], json!(false), "{r}");
    assert!(num(&r["loudness"]["gain_db"]) > 10.0, "{r}");
    assert_eq!(num(&r["loudness"]["true_peak_ceiling_db"]), -1.0, "{r}");
    let tp = measure_wav(&out).true_peak_dbtp;
    assert!(tp <= -1.0 + 0.1, "true peak {tp} {r}");
}

#[test]
fn bounce_without_a_loudness_target_is_unchanged() {
    let (mut s, _) = loudness_session("loud_plain_src.wav");
    let out = tmp("loud_plain.wav");
    let p = out.display().to_string();
    let plain = ok(&mut s, "bounce", json!({"path": p, "bit_depth": 32, "tail_sec": 0}));
    assert_eq!(plain.get("loudness"), None, "{plain}");
    assert_eq!(plain.get("warnings"), None, "{plain}");
    let plain_peak = file_peak(&out);
    let peak_db = 20.0 * (plain_peak as f64).log10();
    assert!((peak_db - num(&plain["master_analysis"]["peak_dbfs"])).abs() <= 0.1, "{peak_db} {plain}");
    let target = ok(&mut s, "bounce", json!({"path": p, "bit_depth": 32, "tail_sec": 0, "normalize_lufs": -14}));
    assert_eq!(plain["duration_sec"], target["duration_sec"], "{plain} {target}");
    // normalize=true still scales the peak to -1 dBFS.
    let r = ok(&mut s, "bounce", json!({"path": p, "bit_depth": 32, "tail_sec": 0, "normalize": true}));
    assert!((file_peak(&out) - 0.891).abs() < 1e-3, "{}", file_peak(&out));
    assert_eq!(r.get("loudness"), None, "{r}");
    // With both, normalize_lufs sets the level and says normalize was ignored.
    let r = ok(&mut s, "bounce", json!({"path": p, "bit_depth": 32, "tail_sec": 0, "normalize": true, "normalize_lufs": -14}));
    assert!(r.get("loudness").is_some(), "{r}");
    assert!((file_peak(&out) - 0.891).abs() > 0.01, "{}", file_peak(&out));
    let warnings = r["warnings"].as_array().unwrap_or_else(|| panic!("no warnings: {r}"));
    assert!(warnings.iter().any(|w| w.as_str().unwrap().contains("normalize")), "{r}");
    // A ceiling alone changes nothing, and says so.
    let r = ok(&mut s, "bounce", json!({"path": p, "bit_depth": 32, "tail_sec": 0, "true_peak_ceiling_db": -2}));
    let warnings = r["warnings"].as_array().unwrap_or_else(|| panic!("no warnings: {r}"));
    assert!(warnings.iter().any(|w| w.as_str().unwrap().contains("true_peak_ceiling_db")), "{r}");
    assert_eq!(r.get("loudness"), None, "{r}");
    assert_eq!(file_peak(&out), plain_peak);
}

#[test]
fn bounce_loudness_values_out_of_range_are_refused() {
    let (mut s, _) = loudness_session("loud_refused_src.wav");
    let out = tmp("loud_refused.wav");
    for (field, value) in [("normalize_lufs", -31.0), ("normalize_lufs", -5.0), ("true_peak_ceiling_db", 0.5), ("true_peak_ceiling_db", -10.0)] {
        let _ = std::fs::remove_file(&out);
        let mut args = json!({"path": out.display().to_string(), "tail_sec": 0});
        args[field] = json!(value);
        let e = call(&mut s, "bounce", args).unwrap_err();
        assert!(e.contains(field), "{field} {value}: {e}");
        assert!(!out.exists(), "{field} {value}: the file was written");
    }
    let spec = tool_specs().into_iter().find(|t| t.name == "bounce").unwrap();
    let req = spec.input_schema.get("required").and_then(|r| r.as_array()).cloned().unwrap_or_default();
    for field in ["normalize_lufs", "true_peak_ceiling_db"] {
        assert!(spec.input_schema["properties"].get(field).is_some(), "{field}");
        assert!(!req.iter().any(|v| v == field), "{field}");
    }
}

#[test]
fn session_players_return_a_seed_that_repeats_the_part() {
    use ondina_core::model::RegionContent;
    let mut s = Session::new(false);
    ok(&mut s, "set_chord_progression", json!({"start": 0, "chords": ["C", "Am", "F", "G"]}));
    let t1 = ok(&mut s, "add_track", json!({"kind": "instrument"}))["track_id"].as_u64().unwrap();
    let t2 = ok(&mut s, "add_track", json!({"kind": "instrument"}))["track_id"].as_u64().unwrap();
    let notes = |s: &Session, rid: u64| {
        let (_, region) = s.project.find_region(rid).unwrap();
        let RegionContent::Midi(m) = &region.content else { panic!("region {rid} is not MIDI") };
        m.notes.clone()
    };

    let drums = ok(&mut s, "generate_drums", json!({"track_id": t1, "start": 4, "bars": 2, "style": "funk"}));
    let seed = drums["seed"].as_u64().expect("generate_drums returns its seed");
    assert!(seed < 1_000_000, "drawn seed {seed}");
    assert_eq!(drums["start"], json!(4.0));
    assert_eq!(drums["bars"], json!(2));
    let again = ok(&mut s, "generate_drums", json!({"track_id": t2, "start": 4, "bars": 2, "style": "funk", "seed": seed}));
    let (a, b) = (notes(&s, drums["region_id"].as_u64().unwrap()), notes(&s, again["region_id"].as_u64().unwrap()));
    assert!(!a.is_empty());
    assert_eq!(a, b, "the returned drum seed repeats the groove");

    let mel = ok(&mut s, "generate_melody", json!({"track_id": t1, "start": 0, "bars": 4, "density": 0.6}));
    let seed = mel["seed"].as_u64().expect("generate_melody returns its seed");
    assert!(seed < 1_000_000, "drawn seed {seed}");
    let again = ok(&mut s, "generate_melody", json!({"track_id": t2, "start": 0, "bars": 4, "density": 0.6, "seed": seed}));
    let second = again["region_id"].as_u64().unwrap();
    let (a, b) = (notes(&s, mel["region_id"].as_u64().unwrap()), notes(&s, second));
    assert!(!a.is_empty());
    assert_eq!(a, b, "the returned melody seed repeats the line");

    ok(&mut s, "undo", json!({}));
    assert!(s.project.find_region(second).is_none(), "one undo removes the last generated part");
}

#[test]
fn get_plugin_reports_description_defaults_and_track() {
    let mut s = Session::new(false);
    let vocals = ok(&mut s, "add_track", json!({"kind": "audio", "name": "Vocals"}))["track_id"].as_u64().unwrap();
    ok(&mut s, "add_effect", json!({"track_id": vocals, "kind": "compressor"}));
    let eq = ok(&mut s, "add_effect", json!({"track_id": vocals, "kind": "channel_eq"}))["plugin_id"].as_u64().unwrap();
    let lead = ok(&mut s, "add_track", json!({"kind": "instrument", "name": "Lead", "instrument": "analog_synth"}))["track_id"].as_u64().unwrap();
    let synth = ok(&mut s, "get_track", json!({"track_id": lead}))["instrument"]["id"].as_u64().unwrap();
    let arp = ok(&mut s, "add_midi_effect", json!({"track_id": lead, "kind": "arpeggiator"}))["plugin_id"].as_u64().unwrap();
    let lim = ok(&mut s, "add_effect", json!({"track_id": 0, "kind": "limiter"}))["plugin_id"].as_u64().unwrap();

    let v = ok(&mut s, "get_plugin", json!({"plugin_id": eq}));
    let info = ondina_core::plugins::find("channel_eq").unwrap();
    assert_eq!(v["id"], json!(eq));
    assert_eq!(v["kind"], json!("channel_eq"));
    assert_eq!(v["name"], json!("Channel EQ"));
    assert_eq!(v["description"], json!(info.description));
    let params = v["params"].as_array().unwrap();
    assert_eq!(params.len(), info.params.len());
    for (o, pi) in params.iter().zip(info.params) {
        assert_eq!(o["id"], json!(pi.id));
        assert_eq!(o["default"], json!(pi.default), "{o}");
        assert_eq!(o["value"], json!(pi.default), "{o}");
        assert_eq!(o["min"], json!(pi.min));
        assert_eq!(o["max"], json!(pi.max));
        assert_eq!(o["unit"], json!(pi.unit));
    }
    let hp_on = params.iter().find(|o| o["id"] == "hp_on").unwrap();
    assert!(hp_on["choices"].is_array());
    assert_eq!(hp_on["selected"], json!("Off"));
    assert_eq!(v["track"], json!({"id": vocals, "name": "Vocals", "kind": "audio", "slot": "insert", "index": 1}));

    let v = ok(&mut s, "get_plugin", json!({"plugin_id": synth}));
    assert_eq!(v["description"], json!(ondina_core::plugins::find("analog_synth").unwrap().description));
    assert_eq!(v["track"]["id"], json!(lead));
    assert_eq!(v["track"]["name"], json!("Lead"));
    assert_eq!(v["track"]["slot"], json!("instrument"));
    assert_eq!(v["track"]["kind"], json!("instrument"));
    assert!(v["track"].get("index").is_none(), "{v}");
    let cutoff = v["params"].as_array().unwrap().iter().find(|o| o["id"] == "cutoff").unwrap();
    assert_eq!(cutoff["default"], json!(2500.0));

    let v = ok(&mut s, "get_plugin", json!({"plugin_id": arp}));
    assert_eq!(v["track"]["slot"], json!("midi_fx"));
    assert_eq!(v["track"]["index"], json!(0));

    let v = ok(&mut s, "get_plugin", json!({"plugin_id": lim}));
    assert_eq!(v["track"], json!({"id": 0, "name": "Master", "kind": "master", "slot": "insert", "index": 0}));

    // get_track keeps the short form of every plug-in
    let t = ok(&mut s, "get_track", json!({"track_id": vocals}));
    let inserts = t["channel"]["inserts"].as_array().unwrap();
    assert_eq!(inserts.len(), 2);
    for p in inserts {
        assert!(p.get("description").is_none(), "{p}");
        assert!(p.get("track").is_none(), "{p}");
    }
}

#[test]
fn plugin_bar_edits_undo_in_one_step() {
    let mut s = Session::new(false);
    let vocals = ok(&mut s, "add_track", json!({"kind": "audio", "name": "Vocals"}))["track_id"].as_u64().unwrap();
    let eq = ok(&mut s, "add_effect", json!({"track_id": vocals, "kind": "channel_eq"}))["plugin_id"].as_u64().unwrap();
    let changed = [("hp_on", 1.0), ("hp_freq", 120.0), ("low_gain", -4.0), ("low_freq", 250.0), ("mid1_gain", -3.0)];

    s.begin_undo_group("assistant: Clean up the muddy frequencies");
    run(&mut s, "get_plugin", json!({"plugin_id": eq}), Source::Ai);
    run(&mut s, "set_plugin_params", json!({"plugin_id": eq, "params": {"hp_on": 1, "hp_freq": 120, "low_gain": -4, "low_freq": 250}}), Source::Ai);
    run(&mut s, "set_plugin_param", json!({"plugin_id": eq, "param": "mid1_gain", "value": -3}), Source::Ai);
    s.end_undo_group();

    // the plug-in panel reads its sliders from the project, so it shows the change at once
    let p = s.project.find_plugin(eq).unwrap();
    for (id, v) in changed {
        assert_eq!(p.param(id), v, "{id}");
    }
    assert_eq!(s.can_undo(), Some("assistant: Clean up the muddy frequencies"));
    assert!(s.last_undo_is_ai());

    run(&mut s, "undo", json!({}), Source::Gui);
    let v = ok(&mut s, "get_plugin", json!({"plugin_id": eq}));
    for o in v["params"].as_array().unwrap() {
        assert_eq!(o["value"], o["default"], "{o}");
    }
    assert_eq!(s.can_undo().map(|l| l.starts_with("assistant")), Some(false));

    run(&mut s, "redo", json!({}), Source::Gui);
    let p = s.project.find_plugin(eq).unwrap();
    for (id, v) in changed {
        assert_eq!(p.param(id), v, "{id}");
    }
}

#[test]
fn dynamic_eq_quick_actions_are_covered_by_the_local_tools() {
    use ondina_studio::local_ai::LOCAL_TOOLS;
    use ondina_studio::prompts::DYNAMIC_EQ_ACTIONS;
    let labels: Vec<&str> = DYNAMIC_EQ_ACTIONS.iter().map(|a| a.0).collect();
    assert_eq!(labels, ["Clean up the muddy frequencies", "Tame harsh resonances", "More air on the vocal"]);
    let all: Vec<String> = tool_specs().into_iter().map(|t| t.name).collect();
    for (label, text) in DYNAMIC_EQ_ACTIONS {
        for word in text.split(|c: char| !(c.is_ascii_alphanumeric() || c == '_')) {
            if all.iter().any(|t| t == word) {
                assert!(LOCAL_TOOLS.contains(&word), "{word}, named by {label:?}, is not offered to the local model");
            }
        }
        for tool in ["get_plugin", "set_plugin_params", "analyze_mix"] {
            assert!(LOCAL_TOOLS.contains(&tool), "{tool} is needed by {label:?}");
        }
        assert!(text.contains("explain in a few lines which bands you changed and why"), "{label:?} must ask for an explanation");
        assert!(text.contains("by ear"), "{label:?} must say the frequency is chosen by ear");
    }
}

#[test]
fn dynamic_eq_is_listed_added_and_applied_in_a_bounce() {
    let mut s = Session::new(false);
    let effects = ok(&mut s, "list_plugins", json!({"category": "effect"}));
    let deq = effects.as_array().unwrap().iter().find(|p| p["kind"] == "dynamic_eq").expect("dynamic_eq in list_plugins");
    assert_eq!(deq["group"], "EQ");
    assert_eq!(deq["name"], "Dynamic EQ");
    let desc = deq["description"].as_str().unwrap();
    assert!(desc.contains("Threshold") && desc.contains("Range"), "{desc}");
    let params = deq["params"].as_array().unwrap();
    assert_eq!(params.len(), 31);
    for p in params {
        for field in ["id", "name", "min", "max", "default", "unit", "choices"] {
            assert!(p.get(field).is_some(), "{} has no {field}", p["id"]);
        }
    }
    let param = |id: &str| params.iter().find(|p| p["id"] == id).unwrap_or_else(|| panic!("no {id}")).clone();
    assert_eq!(param("b2_type")["choices"], json!(["Bell", "Low Shelf", "High Shelf"]));
    assert_eq!(param("b4_freq")["unit"], "Hz");
    assert_eq!(param("b1_range")["min"].as_f64(), Some(-24.0));
    assert_eq!(param("attack")["unit"], "ms");
    let presets = ok(&mut s, "list_presets", json!({"kind": "dynamic_eq"}));
    let names: Vec<&str> = presets.as_array().unwrap().iter().map(|p| p["name"].as_str().unwrap()).collect();
    for n in ["Tame Resonance", "Vocal Mud Control", "Bass Boom Control"] {
        assert!(names.contains(&n), "{presets}");
    }

    // Three seconds of a loud 300 Hz tone (-6 dBFS) and a quiet 3 kHz one.
    let sr_in = 48000usize;
    let x: Vec<f32> = (0..3 * sr_in)
        .map(|n| {
            let t = n as f64 / sr_in as f64;
            (0.5 * (2.0 * std::f64::consts::PI * 300.0 * t).sin() + 0.05 * (2.0 * std::f64::consts::PI * 3000.0 * t).sin()) as f32
        })
        .collect();
    let wav = tmp("dynamic_eq.wav");
    write_wav(&wav, &[x], sr_in as u32);
    let rid = ok(&mut s, "import_audio", json!({"path": wav.display().to_string(), "start": 0}))["region_id"].as_u64().unwrap();
    let t = s.project.find_region(rid).unwrap().0.id;
    let fx = ok(&mut s, "add_effect", json!({"track_id": t, "kind": "dynamic_eq"}))["plugin_id"].as_u64().unwrap();
    assert_eq!(s.can_undo(), Some("add_effect"));
    let set = ok(&mut s, "set_plugin_params", json!({"plugin_id": fx, "params": {"b2_threshold": -20, "b2_range": -12, "b3_range": 99}}));
    assert_eq!(set["b3_range"].as_f64(), Some(24.0), "values are clamped to the registry range");
    ok(&mut s, "set_plugin_param", json!({"plugin_id": fx, "param": "b3_range", "value": 0}));
    let info = ok(&mut s, "get_plugin", json!({"plugin_id": fx}));
    let got = |id: &str| info["params"].as_array().unwrap().iter().find(|p| p["id"] == id).unwrap().clone();
    assert_eq!(got("b2_range")["value"].as_f64(), Some(-12.0));
    assert_eq!(got("b2_range")["unit"], "dB");
    assert_eq!(got("b2_type")["selected"], "Bell");
    assert_eq!(info["params"].as_array().unwrap().len(), 31);

    let bounce = |s: &mut Session, name: &str| {
        let path = tmp(name);
        let r = ok(s, "bounce", json!({"path": path.display().to_string(), "bit_depth": 32, "tail_sec": 0}));
        let sr = r["sample_rate"].as_u64().unwrap() as usize;
        let reader = hound::WavReader::open(&path).unwrap();
        let channels = reader.spec().channels as usize;
        let left: Vec<f32> = reader.into_samples::<f32>().map(|x| x.unwrap()).step_by(channels).collect();
        let window = &left[sr..2 * sr];
        (tone_level(window, 300.0, sr as f64), tone_level(window, 3000.0, sr as f64))
    };
    let (low_on, high_on) = bounce(&mut s, "dynamic_eq_on.wav");
    ok(&mut s, "set_plugin_bypass", json!({"plugin_id": fx, "bypass": true}));
    let (low_off, high_off) = bounce(&mut s, "dynamic_eq_off.wav");
    let low = 20.0 * (low_on / low_off).log10();
    let high = 20.0 * (high_on / high_off).log10();
    assert!((-13.0..=-6.0).contains(&low), "300 Hz changed by {low} dB");
    assert!(high.abs() < 0.5, "3 kHz changed by {high} dB");
}

#[test]
fn assistant_changes_to_the_dynamic_eq_are_one_undo_step() {
    let mut s = Session::new(false);
    let t = ok(&mut s, "add_track", json!({"kind": "audio"}))["track_id"].as_u64().unwrap();
    let fx = ok(&mut s, "add_effect", json!({"track_id": t, "kind": "dynamic_eq"}))["plugin_id"].as_u64().unwrap();
    let values = |s: &mut Session| -> Vec<(String, f64)> {
        let info = ok(s, "get_plugin", json!({"plugin_id": fx}));
        info["params"].as_array().unwrap().iter().map(|p| (p["id"].as_str().unwrap().to_string(), p["value"].as_f64().unwrap())).collect()
    };
    let before = values(&mut s);
    s.begin_undo_group("assistant: Clean up the muddy frequencies");
    run(&mut s, "set_plugin_params", json!({"plugin_id": fx, "params": {"b2_freq": 320, "b2_q": 1.4, "b2_threshold": -28, "b2_range": -6, "b4_gain": 3}}), Source::Ai);
    run(&mut s, "set_plugin_param", json!({"plugin_id": fx, "param": "release", "value": 200}), Source::Ai);
    s.end_undo_group();
    let after = values(&mut s);
    assert_ne!(after, before);
    assert_eq!(s.can_undo(), Some("assistant: Clean up the muddy frequencies"));
    assert!(s.last_undo_is_ai());
    run(&mut s, "undo", json!({}), Source::Gui);
    assert_eq!(values(&mut s), before, "one undo restores every band");
    run(&mut s, "redo", json!({}), Source::Gui);
    assert_eq!(values(&mut s), after);
}

#[test]
fn dynamic_eq_panel_gets_its_quick_actions_and_full_context() {
    use ondina_studio::prompts;
    assert_eq!(prompts::plugin_quick_actions("dynamic_eq"), prompts::DYNAMIC_EQ_ACTIONS);
    let mut s = Session::new(false);
    let t = ok(&mut s, "add_track", json!({"kind": "audio", "name": "Vocals"}))["track_id"].as_u64().unwrap();
    let fx = ok(&mut s, "add_effect", json!({"track_id": t, "kind": "dynamic_eq"}))["plugin_id"].as_u64().unwrap();
    let ctx = prompts::plugin_context(&s.project, fx).expect("the dynamic EQ has a context");
    assert!(ctx.contains("(kind dynamic_eq)") && ctx.contains("\"Vocals\""), "{ctx}");
    for n in 1..=4 {
        for k in ["on", "type", "freq", "gain", "q", "threshold", "range"] {
            assert!(ctx.contains(&format!("- b{n}_{k} ")), "b{n}_{k} missing from the context");
        }
    }
    for id in ["attack", "release", "output"] {
        assert!(ctx.contains(&format!("- {id} ")), "{id} missing from the context");
    }
    assert!(ctx.contains("1 Low Shelf"), "selector choices are in the context");
    let bar = prompts::plugin_bar_context(&s.project, fx).expect("the dynamic EQ has a bar context");
    assert!(bar.contains("set_plugin_params") && bar.contains(&ctx), "{bar}");
}

#[test]
fn stereo_imager_quick_actions_are_covered_by_the_local_tools() {
    use ondina_studio::local_ai::LOCAL_TOOLS;
    use ondina_studio::prompts::STEREO_IMAGER_ACTIONS;
    let labels: Vec<&str> = STEREO_IMAGER_ACTIONS.iter().map(|a| a.0).collect();
    assert_eq!(labels, ["Mono the low end", "Wider without losing mono", "Check mono compatibility"]);
    let all: Vec<String> = tool_specs().into_iter().map(|t| t.name).collect();
    for (label, text) in STEREO_IMAGER_ACTIONS {
        for word in text.split(|c: char| !(c.is_ascii_alphanumeric() || c == '_')) {
            if all.iter().any(|t| t == word) {
                assert!(LOCAL_TOOLS.contains(&word), "{word}, named by {label:?}, is not offered to the local model");
            }
        }
        for tool in ["analyze_mix", "set_plugin_params"] {
            assert!(text.contains(tool), "{label:?} must name {tool}");
            assert!(LOCAL_TOOLS.contains(&tool), "{tool} is needed by {label:?}");
        }
        assert!(text.contains("stereo correlation"), "{label:?} must read the stereo correlation");
        assert!(text.contains("which parameters you changed"), "{label:?} must ask for an explanation");
    }
    let check = STEREO_IMAGER_ACTIONS[2].1;
    assert!(check.contains("change nothing") && check.contains("why nothing needed changing"), "{check}");
}

#[test]
fn stereo_imager_panel_gets_its_quick_actions_and_full_context() {
    use ondina_studio::prompts;
    assert_eq!(prompts::plugin_quick_actions("stereo_imager"), prompts::STEREO_IMAGER_ACTIONS);
    let mut s = Session::new(false);
    let t = ok(&mut s, "add_track", json!({"kind": "audio", "name": "Pad"}))["track_id"].as_u64().unwrap();
    let fx = ok(&mut s, "add_effect", json!({"track_id": t, "kind": "stereo_imager"}))["plugin_id"].as_u64().unwrap();
    let ctx = prompts::plugin_context(&s.project, fx).expect("the stereo imager has a context");
    for want in [format!("Plug-in {fx}: Stereo Imager (kind stereo_imager)"), "insert at index 0".to_string(), format!("track {t} \"Pad\" (audio track)")] {
        assert!(ctx.contains(&want), "missing {want:?} in:\n{ctx}");
    }
    for pi in ondina_core::plugins::find("stereo_imager").unwrap().params {
        assert!(ctx.contains(&format!("- {} \"{}\" = ", pi.id, pi.name)), "{} missing from the context", pi.id);
    }
    for want in ["40..1000 Hz", "1000..16000 Hz", "0..2 x", "-1..1", "-24..24 dB", "0 Stereo, 1 Mid, 2 Side", "0 Off, 1 On"] {
        assert!(ctx.contains(want), "missing {want:?} in:\n{ctx}");
    }
    let bar = prompts::plugin_bar_context(&s.project, fx).expect("the stereo imager has a bar context");
    assert!(bar.starts_with(&prompts::plugin_bar_rules(fx, "Stereo Imager")), "{bar}");
    assert!(bar.contains(&ctx));
    let m = ok(&mut s, "add_effect", json!({"track_id": 0, "kind": "stereo_imager"}))["plugin_id"].as_u64().unwrap();
    let ctx = prompts::plugin_context(&s.project, m).unwrap();
    assert!(ctx.contains("on the master channel."), "{ctx}");
}

#[test]
fn stereo_imager_on_the_master_lifts_the_correlation_of_an_antiphase_bass() {
    let mut s = Session::new(false);
    let effects = ok(&mut s, "list_plugins", json!({"category": "effect"}));
    let img = effects.as_array().unwrap().iter().find(|p| p["kind"] == "stereo_imager").expect("stereo_imager in list_plugins");
    assert_eq!(img["group"], "Utility");
    assert_eq!(img["name"], "Stereo Imager");
    let desc = img["description"].as_str().unwrap();
    assert!(desc.contains("Mono Bass") && desc.contains("analyze_mix"), "{desc}");
    let ids: Vec<&str> = img["params"].as_array().unwrap().iter().map(|p| p["id"].as_str().unwrap()).collect();
    assert_eq!(ids, ["low_freq", "high_freq", "low_width", "mid_width", "high_width", "mono_bass", "balance", "listen", "output"]);
    let presets = ok(&mut s, "list_presets", json!({"kind": "stereo_imager"}));
    let names: Vec<&str> = presets.as_array().unwrap().iter().map(|p| p["name"].as_str().unwrap()).collect();
    for n in ["Mono Low End", "Wide Synth", "Master Width Subtle"] {
        assert!(names.contains(&n), "{presets}");
    }

    // Four seconds at 44.1 kHz: a 1 kHz tone in the middle and a 60 Hz bass in opposite phase on the two sides.
    let sr = 44100usize;
    let wave = |f: f64, n: usize| 0.3 * (2.0 * std::f64::consts::PI * f * n as f64 / sr as f64).sin();
    let l: Vec<f32> = (0..4 * sr).map(|n| (wave(1000.0, n) + wave(60.0, n)) as f32).collect();
    let r: Vec<f32> = (0..4 * sr).map(|n| (wave(1000.0, n) - wave(60.0, n)) as f32).collect();
    let wav = tmp("stereo_imager_antiphase_bass.wav");
    write_wav(&wav, &[l, r], sr as u32);
    let t = ok(&mut s, "import_audio", json!({"path": wav.display().to_string(), "start": 0}))["track_id"].as_u64().unwrap();
    let measure = |s: &mut Session| -> (f64, f64, Vec<String>) {
        let a = ok(s, "analyze_mix", json!({}));
        let track = a["tracks"].as_array().unwrap().iter().find(|x| x["track_id"] == json!(t)).expect("the track is analyzed");
        let advice = a["advice"].as_array().unwrap().iter().map(|x| x.as_str().unwrap().to_string()).collect();
        (a["master"]["stereo_correlation"].as_f64().unwrap(), track["analysis"]["stereo_correlation"].as_f64().unwrap(), advice)
    };
    let (before, track_before, advice) = measure(&mut s);
    assert!(before < 0.2, "master correlation before: {before}");
    assert!(advice.iter().any(|a| a.contains("Low stereo correlation")), "{advice:?}");

    let fx = ok(&mut s, "add_effect", json!({"track_id": 0, "kind": "stereo_imager", "preset": "Mono Low End"}))["plugin_id"].as_u64().unwrap();
    assert_eq!(s.can_undo(), Some("add_effect"));
    let info = ok(&mut s, "get_plugin", json!({"plugin_id": fx}));
    let len = info.to_string().chars().count();
    assert!(len < 6000, "get_plugin is {len} characters");
    assert_eq!(info["description"], json!(ondina_core::plugins::find("stereo_imager").unwrap().description));
    assert_eq!(info["preset"], "Mono Low End");
    assert_eq!(info["track"]["kind"], "master");
    let param = |id: &str| info["params"].as_array().unwrap().iter().find(|p| p["id"] == id).unwrap_or_else(|| panic!("no {id}")).clone();
    assert_eq!(param("mono_bass")["selected"], "On");
    assert_eq!(param("listen")["choices"], json!(["Stereo", "Mid", "Side"]));
    assert_eq!(param("low_freq")["unit"], "Hz");
    assert_eq!(param("low_freq")["value"].as_f64(), Some(120.0));

    let (after, track_after, advice) = measure(&mut s);
    assert!(after > 0.9, "master correlation {before} -> {after}");
    assert!((track_after - track_before).abs() < 0.05, "the imager is on the master, the track is unchanged: {track_before} -> {track_after}");
    assert!(!advice.iter().any(|a| a.contains("Low stereo correlation")), "{advice:?}");

    let set = ok(&mut s, "set_plugin_params", json!({"plugin_id": fx, "params": {"mono_bass": 0, "high_width": 1.5}}));
    assert_eq!(set["mono_bass"].as_f64(), Some(0.0));
    let (wide, _, _) = measure(&mut s);
    assert!(wide < 0.2, "with Mono Bass off the bass is wide again: {wide}");
}

#[test]
fn assistant_changes_to_the_stereo_imager_are_one_undo_step() {
    let mut s = Session::new(false);
    let t = ok(&mut s, "add_track", json!({"kind": "audio", "name": "Pad"}))["track_id"].as_u64().unwrap();
    let fx = ok(&mut s, "add_effect", json!({"track_id": t, "kind": "stereo_imager"}))["plugin_id"].as_u64().unwrap();
    let values = |s: &mut Session| -> Vec<(String, f64)> {
        let info = ok(s, "get_plugin", json!({"plugin_id": fx}));
        info["params"].as_array().unwrap().iter().map(|p| (p["id"].as_str().unwrap().to_string(), p["value"].as_f64().unwrap())).collect()
    };
    let before = values(&mut s);
    s.begin_undo_group("assistant: Wider without losing mono");
    run(&mut s, "get_plugin", json!({"plugin_id": fx}), Source::Ai);
    run(&mut s, "set_plugin_params", json!({"plugin_id": fx, "params": {"mono_bass": 1, "low_freq": 180, "mid_width": 1.2, "high_width": 1.6}}), Source::Ai);
    s.end_undo_group();
    let after = values(&mut s);
    assert_ne!(after, before);
    let p = s.project.find_plugin(fx).unwrap();
    assert_eq!((p.param("mono_bass"), p.param("high_width")), (1.0, 1.6));
    assert_eq!(s.can_undo(), Some("assistant: Wider without losing mono"));
    assert!(s.last_undo_is_ai());
    run(&mut s, "undo", json!({}), Source::Gui);
    assert_eq!(values(&mut s), before, "one undo restores every parameter");
    run(&mut s, "redo", json!({}), Source::Gui);
    assert_eq!(values(&mut s), after);
}

/// The parameter values of plug-in `fx` as get_plugin reports them (only the parameters, never the whole result).
fn plugin_values(s: &mut Session, fx: u64) -> Vec<(String, f64)> {
    let info = ok(s, "get_plugin", json!({"plugin_id": fx}));
    info["params"].as_array().unwrap().iter().map(|p| (p["id"].as_str().unwrap().to_string(), p["value"].as_f64().unwrap())).collect()
}

fn plugin_value(s: &mut Session, fx: u64, id: &str) -> f64 {
    plugin_values(s, fx).into_iter().find(|(k, _)| k == id).unwrap_or_else(|| panic!("no {id}")).1
}

#[test]
fn plugin_window_default_is_one_undo_step() {
    let mut s = Session::new(false);
    let t = ok(&mut s, "add_track", json!({"kind": "audio", "name": "Vox"}))["track_id"].as_u64().unwrap();
    let fx = ok(&mut s, "add_effect", json!({"track_id": t, "kind": "compressor"}))["plugin_id"].as_u64().unwrap();
    ok(&mut s, "set_plugin_param", json!({"plugin_id": fx, "param": "threshold", "value": -30}));
    ok(&mut s, "set_plugin_param", json!({"plugin_id": fx, "param": "ratio", "value": 8}));
    let defaults = ondina_core::plugins::default_params(s.project.find_plugin(fx).unwrap());
    s.execute(Command::SetPluginParams { plugin_id: fx, params: defaults }, Source::Gui).unwrap();
    let info = ondina_core::plugins::find("compressor").unwrap();
    for pi in info.params {
        assert!((plugin_value(&mut s, fx, pi.id) - pi.default as f64).abs() < 1e-6, "{} is not at its default", pi.id);
    }
    s.execute(Command::Undo {}, Source::Gui).unwrap();
    assert_eq!((plugin_value(&mut s, fx, "threshold"), plugin_value(&mut s, fx, "ratio")), (-30.0, 8.0), "one undo brings both back");
    s.execute(Command::Undo {}, Source::Gui).unwrap();
    assert_eq!(plugin_value(&mut s, fx, "threshold"), -30.0);
    assert!((plugin_value(&mut s, fx, "ratio") - info.params.iter().find(|p| p.id == "ratio").unwrap().default as f64).abs() < 1e-6);
}

#[test]
fn plugin_window_two_drags_are_two_undo_steps() {
    let mut s = Session::new(false);
    let t = ok(&mut s, "add_track", json!({"kind": "audio", "name": "Vox"}))["track_id"].as_u64().unwrap();
    let fx = ok(&mut s, "add_effect", json!({"track_id": t, "kind": "compressor"}))["plugin_id"].as_u64().unwrap();
    // the merge keys have the format of the window's knob_merge_key: "pw{plugin}:{param}#{gesture}"
    let set = |s: &mut Session, v: f32, key: &str| {
        s.execute_coalesced(Command::SetPluginParam { plugin_id: fx, param: "threshold".into(), value: v }, key).unwrap();
    };
    let first = format!("pw{fx}:threshold#1");
    let second = format!("pw{fx}:threshold#2");
    for v in [-20.0, -24.0, -28.0] {
        set(&mut s, v, &first);
    }
    for v in [-10.0, -12.0] {
        set(&mut s, v, &second);
    }
    assert_eq!(plugin_value(&mut s, fx, "threshold"), -12.0);
    s.execute(Command::Undo {}, Source::Gui).unwrap();
    assert_eq!(plugin_value(&mut s, fx, "threshold"), -28.0, "the second drag is its own step");
    s.execute(Command::Undo {}, Source::Gui).unwrap();
    let default = ondina_core::plugins::find("compressor").unwrap().params.iter().find(|p| p.id == "threshold").unwrap().default;
    assert_eq!(plugin_value(&mut s, fx, "threshold"), default as f64, "the first drag is one step");
}

#[test]
fn plugin_window_ai_and_mcp_changes_are_undoable() {
    let mut s = Session::new(false);
    let t = ok(&mut s, "add_track", json!({"kind": "audio", "name": "Vox"}))["track_id"].as_u64().unwrap();
    let fx = ok(&mut s, "add_effect", json!({"track_id": t, "kind": "compressor"}))["plugin_id"].as_u64().unwrap();
    let before = plugin_values(&mut s, fx);
    let params = [("threshold".to_string(), -30.0f32), ("ratio".to_string(), 6.0)].into_iter().collect();
    s.execute(Command::SetPluginParams { plugin_id: fx, params }, Source::Ai).unwrap();
    assert!(s.last_undo_is_ai());
    // the window reads the project the session publishes every frame: these are the values its knobs show
    assert_eq!((plugin_value(&mut s, fx, "threshold"), plugin_value(&mut s, fx, "ratio")), (-30.0, 6.0));
    s.execute(Command::Undo {}, Source::Gui).unwrap();
    assert_eq!(plugin_values(&mut s, fx), before);
    ok(&mut s, "set_plugin_param", json!({"plugin_id": fx, "param": "knee", "value": 12}));
    assert_eq!(plugin_value(&mut s, fx, "knee"), 12.0);
    s.execute(Command::Undo {}, Source::Gui).unwrap();
    assert_eq!(plugin_values(&mut s, fx), before);
}

#[test]
fn plugin_window_ai_changes_to_a_vst3_are_one_undo_step() {
    use ondina_core::model::{PluginInstance, Vst3Param};
    let mut s = Session::new(false);
    let t = ok(&mut s, "add_track", json!({"kind": "audio", "name": "Keys"}))["track_id"].as_u64().unwrap();
    let pid = s.project.alloc_id();
    let vst3_params = vec![
        Vst3Param { id: "p1".into(), name: "Gain".into(), default: 0.5, unit: "dB".into() },
        Vst3Param { id: "p2".into(), name: "Mix".into(), default: 0.25, unit: String::new() },
    ];
    let path = Some(r"C:\NotThere\Test Synth.vst3".to_string());
    let inst = PluginInstance { id: pid, kind: "vst3".into(), params: Default::default(), bypass: false, preset: Some("Test Synth".into()), sample: None, vst3_path: path, vst3_params, vst3_state: None };
    s.project.track_mut(t).unwrap().channel.inserts.push(inst);

    // MCP: name, unit and range of every parameter
    let info = ok(&mut s, "get_plugin", json!({"plugin_id": pid}));
    let p1 = info["params"].as_array().unwrap().iter().find(|p| p["id"] == "p1").unwrap().clone();
    assert_eq!(p1["unit"], "dB", "{p1}");
    assert_eq!((p1["min"].as_f64(), p1["max"].as_f64()), (Some(0.0), Some(1.0)), "{p1}");
    assert_eq!(p1["normalized"], true, "{p1}");

    // what the window's AI bar sends with the request
    let text = ondina_studio::prompts::plugin_bar_context(&s.project, pid).unwrap();
    assert!(text.contains("- p1 \"Gain\" = 0.5 [0..1, shown in dB]"), "{text}");
    assert!(text.contains("- p2 \"Mix\" = 0.25 [0..1]"), "{text}");

    let params = [("p1".to_string(), 0.8f32), ("p2".to_string(), 0.6)].into_iter().collect();
    s.execute(Command::SetPluginParams { plugin_id: pid, params }, Source::Ai).unwrap();
    assert!(s.last_undo_is_ai());
    let near = |a: f64, b: f64| (a - b).abs() < 1e-6;
    let (a, b) = (plugin_value(&mut s, pid, "p1"), plugin_value(&mut s, pid, "p2"));
    assert!(near(a, 0.8) && near(b, 0.6), "{a} {b}");
    s.execute(Command::Undo {}, Source::Gui).unwrap();
    let (a, b) = (plugin_value(&mut s, pid, "p1"), plugin_value(&mut s, pid, "p2"));
    assert!(near(a, 0.5) && near(b, 0.25), "one undo restores both: {a} {b}");
    s.execute(Command::Redo {}, Source::Gui).unwrap();
    let (a, b) = (plugin_value(&mut s, pid, "p1"), plugin_value(&mut s, pid, "p2"));
    assert!(near(a, 0.8) && near(b, 0.6), "{a} {b}");

    ok(&mut s, "set_plugin_param", json!({"plugin_id": pid, "param": "p1", "value": 1.7}));
    assert_eq!(plugin_value(&mut s, pid, "p1"), 1.0);
    s.execute(Command::Undo {}, Source::Gui).unwrap();
    assert!(near(plugin_value(&mut s, pid, "p1"), 0.8));
}

/// The Channel EQ curve the plug-in window draws for `fx`, at `f` Hz.
fn ceq_curve(s: &Session, fx: u64, f: f32) -> f32 {
    use ondina_dsp::effects::{channel_eq_params, channel_eq_response_db};
    let inst = s.project.find_plugin(fx).unwrap();
    channel_eq_response_db(&channel_eq_params(|id| inst.param(id)), f, 48000.0)
}

/// The Dynamic EQ curve the plug-in window draws for `fx`, at `f` Hz, with `extra` dB per band.
fn deq_curve(s: &Session, fx: u64, f: f32, extra: [f32; 4]) -> f32 {
    use ondina_dsp::effects::{dynamic_eq_params, dynamic_eq_response_db};
    let inst = s.project.find_plugin(fx).unwrap();
    dynamic_eq_response_db(&dynamic_eq_params(|id| inst.param(id)), f, 48000.0, &extra)
}

#[test]
fn eq_curve_follows_the_assistant_and_undo() {
    let mut s = Session::new(false);
    let t = ok(&mut s, "add_track", json!({"kind": "audio", "name": "Vox"}))["track_id"].as_u64().unwrap();
    let fx = ok(&mut s, "add_effect", json!({"track_id": t, "kind": "channel_eq"}))["plugin_id"].as_u64().unwrap();
    let before = plugin_values(&mut s, fx);
    assert!(ceq_curve(&s, fx, 300.0).abs() <= 0.01);
    let params = [("mid1_freq".to_string(), 300.0f32), ("mid1_gain".to_string(), -6.0), ("mid1_q".to_string(), 2.0)].into_iter().collect();
    s.execute(Command::SetPluginParams { plugin_id: fx, params }, Source::Ai).unwrap();
    assert!(s.last_undo_is_ai());
    // the window reads the project the session publishes every frame: this is the curve it draws at once
    let at = ceq_curve(&s, fx, 300.0);
    assert!((at + 6.0).abs() <= 0.1, "{at}");
    let far = ceq_curve(&s, fx, 5000.0);
    assert!(far.abs() <= 0.3, "{far}");
    s.execute(Command::Undo {}, Source::Gui).unwrap();
    assert_eq!(plugin_values(&mut s, fx), before);
    assert!(ceq_curve(&s, fx, 300.0).abs() <= 0.01);
}

#[test]
fn eq_point_drag_is_one_undo_step() {
    let mut s = Session::new(false);
    let t = ok(&mut s, "add_track", json!({"kind": "audio", "name": "Vox"}))["track_id"].as_u64().unwrap();
    let fx = ok(&mut s, "add_effect", json!({"track_id": t, "kind": "channel_eq"}))["plugin_id"].as_u64().unwrap();
    // the merge keys have the format of the EQ graph's drag_key ("pw{plugin}:eq{band}#{gesture}") and wheel_key
    // ("pw{plugin}:eq{band}:q"); Mid 1 is band 2
    let drag = |s: &mut Session, freq: f32, gain: f32, key: &str| {
        let params = [("mid1_freq".to_string(), freq), ("mid1_gain".to_string(), gain)].into_iter().collect();
        s.execute_coalesced(Command::SetPluginParams { plugin_id: fx, params }, key).unwrap();
    };
    let first = format!("pw{fx}:eq2#1");
    let second = format!("pw{fx}:eq2#2");
    for (f, g) in [(500.0, 2.0), (700.0, 4.0), (900.0, 6.0)] {
        drag(&mut s, f, g, &first);
    }
    for (f, g) in [(1200.0, -3.0), (1500.0, -6.0)] {
        drag(&mut s, f, g, &second);
    }
    let mid1 = |s: &mut Session| (plugin_value(s, fx, "mid1_freq"), plugin_value(s, fx, "mid1_gain"));
    assert_eq!(mid1(&mut s), (1500.0, -6.0));
    s.execute(Command::Undo {}, Source::Gui).unwrap();
    assert_eq!(mid1(&mut s), (900.0, 6.0), "the second drag is its own step");
    s.execute(Command::Undo {}, Source::Gui).unwrap();
    assert_eq!(mid1(&mut s), (400.0, 0.0), "the first drag is one step");
    let wheel = format!("pw{fx}:eq2:q");
    for q in [1.2, 1.4, 1.7] {
        let params = [("mid1_q".to_string(), q)].into_iter().collect();
        s.execute_coalesced(Command::SetPluginParams { plugin_id: fx, params }, &wheel).unwrap();
    }
    assert!((plugin_value(&mut s, fx, "mid1_q") - 1.7).abs() < 1e-6);
    s.execute(Command::Undo {}, Source::Gui).unwrap();
    assert_eq!(plugin_value(&mut s, fx, "mid1_q"), 1.0, "close wheel turns are one step");
}

#[test]
fn get_plugin_explains_every_eq_band() {
    let mut s = Session::new(false);
    let t = ok(&mut s, "add_track", json!({"kind": "audio", "name": "Vox"}))["track_id"].as_u64().unwrap();
    let ceq = ok(&mut s, "add_effect", json!({"track_id": t, "kind": "channel_eq"}))["plugin_id"].as_u64().unwrap();
    let deq = ok(&mut s, "add_effect", json!({"track_id": t, "kind": "dynamic_eq"}))["plugin_id"].as_u64().unwrap();
    for (fx, kind) in [(ceq, "channel_eq"), (deq, "dynamic_eq")] {
        let info = ok(&mut s, "get_plugin", json!({"plugin_id": fx}));
        let params = info["params"].as_array().unwrap();
        assert!(!params.is_empty());
        for p in params {
            let id = p["id"].as_str().unwrap();
            let name = p["name"].as_str().unwrap();
            let unit = p["unit"].as_str().unwrap_or_default();
            assert!(!name.is_empty(), "{kind}: {id} has no name");
            assert!(p["min"].as_f64().unwrap() < p["max"].as_f64().unwrap(), "{kind}: {id} has no range");
            if id.ends_with("_freq") {
                assert_eq!(unit, "Hz", "{kind}: {id}");
            }
            if id.ends_with("_gain") || id.ends_with("_threshold") || id.ends_with("_range") || id == "output" {
                assert_eq!(unit, "dB", "{kind}: {id}");
            }
            let band_param = id.len() > 2 && id.starts_with('b') && id.as_bytes()[1].is_ascii_digit();
            if kind == "dynamic_eq" && band_param {
                assert!(name.starts_with("Band"), "{id}: {name}");
            }
        }
    }
    let info = ok(&mut s, "get_plugin", json!({"plugin_id": ceq}));
    assert!(info["description"].as_str().unwrap().contains("Mid 1"));
    ok(&mut s, "set_plugin_param", json!({"plugin_id": deq, "param": "b2_freq", "value": 500}));
    ok(&mut s, "set_plugin_param", json!({"plugin_id": deq, "param": "b2_gain", "value": 6}));
    let at = deq_curve(&s, deq, 500.0, [0.0; 4]);
    assert!((at - 6.0).abs() <= 0.1, "{at}");
    ok(&mut s, "set_plugin_param", json!({"plugin_id": deq, "param": "b2_range", "value": -6}));
    let reach = deq_curve(&s, deq, 500.0, [0.0, -6.0, 0.0, 0.0]);
    assert!(reach.abs() <= 0.1, "{reach}");
    let still = deq_curve(&s, deq, 500.0, [0.0; 4]);
    assert!((still - 6.0).abs() <= 0.1, "{still}");
}

/// Polls the engine until `f` holds (at most 5 s): the null device renders in real time.
fn wait_for_meter(s: &Session, id: u64, f: impl Fn(&ondina_engine::PluginMeter) -> bool) -> bool {
    let t0 = std::time::Instant::now();
    while t0.elapsed() < std::time::Duration::from_secs(5) {
        if s.engine.plugin_meter(id).is_some_and(|m| f(&m)) {
            return true;
        }
        std::thread::sleep(std::time::Duration::from_millis(10));
    }
    false
}

#[test]
fn live_plug_in_meters_show_what_a_compressor_does() {
    let mut s = Session::new(false);
    let t = ok(&mut s, "add_track", json!({"kind": "instrument", "name": "Keys", "instrument": "analog_synth"}))["track_id"].as_u64().unwrap();
    let chord: Vec<Value> = [48, 55, 60, 64].iter().map(|p| json!({"pitch": p, "velocity": 127, "start": 0, "length": 16})).collect();
    ok(&mut s, "create_midi_region", json!({"track_id": t, "start": 0, "length": 16, "notes": chord}));
    let comp = ok(&mut s, "add_effect", json!({"track_id": t, "kind": "compressor"}))["plugin_id"].as_u64().unwrap();
    ok(&mut s, "set_plugin_params", json!({"plugin_id": comp, "params": {"threshold": -40, "ratio": 8, "knee": 0, "attack": 1}}));
    let eq = ok(&mut s, "add_effect", json!({"track_id": t, "kind": "channel_eq"}))["plugin_id"].as_u64().unwrap();
    ok(&mut s, "play", json!({"from_beat": 0}));
    assert!(wait_for_meter(&s, comp, |m| m.gain_reduction_db.is_some_and(|g| g < -1.0)), "the compressor never reduced");
    assert!(wait_for_meter(&s, eq, |m| m.input_peak > 0.01), "the Channel EQ never saw the synth");

    // get_plugin: the fields of today, plus the live meters
    let v = ok(&mut s, "get_plugin", json!({"plugin_id": comp}));
    for key in ["id", "kind", "name", "description", "params", "track"] {
        assert!(v.get(key).is_some(), "{key} missing: {v}");
    }
    assert_eq!(v["track"], json!({"id": t, "name": "Keys", "kind": "instrument", "slot": "insert", "index": 0}));
    assert_eq!(v["params"].as_array().unwrap().len(), ondina_core::plugins::find("compressor").unwrap().params.len());
    let m = &v["meters"];
    let (input, output) = (m["input_db"].as_f64().unwrap(), m["output_db"].as_f64().unwrap());
    assert!(input > -60.0, "{m}");
    assert!(output < input, "{m}");
    assert!(m["gain_reduction_db"].as_f64().unwrap() < -1.0, "{m}");
    let v = ok(&mut s, "get_plugin", json!({"plugin_id": eq}));
    assert!(v["meters"]["input_db"].is_number() && v["meters"]["output_db"].is_number(), "{v}");
    assert!(v["meters"].get("gain_reduction_db").is_none(), "{v}");

    // get_meters: the track keys of today, plus every insert under "plugins"
    let meters = ok(&mut s, "get_meters", json!({}));
    let track = &meters[t.to_string()];
    assert_eq!(track["name"], json!("Keys"));
    assert!(track["peak_db_l"].is_number() && track["peak_db_r"].is_number(), "{meters}");
    assert_eq!(meters["0"]["name"], json!("master"));
    let c = &meters["plugins"][comp.to_string()];
    assert_eq!(c["name"], json!("Compressor"));
    assert_eq!(c["track_id"], json!(t));
    assert!(c["gain_reduction_db"].as_f64().unwrap() < 0.0, "{meters}");
    assert!(c["input_db"].is_number() && c["output_db"].is_number(), "{meters}");
    let e = &meters["plugins"][eq.to_string()];
    assert_eq!(e["track_id"], json!(t));
    assert!(e.get("gain_reduction_db").is_none(), "{meters}");

    // the assistant adjusts after reading the meters: one undo step, the meters made none
    s.begin_undo_group("assistant: Gentle glue");
    run(&mut s, "get_plugin", json!({"plugin_id": comp}), Source::Ai);
    run(&mut s, "set_plugin_param", json!({"plugin_id": comp, "param": "threshold", "value": -20}), Source::Ai);
    s.end_undo_group();
    assert_eq!(s.can_undo(), Some("assistant: Gentle glue"));
    assert_eq!(s.project.find_plugin(comp).unwrap().param("threshold"), -20.0);
    run(&mut s, "undo", json!({}), Source::Gui);
    assert_eq!(s.project.find_plugin(comp).unwrap().param("threshold"), -40.0);

    // bypassed, the compressor reduces nothing
    ok(&mut s, "set_plugin_bypass", json!({"plugin_id": comp, "bypass": true}));
    assert!(wait_for_meter(&s, comp, |m| m.gain_reduction_db.is_none()), "a bypassed compressor still reports a reduction");
    ok(&mut s, "stop", json!({}));
}

#[test]
fn get_plugin_of_instruments_and_midi_effects_has_no_meters() {
    let mut s = Session::new(false);
    let t = ok(&mut s, "add_track", json!({"kind": "instrument", "name": "Lead", "instrument": "analog_synth"}))["track_id"].as_u64().unwrap();
    let synth = ok(&mut s, "get_track", json!({"track_id": t}))["instrument"]["id"].as_u64().unwrap();
    let arp = ok(&mut s, "add_midi_effect", json!({"track_id": t, "kind": "arpeggiator"}))["plugin_id"].as_u64().unwrap();
    std::thread::sleep(std::time::Duration::from_millis(300));
    for id in [synth, arp] {
        let v = ok(&mut s, "get_plugin", json!({"plugin_id": id}));
        assert!(v.get("meters").is_none(), "{v}");
    }
    let meters = ok(&mut s, "get_meters", json!({}));
    assert_eq!(meters["plugins"], json!({}));
}

#[test]
fn auto_master_keeps_the_true_peak_under_its_ceiling() {
    let (mut s, _) = loudness_session("tp_master_src.wav");
    let r = ok(&mut s, "auto_master", json!({"target_lufs": -8}));
    assert!(num(&r["true_peak_dbtp"]) <= -0.8, "{r}");
    let lim = s.project.master.inserts.iter().find(|p| p.kind == "limiter").expect("auto_master adds a limiter");
    assert_eq!(lim.param("true_peak"), 1.0, "the mastering limiter runs with True Peak On");
    let m = ok(&mut s, "analyze_mix", json!({}));
    assert!(num(&m["master"]["true_peak_dbtp"]) <= -0.8, "{}", m["master"]);
}

#[test]
fn limiter_true_peak_through_mcp() {
    let (mut s, _) = loudness_session("tp_limiter_src.wav");
    let lim = ok(&mut s, "add_effect", json!({"track_id": 0, "kind": "limiter"}))["plugin_id"].as_u64().unwrap();
    let true_peak = |v: &Value| v["params"].as_array().unwrap().iter().find(|p| p["id"] == "true_peak").cloned().expect("true_peak parameter");
    let v = ok(&mut s, "get_plugin", json!({"plugin_id": lim}));
    assert!(v["description"].as_str().unwrap().contains("True Peak"), "{v}");
    let tp = true_peak(&v);
    assert_eq!(tp["choices"], json!(["Off", "On"]));
    assert_eq!(tp["selected"], "Off");
    assert_eq!(tp["default"], json!(0.0));
    assert!(v.to_string().chars().count() < 6000, "get_plugin of the limiter is too long for the local model");

    let before = ok(&mut s, "analyze_mix", json!({}));
    ok(&mut s, "set_plugin_params", json!({"plugin_id": lim, "params": {"gain": 12, "ceiling": -1, "true_peak": 1}}));
    assert_eq!(true_peak(&ok(&mut s, "get_plugin", json!({"plugin_id": lim})))["selected"], "On");
    let after = ok(&mut s, "analyze_mix", json!({}));
    assert!(num(&after["master"]["true_peak_dbtp"]) <= -0.8, "{}", after["master"]);
    let (lufs_before, lufs_after) = (num(&before["master"]["integrated_lufs"]), num(&after["master"]["integrated_lufs"]));
    assert!(lufs_after > lufs_before + 3.0, "loudness {lufs_before} -> {lufs_after}");

    let preset = ok(&mut s, "add_effect", json!({"track_id": 0, "kind": "limiter", "preset": "Master Limiter"}))["plugin_id"].as_u64().unwrap();
    assert_eq!(true_peak(&ok(&mut s, "get_plugin", json!({"plugin_id": preset})))["selected"], "On");
}

#[test]
fn assistant_changes_to_the_limiter_are_one_undo_step() {
    let mut s = Session::new(false);
    let fx = ok(&mut s, "add_effect", json!({"track_id": 0, "kind": "limiter"}))["plugin_id"].as_u64().unwrap();
    let values = |s: &mut Session| -> Vec<(String, f64)> {
        let info = ok(s, "get_plugin", json!({"plugin_id": fx}));
        info["params"].as_array().unwrap().iter().map(|p| (p["id"].as_str().unwrap().to_string(), p["value"].as_f64().unwrap())).collect()
    };
    let before = values(&mut s);
    s.begin_undo_group("assistant: Bring the master to -14 LUFS");
    run(&mut s, "set_plugin_params", json!({"plugin_id": fx, "params": {"gain": 6, "ceiling": -1.5, "true_peak": 1}}), Source::Ai);
    s.end_undo_group();
    let after = values(&mut s);
    assert_ne!(after, before);
    assert!(after.contains(&("true_peak".to_string(), 1.0)), "{after:?}");
    assert_eq!(s.can_undo(), Some("assistant: Bring the master to -14 LUFS"));
    assert!(s.last_undo_is_ai());
    run(&mut s, "undo", json!({}), Source::Gui);
    assert_eq!(values(&mut s), before, "one undo restores gain, ceiling and True Peak");
    run(&mut s, "redo", json!({}), Source::Gui);
    assert_eq!(values(&mut s), after);
}

#[test]
fn limiter_saved_without_true_peak_reads_off_and_sounds_the_same() {
    let path = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../examples/demo-song.aurora");
    let mut s = Session::new(false);
    ok(&mut s, "open_project", json!({"path": path.display().to_string(), "discard_unsaved": true}));
    let lim = s
        .project
        .master
        .inserts
        .iter()
        .chain(s.project.tracks.iter().flat_map(|t| t.channel.inserts.iter()))
        .find(|p| p.kind == "limiter")
        .expect("the demo song has a limiter");
    assert!(!lim.params.contains_key("true_peak"), "the demo song was saved before True Peak");
    let id = lim.id;
    let v = ok(&mut s, "get_plugin", json!({"plugin_id": id}));
    let tp = v["params"].as_array().unwrap().iter().find(|p| p["id"] == "true_peak").cloned().unwrap();
    assert_eq!(tp["selected"], "Off");

    let bounce = |s: &mut Session, name: &str| {
        let p = tmp(name);
        ok(s, "bounce", json!({"path": p.display().to_string(), "start": 0, "end": 8, "bit_depth": 32, "tail_sec": 0}));
        read_stereo(&p)
    };
    let (la, ra, _) = bounce(&mut s, "tp_old_a.wav");
    ok(&mut s, "set_plugin_param", json!({"plugin_id": id, "param": "true_peak", "value": 0}));
    let (lb, rb, _) = bounce(&mut s, "tp_old_b.wav");
    assert!(la.iter().any(|x| *x != 0.0), "the bounce is silent");
    assert_eq!(la, lb, "left channel changed");
    assert_eq!(ra, rb, "right channel changed");
}

/// An instrument track called `name` playing `instrument`, and the instrument's plug-in id.
fn synth_track(s: &mut Session, name: &str, instrument: &str) -> (u64, u64) {
    let t = ok(s, "add_track", json!({"kind": "instrument", "name": name, "instrument": instrument}))["track_id"].as_u64().unwrap();
    let fx = s.project.track(t).unwrap().instrument.as_ref().unwrap().id;
    (t, fx)
}

/// The Analog Synth's filter curve the plug-in window draws for `fx`, at `f` Hz, from the project's values.
fn synth_curve(s: &Session, fx: u64, f: f32) -> f32 {
    let inst = s.project.find_plugin(fx).unwrap();
    ondina_dsp::synth::analog_filter_db(inst.param("filter_type"), inst.param("cutoff"), inst.param("resonance"), f, 48000.0)
}

/// The level of the amp envelope the window draws for `fx`, `t` seconds into a held note.
fn amp_level(s: &Session, fx: u64, t: f32) -> f32 {
    let inst = s.project.find_plugin(fx).unwrap();
    ondina_dsp::util::adsr_level(inst.param("attack"), inst.param("decay"), inst.param("sustain"), inst.param("release"), f32::INFINITY, t)
}

#[test]
fn synth_view_follows_the_assistant_and_undo() {
    let mut s = Session::new(false);
    let (_, fx) = synth_track(&mut s, "Lead", "analog_synth");
    let before = plugin_values(&mut s, fx);
    let (curve_at_5k, level_at_06) = (synth_curve(&s, fx, 5000.0), amp_level(&s, fx, 0.6));
    s.begin_undo_group("assistant: A warm pad that opens slowly");
    run(&mut s, "set_plugin_params", json!({"plugin_id": fx, "params": {"attack": 1.2, "f_attack": 2.0, "cutoff": 600, "resonance": 0.6, "filter_env": 0.5}}), Source::Ai);
    s.end_undo_group();
    // the window reads the project the session publishes every frame: this is what it draws at once
    let half = amp_level(&s, fx, 0.6);
    assert!((half - 0.5).abs() <= 1e-3, "the linear attack is half way: {half}");
    let peak = synth_curve(&s, fx, 600.0);
    assert!((peak - ondina_dsp::synth::analog_resonance_db(0.6)).abs() <= 0.05, "{peak}");
    let far = synth_curve(&s, fx, 5000.0);
    assert!(far < curve_at_5k - 12.0, "{far} vs {curve_at_5k}");
    assert_eq!(s.can_undo(), Some("assistant: A warm pad that opens slowly"));
    assert!(s.last_undo_is_ai());
    s.execute(Command::Undo {}, Source::Gui).unwrap();
    assert_eq!(plugin_values(&mut s, fx), before, "one undo restores every parameter");
    assert_eq!(synth_curve(&s, fx, 5000.0), curve_at_5k);
    assert_eq!(amp_level(&s, fx, 0.6), level_at_06);
    s.execute(Command::Redo {}, Source::Gui).unwrap();
    assert!((amp_level(&s, fx, 0.6) - half).abs() <= 1e-6);
    assert!((synth_curve(&s, fx, 600.0) - peak).abs() <= 1e-6);
}

#[test]
fn synth_envelope_drag_is_one_undo_step() {
    let mut s = Session::new(false);
    let (_, fx) = synth_track(&mut s, "Lead", "analog_synth");
    // the merge keys have the format of the envelope editor's env_drag_key ("pw{plugin}:amp:D#{gesture}") and the
    // filter wheel's key ("pw{plugin}:filter:res")
    let drag = |s: &mut Session, decay: f32, sustain: f32, key: &str| {
        let params = [("decay".to_string(), decay), ("sustain".to_string(), sustain)].into_iter().collect();
        s.execute_coalesced(Command::SetPluginParams { plugin_id: fx, params }, key).unwrap();
    };
    let first = format!("pw{fx}:amp:D#1");
    let second = format!("pw{fx}:amp:D#2");
    for (d, v) in [(0.3, 0.7), (0.4, 0.6), (0.5, 0.5)] {
        drag(&mut s, d, v, &first);
    }
    for (d, v) in [(1.0, 0.3), (1.5, 0.2)] {
        drag(&mut s, d, v, &second);
    }
    let amp = |s: &mut Session| (plugin_value(s, fx, "decay"), plugin_value(s, fx, "sustain"));
    let close = |a: (f64, f64), b: (f64, f64)| (a.0 - b.0).abs() < 1e-6 && (a.1 - b.1).abs() < 1e-6;
    assert!(close(amp(&mut s), (1.5, 0.2)));
    s.execute(Command::Undo {}, Source::Gui).unwrap();
    assert!(close(amp(&mut s), (0.5, 0.5)), "the second drag is its own step: {:?}", amp(&mut s));
    s.execute(Command::Undo {}, Source::Gui).unwrap();
    assert!(close(amp(&mut s), (0.2, 0.8)), "the first drag is one step: {:?}", amp(&mut s));
    let wheel = format!("pw{fx}:filter:res");
    for r in [0.3, 0.4, 0.5] {
        let params = [("resonance".to_string(), r)].into_iter().collect();
        s.execute_coalesced(Command::SetPluginParams { plugin_id: fx, params }, &wheel).unwrap();
    }
    assert!((plugin_value(&mut s, fx, "resonance") - 0.5).abs() < 1e-6);
    s.execute(Command::Undo {}, Source::Gui).unwrap();
    assert!((plugin_value(&mut s, fx, "resonance") - 0.2).abs() < 1e-6, "close wheel turns are one step");
}

#[test]
fn synth_quick_actions_are_covered_by_the_local_tools() {
    use ondina_studio::local_ai::LOCAL_TOOLS;
    use ondina_studio::prompts::plugin_quick_actions;
    let all: Vec<String> = tool_specs().into_iter().map(|t| t.name).collect();
    for kind in ["analog_synth", "fm_synth"] {
        for (label, text) in plugin_quick_actions(kind).iter().filter(|a| !a.1.is_empty()) {
            for word in text.split(|c: char| !(c.is_ascii_alphanumeric() || c == '_')) {
                if all.iter().any(|t| t == word) {
                    assert!(LOCAL_TOOLS.contains(&word), "{word}, named by {kind} {label:?}, is not offered to the local model");
                }
            }
            assert!(text.contains("set_plugin_params"), "{kind} {label:?} changes the parameters with set_plugin_params");
            if label.contains("bass for this song") {
                assert!(text.contains("get_project"), "{kind} {label:?} reads the song with get_project");
            }
            for tool in ["get_project", "set_plugin_params"] {
                assert!(LOCAL_TOOLS.contains(&tool), "{tool} is needed by {kind} {label:?}");
            }
            assert!(text.contains("explain in a few lines which parameters you changed and why"), "{kind} {label:?} must ask for an explanation");
        }
    }
}

#[test]
fn synth_panel_gets_its_quick_actions_and_full_context() {
    use ondina_studio::prompts;
    let mut s = Session::new(false);
    let (t, fx) = synth_track(&mut s, "Lead", "analog_synth");
    let ctx = prompts::plugin_context(&s.project, fx).expect("the synth has a context");
    assert!(ctx.contains("(kind analog_synth)") && ctx.contains("\"Lead\"") && ctx.contains("the instrument"), "{ctx}");
    let info = ondina_core::plugins::find("analog_synth").unwrap();
    assert_eq!(info.params.len(), 28);
    for p in info.params {
        assert!(ctx.contains(&format!("- {} ", p.id)), "{} missing from the context", p.id);
    }
    for want in ["- cutoff \"Cutoff\" = 2500 [20..20000 Hz]", "- attack \"Amp Attack\" = 0.005 [0.001..5 s]", "1 Square"] {
        assert!(ctx.contains(want), "missing {want:?} in {ctx}");
    }
    let bar = prompts::plugin_bar_context(&s.project, fx).expect("the synth has a bar context");
    assert!(bar.contains("set_plugin_params") && bar.contains(&ctx), "{bar}");
    assert!(ctx.contains(&format!("track {t}")), "{ctx}");

    let (_, fm) = synth_track(&mut s, "Keys", "fm_synth");
    let ctx = prompts::plugin_context(&s.project, fm).unwrap();
    let info = ondina_core::plugins::find("fm_synth").unwrap();
    assert_eq!(info.params.len(), 13);
    for p in info.params {
        assert!(ctx.contains(&format!("- {} ", p.id)), "{} missing from the context", p.id);
    }
    let bar = prompts::plugin_bar_context(&s.project, fm).unwrap();
    assert!(bar.contains("set_plugin_params") && bar.contains(&ctx));
    assert_eq!(prompts::plugin_quick_actions("analog_synth").len(), 3);
    assert_eq!(prompts::plugin_quick_actions("fm_synth").len(), 3);
}

#[test]
fn get_plugin_explains_every_synth_parameter() {
    let mut s = Session::new(false);
    let (_, analog) = synth_track(&mut s, "Lead", "analog_synth");
    let (_, fm) = synth_track(&mut s, "Keys", "fm_synth");
    let times = ["attack", "decay", "release", "f_attack", "f_decay", "f_release", "index_decay"];
    for (fx, kind) in [(analog, "analog_synth"), (fm, "fm_synth")] {
        let info = ok(&mut s, "get_plugin", json!({"plugin_id": fx}));
        let params = info["params"].as_array().unwrap();
        assert_eq!(params.len(), ondina_core::plugins::find(kind).unwrap().params.len(), "{kind}");
        for p in params {
            let id = p["id"].as_str().unwrap();
            let name = p["name"].as_str().unwrap();
            let unit = p["unit"].as_str().unwrap_or_default();
            assert!(!name.is_empty(), "{kind}: {id} has no name");
            assert!(p["min"].as_f64().unwrap() < p["max"].as_f64().unwrap(), "{kind}: {id} has no range");
            if times.contains(&id) {
                assert_eq!(unit, "s", "{kind}: {id}");
            }
            if id == "cutoff" {
                assert_eq!(unit, "Hz");
            }
            if ["osc1_wave", "osc2_wave", "filter_type"].contains(&id) {
                assert!(p["choices"].as_array().is_some_and(|c| !c.is_empty()), "{kind}: {id} has no choices");
            }
        }
    }
    ok(&mut s, "set_plugin_param", json!({"plugin_id": analog, "param": "cutoff", "value": 800}));
    assert_eq!(plugin_value(&mut s, analog, "cutoff"), 800.0);
    s.execute(Command::Undo {}, Source::Gui).unwrap();
    assert_eq!(plugin_value(&mut s, analog, "cutoff"), 2500.0);
}
