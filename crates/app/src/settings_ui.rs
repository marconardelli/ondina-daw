//! The Settings window and the Bounce dialog.
//!
//! Settings apply as they change and are written to settings.json once the change is complete (no text field
//! being typed in, or the window closing), so there is no separate Save step. The one exception is a settings file
//! that could not be read at startup: it is replaced only when the person presses the button that says so.

use std::time::Duration;

use eframe::egui::{self, RichText};
use ondina_core::command::Command;
use ondina_studio::cloud_ai::CloudProvider;

use crate::app::OndinaApp;
use crate::bounce_menu;
use crate::dialog;
use crate::theme;
use crate::widgets::{self, Icon};

/// Choices of Settings → Provider: the value stored in settings.json and its label.
const PROVIDERS: &[(&str, &str)] = &[
    ("auto", "Auto"),
    ("local", "Local (offline)"),
    ("anthropic", "Claude API"),
    ("openai", "OpenAI"),
    ("google", "Google Gemini"),
    ("kimi", "Kimi (Moonshot)"),
];

/// Whether the Settings window should write the settings now: they differ from what was last written, and nobody is
/// typing in one of its fields (or the window is closing). Never while the file on disk could not be read.
pub fn should_save(last_saved: Option<&str>, now: &str, typing: bool, closing: bool, file_unread: bool) -> bool {
    !file_unread && last_saved != Some(now) && (!typing || closing)
}

/// The bit depths the bounce offers: value and label.
pub const BIT_DEPTHS: [(u16, &str); 3] = [(16, "16-bit"), (24, "24-bit"), (32, "32-bit float")];

/// The range line of the Bounce dialog.
pub fn bounce_range_text(cycle: Option<(f64, f64)>, beats_per_bar: f64) -> String {
    match cycle {
        Some((a, b)) => {
            let bpb = beats_per_bar.max(1e-9);
            let first = (a / bpb).floor() as i64 + 1;
            // the end is where the cycle stops: the last bar is the one before it, unless it ends mid-bar
            let last = ((b / bpb).ceil() as i64).max(first);
            if last == first {
                format!("Cycle range, bar {first}")
            } else {
                format!("Cycle range, bars {first} to {last}")
            }
        }
        None => "The whole song, with the effects' tail".into(),
    }
}

impl OndinaApp {
    /// The settings as they would be written, with the API key field applied.
    fn settings_now(&self) -> String {
        let mut s = self.settings.clone();
        s.api_key = Some(self.api_key_buf.trim().to_string()).filter(|k| !k.is_empty());
        serde_json::to_string(&s).unwrap_or_default()
    }

    fn apply_settings(&mut self, replace_unread: bool) {
        self.settings.api_key = Some(self.api_key_buf.trim().to_string()).filter(|k| !k.is_empty());
        let saved = if replace_unread { self.settings.save_replacing() } else { self.settings.try_save() };
        if let Some(mut s) = self.session.try_lock_for(Duration::from_millis(300)) {
            s.settings = self.settings.clone();
        }
        match saved {
            Ok(()) => {
                self.settings_saved = Some(self.settings_now());
                if replace_unread {
                    self.status = "Settings saved".into();
                }
            }
            Err(e) => self.status = format!("\u{26A0} Settings not saved: {e}"),
        }
    }

