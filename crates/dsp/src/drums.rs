//! Synthesized drum kit on the General MIDI drum map.

use crate::util::*;
use crate::{Instrument, ProcessContext};

#[derive(Clone, Copy, PartialEq, Debug)]
enum DrumKind {
    Kick,
    Snare,
    Rim,
    Clap,
    ClosedHat,
    PedalHat,
    OpenHat,
    Tom(f32),
    Crash,
    Ride,
    Tambourine,
    Cowbell,
    Claves,
}

fn kind_for(note: u8) -> Option<DrumKind> {
    use DrumKind::*;
    Some(match note {
        35 | 36 => Kick,
        37 => Rim,
        38 | 40 => Snare,
        39 => Clap,
        42 => ClosedHat,
        44 => PedalHat,
        46 => OpenHat,
        41 => Tom(80.0),
        43 => Tom(98.0),
        45 => Tom(117.0),
        47 => Tom(139.0),
        48 => Tom(165.0),
        50 => Tom(196.0),
        49 | 52 | 55 | 57 => Crash,
        51 | 53 | 59 => Ride,
        54 => Tambourine,
        56 => Cowbell,
        75 => Claves,
        _ => return None,
    })
}

#[derive(Clone)]
struct DrumVoice {
    kind: DrumKind,
    active: bool,
    t: f32,
    vel: f32,
    phase: f32,
    phase2: f32,
    metal: [f32; 6],
    bp: Biquad,
    hp: Biquad,
    note: u8,
    pan: f32,
}

impl Default for DrumVoice {
    fn default() -> Self {
        DrumVoice { kind: DrumKind::Kick, active: false, t: 0.0, vel: 0.0, phase: 0.0, phase2: 0.0, metal: [0.0; 6], bp: Biquad::default(), hp: Biquad::default(), note: 0, pan: 0.0 }
    }
}

const METAL_RATIOS: [f32; 6] = [2.0, 3.0, 4.16, 5.43, 6.79, 8.21];

pub struct DrumSynth {
    sr: f32,
    kick_tune: f32,
    kick_decay: f32,
    kick_punch: f32,
    snare_tune: f32,
    snare_snappy: f32,
    snare_decay: f32,
    clap_decay: f32,
    hat_tone: f32,
    hat_decay: f32,
    open_hat_decay: f32,
    tom_tune: f32,
    cymbal_decay: f32,
    drive: f32,
    level: f32,
    voices: Vec<DrumVoice>,
    noise: Noise,
    next: usize,
}

impl DrumSynth {
    pub fn new(sr: f32) -> Self {
        DrumSynth {
            sr,
            kick_tune: 52.0,
            kick_decay: 0.45,
            kick_punch: 0.5,
            snare_tune: 190.0,
            snare_snappy: 0.6,
            snare_decay: 0.2,
            clap_decay: 0.25,
            hat_tone: 0.6,
            hat_decay: 0.05,
            open_hat_decay: 0.45,
            tom_tune: 1.0,
            cymbal_decay: 1.6,
            drive: 0.0,
            level: db_to_gain(-4.0),
            voices: vec![DrumVoice::default(); 24],
            noise: Noise::new(4242),
            next: 0,
        }
    }

