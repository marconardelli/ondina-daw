//! Plug-in registry: metadata for every built-in instrument, audio effect and MIDI effect,
//! plus factory presets and track "patches" (instrument + effect chain).
//!
//! The DSP crate implements processors for each `kind` listed here. The GUI, the MCP server
//! and the AI assistant all read this registry to discover parameters.

use serde::Serialize;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum PluginCategory {
    Instrument,
    Effect,
    MidiEffect,
}

#[derive(Debug, Clone, Copy, Serialize)]
pub struct ParamInfo {
    pub id: &'static str,
    pub name: &'static str,
    pub min: f32,
    pub max: f32,
    pub default: f32,
    pub unit: &'static str,
    /// If non-empty, the parameter is a selector and the value is the index into this list.
    pub choices: &'static [&'static str],
}

#[derive(Debug, Clone, Copy, Serialize)]
pub struct PluginInfo {
    pub kind: &'static str,
    pub name: &'static str,
    pub category: PluginCategory,
    pub group: &'static str,
    pub description: &'static str,
    pub params: &'static [ParamInfo],
}

const fn p(id: &'static str, name: &'static str, min: f32, max: f32, default: f32, unit: &'static str) -> ParamInfo {
    ParamInfo { id, name, min, max, default, unit, choices: &[] }
}

const fn c(id: &'static str, name: &'static str, default: f32, choices: &'static [&'static str]) -> ParamInfo {
    ParamInfo { id, name, min: 0.0, max: (choices.len() - 1) as f32, default, unit: "", choices }
}

const ONOFF: &[&str] = &["Off", "On"];
const WAVES: &[&str] = &["Saw", "Square", "Triangle", "Sine", "Noise"];
const FILTERS: &[&str] = &["Lowpass", "Highpass", "Bandpass"];
const DEQ_TYPES: &[&str] = &["Bell", "Low Shelf", "High Shelf"];
const LISTEN_MS: &[&str] = &["Stereo", "Mid", "Side"];

use PluginCategory::*;

