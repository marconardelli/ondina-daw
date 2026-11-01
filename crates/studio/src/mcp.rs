//! Model Context Protocol server.
//!
//! Every [`Command`] is published as an MCP tool (generated from the command schema), so an
//! MCP client can do everything the GUI can. Transports:
//! * Streamable HTTP (JSON responses) on `http://127.0.0.1:<port>/mcp` — controls the running app
//! * stdio (newline-delimited JSON-RPC) — headless sessions
//! * stdio→HTTP bridge — lets stdio-only clients drive the running app

use std::io::{BufRead, Write};
use std::sync::OnceLock;

use ondina_core::command::{tool_specs, Command, ToolSpec};
use ondina_core::ops;
use ondina_core::plugins;
use serde_json::{json, Value};

use crate::session::{SharedSession, Source};

pub const PROTOCOL_VERSIONS: &[&str] = &["2025-11-25", "2025-06-18", "2025-03-26", "2024-11-05"];
pub const SERVER_NAME: &str = "ondina-daw";

pub fn tools() -> &'static [ToolSpec] {
    static T: OnceLock<Vec<ToolSpec>> = OnceLock::new();
    T.get_or_init(tool_specs)
}

pub const GUIDE: &str = r#"# Ondina DAW — guide for AI agents

Ondina is a digital audio workstation. Everything is controlled through tools that mirror the app's command API.

## Conventions
- Time is in **beats** (quarter notes). Beat 0 = start of bar 1. In 4/4 a bar is 4 beats: bar N starts at beat (N-1)*4.
- Note positions inside a region are relative to the region start.
- MIDI pitch 60 = C4 (middle C). Drums use the General MIDI map: 36 kick, 38 snare, 39 clap, 42 closed hat, 46 open hat, 49 crash, 51 ride, 45/47/50 toms.
- track_id 0 addresses the master channel (volume, pan, effects).
- Every edit is undoable with `undo`.
- `get_selection` tells you what the user selected in the Ondina window (track, region, notes with their index, playhead, cycle). When the user says "these notes" or "this region", call it first and pass the note `index` values as `indices` to `quantize_notes`, `transpose_notes`, `humanize_notes`, `set_note_velocities`, `snap_notes_to_key` or `remove_notes`. Read it again after editing: indices change when notes move.
- `get_activity` tells you what happened since you last looked: the edits the user made in the window, the assistant's and other clients' tool calls, and whether the project was replaced (`document`) or Ondina restarted (`session_id`). Keep `latest_seq` and pass it as `since` next time.

## Typical workflow to create a song
1. `get_project` to see what exists (ids!).
2. `set_tempo`, `set_key`, `set_time_signature`.
3. `set_chord_progression` (symbols or roman numerals) for the harmony.
4. `add_track` with a `patch` from `list_patches` (e.g. "Pop Drum Kit", "Deep Sub Bass", "Lush Pad", "Stage Electric Piano", "Supersaw Lead").
5. Fill tracks: session players `generate_drums`, `generate_bass`, `generate_chords`, `generate_arpeggio`, `generate_melody` — or write exact notes with `create_midi_region` / `add_notes`.
6. Arrange: `add_marker` (with length for sections), `copy_section`, `duplicate_region`, `paste_region` (a region object from `get_project_json` onto any track of the same type).
7. Mix: `set_volume`, `set_pan`, `add_effect`, buses + `add_send` for shared reverb/delay, `set_automation`.
8. Listen: `analyze_mix` returns loudness, peaks, spectral balance and advice. `auto_master` builds a master chain.
9. Deliver: `bounce` to WAV, `save_project`, `export_midi`.
"#;

fn ok(id: &Value, result: Value) -> Value {
    json!({"jsonrpc": "2.0", "id": id, "result": result})
}

fn error(id: &Value, code: i64, msg: &str) -> Value {
    json!({"jsonrpc": "2.0", "id": id, "error": {"code": code, "message": msg}})
}

/// Handle one JSON-RPC message or batch. Returns `None` when no response is due (notifications).
pub fn handle(session: &SharedSession, msg: &Value) -> Option<Value> {
    if let Value::Array(batch) = msg {
        let out: Vec<Value> = batch.iter().filter_map(|m| handle_one(session, m)).collect();
        return if out.is_empty() { None } else { Some(Value::Array(out)) };
    }
    handle_one(session, msg)
}

