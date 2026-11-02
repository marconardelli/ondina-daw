//! Project data model. Everything that is saved in a project file lives here.
//!
//! Time is expressed in **beats** (quarter notes) from the project start, as `f64`.
//! The tempo map converts beats to seconds.

use std::collections::BTreeMap;

use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

use crate::tempo::TempoMap;
use crate::theory::{Key, ScaleKind};

pub type Id = u64;

/// Reserved id used to address the master channel in channel-level commands.
pub const MASTER_ID: Id = 0;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Project {
    pub name: String,
    pub tempo: TempoMap,
    pub time_signature: TimeSignature,
    pub key: Key,
    pub tracks: Vec<Track>,
    pub master: Channel,
    pub markers: Vec<Marker>,
    pub chords: Vec<ChordEvent>,
    pub cycle: Cycle,
    pub metronome: bool,
    /// Metronome click level in dB (0 = full scale). Projects saved before the level existed get the level the
    /// click always had.
    #[serde(default = "default_metronome_level_db")]
    pub metronome_level_db: f32,
    /// The metronome clicks only while recording; the count-in always clicks.
    #[serde(default)]
    pub metronome_only_recording: bool,
    pub count_in_bars: u32,
    pub audio_files: Vec<AudioFileRef>,
    /// Free-form notes, e.g. lyrics or an AI-generated brief.
    #[serde(default)]
    pub notes: String,
    pub next_id: Id,
    /// Clip Grid (cells × scenes).
    #[serde(default)]
    pub live: LiveLoops,
    /// Output format for surround / spatial mixing.
    #[serde(default)]
    pub surround: SurroundFormat,
    /// Linked movie for scoring to picture.
    #[serde(default)]
    pub movie: Option<MovieRef>,
    /// SMPTE frame rate used for timecode display.
    #[serde(default = "default_fps")]
    pub frame_rate: f64,
    /// MIDI input routing (Environment): keyboard splits, layers, transforms.
    #[serde(default)]
    pub midi_routes: Vec<MidiRoute>,
}

fn default_fps() -> f64 {
    25.0
}

/// 20·log10(0.35): the fixed click gain used before the level could be set, so older projects sound the same.
pub const METRONOME_DEFAULT_DB: f32 = -9.118_639;
/// Range of the metronome level, the same in the GUI slider and in `set_metronome`.
pub const METRONOME_MIN_DB: f32 = -60.0;
pub const METRONOME_MAX_DB: f32 = 0.0;

fn default_metronome_level_db() -> f32 {
    METRONOME_DEFAULT_DB
}

/// Most scenes the Clip Grid can have (`add_scene`, `create_live_cell`).
pub const MAX_SCENES: usize = 256;

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct LiveLoops {
    pub scenes: Vec<String>,
    pub cells: Vec<LiveCell>,
    /// Launch quantization in beats (0 = immediate).
    #[serde(default = "default_live_q")]
    pub quantize: f64,
}

fn default_live_q() -> f64 {
    4.0
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct LiveCell {
    pub id: Id,
    pub track_id: Id,
    pub scene: usize,
    pub name: String,
    /// Loop length in beats.
    pub length: f64,
    pub content: RegionContent,
}

#[derive(Debug, Clone, Copy, Default, Serialize, Deserialize, JsonSchema, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum SurroundFormat {
    #[default]
    Stereo,
    /// Headphone spatial audio (binaural rendering of 3D positions).
    Binaural,
    #[serde(rename = "5.1")]
    Surround51,
    #[serde(rename = "7.1")]
    Surround71,
    /// 7.1.4 immersive bed (Atmos-style layout with height speakers).
    #[serde(rename = "7.1.4")]
    Immersive714,
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, JsonSchema, PartialEq)]
pub struct SurroundPan {
    /// Horizontal angle in degrees: 0 = front center, +90 = right, -90 = left, 180 = rear.
    pub azimuth: f32,
    /// Vertical angle in degrees (0 = ear level, 90 = above).
    pub elevation: f32,
    /// Source width 0..1.
    pub spread: f32,
    /// Object-based (Atmos-style audio object) instead of part of the channel bed.
    #[serde(default)]
    pub object: bool,
}

