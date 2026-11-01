//! The Inspector's track card and channel strip: color bar, name, type and instrument with an options menu; then
//! instrument, MIDI effects, audio effects, sends, output and pan on the left and the volume fader with its dB scale,
//! stereo meter and Mute / Solo on the right.

use eframe::egui::{self, vec2, RichText, Rounding, Stroke};
use ondina_core::command::Command;
use ondina_core::model::*;
use ondina_core::plugins::{self, PluginCategory};

use crate::app::OndinaApp;
use crate::chrome::{self, SlotLook};
use crate::mixer::effect_menu;
use crate::theme;
use crate::widgets::{self, Fader, Knob, PeakHold, SlotState, StateKind};

/// Width of the fader column (scale, fader, meter).
pub const FADER_COL: f32 = 76.0;
/// Fader height when the strip has not been measured yet, and its limits.
const FADER_H: (f32, f32, f32) = (200.0, 150.0, 340.0);
/// The quietest volume the fader sends: its "-inf" end (the engine's floor).
pub const VOLUME_FLOOR_DB: f32 = -96.0;

/// Data key: open the Track section on the next frame (from the card's options menu).
pub const OPEN_TRACK_SECTION: &str = "inspector-open-track";
/// Data key: open the Region section on the next frame (from the region row).
pub const OPEN_REGION_SECTION: &str = "inspector-open-region";

/// The sections of the Inspector below the strip, in order.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Section {
    /// Parameters of the selected plug-in.
    Plugin,
    /// Name, color, loops and velocity of the selected region.
    Region,
    /// Name and color of the track, Musical Typing and preview.
    Track,
    Surround,
    Project,
    Master,
}

/// Which sections the Inspector shows: the plug-in editor when a plug-in is selected, the region and track ones when
/// they are selected, and always surround, project and master.
pub fn sections(track: bool, plugin: bool, region: bool) -> Vec<Section> {
    let mut v = Vec::with_capacity(6);
    if plugin {
        v.push(Section::Plugin);
    }
    if region {
        v.push(Section::Region);
    }
    if track {
        v.push(Section::Track);
    }
    v.extend([Section::Surround, Section::Project, Section::Master]);
    v
}

/// Parts of the strip a track kind has.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct StripParts {
    pub instrument: bool,
    pub midi_fx: bool,
    pub preview: bool,
}

pub fn strip_parts(kind: TrackKind) -> StripParts {
    let inst = kind == TrackKind::Instrument;
    StripParts { instrument: inst, midi_fx: inst, preview: inst }
}

pub fn kind_label(kind: TrackKind) -> &'static str {
    match kind {
        TrackKind::Instrument => "Instrument track",
        TrackKind::Audio => "Audio track",
        TrackKind::Bus => "Bus",
    }
}

/// "Instrument track · FM Synth" under the track name.
pub fn track_subtitle(kind: TrackKind, instrument: Option<&str>) -> String {
    match instrument {
        Some(i) if kind == TrackKind::Instrument => format!("{} · {i}", kind_label(kind)),
        _ => kind_label(kind).to_string(),
    }
}

/// Pan as the strip shows it: "C", "L 12", "R 64" (steps of 1/64).
pub fn pan_label(pan: f32) -> String {
    let steps = (pan.clamp(-1.0, 1.0) * 64.0).round() as i32;
    match steps {
        0 => "C".into(),
        s if s < 0 => format!("L {}", -s),
        s => format!("R {s}"),
    }
}

/// The volume to send for a fader position: its "-inf" end becomes the engine's floor, so the command stays a finite
/// number.
pub fn fader_command_db(db: f32) -> f32 {
    if db.is_finite() {
        db.clamp(VOLUME_FLOOR_DB, 6.0)
    } else {
        VOLUME_FLOOR_DB
    }
}

