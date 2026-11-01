//! Mixer: one channel strip per track plus the master.

use ondina_core::command::Command;
use ondina_core::model::*;
use ondina_core::plugins::{self, PluginCategory};
use eframe::egui::{self, vec2, Color32, RichText, Rounding, Sense};

use crate::app::{OndinaApp, LeftTab};
use crate::arrange::draw_meter;
use crate::theme;
use crate::widgets::{self, Icon};

const STRIP_W: f32 = 108.0;

impl OndinaApp {
    pub fn mixer_ui(&mut self, ui: &mut egui::Ui) {
        let p = self.project();
        egui::Frame::none().inner_margin(theme::SP_4).show(ui, |ui| {
            // the Master stays anchored on the right whatever the width; the tracks scroll beside it
            ui.with_layout(egui::Layout::right_to_left(egui::Align::Min), |ui| {
                self.channel_strip(ui, &p, None, MASTER_ID, &p.master);
                ui.separator();
                ui.with_layout(egui::Layout::left_to_right(egui::Align::Min), |ui| {
                    egui::ScrollArea::horizontal().auto_shrink([false, false]).show(ui, |ui| {
                        ui.horizontal_top(|ui| {
                            for t in &p.tracks {
                                self.channel_strip(ui, &p, Some(t), t.id, &t.channel);
                            }
                        });
                    });
                });
            });
        });
    }

    /// Double click on a VST3 slot of the strip: opens the plug-in's native window, or says in the status bar why it
    /// cannot (the engine does not load it).
    fn open_vst3_window(&mut self, plugin_id: Id, not_loaded: Option<&str>) {
        if let Some(reason) = not_loaded {
            self.status = format!("⚠ Plug-in not loaded: {reason}");
            return;
        }
        self.exec(Command::OpenPluginEditor { plugin_id });
    }

    /// Buses a track can send or route to (not itself).
    pub(crate) fn buses_for(p: &Project, id: Id) -> Vec<(Id, String)> {
        p.tracks
            .iter()
            .filter(|b| b.kind == TrackKind::Bus && b.id != id)
            .map(|b| (b.id, b.name.clone()))
            .collect()
    }

    /// Entries of a "send" menu: the existing buses, then a new reverb or delay bus with the send to it.
    pub(crate) fn add_send_items(&mut self, ui: &mut egui::Ui, id: Id, buses: &[(Id, String)]) {
        for (bid, bname) in buses {
            if ui.button(bname).clicked() {
                self.cmd(Command::AddSend {
                    track_id: id,
                    bus_id: *bid,
                    level_db: Some(-10.0),
                    pre_fader: None,
                });
                ui.close_menu();
            }
        }
        if ui.button("New reverb bus…").clicked() {
            if let Some(v) = self.exec(Command::AddTrack {
                kind: TrackKind::Bus,
                name: Some("Reverb".into()),
                patch: None,
                instrument: None,
                preset: None,
                index: None,
            }) {
                if let Some(bid) = v["track_id"].as_u64() {
                    self.cmd(Command::AddEffect {
                        track_id: bid,
                        kind: "reverb".into(),
                        preset: Some("Reverb Return 100%".into()),
                        index: None,
                    });
                    self.cmd(Command::AddSend {
                        track_id: id,
                        bus_id: bid,
                        level_db: Some(-10.0),
                        pre_fader: None,
                    });
                }
            }
            ui.close_menu();
        }
        if ui.button("New delay bus…").clicked() {
            if let Some(v) = self.exec(Command::AddTrack {
                kind: TrackKind::Bus,
                name: Some("Delay".into()),
                patch: None,
                instrument: None,
                preset: None,
                index: None,
            }) {
                if let Some(bid) = v["track_id"].as_u64() {
                    self.cmd(Command::AddEffect {
                        track_id: bid,
                        kind: "stereo_delay".into(),
                        preset: Some("Delay Return 100%".into()),
                        index: None,
                    });
                    self.cmd(Command::AddSend {
                        track_id: id,
                        bus_id: bid,
                        level_db: Some(-12.0),
                        pre_fader: None,
                    });
                }
            }
            ui.close_menu();
        }
    }

    /// Entries of the output menu: Master or one of the buses.
    pub(crate) fn output_items(&mut self, ui: &mut egui::Ui, id: Id, buses: &[(Id, String)]) {
        if ui.button("Master").clicked() {
            self.cmd(Command::SetTrackOutput {
                track_id: id,
                bus_id: None,
            });
            ui.close_menu();
        }
        for (bid, bname) in buses {
            if ui.button(bname).clicked() {
                self.cmd(Command::SetTrackOutput {
                    track_id: id,
                    bus_id: Some(*bid),
                });
                ui.close_menu();
            }
        }
    }

