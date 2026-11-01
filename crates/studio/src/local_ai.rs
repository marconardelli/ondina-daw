//! Local, offline AI assistant: any OpenAI-compatible chat endpoint with tool calling
//! (llama.cpp `llama-server --jinja`, Ollama, LM Studio...). No API key, no account, data stays
//! on the machine. Ondina can start a bundled llama.cpp server automatically.

use std::path::PathBuf;
use std::process::Child;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;
use std::time::{Duration, Instant};

use ondina_core::command::Command;
use ondina_core::ops;
use crossbeam_channel::{RecvTimeoutError, Sender};
use serde_json::{json, Value};

use crate::ai::AiEvent;
use crate::mcp;
use crate::session::{AssistantUndoGroup, SharedSession, Source};
use crate::settings::Settings;

/// Tools offered to small local models (the full set exceeds their context and abilities).
/// Keep the schema small: `history_budget` subtracts its length from the room for the conversation.
pub const LOCAL_TOOLS: &[&str] = &[
    "get_project", "set_tempo", "set_key", "set_time_signature", "list_patches", "add_track", "remove_track", "rename_track",
    "set_chord_progression", "generate_drums", "generate_bass", "generate_chords", "generate_arpeggio", "generate_melody",
    "create_midi_region", "add_notes", "duplicate_region", "delete_region", "set_volume", "set_pan", "set_mute", "add_effect",
    "add_marker", "copy_section", "play", "stop", "set_playhead", "analyze_mix", "auto_master", "undo", "bounce",
    "redo", "get_region", "replace_notes", "quantize_notes", "transpose_notes", "set_note_velocities", "humanize_notes",
    "snap_notes_to_key", "set_pattern_row",
    "add_pattern_row", "get_plugin", "set_plugin_params", "add_scene", "create_live_cell",
];

const SYSTEM: &str = "You are the assistant inside Ondina DAW. You control the DAW only by calling the provided tools. Positions are in beats (quarter notes); bar N starts at beat (N-1)*4 in 4/4. MIDI pitch 60 = C4. Call get_project to learn track ids before editing existing tracks; add_track returns the new track_id. Prefer library patches (e.g. \"Pop Drum Kit\", \"Deep Sub Bass\", \"Lush Pad\", \"Stage Electric Piano\") and the generate_* tools. The [Ondina context] lists the selected notes as index:pitch@start+length; to change only those notes pass their index values as indices (quantize_notes, humanize_notes, transpose_notes, set_note_velocities, snap_notes_to_key). quantize_notes and humanize_notes re-sort the region: for a further edit of the same notes use the indices they return, not the ones from the context. Read a region with get_region before rewriting it and a plug-in with get_plugin before setting its parameters. After finishing, answer briefly in the user's language. All your edits for one request are one undo step: undo reverts everything you did in this request, so fix a single mistake directly.";

/// Context size the bundled server is started with; the history budget is derived from it.
const CONTEXT_TOKENS: usize = 16384;
/// Longest answer requested from the model per step.
const MAX_TOKENS: usize = 1024;
/// Conservative characters-per-token ratio (JSON and tool output tokenise worse than prose).
const CHARS_PER_TOKEN: usize = 3;
/// Never shrink the history below this, even with a very large tool list.
const MIN_HISTORY_CHARS: usize = 6000;
/// Appended once to the system message when older turns are dropped.
const OMITTED_NOTE: &str = "(earlier conversation omitted)";
/// Characters of a tool result the model sees.
const TOOL_RESULT_CHARS: usize = 1500;
/// The reads the ✨ requests depend on (a synth has about 3000 characters of parameters) get more room.
const READ_RESULT_CHARS: usize = 4000;
const LONG_READS: &[&str] = &["get_region", "get_plugin"];

/// The part of a tool result that goes back to the model.
fn result_for_model(name: &str, out: &str) -> String {
    let limit = if LONG_READS.contains(&name) { READ_RESULT_CHARS } else { TOOL_RESULT_CHARS };
    out.chars().take(limit).collect()
}

fn local_tools() -> Value {
    Value::Array(
        mcp::tools()
            .iter()
            .filter(|t| LOCAL_TOOLS.contains(&t.name.as_str()))
            .map(|t| json!({"type": "function", "function": {"name": t.name, "description": t.description, "parameters": t.input_schema}}))
            .collect(),
    )
}

/// How many tools the local model is offered: the names of `LOCAL_TOOLS` that exist among the MCP tools.
/// Counted with the same filter as `local_tools()`, without building the schemas, so the panel can call it every frame.
pub fn tool_count() -> usize {
    mcp::tools().iter().filter(|t| LOCAL_TOOLS.contains(&t.name.as_str())).count()
}

/// Characters of history that fit next to the tool list and the answer in the server context.
fn history_budget(tools_chars: usize) -> usize {
    ((CONTEXT_TOKENS - MAX_TOKENS) * CHARS_PER_TOKEN * 4 / 5).saturating_sub(tools_chars).max(MIN_HISTORY_CHARS)
}

