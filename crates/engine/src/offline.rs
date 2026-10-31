//! Offline (faster than real-time) rendering: bounce and mix analysis.

use std::collections::HashMap;
use std::sync::Arc;

use ondina_core::model::{Id, Project, MASTER_ID};

use crate::analysis::{AnalysisReport, Analyzer};
use crate::pool::AudioPool;
use crate::renderer::Renderer;

/// Fixed tail the bounce commands always render after the end, in seconds (also the minimum of the automatic tail).
pub const BOUNCE_TAIL_SEC: f64 = 2.0;
/// Ceiling of the automatic tail of the bounce commands, in seconds after the end.
pub const AUTO_TAIL_MAX_SEC: f64 = 20.0;
/// Below this level (about -90 dBFS) the output counts as silent.
const TAIL_SILENCE: f32 = 3.2e-5;
/// The automatic tail ends after this much continuous silence, which is also left at the end of the file.
const TAIL_QUIET_SEC: f64 = 0.25;

/// Seconds to frames, rounding up. The small epsilon keeps exact lengths exact: 9.6 s at 44.1 kHz is
/// 423360.00000000006 in floating point and must stay 423360 frames.
fn frames(sec: f64, sample_rate: u32) -> usize {
    (sec * sample_rate as f64 - 1e-6).ceil().max(0.0) as usize
}

/// Follows a render block by block and remembers the last audible frame, to end an automatic tail.
struct TailGate {
    threshold: f32,
    quiet_needed: usize,
    frames: usize,
    /// One past the last frame above the threshold (0 = nothing audible yet).
    last_loud: usize,
}

impl TailGate {
    fn new(sample_rate: u32) -> Self {
        TailGate { threshold: TAIL_SILENCE, quiet_needed: (TAIL_QUIET_SEC * sample_rate as f64).round() as usize, frames: 0, last_loud: 0 }
    }

    /// Feed one block: one slice per channel, all of the same length. NaN counts as audible.
    fn feed(&mut self, channels: &[&[f32]]) {
        let n = channels.first().map_or(0, |c| c.len());
        let thr = self.threshold;
        if let Some(i) = (0..n).rev().find(|&i| channels.iter().any(|c| c[i].abs() > thr || c[i].is_nan())) {
            self.last_loud = self.frames + i + 1;
        }
        self.frames += n;
    }

    /// True once the last `TAIL_QUIET_SEC` fed were all silent.
    fn is_quiet(&self) -> bool {
        self.frames - self.last_loud >= self.quiet_needed
    }

    /// Frames to keep: the quiet margin after the last audible frame, never less than `min`, never more than fed.
    fn keep_len(&self, min: usize) -> usize {
        (self.last_loud + self.quiet_needed).max(min).min(self.frames)
    }
}

pub struct OfflineResult {
    pub left: Vec<f32>,
    pub right: Vec<f32>,
    pub sample_rate: u32,
    pub analysis: Option<HashMap<Id, AnalysisReport>>,
}

pub struct OfflineOptions {
    pub sample_rate: u32,
    pub start: f64,
    pub end: f64,
    /// Seconds always rendered after `end`. With an automatic tail this is the minimum.
    pub tail_sec: f64,
    /// Automatic tail: when larger than `tail_sec`, keep rendering after `end + tail_sec` until the output stays
    /// below about -90 dBFS for 250 ms, up to `end + auto_tail_max_sec`, then trim the trailing silence.
    /// 0 turns it off.
    pub auto_tail_max_sec: f64,
    pub solo: Option<Vec<Id>>,
    pub analyze: bool,
    pub keep_audio: bool,
}

