//! Pure project-model command execution. Engine/IO-level commands (transport, bounce,
//! file import, devices, analysis) are handled by the studio session and return
//! [`Outcome::NotHandled`] here.

use serde_json::{json, Value};

use crate::command::{CcPoint, Command, NoteLengthMode};
use crate::generate::{self, TimedChord};
use crate::model::*;
use crate::plugins::{self, PluginCategory};
use crate::theory::{self, Chord, Key, ScaleKind};

pub enum Outcome {
    Done(Value),
    NotHandled,
}

type R = Result<Outcome, String>;

fn ok(v: Value) -> R {
    Ok(Outcome::Done(v))
}

fn err<T>(msg: impl Into<String>) -> Result<T, String> {
    Err(msg.into())
}

pub fn parse_key(s: &str) -> Result<Key, String> {
    let s = s.trim();
    let (pc, used) = theory::parse_pitch_class(s).ok_or_else(|| format!("invalid key '{s}'"))?;
    let rest = s[used..].trim().to_lowercase().replace([' ', '-'], "_");
    let scale = match rest.as_str() {
        "" | "major" | "maj" | "ionian" => ScaleKind::Major,
        "m" | "minor" | "min" | "aeolian" | "natural_minor" => ScaleKind::Minor,
        "harmonic_minor" => ScaleKind::HarmonicMinor,
        "melodic_minor" => ScaleKind::MelodicMinor,
        "dorian" => ScaleKind::Dorian,
        "phrygian" => ScaleKind::Phrygian,
        "lydian" => ScaleKind::Lydian,
        "mixolydian" => ScaleKind::Mixolydian,
        "locrian" => ScaleKind::Locrian,
        "major_pentatonic" | "pentatonic" => ScaleKind::MajorPentatonic,
        "minor_pentatonic" => ScaleKind::MinorPentatonic,
        "blues" => ScaleKind::Blues,
        "chromatic" => ScaleKind::Chromatic,
        other => return err(format!("unknown scale '{other}'")),
    };
    Ok(Key { root: pc, scale })
}

pub fn parse_color(s: &str) -> Result<[u8; 3], String> {
    let h = s.trim().trim_start_matches('#');
    if h.len() != 6 {
        return err("color must be #RRGGBB");
    }
    let v = u32::from_str_radix(h, 16).map_err(|_| "invalid hex color".to_string())?;
    Ok([(v >> 16) as u8, (v >> 8) as u8, v as u8])
}

pub fn new_plugin(project: &mut Project, kind: &str, preset: Option<&str>) -> Result<PluginInstance, String> {
    let info = plugins::find(kind).ok_or_else(|| format!("unknown plug-in kind '{kind}' (see list_plugins)"))?;
    let mut inst = PluginInstance { id: project.alloc_id(), kind: info.kind.to_string(), params: Default::default(), bypass: false, preset: None, sample: None, vst3_path: None, vst3_params: vec![], vst3_state: None };
    if let Some(name) = preset.filter(|s| !s.is_empty()) {
        apply_preset(&mut inst, name)?;
    }
    Ok(inst)
}

fn apply_preset(inst: &mut PluginInstance, name: &str) -> Result<(), String> {
    if inst.kind == "vst3" {
        return Err("factory presets are not available for VST3 plug-ins; set parameters instead".into());
    }
    let preset = plugins::presets_for(&inst.kind)
        .find(|p| p.name.eq_ignore_ascii_case(name))
        .ok_or_else(|| format!("no preset '{name}' for {}", inst.kind))?;
    inst.params.clear();
    for (k, v) in preset.params {
        inst.params.insert(k.to_string(), *v);
    }
    inst.preset = Some(preset.name.to_string());
    Ok(())
}

fn set_param(inst: &mut PluginInstance, param: &str, value: f32) -> Result<f32, String> {
    if !value.is_finite() {
        return Err(format!("value for '{param}' must be a finite number"));
    }
    if inst.kind == "vst3" {
        if !inst.vst3_params.iter().any(|p| p.id == param) {
            let some: Vec<String> = inst.vst3_params.iter().take(20).map(|p| format!("{} ({})", p.id, p.name)).collect();
            return Err(format!("VST3 plug-in has no parameter '{param}'. Examples: {}", some.join(", ")));
        }
        let v = value.clamp(0.0, 1.0);
        inst.params.insert(param.to_string(), v);
        return Ok(v);
    }
    let info = plugins::find(&inst.kind).ok_or("unknown plug-in")?;
    let p = info
        .params
        .iter()
        .find(|p| p.id == param)
        .ok_or_else(|| format!("{} has no parameter '{param}'. Valid: {}", inst.kind, info.params.iter().map(|p| p.id).collect::<Vec<_>>().join(", ")))?;
    let mut v = value.clamp(p.min, p.max);
    if !p.choices.is_empty() {
        v = v.round();
    }
    inst.params.insert(param.to_string(), v);
    Ok(v)
}

pub fn apply_patch(project: &mut Project, track_id: Id, patch_name: &str) -> Result<usize, String> {
    let patch = plugins::find_patch(patch_name).ok_or_else(|| format!("unknown patch '{patch_name}' (see list_patches)"))?;
    let instrument = match patch.instrument {
        Some((k, pr)) => Some(new_plugin(project, k, Some(pr))?),
        None => None,
    };
    let mut fx = vec![];
    for (k, pr) in patch.effects {
        fx.push(new_plugin(project, k, Some(pr))?);
    }
    let t = project.track_mut(track_id).ok_or("track not found")?;
    // the replaced plug-ins take their automation lanes with them, as remove_plugin does
    let mut replaced: Vec<Id> = t.channel.inserts.iter().map(|p| p.id).collect();
    let replaces_instrument = t.kind == TrackKind::Instrument && instrument.is_some();
    if replaces_instrument {
        replaced.extend(t.instrument.as_ref().map(|p| p.id));
        t.instrument = instrument;
    }
    t.channel.inserts = fx;
    Ok(remove_plugin_lanes(project, &replaced))
}

pub fn add_track(project: &mut Project, kind: TrackKind, name: Option<String>, patch: Option<&str>, instrument: Option<&str>, preset: Option<&str>, index: Option<usize>) -> Result<Id, String> {
    // infer the right track type from the patch / instrument so callers cannot create
    // an audio track carrying an instrument patch (or vice versa)
    let kind = match (patch.and_then(plugins::find_patch), instrument) {
        (Some(pt), _) if kind != TrackKind::Bus => if pt.instrument.is_some() { TrackKind::Instrument } else { TrackKind::Audio },
        (None, Some(_)) if kind != TrackKind::Bus => TrackKind::Instrument,
        _ => kind,
    };
    let id = project.alloc_id();
    let n = project.tracks.len();
    let default_name = match kind {
        TrackKind::Instrument => patch.map(|s| s.to_string()).or(preset.map(|s| s.to_string())).unwrap_or_else(|| format!("Inst {}", n + 1)),
        TrackKind::Audio => patch.map(|s| s.to_string()).unwrap_or_else(|| format!("Audio {}", n + 1)),
        TrackKind::Bus => format!("Bus {}", n + 1),
    };
    let track = Track {
        id,
        name: name.unwrap_or(default_name),
        kind,
        color: TRACK_COLORS[n % TRACK_COLORS.len()],
        // instrument tracks start at -6 dB to leave mix headroom when many parts are summed
        channel: Channel { volume_db: if kind == TrackKind::Instrument { -6.0 } else { 0.0 }, ..Channel::default() },
        instrument: None,
        midi_fx: vec![],
        regions: vec![],
        automation: vec![],
        record_armed: false,
        input_channel: 0,
        height: 0.0,
    };
    let idx = index.unwrap_or(n).min(n);
    project.tracks.insert(idx, track);
    if let Some(p) = patch {
        apply_patch(project, id, p)?;
    }
    if kind == TrackKind::Instrument && project.track(id).unwrap().instrument.is_none() {
        let kind = instrument.unwrap_or("analog_synth");
        let inst = new_plugin(project, kind, preset)?;
        if plugins::find(kind).map(|p| p.category) != Some(PluginCategory::Instrument) {
            return err(format!("'{kind}' is not an instrument"));
        }
        project.track_mut(id).unwrap().instrument = Some(inst);
    }
    Ok(id)
}

fn region_mut<'a>(p: &'a mut Project, id: Id) -> Result<&'a mut Region, String> {
    p.find_region_mut(id).ok_or_else(|| format!("region {id} not found"))
}

fn midi_mut<'a>(p: &'a mut Project, id: Id) -> Result<&'a mut MidiContent, String> {
    match &mut region_mut(p, id)?.content {
        RegionContent::Midi(m) => Ok(m),
        _ => err("region is not a MIDI region"),
    }
}

/// Replaces the events of `controller` with from <= beat < to by `events` and keeps `cc` sorted by beat (stable:
/// a kept event and a new one at the same beat stay in that order). Returns (removed, added). No checks: callers
/// validate first. Shared by `replace_cc` and the piano roll's preview, so both give the same result.
pub fn replace_cc_events(cc: &mut Vec<CcEvent>, controller: u8, from: f64, to: f64, events: &[CcPoint]) -> (usize, usize) {
    let before = cc.len();
    cc.retain(|e| !(e.controller == controller && from <= e.beat && e.beat < to));
    let removed = before - cc.len();
    cc.extend(events.iter().map(|p| CcEvent { beat: p.beat, controller, value: p.value }));
    cc.sort_by(|a, b| a.beat.total_cmp(&b.beat));
    (removed, events.len())
}

/// Checks the arguments of `replace_cc` and returns the range, open ends as infinities.
fn check_replace_cc(controller: u8, from: Option<f64>, to: Option<f64>, events: &[CcPoint]) -> Result<(f64, f64), String> {
    if controller > 128 {
        return err("controller must be 0..127, or 128 for pitch bend");
    }
    if from.is_some_and(|f| !f.is_finite()) || to.is_some_and(|t| !t.is_finite()) {
        return err("from_beat and to_beat must be finite numbers");
    }
    if let (Some(f), Some(t)) = (from, to) {
        if f >= t {
            return err("from_beat must be before to_beat");
        }
    }
    for p in events {
        if !p.beat.is_finite() || p.beat < 0.0 {
            return Err(format!("event beat {} must be 0 or more", p.beat));
        }
        if p.value > 127 {
            return Err(format!("event value {} at beat {} is out of range 0..127", p.value, p.beat));
        }
    }
    Ok((from.unwrap_or(f64::NEG_INFINITY), to.unwrap_or(f64::INFINITY)))
}

fn pattern_mut<'a>(p: &'a mut Project, id: Id) -> Result<&'a mut PatternContent, String> {
    match &mut region_mut(p, id)?.content {
        RegionContent::Pattern(m) => Ok(m),
        _ => err("region is not a pattern region"),
    }
}

/// The error for a row index the pattern does not have.
fn row_not_found(row: usize, rows: usize) -> String {
    format!("row {row} not found: the pattern has {rows} rows")
}

fn clamp_note(n: &mut Note) {
    n.pitch = n.pitch.min(127);
    n.velocity = n.velocity.clamp(1, 127);
    n.start = n.start.max(0.0);
    n.length = n.length.max(0.01);
}

/// Share of the original velocity a harmony copy gets: a second voice sits a little behind the line it doubles.
const HARMONY_VELOCITY: f32 = 0.85;

fn note_order(a: &Note, b: &Note) -> std::cmp::Ordering {
    a.start.partial_cmp(&b.start).unwrap().then(a.pitch.cmp(&b.pitch))
}

pub(crate) fn sanitize_notes(notes: &mut Vec<Note>) {
    notes.iter_mut().for_each(clamp_note);
    notes.sort_by(note_order);
}

/// Same as `sanitize_notes`, and also reports where the notes flagged in `mask` are after sorting,
/// so a caller that addresses notes by index can follow them.
fn sanitize_notes_tracked(notes: &mut Vec<Note>, mask: &[bool]) -> Vec<usize> {
    notes.iter_mut().for_each(clamp_note);
    let mut order: Vec<usize> = (0..notes.len()).collect();
    order.sort_by(|&a, &b| note_order(&notes[a], &notes[b]));
    let sorted: Vec<Note> = order.iter().map(|&i| notes[i]).collect();
    *notes = sorted;
    order.iter().enumerate().filter(|&(_, &old)| mask[old]).map(|(new, _)| new).collect()
}

/// Which notes a command acts on: all of them without `indices`, otherwise the listed ones.
/// Indices past the end are ignored, as `TransposeNotes` does.
fn note_mask(len: usize, indices: Option<&[usize]>) -> Vec<bool> {
    match indices {
        None => vec![true; len],
        Some(ix) => {
            let mut m = vec![false; len];
            for &i in ix {
                if i < len {
                    m[i] = true;
                }
            }
            m
        }
    }
}

/// Run `f` on the masked notes only, in their current order, and write them back in place.
/// Humanize's random sequence then depends only on the chosen notes, so a seed is reproducible
/// for the same selection.
fn on_masked(notes: &mut [Note], mask: &[bool], f: impl FnOnce(&mut [Note])) {
    let mut sub: Vec<Note> = notes.iter().zip(mask).filter(|(_, s)| **s).map(|(n, _)| *n).collect();
    f(&mut sub);
    for (n, v) in notes.iter_mut().zip(mask).filter(|(_, s)| **s).map(|(n, _)| n).zip(sub) {
        *n = v;
    }
}

/// The `indices` field is added only when the caller sent indices, so results without it stay as they were.
fn with_indices(mut v: Value, asked: bool, indices: Vec<usize>) -> Value {
    if asked {
        v["indices"] = json!(indices);
    }
    v
}

/// The shortest length `set_note_lengths` gives a note: 1/64 of a beat.
const MIN_NOTE_LENGTH: f64 = 1.0 / 64.0;
/// For legato, chosen notes that start less than this (beats) after the first note of a group are one chord:
/// a chord played live is never exactly together.
const LEGATO_CHORD_WINDOW: f64 = 1.0 / 32.0;

/// The length a note gets when `set_note_lengths` asks for `wanted`: never under the minimum, and a note that grows
/// stops at the end of the region (the renderer cuts it there anyway); a note already past the end keeps its length.
fn fit_length(old: f64, wanted: f64, start: f64, region_len: f64) -> f64 {
    let l = wanted.max(MIN_NOTE_LENGTH);
    if l <= old {
        return l;
    }
    l.min((region_len - start).max(old))
}

/// The new length of every note for `set_note_lengths`; notes outside `mask` keep theirs. `notes` is sorted by
/// start, as a region keeps it; `value` is the length of "fixed" or the factor of "scale".
fn new_note_lengths(notes: &[Note], mask: &[bool], mode: NoteLengthMode, value: f64, region_len: f64) -> Vec<f64> {
    let mut out: Vec<f64> = notes.iter().map(|n| n.length).collect();
    let chosen: Vec<usize> = (0..notes.len()).filter(|&i| mask[i]).collect();
    let fit = |i: usize, wanted: f64| fit_length(notes[i].length, wanted, notes[i].start, region_len);
    match mode {
        NoteLengthMode::Legato => {
            // (start of the group's first note, notes of the group)
            let mut groups: Vec<(f64, Vec<usize>)> = Vec::new();
            for &i in &chosen {
                match groups.last_mut() {
                    Some((first, g)) if notes[i].start - *first < LEGATO_CHORD_WINDOW => g.push(i),
                    _ => groups.push((notes[i].start, vec![i])),
                }
            }
            for w in groups.windows(2) {
                for &i in &w[0].1 {
                    out[i] = fit(i, w[1].0 - notes[i].start);
                }
            }
        }
        NoteLengthMode::NoOverlap => {
            for &i in &chosen {
                let n = notes[i];
                // Sorted by start: the first later note of the same pitch is the one that cuts.
                if let Some(m) = notes[i + 1..].iter().find(|m| m.pitch == n.pitch && m.start > n.start + 1e-9) {
                    if n.start + n.length > m.start {
                        out[i] = fit(i, m.start - n.start);
                    }
                }
            }
        }
        NoteLengthMode::Fixed => chosen.iter().for_each(|&i| out[i] = fit(i, value)),
        NoteLengthMode::Scale => chosen.iter().for_each(|&i| out[i] = fit(i, notes[i].length * value)),
    }
    out
}

pub fn default_drum_rows(steps: u32) -> Vec<PatternRow> {
    [("Kick", 36u8), ("Snare", 38), ("Clap", 39), ("Closed Hat", 42), ("Open Hat", 46), ("Low Tom", 45), ("Hi Tom", 50), ("Crash", 49)]
        .iter()
        .map(|(n, p)| PatternRow { name: n.to_string(), pitch: *p, steps: vec![0; steps as usize], muted: false })
        .collect()
}

pub fn pattern_to_notes(p: &PatternContent) -> Vec<Note> {
    let mut out = vec![];
    for row in &p.rows {
        if row.muted {
            continue;
        }
        for (i, v) in row.steps.iter().enumerate() {
            if *v == 0 {
                continue;
            }
            let mut start = i as f64 * p.step_length;
            if i % 2 == 1 {
                start += p.swing as f64 * p.step_length * 0.5;
            }
            out.push(Note { pitch: row.pitch, velocity: *v, start, length: p.step_length * 0.9 });
        }
    }
    out
}

fn parse_step_string(s: &str, steps: usize) -> Vec<u8> {
    let mut v: Vec<u8> = s
        .chars()
        .filter(|c| !c.is_whitespace() && *c != '|')
        .map(|c| match c {
            'x' | 'X' => 115,
            'o' | 'O' => 90,
            'g' | 'G' => 45,
            _ => 0,
        })
        .collect();
    v.resize(steps, 0);
    v
}

/// Resolve chords for generators: chord symbols or roman numerals, from `chords` or the chord track (when `chords`
/// is None). Roman numerals on the chord track are read in the current project key.
pub fn resolve_chords(project: &Project, start: f64, bars: u32, chords: &Option<Vec<String>>, beats_per_chord: Option<f64>) -> Result<Vec<TimedChord>, String> {
    check_generated_bars(bars)?;
    if let Some(per) = beats_per_chord {
        if !per.is_finite() || per < MIN_BEATS_PER_CHORD {
            return Err(format!("beats_per_chord must be at least {MIN_BEATS_PER_CHORD} beats"));
        }
    }
    let bpb = project.time_signature.beats_per_bar();
    let total = bars as f64 * bpb;
    if let Some(list) = chords {
        if list.is_empty() {
            return err("chords list is empty");
        }
        let per = beats_per_chord.unwrap_or(bpb);
        let mut out = vec![];
        let mut t = 0.0;
        let mut i = 0;
        while t < total - 1e-9 {
            let sym = &list[i % list.len()];
            let ch = parse_chord_or_roman(sym, project.key)?;
            out.push(TimedChord { start: t, length: per.min(total - t), chord: ch });
            t += per;
            i += 1;
        }
        return Ok(out);
    }
    let end = start + total;
    let mut out: Vec<TimedChord> = project
        .chords
        .iter()
        .filter(|c| c.beat < end && c.beat + c.length > start)
        .filter_map(|c| {
            let s = c.beat.max(start);
            let e = (c.beat + c.length).min(end);
            parse_chord_or_roman(&c.symbol, project.key).ok().map(|ch| TimedChord { start: s - start, length: e - s, chord: ch })
        })
        .collect();
    if out.is_empty() {
        // fall back to the tonic chord of the project key
        let tonic = theory::NOTE_NAMES[project.key.root as usize];
        let minor = matches!(project.key.scale, ScaleKind::Minor | ScaleKind::Dorian | ScaleKind::Phrygian | ScaleKind::HarmonicMinor | ScaleKind::MelodicMinor | ScaleKind::MinorPentatonic | ScaleKind::Blues);
        let sym = format!("{tonic}{}", if minor { "m" } else { "" });
        out.push(TimedChord { start: 0.0, length: total, chord: Chord::parse(&sym).unwrap() });
    }
    out.sort_by(|a, b| a.start.partial_cmp(&b.start).unwrap());
    Ok(out)
}

