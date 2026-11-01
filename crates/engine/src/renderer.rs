//! The mix renderer. Owns all DSP state and turns a [`Project`] snapshot into audio.
//! Used both by the real-time engine and by offline bounce/analysis.

use std::collections::{HashMap, HashSet};
use std::sync::Arc;

use ondina_core::model::*;
use ondina_core::ops::pattern_to_notes;
use ondina_dsp::util::{db_to_gain, pan_gains};
use ondina_dsp::spatial::{self, Binaural};
use ondina_dsp::vst3::{Vst3Effect, Vst3Processor};
use ondina_dsp::{configure, create_effect, create_instrument, create_midi_effect, Effect, Instrument, MidiEffect, MidiEvent, MidiKind, ProcessContext};

/// Configure a processor: registry defaults + params, or raw params for VST3.
fn configure_any<F: FnMut(&str, f32)>(kind: &str, params: &std::collections::BTreeMap<String, f32>, mut set: F) {
    if kind == "vst3" {
        for (k, v) in params {
            set(k, *v);
        }
    } else {
        configure(kind, params, set);
    }
}

fn make_effect(p: &PluginInstance, sr: f32) -> Option<Box<dyn Effect>> {
    if p.kind == "vst3" {
        let path = p.vst3_path.as_deref()?;
        match Vst3Processor::load(path, sr, false) {
            Ok(v) => Some(Box::new(Vst3Effect(v))),
            Err(e) => {
                eprintln!("[ondina] {e}");
                None
            }
        }
    } else {
        create_effect(&p.kind, sr)
    }
}

fn make_instrument(p: &PluginInstance, sr: f32) -> Option<Box<dyn Instrument>> {
    if p.kind == "vst3" {
        let path = p.vst3_path.as_deref()?;
        match Vst3Processor::load(path, sr, true) {
            Ok(v) => Some(Box::new(v)),
            Err(e) => {
                eprintln!("[ondina] {e}");
                None
            }
        }
    } else {
        create_instrument(&p.kind, sr)
    }
}

/// A Clip Grid cell currently playing on a track.
pub struct LivePlay {
    pub cell_id: Id,
    pub start: f64,
    track: Track,
}

#[derive(Debug, Clone, Copy)]
pub enum LiveAction {
    Launch { cell_id: Id, track_id: Id },
    Stop { track_id: Id },
}

#[derive(Debug, Clone, Copy, serde::Serialize)]
pub struct LiveLogEntry {
    pub track_id: Id,
    pub cell_id: Id,
    pub start: f64,
    pub end: f64,
}

/// Multichannel accumulation for offline surround bounces.
pub struct SurroundOut {
    pub layout: Vec<(&'static str, f32, f32)>,
    pub channels: Vec<Vec<f32>>,
}

use crate::analysis::Analyzer;
use crate::pool::AudioPool;

pub const MAX_BLOCK: usize = 64;

struct FxSlot {
    id: Id,
    kind: String,
    fx: Box<dyn Effect>,
    bypass: bool,
    state: Option<String>,
}

struct MidiFxSlot {
    id: Id,
    fx: Box<dyn MidiEffect>,
    bypass: bool,
}

struct InstSlot {
    id: Id,
    kind: String,
    inst: Box<dyn Instrument>,
    sample: Option<Id>,
    state: Option<String>,
}

#[derive(Default)]
struct TrackState {
    inst: Option<InstSlot>,
    midi_fx: Vec<MidiFxSlot>,
    fx: Vec<FxSlot>,
    l: Vec<f32>,
    r: Vec<f32>,
    /// Bus input accumulators.
    in_l: Vec<f32>,
    in_r: Vec<f32>,
    timeline_notes: HashSet<u8>,
    binaural: Option<Binaural>,
    gain_l: f32,
    gain_r: f32,
    peak: [f32; 2],
}

/// A MIDI event recorded from live input while recording.
#[derive(Debug, Clone, Copy)]
pub struct RecordedEvent {
    pub track_id: Id,
    pub beat: f64,
    pub kind: MidiKind,
}

pub struct Renderer {
    pub sr: f32,
    project: Arc<Project>,
    pool: Arc<AudioPool>,
    tracks: HashMap<Id, TrackState>,
    master_fx: Vec<FxSlot>,
    master_gain: [f32; 2],
    master_l: Vec<f32>,
    master_r: Vec<f32>,
    pub playing: bool,
    pub recording: bool,
    /// Playhead in beats.
    pub beat: f64,
    live: Vec<(Id, MidiKind)>,
    pub recorded: Vec<RecordedEvent>,
    /// Render only these tracks (offline bounce of selected tracks).
    pub solo_override: Option<Vec<Id>>,
    pub analyzers: Option<HashMap<Id, Analyzer>>,
    click_phase: f32,
    click_env: f32,
    click_freq: f32,
    pub master_peak: [f32; 2],
    /// Count-in: number of beats of click before the song position starts moving.
    pub count_in_beats: f64,
    pub live_playing: HashMap<Id, LivePlay>,
    pub live_queue: Vec<(f64, LiveAction)>,
    pub live_log: Vec<LiveLogEntry>,
    pub surround_out: Option<SurroundOut>,
    /// Object-based capture (ADM export): post-fader mono signal of every object track.
    pub object_out: Option<HashMap<Id, Vec<f32>>>,
}

impl Renderer {
    pub fn new(sr: f32, project: Arc<Project>, pool: Arc<AudioPool>) -> Self {
        let mut r = Renderer {
            sr,
            project: Arc::new(Project::new("")),
            pool,
            tracks: HashMap::new(),
            master_fx: vec![],
            master_gain: [1.0, 1.0],
            master_l: vec![0.0; MAX_BLOCK],
            master_r: vec![0.0; MAX_BLOCK],
            playing: false,
            recording: false,
            beat: 0.0,
            live: vec![],
            recorded: vec![],
            solo_override: None,
            analyzers: None,
            click_phase: 0.0,
            click_env: 0.0,
            click_freq: 1000.0,
            master_peak: [0.0; 2],
            count_in_beats: 0.0,
            live_playing: HashMap::new(),
            live_queue: vec![],
            live_log: vec![],
            surround_out: None,
            object_out: None,
        };
        r.set_project(project);
        r
    }

    pub fn project(&self) -> &Arc<Project> {
        &self.project
    }

