//! Built-in AI assistant: a Claude tool-use loop over the same tools the MCP server exposes.
//! Runs on a background thread and reports progress through a channel.

use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;
use std::time::Duration;

use ondina_core::command::Command;
use ondina_core::ops;
use crossbeam_channel::Sender;
use serde_json::{json, Value};

use crate::mcp;
use crate::session::{AssistantUndoGroup, SharedSession, Source};
use crate::settings::Settings;

pub const API_URL: &str = "https://api.anthropic.com/v1/messages";
pub const DEFAULT_MODEL: &str = "claude-opus-5-5";
const MAX_STEPS: usize = 60;

#[derive(Debug, Clone)]
pub enum AiEvent {
    /// Assistant prose.
    Text(String),
    /// A tool was executed.
    ToolCall { name: String, input: Value, ok: bool, result: String },
    Status(String),
    Error(String),
    /// The turn has finished.
    Done,
}

const SYSTEM_PROMPT: &str = r#"You are the music production assistant built into Ondina DAW, a digital audio workstation for Windows. You operate the DAW directly through tools; every tool is an undoable command identical to what the user can do in the GUI.

How to work:
- Start a task by calling get_project unless the context already gives you the ids you need. Never guess ids.
- Positions and lengths are in beats (quarter notes). Bar N starts at beat (N-1) * beats_per_bar. Note starts are relative to their region.
- MIDI pitch 60 = C4. Drum tracks use the General MIDI map (36 kick, 38 snare, 39 clap, 42 closed hat, 46 open hat, 49 crash, 51 ride).
- track_id 0 is the master channel.
- For new parts prefer library patches (list_patches) and the session-player generators (generate_drums, generate_bass, generate_chords, generate_arpeggio, generate_melody) driven by the chord track (set_chord_progression). Write exact notes with create_midi_region/add_notes when the user asks for a specific melody, riff or voicing.
- Arrange with add_marker (length = section), copy_section, duplicate_region. Mix with set_volume, set_pan, add_effect, buses with add_send, and automation.
- To judge a mix, call analyze_mix (your ears): it reports LUFS, peaks, spectral balance, stereo correlation and advice. Use auto_master for mastering.
- Batch independent tool calls in parallel when possible.
- Ask before destructive operations the user did not clearly request (new_project, remove_track, delete_section on existing material).
- Everything you change for one user request is a single undo step for the user. Calling undo reverts all your edits so far in this request, so fix a single mistake with a targeted edit (remove_notes, delete_region, set_volume...) instead.
- Keep replies short: say what you did and what the user can try next. Reply in the user's language."#;

pub struct Assistant {
    pub messages: Vec<Value>,
    tools: Value,
}

impl Default for Assistant {
    fn default() -> Self {
        Self::new()
    }
}

impl Assistant {
    pub fn new() -> Self {
        let tools: Vec<Value> = mcp::tools().iter().map(|t| json!({"name": t.name, "description": t.description, "input_schema": t.input_schema})).collect();
        Assistant { messages: vec![], tools: Value::Array(tools) }
    }

    pub fn clear(&mut self) {
        self.messages.clear();
    }

