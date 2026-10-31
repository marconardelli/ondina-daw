//! Offline (faster than real-time) rendering: bounce and mix analysis.

use std::collections::HashMap;
use std::sync::Arc;

use aurora_core::model::{Id, Project, MASTER_ID};

use crate::analysis::{AnalysisReport, Analyzer};
use crate::pool::AudioPool;
use crate::renderer::Renderer;

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
    pub tail_sec: f64,
    pub solo: Option<Vec<Id>>,
    pub analyze: bool,
    pub keep_audio: bool,
}

pub fn render_offline(project: &Project, pool: Arc<AudioPool>, opt: OfflineOptions) -> OfflineResult {
    let mut p = project.clone();
    p.cycle.enabled = false;
    p.metronome = false;
    let sr = opt.sample_rate;
    let secs = (p.tempo.beat_to_sec(opt.end) - p.tempo.beat_to_sec(opt.start)).max(0.0) + opt.tail_sec;
    let total = (secs * sr as f64).ceil() as usize;
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
    let mut left = Vec::with_capacity(if opt.keep_audio { total } else { 0 });
    let mut right = Vec::with_capacity(if opt.keep_audio { total } else { 0 });
    let mut bl = vec![0.0; block];
    let mut br = vec![0.0; block];
    let mut done = 0;
    while done < total {
        let n = (total - done).min(block);
        r.render(&mut bl[..n], &mut br[..n]);
        if opt.keep_audio {
            left.extend_from_slice(&bl[..n]);
            right.extend_from_slice(&br[..n]);
        }
        done += n;
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
pub fn render_surround(project: &Project, pool: Arc<AudioPool>, sample_rate: u32, start: f64, end: f64, layout_name: &str) -> (Vec<&'static str>, Vec<Vec<f32>>) {
    let mut p = project.clone();
    p.cycle.enabled = false;
    p.metronome = false;
    if p.surround == aurora_core::model::SurroundFormat::Stereo || p.surround == aurora_core::model::SurroundFormat::Binaural {
        p.surround = aurora_core::model::SurroundFormat::Surround51;
    }
    let layout = aurora_dsp::spatial::layout(layout_name);
    let names: Vec<&'static str> = layout.iter().map(|s| s.0).collect();
    let n = layout.len();
    let secs = (p.tempo.beat_to_sec(end) - p.tempo.beat_to_sec(start)).max(0.0) + 2.0;
    let total = (secs * sample_rate as f64).ceil() as usize;
    let mut r = Renderer::new(sample_rate as f32, Arc::new(p), pool);
    r.surround_out = Some(crate::renderer::SurroundOut { layout: layout.clone(), channels: vec![Vec::with_capacity(total); n] });
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
    let mut chans = r.surround_out.take().unwrap().channels;
    // LFE: low-passed sum of all main channels
    if let Some(lfe) = layout.iter().position(|s| s.2 < -900.0) {
        let mut lp = aurora_dsp::util::Biquad::new(aurora_dsp::util::BiquadKind::LowPass, 120.0, 0.707, 0.0, sample_rate as f32);
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
    p.surround = aurora_core::model::SurroundFormat::Immersive714;
    let layout = aurora_dsp::spatial::layout("7.1.2");
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
        let mut lp = aurora_dsp::util::Biquad::new(aurora_dsp::util::BiquadKind::LowPass, 120.0, 0.707, 0.0, sample_rate as f32);
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
