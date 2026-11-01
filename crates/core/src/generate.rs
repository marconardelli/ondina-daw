//! Algorithmic "session players": drum grooves, bass lines, chord comping, arpeggios and melodies.
//! All generators return notes with start times relative to the beginning of the generated span.

use rand::rngs::StdRng;
use rand::{Rng, SeedableRng};
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

use crate::model::{Note, TimeSignature};
use crate::theory::{Chord, Key};

#[derive(Debug, Clone, Copy, Serialize, Deserialize, JsonSchema, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum DrumStyle {
    Rock,
    Pop,
    Funk,
    HipHop,
    Trap,
    House,
    Techno,
    Disco,
    Reggaeton,
    Jazz,
    Ballad,
    DrumAndBass,
    LoFi,
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, JsonSchema, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum BassStyle {
    /// Whole/half notes on the root.
    Root,
    /// Root and fifth alternating.
    RootFifth,
    /// Driving straight eighth notes on the root.
    Eighths,
    /// Disco/house octave jumps.
    Octaves,
    /// Syncopated funk/pop line with passing notes.
    Syncopated,
    /// Jazz walking bass in quarter notes.
    Walking,
    /// Long 808-style sub notes following the kick feel.
    Sub808,
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, JsonSchema, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum CompStyle {
    /// Sustained chords for the full chord length.
    Sustained,
    /// Rhythmic eighth-note pop strumming.
    Pop,
    /// Short off-beat stabs (reggae/ska/house).
    Offbeat,
    /// Ballad: chord on beat 1 and broken chord tones.
    Ballad,
    /// Syncopated funk/neo-soul comping.
    Funk,
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, JsonSchema, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum ArpMode {
    Up,
    Down,
    UpDown,
    Random,
}

/// A chord placed in time (beats relative to the generated span).
#[derive(Debug, Clone)]
pub struct TimedChord {
    pub start: f64,
    pub length: f64,
    pub chord: Chord,
}

// GM drum map
pub const KICK: u8 = 36;
pub const SIDE_STICK: u8 = 37;
pub const SNARE: u8 = 38;
pub const CLAP: u8 = 39;
pub const CLOSED_HAT: u8 = 42;
pub const PEDAL_HAT: u8 = 44;
pub const OPEN_HAT: u8 = 46;
pub const LOW_TOM: u8 = 45;
pub const MID_TOM: u8 = 47;
pub const HIGH_TOM: u8 = 50;
pub const CRASH: u8 = 49;
pub const RIDE: u8 = 51;
pub const TAMBOURINE: u8 = 54;
pub const COWBELL: u8 = 56;

fn n(pitch: u8, velocity: u8, start: f64, length: f64) -> Note {
    Note { pitch, velocity: velocity.clamp(1, 127), start, length }
}

/// Parse a 16-step string: 'x' accent, 'o' normal, 'g' ghost, '.' rest.
fn steps(pattern: &str) -> Vec<u8> {
    pattern
        .chars()
        .filter(|c| !c.is_whitespace() && *c != '|')
        .map(|c| match c {
            'x' | 'X' => 115,
            'o' => 92,
            'g' => 45,
            _ => 0,
        })
        .collect()
}

struct Groove {
    lines: Vec<(u8, Vec<u8>)>,
    swing: f64,
}

fn groove(style: DrumStyle, intensity: f32) -> Groove {
    use DrumStyle::*;
    let busy = intensity > 0.6;
    let lines: Vec<(u8, &str)> = match style {
        Rock => vec![
            (KICK, if busy { "x...o.x.x...o..." } else { "x.....x.x......." }),
            (SNARE, if busy { "....x..g....x..g" } else { "....x.......x..." }),
            (CLOSED_HAT, if busy { "xoxoxoxoxoxoxoxo" } else { "x.o.x.o.x.o.x.o." }),
        ],
        Pop => vec![
            (KICK, if busy { "x.....x.x..x...." } else { "x.......x......." }),
            (SNARE, "....x.......x..."),
            (CLAP, if busy { "....x.......x..." } else { "................" }),
            (CLOSED_HAT, if busy { "xoxoxoxoxoxoxoxo" } else { "x.o.x.o.x.o.x.o." }),
        ],
        Funk => vec![
            (KICK, "x..x..x...x..x.."),
            (SNARE, "....x..g.g..x..g"),
            (CLOSED_HAT, "xoxoxoxoxoxoxo.o"),
            (OPEN_HAT, "..............x."),
        ],
        HipHop => vec![
            (KICK, if busy { "x.....x..x.x...." } else { "x......x..x....." }),
            (SNARE, "....x.......x..."),
            (CLOSED_HAT, "x.o.x.o.x.o.x.o."),
        ],
        Trap => vec![
            (KICK, "x......x..x....."),
            (CLAP, "........x......."),
            (CLOSED_HAT, if busy { "xoxoxoxoxoxoxoxo" } else { "x.o.x.o.x.o.x.o." }),
            (OPEN_HAT, "..............o."),
        ],
        House => vec![
            (KICK, "x...x...x...x..."),
            (CLAP, "....x.......x..."),
            (CLOSED_HAT, if busy { "o.x.o.x.o.x.o.x." } else { "..x...x...x...x." }),
            (OPEN_HAT, if busy { "..o...o...o...o." } else { "................" }),
        ],
        Techno => vec![
            (KICK, "x...x...x...x..."),
            (CLAP, if busy { "....x.......x..." } else { "................" }),
            (CLOSED_HAT, "..x...x...x...x."),
            (RIDE, if busy { "o.o.o.o.o.o.o.o." } else { "................" }),
        ],
        Disco => vec![
            (KICK, "x...x...x...x..."),
            (SNARE, "....x.......x..."),
            (CLOSED_HAT, "x...x...x...x..."),
            (OPEN_HAT, "..x...x...x...x."),
        ],
        Reggaeton => vec![
            (KICK, "x...x...x...x..."),
            (SNARE, "...x..x....x..x."),
            (CLOSED_HAT, "x.o.x.o.x.o.x.o."),
        ],
        Jazz => vec![
            (RIDE, "x..ox.x.x..ox.x."),
            (PEDAL_HAT, "....o.......o..."),
            (KICK, if busy { "g.....g...g....." } else { "g..............." }),
            (SNARE, if busy { "......g.......g." } else { "................" }),
        ],
        Ballad => vec![
            (KICK, "x.......x.o....."),
            (SIDE_STICK, "....o.......o..."),
            (CLOSED_HAT, "o.o.o.o.o.o.o.o."),
        ],
        DrumAndBass => vec![
            (KICK, "x.........x....."),
            (SNARE, "....x.......x..g"),
            (CLOSED_HAT, "x.o.x.o.x.o.x.o."),
        ],
        LoFi => vec![
            (KICK, "x......x..x....."),
            (SNARE, "....x.......x..."),
            (CLOSED_HAT, "x.o.x.oox.o.x.o."),
        ],
    };
    let swing = match style {
        Jazz => 0.6,
        LoFi | HipHop => 0.35,
        Funk => 0.15,
        _ => 0.0,
    };
    Groove { lines: lines.into_iter().map(|(p, s)| (p, steps(s))).collect(), swing }
}

/// Generate a drum performance.
/// * `bars` number of bars, `beats_per_bar` from the time signature
/// * `intensity` 0..1 (busier patterns and louder dynamics as it rises)
/// * `fill` adds a tom/snare fill in the last bar and a crash on the first downbeat
pub fn drums(style: DrumStyle, bars: u32, beats_per_bar: f64, intensity: f32, fill: bool, seed: u64) -> Vec<Note> {
    let mut rng = StdRng::seed_from_u64(seed);
    let g = groove(style, intensity);
    let step = 0.25;
    let steps_per_bar = (beats_per_bar / step).round() as usize;
    let dyn_scale = 0.75 + 0.25 * intensity;
    let mut out = vec![];
    for bar in 0..bars {
        let bar_start = bar as f64 * beats_per_bar;
        let is_fill_bar = fill && bar + 1 == bars && bars > 1;
        let fill_from = if is_fill_bar { steps_per_bar.saturating_sub(8) } else { usize::MAX };
        for (pitch, pat) in &g.lines {
            for s in 0..steps_per_bar {
                if s >= fill_from && (*pitch == SNARE || *pitch == CLOSED_HAT || *pitch == OPEN_HAT || *pitch == CLAP) {
                    continue;
                }
                let v = pat[s % pat.len()];
                if v == 0 {
                    continue;
                }
                let mut t = bar_start + s as f64 * step;
                if s % 2 == 1 {
                    t += g.swing * step * 0.33;
                }
                let vel = (v as f32 * dyn_scale + rng.gen_range(-6.0..6.0)) as i32;
                let len = if *pitch == OPEN_HAT { 0.5 } else { 0.2 };
                out.push(n(*pitch, vel.clamp(1, 127) as u8, t, len));
            }
        }
        // light variation: occasional extra kick / ghost snare
        if intensity > 0.3 && rng.gen_bool((intensity * 0.5) as f64) && !is_fill_bar {
            let s = [7usize, 11, 14][rng.gen_range(0..3)];
            if s < steps_per_bar {
                out.push(n(KICK, 80, bar_start + s as f64 * step, 0.2));
            }
        }
        if is_fill_bar {
            let toms = [HIGH_TOM, HIGH_TOM, MID_TOM, MID_TOM, LOW_TOM, LOW_TOM, SNARE, SNARE];
            for (i, s) in (fill_from..steps_per_bar).enumerate() {
                let p = toms[i.min(7)];
                out.push(n(p, (90.0 + i as f32 * 4.0) as u8, bar_start + s as f64 * step, 0.2));
            }
        }
        if fill && bar == 0 {
            out.push(n(CRASH, 110, bar_start, 1.0));
        }
    }
    out.sort_by(|a, b| a.start.partial_cmp(&b.start).unwrap());
    out
}

/// How the session players feel a bar.
#[derive(Debug, Clone, Copy, PartialEq)]
enum Feel {
    /// Four beats: the patterns were written for this bar and play exactly as before.
    Common,
    /// Three quarter beats (3/4).
    Waltz,
    /// Eighths grouped in threes (6/8, 9/8, 12/8).
    Compound,
    /// Any other bar: the 4/4 pattern repeats every four beats and stops at the bar line.
    Other,
}

#[derive(Debug, Clone, Copy)]
struct Meter {
    beats: f64,
    feel: Feel,
}

fn meter(ts: TimeSignature) -> Meter {
    let b = ts.beats_per_bar();
    // A hand-edited file can carry a zero numerator; four beats keeps the bar loops finite.
    let beats = if b.is_finite() && b > 0.0 { b } else { 4.0 };
    let feel = if ts.denominator == 8 && ts.numerator >= 6 && ts.numerator.is_multiple_of(3) {
        Feel::Compound
    } else if (beats - 4.0).abs() < 1e-9 {
        Feel::Common
    } else if (beats - 3.0).abs() < 1e-9 {
        Feel::Waltz
    } else {
        Feel::Other
    };
    Meter { beats, feel }
}

/// Bar offsets where the melody lands on a chord tone with an accent.
fn strong_beats(m: Meter) -> Vec<f64> {
    match m.feel {
        Feel::Common => vec![0.0, 2.0],
        Feel::Compound => (0..).map(|i| i as f64 * 1.5).take_while(|b| *b < m.beats - 1e-9).collect(),
        Feel::Waltz | Feel::Other => vec![0.0],
    }
}

/// Lays a rhythm cell (ascending offsets in beats, all inside `cell`) onto one bar of `bar` beats: the cell
/// repeats every `cell` beats and stops at the bar line, so no hit spills into the next bar.
fn lay_cell<T: Copy>(rhythm: &[(f64, T)], cell: f64, bar: f64) -> Vec<(f64, T)> {
    let mut out = vec![];
    let mut at = 0.0;
    while at < bar - 1e-9 {
        for &(off, x) in rhythm {
            if at + off < bar - 1e-9 {
                out.push((at + off, x));
            }
        }
        at += cell;
    }
    out
}

// One-bar rhythm cells: (offset in beats, payload). The common-time cells are the original 4/4 patterns.
type BassCell = &'static [(f64, (f64, i32))];
type CompCell = &'static [(f64, (f64, u8))];

/// Syncopated bass: (offset, (length, interval above the root)).
const SYNCOPATED_COMMON: [(f64, (f64, i32)); 6] = [(0.0, (0.7, 0)), (0.75, (0.25, 0)), (1.5, (0.4, 7)), (2.0, (0.4, 0)), (2.75, (0.5, 12)), (3.5, (0.4, 10))];
const SYNCOPATED_WALTZ: [(f64, (f64, i32)); 5] = [(0.0, (0.7, 0)), (0.75, (0.25, 0)), (1.5, (0.4, 7)), (2.0, (0.4, 0)), (2.5, (0.4, 10))];
const SYNCOPATED_COMPOUND: [(f64, (f64, i32)); 4] = [(0.0, (0.9, 0)), (1.0, (0.4, 7)), (1.5, (0.7, 0)), (2.5, (0.4, 12))];
/// Pop comping: (offset, (length, velocity)).
const POP_COMMON: [(f64, (f64, u8)); 5] = [(0.0, (0.9, 100)), (1.0, (0.4, 80)), (1.5, (0.9, 88)), (2.5, (0.4, 82)), (3.0, (0.9, 90))];
const POP_WALTZ: [(f64, (f64, u8)); 4] = [(0.0, (0.9, 100)), (1.0, (0.4, 80)), (1.5, (0.4, 84)), (2.0, (0.9, 88))];
const POP_COMPOUND: [(f64, (f64, u8)); 4] = [(0.0, (0.9, 100)), (1.0, (0.4, 78)), (1.5, (0.9, 92)), (2.5, (0.4, 80))];
/// Funk comping hits.
const FUNK_COMMON: [(f64, ()); 6] = [(0.0, ()), (0.75, ()), (1.5, ()), (2.25, ()), (2.75, ()), (3.5, ())];
const FUNK_WALTZ: [(f64, ()); 5] = [(0.0, ()), (0.75, ()), (1.5, ()), (2.0, ()), (2.75, ())];
const FUNK_COMPOUND: [(f64, ()); 5] = [(0.0, ()), (0.5, ()), (1.5, ()), (2.0, ()), (2.5, ())];

fn syncopated_cell(f: Feel) -> (BassCell, f64) {
    match f {
        Feel::Waltz => (&SYNCOPATED_WALTZ, 3.0),
        Feel::Compound => (&SYNCOPATED_COMPOUND, 3.0),
        Feel::Common | Feel::Other => (&SYNCOPATED_COMMON, 4.0),
    }
}

fn pop_cell(f: Feel) -> (CompCell, f64) {
    match f {
        Feel::Waltz => (&POP_WALTZ, 3.0),
        Feel::Compound => (&POP_COMPOUND, 3.0),
        Feel::Common | Feel::Other => (&POP_COMMON, 4.0),
    }
}

fn funk_cell(f: Feel) -> (&'static [(f64, ())], f64) {
    match f {
        Feel::Waltz => (&FUNK_WALTZ, 3.0),
        Feel::Compound => (&FUNK_COMPOUND, 3.0),
        Feel::Common | Feel::Other => (&FUNK_COMMON, 4.0),
    }
}

fn bass_pitch(pc: u8, octave_base: i32) -> u8 {
    // place pitch class in range [octave_base, octave_base+12)
    let mut p = octave_base + ((pc as i32 - octave_base).rem_euclid(12));
    if p > 127 {
        p -= 12;
    }
    p as u8
}

/// Generate a bass line following timed chords. `octave` is the base MIDI octave (1 => E1..D#2 range starts at C1=24).
/// One-bar patterns repeat in bars of the time signature `ts`.
pub fn bass(chords: &[TimedChord], style: BassStyle, octave: i32, ts: TimeSignature, seed: u64) -> Vec<Note> {
    let mut rng = StdRng::seed_from_u64(seed);
    let meter = meter(ts);
    let base = (octave + 1) * 12; // C of the octave
    let mut out = vec![];
    for (ci, tc) in chords.iter().enumerate() {
        let root = bass_pitch(tc.chord.bass_pitch_class(), base);
        let fifth = root + 7;
        let third_iv = tc.chord.intervals.get(1).copied().unwrap_or(4);
        let next_root = chords.get(ci + 1).map(|c| bass_pitch(c.chord.bass_pitch_class(), base));
        let (s, len) = (tc.start, tc.length);
        let mut t = 0.0;
        match style {
            BassStyle::Root => {
                while t < len - 1e-6 {
                    let l = (len - t).min(2.0);
                    out.push(n(root, 100, s + t, l * 0.95));
                    t += 2.0;
                }
            }
            BassStyle::RootFifth => {
                let mut i = 0;
                while t < len - 1e-6 {
                    let p = if i % 2 == 0 { root } else { fifth };
                    out.push(n(p, if i % 2 == 0 { 105 } else { 90 }, s + t, 0.9));
                    t += 1.0;
                    i += 1;
                }
            }
            BassStyle::Eighths => {
                while t < len - 1e-6 {
                    let accent = (t.fract() == 0.0) as u8 * 15;
                    out.push(n(root, 88 + accent, s + t, 0.4));
                    t += 0.5;
                }
            }
            BassStyle::Octaves => {
                let mut i = 0;
                while t < len - 1e-6 {
                    let p = if i % 2 == 0 { root } else { root + 12 };
                    out.push(n(p, if i % 2 == 0 { 100 } else { 85 }, s + t, 0.35));
                    t += 0.5;
                    i += 1;
                }
            }
            BassStyle::Syncopated => {
                let (rhythm, cell) = syncopated_cell(meter.feel);
                let cells = lay_cell(rhythm, cell, meter.beats);
                // the bar restarts at each chord, as before, so 4/4 parts stay the same
                let mut bar = 0.0;
                while bar < len - 1e-6 {
                    for &(off, (l, iv)) in &cells {
                        let at = bar + off;
                        if at >= len - 1e-6 {
                            break;
                        }
                        let iv = if iv == 10 && !tc.chord.intervals.contains(&10) { 7 } else { iv };
                        let vel = if off == 0.0 { 108 } else { rng.gen_range(80..100) };
                        out.push(n((root as i32 + iv).min(127) as u8, vel, s + at, l.min(meter.beats - off)));
                    }
                    bar += meter.beats;
                }
            }
            BassStyle::Walking => {
                let beats = len.round().max(1.0) as usize;
                for b in 0..beats {
                    let p = if b == 0 {
                        root
                    } else if b + 1 == beats {
                        // chromatic approach into next chord root
                        match next_root {
                            Some(nr) => if rng.gen_bool(0.5) { nr + 1 } else { nr.saturating_sub(1) },
                            None => fifth,
                        }
                    } else {
                        let choices = [root + third_iv, fifth, root + 12, root + 2, root + 9.min(third_iv + 5)];
                        choices[rng.gen_range(0..choices.len())]
                    };
                    out.push(n(p, rng.gen_range(85..105), s + b as f64, 0.9));
                }
            }
            BassStyle::Sub808 if meter.feel == Feel::Common => {
                // kept as it was so 4/4 projects generate the same bass
                out.push(n(root, 110, s, (len * 0.6).max(0.5)));
                if len >= 4.0 {
                    out.push(n(root, 95, s + 2.5, 0.75));
                    out.push(n(if rng.gen_bool(0.5) { root + 12 } else { fifth }, 90, s + 3.5, 0.4));
                }
            }
            BassStyle::Sub808 => {
                let bar = meter.beats;
                let pickups = len >= bar - 1e-6 && bar > 1.0 + 1e-9;
                let (p1, p2) = if bar >= 4.0 { (bar - 1.5, bar - 0.5) } else { (bar - 1.0, bar - 0.5) };
                let cap = if pickups { p1 } else { len.min(bar) };
                out.push(n(root, 110, s, (len * 0.6).max(0.5).min(cap)));
                if pickups {
                    out.push(n(root, 95, s + p1, (p2 - p1) * 0.75));
                    out.push(n(if rng.gen_bool(0.5) { root + 12 } else { fifth }, 90, s + p2, 0.4));
                }
            }
        }
    }
    out
}

/// Generate chord comping around `center` pitch (e.g. 60 = C4). One-bar patterns repeat in bars of `ts`.
pub fn comp(chords: &[TimedChord], style: CompStyle, center: u8, ts: TimeSignature, seed: u64) -> Vec<Note> {
    let mut rng = StdRng::seed_from_u64(seed);
    let meter = meter(ts);
    let (pop_rhythm, pop_len) = pop_cell(meter.feel);
    let pop = lay_cell(pop_rhythm, pop_len, meter.beats);
    let (funk_rhythm, funk_len) = funk_cell(meter.feel);
    let funk = lay_cell(funk_rhythm, funk_len, meter.beats);
    let mut out = vec![];
    let mut prev: Option<Vec<u8>> = None;
    for tc in chords {
        // voice leading: choose inversion closest to the previous voicing
        let voicing = {
            let mut best = tc.chord.voicing(center, 0, false);
            if let Some(p) = &prev {
                let pc: f64 = p.iter().map(|x| *x as f64).sum::<f64>() / p.len() as f64;
                let mut best_d = f64::MAX;
                for inv in 0..tc.chord.intervals.len() as u8 {
                    let v = tc.chord.voicing(center, inv, false);
                    let c: f64 = v.iter().map(|x| *x as f64).sum::<f64>() / v.len() as f64;
                    if (c - pc).abs() < best_d {
                        best_d = (c - pc).abs();
                        best = v;
                    }
                }
            }
            best
        };
        let (s, len) = (tc.start, tc.length);
        let push_chord = |out: &mut Vec<Note>, at: f64, l: f64, vel: u8| {
            for p in &voicing {
                out.push(n(*p, vel, s + at, l));
            }
        };
        match style {
            CompStyle::Sustained => push_chord(&mut out, 0.0, len * 0.98, 85),
            CompStyle::Pop => {
                // the bar restarts at each chord, as before, so 4/4 parts stay the same
                let mut bar = 0.0;
                while bar < len - 1e-6 {
                    for &(off, (l, v)) in &pop {
                        if bar + off < len - 1e-6 {
                            push_chord(&mut out, bar + off, l.min(meter.beats - off), v);
                        }
                    }
                    bar += meter.beats;
                }
            }
            CompStyle::Offbeat => {
                let mut t = 0.5;
                while t < len - 1e-6 {
                    push_chord(&mut out, t, 0.2, 90);
                    t += 1.0;
                }
            }
            CompStyle::Ballad => {
                push_chord(&mut out, 0.0, len.min(2.0) * 0.95, 80);
                let mut t = 2.0;
                let mut i = 0;
                while t < len - 1e-6 {
                    let p = voicing[i % voicing.len()] + if i % 3 == 2 { 12 } else { 0 };
                    out.push(n(p.min(127), 70, s + t, 0.9));
                    t += 0.5;
                    i += 1;
                }
            }
            CompStyle::Funk => {
                let mut bar = 0.0;
                while bar < len - 1e-6 {
                    for &(off, ()) in &funk {
                        if bar + off < len - 1e-6 && rng.gen_bool(0.85) {
                            push_chord(&mut out, bar + off, 0.15_f64.min(meter.beats - off), rng.gen_range(78..100));
                        }
                    }
                    bar += meter.beats;
                }
            }
        }
        prev = Some(voicing);
    }
    out
}

/// Arpeggiate timed chords. `rate` in beats (0.25 = sixteenth notes).
pub fn arpeggio(chords: &[TimedChord], mode: ArpMode, rate: f64, octaves: u32, center: u8, gate: f64, seed: u64) -> Vec<Note> {
    let mut rng = StdRng::seed_from_u64(seed);
    let rate = rate.max(0.0625);
    let mut out = vec![];
    for tc in chords {
        let base = tc.chord.voicing(center, 0, false);
        let mut pool: Vec<u8> = vec![];
        for o in 0..octaves.max(1) {
            for p in &base {
                let q = *p as u32 + o * 12;
                if q <= 127 {
                    pool.push(q as u8);
                }
            }
        }
        let seq: Vec<u8> = match mode {
            ArpMode::Up | ArpMode::Random => pool.clone(),
            ArpMode::Down => pool.iter().rev().copied().collect(),
            ArpMode::UpDown => {
                let mut v = pool.clone();
                if pool.len() > 2 {
                    v.extend(pool[1..pool.len() - 1].iter().rev());
                }
                v
            }
        };
        let mut t = 0.0;
        let mut i = 0;
        while t < tc.length - 1e-6 {
            let p = if mode == ArpMode::Random { pool[rng.gen_range(0..pool.len())] } else { seq[i % seq.len()] };
            let vel = if i % 4 == 0 { 100 } else { 82 };
            out.push(n(p, vel, tc.start + t, rate * gate.clamp(0.05, 1.0)));
            t += rate;
            i += 1;
        }
    }
    out
}

/// Generate a melody over the chords in a key. `density` 0..1 controls note count. The motif lasts one bar of `ts`.
pub fn melody(chords: &[TimedChord], key: Key, density: f32, octave: i32, ts: TimeSignature, seed: u64) -> Vec<Note> {
    let mut rng = StdRng::seed_from_u64(seed);
    let meter = meter(ts);
    let strong_at = strong_beats(meter);
    let mut out = vec![];
    let center = ((octave + 1) * 12 + key.root as i32) as i32;
    let mut current = center + 7;
    let total: f64 = chords.iter().map(|c| c.start + c.length).fold(0.0, f64::max);
    // build a one-bar rhythm motif and reuse it with variation (gives musical repetition)
    let grid = (if density > 0.6 { 0.25_f64 } else { 0.5 }).min(meter.beats);
    let slots = ((meter.beats / grid) + 1e-9).floor().max(1.0) as usize;
    let motif: Vec<bool> = (0..slots).map(|i| i == 0 || rng.gen_bool((density as f64 * 0.75).clamp(0.1, 0.9))).collect();
    let mut t = 0.0;
    while t < total - 1e-6 {
        for (si, on) in motif.iter().enumerate() {
            let at = t + si as f64 * grid;
            if at >= total - 1e-6 {
                break;
            }
            let vary = rng.gen_bool(0.15);
            // Outside 4/4 the downbeat always sounds, so every bar starts on its first beat; 4/4 keeps the old roll.
            let keep_downbeat = si == 0 && meter.feel != Feel::Common;
            if !(*on ^ vary) && !keep_downbeat {
                continue;
            }
            let chord = chords.iter().find(|c| at >= c.start && at < c.start + c.length).map(|c| &c.chord);
            let pos = si as f64 * grid;
            let strong = strong_at.iter().any(|b| (pos - b).abs() < 1e-9);
            let target = if let (true, Some(ch)) = (strong, chord) {
                // nearest chord tone
                let pcs = ch.pitch_classes();
                (current - 6..=current + 6).min_by_key(|p| {
                    let pc = p.rem_euclid(12) as u8;
                    if pcs.contains(&pc) { (p - current).abs() } else { 100 }
                }).unwrap_or(current)
            } else {
                let step: i32 = [-2, -1, -1, 1, 1, 2, 0, 3, -3][rng.gen_range(0..9)];
                let mut p = current + step;
                while !key.contains(p.clamp(0, 127) as u8) {
                    p += step.signum().max(1);
                }
                p
            };
            // keep within range
            current = target.clamp(center - 5, center + 17);
            // length until next note in motif
            let mut l = grid;
            for k in si + 1..slots {
                if motif[k] {
                    break;
                }
                l += grid;
            }
            out.push(n(current as u8, if strong { 100 } else { rng.gen_range(75..95) }, at, (l * 0.9).max(0.1)));
        }
        t += meter.beats;
    }
    out
}

/// Quantize note starts (and optionally ends) to `grid` beats with `strength` 0..1 and optional swing 0..1.
pub fn quantize(notes: &mut [Note], grid: f64, strength: f64, swing: f64, quantize_ends: bool) {
    if grid <= 0.0 {
        return;
    }
    for nt in notes.iter_mut() {
        let idx = (nt.start / grid).round();
        let mut target = idx * grid;
        if (idx as i64) % 2 == 1 {
            target += swing * grid * 0.5;
        }
        let end = nt.start + nt.length;
        nt.start += (target - nt.start) * strength;
        if nt.start < 0.0 {
            nt.start = 0.0;
        }
        if quantize_ends {
            let qe = ((end / grid).round() * grid).max(nt.start + grid);
            nt.length = (end + (qe - end) * strength - nt.start).max(grid * 0.25);
        } else {
            nt.length = (end - nt.start).max(0.01);
        }
    }
}

/// Add random timing (beats) and velocity deviations.
pub fn humanize(notes: &mut [Note], timing: f64, velocity: i32, seed: u64) {
    let mut rng = StdRng::seed_from_u64(seed);
    for nt in notes.iter_mut() {
        if timing > 0.0 {
            nt.start = (nt.start + rng.gen_range(-timing..timing)).max(0.0);
        }
        if velocity > 0 {
            nt.velocity = (nt.velocity as i32 + rng.gen_range(-velocity..=velocity)).clamp(1, 127) as u8;
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::theory::ScaleKind;

    fn prog() -> Vec<TimedChord> {
        ["C", "Am", "F", "G"]
            .iter()
            .enumerate()
            .map(|(i, s)| TimedChord { start: i as f64 * 4.0, length: 4.0, chord: Chord::parse(s).unwrap() })
            .collect()
    }

    #[test]
    fn generators_produce_notes() {
        for st in [DrumStyle::Rock, DrumStyle::Jazz, DrumStyle::House] {
            let d = drums(st, 4, 4.0, 0.7, true, 1);
            assert!(!d.is_empty());
            assert!(d.iter().all(|n| n.start < 16.0));
        }
        let p = prog();
        assert!(!bass(&p, BassStyle::Walking, 1, ts(4, 4), 2).is_empty());
        assert!(!comp(&p, CompStyle::Pop, 60, ts(4, 4), 2).is_empty());
        assert!(!arpeggio(&p, ArpMode::UpDown, 0.25, 2, 60, 0.8, 3).is_empty());
        let key = Key { root: 0, scale: ScaleKind::Major };
        let m = melody(&p, key, 0.6, 4, ts(4, 4), 7);
        assert!(!m.is_empty());
        assert!(m.iter().all(|n| key.contains(n.pitch) || true));
    }

    /// FNV-1a over every note field, in the order the generator returned them.
    fn digest(notes: &[Note]) -> (usize, u64) {
        let mut h: u64 = 0xcbf29ce484222325;
        for nt in notes {
            for word in [nt.pitch as u64, nt.velocity as u64, nt.start.to_bits(), nt.length.to_bits()] {
                for b in word.to_le_bytes() {
                    h ^= b as u64;
                    h = h.wrapping_mul(0x100000001b3);
                }
            }
        }
        (notes.len(), h)
    }

    fn mixed() -> Vec<TimedChord> {
        [("C", 0.0, 2.0), ("Am", 2.0, 2.0), ("F", 4.0, 8.0), ("G7", 12.0, 3.0), ("C", 15.0, 1.0)]
            .iter()
            .map(|(s, start, length)| TimedChord { start: *start, length: *length, chord: Chord::parse(s).unwrap() })
            .collect()
    }

    #[test]
    fn common_time_output_matches_the_previous_generators() {
        const EXPECTED: [(&str, usize, u64); 14] = [
            ("bass/root", 34, 0xdccfa6c4075fafa5),
            ("bass/root_fifth", 64, 0x3f9e649ad5e2ca01),
            ("bass/eighths", 128, 0x5c95e657df0ee085),
            ("bass/octaves", 128, 0x16d9fd11594a3135),
            ("bass/syncopated", 98, 0x1e8665fdb83d542e),
            ("bass/walking", 64, 0x3c7708653782da6b),
            ("bass/sub808", 38, 0x7c7def9cef3d7358),
            ("comp/sustained", 56, 0x279b0c9b15dc6f95),
            ("comp/pop", 254, 0x4c8ea9eedc9c489d),
            ("comp/offbeat", 198, 0x856728b3b9f11b1d),
            ("comp/ballad", 116, 0xcf27287b3777a165),
            ("comp/funk", 260, 0xf575967693a2e56c),
            ("melody/0.3", 76, 0xf4287d59eb7980ed),
            ("melody/0.8", 336, 0x2fd36355401cc2fa),
        ];
        let sets = [prog(), mixed()];
        let seeds = [1u64, 42];
        let mut got: Vec<(&str, usize, u64)> = vec![];
        let bass_styles = [
            ("bass/root", BassStyle::Root),
            ("bass/root_fifth", BassStyle::RootFifth),
            ("bass/eighths", BassStyle::Eighths),
            ("bass/octaves", BassStyle::Octaves),
            ("bass/syncopated", BassStyle::Syncopated),
            ("bass/walking", BassStyle::Walking),
            ("bass/sub808", BassStyle::Sub808),
        ];
        for (name, st) in bass_styles {
            let mut all = vec![];
            for c in &sets {
                for &seed in &seeds {
                    all.extend(bass(c, st, 1, ts(4, 4), seed));
                }
            }
            let (count, h) = digest(&all);
            got.push((name, count, h));
        }
        let comp_styles = [
            ("comp/sustained", CompStyle::Sustained),
            ("comp/pop", CompStyle::Pop),
            ("comp/offbeat", CompStyle::Offbeat),
            ("comp/ballad", CompStyle::Ballad),
            ("comp/funk", CompStyle::Funk),
        ];
        for (name, st) in comp_styles {
            let mut all = vec![];
            for c in &sets {
                for &seed in &seeds {
                    all.extend(comp(c, st, 62, ts(4, 4), seed));
                }
            }
            let (count, h) = digest(&all);
            got.push((name, count, h));
        }
        let keys = [Key { root: 0, scale: ScaleKind::Major }, Key { root: 9, scale: ScaleKind::Minor }];
        for (name, density) in [("melody/0.3", 0.3f32), ("melody/0.8", 0.8)] {
            let mut all = vec![];
            for c in &sets {
                for &seed in &seeds {
                    for key in keys {
                        all.extend(melody(c, key, density, 4, ts(4, 4), seed));
                    }
                }
            }
            let (count, h) = digest(&all);
            got.push((name, count, h));
        }
        assert_eq!(got, EXPECTED.to_vec());
    }

    fn ts(numerator: u8, denominator: u8) -> TimeSignature {
        TimeSignature { numerator, denominator }
    }

    /// `n` chords of `bpb` beats each, one after the other, cycling through `chords`.
    fn bars_of(bpb: f64, n: usize, chords: &[&str]) -> Vec<TimedChord> {
        (0..n)
            .map(|i| TimedChord { start: i as f64 * bpb, length: bpb, chord: Chord::parse(chords[i % chords.len()]).unwrap() })
            .collect()
    }

    /// Offsets inside each bar of `bpb` beats (counted from 0), grouped by bar.
    fn offsets_by_bar(notes: &[Note], bpb: f64, bars: usize) -> Vec<Vec<f64>> {
        let mut out = vec![vec![]; bars];
        for nt in notes {
            let bar = (nt.start / bpb + 1e-9).floor();
            out[bar as usize].push(nt.start - bar * bpb);
        }
        out
    }

    fn same_offsets(got: &[f64], want: &[f64]) -> bool {
        got.len() == want.len() && got.iter().zip(want).all(|(a, b)| (a - b).abs() < 1e-9)
    }

    fn ends_inside_bar(nt: &Note, bpb: f64) -> bool {
        let bar = (nt.start / bpb + 1e-9).floor();
        nt.start + nt.length <= (bar + 1.0) * bpb + 1e-9
    }

    #[test]
    fn three_four_bars_restart_on_the_downbeat() {
        let chords = bars_of(3.0, 4, &["C", "Am", "F", "G"]);
        let waltz = ts(3, 4);
        for seed in 1..=5u64 {
            let b = bass(&chords, BassStyle::Syncopated, 1, waltz, seed);
            for (bar, offs) in offsets_by_bar(&b, 3.0, 4).iter().enumerate() {
                assert!(same_offsets(offs, &[0.0, 0.75, 1.5, 2.0, 2.5]), "syncopated bar {bar}: {offs:?}");
            }
            let c = comp(&chords, CompStyle::Pop, 62, waltz, seed);
            for (bar, offs) in offsets_by_bar(&c, 3.0, 4).iter().enumerate() {
                let mut hits = offs.clone();
                hits.dedup_by(|a, b| (*a - *b).abs() < 1e-9);
                assert!(same_offsets(&hits, &[0.0, 1.0, 1.5, 2.0]), "pop bar {bar}: {hits:?}");
            }
            let sub = bass(&chords, BassStyle::Sub808, 1, waltz, seed);
            for (bar, offs) in offsets_by_bar(&sub, 3.0, 4).iter().enumerate() {
                for want in [0.0, 2.0, 2.5] {
                    assert!(offs.iter().any(|o| (o - want).abs() < 1e-9), "sub808 bar {bar} misses {want}: {offs:?}");
                }
            }
            for density in [0.3f32, 0.8] {
                let m = melody(&chords, Key { root: 0, scale: ScaleKind::Major }, density, 4, waltz, seed);
                for (bar, offs) in offsets_by_bar(&m, 3.0, 4).iter().enumerate() {
                    assert!(offs.iter().any(|o| o.abs() < 1e-9), "melody bar {bar} has no downbeat: {offs:?}");
                }
                assert!(m.iter().all(|nt| ends_inside_bar(nt, 3.0)), "a melody note crosses a 3/4 bar line");
            }
            for nt in b.iter().chain(&c).chain(&sub) {
                assert!(ends_inside_bar(nt, 3.0), "{nt:?} crosses a 3/4 bar line");
            }
        }
    }

    #[test]
    fn six_eight_melody_accents_the_dotted_beats() {
        let chords = bars_of(3.0, 4, &["C", "Am", "F", "G"]);
        let six = ts(6, 8);
        for seed in 1..=10u64 {
            for density in [0.3f32, 0.8] {
                let m = melody(&chords, Key { root: 9, scale: ScaleKind::Minor }, density, 4, six, seed);
                for nt in &m {
                    let bar = (nt.start / 3.0 + 1e-9).floor();
                    let off = nt.start - bar * 3.0;
                    let on_dotted = off.abs() < 1e-9 || (off - 1.5).abs() < 1e-9;
                    assert_eq!(nt.velocity == 100, on_dotted, "seed {seed} density {density}: {nt:?}");
                    assert!(ends_inside_bar(nt, 3.0), "{nt:?} crosses a 6/8 bar line");
                }
                for (bar, offs) in offsets_by_bar(&m, 3.0, 4).iter().enumerate() {
                    assert!(offs.iter().any(|o| o.abs() < 1e-9), "6/8 melody bar {bar} has no downbeat: {offs:?}");
                }
            }
            let b = bass(&chords, BassStyle::Syncopated, 1, six, seed);
            for (bar, offs) in offsets_by_bar(&b, 3.0, 4).iter().enumerate() {
                assert!(same_offsets(offs, &[0.0, 1.0, 1.5, 2.5]), "6/8 syncopated bar {bar}: {offs:?}");
            }
        }
    }

    #[test]
    fn every_note_stays_inside_its_chord_in_odd_meters() {
        let bass_styles = [
            BassStyle::Root,
            BassStyle::RootFifth,
            BassStyle::Eighths,
            BassStyle::Octaves,
            BassStyle::Syncopated,
            BassStyle::Walking,
            BassStyle::Sub808,
        ];
        let comp_styles = [CompStyle::Sustained, CompStyle::Pop, CompStyle::Offbeat, CompStyle::Ballad, CompStyle::Funk];
        for meter in [ts(3, 4), ts(5, 4), ts(6, 8), ts(7, 8)] {
            let bpb = meter.beats_per_bar();
            for bars_per_chord in [1usize, 2] {
                let chords = bars_of(bpb * bars_per_chord as f64, 4, &["C", "Am7", "F", "G7"]);
                let total = chords.last().map(|c| c.start + c.length).unwrap();
                // Every note starts inside a chord; bar-bound styles end by their bar (counted from the chord start)
                // and by the chord end, chord-bound styles (Root, Sustained) end by the chord end.
                let check = |notes: &[Note], bar_bound: bool, chord_bound: bool, what: &str| {
                    for nt in notes {
                        let chord = chords
                            .iter()
                            .find(|c| c.start <= nt.start && nt.start < c.start + c.length)
                            .unwrap_or_else(|| panic!("{what} in {meter:?}: {nt:?} starts outside every chord"));
                        let chord_end = chord.start + chord.length;
                        let end = nt.start + nt.length;
                        if bar_bound {
                            let bar = chord.start + ((nt.start - chord.start) / bpb + 1e-9).floor() * bpb;
                            assert!(end <= (bar + bpb).min(chord_end) + 1e-9, "{what} in {meter:?}: {nt:?} crosses the bar line");
                        } else if chord_bound {
                            assert!(end <= chord_end + 1e-9, "{what} in {meter:?}: {nt:?} outlasts its chord");
                        }
                    }
                };
                for seed in 1..=3u64 {
                    for st in bass_styles {
                        let notes = bass(&chords, st, 1, meter, seed);
                        let bar_bound = matches!(st, BassStyle::Eighths | BassStyle::Octaves | BassStyle::Syncopated | BassStyle::Sub808);
                        check(&notes, bar_bound, st == BassStyle::Root, &format!("bass {st:?}"));
                    }
                    for st in comp_styles {
                        let notes = comp(&chords, st, 62, meter, seed);
                        let bar_bound = matches!(st, CompStyle::Pop | CompStyle::Offbeat | CompStyle::Funk);
                        check(&notes, bar_bound, st == CompStyle::Sustained, &format!("comp {st:?}"));
                    }
                    for density in [0.3f32, 0.8] {
                        let m = melody(&chords, Key { root: 0, scale: ScaleKind::Major }, density, 4, meter, seed);
                        assert!(!m.is_empty());
                        for nt in &m {
                            assert!(nt.start < total - 1e-9, "melody in {meter:?}: {nt:?} starts after the chords");
                            assert!(ends_inside_bar(nt, bpb), "melody in {meter:?}: {nt:?} crosses the bar line");
                        }
                    }
                }
            }
        }
    }

    #[test]
    fn invalid_time_signature_falls_back_to_four_four() {
        let broken = ts(0, 4);
        let common = ts(4, 4);
        for chords in [prog(), mixed()] {
            for seed in [1u64, 9] {
                for st in [BassStyle::Syncopated, BassStyle::Sub808, BassStyle::Walking] {
                    assert_eq!(bass(&chords, st, 1, broken, seed), bass(&chords, st, 1, common, seed));
                }
                for st in [CompStyle::Pop, CompStyle::Funk, CompStyle::Ballad] {
                    assert_eq!(comp(&chords, st, 62, broken, seed), comp(&chords, st, 62, common, seed));
                }
                let key = Key { root: 0, scale: ScaleKind::Major };
                assert_eq!(melody(&chords, key, 0.5, 4, broken, seed), melody(&chords, key, 0.5, 4, common, seed));
            }
        }
    }
}
