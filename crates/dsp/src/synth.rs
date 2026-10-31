//! Melodic instruments: Analog Synth, FM Synth, Tonewheel Organ, Plucked String, Sampler.

use std::sync::Arc;

use crate::util::*;
use crate::{AudioData, Instrument, ProcessContext};

const CONTROL_RATE: usize = 16;

/// Generic voice bookkeeping shared by the polyphonic instruments.
#[derive(Clone, Copy, Default)]
struct VoiceSlot {
    note: u8,
    gate: bool,
    age: u64,
    active: bool,
}

fn pick_voice(slots: &[VoiceSlot], note: u8) -> usize {
    // same note retrigger, then free voice, then oldest released, then oldest
    if let Some(i) = slots.iter().position(|s| s.active && s.note == note) {
        return i;
    }
    if let Some(i) = slots.iter().position(|s| !s.active) {
        return i;
    }
    if let Some((i, _)) = slots.iter().enumerate().filter(|(_, s)| !s.gate).min_by_key(|(_, s)| s.age) {
        return i;
    }
    slots.iter().enumerate().min_by_key(|(_, s)| s.age).map(|(i, _)| i).unwrap_or(0)
}

/// Sustain pedal helper.
#[derive(Default)]
struct Sustain {
    down: bool,
    held: Vec<u8>,
}

// ============================================================================ Analog Synth

#[derive(Clone)]
struct AnalogParams {
    w1: Wave,
    w2: Wave,
    semi2: f32,
    detune2: f32,
    mix: f32,
    sub: f32,
    noise: f32,
    pw: f32,
    spread: f32,
    fmode: FilterMode,
    cutoff: f32,
    res: f32,
    fenv: f32,
    keytrack: f32,
    fa: f32,
    fd: f32,
    fs: f32,
    fr: f32,
    a: f32,
    d: f32,
    s: f32,
    r: f32,
    lfo_rate: f32,
    lfo_pitch: f32,
    lfo_cut: f32,
    glide: f32,
    vel: f32,
    level: f32,
}

impl Default for AnalogParams {
    fn default() -> Self {
        AnalogParams {
            w1: Wave::Saw,
            w2: Wave::Saw,
            semi2: 0.0,
            detune2: 7.0,
            mix: 0.5,
            sub: 0.0,
            noise: 0.0,
            pw: 0.5,
            spread: 0.3,
            fmode: FilterMode::Low,
            cutoff: 2500.0,
            res: 0.2,
            fenv: 0.3,
            keytrack: 0.5,
            fa: 0.005,
            fd: 0.3,
            fs: 0.3,
            fr: 0.3,
            a: 0.005,
            d: 0.2,
            s: 0.8,
            r: 0.25,
            lfo_rate: 5.0,
            lfo_pitch: 0.0,
            lfo_cut: 0.0,
            glide: 0.0,
            vel: 0.7,
            level: db_to_gain(-8.0),
        }
    }
}

#[derive(Clone, Default)]
struct AnalogVoice {
    osc1: Osc,
    osc2: Osc,
    sub: Osc,
    filt: Svf,
    amp: Adsr,
    fenv: Adsr,
    note: f32,
    target_note: f32,
    velocity: f32,
    pan: f32,
}

pub struct AnalogSynth {
    sr: f32,
    p: AnalogParams,
    voices: Vec<AnalogVoice>,
    slots: Vec<VoiceSlot>,
    age: u64,
    lfo_phase: f32,
    noise: Noise,
    last_note: f32,
    sustain: Sustain,
    counter: usize,
}

impl AnalogSynth {
    pub fn new(sr: f32) -> Self {
        let n = 16;
        AnalogSynth { sr, p: AnalogParams::default(), voices: vec![AnalogVoice::default(); n], slots: vec![VoiceSlot::default(); n], age: 0, lfo_phase: 0.0, noise: Noise::new(12345), last_note: 60.0, sustain: Sustain::default(), counter: 0 }
    }
    fn update_envs(&mut self) {
        let (p, sr) = (&self.p, self.sr);
        for v in &mut self.voices {
            v.amp.set(p.a, p.d, p.s, p.r, sr);
            v.fenv.set(p.fa, p.fd, p.fs, p.fr, sr);
        }
    }
}

