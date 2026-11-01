//! AI assistant side panel: header with the engine's status, the empty state when no model is connected, quick
//! actions, the chat (or the recent MCP activity), and the prompt box.

use std::sync::atomic::Ordering;
use std::sync::OnceLock;

use eframe::egui::{self, vec2, Color32, RichText, Rounding, Stroke};
use ondina_studio::Source;

use crate::app::{ChatItem, OndinaApp};
use crate::chrome;
use crate::theme;
use crate::widgets::{self, Icon};

const QUICK: &[(&str, &str)] = &[
    ("Compose an idea", "Compose an 8-bar musical idea in a style that fits the project (or pick a modern pop style if the project is empty): chords on the chord track, drums, bass, keys and a melody. Keep it tasteful and explain briefly."),
    ("Add drums", "Add a drum track with a groove that fits the current tempo and material, covering the whole song."),
    ("Fix the mix", "Analyze the mix with analyze_mix, fix level/EQ/compression/panning problems, then verify with analyze_mix again and summarize the numbers."),
    ("Arrange song", "Turn the current material into a full song structure (intro, verse, chorus, bridge, outro) with arrangement markers and variations."),
    ("Master", "Master the song for streaming at -14 LUFS with auto_master and report the result."),
    ("Suggest", "Look at the project and suggest three concrete creative next steps (don't change anything yet)."),
];

/// Hover text of an AI button while no model is connected.
pub const NO_MODEL_HINT: &str =
    "No AI model connected: add an Anthropic API key or start the local model in Settings";
/// Hover text of an AI button while the assistant is answering.
pub const BUSY_HINT: &str = "The assistant is working: wait for it or press Stop";

/// Which engine answers the built-in assistant.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum AiEngine {
    /// No key for Claude and no local model: the assistant cannot answer.
    Offline,
    /// The local OpenAI-compatible model (bundled llama.cpp, or a server the person set up).
    Local,
    /// Claude through the Anthropic API.
    Claude,
}

impl AiEngine {
    pub fn label(self) -> &'static str {
        match self {
            AiEngine::Offline => "Offline",
            AiEngine::Local => "Local",
            AiEngine::Claude => "Claude",
        }
    }

    /// (dot, fill, text) of the status pill.
    pub fn pill_colors(self) -> (Color32, Color32, Color32) {
        match self {
            AiEngine::Offline => (theme::TEXT_3, theme::BORDER, theme::TEXT_2),
            AiEngine::Local => (theme::PLAY, theme::PLAY_BG, theme::PLAY),
            AiEngine::Claude => (theme::AI, theme::AI_BG, theme::AI_TEXT),
        }
    }
}

/// The engine the assistant uses, as `Settings::use_local` picks it: "local" always means the local model (its
/// server may be one the person runs), "anthropic" needs a key, and "auto" takes Claude with a key, else the bundled
/// model when it is installed.
pub fn ai_engine(provider: &str, has_key: bool, bundled_local: bool) -> AiEngine {
    match provider {
        "local" => AiEngine::Local,
        "anthropic" if has_key => AiEngine::Claude,
        "anthropic" => AiEngine::Offline,
        _ if has_key => AiEngine::Claude,
        _ if bundled_local => AiEngine::Local,
        _ => AiEngine::Offline,
    }
}

/// Which of the panel's controls work.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct AiControls {
    /// Typing in the prompt box.
    pub prompt: bool,
    /// Send, Ctrl+Enter, the quick actions and every "Ask the AI" button in the app.
    pub ask: bool,
    /// Stop is shown instead of Send.
    pub stop: bool,
    pub new_chat: bool,
    /// The "no model" card is shown.
    pub empty_state: bool,
}

pub fn ai_controls(engine: AiEngine, busy: bool) -> AiControls {
    let online = engine != AiEngine::Offline;
    AiControls {
        prompt: online,
        ask: online && !busy,
        stop: busy,
        new_chat: !busy,
        empty_state: !online,
    }
}

/// Why an AI button is disabled, None when it works.
pub fn ask_blocked_reason(engine: AiEngine, busy: bool) -> Option<&'static str> {
    if engine == AiEngine::Offline {
        Some(NO_MODEL_HINT)
    } else if busy {
        Some(BUSY_HINT)
    } else {
        None
    }
}