fn handle_one(session: &SharedSession, msg: &Value) -> Option<Value> {
    let method = msg.get("method").and_then(|m| m.as_str()).unwrap_or("");
    let id = msg.get("id").cloned();
    let params = msg.get("params").cloned().unwrap_or(json!({}));
    let Some(id) = id else {
        // notification (notifications/initialized, notifications/cancelled, ...)
        return None;
    };
    if method.is_empty() {
        // a response from the client (we never send requests) — ignore
        return None;
    }
    Some(match method {
        "initialize" => {
            let requested = params.get("protocolVersion").and_then(|v| v.as_str()).unwrap_or(PROTOCOL_VERSIONS[0]);
            let version = if PROTOCOL_VERSIONS.contains(&requested) { requested } else { PROTOCOL_VERSIONS[0] };
            ok(
                &id,
                json!({
                    "protocolVersion": version,
                    "capabilities": {"tools": {"listChanged": false}, "resources": {"listChanged": false, "subscribe": false}, "prompts": {"listChanged": false}},
                    "serverInfo": {"name": SERVER_NAME, "title": "Ondina DAW", "version": env!("CARGO_PKG_VERSION")},
                    "instructions": "Ondina DAW: a full digital audio workstation. Call get_project first to learn track/region/plugin ids. Positions are in beats (quarter notes); track_id 0 is the master. Read resource ondina://guide for the full workflow."
                }),
            )
        }
        "ping" => ok(&id, json!({})),
        "tools/list" => {
            let list: Vec<Value> = tools()
                .iter()
                .map(|t| {
                    json!({
                        "name": t.name,
                        "title": title_case(&t.name),
                        "description": t.description,
                        "inputSchema": t.input_schema,
                        "annotations": {"readOnlyHint": t.read_only, "destructiveHint": matches!(t.name.as_str(), "remove_track" | "delete_region" | "new_project" | "delete_section" | "remove_notes" | "replace_notes" | "replace_cc" | "open_project"), "idempotentHint": t.read_only, "openWorldHint": false}
                    })
                })
                .collect();
            ok(&id, json!({"tools": list}))
        }
        "tools/call" => {
            let name = params.get("name").and_then(|n| n.as_str()).unwrap_or("");
            let args = params.get("arguments").cloned().unwrap_or(json!({}));
            if !tools().iter().any(|t| t.name == name) {
                return Some(error(&id, -32602, &format!("unknown tool: {name}")));
            }
            let (text, is_error, structured) = match Command::from_tool_call(name, &args) {
                Ok(cmd) => match session.lock().execute(cmd, Source::Mcp) {
                    Ok(v) => (serde_json::to_string_pretty(&v).unwrap_or_default(), false, Some(v)),
                    Err(e) => (format!("Error: {e}"), true, None),
                },
                Err(e) => (format!("Error: {e}"), true, None),
            };
            let mut result = json!({"content": [{"type": "text", "text": text}], "isError": is_error});
            if let Some(Value::Object(m)) = structured {
                result["structuredContent"] = Value::Object(m);
            }
            ok(&id, result)
        }
        "resources/list" => ok(
            &id,
            json!({"resources": [
                {"uri": "ondina://guide", "name": "guide", "title": "Ondina agent guide", "mimeType": "text/markdown", "description": "Conventions and workflow for controlling Ondina"},
                {"uri": "ondina://project", "name": "project", "title": "Project overview", "mimeType": "application/json", "description": "Compact overview of the current project"},
                {"uri": "ondina://project/document", "name": "project-document", "title": "Project document", "mimeType": "application/json", "description": "The complete project document"},
                {"uri": "ondina://plugins", "name": "plugins", "title": "Plug-in registry", "mimeType": "application/json", "description": "All instruments/effects with parameters"},
                {"uri": "ondina://patches", "name": "patches", "title": "Library patches", "mimeType": "application/json", "description": "Track patches by category"}
            ]}),
        ),
        "resources/templates/list" => ok(&id, json!({"resourceTemplates": []})),
        "resources/read" => {
            let uri = params.get("uri").and_then(|u| u.as_str()).unwrap_or("");
            // Clients written before the rename still ask for aurora:// URIs.
            let uri = uri.replacen("aurora://", "ondina://", 1);
            let (mime, text) = match uri.as_str() {
                "ondina://guide" => ("text/markdown", GUIDE.to_string()),
                "ondina://project" => ("application/json", serde_json::to_string_pretty(&ops::project_summary(&session.lock().project)).unwrap()),
                "ondina://project/document" => ("application/json", serde_json::to_string_pretty(&session.lock().project).unwrap()),
                "ondina://plugins" => ("application/json", serde_json::to_string_pretty(plugins::registry()).unwrap()),
                "ondina://patches" => ("application/json", serde_json::to_string_pretty(&plugins::PATCHES.iter().map(|p| json!({"name": p.name, "category": p.category})).collect::<Vec<_>>()).unwrap()),
                _ => return Some(error(&id, -32002, &format!("resource not found: {uri}"))),
            };
            ok(&id, json!({"contents": [{"uri": uri, "mimeType": mime, "text": text}]}))
        }
        "prompts/list" => ok(
            &id,
            json!({"prompts": [
                {"name": "compose_song", "title": "Compose a song", "description": "Build a complete arrangement from a short brief", "arguments": [
                    {"name": "genre", "description": "e.g. synthwave, lo-fi hip hop, pop ballad", "required": true},
                    {"name": "mood", "description": "e.g. melancholic, energetic", "required": false},
                    {"name": "length_bars", "description": "total length in bars (default 32)", "required": false}
                ]},
                {"name": "mix_review", "title": "Review and fix the mix", "description": "Analyze the mix and apply corrections", "arguments": []},
                {"name": "arrange_sections", "title": "Arrange into sections", "description": "Turn loops into intro/verse/chorus/outro", "arguments": []}
            ]}),
        ),
        "prompts/get" => {
            let name = params.get("name").and_then(|n| n.as_str()).unwrap_or("");
            let a = params.get("arguments").cloned().unwrap_or(json!({}));
            let get = |k: &str, d: &str| a.get(k).and_then(|v| v.as_str()).unwrap_or(d).to_string();
            let text = match name {
                "compose_song" => format!(
                    "Compose a complete {} song in Ondina with a {} mood, about {} bars long. Choose a fitting tempo and key, write a chord progression on the chord track, add drums, bass, harmony and a lead using library patches, create arrangement markers (intro, verse, chorus, outro) with copy_section, set a balanced mix with a shared reverb bus, run analyze_mix and fix problems, then auto_master. Explain briefly what you built.",
                    get("genre", "pop"), get("mood", "uplifting"), get("length_bars", "32")
                ),
                "mix_review" => "Run analyze_mix on the current project. Based on loudness, peaks, spectral balance, stereo correlation and per-track levels, apply concrete fixes (faders, pans, EQ, compression, sends). Re-run analyze_mix to verify and summarize the before/after numbers.".to_string(),
                "arrange_sections" => "Look at the current project with get_project. Turn the existing material into a song structure (intro, verse, chorus, bridge, outro) using add_marker with lengths, copy_section, region mutes and automation for builds. Keep the user's material.".to_string(),
                _ => return Some(error(&id, -32602, &format!("unknown prompt: {name}"))),
            };
            ok(&id, json!({"description": name, "messages": [{"role": "user", "content": {"type": "text", "text": text}}]}))
        }
        "logging/setLevel" => ok(&id, json!({})),
        "completion/complete" => ok(&id, json!({"completion": {"values": [], "hasMore": false}})),
        _ => error(&id, -32601, &format!("method not found: {method}")),
    })
}