pub static REGISTRY: &[PluginInfo] = &[
    // ------------------------------------------------------------------ instruments
    PluginInfo {
        kind: "analog_synth",
        name: "Analog Synth",
        category: Instrument,
        group: "Synthesizer",
        description: "Polyphonic virtual-analog subtractive synthesizer: two band-limited oscillators, sub oscillator, multimode resonant filter with envelope, amp envelope, LFO and glide. Leads, basses, pads, plucks, brass.",
        params: &[
            c("osc1_wave", "Osc 1 Wave", 0.0, WAVES),
            c("osc2_wave", "Osc 2 Wave", 0.0, WAVES),
            p("osc2_semi", "Osc 2 Semitones", -24.0, 24.0, 0.0, "st"),
            p("osc2_detune", "Osc 2 Detune", -100.0, 100.0, 7.0, "ct"),
            p("osc_mix", "Osc Mix (1→2)", 0.0, 1.0, 0.5, ""),
            p("sub_level", "Sub Osc Level", 0.0, 1.0, 0.0, ""),
            p("noise_level", "Noise Level", 0.0, 1.0, 0.0, ""),
            p("pulse_width", "Pulse Width", 0.05, 0.95, 0.5, ""),
            p("unison_spread", "Stereo Spread", 0.0, 1.0, 0.3, ""),
            c("filter_type", "Filter Type", 0.0, FILTERS),
            p("cutoff", "Cutoff", 20.0, 20000.0, 2500.0, "Hz"),
            p("resonance", "Resonance", 0.0, 1.0, 0.2, ""),
            p("filter_env", "Filter Env Amount", -1.0, 1.0, 0.3, ""),
            p("key_track", "Key Tracking", 0.0, 1.0, 0.5, ""),
            p("f_attack", "Filter Attack", 0.001, 5.0, 0.005, "s"),
            p("f_decay", "Filter Decay", 0.001, 5.0, 0.3, "s"),
            p("f_sustain", "Filter Sustain", 0.0, 1.0, 0.3, ""),
            p("f_release", "Filter Release", 0.001, 5.0, 0.3, "s"),
            p("attack", "Amp Attack", 0.001, 5.0, 0.005, "s"),
            p("decay", "Amp Decay", 0.001, 5.0, 0.2, "s"),
            p("sustain", "Amp Sustain", 0.0, 1.0, 0.8, ""),
            p("release", "Amp Release", 0.001, 8.0, 0.25, "s"),
            p("lfo_rate", "LFO Rate", 0.05, 20.0, 5.0, "Hz"),
            p("lfo_pitch", "LFO → Pitch", 0.0, 1.0, 0.0, ""),
            p("lfo_cutoff", "LFO → Cutoff", 0.0, 1.0, 0.0, ""),
            p("glide", "Glide", 0.0, 1.0, 0.0, "s"),
            p("velocity_sens", "Velocity Sensitivity", 0.0, 1.0, 0.7, ""),
            p("level", "Level", -40.0, 6.0, -8.0, "dB"),
        ],
    },
    PluginInfo {
        kind: "fm_synth",
        name: "FM Synth",
        category: Instrument,
        group: "Synthesizer",
        description: "Two-operator FM synthesizer with feedback and modulation-index envelope. Electric pianos, bells, mallets, metallic basses.",
        params: &[
            p("ratio", "Modulator Ratio", 0.5, 16.0, 1.0, ""),
            p("fine", "Modulator Fine", -0.5, 0.5, 0.0, ""),
            p("index", "Mod Index", 0.0, 20.0, 2.5, ""),
            p("index_decay", "Index Decay", 0.01, 5.0, 0.8, "s"),
            p("index_sustain", "Index Sustain", 0.0, 1.0, 0.25, ""),
            p("feedback", "Feedback", 0.0, 1.0, 0.0, ""),
            p("velocity_index", "Velocity → Index", 0.0, 1.0, 0.6, ""),
            p("attack", "Attack", 0.001, 5.0, 0.002, "s"),
            p("decay", "Decay", 0.01, 8.0, 1.2, "s"),
            p("sustain", "Sustain", 0.0, 1.0, 0.35, ""),
            p("release", "Release", 0.001, 8.0, 0.4, "s"),
            p("tremolo", "Tremolo", 0.0, 1.0, 0.0, ""),
            p("level", "Level", -40.0, 6.0, -8.0, "dB"),
        ],
    },
    PluginInfo {
        kind: "drum_synth",
        name: "Drum Synth",
        category: Instrument,
        group: "Drums",
        description: "Synthesized drum kit on the General MIDI drum map: 35/36 kick, 37 rim, 38/40 snare, 39 clap, 41/43/45/47/48/50 toms, 42 closed hat, 44 pedal hat, 46 open hat, 49/57 crash, 51/59 ride, 54 tambourine, 56 cowbell, 75 claves.",
        params: &[
            p("kick_tune", "Kick Tune", 30.0, 120.0, 52.0, "Hz"),
            p("kick_decay", "Kick Decay", 0.05, 2.0, 0.45, "s"),
            p("kick_punch", "Kick Punch", 0.0, 1.0, 0.5, ""),
            p("snare_tune", "Snare Tune", 100.0, 400.0, 190.0, "Hz"),
            p("snare_snappy", "Snare Snappy", 0.0, 1.0, 0.6, ""),
            p("snare_decay", "Snare Decay", 0.05, 1.0, 0.2, "s"),
            p("clap_decay", "Clap Decay", 0.05, 1.0, 0.25, "s"),
            p("hat_tone", "Hat Tone", 0.0, 1.0, 0.6, ""),
            p("hat_decay", "Closed Hat Decay", 0.01, 0.5, 0.05, "s"),
            p("open_hat_decay", "Open Hat Decay", 0.1, 2.0, 0.45, "s"),
            p("tom_tune", "Tom Tune", 0.5, 2.0, 1.0, "x"),
            p("cymbal_decay", "Cymbal Decay", 0.3, 4.0, 1.6, "s"),
            p("drive", "Drive", 0.0, 1.0, 0.0, ""),
            p("level", "Level", -40.0, 6.0, -4.0, "dB"),
        ],
    },
    PluginInfo {
        kind: "sampler",
        name: "Sampler",
        category: Instrument,
        group: "Sampler",
        description: "Single-zone sampler: plays the assigned audio file chromatically from a root note with ADSR envelope and optional looping. Assign a file with set_sampler_sample.",
        params: &[
            p("root_note", "Root Note", 0.0, 127.0, 60.0, ""),
            p("tune", "Tune", -24.0, 24.0, 0.0, "st"),
            p("attack", "Attack", 0.001, 5.0, 0.002, "s"),
            p("decay", "Decay", 0.001, 5.0, 0.3, "s"),
            p("sustain", "Sustain", 0.0, 1.0, 1.0, ""),
            p("release", "Release", 0.001, 8.0, 0.2, "s"),
            c("loop", "Loop", 0.0, ONOFF),
            p("velocity_sens", "Velocity Sensitivity", 0.0, 1.0, 0.8, ""),
            p("level", "Level", -40.0, 6.0, -6.0, "dB"),
        ],
    },
    PluginInfo {
        kind: "organ",
        name: "Tonewheel Organ",
        category: Instrument,
        group: "Keyboards",
        description: "Drawbar tonewheel organ with percussion, vibrato/chorus, rotary speaker simulation and drive.",
        params: &[
            p("db_16", "Drawbar 16'", 0.0, 8.0, 8.0, ""),
            p("db_5_1_3", "Drawbar 5 1/3'", 0.0, 8.0, 8.0, ""),
            p("db_8", "Drawbar 8'", 0.0, 8.0, 8.0, ""),
            p("db_4", "Drawbar 4'", 0.0, 8.0, 0.0, ""),
            p("db_2_2_3", "Drawbar 2 2/3'", 0.0, 8.0, 0.0, ""),
            p("db_2", "Drawbar 2'", 0.0, 8.0, 0.0, ""),
            p("db_1_3_5", "Drawbar 1 3/5'", 0.0, 8.0, 0.0, ""),
            p("db_1_1_3", "Drawbar 1 1/3'", 0.0, 8.0, 0.0, ""),
            p("db_1", "Drawbar 1'", 0.0, 8.0, 0.0, ""),
            p("percussion", "Percussion", 0.0, 1.0, 0.0, ""),
            p("vibrato", "Vibrato/Chorus", 0.0, 1.0, 0.3, ""),
            c("rotary", "Rotary Speed", 1.0, &["Off", "Slow", "Fast"]),
            p("drive", "Drive", 0.0, 1.0, 0.2, ""),
            p("level", "Level", -40.0, 6.0, -10.0, "dB"),
        ],
    },
    PluginInfo {
        kind: "pluck_synth",
        name: "Plucked String",
        category: Instrument,
        group: "Physical Model",
        description: "Physical-model plucked string (Karplus-Strong): guitars, harps, kotos, plucked basses.",
        params: &[
            p("decay", "Decay", 0.1, 10.0, 2.5, "s"),
            p("brightness", "Brightness", 0.0, 1.0, 0.6, ""),
            p("pick_position", "Pick Position", 0.05, 0.5, 0.2, ""),
            p("body", "Body Resonance", 0.0, 1.0, 0.4, ""),
            p("release", "Release (Damping)", 0.01, 2.0, 0.15, "s"),
            p("level", "Level", -40.0, 6.0, -6.0, "dB"),
        ],
    },
    // ------------------------------------------------------------------ effects
    PluginInfo {
        kind: "channel_eq",
        name: "Channel EQ",
        category: Effect,
        group: "EQ",
        description: "Six-band parametric equalizer: high-pass, low shelf, two peaking bands, high shelf, low-pass. High Pass and Low Pass (12 dB per octave, turned on by their switches) cut below or above their frequency. Low Shelf and High Shelf raise or lower everything below or above their Freq by their Gain. Mid 1 and Mid 2 are bells at Freq that add Gain dB, as narrow as their Q (a higher Q is narrower). A shelf or mid band at 0 dB does nothing. Output Gain is applied last.",
        params: &[
            c("hp_on", "High Pass", 0.0, ONOFF),
            p("hp_freq", "HP Freq", 20.0, 1000.0, 80.0, "Hz"),
            p("low_gain", "Low Shelf Gain", -24.0, 24.0, 0.0, "dB"),
            p("low_freq", "Low Shelf Freq", 30.0, 500.0, 120.0, "Hz"),
            p("mid1_gain", "Mid 1 Gain", -24.0, 24.0, 0.0, "dB"),
            p("mid1_freq", "Mid 1 Freq", 80.0, 8000.0, 400.0, "Hz"),
            p("mid1_q", "Mid 1 Q", 0.1, 10.0, 1.0, ""),
            p("mid2_gain", "Mid 2 Gain", -24.0, 24.0, 0.0, "dB"),
            p("mid2_freq", "Mid 2 Freq", 200.0, 16000.0, 2500.0, "Hz"),
            p("mid2_q", "Mid 2 Q", 0.1, 10.0, 1.0, ""),
            p("high_gain", "High Shelf Gain", -24.0, 24.0, 0.0, "dB"),
            p("high_freq", "High Shelf Freq", 1000.0, 16000.0, 8000.0, "Hz"),
            c("lp_on", "Low Pass", 0.0, ONOFF),
            p("lp_freq", "LP Freq", 1000.0, 20000.0, 16000.0, "Hz"),
            p("output", "Output Gain", -24.0, 24.0, 0.0, "dB"),
        ],
    },
    PluginInfo {
        kind: "dynamic_eq",
        name: "Dynamic EQ",
        category: Effect,
        group: "EQ",
        description: "Four-band dynamic equalizer. Each band is a Bell (shaped by Q) or a Low or High Shelf at Freq with a static Gain, and moves further only while its own frequencies are loud: when the level around Freq goes above Threshold the band adds up to Range dB on top of Gain, 1 dB for every dB over Threshold. A negative Range cuts the band only when it gets too loud (mud, boom, resonances), a positive Range boosts it only then. With Range 0 a band is a plain static EQ band. Attack and Release set how fast the bands follow the level. No latency.",
        params: &[
            c("b1_on", "Band 1", 1.0, ONOFF),
            c("b1_type", "Band 1 Type", 1.0, DEQ_TYPES),
            p("b1_freq", "Band 1 Freq", 20.0, 20000.0, 100.0, "Hz"),
            p("b1_gain", "Band 1 Gain", -24.0, 24.0, 0.0, "dB"),
            p("b1_q", "Band 1 Q", 0.1, 10.0, 0.7, ""),
            p("b1_threshold", "Band 1 Threshold", -60.0, 0.0, -24.0, "dB"),
            p("b1_range", "Band 1 Range", -24.0, 24.0, 0.0, "dB"),
            c("b2_on", "Band 2", 1.0, ONOFF),
            c("b2_type", "Band 2 Type", 0.0, DEQ_TYPES),
            p("b2_freq", "Band 2 Freq", 20.0, 20000.0, 300.0, "Hz"),
            p("b2_gain", "Band 2 Gain", -24.0, 24.0, 0.0, "dB"),
            p("b2_q", "Band 2 Q", 0.1, 10.0, 1.0, ""),
            p("b2_threshold", "Band 2 Threshold", -60.0, 0.0, -24.0, "dB"),
            p("b2_range", "Band 2 Range", -24.0, 24.0, 0.0, "dB"),
            c("b3_on", "Band 3", 1.0, ONOFF),
            c("b3_type", "Band 3 Type", 0.0, DEQ_TYPES),
            p("b3_freq", "Band 3 Freq", 20.0, 20000.0, 2500.0, "Hz"),
            p("b3_gain", "Band 3 Gain", -24.0, 24.0, 0.0, "dB"),
            p("b3_q", "Band 3 Q", 0.1, 10.0, 1.0, ""),
            p("b3_threshold", "Band 3 Threshold", -60.0, 0.0, -24.0, "dB"),
            p("b3_range", "Band 3 Range", -24.0, 24.0, 0.0, "dB"),
            c("b4_on", "Band 4", 1.0, ONOFF),
            c("b4_type", "Band 4 Type", 2.0, DEQ_TYPES),
            p("b4_freq", "Band 4 Freq", 20.0, 20000.0, 8000.0, "Hz"),
            p("b4_gain", "Band 4 Gain", -24.0, 24.0, 0.0, "dB"),
            p("b4_q", "Band 4 Q", 0.1, 10.0, 0.7, ""),
            p("b4_threshold", "Band 4 Threshold", -60.0, 0.0, -24.0, "dB"),
            p("b4_range", "Band 4 Range", -24.0, 24.0, 0.0, "dB"),
            p("attack", "Attack", 0.1, 200.0, 10.0, "ms"),
            p("release", "Release", 10.0, 2000.0, 150.0, "ms"),
            p("output", "Output Gain", -24.0, 24.0, 0.0, "dB"),
        ],
    },
    PluginInfo {
        kind: "compressor",
        name: "Compressor",
        category: Effect,
        group: "Dynamics",
        description: "Feed-forward peak compressor. Above Threshold the output rises 1 dB for every Ratio dB more at the input; Knee bends the curve smoothly over that many dB around the threshold. Attack and Release set how fast the gain reduction follows the level. Makeup Gain raises the output after compressing, and Mix blends in the dry signal for parallel compression.",
        params: &[
            p("threshold", "Threshold", -60.0, 0.0, -18.0, "dB"),
            p("ratio", "Ratio", 1.0, 20.0, 4.0, ":1"),
            p("attack", "Attack", 0.1, 200.0, 10.0, "ms"),
            p("release", "Release", 10.0, 2000.0, 150.0, "ms"),
            p("knee", "Knee", 0.0, 24.0, 6.0, "dB"),
            p("makeup", "Makeup Gain", -12.0, 24.0, 0.0, "dB"),
            p("mix", "Mix", 0.0, 1.0, 1.0, ""),
        ],
    },
    PluginInfo {
        kind: "limiter",
        name: "Limiter",
        category: Effect,
        group: "Dynamics",
        description: "Look-ahead brickwall limiter with input gain and output ceiling. True Peak On also measures the peaks between samples (4x oversampling) and keeps them under the ceiling, as streaming services measure them; Off limits the sample peaks only. Use it last on the master, with True Peak On, to reach a loudness target.",
        params: &[
            p("gain", "Input Gain", 0.0, 24.0, 0.0, "dB"),
            p("ceiling", "Output Ceiling", -12.0, 0.0, -1.0, "dB"),
            p("release", "Release", 1.0, 1000.0, 60.0, "ms"),
            c("true_peak", "True Peak", 0.0, ONOFF),
        ],
    },
    PluginInfo {
        kind: "noise_gate",
        name: "Noise Gate",
        category: Effect,
        group: "Dynamics",
        description: "Noise gate: while the level stays under Threshold the signal is turned down by Reduction dB (-90 dB closes it). Attack sets how fast it opens, Hold how long it stays open after the level drops, and Release how fast it closes.",
        params: &[
            p("threshold", "Threshold", -90.0, 0.0, -50.0, "dB"),
            p("reduction", "Reduction", -90.0, 0.0, -60.0, "dB"),
            p("attack", "Attack", 0.1, 50.0, 1.0, "ms"),
            p("hold", "Hold", 0.0, 500.0, 20.0, "ms"),
            p("release", "Release", 5.0, 2000.0, 100.0, "ms"),
        ],
    },
    PluginInfo {
        kind: "de_esser",
        name: "De-Esser",
        category: Effect,
        group: "Dynamics",
        description: "Tames harsh S and T sounds on vocals: when the band above Frequency rises over Threshold it is turned down, by at most Range. Split turns down only that band, Wideband the whole signal; Listen plays the band being detected.",
        params: &[
            p("freq", "Frequency", 2000.0, 12000.0, 6000.0, "Hz"),
            p("threshold", "Threshold", -60.0, 0.0, -24.0, "dB"),
            p("range", "Range", 0.0, 24.0, 8.0, "dB"),
            c("mode", "Mode", 0.0, &["Split", "Wideband"]),
            c("listen", "Listen", 0.0, ONOFF),
        ],
    },
    PluginInfo {
        kind: "stereo_delay",
        name: "Stereo Delay",
        category: Effect,
        group: "Delay",
        description: "Tempo-synced stereo delay with feedback filtering and ping-pong crossfeed. With Tempo Sync on, Left Time and Right Time are in beats (quarter notes: 0.75 = dotted eighth, 0.5 = eighth); with it off, Time (unsynced) is the left delay in ms and the right one is 3/4 of it. Feedback sets how much of each repeat comes back (0.5 = each repeat 6 dB lower), Ping-Pong how much crosses to the other side, Low Cut and High Cut thin the repeats.",
        params: &[
            c("sync", "Tempo Sync", 1.0, ONOFF),
            p("time_l", "Left Time", 0.0625, 4.0, 0.75, "beats"),
            p("time_r", "Right Time", 0.0625, 4.0, 0.5, "beats"),
            p("time_ms", "Time (unsynced)", 1.0, 2000.0, 350.0, "ms"),
            p("feedback", "Feedback", 0.0, 0.95, 0.35, ""),
            p("crossfeed", "Ping-Pong", 0.0, 1.0, 0.3, ""),
            p("low_cut", "Low Cut", 20.0, 2000.0, 150.0, "Hz"),
            p("high_cut", "High Cut", 500.0, 20000.0, 7000.0, "Hz"),
            p("mix", "Mix", 0.0, 1.0, 0.25, ""),
        ],
    },
    PluginInfo {
        kind: "tape_delay",
        name: "Tape Delay",
        category: Effect,
        group: "Delay",
        description: "Vintage tape echo, always in time with the project: Time is in beats (0.75 = dotted eighth). Feedback sets how much of each repeat comes back, Saturation drives the tape (with high Saturation the repeats fade slowly or hold), Tone darkens every repeat, Wow/Flutter wobbles the pitch.",
        params: &[
            p("time", "Time", 0.0625, 4.0, 0.5, "beats"),
            p("feedback", "Feedback", 0.0, 1.0, 0.4, ""),
            p("wow", "Wow/Flutter", 0.0, 1.0, 0.3, ""),
            p("tone", "Tone", 0.0, 1.0, 0.4, ""),
            p("saturation", "Saturation", 0.0, 1.0, 0.3, ""),
            p("mix", "Mix", 0.0, 1.0, 0.25, ""),
        ],
    },
    PluginInfo {
        kind: "reverb",
        name: "Reverb",
        category: Effect,
        group: "Reverb",
        description: "Algorithmic stereo reverb (comb/allpass network) with pre-delay, damping, width and low cut. Rooms, halls and plates.",
        params: &[
            p("size", "Room Size", 0.0, 1.0, 0.6, ""),
            p("damping", "Damping", 0.0, 1.0, 0.4, ""),
            p("predelay", "Pre-delay", 0.0, 200.0, 12.0, "ms"),
            p("width", "Width", 0.0, 1.0, 1.0, ""),
            p("low_cut", "Low Cut", 20.0, 1000.0, 120.0, "Hz"),
            p("mix", "Mix", 0.0, 1.0, 0.25, ""),
        ],
    },
    PluginInfo {
        kind: "chorus",
        name: "Chorus",
        category: Effect,
        group: "Modulation",
        description: "Stereo chorus with modulated delay lines.",
        params: &[
            p("rate", "Rate", 0.05, 5.0, 0.8, "Hz"),
            p("depth", "Depth", 0.0, 1.0, 0.5, ""),
            p("delay", "Delay", 2.0, 30.0, 12.0, "ms"),
            p("mix", "Mix", 0.0, 1.0, 0.5, ""),
        ],
    },
    PluginInfo {
        kind: "flanger",
        name: "Flanger",
        category: Effect,
        group: "Modulation",
        description: "Stereo flanger with feedback.",
        params: &[
            p("rate", "Rate", 0.02, 5.0, 0.25, "Hz"),
            p("depth", "Depth", 0.0, 1.0, 0.7, ""),
            p("delay", "Delay", 0.3, 10.0, 2.0, "ms"),
            p("feedback", "Feedback", -0.95, 0.95, 0.5, ""),
            p("mix", "Mix", 0.0, 1.0, 0.5, ""),
        ],
    },
    PluginInfo {
        kind: "phaser",
        name: "Phaser",
        category: Effect,
        group: "Modulation",
        description: "Multi-stage all-pass phaser with feedback.",
        params: &[
            p("rate", "Rate", 0.02, 8.0, 0.4, "Hz"),
            p("depth", "Depth", 0.0, 1.0, 0.7, ""),
            c("stages", "Stages", 1.0, &["2", "4", "6", "8", "12"]),
            p("feedback", "Feedback", 0.0, 0.95, 0.4, ""),
            p("mix", "Mix", 0.0, 1.0, 0.5, ""),
        ],
    },
    PluginInfo {
        kind: "tremolo",
        name: "Tremolo",
        category: Effect,
        group: "Modulation",
        description: "Tempo-synced amplitude modulation, optional stereo auto-pan.",
        params: &[
            p("rate", "Rate", 0.0625, 4.0, 0.25, "beats"),
            p("depth", "Depth", 0.0, 1.0, 0.5, ""),
            c("shape", "Shape", 0.0, &["Sine", "Square", "Triangle"]),
            p("stereo_phase", "Stereo Phase (Auto-pan)", 0.0, 1.0, 0.0, ""),
        ],
    },
    PluginInfo {
        kind: "overdrive",
        name: "Overdrive",
        category: Effect,
        group: "Distortion",
        description: "Waveshaping distortion with selectable character, tone control and dry/wet.",
        params: &[
            c("mode", "Mode", 0.0, &["Soft", "Hard", "Fuzz", "Tube"]),
            p("drive", "Drive", 0.0, 1.0, 0.4, ""),
            p("tone", "Tone", 0.0, 1.0, 0.5, ""),
            p("output", "Output", -24.0, 12.0, -3.0, "dB"),
            p("mix", "Mix", 0.0, 1.0, 1.0, ""),
        ],
    },
    PluginInfo {
        kind: "bitcrusher",
        name: "Bitcrusher",
        category: Effect,
        group: "Distortion",
        description: "Bit depth and sample-rate reduction for lo-fi textures.",
        params: &[
            p("bits", "Resolution", 1.0, 24.0, 8.0, "bit"),
            p("downsample", "Downsampling", 1.0, 64.0, 4.0, "x"),
            p("mix", "Mix", 0.0, 1.0, 1.0, ""),
        ],
    },
    PluginInfo {
        kind: "auto_filter",
        name: "Auto Filter",
        category: Effect,
        group: "Filter",
        description: "Resonant multimode filter with tempo-synced LFO sweep.",
        params: &[
            c("type", "Type", 0.0, FILTERS),
            p("cutoff", "Cutoff", 20.0, 20000.0, 1200.0, "Hz"),
            p("resonance", "Resonance", 0.0, 1.0, 0.4, ""),
            p("lfo_rate", "LFO Rate", 0.0625, 16.0, 1.0, "beats"),
            p("lfo_depth", "LFO Depth", 0.0, 1.0, 0.5, ""),
            p("mix", "Mix", 0.0, 1.0, 1.0, ""),
        ],
    },
    PluginInfo {
        kind: "gain",
        name: "Gain",
        category: Effect,
        group: "Utility",
        description: "Gain, polarity inversion, mono and stereo width utility.",
        params: &[
            p("gain", "Gain", -48.0, 24.0, 0.0, "dB"),
            c("invert_l", "Invert Left", 0.0, ONOFF),
            c("invert_r", "Invert Right", 0.0, ONOFF),
            c("mono", "Mono", 0.0, ONOFF),
            p("width", "Stereo Width", 0.0, 2.0, 1.0, "x"),
        ],
    },
    PluginInfo {
        kind: "stereo_imager",
        name: "Stereo Imager",
        category: Effect,
        group: "Utility",
        description: "Three-band stereo width. Two crossovers at Low Freq and High Freq (24 dB/oct, they sum back flat in level) split the sound into low, mid and high bands, each with its own Width: 0 is mono, 1 leaves the band as it is, 2 doubles its Side. Width changes only the Side (L-R), so the mono sum keeps its level; measure the result with the stereo correlation of analyze_mix (1 is mono, below 0 parts cancel in mono). Mono Bass makes everything below Low Freq mono, which keeps the low end steady. Balance turns the left (positive) or the right (negative) side down. Listen plays only the Mid (L+R) or only the Side (L-R) to find problems; set it back to Stereo. Like any crossover, the bands shift the phase a little around the two frequencies. No latency.",
        params: &[
            p("low_freq", "Low Freq", 40.0, 1000.0, 150.0, "Hz"),
            p("high_freq", "High Freq", 1000.0, 16000.0, 4000.0, "Hz"),
            p("low_width", "Low Width", 0.0, 2.0, 1.0, "x"),
            p("mid_width", "Mid Width", 0.0, 2.0, 1.0, "x"),
            p("high_width", "High Width", 0.0, 2.0, 1.0, "x"),
            c("mono_bass", "Mono Bass", 0.0, ONOFF),
            p("balance", "Balance", -1.0, 1.0, 0.0, ""),
            c("listen", "Listen", 0.0, LISTEN_MS),
            p("output", "Output Gain", -24.0, 24.0, 0.0, "dB"),
        ],
    },
    PluginInfo {
        kind: "exciter",
        name: "Exciter",
        category: Effect,
        group: "Specialized",
        description: "Harmonic exciter: adds saturated high-frequency harmonics above a frequency for presence and air.",
        params: &[
            p("freq", "Frequency", 1000.0, 12000.0, 4000.0, "Hz"),
            p("amount", "Harmonics", 0.0, 1.0, 0.4, ""),
            p("mix", "Mix", 0.0, 1.0, 0.3, ""),
        ],
    },
    // ------------------------------------------------------------------ MIDI effects
    PluginInfo {
        kind: "arpeggiator",
        name: "Arpeggiator",
        category: MidiEffect,
        group: "MIDI",
        description: "Turns held chords into tempo-synced arpeggios.",
        params: &[
            c("rate", "Rate", 2.0, &["1/4", "1/8", "1/16", "1/32", "1/8T", "1/16T"]),
            c("mode", "Mode", 0.0, &["Up", "Down", "Up/Down", "Random", "As Played"]),
            p("octaves", "Octave Range", 1.0, 4.0, 1.0, ""),
            p("gate", "Note Length", 0.05, 1.0, 0.6, ""),
        ],
    },
    PluginInfo {
        kind: "transposer",
        name: "Transposer",
        category: MidiEffect,
        group: "MIDI",
        description: "Transposes notes; optionally snaps them to the project key.",
        params: &[p("semitones", "Transpose", -36.0, 36.0, 0.0, "st"), c("scale_lock", "Lock to Project Key", 0.0, ONOFF)],
    },
    PluginInfo {
        kind: "chord_trigger",
        name: "Chord Trigger",
        category: MidiEffect,
        group: "MIDI",
        description: "Plays a full chord from each single note.",
        params: &[c("chord", "Chord", 0.0, &["Major", "Minor", "Power", "Maj7", "Min7", "Dom7", "Sus2", "Sus4", "Octave", "Diatonic (Key)"])],
    },
    PluginInfo {
        kind: "velocity_processor",
        name: "Velocity Processor",
        category: MidiEffect,
        group: "MIDI",
        description: "Scales, offsets and randomizes note velocities.",
        params: &[
            p("scale", "Scale", 0.0, 2.0, 1.0, "x"),
            p("offset", "Offset", -64.0, 64.0, 0.0, ""),
            p("random", "Random", 0.0, 64.0, 0.0, ""),
        ],
    },
];