pub fn parse_chord_or_roman(sym: &str, key: Key) -> Result<Chord, String> {
    if let Some(c) = Chord::parse(sym) {
        return Ok(c);
    }
    if let Some(s) = theory::roman_to_symbol(sym, key) {
        if let Some(c) = Chord::parse(&s) {
            return Ok(c);
        }
    }
    err(format!("cannot parse chord '{sym}'"))
}

/// The symbol the chord track stores for `sym`: a chord symbol as typed (trimmed), or a roman numeral turned
/// into the chord of that degree in `key`. Stored as a symbol, a chord keeps its sound when the key changes.
pub fn resolve_chord_symbol(sym: &str, key: Key) -> Result<String, String> {
    let s = sym.trim();
    if Chord::parse(s).is_some() {
        return Ok(s.to_string());
    }
    if let Some(r) = theory::roman_to_symbol(s, key) {
        if Chord::parse(&r).is_some() {
            return Ok(r);
        }
    }
    err(format!("cannot parse chord '{s}'"))
}

/// Most bars one generator call writes.
pub const MAX_GENERATED_BARS: u32 = 512;
/// Shortest chord a generator follows (a sixteenth note): shorter ones would only multiply the work.
pub const MIN_BEATS_PER_CHORD: f64 = 0.25;

fn check_generated_bars(bars: u32) -> Result<(), String> {
    if bars > MAX_GENERATED_BARS {
        return Err(format!("bars must be at most {MAX_GENERATED_BARS} (got {bars}): generate a long part in pieces"));
    }
    Ok(())
}

fn put_generated_region(project: &mut Project, track_id: Id, start: f64, bars: u32, name: &str, notes: Vec<Note>) -> Result<Id, String> {
    let bpb = project.time_signature.beats_per_bar();
    let id = project.alloc_id();
    let t = project.track_mut(track_id).ok_or("track not found")?;
    if t.kind != TrackKind::Instrument {
        return err("generators need an instrument track");
    }
    let mut notes = notes;
    sanitize_notes(&mut notes);
    t.regions.push(Region { take: None,
        id,
        name: name.to_string(),
        start,
        length: bars as f64 * bpb,
        loop_count: 1,
        muted: false,
        color: None,
        content: RegionContent::Midi(MidiContent { notes, ..Default::default() }),
    });
    Ok(id)
}

/// A seed drawn when the caller gives none stays below 1,000,000: short enough to read in the status bar, and exact
/// in any JSON client (JavaScript numbers lose integers above 2^53), so the seed a generator returns repeats the part.
fn seed_or_random(seed: Option<u64>) -> u64 {
    seed.unwrap_or_else(|| rand::random::<u64>() % 1_000_000)
}

/// What every generator answers: the new region, and the seed, start and bars that rebuild the same part.
fn generated(region_id: Id, seed: u64, start: f64, bars: u32) -> Value {
    json!({"region_id": region_id, "seed": seed, "start": start, "bars": bars})
}

/// Shift a region list for section operations.
fn shift_after(project: &mut Project, at: f64, delta: f64) {
    for t in &mut project.tracks {
        for r in &mut t.regions {
            if r.start >= at - 1e-9 {
                r.start += delta;
            }
        }
        for lane in &mut t.automation {
            for p in &mut lane.points {
                if p.beat >= at - 1e-9 {
                    p.beat += delta;
                }
            }
        }
    }
    for m in &mut project.markers {
        if m.beat >= at - 1e-9 {
            m.beat += delta;
        }
    }
    for c in &mut project.chords {
        if c.beat >= at - 1e-9 {
            c.beat += delta;
        }
    }
}

/// Split all regions crossing `at` so section operations act on clean boundaries.
fn split_all_at(project: &mut Project, at: f64) {
    let ids: Vec<Id> = project
        .tracks
        .iter()
        .flat_map(|t| t.regions.iter().filter(|r| r.start < at - 1e-9 && r.end() > at + 1e-9).map(|r| r.id))
        .collect();
    for id in ids {
        let _ = split_region(project, id, at);
    }
}

pub fn split_region(project: &mut Project, region_id: Id, beat: f64) -> Result<Id, String> {
    let new_id = project.alloc_id();
    let tempo = project.tempo.clone();
    let t = project.tracks.iter_mut().find(|t| t.regions.iter().any(|r| r.id == region_id)).ok_or("region not found")?;
    let idx = t.regions.iter().position(|r| r.id == region_id).unwrap();
    let r = &mut t.regions[idx];
    if beat <= r.start + 1e-9 || beat >= r.end() - 1e-9 {
        return err("split position must be inside the region");
    }
    // unroll loops first
    if r.loop_count > 1 {
        if let RegionContent::Midi(m) = &mut r.content {
            let mut all = vec![];
            for i in 0..r.loop_count {
                for n in &m.notes {
                    if n.start < r.length {
                        let mut c = *n;
                        c.start += i as f64 * r.length;
                        all.push(c);
                    }
                }
            }
            m.notes = all;
        }
        r.length *= r.loop_count as f64;
        r.loop_count = 1;
    }
    let rel = beat - r.start;
    let mut right = r.clone();
    right.id = new_id;
    right.start = beat;
    right.length = r.length - rel;
    r.length = rel;
    match (&mut r.content, &mut right.content) {
        (RegionContent::Midi(l), RegionContent::Midi(rr)) => {
            let notes = std::mem::take(&mut l.notes);
            for n in notes {
                if n.start < rel {
                    let mut c = n;
                    c.length = c.length.min(rel - c.start);
                    l.notes.push(c);
                } else {
                    let mut c = n;
                    c.start -= rel;
                    rr.notes.push(c);
                }
            }
            rr.notes.retain(|n| n.start >= 0.0);
            let cc = std::mem::take(&mut l.cc);
            l.cc = cc.iter().filter(|c| c.beat < rel).copied().collect();
            rr.cc = cc.iter().filter(|c| c.beat >= rel).map(|c| CcEvent { beat: c.beat - rel, ..*c }).collect();
        }
        (RegionContent::Audio(_), RegionContent::Audio(ra)) => {
            let secs = tempo.beat_to_sec(beat) - tempo.beat_to_sec(beat - rel);
            ra.offset_sec += secs;
            ra.fade_in = 0.0;
        }
        (RegionContent::Pattern(_), RegionContent::Pattern(_)) => {}
        _ => {}
    }
    if let RegionContent::Audio(la) = &mut r.content {
        la.fade_out = 0.0;
    }
    t.regions.insert(idx + 1, right);
    Ok(new_id)
}

fn region_summary(r: &Region, project: &Project) -> Value {
    let (kind, extra) = match &r.content {
        RegionContent::Midi(m) => ("midi", json!({"note_count": m.notes.len(), "transpose": m.transpose})),
        RegionContent::Audio(a) => ("audio", json!({"file": project.audio_file(a.file_id).map(|f| f.name.clone()), "gain_db": a.gain_db})),
        RegionContent::Pattern(p) => ("pattern", json!({"steps": p.steps, "rows": p.rows.len()})),
    };
    let mut v = json!({
        "id": r.id, "name": r.name, "type": kind, "start": r.start, "length": r.length,
        "loop_count": r.loop_count, "end": r.end(), "muted": r.muted,
        "position": project.bar_beat_string(r.start), "info": extra
    });
    // only when set: the local model truncates long results
    if let Some(c) = r.color {
        v["color"] = json!(format!("#{:02X}{:02X}{:02X}", c[0], c[1], c[2]));
    }
    v
}

fn plugin_brief(p: &PluginInstance) -> Value {
    json!({"id": p.id, "kind": p.kind, "preset": p.preset, "bypass": p.bypass})
}

pub fn plugin_detail(p: &PluginInstance) -> Value {
    if p.kind == "vst3" {
        let params: Vec<Value> = p.vst3_params.iter().map(|vp| json!({"id": vp.id, "name": vp.name, "value": p.param(&vp.id), "min": 0.0, "max": 1.0, "unit": vp.unit, "normalized": true})).collect();
        return json!({"id": p.id, "kind": "vst3", "name": p.preset, "path": p.vst3_path, "bypass": p.bypass, "params": params});
    }
    let info = plugins::find(&p.kind);
    let params: Vec<Value> = info
        .map(|i| {
            i.params
                .iter()
                .map(|pi| {
                    let v = p.param(pi.id);
                    let mut o = json!({"id": pi.id, "name": pi.name, "value": v, "min": pi.min, "max": pi.max, "unit": pi.unit});
                    if !pi.choices.is_empty() {
                        o["choices"] = json!(pi.choices);
                        o["selected"] = json!(pi.choices.get(v.round() as usize));
                    }
                    o
                })
                .collect()
        })
        .unwrap_or_default();
    json!({"id": p.id, "kind": p.kind, "name": info.map(|i| i.name), "preset": p.preset, "bypass": p.bypass, "sample_file_id": p.sample, "params": params})
}

fn channel_brief(c: &Channel) -> Value {
    json!({
        "volume_db": c.volume_db, "pan": c.pan, "mute": c.mute, "solo": c.solo,
        "output": match c.output { Output::Master => json!("master"), Output::Bus(b) => json!({"bus": b}) },
        "sends": c.sends.iter().enumerate().map(|(i, s)| json!({"index": i, "bus_id": s.bus_id, "level_db": s.level_db, "pre_fader": s.pre_fader})).collect::<Vec<_>>(),
        "inserts": c.inserts.iter().map(plugin_brief).collect::<Vec<_>>(),
    })
}

/// Indices that exist in `notes`, sorted and without repeats.
fn valid_note_indices(notes: &[Note], indices: &[usize]) -> Vec<usize> {
    let mut ix: Vec<usize> = indices.iter().copied().filter(|&i| i < notes.len()).collect();
    ix.sort_unstable();
    ix.dedup();
    ix
}

/// Most notes `get_selection` lists; `selected_note_count` still counts every selected note.
pub const SELECTION_NOTE_LIMIT: usize = 256;

/// The user's selection as `get_selection` reports it, without the playhead (that lives in the engine).
/// Ids that no longer exist report null, so a selection the GUI has not refreshed yet never fails.
pub fn selection_summary(project: &Project, track: Option<Id>, region: Option<Id>, note_indices: &[usize], limit: usize) -> Value {
    let track_v = track
        .and_then(|id| project.track(id))
        .map(|t| json!({"id": t.id, "name": t.name, "kind": t.kind}))
        .unwrap_or(Value::Null);
    let mut notes: Vec<Value> = Vec::new();
    let mut count = 0;
    let region_v = match region.and_then(|id| project.find_region(id)) {
        Some((t, r)) => {
            if let RegionContent::Midi(m) = &r.content {
                let ix = valid_note_indices(&m.notes, note_indices);
                count = ix.len();
                notes = ix
                    .iter()
                    .take(limit)
                    .map(|&i| {
                        let n = &m.notes[i];
                        json!({"index": i, "pitch": n.pitch, "name": theory::note_name(n.pitch), "velocity": n.velocity, "start": n.start, "length": n.length})
                    })
                    .collect();
            }
            let mut v = region_summary(r, project);
            v["track_id"] = json!(t.id);
            v
        }
        None => Value::Null,
    };
    json!({
        "cycle": {"enabled": project.cycle.enabled, "start": project.cycle.start, "end": project.cycle.end},
        "track": track_v,
        "region": region_v,
        "notes": notes,
        "selected_note_count": count,
    })
}

/// The selected notes as one sentence for the assistant's context, so the model knows what the
/// indices point at without calling get_region first. Indices past the end (a stale selection) are skipped.
pub fn selected_notes_context(notes: &[Note], indices: &[usize], limit: usize) -> String {
    let ix = valid_note_indices(notes, indices);
    if ix.is_empty() {
        return String::new();
    }
    let shown: Vec<String> = ix
        .iter()
        .take(limit)
        .map(|&i| {
            let n = &notes[i];
            format!("{i}:{}@{}+{} v{}", theory::note_name(n.pitch), beat_text(n.start), beat_text(n.length), n.velocity)
        })
        .collect();
    let mut s = format!(
        " Selected notes ({} of {}; index:pitch@start+length velocity, beats from the region start): {}",
        ix.len(),
        notes.len(),
        shown.join(", ")
    );
    if ix.len() > limit {
        s += &format!(", +{} more", ix.len() - limit);
    }
    s.push('.');
    s
}

/// Beats with at most three decimals and no trailing zeros: 1 instead of 1.000, 2.333 instead of 2.3333333.
fn beat_text(b: f64) -> String {
    let s = format!("{b:.3}");
    s.trim_end_matches('0').trim_end_matches('.').to_string()
}

pub fn project_summary(p: &Project) -> Value {
    let tracks: Vec<Value> = p
        .tracks
        .iter()
        .enumerate()
        .map(|(i, t)| {
            json!({
                "id": t.id, "index": i, "name": t.name, "kind": t.kind,
                "instrument": t.instrument.as_ref().map(plugin_brief),
                "midi_fx": t.midi_fx.iter().map(plugin_brief).collect::<Vec<_>>(),
                "channel": channel_brief(&t.channel),
                "record_armed": t.record_armed,
                "regions": t.regions.iter().map(|r| region_summary(r, p)).collect::<Vec<_>>(),
                "automation": t.automation.iter().map(|a| json!({"target": a.target, "points": a.points.len()})).collect::<Vec<_>>(),
            })
        })
        .collect();
    json!({
        "name": p.name,
        "tempo_bpm": p.tempo.initial_bpm(),
        "tempo_changes": p.tempo.points.iter().skip(1).collect::<Vec<_>>(),
        "time_signature": format!("{}/{}", p.time_signature.numerator, p.time_signature.denominator),
        "beats_per_bar": p.time_signature.beats_per_bar(),
        "key": p.key.name(),
        "length_beats": p.content_end(),
        "length_bars": (p.content_end() / p.time_signature.beats_per_bar()).ceil(),
        "master": { "id": MASTER_ID, "channel": channel_brief(&p.master) },
        "tracks": tracks,
        "markers": p.markers,
        "chords": p.chords,
        "cycle": {"enabled": p.cycle.enabled, "start": p.cycle.start, "end": p.cycle.end},
        "metronome": p.metronome,
        "metronome_level_db": (p.metronome_level_db * 100.0).round() / 100.0,
        "metronome_only_while_recording": p.metronome_only_recording,
        "audio_files": p.audio_files.iter().map(|f| json!({"id": f.id, "name": f.name, "duration_sec": f.duration_sec})).collect::<Vec<_>>(),
        "notes": p.notes,
    })
}

pub fn track_detail(p: &Project, t: &Track) -> Value {
    json!({
        "id": t.id, "name": t.name, "kind": t.kind,
        "color": format!("#{:02X}{:02X}{:02X}", t.color[0], t.color[1], t.color[2]),
        "instrument": t.instrument.as_ref().map(plugin_detail),
        "midi_fx": t.midi_fx.iter().map(plugin_detail).collect::<Vec<_>>(),
        "channel": {
            "volume_db": t.channel.volume_db, "pan": t.channel.pan, "mute": t.channel.mute, "solo": t.channel.solo,
            "output": match t.channel.output { Output::Master => json!("master"), Output::Bus(b) => json!({"bus": b}) },
            "sends": t.channel.sends.iter().enumerate().map(|(i, s)| json!({"index": i, "bus_id": s.bus_id, "level_db": s.level_db})).collect::<Vec<_>>(),
            "inserts": t.channel.inserts.iter().map(plugin_detail).collect::<Vec<_>>(),
        },
        "regions": t.regions.iter().map(|r| region_summary(r, p)).collect::<Vec<_>>(),
        "automation": t.automation.iter().map(|a| json!({"target": a.target, "points": a.points})).collect::<Vec<_>>(),
    })
}

fn all_regions_in(project: &Project, from: f64, to: f64) -> Vec<(Id, Region)> {
    project
        .tracks
        .iter()
        .flat_map(|t| t.regions.iter().filter(|r| r.start >= from - 1e-9 && r.start < to - 1e-9).map(move |r| (t.id, r.clone())))
        .collect()
}