impl Instrument for AnalogSynth {
    fn set_param(&mut self, id: &str, v: f32) {
        let p = &mut self.p;
        match id {
            "osc1_wave" => p.w1 = Wave::from_index(v),
            "osc2_wave" => p.w2 = Wave::from_index(v),
            "osc2_semi" => p.semi2 = v,
            "osc2_detune" => p.detune2 = v,
            "osc_mix" => p.mix = v,
            "sub_level" => p.sub = v,
            "noise_level" => p.noise = v,
            "pulse_width" => p.pw = v,
            "unison_spread" => p.spread = v,
            "filter_type" => p.fmode = FilterMode::from_index(v),
            "cutoff" => p.cutoff = v,
            "resonance" => p.res = v,
            "filter_env" => p.fenv = v,
            "key_track" => p.keytrack = v,
            "f_attack" => p.fa = v,
            "f_decay" => p.fd = v,
            "f_sustain" => p.fs = v,
            "f_release" => p.fr = v,
            "attack" => p.a = v,
            "decay" => p.d = v,
            "sustain" => p.s = v,
            "release" => p.r = v,
            "lfo_rate" => p.lfo_rate = v,
            "lfo_pitch" => p.lfo_pitch = v,
            "lfo_cutoff" => p.lfo_cut = v,
            "glide" => p.glide = v,
            "velocity_sens" => p.vel = v,
            "level" => p.level = db_to_gain(v),
            _ => {}
        }
        self.update_envs();
    }

    fn note_on(&mut self, pitch: u8, velocity: u8) {
        let i = pick_voice(&self.slots, pitch);
        self.age += 1;
        self.slots[i] = VoiceSlot { note: pitch, gate: true, age: self.age, active: true };
        let v = &mut self.voices[i];
        let was_active = v.amp.is_active();
        v.target_note = pitch as f32;
        v.note = if self.p.glide > 0.0 { self.last_note } else { pitch as f32 };
        self.last_note = pitch as f32;
        v.velocity = 1.0 - self.p.vel + self.p.vel * velocity as f32 / 127.0;
        v.pan = if i % 2 == 0 { -1.0 } else { 1.0 } * ((i / 2) as f32 / 8.0 + 0.3).min(1.0);
        if !was_active {
            v.osc1.phase = 0.0;
            v.osc2.phase = 0.37;
            v.filt.reset();
        }
        v.amp.trigger();
        v.fenv.trigger();
    }

    fn note_off(&mut self, pitch: u8) {
        if self.sustain.down {
            self.sustain.held.push(pitch);
            return;
        }
        for (i, s) in self.slots.iter_mut().enumerate() {
            if s.active && s.gate && s.note == pitch {
                s.gate = false;
                self.voices[i].amp.release();
                self.voices[i].fenv.release();
            }
        }
    }

    fn control_change(&mut self, cc: u8, value: u8) {
        if cc == 64 {
            self.sustain.down = value >= 64;
            if !self.sustain.down {
                for n in std::mem::take(&mut self.sustain.held) {
                    self.note_off(n);
                }
            }
        }
    }

    fn all_notes_off(&mut self) {
        self.sustain = Sustain::default();
        for (i, s) in self.slots.iter_mut().enumerate() {
            s.gate = false;
            self.voices[i].amp.release();
            self.voices[i].fenv.release();
        }
    }

    fn reset(&mut self) {
        for (i, s) in self.slots.iter_mut().enumerate() {
            *s = VoiceSlot::default();
            self.voices[i].amp.kill();
            self.voices[i].fenv.kill();
        }
    }