    /// Run one user turn to completion (blocking). Emits events as it goes.
    pub fn run_turn(&mut self, session: &SharedSession, settings: &Settings, user_text: &str, gui_context: &str, events: &Sender<AiEvent>, cancel: &Arc<AtomicBool>) {
        let Some(key) = credential(settings) else {
            let _ = events.send(AiEvent::Error("No Anthropic credentials. Set ANTHROPIC_API_KEY (or ANTHROPIC_AUTH_TOKEN) or enter a key in Settings.".into()));
            let _ = events.send(AiEvent::Done);
            return;
        };
        let summary = {
            let s = session.lock();
            let mut v = ops::project_summary(&s.project);
            v["transport"] = s.transport_json();
            v
        };
        let text = format!("{user_text}\n\n<ondina_context>\n{gui_context}\nProject state: {}\n</ondina_context>", serde_json::to_string(&summary).unwrap_or_default());
        self.messages.push(json!({"role": "user", "content": [{"type": "text", "text": text}]}));
        let undo_group = AssistantUndoGroup::open(session, undo_label(user_text));

        for _step in 0..MAX_STEPS {
            if cancel.load(Ordering::Relaxed) {
                let _ = events.send(AiEvent::Status("Stopped.".into()));
                break;
            }
            let _ = events.send(AiEvent::Status("Thinking…".into()));
            let body = json!({
                "model": settings.model(),
                "max_tokens": 16000,
                "system": SYSTEM_PROMPT,
                "tools": self.tools,
                "messages": self.messages,
                "thinking": {"type": "adaptive"},
                "output_config": {"effort": settings.effort()},
                "fallbacks": "default",
                "cache_control": {"type": "ephemeral"},
            });
            let resp = match post_with_retry(&key, &body, cancel) {
                Ok(r) => r,
                Err(e) => {
                    let _ = events.send(AiEvent::Error(e));
                    // drop the dangling user message so the conversation stays valid
                    if self.messages.last().map(|m| m["role"] == "user").unwrap_or(false) {
                        self.messages.pop();
                    }
                    break;
                }
            };
            let content = resp.get("content").cloned().unwrap_or(json!([]));
            let stop = resp.get("stop_reason").and_then(|s| s.as_str()).unwrap_or("").to_string();
            // keep the full assistant content (thinking blocks included) unchanged
            self.messages.push(json!({"role": "assistant", "content": content.clone()}));
            let mut tool_results = vec![];
            for block in content.as_array().cloned().unwrap_or_default() {
                match block.get("type").and_then(|t| t.as_str()) {
                    Some("text") => {
                        let t = block["text"].as_str().unwrap_or("").to_string();
                        if !t.trim().is_empty() {
                            let _ = events.send(AiEvent::Text(t));
                        }
                    }
                    Some("tool_use") => {
                        let id = block["id"].as_str().unwrap_or("").to_string();
                        let name = block["name"].as_str().unwrap_or("").to_string();
                        let input = block.get("input").cloned().unwrap_or(json!({}));
                        let (ok, out) = match Command::from_tool_call(&name, &input) {
                            Ok(cmd) => match session.lock().execute(cmd, Source::Ai) {
                                Ok(v) => (true, serde_json::to_string(&v).unwrap_or_default()),
                                Err(e) => (false, e),
                            },
                            Err(e) => (false, e),
                        };
                        let _ = events.send(AiEvent::ToolCall { name: name.clone(), input: input.clone(), ok, result: short(&out, 300) });
                        tool_results.push(json!({"type": "tool_result", "tool_use_id": id, "content": out, "is_error": !ok}));
                    }
                    _ => {}
                }
            }
            match stop.as_str() {
                "tool_use" => {
                    // all results in one user message
                    self.messages.push(json!({"role": "user", "content": tool_results}));
                }
                "refusal" => {
                    let why = resp.pointer("/stop_details/explanation").and_then(|x| x.as_str()).unwrap_or("the request was declined");
                    let _ = events.send(AiEvent::Error(format!("Claude declined: {why}")));
                    break;
                }
                "max_tokens" => {
                    if !tool_results.is_empty() {
                        self.messages.push(json!({"role": "user", "content": tool_results}));
                    }
                    let _ = events.send(AiEvent::Error("Response truncated (max tokens).".into()));
                    break;
                }
                _ => {
                    if !tool_results.is_empty() {
                        self.messages.push(json!({"role": "user", "content": tool_results}));
                        continue;
                    }
                    break;
                }
            }
        }
        // a trailing tool_result user message without a following assistant turn would be fine,
        // but a conversation must end with an assistant message before the next user text:
        if self.messages.last().map(|m| m["role"] == "user").unwrap_or(false) {
            self.messages.push(json!({"role": "assistant", "content": [{"type": "text", "text": "(stopped)"}]}));
        }
        // closed before Done, so the GUI already sees the finished step
        drop(undo_group);
        let _ = events.send(AiEvent::Done);
    }
}