/// "12s", "4m", "2h": how long ago an MCP command ran.
pub fn short_age(secs: u64) -> String {
    match secs {
        0..=59 => format!("{secs}s"),
        60..=3599 => format!("{}m", secs / 60),
        _ => format!("{}h", secs / 3600),
    }
}

/// Whether a bundled local model is installed; looked up once (it scans folders on disk).
fn bundled_local() -> bool {
    static FOUND: OnceLock<bool> = OnceLock::new();
    *FOUND.get_or_init(|| ondina_studio::local_ai::bundled_llm().is_some())
}

impl OndinaApp {
    /// The engine the assistant would use now.
    pub fn ai_engine(&self) -> AiEngine {
        ai_engine(&self.settings.provider, self.settings.api_key().is_some(), bundled_local())
    }

    /// An "Ask the AI" button, disabled (with the reason on hover) while no model is connected or the assistant is
    /// busy. True when clicked.
    pub fn ai_button(&self, ui: &mut egui::Ui, text: &str) -> bool {
        let blocked = ask_blocked_reason(self.ai_engine(), self.ai.busy);
        let enabled = blocked.is_none();
        let fg = if enabled { theme::AI_TEXT } else { theme::TEXT_DISABLED };
        let b = egui::Button::new(widgets::leading_icon_text(ui, RichText::new(text).color(fg)))
            .fill(theme::AI_BG)
            .stroke(Stroke::new(1.0_f32, theme::AI_BORDER))
            .rounding(Rounding::same(theme::R_LG));
        let resp = ui.add_enabled(enabled, b);
        widgets::paint_leading_icon(ui, &resp, Icon::Ai, if enabled { theme::AI } else { theme::TEXT_DISABLED });
        match blocked {
            Some(why) => resp.on_disabled_hover_text(why).clicked(),
            None => resp.clicked(),
        }
    }

    pub fn assistant_ui(&mut self, ui: &mut egui::Ui) {
        let engine = self.ai_engine();
        let controls = ai_controls(engine, self.ai.busy);
        ui.spacing_mut().item_spacing.y = theme::SP_3;
        self.assistant_header(ui, engine, controls);
        let pad = theme::SP_5;
        egui::Frame::none()
            .inner_margin(egui::Margin { left: pad, right: pad, top: theme::SP_4, bottom: pad })
            .show(ui, |ui| {
                if controls.empty_state {
                    self.no_model_card(ui);
                } else {
                    let caption = engine_caption(
                        engine == AiEngine::Local,
                        &self.settings.model(),
                        ondina_studio::local_ai::tool_count(),
                        ondina_studio::mcp::tools().len(),
                    );
                    ui.label(RichText::new(caption).size(theme::SIZE_NUMBER).color(theme::TEXT_3));
                }
                ui.add_space(theme::SP_3);
                chrome::section_label(ui, "Quick actions");
                ui.horizontal_wrapped(|ui| {
                    ui.spacing_mut().item_spacing = vec2(6.0, 6.0);
                    for (label, prompt) in QUICK {
                        let fg = if controls.ask { theme::TEXT } else { theme::TEXT_DISABLED };
                        let b = egui::Button::new(RichText::new(*label).size(theme::SIZE_BODY).color(fg))
                            .fill(Color32::TRANSPARENT)
                            .stroke(Stroke::new(1.0_f32, theme::BORDER))
                            .rounding(Rounding::same(13.0))
                            .min_size(vec2(0.0, 26.0));
                        let resp = ui.add_enabled(controls.ask, b);
                        let resp = match ask_blocked_reason(engine, self.ai.busy) {
                            Some(why) => resp.on_disabled_hover_text(why),
                            None => resp.on_hover_text(*prompt),
                        };
                        if resp.clicked() {
                            self.ask_ai(prompt.to_string());
                        }
                    }
                });
                ui.add_space(theme::SP_4);
                let prompt_h = 112.0;
                let avail = (ui.available_height() - prompt_h).max(60.0);
                egui::ScrollArea::vertical()
                    .max_height(avail)
                    .stick_to_bottom(true)
                    .auto_shrink([false, false])
                    .show(ui, |ui| self.chat_or_activity(ui));
                self.prompt_box(ui, controls);
            });
    }