    fn render(&mut self, left: &mut [f32], right: &mut [f32], _ctx: &ProcessContext) {
        let sr = self.sr;
        let p = self.p.clone();
        let glide_coef = if p.glide > 0.0 { 1.0 - (-1.0 / (p.glide * sr / 4.0)).exp() } else { 1.0 };
        let lfo_inc = p.lfo_rate / sr;
        for n in 0..left.len() {
            let lfo = (self.lfo_phase * std::f32::consts::TAU).sin();
            self.lfo_phase = (self.lfo_phase + lfo_inc).fract();
            let update = self.counter % CONTROL_RATE == 0;
            self.counter = self.counter.wrapping_add(1);
            let (mut l, mut r) = (0.0, 0.0);
            for (i, v) in self.voices.iter_mut().enumerate() {
                if !self.slots[i].active {
                    continue;
                }
                if !v.amp.is_active() {
                    self.slots[i].active = false;
                    continue;
                }
                v.note += (v.target_note - v.note) * glide_coef;
                let pitch = v.note + lfo * p.lfo_pitch * 1.0;
                let f1 = midi_to_freq(pitch);
                let f2 = midi_to_freq(pitch + p.semi2 + p.detune2 / 100.0);
                let fe = v.fenv.next();
                if update {
                    let oct = p.fenv * fe * 6.0 + (pitch - 60.0) / 12.0 * p.keytrack + lfo * p.lfo_cut * 2.0;
                    v.filt.set(p.cutoff * 2f32.powf(oct), p.res, sr);
                }
                let o1 = v.osc1.next(p.w1, f1 / sr, p.pw, &mut self.noise);
                let o2 = v.osc2.next(p.w2, f2 / sr, p.pw, &mut self.noise);
                let sub = v.sub.next(Wave::Square, f1 * 0.5 / sr, 0.5, &mut self.noise);
                let mut s = o1 * (1.0 - p.mix) + o2 * p.mix + sub * p.sub * 0.7 + self.noise.next() * p.noise * 0.5;
                s = v.filt.process(s * 0.5, p.fmode);
                let a = v.amp.next();
                s *= a * v.velocity;
                let pan = v.pan * p.spread;
                let (gl, gr) = pan_gains(pan);
                l += s * gl;
                r += s * gr;
            }
            left[n] += l * p.level;
            right[n] += r * p.level;
        }
    }
}

// ============================================================================ FM Synth

#[derive(Clone, Default)]
struct FmVoice {
    pc: f32,
    pm: f32,
    fb: f32,
    amp: Adsr,
    idx_env: Adsr,
    freq: f32,
    vel: f32,
}

pub struct FmSynth {
    sr: f32,
    ratio: f32,
    fine: f32,
    index: f32,
    idx_decay: f32,
    idx_sus: f32,
    feedback: f32,
    vel_index: f32,
    a: f32,
    d: f32,
    s: f32,
    r: f32,
    trem: f32,
    level: f32,
    voices: Vec<FmVoice>,
    slots: Vec<VoiceSlot>,
    age: u64,
    trem_phase: f32,
    sustain: Sustain,
    sine: SineTable,
}

impl FmSynth {
    pub fn new(sr: f32) -> Self {
        let n = 16;
        FmSynth { sr, ratio: 1.0, fine: 0.0, index: 2.5, idx_decay: 0.8, idx_sus: 0.25, feedback: 0.0, vel_index: 0.6, a: 0.002, d: 1.2, s: 0.35, r: 0.4, trem: 0.0, level: db_to_gain(-8.0), voices: vec![FmVoice::default(); n], slots: vec![VoiceSlot::default(); n], age: 0, trem_phase: 0.0, sustain: Sustain::default(), sine: SineTable::new() }
    }
    fn update(&mut self) {
        for v in &mut self.voices {
            v.amp.set(self.a, self.d, self.s, self.r, self.sr);
            v.idx_env.set(0.001, self.idx_decay, self.idx_sus, self.r, self.sr);
        }
    }
}

