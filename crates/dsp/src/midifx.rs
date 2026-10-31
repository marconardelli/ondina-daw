//! MIDI effects, applied to note events before the instrument.

use std::collections::HashMap;

use ondina_core::theory::Key;

use crate::{MidiEffect, MidiEvent, MidiKind, ProcessContext};

// ============================================================================ Transposer

#[derive(Default)]
pub struct Transposer {
    semis: i32,
    lock: bool,
    map: HashMap<u8, u8>,
}

impl MidiEffect for Transposer {
    fn set_param(&mut self, id: &str, v: f32) {
        match id {
            "semitones" => self.semis = v.round() as i32,
            "scale_lock" => self.lock = v >= 0.5,
            _ => {}
        }
    }
    fn process(&mut self, events: &mut Vec<MidiEvent>, _frames: usize, _ctx: &ProcessContext, key: Key) {
        for e in events.iter_mut() {
            match &mut e.kind {
                MidiKind::NoteOn { pitch, .. } => {
                    let mut p = (*pitch as i32 + self.semis).clamp(0, 127) as u8;
                    if self.lock {
                        p = key.snap(p);
                    }
                    self.map.insert(*pitch, p);
                    *pitch = p;
                }
                MidiKind::NoteOff { pitch } => {
                    if let Some(p) = self.map.remove(pitch) {
                        *pitch = p;
                    }
                }
                _ => {}
            }
        }
    }
    fn reset(&mut self) {
        self.map.clear();
    }
}

// ============================================================================ Chord trigger

#[derive(Default)]
pub struct ChordTrigger {
    chord: u8,
    map: HashMap<u8, Vec<u8>>,
}

impl ChordTrigger {
    fn intervals(&self, root: u8, key: Key) -> Vec<i32> {
        match self.chord {
            1 => vec![0, 3, 7],
            2 => vec![0, 7, 12],
            3 => vec![0, 4, 7, 11],
            4 => vec![0, 3, 7, 10],
            5 => vec![0, 4, 7, 10],
            6 => vec![0, 2, 7],
            7 => vec![0, 5, 7],
            8 => vec![0, 12],
            9 => {
                // diatonic triad built on the played degree
                let r = key.snap(root);
                let mut third = r as i32 + 3;
                while !key.contains(third.clamp(0, 127) as u8) {
                    third += 1;
                }
                let mut fifth = third + 3;
                while !key.contains(fifth.clamp(0, 127) as u8) {
                    fifth += 1;
                }
                vec![r as i32 - root as i32, third - root as i32, fifth - root as i32]
            }
            _ => vec![0, 4, 7],
        }
    }
}

impl MidiEffect for ChordTrigger {
    fn set_param(&mut self, id: &str, v: f32) {
        if id == "chord" {
            self.chord = v.round() as u8;
        }
    }
    fn process(&mut self, events: &mut Vec<MidiEvent>, _frames: usize, _ctx: &ProcessContext, key: Key) {
        let mut out = Vec::with_capacity(events.len() * 3);
        for e in events.drain(..) {
            match e.kind {
                MidiKind::NoteOn { pitch, velocity } => {
                    let notes: Vec<u8> = self.intervals(pitch, key).iter().map(|i| (pitch as i32 + i).clamp(0, 127) as u8).collect();
                    for n in &notes {
                        out.push(MidiEvent { offset: e.offset, kind: MidiKind::NoteOn { pitch: *n, velocity } });
                    }
                    self.map.insert(pitch, notes);
                }
                MidiKind::NoteOff { pitch } => {
                    for n in self.map.remove(&pitch).unwrap_or_else(|| vec![pitch]) {
                        out.push(MidiEvent { offset: e.offset, kind: MidiKind::NoteOff { pitch: n } });
                    }
                }
                _ => out.push(e),
            }
        }
        *events = out;
    }
    fn reset(&mut self) {
        self.map.clear();
    }
}

// ============================================================================ Velocity processor

pub struct VelocityProcessor {
    scale: f32,
    offset: f32,
    random: f32,
    rng: u32,
}

impl Default for VelocityProcessor {
    fn default() -> Self {
        VelocityProcessor { scale: 1.0, offset: 0.0, random: 0.0, rng: 99991 }
    }
}

impl MidiEffect for VelocityProcessor {
    fn set_param(&mut self, id: &str, v: f32) {
        match id {
            "scale" => self.scale = v,
            "offset" => self.offset = v,
            "random" => self.random = v,
            _ => {}
        }
    }
    fn process(&mut self, events: &mut Vec<MidiEvent>, _frames: usize, _ctx: &ProcessContext, _key: Key) {
        for e in events.iter_mut() {
            if let MidiKind::NoteOn { velocity, .. } = &mut e.kind {
                self.rng ^= self.rng << 13;
                self.rng ^= self.rng >> 17;
                self.rng ^= self.rng << 5;
                let rnd = (self.rng as f32 / u32::MAX as f32 * 2.0 - 1.0) * self.random;
                *velocity = (*velocity as f32 * self.scale + self.offset + rnd).round().clamp(1.0, 127.0) as u8;
            }
        }
    }
}

