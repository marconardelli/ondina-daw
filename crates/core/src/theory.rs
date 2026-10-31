//! Music theory helpers: note names, scales, keys and chord symbols.

use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

pub const NOTE_NAMES: [&str; 12] = ["C", "C#", "D", "D#", "E", "F", "F#", "G", "G#", "A", "A#", "B"];

#[derive(Debug, Clone, Copy, Serialize, Deserialize, JsonSchema, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum ScaleKind {
    Major,
    Minor,
    HarmonicMinor,
    MelodicMinor,
    Dorian,
    Phrygian,
    Lydian,
    Mixolydian,
    Locrian,
    MajorPentatonic,
    MinorPentatonic,
    Blues,
    Chromatic,
}

impl ScaleKind {
    pub fn intervals(&self) -> &'static [u8] {
        match self {
            ScaleKind::Major => &[0, 2, 4, 5, 7, 9, 11],
            ScaleKind::Minor => &[0, 2, 3, 5, 7, 8, 10],
            ScaleKind::HarmonicMinor => &[0, 2, 3, 5, 7, 8, 11],
            ScaleKind::MelodicMinor => &[0, 2, 3, 5, 7, 9, 11],
            ScaleKind::Dorian => &[0, 2, 3, 5, 7, 9, 10],
            ScaleKind::Phrygian => &[0, 1, 3, 5, 7, 8, 10],
            ScaleKind::Lydian => &[0, 2, 4, 6, 7, 9, 11],
            ScaleKind::Mixolydian => &[0, 2, 4, 5, 7, 9, 10],
            ScaleKind::Locrian => &[0, 1, 3, 5, 6, 8, 10],
            ScaleKind::MajorPentatonic => &[0, 2, 4, 7, 9],
            ScaleKind::MinorPentatonic => &[0, 3, 5, 7, 10],
            ScaleKind::Blues => &[0, 3, 5, 6, 7, 10],
            ScaleKind::Chromatic => &[0, 1, 2, 3, 4, 5, 6, 7, 8, 9, 10, 11],
        }
    }

    pub fn all() -> &'static [ScaleKind] {
        use ScaleKind::*;
        &[Major, Minor, HarmonicMinor, MelodicMinor, Dorian, Phrygian, Lydian, Mixolydian, Locrian, MajorPentatonic, MinorPentatonic, Blues, Chromatic]
    }

    pub fn label(&self) -> &'static str {
        use ScaleKind::*;
        match self {
            Major => "Major",
            Minor => "Minor",
            HarmonicMinor => "Harmonic Minor",
            MelodicMinor => "Melodic Minor",
            Dorian => "Dorian",
            Phrygian => "Phrygian",
            Lydian => "Lydian",
            Mixolydian => "Mixolydian",
            Locrian => "Locrian",
            MajorPentatonic => "Major Pentatonic",
            MinorPentatonic => "Minor Pentatonic",
            Blues => "Blues",
            Chromatic => "Chromatic",
        }
    }
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, JsonSchema, PartialEq, Eq)]
pub struct Key {
    /// Pitch class of the tonic, 0 = C .. 11 = B.
    pub root: u8,
    pub scale: ScaleKind,
}

impl Key {
    pub fn contains(&self, pitch: u8) -> bool {
        let pc = (pitch as i32 - self.root as i32).rem_euclid(12) as u8;
        self.scale.intervals().contains(&pc)
    }

    /// Snap a pitch to the nearest scale tone (prefers downward on ties).
    pub fn snap(&self, pitch: u8) -> u8 {
        if self.contains(pitch) {
            return pitch;
        }
        for d in 1..12i32 {
            let down = pitch as i32 - d;
            if down >= 0 && self.contains(down as u8) {
                return down as u8;
            }
            let up = pitch as i32 + d;
            if up <= 127 && self.contains(up as u8) {
                return up as u8;
            }
        }
        pitch
    }

    /// Pitch of the `degree`-th scale step (0-based, may be negative or exceed scale size) from the tonic in `octave`.
    pub fn degree_pitch(&self, degree: i32, octave: i32) -> u8 {
        let iv = self.scale.intervals();
        let n = iv.len() as i32;
        let oct = degree.div_euclid(n);
        let idx = degree.rem_euclid(n) as usize;
        let p = (octave + 1 + oct) * 12 + self.root as i32 + iv[idx] as i32;
        p.clamp(0, 127) as u8
    }

    pub fn name(&self) -> String {
        format!("{} {}", NOTE_NAMES[self.root as usize % 12], self.scale.label())
    }
}

pub fn note_name(pitch: u8) -> String {
    format!("{}{}", NOTE_NAMES[pitch as usize % 12], pitch as i32 / 12 - 1)
}

/// Parse a pitch class like "C", "F#", "Bb", "eb".
pub fn parse_pitch_class(s: &str) -> Option<(u8, usize)> {
    let mut chars = s.chars();
    let letter = chars.next()?.to_ascii_uppercase();
    let base: i32 = match letter {
        'C' => 0,
        'D' => 2,
        'E' => 4,
        'F' => 5,
        'G' => 7,
        'A' => 9,
        'B' => 11,
        _ => return None,
    };
    let mut used = 1;
    let mut pc = base;
    for c in chars {
        match c {
            '#' | '♯' => {
                pc += 1;
                used += c.len_utf8();
            }
            'b' | '♭' => {
                pc -= 1;
                used += c.len_utf8();
            }
            _ => break,
        }
    }
    Some((pc.rem_euclid(12) as u8, used))
}

