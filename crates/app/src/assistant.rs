//! AI assistant side panel.

use std::sync::atomic::Ordering;

use ondina_studio::Source;
use eframe::egui::{self, Color32, RichText};

use crate::app::{OndinaApp, ChatItem};
use crate::theme;

const QUICK: &[(&str, &str)] = &[
    ("🎼 Compose an idea", "Compose an 8-bar musical idea in a style that fits the project (or pick a modern pop style if the project is empty): chords on the chord track, drums, bass, keys and a melody. Keep it tasteful and explain briefly."),
    ("🥁 Add drums", "Add a drum track with a groove that fits the current tempo and material, covering the whole song."),
    ("🎚 Fix the mix", "Analyze the mix with analyze_mix, fix level/EQ/compression/panning problems, then verify with analyze_mix again and summarize the numbers."),
    ("🏗 Arrange song", "Turn the current material into a full song structure (intro, verse, chorus, bridge, outro) with arrangement markers and variations."),
    ("🎛 Master", "Master the song for streaming at -14 LUFS with auto_master and report the result."),
    ("💡 Suggest", "Look at the project and suggest three concrete creative next steps (don't change anything yet)."),
];

impl OndinaApp {
    pub fn assistant_ui(&mut self, ui: &mut egui::Ui) {
        ui.horizontal(|ui| {
            ui.label(
                RichText::new("✨ AI Assistant")
                    .strong()
                    .color(theme::AI)
                    .size(15.0),
            );
            ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                if ui.small_button("New chat").clicked() && !self.ai.busy {
                    self.ai.assistant.lock().clear();
                    self.ai.local.lock().clear();
                    self.ai.chat.clear();
                }
                if ui.small_button("⚙").on_hover_text("AI settings").clicked() {
                    self.show_settings = true;
                }
            });
        });
        let has_key = self.settings.api_key().is_some()
            || !self.api_key_buf.trim().is_empty()
            || ondina_studio::local_ai::bundled_llm().is_some()
            || self.settings.provider == "local";
        if !has_key {
            egui::Frame::none().fill(Color32::from_rgb(60, 45, 30)).rounding(4.0).inner_margin(6.0).show(ui, |ui| {
                ui.label(RichText::new("Add an Anthropic API key (Settings ⚙ or ANTHROPIC_API_KEY) to use the built-in assistant. External agents can always drive Ondina through MCP.").small());
            });
        }
        let caption = engine_caption(
            self.settings.use_local(),
            &self.settings.model(),
            ondina_studio::local_ai::tool_count(),
            ondina_studio::mcp::tools().len(),
        );
        ui.label(RichText::new(caption).small().color(theme::TEXT_DIM));
        ui.separator();
        ui.horizontal_wrapped(|ui| {
            for (label, prompt) in QUICK {
                if ui
                    .add_enabled(
                        !self.ai.busy,
                        egui::Button::new(RichText::new(*label).small()),
                    )
                    .on_hover_text(*prompt)
                    .clicked()
                {
                    self.ask_ai(prompt.to_string());
                }
            }
        });
        ui.separator();
        let input_h = 92.0;
        let avail = ui.available_height() - input_h;
        egui::ScrollArea::vertical().max_height(avail.max(80.0)).stick_to_bottom(true).auto_shrink([false, false]).show(ui, |ui| {
            if self.ai.chat.is_empty() {
                ui.label(RichText::new("Ask anything: “make a lo-fi beat at 85 bpm”, “double the chorus”, “make the bass punchier”, “why does my mix sound muddy?”.\n\nThe assistant edits the project live with undoable commands. External agents (Claude Code, Claude Desktop…) can do the same through the MCP server.").color(theme::TEXT_DIM));
                ui.add_space(8.0);
                let ext: Vec<_> = self.view.log.iter().filter(|l| l.source == Source::Mcp).take(8).cloned().collect();
                if !ext.is_empty() {
                    ui.label(RichText::new("Recent MCP activity").strong());
                    for l in ext {
                        ui.label(RichText::new(format!("{} {}", if l.ok { "✓" } else { "✗" }, l.command)).small().color(Color32::from_rgb(100, 200, 120)));
                    }
                }
            }
            for item in &self.ai.chat {
                match item {
                    ChatItem::User(t) => {
                        egui::Frame::none().fill(Color32::from_rgb(45, 55, 75)).rounding(6.0).inner_margin(6.0).show(ui, |ui| {
                            ui.label(RichText::new(t).color(Color32::WHITE));
                        });
                    }
                    ChatItem::Assistant(t) => {
                        egui::Frame::none().fill(Color32::from_rgb(48, 42, 64)).rounding(6.0).inner_margin(6.0).show(ui, |ui| {
                            ui.label(t);
                        });
                    }
                    ChatItem::Tool { name, ok, input, result } => {
                        let head = RichText::new(format!("{} {}", if *ok { "✓" } else { "✗" }, name)).small().monospace().color(if *ok { Color32::from_rgb(130, 200, 140) } else { theme::REC });
                        egui::CollapsingHeader::new(head).id_salt((name, input.len(), result.len(), input.as_ptr() as usize)).show(ui, |ui| {
                            ui.label(RichText::new(format!("in: {input}")).small().monospace());
                            ui.label(RichText::new(format!("out: {result}")).small().monospace().color(theme::TEXT_DIM));
                        });
                    }
                    ChatItem::Error(e) => {
                        ui.label(RichText::new(format!("⚠ {e}")).color(theme::REC));
                    }
                }
                ui.add_space(3.0);
            }
            if self.ai.busy {
                ui.horizontal(|ui| {
                    ui.spinner();
                    ui.label(RichText::new(if self.ai.status.is_empty() { "Working…" } else { &self.ai.status }).color(theme::TEXT_DIM));
                });
            }
        });
        ui.separator();
        let resp = ui.add(
            egui::TextEdit::multiline(&mut self.ai.input)
                .desired_rows(3)
                .desired_width(f32::INFINITY)
                .hint_text("Describe what you want… (Ctrl+Enter to send)"),
        );
        let send_key = resp.has_focus()
            && ui.input(|i| i.key_pressed(egui::Key::Enter) && i.modifiers.command);
        ui.horizontal(|ui| {
            if self.ai.busy {
                if ui.button("⏹ Stop").clicked() {
                    self.ai.cancel.store(true, Ordering::Relaxed);
                }
            } else if ui
                .add(
                    egui::Button::new(RichText::new("Send ➤").color(Color32::WHITE))
                        .fill(Color32::from_rgb(110, 80, 190)),
                )
                .clicked()
                || send_key
            {
                let t = std::mem::take(&mut self.ai.input);
                self.ask_ai(t.trim().to_string());
            }
            // only offered when the last step is a whole assistant request, never a user edit
            let can_undo_turn = self.view.undo_ai && !self.ai.busy;
            let request = self.view.undo.as_deref().unwrap_or("");
            let hover = format!("Undo everything the assistant did for: {}", request.strip_prefix("assistant: ").unwrap_or(request));
            let why_not = if self.ai.busy {
                "Stop the assistant first"
            } else {
                "The last edit was not made by the assistant: use Ctrl+Z to undo it"
            };
            if ui
                .add_enabled(can_undo_turn, egui::Button::new("↶ Undo AI edit").small())
                .on_hover_text(hover)
                .on_disabled_hover_text(why_not)
                .clicked()
            {
                self.cmd(ondina_core::Command::Undo {});
            }
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
}
