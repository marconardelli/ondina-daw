//! Local, offline AI assistant: any OpenAI-compatible chat endpoint with tool calling
//! (llama.cpp `llama-server --jinja`, Ollama, LM Studio...). No API key, no account, data stays
//! on the machine. Aurora can start a bundled llama.cpp server automatically.

use std::path::PathBuf;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;
use std::time::{Duration, Instant};

use aurora_core::command::Command;
use aurora_core::ops;
use crossbeam_channel::Sender;
use serde_json::{json, Value};

use crate::ai::AiEvent;
use crate::mcp;
use crate::session::{SharedSession, Source};
use crate::settings::Settings;

/// Tools offered to small local models (the full set exceeds their context and abilities).
pub const LOCAL_TOOLS: &[&str] = &[
    "get_project", "set_tempo", "set_key", "set_time_signature", "list_patches", "add_track", "remove_track", "rename_track",
    "set_chord_progression", "generate_drums", "generate_bass", "generate_chords", "generate_arpeggio", "generate_melody",
    "create_midi_region", "add_notes", "duplicate_region", "delete_region", "set_volume", "set_pan", "set_mute", "add_effect",
    "add_marker", "copy_section", "play", "stop", "set_playhead", "analyze_mix", "auto_master", "undo", "bounce",
];

const SYSTEM: &str = "You are the assistant inside Aurora DAW. You control the DAW only by calling the provided tools. Positions are in beats (quarter notes); bar N starts at beat (N-1)*4 in 4/4. MIDI pitch 60 = C4. Call get_project to learn track ids before editing existing tracks; add_track returns the new track_id. Prefer library patches (e.g. \"Pop Drum Kit\", \"Deep Sub Bass\", \"Lush Pad\", \"Stage Electric Piano\") and the generate_* tools. After finishing, answer briefly in the user's language.";

fn local_tools() -> Value {
    Value::Array(
        mcp::tools()
            .iter()
            .filter(|t| LOCAL_TOOLS.contains(&t.name.as_str()))
            .map(|t| json!({"type": "function", "function": {"name": t.name, "description": t.description, "parameters": t.input_schema}}))
            .collect(),
    )
}

/// Bundled llama.cpp server and model, looked up next to the executable or in ./tools/llm.
pub fn bundled_llm() -> Option<(PathBuf, PathBuf)> {
    let mut roots = vec![PathBuf::from("tools").join("llm")];
    if let Ok(exe) = std::env::current_exe() {
        for a in exe.ancestors().take(5) {
            roots.push(a.join("tools").join("llm"));
            roots.push(a.join("llm"));
        }
    }
    for r in roots {
        let server = r.join("llama").join("llama-server.exe");
        let model = std::fs::read_dir(&r).ok().and_then(|d| d.filter_map(|e| e.ok()).map(|e| e.path()).find(|p| p.extension().map(|x| x == "gguf").unwrap_or(false)));
        if let (true, Some(m)) = (server.exists(), model) {
            return Some((server, m));
        }
    }
    None
}

fn base_url(settings: &Settings) -> String {
    settings.local_url.trim_end_matches('/').to_string()
}

fn healthy(base: &str) -> bool {
    let root = base.trim_end_matches("/v1");
    ureq::get(&format!("{root}/health")).timeout(Duration::from_secs(2)).call().map(|r| r.status() == 200).unwrap_or(false)
}

/// Make sure a local server answers; start the bundled llama.cpp server if needed.
pub fn ensure_server(settings: &Settings, events: Option<&Sender<AiEvent>>) -> Result<(), String> {
    let base = base_url(settings);
    if healthy(&base) {
        return Ok(());
    }
    let (server, model) = bundled_llm().ok_or("no local AI server is running and no bundled model was found (tools/llm)")?;
    if let Some(e) = events {
        let _ = e.send(AiEvent::Status("Starting local AI model…".into()));
    }
    let port = base.rsplit(':').next().and_then(|p| p.split('/').next()).and_then(|p| p.parse::<u16>().ok()).unwrap_or(8080);
    let threads = std::thread::available_parallelism().map(|n| n.get().max(2) - 1).unwrap_or(4).to_string();
    let mut cmd = std::process::Command::new(server);
    cmd.args(["-m", &model.display().to_string(), "--jinja", "-c", "16384", "--port", &port.to_string(), "--host", "127.0.0.1", "-t", &threads])
        .stdout(std::process::Stdio::null())
        .stderr(std::process::Stdio::null());
    #[cfg(windows)]
    {
        use std::os::windows::process::CommandExt;
        cmd.creation_flags(0x0800_0000); // CREATE_NO_WINDOW
    }
    cmd.spawn().map_err(|e| format!("cannot start llama-server: {e}"))?;
    let t0 = Instant::now();
    while t0.elapsed() < Duration::from_secs(120) {
        if healthy(&base) {
            return Ok(());
        }
        std::thread::sleep(Duration::from_millis(500));
    }
    Err("the local AI server did not become ready".into())
}