impl Instrument for FmSynth {
    fn set_param(&mut self, id: &str, v: f32) {
        match id {
            "ratio" => self.ratio = v,
            "fine" => self.fine = v,
            "index" => self.index = v,
            "index_decay" => self.idx_decay = v,
            "index_sustain" => self.idx_sus = v,
            "feedback" => self.feedback = v,
            "velocity_index" => self.vel_index = v,
            "attack" => self.a = v,
            "decay" => self.d = v,
            "sustain" => self.s = v,
            "release" => self.r = v,
            "tremolo" => self.trem = v,
            "level" => self.level = db_to_gain(v),
            _ => {}
        }
        self.update();
    }
    fn note_on(&mut self, pitch: u8, velocity: u8) {
        let i = pick_voice(&self.slots, pitch);
        self.age += 1;
        self.slots[i] = VoiceSlot { note: pitch, gate: true, age: self.age, active: true };
        let v = &mut self.voices[i];
        if !v.amp.is_active() {
            v.pc = 0.0;
            v.pm = 0.0;
            v.fb = 0.0;
        }
        v.freq = midi_to_freq(pitch as f32);
        v.vel = velocity as f32 / 127.0;
        v.amp.trigger();
        v.idx_env.trigger();
    }
    fn note_off(&mut self, pitch: u8) {
        if self.sustain.down {
            self.sustain.held.push(pitch);
            return;
        }
        for (i, s) in self.slots.iter_mut().enumerate() {
            if s.active && s.gate && s.note == pitch {
                s.gate = false;
                self.voices[i].amp.release();
                self.voices[i].idx_env.release();
            }
        }
    }
    fn control_change(&mut self, cc: u8, value: u8) {
        if cc == 64 {
            self.sustain.down = value >= 64;
            if !self.sustain.down {
                for n in std::mem::take(&mut self.sustain.held) {
                    self.note_off(n);
                }
            }
        }
    }
    fn all_notes_off(&mut self) {
        self.sustain = Sustain::default();
        for (i, s) in self.slots.iter_mut().enumerate() {
            s.gate = false;
            self.voices[i].amp.release();
            self.voices[i].idx_env.release();
        }
    }
    fn reset(&mut self) {
        for (i, s) in self.slots.iter_mut().enumerate() {
            *s = VoiceSlot::default();
            self.voices[i].amp.kill();
        }
    }
    fn render(&mut self, left: &mut [f32], right: &mut [f32], _ctx: &ProcessContext) {
        let sr = self.sr;
        let ratio = self.ratio + self.fine;
        for n in 0..left.len() {
            self.trem_phase = (self.trem_phase + 4.5 / sr).fract();
            let trem = 1.0 - self.trem * 0.5 * (1.0 + self.sine.get(self.trem_phase));
            let mut out = 0.0;
            for (i, v) in self.voices.iter_mut().enumerate() {
                if !self.slots[i].active {
                    continue;
                }
                if !v.amp.is_active() {
                    self.slots[i].active = false;
                    continue;
                }
                let idx = self.index * v.idx_env.next() * (1.0 - self.vel_index + self.vel_index * v.vel);
                let m = self.sine.get(v.pm + self.feedback * v.fb * 0.25);
                v.fb = m;
                let c = self.sine.get(v.pc + idx * m / std::f32::consts::TAU);
                v.pm = (v.pm + v.freq * ratio / sr).fract();
                v.pc = (v.pc + v.freq / sr).fract();
                out += c * v.amp.next() * (0.4 + 0.6 * v.vel);
            }
            let o = out * self.level * trem * 0.5;
            left[n] += o;
            right[n] += o;
        }
    }
}

// ============================================================================ Organ

const DRAWBAR_RATIOS: [f32; 9] = [0.5, 1.5, 1.0, 2.0, 3.0, 4.0, 5.0, 6.0, 8.0];
const DRAWBAR_IDS: [&str; 9] = ["db_16", "db_5_1_3", "db_8", "db_4", "db_2_2_3", "db_2", "db_1_3_5", "db_1_1_3", "db_1"];

#[derive(Clone, Default)]
struct OrganVoice {
    phases: [f32; 9],
    freq: f32,
    env: Adsr,
    perc: f32,
}