    fn assistant_header(&mut self, ui: &mut egui::Ui, engine: AiEngine, controls: AiControls) {
        chrome::header_strip(ui, chrome::ASSISTANT_HEADER, |ui| {
            chrome::icon_title(
                ui,
                Icon::Ai,
                theme::AI,
                RichText::new("Assistant").font(egui::FontId::new(13.0, theme::semibold())).color(theme::TEXT),
            );
            let (dot, fill, fg) = engine.pill_colors();
            let tip = match engine {
                AiEngine::Offline => NO_MODEL_HINT.to_string(),
                AiEngine::Local => "Answered by the local model on this computer".to_string(),
                AiEngine::Claude => format!("Answered by {}", self.settings.model()),
            };
            chrome::status_pill(ui, engine.label(), dot, fill, fg).on_hover_text(tip);
            ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                if chrome::header_button(ui, Icon::Close, "Hide the assistant").clicked() {
                    self.show_assistant = false;
                }
                if chrome::header_button(ui, Icon::Settings, "AI settings").clicked() {
                    self.show_settings = true;
                }
                let new_chat = egui::Button::new(RichText::new("New chat").color(theme::ICON))
                    .fill(theme::RAISED)
                    .stroke(Stroke::new(1.0_f32, theme::BORDER))
                    .min_size(vec2(0.0, 24.0));
                if ui
                    .add_enabled(controls.new_chat, new_chat)
                    .on_disabled_hover_text(BUSY_HINT)
                    .clicked()
                {
                    self.ai.assistant.lock().clear();
                    self.ai.local.lock().clear();
                    self.ai.chat.clear();
                }
            });
        });
    }

    fn no_model_card(&mut self, ui: &mut egui::Ui) {
        egui::Frame::none()
            .fill(theme::RAISED)
            .stroke(Stroke::new(1.0_f32, theme::BORDER))
            .rounding(Rounding::same(theme::R_XL))
            .inner_margin(14.0)
            .show(ui, |ui| {
                ui.set_width(ui.available_width());
                ui.label(RichText::new("No AI model connected").font(egui::FontId::new(13.0, theme::semibold())));
                ui.label(
                    RichText::new(
                        "Add an Anthropic API key or start the local model to use the assistant. \
                         External agents can always use Ondina through MCP.",
                    )
                    .color(theme::TEXT_2),
                );
                ui.add_space(theme::SP_3);
                let open = egui::Button::new(RichText::new("Open settings").color(Color32::WHITE))
                    .fill(Color32::from_rgb(0x7C, 0x5C, 0xE0))
                    .stroke(Stroke::NONE)
                    .rounding(Rounding::same(theme::R_LG))
                    .min_size(vec2(0.0, 28.0));
                if ui.add(open).clicked() {
                    self.show_settings = true;
                }
            });
    }

    fn chat_or_activity(&mut self, ui: &mut egui::Ui) {
        if self.ai.chat.is_empty() {
            if self.ai_engine() != AiEngine::Offline {
                ui.label(RichText::new("Ask anything: “make a lo-fi beat at 85 bpm”, “double the chorus”, “make the bass punchier”, “why does my mix sound muddy?”.\n\nThe assistant edits the project live with undoable commands. External agents (Claude Code, Claude Desktop…) can do the same through the MCP server.").color(theme::TEXT_2));
                ui.add_space(theme::SP_4);
            }
            self.mcp_activity(ui);
        }
        for item in &self.ai.chat {
            match item {
                ChatItem::User(t) => {
                    egui::Frame::none().fill(theme::SELECTED_BG).rounding(theme::R_LG).inner_margin(8.0).show(ui, |ui| {
                        ui.label(RichText::new(t).color(theme::TEXT));
                    });
                }
                ChatItem::Assistant(t) => {
                    egui::Frame::none().fill(theme::AI_BG).rounding(theme::R_LG).inner_margin(8.0).show(ui, |ui| {
                        ui.label(RichText::new(t).color(theme::TEXT));
                    });
                }
                ChatItem::Tool { name, ok, input, result } => {
                    let head = RichText::new(name.as_str())
                        .font(theme::number_font())
                        .color(if *ok { theme::PLAY } else { theme::REC });
                    let header = egui::CollapsingHeader::new(head).id_salt((name, input.len(), result.len(), input.as_ptr() as usize)).show(ui, |ui| {
                        ui.label(RichText::new(format!("in: {input}")).font(theme::number_font()));
                        ui.label(RichText::new(format!("out: {result}")).font(theme::number_font()).color(theme::TEXT_2));
                    });
                    // the result mark after the tool's name, where a check or a cross glyph used to be
                    let r = header.header_response.rect;
                    let at = egui::Rect::from_center_size(egui::pos2(r.right() - 10.0, r.center().y), vec2(11.0, 11.0));
                    let (icon, color) = if *ok { (Icon::Check, theme::PLAY) } else { (Icon::Close, theme::REC) };
                    widgets::paint_icon(ui.painter(), icon, at, color);
                }
                ChatItem::Error(e) => {
                    ui.horizontal_top(|ui| {
                        widgets::icon(ui, Icon::Warning, 14.0, theme::REC);
                        ui.label(RichText::new(e.as_str()).color(theme::REC));
                    });
                }
            }
            ui.add_space(3.0);
        }
        if self.ai.busy {
            ui.horizontal(|ui| {
                ui.spinner();
                ui.label(RichText::new(if self.ai.status.is_empty() { "Working…" } else { &self.ai.status }).color(theme::TEXT_2));
            });
        }
    }

    /// The commands external agents ran last, in mono with a check (or a cross) and their age.
    fn mcp_activity(&self, ui: &mut egui::Ui) {
        chrome::section_label(ui, "MCP activity");
        let recent: Vec<_> = self.view.log.iter().filter(|l| l.source == Source::Mcp).take(8).collect();
        if recent.is_empty() {
            ui.label(RichText::new("No MCP commands yet").size(theme::SIZE_NUMBER).color(theme::TEXT_3));
            return;
        }
        let w = ui.available_width();
        for l in recent {
            ui.allocate_ui_with_layout(vec2(w, 22.0), egui::Layout::left_to_right(egui::Align::Center), |ui| {
                ui.spacing_mut().item_spacing.x = theme::SP_4;
                let (mark, color) = if l.ok { (Icon::Check, theme::PLAY) } else { (Icon::Close, theme::REC) };
                widgets::icon(ui, mark, 11.0, color);
                ui.label(RichText::new(&l.command).font(theme::number_font()).color(theme::ICON));
                ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                    let age = short_age(l.at.elapsed().as_secs());
                    ui.label(RichText::new(age).font(theme::number_font()).color(theme::TEXT_DISABLED));
                });
            });
        }
    }

    fn prompt_box(&mut self, ui: &mut egui::Ui, controls: AiControls) {
        egui::Frame::none()
            .fill(theme::GROUND)
            .stroke(Stroke::new(1.0_f32, theme::BORDER))
            .rounding(Rounding::same(theme::R_XL))
            .inner_margin(10.0)
            .show(ui, |ui| {
                let hint = if controls.prompt {
                    "Describe what you want… (Ctrl+Enter)"
                } else {
                    "Connect a model in Settings to ask the assistant"
                };
                let resp = ui.add_enabled(
                    controls.prompt,
                    egui::TextEdit::multiline(&mut self.ai.input)
                        .frame(false)
                        .desired_rows(3)
                        .desired_width(f32::INFINITY)
                        .hint_text(hint),
                );
                let send_key = controls.ask
                    && resp.has_focus()
                    && ui.input(|i| i.key_pressed(egui::Key::Enter) && i.modifiers.command);
                ui.horizontal(|ui| {
                    // only offered when the last step is a whole assistant request, never a user edit
                    let can_undo_turn = self.view.undo_ai && !self.ai.busy;
                    let request = self.view.undo.as_deref().unwrap_or("");
                    let hover = format!("Undo everything the assistant did for: {}", request.strip_prefix("assistant: ").unwrap_or(request));
                    let why_not = if self.ai.busy {
                        "Stop the assistant first"
                    } else {
                        "The last edit was not made by the assistant: use Ctrl+Z to undo it"
                    };
                    let undo_text = widgets::leading_icon_text(ui, RichText::new("Undo AI edit").size(theme::SIZE_NUMBER));
                    let undo = ui.add_enabled(can_undo_turn, egui::Button::new(undo_text).frame(false));
                    let color = if can_undo_turn { ui.style().interact(&undo).text_color() } else { theme::TEXT_DISABLED };
                    widgets::paint_leading_icon(ui, &undo, Icon::Undo, color);
                    if undo.on_hover_text(hover).on_disabled_hover_text(why_not).clicked() {
                        self.cmd(ondina_core::Command::Undo {});
                    }
                    ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                        if controls.stop {
                            if ui.button("Stop").clicked() {
                                self.ai.cancel.store(true, Ordering::Relaxed);
                            }
                            return;
                        }
                        let (fill, fg) = if controls.ask { (theme::AI_BORDER, Color32::WHITE) } else { (theme::BORDER, theme::TEXT_DISABLED) };
                        let send = egui::Button::new(RichText::new("Send").color(fg))
                            .fill(fill)
                            .stroke(Stroke::NONE)
                            .rounding(Rounding::same(theme::R_LG))
                            .min_size(vec2(0.0, 26.0));
                        let resp = ui.add_enabled(controls.ask, send);
                        let resp = match ask_blocked_reason(self.ai_engine(), self.ai.busy) {
                            Some(why) => resp.on_disabled_hover_text(why),
                            None => resp,
                        };
                        if (resp.clicked() || send_key) && !self.ai.input.trim().is_empty() {
                            let t = std::mem::take(&mut self.ai.input);
                            self.ask_ai(t.trim().to_string());
                        }
                    });
                });
            });
    }
}