    pub fn set_pool(&mut self, pool: Arc<AudioPool>) {
        self.pool = pool;
        // re-resolve sampler data
        let p = self.project.clone();
        self.project = Arc::new(Project::new(""));
        for st in self.tracks.values_mut() {
            if let Some(i) = st.inst.as_mut() {
                i.sample = None;
            }
        }
        self.set_project(p);
    }

    fn sync_fx(old: &mut Vec<FxSlot>, desired: &[PluginInstance], sr: f32) {
        let mut pool: HashMap<Id, FxSlot> = old.drain(..).map(|s| (s.id, s)).collect();
        for p in desired {
            let slot = match pool.remove(&p.id) {
                Some(s) if s.kind == p.kind => Some(s),
                _ => make_effect(p, sr).map(|fx| FxSlot { id: p.id, kind: p.kind.clone(), fx, bypass: false, state: None }),
            };
            if let Some(mut s) = slot {
                if s.state != p.vst3_state {
                    s.state = p.vst3_state.clone();
                    if let Some(b) = p.vst3_state.as_deref().and_then(base64_decode) {
                        s.fx.load_state(&b);
                    }
                }
                configure_any(&p.kind, &p.params, |k, v| s.fx.set_param(k, v));
                s.bypass = p.bypass;
                old.push(s);
            }
        }
    }

    /// Install a new project snapshot, reusing DSP instances where possible.
    pub fn set_project(&mut self, project: Arc<Project>) {
        let sr = self.sr;
        let mut keep: HashSet<Id> = HashSet::new();
        for t in &project.tracks {
            keep.insert(t.id);
            let st = self.tracks.entry(t.id).or_insert_with(|| TrackState {
                l: vec![0.0; MAX_BLOCK],
                r: vec![0.0; MAX_BLOCK],
                in_l: vec![0.0; MAX_BLOCK],
                in_r: vec![0.0; MAX_BLOCK],
                gain_l: 0.0,
                gain_r: 0.0,
                ..Default::default()
            });
            // instrument
            match &t.instrument {
                Some(pi) => {
                    let reuse = st.inst.as_ref().map(|i| i.id == pi.id && i.kind == pi.kind).unwrap_or(false);
                    if !reuse {
                        st.inst = make_instrument(pi, sr).map(|inst| InstSlot { id: pi.id, kind: pi.kind.clone(), inst, sample: None, state: None });
                    }
                    if let Some(slot) = st.inst.as_mut() {
                        if slot.state != pi.vst3_state {
                            slot.state = pi.vst3_state.clone();
                            if let Some(b) = pi.vst3_state.as_deref().and_then(base64_decode) {
                                slot.inst.load_state(&b);
                            }
                        }
                        configure_any(&pi.kind, &pi.params, |k, v| slot.inst.set_param(k, v));
                        if slot.sample != pi.sample {
                            slot.sample = pi.sample;
                            slot.inst.set_sample(pi.sample.and_then(|id| self.pool.get(&id).cloned()));
                        }
                    }
                }
                None => st.inst = None,
            }
            // midi fx
            let mut old: HashMap<Id, MidiFxSlot> = st.midi_fx.drain(..).map(|s| (s.id, s)).collect();
            for p in &t.midi_fx {
                let slot = old.remove(&p.id).or_else(|| create_midi_effect(&p.kind).map(|fx| MidiFxSlot { id: p.id, fx, bypass: false }));
                if let Some(mut s) = slot {
                    configure(&p.kind, &p.params, |k, v| s.fx.set_param(k, v));
                    s.bypass = p.bypass;
                    st.midi_fx.push(s);
                }
            }
            Self::sync_fx(&mut st.fx, &t.channel.inserts, sr);
        }
        self.tracks.retain(|id, _| keep.contains(id));
        Self::sync_fx(&mut self.master_fx, &project.master.inserts, sr);
        // refresh Clip Grid overrides from the new cell contents
        let ids: Vec<Id> = self.live_playing.keys().copied().collect();
        for tid in ids {
            let cell_id = self.live_playing[&tid].cell_id;
            let start = self.live_playing[&tid].start;
            match Self::cell_track(&project, cell_id, start) {
                Some((t, _)) => self.live_playing.get_mut(&tid).unwrap().track = t,
                None => {
                    self.live_playing.remove(&tid);
                }
            }
        }
        self.project = project;
    }

    pub fn live_event(&mut self, track_id: Id, kind: MidiKind) {
        self.live.push((track_id, kind));
    }

    /// Stop all timeline notes (on stop / locate / cycle jump).
    fn release_timeline_notes(&mut self) {
        for st in self.tracks.values_mut() {
            if let Some(i) = st.inst.as_mut() {
                for n in st.timeline_notes.drain() {
                    i.inst.note_off(n);
                }
                i.inst.control_change(64, 0);
            }
        }
    }

    pub fn play(&mut self) {
        self.playing = true;
    }

    pub fn stop(&mut self) {
        let b = self.beat;
        let ids: Vec<Id> = self.live_playing.keys().copied().collect();
        for t in ids {
            self.close_live_log(t, b);
        }
        self.live_queue.clear();
        self.playing = false;
        self.recording = false;
        self.count_in_beats = 0.0;
        self.release_timeline_notes();
    }

    pub fn locate(&mut self, beat: f64) {
        self.release_timeline_notes();
        self.beat = beat.max(0.0);
    }

    /// Build a virtual track whose single looping region plays a Clip Grid cell from `start`.
    fn cell_track(project: &Project, cell_id: Id, start: f64) -> Option<(Track, Id)> {
        let cell = project.live.cells.iter().find(|c| c.id == cell_id)?;
        let mut t = project.track(cell.track_id)?.clone();
        t.regions = vec![Region { take: None, id: cell.id, name: cell.name.clone(), start, length: cell.length, loop_count: 1_000_000, muted: false, color: None, content: cell.content.clone() }];
        Some((t, cell.track_id))
    }

    fn quantized_launch_beat(&self) -> f64 {
        let q = self.project.live.quantize;
        if !self.playing || q <= 0.0 {
            return self.beat;
        }
        let next = (self.beat / q).ceil() * q;
        if next - self.beat < 1e-6 { self.beat } else { next }
    }