    pub(crate) fn settings_window(&mut self, ctx: &egui::Context) {
        if !self.show_settings {
            return;
        }
        if self.settings_saved.is_none() {
            // what was there when the window opened: nothing to write yet
            self.settings_saved = Some(self.settings_now());
        }
        let mut open = true;
        dialog::window("Settings")
            .open(&mut open)
            .resizable(false)
            .default_width(460.0)
            .show(ctx, |ui| {
                ui.set_width(460.0);
                if self.settings.file_unread {
                    ui.horizontal(|ui| {
                        widgets::icon(ui, Icon::Warning, 14.0, theme::MONITOR);
                        ui.label(RichText::new("settings.json could not be read when Ondina started.").color(theme::MONITOR));
                    });
                    dialog::hint(ui, "The defaults are in use and the file is left as it is. Changes here are not written until you replace it.");
                    if dialog::primary_button(ui, "Replace settings.json").clicked() {
                        self.apply_settings(true);
                    }
                    ui.add_space(theme::SP_2);
                }
                self.settings_ai(ui);
                self.settings_mcp(ui);
                self.settings_audio(ui);
                self.settings_general(ui);
                ui.add_space(theme::SP_4);
                let note = if self.settings.file_unread {
                    "Changes apply now; they are written once settings.json is replaced."
                } else {
                    "Changes apply and are saved as you make them."
                };
                dialog::hint(ui, note);
            });
        let typing = ctx.memory(|m| m.focused().is_some());
        let now = self.settings_now();
        if should_save(self.settings_saved.as_deref(), &now, typing, !open, self.settings.file_unread) {
            self.apply_settings(false);
        }
        if !open {
            self.show_settings = false;
            self.settings_saved = None;
        }
    }

    fn settings_ai(&mut self, ui: &mut egui::Ui) {
        dialog::section(ui, "AI assistant", true);
        dialog::row(ui, "Provider", |ui| {
            let shown = PROVIDERS.iter().find(|(v, _)| *v == self.settings.provider).map(|(_, l)| *l).unwrap_or("Auto");
            egui::ComboBox::from_id_salt("ai-provider")
                .width(260.0)
                .selected_text(shown)
                .show_ui(ui, |ui| {
                    for (v, l) in PROVIDERS {
                        if ui.selectable_label(self.settings.provider == *v, *l).clicked() {
                            self.settings.provider = v.to_string();
                        }
                    }
                });
        });
        if let Some(p) = self.settings.cloud_provider() {
            self.settings_cloud(ui, p);
            return;
        }
        dialog::row(ui, "Anthropic API key", |ui| {
            ui.add(egui::TextEdit::singleline(&mut self.api_key_buf).password(true).desired_width(260.0));
        });
        if std::env::var("ANTHROPIC_API_KEY").map(|k| !k.is_empty()).unwrap_or(false) {
            dialog::hint(ui, "Using ANTHROPIC_API_KEY from the environment.");
        }
        dialog::row(ui, "Local endpoint", |ui| {
            ui.add(egui::TextEdit::singleline(&mut self.settings.local_url).desired_width(260.0));
        });
        let bundled = match ondina_studio::local_ai::bundled_llm() {
            Some((_, m)) => format!("Bundled local model: {}", m.file_name().and_then(|f| f.to_str()).unwrap_or("")),
            None => "No bundled local model (tools/llm); any OpenAI-compatible server works.".into(),
        };
        dialog::hint(ui, bundled);
        dialog::row(ui, "Claude model", |ui| {
            ui.add(egui::TextEdit::singleline(&mut self.settings.model).desired_width(260.0));
        });
        dialog::row(ui, "Effort", |ui| {
            let efforts = ["low", "medium", "high", "xhigh", "max"];
            let mut i = efforts.iter().position(|e| *e == self.settings.effort).unwrap_or(1);
            if widgets::segmented(ui, &mut i, &efforts).changed() {
                self.settings.effort = efforts[i].to_string();
            }
        });
    }

    /// API key and model of OpenAI, Gemini or Kimi.
    fn settings_cloud(&mut self, ui: &mut egui::Ui, p: CloudProvider) {
        dialog::row(ui, &format!("{} API key", p.label()), |ui| {
            ui.add(egui::TextEdit::singleline(self.settings.cloud_key_field(p)).password(true).desired_width(260.0));
        });
        match ondina_studio::settings::Settings::cloud_key_env(p) {
            Some(var) => dialog::hint(ui, format!("Using {var} from the environment.")),
            None => dialog::hint(ui, format!("Get a key at {}.", p.key_page())),
        };
        dialog::row(ui, "Model", |ui| {
            ui.add(egui::TextEdit::singleline(self.settings.cloud_model_field(p)).hint_text(p.default_model()).desired_width(260.0));
        });
        dialog::hint(ui, format!("Empty uses {}. The assistant gets all the tools; requests and project data go to {}.", p.default_model(), p.label()));
    }