    fn channel_strip(
        &mut self,
        ui: &mut egui::Ui,
        p: &Project,
        track: Option<&Track>,
        id: Id,
        ch: &Channel,
    ) {
        let selected = self.selected_track == Some(id) && id != MASTER_ID;
        let color = track.map(|t| theme::rgb(t.color)).unwrap_or(theme::ACCENT);
        egui::Frame::none()
            .fill(if selected {
                theme::SELECTED_BG
            } else {
                theme::LANE_ALT
            })
            .rounding(4.0_f32)
            .inner_margin(4.0)
            .show(ui, |ui| {
                ui.set_width(STRIP_W);
                ui.vertical(|ui| {
                    // name
                    let name = track.map(|t| t.name.clone()).unwrap_or("Master".into());
                    let (r, resp) = ui.allocate_exact_size(vec2(STRIP_W, 18.0), Sense::click());
                    ui.painter().rect_filled(r, Rounding::same(3.0_f32), color);
                    ui.painter().text(
                        r.center(),
                        egui::Align2::CENTER_CENTER,
                        short(&name, 14),
                        egui::FontId::proportional(11.5),
                        Color32::BLACK,
                    );
                    if resp.clicked() && id != MASTER_ID {
                        self.select_track(id);
                    }
                    // instrument
                    if let Some(t) = track {
                        if let Some(inst) = &t.instrument {
                            let label = plugins::plugin_label(inst);
                            let not_loaded = self.view.unloaded_vst3.get(&inst.id).cloned();
                            let mut txt = RichText::new(short(&label, 15)).small();
                            txt = if not_loaded.is_some() {
                                txt.color(theme::TEXT_DIM).strikethrough()
                            } else {
                                txt.color(theme::ACCENT)
                            };
                            let mut resp = ui.add_sized(vec2(STRIP_W, 18.0), egui::Button::new(txt));
                            if let Some(tip) = slot_tooltip(&label, 15, not_loaded.as_deref()) {
                                resp = resp.on_hover_text(tip);
                            }
                            if resp.clicked() {
                                self.selected_track = Some(id);
                                self.selected_plugin = Some(inst.id);
                                self.left = LeftTab::Inspector;
                                self.show_left = true;
                            }
                            if resp.double_clicked() && inst.kind == "vst3" {
                                self.open_vst3_window(inst.id, not_loaded.as_deref());
                            }
                        } else {
                            ui.add_space(22.0);
                        }
                    } else {
                        ui.add_space(22.0);
                    }
                    // inserts
                    ui.label(RichText::new("Inserts").small().color(theme::TEXT_DIM));
                    for fx in &ch.inserts {
                        let label = plugins::plugin_label(fx);
                        let not_loaded = self.view.unloaded_vst3.get(&fx.id).cloned();
                        let is_vst3 = fx.kind == "vst3";
                        let mut txt = RichText::new(short(&label, 15)).small();
                        txt = if not_loaded.is_some() {
                            txt.color(theme::TEXT_DIM).strikethrough()
                        } else if fx.bypass {
                            txt.color(theme::TEXT_DIM)
                        } else {
                            txt.color(Color32::WHITE)
                        };
                        let mut resp = ui.add_sized(
                            vec2(STRIP_W, 17.0),
                            egui::Button::new(txt).fill(if self.selected_plugin == Some(fx.id) {
                                Color32::from_rgb(90, 70, 30)
                            } else {
                                theme::LANE
                            }),
                        );
                        if let Some(tip) = slot_tooltip(&label, 15, not_loaded.as_deref()) {
                            resp = resp.on_hover_text(tip);
                        }
                        if resp.clicked() {
                            self.selected_plugin = Some(fx.id);
                            if id != MASTER_ID {
                                self.selected_track = Some(id);
                            }
                            self.left = LeftTab::Inspector;
                            self.show_left = true;
                        }
                        let pid = fx.id;
                        let byp = fx.bypass;
                        if resp.double_clicked() && is_vst3 {
                            self.open_vst3_window(pid, not_loaded.as_deref());
                        }
                        crate::drag_cancel::menu_unless_dragging(&resp, self.drag.is_some(), |ui| {
                            if is_vst3 && ui.button("Open plug-in window").clicked() {
                                self.open_vst3_window(pid, not_loaded.as_deref());
                                ui.close_menu();
                            }
                            if ui.button(if byp { "Enable" } else { "Bypass" }).clicked() {
                                self.cmd(Command::SetPluginBypass {
                                    plugin_id: pid,
                                    bypass: !byp,
                                });
                                ui.close_menu();
                            }
                            if ui.button("Remove").clicked() {
                                self.cmd(Command::RemovePlugin { plugin_id: pid });
                                ui.close_menu();
                            }
                        });
                    }
                    widgets::icon_menu_button(ui, Icon::Effect, RichText::new("effect").small(), |ui| {
                        effect_menu(ui, |kind| {
                            self.cmd(Command::AddEffect {
                                track_id: id,
                                kind: kind.to_string(),
                                preset: None,
                                index: None,
                            });
                        });
                    });
                    // sends
                    if let Some(t) = track {
                        if t.kind != TrackKind::Bus || !t.channel.sends.is_empty() {
                            ui.label(RichText::new("Sends").small().color(theme::TEXT_DIM));
                        }
                        for (si, s) in t.channel.sends.iter().enumerate() {
                            let bus = p
                                .track(s.bus_id)
                                .map(|b| b.name.clone())
                                .unwrap_or("?".into());
                            ui.horizontal(|ui| {
                                ui.label(RichText::new(short(&bus, 6)).small());
                                let mut lv = s.level_db;
                                if ui
                                    .add(
                                        egui::DragValue::new(&mut lv)
                                            .range(-60.0..=6.0)
                                            .speed(0.2)
                                            .suffix("dB")
                                            .max_decimals(1),
                                    )
                                    .changed()
                                {
                                    self.cmd_merge(
                                        Command::SetSendLevel {
                                            track_id: id,
                                            send_index: si,
                                            level_db: lv,
                                        },
                                        format!("send{id}-{si}"),
                                    );
                                }
                                if ui.small_button("×").clicked() {
                                    self.cmd(Command::RemoveSend {
                                        track_id: id,
                                        send_index: si,
                                    });
                                }
                            });
                        }
                        let buses = Self::buses_for(p, id);
                        widgets::icon_menu_button(ui, Icon::Send, RichText::new("send").small(), |ui| {
                            self.add_send_items(ui, id, &buses);
                        });
                        // output
                        let out = match t.channel.output {
                            Output::Master => "→ Master".to_string(),
                            Output::Bus(b) => format!(
                                "→ {}",
                                p.track(b).map(|x| x.name.clone()).unwrap_or("?".into())
                            ),
                        };
                        ui.menu_button(RichText::new(short(&out, 14)).small(), |ui| {
                            self.output_items(ui, id, &buses);
                        });
                    }
                    ui.add_space(4.0);
                    // pan
                    let mut pan = ch.pan;
                    ui.spacing_mut().slider_width = STRIP_W - 8.0;
                    if ui
                        .add(egui::Slider::new(&mut pan, -1.0..=1.0).show_value(false))
                        .on_hover_text(format!("Pan {:+.0}", pan * 64.0))
                        .double_clicked()
                    {
                        self.cmd(Command::SetPan {
                            track_id: id,
                            pan: 0.0,
                        });
                    } else if (pan - ch.pan).abs() > 1e-6 {
                        self.cmd_merge(Command::SetPan { track_id: id, pan }, format!("pan{id}"));
                    }
                    // fader + meter
                    ui.horizontal(|ui| {
                        let mut v = ch.volume_db;
                        ui.spacing_mut().slider_width = 130.0;
                        let resp = ui.add(
                            egui::Slider::new(&mut v, -60.0..=6.0)
                                .vertical()
                                .show_value(false),
                        );
                        if resp.double_clicked() {
                            self.cmd(Command::SetVolume {
                                track_id: id,
                                db: 0.0,
                            });
                        } else if resp.changed() {
                            self.cmd_merge(
                                Command::SetVolume {
                                    track_id: id,
                                    db: v,
                                },
                                format!("vol{id}"),
                            );
                        }
                        let (mr, _) = ui.allocate_exact_size(vec2(14.0, 130.0), Sense::hover());
                        draw_meter(
                            ui.painter(),
                            mr,
                            self.meters.get(&id).copied().unwrap_or([0.0; 2]),
                        );
                        ui.vertical(|ui| {
                            ui.label(
                                RichText::new(format!("{:+.1}", ch.volume_db))
                                    .small()
                                    .monospace(),
                            );
                            let m = self.meters.get(&id).copied().unwrap_or([0.0; 2]);
                            let pk = ondina_dsp::util::gain_to_db(m[0].max(m[1]));
                            ui.label(
                                RichText::new(if pk < -59.0 {
                                    "-∞".into()
                                } else {
                                    format!("{pk:.1}")
                                })
                                .small()
                                .monospace()
                                .color(if pk > -0.5 {
                                    theme::REC
                                } else {
                                    theme::TEXT_DIM
                                }),
                            );
                        });
                    });
                    ui.horizontal(|ui| {
                        if ui
                            .add(
                                egui::Button::new(RichText::new("M").small().color(if ch.mute {
                                    Color32::BLACK
                                } else {
                                    Color32::WHITE
                                }))
                                .fill(if ch.mute {
                                    theme::MUTE
                                } else {
                                    theme::LANE
                                }),
                            )
                            .clicked()
                        {
                            self.cmd(Command::SetMute {
                                track_id: id,
                                mute: !ch.mute,
                            });
                        }
                        if id != MASTER_ID
                            && ui
                                .add(
                                    egui::Button::new(RichText::new("S").small().color(
                                        if ch.solo {
                                            Color32::BLACK
                                        } else {
                                            Color32::WHITE
                                        },
                                    ))
                                    .fill(if ch.solo {
                                        theme::SOLO
                                    } else {
                                        theme::LANE
                                    }),
                                )
                                .clicked()
                        {
                            self.cmd(Command::SetSolo {
                                track_id: id,
                                solo: !ch.solo,
                            });
                        }
                        if let Some(t) = track {
                            if t.kind != TrackKind::Bus
                                && ui
                                    .add(
                                        egui::Button::new(RichText::new("R").small().color(
                                            if t.record_armed {
                                                Color32::BLACK
                                            } else {
                                                Color32::WHITE
                                            },
                                        ))
                                        .fill(
                                            if t.record_armed {
                                                theme::REC
                                            } else {
                                                theme::LANE
                                            },
                                        ),
                                    )
                                    .clicked()
                            {
                                self.cmd(Command::SetRecordArm {
                                    track_id: id,
                                    armed: !t.record_armed,
                                });
                            }
                        }
                    });
                });
            });
    }
}