impl Default for SurroundPan {
    fn default() -> Self {
        SurroundPan { azimuth: 0.0, elevation: 0.0, spread: 0.2, object: false }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct MovieRef {
    pub path: String,
    /// Song time (seconds) at which the movie starts.
    pub offset_sec: f64,
    pub duration_sec: f64,
    pub audio_file_id: Option<Id>,
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema, PartialEq)]
pub struct MidiRoute {
    pub name: String,
    /// MIDI channel filter 1..16 (omit for any).
    pub channel: Option<u8>,
    pub note_min: Option<u8>,
    pub note_max: Option<u8>,
    #[serde(default)]
    pub transpose: i32,
    /// Velocity scale (1.0 = unchanged).
    pub velocity_scale: Option<f32>,
    pub track_id: Id,
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, JsonSchema, PartialEq)]
pub struct TimeSignature {
    pub numerator: u8,
    pub denominator: u8,
}

impl TimeSignature {
    /// At least one beat per bar, and a whole, half, quarter, eighth or sixteenth note as the beat: the time
    /// signatures `set_time_signature` accepts. The metronome divides by the numerator.
    pub fn is_valid(&self) -> bool {
        self.numerator >= 1 && matches!(self.denominator, 1 | 2 | 4 | 8 | 16)
    }