    fn settings_mcp(&mut self, ui: &mut egui::Ui) {
        dialog::section(ui, "MCP server", false);
        ui.checkbox(&mut self.settings.mcp_enabled, "Enable the HTTP MCP endpoint (applies at the next start)");
        dialog::row(ui, "Port", |ui| {
            ui.add(egui::DragValue::new(&mut self.settings.mcp_port).range(1024..=65535));
        });
    }

    fn settings_audio(&mut self, ui: &mut egui::Ui) {
        dialog::section(ui, "Audio & MIDI", false);
        if self.devices_cache.is_none() {
            self.devices_cache = self.exec(Command::ListAudioDevices {});
            if let Some(v) = self.exec(Command::ListMidiInputs {}) {
                self.midi_inputs_cache = v["inputs"]
                    .as_array()
                    .map(|a| a.iter().filter_map(|x| x.as_str().map(|s| s.to_string())).collect())
                    .unwrap_or_default();
            }
        }
        if let Some(d) = self.devices_cache.clone() {
            dialog::hint(ui, format!("Host: {}", d["host"].as_str().unwrap_or("")));
            dialog::row(ui, "Output device", |ui| {
                let current = self
                    .settings
                    .audio_output
                    .clone()
                    .or_else(|| d["current_output"].as_str().map(str::to_string))
                    .unwrap_or("Default".into());
                egui::ComboBox::from_id_salt("settings-output").width(260.0).selected_text(current).show_ui(ui, |ui| {
                    for o in d["outputs"].as_array().cloned().unwrap_or_default() {
                        let name = o.as_str().unwrap_or("").to_string();
                        if ui.selectable_label(self.settings.audio_output.as_deref() == Some(&name), &name).clicked() {
                            self.settings.audio_output = Some(name.clone());
                            self.exec(Command::SetAudioDevice { output: Some(name), input: None });
                            self.devices_cache = None;
                        }
                    }
                });
            });
        }
        dialog::row(ui, "MIDI input", |ui| {
            if self.midi_inputs_cache.is_empty() {
                ui.label(RichText::new("None found").color(theme::TEXT_3));
            }
            for name in self.midi_inputs_cache.clone() {
                let on = self.view.midi_input.as_deref() == Some(name.as_str());
                if ui.selectable_label(on, &name).clicked() {
                    self.exec(Command::ConnectMidiInput { name: Some(name) });
                }
            }
        });
    }

    fn settings_general(&mut self, ui: &mut egui::Ui) {
        dialog::section(ui, "Autosave and backups", false);
        dialog::row(ui, "Recovery copy every", |ui| {
            ui.add(egui::DragValue::new(&mut self.settings.autosave_minutes).range(0..=60).suffix(" min"));
        });
        dialog::hint(ui, "0 turns autosave off. After a crash Ondina offers the copy back at the next start.");
        ui.checkbox(&mut self.settings.keep_save_backup, "Keep a backup of the previous save");
        dialog::hint(ui, "Before Save replaces a project file, its previous version is kept next to it as <name>.ondina.bak.");
        dialog::section(ui, "Display", false);
        dialog::row(ui, "UI scale", |ui| {
            ui.add(egui::Slider::new(&mut self.settings.ui_scale, 0.75..=2.0));
        });
        dialog::hint(ui, "The new scale applies at the next start.");
        ui.checkbox(&mut self.settings.plugin_windows_separate, "Open plug-ins in separate windows")
            .on_hover_text("Off: plug-in windows float inside the main window");
        ui.checkbox(&mut self.settings.plugin_ai_beside_native, "Show the AI bar beside a plug-in's own window")
            .on_hover_text("A compact Ondina window with Bypass, presets, Default and the AI bar opens next to a VST3's own editor");
    }