pub struct Organ {
    sr: f32,
    drawbars: [f32; 9],
    perc: f32,
    vibrato: f32,
    rotary: u8,
    drive: f32,
    level: f32,
    voices: Vec<OrganVoice>,
    slots: Vec<VoiceSlot>,
    age: u64,
    sine: SineTable,
    vib_delay: DelayLine,
    vib_phase: f32,
    horn_phase: f32,
    drum_phase: f32,
    horn_speed: f32,
    drum_speed: f32,
    horn_delay_l: DelayLine,
    horn_delay_r: DelayLine,
    split: OnePole,
    perc_ready: bool,
}

impl Organ {
    pub fn new(sr: f32) -> Self {
        let mut split = OnePole::default();
        split.set_freq(800.0, sr);
        let n = 16;
        let mut o = Organ {
            sr,
            drawbars: [8.0, 8.0, 8.0, 0.0, 0.0, 0.0, 0.0, 0.0, 0.0],
            perc: 0.0,
            vibrato: 0.3,
            rotary: 1,
            drive: 0.2,
            level: db_to_gain(-10.0),
            voices: vec![OrganVoice::default(); n],
            slots: vec![VoiceSlot::default(); n],
            age: 0,
            sine: SineTable::new(),
            vib_delay: DelayLine::new((sr * 0.02) as usize),
            vib_phase: 0.0,
            horn_phase: 0.0,
            drum_phase: 0.0,
            horn_speed: 0.8,
            drum_speed: 0.7,
            horn_delay_l: DelayLine::new((sr * 0.01) as usize),
            horn_delay_r: DelayLine::new((sr * 0.01) as usize),
            split,
            perc_ready: true,
        };
        for v in &mut o.voices {
            v.env.set(0.004, 0.01, 1.0, 0.03, sr);
        }
        o
    }
}