/// The fader's value box: "−6.0", "+2.5", "0.0", "−∞" at the floor.
pub fn db_text(db: f32) -> String {
    if !db.is_finite() || db <= VOLUME_FLOOR_DB + 0.05 {
        return "\u{2212}\u{221E}".into();
    }
    let s = format!("{:.1}", db.abs());
    if s == "0.0" {
        s
    } else if db < 0.0 {
        format!("\u{2212}{s}")
    } else {
        format!("+{s}")
    }
}

/// How an insert slot is drawn: struck through when bypassed or not loaded.
pub fn insert_slot(name: &str, bypass: bool, not_loaded: bool) -> SlotState<'_> {
    if bypass || not_loaded {
        SlotState::Bypassed(name)
    } else {
        SlotState::Normal(name)
    }
}

impl OndinaApp {
    /// Color bar, name, "type · instrument" and the options menu.
    pub(crate) fn track_card(&mut self, ui: &mut egui::Ui, t: &Track) {
        let inst = t.instrument.as_ref().map(plugins::plugin_label);
        ui.horizontal(|ui| {
            ui.spacing_mut().item_spacing.x = theme::SP_5 - 2.0;
            let (bar, _) = ui.allocate_exact_size(vec2(6.0, 34.0), egui::Sense::hover());
            ui.painter().rect_filled(bar, Rounding::same(3.0), theme::rgb(t.color));
            let text_w = (ui.available_width() - 24.0 - theme::SP_5).max(40.0);
            ui.allocate_ui_with_layout(vec2(text_w, 34.0), egui::Layout::top_down(egui::Align::Min), |ui| {
                ui.spacing_mut().item_spacing.y = 2.0;
                ui.add(
                    egui::Label::new(RichText::new(&t.name).font(theme::title_font()).color(theme::TEXT)).truncate(),
                );
                ui.add(
                    egui::Label::new(
                        RichText::new(track_subtitle(t.kind, inst.as_deref())).color(theme::TEXT_3),
                    )
                    .truncate(),
                );
            });
            widgets::more_menu_button(ui, vec2(24.0, 24.0), Some(theme::CONTROL), |ui| self.track_options(ui, t))
                .response
                .on_hover_text("Track options");
        });
    }

    fn track_options(&mut self, ui: &mut egui::Ui, t: &Track) {
        if strip_parts(t.kind).preview {
            if ui
                .selectable_label(self.musical_typing, "Musical Typing")
                .on_hover_text("Play this instrument with the computer keyboard")
                .clicked()
            {
                self.musical_typing = !self.musical_typing;
                ui.close_menu();
            }
            if ui.button("Preview C4").clicked() {
                self.cmd(Command::PreviewNote { track_id: t.id, pitch: 60, velocity: Some(100), duration: Some(0.6) });
                ui.close_menu();
            }
            ui.separator();
        }
        if ui.button("Rename and color…").clicked() {
            ui.data_mut(|d| d.insert_temp(egui::Id::new(OPEN_TRACK_SECTION), true));
            ui.close_menu();
        }
    }

    /// The channel strip of track `t`.
    pub(crate) fn track_strip(&mut self, ui: &mut egui::Ui, p: &Project, t: &Track) {
        let strip_id = egui::Id::new(("inspector-strip-h", t.id));
        let measured: Option<f32> = ui.data(|d| d.get_temp(strip_id));
        ui.horizontal_top(|ui| {
            ui.spacing_mut().item_spacing.x = theme::SP_5 - 2.0;
            let left_w = (ui.available_width() - FADER_COL - theme::SP_5).max(96.0);
            let left = ui
                .allocate_ui_with_layout(vec2(left_w, 0.0), egui::Layout::top_down(egui::Align::Min), |ui| {
                    ui.set_width(left_w);
                    ui.spacing_mut().item_spacing.y = theme::SP_3;
                    self.strip_slots(ui, p, t);
                })
                .response;
            ui.data_mut(|d| d.insert_temp(strip_id, left.rect.height()));
            let fader_h = measured.map(|h| h - 78.0).unwrap_or(FADER_H.0).clamp(FADER_H.1, FADER_H.2);
            ui.allocate_ui_with_layout(vec2(FADER_COL, 0.0), egui::Layout::top_down(egui::Align::Center), |ui| {
                ui.set_width(FADER_COL);
                ui.spacing_mut().item_spacing.y = theme::SP_3;
                self.strip_fader(ui, t.id, &t.channel, fader_h);
            });
        });
    }