fn title_case(s: &str) -> String {
    s.split('_')
        .map(|w| {
            let mut c = w.chars();
            match c.next() {
                Some(f) => f.to_uppercase().collect::<String>() + c.as_str(),
                None => String::new(),
            }
        })
        .collect::<Vec<_>>()
        .join(" ")
}

/// Serve MCP over stdio until stdin closes.
pub fn serve_stdio(session: SharedSession) {
    let stdin = std::io::stdin();
    let mut stdout = std::io::stdout();
    for line in stdin.lock().lines() {
        let Ok(line) = line else { break };
        let line = line.trim();
        if line.is_empty() {
            continue;
        }
        let resp = match serde_json::from_str::<Value>(line) {
            Ok(msg) => handle(&session, &msg),
            Err(e) => Some(error(&Value::Null, -32700, &format!("parse error: {e}"))),
        };
        if let Some(r) = resp {
            let _ = writeln!(stdout, "{}", serde_json::to_string(&r).unwrap());
            let _ = stdout.flush();
        }
    }
}

/// Forward stdio JSON-RPC to a running Ondina HTTP MCP endpoint.
pub fn serve_bridge(url: &str) {
    let stdin = std::io::stdin();
    let mut stdout = std::io::stdout();
    let token = ondina_core::env_var("MCP_TOKEN");
    let mut session_id: Option<String> = None;
    for line in stdin.lock().lines() {
        let Ok(line) = line else { break };
        if line.trim().is_empty() {
            continue;
        }
        let mut req = ureq::post(url).set("Content-Type", "application/json").set("Accept", "application/json, text/event-stream");
        if let Some(t) = &token {
            req = req.set("Authorization", &format!("Bearer {t}"));
        }
        if let Some(s) = &session_id {
            req = req.set("Mcp-Session-Id", s);
        }
        match req.send_string(&line) {
            Ok(resp) => {
                if let Some(s) = resp.header("Mcp-Session-Id") {
                    session_id = Some(s.to_string());
                }
                if resp.status() == 202 {
                    continue;
                }
                let body = resp.into_string().unwrap_or_default();
                if !body.trim().is_empty() {
                    let _ = writeln!(stdout, "{}", body.trim());
                    let _ = stdout.flush();
                }
            }
            Err(e) => {
                let id = serde_json::from_str::<Value>(&line).ok().and_then(|v| v.get("id").cloned());
                if let Some(id) = id {
                    let msg = format!("Ondina is not reachable at {url}: {e}. Start the Ondina app first.");
                    let _ = writeln!(stdout, "{}", error(&id, -32000, &msg));
                    let _ = stdout.flush();
                }
            }
        }
    }
}