impl Instrument for Organ {
    fn set_param(&mut self, id: &str, v: f32) {
        if let Some(i) = DRAWBAR_IDS.iter().position(|d| *d == id) {
            self.drawbars[i] = v;
            return;
        }
        match id {
            "percussion" => self.perc = v,
            "vibrato" => self.vibrato = v,
            "rotary" => self.rotary = v.round() as u8,
            "drive" => self.drive = v,
            "level" => self.level = db_to_gain(v),
            _ => {}
        }
    }
    fn note_on(&mut self, pitch: u8, _velocity: u8) {
        let i = pick_voice(&self.slots, pitch);
        self.age += 1;
        // single-trigger percussion: only when no other key is held
        let any_held = self.slots.iter().any(|s| s.active && s.gate);
        self.slots[i] = VoiceSlot { note: pitch, gate: true, age: self.age, active: true };
        let v = &mut self.voices[i];
        v.freq = midi_to_freq(pitch as f32);
        v.env.trigger();
        if !any_held || self.perc_ready {
            v.perc = 1.0;
        }
        self.perc_ready = false;
    }
    fn note_off(&mut self, pitch: u8) {
        for (i, s) in self.slots.iter_mut().enumerate() {
            if s.active && s.gate && s.note == pitch {
                s.gate = false;
                self.voices[i].env.release();
            }
        }
        if !self.slots.iter().any(|s| s.active && s.gate) {
            self.perc_ready = true;
        }
    }
    fn all_notes_off(&mut self) {
        for (i, s) in self.slots.iter_mut().enumerate() {
            s.gate = false;
            self.voices[i].env.release();
        }
        self.perc_ready = true;
    }
    fn render(&mut self, left: &mut [f32], right: &mut [f32], _ctx: &ProcessContext) {
        let sr = self.sr;
        let gains: Vec<f32> = self.drawbars.iter().map(|d| (d / 8.0).powf(1.5)).collect();
        let total: f32 = gains.iter().sum::<f32>().max(1.0);
        let perc_decay = (-1.0 / (0.25 * sr)).exp();
        let (target_horn, target_drum) = match self.rotary {
            0 => (0.0, 0.0),
            1 => (0.8, 0.7),
            _ => (6.7, 5.8),
        };
        for n in 0..left.len() {
            let mut s = 0.0;
            for (i, v) in self.voices.iter_mut().enumerate() {
                if !self.slots[i].active {
                    continue;
                }
                if !v.env.is_active() {
                    self.slots[i].active = false;
                    continue;
                }
                let mut vs = 0.0;
                for h in 0..9 {
                    if gains[h] > 0.0 {
                        vs += self.sine.get(v.phases[h]) * gains[h];
                    }
                    v.phases[h] = (v.phases[h] + v.freq * DRAWBAR_RATIOS[h] / sr).fract();
                }
                vs /= total;
                if self.perc > 0.0 && v.perc > 1e-4 {
                    vs += self.sine.get(v.phases[4]) * v.perc * self.perc * 0.6;
                    v.perc *= perc_decay;
                }
                s += vs * v.env.next();
            }
            // scanner vibrato
            self.vib_delay.write(s);
            self.vib_phase = (self.vib_phase + 6.8 / sr).fract();
            let vd = 1.0 + (1.0 + self.sine.get(self.vib_phase)) * 0.0012 * sr * self.vibrato;
            let vib = self.vib_delay.read(vd);
            let s = s * (1.0 - self.vibrato * 0.5) + vib * self.vibrato * 0.5;
            // drive
            let d = 1.0 + self.drive * 6.0;
            let s = (s * d).tanh() / d.tanh().max(0.3);
            // rotary
            let (l, r) = if self.rotary == 0 {
                (s, s)
            } else {
                self.horn_speed += (target_horn - self.horn_speed) * 1.5 / sr;
                self.drum_speed += (target_drum - self.drum_speed) * 0.4 / sr;
                self.horn_phase = (self.horn_phase + self.horn_speed / sr).fract();
                self.drum_phase = (self.drum_phase + self.drum_speed / sr).fract();
                let low = self.split.process(s);
                let high = s - low;
                let hs = self.sine.get(self.horn_phase);
                let hc = self.sine.get(self.horn_phase + 0.25);
                self.horn_delay_l.write(high);
                self.horn_delay_r.write(high);
                let hl = self.horn_delay_l.read(1.0 + (1.0 + hs) * 0.0008 * sr) * (1.0 + 0.4 * hc);
                let hr = self.horn_delay_r.read(1.0 + (1.0 - hs) * 0.0008 * sr) * (1.0 - 0.4 * hc);
                let dm = self.sine.get(self.drum_phase);
                let dl = low * (1.0 + 0.25 * dm);
                let dr = low * (1.0 - 0.25 * dm);
                (hl + dl, hr + dr)
            };
            left[n] += l * self.level;
            right[n] += r * self.level;
        }
    }
}

// ============================================================================ Plucked string

#[derive(Clone)]
struct PluckVoice {
    line: DelayLine,
    delay: f32,
    last: f32,
    gain: f32,
    damping_gain: f32,
    released: bool,
    level: f32,
    vel: f32,
    age_samples: usize,
}

pub struct PluckSynth {
    sr: f32,
    decay: f32,
    brightness: f32,
    pick: f32,
    body: f32,
    release: f32,
    level: f32,
    voices: Vec<PluckVoice>,
    slots: Vec<VoiceSlot>,
    age: u64,
    noise: Noise,
    body1: Biquad,
    body2: Biquad,
    body1r: Biquad,
    body2r: Biquad,
}

impl PluckSynth {
    pub fn new(sr: f32) -> Self {
        let n = 12;
        let max_delay = (sr / 25.0) as usize + 4;
        PluckSynth {
            sr,
            decay: 2.5,
            brightness: 0.6,
            pick: 0.2,
            body: 0.4,
            release: 0.15,
            level: db_to_gain(-6.0),
            voices: (0..n).map(|_| PluckVoice { line: DelayLine::new(max_delay), delay: 100.0, last: 0.0, gain: 0.99, damping_gain: 0.9, released: false, level: 0.0, vel: 0.0, age_samples: 0 }).collect(),
            slots: vec![VoiceSlot::default(); n],
            age: 0,
            noise: Noise::new(777),
            body1: Biquad::new(BiquadKind::Peak, 110.0, 1.2, 6.0, sr),
            body2: Biquad::new(BiquadKind::Peak, 220.0, 1.5, 4.0, sr),
            body1r: Biquad::new(BiquadKind::Peak, 115.0, 1.2, 6.0, sr),
            body2r: Biquad::new(BiquadKind::Peak, 230.0, 1.5, 4.0, sr),
        }
    }
}