pub fn effect_menu(ui: &mut egui::Ui, mut on_pick: impl FnMut(&'static str)) {
    let mut groups: Vec<&str> = plugins::registry()
        .iter()
        .filter(|p| p.category == PluginCategory::Effect)
        .map(|p| p.group)
        .collect();
    groups.dedup();
    let mut seen = std::collections::HashSet::new();
    for g in groups {
        if !seen.insert(g) {
            continue;
        }
        ui.menu_button(g, |ui| {
            for p in plugins::registry()
                .iter()
                .filter(|p| p.category == PluginCategory::Effect && p.group == g)
            {
                if ui.button(p.name).on_hover_text(p.description).clicked() {
                    on_pick(p.kind);
                    ui.close_menu();
                }
            }
        });
    }
}

/// Hover text for a plug-in slot of the strip: the full name when `short` cut it to `max_chars`, and why the
/// plug-in is not loaded. None when there is nothing to add.
pub fn slot_tooltip(label: &str, max_chars: usize, not_loaded: Option<&str>) -> Option<String> {
    let cut = label.chars().count() > max_chars;
    match (cut, not_loaded) {
        (false, None) => None,
        (true, None) => Some(label.to_string()),
        (false, Some(reason)) => Some(format!("Not loaded: {reason}")),
        (true, Some(reason)) => Some(format!("{label}\nNot loaded: {reason}")),
    }
}

pub fn short(s: &str, n: usize) -> String {
    if s.chars().count() > n {
        format!("{}…", s.chars().take(n - 1).collect::<String>())
    } else {
        s.to_string()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn slot_tooltip_is_empty_for_a_short_loaded_plugin() {
        assert_eq!(slot_tooltip("Compressor", 15, None), None);
    }

    #[test]
    fn slot_tooltip_shows_the_full_name_when_cut() {
        assert_eq!(slot_tooltip("Valhalla VintageVerb", 15, None), Some("Valhalla VintageVerb".into()));
        assert_eq!(slot_tooltip("Fifteen chars!!", 15, None), None);
        assert_eq!(short("Fifteen chars!!", 15), "Fifteen chars!!");
    }

    #[test]
    fn slot_tooltip_says_why_a_plugin_is_not_loaded() {
        assert_eq!(
            slot_tooltip("Foo", 15, Some("X is not a .vst3 plug-in bundle")),
            Some("Not loaded: X is not a .vst3 plug-in bundle".into())
        );
        assert_eq!(
            slot_tooltip("Valhalla VintageVerb", 15, Some("X is not a .vst3 plug-in bundle")),
            Some("Valhalla VintageVerb\nNot loaded: X is not a .vst3 plug-in bundle".into())
        );
    }
}
