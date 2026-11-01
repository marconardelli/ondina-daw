//! Left panel: Inspector (track, instrument, plug-in parameters) and Library (patches).

use ondina_core::command::Command;
use ondina_core::model::*;
use ondina_core::plugins::{self, PluginCategory};
use eframe::egui::{self, Color32, RichText};

use crate::app::{OndinaApp, LeftTab};
use crate::mixer::effect_menu;
use crate::region_edit::{self, ColorChoice};
use crate::theme;

impl OndinaApp {
    pub fn left_ui(&mut self, ui: &mut egui::Ui) {
        ui.horizontal(|ui| {
            ui.selectable_value(&mut self.left, LeftTab::Inspector, "ℹ Inspector");
            ui.selectable_value(&mut self.left, LeftTab::Library, "📚 Library");
        });
        ui.separator();
        egui::ScrollArea::vertical()
            .auto_shrink([false, false])
            .show(ui, |ui| match self.left {
                LeftTab::Inspector => self.inspector(ui),
                LeftTab::Library => self.library(ui),
            });
    }

    fn library(&mut self, ui: &mut egui::Ui) {
        let p = self.project();
        let target = self.selected_track.and_then(|t| p.track(t)).cloned();
        ui.label(
            RichText::new(match &target {
                Some(t) => format!(
                    "Click a patch to load it on '{}', or ➕ for a new track.",
                    t.name
                ),
                None => "Click ➕ to create a track with the patch.".into(),
            })
            .small()
            .color(theme::TEXT_DIM),
        );
        self.vst_library(ui);
        let mut cats: Vec<&str> = plugins::PATCHES.iter().map(|p| p.category).collect();
        cats.sort();
        cats.dedup();
        for c in cats {
            egui::CollapsingHeader::new(c)
                .default_open(true)
                .show(ui, |ui| {
                    for patch in plugins::PATCHES.iter().filter(|x| x.category == c) {
                        ui.horizontal(|ui| {
                            let kind = if patch.instrument.is_some() {
                                TrackKind::Instrument
                            } else {
                                TrackKind::Audio
                            };
                            if ui
                                .small_button("➕")
                                .on_hover_text("New track with this patch")
                                .clicked()
                            {
                                self.add_track(kind, Some(patch.name));
                            }
                            let compatible =
                                target.as_ref().map(|t| t.kind == kind).unwrap_or(false);
                            if ui
                                .add_enabled(compatible, egui::Button::new(patch.name).frame(false))
                                .on_hover_text(format!(
                                    "{} + {}",
                                    patch.instrument.map(|i| i.0).unwrap_or("audio"),
                                    patch
                                        .effects
                                        .iter()
                                        .map(|e| e.0)
                                        .collect::<Vec<_>>()
                                        .join(", ")
                                ))
                                .clicked()
                            {
                                if let Some(t) = &target {
                                    self.cmd(Command::ApplyPatch {
                                        track_id: t.id,
                                        patch: patch.name.to_string(),
                                    });
                                }
                            }
                        });
                    }
                });
        }
    }