    fn strip_slots(&mut self, ui: &mut egui::Ui, p: &Project, t: &Track) {
        let tid = t.id;
        let parts = strip_parts(t.kind);
        if parts.instrument {
            chrome::section_label(ui, "Instrument");
            self.instrument_slot(ui, p, t, 26.0);
        }
        if parts.midi_fx {
            chrome::section_label(ui, "MIDI FX");
            for fx in &t.midi_fx {
                let name = plugins::plugin_label(fx);
                let resp = widgets::effect_slot(ui, insert_slot(&name, fx.bypass, false), ui.available_width());
                self.mark_selected(ui, &resp, fx.id);
                let resp = resp.on_hover_text("Click: edit · Right-click: bypass or remove");
                if resp.clicked() {
                    self.selected_plugin = Some(fx.id);
                }
                let (pid, byp) = (fx.id, fx.bypass);
                resp.context_menu(|ui| {
                    if ui.button(if byp { "Enable" } else { "Bypass" }).clicked() {
                        self.cmd(Command::SetPluginBypass { plugin_id: pid, bypass: !byp });
                        ui.close_menu();
                    }
                    if ui.button("Remove").clicked() {
                        self.cmd(Command::RemovePlugin { plugin_id: pid });
                        ui.close_menu();
                    }
                });
            }
            chrome::add_slot_menu(ui, "+ Add", |ui| {
                for info in plugins::registry().iter().filter(|p| p.category == PluginCategory::MidiEffect) {
                    if ui.button(info.name).on_hover_text(info.description).clicked() {
                        self.cmd(Command::AddMidiEffect { track_id: tid, kind: info.kind.to_string(), preset: None });
                        ui.close_menu();
                    }
                }
            });
        }
        chrome::section_label(ui, "Effects");
        self.insert_slots(ui, tid, &t.channel.inserts, chrome::ROW_H);
        chrome::section_label(ui, "Sends");
        for (si, s) in t.channel.sends.iter().enumerate() {
            self.send_row(ui, p, tid, si, s);
        }
        let buses = Self::buses_for(p, tid);
        chrome::add_slot_menu(ui, "+ Send", |ui| self.add_send_items(ui, tid, &buses));
        chrome::section_label(ui, "Output");
        let out = match t.channel.output {
            Output::Master => "Master".to_string(),
            Output::Bus(b) => p.track(b).map(|x| x.name.clone()).unwrap_or("?".into()),
        };
        chrome::slot_menu(ui, SlotLook::VALUE, chrome::ROW_H, RichText::new(out), |ui| {
            self.output_items(ui, tid, &buses)
        });
        ui.add_space(theme::SP_3);
        ui.vertical_centered(|ui| self.pan_knob(ui, tid, t.channel.pan, 40.0));
    }

    /// The pan knob of channel `id` with its "C" / "L n" / "R n" label; double-click centers it.
    pub(crate) fn pan_knob(&mut self, ui: &mut egui::Ui, id: Id, pan: f32, diameter: f32) {
        let mut pan = pan;
        let text = pan_label(pan);
        let resp = ui
            .add(Knob::new(&mut pan, -1.0..=1.0).bipolar(true).default_value(0.0).diameter(diameter).text(text))
            .on_hover_text("Pan · drag up or down (Shift: fine) · double-click: center");
        if resp.double_clicked() {
            self.cmd(Command::SetPan { track_id: id, pan: 0.0 });
        } else if resp.changed() {
            self.cmd_merge(Command::SetPan { track_id: id, pan }, format!("pan{id}"));
        }
    }