/// Execute a model-level command.
pub fn apply(project: &mut Project, cmd: &Command) -> R {
    use Command::*;
    match cmd {
        GetProject {} => ok(project_summary(project)),
        GetProjectJson {} => ok(serde_json::to_value(&*project).map_err(|e| e.to_string())?),
        NewProject { name, tempo, time_signature, key, .. } => {
            let mut p = Project::new(name.as_deref().unwrap_or("Untitled"));
            if let Some(t) = tempo {
                p.tempo.set_initial_bpm(*t);
            }
            if let Some(ts) = time_signature {
                let (a, b) = ts.split_once('/').ok_or("time signature like 4/4")?;
                p.time_signature = TimeSignature { numerator: a.trim().parse().map_err(|_| "bad numerator")?, denominator: b.trim().parse().map_err(|_| "bad denominator")? };
                if !p.time_signature.is_valid() {
                    return err(format!("invalid time signature {ts}: at least 1 beat per bar, denominator 1, 2, 4, 8 or 16"));
                }
            }
            if let Some(k) = key {
                p.key = parse_key(k)?;
            }
            *project = p;
            ok(json!({"ok": true}))
        }
        SetProjectName { name } => {
            project.name = name.clone();
            ok(json!({"ok": true}))
        }
        SetProjectNotes { notes } => {
            project.notes = notes.clone();
            ok(json!({"ok": true}))
        }
        SetTempo { bpm } => {
            project.tempo.set_initial_bpm(*bpm);
            ok(json!({"tempo": project.tempo.initial_bpm()}))
        }
        AddTempoChange { beat, bpm } => {
            project.tempo.add_change(*beat, *bpm);
            ok(json!({"tempo_points": project.tempo.points}))
        }
        ClearTempoChanges {} => {
            project.tempo.clear_changes();
            ok(json!({"ok": true}))
        }
        SetTimeSignature { numerator, denominator } => {
            let ts = TimeSignature { numerator: *numerator, denominator: *denominator };
            if !ts.is_valid() {
                return err("invalid time signature");
            }
            project.time_signature = ts;
            ok(json!({"ok": true}))
        }
        SetKey { key } => {
            project.key = parse_key(key)?;
            ok(json!({"key": project.key.name()}))
        }
        SetCycle { enabled, start, end } => {
            project.cycle.enabled = *enabled;
            if let Some(s) = start {
                project.cycle.start = s.max(0.0);
            }
            if let Some(e) = end {
                project.cycle.end = *e;
            }
            if project.cycle.end <= project.cycle.start {
                project.cycle.end = project.cycle.start + project.time_signature.beats_per_bar();
            }
            ok(json!({"cycle": {"enabled": project.cycle.enabled, "start": project.cycle.start, "end": project.cycle.end}}))
        }
        SetMetronome { enabled, level_db, only_while_recording } => {
            if let Some(db) = level_db {
                if !db.is_finite() {
                    return err("level_db must be a number of dB between -60 and 0");
                }
            }
            project.metronome = *enabled;
            if let Some(db) = level_db {
                project.metronome_level_db = db.clamp(METRONOME_MIN_DB, METRONOME_MAX_DB);
            }
            if let Some(o) = only_while_recording {
                project.metronome_only_recording = *o;
            }
            ok(json!({
                "ok": true,
                "metronome": project.metronome,
                "level_db": project.metronome_level_db,
                "only_while_recording": project.metronome_only_recording,
            }))
        }
        SetCountIn { bars } => {
            project.count_in_bars = (*bars).min(4);
            ok(json!({"count_in_bars": project.count_in_bars}))
        }

        // tracks
        AddTrack { kind, name, patch, instrument, preset, index } => {
            let id = add_track(project, *kind, name.clone(), patch.as_deref(), instrument.as_deref(), preset.as_deref(), *index)?;
            ok(json!({"track_id": id, "track": track_detail(project, project.track(id).unwrap())}))
        }
        RemoveTrack { track_id } => {
            let idx = project.track_index(*track_id).ok_or("track not found")?;
            let removed = project.tracks.remove(idx);
            let pruned = prune_references(project, &removed);
            ok(json!({
                "ok": true,
                "removed_automation_lanes": pruned.automation_lanes,
                "removed_live_cells": pruned.live_cells,
                "removed_midi_routes": pruned.midi_routes,
            }))
        }
        DuplicateTrack { track_id } => {
            let idx = project.track_index(*track_id).ok_or("track not found")?;
            let mut t = project.tracks[idx].clone();
            t.id = project.alloc_id();
            t.name = format!("{} copy", t.name);
            // the copy keeps its automation; lanes of its own plug-ins follow the new plug-in ids
            let mut new_ids: Vec<(Id, Id)> = vec![];
            for p in t.instrument.iter_mut().chain(t.midi_fx.iter_mut()).chain(t.channel.inserts.iter_mut()) {
                let new = project.alloc_id();
                new_ids.push((p.id, new));
                p.id = new;
            }
            for r in t.regions.iter_mut() {
                r.id = project.alloc_id();
            }
            remap_plugin_lanes(&mut t.automation, &new_ids);
            let id = t.id;
            project.tracks.insert(idx + 1, t);
            ok(json!({"track_id": id}))
        }
        RenameTrack { track_id, name } => {
            project.track_mut(*track_id).ok_or("track not found")?.name = name.clone();
            ok(json!({"ok": true}))
        }
        SetTrackColor { track_id, color } => {
            let c = parse_color(color)?;
            project.track_mut(*track_id).ok_or("track not found")?.color = c;
            ok(json!({"ok": true}))
        }
        MoveTrack { track_id, index } => {
            let idx = project.track_index(*track_id).ok_or("track not found")?;
            let t = project.tracks.remove(idx);
            let i = (*index).min(project.tracks.len());
            project.tracks.insert(i, t);
            ok(json!({"ok": true}))
        }
        SetVolume { track_id, db } => {
            let c = project.channel_mut(*track_id).ok_or("track not found")?;
            c.volume_db = db.clamp(-96.0, 6.0);
            ok(json!({"volume_db": c.volume_db}))
        }
        SetPan { track_id, pan } => {
            let c = project.channel_mut(*track_id).ok_or("track not found")?;
            c.pan = pan.clamp(-1.0, 1.0);
            ok(json!({"pan": c.pan}))
        }
        SetMute { track_id, mute } => {
            project.channel_mut(*track_id).ok_or("track not found")?.mute = *mute;
            ok(json!({"ok": true}))
        }
        SetSolo { track_id, solo } => {
            project.channel_mut(*track_id).ok_or("track not found")?.solo = *solo;
            ok(json!({"ok": true}))
        }
        SetRecordArm { track_id, armed } => {
            project.track_mut(*track_id).ok_or("track not found")?.record_armed = *armed;
            ok(json!({"ok": true}))
        }
        SetTrackOutput { track_id, bus_id } => {
            if let Some(b) = bus_id {
                let bt = project.track(*b).ok_or("bus not found")?;
                if bt.kind != TrackKind::Bus {
                    return err("target is not a bus track");
                }
                if *b == *track_id {
                    return err("cannot route a bus into itself");
                }
            }
            let c = project.channel_mut(*track_id).ok_or("track not found")?;
            c.output = bus_id.map(Output::Bus).unwrap_or(Output::Master);
            ok(json!({"ok": true}))
        }
        AddSend { track_id, bus_id, level_db, pre_fader } => {
            let bt = project.track(*bus_id).ok_or("bus not found")?;
            if bt.kind != TrackKind::Bus {
                return err("send target must be a bus track (add_track kind=bus)");
            }
            if bus_id == track_id {
                return err("cannot send a bus to itself");
            }
            let t = project.track_mut(*track_id).ok_or("track not found")?;
            t.channel.sends.push(Send { bus_id: *bus_id, level_db: level_db.unwrap_or(-10.0), pre_fader: pre_fader.unwrap_or(false) });
            ok(json!({"send_index": t.channel.sends.len() - 1}))
        }
        SetSendLevel { track_id, send_index, level_db } => {
            let t = project.track_mut(*track_id).ok_or("track not found")?;
            t.channel.sends.get_mut(*send_index).ok_or("send not found")?.level_db = level_db.clamp(-96.0, 6.0);
            ok(json!({"ok": true}))
        }
        RemoveSend { track_id, send_index } => {
            let t = project.track_mut(*track_id).ok_or("track not found")?;
            if *send_index >= t.channel.sends.len() {
                return err("send not found");
            }
            let lanes = remove_sends(t, |i, _| i == *send_index);
            ok(json!({"ok": true, "removed_automation_lanes": lanes}))
        }
        GetTrack { track_id } => {
            if *track_id == MASTER_ID {
                return ok(json!({"id": 0, "name": "Master", "volume_db": project.master.volume_db, "inserts": project.master.inserts.iter().map(plugin_detail).collect::<Vec<_>>()}));
            }
            let t = project.track(*track_id).ok_or("track not found")?;
            ok(track_detail(project, t))
        }
        ApplyPatch { track_id, patch } => {
            let lanes = apply_patch(project, *track_id, patch)?;
            let mut v = track_detail(project, project.track(*track_id).unwrap());
            v["removed_automation_lanes"] = json!(lanes);
            ok(v)
        }

        // plug-ins
        ListPlugins { category } => {
            let list: Vec<Value> = plugins::registry()
                .iter()
                .filter(|p| match category.as_deref() {
                    Some("instrument") => p.category == PluginCategory::Instrument,
                    Some("effect") => p.category == PluginCategory::Effect,
                    Some("midi_effect") => p.category == PluginCategory::MidiEffect,
                    _ => true,
                })
                .map(|p| serde_json::to_value(p).unwrap())
                .collect();
            ok(json!(list))
        }
        ListPresets { kind } => {
            let list: Vec<Value> = plugins::PRESETS
                .iter()
                .filter(|p| kind.as_deref().map(|k| k == p.kind).unwrap_or(true))
                .map(|p| json!({"name": p.name, "kind": p.kind}))
                .collect();
            ok(json!(list))
        }
        ListPatches {} => {
            let list: Vec<Value> = plugins::PATCHES
                .iter()
                .map(|p| json!({"name": p.name, "category": p.category, "instrument": p.instrument.map(|i| i.0), "effects": p.effects.iter().map(|e| e.0).collect::<Vec<_>>()}))
                .collect();
            ok(json!(list))
        }
        SetInstrument { track_id, kind, preset } => {
            if plugins::find(kind).map(|p| p.category) != Some(PluginCategory::Instrument) {
                return err(format!("'{kind}' is not an instrument"));
            }
            let inst = new_plugin(project, kind, preset.as_deref())?;
            let id = inst.id;
            let t = project.track_mut(*track_id).ok_or("track not found")?;
            if t.kind != TrackKind::Instrument {
                return err("not an instrument track");
            }
            let old = t.instrument.replace(inst).map(|p| p.id);
            let lanes = remove_plugin_lanes(project, old.as_slice());
            ok(json!({"plugin_id": id, "removed_automation_lanes": lanes}))
        }
        AddEffect { track_id, kind, preset, index } => {
            if plugins::find(kind).map(|p| p.category) != Some(PluginCategory::Effect) {
                return err(format!("'{kind}' is not an audio effect"));
            }
            let fx = new_plugin(project, kind, preset.as_deref())?;
            let id = fx.id;
            let c = project.channel_mut(*track_id).ok_or("track not found")?;
            let i = index.unwrap_or(c.inserts.len()).min(c.inserts.len());
            c.inserts.insert(i, fx);
            ok(json!({"plugin_id": id, "slot": i}))
        }
        AddMidiEffect { track_id, kind, preset } => {
            if plugins::find(kind).map(|p| p.category) != Some(PluginCategory::MidiEffect) {
                return err(format!("'{kind}' is not a MIDI effect"));
            }
            let fx = new_plugin(project, kind, preset.as_deref())?;
            let id = fx.id;
            let t = project.track_mut(*track_id).ok_or("track not found")?;
            t.midi_fx.push(fx);
            ok(json!({"plugin_id": id}))
        }
        RemovePlugin { plugin_id } => {
            if !remove_plugin_instance(project, *plugin_id) {
                return err("plug-in not found");
            }
            let lanes = remove_plugin_lanes(project, &[*plugin_id]);
            ok(json!({"ok": true, "removed_automation_lanes": lanes}))
        }
        MovePlugin { plugin_id, index } => {
            let owner = project.plugin_owner(*plugin_id).ok_or("plug-in not found")?;
            let c = project.channel_mut(owner).unwrap();
            let pos = c.inserts.iter().position(|p| p.id == *plugin_id).ok_or("only insert effects can be moved")?;
            let p = c.inserts.remove(pos);
            let i = (*index).min(c.inserts.len());
            c.inserts.insert(i, p);
            ok(json!({"ok": true}))
        }
        SetPluginParam { plugin_id, param, value } => {
            let p = project.find_plugin_mut(*plugin_id).ok_or("plug-in not found")?;
            let v = set_param(p, param, *value)?;
            ok(json!({"param": param, "value": v}))
        }
        SetPluginParams { plugin_id, params } => {
            let p = project.find_plugin_mut(*plugin_id).ok_or("plug-in not found")?;
            let mut out = serde_json::Map::new();
            for (k, v) in params {
                out.insert(k.clone(), json!(set_param(p, k, *v)?));
            }
            ok(Value::Object(out))
        }
        LoadPreset { plugin_id, preset } => {
            let p = project.find_plugin_mut(*plugin_id).ok_or("plug-in not found")?;
            apply_preset(p, preset)?;
            ok(json!({"ok": true}))
        }
        SetPluginBypass { plugin_id, bypass } => {
            project.find_plugin_mut(*plugin_id).ok_or("plug-in not found")?.bypass = *bypass;
            ok(json!({"ok": true}))
        }
        GetPlugin { plugin_id } => {
            let p = project.find_plugin(*plugin_id).ok_or("plug-in not found")?;
            ok(plugin_detail(p))
        }

        // regions
        CreateMidiRegion { track_id, start, length, name, notes } => {
            let id = project.alloc_id();
            let t = project.track_mut(*track_id).ok_or("track not found")?;
            if t.kind != TrackKind::Instrument {
                return err("MIDI regions need an instrument track");
            }
            let mut notes = notes.clone().unwrap_or_default();
            sanitize_notes(&mut notes);
            let name = name.clone().unwrap_or_else(|| t.name.clone());
            t.regions.push(Region { take: None, id, name, start: start.max(0.0), length: length.max(0.0625), loop_count: 1, muted: false, color: None, content: RegionContent::Midi(MidiContent { notes, ..Default::default() }) });
            ok(json!({"region_id": id}))
        }
        CreatePatternRegion { track_id, start, steps, step_length, rows, name } => {
            let id = project.alloc_id();
            let steps = steps.unwrap_or(16).clamp(1, 128);
            let step_length = step_length.unwrap_or(0.25).max(0.03125);
            let t = project.track_mut(*track_id).ok_or("track not found")?;
            if t.kind != TrackKind::Instrument {
                return err("pattern regions need an instrument track");
            }
            let mut rows = rows.clone().unwrap_or_else(|| default_drum_rows(steps));
            for r in &mut rows {
                r.steps.resize(steps as usize, 0);
            }
            t.regions.push(Region { take: None,
                id,
                name: name.clone().unwrap_or_else(|| "Pattern".into()),
                start: start.max(0.0),
                length: steps as f64 * step_length,
                loop_count: 1,
                muted: false,
                color: None,
                content: RegionContent::Pattern(PatternContent { steps, step_length, rows, swing: 0.0 }),
            });
            ok(json!({"region_id": id}))
        }
        DeleteRegion { region_id } => {
            for t in &mut project.tracks {
                if let Some(i) = t.regions.iter().position(|r| r.id == *region_id) {
                    t.regions.remove(i);
                    return ok(json!({"ok": true}));
                }
            }
            err("region not found")
        }
        MoveRegion { region_id, start, track_id } => {
            let src_track = project.region_track_id(*region_id).ok_or("region not found")?;
            if let Some(dst) = track_id {
                if *dst != src_track {
                    let dst_kind = project.track(*dst).ok_or("destination track not found")?.kind;
                    let st = project.track_mut(src_track).unwrap();
                    let i = st.regions.iter().position(|r| r.id == *region_id).unwrap();
                    let is_audio = matches!(st.regions[i].content, RegionContent::Audio(_));
                    if (is_audio && dst_kind != TrackKind::Audio) || (!is_audio && dst_kind != TrackKind::Instrument) {
                        return err("region type does not match destination track type");
                    }
                    let r = st.regions.remove(i);
                    project.track_mut(*dst).unwrap().regions.push(r);
                }
            }
            if let Some(s) = start {
                region_mut(project, *region_id)?.start = s.max(0.0);
            }
            ok(json!({"ok": true}))
        }
        ResizeRegion { region_id, length } => {
            if !length.is_finite() {
                return err("length must be a finite number of beats");
            }
            let r = region_mut(project, *region_id)?;
            if let RegionContent::Pattern(p) = &r.content {
                let steps = (length.max(0.0625) / p.step_length).round();
                if steps > f64::from(MAX_PATTERN_STEPS) {
                    return Err(format!("a pattern can have at most {MAX_PATTERN_STEPS} steps: {length} beats would need {steps} steps of {} beats", p.step_length));
                }
            }
            r.length = length.max(0.0625);
            if let RegionContent::Pattern(p) = &mut r.content {
                let steps = (r.length / p.step_length).round().max(1.0) as u32;
                p.steps = steps;
                for row in &mut p.rows {
                    row.steps.resize(steps as usize, 0);
                }
                r.length = steps as f64 * p.step_length;
            }
            ok(json!({"length": r.length}))
        }
        SplitRegion { region_id, beat } => {
            let id = split_region(project, *region_id, *beat)?;
            ok(json!({"new_region_id": id}))
        }
        JoinRegions { region_ids } => {
            if region_ids.len() < 2 {
                return err("need at least two regions");
            }
            let tid = project.region_track_id(region_ids[0]).ok_or("region not found")?;
            let t = project.track_mut(tid).unwrap();
            let mut regs: Vec<Region> = vec![];
            for id in region_ids {
                let i = t.regions.iter().position(|r| r.id == *id).ok_or("all regions must be on the same track")?;
                if !matches!(t.regions[i].content, RegionContent::Midi(_)) {
                    return err("only MIDI regions can be joined");
                }
                regs.push(t.regions[i].clone());
            }
            regs.sort_by(|a, b| a.start.partial_cmp(&b.start).unwrap());
            let start = regs[0].start;
            let end = regs.iter().map(|r| r.end()).fold(0.0, f64::max);
            let mut notes = vec![];
            for r in &regs {
                if let RegionContent::Midi(m) = &r.content {
                    for k in 0..r.loop_count.max(1) {
                        for n in m.notes.iter().filter(|n| n.start < r.length) {
                            let mut c = *n;
                            c.start += r.start - start + k as f64 * r.length;
                            c.pitch = (c.pitch as i32 + m.transpose).clamp(0, 127) as u8;
                            notes.push(c);
                        }
                    }
                }
            }
            t.regions.retain(|r| !region_ids.contains(&r.id));
            let mut first = regs[0].clone();
            first.start = start;
            first.length = end - start;
            first.loop_count = 1;
            sanitize_notes(&mut notes);
            first.content = RegionContent::Midi(MidiContent { notes, ..Default::default() });
            let id = first.id;
            t.regions.push(first);
            ok(json!({"region_id": id}))
        }
        DuplicateRegion { region_id, start, count } => {
            let (tid, r) = project.find_region(*region_id).map(|(t, r)| (t.id, r.clone())).ok_or("region not found")?;
            let mut ids = vec![];
            let mut pos = start.unwrap_or(r.end());
            for _ in 0..count.unwrap_or(1).clamp(1, 256) {
                let mut c = r.clone();
                c.id = project.alloc_id();
                c.start = pos;
                pos += c.end() - c.start;
                ids.push(c.id);
                project.track_mut(tid).unwrap().regions.push(c);
            }
            ok(json!({"region_ids": ids}))
        }
        SetRegionProperties { region_id, name, muted, loop_count, transpose, velocity_offset, gain_db, fade_in, fade_out, reverse, color } => {
            // Outer None leaves the color alone; Some(None) removes it so the region follows its track.
            let col = match color.as_deref().map(str::trim) {
                None => None,
                Some(c) if c.is_empty() || c.eq_ignore_ascii_case("none") => Some(None),
                Some(c) => Some(Some(parse_color(c)?)),
            };
            let r = region_mut(project, *region_id)?;
            if let Some(v) = name {
                r.name = v.clone();
            }
            if let Some(v) = muted {
                r.muted = *v;
            }
            if let Some(v) = loop_count {
                r.loop_count = (*v).clamp(1, 1024);
            }
            if let Some(c) = col {
                r.color = c;
            }
            match &mut r.content {
                RegionContent::Midi(m) => {
                    if let Some(v) = transpose {
                        m.transpose = (*v).clamp(-48, 48);
                    }
                    if let Some(v) = velocity_offset {
                        m.velocity_offset = (*v).clamp(-127, 127);
                    }
                }
                RegionContent::Audio(a) => {
                    if let Some(v) = gain_db {
                        a.gain_db = v.clamp(-60.0, 24.0);
                    }
                    if let Some(v) = fade_in {
                        a.fade_in = v.max(0.0);
                    }
                    if let Some(v) = fade_out {
                        a.fade_out = v.max(0.0);
                    }
                    if let Some(v) = reverse {
                        a.reverse = *v;
                    }
                }
                _ => {}
            }
            ok(json!({"ok": true}))
        }
        GetRegion { region_id } => {
            let (t, r) = project.find_region(*region_id).ok_or("region not found")?;
            let mut v = region_summary(r, project);
            v["track_id"] = json!(t.id);
            match &r.content {
                RegionContent::Midi(m) => {
                    v["notes"] = json!(m.notes.iter().enumerate().map(|(i, n)| json!({"index": i, "pitch": n.pitch, "name": theory::note_name(n.pitch), "velocity": n.velocity, "start": n.start, "length": n.length})).collect::<Vec<_>>());
                    v["cc"] = json!(m.cc);
                    v["velocity_offset"] = json!(m.velocity_offset);
                }
                RegionContent::Pattern(p) => {
                    v["step_length"] = json!(p.step_length);
                    v["swing"] = json!(p.swing);
                    v["rows"] = json!(p.rows.iter().enumerate().map(|(i, r)| json!({
                        "index": i, "name": r.name, "pitch": r.pitch, "muted": r.muted,
                        "pattern": r.steps.iter().map(|s| match s { 0 => '.', 1..=60 => 'g', 61..=100 => 'o', _ => 'x' }).collect::<String>()
                    })).collect::<Vec<_>>());
                }
                RegionContent::Audio(a) => {
                    v["audio"] = json!({"file_id": a.file_id, "offset_sec": a.offset_sec, "gain_db": a.gain_db, "fade_in": a.fade_in, "fade_out": a.fade_out, "reverse": a.reverse});
                }
            }
            ok(v)
        }

        // notes
        AddNotes { region_id, notes } => {
            let m = midi_mut(project, *region_id)?;
            let before = m.notes.len();
            m.notes.extend(notes.iter().copied());
            let added: Vec<bool> = (0..m.notes.len()).map(|i| i >= before).collect();
            let indices = sanitize_notes_tracked(&mut m.notes, &added);
            ok(json!({"note_count": m.notes.len(), "indices": indices}))
        }
        ReplaceNotes { region_id, notes } => {
            let m = midi_mut(project, *region_id)?;
            m.notes = notes.clone();
            sanitize_notes(&mut m.notes);
            ok(json!({"note_count": m.notes.len()}))
        }
        RemoveNotes { region_id, indices, pitch, from_beat, to_beat } => {
            let m = midi_mut(project, *region_id)?;
            let before = m.notes.len();
            let mut i = 0;
            m.notes.retain(|n| {
                let idx = i;
                i += 1;
                let mut matched = true;
                if let Some(ix) = indices {
                    matched &= ix.contains(&idx);
                }
                if let Some(p) = pitch {
                    matched &= n.pitch == *p;
                }
                if let Some(f) = from_beat {
                    matched &= n.start >= *f;
                }
                if let Some(t) = to_beat {
                    matched &= n.start < *t;
                }
                !matched
            });
            ok(json!({"removed": before - m.notes.len(), "note_count": m.notes.len()}))
        }
        TransposeNotes { region_id, semitones, indices } => {
            let m = midi_mut(project, *region_id)?;
            for (i, n) in m.notes.iter_mut().enumerate() {
                if indices.as_ref().map(|ix| ix.contains(&i)).unwrap_or(true) {
                    n.pitch = (n.pitch as i32 + semitones).clamp(0, 127) as u8;
                }
            }
            ok(json!({"ok": true}))
        }
        QuantizeNotes { region_id, grid, strength, swing, quantize_ends, indices } => {
            let m = midi_mut(project, *region_id)?;
            let mask = note_mask(m.notes.len(), indices.as_deref());
            let (g, st, sw, qe) = (*grid, strength.unwrap_or(1.0).clamp(0.0, 1.0), swing.unwrap_or(0.0), quantize_ends.unwrap_or(false));
            on_masked(&mut m.notes, &mask, |sub| generate::quantize(sub, g, st, sw, qe));
            let moved = sanitize_notes_tracked(&mut m.notes, &mask);
            ok(with_indices(json!({"ok": true}), indices.is_some(), moved))
        }
        HumanizeNotes { region_id, timing, velocity, seed, indices } => {
            let m = midi_mut(project, *region_id)?;
            let mask = note_mask(m.notes.len(), indices.as_deref());
            let (t, v, s) = (timing.unwrap_or(0.01), velocity.unwrap_or(8), seed_or_random(*seed));
            on_masked(&mut m.notes, &mask, |sub| generate::humanize(sub, t, v, s));
            let moved = sanitize_notes_tracked(&mut m.notes, &mask);
            ok(with_indices(json!({"ok": true}), indices.is_some(), moved))
        }
        SetNoteVelocities { region_id, velocity, scale, indices } => {
            let m = midi_mut(project, *region_id)?;
            for (i, n) in m.notes.iter_mut().enumerate() {
                if indices.as_ref().map(|ix| ix.contains(&i)).unwrap_or(true) {
                    if let Some(v) = velocity {
                        n.velocity = (*v).clamp(1, 127);
                    }
                    if let Some(s) = scale {
                        n.velocity = ((n.velocity as f32) * s).round().clamp(1.0, 127.0) as u8;
                    }
                }
            }
            ok(json!({"ok": true}))
        }
        SetNoteLengths { region_id, mode, length, factor, indices } => {
            let positive = |v: Option<f64>, msg: &str| match v {
                Some(x) if x.is_finite() && x > 0.0 => Ok(x),
                _ => Err(msg.to_string()),
            };
            let value = match mode {
                NoteLengthMode::Fixed => positive(*length, "mode \"fixed\" needs length: the new length in beats, more than 0")?,
                NoteLengthMode::Scale => positive(*factor, "mode \"scale\" needs factor: a multiplier more than 0")?,
                NoteLengthMode::Legato | NoteLengthMode::NoOverlap => 0.0,
            };
            let r = region_mut(project, *region_id)?;
            let region_len = r.length;
            let RegionContent::Midi(m) = &mut r.content else { return err("region is not a MIDI region") };
            let mask = note_mask(m.notes.len(), indices.as_deref());
            let lengths = new_note_lengths(&m.notes, &mask, *mode, value, region_len);
            let mut changed = 0;
            for (n, l) in m.notes.iter_mut().zip(lengths) {
                if (n.length - l).abs() > 1e-9 {
                    n.length = l;
                    changed += 1;
                }
            }
            // Starts and pitches stay, so the order and every index stay too.
            let kept: Vec<usize> = (0..mask.len()).filter(|&i| mask[i]).collect();
            ok(with_indices(json!({"changed": changed, "note_count": m.notes.len()}), indices.is_some(), kept))
        }
        SnapNotesToKey { region_id, indices } => {
            let key = project.key;
            let m = midi_mut(project, *region_id)?;
            let mask = note_mask(m.notes.len(), indices.as_deref());
            let mut changed = 0;
            for (n, _) in m.notes.iter_mut().zip(&mask).filter(|(_, s)| **s) {
                let s = key.snap(n.pitch);
                if s != n.pitch {
                    changed += 1;
                    n.pitch = s;
                }
            }
            // Snap does not re-sort, so the chosen notes keep their positions.
            let kept: Vec<usize> = (0..mask.len()).filter(|&i| mask[i]).collect();
            ok(with_indices(json!({"changed": changed}), indices.is_some(), kept))
        }
        AddHarmony { region_id, interval, below, indices } => {
            let key = project.key;
            let m = midi_mut(project, *region_id)?;
            let mask = note_mask(m.notes.len(), indices.as_deref());
            let (mut copies, mut already, mut out_of_range) = (Vec::<Note>::new(), 0, 0);
            for (n, _) in m.notes.iter().zip(&mask).filter(|(_, s)| **s) {
                let Some(pitch) = key.harmony(n.pitch, *interval, below.unwrap_or(false)) else {
                    out_of_range += 1;
                    continue;
                };
                let same = |o: &Note| o.pitch == pitch && (o.start - n.start).abs() < 1e-9;
                if m.notes.iter().any(same) || copies.iter().any(same) {
                    already += 1;
                    continue;
                }
                let velocity = (n.velocity as f32 * HARMONY_VELOCITY).round().clamp(1.0, 127.0) as u8;
                copies.push(Note { pitch, velocity, start: n.start, length: n.length });
            }
            let before = m.notes.len();
            m.notes.extend(copies);
            let added: Vec<bool> = (0..m.notes.len()).map(|i| i >= before).collect();
            let indices = sanitize_notes_tracked(&mut m.notes, &added);
            ok(json!({"added": indices.len(), "indices": indices, "already_present": already, "out_of_range": out_of_range, "note_count": m.notes.len()}))
        }
        InsertChord { region_id, symbol, start, length, octave, inversion, velocity, with_bass } => {
            let chord = parse_chord_or_roman(symbol, project.key)?;
            let center = ((octave.unwrap_or(4) + 1) * 12 + 4).clamp(12, 115) as u8;
            let pitches = chord.voicing(center, inversion.unwrap_or(0), with_bass.unwrap_or(false));
            let m = midi_mut(project, *region_id)?;
            for p in &pitches {
                m.notes.push(Note { pitch: *p, velocity: velocity.unwrap_or(90), start: *start, length: *length });
            }
            sanitize_notes(&mut m.notes);
            ok(json!({"pitches": pitches}))
        }
        AddCc { region_id, events } => {
            let m = midi_mut(project, *region_id)?;
            m.cc.extend(events.iter().copied());
            m.cc.sort_by(|a, b| a.beat.partial_cmp(&b.beat).unwrap());
            ok(json!({"cc_count": m.cc.len()}))
        }
        ReplaceCc { region_id, controller, from_beat, to_beat, events } => {
            let (from, to) = check_replace_cc(*controller, *from_beat, *to_beat, events)?;
            let m = midi_mut(project, *region_id)?;
            let (removed, added) = replace_cc_events(&mut m.cc, *controller, from, to, events);
            ok(json!({"removed": removed, "added": added, "cc_count": m.cc.len()}))
        }

        // pattern
        SetPatternStep { region_id, row, step, velocity } => {
            let p = pattern_mut(project, *region_id)?;
            let r = p.rows.get_mut(*row).ok_or("row not found")?;
            *r.steps.get_mut(*step).ok_or("step out of range")? = (*velocity).min(127);
            ok(json!({"ok": true}))
        }
        SetPatternRow { region_id, row, pattern } => {
            let p = pattern_mut(project, *region_id)?;
            let steps = p.steps as usize;
            let r = p.rows.get_mut(*row).ok_or("row not found")?;
            r.steps = parse_step_string(pattern, steps);
            ok(json!({"ok": true}))
        }
        AddPatternRow { region_id, name, pitch } => {
            let p = pattern_mut(project, *region_id)?;
            p.rows.push(PatternRow { name: name.clone(), pitch: (*pitch).min(127), steps: vec![0; p.steps as usize], muted: false });
            ok(json!({"row": p.rows.len() - 1}))
        }
        UpdatePatternRow { region_id, row, name, pitch, muted } => {
            if name.is_none() && pitch.is_none() && muted.is_none() {
                return err("nothing to change: pass name, pitch or muted");
            }
            let name = match name {
                Some(n) if n.trim().is_empty() => return err("name must not be empty"),
                Some(n) => Some(n.trim().to_string()),
                None => None,
            };
            if let Some(p) = pitch {
                if *p > 127 {
                    return Err(format!("pitch {p} is out of range 0..127"));
                }
            }
            let p = pattern_mut(project, *region_id)?;
            let rows = p.rows.len();
            let r = p.rows.get_mut(*row).ok_or_else(|| row_not_found(*row, rows))?;
            if let Some(n) = name {
                r.name = n;
            }
            if let Some(v) = pitch {
                r.pitch = *v;
            }
            if let Some(m) = muted {
                r.muted = *m;
            }
            ok(json!({"row": row, "name": r.name, "pitch": r.pitch, "muted": r.muted}))
        }
        RemovePatternRow { region_id, row } => {
            let p = pattern_mut(project, *region_id)?;
            if *row >= p.rows.len() {
                return Err(row_not_found(*row, p.rows.len()));
            }
            let removed = p.rows.remove(*row);
            ok(json!({"removed": {"name": removed.name, "pitch": removed.pitch}, "rows": p.rows.len()}))
        }
        SetPatternSwing { region_id, swing } => {
            pattern_mut(project, *region_id)?.swing = swing.clamp(0.0, 1.0);
            ok(json!({"ok": true}))
        }
        ConvertPatternToMidi { region_id } => {
            let r = region_mut(project, *region_id)?;
            let notes = match &r.content {
                RegionContent::Pattern(p) => pattern_to_notes(p),
                _ => return err("not a pattern region"),
            };
            r.content = RegionContent::Midi(MidiContent { notes, ..Default::default() });
            ok(json!({"ok": true}))
        }

        // generators
        GenerateDrums { track_id, start, bars, style, intensity, fill, seed } => {
            check_generated_bars(*bars)?;
            let bpb = project.time_signature.beats_per_bar();
            let seed = seed_or_random(*seed);
            let notes = generate::drums(*style, (*bars).max(1), bpb, intensity.unwrap_or(0.5).clamp(0.0, 1.0), fill.unwrap_or(true), seed);
            let bars = (*bars).max(1);
            let id = put_generated_region(project, *track_id, *start, bars, &format!("Drums ({style:?})"), notes)?;
            ok(generated(id, seed, *start, bars))
        }
        GenerateBass { track_id, start, bars, style, chords, beats_per_chord, octave, seed } => {
            let tc = resolve_chords(project, *start, *bars, chords, *beats_per_chord)?;
            let ts = project.time_signature;
            let seed = seed_or_random(*seed);
            let notes = generate::bass(&tc, *style, octave.unwrap_or(1), ts, seed);
            let bars = (*bars).max(1);
            let id = put_generated_region(project, *track_id, *start, bars, &format!("Bass ({style:?})"), notes)?;
            ok(generated(id, seed, *start, bars))
        }
        GenerateChords { track_id, start, bars, style, chords, beats_per_chord, center_pitch, seed } => {
            let tc = resolve_chords(project, *start, *bars, chords, *beats_per_chord)?;
            let ts = project.time_signature;
            let seed = seed_or_random(*seed);
            let notes = generate::comp(&tc, *style, center_pitch.unwrap_or(62), ts, seed);
            let bars = (*bars).max(1);
            let id = put_generated_region(project, *track_id, *start, bars, &format!("Chords ({style:?})"), notes)?;
            ok(generated(id, seed, *start, bars))
        }
        GenerateArpeggio { track_id, start, bars, mode, rate, octaves, chords, beats_per_chord, center_pitch, seed } => {
            let tc = resolve_chords(project, *start, *bars, chords, *beats_per_chord)?;
            let seed = seed_or_random(*seed);
            let notes = generate::arpeggio(&tc, *mode, rate.unwrap_or(0.25), octaves.unwrap_or(2), center_pitch.unwrap_or(60), 0.8, seed);
            let bars = (*bars).max(1);
            let id = put_generated_region(project, *track_id, *start, bars, "Arpeggio", notes)?;
            ok(generated(id, seed, *start, bars))
        }
        GenerateMelody { track_id, start, bars, density, octave, chords, beats_per_chord, seed } => {
            let tc = resolve_chords(project, *start, *bars, chords, *beats_per_chord)?;
            let ts = project.time_signature;
            let seed = seed_or_random(*seed);
            let notes = generate::melody(&tc, project.key, density.unwrap_or(0.5), octave.unwrap_or(4), ts, seed);
            let bars = (*bars).max(1);
            let id = put_generated_region(project, *track_id, *start, bars, "Melody", notes)?;
            ok(generated(id, seed, *start, bars))
        }

        // chord track
        SetChordProgression { start, chords, beats_per_chord, replace } => {
            let per = beats_per_chord.unwrap_or(project.time_signature.beats_per_bar());
            let end = start + per * chords.len() as f64;
            if replace.unwrap_or(true) {
                project.chords.retain(|c| c.beat + c.length <= *start + 1e-9 || c.beat >= end - 1e-9);
            }
            let mut ids = vec![];
            for (i, s) in chords.iter().enumerate() {
                let symbol = resolve_chord_symbol(s, project.key)?;
                let id = project.alloc_id();
                project.chords.push(ChordEvent { id, beat: start + i as f64 * per, length: per, symbol });
                ids.push(id);
            }
            project.chords.sort_by(|a, b| a.beat.partial_cmp(&b.beat).unwrap());
            ok(json!({"chord_ids": ids, "chords": project.chords}))
        }
        AddChord { beat, length, symbol } => {
            let symbol = resolve_chord_symbol(symbol, project.key)?;
            let id = project.alloc_id();
            project.chords.push(ChordEvent { id, beat: *beat, length: *length, symbol: symbol.clone() });
            project.chords.sort_by(|a, b| a.beat.partial_cmp(&b.beat).unwrap());
            ok(json!({"chord_id": id, "symbol": symbol}))
        }
        UpdateChord { chord_id, symbol, beat, length } => {
            let symbol = symbol.as_deref().map(|s| resolve_chord_symbol(s, project.key)).transpose()?;
            if let Some(l) = length {
                if !l.is_finite() || *l <= 0.0 {
                    return err("length must be greater than 0");
                }
            }
            let c = project.chords.iter_mut().find(|c| c.id == *chord_id).ok_or("chord not found")?;
            if let Some(s) = symbol {
                c.symbol = s;
            }
            if let Some(b) = beat {
                c.beat = b.max(0.0);
            }
            if let Some(l) = length {
                c.length = *l;
            }
            let out = json!({"chord_id": c.id, "beat": c.beat, "length": c.length, "symbol": c.symbol});
            project.chords.sort_by(|a, b| a.beat.partial_cmp(&b.beat).unwrap());
            ok(out)
        }
        RemoveChord { chord_id } => {
            project.chords.retain(|c| c.id != *chord_id);
            ok(json!({"ok": true}))
        }
        ClearChords {} => {
            project.chords.clear();
            ok(json!({"ok": true}))
        }

        // automation
        SetAutomation { track_id, target, points } => {
            let range = validate_target(project, *track_id, target)?;
            if points.iter().any(|p| !p.beat.is_finite() || !p.value.is_finite()) {
                return err("automation beats and values must be finite numbers");
            }
            let lanes = automation_lanes_mut(project, *track_id)?;
            let mut pts = points.clone();
            for p in &mut pts {
                p.value = range.clamp(p.value);
            }
            pts.sort_by(|a, b| a.beat.total_cmp(&b.beat));
            lanes.retain(|l| l.target != *target);
            lanes.push(AutomationLane { target: target.clone(), points: pts.clone(), enabled: true });
            ok(json!({"ok": true, "points": pts}))
        }
        AddAutomationPoint { track_id, target, beat, value } => {
            let range = validate_target(project, *track_id, target)?;
            if !beat.is_finite() || !value.is_finite() {
                return err("automation beat and value must be finite numbers");
            }
            let value = range.clamp(*value);
            let lanes = automation_lanes_mut(project, *track_id)?;
            let lane = match lanes.iter_mut().position(|l| l.target == *target) {
                Some(i) => &mut lanes[i],
                None => {
                    lanes.push(AutomationLane { target: target.clone(), points: vec![], enabled: true });
                    lanes.last_mut().unwrap()
                }
            };
            lane.points.retain(|p| (p.beat - beat).abs() > 1e-6);
            lane.points.push(AutomationPoint { beat: *beat, value });
            lane.points.sort_by(|a, b| a.beat.total_cmp(&b.beat));
            ok(json!({"points": lane.points.len(), "value": value}))
        }
        ClearAutomation { track_id, target } => {
            let lanes = automation_lanes_mut(project, *track_id)?;
            match target {
                Some(t) => lanes.retain(|l| l.target != *t),
                None => lanes.clear(),
            }
            ok(json!({"ok": true}))
        }

        // markers & sections
        AddMarker { beat, name, length } => {
            let id = project.alloc_id();
            project.markers.push(Marker { id, beat: beat.max(0.0), name: name.clone(), length: *length });
            project.markers.sort_by(|a, b| a.beat.partial_cmp(&b.beat).unwrap());
            ok(json!({"marker_id": id}))
        }
        RemoveMarker { marker_id } => {
            project.markers.retain(|m| m.id != *marker_id);
            ok(json!({"ok": true}))
        }
        UpdateMarker { marker_id, name, beat, length } => {
            let m = project.markers.iter_mut().find(|m| m.id == *marker_id).ok_or("marker not found")?;
            if let Some(n) = name {
                m.name = n.clone();
            }
            if let Some(b) = beat {
                m.beat = b.max(0.0);
            }
            if let Some(l) = length {
                m.length = if *l > 0.0 { Some(*l) } else { None };
            }
            project.markers.sort_by(|a, b| a.beat.partial_cmp(&b.beat).unwrap());
            ok(json!({"ok": true}))
        }
        CopySection { from_beat, length, to_beat, insert } => {
            split_all_at(project, *from_beat);
            split_all_at(project, from_beat + length);
            let regs = all_regions_in(project, *from_beat, from_beat + length);
            let chords: Vec<ChordEvent> = project.chords.iter().filter(|c| c.beat >= *from_beat && c.beat < from_beat + length).cloned().collect();
            if insert.unwrap_or(false) {
                split_all_at(project, *to_beat);
                shift_after(project, *to_beat, *length);
            }
            let delta = to_beat - from_beat;
            let mut n = 0;
            for (tid, mut r) in regs {
                r.id = project.alloc_id();
                r.start += delta;
                project.track_mut(tid).unwrap().regions.push(r);
                n += 1;
            }
            for mut c in chords {
                c.id = project.alloc_id();
                c.beat += delta;
                project.chords.push(c);
            }
            project.chords.sort_by(|a, b| a.beat.partial_cmp(&b.beat).unwrap());
            ok(json!({"copied_regions": n}))
        }
        DeleteSection { from_beat, length, ripple } => {
            let end = from_beat + length;
            split_all_at(project, *from_beat);
            split_all_at(project, end);
            for t in &mut project.tracks {
                t.regions.retain(|r| !(r.start >= *from_beat - 1e-9 && r.start < end - 1e-9));
            }
            project.chords.retain(|c| !(c.beat >= *from_beat && c.beat < end));
            project.markers.retain(|m| !(m.beat >= *from_beat && m.beat < end));
            if ripple.unwrap_or(true) {
                shift_after(project, end, -length);
            }
            ok(json!({"ok": true}))
        }
        InsertTime { at_beat, length } => {
            split_all_at(project, *at_beat);
            shift_after(project, *at_beat, *length);
            ok(json!({"ok": true}))
        }

        _ => crate::ops2::apply(project, cmd),
    }
}