pub struct HttpServerHandle {
    pub address: String,
    server: std::sync::Arc<tiny_http::Server>,
}

impl HttpServerHandle {
    pub fn shutdown(&self) {
        self.server.unblock();
    }
}

/// Only local origins may call the server. `null` is what sandboxed iframes and pages opened from
/// files send, so it is accepted only when a bearer token still guards the endpoint.
fn origin_allowed(origin: &str, token_configured: bool) -> bool {
    let o = origin.trim_end_matches('/');
    if o == "null" {
        return token_configured;
    }
    let authority = o.split("://").nth(1).unwrap_or(o);
    // An IPv6 host is bracketed and contains ':', so it ends at ']' rather than at the first ':'.
    let host = if let Some(end) = authority.strip_prefix('[').and_then(|r| r.find(']')) {
        let (host, rest) = authority.split_at(end + 2);
        if !rest.is_empty() && !rest.starts_with(':') {
            return false;
        }
        host
    } else {
        authority.split(':').next().unwrap_or(authority)
    };
    matches!(host, "localhost" | "127.0.0.1" | "[::1]")
}

/// Start the Streamable HTTP MCP endpoint on 127.0.0.1:`port` in background threads.
pub fn serve_http(session: SharedSession, port: u16) -> Result<HttpServerHandle, String> {
    let token = ondina_core::env_var("MCP_TOKEN").filter(|t| !t.is_empty());
    serve_http_with_token(session, port, token)
}