    fn inspector(&mut self, ui: &mut egui::Ui) {
        let p = self.project();
        // project
        egui::CollapsingHeader::new("Project")
            .default_open(false)
            .show(ui, |ui| {
                let mut name = p.name.clone();
                if ui.text_edit_singleline(&mut name).lost_focus() && name != p.name {
                    self.cmd(Command::SetProjectName { name });
                }
                ui.label(format!(
                    "{} · {}/{} · {:.1} bpm",
                    p.key.name(),
                    p.time_signature.numerator,
                    p.time_signature.denominator,
                    p.tempo.initial_bpm()
                ));
                let mut ci = p.count_in_bars;
                ui.horizontal(|ui| {
                    ui.label("Count-in bars");
                    if ui.add(egui::DragValue::new(&mut ci).range(0..=4)).changed() {
                        self.cmd(Command::SetCountIn { bars: ci });
                    }
                });
                let mut notes = p.notes.clone();
                ui.label("Notes / lyrics");
                if ui
                    .add(egui::TextEdit::multiline(&mut notes).desired_rows(3))
                    .lost_focus()
                    && notes != p.notes
                {
                    self.cmd(Command::SetProjectNotes { notes });
                }
            });

        let Some(t) = self.selected_track.and_then(|id| p.track(id)).cloned() else {
            ui.label(RichText::new("Select a track to inspect it.").color(theme::TEXT_DIM));
            self.surround_inspector(ui, &p, None);
            self.master_inspector(ui, &p);
            return;
        };
        let tid = t.id;
        egui::CollapsingHeader::new(RichText::new(format!("Track: {}", t.name)).strong())
            .default_open(true)
            .show(ui, |ui| {
                if self.rename_buf.is_empty() || ui.memory(|m| !m.has_focus(ui.id().with("rename")))
                {
                    self.rename_buf = t.name.clone();
                }
                let resp = ui.add(
                    egui::TextEdit::singleline(&mut self.rename_buf).id(ui.id().with("rename")),
                );
                if resp.lost_focus()
                    && self.rename_buf != t.name
                    && !self.rename_buf.trim().is_empty()
                {
                    self.cmd(Command::RenameTrack {
                        track_id: tid,
                        name: self.rename_buf.trim().to_string(),
                    });
                }
                ui.horizontal(|ui| {
                    let mut c = [t.color[0], t.color[1], t.color[2]];
                    if ui.color_edit_button_srgb(&mut c).changed() {
                        self.cmd_merge(
                            Command::SetTrackColor {
                                track_id: tid,
                                color: format!("#{:02X}{:02X}{:02X}", c[0], c[1], c[2]),
                            },
                            format!("color{tid}"),
                        );
                    }
                    ui.label(format!("{:?} track · id {}", t.kind, t.id));
                });
                let mut vol = t.channel.volume_db;
                if ui
                    .add(egui::Slider::new(&mut vol, -60.0..=6.0).text("Volume dB"))
                    .changed()
                {
                    self.cmd_merge(
                        Command::SetVolume {
                            track_id: tid,
                            db: vol,
                        },
                        format!("vol{tid}"),
                    );
                }
                let mut pan = t.channel.pan;
                if ui
                    .add(egui::Slider::new(&mut pan, -1.0..=1.0).text("Pan"))
                    .changed()
                {
                    self.cmd_merge(Command::SetPan { track_id: tid, pan }, format!("pan{tid}"));
                }
                if t.kind == TrackKind::Instrument {
                    ui.horizontal(|ui| {
                        if ui
                            .button("🎹 Musical Typing")
                            .on_hover_text("Play this instrument with the computer keyboard")
                            .clicked()
                        {
                            self.musical_typing = !self.musical_typing;
                        }
                        if ui.button("Preview C4").clicked() {
                            self.cmd(Command::PreviewNote {
                                track_id: tid,
                                pitch: 60,
                                velocity: Some(100),
                                duration: Some(0.6),
                            });
                        }
                    });
                }
            });

        self.surround_inspector(ui, &p, Some(&t));
        if t.kind == TrackKind::Instrument {
            egui::CollapsingHeader::new("Instrument")
                .default_open(true)
                .show(ui, |ui| {
                    let cur = t
                        .instrument
                        .as_ref()
                        .map(|i| i.kind.clone())
                        .unwrap_or_default();
                    egui::ComboBox::from_id_salt("inst")
                        .selected_text(match &t.instrument {
                            Some(i) if i.kind == "vst3" => format!("VST3 · {}", plugins::plugin_label(i)),
                            Some(i) => plugins::plugin_label(i),
                            None => "None".to_string(),
                        })
                        .show_ui(ui, |ui| {
                            for info in plugins::registry()
                                .iter()
                                .filter(|p| p.category == PluginCategory::Instrument)
                            {
                                if ui
                                    .selectable_label(cur == info.kind, info.name)
                                    .on_hover_text(info.description)
                                    .clicked()
                                {
                                    self.cmd(Command::SetInstrument {
                                        track_id: tid,
                                        kind: info.kind.to_string(),
                                        preset: None,
                                    });
                                }
                            }
                        });
                    if let Some(inst) = &t.instrument {
                        let iid = inst.id;
                        egui::ComboBox::from_id_salt("preset")
                            .selected_text(inst.preset.clone().unwrap_or("Preset…".into()))
                            .show_ui(ui, |ui| {
                                for pr in plugins::presets_for(&inst.kind) {
                                    if ui
                                        .selectable_label(
                                            inst.preset.as_deref() == Some(pr.name),
                                            pr.name,
                                        )
                                        .clicked()
                                    {
                                        self.cmd(Command::LoadPreset {
                                            plugin_id: iid,
                                            preset: pr.name.to_string(),
                                        });
                                    }
                                }
                            });
                        if inst.kind == "sampler" {
                            if ui.button("Load sample…").clicked() {
                                if let Some(f) = rfd::FileDialog::new()
                                    .add_filter(
                                        "Audio",
                                        &["wav", "aif", "aiff", "flac", "mp3", "ogg"],
                                    )
                                    .pick_file()
                                {
                                    self.exec(Command::SetSamplerSample {
                                        plugin_id: iid,
                                        path: f.display().to_string(),
                                    });
                                }
                            }
                            if let Some(s) = inst.sample.and_then(|id| p.audio_file(id)) {
                                ui.label(RichText::new(&s.name).small());
                            }
                        }
                        if ui
                            .selectable_label(self.selected_plugin == Some(iid), "Edit parameters")
                            .clicked()
                        {
                            self.selected_plugin = Some(iid);
                        }
                    }
                });
            egui::CollapsingHeader::new("MIDI Effects")
                .default_open(false)
                .show(ui, |ui| {
                    for fx in &t.midi_fx {
                        let name = plugins::plugin_label(fx);
                        ui.horizontal(|ui| {
                            if ui
                                .selectable_label(self.selected_plugin == Some(fx.id), name)
                                .clicked()
                            {
                                self.selected_plugin = Some(fx.id);
                            }
                            if ui.small_button("×").clicked() {
                                self.cmd(Command::RemovePlugin { plugin_id: fx.id });
                            }
                        });
                    }
                    ui.menu_button("＋ MIDI effect", |ui| {
                        for info in plugins::registry()
                            .iter()
                            .filter(|p| p.category == PluginCategory::MidiEffect)
                        {
                            if ui
                                .button(info.name)
                                .on_hover_text(info.description)
                                .clicked()
                            {
                                self.cmd(Command::AddMidiEffect {
                                    track_id: tid,
                                    kind: info.kind.to_string(),
                                    preset: None,
                                });
                                ui.close_menu();
                            }
                        }
                    });
                });
        }

        egui::CollapsingHeader::new("Audio Effects")
            .default_open(true)
            .show(ui, |ui| {
                self.insert_list(ui, tid, &t.channel.inserts);
            });

        if let Some(pid) = self.selected_plugin {
            if p.plugin_owner(pid).is_some() {
                self.plugin_editor(ui, &p, pid);
            }
        }

        if let Some((rt, r)) = self.selected_region.and_then(|id| p.find_region(id)) {
            // a fixed id: renaming the region must not close the section
            egui::CollapsingHeader::new(format!("Region: {}", r.name))
                .id_salt("region-inspector")
                .default_open(false)
                .show(ui, |ui| {
                    let rid = r.id;
                    let fid = ui.id().with("region-rename");
                    // reload the text unless the user is typing; the id keeps a commit on the region it was typed for
                    if !ui.memory(|m| m.has_focus(fid)) || self.edits.region_name_edit.is_none() {
                        self.edits.region_name_edit = Some((rid, r.name.clone()));
                    }
                    let mut commit = None;
                    if let Some((eid, buf)) = self.edits.region_name_edit.as_mut() {
                        let resp = ui.add(egui::TextEdit::singleline(buf).id(fid).hint_text("Region name"));
                        // Esc takes the focus away too: it cancels, the text is reloaded on the next frame
                        if resp.lost_focus() && !ui.input(|i| i.key_pressed(egui::Key::Escape)) {
                            commit = Some((*eid, buf.clone()));
                        }
                    }
                    if let Some((eid, typed)) = commit {
                        let old = p.find_region(eid).map(|(_, r)| r.name.clone());
                        if let Some(c) = old.and_then(|o| region_edit::name_command(eid, &o, &typed)) {
                            self.cmd(c);
                        }
                    }
                    ui.label("Color");
                    if let Some(choice) = region_edit::color_swatches(ui, r.color) {
                        if let Some(c) = region_edit::color_command(rid, r.color, choice) {
                            self.cmd(c);
                        }
                    }
                    ui.horizontal(|ui| {
                        let mut c = r.color.unwrap_or(rt.color);
                        if ui.color_edit_button_srgb(&mut c).on_hover_text("Any color").changed() {
                            if let Some(cmd) = region_edit::color_command(rid, r.color, ColorChoice::Color(c)) {
                                // one undo step for a whole drag in the picker
                                self.cmd_merge(cmd, format!("rcolor{rid}"));
                            }
                        }
                        ui.label(RichText::new("Custom").small().color(theme::TEXT_DIM));
                    });
                    ui.label(format!(
                        "Start {} · length {:.2} beats · loops {}",
                        p.bar_beat_string(r.start),
                        r.length,
                        r.loop_count
                    ));
                    let mut lc = r.loop_count;
                    if ui
                        .add(
                            egui::DragValue::new(&mut lc)
                                .range(1..=128)
                                .prefix("Loop ×"),
                        )
                        .changed()
                    {
                        self.cmd_merge(
                            Command::SetRegionProperties {
                                region_id: rid,
                                name: None,
                                muted: None,
                                loop_count: Some(lc),
                                transpose: None,
                                velocity_offset: None,
                                gain_db: None,
                                fade_in: None,
                                fade_out: None,
                                reverse: None,
                                color: None,
                            },
                            format!("loop{rid}"),
                        );
                    }
                    if let RegionContent::Midi(m) = &r.content {
                        let mut vo = m.velocity_offset;
                        if ui
                            .add(
                                egui::DragValue::new(&mut vo)
                                    .range(-100..=100)
                                    .prefix("Velocity offset "),
                            )
                            .changed()
                        {
                            self.cmd_merge(
                                Command::SetRegionProperties {
                                    region_id: rid,
                                    name: None,
                                    muted: None,
                                    loop_count: None,
                                    transpose: None,
                                    velocity_offset: Some(vo),
                                    gain_db: None,
                                    fade_in: None,
                                    fade_out: None,
                                    reverse: None,
                                    color: None,
                                },
                                format!("vo{rid}"),
                            );
                        }
                    }
                });
        }

        ui.separator();
        self.master_inspector(ui, &p);
    }