    pub fn launch_cell(&mut self, cell_id: Id) {
        let Some(cell) = self.project.live.cells.iter().find(|c| c.id == cell_id) else { return };
        let track_id = cell.track_id;
        let at = self.quantized_launch_beat();
        if !self.playing {
            self.play();
        }
        self.live_queue.retain(|(_, a)| !matches!(a, LiveAction::Launch { track_id: t, .. } | LiveAction::Stop { track_id: t } if *t == track_id));
        self.live_queue.push((at, LiveAction::Launch { cell_id, track_id }));
    }

    pub fn launch_scene(&mut self, scene: usize) {
        let ids: Vec<Id> = self.project.live.cells.iter().filter(|c| c.scene == scene).map(|c| c.id).collect();
        for id in ids {
            self.launch_cell(id);
        }
    }

    pub fn stop_live(&mut self, track: Option<Id>) {
        let at = self.quantized_launch_beat();
        let tracks: Vec<Id> = match track {
            Some(t) => vec![t],
            None => self.live_playing.keys().copied().collect(),
        };
        for t in tracks {
            self.live_queue.push((at, LiveAction::Stop { track_id: t }));
        }
        if track.is_none() {
            self.live_queue.retain(|(_, a)| matches!(a, LiveAction::Stop { .. }));
        }
    }

    fn close_live_log(&mut self, track_id: Id, at: f64) {
        if let Some(lp) = self.live_playing.remove(&track_id) {
            self.live_log.push(LiveLogEntry { track_id, cell_id: lp.cell_id, start: lp.start, end: at });
        }
        if let Some(st) = self.tracks.get_mut(&track_id) {
            if let Some(i) = st.inst.as_mut() {
                for n in st.timeline_notes.drain() {
                    i.inst.note_off(n);
                }
            }
        }
    }

    fn apply_live_queue(&mut self, b1: f64) {
        if self.live_queue.is_empty() {
            return;
        }
        let mut due: Vec<(f64, LiveAction)> = vec![];
        self.live_queue.retain(|(at, a)| {
            if *at < b1 {
                due.push((*at, *a));
                false
            } else {
                true
            }
        });
        for (at, a) in due {
            match a {
                LiveAction::Launch { cell_id, track_id } => {
                    self.close_live_log(track_id, at);
                    if let Some((t, _)) = Self::cell_track(&self.project, cell_id, at) {
                        self.live_playing.insert(track_id, LivePlay { cell_id, start: at, track: t });
                    }
                }
                LiveAction::Stop { track_id } => self.close_live_log(track_id, at),
            }
        }
    }

    /// Currently playing cells (track, cell, start) and queued actions.
    pub fn live_state(&self) -> (Vec<(Id, Id, f64)>, Vec<(f64, LiveAction)>) {
        (self.live_playing.iter().map(|(t, p)| (*t, p.cell_id, p.start)).collect(), self.live_queue.clone())
    }

    pub fn panic(&mut self) {
        for st in self.tracks.values_mut() {
            st.timeline_notes.clear();
            if let Some(i) = st.inst.as_mut() {
                i.inst.reset();
            }
            for f in &mut st.fx {
                f.fx.reset();
            }
        }
        for f in &mut self.master_fx {
            f.fx.reset();
        }
    }

    pub fn track_peaks(&self) -> Vec<(Id, [f32; 2])> {
        self.tracks.iter().map(|(id, s)| (*id, s.peak)).collect()
    }

    /// Render `left.len()` frames, writing (not adding) into the buffers.
    pub fn render(&mut self, left: &mut [f32], right: &mut [f32]) {
        for st in self.tracks.values_mut() {
            st.peak = [0.0; 2];
        }
        self.master_peak = [0.0; 2];
        let mut done = 0;
        while done < left.len() {
            let mut n = (left.len() - done).min(MAX_BLOCK);
            let bpm = self.project.tempo.bpm_at(self.beat);
            let bps = bpm / 60.0 / self.sr as f64;
            let cycle = self.project.cycle;
            let mut wrap = false;
            if self.playing && self.count_in_beats <= 0.0 && cycle.enabled && cycle.end > cycle.start {
                let end_beat = self.beat + n as f64 * bps;
                if self.beat < cycle.end && end_beat >= cycle.end {
                    let frames_to_end = ((cycle.end - self.beat) / bps).ceil().max(1.0) as usize;
                    n = frames_to_end.min(n);
                    wrap = true;
                }
            }
            self.render_segment(&mut left[done..done + n], &mut right[done..done + n], bps, bpm);
            if self.playing {
                if self.count_in_beats > 0.0 {
                    self.count_in_beats -= n as f64 * bps;
                } else {
                    self.beat += n as f64 * bps;
                }
                if wrap {
                    self.release_timeline_notes();
                    self.beat = cycle.start;
                }
            }
            done += n;
        }
    }

    fn collect_timeline_events(project: &Project, track: &Track, b0: f64, b1: f64, bps: f64, frames: usize, out: &mut Vec<MidiEvent>, sounding: &mut HashSet<u8>) {
        let to_off = |b: f64| (((b - b0) / bps) as usize).min(frames - 1);
        for r in &track.regions {
            if r.muted || r.end() <= b0 || r.start >= b1 + 1e-9 {
                // still need note-offs for notes that end exactly in this block from regions ending at b0
                if r.muted || r.end() < b0 - 1e-9 || r.start >= b1 {
                    continue;
                }
            }
            let (notes_owned, transpose, vel_off, cc): (Option<Vec<Note>>, i32, i32, &[CcEvent]) = match &r.content {
                RegionContent::Midi(m) => (None, m.transpose, m.velocity_offset, &m.cc),
                RegionContent::Pattern(p) => (Some(pattern_to_notes(p)), 0, 0, &[]),
                RegionContent::Audio(_) => continue,
            };
            let notes: &[Note] = match (&notes_owned, &r.content) {
                (Some(v), _) => v,
                (None, RegionContent::Midi(m)) => &m.notes,
                _ => continue,
            };
            let len = r.length;
            let iters = r.loop_count.max(1);
            let first = (((b0 - r.start) / len).floor().max(0.0)) as u32;
            for k in first.saturating_sub(1)..iters.min(first + 2) {
                let it = r.start + k as f64 * len;
                if it >= b1 || it + len < b0 - 1e-9 {
                    continue;
                }
                for nt in notes {
                    if nt.start >= len {
                        continue;
                    }
                    let pitch = (nt.pitch as i32 + transpose).clamp(0, 127) as u8;
                    let on = it + nt.start;
                    let off = it + (nt.start + nt.length).min(len);
                    if on >= b0 && on < b1 {
                        let vel = (nt.velocity as i32 + vel_off).clamp(1, 127) as u8;
                        out.push(MidiEvent { offset: to_off(on), kind: MidiKind::NoteOn { pitch, velocity: vel } });
                        sounding.insert(pitch);
                    }
                    if off >= b0 && off < b1 && on < off {
                        // only send note-off if the note started (avoid orphan offs after locate)
                        if on < b0 || (on >= b0 && on < b1) {
                            out.push(MidiEvent { offset: to_off(off), kind: MidiKind::NoteOff { pitch } });
                        }
                    }
                }
                for c in cc {
                    let at = it + c.beat;
                    if c.beat < len && at >= b0 && at < b1 {
                        out.push(MidiEvent { offset: to_off(at), kind: MidiKind::Cc { controller: c.controller, value: c.value } });
                    }
                }
            }
        }
        let _ = project;
        // note-offs before note-ons at the same offset (retriggers)
        out.sort_by_key(|e| (e.offset, matches!(e.kind, MidiKind::NoteOn { .. }) as u8));
    }

