//! The status bar at the bottom of the window: MCP endpoint, audio device, last command from an agent, the latest
//! message and the save state of the document.

use std::path::PathBuf;
use std::time::{Duration, Instant};

use eframe::egui::{self, vec2, Color32, RichText, Sense, Ui};
use ondina_studio::Source;

use crate::app::OndinaApp;
use crate::theme;

/// Remembers when the document was last saved, from what the session reports frame by frame: a clean document that
/// was dirty a moment ago, or that got a new path, has just been saved; a new document starts with no save time.
#[derive(Debug, Default)]
pub struct SaveClock {
    document: Option<u64>,
    path: Option<PathBuf>,
    dirty: bool,
    saved_at: Option<Instant>,
}

impl SaveClock {
    pub fn observe(&mut self, document: u64, path: Option<&PathBuf>, dirty: bool, now: Instant) {
        if self.document != Some(document) {
            self.saved_at = None;
        } else if !dirty && path.is_some() && (self.dirty || self.path.as_ref() != path) {
            self.saved_at = Some(now);
        }
        self.document = Some(document);
        self.path = path.cloned();
        self.dirty = dirty;
    }

    /// Time since the last save seen in this run.
    pub fn since(&self, now: Instant) -> Option<Duration> {
        self.saved_at.map(|t| now.saturating_duration_since(t))
    }
}

/// The save state in words.
pub fn save_state_text(dirty: bool, has_path: bool, since: Option<Duration>) -> String {
    if dirty {
        return if has_path { "Unsaved changes".into() } else { "Not saved yet".into() };
    }
    if !has_path {
        return "New project".into();
    }
    match since.map(|d| d.as_secs()) {
        None => "Saved".into(),
        Some(s) if s < 60 => "Saved just now".into(),
        Some(s) if s < 3600 => format!("Saved {} min ago", s / 60),
        Some(s) => format!("Saved {} h ago", s / 3600),
    }
}

/// "48 kHz", "44.1 kHz".
pub fn rate_text(hz: u32) -> String {
    if hz.is_multiple_of(1000) {
        format!("{} kHz", hz / 1000)
    } else {
        format!("{:.1} kHz", hz as f64 / 1000.0)
    }
}

const MCP_GREEN: Color32 = theme::PLAY;

fn dot(ui: &mut Ui, color: Color32) {
    let (rect, _) = ui.allocate_exact_size(vec2(6.0, 6.0), Sense::hover());
    ui.painter().circle_filled(rect.center(), 3.0, color);
}

fn small(s: impl Into<String>) -> RichText {
    RichText::new(s).size(theme::SIZE_NUMBER).color(theme::TEXT_3)
}

impl OndinaApp {
    pub(crate) fn status_bar(&mut self, ui: &mut Ui) {
        let now = Instant::now();
        self.save_clock.observe(self.view.document, self.view.path.as_ref(), self.view.dirty, now);
        ui.horizontal_centered(|ui| {
            ui.spacing_mut().item_spacing.x = theme::SP_3;
            let (color, text) = match (&self.mcp, &self.mcp_error) {
                (Some(h), _) => (MCP_GREEN, format!("MCP {}", h.address)),
                (None, Some(e)) => (theme::REC, format!("MCP off: {e}")),
                _ => (theme::TEXT_DISABLED, "MCP disabled".to_string()),
            };
            let r = ui
                .scope(|ui| {
                    dot(ui, color);
                    ui.add(egui::Label::new(small(text)).sense(Sense::click()))
                })
                .inner;
            if r.on_hover_text("Click for MCP setup instructions").clicked() {
                self.show_mcp_info = true;
            }
            ui.add_space(theme::SP_5);
            ui.label(small(format!("{} \u{b7} {}", self.view.device, rate_text(self.view.sample_rate))));
            if let Some(m) = &self.view.midi_input {
                ui.add_space(theme::SP_5);
                ui.label(small(format!("MIDI in: {m}")));
            }
            if let Some(last) = self.view.log.iter().find(|l| l.source != Source::Gui) {
                let label = match last.source {
                    Source::Ai => "Last command (Assistant):",
                    Source::Mcp => "Last command (MCP):",
                    _ => "Last command:",
                };
                ui.add_space(theme::SP_5);
                ui.label(small(label));
                let col = if last.ok { theme::ICON } else { theme::REC };
                ui.label(RichText::new(&last.command).size(theme::SIZE_NUMBER).color(col))
                    .on_hover_text(if last.ok { "Done" } else { "Failed" });
            }
            if !self.status.is_empty() {
                ui.add_space(theme::SP_5);
                ui.add(egui::Label::new(RichText::new(&self.status).size(theme::SIZE_NUMBER).color(theme::TEXT_2)).truncate());
            }
            ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                let state = save_state_text(self.view.dirty, self.view.path.is_some(), self.save_clock.since(now));
                let path = self
                    .view
                    .path
                    .as_ref()
                    .map(|p| p.display().to_string())
                    .unwrap_or_else(|| "unsaved project".into());
                let color = if self.view.dirty { theme::TEXT_2 } else { theme::TEXT_3 };
                ui.label(RichText::new(state).size(theme::SIZE_NUMBER).color(color)).on_hover_text(path);
            });
        });
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn save_clock_starts_on_a_save_and_resets_with_a_new_document() {
        let t0 = Instant::now();
        let path = PathBuf::from("song.ondina");
        let mut c = SaveClock::default();
        c.observe(1, Some(&path), false, t0);
        assert_eq!(c.since(t0), None, "opening is not saving");
        c.observe(1, Some(&path), true, t0);
        assert_eq!(c.since(t0), None);
        let t1 = t0 + Duration::from_secs(5);
        c.observe(1, Some(&path), false, t1);
        assert_eq!(c.since(t1 + Duration::from_secs(120)), Some(Duration::from_secs(120)));
        // save as of a clean document: new path
        let other = PathBuf::from("copy.ondina");
        let t2 = t1 + Duration::from_secs(10);
        c.observe(1, Some(&other), false, t2);
        assert_eq!(c.since(t2), Some(Duration::ZERO));
        // a new or opened document
        c.observe(2, Some(&other), false, t2);
        assert_eq!(c.since(t2), None);
    }

    #[test]
    fn save_state_reads_as_words() {
        assert_eq!(save_state_text(true, true, None), "Unsaved changes");
        assert_eq!(save_state_text(true, false, None), "Not saved yet");
        assert_eq!(save_state_text(false, false, None), "New project");
        assert_eq!(save_state_text(false, true, None), "Saved");
        assert_eq!(save_state_text(false, true, Some(Duration::from_secs(10))), "Saved just now");
        assert_eq!(save_state_text(false, true, Some(Duration::from_secs(150))), "Saved 2 min ago");
        assert_eq!(save_state_text(false, true, Some(Duration::from_secs(7300))), "Saved 2 h ago");
    }

    #[test]
    fn sample_rates_read_in_khz() {
        assert_eq!(rate_text(48_000), "48 kHz");
        assert_eq!(rate_text(44_100), "44.1 kHz");
    }
}