    fn master_inspector(&mut self, ui: &mut egui::Ui, p: &Project) {
        egui::CollapsingHeader::new("Master")
            .default_open(false)
            .show(ui, |ui| {
                let mut vol = p.master.volume_db;
                if ui
                    .add(egui::Slider::new(&mut vol, -60.0..=6.0).text("Master dB"))
                    .changed()
                {
                    self.cmd_merge(
                        Command::SetVolume {
                            track_id: MASTER_ID,
                            db: vol,
                        },
                        "volmaster",
                    );
                }
                self.insert_list(ui, MASTER_ID, &p.master.inserts.clone());
                if ui.button("🎛 Mastering Assistant").clicked() {
                    if let Some(v) = self.exec(Command::AutoMaster {
                        target_lufs: Some(-14.0),
                        character: None,
                    }) {
                        self.status = format!(
                            "Mastered to {:.1} LUFS",
                            v["measured_lufs"].as_f64().unwrap_or(0.0)
                        );
                    }
                }
            });
    }

    fn insert_list(&mut self, ui: &mut egui::Ui, owner: Id, inserts: &[PluginInstance]) {
        for (i, fx) in inserts.iter().enumerate() {
            let name = plugins::plugin_label(fx);
            let not_loaded = self.view.unloaded_vst3.get(&fx.id).cloned();
            ui.horizontal(|ui| {
                let mut on = !fx.bypass;
                if ui.checkbox(&mut on, "").on_hover_text("Bypass").changed() {
                    self.cmd(Command::SetPluginBypass {
                        plugin_id: fx.id,
                        bypass: !on,
                    });
                }
                // a VST3's preset is its name, already in `name`
                let label = match &fx.preset {
                    Some(pr) if fx.kind != "vst3" => format!("{name} · {pr}"),
                    _ => name.clone(),
                };
                let mut txt = RichText::new(label);
                txt = if not_loaded.is_some() {
                    txt.color(theme::TEXT_DIM).strikethrough()
                } else if fx.bypass {
                    txt.color(theme::TEXT_DIM)
                } else {
                    txt.color(Color32::WHITE)
                };
                let mut resp = ui.selectable_label(self.selected_plugin == Some(fx.id), txt);
                if let Some(reason) = &not_loaded {
                    resp = resp.on_hover_text(format!("Not loaded: {reason}"));
                }
                if resp.clicked() {
                    self.selected_plugin = Some(fx.id);
                }
                ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                    if ui.small_button("×").clicked() {
                        self.cmd(Command::RemovePlugin { plugin_id: fx.id });
                    }
                    if i + 1 < inserts.len() && ui.small_button("↓").clicked() {
                        self.cmd(Command::MovePlugin {
                            plugin_id: fx.id,
                            index: i + 1,
                        });
                    }
                    if i > 0 && ui.small_button("↑").clicked() {
                        self.cmd(Command::MovePlugin {
                            plugin_id: fx.id,
                            index: i - 1,
                        });
                    }
                });
            });
        }
        ui.menu_button("＋ Add effect", |ui| {
            effect_menu(ui, |kind| {
                self.pending_effect(owner, kind);
            });
        });
    }

    fn pending_effect(&mut self, owner: Id, kind: &str) {
        if let Some(v) = self.exec(Command::AddEffect {
            track_id: owner,
            kind: kind.to_string(),
            preset: None,
            index: None,
        }) {
            self.selected_plugin = v["plugin_id"].as_u64();
        }
    }

    fn plugin_editor(&mut self, ui: &mut egui::Ui, p: &Project, pid: Id) {
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
        egui::Frame::none().fill(theme::LANE).rounding(4.0).inner_margin(6.0).show(ui, |ui| {
            ui.horizontal(|ui| {
                ui.label(RichText::new(info.name).strong().color(theme::ACCENT));
                ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                    if ui.small_button("✕").clicked() {
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
                if ui.button(RichText::new("✨ AI: design a sound").color(theme::AI)).clicked() {
                    self.ask_ai(ondina_studio::prompts::design_sound(pid, info.name));
                }
            });
        });
    }

    /// Generic parameter editor for third-party VST3 plug-ins (normalized 0..1 values).
    fn vst3_editor(&mut self, ui: &mut egui::Ui, inst: &PluginInstance) {
        let pid = inst.id;
        egui::Frame::none()
            .fill(theme::LANE)
            .rounding(4.0_f32)
            .inner_margin(6.0)
            .show(ui, |ui| {
                ui.horizontal(|ui| {
                    ui.label(
                        RichText::new(format!("VST3 · {}", plugins::plugin_label(inst)))
                        .strong()
                        .color(theme::ACCENT),
                    );
                    ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                        if ui.small_button("✕").clicked() {
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
                    ui.label("🔍");
                    ui.text_edit_singleline(&mut self.vst_filter);
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