    /// Length of one bar in beats (quarter notes).
    pub fn beats_per_bar(&self) -> f64 {
        self.numerator as f64 * 4.0 / self.denominator.max(1) as f64
    }
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq)]
pub struct Cycle {
    pub enabled: bool,
    pub start: f64,
    pub end: f64,
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, JsonSchema, PartialEq, Eq, Hash)]
#[serde(rename_all = "snake_case")]
pub enum TrackKind {
    /// Software instrument track: plays MIDI and pattern regions through an instrument plug-in.
    Instrument,
    /// Audio track: plays audio regions (recorded or imported files).
    Audio,
    /// Auxiliary bus: receives sends from other tracks (reverb/delay returns, groups).
    Bus,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Track {
    pub id: Id,
    pub name: String,
    pub kind: TrackKind,
    pub color: [u8; 3],
    pub channel: Channel,
    pub instrument: Option<PluginInstance>,
    pub midi_fx: Vec<PluginInstance>,
    pub regions: Vec<Region>,
    pub automation: Vec<AutomationLane>,
    pub record_armed: bool,
    /// Audio input channel (0-based, stereo pair start) for audio recording.
    pub input_channel: u16,
    #[serde(default)]
    pub height: f32,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Channel {
    pub volume_db: f32,
    /// -1.0 (left) .. 1.0 (right)
    pub pan: f32,
    pub mute: bool,
    pub solo: bool,
    pub inserts: Vec<PluginInstance>,
    pub sends: Vec<Send>,
    pub output: Output,
    #[serde(default)]
    pub surround_pan: SurroundPan,
}

impl Default for Channel {
    fn default() -> Self {
        Self { volume_db: 0.0, pan: 0.0, mute: false, solo: false, inserts: vec![], sends: vec![], output: Output::Master, surround_pan: SurroundPan::default() }
    }
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, JsonSchema, PartialEq)]
#[serde(rename_all = "snake_case")]
pub enum Output {
    Master,
    Bus(Id),
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Send {
    pub bus_id: Id,
    pub level_db: f32,
    pub pre_fader: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PluginInstance {
    pub id: Id,
    pub kind: String,
    pub params: BTreeMap<String, f32>,
    pub bypass: bool,
    #[serde(default)]
    pub preset: Option<String>,
    /// Audio file id used by sample based plug-ins (Sampler).
    #[serde(default)]
    pub sample: Option<Id>,
    /// Third-party VST3 plug-in bundle path (kind = "vst3").
    #[serde(default)]
    pub vst3_path: Option<String>,
    /// Parameter metadata reported by a VST3 plug-in (ids are "p<number>", values normalized 0..1).
    #[serde(default)]
    pub vst3_params: Vec<Vst3Param>,
    /// Complete internal state of a VST3 plug-in (programs, banks, samples...), base64.
    #[serde(default)]
    pub vst3_state: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Vst3Param {
    pub id: String,
    pub name: String,
    #[serde(deserialize_with = "finite_or_zero")]
    pub default: f32,
    pub unit: String,
}

impl PluginInstance {
    pub fn param(&self, id: &str) -> f32 {
        if let Some(v) = self.params.get(id) {
            return *v;
        }
        if let Some(vp) = self.vst3_params.iter().find(|p| p.id == id) {
            return vp.default;
        }
        crate::plugins::find(&self.kind)
            .and_then(|p| p.params.iter().find(|x| x.id == id))
            .map(|x| x.default)
            .unwrap_or(0.0)
    }
}

/// Where a plug-in sits on its track or on the master channel.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PluginSlot {
    /// The instrument of an instrument track.
    Instrument,
    /// A MIDI effect, by its index among the track's MIDI effects.
    MidiFx(usize),
    /// An audio insert, by its index in the channel strip (as move_plugin counts it).
    Insert(usize),
}

impl PluginSlot {
    /// "instrument", "midi_fx" or "insert", as get_plugin reports it.
    pub fn name(self) -> &'static str {
        match self {
            PluginSlot::Instrument => "instrument",
            PluginSlot::MidiFx(_) => "midi_fx",
            PluginSlot::Insert(_) => "insert",
        }
    }

    /// Index in its chain; None for the instrument.
    pub fn index(self) -> Option<usize> {
        match self {
            PluginSlot::Instrument => None,
            PluginSlot::MidiFx(i) | PluginSlot::Insert(i) => Some(i),
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
pub struct Region {
    pub id: Id,
    pub name: String,
    /// Start position in beats.
    pub start: f64,
    /// Length of one iteration in beats.
    pub length: f64,
    /// Number of times the region content repeats (1 = no loop).
    pub loop_count: u32,
    pub muted: bool,
    pub color: Option<[u8; 3]>,
    pub content: RegionContent,
    /// Take number when the region belongs to a set of recorded takes (comping).
    #[serde(default)]
    pub take: Option<u32>,
}

impl Region {
    pub fn end(&self) -> f64 {
        self.start + self.length * self.loop_count.max(1) as f64
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum RegionContent {
    Midi(MidiContent),
    Audio(AudioContent),
    Pattern(PatternContent),
}

#[derive(Debug, Clone, Default, Serialize, Deserialize, JsonSchema)]
pub struct MidiContent {
    pub notes: Vec<Note>,
    #[serde(default)]
    pub cc: Vec<CcEvent>,
    /// Region-level transposition in semitones applied at playback.
    #[serde(default)]
    pub transpose: i32,
    /// Region-level velocity offset applied at playback.
    #[serde(default)]
    pub velocity_offset: i32,
    /// Lyrics attached to positions (beats relative to region start), for the score.
    #[serde(default)]
    pub lyrics: Vec<LyricEvent>,
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema, PartialEq)]
pub struct LyricEvent {
    pub beat: f64,
    pub text: String,
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, JsonSchema, PartialEq)]
pub struct Note {
    /// MIDI pitch 0..127 (60 = C4/middle C).
    pub pitch: u8,
    /// Velocity 1..127.
    pub velocity: u8,
    /// Start in beats, relative to the region start.
    pub start: f64,
    /// Length in beats.
    pub length: f64,
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, JsonSchema, PartialEq)]
pub struct CcEvent {
    /// Position in beats relative to the region start.
    pub beat: f64,
    /// Controller number (1 = mod wheel, 7 = volume, 10 = pan, 64 = sustain, 128 = pitch bend 0..127).
    pub controller: u8,
    pub value: u8,
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
pub struct AudioContent {
    pub file_id: Id,
    /// Offset into the source file, in seconds.
    pub offset_sec: f64,
    pub gain_db: f32,
    /// Fade in length, in beats.
    pub fade_in: f64,
    /// Fade out length, in beats.
    pub fade_out: f64,
    pub reverse: bool,
}

/// Most steps a pattern region can have: `resize_region`, `paste_region` and project files are held to it (a
/// row stores one byte per step, and every row is resized at once).
pub const MAX_PATTERN_STEPS: u32 = 65_536;

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
pub struct PatternContent {
    pub steps: u32,
    /// Step length in beats (0.25 = sixteenth note).
    pub step_length: f64,
    pub rows: Vec<PatternRow>,
    /// Swing amount 0..1 applied to every second step.
    #[serde(default)]
    pub swing: f32,
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
pub struct PatternRow {
    pub name: String,
    pub pitch: u8,
    /// One entry per step: 0 = off, 1..127 = velocity.
    pub steps: Vec<u8>,
    #[serde(default)]
    pub muted: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AutomationLane {
    /// "volume", "pan", "send:<index>", or "plugin:<plugin_id>:<param>"
    pub target: String,
    pub points: Vec<AutomationPoint>,
    pub enabled: bool,
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, JsonSchema, PartialEq)]
pub struct AutomationPoint {
    pub beat: f64,
    pub value: f32,
}

impl AutomationLane {
    pub fn value_at(&self, beat: f64) -> Option<f32> {
        if !self.enabled || self.points.is_empty() {
            return None;
        }
        let pts = &self.points;
        if beat <= pts[0].beat {
            return Some(pts[0].value);
        }
        let last = pts[pts.len() - 1];
        if beat >= last.beat {
            return Some(last.value);
        }
        let idx = pts.partition_point(|p| p.beat <= beat);
        let a = pts[idx - 1];
        let b = pts[idx];
        let t = ((beat - a.beat) / (b.beat - a.beat).max(1e-9)) as f32;
        Some(a.value + (b.value - a.value) * t)
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
pub struct Marker {
    pub id: Id,
    pub beat: f64,
    pub name: String,
    /// Arrangement markers have a length (e.g. "Verse", "Chorus") and can be moved/copied as sections.
    pub length: Option<f64>,
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
pub struct ChordEvent {
    pub id: Id,
    pub beat: f64,
    pub length: f64,
    /// Chord symbol, e.g. "Am7", "F", "G/B", "Csus4".
    pub symbol: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AudioFileRef {
    pub id: Id,
    pub path: String,
    pub name: String,
    pub sample_rate: u32,
    pub channels: u16,
    pub duration_sec: f64,
}

/// The twelve colors new tracks take in turn ("Graphite Tide" track palette). The GUI theme reads them from here,
/// so the two never drift apart. Projects keep the colors stored in them.
pub const TRACK_COLORS: [[u8; 3]; 12] = [
    [0x4A, 0x90, 0xD9],
    [0x4C, 0xAF, 0x7D],
    [0xD9, 0xA4, 0x41],
    [0xE0, 0x70, 0x5F],
    [0x9A, 0x7B, 0xD9],
    [0x3F, 0xB0, 0xA8],
    [0xD4, 0x6B, 0xA3],
    [0xC9, 0x8B, 0x5A],
    [0x7F, 0xA6, 0x50],
    [0x5F, 0x7F, 0xD9],
    [0xB8, 0xB8, 0xB8],
    [0x8C, 0x6B, 0xD9],
];

impl Default for Project {
    fn default() -> Self {
        Self::new("Untitled")
    }
}

impl Project {
    pub fn new(name: &str) -> Self {
        Project {
            name: name.to_string(),
            tempo: TempoMap::new(120.0),
            time_signature: TimeSignature { numerator: 4, denominator: 4 },
            key: Key { root: 0, scale: ScaleKind::Major },
            tracks: vec![],
            master: Channel::default(),
            markers: vec![],
            chords: vec![],
            cycle: Cycle { enabled: false, start: 0.0, end: 16.0 },
            metronome: false,
            metronome_level_db: METRONOME_DEFAULT_DB,
            metronome_only_recording: false,
            count_in_bars: 1,
            audio_files: vec![],
            notes: String::new(),
            next_id: 1,
            live: LiveLoops { scenes: vec!["Scene 1".into(), "Scene 2".into(), "Scene 3".into(), "Scene 4".into()], cells: vec![], quantize: 4.0 },
            surround: SurroundFormat::Stereo,
            movie: None,
            frame_rate: 25.0,
            midi_routes: vec![],
        }
    }

    /// Linear gain of the metronome click. A level outside the range is clamped; a non-finite one (a hand-edited
    /// file) falls back to the default.
    pub fn metronome_gain(&self) -> f32 {
        let db = if self.metronome_level_db.is_finite() { self.metronome_level_db } else { METRONOME_DEFAULT_DB };
        10f32.powf(db.clamp(METRONOME_MIN_DB, METRONOME_MAX_DB) / 20.0)
    }

    pub fn alloc_id(&mut self) -> Id {
        let id = self.next_id;
        self.next_id += 1;
        id
    }

    pub fn track(&self, id: Id) -> Option<&Track> {
        self.tracks.iter().find(|t| t.id == id)
    }

    pub fn track_mut(&mut self, id: Id) -> Option<&mut Track> {
        self.tracks.iter_mut().find(|t| t.id == id)
    }

    pub fn track_index(&self, id: Id) -> Option<usize> {
        self.tracks.iter().position(|t| t.id == id)
    }

    /// Channel for a track id, or the master channel for [`MASTER_ID`].
    pub fn channel_mut(&mut self, id: Id) -> Option<&mut Channel> {
        if id == MASTER_ID {
            Some(&mut self.master)
        } else {
            self.track_mut(id).map(|t| &mut t.channel)
        }
    }

    pub fn channel(&self, id: Id) -> Option<&Channel> {
        if id == MASTER_ID {
            Some(&self.master)
        } else {
            self.track(id).map(|t| &t.channel)
        }
    }

    pub fn find_region(&self, region_id: Id) -> Option<(&Track, &Region)> {
        self.tracks
            .iter()
            .find_map(|t| t.regions.iter().find(|r| r.id == region_id).map(|r| (t, r)))
    }

    pub fn find_region_mut(&mut self, region_id: Id) -> Option<&mut Region> {
        self.tracks.iter_mut().find_map(|t| t.regions.iter_mut().find(|r| r.id == region_id))
    }

    pub fn region_track_id(&self, region_id: Id) -> Option<Id> {
        self.find_region(region_id).map(|(t, _)| t.id)
    }

    /// Find a plug-in anywhere in the project (instrument, MIDI FX, inserts on tracks or master).
    pub fn find_plugin_mut(&mut self, plugin_id: Id) -> Option<&mut PluginInstance> {
        if let Some(p) = self.master.inserts.iter_mut().find(|p| p.id == plugin_id) {
            return Some(p);
        }
        for t in &mut self.tracks {
            if let Some(i) = t.instrument.as_mut() {
                if i.id == plugin_id {
                    return Some(i);
                }
            }
            if let Some(p) = t.midi_fx.iter_mut().find(|p| p.id == plugin_id) {
                return Some(p);
            }
            if let Some(p) = t.channel.inserts.iter_mut().find(|p| p.id == plugin_id) {
                return Some(p);
            }
        }
        None
    }

    pub fn find_plugin(&self, plugin_id: Id) -> Option<&PluginInstance> {
        if let Some(p) = self.master.inserts.iter().find(|p| p.id == plugin_id) {
            return Some(p);
        }
        for t in &self.tracks {
            if let Some(i) = t.instrument.as_ref() {
                if i.id == plugin_id {
                    return Some(i);
                }
            }
            if let Some(p) = t.midi_fx.iter().find(|p| p.id == plugin_id) {
                return Some(p);
            }
            if let Some(p) = t.channel.inserts.iter().find(|p| p.id == plugin_id) {
                return Some(p);
            }
        }
        None
    }

    /// Owner (track id or MASTER_ID) of a plug-in.
    pub fn plugin_owner(&self, plugin_id: Id) -> Option<Id> {
        if self.master.inserts.iter().any(|p| p.id == plugin_id) {
            return Some(MASTER_ID);
        }
        self.tracks
            .iter()
            .find(|t| {
                t.instrument.as_ref().map(|i| i.id == plugin_id).unwrap_or(false)
                    || t.midi_fx.iter().any(|p| p.id == plugin_id)
                    || t.channel.inserts.iter().any(|p| p.id == plugin_id)
            })
            .map(|t| t.id)
    }

    /// The track a plug-in sits on (None for the master channel) and its slot there.
    pub fn plugin_slot(&self, plugin_id: Id) -> Option<(Option<&Track>, PluginSlot)> {
        if let Some(i) = self.master.inserts.iter().position(|p| p.id == plugin_id) {
            return Some((None, PluginSlot::Insert(i)));
        }
        for t in &self.tracks {
            if t.instrument.as_ref().is_some_and(|p| p.id == plugin_id) {
                return Some((Some(t), PluginSlot::Instrument));
            }
            if let Some(i) = t.midi_fx.iter().position(|p| p.id == plugin_id) {
                return Some((Some(t), PluginSlot::MidiFx(i)));
            }
            if let Some(i) = t.channel.inserts.iter().position(|p| p.id == plugin_id) {
                return Some((Some(t), PluginSlot::Insert(i)));
            }
        }
        None
    }

    /// End of the last region (or marker), in beats.
    pub fn content_end(&self) -> f64 {
        let mut end: f64 = 0.0;
        for t in &self.tracks {
            for r in &t.regions {
                end = end.max(r.end());
            }
        }
        for m in &self.markers {
            end = end.max(m.beat + m.length.unwrap_or(0.0));
        }
        end
    }

    pub fn any_solo(&self) -> bool {
        self.tracks.iter().any(|t| t.channel.solo)
    }

    pub fn audio_file(&self, id: Id) -> Option<&AudioFileRef> {
        self.audio_files.iter().find(|f| f.id == id)
    }

    /// SMPTE timecode HH:MM:SS:FF at a beat position.
    pub fn timecode(&self, beat: f64) -> String {
        let fps = self.frame_rate.max(1.0);
        let secs = self.tempo.beat_to_sec(beat).max(0.0);
        let total_frames = (secs * fps).floor() as i64;
        let f = total_frames % fps.round() as i64;
        let s = (secs as i64) % 60;
        let m = (secs as i64 / 60) % 60;
        let h = secs as i64 / 3600;
        format!("{h:02}:{m:02}:{s:02}:{f:02}")
    }

    pub fn bar_beat_string(&self, beat: f64) -> String {
        let bpb = self.time_signature.beats_per_bar();
        let bar = (beat / bpb).floor();
        let in_bar = beat - bar * bpb;
        let beat_len = 4.0 / self.time_signature.denominator as f64;
        let b = (in_bar / beat_len).floor();
        let ticks = ((in_bar - b * beat_len) / beat_len * 960.0).floor();
        format!("{}.{}.{:03}", bar as i64 + 1, b as i64 + 1, ticks as i64)
    }
}

/// Accept `null` (a NaN written by older files) as 0.0.
fn finite_or_zero<'de, D: serde::Deserializer<'de>>(d: D) -> Result<f32, D::Error> {
    let v: Option<f32> = Option::deserialize(d)?;
    Ok(v.filter(|x| x.is_finite()).unwrap_or(0.0))
}

const B64: &[u8; 64] = b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789+/";

pub fn base64_encode(data: &[u8]) -> String {
    let mut out = String::with_capacity(data.len().div_ceil(3) * 4);
    for c in data.chunks(3) {
        let n = (c[0] as u32) << 16 | (*c.get(1).unwrap_or(&0) as u32) << 8 | *c.get(2).unwrap_or(&0) as u32;
        out.push(B64[(n >> 18) as usize & 63] as char);
        out.push(B64[(n >> 12) as usize & 63] as char);
        out.push(if c.len() > 1 { B64[(n >> 6) as usize & 63] as char } else { '=' });
        out.push(if c.len() > 2 { B64[n as usize & 63] as char } else { '=' });
    }
    out
}

pub fn base64_decode(s: &str) -> Option<Vec<u8>> {
    let val = |c: u8| B64.iter().position(|x| *x == c).map(|p| p as u32);
    let bytes: Vec<u8> = s.bytes().filter(|b| !b.is_ascii_whitespace()).collect();
    if bytes.len() % 4 != 0 {
        return None;
    }
    let mut out = Vec::with_capacity(bytes.len() / 4 * 3);
    for c in bytes.chunks(4) {
        let mut n = 0u32;
        let mut pad = 0;
        for &b in c {
            n <<= 6;
            if b == b'=' {
                pad += 1;
            } else {
                n |= val(b)?;
            }
        }
        out.push((n >> 16) as u8);
        if pad < 2 {
            out.push((n >> 8) as u8);
        }
        if pad < 1 {
            out.push(n as u8);
        }
    }
    Some(out)
}

#[cfg(test)]
mod b64_tests {
    #[test]
    fn roundtrip() {
        for d in [&b""[..], b"a", b"ab", b"abc", b"\x00\xff\x10hello world"] {
            assert_eq!(super::base64_decode(&super::base64_encode(d)).unwrap(), d);
        }
    }
}

#[cfg(test)]
mod metronome_tests {
    use super::*;

    #[test]
    fn project_without_metronome_level_gets_the_old_click() {
        let mut v = serde_json::to_value(Project::new("x")).unwrap();
        let map = v.as_object_mut().unwrap();
        assert!(map.remove("metronome_level_db").is_some());
        assert!(map.remove("metronome_only_recording").is_some());
        let p: Project = serde_json::from_value(v).unwrap();
        assert!((p.metronome_gain() - 0.35).abs() < 0.001);
        assert!(!p.metronome_only_recording);
    }

    #[test]
    fn metronome_gain_follows_the_level_and_stays_in_range() {
        let mut p = Project::new("x");
        let gain = |p: &mut Project, db: f32| {
            p.metronome_level_db = db;
            p.metronome_gain()
        };
        assert!((gain(&mut p, 0.0) - 1.0).abs() < 1e-6);
        assert!((gain(&mut p, -20.0) - 0.1).abs() < 1e-4);
        let floor = gain(&mut p, -60.0);
        assert!((floor - 0.001).abs() < 1e-5);
        assert_eq!(gain(&mut p, -100.0), floor);
        assert!((gain(&mut p, 12.0) - 1.0).abs() < 1e-6);
        assert!((gain(&mut p, f32::NAN) - 0.35).abs() < 0.001);
    }
}
