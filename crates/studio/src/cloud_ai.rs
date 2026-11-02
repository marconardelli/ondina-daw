//! Built-in assistant on cloud models other than Claude: OpenAI (Responses API), Google Gemini and Moonshot Kimi
//! (their OpenAI-compatible chat completions endpoints). Same tools, system prompt and undo grouping as the Claude
//! assistant in `ai.rs`; each provider has its own API key and model in Settings.

use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;
use std::time::Duration;

use crossbeam_channel::{RecvTimeoutError, Sender};
use ondina_core::command::Command;
use ondina_core::ops;
use serde_json::{json, Value};

use crate::ai::AiEvent;
use crate::mcp;
use crate::session::{AssistantUndoGroup, SharedSession, Source};
use crate::settings::Settings;

const MAX_STEPS: usize = 60;
/// Characters of a tool result shown in the chat (the model gets all of it).
const SHOWN_RESULT_CHARS: usize = 300;

/// A cloud model provider reached with an API key.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum CloudProvider {
    OpenAi,
    Google,
    Kimi,
}

impl CloudProvider {
    pub const ALL: [CloudProvider; 3] = [CloudProvider::OpenAi, CloudProvider::Google, CloudProvider::Kimi];

    /// The value stored in `Settings::provider`.
    pub fn id(self) -> &'static str {
        match self {
            CloudProvider::OpenAi => "openai",
            CloudProvider::Google => "google",
            CloudProvider::Kimi => "kimi",
        }
    }

    pub fn from_id(id: &str) -> Option<CloudProvider> {
        Self::ALL.into_iter().find(|p| p.id() == id)
    }

    /// Name shown in Settings and in the assistant's status pill.
    pub fn label(self) -> &'static str {
        match self {
            CloudProvider::OpenAi => "OpenAI",
            CloudProvider::Google => "Gemini",
            CloudProvider::Kimi => "Kimi",
        }
    }

    /// Environment variables read for the API key, in order; they take precedence over the key in Settings.
    pub fn key_env(self) -> &'static [&'static str] {
        match self {
            CloudProvider::OpenAi => &["OPENAI_API_KEY"],
            CloudProvider::Google => &["GEMINI_API_KEY", "GOOGLE_API_KEY"],
            CloudProvider::Kimi => &["MOONSHOT_API_KEY", "KIMI_API_KEY"],
        }
    }

    /// Environment variable that replaces the endpoint (proxies, gateways, the China endpoint of Kimi, tests).
    pub fn base_env(self) -> &'static str {
        match self {
            CloudProvider::OpenAi => "OPENAI_BASE_URL",
            CloudProvider::Google => "GEMINI_BASE_URL",
            CloudProvider::Kimi => "MOONSHOT_BASE_URL",
        }
    }

    pub fn default_base(self) -> &'static str {
        match self {
            CloudProvider::OpenAi => "https://api.openai.com/v1",
            CloudProvider::Google => "https://generativelanguage.googleapis.com/v1beta/openai",
            CloudProvider::Kimi => "https://api.moonshot.ai/v1",
        }
    }

    pub fn default_model(self) -> &'static str {
        match self {
            CloudProvider::OpenAi => "gpt-6-astra",
            CloudProvider::Google => "gemini-3.6-flash",
            CloudProvider::Kimi => "kimi-k3.1-pro",
        }
    }

    /// Where to get a key, for the Settings hint.
    pub fn key_page(self) -> &'static str {
        match self {
            CloudProvider::OpenAi => "platform.openai.com",
            CloudProvider::Google => "aistudio.google.com",
            CloudProvider::Kimi => "platform.moonshot.ai",
        }
    }

    /// Endpoint root: the environment override, else the provider's public API.
    pub fn base_url(self) -> String {
        match std::env::var(self.base_env()) {
            Ok(b) if !b.trim().is_empty() => b.trim().trim_end_matches('/').to_string(),
            _ => self.default_base().to_string(),
        }
    }

    /// OpenAI's current models take tools only through the Responses API; Gemini and Kimi use chat completions.
    fn uses_responses_api(self) -> bool {
        self == CloudProvider::OpenAi
    }
}

