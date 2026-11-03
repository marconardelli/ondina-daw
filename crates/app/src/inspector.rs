//! Left panel: Inspector (region, track card, channel strip and the sections below it) and Library (patches).

use eframe::egui::{self, RichText};
use ondina_core::command::Command;
use ondina_core::model::*;
use ondina_core::plugins;

use crate::app::{LeftTab, OndinaApp};
use crate::chrome;
use crate::inspector_strip::{sections, Section, OPEN_REGION_SECTION, OPEN_TRACK_SECTION};
use crate::region_edit::{self, ColorChoice};
use crate::theme;
use crate::widgets::{self, Icon};

/// Whether a Library patch matches the search: every word of `filter` in its name or category, any case.
pub fn patch_matches(name: &str, category: &str, filter: &str) -> bool {
    let hay = format!("{name} {category}").to_lowercase();
    filter.split_whitespace().all(|w| hay.contains(&w.to_lowercase()))
}

/// The Library's search field: a sunken box with a lens, and a clear button while it has text.
fn library_search(ui: &mut egui::Ui, filter: &mut String) {
    egui::Frame::none()
        .fill(theme::INSET)
        .stroke(egui::Stroke::new(1.0_f32, theme::BORDER))
        .rounding(theme::R_LG)
        .inner_margin(egui::Margin::symmetric(theme::SP_3, theme::SP_2))
        .show(ui, |ui| {
            ui.horizontal(|ui| {
                widgets::icon(ui, Icon::Search, 13.0, theme::TEXT_3);
                let clear_w = if filter.is_empty() { 0.0 } else { 22.0 };
                ui.add(
                    egui::TextEdit::singleline(filter)
                        .frame(false)
                        .hint_text("Search patches")
                        .desired_width(ui.available_width() - clear_w),
                );
                if !filter.is_empty()
                    && widgets::icon_only_button(ui, Icon::Close, 16.0, theme::TEXT_3)
                        .on_hover_text("Clear the search")
                        .clicked()
                {
                    filter.clear();
                }
            });
        });
}