/// The line under the panel title: which engine answers and how many tools it is given.
/// The local model gets a subset of the MCP tools, Claude gets all of them.
fn engine_caption(local: bool, model: &str, local_tools: usize, all_tools: usize) -> String {
    if local {
        format!("local model (offline) · {local_tools} tools")
    } else {
        format!("{model} · {all_tools} tools · same API as MCP")
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn engine_caption_counts_the_local_tools_when_offline() {
        let caption = engine_caption(true, "claude-x", 45, 160);
        assert_eq!(caption, "local model (offline) · 45 tools");
        assert!(!caption.contains("160"));
        assert!(!caption.contains("same API as MCP"));
    }

    #[test]
    fn engine_caption_counts_all_tools_with_claude() {
        assert_eq!(engine_caption(false, "claude-x", 45, 160), "claude-x · 160 tools · same API as MCP");
    }

    #[test]
    fn engine_follows_the_provider_the_key_and_the_bundled_model() {
        assert_eq!(ai_engine("auto", true, false), AiEngine::Claude);
        assert_eq!(ai_engine("auto", true, true), AiEngine::Claude);
        assert_eq!(ai_engine("auto", false, true), AiEngine::Local);
        assert_eq!(ai_engine("auto", false, false), AiEngine::Offline);
        assert_eq!(ai_engine("anthropic", true, true), AiEngine::Claude);
        assert_eq!(ai_engine("anthropic", false, true), AiEngine::Offline);
        // a local server the person runs themselves (Ollama, LM Studio) needs no bundled model
        assert_eq!(ai_engine("local", false, false), AiEngine::Local);
        assert_eq!(ai_engine("", false, false), AiEngine::Offline);
    }

    #[test]
    fn without_a_model_nothing_can_be_sent() {
        let c = ai_controls(AiEngine::Offline, false);
        assert!(!c.ask, "quick actions and Send must be disabled with no model");
        assert!(!c.prompt);
        assert!(c.empty_state);
        assert!(c.new_chat);
        assert!(!c.stop);
        assert_eq!(ask_blocked_reason(AiEngine::Offline, false), Some(NO_MODEL_HINT));
        // even while a stale turn is still marked busy, the reason is the missing model
        assert_eq!(ask_blocked_reason(AiEngine::Offline, true), Some(NO_MODEL_HINT));
    }

    #[test]
    fn a_connected_model_can_be_asked_unless_it_is_busy() {
        for engine in [AiEngine::Local, AiEngine::Claude] {
            let idle = ai_controls(engine, false);
            assert!(idle.ask && idle.prompt && !idle.empty_state && !idle.stop && idle.new_chat);
            assert_eq!(ask_blocked_reason(engine, false), None);
            let busy = ai_controls(engine, true);
            assert!(!busy.ask && busy.prompt && busy.stop && !busy.new_chat);
            assert_eq!(ask_blocked_reason(engine, true), Some(BUSY_HINT));
        }
    }

    #[test]
    fn status_pill_names_each_engine() {
        assert_eq!(AiEngine::Offline.label(), "Offline");
        assert_eq!(AiEngine::Local.label(), "Local");
        assert_eq!(AiEngine::Claude.label(), "Claude");
        assert_ne!(AiEngine::Offline.pill_colors(), AiEngine::Claude.pill_colors());
    }

    #[test]
    fn mcp_ages_are_short() {
        assert_eq!(short_age(0), "0s");
        assert_eq!(short_age(59), "59s");
        assert_eq!(short_age(60), "1m");
        assert_eq!(short_age(3599), "59m");
        assert_eq!(short_age(7200), "2h");
    }
}