pub fn registry() -> &'static [PluginInfo] {
    REGISTRY
}

pub fn find(kind: &str) -> Option<&'static PluginInfo> {
    REGISTRY.iter().find(|p| p.kind == kind)
}

/// The name to show for a plug-in instance. A VST3 is called by the name it reported when it was added (kept in
/// `preset`), or by its bundle file name when the project has none; a built-in by its registry name. An unknown
/// kind (a project saved by a newer version) shows the kind itself.
pub fn plugin_label(p: &crate::model::PluginInstance) -> String {
    if p.kind == "vst3" {
        if let Some(name) = p.preset.as_deref().map(str::trim).filter(|s| !s.is_empty()) {
            return name.to_string();
        }
        return p.vst3_path.as_deref().and_then(vst3_bundle_name).unwrap_or_else(|| "VST3".to_string());
    }
    find(&p.kind).map_or_else(|| p.kind.clone(), |i| i.name.to_string())
}

/// `C:\VST3\Foo Synth.vst3` -> `Foo Synth`: the last path component without a `.vst3` extension (any case).
/// Plain string work, so it reads both `\` and `/` the same way on every platform.
fn vst3_bundle_name(path: &str) -> Option<String> {
    let is_sep = |c: char| c == '\\' || c == '/';
    let file = path.trim_end_matches(is_sep).rsplit(is_sep).next()?;
    let name = match file.len().checked_sub(5).and_then(|i| Some((file.get(..i)?, file.get(i..)?))) {
        Some((stem, ext)) if ext.eq_ignore_ascii_case(".vst3") => stem,
        _ => file,
    };
    let name = name.trim();
    (!name.is_empty()).then(|| name.to_string())
}