    fn sample(&mut self, vi: usize) -> f32 {
        let sr = self.sr;
        let dt = 1.0 / sr;
        let noise = self.noise.next();
        let v = &mut self.voices[vi];
        let t = v.t;
        v.t += dt;
        let tau = std::f32::consts::TAU;
        let out = match v.kind {
            DrumKind::Kick => {
                let pitch_env = (-t / (0.012 + 0.03 * (1.0 - self.kick_punch))).exp();
                let f = self.kick_tune * (1.0 + pitch_env * (2.0 + self.kick_punch * 6.0));
                v.phase = (v.phase + f * dt).fract();
                let amp = (-t / (self.kick_decay * 0.35)).exp();
                let click = if t < 0.004 { noise * (1.0 - t / 0.004) * 0.4 * self.kick_punch } else { 0.0 };
                if amp < 1e-4 {
                    v.active = false;
                }
                (v.phase * tau).sin() * amp + click
            }
            DrumKind::Snare => {
                v.phase = (v.phase + self.snare_tune * dt).fract();
                v.phase2 = (v.phase2 + self.snare_tune * 1.47 * dt).fract();
                let tone_env = (-t / (self.snare_decay * 0.4)).exp();
                let tone = ((v.phase * tau).sin() + 0.5 * (v.phase2 * tau).sin()) * tone_env * (1.0 - self.snare_snappy * 0.6);
                let n_env = (-t / (self.snare_decay * 0.7)).exp();
                let nz = v.hp.process(noise) * n_env * (0.3 + self.snare_snappy);
                if n_env < 1e-4 {
                    v.active = false;
                }
                tone * 0.8 + nz * 0.7
            }
            DrumKind::Rim => {
                v.phase = (v.phase + 1700.0 * dt).fract();
                let env = (-t / 0.012).exp();
                if env < 1e-4 {
                    v.active = false;
                }
                ((v.phase * tau).sin() * 0.6 + v.bp.process(noise) * 0.6) * env
            }
            DrumKind::Clap => {
                let burst = |start: f32| if t >= start { (-(t - start) / 0.007).exp() } else { 0.0 };
                let env = burst(0.0).max(burst(0.011)).max(burst(0.022)) * 0.8 + if t > 0.03 { (-(t - 0.03) / (self.clap_decay * 0.5)).exp() } else { 0.0 };
                if t > 0.05 && env < 1e-4 {
                    v.active = false;
                }
                v.bp.process(noise) * env * 2.0
            }
            DrumKind::ClosedHat | DrumKind::PedalHat | DrumKind::OpenHat | DrumKind::Crash | DrumKind::Ride => {
                let base = 300.0 + self.hat_tone * 300.0;
                let mut metal = 0.0;
                for (k, ph) in v.metal.iter_mut().enumerate() {
                    *ph = (*ph + base * METAL_RATIOS[k] * dt).fract();
                    metal += if *ph < 0.5 { 1.0 } else { -1.0 };
                }
                metal /= 6.0;
                let decay = match v.kind {
                    DrumKind::ClosedHat => self.hat_decay,
                    DrumKind::PedalHat => self.hat_decay * 0.8,
                    DrumKind::OpenHat => self.open_hat_decay,
                    DrumKind::Crash => self.cymbal_decay,
                    _ => self.cymbal_decay * 0.9,
                };
                let env = (-t / (decay * 0.4)).exp();
                if env < 1e-4 {
                    v.active = false;
                }
                let mix = match v.kind {
                    DrumKind::Ride => metal * 0.8 + noise * 0.2,
                    DrumKind::Crash => metal * 0.4 + noise * 0.6,
                    _ => metal * 0.6 + noise * 0.4,
                };
                v.hp.process(mix) * env * if matches!(v.kind, DrumKind::Crash | DrumKind::Ride) { 0.6 } else { 0.8 }
            }
            DrumKind::Tom(f0) => {
                let f = f0 * self.tom_tune * (1.0 + 0.5 * (-t / 0.05).exp());
                v.phase = (v.phase + f * dt).fract();
                let env = (-t / 0.18).exp();
                if env < 1e-4 {
                    v.active = false;
                }
                ((v.phase * tau).sin() + noise * 0.05 * (-t / 0.01).exp()) * env
            }
            DrumKind::Tambourine => {
                let env = (-t / 0.08).exp();
                if env < 1e-4 {
                    v.active = false;
                }
                v.hp.process(noise) * env * 0.6
            }
            DrumKind::Cowbell => {
                v.phase = (v.phase + 545.0 * dt).fract();
                v.phase2 = (v.phase2 + 815.0 * dt).fract();
                let sq = |p: f32| if p < 0.5 { 1.0 } else { -1.0 };
                let env = (-t / 0.09).exp();
                if env < 1e-4 {
                    v.active = false;
                }
                v.bp.process((sq(v.phase) + sq(v.phase2)) * 0.5) * env * 1.2
            }
            DrumKind::Claves => {
                v.phase = (v.phase + 2500.0 * dt).fract();
                let env = (-t / 0.025).exp();
                if env < 1e-4 {
                    v.active = false;
                }
                (v.phase * tau).sin() * env * 0.6
            }
        };
        out * v.vel
    }
}