    /// The instrument slot: a menu with the parameters, the plug-in window, the instrument list, presets and the
    /// sampler's sample.
    pub(crate) fn instrument_slot(&mut self, ui: &mut egui::Ui, p: &Project, t: &Track, height: f32) {
        let tid = t.id;
        let not_loaded = t.instrument.as_ref().and_then(|i| self.view.unloaded_vst3.get(&i.id).cloned());
        let label = match &t.instrument {
            Some(i) if i.kind == "vst3" => format!("VST3 · {}", plugins::plugin_label(i)),
            Some(i) => plugins::plugin_label(i),
            None => "None".to_string(),
        };
        let mut text = RichText::new(label).size(theme::SIZE_BODY);
        if not_loaded.is_some() {
            text = text.strikethrough();
        }
        let ir = chrome::slot_menu(ui, SlotLook::INSTRUMENT, height, text, |ui| {
            if let Some(inst) = &t.instrument {
                let iid = inst.id;
                if ui.selectable_label(self.selected_plugin == Some(iid), "Edit parameters").clicked() {
                    self.selected_plugin = Some(iid);
                    ui.close_menu();
                }
                if inst.kind == "vst3" {
                    let open = ondina_dsp::vst3::open_editors().contains(&iid);
                    let enabled = open || not_loaded.is_none();
                    let text = if open { "Close plug-in window" } else { "Open plug-in window" };
                    let resp = ui.add_enabled(enabled, egui::Button::new(text));
                    if let Some(reason) = &not_loaded {
                        resp.clone().on_disabled_hover_text(reason);
                    }
                    if resp.clicked() {
                        if open {
                            self.cmd(Command::ClosePluginEditor { plugin_id: iid });
                        } else {
                            self.exec(Command::OpenPluginEditor { plugin_id: iid });
                        }
                        ui.close_menu();
                    }
                }
                let presets: Vec<_> = plugins::presets_for(&inst.kind).collect();
                if !presets.is_empty() {
                    ui.menu_button("Preset", |ui| {
                        for pr in presets {
                            if ui.selectable_label(inst.preset.as_deref() == Some(pr.name), pr.name).clicked() {
                                self.cmd(Command::LoadPreset { plugin_id: iid, preset: pr.name.to_string() });
                                ui.close_menu();
                            }
                        }
                    });
                }
                if inst.kind == "sampler" {
                    if ui.button("Load sample…").clicked() {
                        ui.close_menu();
                        if let Some(f) = rfd::FileDialog::new()
                            .add_filter("Audio", &["wav", "aif", "aiff", "flac", "mp3", "ogg"])
                            .pick_file()
                        {
                            self.exec(Command::SetSamplerSample { plugin_id: iid, path: f.display().to_string() });
                        }
                    }
                    if let Some(s) = inst.sample.and_then(|id| p.audio_file(id)) {
                        ui.label(RichText::new(&s.name).small().color(theme::TEXT_2));
                    }
                }
                ui.separator();
            }
            let cur = t.instrument.as_ref().map(|i| i.kind.clone()).unwrap_or_default();
            ui.menu_button("Instrument", |ui| {
                for info in plugins::registry().iter().filter(|p| p.category == PluginCategory::Instrument) {
                    if ui.selectable_label(cur == info.kind, info.name).on_hover_text(info.description).clicked() {
                        self.cmd(Command::SetInstrument { track_id: tid, kind: info.kind.to_string(), preset: None });
                        ui.close_menu();
                    }
                }
            });
        });
        let tip = match (&not_loaded, &t.instrument) {
            (Some(reason), _) => format!("Not loaded: {reason}"),
            (None, Some(i)) => match &i.preset {
                Some(pr) if i.kind != "vst3" => format!("Preset: {pr} · click for parameters, presets and instruments"),
                _ => "Click for parameters, presets and instruments".into(),
            },
            (None, None) => "Click to choose an instrument".into(),
        };
        ir.response.on_hover_text(tip);
    }