/// The tool schema in the subset every provider accepts: no numeric `format` hints ("uint64", "double": Gemini
/// rejects the unsigned ones), no `additionalProperties`, `$schema` or `title`, and a single type instead of
/// `["number", "null"]` (optional fields are simply not required).
fn portable_schema(v: &Value) -> Value {
    match v {
        Value::Object(m) => {
            let mut out = serde_json::Map::new();
            for (k, x) in m {
                match k.as_str() {
                    "format" if x.is_string() && !matches!(x.as_str(), Some("date-time") | Some("enum")) => continue,
                    "additionalProperties" | "$schema" | "title" | "definitions" => continue,
                    "type" if x.is_array() => {
                        let first = x.as_array().and_then(|a| a.iter().find(|t| t.as_str() != Some("null")).cloned()).unwrap_or(json!("string"));
                        out.insert(k.clone(), first);
                    }
                    "properties" => {
                        // property names are not keywords: keep every one, clean their schemas
                        let props = x.as_object().map(|p| p.iter().map(|(n, s)| (n.clone(), portable_schema(s))).collect()).unwrap_or_default();
                        out.insert(k.clone(), Value::Object(props));
                    }
                    _ => {
                        out.insert(k.clone(), portable_schema(x));
                    }
                }
            }
            Value::Object(out)
        }
        Value::Array(a) => Value::Array(a.iter().map(portable_schema).collect()),
        _ => v.clone(),
    }
}

/// Every MCP tool in the shape the provider's API expects.
fn tools_for(provider: CloudProvider) -> Value {
    Value::Array(
        mcp::tools()
            .iter()
            .map(|t| {
                let params = portable_schema(&t.input_schema);
                if provider.uses_responses_api() {
                    json!({"type": "function", "name": t.name, "description": t.description, "parameters": params})
                } else {
                    json!({"type": "function", "function": {"name": t.name, "description": t.description, "parameters": params}})
                }
            })
            .collect(),
    )
}

/// One tool call the model asked for.
#[derive(Debug, Clone, PartialEq)]
struct ToolCall {
    id: String,
    name: String,
    args: Value,
}

/// What one model step produced.
#[derive(Debug, Default)]
struct Step {
    texts: Vec<String>,
    calls: Vec<ToolCall>,
}

fn parse_args(raw: Option<&Value>) -> Value {
    match raw {
        Some(Value::String(s)) if s.trim().is_empty() => json!({}),
        Some(Value::String(s)) => serde_json::from_str(s).unwrap_or(json!({})),
        Some(v @ Value::Object(_)) => v.clone(),
        _ => json!({}),
    }
}

/// Reads a chat completions answer. The assistant message goes back into the history exactly as it came, so
/// provider fields the next request needs (Kimi's `reasoning_content`, Gemini's thought signatures in
/// `tool_calls[].extra_content`) are kept.
fn read_chat_answer(resp: &Value) -> Result<(Value, Step), String> {
    let msg = resp.pointer("/choices/0/message").cloned().ok_or_else(|| "the answer has no message".to_string())?;
    let mut stored = msg.clone();
    stored["role"] = json!("assistant");
    if stored.get("content").map(|c| c.is_null()).unwrap_or(true) && stored.get("tool_calls").is_none() {
        stored["content"] = json!("");
    }
    let mut step = Step::default();
    if let Some(t) = msg.get("content").and_then(|c| c.as_str()) {
        if !t.trim().is_empty() {
            step.texts.push(t.to_string());
        }
    }
    for c in msg.get("tool_calls").and_then(|c| c.as_array()).cloned().unwrap_or_default() {
        step.calls.push(ToolCall {
            id: c.get("id").and_then(|x| x.as_str()).unwrap_or("call").to_string(),
            name: c.pointer("/function/name").and_then(|x| x.as_str()).unwrap_or("").to_string(),
            args: parse_args(c.pointer("/function/arguments")),
        });
    }
    Ok((stored, step))
}

