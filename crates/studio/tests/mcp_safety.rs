//! What an MCP client can do to the process and to the user's work: renders are bounded, unsaved work is not
//! dropped silently, network paths are refused and the tool annotations tell a client which tools change things.

use std::path::PathBuf;

use ondina_core::command::Command;
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
    let d = std::env::temp_dir().join("ondina_mcp_safety");
    std::fs::create_dir_all(&d).unwrap();
    d.join(name)
}

fn song(s: &mut Session) -> u64 {
    let t = ok(s, "add_track", json!({"kind": "instrument"}))["track_id"].as_u64().unwrap();
    ok(s, "create_midi_region", json!({"track_id": t, "start": 0.0, "length": 4.0, "notes": [{"pitch": 60, "velocity": 100, "start": 0.0, "length": 1.0}]}))["region_id"].as_u64().unwrap()
}

// ---------------------------------------------------------------- renders

#[test]
fn bounce_refuses_a_range_longer_than_the_render_limit() {
    let mut s = Session::new(false);
    song(&mut s);
    let out = tmp("too-long.wav");
    let _ = std::fs::remove_file(&out);
    let err = call(&mut s, "bounce", json!({"path": out.display().to_string(), "end": 1e7})).unwrap_err();
    assert!(err.contains("60 minutes"), "{err}");
    assert!(!out.exists());
}

#[test]
fn analyze_mix_refuses_a_range_longer_than_the_render_limit() {
    let mut s = Session::new(false);
    song(&mut s);
    let err = call(&mut s, "analyze_mix", json!({"end": 1e7})).unwrap_err();
    assert!(err.contains("60 minutes"), "{err}");
}

#[test]
fn renders_of_the_whole_song_refuse_a_song_longer_than_the_render_limit() {
    let mut s = Session::new(false);
    let rid = song(&mut s);
    // a region moved far away makes the song end there
    ok(&mut s, "move_region", json!({"region_id": rid, "start": 1e8}));
    for (tool, file) in [("bounce_surround", "far-51.wav"), ("bounce_binaural", "far-bin.wav"), ("export_adm", "far-adm.wav")] {
        let out = tmp(file);
        let err = call(&mut s, tool, json!({"path": out.display().to_string()})).unwrap_err();
        assert!(err.contains("60 minutes"), "{tool}: {err}");
    }
    let t = s.project.tracks[0].id;
    let err = call(&mut s, "bounce_in_place", json!({"track_id": t})).unwrap_err();
    assert!(err.contains("60 minutes"), "bounce_in_place: {err}");
}