impl Instrument for PluckSynth {
    fn set_param(&mut self, id: &str, v: f32) {
        match id {
            "decay" => self.decay = v,
            "brightness" => self.brightness = v,
            "pick_position" => self.pick = v,
            "body" => self.body = v,
            "release" => self.release = v,
            "level" => self.level = db_to_gain(v),
            _ => {}
        }
    }
    fn note_on(&mut self, pitch: u8, velocity: u8) {
        let i = pick_voice(&self.slots, pitch);
        self.age += 1;
        self.slots[i] = VoiceSlot { note: pitch, gate: true, age: self.age, active: true };
        let freq = midi_to_freq(pitch as f32).max(27.5);
        let sr = self.sr;
        let v = &mut self.voices[i];
        v.delay = (sr / freq - 0.5).clamp(2.0, (v.line.len() - 3) as f32);
        v.gain = 0.001f32.powf(1.0 / (self.decay.max(0.05) * freq));
        v.damping_gain = 0.001f32.powf(1.0 / (self.release.max(0.01) * freq));
        v.released = false;
        v.vel = velocity as f32 / 127.0;
        v.level = 1.0;
        v.age_samples = 0;
        // excitation: filtered noise burst with pick-position comb
        let len = v.delay.ceil() as usize;
        let bright = 0.15 + self.brightness * 0.85 * v.vel.max(0.3);
        let mut lp = 0.0;
        let mut burst = Vec::with_capacity(len);
        for _ in 0..len {
            let x = self.noise.next();
            lp += (x - lp) * bright;
            burst.push(lp);
        }
        let pick_off = ((self.pick * len as f32) as usize).max(1);
        v.line.clear();
        for k in 0..len {
            let comb = burst[k] - if k >= pick_off { burst[k - pick_off] } else { 0.0 };
            v.line.write(comb * v.vel);
        }
        v.last = 0.0;
    }
    fn note_off(&mut self, pitch: u8) {
        for (i, s) in self.slots.iter_mut().enumerate() {
            if s.active && s.gate && s.note == pitch {
                s.gate = false;
                self.voices[i].released = true;
            }
        }
    }
    fn all_notes_off(&mut self) {
        for (i, s) in self.slots.iter_mut().enumerate() {
            s.gate = false;
            self.voices[i].released = true;
        }
    }
    fn render(&mut self, left: &mut [f32], right: &mut [f32], _ctx: &ProcessContext) {
        let blend = 0.5 + 0.45 * self.brightness;
        for n in 0..left.len() {
            let mut s = 0.0;
            for (i, v) in self.voices.iter_mut().enumerate() {
                if !self.slots[i].active {
                    continue;
                }
                let x = v.line.read(v.delay);
                let y = blend * x + (1.0 - blend) * v.last;
                v.last = x;
                let g = if v.released { v.damping_gain } else { v.gain };
                v.line.write(y * g);
                v.level = v.level * 0.9995 + y.abs() * 0.0005;
                v.age_samples += 1;
                if v.age_samples > 2048 && v.level < 1e-5 {
                    self.slots[i].active = false;
                }
                s += x;
            }
            let bl = self.body1.process(self.body2.process(s));
            let br = self.body1r.process(self.body2r.process(s));
            let l = s * (1.0 - self.body * 0.5) + bl * self.body * 0.5;
            let r = s * (1.0 - self.body * 0.5) + br * self.body * 0.5;
            left[n] += l * self.level;
            right[n] += r * self.level;
        }
    }
}

// ============================================================================ Sampler

#[derive(Clone, Default)]
struct SamplerVoice {
    pos: f64,
    rate: f64,
    env: Adsr,
    vel: f32,
}