pub struct Preset {
    pub name: &'static str,
    pub kind: &'static str,
    pub params: &'static [(&'static str, f32)],
}

pub static PRESETS: &[Preset] = &[
    // Analog synth
    Preset { name: "Init Synth", kind: "analog_synth", params: &[] },
    Preset { name: "Fat Saw Lead", kind: "analog_synth", params: &[("osc1_wave", 0.0), ("osc2_wave", 0.0), ("osc2_detune", 12.0), ("cutoff", 4500.0), ("resonance", 0.25), ("filter_env", 0.35), ("attack", 0.003), ("sustain", 0.85), ("release", 0.2), ("glide", 0.04), ("lfo_pitch", 0.05), ("lfo_rate", 5.5)] },
    Preset { name: "Square Lead", kind: "analog_synth", params: &[("osc1_wave", 1.0), ("osc2_wave", 1.0), ("osc2_semi", 12.0), ("osc_mix", 0.3), ("cutoff", 3200.0), ("resonance", 0.15), ("release", 0.15)] },
    Preset { name: "Acid Bass", kind: "analog_synth", params: &[("osc1_wave", 0.0), ("osc_mix", 0.0), ("cutoff", 380.0), ("resonance", 0.82), ("filter_env", 0.75), ("f_decay", 0.18), ("f_sustain", 0.0), ("decay", 0.25), ("sustain", 0.6), ("release", 0.06), ("glide", 0.06), ("key_track", 0.3), ("unison_spread", 0.0)] },
    Preset { name: "Sub Bass", kind: "analog_synth", params: &[("osc1_wave", 3.0), ("osc2_wave", 2.0), ("osc2_semi", 0.0), ("osc2_detune", 0.0), ("osc_mix", 0.25), ("sub_level", 0.6), ("cutoff", 600.0), ("resonance", 0.0), ("filter_env", 0.1), ("attack", 0.004), ("sustain", 1.0), ("release", 0.08), ("unison_spread", 0.0), ("level", -6.0)] },
    Preset { name: "Synth Bass", kind: "analog_synth", params: &[("osc1_wave", 0.0), ("osc2_wave", 1.0), ("osc2_semi", -12.0), ("osc2_detune", 4.0), ("cutoff", 900.0), ("resonance", 0.3), ("filter_env", 0.45), ("f_decay", 0.25), ("f_sustain", 0.2), ("decay", 0.3), ("sustain", 0.7), ("release", 0.08), ("unison_spread", 0.05)] },
    Preset { name: "Warm Pad", kind: "analog_synth", params: &[("osc1_wave", 0.0), ("osc2_wave", 0.0), ("osc2_detune", 14.0), ("cutoff", 1400.0), ("resonance", 0.1), ("filter_env", 0.15), ("f_attack", 1.2), ("f_decay", 2.0), ("f_sustain", 0.5), ("attack", 0.9), ("decay", 1.5), ("sustain", 0.85), ("release", 2.2), ("lfo_cutoff", 0.15), ("lfo_rate", 0.3), ("unison_spread", 0.8), ("level", -12.0)] },
    Preset { name: "String Ensemble", kind: "analog_synth", params: &[("osc1_wave", 0.0), ("osc2_wave", 0.0), ("osc2_detune", 9.0), ("cutoff", 3200.0), ("filter_env", 0.05), ("attack", 0.35), ("decay", 0.8), ("sustain", 0.9), ("release", 0.9), ("lfo_pitch", 0.04), ("lfo_rate", 5.0), ("unison_spread", 0.9), ("level", -12.0)] },
    Preset { name: "Pluck Keys", kind: "analog_synth", params: &[("osc1_wave", 0.0), ("osc2_wave", 1.0), ("osc2_semi", 12.0), ("osc_mix", 0.35), ("cutoff", 700.0), ("resonance", 0.3), ("filter_env", 0.7), ("f_decay", 0.22), ("f_sustain", 0.0), ("decay", 0.45), ("sustain", 0.0), ("release", 0.3), ("unison_spread", 0.5)] },
    Preset { name: "Brass Stab", kind: "analog_synth", params: &[("osc1_wave", 0.0), ("osc2_wave", 0.0), ("osc2_detune", 8.0), ("cutoff", 900.0), ("resonance", 0.15), ("filter_env", 0.6), ("f_attack", 0.06), ("f_decay", 0.4), ("f_sustain", 0.4), ("attack", 0.03), ("sustain", 0.8), ("release", 0.2)] },
    Preset { name: "Soft Sine", kind: "analog_synth", params: &[("osc1_wave", 3.0), ("osc2_wave", 3.0), ("osc2_semi", 12.0), ("osc_mix", 0.2), ("cutoff", 8000.0), ("filter_env", 0.0), ("attack", 0.02), ("release", 0.6)] },
    // FM
    Preset { name: "Electric Piano", kind: "fm_synth", params: &[("ratio", 1.0), ("index", 2.2), ("index_decay", 0.9), ("index_sustain", 0.15), ("decay", 2.5), ("sustain", 0.25), ("release", 0.5), ("tremolo", 0.2)] },
    Preset { name: "DX Bell", kind: "fm_synth", params: &[("ratio", 3.5), ("index", 5.0), ("index_decay", 2.5), ("index_sustain", 0.1), ("decay", 4.0), ("sustain", 0.0), ("release", 2.0)] },
    Preset { name: "Marimba", kind: "fm_synth", params: &[("ratio", 4.0), ("index", 3.0), ("index_decay", 0.08), ("index_sustain", 0.0), ("decay", 0.5), ("sustain", 0.0), ("release", 0.3)] },
    Preset { name: "FM Bass", kind: "fm_synth", params: &[("ratio", 1.0), ("index", 4.0), ("index_decay", 0.2), ("index_sustain", 0.2), ("feedback", 0.3), ("decay", 0.6), ("sustain", 0.6), ("release", 0.1)] },
    // Drums
    Preset { name: "Studio Kit", kind: "drum_synth", params: &[] },
    Preset { name: "808 Kit", kind: "drum_synth", params: &[("kick_tune", 45.0), ("kick_decay", 1.2), ("kick_punch", 0.3), ("snare_tune", 220.0), ("snare_snappy", 0.8), ("hat_tone", 0.8), ("hat_decay", 0.04)] },
    Preset { name: "Tight Kit", kind: "drum_synth", params: &[("kick_tune", 60.0), ("kick_decay", 0.25), ("kick_punch", 0.8), ("snare_decay", 0.12), ("hat_decay", 0.03), ("open_hat_decay", 0.25)] },
    Preset { name: "Lo-Fi Kit", kind: "drum_synth", params: &[("kick_tune", 55.0), ("kick_decay", 0.35), ("snare_tune", 160.0), ("snare_snappy", 0.4), ("hat_tone", 0.3), ("drive", 0.5), ("level", -6.0)] },
    // Organ
    Preset { name: "Rock Organ", kind: "organ", params: &[("db_16", 8.0), ("db_5_1_3", 8.0), ("db_8", 8.0), ("db_4", 6.0), ("drive", 0.5), ("rotary", 2.0)] },
    Preset { name: "Jazz Organ", kind: "organ", params: &[("db_16", 8.0), ("db_5_1_3", 8.0), ("db_8", 8.0), ("db_4", 0.0), ("percussion", 0.8), ("drive", 0.1), ("rotary", 1.0)] },
    Preset { name: "Gospel Full", kind: "organ", params: &[("db_16", 8.0), ("db_5_1_3", 8.0), ("db_8", 8.0), ("db_4", 8.0), ("db_2_2_3", 8.0), ("db_2", 8.0), ("db_1_3_5", 8.0), ("db_1_1_3", 8.0), ("db_1", 8.0), ("drive", 0.3), ("rotary", 2.0)] },
    // Pluck
    Preset { name: "Nylon Guitar", kind: "pluck_synth", params: &[("decay", 3.0), ("brightness", 0.45), ("body", 0.6)] },
    Preset { name: "Steel Guitar", kind: "pluck_synth", params: &[("decay", 4.0), ("brightness", 0.8), ("pick_position", 0.12), ("body", 0.4)] },
    Preset { name: "Harp", kind: "pluck_synth", params: &[("decay", 5.0), ("brightness", 0.55), ("pick_position", 0.5), ("body", 0.2)] },
    Preset { name: "Plucked Bass", kind: "pluck_synth", params: &[("decay", 1.5), ("brightness", 0.3), ("pick_position", 0.3), ("body", 0.7), ("level", -3.0)] },
    // Effects
    Preset { name: "Small Room", kind: "reverb", params: &[("size", 0.3), ("damping", 0.5), ("predelay", 5.0), ("mix", 0.2)] },
    Preset { name: "Large Hall", kind: "reverb", params: &[("size", 0.88), ("damping", 0.35), ("predelay", 30.0), ("mix", 0.3)] },
    Preset { name: "Bright Plate", kind: "reverb", params: &[("size", 0.65), ("damping", 0.1), ("predelay", 10.0), ("low_cut", 250.0), ("mix", 0.25)] },
    Preset { name: "Reverb Return 100%", kind: "reverb", params: &[("size", 0.75), ("damping", 0.35), ("predelay", 20.0), ("mix", 1.0)] },
    Preset { name: "Quarter Ping-Pong", kind: "stereo_delay", params: &[("time_l", 1.0), ("time_r", 0.5), ("crossfeed", 0.8), ("feedback", 0.4)] },
    Preset { name: "Slapback", kind: "stereo_delay", params: &[("sync", 0.0), ("time_ms", 110.0), ("feedback", 0.1), ("crossfeed", 0.0), ("mix", 0.2)] },
    Preset { name: "Delay Return 100%", kind: "stereo_delay", params: &[("time_l", 0.75), ("time_r", 0.5), ("feedback", 0.4), ("mix", 1.0)] },
    Preset { name: "Vocal Compressor", kind: "compressor", params: &[("threshold", -20.0), ("ratio", 3.5), ("attack", 5.0), ("release", 120.0), ("makeup", 4.0)] },
    Preset { name: "Drum Bus Glue", kind: "compressor", params: &[("threshold", -14.0), ("ratio", 2.0), ("attack", 30.0), ("release", 100.0), ("knee", 10.0), ("makeup", 2.0)] },
    Preset { name: "Parallel Smash", kind: "compressor", params: &[("threshold", -35.0), ("ratio", 10.0), ("attack", 1.0), ("release", 80.0), ("makeup", 12.0), ("mix", 0.35)] },
    Preset { name: "Master Bus Glue", kind: "compressor", params: &[("threshold", -10.0), ("ratio", 1.8), ("attack", 30.0), ("release", 200.0), ("knee", 12.0), ("makeup", 1.0)] },
    Preset { name: "Vocal De-Ess", kind: "de_esser", params: &[("freq", 6500.0), ("threshold", -30.0), ("range", 8.0), ("mode", 0.0)] },
    Preset { name: "Soft De-Ess", kind: "de_esser", params: &[("freq", 7500.0), ("threshold", -24.0), ("range", 4.0), ("mode", 0.0)] },
    Preset { name: "Vocal Presence", kind: "channel_eq", params: &[("hp_on", 1.0), ("hp_freq", 90.0), ("mid1_gain", -2.5), ("mid1_freq", 300.0), ("mid2_gain", 3.0), ("mid2_freq", 3500.0), ("high_gain", 2.0), ("high_freq", 10000.0)] },
    Preset { name: "Kick Punch", kind: "channel_eq", params: &[("low_gain", 3.0), ("low_freq", 60.0), ("mid1_gain", -4.0), ("mid1_freq", 350.0), ("mid2_gain", 3.0), ("mid2_freq", 4000.0)] },
    Preset { name: "Low Cut 100", kind: "channel_eq", params: &[("hp_on", 1.0), ("hp_freq", 100.0)] },
    Preset { name: "Master Polish", kind: "channel_eq", params: &[("hp_on", 1.0), ("hp_freq", 25.0), ("low_gain", 1.0), ("low_freq", 80.0), ("mid1_gain", -1.0), ("mid1_freq", 350.0), ("high_gain", 1.5), ("high_freq", 12000.0)] },
    Preset { name: "Tame Resonance", kind: "dynamic_eq", params: &[("b3_freq", 3000.0), ("b3_q", 5.0), ("b3_threshold", -30.0), ("b3_range", -8.0), ("attack", 2.0), ("release", 80.0)] },
    Preset { name: "Vocal Mud Control", kind: "dynamic_eq", params: &[("b2_freq", 300.0), ("b2_q", 1.2), ("b2_threshold", -30.0), ("b2_range", -6.0), ("attack", 10.0), ("release", 150.0)] },
    Preset { name: "Bass Boom Control", kind: "dynamic_eq", params: &[("b1_type", 1.0), ("b1_freq", 150.0), ("b1_threshold", -20.0), ("b1_range", -8.0), ("attack", 15.0), ("release", 200.0)] },
    Preset { name: "Mono Low End", kind: "stereo_imager", params: &[("low_freq", 120.0), ("mono_bass", 1.0)] },
    Preset { name: "Wide Synth", kind: "stereo_imager", params: &[("low_freq", 200.0), ("low_width", 0.7), ("mid_width", 1.4), ("high_width", 1.6)] },
    Preset { name: "Master Width Subtle", kind: "stereo_imager", params: &[("low_freq", 100.0), ("mono_bass", 1.0), ("mid_width", 1.05), ("high_width", 1.15)] },
    Preset { name: "Warm Drive", kind: "overdrive", params: &[("mode", 3.0), ("drive", 0.3), ("tone", 0.4)] },
    Preset { name: "Fuzz Box", kind: "overdrive", params: &[("mode", 2.0), ("drive", 0.85), ("tone", 0.6), ("output", -9.0)] },
    Preset { name: "Lo-Fi", kind: "bitcrusher", params: &[("bits", 10.0), ("downsample", 3.0), ("mix", 0.6)] },
    Preset { name: "Filter Sweep", kind: "auto_filter", params: &[("cutoff", 800.0), ("resonance", 0.6), ("lfo_rate", 8.0), ("lfo_depth", 0.8)] },
    Preset { name: "Master Limiter", kind: "limiter", params: &[("gain", 3.0), ("ceiling", -1.0), ("release", 80.0), ("true_peak", 1.0)] },
];