// ============================================================================ Arpeggiator

pub struct Arpeggiator {
    rate: f64,
    mode: u8,
    octaves: u32,
    gate: f64,
    held: Vec<(u8, u8)>,
    step: usize,
    sounding: Option<(u8, f64)>,
    free_beat: f64,
    rng: u32,
    last_step_index: i64,
}

impl Default for Arpeggiator {
    fn default() -> Self {
        Arpeggiator { rate: 0.25, mode: 0, octaves: 1, gate: 0.6, held: vec![], step: 0, sounding: None, free_beat: 0.0, rng: 31337, last_step_index: i64::MIN }
    }
}

impl Arpeggiator {
    fn sequence(&self) -> Vec<(u8, u8)> {
        let mut base = self.held.clone();
        if self.mode != 4 {
            base.sort_by_key(|x| x.0);
        }
        let mut seq = vec![];
        for o in 0..self.octaves.max(1) {
            for (p, v) in &base {
                let q = *p as u32 + o * 12;
                if q <= 127 {
                    seq.push((q as u8, *v));
                }
            }
        }
        match self.mode {
            1 => seq.reverse(),
            2 if seq.len() > 2 => {
                let mut down: Vec<_> = seq[1..seq.len() - 1].iter().rev().copied().collect();
                seq.append(&mut down);
            }
            _ => {}
        }
        seq
    }
}

impl MidiEffect for Arpeggiator {
    fn set_param(&mut self, id: &str, v: f32) {
        match id {
            "rate" => {
                self.rate = match v.round() as i32 {
                    0 => 1.0,
                    1 => 0.5,
                    2 => 0.25,
                    3 => 0.125,
                    4 => 1.0 / 3.0,
                    _ => 1.0 / 6.0,
                }
            }
            "mode" => self.mode = v.round() as u8,
            "octaves" => self.octaves = v.round().max(1.0) as u32,
            "gate" => self.gate = v as f64,
            _ => {}
        }
    }

    fn process(&mut self, events: &mut Vec<MidiEvent>, frames: usize, ctx: &ProcessContext, _key: Key) {
        let mut passthrough = vec![];
        for e in events.drain(..) {
            match e.kind {
                MidiKind::NoteOn { pitch, velocity } => {
                    if !self.held.iter().any(|h| h.0 == pitch) {
                        self.held.push((pitch, velocity));
                    }
                }
                MidiKind::NoteOff { pitch } => self.held.retain(|h| h.0 != pitch),
                _ => passthrough.push(e),
            }
        }
        let mut out = passthrough;
        let start_beat = if ctx.playing { ctx.beat } else { self.free_beat };
        let bps = ctx.beats_per_sample;
        if !ctx.playing {
            self.free_beat += bps * frames as f64;
        }
        for i in 0..frames {
            let b = start_beat + i as f64 * bps;
            if let Some((p, off)) = self.sounding {
                if b >= off {
                    out.push(MidiEvent { offset: i, kind: MidiKind::NoteOff { pitch: p } });
                    self.sounding = None;
                }
            }
            let step_index = (b / self.rate).floor() as i64;
            if step_index != self.last_step_index {
                self.last_step_index = step_index;
                if self.held.is_empty() {
                    self.step = 0;
                    continue;
                }
                let seq = self.sequence();
                let (p, v) = if self.mode == 3 {
                    self.rng ^= self.rng << 13;
                    self.rng ^= self.rng >> 17;
                    self.rng ^= self.rng << 5;
                    seq[self.rng as usize % seq.len()]
                } else {
                    seq[self.step % seq.len()]
                };
                self.step += 1;
                if let Some((sp, _)) = self.sounding.take() {
                    out.push(MidiEvent { offset: i, kind: MidiKind::NoteOff { pitch: sp } });
                }
                out.push(MidiEvent { offset: i, kind: MidiKind::NoteOn { pitch: p, velocity: v } });
                self.sounding = Some((p, b + self.rate * self.gate));
            }
        }
        if self.held.is_empty() {
            if let Some((p, _)) = self.sounding.take() {
                out.push(MidiEvent { offset: frames.saturating_sub(1), kind: MidiKind::NoteOff { pitch: p } });
            }
        }
        out.sort_by_key(|e| e.offset);
        *events = out;
    }

    fn reset(&mut self) {
        self.held.clear();
        self.sounding = None;
        self.step = 0;
        self.last_step_index = i64::MIN;
    }
}
