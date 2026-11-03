//! The AI bar drawn in the header of every plug-in window: a request box with its send button (Stop while the
//! assistant works) and the plug-in's ready-made requests. One component for the built-in plug-ins and the generic
//! VST3 window; the request goes to the assistant with the plug-in's settings as context.

use std::sync::atomic::Ordering;

use eframe::egui;
use ondina_core::model::{Id, Project};
use ondina_core::plugins;
use ondina_engine::PluginMeter;
use ondina_studio::prompts;

use crate::app::OndinaApp;
use crate::assistant::{ai_button_for, ai_controls, ask_blocked_reason, AiEngine};
use crate::{chrome, theme};

/// The request a press of the send button or of Enter in the box sends: the trimmed text, or None when nothing was
/// pressed, the assistant cannot be asked now or the box is empty (the text then stays in the box).
fn bar_request(pressed: bool, blocked: bool, text: &str) -> Option<String> {
    let t = text.trim();
    (pressed && !blocked && !t.is_empty()).then(|| t.to_string())
}

/// What the bar sends to the assistant: the request and, as context, the plug-in's id, kind, track and role, every
/// parameter with its value and range and, for an insert, its live meters (None when the plug-in is gone).
fn bar_ask(project: &Project, pid: Id, request: String, meter: Option<PluginMeter>) -> (String, Option<String>) {
    (request, prompts::plugin_bar_context_live(project, pid, meter.as_ref()))
}

/// What the person did on the AI bar in one frame.
#[derive(Debug, Default, PartialEq)]
pub(crate) struct BarOut {
    /// The request to send: the text in the box (button or Enter) or a ready-made request.
    pub send: Option<String>,
    /// Stop was pressed while the assistant works.
    pub stop: bool,
}

/// The widgets of the AI bar of plug-in `pid`: the request box with its button (Stop while the assistant works) and
/// the plug-in's ready-made requests, enabled for `engine` and `busy` like every AI button. It sends nothing itself:
/// plugin_ai_bar does, so a test can draw the bar without the app.
pub(crate) fn ai_bar_widgets(ui: &mut egui::Ui, project: &Project, pid: Id, engine: AiEngine, busy: bool) -> BarOut {
    let mut out = BarOut::default();
    let Some(inst) = project.find_plugin(pid) else {
        return out;
    };
    let label = plugins::plugin_label(inst);
    let controls = ai_controls(engine, busy);
    let reason = ask_blocked_reason(engine, busy);
    // the draft lives in egui's memory, one per plug-in, so switching plug-ins keeps what was typed
    let draft_id = egui::Id::new(("plugin_ai_draft", pid));
    let mut text = ui.data_mut(|d| d.get_temp::<String>(draft_id)).unwrap_or_default();

    ui.add_space(theme::SP_4);
    chrome::section_label(ui, "Ask the AI");
    ui.horizontal(|ui| {
        ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
            // right to left: the button is laid out first and the box takes the rest of the row
            let clicked = ai_button_for(ui, "Ask", reason);
            if controls.stop && ui.button("Stop").on_hover_text("Stop the assistant").clicked() {
                out.stop = true;
            }
            let hint = if controls.prompt { "Ask the AI to change this plug-in… (Enter)" } else { "Connect a model in Settings to ask the AI" };
            let resp = ui.add_enabled(
                controls.prompt,
                egui::TextEdit::singleline(&mut text).hint_text(hint).desired_width(ui.available_width()),
            );
            let enter = resp.lost_focus() && ui.input(|i| i.key_pressed(egui::Key::Enter));
            if let Some(req) = bar_request(clicked || enter, reason.is_some(), &text) {
                text.clear();
                out.send = Some(req);
            }
        });
    });
    ui.horizontal_wrapped(|ui| {
        for &action in prompts::plugin_bar_actions(project, pid) {
            if ai_button_for(ui, action.0, reason) {
                out.send = Some(prompts::quick_action_request(action, pid, &label));
            }
        }
    });
    ui.data_mut(|d| d.insert_temp(draft_id, text));
    out
}

impl OndinaApp {
    /// The AI bar of the plug-in `pid`, drawn in the header of every plug-in window.
    pub(crate) fn plugin_ai_bar(&mut self, ui: &mut egui::Ui, project: &Project, pid: Id) {
        let out = ai_bar_widgets(ui, project, pid, self.ai_engine(), self.ai.busy);
        if out.stop {
            self.ai.cancel.store(true, Ordering::Relaxed);
        }
        if let Some(request) = out.send {
            // an atomic read of the meter table, without the session lock
            let meter = self.shared.plugin_meter(pid);
            let (request, context) = bar_ask(project, pid, request, meter);
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
        let (request, context) = bar_ask(&s.project, fx, "more glue".into(), None);
        assert_eq!(request, "more glue");
        assert_eq!(context, prompts::plugin_bar_context(&s.project, fx));
        let context = context.unwrap();
        for part in [format!("AI bar of plug-in {fx}"), "Vox".to_string(), "- threshold".to_string()] {
            assert!(context.contains(&part), "{part} missing from {context}");
        }
        assert_eq!(bar_ask(&s.project, 999_999, "x".into(), None).1, None);
    }

    #[test]
    fn bar_request_carries_the_live_meters() {
        use ondina_core::command::Command;
        use ondina_engine::PluginMeter;
        use ondina_studio::{Session, Source};
        let mut s = Session::new(false);
        let add = |s: &mut Session, name: &str, args: serde_json::Value| s.execute(Command::from_tool_call(name, &args).unwrap(), Source::Gui).unwrap();
        let t = add(&mut s, "add_track", serde_json::json!({"kind": "audio", "name": "Bus"}))["track_id"].as_u64().unwrap();
        let fx = add(&mut s, "add_effect", serde_json::json!({"track_id": t, "kind": "compressor"}))["plugin_id"].as_u64().unwrap();
        let m = PluginMeter { input_peak: 0.5, output_peak: 0.25, gain_reduction_db: Some(-3.0) };
        let (request, context) = bar_ask(&s.project, fx, "more glue".into(), Some(m));
        assert_eq!(request, "more glue");
        assert_eq!(context, prompts::plugin_bar_context_live(&s.project, fx, Some(&m)));
        assert!(context.unwrap().contains("Live meters now"));
    }

    #[test]
    fn bar_request_from_a_synth_window_carries_the_synth_context() {
        use ondina_core::command::Command;
        use ondina_studio::{Session, Source};
        let mut s = Session::new(false);
        let add = |s: &mut Session, name: &str, args: serde_json::Value| s.execute(Command::from_tool_call(name, &args).unwrap(), Source::Gui).unwrap();
        let t = add(&mut s, "add_track", serde_json::json!({"kind": "instrument", "name": "Pad", "instrument": "analog_synth"}))["track_id"].as_u64().unwrap();
        let synth = s.project.track(t).unwrap().instrument.as_ref().unwrap().id;
        let (request, context) = bar_ask(&s.project, synth, "a warm pad".into(), None);
        assert_eq!(request, "a warm pad");
        let context = context.unwrap();
        assert_eq!(Some(&context), prompts::plugin_bar_context_live(&s.project, synth, None).as_ref());
        assert!(context.contains(prompts::synth_bar_note("analog_synth")), "{context}");
        assert!(context.contains("- cutoff \"Cutoff\" = "), "{context}");
    }
}