impl Instrument for DrumSynth {
    fn set_param(&mut self, id: &str, v: f32) {
        match id {
            "kick_tune" => self.kick_tune = v,
            "kick_decay" => self.kick_decay = v,
            "kick_punch" => self.kick_punch = v,
            "snare_tune" => self.snare_tune = v,
            "snare_snappy" => self.snare_snappy = v,
            "snare_decay" => self.snare_decay = v,
            "clap_decay" => self.clap_decay = v,
            "hat_tone" => self.hat_tone = v,
            "hat_decay" => self.hat_decay = v,
            "open_hat_decay" => self.open_hat_decay = v,
            "tom_tune" => self.tom_tune = v,
            "cymbal_decay" => self.cymbal_decay = v,
            "drive" => self.drive = v,
            "level" => self.level = db_to_gain(v),
            _ => {}
        }
    }

    fn note_on(&mut self, pitch: u8, velocity: u8) {
        let Some(kind) = kind_for(pitch) else { return };
        // hi-hat choke group
        if matches!(kind, DrumKind::ClosedHat | DrumKind::PedalHat) {
            for v in &mut self.voices {
                if v.active && v.kind == DrumKind::OpenHat {
                    v.active = false;
                }
            }
        }
        // retrigger same note on the same voice to avoid voice pile-up
        let idx = self.voices.iter().position(|v| v.active && v.note == pitch).unwrap_or_else(|| {
            let i = self.voices.iter().position(|v| !v.active).unwrap_or(self.next);
            self.next = (self.next + 1) % self.voices.len();
            i
        });
        let sr = self.sr;
        let v = &mut self.voices[idx];
        *v = DrumVoice::default();
        v.kind = kind;
        v.note = pitch;
        v.active = true;
        v.vel = (velocity as f32 / 127.0).powf(1.3);
        v.pan = match kind {
            DrumKind::ClosedHat | DrumKind::OpenHat | DrumKind::PedalHat => 0.3,
            DrumKind::Tom(f) => ((f - 140.0) / 120.0).clamp(-0.6, 0.6),
            DrumKind::Crash => -0.4,
            DrumKind::Ride => 0.45,
            DrumKind::Tambourine => -0.3,
            _ => 0.0,
        };
        match kind {
            DrumKind::Snare => v.hp = Biquad::new(BiquadKind::HighPass, 1800.0, 0.7, 0.0, sr),
            DrumKind::Rim => v.bp = Biquad::new(BiquadKind::BandPass, 2500.0, 2.0, 0.0, sr),
            DrumKind::Clap => v.bp = Biquad::new(BiquadKind::BandPass, 1200.0, 1.2, 0.0, sr),
            DrumKind::ClosedHat | DrumKind::PedalHat | DrumKind::OpenHat => v.hp = Biquad::new(BiquadKind::HighPass, 7000.0, 0.8, 0.0, sr),
            DrumKind::Crash | DrumKind::Ride => v.hp = Biquad::new(BiquadKind::HighPass, 4500.0, 0.7, 0.0, sr),
            DrumKind::Tambourine => v.hp = Biquad::new(BiquadKind::HighPass, 6000.0, 0.9, 0.0, sr),
            DrumKind::Cowbell => v.bp = Biquad::new(BiquadKind::BandPass, 700.0, 3.0, 0.0, sr),
            _ => {}
        }
    }

    fn note_off(&mut self, _pitch: u8) {}

    fn all_notes_off(&mut self) {}

    fn reset(&mut self) {
        for v in &mut self.voices {
            v.active = false;
        }
    }

    fn render(&mut self, left: &mut [f32], right: &mut [f32], _ctx: &ProcessContext) {
        let drive = 1.0 + self.drive * 5.0;
        for n in 0..left.len() {
            let (mut l, mut r) = (0.0, 0.0);
            for i in 0..self.voices.len() {
                if !self.voices[i].active {
                    continue;
                }
                let s = self.sample(i);
                let (gl, gr) = pan_gains(self.voices[i].pan);
                l += s * gl;
                r += s * gr;
            }
            if self.drive > 0.0 {
                l = (l * drive).tanh() / drive.tanh();
                r = (r * drive).tanh() / drive.tanh();
            }
            left[n] += l * self.level;
            right[n] += r * self.level;
        }
    }
}