    /// The audio effect slots of `owner` (a track or the master) and the "+ Add" slot, `add_h` tall.
    pub(crate) fn insert_slots(&mut self, ui: &mut egui::Ui, owner: Id, inserts: &[PluginInstance], add_h: f32) {
        for (i, fx) in inserts.iter().enumerate() {
            let name = plugins::plugin_label(fx);
            let label = match &fx.preset {
                Some(pr) if fx.kind != "vst3" => format!("{name} · {pr}"),
                _ => name.clone(),
            };
            let not_loaded = self.view.unloaded_vst3.get(&fx.id).cloned();
            let resp = widgets::effect_slot(ui, insert_slot(&label, fx.bypass, not_loaded.is_some()), ui.available_width());
            self.mark_selected(ui, &resp, fx.id);
            let mut tip = String::from("Click: edit · Right-click: bypass, move, remove");
            if fx.kind == "vst3" {
                tip.push_str(" · Double-click: plug-in window");
            }
            if fx.bypass {
                tip = format!("Bypassed · {tip}");
            }
            if let Some(reason) = &not_loaded {
                tip = format!("Not loaded: {reason}\n{tip}");
            }
            let resp = resp.on_hover_text(tip);
            if resp.clicked() {
                self.selected_plugin = Some(fx.id);
            }
            if resp.double_clicked() && fx.kind == "vst3" {
                match &not_loaded {
                    Some(reason) => self.status = format!("⚠ Plug-in not loaded: {reason}"),
                    None => {
                        self.exec(Command::OpenPluginEditor { plugin_id: fx.id });
                    }
                }
            }
            let moves = (i > 0, i + 1 < inserts.len());
            let (pid, byp, vst3) = (fx.id, fx.bypass, fx.kind == "vst3");
            resp.context_menu(|ui| {
                if vst3 && ui.add_enabled(not_loaded.is_none(), egui::Button::new("Open plug-in window")).clicked() {
                    self.exec(Command::OpenPluginEditor { plugin_id: pid });
                    ui.close_menu();
                }
                if ui.button(if byp { "Enable" } else { "Bypass" }).clicked() {
                    self.cmd(Command::SetPluginBypass { plugin_id: pid, bypass: !byp });
                    ui.close_menu();
                }
                if moves.0 && ui.button("Move up").clicked() {
                    self.cmd(Command::MovePlugin { plugin_id: pid, index: i - 1 });
                    ui.close_menu();
                }
                if moves.1 && ui.button("Move down").clicked() {
                    self.cmd(Command::MovePlugin { plugin_id: pid, index: i + 1 });
                    ui.close_menu();
                }
                if ui.button("Remove").clicked() {
                    self.cmd(Command::RemovePlugin { plugin_id: pid });
                    ui.close_menu();
                }
            });
        }
        chrome::add_slot_menu_sized(ui, "+ Add", add_h, |ui| {
            effect_menu(ui, |kind| self.pending_effect(owner, kind));
        });
    }

    /// An accent outline on the slot of the plug-in whose parameters are open.
    fn mark_selected(&self, ui: &egui::Ui, resp: &egui::Response, pid: Id) {
        if self.selected_plugin == Some(pid) {
            ui.painter().rect_stroke(resp.rect, Rounding::same(theme::R_SM), Stroke::new(1.0_f32, theme::ACCENT));
        }
    }