pub fn render_offline(project: &Project, pool: Arc<AudioPool>, opt: OfflineOptions) -> OfflineResult {
    let mut p = project.clone();
    p.cycle.enabled = false;
    p.metronome = false;
    let sr = opt.sample_rate;
    let span = (p.tempo.beat_to_sec(opt.end) - p.tempo.beat_to_sec(opt.start)).max(0.0);
    let min_total = frames(span + opt.tail_sec, sr);
    let max_total = if opt.auto_tail_max_sec > opt.tail_sec { frames(span + opt.auto_tail_max_sec, sr) } else { min_total };
    let mut gate = if max_total > min_total { Some(TailGate::new(sr)) } else { None };
    let ids: Vec<Id> = p.tracks.iter().map(|t| t.id).collect();
    let mut r = Renderer::new(sr as f32, Arc::new(p), pool);
    r.solo_override = opt.solo;
    if opt.analyze {
        let mut m: HashMap<Id, Analyzer> = ids.iter().map(|id| (*id, Analyzer::new(sr as f32))).collect();
        m.insert(MASTER_ID, Analyzer::new(sr as f32));
        r.analyzers = Some(m);
    }
    r.locate(opt.start);
    r.play();
    let block = 1024;
    let mut left = Vec::with_capacity(if opt.keep_audio { min_total } else { 0 });
    let mut right = Vec::with_capacity(if opt.keep_audio { min_total } else { 0 });
    let mut bl = vec![0.0; block];
    let mut br = vec![0.0; block];
    let mut done = 0;
    while done < max_total {
        let quiet = match &gate {
            Some(g) => g.is_quiet(),
            None => true,
        };
        if done >= min_total && quiet {
            break;
        }
        let n = (max_total - done).min(block);
        r.render(&mut bl[..n], &mut br[..n]);
        if let Some(g) = gate.as_mut() {
            g.feed(&[&bl[..n], &br[..n]]);
        }
        if opt.keep_audio {
            left.extend_from_slice(&bl[..n]);
            right.extend_from_slice(&br[..n]);
        }
        done += n;
    }
    if let Some(g) = &gate {
        let keep = g.keep_len(min_total);
        left.truncate(keep);
        right.truncate(keep);
    }
    let analysis = r.analyzers.take().map(|m| m.into_iter().map(|(k, v)| (k, v.report())).collect());
    OfflineResult { left, right, sample_rate: sr, analysis }
}

/// Peak-normalize to -1 dBFS.
pub fn normalize(left: &mut [f32], right: &mut [f32]) {
    let peak = left.iter().chain(right.iter()).fold(0.0f32, |a, b| a.max(b.abs()));
    if peak > 1e-6 {
        let g = 0.891 / peak;
        left.iter_mut().chain(right.iter_mut()).for_each(|x| *x *= g);
    }
}

/// Render a multichannel surround bed ("5.1", "7.1", "7.1.4"). Returns (channel names, channels).
/// The tail is at least `BOUNCE_TAIL_SEC`; with `auto_tail_max_sec` above that it goes on until the bed fades.
pub fn render_surround(project: &Project, pool: Arc<AudioPool>, sample_rate: u32, start: f64, end: f64, layout_name: &str, auto_tail_max_sec: f64) -> (Vec<&'static str>, Vec<Vec<f32>>) {
    let mut p = project.clone();
    p.cycle.enabled = false;
    p.metronome = false;
    if p.surround == ondina_core::model::SurroundFormat::Stereo || p.surround == ondina_core::model::SurroundFormat::Binaural {
        p.surround = ondina_core::model::SurroundFormat::Surround51;
    }
    let layout = ondina_dsp::spatial::layout(layout_name);
    let names: Vec<&'static str> = layout.iter().map(|s| s.0).collect();
    let n = layout.len();
    let span = (p.tempo.beat_to_sec(end) - p.tempo.beat_to_sec(start)).max(0.0);
    let min_total = frames(span + BOUNCE_TAIL_SEC, sample_rate);
    let max_total = if auto_tail_max_sec > BOUNCE_TAIL_SEC { frames(span + auto_tail_max_sec, sample_rate) } else { min_total };
    let mut gate = if max_total > min_total { Some(TailGate::new(sample_rate)) } else { None };
    let mut r = Renderer::new(sample_rate as f32, Arc::new(p), pool);
    r.surround_out = Some(crate::renderer::SurroundOut { layout: layout.clone(), channels: vec![Vec::with_capacity(min_total); n] });
    r.locate(start);
    r.play();
    let mut bl = vec![0.0; 1024];
    let mut br = vec![0.0; 1024];
    let mut done = 0;
    while done < max_total {
        let quiet = match &gate {
            Some(g) => g.is_quiet(),
            None => true,
        };
        if done >= min_total && quiet {
            break;
        }
        let k = (max_total - done).min(1024);
        let off = r.surround_out.as_ref().map_or(0, |so| so.channels[0].len());
        r.render(&mut bl[..k], &mut br[..k]);
        // In surround mode the tracks go to the bed, not to bl/br; LFE is still zero here and is derived below.
        if let (Some(g), Some(so)) = (gate.as_mut(), r.surround_out.as_ref()) {
            let block: Vec<&[f32]> = so.channels.iter().map(|c| &c[off..]).collect();
            g.feed(&block);
        }
        done += k;
    }
    let mut chans = r.surround_out.take().unwrap().channels;
    if let Some(g) = &gate {
        let keep = g.keep_len(min_total);
        for c in chans.iter_mut() {
            c.truncate(keep);
        }
    }
    // LFE: low-passed sum of all main channels
    if let Some(lfe) = layout.iter().position(|s| s.2 < -900.0) {
        let mut lp = ondina_dsp::util::Biquad::new(ondina_dsp::util::BiquadKind::LowPass, 120.0, 0.707, 0.0, sample_rate as f32);
        let len = chans[0].len();
        let sum: Vec<f32> = (0..len).map(|i| (0..n).filter(|c| *c != lfe).map(|c| chans[c][i]).sum::<f32>()).collect();
        chans[lfe] = sum.iter().map(|x| lp.process(*x) * 0.3).collect();
    }
    (names, chans)
}

