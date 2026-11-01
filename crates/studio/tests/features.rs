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

    ok(&mut s, "open_project", json!({"path": path.display().to_string()}));
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

    ok(&mut s, "new_project", json!({"name": "Second", "tempo": 90}));
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

    ok(&mut s, "new_project", json!({"name": "Fresh"}));
    std::thread::sleep(std::time::Duration::from_millis(100));
    let tr = ok(&mut s, "get_transport", json!({}));
    assert_eq!(tr["playing"], false, "{tr}");
    assert_eq!(tr["beat"].as_f64().unwrap(), 0.0, "{tr}");
    let live = ok(&mut s, "get_live_loops", json!({}));
    assert!(live["playing"].as_array().unwrap().is_empty(), "{live}");
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

    s.execute(Command::OpenProject { path: path.display().to_string() }, Source::Gui).unwrap();
    assert!(!s.engine.is_recording(), "the recording must not survive the open");
    // the reopened project has the same armed track: a take written into it would show up here
    assert!(s.project.tracks.iter().all(|tr| tr.regions.is_empty()));
    let r = ok(&mut s, "stop", json!({}));
    assert!(r.get("recorded_regions").is_none(), "{r}");
    assert!(s.project.tracks.iter().all(|tr| tr.regions.is_empty()));

    // the same for New
    ok(&mut s, "record", json!({"from_beat": 0}));
    s.execute(Command::NewProject { name: None, tempo: None, time_signature: None, key: None }, Source::Gui).unwrap();
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
    ok(&mut s, "new_project", json!({}));
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
    run(&mut s, "new_project", json!({"name": "Fresh"}), Source::Ai);
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
    ok(&mut s, "new_project", json!({"name": "Second"}));
    assert_eq!(s.document, first + 1);
    ok(&mut s, "open_project", json!({"path": path.display().to_string()}));
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