    /// A send: the bus name and its level (drag or type), right-click to remove.
    pub(crate) fn send_row(&mut self, ui: &mut egui::Ui, p: &Project, tid: Id, si: usize, s: &Send) {
        let bus = p.track(s.bus_id).map(|b| b.name.clone()).unwrap_or("?".into());
        let w = ui.available_width();
        let resp = egui::Frame::none()
            .fill(SlotLook::VALUE.fill)
            .stroke(Stroke::new(1.0_f32, SlotLook::VALUE.border))
            .rounding(Rounding::same(theme::R_MD))
            .inner_margin(egui::Margin::symmetric(theme::SP_3, 0.0))
            .show(ui, |ui| {
                ui.set_width(w - 2.0 * theme::SP_3);
                ui.set_height(chrome::ROW_H - 2.0);
                ui.horizontal_centered(|ui| {
                    ui.add(egui::Label::new(RichText::new(&bus).size(theme::SIZE_BODY)).truncate());
                    ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                        let mut lv = s.level_db;
                        let drag = egui::DragValue::new(&mut lv)
                            .range(-60.0..=6.0)
                            .speed(0.2)
                            .max_decimals(1)
                            .custom_formatter(|v, _| db_text(v as f32));
                        if ui.add(drag).on_hover_text("Send level in dB · drag or double-click to type").changed() {
                            self.cmd_merge(
                                Command::SetSendLevel { track_id: tid, send_index: si, level_db: lv },
                                format!("send{tid}-{si}"),
                            );
                        }
                    });
                });
            })
            .response;
        resp.on_hover_text("Right-click to remove the send").context_menu(|ui| {
            if ui.button("Remove send").clicked() {
                self.cmd(Command::RemoveSend { track_id: tid, send_index: si });
                ui.close_menu();
            }
        });
    }

    /// VOLUME label, fader with its dB scale and the stereo meter, the dB value, Mute and Solo.
    fn strip_fader(&mut self, ui: &mut egui::Ui, id: Id, ch: &Channel, height: f32) {
        chrome::section_label(ui, "Volume");
        ui.horizontal_top(|ui| {
            self.fader_and_meter(ui, id, ch, height, 12.0, "inspector-holds");
        });
        chrome::lcd_value(ui, &db_text(ch.volume_db));
        ui.horizontal(|ui| {
            ui.spacing_mut().item_spacing.x = 4.0;
            let mut mute = ch.mute;
            if widgets::state_toggle(ui, &mut mute, StateKind::Mute).on_hover_text("Mute").changed() {
                self.cmd(Command::SetMute { track_id: id, mute });
            }
            if id != MASTER_ID {
                let mut solo = ch.solo;
                if widgets::state_toggle(ui, &mut solo, StateKind::Solo).on_hover_text("Solo").changed() {
                    self.cmd(Command::SetSolo { track_id: id, solo });
                }
            }
        });
    }
}

impl OndinaApp {
    /// The volume fader with its dB scale and the stereo meter beside it, laid out left to right in `ui`. The meter's
    /// peak holds live in the egui memory under `(holds_key, id)`; they are returned for the peak readout.
    pub(crate) fn fader_and_meter(
        &mut self,
        ui: &mut egui::Ui,
        id: Id,
        ch: &Channel,
        height: f32,
        meter_w: f32,
        holds_key: &'static str,
    ) -> [PeakHold; 2] {
        ui.spacing_mut().item_spacing.x = 4.0;
        let mut db = ch.volume_db;
        let resp = ui
            .add(Fader::new(&mut db).height(height).scale(true))
            .on_hover_text("Volume · drag the cap (Shift: fine) · double-click: 0 dB");
        if resp.double_clicked() {
            self.cmd(Command::SetVolume { track_id: id, db: 0.0 });
        } else if resp.changed() {
            self.cmd_merge(Command::SetVolume { track_id: id, db: fader_command_db(db) }, format!("vol{id}"));
        }
        let m = self.meters.get(&id).copied().unwrap_or([0.0; 2]);
        let levels = [widgets::gain_to_db(m[0]), widgets::gain_to_db(m[1])];
        let hold_id = egui::Id::new((holds_key, id));
        let mut holds: [PeakHold; 2] = ui.data(|d| d.get_temp(hold_id)).unwrap_or_default();
        widgets::stereo_meter(ui, vec2(meter_w, height), levels, &mut holds)
            .on_hover_text("Click to clear the clip lights");
        ui.data_mut(|d| d.insert_temp(hold_id, holds));
        holds
    }
}