fn automation_lanes_mut(project: &mut Project, track_id: Id) -> Result<&mut Vec<AutomationLane>, String> {
    if track_id == MASTER_ID {
        return err("master automation is not supported; automate a track or a bus");
    }
    Ok(&mut project.track_mut(track_id).ok_or("track not found")?.automation)
}

/// The values an automation lane may hold: the same ranges the commands that set the parameter directly use
/// (set_volume, set_pan, set_send_level, set_surround_pan, set_plugin_param).
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct AutomationRange {
    pub min: f32,
    pub max: f32,
    /// A selector parameter: values are whole numbers.
    pub stepped: bool,
}

impl AutomationRange {
    const fn new(min: f32, max: f32) -> Self {
        Self { min, max, stepped: false }
    }

    /// `v` in range (rounded for a selector). `v` must be finite.
    pub fn clamp(&self, v: f32) -> f32 {
        let v = v.clamp(self.min, self.max);
        if self.stepped { v.round() } else { v }
    }
}

/// Fader and send levels in dB, as set_volume and set_send_level keep them.
const LEVEL_RANGE: AutomationRange = AutomationRange::new(-96.0, 6.0);

/// Checks an automation target of a track and returns the range of its values.
pub fn validate_target(project: &Project, track_id: Id, target: &str) -> Result<AutomationRange, String> {
    match target {
        "volume" => return Ok(LEVEL_RANGE),
        "pan" => return Ok(AutomationRange::new(-1.0, 1.0)),
        "surround:azimuth" => return Ok(AutomationRange::new(-180.0, 180.0)),
        "surround:elevation" => return Ok(AutomationRange::new(-30.0, 90.0)),
        "surround:spread" => return Ok(AutomationRange::new(0.0, 1.0)),
        _ => {}
    }
    if let Some(rest) = target.strip_prefix("send:") {
        let idx: usize = rest.parse().map_err(|_| "send target must be send:<index>")?;
        let t = project.track(track_id).ok_or("track not found")?;
        return if idx < t.channel.sends.len() { Ok(LEVEL_RANGE) } else { err("send index out of range") };
    }
    if let Some(rest) = target.strip_prefix("plugin:") {
        let (pid, param) = rest.split_once(':').ok_or("plugin target must be plugin:<id>:<param>")?;
        let pid: Id = pid.parse().map_err(|_| "invalid plugin id")?;
        let p = project.find_plugin(pid).ok_or("plug-in not found")?;
        let info = plugins::find(&p.kind).ok_or("unknown plug-in")?;
        return match info.params.iter().find(|x| x.id == param) {
            Some(x) => Ok(AutomationRange { min: x.min, max: x.max, stepped: !x.choices.is_empty() }),
            None => err(format!("{} has no parameter {param}", p.kind)),
        };
    }
    err("target must be volume, pan, surround:azimuth, surround:elevation, surround:spread, send:<i> or plugin:<id>:<param>")
}

