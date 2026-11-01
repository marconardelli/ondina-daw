//! The session players of the track menu: where their region goes and how long it is, the drummer's choices, the
//! generator command they build and the status line that reports the seed.

use ondina_core::command::Command;
use ondina_core::generate::{ArpMode, BassStyle, CompStyle, DrumStyle};
use ondina_core::model::{Cycle, Id};
use serde_json::Value;

use crate::app::OndinaApp;

/// The lengths offered by the Length row when the cycle does not set one.
pub const LENGTHS: [u32; 5] = [1, 2, 4, 8, 16];
/// Longest part, as the drummer already allows.
const MAX_BARS: u32 = 512;

/// How hard the session drummer plays.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum Intensity {
    Soft,
    #[default]
    Medium,
    Hard,
}

impl Intensity {
    pub const ALL: [Intensity; 3] = [Intensity::Soft, Intensity::Medium, Intensity::Hard];

    pub fn label(self) -> &'static str {
        match self {
            Intensity::Soft => "Soft",
            Intensity::Medium => "Medium",
            Intensity::Hard => "Hard",
        }
    }

    fn level(self) -> f32 {
        match self {
            Intensity::Soft => 0.3,
            Intensity::Medium => 0.6,
            Intensity::Hard => 0.9,
        }
    }
}

/// What the menu remembers while the app is open (not saved).
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct PlayerChoices {
    pub bars: u32,
    pub intensity: Intensity,
    pub fill: bool,
}

impl Default for PlayerChoices {
    fn default() -> Self {
        PlayerChoices { bars: 4, intensity: Intensity::Medium, fill: true }
    }
}

/// Where a generated part goes.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Span {
    pub start: f64,
    pub bars: u32,
    pub from_cycle: bool,
}

/// The cycle when it is on and the playhead is inside it (start of the cycle, bars rounded up); otherwise the bar the
/// playhead is in, for the chosen number of bars.
pub fn generator_span(cycle: Cycle, playhead: f64, bpb: f64, bars: u32) -> Span {
    let bpb = if bpb > 0.0 { bpb } else { 4.0 };
    let in_cycle = cycle.enabled
        && cycle.end - cycle.start > 1e-9
        && playhead >= cycle.start - 1e-9
        && playhead < cycle.end - 1e-9;
    if in_cycle {
        let whole = ((cycle.end - cycle.start) / bpb - 1e-9).ceil().clamp(1.0, MAX_BARS as f64);
        Span { start: cycle.start.max(0.0), bars: whole as u32, from_cycle: true }
    } else {
        let start = ((playhead.max(0.0) / bpb) + 1e-9).floor() * bpb;
        Span { start, bars: bars.clamp(1, MAX_BARS), from_cycle: false }
    }
}

/// A session player of the track menu.
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum Player {
    Drums(DrumStyle),
    Bass(BassStyle),
    Keys(CompStyle),
    Arpeggio,
    Melody,
}

impl Player {
    /// The name the generator gives the region ("Drums (Pop)"), used in the status line.
    pub fn label(self) -> String {
        match self {
            Player::Drums(s) => format!("Drums ({s:?})"),
            Player::Bass(s) => format!("Bass ({s:?})"),
            Player::Keys(s) => format!("Chords ({s:?})"),
            Player::Arpeggio => "Arpeggio".into(),
            Player::Melody => "Melody".into(),
        }
    }
}

/// The generator command of a menu choice.
pub fn command(player: Player, track_id: Id, span: Span, choices: &PlayerChoices) -> Command {
    let (start, bars) = (span.start, span.bars);
    match player {
        Player::Drums(style) => Command::GenerateDrums {
            track_id,
            start,
            bars,
            style,
            intensity: Some(choices.intensity.level()),
            fill: Some(choices.fill),
            seed: None,
        },
        Player::Bass(style) => Command::GenerateBass { track_id, start, bars, style, chords: None, beats_per_chord: None, octave: None, seed: None },
        Player::Keys(style) => Command::GenerateChords { track_id, start, bars, style, chords: None, beats_per_chord: None, center_pitch: None, seed: None },
        Player::Arpeggio => Command::GenerateArpeggio {
            track_id,
            start,
            bars,
            mode: ArpMode::UpDown,
            rate: Some(0.25),
            octaves: Some(2),
            chords: None,
            beats_per_chord: None,
            center_pitch: None,
            seed: None,
        },
        Player::Melody => Command::GenerateMelody { track_id, start, bars, density: Some(0.5), octave: Some(5), chords: None, beats_per_chord: None, seed: None },
    }
}

