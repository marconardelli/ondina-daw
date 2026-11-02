//! The AI bar at the bottom of every plug-in's parameters: a request box with its send button and the plug-in's
//! ready-made requests. One component for the built-in plug-ins and the generic VST3 editor; the request goes to the
//! assistant with the plug-in's settings as context.

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

impl OndinaApp {
    /// The AI bar of the plug-in `pid`, drawn by plugin_editor and vst3_editor.
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
            self.ask_ai_with(request, prompts::plugin_bar_context(project, pid));
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
}
