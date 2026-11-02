//! Ondina DSP: real-time instruments, audio effects and MIDI effects.
//!
//! Every processor is created by `kind` (see `ondina_core::plugins::REGISTRY`) and
//! configured through string parameter ids, so the engine can stay generic.

use std::sync::Arc;

pub mod denormal;
pub mod drums;
pub mod effects;
pub mod midifx;
pub mod synth;
pub mod util;
pub mod vst3;
pub mod audio_ops;
pub mod spatial;
pub mod ml_stems;

/// Decoded audio, deinterleaved, at its native sample rate.
#[derive(Debug, Clone)]
pub struct AudioData {
    pub sample_rate: u32,
    pub channels: Vec<Vec<f32>>,
}

impl AudioData {
    pub fn frames(&self) -> usize {
        self.channels.first().map(|c| c.len()).unwrap_or(0)
    }
    pub fn duration_sec(&self) -> f64 {
        self.frames() as f64 / self.sample_rate.max(1) as f64
    }
    /// Linear-interpolated sample of channel `ch` at fractional frame position.
    #[inline]
    pub fn sample(&self, ch: usize, pos: f64) -> f32 {
        let c = &self.channels[ch.min(self.channels.len() - 1)];
        if pos < 0.0 {
            return 0.0;
        }
        let i = pos as usize;
        if i + 1 >= c.len() {
            return if i < c.len() { c[i] } else { 0.0 };
        }
        let f = (pos - i as f64) as f32;
        c[i] + (c[i + 1] - c[i]) * f
    }
}

/// Information available to processors for each block.
#[derive(Debug, Clone, Copy)]
pub struct ProcessContext {
    pub sample_rate: f32,
    pub bpm: f32,
    /// Song position (beats) at the start of the block.
    pub beat: f64,
    /// Beats advanced per sample.
    pub beats_per_sample: f64,
    pub playing: bool,
}

impl ProcessContext {
    pub fn idle(sample_rate: f32) -> Self {
        ProcessContext { sample_rate, bpm: 120.0, beat: 0.0, beats_per_sample: 120.0 / 60.0 / sample_rate as f64, playing: false }
    }
}

pub trait Instrument: Send {
    fn set_param(&mut self, id: &str, value: f32);
    fn note_on(&mut self, pitch: u8, velocity: u8);
    fn note_off(&mut self, pitch: u8);
    fn control_change(&mut self, _cc: u8, _value: u8) {}
    fn all_notes_off(&mut self);
    /// Hard reset: silence immediately.
    fn reset(&mut self) {
        self.all_notes_off();
    }
    /// Render and **add** into the buffers.
    fn render(&mut self, left: &mut [f32], right: &mut [f32], ctx: &ProcessContext);
    fn set_sample(&mut self, _data: Option<Arc<AudioData>>) {}
    /// Restore a complete plug-in state blob (third-party plug-ins).
    fn load_state(&mut self, _data: &[u8]) {}
}

pub trait Effect: Send {
    fn set_param(&mut self, id: &str, value: f32);
    /// Process in place.
    fn process(&mut self, left: &mut [f32], right: &mut [f32], ctx: &ProcessContext);
    fn reset(&mut self);
    /// Restore a complete plug-in state blob (third-party plug-ins).
    fn load_state(&mut self, _data: &[u8]) {}
}

#[derive(Debug, Clone, Copy, PartialEq)]
pub enum MidiKind {
    NoteOn { pitch: u8, velocity: u8 },
    NoteOff { pitch: u8 },
    Cc { controller: u8, value: u8 },
}

#[derive(Debug, Clone, Copy, PartialEq)]
pub struct MidiEvent {
    /// Sample offset within the block.
    pub offset: usize,
    pub kind: MidiKind,
}

pub trait MidiEffect: Send {
    fn set_param(&mut self, id: &str, value: f32);
    /// Transform the block's events (sorted by offset). `frames` = block length.
    fn process(&mut self, events: &mut Vec<MidiEvent>, frames: usize, ctx: &ProcessContext, key: ondina_core::theory::Key);
    fn reset(&mut self) {}
}

pub fn create_instrument(kind: &str, sample_rate: f32) -> Option<Box<dyn Instrument>> {
    Some(match kind {
        "analog_synth" => Box::new(synth::AnalogSynth::new(sample_rate)),
        "fm_synth" => Box::new(synth::FmSynth::new(sample_rate)),
        "organ" => Box::new(synth::Organ::new(sample_rate)),
        "pluck_synth" => Box::new(synth::PluckSynth::new(sample_rate)),
        "sampler" => Box::new(synth::Sampler::new(sample_rate)),
        "drum_synth" => Box::new(drums::DrumSynth::new(sample_rate)),
        _ => return None,
    })
}