/// Reads a Responses API answer. All output items (reasoning, messages, function calls) go back into the input of
/// the next request unchanged.
fn read_responses_answer(resp: &Value) -> Result<(Vec<Value>, Step), String> {
    if let Some(e) = resp.get("error").filter(|e| !e.is_null()) {
        return Err(e.get("message").and_then(|m| m.as_str()).unwrap_or("the request failed").to_string());
    }
    let output = resp.get("output").and_then(|o| o.as_array()).cloned().ok_or_else(|| "the answer has no output".to_string())?;
    let mut step = Step::default();
    for item in &output {
        match item.get("type").and_then(|t| t.as_str()) {
            Some("message") => {
                for part in item.get("content").and_then(|c| c.as_array()).cloned().unwrap_or_default() {
                    match part.get("type").and_then(|t| t.as_str()) {
                        Some("output_text") => {
                            let t = part.get("text").and_then(|t| t.as_str()).unwrap_or("");
                            if !t.trim().is_empty() {
                                step.texts.push(t.to_string());
                            }
                        }
                        Some("refusal") => {
                            let why = part.get("refusal").and_then(|t| t.as_str()).unwrap_or("the request was declined");
                            step.texts.push(why.to_string());
                        }
                        _ => {}
                    }
                }
            }
            Some("function_call") => step.calls.push(ToolCall {
                id: item.get("call_id").and_then(|x| x.as_str()).unwrap_or("call").to_string(),
                name: item.get("name").and_then(|x| x.as_str()).unwrap_or("").to_string(),
                args: parse_args(item.get("arguments")),
            }),
            _ => {}
        }
    }
    Ok((output, step))
}

/// The error text in a provider's error body: `{"error": {"message"}}`, or Gemini's `[{"error": {...}}]`.
fn error_message(body: &str) -> String {
    let v: Option<Value> = serde_json::from_str(body).ok();
    let found = v.as_ref().and_then(|v| {
        v.pointer("/error/message").or_else(|| v.pointer("/0/error/message")).and_then(|m| m.as_str()).map(str::to_string)
    });
    found.unwrap_or_else(|| body.chars().take(500).collect())
}

/// Posts `body` with the key as a bearer token, retrying rate limits and server errors. The request runs on a helper
/// thread so Stop returns at once (Ok(None)); an abandoned request ends on its own.
fn post(url: &str, key: &str, body: &Value, cancel: &AtomicBool) -> Result<Option<Value>, String> {
    let mut delay = 2u64;
    for attempt in 0..4 {
        if cancel.load(Ordering::Relaxed) {
            return Ok(None);
        }
        let (tx, rx) = crossbeam_channel::bounded::<Result<Value, (bool, String)>>(1);
        let (url, key, text) = (url.to_string(), key.to_string(), body.to_string());
        std::thread::Builder::new()
            .name("cloud-ai-request".into())
            .spawn(move || {
                let r = ureq::post(&url)
                    .timeout(Duration::from_secs(600))
                    .set("content-type", "application/json")
                    .set("authorization", &format!("Bearer {key}"))
                    .send_string(&text);
                let out = match r {
                    Ok(resp) => resp
                        .into_string()
                        .map_err(|e| (true, format!("network error: {e}")))
                        .and_then(|t| serde_json::from_str(&t).map_err(|e| (false, format!("invalid API response: {e}")))),
                    Err(ureq::Error::Status(code, resp)) => {
                        let msg = error_message(&resp.into_string().unwrap_or_default());
                        Err((code == 429 || code >= 500, format!("API error {code}: {msg}")))
                    }
                    Err(e) => Err((true, format!("network error: {e}"))),
                };
                let _ = tx.send(out);
            })
            .map_err(|e| format!("cannot start the request thread: {e}"))?;
        let result = loop {
            if cancel.load(Ordering::Relaxed) {
                return Ok(None);
            }
            match rx.recv_timeout(Duration::from_millis(100)) {
                Ok(r) => break r,
                Err(RecvTimeoutError::Timeout) => continue,
                Err(RecvTimeoutError::Disconnected) => break Err((false, "the request ended unexpectedly".to_string())),
            }
        };
        match result {
            Ok(v) => return Ok(Some(v)),
            Err((true, _)) if attempt < 3 => {
                for _ in 0..delay * 10 {
                    if cancel.load(Ordering::Relaxed) {
                        return Ok(None);
                    }
                    std::thread::sleep(Duration::from_millis(100));
                }
                delay *= 2;
            }
            Err((_, e)) => return Err(e),
        }
    }
    Err("API unreachable".into())
}

/// Conversation with a cloud provider. The history is in that provider's format (chat messages, or Responses input
/// items for OpenAI), so changing provider starts a new conversation.
#[derive(Default)]
pub struct CloudAssistant {
    provider: Option<CloudProvider>,
    history: Vec<Value>,
}