/// Rewrites `send:<i>` lane targets after some sends of the same track were removed.
/// `remap[i]` is the new index of old send `i`, or `None` if it was removed (its lane goes too).
/// Lanes whose index was already out of range point at nothing today and are left as they are.
/// Returns the number of lanes removed.
fn remap_send_lanes(lanes: &mut Vec<AutomationLane>, remap: &[Option<usize>]) -> usize {
    let before = lanes.len();
    lanes.retain_mut(|lane| {
        let Some(i) = lane.target.strip_prefix("send:").and_then(|s| s.parse::<usize>().ok()) else { return true };
        match remap.get(i) {
            None => true,
            Some(None) => false,
            Some(Some(j)) => {
                if *j != i {
                    lane.target = format!("send:{j}");
                }
                true
            }
        }
    });
    before - lanes.len()
}

/// Removes the sends of `track` for which `remove` is true and keeps every remaining `send:<i>` lane
/// on the send it was automating. Returns the number of lanes removed.
fn remove_sends(track: &mut Track, mut remove: impl FnMut(usize, &Send) -> bool) -> usize {
    let mut next = 0;
    let remap: Vec<Option<usize>> = track
        .channel
        .sends
        .iter()
        .enumerate()
        .map(|(i, s)| {
            if remove(i, s) {
                None
            } else {
                next += 1;
                Some(next - 1)
            }
        })
        .collect();
    let old = std::mem::take(&mut track.channel.sends);
    track.channel.sends = old.into_iter().zip(&remap).filter(|(_, m)| m.is_some()).map(|(s, _)| s).collect();
    remap_send_lanes(&mut track.automation, &remap)
}

/// Plug-in id of a `plugin:<id>:<param>` lane target.
fn lane_plugin_id(target: &str) -> Option<Id> {
    target.strip_prefix("plugin:")?.split_once(':')?.0.parse().ok()
}

/// Removes, on every track, the automation lanes that target one of `plugin_ids`. Returns how many went.
fn remove_plugin_lanes(project: &mut Project, plugin_ids: &[Id]) -> usize {
    if plugin_ids.is_empty() {
        return 0;
    }
    let mut n = 0;
    for t in &mut project.tracks {
        let before = t.automation.len();
        t.automation.retain(|l| !lane_plugin_id(&l.target).is_some_and(|id| plugin_ids.contains(&id)));
        n += before - t.automation.len();
    }
    n
}

/// Points the `plugin:<old>:<param>` lanes of a copied track at the copy's own plug-ins. `ids` pairs each old
/// plug-in id with the new one; lanes of plug-ins that are not in `ids` (on another track or on the master) and
/// every other target are left as they are.
fn remap_plugin_lanes(lanes: &mut [AutomationLane], ids: &[(Id, Id)]) {
    for lane in lanes {
        let Some(old) = lane_plugin_id(&lane.target) else { continue };
        let Some(&(_, new)) = ids.iter().find(|(o, _)| *o == old) else { continue };
        let Some((_, param)) = lane.target.strip_prefix("plugin:").and_then(|r| r.split_once(':')) else { continue };
        lane.target = format!("plugin:{new}:{param}");
    }
}

/// Ids of every plug-in on a track: instrument, MIDI effects and inserts.
fn track_plugin_ids(t: &Track) -> Vec<Id> {
    t.instrument.iter().chain(&t.midi_fx).chain(&t.channel.inserts).map(|p| p.id).collect()
}

/// Removes a plug-in wherever it is (master inserts, instrument, MIDI effects, inserts). False if not found.
fn remove_plugin_instance(project: &mut Project, pid: Id) -> bool {
    if project.master.inserts.iter().any(|p| p.id == pid) {
        project.master.inserts.retain(|p| p.id != pid);
        return true;
    }
    for t in &mut project.tracks {
        if t.instrument.as_ref().map(|i| i.id == pid).unwrap_or(false) {
            t.instrument = None;
            return true;
        }
        let before = t.midi_fx.len() + t.channel.inserts.len();
        t.midi_fx.retain(|p| p.id != pid);
        t.channel.inserts.retain(|p| p.id != pid);
        if t.midi_fx.len() + t.channel.inserts.len() != before {
            return true;
        }
    }
    false
}

/// What else went away with a removed track, for the command result.
struct Pruned {
    automation_lanes: usize,
    live_cells: usize,
    midi_routes: usize,
}