impl OndinaApp {
    pub fn left_ui(&mut self, ui: &mut egui::Ui) {
        // the control bar's Library and Inspector buttons choose the page; the header says which one is open
        let (icon, title) = match self.left {
            LeftTab::Inspector => (Icon::Inspector, "Inspector"),
            LeftTab::Library => (Icon::Library, "Library"),
        };
        chrome::header_strip(ui, chrome::HEADER_H, |ui| {
            chrome::icon_title(ui, icon, theme::TEXT_2, RichText::new(title).color(theme::TEXT_2));
            ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                if chrome::header_button(ui, Icon::ChevronLeft, "Hide the side panel").clicked() {
                    self.show_left = false;
                }
            });
        });
        egui::ScrollArea::vertical()
            .auto_shrink([false, false])
            .show(ui, |ui| match self.left {
                LeftTab::Inspector => self.inspector(ui),
                LeftTab::Library => {
                    egui::Frame::none().inner_margin(theme::SP_5 - 2.0).show(ui, |ui| self.library(ui));
                }
            });
    }

    fn library(&mut self, ui: &mut egui::Ui) {
        let p = self.project();
        let target = self.selected_track.and_then(|t| p.track(t)).cloned();
        let hint = match &target {
            Some(t) => format!("Click a patch to load it on '{}', or + to put it on a new track.", t.name),
            None => "Click + to create a track with the patch.".into(),
        };
        ui.label(RichText::new(hint).size(theme::SIZE_NUMBER).color(theme::TEXT_3));
        ui.add_space(theme::SP_2);
        library_search(ui, &mut self.library_filter);
        ui.add_space(theme::SP_2);
        self.vst_library(ui);
        let filter = self.library_filter.clone();
        let matching: Vec<_> = plugins::PATCHES.iter().filter(|x| patch_matches(x.name, x.category, &filter)).collect();
        if matching.is_empty() {
            chrome::empty_state(
                ui,
                Icon::Search,
                &format!("No patches match \u{201c}{}\u{201d}", filter.trim()),
                "Try another word, such as bass, piano or drums.",
            );
            return;
        }
        let mut cats: Vec<&str> = matching.iter().map(|p| p.category).collect();
        cats.sort();
        cats.dedup();
        for c in cats {
            egui::CollapsingHeader::new(c)
                .id_salt(("library-cat", c))
                .default_open(true)
                .show(ui, |ui| {
                    for patch in matching.iter().filter(|x| x.category == c) {
                        ui.horizontal(|ui| {
                            let kind = if patch.instrument.is_some() {
                                TrackKind::Instrument
                            } else {
                                TrackKind::Audio
                            };
                            if widgets::icon_only_button(ui, Icon::Plus, 18.0, theme::TEXT_2)
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
        let region = self.selected_region.and_then(|id| p.find_region(id)).map(|(t, r)| (t.clone(), r.clone()));
        let track = self.selected_track.and_then(|id| p.track(id)).cloned();
        self.region_row(ui, region.as_ref().map(|(_, r)| r));
        let pad = theme::SP_5 - 2.0;
        if let Some(t) = &track {
            egui::Frame::none().inner_margin(pad).show(ui, |ui| self.track_card(ui, t));
            separator(ui);
            egui::Frame::none().inner_margin(pad).show(ui, |ui| self.track_strip(ui, &p, t));
        } else {
            egui::Frame::none().inner_margin(pad).show(ui, |ui| {
                ui.label(RichText::new("Select a track to inspect it.").color(theme::TEXT_DIM));
            });
        }
        separator(ui);
        let plugin = self.selected_plugin.filter(|pid| p.plugin_owner(*pid).is_some());
        egui::Frame::none().inner_margin(egui::Margin::symmetric(pad, theme::SP_3)).show(ui, |ui| {
            for section in sections(track.is_some(), plugin.is_some(), region.is_some()) {
                match section {
                    Section::Plugin => {
                        if let Some(pid) = plugin {
                            self.plugin_summary(ui, &p, pid);
                        }
                    }
                    Section::Region => {
                        if let Some((rt, r)) = &region {
                            self.region_section(ui, &p, rt, r);
                        }
                    }
                    Section::Track => {
                        if let Some(t) = &track {
                            self.track_section(ui, t);
                        }
                    }
                    Section::Surround => self.surround_inspector(ui, &p, track.as_ref()),
                    Section::Project => self.project_section(ui, &p),
                    Section::Master => self.master_inspector(ui, &p),
                }
            }
        });
    }

    /// "Region: name" at the top of the Inspector; a click opens the region's section below.
    fn region_row(&mut self, ui: &mut egui::Ui, region: Option<&Region>) {
        let resp = chrome::header_strip(ui, chrome::HEADER_H, |ui| {
            let icon = match region.map(|r| &r.content) {
                Some(RegionContent::Audio(_)) => Icon::Audio,
                Some(RegionContent::Pattern(_)) => Icon::Drums,
                _ => Icon::Piano,
            };
            chrome::icon_title(ui, icon, theme::TEXT_2, RichText::new("Region:").color(theme::TEXT_2));
            match region {
                Some(r) => ui.add(egui::Label::new(RichText::new(&r.name).color(theme::TEXT)).truncate()),
                None => ui.label(RichText::new("none selected").color(theme::TEXT_3)),
            };
        })
        .response;
        if region.is_some() {
            let resp = resp.interact(egui::Sense::click()).on_hover_text("Show the region's properties below");
            if resp.clicked() {
                ui.data_mut(|d| d.insert_temp(egui::Id::new(OPEN_REGION_SECTION), true));
            }
        }
    }

    /// A collapsing section that the region row or the track card's menu can open from outside.
    fn section_header(ui: &mut egui::Ui, title: impl Into<egui::WidgetText>, salt: &str, open_key: &str) -> egui::CollapsingHeader {
        let force = ui.data_mut(|d| d.remove_temp::<bool>(egui::Id::new(open_key))).unwrap_or(false);
        let h = egui::CollapsingHeader::new(title).id_salt(salt).default_open(false);
        if force {
            h.open(Some(true))
        } else {
            h
        }
    }

    fn region_section(&mut self, ui: &mut egui::Ui, p: &Project, rt: &Track, r: &Region) {
        // a fixed id: renaming the region must not close the section
        Self::section_header(ui, format!("Region: {}", r.name), "region-inspector", OPEN_REGION_SECTION).show(ui, |ui| {
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

    /// Name and color of the track, its id, Musical Typing and preview (the old Track section without volume and
    /// pan, which are in the strip).
    fn track_section(&mut self, ui: &mut egui::Ui, t: &Track) {
        let tid = t.id;
        Self::section_header(ui, "Track", "track-inspector", OPEN_TRACK_SECTION).show(ui, |ui| {
            if self.rename_buf.is_empty() || ui.memory(|m| !m.has_focus(ui.id().with("rename"))) {
                self.rename_buf = t.name.clone();
            }
            let resp = ui.add(egui::TextEdit::singleline(&mut self.rename_buf).id(ui.id().with("rename")));
            if resp.lost_focus() && self.rename_buf != t.name && !self.rename_buf.trim().is_empty() {
                self.cmd(Command::RenameTrack { track_id: tid, name: self.rename_buf.trim().to_string() });
            }
            ui.horizontal(|ui| {
                let mut c = [t.color[0], t.color[1], t.color[2]];
                if ui.color_edit_button_srgb(&mut c).changed() {
                    self.cmd_merge(
                        Command::SetTrackColor { track_id: tid, color: format!("#{:02X}{:02X}{:02X}", c[0], c[1], c[2]) },
                        format!("color{tid}"),
                    );
                }
                ui.label(RichText::new(format!("{:?} track · id {}", t.kind, t.id)).color(theme::TEXT_2));
            });
            if t.kind == TrackKind::Instrument {
                ui.horizontal(|ui| {
                    if ui
                        .selectable_label(self.musical_typing, "Musical Typing")
                        .on_hover_text("Play this instrument with the computer keyboard")
                        .clicked()
                    {
                        self.musical_typing = !self.musical_typing;
                    }
                    if ui.button("Preview C4").clicked() {
                        self.cmd(Command::PreviewNote { track_id: tid, pitch: 60, velocity: Some(100), duration: Some(0.6) });
                    }
                });
            }
        });
    }

    fn project_section(&mut self, ui: &mut egui::Ui, p: &Project) {
        egui::CollapsingHeader::new("Project").default_open(false).show(ui, |ui| {
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
            if ui.add(egui::TextEdit::multiline(&mut notes).desired_rows(3)).lost_focus() && notes != p.notes {
                self.cmd(Command::SetProjectNotes { notes });
            }
        });
    }

    fn master_inspector(&mut self, ui: &mut egui::Ui, p: &Project) {
        egui::CollapsingHeader::new("Master").default_open(false).show(ui, |ui| {
            let mut vol = p.master.volume_db;
            if ui.add(egui::Slider::new(&mut vol, -60.0..=6.0).text("Master dB")).changed() {
                self.cmd_merge(Command::SetVolume { track_id: MASTER_ID, db: vol }, "volmaster");
            }
            chrome::section_label(ui, "Effects");
            self.insert_slots(ui, MASTER_ID, &p.master.inserts.clone(), crate::chrome::ROW_H);
            if ui.button("Mastering Assistant").clicked() {
                if let Some(v) = self.exec(Command::AutoMaster { target_lufs: Some(-14.0), character: None }) {
                    self.status = format!("Mastered to {:.1} LUFS", v["measured_lufs"].as_f64().unwrap_or(0.0));
                }
            }
        });
    }

    pub(crate) fn pending_effect(&mut self, owner: Id, kind: &str) {
        if let Some(v) = self.exec(Command::AddEffect {
            track_id: owner,
            kind: kind.to_string(),
            preset: None,
            index: None,
        }) {
            self.selected_plugin = v["plugin_id"].as_u64();
        }
    }
}

/// A hairline across the panel between the card, the strip and the sections.
fn separator(ui: &mut egui::Ui) {
    let (rect, _) = ui.allocate_exact_size(egui::vec2(ui.available_width(), 1.0), egui::Sense::hover());
    ui.painter().rect_filled(rect, 0.0, theme::CONTROL);
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn library_search_matches_every_word_in_name_or_category() {
        assert!(patch_matches("Warm Bass", "Bass", ""));
        assert!(patch_matches("Warm Bass", "Bass", "  "));
        assert!(patch_matches("Warm Bass", "Bass", "warm"));
        assert!(patch_matches("Grand Piano", "Keys", "keys piano"));
        assert!(patch_matches("Grand Piano", "Keys", "PIA"));
        assert!(!patch_matches("Grand Piano", "Keys", "piano drums"));
        assert!(!patch_matches("Grand Piano", "Keys", "zzz"));
    }
}