fn serve_http_with_token(session: SharedSession, port: u16, token: Option<String>) -> Result<HttpServerHandle, String> {
    let addr = format!("127.0.0.1:{port}");
    let server = std::sync::Arc::new(tiny_http::Server::http(&addr).map_err(|e| format!("cannot bind {addr}: {e}"))?);
    for _ in 0..4 {
        let server = server.clone();
        let session = session.clone();
        let token = token.clone();
        std::thread::Builder::new()
            .name("ondina-mcp-http".into())
            .spawn(move || {
                for mut req in server.incoming_requests() {
                    let header = |name: &str| req.headers().iter().find(|h| h.field.as_str().as_str().eq_ignore_ascii_case(name)).map(|h| h.value.as_str().to_string());
                    let json_header = tiny_http::Header::from_bytes("Content-Type", "application/json").unwrap();
                    if let Some(o) = header("Origin") {
                        if !origin_allowed(&o, token.is_some()) {
                            let msg = if o.trim_end_matches('/') == "null" {
                                "forbidden origin: Origin null is accepted only when ONDINA_MCP_TOKEN is set"
                            } else {
                                "forbidden origin"
                            };
                            let _ = req.respond(tiny_http::Response::from_string(msg).with_status_code(403));
                            continue;
                        }
                    }
                    if let Some(t) = &token {
                        if header("Authorization").as_deref() != Some(&format!("Bearer {t}")) {
                            let _ = req.respond(tiny_http::Response::from_string("unauthorized").with_status_code(401));
                            continue;
                        }
                    }
                    let path = req.url().split('?').next().unwrap_or("").to_string();
                    match (req.method(), path.as_str()) {
                        (tiny_http::Method::Get, "/health") => {
                            let _ = req.respond(tiny_http::Response::from_string("{\"status\":\"ok\"}").with_header(json_header));
                        }
                        (tiny_http::Method::Post, "/mcp") | (tiny_http::Method::Post, "/") => {
                            let mut body = String::new();
                            if req.as_reader().read_to_string(&mut body).is_err() {
                                let _ = req.respond(tiny_http::Response::from_string("bad body").with_status_code(400));
                                continue;
                            }
                            let parsed: Result<Value, _> = serde_json::from_str(&body);
                            let is_init = parsed.as_ref().ok().and_then(|v| v.get("method")).and_then(|m| m.as_str()) == Some("initialize");
                            let resp = match parsed {
                                Ok(msg) => handle(&session, &msg),
                                Err(e) => Some(error(&Value::Null, -32700, &format!("parse error: {e}"))),
                            };
                            match resp {
                                Some(r) => {
                                    let mut response = tiny_http::Response::from_string(serde_json::to_string(&r).unwrap()).with_header(json_header);
                                    if is_init {
                                        let sid = format!("ondina-{:x}", std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map(|d| d.as_nanos()).unwrap_or(0));
                                        response = response.with_header(tiny_http::Header::from_bytes("Mcp-Session-Id", sid.as_bytes()).unwrap());
                                    }
                                    let _ = req.respond(response);
                                }
                                None => {
                                    let _ = req.respond(tiny_http::Response::empty(202));
                                }
                            }
                        }
                        (tiny_http::Method::Get, "/mcp") => {
                            let _ = req.respond(tiny_http::Response::from_string("SSE stream not supported; use POST").with_status_code(405));
                        }
                        (tiny_http::Method::Delete, "/mcp") => {
                            let _ = req.respond(tiny_http::Response::empty(200));
                        }
                        _ => {
                            let _ = req.respond(tiny_http::Response::from_string("not found").with_status_code(404));
                        }
                    }
                }
            })
            .map_err(|e| e.to_string())?;
    }
    Ok(HttpServerHandle { address: format!("http://{addr}/mcp"), server })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::session::Session;

    #[test]
    fn mcp_roundtrip_over_http() {
        let s = Session::shared(false);
        let h = serve_http(s.clone(), 39171).unwrap();
        let post = |body: Value| -> Value {
            let r = ureq::post(&h.address).set("Content-Type", "application/json").send_string(&body.to_string()).unwrap();
            serde_json::from_str(&r.into_string().unwrap()).unwrap()
        };
        let init = post(json!({"jsonrpc": "2.0", "id": 1, "method": "initialize", "params": {"protocolVersion": "2025-06-18", "capabilities": {}, "clientInfo": {"name": "t", "version": "1"}}}));
        assert_eq!(init["result"]["protocolVersion"], "2025-06-18");
        let list = post(json!({"jsonrpc": "2.0", "id": 2, "method": "tools/list"}));
        assert!(list["result"]["tools"].as_array().unwrap().len() > 80);
        let add = post(json!({"jsonrpc": "2.0", "id": 3, "method": "tools/call", "params": {"name": "add_track", "arguments": {"kind": "instrument", "patch": "Lush Pad"}}}));
        assert_eq!(add["result"]["isError"], false, "{add}");
        let tid = add["result"]["structuredContent"]["track_id"].as_u64().unwrap();
        let reg = post(json!({"jsonrpc": "2.0", "id": 4, "method": "tools/call", "params": {"name": "create_midi_region", "arguments": {"track_id": tid, "start": 0, "length": 4, "notes": [{"pitch": 60, "velocity": 100, "start": 0, "length": 1}]}}}));
        assert_eq!(reg["result"]["isError"], false, "{reg}");
        let bad = post(json!({"jsonrpc": "2.0", "id": 5, "method": "tools/call", "params": {"name": "set_volume", "arguments": {"track_id": 9999, "db": 0}}}));
        assert_eq!(bad["result"]["isError"], true);
        let res = post(json!({"jsonrpc": "2.0", "id": 6, "method": "resources/read", "params": {"uri": "ondina://project"}}));
        assert!(res["result"]["contents"][0]["text"].as_str().unwrap().contains("Lush Pad"));
        let old = post(json!({"jsonrpc": "2.0", "id": 7, "method": "resources/read", "params": {"uri": "aurora://project"}}));
        assert!(old["result"]["contents"][0]["text"].as_str().unwrap().contains("Lush Pad"), "pre-rename URI still served: {old}");
        let r = ureq::post(&h.address).set("Content-Type", "application/json").send_string(&json!({"jsonrpc": "2.0", "method": "notifications/initialized"}).to_string()).unwrap();
        assert_eq!(r.status(), 202);
        assert_eq!(s.lock().project.tracks.len(), 1);
        h.shutdown();
    }

    #[test]
    fn origin_allowed_accepts_null_only_with_token() {
        assert!(!origin_allowed("null", false));
        assert!(origin_allowed("null", true));
        for token in [false, true] {
            for ok in ["http://localhost:5173", "http://localhost", "http://localhost/", "http://127.0.0.1", "http://127.0.0.1:3917", "http://[::1]", "http://[::1]:8080"] {
                assert!(origin_allowed(ok, token), "{ok} should be allowed (token: {token})");
            }
            for bad in ["http://localhost.evil.com", "http://evil.com", "http://127.0.0.1.evil.com", "http://[::1].evil.com"] {
                assert!(!origin_allowed(bad, token), "{bad} should be refused (token: {token})");
            }
        }
    }

    fn post_with(address: &str, headers: &[(&str, &str)], body: &Value) -> (u16, String) {
        let mut req = ureq::post(address).set("Content-Type", "application/json");
        for (k, v) in headers {
            req = req.set(k, v);
        }
        match req.send_string(&body.to_string()) {
            Ok(r) => (r.status(), r.into_string().unwrap_or_default()),
            Err(ureq::Error::Status(code, r)) => (code, r.into_string().unwrap_or_default()),
            Err(e) => panic!("transport error: {e}"),
        }
    }

    fn init_request() -> Value {
        json!({"jsonrpc": "2.0", "id": 1, "method": "initialize", "params": {"protocolVersion": "2025-06-18", "capabilities": {}, "clientInfo": {"name": "t", "version": "1"}}})
    }

    #[test]
    fn http_rejects_null_origin_without_token() {
        let s = Session::shared(false);
        let h = serve_http_with_token(s, 39173, None).unwrap();
        let (status, body) = post_with(&h.address, &[("Origin", "null")], &init_request());
        assert_eq!(status, 403);
        assert!(body.contains("ONDINA_MCP_TOKEN"), "{body}");
        let (status, body) = post_with(&h.address, &[], &init_request());
        assert_eq!(status, 200, "{body}");
        let v: Value = serde_json::from_str(&body).unwrap();
        assert_eq!(v["result"]["protocolVersion"], "2025-06-18");
        let (status, _) = post_with(&h.address, &[("Origin", "http://localhost:5173")], &init_request());
        assert_eq!(status, 200);
        let (status, _) = post_with(&h.address, &[("Origin", "http://localhost.evil.com")], &init_request());
        assert_eq!(status, 403);
        h.shutdown();
    }

    #[test]
    fn http_accepts_null_origin_with_token() {
        let s = Session::shared(false);
        let h = serve_http_with_token(s, 39174, Some("t0ken".into())).unwrap();
        let (status, body) = post_with(&h.address, &[("Origin", "null"), ("Authorization", "Bearer t0ken")], &init_request());
        assert_eq!(status, 200, "{body}");
        let v: Value = serde_json::from_str(&body).unwrap();
        assert_eq!(v["result"]["protocolVersion"], "2025-06-18");
        let (status, _) = post_with(&h.address, &[("Origin", "null")], &init_request());
        assert_eq!(status, 401);
        h.shutdown();
    }
}