/// Drops everything that still refers to a track that was just removed: sends to it on the other tracks
/// (with their lanes, renumbering the later `send:<i>` lanes), output routing to it, lanes on other tracks that
/// target its plug-ins, its Live Loops cells and the MIDI input routes to it.
fn prune_references(project: &mut Project, removed: &Track) -> Pruned {
    let mut automation_lanes = 0;
    for t in &mut project.tracks {
        automation_lanes += remove_sends(t, |_, s| s.bus_id == removed.id);
        if t.channel.output == Output::Bus(removed.id) {
            t.channel.output = Output::Master;
        }
    }
    automation_lanes += remove_plugin_lanes(project, &track_plugin_ids(removed));
    let cells = project.live.cells.len();
    project.live.cells.retain(|c| c.track_id != removed.id);
    let routes = project.midi_routes.len();
    project.midi_routes.retain(|r| r.track_id != removed.id);
    Pruned { automation_lanes, live_cells: cells - project.live.cells.len(), midi_routes: routes - project.midi_routes.len() }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::generate::DrumStyle;

    fn run(p: &mut Project, c: Command) -> Value {
        match apply(p, &c) {
            Ok(Outcome::Done(v)) => v,
            Ok(Outcome::NotHandled) => panic!("not handled"),
            Err(e) => panic!("{e}"),
        }
    }

    fn region_color_props(region_id: Id, name: Option<&str>, color: Option<&str>) -> Command {
        Command::SetRegionProperties { region_id, name: name.map(String::from), muted: None, loop_count: None, transpose: None, velocity_offset: None, gain_db: None, fade_in: None, fade_out: None, reverse: None, color: color.map(String::from) }
    }

    #[test]
    fn region_color_clears_with_an_empty_string_or_none() {
        let mut p = Project::new("t");
        let tid = add_track_named(&mut p, TrackKind::Instrument, "Keys");
        let rid = run(&mut p, Command::CreateMidiRegion { track_id: tid, start: 0.0, length: 4.0, name: None, notes: None })["region_id"].as_u64().unwrap();
        let state = |p: &Project| {
            let r = p.find_region(rid).unwrap().1;
            (r.name.clone(), r.color)
        };
        let shown = |p: &mut Project| run(p, Command::GetRegion { region_id: rid })["color"].clone();

        run(&mut p, region_color_props(rid, None, Some("#FF8000")));
        assert_eq!(state(&p).1, Some([255, 128, 0]));
        assert_eq!(shown(&mut p), json!("#FF8000"));

        run(&mut p, region_color_props(rid, None, Some("")));
        assert_eq!(state(&p).1, None);
        assert!(shown(&mut p).is_null());

        run(&mut p, region_color_props(rid, None, Some("#102030")));
        run(&mut p, region_color_props(rid, None, Some(" None ")));
        assert_eq!(state(&p).1, None);

        // A name alone leaves the color as it is.
        run(&mut p, region_color_props(rid, None, Some("#102030")));
        run(&mut p, region_color_props(rid, Some("Fill"), None));
        assert_eq!(state(&p), ("Fill".to_string(), Some([16, 32, 48])));

        // A bad color changes nothing, not even the name sent with it.
        assert!(apply(&mut p, &region_color_props(rid, Some("X"), Some("#12"))).is_err());
        assert_eq!(state(&p), ("Fill".to_string(), Some([16, 32, 48])));
    }

    #[test]
    fn resolve_chords_reads_roman_numerals_saved_on_the_chord_track() {
        let mut p = Project::new("t");
        // An older version stored roman numerals on the chord track as typed.
        let mut c: ChordEvent = serde_json::from_value(json!({"id": 0, "beat": 0.0, "length": 4.0, "symbol": "vi"})).unwrap();
        c.id = p.alloc_id();
        p.chords.push(c);
        let tc = resolve_chords(&p, 0.0, 1, &None, None).unwrap();
        assert_eq!(tc.len(), 1);
        assert_eq!(tc[0].chord.root, 9);
        assert_eq!(tc[0].chord.intervals, vec![0, 3, 7]);

        let mut bad: ChordEvent = serde_json::from_value(json!({"id": 0, "beat": 4.0, "length": 4.0, "symbol": "zz"})).unwrap();
        bad.id = p.alloc_id();
        p.chords.push(bad);
        let tc = resolve_chords(&p, 0.0, 2, &None, None).unwrap();
        assert_eq!(tc.len(), 1, "an unreadable symbol is skipped");
        assert_eq!(tc[0].chord.root, 9);
    }

    #[test]
    fn build_song() {
        let mut p = Project::new("t");
        let d = run(&mut p, Command::AddTrack { kind: TrackKind::Instrument, name: None, patch: Some("Pop Drum Kit".into()), instrument: None, preset: None, index: None });
        let did = d["track_id"].as_u64().unwrap();
        run(&mut p, Command::SetChordProgression { start: 0.0, chords: vec!["I".into(), "vi".into(), "IV".into(), "V".into()], beats_per_chord: None, replace: None });
        assert_eq!(p.chords[1].symbol, "Am");
        let r = run(&mut p, Command::GenerateDrums { track_id: did, start: 0.0, bars: 4, style: DrumStyle::Pop, intensity: None, fill: None, seed: Some(1) });
        let rid = r["region_id"].as_u64().unwrap();
        let b = run(&mut p, Command::AddTrack { kind: TrackKind::Instrument, name: None, patch: Some("Deep Sub Bass".into()), instrument: None, preset: None, index: None });
        let bid = b["track_id"].as_u64().unwrap();
        run(&mut p, Command::GenerateBass { track_id: bid, start: 0.0, bars: 4, style: crate::generate::BassStyle::RootFifth, chords: None, beats_per_chord: None, octave: None, seed: Some(2) });
        let s = run(&mut p, Command::SplitRegion { region_id: rid, beat: 8.0 });
        assert!(s["new_region_id"].as_u64().is_some());
        run(&mut p, Command::CopySection { from_beat: 0.0, length: 16.0, to_beat: 16.0, insert: Some(false) });
        assert!(p.content_end() >= 32.0);
        let summary = project_summary(&p);
        assert_eq!(summary["tracks"].as_array().unwrap().len(), 2);
        let bus = run(&mut p, Command::AddTrack { kind: TrackKind::Bus, name: Some("Verb".into()), patch: None, instrument: None, preset: None, index: None });
        let bus_id = bus["track_id"].as_u64().unwrap();
        run(&mut p, Command::AddSend { track_id: bid, bus_id, level_db: None, pre_fader: None });
        run(&mut p, Command::AddAutomationPoint { track_id: bid, target: "send:0".into(), beat: 0.0, value: -20.0 });
        assert!(apply(&mut p, &Command::AddAutomationPoint { track_id: bid, target: "send:5".into(), beat: 0.0, value: 0.0 }).is_err());
    }

    /// A project with one instrument track over C Am F G, for the generator tests.
    fn generator_project() -> (Project, Id) {
        let mut p = Project::new("t");
        let t = run(&mut p, Command::AddTrack { kind: TrackKind::Instrument, name: None, patch: None, instrument: None, preset: None, index: None });
        let chords = ["C", "Am", "F", "G"].iter().map(|c| c.to_string()).collect();
        run(&mut p, Command::SetChordProgression { start: 0.0, chords, beats_per_chord: None, replace: None });
        (p, t["track_id"].as_u64().unwrap())
    }

    /// The five generators, by name, for one track, start, length and seed.
    fn generator_commands(track_id: Id, start: f64, bars: u32, seed: Option<u64>) -> Vec<(&'static str, Command)> {
        use crate::generate::{ArpMode, BassStyle, CompStyle};
        vec![
            ("drums", Command::GenerateDrums { track_id, start, bars, style: DrumStyle::Pop, intensity: None, fill: None, seed }),
            ("bass", Command::GenerateBass { track_id, start, bars, style: BassStyle::Walking, chords: None, beats_per_chord: None, octave: None, seed }),
            ("chords", Command::GenerateChords { track_id, start, bars, style: CompStyle::Pop, chords: None, beats_per_chord: None, center_pitch: None, seed }),
            ("arpeggio", Command::GenerateArpeggio { track_id, start, bars, mode: ArpMode::UpDown, rate: None, octaves: None, chords: None, beats_per_chord: None, center_pitch: None, seed }),
            ("melody", Command::GenerateMelody { track_id, start, bars, density: None, octave: None, chords: None, beats_per_chord: None, seed }),
        ]
    }

    #[test]
    fn generators_return_the_seed_start_and_bars() {
        let (mut p, tid) = generator_project();
        for (name, c) in generator_commands(tid, 8.0, 2, Some(7)) {
            let r = run(&mut p, c);
            assert!(r["region_id"].as_u64().is_some(), "{name}: {r}");
            assert_eq!(r["seed"], json!(7), "{name}: {r}");
            assert_eq!(r["start"], json!(8.0), "{name}: {r}");
            assert_eq!(r["bars"], json!(2), "{name}: {r}");
        }
    }

    #[test]
    fn a_returned_seed_repeats_the_same_notes() {
        let (mut p, tid) = generator_project();
        let first = generator_commands(tid, 0.0, 4, None);
        for (name, c) in first {
            let r = run(&mut p, c);
            let seed = r["seed"].as_u64().unwrap_or_else(|| panic!("{name}: no seed in {r}"));
            assert!(seed < 1_000_000, "{name}: drawn seed {seed}");
            let again = generator_commands(tid, 0.0, 4, Some(seed)).into_iter().find(|(n, _)| *n == name).unwrap().1;
            let r2 = run(&mut p, again);
            let a = notes_of(&p, r["region_id"].as_u64().unwrap());
            let b = notes_of(&p, r2["region_id"].as_u64().unwrap());
            assert!(!a.is_empty(), "{name}: no notes");
            assert_eq!(a, b, "{name}: the same seed gives other notes");
        }
    }

    fn n(pitch: u8, start: f64) -> Note {
        Note { pitch, velocity: 100, start, length: 0.5 }
    }

    fn midi_region(notes: &[Note]) -> (Project, Id) {
        let mut p = Project::new("t");
        let t = run(&mut p, Command::AddTrack { kind: TrackKind::Instrument, name: None, patch: None, instrument: None, preset: None, index: None });
        let tid = t["track_id"].as_u64().unwrap();
        let r = run(&mut p, Command::CreateMidiRegion { track_id: tid, start: 0.0, length: 8.0, name: None, notes: Some(notes.to_vec()) });
        (p, r["region_id"].as_u64().unwrap())
    }

    fn notes_of(p: &Project, rid: Id) -> Vec<Note> {
        match &p.find_region(rid).unwrap().1.content {
            RegionContent::Midi(m) => m.notes.clone(),
            _ => panic!("not a MIDI region"),
        }
    }

    #[test]
    fn add_notes_reports_where_the_added_notes_are() {
        let (mut p, rid) = midi_region(&[n(60, 0.0), n(64, 2.0)]);
        let v = run(&mut p, Command::AddNotes { region_id: rid, notes: vec![n(67, 1.0), n(60, 0.0), n(62, 3.0)] });
        assert_eq!(v["indices"], json!([1, 2, 4]));
        assert_eq!(v["note_count"], json!(5));
        let after = notes_of(&p, rid);
        // the added 60 is identical to the existing one and comes right after it
        assert_eq!((after[0].pitch, after[0].start), (60, 0.0));
        assert_eq!((after[1].pitch, after[1].start), (60, 0.0));
        assert_eq!((after[2].pitch, after[2].start), (67, 1.0));
        assert_eq!((after[3].pitch, after[3].start), (64, 2.0));
        assert_eq!((after[4].pitch, after[4].start), (62, 3.0));
        // a note that clamp_note corrects is still found
        let v = run(&mut p, Command::AddNotes { region_id: rid, notes: vec![Note { pitch: 72, velocity: 0, start: -1.0, length: 0.5 }] });
        let ix = v["indices"].as_array().unwrap();
        assert_eq!(ix.len(), 1);
        let added = notes_of(&p, rid)[ix[0].as_u64().unwrap() as usize];
        assert_eq!((added.pitch, added.start, added.velocity), (72, 0.0, 1));
        let v = run(&mut p, Command::AddNotes { region_id: rid, notes: vec![] });
        assert_eq!(v["indices"], json!([]));
        assert_eq!(v["note_count"], json!(6));
    }

    #[test]
    fn quantize_notes_acts_on_given_indices() {
        let start = [n(60, 0.1), n(62, 0.8), n(64, 0.9), n(65, 0.95)];
        let (mut p, rid) = midi_region(&start);
        let v = run(&mut p, Command::QuantizeNotes { region_id: rid, grid: 0.5, strength: None, swing: None, quantize_ends: None, indices: Some(vec![1, 2]) });
        // 62 and 64 land on beat 1, after 65 at 0.95, so sorting moves them to positions 2 and 3
        assert_eq!(v["indices"], json!([2, 3]));
        let after = notes_of(&p, rid);
        assert_eq!(after[0], start[0]);
        assert_eq!(after[1], start[3]);
        for i in [2, 3] {
            assert!((after[i].start - 1.0).abs() < 1e-9, "note {i} at {}", after[i].start);
        }
        let mut moved = vec![after[2].pitch, after[3].pitch];
        moved.sort();
        assert_eq!(moved, vec![62, 64]);
        // duplicates and indices past the end are ignored, as in transpose_notes
        let (mut q, rid2) = midi_region(&start);
        let w = run(&mut q, Command::QuantizeNotes { region_id: rid2, grid: 0.5, strength: None, swing: None, quantize_ends: None, indices: Some(vec![2, 1, 1, 99]) });
        assert_eq!(w["indices"], json!([2, 3]));
        assert_eq!(notes_of(&q, rid2), after);
    }

    #[test]
    fn humanize_notes_acts_on_given_indices() {
        let start = [n(60, 0.0), n(62, 1.0), n(64, 2.0), n(65, 3.0)];
        let (mut p, rid) = midi_region(&start);
        let v = run(&mut p, Command::HumanizeNotes { region_id: rid, timing: Some(0.05), velocity: Some(10), seed: Some(7), indices: Some(vec![0, 2]) });
        assert_eq!(v["indices"], json!([0, 2]));
        let after = notes_of(&p, rid);
        assert_eq!(after[1], start[1]);
        assert_eq!(after[3], start[3]);
        // the seed drives only the chosen notes, in index order
        let mut expect = vec![start[0], start[2]];
        generate::humanize(&mut expect, 0.05, 10, 7);
        assert_eq!(vec![after[0], after[2]], expect);
    }

    #[test]
    fn snap_notes_to_key_acts_on_given_indices() {
        // C major: C#, D, D#, F#
        let (mut p, rid) = midi_region(&[n(61, 0.0), n(62, 1.0), n(63, 2.0), n(66, 3.0)]);
        let v = run(&mut p, Command::SnapNotesToKey { region_id: rid, indices: Some(vec![0, 1]) });
        assert_eq!(v["changed"], json!(1));
        assert_eq!(v["indices"], json!([0, 1]));
        let pitches: Vec<u8> = notes_of(&p, rid).iter().map(|n| n.pitch).collect();
        assert_eq!(pitches, vec![60, 62, 63, 66]);
    }

    fn lengths(c: NoteLengthMode, region_id: Id, length: Option<f64>, factor: Option<f64>, indices: Option<Vec<usize>>) -> Command {
        Command::SetNoteLengths { region_id, mode: c, length, factor, indices }
    }

    fn note(pitch: u8, start: f64, length: f64) -> Note {
        Note { pitch, velocity: 100, start, length }
    }

    fn lens(p: &Project, rid: Id) -> Vec<f64> {
        notes_of(p, rid).iter().map(|n| n.length).collect()
    }

    #[test]
    fn set_note_lengths_legato_reaches_the_next_start() {
        let (mut p, rid) = midi_region(&[note(60, 0.0, 1.0), note(62, 2.0, 0.5), note(64, 4.0, 0.5)]);
        let v = run(&mut p, lengths(NoteLengthMode::Legato, rid, None, None, None));
        assert_eq!(v, json!({"changed": 2, "note_count": 3}));
        assert_eq!(lens(&p, rid), vec![2.0, 2.0, 0.5]);
    }

    #[test]
    fn set_note_lengths_legato_ends_a_chord_together() {
        let (mut p, rid) = midi_region(&[note(60, 0.0, 0.5), note(64, 0.0, 1.5), note(67, 0.0, 3.0), note(65, 2.0, 0.5)]);
        run(&mut p, lengths(NoteLengthMode::Legato, rid, None, None, None));
        assert_eq!(lens(&p, rid), vec![2.0, 2.0, 2.0, 0.5]);
        // a chord played live is never exactly together: it still ends together
        let (mut q, rid2) = midi_region(&[note(60, 0.0, 0.3), note(64, 0.01, 0.3), note(67, 0.02, 0.3), note(62, 1.0, 0.5)]);
        run(&mut q, lengths(NoteLengthMode::Legato, rid2, None, None, None));
        let after = notes_of(&q, rid2);
        for n in &after[..3] {
            assert!((n.start + n.length - 1.0).abs() < 1e-9, "note {} ends at {}", n.pitch, n.start + n.length);
        }
        assert_eq!(after[3].length, 0.5);
    }

    #[test]
    fn set_note_lengths_legato_looks_only_at_the_chosen_notes() {
        let start = [note(60, 0.0, 0.5), note(72, 1.0, 0.5), note(62, 2.0, 0.5), note(64, 3.0, 0.5)];
        let (mut p, rid) = midi_region(&start);
        let v = run(&mut p, lengths(NoteLengthMode::Legato, rid, None, None, Some(vec![0, 2])));
        assert_eq!(lens(&p, rid), vec![2.0, 0.5, 0.5, 0.5]);
        assert_eq!(v["changed"], json!(1));
        assert_eq!(v["indices"], json!([0, 2]));
        // duplicates, order and indices past the end do not matter
        let (mut q, rid2) = midi_region(&start);
        let w = run(&mut q, lengths(NoteLengthMode::Legato, rid2, None, None, Some(vec![2, 0, 0, 99])));
        assert_eq!(lens(&q, rid2), vec![2.0, 0.5, 0.5, 0.5]);
        assert_eq!(w["indices"], json!([0, 2]));
    }

    #[test]
    fn set_note_lengths_no_overlap_cuts_only_the_same_pitch() {
        let start = [note(60, 0.0, 2.0), note(64, 0.5, 3.0), note(60, 1.0, 0.5), note(67, 4.0, 1.0)];
        let (mut p, rid) = midi_region(&start);
        let v = run(&mut p, lengths(NoteLengthMode::NoOverlap, rid, None, None, None));
        assert_eq!(lens(&p, rid), vec![1.0, 3.0, 0.5, 1.0]);
        assert_eq!(v["changed"], json!(1));
        // the note that starts again cuts even when it is not chosen
        let (mut q, rid2) = midi_region(&start);
        let v = run(&mut q, lengths(NoteLengthMode::NoOverlap, rid2, None, None, Some(vec![0])));
        assert_eq!(lens(&q, rid2), vec![1.0, 3.0, 0.5, 1.0]);
        assert_eq!(v["indices"], json!([0]));
        // only the chosen notes change
        let (mut q, rid2) = midi_region(&start);
        let v = run(&mut q, lengths(NoteLengthMode::NoOverlap, rid2, None, None, Some(vec![1, 2])));
        assert_eq!(lens(&q, rid2), vec![2.0, 3.0, 0.5, 1.0]);
        assert_eq!(v["changed"], json!(0));
        // a note that ends exactly where the same pitch starts again stays
        let (mut q, rid2) = midi_region(&[note(60, 0.0, 1.0), note(60, 1.0, 0.5)]);
        let v = run(&mut q, lengths(NoteLengthMode::NoOverlap, rid2, None, None, None));
        assert_eq!(lens(&q, rid2), vec![1.0, 0.5]);
        assert_eq!(v["changed"], json!(0));
    }

    #[test]
    fn set_note_lengths_fixed_and_scale_keep_the_minimum() {
        let start = [note(60, 0.0, 0.5), note(62, 1.0, 0.5)];
        let (mut p, rid) = midi_region(&start);
        let v = run(&mut p, lengths(NoteLengthMode::Fixed, rid, Some(0.001), None, None));
        assert_eq!(lens(&p, rid), vec![1.0 / 64.0, 1.0 / 64.0]);
        assert_eq!(v["changed"], json!(2));
        let (mut q, rid2) = midi_region(&start);
        run(&mut q, lengths(NoteLengthMode::Scale, rid2, None, Some(0.01), None));
        assert_eq!(lens(&q, rid2), vec![1.0 / 64.0, 1.0 / 64.0]);
        let (mut q, rid2) = midi_region(&start);
        run(&mut q, lengths(NoteLengthMode::Scale, rid2, None, Some(2.0), None));
        assert_eq!(lens(&q, rid2), vec![1.0, 1.0]);
        run(&mut q, lengths(NoteLengthMode::Fixed, rid2, Some(0.25), None, None));
        assert_eq!(lens(&q, rid2), vec![0.25, 0.25]);
    }

    #[test]
    fn set_note_lengths_stop_at_the_region_end() {
        let mut p = Project::new("t");
        let t = run(&mut p, Command::AddTrack { kind: TrackKind::Instrument, name: None, patch: None, instrument: None, preset: None, index: None });
        let tid = t["track_id"].as_u64().unwrap();
        let notes = vec![note(64, 2.0, 3.0), note(60, 3.0, 0.5), note(62, 5.0, 0.5)];
        let r = run(&mut p, Command::CreateMidiRegion { track_id: tid, start: 0.0, length: 4.0, name: None, notes: Some(notes) });
        let rid = r["region_id"].as_u64().unwrap();
        // 64 is already past the end and keeps its length; 60 stops at 4; 62 is past the end and does not grow
        let v = run(&mut p, lengths(NoteLengthMode::Fixed, rid, Some(8.0), None, None));
        assert_eq!(lens(&p, rid), vec![3.0, 1.0, 0.5]);
        assert_eq!(v["changed"], json!(1));
        // shortening is always possible
        run(&mut p, lengths(NoteLengthMode::Scale, rid, None, Some(0.5), None));
        assert_eq!(lens(&p, rid), vec![1.5, 0.5, 0.25]);
        // a looped region: one repeat counts, not the whole looped length
        let mut props = region_color_props(rid, None, None);
        if let Command::SetRegionProperties { loop_count, .. } = &mut props {
            *loop_count = Some(3);
        }
        run(&mut p, props);
        run(&mut p, lengths(NoteLengthMode::Fixed, rid, Some(8.0), None, None));
        assert_eq!(lens(&p, rid), vec![2.0, 1.0, 0.25]);
    }

    #[test]
    fn set_note_lengths_keeps_order_and_indices() {
        let start = [note(60, 0.0, 1.0), note(64, 0.0, 2.0), note(67, 0.01, 0.1), note(60, 0.5, 1.0), note(72, 0.5, 0.05), note(62, 3.0, 0.25)];
        let modes = [
            (NoteLengthMode::Legato, None, None),
            (NoteLengthMode::NoOverlap, None, None),
            (NoteLengthMode::Fixed, Some(0.75), None),
            (NoteLengthMode::Scale, None, Some(1.5)),
        ];
        let rest = |p: &Project, rid: Id| notes_of(p, rid).iter().map(|n| (n.pitch, n.start, n.velocity)).collect::<Vec<_>>();
        for (mode, length, factor) in modes {
            let (mut p, rid) = midi_region(&start);
            let before = rest(&p, rid);
            run(&mut p, lengths(mode, rid, length, factor, None));
            assert_eq!(rest(&p, rid), before, "{mode:?}");
        }
    }

    #[test]
    fn set_note_lengths_rejects_missing_or_bad_values() {
        let (mut p, rid) = midi_region(&[note(60, 0.0, 0.5), note(62, 1.0, 0.5)]);
        let before = notes_of(&p, rid);
        for length in [None, Some(0.0), Some(-1.0), Some(f64::NAN)] {
            assert!(apply(&mut p, &lengths(NoteLengthMode::Fixed, rid, length, None, None)).is_err(), "{length:?}");
        }
        for factor in [None, Some(0.0), Some(f64::INFINITY)] {
            assert!(apply(&mut p, &lengths(NoteLengthMode::Scale, rid, None, factor, None)).is_err(), "{factor:?}");
        }
        assert_eq!(notes_of(&p, rid), before);
        let tid = p.tracks.iter().find(|t| t.kind == TrackKind::Instrument).unwrap().id;
        let pr = run(&mut p, Command::CreatePatternRegion { track_id: tid, start: 8.0, steps: None, step_length: None, rows: None, name: None });
        assert!(apply(&mut p, &lengths(NoteLengthMode::Legato, pr["region_id"].as_u64().unwrap(), None, None, None)).is_err());
        assert!(apply(&mut p, &lengths(NoteLengthMode::Legato, 9999, None, None, None)).is_err());
    }

    fn harmony(region_id: Id, interval: theory::HarmonyInterval, below: Option<bool>, indices: Option<Vec<usize>>) -> Command {
        Command::AddHarmony { region_id, interval, below, indices }
    }

    fn pitch_starts(p: &Project, rid: Id) -> Vec<(u8, f64)> {
        notes_of(p, rid).iter().map(|n| (n.pitch, n.start)).collect()
    }

    #[test]
    fn add_harmony_adds_thirds_in_c_major() {
        use theory::HarmonyInterval::Third;
        let start = [n(60, 0.0), n(64, 1.0), n(71, 2.0)];
        let (mut p, rid) = midi_region(&start);
        let v = run(&mut p, harmony(rid, Third, None, None));
        assert_eq!(v["added"], json!(3));
        assert_eq!(v["indices"], json!([1, 3, 5]));
        assert_eq!(v["note_count"], json!(6));
        assert_eq!(v["already_present"], json!(0));
        assert_eq!(v["out_of_range"], json!(0));
        let after = notes_of(&p, rid);
        for (i, (pitch, at)) in [(1, (64, 0.0)), (3, (67, 1.0)), (5, (74, 2.0))] {
            assert_eq!(after[i], Note { pitch, velocity: 85, start: at, length: 0.5 });
        }
        assert_eq!(vec![after[0], after[2], after[4]], start.to_vec());
    }

    #[test]
    fn add_harmony_below_and_octaves_stay_in_the_midi_range() {
        use theory::HarmonyInterval::{Octave, Sixth};
        let start = [n(2, 0.0), n(60, 1.0), n(127, 2.0)];
        let (mut p, rid) = midi_region(&start);
        let v = run(&mut p, harmony(rid, Sixth, Some(true), None));
        assert_eq!(v["added"], json!(2));
        assert_eq!(v["out_of_range"], json!(1));
        // G9 minus a sixth in C major is B8
        assert_eq!(pitch_starts(&p, rid), vec![(2, 0.0), (52, 1.0), (60, 1.0), (119, 2.0), (127, 2.0)]);

        let (mut p, rid) = midi_region(&start);
        let v = run(&mut p, harmony(rid, Octave, None, None));
        assert_eq!(v["added"], json!(2));
        assert_eq!(v["out_of_range"], json!(1));
        assert_eq!(pitch_starts(&p, rid), vec![(2, 0.0), (14, 0.0), (60, 1.0), (72, 1.0), (127, 2.0)]);

        let (mut p, rid) = midi_region(&start);
        let v = run(&mut p, harmony(rid, Octave, Some(true), None));
        assert_eq!(v["added"], json!(2));
        assert_eq!(v["out_of_range"], json!(1));
        assert_eq!(pitch_starts(&p, rid), vec![(2, 0.0), (48, 1.0), (60, 1.0), (115, 2.0), (127, 2.0)]);
    }

    #[test]
    fn add_harmony_in_a_minor_starts_off_key_notes_from_the_nearest_degree() {
        let (mut p, rid) = midi_region(&[n(69, 0.0), n(68, 1.0)]);
        p.key = parse_key("A minor").unwrap();
        let v = run(&mut p, harmony(rid, theory::HarmonyInterval::Third, None, None));
        assert_eq!(v["indices"], json!([1, 3]));
        // G#4 is outside A minor and starts from G4
        assert_eq!(pitch_starts(&p, rid), vec![(69, 0.0), (72, 0.0), (68, 1.0), (71, 1.0)]);
    }

    #[test]
    fn add_harmony_limits_to_indices_and_skips_existing_notes() {
        use theory::HarmonyInterval::Third;
        let (mut p, rid) = midi_region(&[n(60, 0.0), n(64, 0.0), n(62, 1.0)]);
        let v = run(&mut p, harmony(rid, Third, None, Some(vec![0, 1, 99])));
        // the copy of 60 is 64@0, which is already there; 62 is not among the indices
        assert_eq!(v["added"], json!(1));
        assert_eq!(v["already_present"], json!(1));
        assert_eq!(v["indices"], json!([2]));
        assert_eq!(v["note_count"], json!(4));
        assert_eq!(pitch_starts(&p, rid), vec![(60, 0.0), (64, 0.0), (67, 0.0), (62, 1.0)]);
        let before = notes_of(&p, rid);
        let v = run(&mut p, harmony(rid, Third, None, Some(vec![0, 1, 99])));
        assert_eq!(v["added"], json!(0));
        assert_eq!(v["indices"], json!([]));
        assert_eq!(v["already_present"], json!(2));
        assert_eq!(notes_of(&p, rid), before);
        // two notes whose copies coincide: C# starts from C in C major, so both give 64@0
        let (mut q, rid2) = midi_region(&[n(60, 0.0), n(61, 0.0)]);
        let v = run(&mut q, harmony(rid2, Third, None, None));
        assert_eq!(v["added"], json!(1));
        assert_eq!(v["already_present"], json!(1));
        let v = run(&mut q, harmony(rid2, Third, Some(true), Some(vec![])));
        assert_eq!(v["added"], json!(0));
        assert_eq!(v["indices"], json!([]));
        // not a MIDI region, or no such region
        let tid = q.tracks.iter().find(|t| t.kind == TrackKind::Instrument).unwrap().id;
        let pr = run(&mut q, Command::CreatePatternRegion { track_id: tid, start: 8.0, steps: None, step_length: None, rows: None, name: None });
        assert!(apply(&mut q, &harmony(pr["region_id"].as_u64().unwrap(), Third, None, None)).is_err());
        assert!(apply(&mut q, &harmony(9999, Third, None, None)).is_err());
    }

    #[test]
    fn note_tools_without_indices_are_unchanged() {
        let start = [n(60, 0.1), n(62, 0.8), n(64, 0.9), n(65, 0.95)];
        let (mut p, rid) = midi_region(&start);
        let v = run(&mut p, Command::QuantizeNotes { region_id: rid, grid: 0.5, strength: Some(0.8), swing: Some(0.3), quantize_ends: Some(true), indices: None });
        assert_eq!(v, json!({"ok": true}));
        let mut expect = start.to_vec();
        generate::quantize(&mut expect, 0.5, 0.8, 0.3, true);
        sanitize_notes(&mut expect);
        assert_eq!(notes_of(&p, rid), expect);

        let before = notes_of(&p, rid);
        let v = run(&mut p, Command::HumanizeNotes { region_id: rid, timing: Some(0.05), velocity: Some(10), seed: Some(3), indices: None });
        assert_eq!(v, json!({"ok": true}));
        let mut expect = before;
        generate::humanize(&mut expect, 0.05, 10, 3);
        sanitize_notes(&mut expect);
        assert_eq!(notes_of(&p, rid), expect);

        let (mut q, rid2) = midi_region(&[n(61, 0.0), n(62, 1.0), n(63, 2.0), n(66, 3.0)]);
        let v = run(&mut q, Command::SnapNotesToKey { region_id: rid2, indices: None });
        assert_eq!(v, json!({"changed": 3}));
        let pitches: Vec<u8> = notes_of(&q, rid2).iter().map(|n| n.pitch).collect();
        assert_eq!(pitches, vec![60, 62, 62, 65]);
    }

    #[test]
    fn selected_notes_context_lists_pitch_and_position() {
        let notes = [
            Note { pitch: 60, velocity: 100, start: 0.0, length: 1.0 },
            Note { pitch: 62, velocity: 90, start: 1.0, length: 0.5 },
            Note { pitch: 64, velocity: 80, start: 1.5, length: 0.25 },
            Note { pitch: 67, velocity: 70, start: 2.333333, length: 0.5 },
        ];
        assert_eq!(
            selected_notes_context(&notes, &[3, 0, 9], 32),
            " Selected notes (2 of 4; index:pitch@start+length velocity, beats from the region start): 0:C4@0+1 v100, 3:G4@2.333+0.5 v70."
        );
        assert_eq!(selected_notes_context(&notes, &[9], 32), "");
        let many: Vec<Note> = (0..40).map(|i| Note { pitch: 48 + (i % 24) as u8, velocity: 100, start: i as f64 * 0.25, length: 0.25 }).collect();
        let all: Vec<usize> = (0..40).collect();
        let s = selected_notes_context(&many, &all, 32);
        assert!(s.starts_with(" Selected notes (40 of 40;"), "{s}");
        assert!(s.contains(" 31:") && !s.contains(" 32:"), "{s}");
        assert!(s.ends_with(", +8 more."), "{s}");
    }

    #[test]
    fn selection_summary_respects_the_limit_and_skips_missing_ids() {
        let mut p = Project::new("t");
        let tid = add_track_named(&mut p, TrackKind::Instrument, "Keys");
        let notes: Vec<Note> = (0..5).map(|i| Note { pitch: 60 + i as u8, velocity: 100, start: i as f64, length: 1.0 }).collect();
        let rid = run(&mut p, Command::CreateMidiRegion { track_id: tid, start: 0.0, length: 8.0, name: None, notes: Some(notes) })["region_id"].as_u64().unwrap();
        let v = selection_summary(&p, Some(tid), Some(rid), &[4, 1, 1, 7], 1);
        assert_eq!(v["track"]["id"], json!(tid), "{v}");
        assert_eq!(v["region"]["track_id"], json!(tid), "{v}");
        assert_eq!(v["notes"].as_array().unwrap().len(), 1, "{v}");
        assert_eq!(v["notes"][0]["index"], json!(1), "{v}");
        assert_eq!(v["notes"][0]["pitch"], json!(61), "{v}");
        assert_eq!(v["selected_note_count"], json!(2), "{v}");
        let v = selection_summary(&p, Some(12345), Some(12345), &[0], 256);
        assert!(v["track"].is_null() && v["region"].is_null(), "{v}");
        assert_eq!(v["notes"], json!([]), "{v}");
        assert_eq!(v["selected_note_count"], json!(0), "{v}");
    }

    fn add_track_named(p: &mut Project, kind: TrackKind, name: &str) -> Id {
        run(p, Command::AddTrack { kind, name: Some(name.into()), patch: None, instrument: None, preset: None, index: None })["track_id"].as_u64().unwrap()
    }

    fn set_lane(p: &mut Project, track_id: Id, target: &str, value: f32) {
        run(p, Command::SetAutomation { track_id, target: target.into(), points: vec![AutomationPoint { beat: 0.0, value }] });
    }

    /// (target, first point value) of every lane of a track, in order.
    fn lanes_of(p: &Project, track_id: Id) -> Vec<(String, f32)> {
        p.track(track_id).unwrap().automation.iter().map(|l| (l.target.clone(), l.points.first().map_or(f32::NAN, |x| x.value))).collect()
    }

    fn lane(target: &str, value: f32) -> (String, f32) {
        (target.to_string(), value)
    }

    #[test]
    fn remap_send_lanes_renames_drops_and_keeps_out_of_range() {
        let mk = |target: &str, value: f32| AutomationLane { target: target.into(), points: vec![AutomationPoint { beat: 0.0, value }], enabled: true };
        let mut lanes = vec![mk("volume", 1.0), mk("send:0", 2.0), mk("send:1", 3.0), mk("send:2", 4.0), mk("send:7", 5.0), mk("send:x", 6.0), mk("plugin:5:mix", 7.0)];
        assert_eq!(remap_send_lanes(&mut lanes, &[Some(0), None, Some(1)]), 1);
        let got: Vec<(String, f32)> = lanes.iter().map(|l| (l.target.clone(), l.points[0].value)).collect();
        assert_eq!(got, vec![lane("volume", 1.0), lane("send:0", 2.0), lane("send:1", 4.0), lane("send:7", 5.0), lane("send:x", 6.0), lane("plugin:5:mix", 7.0)]);
    }

    #[test]
    fn remove_send_renumbers_later_send_lanes() {
        let mut p = Project::new("t");
        let src = add_track_named(&mut p, TrackKind::Instrument, "src");
        let a = add_track_named(&mut p, TrackKind::Bus, "A");
        let b = add_track_named(&mut p, TrackKind::Bus, "B");
        run(&mut p, Command::AddSend { track_id: src, bus_id: a, level_db: None, pre_fader: None });
        run(&mut p, Command::AddSend { track_id: src, bus_id: b, level_db: None, pre_fader: None });
        set_lane(&mut p, src, "volume", -6.0);
        set_lane(&mut p, src, "send:0", -1.0);
        set_lane(&mut p, src, "send:1", -2.0);
        let mut last = p.clone();
        let r = run(&mut p, Command::RemoveSend { track_id: src, send_index: 0 });
        assert_eq!(r, json!({"ok": true, "removed_automation_lanes": 1}));
        let sends = &p.track(src).unwrap().channel.sends;
        assert_eq!(sends.len(), 1);
        assert_eq!(sends[0].bus_id, b);
        assert_eq!(lanes_of(&p, src), vec![lane("volume", -6.0), lane("send:0", -2.0)]);

        // removing the last send leaves the earlier lanes alone
        let r = run(&mut last, Command::RemoveSend { track_id: src, send_index: 1 });
        assert_eq!(r, json!({"ok": true, "removed_automation_lanes": 1}));
        assert_eq!(lanes_of(&last, src), vec![lane("volume", -6.0), lane("send:0", -1.0)]);
        assert_eq!(last.track(src).unwrap().channel.sends[0].bus_id, a);
        assert!(apply(&mut last, &Command::RemoveSend { track_id: src, send_index: 1 }).is_err());
    }

    #[test]
    fn remove_track_renumbers_send_lanes_of_other_tracks() {
        let mut p = Project::new("t");
        let src = add_track_named(&mut p, TrackKind::Instrument, "src");
        let other = add_track_named(&mut p, TrackKind::Instrument, "other");
        let a = add_track_named(&mut p, TrackKind::Bus, "A");
        let b = add_track_named(&mut p, TrackKind::Bus, "B");
        let c = add_track_named(&mut p, TrackKind::Bus, "C");
        for bus in [a, b, c] {
            run(&mut p, Command::AddSend { track_id: src, bus_id: bus, level_db: None, pre_fader: None });
        }
        run(&mut p, Command::AddSend { track_id: other, bus_id: c, level_db: None, pre_fader: None });
        set_lane(&mut p, src, "send:0", -1.0);
        set_lane(&mut p, src, "send:1", -2.0);
        set_lane(&mut p, src, "send:2", -3.0);
        set_lane(&mut p, other, "send:0", -4.0);
        let routed = add_track_named(&mut p, TrackKind::Audio, "routed");
        p.track_mut(routed).unwrap().channel.output = Output::Bus(b);

        let r = run(&mut p, Command::RemoveTrack { track_id: b });
        assert_eq!(r["ok"], json!(true));
        assert_eq!(r["removed_automation_lanes"], json!(1));
        assert_eq!(r["removed_live_cells"], json!(0));
        assert_eq!(r["removed_midi_routes"], json!(0));
        let buses: Vec<Id> = p.track(src).unwrap().channel.sends.iter().map(|s| s.bus_id).collect();
        assert_eq!(buses, vec![a, c]);
        assert_eq!(lanes_of(&p, src), vec![lane("send:0", -1.0), lane("send:1", -3.0)]);
        assert_eq!(lanes_of(&p, other), vec![lane("send:0", -4.0)]);
        assert_eq!(p.track(other).unwrap().channel.sends[0].bus_id, c);
        assert_eq!(p.track(routed).unwrap().channel.output, Output::Master);
    }

    #[test]
    fn remove_plugin_drops_only_its_lanes() {
        let mut p = Project::new("t");
        let src = add_track_named(&mut p, TrackKind::Instrument, "src");
        let other = add_track_named(&mut p, TrackKind::Instrument, "other");
        let fx = |p: &mut Project, track_id: Id| run(p, Command::AddEffect { track_id, kind: "reverb".into(), preset: None, index: None })["plugin_id"].as_u64().unwrap();
        let fx1 = fx(&mut p, src);
        let fx2 = fx(&mut p, src);
        set_lane(&mut p, src, &format!("plugin:{fx1}:mix"), 0.1);
        set_lane(&mut p, src, &format!("plugin:{fx1}:size"), 0.2);
        set_lane(&mut p, src, &format!("plugin:{fx2}:mix"), 0.3);
        set_lane(&mut p, src, "volume", -6.0);
        set_lane(&mut p, other, &format!("plugin:{fx1}:mix"), 0.4);

        let r = run(&mut p, Command::RemovePlugin { plugin_id: fx1 });
        assert_eq!(r, json!({"ok": true, "removed_automation_lanes": 3}));
        assert_eq!(lanes_of(&p, src), vec![lane(&format!("plugin:{fx2}:mix"), 0.3), lane("volume", -6.0)]);
        assert!(lanes_of(&p, other).is_empty());
        assert_eq!(p.track(src).unwrap().channel.inserts.len(), 1);
        assert_eq!(apply(&mut p, &Command::RemovePlugin { plugin_id: 9999 }).err().as_deref(), Some("plug-in not found"));

        // an effect on the master, automated from a track
        let m = fx(&mut p, MASTER_ID);
        set_lane(&mut p, other, &format!("plugin:{m}:mix"), 0.5);
        let r = run(&mut p, Command::RemovePlugin { plugin_id: m });
        assert_eq!(r, json!({"ok": true, "removed_automation_lanes": 1}));
        assert!(p.master.inserts.is_empty());
        assert!(lanes_of(&p, other).is_empty());
    }

    #[test]
    fn remove_track_drops_its_live_cells_routes_and_plugin_lanes() {
        let mut p = Project::new("t");
        let t1 = add_track_named(&mut p, TrackKind::Instrument, "t1");
        let t2 = add_track_named(&mut p, TrackKind::Instrument, "t2");
        let cell = |p: &mut Project, track_id: Id, scene: usize| run(p, Command::CreateLiveCell { track_id, scene, length: Some(4.0), notes: Some(vec![]), from_region_id: None, name: None })["cell_id"].as_u64().unwrap();
        cell(&mut p, t1, 0);
        cell(&mut p, t1, 1);
        let kept_cell = cell(&mut p, t2, 0);
        let route = |name: &str, track_id: Id| MidiRoute { name: name.into(), channel: None, note_min: None, note_max: None, transpose: 0, velocity_scale: None, track_id };
        run(&mut p, Command::SetMidiRoutes { routes: vec![route("one", t1), route("two", t2)] });
        let fx = run(&mut p, Command::AddEffect { track_id: t1, kind: "reverb".into(), preset: None, index: None })["plugin_id"].as_u64().unwrap();
        set_lane(&mut p, t2, &format!("plugin:{fx}:mix"), 0.4);
        set_lane(&mut p, t1, "volume", -3.0);

        let r = run(&mut p, Command::RemoveTrack { track_id: t1 });
        assert_eq!(r, json!({"ok": true, "removed_automation_lanes": 1, "removed_live_cells": 2, "removed_midi_routes": 1}));
        let cells: Vec<Id> = p.live.cells.iter().map(|c| c.id).collect();
        assert_eq!(cells, vec![kept_cell]);
        assert_eq!(p.midi_routes, vec![route("two", t2)]);
        assert!(lanes_of(&p, t2).is_empty());
    }

    #[test]
    fn duplicate_track_copies_automation_onto_its_own_plugins() {
        let mut p = Project::new("t");
        let src = add_track_named(&mut p, TrackKind::Instrument, "src");
        let other = add_track_named(&mut p, TrackKind::Instrument, "other");
        let bus = add_track_named(&mut p, TrackKind::Bus, "bus");
        run(&mut p, Command::AddSend { track_id: src, bus_id: bus, level_db: None, pre_fader: None });
        let inst = p.track(src).unwrap().instrument.as_ref().unwrap().id;
        let arp = run(&mut p, Command::AddMidiEffect { track_id: src, kind: "arpeggiator".into(), preset: None })["plugin_id"].as_u64().unwrap();
        let filt = run(&mut p, Command::AddEffect { track_id: src, kind: "auto_filter".into(), preset: None, index: None })["plugin_id"].as_u64().unwrap();
        let m = run(&mut p, Command::AddEffect { track_id: MASTER_ID, kind: "reverb".into(), preset: None, index: None })["plugin_id"].as_u64().unwrap();
        set_lane(&mut p, src, "volume", -6.0);
        set_lane(&mut p, src, "pan", 0.5);
        set_lane(&mut p, src, "send:0", -12.0);
        let sweep = vec![AutomationPoint { beat: 0.0, value: 300.0 }, AutomationPoint { beat: 4.0, value: 5000.0 }];
        run(&mut p, Command::SetAutomation { track_id: src, target: format!("plugin:{filt}:cutoff"), points: sweep });
        set_lane(&mut p, src, &format!("plugin:{inst}:cutoff"), 800.0);
        set_lane(&mut p, src, &format!("plugin:{arp}:gate"), 0.3);
        set_lane(&mut p, src, &format!("plugin:{m}:mix"), 0.2);
        p.track_mut(src).unwrap().automation[1].enabled = false;
        set_lane(&mut p, other, &format!("plugin:{filt}:mix"), 0.4);
        let src_before = serde_json::to_value(p.track(src).unwrap()).unwrap();
        let other_before = serde_json::to_value(&p.track(other).unwrap().automation).unwrap();

        let r = run(&mut p, Command::DuplicateTrack { track_id: src });
        let copy = r["track_id"].as_u64().unwrap();
        assert_eq!(r, json!({"track_id": copy}));
        assert_eq!(p.track_index(copy), Some(p.track_index(src).unwrap() + 1));
        let t = p.track(copy).unwrap();
        let (inst2, arp2, filt2) = (t.instrument.as_ref().unwrap().id, t.midi_fx[0].id, t.channel.inserts[0].id);
        for (old, new) in [(inst, inst2), (arp, arp2), (filt, filt2)] {
            assert_ne!(old, new);
        }
        let targets: Vec<String> = t.automation.iter().map(|l| l.target.clone()).collect();
        let expected = ["volume".to_string(), "pan".into(), "send:0".into(), format!("plugin:{filt2}:cutoff"), format!("plugin:{inst2}:cutoff"), format!("plugin:{arp2}:gate"), format!("plugin:{m}:mix")];
        assert_eq!(targets, expected);
        // points and enabled flags are copied as they are
        let shape = |v: &Value| -> Vec<Value> { v.as_array().unwrap().iter().map(|l| json!([l["points"], l["enabled"]])).collect() };
        assert_eq!(shape(&serde_json::to_value(&t.automation).unwrap()), shape(&src_before["automation"]));
        assert_eq!(t.channel.sends[0].bus_id, bus);
        // the original and the lanes of the other track are untouched
        assert_eq!(serde_json::to_value(p.track(src).unwrap()).unwrap(), src_before);
        assert_eq!(serde_json::to_value(&p.track(other).unwrap().automation).unwrap(), other_before);
    }

    #[test]
    fn set_instrument_drops_lanes_of_the_old_instrument() {
        let mut p = Project::new("t");
        let src = add_track_named(&mut p, TrackKind::Instrument, "src");
        let other = add_track_named(&mut p, TrackKind::Instrument, "other");
        let inst = p.track(src).unwrap().instrument.as_ref().unwrap().id;
        let fx = run(&mut p, Command::AddEffect { track_id: src, kind: "reverb".into(), preset: None, index: None })["plugin_id"].as_u64().unwrap();
        set_lane(&mut p, src, "volume", -6.0);
        set_lane(&mut p, src, &format!("plugin:{inst}:cutoff"), 800.0);
        set_lane(&mut p, src, &format!("plugin:{fx}:mix"), 0.3);
        set_lane(&mut p, other, &format!("plugin:{inst}:cutoff"), 900.0);
        let kept = vec![lane("volume", -6.0), lane(&format!("plugin:{fx}:mix"), 0.3)];

        // errors leave everything alone
        assert_eq!(apply(&mut p, &Command::SetInstrument { track_id: src, kind: "reverb".into(), preset: None }).err().as_deref(), Some("'reverb' is not an instrument"));
        let audio = add_track_named(&mut p, TrackKind::Audio, "audio");
        assert_eq!(apply(&mut p, &Command::SetInstrument { track_id: audio, kind: "organ".into(), preset: None }).err().as_deref(), Some("not an instrument track"));
        assert_eq!(lanes_of(&p, src).len(), 3);
        assert_eq!(lanes_of(&p, other).len(), 1);

        let r = run(&mut p, Command::SetInstrument { track_id: src, kind: "fm_synth".into(), preset: None });
        let new = r["plugin_id"].as_u64().unwrap();
        assert_eq!(r, json!({"plugin_id": new, "removed_automation_lanes": 2}));
        assert_eq!(lanes_of(&p, src), kept);
        assert!(lanes_of(&p, other).is_empty());

        // a track without an instrument has nothing to remove
        run(&mut p, Command::RemovePlugin { plugin_id: new });
        let r = run(&mut p, Command::SetInstrument { track_id: src, kind: "organ".into(), preset: None });
        assert_eq!(r["removed_automation_lanes"], json!(0));
        assert_eq!(lanes_of(&p, src), kept);
    }

    #[test]
    fn apply_patch_drops_lanes_of_the_replaced_plugins() {
        let mut p = Project::new("t");
        let src = add_track_named(&mut p, TrackKind::Instrument, "src");
        let other = add_track_named(&mut p, TrackKind::Instrument, "other");
        let inst = p.track(src).unwrap().instrument.as_ref().unwrap().id;
        let arp = run(&mut p, Command::AddMidiEffect { track_id: src, kind: "arpeggiator".into(), preset: None })["plugin_id"].as_u64().unwrap();
        let fx = run(&mut p, Command::AddEffect { track_id: src, kind: "reverb".into(), preset: None, index: None })["plugin_id"].as_u64().unwrap();
        set_lane(&mut p, src, "volume", -6.0);
        set_lane(&mut p, src, "pan", 0.5);
        set_lane(&mut p, src, &format!("plugin:{inst}:cutoff"), 800.0);
        set_lane(&mut p, src, &format!("plugin:{fx}:mix"), 0.3);
        set_lane(&mut p, src, &format!("plugin:{arp}:gate"), 0.3);
        set_lane(&mut p, other, &format!("plugin:{fx}:mix"), 0.4);
        set_lane(&mut p, other, "volume", -3.0);
        let mut audio_patch = p.clone();

        let r = run(&mut p, Command::ApplyPatch { track_id: src, patch: "Lush Pad".into() });
        assert_eq!(r["id"], json!(src));
        assert_eq!(r["removed_automation_lanes"], json!(3));
        assert_ne!(p.track(src).unwrap().instrument.as_ref().unwrap().id, inst);
        assert_eq!(lanes_of(&p, src), vec![lane("volume", -6.0), lane("pan", 0.5), lane(&format!("plugin:{arp}:gate"), 0.3)]);
        assert_eq!(lanes_of(&p, other), vec![lane("volume", -3.0)]);

        // a patch without an instrument keeps the instrument and its lanes; only the inserts go
        let r = run(&mut audio_patch, Command::ApplyPatch { track_id: src, patch: "Clean Audio".into() });
        assert_eq!(r["removed_automation_lanes"], json!(2));
        assert_eq!(audio_patch.track(src).unwrap().instrument.as_ref().unwrap().id, inst);
        assert_eq!(lanes_of(&audio_patch, src), vec![lane("volume", -6.0), lane("pan", 0.5), lane(&format!("plugin:{inst}:cutoff"), 800.0), lane(&format!("plugin:{arp}:gate"), 0.3)]);
    }

    fn cc(beat: f64, controller: u8, value: u8) -> CcEvent {
        CcEvent { beat, controller, value }
    }

    fn cc_of(p: &Project, rid: Id) -> Vec<CcEvent> {
        match &p.find_region(rid).unwrap().1.content {
            RegionContent::Midi(m) => m.cc.clone(),
            _ => panic!("not a MIDI region"),
        }
    }

    #[test]
    fn replace_cc_events_keeps_other_controllers_and_sorts() {
        let mut v = vec![cc(0.0, 64, 127), cc(0.5, 1, 10), cc(1.0, 64, 0), cc(1.5, 1, 50), cc(2.0, 64, 127), cc(3.0, 64, 0)];
        let pts = [CcPoint { beat: 2.5, value: 0 }, CcPoint { beat: 1.25, value: 127 }];
        let (removed, added) = replace_cc_events(&mut v, 64, 1.0, 3.0, &pts);
        assert_eq!((removed, added), (2, 2));
        // the event at `from` goes, the one at `to` stays
        assert_eq!(v, vec![cc(0.0, 64, 127), cc(0.5, 1, 10), cc(1.25, 64, 127), cc(1.5, 1, 50), cc(2.5, 64, 0), cc(3.0, 64, 0)]);
        assert!(v.windows(2).all(|w| w[0].beat <= w[1].beat));
    }

    #[test]
    fn replace_cc_events_with_open_range_clears_the_controller() {
        let mut v = vec![cc(0.0, 1, 10), cc(1.0, 64, 127), cc(20.0, 1, 90)];
        let (removed, added) = replace_cc_events(&mut v, 1, f64::NEG_INFINITY, f64::INFINITY, &[]);
        assert_eq!((removed, added), (2, 0));
        assert_eq!(v, vec![cc(1.0, 64, 127)]);
    }

    #[test]
    fn replace_cc_command_checks_its_arguments() {
        let (mut p, rid) = midi_region(&[]);
        run(&mut p, Command::AddCc { region_id: rid, events: vec![cc(0.0, 64, 127), cc(1.0, 64, 0)] });
        let before = cc_of(&p, rid);
        let pt = |beat: f64, value: u8| vec![CcPoint { beat, value }];
        let bad = [
            (129, None, None, pt(0.0, 1)),
            (64, None, None, pt(0.0, 128)),
            (64, Some(3.0), Some(1.0), pt(0.0, 1)),
            (64, Some(2.0), Some(2.0), pt(0.0, 1)),
            (64, Some(f64::NAN), None, pt(0.0, 1)),
            (64, None, None, pt(-1.0, 1)),
            (64, None, None, pt(f64::NAN, 1)),
        ];
        for (controller, from_beat, to_beat, events) in bad {
            let c = Command::ReplaceCc { region_id: rid, controller, from_beat, to_beat, events };
            assert!(apply(&mut p, &c).is_err(), "{c:?} should fail");
            assert_eq!(cc_of(&p, rid), before);
        }
        let tid = p.find_region(rid).unwrap().0.id;
        let pr = run(&mut p, Command::CreatePatternRegion { track_id: tid, start: 8.0, steps: None, step_length: None, rows: None, name: None });
        let prid = pr["region_id"].as_u64().unwrap();
        let c = Command::ReplaceCc { region_id: prid, controller: 64, from_beat: None, to_beat: None, events: vec![] };
        assert!(apply(&mut p, &c).is_err());
        let c = Command::ReplaceCc { region_id: 9999, controller: 64, from_beat: None, to_beat: None, events: vec![] };
        assert!(apply(&mut p, &c).is_err());
        assert_eq!(cc_of(&p, rid), before);

        let v = run(&mut p, Command::ReplaceCc { region_id: rid, controller: 64, from_beat: Some(1.0), to_beat: None, events: pt(1.5, 0) });
        assert_eq!(v, json!({"removed": 1, "added": 1, "cc_count": 2}));
        assert_eq!(cc_of(&p, rid), vec![cc(0.0, 64, 127), cc(1.5, 64, 0)]);
    }

    /// A Drums instrument track with a default 16-step pattern; returns the project and the region id.
    fn drum_pattern() -> (Project, Id) {
        let mut p = Project::new("t");
        let tid = add_track_named(&mut p, TrackKind::Instrument, "Drums");
        let v = run(&mut p, Command::CreatePatternRegion { track_id: tid, start: 0.0, steps: Some(16), step_length: None, rows: None, name: None });
        (p, v["region_id"].as_u64().unwrap())
    }

    fn pattern_of(p: &Project, rid: Id) -> PatternContent {
        match &p.find_region(rid).unwrap().1.content {
            RegionContent::Pattern(pt) => pt.clone(),
            _ => panic!("not a pattern"),
        }
    }

    /// (name, pitch, steps, muted) of every row.
    fn row_fields(p: &Project, rid: Id) -> Vec<(String, u8, Vec<u8>, bool)> {
        pattern_of(p, rid).rows.iter().map(|r| (r.name.clone(), r.pitch, r.steps.clone(), r.muted)).collect()
    }

    fn update_row(region_id: Id, row: usize, name: Option<&str>, pitch: Option<u8>, muted: Option<bool>) -> Command {
        Command::UpdatePatternRow { region_id, row, name: name.map(String::from), pitch, muted }
    }

    /// Kick, Closed Hat and Open Hat with different steps.
    fn fill_rows(p: &mut Project, rid: Id) {
        for (row, pattern) in [(0, "x...x...x...x..."), (3, "o.o.o.o.o.o.o.o."), (4, "..x...x...x...x.")] {
            run(p, Command::SetPatternRow { region_id: rid, row, pattern: pattern.into() });
        }
    }

    #[test]
    fn update_pattern_row_changes_only_the_fields_passed() {
        let (mut p, rid) = drum_pattern();
        fill_rows(&mut p, rid);
        let before = row_fields(&p, rid);

        let v = run(&mut p, update_row(rid, 3, None, None, Some(true)));
        assert_eq!(v, json!({"row": 3, "name": "Closed Hat", "pitch": 42, "muted": true}));
        let mut expected = before.clone();
        expected[3].3 = true;
        assert_eq!(row_fields(&p, rid), expected);

        run(&mut p, update_row(rid, 3, Some("  Hats "), None, None));
        expected[3].0 = "Hats".into();
        assert_eq!(row_fields(&p, rid), expected);

        run(&mut p, update_row(rid, 3, None, Some(44), None));
        expected[3].1 = 44;
        assert_eq!(row_fields(&p, rid), expected);

        let v = run(&mut p, update_row(rid, 0, Some("Big Kick"), Some(35), Some(true)));
        assert_eq!(v, json!({"row": 0, "name": "Big Kick", "pitch": 35, "muted": true}));
        expected[0] = ("Big Kick".into(), 35, before[0].2.clone(), true);
        assert_eq!(row_fields(&p, rid), expected);
        for (after, b) in row_fields(&p, rid).iter().zip(&before) {
            assert_eq!(after.2, b.2, "the steps never change");
        }
    }

    #[test]
    fn muted_pattern_row_is_left_out_of_the_notes() {
        let (mut p, rid) = drum_pattern();
        fill_rows(&mut p, rid);
        let notes = pattern_to_notes(&pattern_of(&p, rid));
        assert!(notes.iter().any(|n| n.pitch == 36) && notes.iter().any(|n| n.pitch == 42));

        run(&mut p, update_row(rid, 3, None, None, Some(true)));
        let muted = pattern_to_notes(&pattern_of(&p, rid));
        assert!(muted.iter().any(|n| n.pitch == 36));
        assert!(!muted.iter().any(|n| n.pitch == 42));

        run(&mut p, update_row(rid, 3, None, None, Some(false)));
        let back = pattern_to_notes(&pattern_of(&p, rid));
        assert!(back.iter().any(|n| n.pitch == 36) && back.iter().any(|n| n.pitch == 42));
        assert_eq!(back.len(), notes.len());
    }

    #[test]
    fn pattern_row_commands_reject_bad_arguments() {
        let (mut p, rid) = drum_pattern();
        fill_rows(&mut p, rid);
        let tid = p.find_region(rid).unwrap().0.id;
        let midi = run(&mut p, Command::CreateMidiRegion { track_id: tid, start: 8.0, length: 4.0, name: None, notes: None })["region_id"].as_u64().unwrap();
        let before = row_fields(&p, rid);
        let error = |p: &mut Project, c: Command| match apply(p, &c) {
            Err(e) => e,
            Ok(_) => panic!("{c:?} should fail"),
        };

        assert_eq!(error(&mut p, update_row(rid, 8, None, None, Some(true))), "row 8 not found: the pattern has 8 rows");
        assert_eq!(error(&mut p, Command::RemovePatternRow { region_id: rid, row: 8 }), "row 8 not found: the pattern has 8 rows");
        assert!(error(&mut p, update_row(rid, 0, None, Some(128), None)).contains("pitch 128"));
        assert!(error(&mut p, update_row(rid, 0, Some("   "), None, None)).contains("name"));
        assert!(error(&mut p, update_row(rid, 0, None, None, None)).contains("nothing to change"));
        assert!(error(&mut p, update_row(midi, 0, None, None, Some(true))).contains("not a pattern"));
        assert!(error(&mut p, Command::RemovePatternRow { region_id: midi, row: 0 }).contains("not a pattern"));
        error(&mut p, update_row(9999, 0, None, None, Some(true)));
        error(&mut p, Command::RemovePatternRow { region_id: 9999, row: 0 });
        assert_eq!(row_fields(&p, rid), before);
    }

    #[test]
    fn remove_pattern_row_moves_the_later_rows_up() {
        let (mut p, rid) = drum_pattern();
        fill_rows(&mut p, rid);
        let before = row_fields(&p, rid);
        let shape = |p: &Project| {
            let pt = pattern_of(p, rid);
            (pt.steps, pt.step_length, pt.swing, p.find_region(rid).unwrap().1.length)
        };
        let shape_before = shape(&p);

        let v = run(&mut p, Command::RemovePatternRow { region_id: rid, row: 3 });
        assert_eq!(v, json!({"removed": {"name": "Closed Hat", "pitch": 42}, "rows": 7}));
        let after = row_fields(&p, rid);
        assert_eq!(after.len(), 7);
        assert_eq!(after[..3], before[..3]);
        assert_eq!(after[3..], before[4..]);
        assert_eq!(after[3].0, "Open Hat");
        assert_eq!(shape(&p), shape_before);
    }

    #[test]
    fn remove_pattern_row_can_empty_the_pattern() {
        let (mut p, rid) = drum_pattern();
        fill_rows(&mut p, rid);
        for _ in 0..8 {
            run(&mut p, Command::RemovePatternRow { region_id: rid, row: 0 });
        }
        let pt = pattern_of(&p, rid);
        assert!(pt.rows.is_empty());
        assert_eq!(pt.steps, 16);
        assert!(pattern_to_notes(&pt).is_empty());
        assert!(matches!(p.find_region(rid).unwrap().1.content, RegionContent::Pattern(_)));
        match apply(&mut p, &Command::RemovePatternRow { region_id: rid, row: 0 }) {
            Err(e) => assert_eq!(e, "row 0 not found: the pattern has 0 rows"),
            Ok(_) => panic!("an empty pattern has no row to remove"),
        }
        let v = run(&mut p, Command::AddPatternRow { region_id: rid, name: "Kick".into(), pitch: 36 });
        assert_eq!(v, json!({"row": 0}));
        assert_eq!(pattern_of(&p, rid).rows[0].steps, vec![0; 16]);
    }
}
