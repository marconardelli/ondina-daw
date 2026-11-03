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

/// What the bar sends to the assistant, without what a window adds (the tests of the plain bar).
#[cfg(test)]
fn bar_ask(project: &Project, pid: Id, request: String, meter: Option<PluginMeter>) -> (String, Option<String>) {
    bar_ask_window(project, pid, request, meter, prompts::WindowContext::default())
}

/// What the bar sends to the assistant: the request and, as context, the plug-in's id, kind, track and role, every
/// parameter with its value and range, for an insert its live meters, and what its window shows besides (the Drum
/// Synth's selected pad, the Sampler's sample and its pitch). None when the plug-in is gone.
fn bar_ask_window(project: &Project, pid: Id, request: String, meter: Option<PluginMeter>, window: prompts::WindowContext) -> (String, Option<String>) {
    (request, prompts::plugin_bar_context_window(project, pid, meter.as_ref(), window))
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
            let window = self.bar_window(project, pid);
            let (request, context) = bar_ask_window(project, pid, request, meter, window);
            self.ask_ai_with(request, context);
        }
    }

    /// What the window of `pid` adds to its bar's request: the pad selected in a Drum Synth window, the estimated
    /// pitch of a Sampler's sample (its audio is taken from the session in a short lock and analysed after it).
    fn bar_window(&mut self, project: &Project, pid: Id) -> prompts::WindowContext {
        let Some(inst) = project.find_plugin(pid) else {
            return prompts::WindowContext::default();
        };
        match (inst.kind.as_str(), inst.sample) {
            ("drum_synth", _) => prompts::WindowContext { pad: Some(self.plugin_windows.drum_pad(pid)), ..Default::default() },
            ("sampler", Some(f)) => {
                let data = self.exec_with(|s| s.pool.get(&f).cloned()).flatten();
                prompts::WindowContext { sample_pitch_hz: data.and_then(|d| ondina_dsp::audio_ops::sample_pitch(&d)), ..Default::default() }
            }
            _ => prompts::WindowContext::default(),
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

    #[test]
    fn bar_request_from_a_delay_window_carries_the_tempo() {
        use ondina_core::command::Command;
        use ondina_studio::{Session, Source};
        let mut s = Session::new(false);
        let add = |s: &mut Session, name: &str, args: serde_json::Value| s.execute(Command::from_tool_call(name, &args).unwrap(), Source::Gui).unwrap();
        let t = add(&mut s, "add_track", serde_json::json!({"kind": "audio", "name": "Vox"}))["track_id"].as_u64().unwrap();
        let fx = add(&mut s, "add_effect", serde_json::json!({"track_id": t, "kind": "tape_delay"}))["plugin_id"].as_u64().unwrap();
        let (request, context) = bar_ask(&s.project, fx, "a dotted eighth echo".into(), None);
        assert_eq!(request, "a dotted eighth echo");
        let context = context.unwrap();
        let inst = s.project.find_plugin(fx).unwrap();
        let note = prompts::delay_bar_note(&s.project, inst);
        assert!(!note.is_empty() && context.contains(&note), "{context}");
        assert!(context.contains("- time \"Time\" = 0.5"), "{context}");
        assert!(context.contains("120 BPM") && context.contains("1/8 (250 ms)"), "{context}");
    }

    #[test]
    fn bar_request_from_a_saturation_window_carries_the_curve_note() {
        use ondina_core::command::Command;
        use ondina_engine::PluginMeter;
        use ondina_studio::{Session, Source};
        let mut s = Session::new(false);
        let add = |s: &mut Session, name: &str, args: serde_json::Value| s.execute(Command::from_tool_call(name, &args).unwrap(), Source::Gui).unwrap();
        let t = add(&mut s, "add_track", serde_json::json!({"kind": "audio", "name": "Gtr"}))["track_id"].as_u64().unwrap();
        let od = add(&mut s, "add_effect", serde_json::json!({"track_id": t, "kind": "overdrive"}))["plugin_id"].as_u64().unwrap();
        let m = PluginMeter { input_peak: 0.5, output_peak: 0.6, gain_reduction_db: None };
        let (request, context) = bar_ask(&s.project, od, "warmer".into(), Some(m));
        assert_eq!(request, "warmer");
        assert_eq!(context, prompts::plugin_bar_context_live(&s.project, od, Some(&m)));
        let context = context.unwrap();
        for part in [prompts::saturation_bar_note("overdrive"), "Live meters now", "- drive \"Drive\" = 0.4", "Gtr", "At the live input peak"] {
            assert!(context.contains(part), "{part} missing from {context}");
        }
    }

    #[test]
    fn bar_request_from_a_drum_window_carries_the_selected_pad() {
        use ondina_core::command::Command;
        use ondina_studio::{Session, Source};
        let mut s = Session::new(false);
        let add = |s: &mut Session, name: &str, args: serde_json::Value| s.execute(Command::from_tool_call(name, &args).unwrap(), Source::Gui).unwrap();
        let t = add(&mut s, "add_track", serde_json::json!({"kind": "instrument", "name": "Drums", "instrument": "drum_synth"}))["track_id"].as_u64().unwrap();
        let drums = s.project.track(t).unwrap().instrument.as_ref().unwrap().id;
        let (request, context) = bar_ask_window(&s.project, drums, "tighter".into(), None, prompts::WindowContext { pad: Some(38), ..Default::default() });
        assert_eq!(request, "tighter");
        let context = context.unwrap();
        assert!(context.contains(prompts::drum_bar_note("drum_synth")), "{context}");
        assert!(context.contains("Snare (note 38") && context.contains("snare_tune \"Snare Tune\" = 190"), "{context}");
        assert!(context.contains("- snare_tune \"Snare Tune\" = 190"), "{context}");
        let plain = bar_ask(&s.project, drums, "tighter".into(), None).1.unwrap();
        assert!(plain.contains(prompts::drum_bar_note("drum_synth")) && !plain.contains("The pad selected"), "{plain}");
    }

    #[test]
    fn bar_request_from_a_sampler_window_carries_the_sample_note() {
        use ondina_core::command::Command;
        use ondina_studio::{Session, Source};
        let mut s = Session::new(false);
        let add = |s: &mut Session, name: &str, args: serde_json::Value| s.execute(Command::from_tool_call(name, &args).unwrap(), Source::Gui).unwrap();
        let t = add(&mut s, "add_track", serde_json::json!({"kind": "instrument", "name": "Keys", "instrument": "sampler"}))["track_id"].as_u64().unwrap();
        let sampler = s.project.track(t).unwrap().instrument.as_ref().unwrap().id;
        let context = bar_ask_window(&s.project, sampler, "in tune".into(), None, prompts::WindowContext::default()).1.unwrap();
        assert!(context.contains("no sample loaded yet"), "{context}");
        assert!(context.contains("- root_note \"Root Note\" = 60"), "{context}");
    }
}