/// Write an N-channel WAV (32-bit float).
pub fn write_multichannel_wav(path: &std::path::Path, channels: &[Vec<f32>], sample_rate: u32) -> Result<(), String> {
    if let Some(parent) = path.parent() {
        if !parent.as_os_str().is_empty() {
            std::fs::create_dir_all(parent).map_err(|e| e.to_string())?;
        }
    }
    let spec = hound::WavSpec { channels: channels.len() as u16, sample_rate, bits_per_sample: 32, sample_format: hound::SampleFormat::Float };
    let mut w = hound::WavWriter::create(path, spec).map_err(|e| e.to_string())?;
    let len = channels.iter().map(|c| c.len()).min().unwrap_or(0);
    for i in 0..len {
        for c in channels {
            w.write_sample(c[i]).map_err(|e| e.to_string())?;
        }
    }
    w.finalize().map_err(|e| e.to_string())
}

/// Render an object-based mix: a 7.1.2 bed (non-object tracks) plus one mono stem per object track.
/// Returns (bed channels in 7.1.2 order, objects as (track id, audio)).
pub fn render_objects(project: &Project, pool: Arc<AudioPool>, sample_rate: u32, start: f64, end: f64) -> (Vec<Vec<f32>>, Vec<(Id, Vec<f32>)>) {
    let mut p = project.clone();
    p.cycle.enabled = false;
    p.metronome = false;
    p.surround = ondina_core::model::SurroundFormat::Immersive714;
    let layout = ondina_dsp::spatial::layout("7.1.2");
    let n = layout.len();
    let secs = (p.tempo.beat_to_sec(end) - p.tempo.beat_to_sec(start)).max(0.0) + 1.0;
    let total = (secs * sample_rate as f64).ceil() as usize;
    let object_ids: Vec<Id> = p.tracks.iter().filter(|t| t.channel.surround_pan.object).map(|t| t.id).collect();
    let mut r = Renderer::new(sample_rate as f32, Arc::new(p), pool);
    r.surround_out = Some(crate::renderer::SurroundOut { layout: layout.clone(), channels: vec![Vec::with_capacity(total); n] });
    r.object_out = Some(object_ids.iter().map(|id| (*id, Vec::with_capacity(total))).collect());
    r.locate(start);
    r.play();
    let mut bl = vec![0.0; 1024];
    let mut br = vec![0.0; 1024];
    let mut done = 0;
    while done < total {
        let k = (total - done).min(1024);
        r.render(&mut bl[..k], &mut br[..k]);
        done += k;
    }
    let mut bed = r.surround_out.take().unwrap().channels;
    if let Some(lfe) = layout.iter().position(|s| s.2 < -900.0) {
        let mut lp = ondina_dsp::util::Biquad::new(ondina_dsp::util::BiquadKind::LowPass, 120.0, 0.707, 0.0, sample_rate as f32);
        let len = bed[0].len();
        let sum: Vec<f32> = (0..len).map(|i| (0..n).filter(|c| *c != lfe).map(|c| bed[c][i]).sum::<f32>()).collect();
        bed[lfe] = sum.iter().map(|x| lp.process(*x) * 0.3).collect();
    }
    let len = bed[0].len();
    let mut objs = r.object_out.take().unwrap_or_default();
    let objects = object_ids
        .into_iter()
        .map(|id| {
            let mut a = objs.remove(&id).unwrap_or_default();
            a.resize(len, 0.0);
            (id, a)
        })
        .collect();
    (bed, objects)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::BTreeMap;
    use ondina_core::command::Command;
    use ondina_core::model::{Note, TrackKind};
    use ondina_core::ops::{self, Outcome};

    fn run(p: &mut Project, c: Command) -> serde_json::Value {
        match ops::apply(p, &c).unwrap() {
            Outcome::Done(v) => v,
            _ => panic!(),
        }
    }

    fn note_project(reverb: bool, note_beats: f64, region_beats: f64) -> Project {
        let mut p = Project::new("t");
        let t = run(&mut p, Command::AddTrack { kind: TrackKind::Instrument, name: None, patch: None, instrument: None, preset: None, index: None })["track_id"].as_u64().unwrap();
        run(&mut p, Command::CreateMidiRegion { track_id: t, start: 0.0, length: region_beats, name: None, notes: Some(vec![Note { pitch: 60, velocity: 110, start: 0.0, length: note_beats }]) });
        if reverb {
            let fx = run(&mut p, Command::AddEffect { track_id: t, kind: "reverb".into(), preset: None, index: None })["plugin_id"].as_u64().unwrap();
            let params: BTreeMap<String, f32> = [("size", 0.95), ("damping", 0.0), ("mix", 0.5)].iter().map(|(k, v)| (k.to_string(), *v)).collect();
            run(&mut p, Command::SetPluginParams { plugin_id: fx, params });
        }
        p
    }

    fn opts(end: f64, tail_sec: f64, auto_tail_max_sec: f64) -> OfflineOptions {
        OfflineOptions { sample_rate: 48000, start: 0.0, end, tail_sec, auto_tail_max_sec, solo: None, analyze: false, keep_audio: true }
    }

    /// Peak of the last `ms` milliseconds at 48 kHz.
    fn tail_peak(x: &[f32], ms: usize) -> f32 {
        let n = (48 * ms).min(x.len());
        x[x.len() - n..].iter().fold(0.0f32, |a, b| a.max(b.abs()))
    }

    fn feed_const(g: &mut TailGate, v: f32, n: usize) {
        let block = vec![v; n];
        g.feed(&[&block, &block]);
    }

    #[test]
    fn frames_keeps_exact_lengths_exact() {
        assert_eq!(frames(9.6, 44100), 423360);
        assert_eq!(frames(9.6, 48000), 460800);
        assert_eq!(frames(4.0, 48000), 192000);
        assert_eq!(frames(0.00001, 48000), 1);
        assert_eq!(frames(0.0, 48000), 0);
    }

    #[test]
    fn tail_gate_trims_to_the_quiet_margin() {
        let mut g = TailGate::new(48000);
        feed_const(&mut g, 0.5, 4800);
        assert!(!g.is_quiet());
        let mut zeros = 0;
        while g.frames < 48000 {
            let n = 1024.min(48000 - g.frames);
            feed_const(&mut g, 0.0, n);
            zeros += n;
            assert_eq!(g.is_quiet(), zeros >= 12000, "after {zeros} silent frames");
        }
        assert_eq!(g.keep_len(0), 4800 + 12000);
        assert_eq!(g.keep_len(30000), 30000);
        assert_eq!(g.keep_len(10_000_000), 48000);
    }

    #[test]
    fn tail_gate_threshold_is_about_minus_90_db() {
        let mut g = TailGate::new(48000);
        feed_const(&mut g, 1.8e-5, 1024);
        assert_eq!(g.last_loud, 0);
        feed_const(&mut g, 5.6e-5, 1024);
        assert_eq!(g.last_loud, 2048);
        let mut g = TailGate::new(48000);
        let l = vec![0.0f32; 512];
        let r = vec![0.5f32; 512];
        g.feed(&[&l, &r]);
        assert_eq!(g.last_loud, 512);
    }

    #[test]
    fn tail_gate_never_quiet_on_noise() {
        let mut g = TailGate::new(48000);
        let mut seed: u32 = 12345;
        let mut fed = 0;
        while fed < 96000 {
            let block: Vec<f32> = (0..1024)
                .map(|_| {
                    seed = seed.wrapping_mul(1664525).wrapping_add(1013904223);
                    0.001 * ((seed >> 8) as f32 / 16_777_216.0 * 2.0 - 1.0).signum()
                })
                .collect();
            g.feed(&[&block, &block]);
            fed += 1024;
            assert!(!g.is_quiet());
        }
        assert_eq!(g.keep_len(0), fed);
    }

    #[test]
    fn tail_gate_counts_nan_as_audible() {
        let mut g = TailGate::new(48000);
        let mut block = vec![0.0f32; 1024];
        block[100] = f32::NAN;
        g.feed(&[&block, &block]);
        assert_eq!(g.last_loud, 101);
    }

    #[test]
    fn auto_tail_keeps_a_long_reverb_tail() {
        let p = note_project(true, 0.5, 4.0);
        let res = render_offline(&p, Arc::new(AudioPool::new()), opts(4.0, 2.0, 20.0));
        assert_eq!(res.left.len(), res.right.len());
        assert!(res.left.len() > frames(4.5, 48000), "length {}", res.left.len());
        assert!(res.left.len() < frames(21.0, 48000), "length {}", res.left.len());
        assert!(tail_peak(&res.left, 10) < 1e-4 && tail_peak(&res.right, 10) < 1e-4);
        assert!(res.left.iter().chain(res.right.iter()).all(|x| x.is_finite()));
        // Today's fixed 2 s cuts the tail while it is still audible.
        let cut = render_offline(&p, Arc::new(AudioPool::new()), opts(4.0, 2.0, 0.0));
        assert_eq!(cut.left.len(), frames(4.0, 48000));
        assert!(tail_peak(&cut.left, 10).max(tail_peak(&cut.right, 10)) > 1e-4);
    }

    #[test]
    fn auto_tail_adds_nothing_to_a_dry_mix() {
        let p = note_project(false, 0.5, 4.0);
        assert!(p.tracks.iter().all(|t| t.channel.inserts.is_empty()));
        let res = render_offline(&p, Arc::new(AudioPool::new()), opts(4.0, 2.0, 20.0));
        assert_eq!(res.left.len(), 192000);
        assert_eq!(res.right.len(), 192000);
    }

    #[test]
    fn auto_tail_stops_at_the_ceiling() {
        let p = note_project(false, 400.0, 400.0);
        let res = render_offline(&p, Arc::new(AudioPool::new()), opts(4.0, 0.5, 3.0));
        assert_eq!(res.left.len(), 240000);
    }

    #[test]
    fn auto_tail_off_keeps_todays_length() {
        let p = note_project(true, 0.5, 4.0);
        let mut o = opts(4.0, 1.0, 0.0);
        o.keep_audio = false;
        o.analyze = true;
        let res = render_offline(&p, Arc::new(AudioPool::new()), o);
        assert!(res.left.is_empty());
        assert!(res.analysis.is_some());
    }

    #[test]
    fn fixed_tail_is_sample_exact() {
        let mut p = note_project(false, 1.0, 24.0);
        run(&mut p, Command::SetTempo { bpm: 100.0 });
        for (sr, want) in [(44100, 423360), (48000, 460800)] {
            let o = OfflineOptions { sample_rate: sr, start: 4.0, end: 20.0, tail_sec: 0.0, auto_tail_max_sec: 0.0, solo: None, analyze: false, keep_audio: true };
            let res = render_offline(&p, Arc::new(AudioPool::new()), o);
            assert_eq!(res.left.len(), want);
        }
    }

    #[test]
    fn surround_auto_tail_keeps_the_reverb() {
        let p = note_project(true, 0.5, 4.0);
        let (names, chans) = render_surround(&p, Arc::new(AudioPool::new()), 48000, 0.0, 4.0, "5.1", AUTO_TAIL_MAX_SEC);
        assert_eq!(names.len(), 6);
        assert_eq!(chans.len(), 6);
        let len = chans[0].len();
        assert!(chans.iter().all(|c| c.len() == len));
        assert!(len > frames(4.5, 48000) && len < frames(21.0, 48000), "length {len}");
        for c in &chans {
            assert!(tail_peak(c, 10) < 1e-4);
        }
        let (_, fixed) = render_surround(&p, Arc::new(AudioPool::new()), 48000, 0.0, 4.0, "5.1", 0.0);
        assert_eq!(fixed[0].len(), frames(4.0, 48000));
    }
}