pub fn find_preset(kind: &str, name: &str) -> Option<&'static Preset> {
    PRESETS.iter().find(|p| p.kind == kind && p.name.eq_ignore_ascii_case(name))
        .or_else(|| PRESETS.iter().find(|p| p.name.eq_ignore_ascii_case(name)))
}

pub fn presets_for(kind: &str) -> impl Iterator<Item = &'static Preset> + '_ {
    PRESETS.iter().filter(move |p| p.kind == kind)
}

/// Every parameter of `p` at its default: the registry's defaults for a built-in plug-in, the reported defaults for
/// a VST3. One `SetPluginParams` with this map is the plug-in window's Default button (one undo step).
pub fn default_params(p: &crate::model::PluginInstance) -> std::collections::BTreeMap<String, f32> {
    if p.kind == "vst3" {
        return p.vst3_params.iter().map(|v| (v.id.clone(), v.default.clamp(0.0, 1.0))).collect();
    }
    find(&p.kind).map(|i| i.params.iter().map(|pi| (pi.id.to_string(), pi.default)).collect()).unwrap_or_default()
}

/// A track patch: instrument (optional) + insert chain, like a library entry.
pub struct Patch {
    pub name: &'static str,
    pub category: &'static str,
    pub instrument: Option<(&'static str, &'static str)>,
    pub effects: &'static [(&'static str, &'static str)],
}