/// Clears the clip lights of the meter whose holds live under `(holds_key, id)`.
pub fn clear_clip(ctx: &egui::Context, holds_key: &'static str, id: Id) {
    let hold_id = egui::Id::new((holds_key, id));
    ctx.data_mut(|d| {
        d.get_temp_mut_or_default::<[PeakHold; 2]>(hold_id).iter_mut().for_each(PeakHold::reset_clip);
    });
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn inspector_sections_follow_the_selection() {
        use Section::*;
        assert_eq!(sections(false, false, false), [Surround, Project, Master]);
        assert_eq!(sections(true, false, false), [Track, Surround, Project, Master]);
        assert_eq!(sections(true, true, true), [Plugin, Region, Track, Surround, Project, Master]);
        // a master insert can be edited with no track selected
        assert_eq!(sections(false, true, false), [Plugin, Surround, Project, Master]);
        // the always-there sections keep every old capability reachable
        for s in [Surround, Project, Master] {
            assert!(sections(false, false, true).contains(&s));
        }
    }

    #[test]
    fn only_instrument_tracks_have_an_instrument_and_midi_effects() {
        let i = strip_parts(TrackKind::Instrument);
        assert!(i.instrument && i.midi_fx && i.preview);
        for k in [TrackKind::Audio, TrackKind::Bus] {
            let p = strip_parts(k);
            assert!(!p.instrument && !p.midi_fx && !p.preview, "{k:?}");
        }
    }

    #[test]
    fn subtitle_names_the_kind_and_the_instrument() {
        assert_eq!(track_subtitle(TrackKind::Instrument, Some("FM Synth")), "Instrument track · FM Synth");
        assert_eq!(track_subtitle(TrackKind::Instrument, None), "Instrument track");
        assert_eq!(track_subtitle(TrackKind::Audio, Some("ignored")), "Audio track");
        assert_eq!(track_subtitle(TrackKind::Bus, None), "Bus");
    }

    #[test]
    fn pan_reads_left_center_right() {
        assert_eq!(pan_label(0.0), "C");
        assert_eq!(pan_label(0.004), "C");
        assert_eq!(pan_label(-0.5), "L 32");
        assert_eq!(pan_label(0.1875), "R 12");
        assert_eq!(pan_label(1.0), "R 64");
        assert_eq!(pan_label(-3.0), "L 64");
    }

    #[test]
    fn fader_values_stay_finite_and_read_well() {
        assert_eq!(fader_command_db(f32::NEG_INFINITY), VOLUME_FLOOR_DB);
        assert_eq!(fader_command_db(f32::NAN), VOLUME_FLOOR_DB);
        assert_eq!(fader_command_db(12.0), 6.0);
        assert_eq!(fader_command_db(-6.0), -6.0);
        assert_eq!(db_text(-6.0), "\u{2212}6.0");
        assert_eq!(db_text(2.5), "+2.5");
        assert_eq!(db_text(0.0), "0.0");
        assert_eq!(db_text(-0.04), "0.0");
        assert_eq!(db_text(VOLUME_FLOOR_DB), "\u{2212}\u{221E}");
        assert_eq!(db_text(f32::NEG_INFINITY), "\u{2212}\u{221E}");
    }

    #[test]
    fn bypassed_or_missing_plugins_are_struck_through() {
        assert_eq!(insert_slot("EQ", false, false), SlotState::Normal("EQ"));
        assert_eq!(insert_slot("EQ", true, false), SlotState::Bypassed("EQ"));
        assert_eq!(insert_slot("EQ", false, true), SlotState::Bypassed("EQ"));
    }
}