pub struct LocalAssistant {
    pub messages: Vec<Value>,
}

impl Default for LocalAssistant {
    fn default() -> Self {
        LocalAssistant { messages: vec![json!({"role": "system", "content": SYSTEM})] }
    }
}

impl LocalAssistant {
    pub fn clear(&mut self) {
        *self = Self::default();
    }

    pub fn run_turn(&mut self, session: &SharedSession, settings: &Settings, user_text: &str, gui_context: &str, events: &Sender<AiEvent>, cancel: &Arc<AtomicBool>) {
        if let Err(e) = ensure_server(settings, Some(events)) {
            let _ = events.send(AiEvent::Error(e));
            let _ = events.send(AiEvent::Done);
            return;
        }
        let summary = {
            let s = session.lock();
            let p = ops::project_summary(&s.project);
            // compact context for small models
            json!({"tempo": p["tempo_bpm"], "key": p["key"], "tracks": p["tracks"].as_array().map(|ts| ts.iter().map(|t| json!({"id": t["id"], "name": t["name"], "regions": t["regions"].as_array().map(|r| r.len()).unwrap_or(0)})).collect::<Vec<_>>())})
        };
        self.messages.push(json!({"role": "user", "content": format!("{user_text}\n\n[Aurora context] {gui_context} Project: {summary}")}));
        let tools = local_tools();
        let url = format!("{}/chat/completions", base_url(settings));
        for _ in 0..20 {
            if cancel.load(Ordering::Relaxed) {
                break;
            }
            let _ = events.send(AiEvent::Status("Thinking (local model)…".into()));
            let body = json!({"model": settings.local_model, "messages": self.messages, "tools": tools, "tool_choice": "auto", "temperature": 0.2, "max_tokens": 1024});
            let resp: Value = match ureq::post(&url).timeout(Duration::from_secs(600)).set("content-type", "application/json").send_string(&body.to_string()) {
                Ok(r) => match r.into_string().ok().and_then(|t| serde_json::from_str(&t).ok()) {
                    Some(v) => v,
                    None => {
                        let _ = events.send(AiEvent::Error("invalid response from the local model".into()));
                        break;
                    }
                },
                Err(e) => {
                    let _ = events.send(AiEvent::Error(format!("local model error: {e}")));
                    break;
                }
            };
            let msg = resp.pointer("/choices/0/message").cloned().unwrap_or(json!({}));
            let calls = msg.get("tool_calls").and_then(|c| c.as_array()).cloned().unwrap_or_default();
            let text = msg.get("content").and_then(|c| c.as_str()).unwrap_or("").to_string();
            let mut stored = json!({"role": "assistant", "content": text});
            if !calls.is_empty() {
                stored["tool_calls"] = Value::Array(calls.clone());
            }
            self.messages.push(stored);
            if !text.trim().is_empty() {
                let _ = events.send(AiEvent::Text(text));
            }
            if calls.is_empty() {
                break;
            }
            for c in calls {
                let id = c.get("id").and_then(|x| x.as_str()).unwrap_or("call").to_string();
                let name = c.pointer("/function/name").and_then(|x| x.as_str()).unwrap_or("").to_string();
                let args_raw = c.pointer("/function/arguments").cloned().unwrap_or(json!("{}"));
                let args: Value = match &args_raw {
                    Value::String(s) => serde_json::from_str(s).unwrap_or(json!({})),
                    v => v.clone(),
                };
                let (ok, out) = if !LOCAL_TOOLS.contains(&name.as_str()) {
                    (false, format!("unknown tool {name}"))
                } else {
                    match Command::from_tool_call(&name, &args) {
                        Ok(cmd) => match session.lock().execute(cmd, Source::Ai) {
                            Ok(v) => (true, serde_json::to_string(&v).unwrap_or_default()),
                            Err(e) => (false, format!("Error: {e}")),
                        },
                        Err(e) => (false, format!("Error: {e}")),
                    }
                };
                let short: String = out.chars().take(1500).collect();
                let _ = events.send(AiEvent::ToolCall { name: name.clone(), input: args.clone(), ok, result: short.chars().take(300).collect() });
                self.messages.push(json!({"role": "tool", "tool_call_id": id, "content": short}));
            }
        }
        let _ = events.send(AiEvent::Done);
    }
}