fn bars_text(bars: u64) -> String {
    if bars == 1 {
        "1 bar".into()
    } else {
        format!("{bars} bars")
    }
}

/// "4 bars", "1 bar", "cycle, 4 bars": the length the menu entries show.
pub fn span_text(span: Span) -> String {
    let len = bars_text(span.bars as u64);
    if span.from_cycle {
        format!("cycle, {len}")
    } else {
        len
    }
}

/// The status bar line after a generation: "Drums (Pop), 8 bars, seed 1234".
pub fn status(part: &str, result: &Value) -> String {
    let Some(bars) = result["bars"].as_u64() else { return part.to_string() };
    let mut s = format!("{part}, {}", bars_text(bars));
    if let Some(seed) = result["seed"].as_u64() {
        s.push_str(&format!(", seed {seed}"));
    }
    s
}

impl OndinaApp {
    /// Runs a session player now (not queued), so the status bar can show the seed it used.
    pub fn run_player(&mut self, player: Player, track_id: Id, span: Span) {
        let c = command(player, track_id, span, &self.extra.players);
        if let Some(v) = self.exec(c) {
            self.status = status(&player.label(), &v);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    fn cycle(enabled: bool, start: f64, end: f64) -> Cycle {
        Cycle { enabled, start, end }
    }

    fn span(start: f64, bars: u32, from_cycle: bool) -> Span {
        Span { start, bars, from_cycle }
    }

    #[test]
    fn a_cycle_around_the_playhead_sets_start_and_bars() {
        assert_eq!(generator_span(cycle(true, 9.0, 25.0), 12.0, 4.0, 8), span(9.0, 4, true));
        assert_eq!(generator_span(cycle(true, 9.0, 25.0), 9.0, 4.0, 8), span(9.0, 4, true));
    }

    #[test]
    fn outside_the_cycle_the_part_starts_at_the_playhead_bar() {
        assert_eq!(generator_span(cycle(false, 9.0, 25.0), 13.5, 4.0, 8), span(12.0, 8, false));
        assert_eq!(generator_span(cycle(true, 9.0, 25.0), 30.0, 4.0, 8), span(28.0, 8, false));
        assert_eq!(generator_span(cycle(true, 9.0, 25.0), 25.0, 4.0, 8), span(24.0, 8, false));
        assert_eq!(generator_span(cycle(false, 0.0, 0.0), 7.0, 3.0, 2).start, 6.0);
    }

    #[test]
    fn a_cycle_that_does_not_fill_whole_bars_rounds_up() {
        assert_eq!(generator_span(cycle(true, 0.0, 5.0), 1.0, 4.0, 8).bars, 2);
        assert_eq!(generator_span(cycle(true, 9.0, 25.5), 10.0, 4.0, 8).bars, 5);
        assert_eq!(generator_span(cycle(true, 0.0, 6.0), 1.0, 3.0, 8).bars, 2);
        assert_eq!(generator_span(cycle(true, 0.0, 12.0), 1.0, 3.0, 8).bars, 4);
    }

    #[test]
    fn bars_stay_between_1_and_512() {
        assert_eq!(generator_span(cycle(false, 0.0, 0.0), 0.0, 4.0, 0).bars, 1);
        assert_eq!(generator_span(cycle(false, 0.0, 0.0), 0.0, 4.0, 1000).bars, 512);
        assert_eq!(generator_span(cycle(true, 0.0, 10000.0), 5.0, 4.0, 4), span(0.0, 512, true));
        assert_eq!(generator_span(cycle(true, 8.0, 8.0), 13.0, 4.0, 2), span(12.0, 2, false));
        assert_eq!(generator_span(cycle(false, 0.0, 0.0), -3.0, 4.0, 4).start, 0.0);
        assert_eq!(generator_span(cycle(false, 0.0, 0.0), 5.0, 0.0, 4).start, 4.0);
    }

    #[test]
    fn the_drummer_sends_the_chosen_intensity_and_fill() {
        let at = span(9.0, 4, true);
        for (intensity, level, fill) in [(Intensity::Soft, 0.3f32, false), (Intensity::Medium, 0.6, true), (Intensity::Hard, 0.9, false)] {
            let choices = PlayerChoices { bars: 2, intensity, fill };
            match command(Player::Drums(DrumStyle::Funk), 3, at, &choices) {
                Command::GenerateDrums { track_id, start, bars, style, intensity: i, fill: f, seed } => {
                    assert_eq!((track_id, start, bars, style), (3, 9.0, 4, DrumStyle::Funk));
                    assert_eq!(i, Some(level), "{intensity:?}");
                    assert_eq!(f, Some(fill), "{intensity:?}");
                    assert_eq!(seed, None);
                }
                other => panic!("not a drum command: {other:?}"),
            }
        }
    }

    #[test]
    fn every_player_builds_its_generator_at_the_span() {
        let at = span(8.0, 2, false);
        let choices = PlayerChoices::default();
        match command(Player::Bass(BassStyle::Walking), 1, at, &choices) {
            Command::GenerateBass { track_id, start, bars, style, chords, beats_per_chord, octave, seed } => {
                assert_eq!((track_id, start, bars, style), (1, 8.0, 2, BassStyle::Walking));
                assert_eq!((chords, beats_per_chord, octave, seed), (None, None, None, None));
            }
            other => panic!("not a bass command: {other:?}"),
        }
        match command(Player::Keys(CompStyle::Funk), 1, at, &choices) {
            Command::GenerateChords { track_id, start, bars, style, chords, beats_per_chord, center_pitch, seed } => {
                assert_eq!((track_id, start, bars, style), (1, 8.0, 2, CompStyle::Funk));
                assert_eq!((chords, beats_per_chord, center_pitch, seed), (None, None, None, None));
            }
            other => panic!("not a chords command: {other:?}"),
        }
        match command(Player::Arpeggio, 1, at, &choices) {
            Command::GenerateArpeggio { track_id, start, bars, mode, rate, octaves, chords, beats_per_chord, center_pitch, seed } => {
                assert_eq!((track_id, start, bars, mode), (1, 8.0, 2, ArpMode::UpDown));
                assert_eq!((rate, octaves), (Some(0.25), Some(2)));
                assert_eq!((chords, beats_per_chord, center_pitch, seed), (None, None, None, None));
            }
            other => panic!("not an arpeggio command: {other:?}"),
        }
        match command(Player::Melody, 1, at, &choices) {
            Command::GenerateMelody { track_id, start, bars, density, octave, chords, beats_per_chord, seed } => {
                assert_eq!((track_id, start, bars), (1, 8.0, 2));
                assert_eq!((density, octave), (Some(0.5), Some(5)));
                assert_eq!((chords, beats_per_chord, seed), (None, None, None));
            }
            other => panic!("not a melody command: {other:?}"),
        }
    }

    #[test]
    fn labels_match_the_region_names() {
        assert_eq!(Player::Drums(DrumStyle::Pop).label(), "Drums (Pop)");
        assert_eq!(Player::Drums(DrumStyle::HipHop).label(), "Drums (HipHop)");
        assert_eq!(Player::Bass(BassStyle::Walking).label(), "Bass (Walking)");
        assert_eq!(Player::Keys(CompStyle::Funk).label(), "Chords (Funk)");
        assert_eq!(Player::Arpeggio.label(), "Arpeggio");
        assert_eq!(Player::Melody.label(), "Melody");
    }

    #[test]
    fn the_status_line_reports_part_bars_and_seed() {
        assert_eq!(status("Drums (Pop)", &json!({"region_id": 5, "seed": 1234, "start": 0.0, "bars": 8})), "Drums (Pop), 8 bars, seed 1234");
        assert_eq!(status("Melody", &json!({"region_id": 5, "seed": 9, "start": 0.0, "bars": 1})), "Melody, 1 bar, seed 9");
        assert_eq!(status("Drums (Pop)", &json!({"region_id": 5, "start": 0.0, "bars": 8})), "Drums (Pop), 8 bars");
        assert_eq!(status("Drums (Pop)", &json!({"region_id": 5})), "Drums (Pop)");
    }

    #[test]
    fn span_text_says_bars_and_cycle() {
        assert_eq!(span_text(span(0.0, 4, false)), "4 bars");
        assert_eq!(span_text(span(0.0, 1, false)), "1 bar");
        assert_eq!(span_text(span(9.0, 4, true)), "cycle, 4 bars");
    }

    #[test]
    fn default_choices_are_four_bars_medium_with_fill() {
        assert_eq!(PlayerChoices::default(), PlayerChoices { bars: 4, intensity: Intensity::Medium, fill: true });
    }
}
