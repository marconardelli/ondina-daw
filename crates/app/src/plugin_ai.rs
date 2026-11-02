//! The AI bar drawn in the header of every plug-in window: a request box with its send button (Stop while the
//! assistant works) and the plug-in's ready-made requests. One component for the built-in plug-ins and the generic
//! VST3 window; the request goes to the assistant with the plug-in's settings as context.

use std::sync::atomic::Ordering;

use eframe::egui;
use ondina_core::model::{Id, Project};
use ondina_core::plugins;
use ondina_studio::prompts;

use crate::app::OndinaApp;
use crate::assistant::{ai_controls, ask_blocked_reason};
use crate::{chrome, theme};

/// The request a press of the send button or of Enter in the box sends: the trimmed text, or None when nothing was
/// pressed, the assistant cannot be asked now or the box is empty (the text then stays in the box).
fn bar_request(pressed: bool, blocked: bool, text: &str) -> Option<String> {
    let t = text.trim();
    (pressed && !blocked && !t.is_empty()).then(|| t.to_string())
}

/// What the bar sends to the assistant: the request and, as context, the plug-in's id, kind, track and role and every
/// parameter with its value and range (None when the plug-in is gone).
fn bar_ask(project: &Project, pid: Id, request: String) -> (String, Option<String>) {
    (request, prompts::plugin_bar_context(project, pid))
}

impl OndinaApp {
    /// The AI bar of the plug-in `pid`, drawn in the header of every plug-in window.
    pub(crate) fn plugin_ai_bar(&mut self, ui: &mut egui::Ui, project: &Project, pid: Id) {
        let Some(inst) = project.find_plugin(pid) else {
            return;
        };
        let label = plugins::plugin_label(inst);
        let engine = self.ai_engine();
        let controls = ai_controls(engine, self.ai.busy);
        let blocked = ask_blocked_reason(engine, self.ai.busy).is_some();
        // the draft lives in egui's memory, one per plug-in, so switching plug-ins keeps what was typed
        let draft_id = egui::Id::new(("plugin_ai_draft", pid));
        let mut text = ui.data_mut(|d| d.get_temp::<String>(draft_id)).unwrap_or_default();
        let mut send: Option<String> = None;

        ui.add_space(theme::SP_4);
        chrome::section_label(ui, "Ask the AI");
        ui.horizontal(|ui| {
            ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                // right to left: the button is laid out first and the box takes the rest of the row
                let clicked = self.ai_button(ui, "Ask");
                if controls.stop && ui.button("Stop").on_hover_text("Stop the assistant").clicked() {
                    self.ai.cancel.store(true, Ordering::Relaxed);
                }
                let hint = if controls.prompt { "Ask the AI to change this plug-in… (Enter)" } else { "Connect a model in Settings to ask the AI" };
                let resp = ui.add_enabled(
                    controls.prompt,
                    egui::TextEdit::singleline(&mut text).hint_text(hint).desired_width(ui.available_width()),
                );
                let enter = resp.lost_focus() && ui.input(|i| i.key_pressed(egui::Key::Enter));
                if let Some(req) = bar_request(clicked || enter, blocked, &text) {
                    text.clear();
                    send = Some(req);
                }
            });
        });
        ui.horizontal_wrapped(|ui| {
            for &action in prompts::plugin_quick_actions(&inst.kind) {
                if self.ai_button(ui, action.0) {
                    send = Some(prompts::quick_action_request(action, pid, &label));
                }
            }
        });
        ui.data_mut(|d| d.insert_temp(draft_id, text));
        if let Some(request) = send {
            let (request, context) = bar_ask(project, pid, request);
            self.ask_ai_with(request, context);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn bar_sends_the_trimmed_text_on_click_or_enter() {
        assert_eq!(bar_request(true, false, "  more air "), Some("more air".to_string()));
    }

    #[test]
    fn bar_keeps_the_text_when_it_cannot_send() {
        assert_eq!(bar_request(true, true, "x"), None);
        assert_eq!(bar_request(true, false, "   "), None);
        assert_eq!(bar_request(false, false, "x"), None);
    }

    #[test]
    fn bar_request_carries_the_plugin_context() {
        use ondina_core::command::Command;
        use ondina_studio::{Session, Source};
        let mut s = Session::new(false);
        let add = |s: &mut Session, name: &str, args: serde_json::Value| s.execute(Command::from_tool_call(name, &args).unwrap(), Source::Gui).unwrap();
        let t = add(&mut s, "add_track", serde_json::json!({"kind": "audio", "name": "Vox"}))["track_id"].as_u64().unwrap();
        let fx = add(&mut s, "add_effect", serde_json::json!({"track_id": t, "kind": "compressor"}))["plugin_id"].as_u64().unwrap();
        let (request, context) = bar_ask(&s.project, fx, "more glue".into());
        assert_eq!(request, "more glue");
        assert_eq!(context, prompts::plugin_bar_context(&s.project, fx));
        let context = context.unwrap();
        for part in [format!("AI bar of plug-in {fx}"), "Vox".to_string(), "- threshold".to_string()] {
            assert!(context.contains(&part), "{part} missing from {context}");
        }
        assert_eq!(bar_ask(&s.project, 999_999, "x".into()).1, None);
    }
}