/// Longest part of the request shown in the undo label.
const UNDO_LABEL_CHARS: usize = 40;

/// Undo label of one assistant request: "assistant: " and the start of its first non-empty line.
pub fn undo_label(request: &str) -> String {
    let line = request.lines().map(str::trim).find(|l| !l.is_empty()).unwrap_or("");
    if line.is_empty() {
        return "assistant".into();
    }
    let head: String = line.chars().take(UNDO_LABEL_CHARS).collect();
    let more = if line.chars().count() > UNDO_LABEL_CHARS { "…" } else { "" };
    format!("assistant: {}{more}", head.trim_end())
}

fn short(s: &str, n: usize) -> String {
    if s.chars().count() > n {
        format!("{}…", s.chars().take(n).collect::<String>())
    } else {
        s.to_string()
    }
}

/// How requests authenticate.
#[derive(Debug, Clone)]
pub enum Credential {
    ApiKey(String),
    Bearer(String),
}

pub fn credential(settings: &Settings) -> Option<Credential> {
    if let Some(k) = settings.api_key() {
        return Some(Credential::ApiKey(k));
    }
    std::env::var("ANTHROPIC_AUTH_TOKEN").ok().filter(|t| !t.trim().is_empty()).map(Credential::Bearer)
}

/// Messages endpoint, honoring ANTHROPIC_BASE_URL (proxies, gateways, tests).
pub fn api_url() -> String {
    match std::env::var("ANTHROPIC_BASE_URL") {
        Ok(b) if !b.trim().is_empty() => format!("{}/v1/messages", b.trim_end_matches('/')),
        _ => API_URL.to_string(),
    }
}