/// Parse a note name like "C4", "F#3", "Bb-1" or a plain MIDI number "60".
pub fn parse_note(s: &str) -> Option<u8> {
    let s = s.trim();
    if let Ok(n) = s.parse::<i32>() {
        return (0..=127).contains(&n).then_some(n as u8);
    }
    let (pc, used) = parse_pitch_class(s)?;
    let oct: i32 = s[used..].parse().ok()?;
    let p = (oct + 1) * 12 + pc as i32;
    (0..=127).contains(&p).then_some(p as u8)
}

/// A parsed chord symbol.
#[derive(Debug, Clone, PartialEq)]
pub struct Chord {
    pub root: u8,
    /// Intervals in semitones above the root (including 0).
    pub intervals: Vec<u8>,
    pub bass: Option<u8>,
    pub quality: String,
}

impl Chord {
    pub fn parse(symbol: &str) -> Option<Chord> {
        let symbol = symbol.trim();
        let (main, bass) = match symbol.split_once('/') {
            Some((m, b)) => (m, parse_pitch_class(b).map(|x| x.0)),
            None => (symbol, None),
        };
        let (root, used) = parse_pitch_class(main)?;
        let q = &main[used..];
        let intervals: Vec<u8> = match q {
            "" | "maj" | "M" => vec![0, 4, 7],
            "m" | "min" | "-" => vec![0, 3, 7],
            "5" => vec![0, 7],
            "dim" | "o" => vec![0, 3, 6],
            "aug" | "+" => vec![0, 4, 8],
            "sus2" => vec![0, 2, 7],
            "sus4" | "sus" => vec![0, 5, 7],
            "6" => vec![0, 4, 7, 9],
            "m6" => vec![0, 3, 7, 9],
            "7" => vec![0, 4, 7, 10],
            "maj7" | "M7" | "Δ" | "Δ7" => vec![0, 4, 7, 11],
            "m7" | "min7" | "-7" => vec![0, 3, 7, 10],
            "mMaj7" | "mM7" => vec![0, 3, 7, 11],
            "m7b5" | "ø" | "ø7" => vec![0, 3, 6, 10],
            "dim7" | "o7" => vec![0, 3, 6, 9],
            "7sus4" => vec![0, 5, 7, 10],
            "add9" => vec![0, 4, 7, 14],
            "madd9" => vec![0, 3, 7, 14],
            "9" => vec![0, 4, 7, 10, 14],
            "maj9" | "M9" => vec![0, 4, 7, 11, 14],
            "m9" => vec![0, 3, 7, 10, 14],
            "11" => vec![0, 4, 7, 10, 14, 17],
            "m11" => vec![0, 3, 7, 10, 14, 17],
            "13" => vec![0, 4, 7, 10, 14, 21],
            "7b9" => vec![0, 4, 7, 10, 13],
            "7#9" => vec![0, 4, 7, 10, 15],
            _ => return None,
        };
        Some(Chord { root, intervals, bass, quality: q.to_string() })
    }

    /// Close voicing around `center` (MIDI pitch), optional inversion, plus optional bass note an octave below.
    pub fn voicing(&self, center: u8, inversion: u8, with_bass: bool) -> Vec<u8> {
        let mut base = (center as i32 / 12) * 12 + self.root as i32;
        if base > center as i32 + 6 {
            base -= 12;
        }
        let mut notes: Vec<i32> = self.intervals.iter().map(|i| base + *i as i32).collect();
        for _ in 0..inversion.min(notes.len() as u8 - 1) {
            notes.sort();
            let low = notes.remove(0);
            notes.push(low + 12);
        }
        notes.sort();
        if with_bass {
            let b = self.bass.unwrap_or(self.root) as i32;
            let mut bass = (notes[0] / 12) * 12 + b;
            while bass >= notes[0] {
                bass -= 12;
            }
            if bass - 12 >= 24 {
                bass -= 12;
            }
            notes.insert(0, bass);
        }
        notes.into_iter().filter(|p| (0..=127).contains(p)).map(|p| p as u8).collect()
    }

    pub fn bass_pitch_class(&self) -> u8 {
        self.bass.unwrap_or(self.root)
    }

    /// Pitch classes of chord tones.
    pub fn pitch_classes(&self) -> Vec<u8> {
        self.intervals.iter().map(|i| (self.root + i) % 12).collect()
    }
}

/// Roman numeral to chord symbol in a key, e.g. ("vi", C major) -> "Am".
pub fn roman_to_symbol(roman: &str, key: Key) -> Option<String> {
    let r = roman.trim();
    let upper = r.chars().next()?.is_uppercase();
    let core: String = r.chars().take_while(|c| "IViv".contains(*c)).collect();
    let suffix = &r[core.len()..];
    let degree = match core.to_uppercase().as_str() {
        "I" => 0,
        "II" => 1,
        "III" => 2,
        "IV" => 3,
        "V" => 4,
        "VI" => 5,
        "VII" => 6,
        _ => return None,
    };
    let pitch = key.degree_pitch(degree, 4) % 12;
    let q = if suffix.is_empty() { if upper { "" } else { "m" } } else { suffix };
    Some(format!("{}{}", NOTE_NAMES[pitch as usize], q))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn chords() {
        let c = Chord::parse("Am7").unwrap();
        assert_eq!(c.root, 9);
        assert_eq!(c.intervals, vec![0, 3, 7, 10]);
        let g = Chord::parse("G/B").unwrap();
        assert_eq!(g.bass, Some(11));
        assert_eq!(parse_note("C4"), Some(60));
        assert_eq!(parse_note("Bb3"), Some(58));
        let key = Key { root: 0, scale: ScaleKind::Major };
        assert_eq!(roman_to_symbol("vi", key).unwrap(), "Am");
        assert_eq!(roman_to_symbol("V7", key).unwrap(), "G7");
        assert_eq!(key.snap(61), 60);
    }
}