pub static PATCHES: &[Patch] = &[
    Patch { name: "Pop Drum Kit", category: "Drums", instrument: Some(("drum_synth", "Studio Kit")), effects: &[("compressor", "Drum Bus Glue")] },
    Patch { name: "808 Trap Kit", category: "Drums", instrument: Some(("drum_synth", "808 Kit")), effects: &[("compressor", "Drum Bus Glue")] },
    Patch { name: "Lo-Fi Beat Kit", category: "Drums", instrument: Some(("drum_synth", "Lo-Fi Kit")), effects: &[("bitcrusher", "Lo-Fi")] },
    Patch { name: "Deep Sub Bass", category: "Bass", instrument: Some(("analog_synth", "Sub Bass")), effects: &[] },
    Patch { name: "Classic Synth Bass", category: "Bass", instrument: Some(("analog_synth", "Synth Bass")), effects: &[("compressor", "Drum Bus Glue")] },
    Patch { name: "Acid Line", category: "Bass", instrument: Some(("analog_synth", "Acid Bass")), effects: &[("overdrive", "Warm Drive"), ("stereo_delay", "Quarter Ping-Pong")] },
    Patch { name: "Fingerpicked Bass", category: "Bass", instrument: Some(("pluck_synth", "Plucked Bass")), effects: &[] },
    Patch { name: "Stage Electric Piano", category: "Keyboards", instrument: Some(("fm_synth", "Electric Piano")), effects: &[("chorus", ""), ("reverb", "Small Room")] },
    Patch { name: "Rock Organ", category: "Keyboards", instrument: Some(("organ", "Rock Organ")), effects: &[] },
    Patch { name: "Jazz Organ", category: "Keyboards", instrument: Some(("organ", "Jazz Organ")), effects: &[("reverb", "Small Room")] },
    Patch { name: "Bell Keys", category: "Keyboards", instrument: Some(("fm_synth", "DX Bell")), effects: &[("reverb", "Large Hall")] },
    Patch { name: "Supersaw Lead", category: "Synth Lead", instrument: Some(("analog_synth", "Fat Saw Lead")), effects: &[("stereo_delay", "Quarter Ping-Pong"), ("reverb", "Bright Plate")] },
    Patch { name: "Retro Square Lead", category: "Synth Lead", instrument: Some(("analog_synth", "Square Lead")), effects: &[("chorus", ""), ("stereo_delay", "Slapback")] },
    Patch { name: "Lush Pad", category: "Synth Pad", instrument: Some(("analog_synth", "Warm Pad")), effects: &[("chorus", ""), ("reverb", "Large Hall")] },
    Patch { name: "Strings", category: "Orchestral", instrument: Some(("analog_synth", "String Ensemble")), effects: &[("reverb", "Large Hall")] },
    Patch { name: "Pluck Arp", category: "Synth Pluck", instrument: Some(("analog_synth", "Pluck Keys")), effects: &[("stereo_delay", "Quarter Ping-Pong"), ("reverb", "Bright Plate")] },
    Patch { name: "Brass Section", category: "Synth Brass", instrument: Some(("analog_synth", "Brass Stab")), effects: &[("reverb", "Small Room")] },
    Patch { name: "Nylon Guitar", category: "Guitar", instrument: Some(("pluck_synth", "Nylon Guitar")), effects: &[("reverb", "Small Room")] },
    Patch { name: "Steel Guitar", category: "Guitar", instrument: Some(("pluck_synth", "Steel Guitar")), effects: &[("chorus", ""), ("reverb", "Small Room")] },
    Patch { name: "Harp", category: "Orchestral", instrument: Some(("pluck_synth", "Harp")), effects: &[("reverb", "Large Hall")] },
    Patch { name: "Mallets", category: "Mallets", instrument: Some(("fm_synth", "Marimba")), effects: &[("reverb", "Small Room")] },
    Patch { name: "Lead Vocal", category: "Audio", instrument: None, effects: &[("channel_eq", "Vocal Presence"), ("compressor", "Vocal Compressor"), ("reverb", "Bright Plate")] },
    Patch { name: "Clean Audio", category: "Audio", instrument: None, effects: &[("channel_eq", "Low Cut 100")] },
];