fn post_with_retry(cred: &Credential, body: &Value, cancel: &Arc<AtomicBool>) -> Result<Value, String> {
    let mut delay = 2u64;
    for attempt in 0..4 {
        if cancel.load(Ordering::Relaxed) {
            return Err("cancelled".into());
        }
        let mut req = ureq::post(&api_url()).timeout(Duration::from_secs(600)).set("anthropic-version", "2023-06-01").set("content-type", "application/json");
        req = match cred {
            Credential::ApiKey(k) => req.set("x-api-key", k).set("anthropic-beta", "server-side-fallback-2026-07-01"),
            Credential::Bearer(t) => req.set("Authorization", &format!("Bearer {t}")).set("anthropic-beta", "server-side-fallback-2026-07-01,oauth-2025-04-20"),
        };
        let r = req.send_string(&body.to_string());
        match r {
            Ok(resp) => {
                let text = resp.into_string().map_err(|e| e.to_string())?;
                return serde_json::from_str(&text).map_err(|e| format!("invalid API response: {e}"));
            }
            Err(ureq::Error::Status(code, resp)) => {
                let text = resp.into_string().unwrap_or_default();
                let msg = serde_json::from_str::<Value>(&text).ok().and_then(|v| v.pointer("/error/message").and_then(|m| m.as_str()).map(|s| s.to_string())).unwrap_or(text);
                let retryable = code == 429 || code == 529 || code >= 500;
                if retryable && attempt < 3 {
                    std::thread::sleep(Duration::from_secs(delay));
                    delay *= 2;
                    continue;
                }
                return Err(format!("API error {code}: {msg}"));
            }
            Err(e) => {
                if attempt < 3 {
                    std::thread::sleep(Duration::from_secs(delay));
                    delay *= 2;
                    continue;
                }
                return Err(format!("network error: {e}"));
            }
        }
    }
    Err("API unreachable".into())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::session::Session;

    #[test]
    fn undo_label_takes_the_start_of_the_first_line() {
        assert_eq!(undo_label("Add drums"), "assistant: Add drums");
        assert_eq!(undo_label("  \n  make it louder \nand brighter"), "assistant: make it louder");
        assert_eq!(undo_label(""), "assistant");
        assert_eq!(undo_label("   \n "), "assistant");
        let long = undo_label(&"è".repeat(100));
        assert!(long.ends_with('…'), "{long}");
        assert_eq!(long.chars().count(), "assistant: ".len() + 40 + 1);
        let exact = "a".repeat(40);
        assert_eq!(undo_label(&exact), format!("assistant: {exact}"));
    }

    /// Runs the agent loop against a local mock of the Messages API.
    #[test]
    fn agent_loop_executes_tools_and_replays_content() {
        let server = tiny_http::Server::http("127.0.0.1:39172").unwrap();
        let handle = std::thread::spawn(move || {
            let mut bodies = vec![];
            for (i, mut req) in server.incoming_requests().enumerate() {
                let mut b = String::new();
                req.as_reader().read_to_string(&mut b).unwrap();
                let v: Value = serde_json::from_str(&b).unwrap();
                bodies.push(v);
                let resp = if i == 0 {
                    json!({"id": "m1", "type": "message", "role": "assistant", "stop_reason": "tool_use", "content": [
                        {"type": "thinking", "thinking": "", "signature": "sig"},
                        {"type": "text", "text": "Adding drums."},
                        {"type": "tool_use", "id": "toolu_1", "name": "add_track", "input": {"kind": "instrument", "patch": "Pop Drum Kit"}},
                        {"type": "tool_use", "id": "toolu_2", "name": "set_tempo", "input": {"bpm": 90}}
                    ]})
                } else {
                    json!({"id": "m2", "type": "message", "role": "assistant", "stop_reason": "end_turn", "content": [{"type": "text", "text": "Done."}]})
                };
                req.respond(tiny_http::Response::from_string(resp.to_string())).unwrap();
                if i == 1 {
                    break;
                }
            }
            bodies
        });
        std::env::set_var("ANTHROPIC_BASE_URL", "http://127.0.0.1:39172");
        let session = Session::shared(false);
        let settings = Settings { api_key: Some("test-key".into()), ..Default::default() };
        let (tx, rx) = crossbeam_channel::unbounded();
        let mut a = Assistant::new();
        a.run_turn(&session, &settings, "add drums", "ctx", &tx, &Arc::new(AtomicBool::new(false)));
        let bodies = handle.join().unwrap();
        std::env::remove_var("ANTHROPIC_BASE_URL");
        // first request shape
        let first = &bodies[0];
        assert_eq!(first["model"], DEFAULT_MODEL);
        assert_eq!(first["thinking"]["type"], "adaptive");
        assert_eq!(first["fallbacks"], "default");
        assert!(first["tools"].as_array().unwrap().len() > 80);
        assert!(first.get("temperature").is_none());
        // second request: assistant content replayed unchanged + both tool results in ONE user message
        let msgs = bodies[1]["messages"].as_array().unwrap();
        assert_eq!(msgs[1]["content"][0]["type"], "thinking");
        assert_eq!(msgs[1]["content"][0]["signature"], "sig");
        let results = msgs[2]["content"].as_array().unwrap();
        assert_eq!(results.len(), 2);
        assert_eq!(results[0]["tool_use_id"], "toolu_1");
        assert_eq!(results[0]["is_error"], false);
        // effects on the session
        let s = session.lock();
        assert_eq!(s.project.tracks.len(), 1);
        assert_eq!(s.project.tempo.initial_bpm(), 90.0);
        // the whole request is one undo step, and the group is closed when the turn ends
        assert_eq!(s.undo_group(), None);
        assert_eq!(s.can_undo(), Some("assistant: add drums"));
        assert!(s.last_undo_is_ai());
        drop(s);
        let events: Vec<AiEvent> = rx.try_iter().collect();
        assert!(events.iter().any(|e| matches!(e, AiEvent::ToolCall { name, ok: true, .. } if name == "add_track")));
        assert!(events.iter().any(|e| matches!(e, AiEvent::Text(t) if t == "Done.")));
        assert!(matches!(events.last(), Some(AiEvent::Done)));
    }
}