    fn render_audio_regions(p: &Project, pool: &AudioPool, sample_rate: f32, track: &Track, b0: f64, frames: usize, l: &mut [f32], r: &mut [f32]) {
        let sec0 = p.tempo.beat_to_sec(b0);
        let bpm = p.tempo.bpm_at(b0);
        let bps = bpm / 60.0 / sample_rate as f64;
        let b1 = b0 + frames as f64 * bps;
        for reg in &track.regions {
            let RegionContent::Audio(a) = &reg.content else { continue };
            if reg.muted || reg.end() <= b0 || reg.start >= b1 {
                continue;
            }
            let Some(data) = pool.get(&a.file_id) else { continue };
            let fsr = data.sample_rate as f64;
            let stereo = data.channels.len() > 1;
            let dur = data.duration_sec();
            let gain = db_to_gain(a.gain_db);
            let iters = reg.loop_count.max(1);
            for k in 0..iters {
                let it = reg.start + k as f64 * reg.length;
                let it_end = it + reg.length;
                if it >= b1 || it_end <= b0 {
                    continue;
                }
                let it_sec = p.tempo.beat_to_sec(it);
                for i in 0..frames {
                    let beat = b0 + i as f64 * bps;
                    if beat < it || beat >= it_end {
                        continue;
                    }
                    let rel_sec = sec0 + i as f64 / sample_rate as f64 - it_sec + a.offset_sec;
                    let src_sec = if a.reverse { dur - rel_sec } else { rel_sec };
                    if src_sec < 0.0 || src_sec >= dur {
                        continue;
                    }
                    let pos = src_sec * fsr;
                    let in_reg = beat - it;
                    let mut g = gain;
                    if a.fade_in > 0.0 && in_reg < a.fade_in {
                        g *= (in_reg / a.fade_in) as f32;
                    }
                    let to_end = it_end - beat;
                    if a.fade_out > 0.0 && to_end < a.fade_out {
                        g *= (to_end / a.fade_out) as f32;
                    }
                    let sl = data.sample(0, pos);
                    let sr_ = if stereo { data.sample(1, pos) } else { sl };
                    l[i] += sl * g;
                    r[i] += sr_ * g;
                }
            }
        }
    }

    fn bus_order(project: &Project) -> Vec<Id> {
        fn depth(p: &Project, id: Id, guard: u32) -> u32 {
            if guard > 16 {
                return 0;
            }
            match p.track(id).map(|t| t.channel.output) {
                Some(Output::Bus(b)) => 1 + depth(p, b, guard + 1),
                _ => 0,
            }
        }
        let mut buses: Vec<(u32, Id)> = project.tracks.iter().filter(|t| t.kind == TrackKind::Bus).map(|t| (depth(project, t.id, 0), t.id)).collect();
        buses.sort_by(|a, b| b.0.cmp(&a.0));
        buses.into_iter().map(|x| x.1).collect()
    }

