//! Parameter editors of the selected plug-in in the Inspector: the built-in plug-ins' parameters and presets, and the
//! generic editor of third-party VST3 plug-ins (normalized 0..1 values, the plug-in's own window).

use eframe::egui::{self, RichText};
use ondina_core::command::Command;
use ondina_core::model::*;
use ondina_core::plugins;

use crate::app::OndinaApp;
use crate::theme;
use crate::widgets::{self, Icon};

/// The card the parameters of the selected plug-in sit in.
fn plugin_card() -> egui::Frame {
    egui::Frame::none()
        .fill(theme::RAISED)
        .stroke(egui::Stroke::new(1.0_f32, theme::BORDER))
        .rounding(theme::R_XL)
        .inner_margin(theme::SP_5)
}

impl OndinaApp {
    pub(crate) fn plugin_editor(&mut self, ui: &mut egui::Ui, p: &Project, pid: Id) {
        let Some(inst) = p.find_plugin(pid).cloned() else {
            return;
        };
        if inst.kind == "vst3" {
            self.vst3_editor(ui, &inst);
            return;
        }
        let Some(info) = plugins::find(&inst.kind) else {
            return;
        };
        plugin_card().show(ui, |ui| {
            ui.horizontal(|ui| {
                ui.label(RichText::new(info.name).font(egui::FontId::new(13.0, theme::semibold())).color(theme::TEXT));
                ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                    if widgets::icon_only_button(ui, Icon::Close, 18.0, theme::TEXT_3).on_hover_text("Close the parameters").clicked() {
                        self.selected_plugin = None;
                    }
                });
            });
            ui.label(RichText::new(info.description).small().color(theme::TEXT_DIM));
            let presets: Vec<_> = plugins::presets_for(&inst.kind).collect();
            if !presets.is_empty() {
                egui::ComboBox::from_id_salt(("plpreset", pid)).selected_text(inst.preset.clone().unwrap_or("Preset…".into())).show_ui(ui, |ui| {
                    for pr in presets {
                        if ui.selectable_label(inst.preset.as_deref() == Some(pr.name), pr.name).clicked() {
                            self.cmd(Command::LoadPreset { plugin_id: pid, preset: pr.name.to_string() });
                        }
                    }
                });
            }
            egui::Grid::new(("params", pid)).num_columns(2).striped(true).show(ui, |ui| {
                for pi in info.params {
                    let v = inst.param(pi.id);
                    ui.label(RichText::new(pi.name).small());
                    if !pi.choices.is_empty() {
                        let sel = pi.choices.get(v.round() as usize).copied().unwrap_or("?");
                        egui::ComboBox::from_id_salt(("ch", pid, pi.id)).selected_text(sel).width(120.0).show_ui(ui, |ui| {
                            for (ci, c) in pi.choices.iter().enumerate() {
                                if ui.selectable_label(v.round() as usize == ci, *c).clicked() {
                                    self.cmd(Command::SetPluginParam { plugin_id: pid, param: pi.id.to_string(), value: ci as f32 });
                                }
                            }
                        });
                    } else {
                        let mut x = v;
                        let log = pi.unit == "Hz" || (pi.unit == "s" && pi.max / pi.min.max(1e-4) > 100.0) || (pi.unit == "ms" && pi.max / pi.min.max(1e-4) > 100.0);
                        let mut s = egui::Slider::new(&mut x, pi.min..=pi.max).logarithmic(log).max_decimals(3);
                        if !pi.unit.is_empty() {
                            s = s.suffix(format!(" {}", pi.unit));
                        }
                        let resp = ui.add(s);
                        if resp.double_clicked() {
                            self.cmd(Command::SetPluginParam { plugin_id: pid, param: pi.id.to_string(), value: pi.default });
                        } else if resp.changed() {
                            self.cmd_merge(Command::SetPluginParam { plugin_id: pid, param: pi.id.to_string(), value: x }, format!("p{pid}{}", pi.id));
                        }
                    }
                    ui.end_row();
                }
            });
            ui.horizontal(|ui| {
                if self.ai_button(ui, "AI: design a sound") {
                    self.ask_ai(ondina_studio::prompts::design_sound(pid, info.name));
                }
            });
        });
    }

    /// Generic parameter editor for third-party VST3 plug-ins (normalized 0..1 values).
    fn vst3_editor(&mut self, ui: &mut egui::Ui, inst: &PluginInstance) {
        let pid = inst.id;
        plugin_card()
            .show(ui, |ui| {
                ui.horizontal(|ui| {
                    ui.label(
                        RichText::new(format!("VST3 · {}", plugins::plugin_label(inst)))
                        .font(egui::FontId::new(13.0, theme::semibold()))
                        .color(theme::TEXT),
                    );
                    ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                        if widgets::icon_only_button(ui, Icon::Close, 18.0, theme::TEXT_3).on_hover_text("Close the parameters").clicked() {
                            self.selected_plugin = None;
                        }
                    });
                });
                ui.label(
                    RichText::new(inst.vst3_path.clone().unwrap_or_default())
                        .small()
                        .color(theme::TEXT_DIM),
                );
                let not_loaded = self.view.unloaded_vst3.get(&pid).cloned();
                if let Some(reason) = &not_loaded {
                    ui.label(RichText::new(format!("Not loaded: {reason}")).small().color(theme::REC));
                }
                let open = ondina_dsp::vst3::open_editors().contains(&pid);
                let clicked = match &not_loaded {
                    Some(reason) if !open => {
                        ui.add_enabled(false, egui::Button::new("Open plug-in window"))
                            .on_disabled_hover_text(reason)
                            .clicked()
                    }
                    _ => ui
                        .button(if open {
                            "Close plug-in window"
                        } else {
                            "Open plug-in window"
                        })
                        .clicked(),
                };
                if clicked {
                    if open {
                        self.cmd(Command::ClosePluginEditor { plugin_id: pid });
                    } else {
                        self.exec(Command::OpenPluginEditor { plugin_id: pid });
                    }
                }
                ui.horizontal(|ui| {
                    widgets::icon(ui, Icon::Search, 13.0, theme::TEXT_3);
                    ui.add(egui::TextEdit::singleline(&mut self.vst_filter).hint_text("Filter parameters"));
                });
                let filter = self.vst_filter.to_lowercase();
                let matching: Vec<&Vst3Param> = inst
                    .vst3_params
                    .iter()
                    .filter(|p| filter.is_empty() || p.name.to_lowercase().contains(&filter))
                    .collect();
                ui.label(
                    RichText::new(format!(
                        "{} of {} parameters",
                        matching.len().min(80),
                        matching.len()
                    ))
                    .small()
                    .color(theme::TEXT_DIM),
                );
                egui::ScrollArea::vertical()
                    .max_height(320.0)
                    .show(ui, |ui| {
                        egui::Grid::new(("vst3p", pid))
                            .num_columns(2)
                            .striped(true)
                            .show(ui, |ui| {
                                for vp in matching.iter().take(80) {
                                    ui.label(RichText::new(&vp.name).small());
                                    let mut v = inst.param(&vp.id);
                                    let resp = ui
                                        .add(egui::Slider::new(&mut v, 0.0..=1.0).max_decimals(3));
                                    if resp.double_clicked() {
                                        self.cmd(Command::SetPluginParam {
                                            plugin_id: pid,
                                            param: vp.id.clone(),
                                            value: vp.default,
                                        });
                                    } else if resp.changed() {
                                        self.cmd_merge(
                                            Command::SetPluginParam {
                                                plugin_id: pid,
                                                param: vp.id.clone(),
                                                value: v,
                                            },
                                            format!("v{pid}{}", vp.id),
                                        );
                                    }
                                    ui.end_row();
                                }
                            });
                    });
            });
    }
}