/// Drops the oldest turns so the history fits `budget` characters (serialized JSON length).
/// The system message is kept. A turn runs from a `user` message to the next one, so cutting at turn
/// boundaries never separates an assistant `tool_calls` message from its `tool` results. If the last turn
/// alone is too long, whole steps (an assistant message plus its tool results) are dropped from its start,
/// keeping the user request and the latest step. The result fits whenever system + note + that user
/// message + the latest step fit. Returns whether anything was removed.
fn trim_history(messages: &mut Vec<Value>, budget: usize) -> bool {
    let size = |ms: &[Value]| ms.iter().map(|m| m.to_string().len()).sum::<usize>();
    if messages.len() < 2 || size(messages) <= budget {
        return false;
    }
    if let Some(content) = messages[0]["content"].as_str() {
        if !content.contains(OMITTED_NOTE) {
            messages[0]["content"] = Value::String(format!("{content}\n{OMITTED_NOTE}"));
        }
    }
    // whole turns, oldest first, while at least one more turn follows
    while size(messages) > budget {
        let users: Vec<usize> = (1..messages.len()).filter(|&i| messages[i]["role"] == "user").collect();
        if users.len() < 2 {
            break;
        }
        messages.drain(1..users[1]);
    }
    // whole steps inside the last turn, keeping its user request and its latest step
    let start = (1..messages.len()).rev().find(|&i| messages[i]["role"] == "user").unwrap_or(1);
    while size(messages) > budget {
        let assistants: Vec<usize> = (start + 1..messages.len()).filter(|&i| messages[i]["role"] == "assistant").collect();
        if assistants.len() < 2 {
            break;
        }
        messages.drain(assistants[0]..assistants[1]);
    }
    true
}

/// A conversation must not end on a user or tool message before the next user text: some chat
/// templates reject two user messages in a row.
fn close_dangling_turn(messages: &mut Vec<Value>) {
    if messages.last().map(|m| m["role"] != "assistant").unwrap_or(false) {
        messages.push(json!({"role": "assistant", "content": "(stopped)"}));
    }
}