pub fn find_patch(name: &str) -> Option<&'static Patch> {
    PATCHES.iter().find(|p| p.name.eq_ignore_ascii_case(name))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn channel_eq_description_explains_its_bands() {
        let d = find("channel_eq").unwrap().description;
        assert!(d.starts_with("Six-band parametric equalizer"), "{d}");
        for word in ["Mid 1", "Q", "High Pass", "0 dB does nothing", "Output Gain"] {
            assert!(d.contains(word), "missing {word:?} in {d}");
        }
    }

    #[test]
    fn dynamics_descriptions_explain_their_parameters() {
        for (kind, words) in [("compressor", &["Threshold", "Ratio", "Knee", "Makeup Gain", "Mix"][..]), ("noise_gate", &["Threshold", "Reduction", "Hold", "Release"][..])] {
            let d = find(kind).unwrap().description;
            for word in words {
                assert!(d.contains(word), "{kind}: missing {word:?} in {d}");
            }
            assert!(d.len() < 450, "{kind}: {} characters", d.len());
        }
    }

    fn inst(kind: &str, preset: Option<&str>, path: Option<&str>) -> crate::model::PluginInstance {
        crate::model::PluginInstance {
            id: 1,
            kind: kind.to_string(),
            params: Default::default(),
            bypass: false,
            preset: preset.map(str::to_string),
            sample: None,
            vst3_path: path.map(str::to_string),
            vst3_params: vec![],
            vst3_state: None,
        }
    }

    #[test]
    fn plugin_label_uses_the_name_a_vst3_reported() {
        assert_eq!(plugin_label(&inst("vst3", Some("Foo Synth"), Some(r"C:\VST3\Other.vst3"))), "Foo Synth");
        assert_eq!(plugin_label(&inst("vst3", Some("  Pad  "), Some(r"C:\VST3\Other.vst3"))), "Pad");
    }

    #[test]
    fn plugin_label_falls_back_to_the_vst3_bundle_name() {
        assert_eq!(plugin_label(&inst("vst3", None, Some(r"C:\VST3\Foo Synth.vst3"))), "Foo Synth");
        assert_eq!(plugin_label(&inst("vst3", None, Some("C:/VST3/Bar.VST3"))), "Bar");
        assert_eq!(plugin_label(&inst("vst3", None, Some(r"C:\VST3\Baz.vst3\"))), "Baz");
        assert_eq!(plugin_label(&inst("vst3", None, Some(r"C:\VST3\Ñandú.vst3"))), "Ñandú");
        assert_eq!(plugin_label(&inst("vst3", Some(""), Some(r"C:\VST3\Foo.vst3"))), "Foo");
        assert_eq!(plugin_label(&inst("vst3", None, Some(r"C:\plugins\evil.dll"))), "evil.dll");
        assert_eq!(plugin_label(&inst("vst3", None, None)), "VST3");
        assert_eq!(plugin_label(&inst("vst3", None, Some(r"C:\VST3\"))), "VST3");
    }

    #[test]
    fn plugin_label_uses_the_registry_name_for_built_ins() {
        assert_eq!(plugin_label(&inst("compressor", Some("Gentle"), None)), find("compressor").unwrap().name);
        assert_eq!(plugin_label(&inst("future_fx", None, None)), "future_fx");
    }

    #[test]
    fn de_esser_is_registered_with_its_presets() {
        let info = find("de_esser").expect("de_esser in the registry");
        assert_eq!(info.category, PluginCategory::Effect);
        assert_eq!(info.group, "Dynamics");
        let ids: Vec<&str> = info.params.iter().map(|p| p.id).collect();
        assert_eq!(ids, ["freq", "threshold", "range", "mode", "listen"]);
        let mode = info.params.iter().find(|p| p.id == "mode").unwrap();
        assert_eq!(mode.choices, ["Split", "Wideband"]);
        let names: Vec<&str> = presets_for("de_esser").map(|p| p.name).collect();
        assert_eq!(names, ["Vocal De-Ess", "Soft De-Ess"]);
        for preset in presets_for("de_esser") {
            for (id, v) in preset.params {
                let p = info.params.iter().find(|p| p.id == *id).unwrap_or_else(|| panic!("{}: no parameter {id}", preset.name));
                assert!((p.min..=p.max).contains(v), "{}: {id} = {v} out of range", preset.name);
            }
        }
        let lead = find_patch("Lead Vocal").unwrap();
        assert_eq!(lead.effects.iter().map(|e| e.0).collect::<Vec<_>>(), ["channel_eq", "compressor", "reverb"]);
    }

    #[test]
    fn dynamic_eq_is_registered_with_its_presets() {
        let info = find("dynamic_eq").expect("dynamic_eq in the registry");
        assert_eq!(info.category, PluginCategory::Effect);
        assert_eq!(info.group, "EQ");
        assert_eq!(info.name, "Dynamic EQ");
        let mut expected: Vec<String> = Vec::new();
        for n in 1..=4 {
            for k in ["on", "type", "freq", "gain", "q", "threshold", "range"] {
                expected.push(format!("b{n}_{k}"));
            }
        }
        expected.extend(["attack", "release", "output"].map(String::from));
        let ids: Vec<&str> = info.params.iter().map(|p| p.id).collect();
        assert_eq!(ids, expected);
        for p in info.params {
            assert!((p.min..=p.max).contains(&p.default), "{}: default out of range", p.id);
            if p.id.ends_with("_type") {
                assert_eq!(p.choices, ["Bell", "Low Shelf", "High Shelf"]);
            }
            if p.id.ends_with("_freq") {
                assert_eq!((p.min, p.max, p.unit), (20.0, 20000.0, "Hz"), "{}", p.id);
            }
            if p.id.ends_with("_threshold") {
                assert_eq!((p.min, p.max, p.unit), (-60.0, 0.0, "dB"), "{}", p.id);
            }
            if p.id.ends_with("_range") {
                assert_eq!((p.min, p.max, p.default, p.unit), (-24.0, 24.0, 0.0, "dB"), "{}", p.id);
            }
            if p.id.ends_with("_gain") {
                assert_eq!(p.default, 0.0, "{}: a new Dynamic EQ must not change the sound", p.id);
            }
        }
        assert!(info.description.contains("Threshold") && info.description.contains("Range"), "the description explains Threshold and Range");
        let names: Vec<&str> = presets_for("dynamic_eq").map(|p| p.name).collect();
        assert_eq!(names, ["Tame Resonance", "Vocal Mud Control", "Bass Boom Control"]);
        for preset in presets_for("dynamic_eq") {
            assert!(preset.params.iter().any(|(id, v)| id.ends_with("_range") && *v < 0.0), "{}: no band works", preset.name);
            for (id, v) in preset.params {
                let p = info.params.iter().find(|p| p.id == *id).unwrap_or_else(|| panic!("{}: no parameter {id}", preset.name));
                assert!((p.min..=p.max).contains(v), "{}: {id} = {v} out of range", preset.name);
            }
        }
    }

    #[test]
    fn stereo_imager_is_registered_with_its_presets() {
        let info = find("stereo_imager").expect("stereo_imager in the registry");
        assert_eq!(info.category, PluginCategory::Effect);
        assert_eq!(info.group, "Utility");
        assert_eq!(info.name, "Stereo Imager");
        let ids: Vec<&str> = info.params.iter().map(|p| p.id).collect();
        assert_eq!(ids, ["low_freq", "high_freq", "low_width", "mid_width", "high_width", "mono_bass", "balance", "listen", "output"]);
        for p in info.params {
            assert!((p.min..=p.max).contains(&p.default), "{}: default out of range", p.id);
            if p.id.ends_with("_width") {
                assert_eq!((p.min, p.max, p.default, p.unit), (0.0, 2.0, 1.0, "x"), "{}: a new Stereo Imager must not change the width", p.id);
            }
        }
        let param = |id: &str| info.params.iter().find(|p| p.id == id).unwrap();
        assert_eq!((param("low_freq").min, param("low_freq").max, param("low_freq").unit), (40.0, 1000.0, "Hz"));
        assert_eq!((param("high_freq").min, param("high_freq").max, param("high_freq").unit), (1000.0, 16000.0, "Hz"));
        assert_eq!(param("mono_bass").choices, ["Off", "On"]);
        assert_eq!(param("mono_bass").default, 0.0);
        assert_eq!(param("listen").choices, ["Stereo", "Mid", "Side"]);
        assert_eq!(param("listen").default, 0.0);
        for word in ["Width", "Mono Bass", "Listen", "phase", "analyze_mix", "No latency"] {
            assert!(info.description.contains(word), "the description must mention {word}");
        }
        let names: Vec<&str> = presets_for("stereo_imager").map(|p| p.name).collect();
        assert_eq!(names, ["Mono Low End", "Wide Synth", "Master Width Subtle"]);
        for preset in presets_for("stereo_imager") {
            assert!(!preset.params.is_empty(), "{}", preset.name);
            for (id, v) in preset.params {
                let p = info.params.iter().find(|p| p.id == *id).unwrap_or_else(|| panic!("{}: no parameter {id}", preset.name));
                assert!((p.min..=p.max).contains(v), "{}: {id} = {v} out of range", preset.name);
            }
        }
        let mono = find_preset("stereo_imager", "Mono Low End").unwrap();
        assert!(mono.params.contains(&("mono_bass", 1.0)));
    }

    #[test]
    fn default_params_cover_every_parameter() {
        for info in registry() {
            let d = default_params(&inst(info.kind, None, None));
            assert_eq!(d.len(), info.params.len(), "{}", info.kind);
            for pi in info.params {
                assert_eq!(d.get(pi.id), Some(&pi.default), "{}: {}", info.kind, pi.id);
            }
        }
        let mut v = inst("vst3", Some("X"), Some(r"C:\VST3\X.vst3"));
        let vp = |id: &str, default: f32| crate::model::Vst3Param { id: id.into(), name: id.into(), default, unit: String::new() };
        v.vst3_params = vec![vp("p1", 0.3), vp("p2", 1.7)];
        let d = default_params(&v);
        assert_eq!(d.into_iter().collect::<Vec<_>>(), [("p1".to_string(), 0.3), ("p2".to_string(), 1.0)]);
        assert!(default_params(&inst("from_the_future", None, None)).is_empty());
    }

    #[test]
    fn limiter_has_a_true_peak_switch_off_by_default() {
        let info = find("limiter").expect("limiter in the registry");
        let ids: Vec<&str> = info.params.iter().map(|p| p.id).collect();
        assert_eq!(ids, ["gain", "ceiling", "release", "true_peak"]);
        let tp = info.params.iter().find(|p| p.id == "true_peak").unwrap();
        assert_eq!(tp.choices, ["Off", "On"]);
        assert_eq!(tp.default, 0.0, "a limiter saved before True Peak must stay as it was");
        assert!(info.description.contains("True Peak") && info.description.contains("between samples"), "the description explains True Peak");
        let preset = find_preset("limiter", "Master Limiter").expect("Master Limiter preset");
        assert!(preset.params.contains(&("true_peak", 1.0)), "Master Limiter turns True Peak On");
        for (id, v) in preset.params {
            let p = info.params.iter().find(|p| p.id == *id).unwrap_or_else(|| panic!("no parameter {id}"));
            assert!((p.min..=p.max).contains(v), "{id} = {v} out of range");
        }
    }

    #[test]
    fn delay_descriptions_explain_their_times() {
        let stereo = find("stereo_delay").unwrap().description;
        for word in ["Tempo Sync", "3/4", "Feedback", "Ping-Pong", "beats"] {
            assert!(stereo.contains(word), "stereo_delay: missing {word:?} in {stereo}");
        }
        let tape = find("tape_delay").unwrap().description;
        for word in ["beats", "Saturation", "Tone", "Feedback"] {
            assert!(tape.contains(word), "tape_delay: missing {word:?} in {tape}");
        }
        for d in [stereo, tape] {
            assert!(d.len() < 450, "{} characters: {d}", d.len());
        }
    }
}
