//! Header of the bottom Editor panel: the editors the selected region can open (Piano Roll, Step, Score, Audio)
//! as tabs, the region's color and "Track / Region", and MIDI Transform on the right.

use eframe::egui::{self, RichText};
use ondina_core::model::{Region, RegionContent, Track};

use crate::app::OndinaApp;
use crate::chrome;
use crate::theme;

/// The editors of the bottom panel.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum EditorTab {
    PianoRoll,
    Step,
    Score,
    Audio,
}

impl EditorTab {
    pub fn label(self) -> &'static str {
        match self {
            EditorTab::PianoRoll => "Piano Roll",
            EditorTab::Step => "Step",
            EditorTab::Score => "Score",
            EditorTab::Audio => "Audio",
        }
    }
}

/// What a region holds, as far as the editor tabs care.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ContentKind {
    Midi,
    Pattern,
    Audio,
}

impl ContentKind {
    pub fn of(content: &RegionContent) -> Self {
        match content {
            RegionContent::Midi(_) => ContentKind::Midi,
            RegionContent::Pattern(_) => ContentKind::Pattern,
            RegionContent::Audio(_) => ContentKind::Audio,
        }
    }
}

/// The tabs a region can open: notes in the piano roll or the score, a pattern in the step sequencer or the score,
/// audio in the audio editor.
pub fn editor_tabs(kind: ContentKind) -> &'static [EditorTab] {
    match kind {
        ContentKind::Midi => &[EditorTab::PianoRoll, EditorTab::Score],
        ContentKind::Pattern => &[EditorTab::Step, EditorTab::Score],
        ContentKind::Audio => &[EditorTab::Audio],
    }
}

/// The tab shown for a region with the score switch at `show_score` (audio has no score).
pub fn current_tab(kind: ContentKind, show_score: bool) -> EditorTab {
    match (kind, show_score) {
        (ContentKind::Audio, _) => EditorTab::Audio,
        (_, true) => EditorTab::Score,
        (ContentKind::Midi, false) => EditorTab::PianoRoll,
        (ContentKind::Pattern, false) => EditorTab::Step,
    }
}

/// The score switch after clicking `tab`. Audio leaves it as it was, so a MIDI region opened next comes back in the
/// view the person last used.
pub fn score_after(tab: EditorTab, show_score: bool) -> bool {
    match tab {
        EditorTab::Score => true,
        EditorTab::PianoRoll | EditorTab::Step => false,
        EditorTab::Audio => show_score,
    }
}

impl OndinaApp {
    pub(crate) fn editor_header(&mut self, ui: &mut egui::Ui, selection: Option<(&Track, &Region)>) {
        chrome::header_strip(ui, chrome::EDITOR_HEADER, |ui| {
            let Some((track, region)) = selection else {
                ui.label(RichText::new("No region selected").color(theme::TEXT_3));
                return;
            };
            let kind = ContentKind::of(&region.content);
            let shown = current_tab(kind, self.extra.show_score);
            ui.spacing_mut().item_spacing.x = 2.0;
            for tab in editor_tabs(kind) {
                if chrome::tab(ui, *tab == shown, tab.label()).clicked() {
                    self.extra.show_score = score_after(*tab, self.extra.show_score);
                }
            }
            ui.spacing_mut().item_spacing.x = theme::SP_4;
            ui.add_space(theme::SP_2);
            let (sep, _) = ui.allocate_exact_size(egui::vec2(1.0, 18.0), egui::Sense::hover());
            ui.painter().rect_filled(sep, 0.0, theme::BORDER);
            let color = theme::rgb(region.color.unwrap_or(track.color));
            chrome::color_dot(ui, color, 8.0);
            ui.spacing_mut().item_spacing.x = theme::SP_3;
            let name = ui.label(RichText::new(&track.name).font(egui::FontId::new(theme::SIZE_BODY, theme::semibold())));
            let region_label = ui.label(RichText::new(format!("/ {}", region.name)).color(theme::TEXT_3));
            if shown == EditorTab::PianoRoll {
                name.on_hover_text(crate::editors::PIANO_ROLL_HELP);
                region_label.on_hover_text(crate::editors::PIANO_ROLL_HELP);
            }
            if kind != ContentKind::Audio {
                ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                    if ui.button("MIDI Transform…").clicked() {
                        self.extra.show_transform = true;
                    }
                });
            }
        });
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn each_region_kind_offers_its_editors() {
        assert_eq!(editor_tabs(ContentKind::Midi), [EditorTab::PianoRoll, EditorTab::Score]);
        assert_eq!(editor_tabs(ContentKind::Pattern), [EditorTab::Step, EditorTab::Score]);
        assert_eq!(editor_tabs(ContentKind::Audio), [EditorTab::Audio]);
        assert_eq!(EditorTab::PianoRoll.label(), "Piano Roll");
    }

    #[test]
    fn the_shown_tab_follows_the_score_switch() {
        assert_eq!(current_tab(ContentKind::Midi, false), EditorTab::PianoRoll);
        assert_eq!(current_tab(ContentKind::Midi, true), EditorTab::Score);
        assert_eq!(current_tab(ContentKind::Pattern, false), EditorTab::Step);
        assert_eq!(current_tab(ContentKind::Pattern, true), EditorTab::Score);
        assert_eq!(current_tab(ContentKind::Audio, true), EditorTab::Audio);
        // the shown tab is always one of the region's tabs
        for kind in [ContentKind::Midi, ContentKind::Pattern, ContentKind::Audio] {
            for s in [false, true] {
                assert!(editor_tabs(kind).contains(&current_tab(kind, s)), "{kind:?} {s}");
            }
        }
    }

    #[test]
    fn clicking_a_tab_sets_the_score_switch() {
        assert!(score_after(EditorTab::Score, false));
        assert!(!score_after(EditorTab::PianoRoll, true));
        assert!(!score_after(EditorTab::Step, true));
        assert!(score_after(EditorTab::Audio, true));
        assert!(!score_after(EditorTab::Audio, false));
    }
}