    /// File > Bounce to WAV and Ctrl+B: what the bounce covers, its bit depth and loudness, then the file.
    pub(crate) fn bounce_window(&mut self, ctx: &egui::Context) {
        if !self.show_bounce {
            return;
        }
        dialog::backdrop(ctx, "bounce_backdrop");
        let cycle = self.view.project.cycle;
        let range = cycle.enabled.then_some((cycle.start, cycle.end));
        let bpb = self.view.project.time_signature.beats_per_bar();
        let mut go = false;
        let mut cancel = false;
        let shown = dialog::modal("Bounce to WAV", "bounce_dialog").show(ctx, |ui| {
            ui.set_width(380.0);
            dialog::row(ui, "Range", |ui| {
                ui.label(RichText::new(bounce_range_text(range, bpb)).color(theme::TEXT));
            });
            dialog::row(ui, "Bit depth", |ui| {
                let mut i = BIT_DEPTHS.iter().position(|(b, _)| *b == self.bounce_bits).unwrap_or(1);
                let labels: Vec<&str> = BIT_DEPTHS.iter().map(|(_, l)| *l).collect();
                if widgets::segmented(ui, &mut i, &labels).changed() {
                    self.bounce_bits = BIT_DEPTHS[i].0;
                }
            });
            dialog::row(ui, "Loudness", |ui| {
                egui::ComboBox::from_id_salt("bounce-loudness")
                    .width(200.0)
                    .selected_text(self.bounce_loudness.label())
                    .show_ui(ui, |ui| {
                        for l in bounce_menu::Loudness::ALL {
                            ui.selectable_value(&mut self.bounce_loudness, l, l.label());
                        }
                    });
            });
            dialog::hint(ui, "LUFS targets apply one gain to the whole mix and keep peaks under -1 dBTP, without a limiter.");
            ui.add_space(theme::SP_5);
            let row = egui::vec2(ui.available_width(), dialog::BUTTON_H);
            ui.allocate_ui_with_layout(row, egui::Layout::right_to_left(egui::Align::Center), |ui| {
                if dialog::primary_button(ui, "Bounce\u{2026}").clicked() {
                    go = true;
                }
                if dialog::secondary_button(ui, "Cancel").clicked() {
                    cancel = true;
                }
            });
        });
        if let Some(r) = shown {
            ctx.move_to_top(r.response.layer_id);
        }
        ctx.input_mut(|i| {
            if i.consume_key(egui::Modifiers::NONE, egui::Key::Escape) {
                cancel = true;
            }
            if i.consume_key(egui::Modifiers::NONE, egui::Key::Enter) {
                go = true;
            }
        });
        if go || cancel {
            self.show_bounce = false;
        }
        if go {
            self.bounce_dialog();
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn settings_are_written_once_a_change_is_complete() {
        // unchanged: nothing to write
        assert!(!should_save(Some("a"), "a", false, false, false));
        assert!(!should_save(Some("a"), "a", false, true, false));
        // changed by a click (no field focused): write now
        assert!(should_save(Some("a"), "b", false, false, false));
        // still typing in a field: wait, unless the window is closing
        assert!(!should_save(Some("a"), "b", true, false, false));
        assert!(should_save(Some("a"), "b", true, true, false));
        // a settings file that could not be read is never replaced behind the person's back
        assert!(!should_save(Some("a"), "b", false, true, true));
        assert!(should_save(None, "b", false, false, false));
    }

    #[test]
    fn bounce_range_names_the_cycle_bars_or_the_whole_song() {
        assert_eq!(bounce_range_text(Some((16.0, 32.0)), 4.0), "Cycle range, bars 5 to 8");
        assert_eq!(bounce_range_text(Some((0.0, 12.0)), 3.0), "Cycle range, bars 1 to 4");
        assert_eq!(bounce_range_text(Some((16.0, 18.0)), 4.0), "Cycle range, bar 5");
        assert_eq!(bounce_range_text(Some((16.0, 22.0)), 4.0), "Cycle range, bars 5 to 6");
        assert!(bounce_range_text(None, 4.0).starts_with("The whole song"));
        let depths: Vec<u16> = BIT_DEPTHS.iter().map(|(b, _)| *b).collect();
        assert_eq!(depths, [16, 24, 32]);
    }
}