pub struct Sampler {
    sr: f32,
    data: Option<Arc<AudioData>>,
    root: f32,
    tune: f32,
    a: f32,
    d: f32,
    s: f32,
    r: f32,
    looping: bool,
    vel_sens: f32,
    level: f32,
    voices: Vec<SamplerVoice>,
    slots: Vec<VoiceSlot>,
    age: u64,
}

impl Sampler {
    pub fn new(sr: f32) -> Self {
        let n = 16;
        Sampler { sr, data: None, root: 60.0, tune: 0.0, a: 0.002, d: 0.3, s: 1.0, r: 0.2, looping: false, vel_sens: 0.8, level: db_to_gain(-6.0), voices: vec![SamplerVoice::default(); n], slots: vec![VoiceSlot::default(); n], age: 0 }
    }
    fn update(&mut self) {
        for v in &mut self.voices {
            v.env.set(self.a, self.d, self.s, self.r, self.sr);
        }
    }
}

impl Instrument for Sampler {
    fn set_param(&mut self, id: &str, v: f32) {
        match id {
            "root_note" => self.root = v.round(),
            "tune" => self.tune = v,
            "attack" => self.a = v,
            "decay" => self.d = v,
            "sustain" => self.s = v,
            "release" => self.r = v,
            "loop" => self.looping = v >= 0.5,
            "velocity_sens" => self.vel_sens = v,
            "level" => self.level = db_to_gain(v),
            _ => {}
        }
        self.update();
    }
    fn set_sample(&mut self, data: Option<Arc<AudioData>>) {
        self.data = data;
        self.reset();
    }
    fn note_on(&mut self, pitch: u8, velocity: u8) {
        let Some(data) = &self.data else { return };
        let i = pick_voice(&self.slots, pitch);
        self.age += 1;
        self.slots[i] = VoiceSlot { note: pitch, gate: true, age: self.age, active: true };
        let v = &mut self.voices[i];
        v.pos = 0.0;
        v.rate = 2f64.powf((pitch as f64 - self.root as f64 + self.tune as f64) / 12.0) * data.sample_rate as f64 / self.sr as f64;
        v.vel = 1.0 - self.vel_sens + self.vel_sens * velocity as f32 / 127.0;
        v.env.trigger();
    }
    fn note_off(&mut self, pitch: u8) {
        for (i, s) in self.slots.iter_mut().enumerate() {
            if s.active && s.gate && s.note == pitch {
                s.gate = false;
                self.voices[i].env.release();
            }
        }
    }
    fn all_notes_off(&mut self) {
        for (i, s) in self.slots.iter_mut().enumerate() {
            s.gate = false;
            self.voices[i].env.release();
        }
    }
    fn reset(&mut self) {
        for (i, s) in self.slots.iter_mut().enumerate() {
            *s = VoiceSlot::default();
            self.voices[i].env.kill();
        }
    }
    fn render(&mut self, left: &mut [f32], right: &mut [f32], _ctx: &ProcessContext) {
        let Some(data) = self.data.clone() else { return };
        let frames = data.frames() as f64;
        if frames < 2.0 {
            return;
        }
        let stereo = data.channels.len() > 1;
        for n in 0..left.len() {
            let (mut l, mut r) = (0.0, 0.0);
            for (i, v) in self.voices.iter_mut().enumerate() {
                if !self.slots[i].active {
                    continue;
                }
                if !v.env.is_active() || (!self.looping && v.pos >= frames - 1.0) {
                    self.slots[i].active = false;
                    continue;
                }
                if self.looping && v.pos >= frames - 1.0 {
                    v.pos -= frames - 1.0;
                }
                let e = v.env.next() * v.vel;
                let sl = data.sample(0, v.pos);
                let sr = if stereo { data.sample(1, v.pos) } else { sl };
                l += sl * e;
                r += sr * e;
                v.pos += v.rate;
            }
            left[n] += l * self.level;
            right[n] += r * self.level;
        }
    }
}