    fn render_segment(&mut self, out_l: &mut [f32], out_r: &mut [f32], bps: f64, bpm: f64) {
        let frames = out_l.len();
        let project = self.project.clone();
        let b0 = self.beat;
        let b1 = b0 + frames as f64 * bps;
        let ctx = ProcessContext { sample_rate: self.sr, bpm: bpm as f32, beat: b0, beats_per_sample: bps, playing: self.playing && self.count_in_beats <= 0.0 };
        let timeline_running = ctx.playing;
        let any_solo = project.any_solo();
        self.master_l[..frames].iter_mut().for_each(|x| *x = 0.0);
        self.master_r[..frames].iter_mut().for_each(|x| *x = 0.0);
        for st in self.tracks.values_mut() {
            st.in_l[..frames].iter_mut().for_each(|x| *x = 0.0);
            st.in_r[..frames].iter_mut().for_each(|x| *x = 0.0);
        }
        let live = std::mem::take(&mut self.live);
        let mut events: Vec<MidiEvent> = Vec::with_capacity(64);
        if timeline_running {
            self.apply_live_queue(b1);
        }
        let surround_mode = project.surround != SurroundFormat::Stereo;
        let surround_offset = if let Some(so) = self.surround_out.as_mut() {
            let off = so.channels[0].len();
            for c in so.channels.iter_mut() {
                c.resize(off + frames, 0.0);
            }
            Some(off)
        } else {
            None
        };

        // ---- source tracks (instrument + audio), then buses in dependency order
        let mut order: Vec<Id> = project.tracks.iter().filter(|t| t.kind != TrackKind::Bus).map(|t| t.id).collect();
        order.extend(Self::bus_order(&project));

        for tid in order {
            let Some(track) = project.track(tid) else { continue };
            // a playing Clip Grid cell replaces the track's timeline regions
            let live_track: Option<Track> = self.live_playing.get(&tid).map(|lp| lp.track.clone());
            let content_track: &Track = live_track.as_ref().unwrap_or(track);
            let audible = match &self.solo_override {
                Some(ids) => ids.contains(&tid) || track.kind == TrackKind::Bus,
                None => !track.channel.mute && (!any_solo || track.channel.solo || track.kind == TrackKind::Bus),
            };
            // automation values at block start
            let mut vol_db = track.channel.volume_db;
            let mut pan = track.channel.pan;
            let mut send_levels: Vec<f32> = track.channel.sends.iter().map(|s| s.level_db).collect();
            let mut plugin_auto: Vec<(Id, &str, f32)> = vec![];
            let mut sp = track.channel.surround_pan;
            if timeline_running {
                for lane in &track.automation {
                    let Some(v) = lane.value_at(b0) else { continue };
                    // The commands keep lanes finite and in range; a project built in code may not. inf or NaN
                    // here would stay in the state of every reverb downstream, so it is skipped, and the channel
                    // targets are held to the ranges of their commands.
                    if !v.is_finite() {
                        continue;
                    }
                    if lane.target == "surround:azimuth" {
                        sp.azimuth = v.clamp(-180.0, 180.0);
                    } else if lane.target == "surround:elevation" {
                        sp.elevation = v.clamp(-30.0, 90.0);
                    } else if lane.target == "surround:spread" {
                        sp.spread = v.clamp(0.0, 1.0);
                    } else if lane.target == "volume" {
                        vol_db = v.clamp(-96.0, 6.0);
                    } else if lane.target == "pan" {
                        pan = v.clamp(-1.0, 1.0);
                    } else if let Some(i) = lane.target.strip_prefix("send:").and_then(|s| s.parse::<usize>().ok()) {
                        if i < send_levels.len() {
                            send_levels[i] = v.clamp(-96.0, 6.0);
                        }
                    } else if let Some(rest) = lane.target.strip_prefix("plugin:") {
                        if let Some((pid, param)) = rest.split_once(':') {
                            if let Ok(pid) = pid.parse::<Id>() {
                                plugin_auto.push((pid, param, v));
                            }
                        }
                    }
                }
            }

            let st = self.tracks.get_mut(&tid).unwrap();
            for (pid, param, v) in &plugin_auto {
                if let Some(i) = st.inst.as_mut().filter(|i| i.id == *pid) {
                    i.inst.set_param(param, *v);
                }
                for f in st.fx.iter_mut().filter(|f| f.id == *pid) {
                    f.fx.set_param(param, *v);
                }
                for f in st.midi_fx.iter_mut().filter(|f| f.id == *pid) {
                    f.fx.set_param(param, *v);
                }
            }

            let (l, r) = (&mut st.l, &mut st.r);
            if track.kind == TrackKind::Bus {
                l[..frames].copy_from_slice(&st.in_l[..frames]);
                r[..frames].copy_from_slice(&st.in_r[..frames]);
            } else {
                l[..frames].iter_mut().for_each(|x| *x = 0.0);
                r[..frames].iter_mut().for_each(|x| *x = 0.0);
            }

            if track.kind == TrackKind::Instrument {
                events.clear();
                if timeline_running {
                    Self::collect_timeline_events(&project, content_track, b0, b1, bps, frames, &mut events, &mut st.timeline_notes);
                    for e in &events {
                        if let MidiKind::NoteOff { pitch } = e.kind {
                            st.timeline_notes.remove(&pitch);
                        }
                    }
                }
                for (lt, k) in &live {
                    if *lt == tid {
                        events.push(MidiEvent { offset: 0, kind: *k });
                        if self.recording && track.record_armed && timeline_running {
                            self.recorded.push(RecordedEvent { track_id: tid, beat: b0, kind: *k });
                        }
                    }
                }
                for m in st.midi_fx.iter_mut().filter(|m| !m.bypass) {
                    m.fx.process(&mut events, frames, &ctx, project.key);
                }
                if let Some(slot) = st.inst.as_mut() {
                    // sample-accurate event dispatch
                    let mut pos = 0;
                    let mut ei = 0;
                    events.sort_by_key(|e| e.offset);
                    while pos < frames {
                        while ei < events.len() && events[ei].offset <= pos {
                            match events[ei].kind {
                                MidiKind::NoteOn { pitch, velocity } => slot.inst.note_on(pitch, velocity),
                                MidiKind::NoteOff { pitch } => slot.inst.note_off(pitch),
                                MidiKind::Cc { controller, value } => slot.inst.control_change(controller, value),
                            }
                            ei += 1;
                        }
                        let next = if ei < events.len() { events[ei].offset.min(frames) } else { frames };
                        let end = next.max(pos + 1).min(frames);
                        slot.inst.render(&mut l[pos..end], &mut r[pos..end], &ctx);
                        pos = end;
                    }
                }
            }
            if track.kind == TrackKind::Audio && timeline_running {
                let st = self.tracks.get_mut(&tid).unwrap();
                Self::render_audio_regions(&project, &self.pool, self.sr, content_track, b0, frames, &mut st.l[..frames], &mut st.r[..frames]);
            }
            let st = self.tracks.get_mut(&tid).unwrap();
            for f in st.fx.iter_mut().filter(|f| !f.bypass) {
                f.fx.process(&mut st.l[..frames], &mut st.r[..frames], &ctx);
            }
            // fader + pan with per-block linear smoothing
            let g = if audible { db_to_gain(vol_db) } else { 0.0 };
            let to_master = !matches!(track.channel.output, Output::Bus(b) if b != tid && project.track(b).is_some());
            let (pl, pr) = if surround_mode && to_master { (1.0, 1.0) } else { pan_gains(pan) };
            let (tl, tr) = (g * pl, g * pr);
            let (sl, sr) = (st.gain_l, st.gain_r);
            let pre_l: Vec<f32> = if track.channel.sends.iter().any(|s| s.pre_fader) { st.l[..frames].to_vec() } else { vec![] };
            let pre_r: Vec<f32> = if !pre_l.is_empty() { st.r[..frames].to_vec() } else { vec![] };
            for i in 0..frames {
                let t = (i + 1) as f32 / frames as f32;
                st.l[i] *= sl + (tl - sl) * t;
                st.r[i] *= sr + (tr - sr) * t;
                st.peak[0] = st.peak[0].max(st.l[i].abs());
                st.peak[1] = st.peak[1].max(st.r[i].abs());
            }
            st.gain_l = tl;
            st.gain_r = tr;
            if let Some(an) = self.analyzers.as_mut().and_then(|a| a.get_mut(&tid)) {
                let st = self.tracks.get(&tid).unwrap();
                an.push(&st.l[..frames], &st.r[..frames]);
            }
            // route: copy out to avoid double mutable borrow of the map
            let st = self.tracks.get(&tid).unwrap();
            let out_l: Vec<f32> = st.l[..frames].to_vec();
            let out_r: Vec<f32> = st.r[..frames].to_vec();
            for (si, send) in track.channel.sends.iter().enumerate() {
                if !audible {
                    continue;
                }
                let sg = db_to_gain(send_levels[si]);
                let (src_l, src_r) = if send.pre_fader { (&pre_l, &pre_r) } else { (&out_l, &out_r) };
                if let Some(bus) = self.tracks.get_mut(&send.bus_id) {
                    for i in 0..frames {
                        bus.in_l[i] += src_l[i] * sg;
                        bus.in_r[i] += src_r[i] * sg;
                    }
                }
            }
            match track.channel.output {
                Output::Bus(b) if self.tracks.contains_key(&b) && b != tid => {
                    let bus = self.tracks.get_mut(&b).unwrap();
                    for i in 0..frames {
                        bus.in_l[i] += out_l[i];
                        bus.in_r[i] += out_r[i];
                    }
                }
                _ => {
                    if let (true, Some(off), true, Some(objs)) = (surround_mode, surround_offset, sp.object, self.object_out.as_mut()) {
                        let buf = objs.entry(tid).or_default();
                        buf.resize(off, 0.0);
                        for i in 0..frames {
                            buf.push((out_l[i] + out_r[i]) * 0.5);
                        }
                    } else if let (true, Some(off), Some(so)) = (surround_mode, surround_offset, self.surround_out.as_mut()) {
                        let w = 15.0 + sp.spread * 60.0;
                        let gl = spatial::vbap_gains(&so.layout, sp.azimuth - w, sp.elevation, sp.spread * 0.5);
                        let gr = spatial::vbap_gains(&so.layout, sp.azimuth + w, sp.elevation, sp.spread * 0.5);
                        for (c, ch) in so.channels.iter_mut().enumerate() {
                            if so.layout[c].2 < -900.0 {
                                continue;
                            }
                            for i in 0..frames {
                                ch[off + i] += out_l[i] * gl[c] + out_r[i] * gr[c];
                            }
                        }
                    } else if surround_mode {
                        let st = self.tracks.get_mut(&tid).unwrap();
                        let b = st.binaural.get_or_insert_with(|| Binaural::new(self.sr));
                        let mut bl = out_l.clone();
                        let mut br = out_r.clone();
                        b.process(&mut bl, &mut br, sp.azimuth, sp.elevation, sp.spread);
                        for i in 0..frames {
                            self.master_l[i] += bl[i];
                            self.master_r[i] += br[i];
                        }
                    } else {
                        for i in 0..frames {
                            self.master_l[i] += out_l[i];
                            self.master_r[i] += out_r[i];
                        }
                    }
                }
            }
        }
        // surround bounce: apply master fader to the multichannel bed
        if let (Some(off), Some(so)) = (surround_offset, self.surround_out.as_mut()) {
            let g = if project.master.mute { 0.0 } else { db_to_gain(project.master.volume_db) };
            for ch in so.channels.iter_mut() {
                for v in &mut ch[off..off + frames] {
                    *v *= g;
                }
            }
            if let Some(objs) = self.object_out.as_mut() {
                for buf in objs.values_mut() {
                    buf.resize(off + frames, 0.0);
                    for v in &mut buf[off..off + frames] {
                        *v *= g;
                    }
                }
            }
        }

        // ---- master
        for f in self.master_fx.iter_mut().filter(|f| !f.bypass) {
            f.fx.process(&mut self.master_l[..frames], &mut self.master_r[..frames], &ctx);
        }
        let g = if project.master.mute { 0.0 } else { db_to_gain(project.master.volume_db) };
        let (pl, pr) = pan_gains(project.master.pan);
        let (tl, tr) = (g * pl, g * pr);
        let [sl, sr] = self.master_gain;
        for i in 0..frames {
            let t = (i + 1) as f32 / frames as f32;
            out_l[i] = self.master_l[i] * (sl + (tl - sl) * t);
            out_r[i] = self.master_r[i] * (sr + (tr - sr) * t);
            self.master_peak[0] = self.master_peak[0].max(out_l[i].abs());
            self.master_peak[1] = self.master_peak[1].max(out_r[i].abs());
        }
        self.master_gain = [tl, tr];
        if let Some(an) = self.analyzers.as_mut().and_then(|a| a.get_mut(&MASTER_ID)) {
            an.push(&out_l[..frames], &out_r[..frames]);
        }

        // ---- metronome (not printed into analysis)
        let counting_in = self.playing && self.count_in_beats > 0.0;
        // "only while recording" silences plain playback; the count-in always clicks
        let click_on = project.metronome && (self.recording || !project.metronome_only_recording);
        if self.playing && (click_on || counting_in) && self.solo_override.is_none() && self.analyzers.is_none() {
            let click_gain = project.metronome_gain();
            // A time signature the commands refuse (0 beats, denominator 0) can still arrive in a project built in
            // code: count it as 1 beat of a quarter note rather than divide by zero on the audio thread.
            let beats_per_bar = i64::from(project.time_signature.numerator.max(1));
            let beat_len = 4.0 / f64::from(project.time_signature.denominator.max(1));
            let bar_len = beats_per_bar as f64 * beat_len;
            let start = if counting_in { -self.count_in_beats } else { b0 };
            for i in 0..frames {
                let b = start + i as f64 * bps;
                let prev = b - bps;
                if (b / beat_len).floor() != (prev / beat_len).floor() || (i == 0 && (b / beat_len).fract().abs() < bps / beat_len) {
                    let down = ((b.rem_euclid(bar_len)) / beat_len).round() as i64 % beats_per_bar == 0;
                    self.click_env = 1.0;
                    self.click_freq = if down { 1600.0 } else { 1000.0 };
                }
                if self.click_env > 1e-3 {
                    self.click_phase = (self.click_phase + self.click_freq / self.sr).fract();
                    let s = (self.click_phase * std::f32::consts::TAU).sin() * self.click_env * click_gain;
                    self.click_env *= 0.9985;
                    out_l[i] += s;
                    out_r[i] += s;
                }
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use ondina_core::command::Command;
    use ondina_core::generate::DrumStyle;
    use ondina_core::ops::{self, Outcome};

    fn run(p: &mut Project, c: Command) -> serde_json::Value {
        match ops::apply(p, &c).unwrap() {
            Outcome::Done(v) => v,
            _ => panic!(),
        }
    }

    #[test]
    fn renders_song_with_cycle_and_sends() {
        let mut p = Project::new("t");
        let d = run(&mut p, Command::AddTrack { kind: TrackKind::Instrument, name: None, patch: Some("Pop Drum Kit".into()), instrument: None, preset: None, index: None })["track_id"].as_u64().unwrap();
        run(&mut p, Command::GenerateDrums { track_id: d, start: 0.0, bars: 2, style: DrumStyle::Rock, intensity: None, fill: None, seed: Some(3) });
        let s = run(&mut p, Command::AddTrack { kind: TrackKind::Instrument, name: None, patch: Some("Lush Pad".into()), instrument: None, preset: None, index: None })["track_id"].as_u64().unwrap();
        let rid = run(&mut p, Command::CreateMidiRegion { track_id: s, start: 0.0, length: 8.0, name: None, notes: None })["region_id"].as_u64().unwrap();
        run(&mut p, Command::InsertChord { region_id: rid, symbol: "Am7".into(), start: 0.0, length: 8.0, octave: None, inversion: None, velocity: None, with_bass: None });
        let bus = run(&mut p, Command::AddTrack { kind: TrackKind::Bus, name: None, patch: None, instrument: None, preset: None, index: None })["track_id"].as_u64().unwrap();
        run(&mut p, Command::AddEffect { track_id: bus, kind: "reverb".into(), preset: Some("Reverb Return 100%".into()), index: None });
        run(&mut p, Command::AddSend { track_id: s, bus_id: bus, level_db: Some(-6.0), pre_fader: None });
        run(&mut p, Command::SetCycle { enabled: true, start: Some(0.0), end: Some(4.0) });
        let mut r = Renderer::new(48000.0, Arc::new(p), Arc::new(AudioPool::new()));
        r.play();
        let mut l = vec![0.0; 512];
        let mut rr = vec![0.0; 512];
        let mut energy = 0.0;
        for _ in 0..(48000 * 3 / 512) {
            r.render(&mut l, &mut rr);
            energy += l.iter().map(|x| x * x).sum::<f32>();
            assert!(l.iter().all(|x| x.is_finite()));
        }
        assert!(energy > 1.0, "no audio rendered");
        assert!(r.beat < 4.0 + 0.1, "cycle did not wrap: {}", r.beat);
    }

    fn add_track(p: &mut Project, kind: TrackKind) -> Id {
        run(p, Command::AddTrack { kind, name: None, patch: None, instrument: None, preset: None, index: None })["track_id"].as_u64().unwrap()
    }

    /// Left-channel energy of about one second rendered from the start by a fresh renderer.
    fn energy(p: &Project) -> f32 {
        let mut r = Renderer::new(48000.0, Arc::new(p.clone()), Arc::new(AudioPool::new()));
        r.play();
        let mut l = vec![0.0; 512];
        let mut rr = vec![0.0; 512];
        let mut e = 0.0;
        for _ in 0..(48000 / 512) {
            r.render(&mut l, &mut rr);
            assert!(l.iter().chain(&rr).all(|x| x.is_finite()));
            e += l.iter().map(|x| x * x).sum::<f32>();
        }
        e
    }

    /// A project that skipped the checks at the boundary (built in code, or a file from an older version) must not
    /// stop the audio thread: the metronome with 0 beats per bar and an empty tempo map used to panic.
    #[test]
    fn metronome_and_empty_tempo_map_never_panic_the_renderer() {
        let mut p = Project::new("t");
        p.metronome = true;
        p.count_in_bars = 1;
        p.time_signature = TimeSignature { numerator: 0, denominator: 0 };
        p.tempo.points.clear();
        let mut r = Renderer::new(48000.0, Arc::new(p), Arc::new(AudioPool::new()));
        r.play();
        let mut l = vec![0.0; 512];
        let mut rr = vec![0.0; 512];
        for _ in 0..(48000 / 512) {
            r.render(&mut l, &mut rr);
            assert!(l.iter().chain(&rr).all(|x| x.is_finite()));
        }
        assert!(r.beat > 0.0, "the transport did not move");
    }

    /// Automation that skipped the checks of the commands (values not finite or far out of range) must not put
    /// inf or NaN into the mix, where a bus reverb would keep it.
    #[test]
    fn automation_that_is_not_finite_or_out_of_range_keeps_the_mix_finite() {
        for bad in [f32::INFINITY, f32::NAN, 1000.0] {
            let mut p = Project::new("t");
            let src = add_track(&mut p, TrackKind::Instrument);
            let held = Note { pitch: 60, velocity: 100, start: 0.0, length: 8.0 };
            run(&mut p, Command::CreateMidiRegion { track_id: src, start: 0.0, length: 8.0, name: None, notes: Some(vec![held]) });
            let bus = add_track(&mut p, TrackKind::Bus);
            run(&mut p, Command::AddEffect { track_id: bus, kind: "reverb".into(), preset: None, index: None });
            run(&mut p, Command::AddSend { track_id: src, bus_id: bus, level_db: Some(0.0), pre_fader: None });
            let points = vec![AutomationPoint { beat: 0.0, value: bad }];
            let t = p.tracks.iter_mut().find(|t| t.id == src).unwrap();
            for target in ["volume", "pan", "send:0"] {
                t.automation.push(AutomationLane { target: target.into(), points: points.clone(), enabled: true });
            }
            // `energy` asserts that every sample is finite
            let e = energy(&p);
            assert!(e.is_finite(), "{bad}: {e}");
        }
    }

    #[test]
    fn send_automation_follows_its_bus_after_another_bus_is_removed() {
        let mut p = Project::new("t");
        let src = add_track(&mut p, TrackKind::Instrument);
        let held = Note { pitch: 60, velocity: 100, start: 0.0, length: 8.0 };
        run(&mut p, Command::CreateMidiRegion { track_id: src, start: 0.0, length: 8.0, name: None, notes: Some(vec![held]) });
        // the direct path is silent: only the pre-fader sends reach the master
        run(&mut p, Command::SetVolume { track_id: src, db: -96.0 });
        let buses = [add_track(&mut p, TrackKind::Bus), add_track(&mut p, TrackKind::Bus), add_track(&mut p, TrackKind::Bus)];
        for bus in buses {
            run(&mut p, Command::AddSend { track_id: src, bus_id: bus, level_db: Some(-96.0), pre_fader: Some(true) });
        }
        run(&mut p, Command::SetMute { track_id: buses[0], mute: true });
        for (i, db) in [0.0, -96.0, 0.0].into_iter().enumerate() {
            run(&mut p, Command::SetAutomation { track_id: src, target: format!("send:{i}"), points: vec![AutomationPoint { beat: 0.0, value: db }] });
        }
        let e_before = energy(&p);
        assert!(e_before > 1e-3, "nothing reached the master through bus C: {e_before}");
        run(&mut p, Command::RemoveTrack { track_id: buses[1] });
        let e_after = energy(&p);
        assert!(e_after > 0.5 * e_before, "bus C lost its send automation: before {e_before}, after {e_after}");
    }

    #[test]
    fn removing_a_track_stops_its_playing_live_cell() {
        let mut p = Project::new("t");
        let t = add_track(&mut p, TrackKind::Instrument);
        add_track(&mut p, TrackKind::Instrument);
        let note = Note { pitch: 60, velocity: 100, start: 0.0, length: 1.0 };
        let cell = run(&mut p, Command::CreateLiveCell { track_id: t, scene: 0, length: Some(4.0), notes: Some(vec![note]), from_region_id: None, name: None })["cell_id"].as_u64().unwrap();
        let mut r = Renderer::new(48000.0, Arc::new(p.clone()), Arc::new(AudioPool::new()));
        r.launch_cell(cell);
        let mut l = vec![0.0; 512];
        let mut rr = vec![0.0; 512];
        r.render(&mut l, &mut rr);
        assert!(r.live_state().0.iter().any(|x| x.1 == cell));
        run(&mut p, Command::RemoveTrack { track_id: t });
        r.set_project(Arc::new(p));
        assert!(r.live_state().0.is_empty());
        for _ in 0..20 {
            r.render(&mut l, &mut rr);
            assert!(l.iter().chain(&rr).all(|x| x.is_finite()));
        }
        assert!(r.live_state().0.is_empty());
    }

    #[test]
    fn duplicated_track_automation_drives_the_copy_plugins() {
        let mut p = Project::new("t");
        let src = add_track(&mut p, TrackKind::Instrument);
        let held = Note { pitch: 60, velocity: 100, start: 0.0, length: 8.0 };
        run(&mut p, Command::CreateMidiRegion { track_id: src, start: 0.0, length: 8.0, name: None, notes: Some(vec![held]) });
        let g = run(&mut p, Command::AddEffect { track_id: src, kind: "gain".into(), preset: None, index: None })["plugin_id"].as_u64().unwrap();
        let open = energy(&p);
        assert!(open > 1e-3, "the track is silent without automation: {open}");
        run(&mut p, Command::SetAutomation { track_id: src, target: format!("plugin:{g}:gain"), points: vec![AutomationPoint { beat: 0.0, value: -60.0 }] });
        let copy = run(&mut p, Command::DuplicateTrack { track_id: src })["track_id"].as_u64().unwrap();
        run(&mut p, Command::SetMute { track_id: src, mute: true });
        let e_copy = energy(&p);
        assert!(e_copy < 0.01 * open, "the copy's gain lane does not reach its own gain plug-in: open {open}, copy {e_copy}");
        // the copy is silent because of its lane, not for another reason
        let mut q = p.clone();
        q.track_mut(copy).unwrap().automation.clear();
        let e_free = energy(&q);
        assert!(e_free > 0.5 * open, "the copy should play without its lane: open {open}, free {e_free}");
    }

    /// Left-channel peak of the first half second of a fresh renderer playing an empty `p` from beat 0, so the
    /// output is the click alone.
    fn click_peak(p: &Project, recording: bool, count_in_beats: f64) -> f32 {
        let mut r = Renderer::new(48000.0, Arc::new(p.clone()), Arc::new(AudioPool::new()));
        r.recording = recording;
        r.count_in_beats = count_in_beats;
        r.play();
        let mut l = vec![0.0; 512];
        let mut rr = vec![0.0; 512];
        let mut peak = 0.0f32;
        for _ in 0..(48000 / 2 / 512) {
            r.render(&mut l, &mut rr);
            assert!(l.iter().chain(&rr).all(|x| x.is_finite()));
            peak = l.iter().fold(peak, |m, x| m.max(x.abs()));
        }
        peak
    }

    #[test]
    fn metronome_level_scales_the_click() {
        let mut p = Project::new("t");
        p.metronome = true;
        p.metronome_level_db = 0.0;
        let p0 = click_peak(&p, false, 0.0);
        p.metronome_level_db = -20.0;
        let p20 = click_peak(&p, false, 0.0);
        p.metronome_level_db = ondina_core::model::METRONOME_DEFAULT_DB;
        let pd = click_peak(&p, false, 0.0);
        assert!(p0 > 0.5, "full-scale click too quiet: {p0}");
        assert!((p20 / p0 - 0.1).abs() < 0.001, "-20 dB: {p20} vs {p0}");
        assert!((pd / p0 - 0.35).abs() < 0.001, "default: {pd} vs {p0}");
    }

    #[test]
    fn click_only_while_recording_is_silent_in_plain_playback() {
        let mut p = Project::new("t");
        p.metronome = true;
        p.metronome_only_recording = true;
        assert_eq!(click_peak(&p, false, 0.0), 0.0, "plain playback must not click");
        assert!(click_peak(&p, true, 0.0) > 0.1, "recording must click");
        assert!(click_peak(&p, false, 4.0) > 0.1, "the count-in always clicks");
        p.metronome = false;
        assert_eq!(click_peak(&p, true, 0.0), 0.0, "the mode does not turn the metronome on");
        p.metronome = true;
        p.metronome_only_recording = false;
        assert!(click_peak(&p, false, 0.0) > 0.1, "by default the click sounds in every playback");
    }
}