pub fn create_effect(kind: &str, sample_rate: f32) -> Option<Box<dyn Effect>> {
    use effects::*;
    Some(match kind {
        "channel_eq" => Box::new(ChannelEq::new(sample_rate)),
        "dynamic_eq" => Box::new(DynamicEq::new(sample_rate)),
        "compressor" => Box::new(Compressor::new(sample_rate)),
        "limiter" => Box::new(Limiter::new(sample_rate)),
        "noise_gate" => Box::new(NoiseGate::new(sample_rate)),
        "de_esser" => Box::new(DeEsser::new(sample_rate)),
        "stereo_delay" => Box::new(StereoDelay::new(sample_rate)),
        "tape_delay" => Box::new(TapeDelay::new(sample_rate)),
        "reverb" => Box::new(Reverb::new(sample_rate)),
        "chorus" => Box::new(Chorus::new(sample_rate)),
        "flanger" => Box::new(Flanger::new(sample_rate)),
        "phaser" => Box::new(Phaser::new(sample_rate)),
        "tremolo" => Box::new(Tremolo::new(sample_rate)),
        "overdrive" => Box::new(Overdrive::new(sample_rate)),
        "bitcrusher" => Box::new(Bitcrusher::new(sample_rate)),
        "auto_filter" => Box::new(AutoFilter::new(sample_rate)),
        "gain" => Box::new(Gain::new(sample_rate)),
        "stereo_imager" => Box::new(StereoImager::new(sample_rate)),
        "exciter" => Box::new(Exciter::new(sample_rate)),
        _ => return None,
    })
}

pub fn create_midi_effect(kind: &str) -> Option<Box<dyn MidiEffect>> {
    use midifx::*;
    Some(match kind {
        "arpeggiator" => Box::new(Arpeggiator::default()),
        "transposer" => Box::new(Transposer::default()),
        "chord_trigger" => Box::new(ChordTrigger::default()),
        "velocity_processor" => Box::new(VelocityProcessor::default()),
        _ => return None,
    })
}

/// Apply registry defaults then the given parameter map.
pub fn configure<F: FnMut(&str, f32)>(kind: &str, params: &std::collections::BTreeMap<String, f32>, mut set: F) {
    if let Some(info) = ondina_core::plugins::find(kind) {
        for p in info.params {
            set(p.id, params.get(p.id).copied().unwrap_or(p.default));
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use ondina_core::plugins::{PluginCategory, REGISTRY};

    #[test]
    fn every_registered_plugin_has_a_processor_and_renders_finite_audio() {
        let sr = 48000.0;
        let ctx = ProcessContext { sample_rate: sr, bpm: 120.0, beat: 0.0, beats_per_sample: 2.0 / sr as f64, playing: true };
        for info in REGISTRY {
            match info.category {
                PluginCategory::Instrument => {
                    let mut i = create_instrument(info.kind, sr).unwrap_or_else(|| panic!("no instrument {}", info.kind));
                    configure(info.kind, &Default::default(), |k, v| i.set_param(k, v));
                    let mut l = vec![0.0; 512];
                    let mut r = vec![0.0; 512];
                    for p in [36u8, 38, 42, 60, 64, 67] {
                        i.note_on(p, 100);
                    }
                    let mut energy = 0.0;
                    for _ in 0..40 {
                        l.iter_mut().for_each(|x| *x = 0.0);
                        r.iter_mut().for_each(|x| *x = 0.0);
                        i.render(&mut l, &mut r, &ctx);
                        assert!(l.iter().chain(r.iter()).all(|x| x.is_finite()), "{} produced NaN", info.kind);
                        energy += l.iter().map(|x| x * x).sum::<f32>();
                    }
                    if info.kind != "sampler" {
                        assert!(energy > 0.0, "{} is silent", info.kind);
                    }
                }
                PluginCategory::Effect => {
                    let mut e = create_effect(info.kind, sr).unwrap_or_else(|| panic!("no effect {}", info.kind));
                    configure(info.kind, &Default::default(), |k, v| e.set_param(k, v));
                    let mut l: Vec<f32> = (0..512).map(|i| (i as f32 * 0.05).sin() * 0.5).collect();
                    let mut r = l.clone();
                    for _ in 0..20 {
                        e.process(&mut l, &mut r, &ctx);
                        assert!(l.iter().chain(r.iter()).all(|x| x.is_finite() && x.abs() < 20.0), "{} unstable", info.kind);
                    }
                }
                PluginCategory::MidiEffect => {
                    let mut m = create_midi_effect(info.kind).unwrap_or_else(|| panic!("no midi fx {}", info.kind));
                    configure(info.kind, &Default::default(), |k, v| m.set_param(k, v));
                    let mut ev = vec![MidiEvent { offset: 0, kind: MidiKind::NoteOn { pitch: 60, velocity: 100 } }];
                    m.process(&mut ev, 512, &ctx, ondina_core::theory::Key { root: 0, scale: ondina_core::theory::ScaleKind::Major });
                }
            }
        }
    }
}
