//! End-to-end tests of the extended feature set, driven exactly like an MCP client would
//! (tool name + JSON arguments).

use std::path::{Path, PathBuf};

use aurora_core::command::{tool_specs, Command};
use aurora_studio::{Session, Source};
use serde_json::{json, Value};

fn call(s: &mut Session, name: &str, args: Value) -> Result<Value, String> {
    let cmd = Command::from_tool_call(name, &args)?;
    s.execute(cmd, Source::Mcp)
}

fn ok(s: &mut Session, name: &str, args: Value) -> Value {
    call(s, name, args.clone()).unwrap_or_else(|e| panic!("{name} {args} failed: {e}"))
}

fn tmp(name: &str) -> PathBuf {
    let d = std::env::temp_dir().join("aurora_features");
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
    let path = tmp("fresh_history_open.aurora");
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
    let path = tmp("fresh_history_keep.aurora");
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
    let Some(ff) = aurora_studio::session_ext::ffmpeg_path() else {
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
    let p = tmp("vst.aurora");
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
    // ADM azimuth is positive to the left: the first block (Aurora -90 = left) must be +90
    assert!(xml.contains("<position coordinate=\"azimuth\">90.00</position>"));
}

#[test]
fn ml_stem_splitter() {
    if aurora_dsp::ml_stems::find_model_dir().is_none() && !Path::new(env!("CARGO_MANIFEST_DIR")).join("../../tools/models").exists() {
        eprintln!("ML model not installed: skipping");
        return;
    }
    std::env::set_var("AURORA_MODELS", Path::new(env!("CARGO_MANIFEST_DIR")).join("../../tools/models"));
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
    s.project.movie = Some(aurora_core::model::MovieRef { path: "http://x/m.mp4".into(), offset_sec: 0.0, duration_sec: 4.0, audio_file_id: None });
    let file = tmp("url_movie.aurora");
    std::fs::write(&file, aurora_core::save_project_string(&s.project).unwrap()).unwrap();
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
    s.project.movie = Some(aurora_core::model::MovieRef { path: tmp("local_movie.mp4").display().to_string(), offset_sec: 0.0, duration_sec: 4.0, audio_file_id: None });
    for (tool, path) in [("extract_movie_frame", "-y.png".to_string()), ("extract_movie_frame", tmp("frame.bmp").display().to_string()), ("export_movie", tmp("scored.avi").display().to_string())] {
        let e = call(&mut s, tool, json!({"path": path})).unwrap_err();
        assert!(e.contains("output path refused"), "{tool} {path}: {e}");
    }
}

#[test]
fn movie_frame_preview_uses_local_files_only() {
    assert!(aurora_studio::session_ext::movie_frame_rgb("http://127.0.0.1:9/m.mp4", 0.0, 320, 180).is_err());
    let Some(ff) = aurora_studio::session_ext::ffmpeg_path() else {
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
    let px = aurora_studio::session_ext::movie_frame_rgb(&mp4.display().to_string(), 1.0, 320, 180).unwrap();
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