/// Posts `body` on a helper thread and waits for the answer while watching `cancel`.
/// Returns Ok(None) when the user pressed Stop: the request is abandoned and its thread ends on its own
/// when the server answers (the reply goes nowhere).
fn post_cancellable(url: &str, body: String, cancel: &AtomicBool) -> Result<Option<Value>, String> {
    let (tx, rx) = crossbeam_channel::bounded::<Result<Value, String>>(1);
    let url = url.to_string();
    std::thread::Builder::new()
        .name("local-ai-request".into())
        .spawn(move || {
            let r = match ureq::post(&url).timeout(Duration::from_secs(600)).set("content-type", "application/json").send_string(&body) {
                Ok(r) => r.into_string().ok().and_then(|t| serde_json::from_str(&t).ok()).ok_or_else(|| "invalid response from the local model".to_string()),
                Err(e) => Err(format!("local model error: {e}")),
            };
            let _ = tx.send(r);
        })
        .map_err(|e| format!("cannot start the request thread: {e}"))?;
    loop {
        if cancel.load(Ordering::Relaxed) {
            return Ok(None);
        }
        match rx.recv_timeout(Duration::from_millis(100)) {
            Ok(r) => return r.map(Some),
            Err(RecvTimeoutError::Timeout) => continue,
            Err(RecvTimeoutError::Disconnected) => return Err("the local model request ended unexpectedly".into()),
        }
    }
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

/// The llama-server process this Ondina started, if any. A server that was already answering when
/// Ondina needed it (Ollama, LM Studio, a server started by hand) is never stored here and never stopped.
static SERVER: parking_lot::Mutex<Option<Child>> = parking_lot::const_mutex(None);
/// Set by `shutdown_server`: after it, the bundled server is never started again in this process.
static SHUTTING_DOWN: AtomicBool = AtomicBool::new(false);

/// Clears `slot` when its process has exited. Returns whether a live process is still there.
fn take_if_exited(slot: &mut Option<Child>) -> bool {
    match slot.as_mut().map(|c| c.try_wait()) {
        Some(Ok(None)) => true,
        Some(_) => {
            *slot = None;
            false
        }
        None => false,
    }
}

/// Kills `child` and waits briefly for it to go. Returns whether it has exited.
fn stop_child(child: &mut Child) -> bool {
    let _ = child.kill();
    let t0 = Instant::now();
    while t0.elapsed() < Duration::from_secs(1) {
        if matches!(child.try_wait(), Ok(Some(_))) {
            return true;
        }
        std::thread::sleep(Duration::from_millis(20));
    }
    false
}

/// Stops the llama-server process Ondina started, if it is still running. Called when the app exits;
/// a server that was already running before Ondina needed it is left alone. Never blocks for more than
/// about a second.
pub fn shutdown_server() {
    SHUTTING_DOWN.store(true, Ordering::SeqCst);
    let child = SERVER.lock().take();
    if let Some(mut c) = child {
        stop_child(&mut c);
    }
}

/// Make sure a local server answers; start the bundled llama.cpp server if needed.
/// `cancel` (Stop) ends the wait for the model to load; the server keeps loading for the next request.
pub fn ensure_server(settings: &Settings, events: Option<&Sender<AiEvent>>, cancel: &AtomicBool) -> Result<(), String> {
    let base = base_url(settings);
    if healthy(&base) {
        return Ok(());
    }
    let status = |s: &str| {
        if let Some(e) = events {
            let _ = e.send(AiEvent::Status(s.into()));
        }
    };
    let alive = take_if_exited(&mut SERVER.lock());
    if alive {
        // our server is still loading the model for an earlier request: a second one on the same port would fail
        status("Waiting for the local AI model…");
    } else {
        let (server, model) = bundled_llm().ok_or("no local AI server is running and no bundled model was found (tools/llm)")?;
        status("Starting local AI model…");
        let port = base.rsplit(':').next().and_then(|p| p.split('/').next()).and_then(|p| p.parse::<u16>().ok()).unwrap_or(8080);
        let threads = std::thread::available_parallelism().map(|n| n.get().max(2) - 1).unwrap_or(4).to_string();
        let mut cmd = std::process::Command::new(server);
        cmd.args(["-m", &model.display().to_string(), "--jinja", "-c", &CONTEXT_TOKENS.to_string(), "--port", &port.to_string(), "--host", "127.0.0.1", "-t", &threads])
            .stdout(std::process::Stdio::null())
            .stderr(std::process::Stdio::null());
        #[cfg(windows)]
        {
            use std::os::windows::process::CommandExt;
            cmd.creation_flags(0x0800_0000); // CREATE_NO_WINDOW
        }
        // spawned under the lock after checking the flag, so no server can start after the app has exited
        let mut slot = SERVER.lock();
        if SHUTTING_DOWN.load(Ordering::SeqCst) {
            return Err("Ondina is closing".into());
        }
        *slot = Some(cmd.spawn().map_err(|e| format!("cannot start llama-server: {e}"))?);
    }
    let t0 = Instant::now();
    while t0.elapsed() < Duration::from_secs(120) {
        if healthy(&base) {
            return Ok(());
        }
        if cancel.load(Ordering::Relaxed) {
            return Err("stopped".into());
        }
        if !take_if_exited(&mut SERVER.lock()) {
            return Err("the local AI server stopped while loading the model".into());
        }
        std::thread::sleep(Duration::from_millis(500));
    }
    // a server that never became ready does not stay in memory
    let child = SERVER.lock().take();
    if let Some(mut c) = child {
        stop_child(&mut c);
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
        if let Err(e) = ensure_server(settings, Some(events), cancel) {
            let _ = events.send(if cancel.load(Ordering::Relaxed) { AiEvent::Status("Stopped.".into()) } else { AiEvent::Error(e) });
            let _ = events.send(AiEvent::Done);
            return;
        }
        let summary = {
            let s = session.lock();
            let p = ops::project_summary(&s.project);
            // compact context for small models
            json!({"tempo": p["tempo_bpm"], "key": p["key"], "tracks": p["tracks"].as_array().map(|ts| ts.iter().map(|t| json!({"id": t["id"], "name": t["name"], "regions": t["regions"].as_array().map(|r| r.len()).unwrap_or(0)})).collect::<Vec<_>>())})
        };
        self.messages.push(json!({"role": "user", "content": format!("{user_text}\n\n[Ondina context] {gui_context} Project: {summary}")}));
        let undo_group = AssistantUndoGroup::open(session, crate::ai::undo_label(user_text));
        let tools = local_tools();
        let budget = history_budget(tools.to_string().len());
        let url = format!("{}/chat/completions", base_url(settings));
        for _ in 0..20 {
            if cancel.load(Ordering::Relaxed) {
                let _ = events.send(AiEvent::Status("Stopped.".into()));
                break;
            }
            let _ = events.send(AiEvent::Status("Thinking (local model)…".into()));
            trim_history(&mut self.messages, budget);
            let body = json!({"model": settings.local_model, "messages": self.messages, "tools": tools, "tool_choice": "auto", "temperature": 0.2, "max_tokens": MAX_TOKENS});
            let resp = match post_cancellable(&url, body.to_string(), cancel) {
                Ok(Some(v)) => v,
                Ok(None) => {
                    let _ = events.send(AiEvent::Status("Stopped.".into()));
                    break;
                }
                Err(e) => {
                    let _ = events.send(AiEvent::Error(e));
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
                if cancel.load(Ordering::Relaxed) {
                    // every tool call still gets its answer, but nothing more runs after Stop
                    self.messages.push(json!({"role": "tool", "tool_call_id": id, "content": "Error: stopped by the user"}));
                    continue;
                }
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
                let short = result_for_model(&name, &out);
                let _ = events.send(AiEvent::ToolCall { name: name.clone(), input: args.clone(), ok, result: short.chars().take(300).collect() });
                self.messages.push(json!({"role": "tool", "tool_call_id": id, "content": short}));
            }
        }
        close_dangling_turn(&mut self.messages);
        // closed before Done, so the GUI already sees the finished step
        drop(undo_group);
        let _ = events.send(AiEvent::Done);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::prompts;
    use crate::session::Session;
    use ondina_core::model::RegionContent;
    use std::process::Stdio;

    /// One user request, `steps` tool-calling steps with `payload` characters of tool output each, a final answer.
    fn turn(tag: &str, steps: usize, payload: usize) -> Vec<Value> {
        let mut ms = vec![json!({"role": "user", "content": format!("request {tag}")})];
        for s in 0..steps {
            let id = format!("{tag}-{s}");
            ms.push(json!({"role": "assistant", "content": "", "tool_calls": [{"id": id, "type": "function", "function": {"name": "set_tempo", "arguments": "{\"bpm\":90}"}}]}));
            ms.push(json!({"role": "tool", "tool_call_id": id, "content": "x".repeat(payload)}));
        }
        ms.push(json!({"role": "assistant", "content": format!("ok {tag}")}));
        ms
    }

    fn history(turns: &[Vec<Value>]) -> Vec<Value> {
        let mut ms = LocalAssistant::default().messages;
        for t in turns {
            ms.extend(t.iter().cloned());
        }
        ms
    }

    fn size(ms: &[Value]) -> usize {
        ms.iter().map(|m| m.to_string().len()).sum()
    }

    /// Serialized length the system message gains with the note ("\n" is two characters in JSON).
    fn note_len() -> usize {
        OMITTED_NOTE.len() + 2
    }

    fn assert_pairs(ms: &[Value]) {
        for (i, m) in ms.iter().enumerate() {
            if m["role"] == "assistant" {
                if let Some(calls) = m["tool_calls"].as_array() {
                    for (k, c) in calls.iter().enumerate() {
                        let t = ms.get(i + 1 + k).unwrap_or_else(|| panic!("missing tool result {k} after message {i}"));
                        assert_eq!(t["role"], "tool", "message {} should be a tool result", i + 1 + k);
                        assert_eq!(t["tool_call_id"], c["id"]);
                    }
                }
            }
            if m["role"] == "tool" {
                let mut j = i;
                while j > 0 && ms[j - 1]["role"] == "tool" {
                    j -= 1;
                }
                assert!(j > 0, "tool message {i} without an assistant");
                let a = &ms[j - 1];
                assert_eq!(a["role"], "assistant", "tool message {i} without an assistant");
                let ids: Vec<&Value> = a["tool_calls"].as_array().map(|c| c.iter().map(|c| &c["id"]).collect()).unwrap_or_default();
                assert!(ids.contains(&&m["tool_call_id"]), "tool message {i} does not answer its assistant");
            }
        }
    }

    #[test]
    fn trim_history_leaves_a_short_history_unchanged() {
        let mut ms = history(&[turn("a", 1, 100)]);
        let before = ms.clone();
        let budget = size(&ms) + 1;
        assert!(!trim_history(&mut ms, budget));
        assert_eq!(ms, before);
        assert!(!ms[0]["content"].as_str().unwrap().contains(OMITTED_NOTE));
    }

    #[test]
    fn trim_history_drops_the_oldest_turns_first() {
        let turns: Vec<Vec<Value>> = (0..5).map(|i| turn(&format!("t{i}"), 2, 2000)).collect();
        let mut ms = history(&turns);
        let budget = size(&ms[..1]) + note_len() + size(&turns[3]) + size(&turns[4]);
        assert!(trim_history(&mut ms, budget));
        assert_eq!(ms[0]["role"], "system");
        assert_eq!(ms[0]["content"].as_str().unwrap().matches(OMITTED_NOTE).count(), 1);
        assert_eq!(ms[1]["role"], "user");
        assert_eq!(ms[1]["content"], "request t3");
        assert_eq!(ms.last().unwrap()["content"], "ok t4");
        let text = Value::Array(ms.clone()).to_string();
        for old in ["t0", "t1", "t2"] {
            assert!(!text.contains(&format!("request {old}")) && !text.contains(&format!("\"{old}-")), "{old} should be gone");
        }
        assert_pairs(&ms);
        assert!(size(&ms) <= budget);
    }

    #[test]
    fn trim_history_never_splits_tool_calls_from_results() {
        let turns: Vec<Vec<Value>> = (0..6).map(|i| turn(&format!("h{i}"), 1 + i % 4, 500 + i * 500)).collect();
        let full = history(&turns);
        for budget in (2000..60000).step_by(1500) {
            let mut ms = full.clone();
            trim_history(&mut ms, budget);
            assert_pairs(&ms);
            assert_eq!(ms[0]["role"], "system");
            let last_user = turns[5][0].clone();
            let last_assistant = full.iter().rposition(|m| m["role"] == "assistant").unwrap();
            let floor = size(&full[..1]) + note_len() + size(&[last_user]) + size(&full[last_assistant..]);
            if floor <= budget {
                assert!(size(&ms) <= budget, "budget {budget}: size {}", size(&ms));
            }
        }
    }

    #[test]
    fn trim_history_drops_steps_inside_one_long_turn() {
        let mut ms = history(&[turn("long", 12, 3000)]);
        assert!(trim_history(&mut ms, 12000));
        assert_eq!(ms[1]["role"], "user");
        assert_eq!(ms[1]["content"], "request long");
        assert_eq!(ms.last().unwrap()["content"], "ok long");
        assert!(ms.iter().any(|m| m["tool_call_id"] == "long-11"), "the latest step is kept");
        assert!(!ms.iter().any(|m| m["tool_call_id"] == "long-0"), "the oldest step is dropped");
        assert_pairs(&ms);
        assert!(size(&ms) <= 12000);
    }

    #[test]
    fn trim_history_adds_the_note_only_once() {
        let turns: Vec<Vec<Value>> = (0..4).map(|i| turn(&format!("n{i}"), 2, 1000)).collect();
        let mut ms = history(&turns);
        trim_history(&mut ms, 3000);
        trim_history(&mut ms, 2500);
        assert_eq!(ms[0]["content"].as_str().unwrap().matches(OMITTED_NOTE).count(), 1);
    }

    #[test]
    fn history_budget_leaves_room_for_the_local_tools() {
        let budget = history_budget(local_tools().to_string().len());
        assert!(budget >= 2 * MIN_HISTORY_CHARS, "only {budget} characters left for the conversation");
        assert_eq!(history_budget(usize::MAX / 2), MIN_HISTORY_CHARS);
    }

    #[test]
    fn local_tools_cover_the_contextual_requests() {
        let design = prompts::design_sound(7, "Analog Synth");
        assert_eq!(design, "Adjust the parameters of plug-in 7 (Analog Synth) to get a great sound for this track's role in the song. Explain briefly.");
        // tools each request needs even when its text does not name them
        let cases: [(&str, &[&str]); 9] = [
            (prompts::CONTINUE_IMPROVE, &["get_region", "replace_notes", "add_notes", "quantize_notes", "transpose_notes"]),
            (prompts::WRITE_GROOVE, &["get_region", "set_pattern_row", "add_pattern_row"]),
            (design.as_str(), &["get_plugin", "set_plugin_params"]),
            (prompts::FILL_GRID, &["get_project", "add_scene", "create_live_cell"]),
            (prompts::FIX_MIX, &["analyze_mix", "set_volume", "set_pan", "add_effect", "set_plugin_params"]),
            (prompts::WRITE_PART, &["get_project", "list_patches", "create_midi_region", "add_notes", "generate_melody"]),
            (prompts::VARY_REGION, &["get_region", "create_midi_region", "duplicate_region", "replace_notes"]),
            ("Humanize the selected notes and snap them to the key.", &["humanize_notes", "snap_notes_to_key"]),
            (SYSTEM, &["quantize_notes", "humanize_notes", "transpose_notes", "set_note_velocities", "snap_notes_to_key"]),
        ];
        let all: Vec<&str> = mcp::tools().iter().map(|t| t.name.as_str()).collect();
        for (text, needed) in cases {
            for word in text.split(|c: char| !(c.is_ascii_alphanumeric() || c == '_')) {
                if all.contains(&word) {
                    assert!(LOCAL_TOOLS.contains(&word), "{word}, named in {text:?}, is not offered to the local model");
                }
            }
            for tool in needed {
                assert!(LOCAL_TOOLS.contains(tool), "{tool} is needed by {text:?}");
            }
        }
        assert!(SYSTEM.contains("indices"), "the local system prompt must explain indices");
    }

    #[test]
    fn long_reads_reach_the_local_model_whole() {
        let big = "x".repeat(3000);
        assert_eq!(result_for_model("get_plugin", &big).len(), 3000);
        assert_eq!(result_for_model("get_region", &big).len(), 3000);
        assert_eq!(result_for_model("set_tempo", &big).len(), TOOL_RESULT_CHARS);
        assert_eq!(result_for_model("get_plugin", &"y".repeat(9000)).len(), READ_RESULT_CHARS);
        // characters, not bytes
        assert_eq!(result_for_model("set_tempo", &"é".repeat(2000)).chars().count(), TOOL_RESULT_CHARS);
    }

    #[test]
    fn local_tools_are_existing_tools() {
        let specs = ondina_core::command::tool_specs();
        for name in LOCAL_TOOLS {
            assert!(specs.iter().any(|t| t.name == *name), "{name} is not a tool");
        }
        let unique: std::collections::HashSet<&&str> = LOCAL_TOOLS.iter().collect();
        assert_eq!(unique.len(), LOCAL_TOOLS.len(), "duplicate names in LOCAL_TOOLS");
        assert_eq!(local_tools().as_array().unwrap().len(), LOCAL_TOOLS.len());
        for name in ["set_pattern_row", "get_plugin", "set_plugin_params", "create_live_cell", "get_region", "replace_notes", "quantize_notes", "transpose_notes", "humanize_notes", "snap_notes_to_key"] {
            assert!(LOCAL_TOOLS.contains(&name), "{name} is missing");
        }
    }

    #[test]
    fn tool_count_matches_the_local_tools() {
        assert_eq!(tool_count(), local_tools().as_array().unwrap().len());
        assert_eq!(tool_count(), LOCAL_TOOLS.len());
        assert!(tool_count() < mcp::tools().len(), "the local model gets a subset");
    }

    #[test]
    fn system_prompt_warns_that_quantize_and_humanize_reorder_notes() {
        assert!(SYSTEM.contains("re-sort"), "the local system prompt must say that quantize and humanize re-sort notes");
        assert!(SYSTEM.contains("indices they return"), "the local system prompt must point to the returned indices");
        for tool in ["quantize_notes", "humanize_notes", "transpose_notes", "set_note_velocities", "snap_notes_to_key"] {
            assert!(SYSTEM.contains(tool), "{tool} should be named with indices");
        }
    }

    #[test]
    fn close_dangling_turn_ends_on_an_assistant_message() {
        let mut ms = history(&[]);
        ms.push(json!({"role": "user", "content": "hi"}));
        close_dangling_turn(&mut ms);
        assert_eq!(ms.last().unwrap(), &json!({"role": "assistant", "content": "(stopped)"}));
        let mut ms = history(&[turn("d", 1, 10)]);
        ms.pop();
        assert_eq!(ms.last().unwrap()["role"], "tool");
        close_dangling_turn(&mut ms);
        assert_eq!(ms.last().unwrap(), &json!({"role": "assistant", "content": "(stopped)"}));
        let mut ms = history(&[turn("e", 1, 10)]);
        let n = ms.len();
        close_dangling_turn(&mut ms);
        assert_eq!(ms.len(), n);
    }

    /// OpenAI-compatible mock on a free port: `/health` answers at once, each chat request gets the next
    /// reply of `replies` (the last one repeats) after `delay`.
    fn mock_server(delay: Duration, replies: Vec<Value>) -> u16 {
        let server = tiny_http::Server::http("127.0.0.1:0").unwrap();
        let port = server.server_addr().to_ip().unwrap().port();
        std::thread::spawn(move || {
            let mut next = 0;
            for mut req in server.incoming_requests() {
                if req.url().ends_with("/health") {
                    let _ = req.respond(tiny_http::Response::from_string(json!({"status": "ok"}).to_string()));
                    continue;
                }
                let mut body = String::new();
                let _ = req.as_reader().read_to_string(&mut body);
                let reply = replies[next.min(replies.len() - 1)].clone();
                next += 1;
                std::thread::spawn(move || {
                    std::thread::sleep(delay);
                    let _ = req.respond(tiny_http::Response::from_string(reply.to_string()));
                });
            }
        });
        port
    }

    fn local_settings(port: u16) -> Settings {
        Settings { provider: "local".into(), local_url: format!("http://127.0.0.1:{port}/v1"), ..Default::default() }
    }

    #[test]
    fn local_turn_runs_tools_against_a_mock_server() {
        let port = mock_server(Duration::ZERO, vec![
            json!({"choices": [{"message": {"role": "assistant", "content": "", "tool_calls": [{"id": "c1", "type": "function", "function": {"name": "set_tempo", "arguments": "{\"bpm\":90}"}}]}}]}),
            json!({"choices": [{"message": {"role": "assistant", "content": "Done."}}]}),
        ]);
        let session = Session::shared(false);
        let (tx, rx) = crossbeam_channel::unbounded();
        let mut a = LocalAssistant::default();
        a.run_turn(&session, &local_settings(port), "set tempo", "", &tx, &Arc::new(AtomicBool::new(false)));
        assert_eq!(session.lock().project.tempo.initial_bpm(), 90.0);
        let events: Vec<AiEvent> = rx.try_iter().collect();
        assert!(events.iter().any(|e| matches!(e, AiEvent::ToolCall { name, ok: true, .. } if name == "set_tempo")));
        assert!(events.iter().any(|e| matches!(e, AiEvent::Text(t) if t == "Done.")));
        assert!(!events.iter().any(|e| matches!(e, AiEvent::Error(_))));
        assert!(matches!(events.last(), Some(AiEvent::Done)));
        let last = a.messages.last().unwrap();
        assert_eq!(last["role"], "assistant");
        assert_eq!(last["content"], "Done.");
    }

    #[test]
    fn local_turn_is_one_undo_step() {
        let port = mock_server(Duration::ZERO, vec![
            json!({"choices": [{"message": {"role": "assistant", "content": "", "tool_calls": [
                {"id": "c1", "type": "function", "function": {"name": "set_tempo", "arguments": "{\"bpm\":90}"}},
                {"id": "c2", "type": "function", "function": {"name": "add_track", "arguments": "{\"kind\":\"instrument\"}"}}
            ]}}]}),
            json!({"choices": [{"message": {"role": "assistant", "content": "Done."}}]}),
        ]);
        let session = Session::shared(false);
        let (tx, _rx) = crossbeam_channel::unbounded();
        let mut a = LocalAssistant::default();
        a.run_turn(&session, &local_settings(port), "make a beat", "", &tx, &Arc::new(AtomicBool::new(false)));
        let mut s = session.lock();
        assert_eq!(s.undo_group(), None);
        assert_eq!(s.can_undo(), Some("assistant: make a beat"));
        assert!(s.last_undo_is_ai());
        s.execute(Command::Undo {}, Source::Gui).unwrap();
        assert_eq!(s.project.tempo.initial_bpm(), 120.0);
        assert!(s.project.tracks.is_empty());
        assert_eq!(s.can_undo(), None);
    }

    #[test]
    fn local_turn_edits_a_pattern_and_selected_notes_in_one_undo_step() {
        let session = Session::shared(false);
        let (pattern_id, midi_id) = {
            let mut s = session.lock();
            let mut run = |name: &str, args: Value| s.execute(Command::from_tool_call(name, &args).unwrap(), Source::Gui).unwrap();
            let t = run("add_track", json!({"kind": "instrument"}))["track_id"].as_u64().unwrap();
            let p = run("create_pattern_region", json!({"track_id": t, "start": 0}))["region_id"].as_u64().unwrap();
            let notes = json!([
                {"pitch": 60, "velocity": 100, "start": 0.1, "length": 0.5},
                {"pitch": 62, "velocity": 100, "start": 1.05, "length": 0.5},
                {"pitch": 64, "velocity": 100, "start": 2.2, "length": 0.5}
            ]);
            let m = run("create_midi_region", json!({"track_id": t, "start": 4, "length": 4, "notes": notes}))["region_id"].as_u64().unwrap();
            (p, m)
        };
        let row0 = |s: &Session| match &s.project.find_region(pattern_id).unwrap().1.content {
            RegionContent::Pattern(p) => p.rows[0].steps.clone(),
            _ => panic!("not a pattern"),
        };
        let start_of = |s: &Session, pitch: u8| match &s.project.find_region(midi_id).unwrap().1.content {
            RegionContent::Midi(m) => m.notes.iter().find(|n| n.pitch == pitch).unwrap().start,
            _ => panic!("not a MIDI region"),
        };
        let row_before = row0(&session.lock());
        let port = mock_server(Duration::ZERO, vec![
            json!({"choices": [{"message": {"role": "assistant", "content": "", "tool_calls": [
                {"id": "c1", "type": "function", "function": {"name": "set_pattern_row", "arguments": json!({"region_id": pattern_id, "row": 0, "pattern": "x...x...x...x..."}).to_string()}},
                {"id": "c2", "type": "function", "function": {"name": "quantize_notes", "arguments": json!({"region_id": midi_id, "grid": 1.0, "indices": [0, 2]}).to_string()}}
            ]}}]}),
            json!({"choices": [{"message": {"role": "assistant", "content": "Done."}}]}),
        ]);
        let (tx, rx) = crossbeam_channel::unbounded();
        let mut a = LocalAssistant::default();
        a.run_turn(&session, &local_settings(port), "tighten the groove", "", &tx, &Arc::new(AtomicBool::new(false)));
        let events: Vec<AiEvent> = rx.try_iter().collect();
        for tool in ["set_pattern_row", "quantize_notes"] {
            assert!(events.iter().any(|e| matches!(e, AiEvent::ToolCall { name, ok: true, .. } if name == tool)), "{tool}: {events:?}");
        }
        let mut s = session.lock();
        assert_eq!(row0(&s), vec![115, 0, 0, 0, 115, 0, 0, 0, 115, 0, 0, 0, 115, 0, 0, 0]);
        assert!((start_of(&s, 60) - 0.0).abs() < 1e-9);
        assert!((start_of(&s, 62) - 1.05).abs() < 1e-9, "a note outside indices moved");
        assert!((start_of(&s, 64) - 2.0).abs() < 1e-9);
        assert_eq!(s.can_undo(), Some("assistant: tighten the groove"));
        s.execute(Command::Undo {}, Source::Gui).unwrap();
        assert_eq!(row0(&s), row_before);
        assert!((start_of(&s, 60) - 0.1).abs() < 1e-9);
        assert!((start_of(&s, 64) - 2.2).abs() < 1e-9);
        assert!(s.can_undo().is_some() && !s.last_undo_is_ai(), "one undo must remove the whole request");
    }

    #[test]
    fn local_turn_humanizes_and_snaps_selected_notes() {
        let session = Session::shared(false);
        let midi_id = {
            let mut s = session.lock();
            let mut run = |name: &str, args: Value| s.execute(Command::from_tool_call(name, &args).unwrap(), Source::Gui).unwrap();
            run("set_key", json!({"key": "C major"}));
            let t = run("add_track", json!({"kind": "instrument"}))["track_id"].as_u64().unwrap();
            let notes = json!([
                {"pitch": 61, "velocity": 100, "start": 0.0, "length": 0.5},
                {"pitch": 63, "velocity": 100, "start": 1.0, "length": 0.5},
                {"pitch": 66, "velocity": 100, "start": 2.0, "length": 0.5}
            ]);
            run("create_midi_region", json!({"track_id": t, "start": 0, "length": 4, "notes": notes}))["region_id"].as_u64().unwrap()
        };
        // (pitch, velocity) of the note that starts at `start`
        let note_at = |s: &Session, start: f64| match &s.project.find_region(midi_id).unwrap().1.content {
            RegionContent::Midi(m) => m.notes.iter().find(|n| (n.start - start).abs() < 1e-9).map(|n| (n.pitch, n.velocity)).unwrap(),
            _ => panic!("not a MIDI region"),
        };
        let port = mock_server(Duration::ZERO, vec![
            json!({"choices": [{"message": {"role": "assistant", "content": "", "tool_calls": [
                {"id": "c1", "type": "function", "function": {"name": "snap_notes_to_key", "arguments": json!({"region_id": midi_id, "indices": [0, 2]}).to_string()}},
                {"id": "c2", "type": "function", "function": {"name": "humanize_notes", "arguments": json!({"region_id": midi_id, "indices": [0, 2], "timing": 0.0, "velocity": 10, "seed": 7}).to_string()}}
            ]}}]}),
            json!({"choices": [{"message": {"role": "assistant", "content": "Done."}}]}),
        ]);
        let (tx, rx) = crossbeam_channel::unbounded();
        let mut a = LocalAssistant::default();
        a.run_turn(&session, &local_settings(port), "humanize and snap", "", &tx, &Arc::new(AtomicBool::new(false)));
        let events: Vec<AiEvent> = rx.try_iter().collect();
        for tool in ["snap_notes_to_key", "humanize_notes"] {
            assert!(events.iter().any(|e| matches!(e, AiEvent::ToolCall { name, ok: true, .. } if name == tool)), "{tool}: {events:?}");
        }
        let mut s = session.lock();
        assert_eq!(note_at(&s, 1.0), (63, 100), "a note outside indices changed");
        for start in [0.0, 2.0] {
            let pitch = note_at(&s, start).0;
            assert!([0, 2, 4, 5, 7, 9, 11].contains(&(pitch % 12)), "pitch {pitch} at {start} is not in C major");
        }
        assert_eq!(s.can_undo(), Some("assistant: humanize and snap"));
        s.execute(Command::Undo {}, Source::Gui).unwrap();
        assert_eq!(note_at(&s, 0.0), (61, 100));
        assert_eq!(note_at(&s, 1.0), (63, 100));
        assert_eq!(note_at(&s, 2.0), (66, 100));
    }

    #[test]
    fn local_turn_error_closes_the_undo_group() {
        let server = tiny_http::Server::http("127.0.0.1:0").unwrap();
        let port = server.server_addr().to_ip().unwrap().port();
        std::thread::spawn(move || {
            let mut chats = 0;
            for mut req in server.incoming_requests() {
                if req.url().ends_with("/health") {
                    let _ = req.respond(tiny_http::Response::from_string(json!({"status": "ok"}).to_string()));
                    continue;
                }
                let mut body = String::new();
                let _ = req.as_reader().read_to_string(&mut body);
                chats += 1;
                if chats == 1 {
                    let reply = json!({"choices": [{"message": {"role": "assistant", "content": "", "tool_calls": [{"id": "c1", "type": "function", "function": {"name": "set_tempo", "arguments": "{\"bpm\":90}"}}]}}]});
                    let _ = req.respond(tiny_http::Response::from_string(reply.to_string()));
                } else {
                    let _ = req.respond(tiny_http::Response::from_string("boom").with_status_code(500));
                }
            }
        });
        let session = Session::shared(false);
        let (tx, rx) = crossbeam_channel::unbounded();
        let mut a = LocalAssistant::default();
        a.run_turn(&session, &local_settings(port), "set the tempo", "", &tx, &Arc::new(AtomicBool::new(false)));
        let events: Vec<AiEvent> = rx.try_iter().collect();
        assert!(events.iter().any(|e| matches!(e, AiEvent::Error(_))), "{events:?}");
        assert!(matches!(events.last(), Some(AiEvent::Done)));
        let s = session.lock();
        assert_eq!(s.undo_group(), None);
        assert_eq!(s.can_undo(), Some("assistant: set the tempo"));
        assert!(s.last_undo_is_ai());
        assert_eq!(s.project.tempo.initial_bpm(), 90.0);
    }

    #[test]
    fn stopped_local_turn_closes_the_undo_group() {
        let port = mock_server(Duration::ZERO, vec![json!({"choices": [{"message": {"role": "assistant", "content": "unused"}}]})]);
        let session = Session::shared(false);
        let (tx, rx) = crossbeam_channel::unbounded();
        let mut a = LocalAssistant::default();
        a.run_turn(&session, &local_settings(port), "hello", "", &tx, &Arc::new(AtomicBool::new(true)));
        let events: Vec<AiEvent> = rx.try_iter().collect();
        assert!(events.iter().any(|e| matches!(e, AiEvent::Status(s) if s == "Stopped.")), "{events:?}");
        let s = session.lock();
        assert_eq!(s.undo_group(), None);
        assert_eq!(s.can_undo(), None);
    }

    #[test]
    fn stop_abandons_a_pending_local_request() {
        let port = mock_server(Duration::from_secs(5), vec![json!({"choices": [{"message": {"role": "assistant", "content": "late"}}]})]);
        let session = Session::shared(false);
        let (tx, rx) = crossbeam_channel::unbounded();
        let cancel = Arc::new(AtomicBool::new(false));
        let c = cancel.clone();
        std::thread::spawn(move || {
            std::thread::sleep(Duration::from_millis(200));
            c.store(true, Ordering::Relaxed);
        });
        let mut a = LocalAssistant::default();
        let t0 = Instant::now();
        a.run_turn(&session, &local_settings(port), "hello", "", &tx, &cancel);
        let took = t0.elapsed();
        assert!(took < Duration::from_millis(1500), "Stop took {took:?}");
        let events: Vec<AiEvent> = rx.try_iter().collect();
        assert!(matches!(events.last(), Some(AiEvent::Done)));
        assert!(events.iter().any(|e| matches!(e, AiEvent::Status(s) if s == "Stopped.")));
        assert!(!events.iter().any(|e| matches!(e, AiEvent::Text(t) if t == "late")));
        assert!(!events.iter().any(|e| matches!(e, AiEvent::Error(_))));
        assert_eq!(a.messages.len(), 3);
        assert_eq!(a.messages[0]["role"], "system");
        assert_eq!(a.messages[1]["role"], "user");
        assert_eq!(a.messages[2], json!({"role": "assistant", "content": "(stopped)"}));
    }

    #[test]
    fn shutdown_server_without_a_started_process_does_nothing() {
        shutdown_server();
        shutdown_server();
        assert!(SERVER.lock().is_none());
    }

    fn ping(count: &str) -> Child {
        std::process::Command::new("ping").args(if cfg!(windows) { ["-n", count, "127.0.0.1"] } else { ["-c", count, "127.0.0.1"] }).stdout(Stdio::null()).spawn().unwrap()
    }

    #[test]
    fn stop_child_terminates_a_running_process() {
        let mut c = ping("30");
        assert!(matches!(c.try_wait(), Ok(None)));
        assert!(stop_child(&mut c));
        assert!(matches!(c.try_wait(), Ok(Some(_))));
    }

    #[test]
    fn take_if_exited_clears_a_dead_process_and_keeps_a_live_one() {
        let mut slot: Option<Child> = None;
        assert!(!take_if_exited(&mut slot));
        let mut slot = Some(ping("30"));
        assert!(take_if_exited(&mut slot));
        assert!(slot.is_some());
        assert!(stop_child(slot.as_mut().unwrap()));
        let mut done = ping("1");
        done.wait().unwrap();
        let mut slot = Some(done);
        assert!(!take_if_exited(&mut slot));
        assert!(slot.is_none());
    }
}