impl CloudAssistant {
    pub fn clear(&mut self) {
        self.history.clear();
    }

    /// Messages or input items so far (tests and diagnostics).
    pub fn history(&self) -> &[Value] {
        &self.history
    }

    /// Run one user turn to completion (blocking) with the provider chosen in `settings`. Emits events as it goes.
    pub fn run_turn(&mut self, session: &SharedSession, settings: &Settings, user_text: &str, gui_context: &str, events: &Sender<AiEvent>, cancel: &Arc<AtomicBool>) {
        let finish = |e: Option<String>| {
            if let Some(e) = e {
                let _ = events.send(AiEvent::Error(e));
            }
            let _ = events.send(AiEvent::Done);
        };
        let Some(provider) = settings.cloud_provider() else {
            return finish(Some("No cloud provider is selected in Settings.".into()));
        };
        let Some(key) = settings.cloud_key(provider) else {
            let env = provider.key_env().join(" or ");
            return finish(Some(format!("No {} API key. Set {env} or enter a key in Settings.", provider.label())));
        };
        self.run_turn_at(&provider.base_url(), provider, &key, session, settings, user_text, gui_context, events, cancel);
    }

    #[allow(clippy::too_many_arguments)]
    fn run_turn_at(&mut self, base: &str, provider: CloudProvider, key: &str, session: &SharedSession, settings: &Settings, user_text: &str, gui_context: &str, events: &Sender<AiEvent>, cancel: &Arc<AtomicBool>) {
        if self.provider != Some(provider) {
            self.history.clear();
            self.provider = Some(provider);
        }
        let summary = {
            let s = session.lock();
            let mut v = ops::project_summary(&s.project);
            v["transport"] = s.transport_json();
            v
        };
        let text = format!("{user_text}\n\n<ondina_context>\n{gui_context}\nProject state: {}\n</ondina_context>", serde_json::to_string(&summary).unwrap_or_default());
        let responses = provider.uses_responses_api();
        let turn_start = self.history.len();
        self.history.push(if responses {
            json!({"role": "user", "content": [{"type": "input_text", "text": text}]})
        } else {
            json!({"role": "user", "content": text})
        });
        let undo_group = AssistantUndoGroup::open(session, crate::ai::undo_label(user_text));
        let tools = tools_for(provider);
        let model = settings.cloud_model(provider);
        let url = if responses { format!("{base}/responses") } else { format!("{base}/chat/completions") };
        let thinking = format!("Thinking ({})…", provider.label());
        let mut stopped = false;
        for _ in 0..MAX_STEPS {
            if cancel.load(Ordering::Relaxed) {
                stopped = true;
                break;
            }
            let _ = events.send(AiEvent::Status(thinking.clone()));
            let body = if responses {
                json!({"model": model, "instructions": crate::ai::SYSTEM_PROMPT, "input": self.history, "tools": tools, "tool_choice": "auto"})
            } else {
                let mut messages = vec![json!({"role": "system", "content": crate::ai::SYSTEM_PROMPT})];
                messages.extend(self.history.iter().cloned());
                json!({"model": model, "messages": messages, "tools": tools, "tool_choice": "auto"})
            };
            let resp = match post(&url, key, &body, cancel) {
                Ok(Some(v)) => v,
                Ok(None) => {
                    stopped = true;
                    break;
                }
                Err(e) => {
                    let _ = events.send(AiEvent::Error(e));
                    // the provider never saw an answer to this request: leave the history as it was before it
                    if self.history.len() == turn_start + 1 {
                        self.history.truncate(turn_start);
                    }
                    break;
                }
            };
            let step = if responses {
                read_responses_answer(&resp).map(|(items, step)| {
                    self.history.extend(items);
                    step
                })
            } else {
                read_chat_answer(&resp).map(|(msg, step)| {
                    self.history.push(msg);
                    step
                })
            };
            let step = match step {
                Ok(s) => s,
                Err(e) => {
                    let _ = events.send(AiEvent::Error(e));
                    break;
                }
            };
            for t in step.texts {
                let _ = events.send(AiEvent::Text(t));
            }
            if step.calls.is_empty() {
                break;
            }
            for c in step.calls {
                let out = if cancel.load(Ordering::Relaxed) {
                    // every call still gets its answer, but nothing more runs after Stop
                    "Error: stopped by the user".to_string()
                } else {
                    let (ok, out) = match Command::from_tool_call(&c.name, &c.args) {
                        Ok(cmd) => match session.lock().execute(cmd, Source::Ai) {
                            Ok(v) => (true, serde_json::to_string(&v).unwrap_or_default()),
                            Err(e) => (false, format!("Error: {e}")),
                        },
                        Err(e) => (false, format!("Error: {e}")),
                    };
                    let shown: String = out.chars().take(SHOWN_RESULT_CHARS).collect();
                    let _ = events.send(AiEvent::ToolCall { name: c.name.clone(), input: c.args.clone(), ok, result: shown });
                    out
                };
                self.history.push(if responses {
                    json!({"type": "function_call_output", "call_id": c.id, "output": out})
                } else {
                    json!({"role": "tool", "tool_call_id": c.id, "content": out})
                });
            }
        }
        if stopped {
            let _ = events.send(AiEvent::Status("Stopped.".into()));
        }
        // chat templates expect an assistant message before the next user text
        if !responses && self.history.len() > turn_start && self.history.last().map(|m| m["role"] != "assistant").unwrap_or(false) {
            self.history.push(json!({"role": "assistant", "content": "(stopped)"}));
        }
        // closed before Done, so the GUI already sees the finished step
        drop(undo_group);
        let _ = events.send(AiEvent::Done);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::session::Session;

    #[test]
    fn provider_ids_round_trip() {
        for p in CloudProvider::ALL {
            assert_eq!(CloudProvider::from_id(p.id()), Some(p));
            assert!(!p.default_model().is_empty());
            assert!(p.default_base().starts_with("https://"));
        }
        assert_eq!(CloudProvider::from_id("anthropic"), None);
        assert_eq!(CloudProvider::from_id("local"), None);
    }

    /// Keywords a portable schema may use.
    const PORTABLE: &[&str] = &["type", "properties", "required", "items", "enum", "description", "minimum", "maximum", "default", "anyOf", "oneOf", "const", "nullable", "minItems", "maxItems"];

    fn check_keywords(v: &Value, path: &str, bad: &mut Vec<String>) {
        match v {
            Value::Object(m) => {
                for (k, x) in m {
                    if k == "properties" {
                        for (n, s) in x.as_object().unwrap() {
                            check_keywords(s, &format!("{path}.{n}"), bad);
                        }
                        continue;
                    }
                    if !PORTABLE.contains(&k.as_str()) {
                        bad.push(format!("{path}: {k}"));
                    }
                    if k == "type" {
                        assert!(x.is_string(), "{path}: type must be a single string, got {x}");
                    }
                    check_keywords(x, path, bad);
                }
            }
            Value::Array(a) => a.iter().for_each(|x| check_keywords(x, path, bad)),
            _ => {}
        }
    }

    #[test]
    fn every_tool_schema_uses_only_portable_keywords() {
        let mut bad = vec![];
        for t in mcp::tools() {
            check_keywords(&portable_schema(&t.input_schema), &t.name, &mut bad);
        }
        bad.sort();
        bad.dedup();
        assert!(bad.is_empty(), "keywords outside the portable subset: {bad:?}");
    }

    #[test]
    fn portable_schema_keeps_property_names_and_drops_null_types() {
        let s = json!({"type": "object", "additionalProperties": false, "properties": {"title": {"type": ["number", "null"], "format": "double"}, "format": {"type": "string"}}, "required": ["format"]});
        let p = portable_schema(&s);
        assert_eq!(p, json!({"type": "object", "properties": {"title": {"type": "number"}, "format": {"type": "string"}}, "required": ["format"]}));
    }

    #[test]
    fn tools_have_the_shape_of_each_api() {
        let r = tools_for(CloudProvider::OpenAi);
        assert_eq!(r[0]["type"], "function");
        assert!(r[0]["name"].is_string() && r[0]["parameters"].is_object());
        let c = tools_for(CloudProvider::Kimi);
        assert!(c[0]["function"]["name"].is_string() && c[0]["function"]["parameters"].is_object());
        assert_eq!(r.as_array().unwrap().len(), mcp::tools().len());
    }

    #[test]
    fn chat_answer_keeps_provider_fields() {
        let resp = json!({"choices": [{"message": {"role": "assistant", "content": null, "reasoning_content": "hmm",
            "tool_calls": [{"id": "c1", "type": "function", "function": {"name": "set_tempo", "arguments": "{\"bpm\":90}"}, "extra_content": {"google": {"thought_signature": "sig"}}}]}}]});
        let (stored, step) = read_chat_answer(&resp).unwrap();
        assert_eq!(stored["reasoning_content"], "hmm");
        assert_eq!(stored["tool_calls"][0]["extra_content"]["google"]["thought_signature"], "sig");
        assert_eq!(step.calls, vec![ToolCall { id: "c1".into(), name: "set_tempo".into(), args: json!({"bpm": 90}) }]);
        assert!(step.texts.is_empty());
        let (plain, _) = read_chat_answer(&json!({"choices": [{"message": {"role": "assistant", "content": null}}]})).unwrap();
        assert_eq!(plain["content"], "");
    }

    #[test]
    fn responses_answer_reads_text_and_calls() {
        let resp = json!({"output": [
            {"type": "reasoning", "id": "rs_1", "summary": []},
            {"type": "message", "role": "assistant", "content": [{"type": "output_text", "text": "Setting the tempo."}]},
            {"type": "function_call", "id": "fc_1", "call_id": "call_1", "name": "set_tempo", "arguments": "{\"bpm\":100}"}
        ]});
        let (items, step) = read_responses_answer(&resp).unwrap();
        assert_eq!(items.len(), 3);
        assert_eq!(step.texts, vec!["Setting the tempo.".to_string()]);
        assert_eq!(step.calls[0].id, "call_1");
        assert_eq!(step.calls[0].args, json!({"bpm": 100}));
        assert!(read_responses_answer(&json!({"error": {"message": "bad model"}, "output": []})).is_err());
    }

    #[test]
    fn error_message_reads_both_shapes() {
        assert_eq!(error_message(r#"{"error":{"message":"Invalid key"}}"#), "Invalid key");
        assert_eq!(error_message(r#"[{"error":{"code":400,"message":"Bad schema"}}]"#), "Bad schema");
        assert_eq!(error_message("plain"), "plain");
    }

    /// Serves `answers` in order on a free port and returns the request bodies and authorization headers.
    fn mock(answers: Vec<(u16, Value)>) -> (String, std::thread::JoinHandle<Vec<(String, Value)>>) {
        let server = tiny_http::Server::http("127.0.0.1:0").unwrap();
        let port = server.server_addr().to_ip().unwrap().port();
        let h = std::thread::spawn(move || {
            let mut seen = vec![];
            for (status, answer) in answers {
                let mut req = server.recv().unwrap();
                let mut b = String::new();
                req.as_reader().read_to_string(&mut b).unwrap();
                let auth = req.headers().iter().find(|h| h.field.equiv("authorization")).map(|h| h.value.to_string()).unwrap_or_default();
                seen.push((format!("{} {auth}", req.url()), serde_json::from_str(&b).unwrap()));
                req.respond(tiny_http::Response::from_string(answer.to_string()).with_status_code(status)).unwrap();
            }
            seen
        });
        (format!("http://127.0.0.1:{port}/v1"), h)
    }

    fn run(provider: CloudProvider, base: &str, a: &mut CloudAssistant, session: &SharedSession, text: &str) -> Vec<AiEvent> {
        let (tx, rx) = crossbeam_channel::unbounded();
        let settings = Settings { provider: provider.id().into(), ..Default::default() };
        a.run_turn_at(base, provider, "k-test", session, &settings, text, "", &tx, &Arc::new(AtomicBool::new(false)));
        rx.try_iter().collect()
    }

    #[test]
    fn chat_loop_runs_tools_and_replays_the_history() {
        let (base, h) = mock(vec![
            (200, json!({"choices": [{"message": {"role": "assistant", "content": "", "reasoning_content": "plan",
                "tool_calls": [{"id": "c1", "type": "function", "function": {"name": "set_tempo", "arguments": "{\"bpm\":97}"}}]}}]})),
            (200, json!({"choices": [{"message": {"role": "assistant", "content": "Tempo set."}}]})),
        ]);
        let session = Session::shared(false);
        let mut a = CloudAssistant::default();
        let events = run(CloudProvider::Kimi, &base, &mut a, &session, "tempo 97");
        let seen = h.join().unwrap();
        assert_eq!(seen[0].0, "/v1/chat/completions Bearer k-test");
        assert_eq!(seen[0].1["model"], CloudProvider::Kimi.default_model());
        assert_eq!(seen[0].1["messages"][0]["role"], "system");
        let second = seen[1].1["messages"].as_array().unwrap();
        assert_eq!(second[2]["reasoning_content"], "plan", "the assistant message is sent back unchanged");
        assert_eq!(second[3]["role"], "tool");
        assert_eq!(second[3]["tool_call_id"], "c1");
        assert!((session.lock().project.tempo.initial_bpm() - 97.0).abs() < 1e-9);
        assert!(events.iter().any(|e| matches!(e, AiEvent::Text(t) if t == "Tempo set.")));
        assert!(matches!(events.last(), Some(AiEvent::Done)));
        assert_eq!(a.history().last().unwrap()["content"], "Tempo set.");
    }

    #[test]
    fn responses_loop_sends_function_call_outputs() {
        let (base, h) = mock(vec![
            (200, json!({"output": [{"type": "reasoning", "id": "rs_1", "summary": []}, {"type": "function_call", "id": "fc_1", "call_id": "call_1", "name": "set_tempo", "arguments": "{\"bpm\":101}"}]})),
            (200, json!({"output": [{"type": "message", "role": "assistant", "content": [{"type": "output_text", "text": "Done."}]}]})),
        ]);
        let session = Session::shared(false);
        let mut a = CloudAssistant::default();
        let events = run(CloudProvider::OpenAi, &base, &mut a, &session, "tempo 101");
        let seen = h.join().unwrap();
        assert_eq!(seen[0].0, "/v1/responses Bearer k-test");
        assert!(seen[0].1["instructions"].is_string());
        let input = seen[1].1["input"].as_array().unwrap();
        assert_eq!(input[1]["type"], "reasoning");
        assert_eq!(input[2]["type"], "function_call");
        assert_eq!(input[3]["type"], "function_call_output");
        assert_eq!(input[3]["call_id"], "call_1");
        assert!(input[3]["output"].is_string());
        assert!((session.lock().project.tempo.initial_bpm() - 101.0).abs() < 1e-9);
        assert!(events.iter().any(|e| matches!(e, AiEvent::Text(t) if t == "Done.")));
    }

    #[test]
    fn an_api_error_leaves_the_history_as_before_and_switching_provider_starts_over() {
        let (base, h) = mock(vec![(401, json!({"error": {"message": "Invalid key"}}))]);
        let session = Session::shared(false);
        let mut a = CloudAssistant::default();
        let events = run(CloudProvider::Google, &base, &mut a, &session, "hi");
        h.join().unwrap();
        assert!(events.iter().any(|e| matches!(e, AiEvent::Error(m) if m == "API error 401: Invalid key")), "{events:?}");
        assert!(a.history().is_empty());
        let (base, h) = mock(vec![(200, json!({"choices": [{"message": {"role": "assistant", "content": "Hello"}}]}))]);
        run(CloudProvider::Google, &base, &mut a, &session, "hi");
        h.join().unwrap();
        assert_eq!(a.history().len(), 2);
        let (base, h) = mock(vec![(200, json!({"output": [{"type": "message", "content": [{"type": "output_text", "text": "Hi"}]}]}))]);
        run(CloudProvider::OpenAi, &base, &mut a, &session, "hi");
        let seen = h.join().unwrap();
        assert_eq!(seen[0].1["input"].as_array().unwrap().len(), 1, "the Gemini conversation is not sent to OpenAI");
    }

    #[test]
    fn a_missing_key_is_reported() {
        let (tx, rx) = crossbeam_channel::unbounded();
        let settings = Settings { provider: "kimi".into(), ..Default::default() };
        if settings.cloud_key(CloudProvider::Kimi).is_some() {
            return; // a key in the environment
        }
        CloudAssistant::default().run_turn(&Session::shared(false), &settings, "hi", "", &tx, &Arc::new(AtomicBool::new(false)));
        let events: Vec<AiEvent> = rx.try_iter().collect();
        assert!(matches!(&events[0], AiEvent::Error(m) if m.starts_with("No Kimi API key")), "{events:?}");
        assert!(matches!(events.last(), Some(AiEvent::Done)));
    }
}
