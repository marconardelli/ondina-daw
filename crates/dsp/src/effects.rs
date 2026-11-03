//! Audio effects.

use std::collections::VecDeque;

use crate::util::*;
use crate::{Effect, ProcessContext};

// ============================================================================ Channel EQ

pub struct ChannelEq {
    sr: f32,
    p: [f32; 15],
    bands_l: [Biquad; 6],
    bands_r: [Biquad; 6],
    dirty: bool,
}

const EQ_IDS: [&str; 15] = ["hp_on", "hp_freq", "low_gain", "low_freq", "mid1_gain", "mid1_freq", "mid1_q", "mid2_gain", "mid2_freq", "mid2_q", "high_gain", "high_freq", "lp_on", "lp_freq", "output"];

/// What one Channel EQ band does: filter shape, frequency, Q, gain and whether it runs.
#[derive(Clone, Copy)]
struct EqBandSpec {
    kind: BiquadKind,
    freq: f32,
    q: f32,
    gain_db: f32,
    active: bool,
}

/// The six bands for parameters `p` (`EQ_IDS` order). The processor and the drawn response read this same table.
fn channel_eq_specs(p: &[f32; 15]) -> [EqBandSpec; 6] {
    let band = |kind, freq, q, gain_db, active| EqBandSpec { kind, freq, q, gain_db, active };
    [
        band(BiquadKind::HighPass, p[1], 0.707, 0.0, p[0] >= 0.5),
        band(BiquadKind::LowShelf, p[3], 0.707, p[2], p[2] != 0.0),
        band(BiquadKind::Peak, p[5], p[6], p[4], p[4] != 0.0),
        band(BiquadKind::Peak, p[8], p[9], p[7], p[7] != 0.0),
        band(BiquadKind::HighShelf, p[11], 0.707, p[10], p[10] != 0.0),
        band(BiquadKind::LowPass, p[13], 0.707, 0.0, p[12] >= 0.5),
    ]
}

/// The Channel EQ parameters in processor order, read with `get` (for example `|id| inst.param(id)`).
pub fn channel_eq_params(get: impl Fn(&str) -> f32) -> [f32; 15] {
    std::array::from_fn(|i| get(EQ_IDS[i]))
}

/// The filter of each band that runs (None for a band that does nothing), in band order: High Pass, Low Shelf,
/// Mid 1, Mid 2, High Shelf, Low Pass.
pub fn channel_eq_filters(p: &[f32; 15], sr: f32) -> [Option<Biquad>; 6] {
    channel_eq_specs(p).map(|s| s.active.then(|| Biquad::new(s.kind, s.freq, s.q, s.gain_db, sr)))
}

/// Gain in dB of the whole Channel EQ at `freq`, output gain included.
pub fn channel_eq_response_db(p: &[f32; 15], freq: f32, sr: f32) -> f32 {
    channel_eq_filters(p, sr).iter().flatten().map(|b| b.magnitude_db(freq, sr)).sum::<f32>() + p[14]
}

impl ChannelEq {
    pub fn new(sr: f32) -> Self {
        ChannelEq { sr, p: [0.0, 80.0, 0.0, 120.0, 0.0, 400.0, 1.0, 0.0, 2500.0, 1.0, 0.0, 8000.0, 0.0, 16000.0, 0.0], bands_l: [Biquad::default(); 6], bands_r: [Biquad::default(); 6], dirty: true }
    }
    fn update(&mut self) {
        let sr = self.sr;
        for (i, s) in channel_eq_specs(&self.p).iter().enumerate() {
            self.bands_l[i].set(s.kind, s.freq, s.q, s.gain_db, sr);
            self.bands_r[i].set(s.kind, s.freq, s.q, s.gain_db, sr);
        }
        self.dirty = false;
    }
}

impl Effect for ChannelEq {
    fn set_param(&mut self, id: &str, v: f32) {
        if let Some(i) = EQ_IDS.iter().position(|x| *x == id) {
            if self.p[i] != v {
                self.p[i] = v;
                self.dirty = true;
            }
        }
    }
    fn process(&mut self, l: &mut [f32], r: &mut [f32], _ctx: &ProcessContext) {
        if self.dirty {
            self.update();
        }
        let active = channel_eq_specs(&self.p).map(|s| s.active);
        let out = db_to_gain(self.p[14]);
        for i in 0..l.len() {
            let (mut a, mut b) = (l[i], r[i]);
            for k in 0..6 {
                if active[k] {
                    a = self.bands_l[k].process(a);
                    b = self.bands_r[k].process(b);
                }
            }
            l[i] = a * out;
            r[i] = b * out;
        }
    }
    fn reset(&mut self) {
        for b in self.bands_l.iter_mut().chain(self.bands_r.iter_mut()) {
            b.reset();
        }
    }
}

// ============================================================================ Compressor

pub struct Compressor {
    sr: f32,
    threshold: f32,
    ratio: f32,
    attack: f32,
    release: f32,
    knee: f32,
    makeup: f32,
    mix: f32,
    env_db: f32,
    pub gain_reduction_db: f32,
}

impl Compressor {
    pub fn new(sr: f32) -> Self {
        Compressor { sr, threshold: -18.0, ratio: 4.0, attack: 10.0, release: 150.0, knee: 6.0, makeup: 0.0, mix: 1.0, env_db: -120.0, gain_reduction_db: 0.0 }
    }
    fn curve(&self, x_db: f32) -> f32 {
        compressor_curve_db(self.threshold, self.ratio, self.knee, x_db)
    }
}

/// What the compressor's gain computer makes of a level `x_db` (soft knee of `knee` dB around `threshold`). The
/// processor and the drawn curve share it.
pub fn compressor_curve_db(threshold: f32, ratio: f32, knee: f32, x_db: f32) -> f32 {
    let (t, r, k) = (threshold, ratio, knee);
    let over = x_db - t;
    if k > 0.0 && over.abs() <= k / 2.0 {
        x_db + (1.0 / r - 1.0) * (over + k / 2.0).powi(2) / (2.0 * k)
    } else if over > 0.0 {
        t + over / r
    } else {
        x_db
    }
}

impl Effect for Compressor {
    fn set_param(&mut self, id: &str, v: f32) {
        match id {
            "threshold" => self.threshold = v,
            "ratio" => self.ratio = v.max(1.0),
            "attack" => self.attack = v,
            "release" => self.release = v,
            "knee" => self.knee = v,
            "makeup" => self.makeup = v,
            "mix" => self.mix = v,
            _ => {}
        }
    }
    fn process(&mut self, l: &mut [f32], r: &mut [f32], _ctx: &ProcessContext) {
        let att = (-1.0 / (self.attack * 0.001 * self.sr)).exp();
        let rel = (-1.0 / (self.release * 0.001 * self.sr)).exp();
        let mk = db_to_gain(self.makeup);
        let mut max_gr: f32 = 0.0;
        for i in 0..l.len() {
            let x = l[i].abs().max(r[i].abs());
            let x_db = gain_to_db(x);
            let coef = if x_db > self.env_db { att } else { rel };
            self.env_db = x_db + coef * (self.env_db - x_db);
            let gr = self.curve(self.env_db) - self.env_db;
            max_gr = max_gr.min(gr);
            let g = db_to_gain(gr) * mk;
            l[i] = l[i] * (1.0 - self.mix) + l[i] * g * self.mix;
            r[i] = r[i] * (1.0 - self.mix) + r[i] * g * self.mix;
        }
        self.gain_reduction_db = max_gr;
    }
    fn reset(&mut self) {
        self.env_db = -120.0;
        self.gain_reduction_db = 0.0;
    }
    fn gain_reduction_db(&self) -> Option<f32> {
        Some(self.gain_reduction_db)
    }
}

// ============================================================================ Limiter

/// Samples on each side of a peak that the true-peak interpolator reads: the gain stays down over all of them.
const TP_SPREAD: usize = TruePeak::DELAY;

pub struct Limiter {
    sr: f32,
    gain: f32,
    ceiling: f32,
    release: f32,
    /// True Peak On: the gain follows the peaks between samples too.
    true_peak: bool,
    look: usize,
    /// Look-ahead delay lines, `look` samples each (ring buffers allocated in `new`), shared by both modes so the
    /// latency is the same.
    dl: Vec<f32>,
    dr: Vec<f32>,
    pos: usize,
    /// Sample-peak mode: monotonic queue of the targets in the window, capacity fixed in `new`.
    mins: VecDeque<(usize, f32)>,
    counter: usize,
    current: f32,
    // True-peak mode.
    tp_l: TruePeak,
    tp_r: TruePeak,
    /// Length of the moving average that smooths the gain.
    tp_win: usize,
    /// Length of the minimum hold: tp_win plus TP_SPREAD on each side of a peak.
    tp_hold: usize,
    tp_mins: VecDeque<(usize, f32)>,
    tp_box: Vec<f32>,
    tp_box_pos: usize,
    tp_sum: f64,
    /// Held minimum after the release.
    tp_rel: f32,
    /// Last gain applied, handed to the sample-peak mode when True Peak is switched off.
    tp_gain: f32,
    /// Deepest reduction of the last block in dB (<= 0).
    gain_reduction_db: f32,
}

impl Limiter {
    pub fn new(sr: f32) -> Self {
        let look = (sr * 0.0015) as usize;
        // The true-peak detector runs DELAY samples late and the gain must cover TP_SPREAD samples after the peak, so
        // the average gets what is left of the look-ahead. Below about 17 kHz that is 1 sample and the margin between
        // samples shrinks.
        let tp_win = (look + 1).saturating_sub(TruePeak::DELAY + TP_SPREAD).max(1);
        let tp_hold = tp_win + 2 * TP_SPREAD;
        Limiter {
            sr,
            gain: 1.0,
            ceiling: db_to_gain(-1.0),
            release: 60.0,
            true_peak: false,
            look,
            dl: vec![0.0; look],
            dr: vec![0.0; look],
            pos: 0,
            // the queue holds at most look + 2 entries between a push and the pops that follow it
            mins: VecDeque::with_capacity(look + 2),
            counter: 0,
            current: 1.0,
            tp_l: TruePeak::new(),
            tp_r: TruePeak::new(),
            tp_win,
            tp_hold,
            tp_mins: VecDeque::with_capacity(tp_hold + 2),
            tp_box: vec![1.0; tp_win],
            tp_box_pos: 0,
            tp_sum: tp_win as f64,
            tp_rel: 1.0,
            tp_gain: 1.0,
            gain_reduction_db: 0.0,
        }
    }

    /// One step of the look-ahead delay: returns the samples that went in `look` samples ago.
    #[inline]
    fn delay(&mut self, a: f32, b: f32) -> (f32, f32) {
        if self.dl.is_empty() {
            return (a, b);
        }
        let out = (self.dl[self.pos], self.dr[self.pos]);
        self.dl[self.pos] = a;
        self.dr[self.pos] = b;
        self.pos = (self.pos + 1) % self.dl.len();
        out
    }

    /// Starts the true-peak mode from the gain `g` the limiter is applying now, so switching does not jump.
    fn start_true_peak(&mut self, g: f32) {
        self.tp_l.reset();
        self.tp_r.reset();
        self.tp_mins.clear();
        self.tp_box.fill(g);
        self.tp_box_pos = 0;
        self.tp_sum = f64::from(g) * self.tp_win as f64;
        self.tp_rel = g;
        self.tp_gain = g;
    }

    /// Sample-peak mode: the limiter as it has always been, operation for operation.
    fn process_sample_peak(&mut self, l: &mut [f32], r: &mut [f32]) -> f32 {
        let rel = 1.0 - (-1.0 / (self.release * 0.001 * self.sr)).exp();
        let att = 1.0 - (-1.0 / (self.look.max(1) as f32 / 3.0)).exp();
        let mut min_gain = 1.0f32;
        for i in 0..l.len() {
            let a = l[i] * self.gain;
            let b = r[i] * self.gain;
            let peak = a.abs().max(b.abs());
            let target = if peak > self.ceiling { self.ceiling / peak } else { 1.0 };
            // monotonic queue: minimum target over the look-ahead window
            self.counter += 1;
            while let Some(&(_, v)) = self.mins.back() {
                if v >= target {
                    self.mins.pop_back();
                } else {
                    break;
                }
            }
            self.mins.push_back((self.counter, target));
            while let Some(&(c, _)) = self.mins.front() {
                if c + self.look < self.counter {
                    self.mins.pop_front();
                } else {
                    break;
                }
            }
            let win_min = self.mins.front().map(|x| x.1).unwrap_or(1.0);
            if win_min < self.current {
                self.current += (win_min - self.current) * att;
            } else {
                self.current += (win_min - self.current) * rel;
            }
            min_gain = min_gain.min(self.current);
            let (da, db) = self.delay(a, b);
            let c = self.ceiling;
            l[i] = (da * self.current).clamp(-c, c);
            r[i] = (db * self.current).clamp(-c, c);
        }
        min_gain
    }

    /// True-peak mode: the gain follows the 4x interpolated peaks, so the output stays under the ceiling between
    /// samples too.
    fn process_true_peak(&mut self, l: &mut [f32], r: &mut [f32]) -> f32 {
        let rel = 1.0 - (-1.0 / (self.release * 0.001 * self.sr)).exp();
        let c = self.ceiling;
        let w = self.tp_win;
        let mut min_gain = 1.0f32;
        for i in 0..l.len() {
            let a = l[i] * self.gain;
            let b = r[i] * self.gain;
            // peak of the segment that started TruePeak::DELAY samples ago, between samples included
            let peak = self.tp_l.push(a).max(self.tp_r.push(b));
            let target = if peak > c { c / peak } else { 1.0 };
            self.counter += 1;
            while let Some(&(_, v)) = self.tp_mins.back() {
                if v >= target {
                    self.tp_mins.pop_back();
                } else {
                    break;
                }
            }
            self.tp_mins.push_back((self.counter, target));
            while let Some(&(n, _)) = self.tp_mins.front() {
                if n + self.tp_hold <= self.counter {
                    self.tp_mins.pop_front();
                } else {
                    break;
                }
            }
            let held = self.tp_mins.front().map(|x| x.1).unwrap_or(1.0);
            // falls at once, rises with the release: never above the held minimum
            self.tp_rel = if held < self.tp_rel { held } else { self.tp_rel + (held - self.tp_rel) * rel };
            // moving average over w samples: it reaches the held minimum when the peak leaves the delay line
            self.tp_sum += f64::from(self.tp_rel) - f64::from(self.tp_box[self.tp_box_pos]);
            self.tp_box[self.tp_box_pos] = self.tp_rel;
            self.tp_box_pos = (self.tp_box_pos + 1) % w;
            if self.tp_box_pos == 0 {
                // a fresh sum once per window keeps rounding from piling up over a long session
                self.tp_sum = self.tp_box.iter().map(|&x| f64::from(x)).sum();
            }
            let g = ((self.tp_sum / w as f64) as f32).min(1.0);
            self.tp_gain = g;
            let (da, db) = self.delay(a, b);
            l[i] = (da * g).clamp(-c, c);
            r[i] = (db * g).clamp(-c, c);
            min_gain = min_gain.min(g);
        }
        min_gain
    }
}

impl Effect for Limiter {
    fn set_param(&mut self, id: &str, v: f32) {
        match id {
            "gain" => self.gain = db_to_gain(v),
            "ceiling" => self.ceiling = db_to_gain(v),
            "release" => self.release = v,
            "true_peak" => {
                // the engine sets every parameter on each project change and automated ones every block: act only on
                // a real switch
                let on = v >= 0.5;
                if on != self.true_peak {
                    self.true_peak = on;
                    if on {
                        self.start_true_peak(self.current);
                    } else {
                        self.current = self.tp_gain;
                        self.mins.clear();
                    }
                }
            }
            _ => {}
        }
    }
    fn process(&mut self, l: &mut [f32], r: &mut [f32], _ctx: &ProcessContext) {
        let min_gain = if self.true_peak { self.process_true_peak(l, r) } else { self.process_sample_peak(l, r) };
        self.gain_reduction_db = gain_to_db(min_gain).min(0.0);
    }
    fn reset(&mut self) {
        self.dl.fill(0.0);
        self.dr.fill(0.0);
        self.mins.clear();
        self.current = 1.0;
        self.start_true_peak(1.0);
        self.gain_reduction_db = 0.0;
    }
    fn gain_reduction_db(&self) -> Option<f32> {
        Some(self.gain_reduction_db)
    }
}

// ============================================================================ Noise Gate

pub struct NoiseGate {
    sr: f32,
    threshold: f32,
    reduction: f32,
    attack: f32,
    hold: f32,
    release: f32,
    env: f32,
    gain: f32,
    hold_count: usize,
    /// Deepest reduction of the last block in dB, between `reduction` and 0.
    gain_reduction_db: f32,
}

impl NoiseGate {
    pub fn new(sr: f32) -> Self {
        NoiseGate { sr, threshold: -50.0, reduction: -60.0, attack: 1.0, hold: 20.0, release: 100.0, env: 0.0, gain: 0.0, hold_count: 0, gain_reduction_db: 0.0 }
    }
}

impl Effect for NoiseGate {
    fn set_param(&mut self, id: &str, v: f32) {
        match id {
            "threshold" => self.threshold = v,
            "reduction" => self.reduction = v,
            "attack" => self.attack = v,
            "hold" => self.hold = v,
            "release" => self.release = v,
            _ => {}
        }
    }
    fn process(&mut self, l: &mut [f32], r: &mut [f32], _ctx: &ProcessContext) {
        let thr = db_to_gain(self.threshold);
        let floor = db_to_gain(self.reduction);
        let att = 1.0 - (-1.0 / (self.attack * 0.001 * self.sr)).exp();
        let rel = 1.0 - (-1.0 / (self.release * 0.001 * self.sr)).exp();
        let det = 1.0 - (-1.0 / (0.002 * self.sr)).exp();
        let hold = (self.hold * 0.001 * self.sr) as usize;
        let mut min_gain = 1.0f32;
        for i in 0..l.len() {
            let x = l[i].abs().max(r[i].abs());
            self.env += (x - self.env) * det;
            let open = if self.env > thr {
                self.hold_count = hold;
                true
            } else if self.hold_count > 0 {
                self.hold_count -= 1;
                true
            } else {
                false
            };
            let target = if open { 1.0 } else { floor };
            self.gain += (target - self.gain) * if open { att } else { rel };
            min_gain = min_gain.min(self.gain);
            l[i] *= self.gain;
            r[i] *= self.gain;
        }
        // A new gate starts fully closed (gain 0): never report more than its own floor.
        self.gain_reduction_db = gain_to_db(min_gain).clamp(self.reduction.min(0.0), 0.0);
    }
    fn reset(&mut self) {
        self.env = 0.0;
        self.gain_reduction_db = 0.0;
    }
    fn gain_reduction_db(&self) -> Option<f32> {
        Some(self.gain_reduction_db)
    }
}

// ============================================================================ Static curves of the dynamics

/// The static input/output curve of a dynamics effect: the level it settles at for a steady input. Attack,
/// release, hold and look-ahead are left out; the De-Esser's levels are those of the band above its frequency.
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum DynamicsCurve {
    Compressor { threshold: f32, ratio: f32, knee: f32, makeup: f32, mix: f32 },
    Limiter { gain: f32, ceiling: f32 },
    NoiseGate { threshold: f32, reduction: f32 },
    DeEsser { threshold: f32, range: f32 },
}

impl DynamicsCurve {
    /// The curve of plug-in `kind` with its parameters read by `get` (for example `|id| inst.param(id)`), clamped as
    /// the processor's `set_param` clamps them. None for a kind without one.
    pub fn of(kind: &str, get: impl Fn(&str) -> f32) -> Option<DynamicsCurve> {
        Some(match kind {
            "compressor" => DynamicsCurve::Compressor { threshold: get("threshold"), ratio: get("ratio").max(1.0), knee: get("knee"), makeup: get("makeup"), mix: get("mix") },
            "limiter" => DynamicsCurve::Limiter { gain: get("gain"), ceiling: get("ceiling") },
            "noise_gate" => DynamicsCurve::NoiseGate { threshold: get("threshold"), reduction: get("reduction") },
            "de_esser" => DynamicsCurve::DeEsser { threshold: get("threshold").clamp(-60.0, 0.0), range: get("range").clamp(0.0, 24.0) },
            _ => return None,
        })
    }

    /// Output level (dB) for a steady input at `in_db`.
    pub fn out_db(&self, in_db: f32) -> f32 {
        let x = in_db;
        match *self {
            DynamicsCurve::Compressor { threshold, ratio, knee, makeup, mix } => {
                let gr = compressor_curve_db(threshold, ratio, knee, x) - x;
                let lin = (1.0 - mix) + mix * db_to_gain(gr) * db_to_gain(makeup);
                x + gain_to_db(lin)
            }
            DynamicsCurve::Limiter { gain, ceiling } => (x + gain).min(ceiling),
            // Under -96 dB of reduction the processor reaches silence; the graph shows that at the bottom of its axis
            // all the same.
            DynamicsCurve::NoiseGate { threshold, reduction } => {
                if x > threshold {
                    x
                } else {
                    x + reduction
                }
            }
            DynamicsCurve::DeEsser { threshold, range } => x - (x - threshold).clamp(0.0, range),
        }
    }

    /// The input level where the curve bends: the threshold (Limiter: ceiling - gain).
    pub fn threshold_db(&self) -> f32 {
        match *self {
            DynamicsCurve::Compressor { threshold, .. } | DynamicsCurve::NoiseGate { threshold, .. } | DynamicsCurve::DeEsser { threshold, .. } => threshold,
            DynamicsCurve::Limiter { gain, ceiling } => ceiling - gain,
        }
    }
}

/// `DynamicsCurve::of(kind, get)` at `in_db`; None for a kind without a curve.
pub fn transfer_db(kind: &str, get: impl Fn(&str) -> f32, in_db: f32) -> Option<f32> {
    DynamicsCurve::of(kind, get).map(|c| c.out_db(in_db))
}

/// Gain (dB) at `f` of what the De-Esser does to the signal when it turns the band down by `reduction_db` (>= 0):
/// its high shelf in Split mode, the whole signal in Wideband.
pub fn de_esser_band_db(freq: f32, reduction_db: f32, wideband: bool, f: f32, sr: f32) -> f32 {
    if reduction_db <= 0.0 {
        0.0
    } else if wideband {
        -reduction_db
    } else {
        Biquad::new(BiquadKind::HighShelf, freq * DE_ESS_SHELF_RATIO, DE_ESS_SHELF_Q, -reduction_db, sr).magnitude_db(f, sr)
    }
}

// ============================================================================ Stereo Delay

pub struct StereoDelay {
    sr: f32,
    sync: bool,
    time_l: f32,
    time_r: f32,
    time_ms: f32,
    feedback: f32,
    cross: f32,
    mix: f32,
    low_cut: f32,
    high_cut: f32,
    dl: DelayLine,
    dr: DelayLine,
    lp_l: OnePole,
    lp_r: OnePole,
    hp_l: OnePole,
    hp_r: OnePole,
    cur_l: f32,
    cur_r: f32,
}

impl StereoDelay {
    pub fn new(sr: f32) -> Self {
        let max = (sr * 8.0) as usize;
        StereoDelay { sr, sync: true, time_l: 0.75, time_r: 0.5, time_ms: 350.0, feedback: 0.35, cross: 0.3, mix: 0.25, low_cut: 150.0, high_cut: 7000.0, dl: DelayLine::new(max), dr: DelayLine::new(max), lp_l: OnePole::default(), lp_r: OnePole::default(), hp_l: OnePole::default(), hp_r: OnePole::default(), cur_l: 0.0, cur_r: 0.0 }
    }
}

impl Effect for StereoDelay {
    fn set_param(&mut self, id: &str, v: f32) {
        match id {
            "sync" => self.sync = v >= 0.5,
            "time_l" => self.time_l = v,
            "time_r" => self.time_r = v,
            "time_ms" => self.time_ms = v,
            "feedback" => self.feedback = v,
            "crossfeed" => self.cross = v,
            "low_cut" => self.low_cut = v,
            "high_cut" => self.high_cut = v,
            "mix" => self.mix = v,
            _ => {}
        }
    }
    fn process(&mut self, l: &mut [f32], r: &mut [f32], ctx: &ProcessContext) {
        let spb = 60.0 / ctx.bpm.max(1.0) * self.sr;
        let (tl, tr) = if self.sync { (self.time_l * spb, self.time_r * spb) } else { (self.time_ms * 0.001 * self.sr, self.time_ms * 0.001 * self.sr * 0.75) };
        let maxd = (self.dl.len() - 4) as f32;
        let (tl, tr) = (tl.min(maxd), tr.min(maxd));
        if self.cur_l == 0.0 {
            self.cur_l = tl;
            self.cur_r = tr;
        }
        self.lp_l.set_freq(self.high_cut, self.sr);
        self.lp_r.set_freq(self.high_cut, self.sr);
        self.hp_l.set_freq(self.low_cut, self.sr);
        self.hp_r.set_freq(self.low_cut, self.sr);
        for i in 0..l.len() {
            self.cur_l += (tl - self.cur_l) * 0.0005;
            self.cur_r += (tr - self.cur_r) * 0.0005;
            let wl = self.dl.read(self.cur_l);
            let wr = self.dr.read(self.cur_r);
            let mut fl = self.lp_l.process(wl);
            fl -= self.hp_l.process(fl);
            let mut fr = self.lp_r.process(wr);
            fr -= self.hp_r.process(fr);
            let in_l = l[i] + (fl * (1.0 - self.cross) + fr * self.cross) * self.feedback;
            let in_r = r[i] + (fr * (1.0 - self.cross) + fl * self.cross) * self.feedback;
            self.dl.write(in_l.clamp(-4.0, 4.0));
            self.dr.write(in_r.clamp(-4.0, 4.0));
            l[i] = l[i] * (1.0 - self.mix) + wl * self.mix;
            r[i] = r[i] * (1.0 - self.mix) + wr * self.mix;
        }
    }
    fn reset(&mut self) {
        self.dl.clear();
        self.dr.clear();
    }
}

// ============================================================================ Tape Delay

pub struct TapeDelay {
    sr: f32,
    time: f32,
    feedback: f32,
    wow: f32,
    tone: f32,
    sat: f32,
    mix: f32,
    line: DelayLine,
    lp: OnePole,
    phase_w: f32,
    phase_f: f32,
    cur: f32,
}

impl TapeDelay {
    pub fn new(sr: f32) -> Self {
        TapeDelay { sr, time: 0.5, feedback: 0.4, wow: 0.3, tone: 0.4, sat: 0.3, mix: 0.25, line: DelayLine::new((sr * 8.0) as usize), lp: OnePole::default(), phase_w: 0.0, phase_f: 0.0, cur: 0.0 }
    }
}

impl Effect for TapeDelay {
    fn set_param(&mut self, id: &str, v: f32) {
        match id {
            "time" => self.time = v,
            "feedback" => self.feedback = v,
            "wow" => self.wow = v,
            "tone" => self.tone = v,
            "saturation" => self.sat = v,
            "mix" => self.mix = v,
            _ => {}
        }
    }
    fn process(&mut self, l: &mut [f32], r: &mut [f32], ctx: &ProcessContext) {
        let target = (self.time * 60.0 / ctx.bpm.max(1.0) * self.sr).min((self.line.len() - 200) as f32);
        if self.cur == 0.0 {
            self.cur = target;
        }
        self.lp.set_freq(800.0 + self.tone * 9000.0, self.sr);
        let drive = 1.0 + self.sat * 4.0;
        for i in 0..l.len() {
            self.cur += (target - self.cur) * 0.0002;
            self.phase_w = (self.phase_w + 0.6 / self.sr).fract();
            self.phase_f = (self.phase_f + 7.0 / self.sr).fract();
            let m = (self.phase_w * std::f32::consts::TAU).sin() * 0.003 + (self.phase_f * std::f32::consts::TAU).sin() * 0.0004;
            let d = self.cur * (1.0 + m * self.wow * 2.0);
            let w = self.line.read(d);
            let fb = (self.lp.process(w) * drive).tanh() / drive.tanh();
            let input = (l[i] + r[i]) * 0.5;
            self.line.write(input + fb * self.feedback.min(1.05));
            l[i] = l[i] * (1.0 - self.mix) + w * self.mix;
            r[i] = r[i] * (1.0 - self.mix) + w * self.mix;
        }
    }
    fn reset(&mut self) {
        self.line.clear();
    }
}

// ============================================================================ Reverb (Freeverb-style)

struct Comb {
    buf: Vec<f32>,
    pos: usize,
    store: f32,
}

impl Comb {
    fn new(n: usize) -> Self {
        Comb { buf: vec![0.0; n.max(1)], pos: 0, store: 0.0 }
    }
    #[inline]
    fn process(&mut self, x: f32, feedback: f32, damp: f32) -> f32 {
        let out = self.buf[self.pos];
        self.store = out * (1.0 - damp) + self.store * damp;
        self.buf[self.pos] = x + self.store * feedback;
        self.pos += 1;
        if self.pos >= self.buf.len() {
            self.pos = 0;
        }
        out
    }
}

struct Allpass {
    buf: Vec<f32>,
    pos: usize,
}

impl Allpass {
    fn new(n: usize) -> Self {
        Allpass { buf: vec![0.0; n.max(1)], pos: 0 }
    }
    #[inline]
    fn process(&mut self, x: f32) -> f32 {
        let b = self.buf[self.pos];
        let out = -x + b;
        self.buf[self.pos] = x + b * 0.5;
        self.pos += 1;
        if self.pos >= self.buf.len() {
            self.pos = 0;
        }
        out
    }
}

pub struct Reverb {
    sr: f32,
    size: f32,
    damping: f32,
    predelay: f32,
    width: f32,
    low_cut: f32,
    mix: f32,
    combs_l: Vec<Comb>,
    combs_r: Vec<Comb>,
    ap_l: Vec<Allpass>,
    ap_r: Vec<Allpass>,
    pre: DelayLine,
    hp: Biquad,
}

const COMBS: [usize; 8] = [1116, 1188, 1277, 1356, 1422, 1491, 1557, 1617];
const APS: [usize; 4] = [556, 441, 341, 225];
const SPREAD: usize = 23;

impl Reverb {
    pub fn new(sr: f32) -> Self {
        let s = sr / 44100.0;
        let sc = |n: usize| (n as f32 * s) as usize;
        Reverb {
            sr,
            size: 0.6,
            damping: 0.4,
            predelay: 12.0,
            width: 1.0,
            low_cut: 120.0,
            mix: 0.25,
            combs_l: COMBS.iter().map(|n| Comb::new(sc(*n))).collect(),
            combs_r: COMBS.iter().map(|n| Comb::new(sc(n + SPREAD))).collect(),
            ap_l: APS.iter().map(|n| Allpass::new(sc(*n))).collect(),
            ap_r: APS.iter().map(|n| Allpass::new(sc(n + SPREAD))).collect(),
            pre: DelayLine::new((sr * 0.25) as usize),
            hp: Biquad::new(BiquadKind::HighPass, 120.0, 0.707, 0.0, sr),
        }
    }
}

impl Effect for Reverb {
    fn set_param(&mut self, id: &str, v: f32) {
        match id {
            "size" => self.size = v,
            "damping" => self.damping = v,
            "predelay" => self.predelay = v,
            "width" => self.width = v,
            "low_cut" => {
                self.low_cut = v;
                self.hp.set(BiquadKind::HighPass, v, 0.707, 0.0, self.sr);
            }
            "mix" => self.mix = v,
            _ => {}
        }
    }
    fn process(&mut self, l: &mut [f32], r: &mut [f32], _ctx: &ProcessContext) {
        let fb = 0.7 + self.size * 0.28;
        let damp = self.damping * 0.4;
        let pre = (self.predelay * 0.001 * self.sr).max(1.0);
        let wet1 = self.width * 0.5 + 0.5;
        let wet2 = (1.0 - self.width) * 0.5;
        for i in 0..l.len() {
            let input = (l[i] + r[i]) * 0.5;
            self.pre.write(input);
            let x = self.hp.process(self.pre.read(pre)) * 0.03;
            let mut ol = 0.0;
            let mut or = 0.0;
            for c in &mut self.combs_l {
                ol += c.process(x, fb, damp);
            }
            for c in &mut self.combs_r {
                or += c.process(x, fb, damp);
            }
            for a in &mut self.ap_l {
                ol = a.process(ol);
            }
            for a in &mut self.ap_r {
                or = a.process(or);
            }
            let wl = ol * wet1 + or * wet2;
            let wr = or * wet1 + ol * wet2;
            l[i] = l[i] * (1.0 - self.mix) + wl * self.mix;
            r[i] = r[i] * (1.0 - self.mix) + wr * self.mix;
        }
    }
    fn reset(&mut self) {
        for c in self.combs_l.iter_mut().chain(self.combs_r.iter_mut()) {
            c.buf.iter_mut().for_each(|x| *x = 0.0);
            c.store = 0.0;
        }
        for a in self.ap_l.iter_mut().chain(self.ap_r.iter_mut()) {
            a.buf.iter_mut().for_each(|x| *x = 0.0);
        }
        self.pre.clear();
    }
}

// ============================================================================ Chorus / Flanger

pub struct Chorus {
    sr: f32,
    rate: f32,
    depth: f32,
    delay: f32,
    mix: f32,
    dl: DelayLine,
    dr: DelayLine,
    phase: f32,
}

impl Chorus {
    pub fn new(sr: f32) -> Self {
        Chorus { sr, rate: 0.8, depth: 0.5, delay: 12.0, mix: 0.5, dl: DelayLine::new((sr * 0.06) as usize), dr: DelayLine::new((sr * 0.06) as usize), phase: 0.0 }
    }
}

impl Effect for Chorus {
    fn set_param(&mut self, id: &str, v: f32) {
        match id {
            "rate" => self.rate = v,
            "depth" => self.depth = v,
            "delay" => self.delay = v,
            "mix" => self.mix = v,
            _ => {}
        }
    }
    fn process(&mut self, l: &mut [f32], r: &mut [f32], _ctx: &ProcessContext) {
        let base = self.delay * 0.001 * self.sr;
        let dep = self.depth * 0.004 * self.sr;
        for i in 0..l.len() {
            self.phase = (self.phase + self.rate / self.sr).fract();
            let ml = (self.phase * std::f32::consts::TAU).sin();
            let mr = ((self.phase + 0.25) * std::f32::consts::TAU).sin();
            self.dl.write(l[i]);
            self.dr.write(r[i]);
            let wl = self.dl.read(base + ml * dep);
            let wr = self.dr.read(base + mr * dep);
            l[i] = l[i] * (1.0 - self.mix * 0.5) + wl * self.mix;
            r[i] = r[i] * (1.0 - self.mix * 0.5) + wr * self.mix;
        }
    }
    fn reset(&mut self) {
        self.dl.clear();
        self.dr.clear();
    }
}

pub struct Flanger {
    sr: f32,
    rate: f32,
    depth: f32,
    delay: f32,
    feedback: f32,
    mix: f32,
    dl: DelayLine,
    dr: DelayLine,
    phase: f32,
    fl: f32,
    fr: f32,
}

impl Flanger {
    pub fn new(sr: f32) -> Self {
        Flanger { sr, rate: 0.25, depth: 0.7, delay: 2.0, feedback: 0.5, mix: 0.5, dl: DelayLine::new((sr * 0.03) as usize), dr: DelayLine::new((sr * 0.03) as usize), phase: 0.0, fl: 0.0, fr: 0.0 }
    }
}

impl Effect for Flanger {
    fn set_param(&mut self, id: &str, v: f32) {
        match id {
            "rate" => self.rate = v,
            "depth" => self.depth = v,
            "delay" => self.delay = v,
            "feedback" => self.feedback = v,
            "mix" => self.mix = v,
            _ => {}
        }
    }
    fn process(&mut self, l: &mut [f32], r: &mut [f32], _ctx: &ProcessContext) {
        let base = self.delay * 0.001 * self.sr;
        for i in 0..l.len() {
            self.phase = (self.phase + self.rate / self.sr).fract();
            let ml = 0.5 + 0.5 * (self.phase * std::f32::consts::TAU).sin();
            let mr = 0.5 + 0.5 * ((self.phase + 0.25) * std::f32::consts::TAU).sin();
            self.dl.write(l[i] + self.fl * self.feedback);
            self.dr.write(r[i] + self.fr * self.feedback);
            self.fl = self.dl.read(1.0 + base * (1.0 + ml * self.depth * 2.0));
            self.fr = self.dr.read(1.0 + base * (1.0 + mr * self.depth * 2.0));
            l[i] = l[i] * (1.0 - self.mix * 0.5) + self.fl * self.mix * 0.7;
            r[i] = r[i] * (1.0 - self.mix * 0.5) + self.fr * self.mix * 0.7;
        }
    }
    fn reset(&mut self) {
        self.dl.clear();
        self.dr.clear();
        self.fl = 0.0;
        self.fr = 0.0;
    }
}

// ============================================================================ Phaser

pub struct Phaser {
    sr: f32,
    rate: f32,
    depth: f32,
    stages: usize,
    feedback: f32,
    mix: f32,
    zl: [f32; 12],
    zr: [f32; 12],
    phase: f32,
    fbl: f32,
    fbr: f32,
}

impl Phaser {
    pub fn new(sr: f32) -> Self {
        Phaser { sr, rate: 0.4, depth: 0.7, stages: 4, feedback: 0.4, mix: 0.5, zl: [0.0; 12], zr: [0.0; 12], phase: 0.0, fbl: 0.0, fbr: 0.0 }
    }
}

impl Effect for Phaser {
    fn set_param(&mut self, id: &str, v: f32) {
        match id {
            "rate" => self.rate = v,
            "depth" => self.depth = v,
            "stages" => self.stages = [2, 4, 6, 8, 12][(v.round() as usize).min(4)],
            "feedback" => self.feedback = v,
            "mix" => self.mix = v,
            _ => {}
        }
    }
    fn process(&mut self, l: &mut [f32], r: &mut [f32], _ctx: &ProcessContext) {
        for i in 0..l.len() {
            self.phase = (self.phase + self.rate / self.sr).fract();
            let coef = |ph: f32, depth: f32, sr: f32| {
                let m = 0.5 + 0.5 * (ph * std::f32::consts::TAU).sin();
                let f = 200.0 * 2f32.powf(m * depth * 4.5);
                let t = (std::f32::consts::PI * f / sr).tan();
                (t - 1.0) / (t + 1.0)
            };
            let al = coef(self.phase, self.depth, self.sr);
            let ar = coef((self.phase + 0.25).fract(), self.depth, self.sr);
            let mut xl = l[i] + self.fbl * self.feedback;
            let mut xr = r[i] + self.fbr * self.feedback;
            for s in 0..self.stages {
                let yl = al * xl + self.zl[s];
                self.zl[s] = xl - al * yl;
                xl = yl;
                let yr = ar * xr + self.zr[s];
                self.zr[s] = xr - ar * yr;
                xr = yr;
            }
            self.fbl = xl.clamp(-2.0, 2.0);
            self.fbr = xr.clamp(-2.0, 2.0);
            l[i] = l[i] * (1.0 - self.mix * 0.5) + xl * self.mix * 0.5;
            r[i] = r[i] * (1.0 - self.mix * 0.5) + xr * self.mix * 0.5;
        }
    }
    fn reset(&mut self) {
        self.zl = [0.0; 12];
        self.zr = [0.0; 12];
        self.fbl = 0.0;
        self.fbr = 0.0;
    }
}

// ============================================================================ Tremolo

pub struct Tremolo {
    sr: f32,
    rate: f32,
    depth: f32,
    shape: u8,
    stereo: f32,
    free_phase: f64,
}

impl Tremolo {
    pub fn new(sr: f32) -> Self {
        Tremolo { sr, rate: 0.25, depth: 0.5, shape: 0, stereo: 0.0, free_phase: 0.0 }
    }
    fn wave(&self, ph: f64) -> f32 {
        let p = ph.fract() as f32;
        match self.shape {
            1 => if p < 0.5 { 1.0 } else { 0.0 },
            2 => 1.0 - (2.0 * p - 1.0).abs(),
            _ => 0.5 + 0.5 * (p * std::f32::consts::TAU).cos(),
        }
    }
}

impl Effect for Tremolo {
    fn set_param(&mut self, id: &str, v: f32) {
        match id {
            "rate" => self.rate = v.max(0.01),
            "depth" => self.depth = v,
            "shape" => self.shape = v.round() as u8,
            "stereo_phase" => self.stereo = v,
            _ => {}
        }
    }
    fn process(&mut self, l: &mut [f32], r: &mut [f32], ctx: &ProcessContext) {
        let _ = self.sr;
        for i in 0..l.len() {
            let beat = if ctx.playing { ctx.beat + i as f64 * ctx.beats_per_sample } else {
                self.free_phase += ctx.beats_per_sample;
                self.free_phase
            };
            let ph = beat / self.rate as f64;
            let gl = 1.0 - self.depth * (1.0 - self.wave(ph));
            let gr = 1.0 - self.depth * (1.0 - self.wave(ph + self.stereo as f64 * 0.5));
            l[i] *= gl;
            r[i] *= gr;
        }
    }
    fn reset(&mut self) {}
}

// ============================================================================ Overdrive

pub struct Overdrive {
    sr: f32,
    mode: u8,
    drive: f32,
    tone: f32,
    output: f32,
    mix: f32,
    lp_l: OnePole,
    lp_r: OnePole,
    dc_l: DcBlock,
    dc_r: DcBlock,
}

impl Overdrive {
    pub fn new(sr: f32) -> Self {
        Overdrive { sr, mode: 0, drive: 0.4, tone: 0.5, output: db_to_gain(-3.0), mix: 1.0, lp_l: OnePole::default(), lp_r: OnePole::default(), dc_l: DcBlock::default(), dc_r: DcBlock::default() }
    }
    #[inline]
    fn shape(&self, x: f32) -> f32 {
        match self.mode {
            1 => x.clamp(-1.0, 1.0),
            2 => {
                let y = x.signum() * (1.0 - (-x.abs() * 2.0).exp());
                (y + 0.2 * y * y).clamp(-1.0, 1.0)
            }
            3 => {
                if x >= 0.0 { x.tanh() } else { (x * 0.7).tanh() * 1.1 }
            }
            _ => x.tanh(),
        }
    }
}

impl Effect for Overdrive {
    fn set_param(&mut self, id: &str, v: f32) {
        match id {
            "mode" => self.mode = v.round() as u8,
            "drive" => self.drive = v,
            "tone" => self.tone = v,
            "output" => self.output = db_to_gain(v),
            "mix" => self.mix = v,
            _ => {}
        }
    }
    fn process(&mut self, l: &mut [f32], r: &mut [f32], _ctx: &ProcessContext) {
        let pre = 1.0 + self.drive * self.drive * 60.0;
        let cut = 800.0 * 2f32.powf(self.tone * 4.0);
        self.lp_l.set_freq(cut, self.sr);
        self.lp_r.set_freq(cut, self.sr);
        let comp = 1.0 / (1.0 + self.drive * 2.0);
        for i in 0..l.len() {
            let a = self.dc_l.process(self.lp_l.process(self.shape(l[i] * pre)));
            let b = self.dc_r.process(self.lp_r.process(self.shape(r[i] * pre)));
            l[i] = l[i] * (1.0 - self.mix) + a * self.mix * self.output * (0.5 + comp);
            r[i] = r[i] * (1.0 - self.mix) + b * self.mix * self.output * (0.5 + comp);
        }
    }
    fn reset(&mut self) {}
}

// ============================================================================ Bitcrusher

pub struct Bitcrusher {
    bits: f32,
    down: f32,
    mix: f32,
    hold_l: f32,
    hold_r: f32,
    count: f32,
}

impl Bitcrusher {
    pub fn new(_sr: f32) -> Self {
        Bitcrusher { bits: 8.0, down: 4.0, mix: 1.0, hold_l: 0.0, hold_r: 0.0, count: 0.0 }
    }
}

impl Effect for Bitcrusher {
    fn set_param(&mut self, id: &str, v: f32) {
        match id {
            "bits" => self.bits = v,
            "downsample" => self.down = v.max(1.0),
            "mix" => self.mix = v,
            _ => {}
        }
    }
    fn process(&mut self, l: &mut [f32], r: &mut [f32], _ctx: &ProcessContext) {
        let q = 2f32.powf(self.bits - 1.0);
        for i in 0..l.len() {
            self.count += 1.0;
            if self.count >= self.down {
                self.count -= self.down;
                self.hold_l = (l[i] * q).round() / q;
                self.hold_r = (r[i] * q).round() / q;
            }
            l[i] = l[i] * (1.0 - self.mix) + self.hold_l * self.mix;
            r[i] = r[i] * (1.0 - self.mix) + self.hold_r * self.mix;
        }
    }
    fn reset(&mut self) {}
}

// ============================================================================ Auto Filter

pub struct AutoFilter {
    sr: f32,
    mode: FilterMode,
    cutoff: f32,
    res: f32,
    rate: f32,
    depth: f32,
    mix: f32,
    fl: Svf,
    fr: Svf,
    free: f64,
}

impl AutoFilter {
    pub fn new(sr: f32) -> Self {
        AutoFilter { sr, mode: FilterMode::Low, cutoff: 1200.0, res: 0.4, rate: 1.0, depth: 0.5, mix: 1.0, fl: Svf::default(), fr: Svf::default(), free: 0.0 }
    }
}

impl Effect for AutoFilter {
    fn set_param(&mut self, id: &str, v: f32) {
        match id {
            "type" => self.mode = FilterMode::from_index(v),
            "cutoff" => self.cutoff = v,
            "resonance" => self.res = v,
            "lfo_rate" => self.rate = v.max(0.01),
            "lfo_depth" => self.depth = v,
            "mix" => self.mix = v,
            _ => {}
        }
    }
    fn process(&mut self, l: &mut [f32], r: &mut [f32], ctx: &ProcessContext) {
        for i in 0..l.len() {
            if i % 16 == 0 {
                let beat = if ctx.playing { ctx.beat + i as f64 * ctx.beats_per_sample } else {
                    self.free += ctx.beats_per_sample * 16.0;
                    self.free
                };
                let lfo = ((beat / self.rate as f64).fract() as f32 * std::f32::consts::TAU).sin();
                let f = self.cutoff * 2f32.powf(lfo * self.depth * 3.0);
                self.fl.set(f, self.res * 0.95, self.sr);
                self.fr.set(f, self.res * 0.95, self.sr);
            }
            let a = self.fl.process(l[i], self.mode);
            let b = self.fr.process(r[i], self.mode);
            l[i] = l[i] * (1.0 - self.mix) + a * self.mix;
            r[i] = r[i] * (1.0 - self.mix) + b * self.mix;
        }
    }
    fn reset(&mut self) {
        self.fl.reset();
        self.fr.reset();
    }
}

// ============================================================================ Gain utility

pub struct Gain {
    gain: f32,
    inv_l: bool,
    inv_r: bool,
    mono: bool,
    width: f32,
}

impl Gain {
    pub fn new(_sr: f32) -> Self {
        Gain { gain: 1.0, inv_l: false, inv_r: false, mono: false, width: 1.0 }
    }
}

impl Effect for Gain {
    fn set_param(&mut self, id: &str, v: f32) {
        match id {
            "gain" => self.gain = db_to_gain(v),
            "invert_l" => self.inv_l = v >= 0.5,
            "invert_r" => self.inv_r = v >= 0.5,
            "mono" => self.mono = v >= 0.5,
            "width" => self.width = v,
            _ => {}
        }
    }
    fn process(&mut self, l: &mut [f32], r: &mut [f32], _ctx: &ProcessContext) {
        for i in 0..l.len() {
            let mut a = l[i] * self.gain * if self.inv_l { -1.0 } else { 1.0 };
            let mut b = r[i] * self.gain * if self.inv_r { -1.0 } else { 1.0 };
            let m = (a + b) * 0.5;
            let s = (a - b) * 0.5 * if self.mono { 0.0 } else { self.width };
            a = m + s;
            b = m - s;
            l[i] = a;
            r[i] = b;
        }
    }
    fn reset(&mut self) {}
}

// ============================================================================ Exciter

pub struct Exciter {
    sr: f32,
    freq: f32,
    amount: f32,
    mix: f32,
    hp1: [Biquad; 2],
    hp2: [Biquad; 2],
}

impl Exciter {
    pub fn new(sr: f32) -> Self {
        let mut e = Exciter { sr, freq: 4000.0, amount: 0.4, mix: 0.3, hp1: [Biquad::default(); 2], hp2: [Biquad::default(); 2] };
        e.update();
        e
    }
    fn update(&mut self) {
        for k in 0..2 {
            self.hp1[k].set(BiquadKind::HighPass, self.freq, 0.707, 0.0, self.sr);
            self.hp2[k].set(BiquadKind::HighPass, self.freq, 0.707, 0.0, self.sr);
        }
    }
}

impl Effect for Exciter {
    fn set_param(&mut self, id: &str, v: f32) {
        match id {
            "freq" => {
                self.freq = v;
                self.update();
            }
            "amount" => self.amount = v,
            "mix" => self.mix = v,
            _ => {}
        }
    }
    fn process(&mut self, l: &mut [f32], r: &mut [f32], _ctx: &ProcessContext) {
        let d = 1.0 + self.amount * 10.0;
        for i in 0..l.len() {
            let hl = self.hp2[0].process((self.hp1[0].process(l[i]) * d).tanh());
            let hr = self.hp2[1].process((self.hp1[1].process(r[i]) * d).tanh());
            l[i] += hl * self.mix * 0.5;
            r[i] += hr * self.mix * 0.5;
        }
    }
    fn reset(&mut self) {
        for b in self.hp1.iter_mut().chain(self.hp2.iter_mut()) {
            b.reset();
        }
    }
}

// ============================================================================ De-Esser

/// Detection envelope: a fast attack catches the start of an "s", the release lets go between syllables.
const DE_ESS_ATTACK_MS: f32 = 1.0;
const DE_ESS_RELEASE_MS: f32 = 60.0;
/// The Split shelf has its midpoint below `freq`, so the reduction is full from `freq` up while the voice an
/// octave lower is barely touched.
const DE_ESS_SHELF_RATIO: f32 = 0.6;
const DE_ESS_SHELF_Q: f32 = 0.8;
/// The shelf coefficients are recomputed at most once every this many samples, and only when the reduction moved.
const DE_ESS_SHELF_STEP: usize = 16;
const DE_ESS_SHELF_EPS_DB: f32 = 0.05;

pub struct DeEsser {
    sr: f32,
    freq: f32,
    threshold: f32,
    range: f32,
    split: bool,
    listen: bool,
    att: f32,
    rel: f32,
    /// High-pass at `freq`, one per channel: the detector input, and what Listen plays.
    detect: [Biquad; 2],
    /// Dynamic high shelf, one per channel; at 0 dB its coefficients make it an exact identity.
    shelf: [Biquad; 2],
    env: f32,
    /// Reduction the shelf coefficients hold now (dB, >= 0).
    applied_db: f32,
    since_update: usize,
    dirty: bool,
    /// Largest reduction of the last block, in dB (<= 0), like the compressor's.
    pub gain_reduction_db: f32,
}

impl DeEsser {
    pub fn new(sr: f32) -> Self {
        let coef = |ms: f32| 1.0 - (-1.0 / (ms * 0.001 * sr)).exp();
        let mut d = DeEsser {
            sr,
            freq: 6000.0,
            threshold: -24.0,
            range: 8.0,
            split: true,
            listen: false,
            att: coef(DE_ESS_ATTACK_MS),
            rel: coef(DE_ESS_RELEASE_MS),
            detect: [Biquad::default(); 2],
            shelf: [Biquad::default(); 2],
            env: 0.0,
            applied_db: 0.0,
            since_update: 0,
            dirty: false,
            gain_reduction_db: 0.0,
        };
        d.update_filters();
        d
    }
    fn update_filters(&mut self) {
        for b in self.detect.iter_mut() {
            b.set(BiquadKind::HighPass, self.freq, 0.707, 0.0, self.sr);
        }
        self.set_shelf(self.applied_db);
        self.dirty = false;
    }
    fn set_shelf(&mut self, red: f32) {
        for b in self.shelf.iter_mut() {
            b.set(BiquadKind::HighShelf, self.freq * DE_ESS_SHELF_RATIO, DE_ESS_SHELF_Q, -red, self.sr);
        }
        self.applied_db = red;
    }
}

impl Effect for DeEsser {
    fn set_param(&mut self, id: &str, v: f32) {
        if !v.is_finite() {
            return;
        }
        match id {
            "freq" => {
                let f = v.clamp(2000.0, 12000.0);
                if f != self.freq {
                    self.freq = f;
                    self.dirty = true;
                }
            }
            "threshold" => self.threshold = v.clamp(-60.0, 0.0),
            "range" => self.range = v.clamp(0.0, 24.0),
            "mode" => self.split = v < 0.5,
            "listen" => self.listen = v >= 0.5,
            _ => {}
        }
    }
    fn process(&mut self, l: &mut [f32], r: &mut [f32], _ctx: &ProcessContext) {
        if self.dirty {
            self.update_filters();
        }
        let mut max_red: f32 = 0.0;
        for (a, b) in l.iter_mut().zip(r.iter_mut()) {
            let (xl, xr) = (*a, *b);
            let hl = self.detect[0].process(xl);
            let hr = self.detect[1].process(xr);
            let level = hl.abs().max(hr.abs());
            let coef = if level > self.env { self.att } else { self.rel };
            self.env += (level - self.env) * coef;
            if self.env < 1e-9 {
                self.env = 0.0;
            }
            let over = gain_to_db(self.env) - self.threshold;
            let red = if over > 0.0 { over.min(self.range) } else { 0.0 };
            max_red = max_red.max(red);
            self.since_update += 1;
            if self.since_update >= DE_ESS_SHELF_STEP {
                self.since_update = 0;
                if (red - self.applied_db).abs() >= DE_ESS_SHELF_EPS_DB || (red == 0.0 && self.applied_db != 0.0) {
                    self.set_shelf(red);
                }
            }
            let sl = self.shelf[0].process(xl);
            let sr = self.shelf[1].process(xr);
            (*a, *b) = if self.listen {
                (hl, hr)
            } else if self.split {
                (sl, sr)
            } else {
                let g = if red > 0.0 { db_to_gain(-red) } else { 1.0 };
                (xl * g, xr * g)
            };
        }
        self.gain_reduction_db = -max_red;
    }
    fn reset(&mut self) {
        for b in self.detect.iter_mut().chain(self.shelf.iter_mut()) {
            b.reset();
        }
        self.env = 0.0;
        self.since_update = 0;
        self.gain_reduction_db = 0.0;
        self.set_shelf(0.0);
    }
    fn gain_reduction_db(&self) -> Option<f32> {
        Some(self.gain_reduction_db)
    }
}

// ============================================================================ Dynamic EQ

const DEQ_BANDS: usize = 4;
/// Parameters per band, in registry order after the `b<n>_` prefix: on, type, freq, gain, q, threshold, range.
const DEQ_BAND_PARAMS: usize = 7;
const DEQ_IDS: [&str; 31] = [
    "b1_on", "b1_type", "b1_freq", "b1_gain", "b1_q", "b1_threshold", "b1_range",
    "b2_on", "b2_type", "b2_freq", "b2_gain", "b2_q", "b2_threshold", "b2_range",
    "b3_on", "b3_type", "b3_freq", "b3_gain", "b3_q", "b3_threshold", "b3_range",
    "b4_on", "b4_type", "b4_freq", "b4_gain", "b4_q", "b4_threshold", "b4_range",
    "attack", "release", "output",
];
/// The registry defaults, in `DEQ_IDS` order (a test keeps them equal).
const DEQ_DEFAULTS: [f32; 31] = [
    1.0, 1.0, 100.0, 0.0, 0.7, -24.0, 0.0,
    1.0, 0.0, 300.0, 0.0, 1.0, -24.0, 0.0,
    1.0, 0.0, 2500.0, 0.0, 1.0, -24.0, 0.0,
    1.0, 2.0, 8000.0, 0.0, 0.7, -24.0, 0.0,
    10.0, 150.0, 0.0,
];
const DEQ_ATTACK: usize = 28;
const DEQ_RELEASE: usize = 29;
const DEQ_OUTPUT: usize = 30;
/// Offset of the threshold inside a band: changing it needs no new filter coefficients.
const DEQ_THRESHOLD: usize = 5;
/// The band filters follow the level at most once every this many samples, and only when the gain moved this much.
const DEQ_STEP: usize = 16;
const DEQ_EPS_DB: f32 = 0.05;
/// Most a band moves the signal, static and dynamic gain together.
const DEQ_MAX_DB: f32 = 30.0;
/// Shelves and their detectors use a fixed slope, like the Channel EQ; Q shapes only the bells.
const DEQ_SHELF_Q: f32 = 0.707;

/// The registry range of parameter `i` (`DEQ_IDS` order), applied again here because automation reaches
/// `set_param` without going through the session.
fn deq_clamp(i: usize, v: f32) -> f32 {
    let (lo, hi) = match i {
        DEQ_ATTACK => (0.1, 200.0),
        DEQ_RELEASE => (10.0, 2000.0),
        DEQ_OUTPUT => (-24.0, 24.0),
        _ => match i % DEQ_BAND_PARAMS {
            0 => (0.0, 1.0),
            1 => (0.0, 2.0),
            2 => (20.0, 20000.0),
            3 => (-24.0, 24.0),
            4 => (0.1, 10.0),
            5 => (-60.0, 0.0),
            _ => (-24.0, 24.0),
        },
    };
    let v = v.clamp(lo, hi);
    if i < DEQ_ATTACK && i % DEQ_BAND_PARAMS < 2 {
        v.round()
    } else {
        v
    }
}

#[derive(Clone, Copy, Default)]
struct DeqBand {
    on: bool,
    /// 0 Bell, 1 Low Shelf, 2 High Shelf.
    kind: u8,
    freq: f32,
    gain: f32,
    q: f32,
    threshold: f32,
    /// `threshold` as a linear level, so a band below it needs no logarithm.
    threshold_lin: f32,
    range: f32,
    /// On the input, one per channel: band-pass for a bell, low-pass for a low shelf, high-pass for a high shelf.
    detect: [Biquad; 2],
    /// The band itself, one per channel; at 0 dB its coefficients are an exact identity.
    eq: [Biquad; 2],
    env: f32,
    /// Dynamic gain the band wants now, in dB, with the sign of `range`.
    dyn_db: f32,
    /// Static plus dynamic gain the `eq` coefficients hold now.
    applied_db: f32,
}

impl DeqBand {
    /// A band with no gain and no range is skipped: its filter would be an identity.
    fn active(&self) -> bool {
        self.on && (self.gain != 0.0 || self.range != 0.0)
    }
    fn eq_shape(&self) -> (BiquadKind, f32) {
        deq_eq_shape(self.kind, self.q)
    }
    /// A shelf listens to everything on its side of `freq`, a bell to the band around it.
    fn detect_shape(&self) -> (BiquadKind, f32) {
        match self.kind {
            1 => (BiquadKind::LowPass, DEQ_SHELF_Q),
            2 => (BiquadKind::HighPass, DEQ_SHELF_Q),
            _ => (BiquadKind::BandPass, self.q),
        }
    }
    fn target_db(&self) -> f32 {
        (self.gain + self.dyn_db).clamp(-DEQ_MAX_DB, DEQ_MAX_DB)
    }
    fn set_eq(&mut self, db: f32, sr: f32) {
        let (kind, q) = self.eq_shape();
        for b in self.eq.iter_mut() {
            b.set(kind, self.freq, q, db, sr);
        }
        self.applied_db = db;
    }
    fn clear_detector(&mut self) {
        for b in self.detect.iter_mut() {
            b.reset();
        }
        self.env = 0.0;
        self.dyn_db = 0.0;
    }
    fn clear(&mut self) {
        self.clear_detector();
        for b in self.eq.iter_mut() {
            b.reset();
        }
    }
}

/// Filter shape and Q of a band of type `kind` (0 Bell, 1 Low Shelf, 2 High Shelf): the processor and the drawn
/// response share it.
fn deq_eq_shape(kind: u8, q: f32) -> (BiquadKind, f32) {
    match kind {
        1 => (BiquadKind::LowShelf, DEQ_SHELF_Q),
        2 => (BiquadKind::HighShelf, DEQ_SHELF_Q),
        _ => (BiquadKind::Peak, q),
    }
}

/// The Dynamic EQ parameters in processor order, read with `get` and clamped like `set_param` (a value that is not
/// finite reads as the default).
pub fn dynamic_eq_params(get: impl Fn(&str) -> f32) -> [f32; 31] {
    std::array::from_fn(|i| {
        let v = get(DEQ_IDS[i]);
        if v.is_finite() {
            deq_clamp(i, v)
        } else {
            DEQ_DEFAULTS[i]
        }
    })
}

/// The filter of each band that runs, with `extra[k]` dB added to band k's static gain (0 for the static curve, the
/// band's Range for how far it can go), limited like the processor's target gain.
pub fn dynamic_eq_filters(p: &[f32; 31], sr: f32, extra: &[f32; 4]) -> [Option<Biquad>; 4] {
    std::array::from_fn(|k| {
        let b = &p[k * DEQ_BAND_PARAMS..(k + 1) * DEQ_BAND_PARAMS];
        let active = b[0] >= 0.5 && (b[3] != 0.0 || b[6] != 0.0);
        active.then(|| {
            let (kind, q) = deq_eq_shape(b[1] as u8, b[4]);
            let gain = (b[3] + extra[k]).clamp(-DEQ_MAX_DB, DEQ_MAX_DB);
            Biquad::new(kind, b[2], q, gain, sr)
        })
    })
}

/// Gain in dB of the whole Dynamic EQ at `freq` with `extra` per band (see `dynamic_eq_filters`), output included.
pub fn dynamic_eq_response_db(p: &[f32; 31], freq: f32, sr: f32, extra: &[f32; 4]) -> f32 {
    dynamic_eq_filters(p, sr, extra).iter().flatten().map(|b| b.magnitude_db(freq, sr)).sum::<f32>() + p[DEQ_OUTPUT]
}

pub struct DynamicEq {
    sr: f32,
    p: [f32; 31],
    bands: [DeqBand; DEQ_BANDS],
    att: f32,
    rel: f32,
    out: f32,
    since_update: usize,
    dirty: bool,
    /// Dynamic gain of each band (dB, without its static gain) with the largest size in the last block: negative
    /// while a band cuts, positive while it boosts, 0 below threshold or when the band is off.
    pub dynamic_gain_db: [f32; DEQ_BANDS],
}

impl DynamicEq {
    pub fn new(sr: f32) -> Self {
        let mut d = DynamicEq {
            sr,
            p: DEQ_DEFAULTS,
            bands: [DeqBand::default(); DEQ_BANDS],
            att: 0.0,
            rel: 0.0,
            out: 1.0,
            since_update: 0,
            dirty: false,
            dynamic_gain_db: [0.0; DEQ_BANDS],
        };
        d.att = d.coef(DEQ_DEFAULTS[DEQ_ATTACK]);
        d.rel = d.coef(DEQ_DEFAULTS[DEQ_RELEASE]);
        d.out = db_to_gain(DEQ_DEFAULTS[DEQ_OUTPUT]);
        d.update();
        d
    }
    fn coef(&self, ms: f32) -> f32 {
        1.0 - (-1.0 / (ms * 0.001 * self.sr)).exp()
    }
    fn update(&mut self) {
        let sr = self.sr;
        for (k, band) in self.bands.iter_mut().enumerate() {
            let p = &self.p[k * DEQ_BAND_PARAMS..(k + 1) * DEQ_BAND_PARAMS];
            band.on = p[0] >= 0.5;
            band.kind = p[1] as u8;
            band.freq = p[2];
            band.gain = p[3];
            band.q = p[4];
            band.threshold = p[DEQ_THRESHOLD];
            band.threshold_lin = db_to_gain(p[DEQ_THRESHOLD]);
            band.range = p[6];
            let (kind, q) = band.detect_shape();
            for b in band.detect.iter_mut() {
                b.set(kind, band.freq, q, 0.0, sr);
            }
            if !band.active() {
                band.clear();
            } else if band.range == 0.0 {
                band.clear_detector();
            }
            // Keeps the dynamic gain it has, so moving a band while it works does not jump.
            band.set_eq(band.target_db(), sr);
        }
        self.dirty = false;
    }
}

impl Effect for DynamicEq {
    fn set_param(&mut self, id: &str, v: f32) {
        if !v.is_finite() {
            return;
        }
        let Some(i) = DEQ_IDS.iter().position(|x| *x == id) else {
            return;
        };
        let v = deq_clamp(i, v);
        if self.p[i] == v {
            return;
        }
        self.p[i] = v;
        match i {
            DEQ_ATTACK => self.att = self.coef(v),
            DEQ_RELEASE => self.rel = self.coef(v),
            DEQ_OUTPUT => self.out = db_to_gain(v),
            _ if i % DEQ_BAND_PARAMS == DEQ_THRESHOLD => {
                let band = &mut self.bands[i / DEQ_BAND_PARAMS];
                band.threshold = v;
                band.threshold_lin = db_to_gain(v);
            }
            _ => self.dirty = true,
        }
    }
    fn process(&mut self, l: &mut [f32], r: &mut [f32], _ctx: &ProcessContext) {
        if self.dirty {
            self.update();
        }
        let (att, rel, sr, out) = (self.att, self.rel, self.sr, self.out);
        let mut peak = [0.0f32; DEQ_BANDS];
        for (a, b) in l.iter_mut().zip(r.iter_mut()) {
            let (xl, xr) = (*a, *b);
            self.since_update += 1;
            let step = self.since_update >= DEQ_STEP;
            if step {
                self.since_update = 0;
            }
            let (mut yl, mut yr) = (xl, xr);
            for (band, pk) in self.bands.iter_mut().zip(peak.iter_mut()) {
                if !band.active() {
                    continue;
                }
                if band.range != 0.0 {
                    // The detectors listen to the input, so one band moving never changes what another hears.
                    let level = band.detect[0].process(xl).abs().max(band.detect[1].process(xr).abs());
                    let coef = if level > band.env { att } else { rel };
                    band.env += (level - band.env) * coef;
                    if band.env < 1e-9 {
                        band.env = 0.0;
                    }
                    band.dyn_db = if band.env > band.threshold_lin {
                        (gain_to_db(band.env) - band.threshold).clamp(0.0, band.range.abs()).copysign(band.range)
                    } else {
                        0.0
                    };
                    if band.dyn_db.abs() > pk.abs() {
                        *pk = band.dyn_db;
                    }
                    if step {
                        let t = band.target_db();
                        // Back to exactly the static gain once the band stops working.
                        if t != band.applied_db && ((t - band.applied_db).abs() >= DEQ_EPS_DB || band.dyn_db == 0.0) {
                            band.set_eq(t, sr);
                        }
                    }
                }
                yl = band.eq[0].process(yl);
                yr = band.eq[1].process(yr);
            }
            *a = yl * out;
            *b = yr * out;
        }
        self.dynamic_gain_db = peak;
    }
    fn reset(&mut self) {
        let sr = self.sr;
        for band in self.bands.iter_mut() {
            band.clear();
            band.set_eq(band.target_db(), sr);
        }
        self.since_update = 0;
        self.dynamic_gain_db = [0.0; DEQ_BANDS];
    }
    /// Only the cuts: a band that boosts is not reducing anything.
    fn gain_reduction_db(&self) -> Option<f32> {
        Some(self.dynamic_gain_db.iter().fold(0.0f32, |m, g| m.min(*g)))
    }
}

// ============================================================================ Stereo Imager

const IMG_IDS: [&str; 9] = ["low_freq", "high_freq", "low_width", "mid_width", "high_width", "mono_bass", "balance", "listen", "output"];
/// The registry defaults and ranges, in `IMG_IDS` order (a test keeps them equal). The ranges are applied again here
/// because automation reaches `set_param` without going through the session.
const IMG_DEFAULTS: [f32; 9] = [150.0, 4000.0, 1.0, 1.0, 1.0, 0.0, 0.0, 0.0, 0.0];
const IMG_RANGES: [(f32, f32); 9] = [(40.0, 1000.0), (1000.0, 16000.0), (0.0, 2.0), (0.0, 2.0), (0.0, 2.0), (0.0, 1.0), (-1.0, 1.0), (0.0, 2.0), (-24.0, 24.0)];
const IMG_LOW_FREQ: usize = 0;
const IMG_HIGH_FREQ: usize = 1;
const IMG_LOW_WIDTH: usize = 2;
const IMG_MID_WIDTH: usize = 3;
const IMG_HIGH_WIDTH: usize = 4;
const IMG_MONO_BASS: usize = 5;
const IMG_BALANCE: usize = 6;
const IMG_LISTEN: usize = 7;
const IMG_OUTPUT: usize = 8;
/// Butterworth Q: two such filters in series make a 4th-order Linkwitz-Riley crossover.
const LR_Q: f32 = std::f32::consts::FRAC_1_SQRT_2;
/// Widths, balance and output glide over about this time, so automation and slider moves do not click.
const IMG_SMOOTH_MS: f32 = 10.0;

/// A 4th-order Linkwitz-Riley split: its low and high outputs are in phase and sum to an all-pass, flat in level.
#[derive(Clone, Copy, Default)]
struct Lr4 {
    lp: [Biquad; 2],
    hp: [Biquad; 2],
}

impl Lr4 {
    fn set(&mut self, freq: f32, sr: f32) {
        for b in self.lp.iter_mut() {
            b.set(BiquadKind::LowPass, freq, LR_Q, 0.0, sr);
        }
        for b in self.hp.iter_mut() {
            b.set(BiquadKind::HighPass, freq, LR_Q, 0.0, sr);
        }
    }

    #[inline]
    fn split(&mut self, x: f32) -> (f32, f32) {
        let low = self.lp[0].process(x);
        let high = self.hp[0].process(x);
        (self.lp[1].process(low), self.hp[1].process(high))
    }

    fn reset(&mut self) {
        for b in self.lp.iter_mut().chain(self.hp.iter_mut()) {
            b.reset();
        }
    }
}

/// The all-pass an `Lr4` at the same frequency makes when its two outputs are summed: x - 2 * band-pass at the
/// Butterworth Q. The RBJ low-, high- and band-pass share one bilinear transform, so the identity holds exactly.
#[derive(Clone, Copy, Default)]
struct Lr4AllPass(Biquad);

impl Lr4AllPass {
    fn set(&mut self, freq: f32, sr: f32) {
        self.0.set(BiquadKind::BandPass, freq, LR_Q, 0.0, sr);
    }

    #[inline]
    fn process(&mut self, x: f32) -> f32 {
        x - 2.0 * self.0.process(x)
    }

    fn reset(&mut self) {
        self.0.reset();
    }
}

pub struct StereoImager {
    sr: f32,
    p: [f32; 9],
    /// Side path: split at Low Freq, the rest split again at High Freq. The low band also goes through the High Freq
    /// all-pass, so the three bands add up in phase.
    split_low: Lr4,
    split_high: Lr4,
    low_ap: Lr4AllPass,
    /// Mid path: the all-passes of both crossovers, so the Mid stays aligned with the Side and width 1 changes no level.
    mid_ap: [Lr4AllPass; 2],
    /// A crossover frequency changed: new coefficients at the next block.
    dirty: bool,
    /// Gains the parameters ask for and the ones in use now: low, mid, high width, left and right balance, output.
    target: [f32; 6],
    current: [f32; 6],
    smooth: f32,
    /// Jump straight to the targets at the next block (after `new` or `reset`), so a project opens without a glide.
    snap: bool,
    /// 0 Stereo, 1 Mid, 2 Side.
    listen: u8,
}

impl StereoImager {
    pub fn new(sr: f32) -> Self {
        let mut s = StereoImager {
            sr,
            p: IMG_DEFAULTS,
            split_low: Lr4::default(),
            split_high: Lr4::default(),
            low_ap: Lr4AllPass::default(),
            mid_ap: [Lr4AllPass::default(); 2],
            dirty: false,
            target: [1.0; 6],
            current: [1.0; 6],
            smooth: 1.0 - (-1.0 / (IMG_SMOOTH_MS * 0.001 * sr)).exp(),
            snap: true,
            listen: 0,
        };
        s.update_crossovers();
        s.retarget();
        s
    }

    fn update_crossovers(&mut self) {
        let (lo, hi, sr) = (self.p[IMG_LOW_FREQ], self.p[IMG_HIGH_FREQ], self.sr);
        self.split_low.set(lo, sr);
        self.split_high.set(hi, sr);
        self.low_ap.set(hi, sr);
        self.mid_ap[0].set(lo, sr);
        self.mid_ap[1].set(hi, sr);
        self.dirty = false;
    }

    fn retarget(&mut self) {
        let p = &self.p;
        let low = if p[IMG_MONO_BASS] >= 0.5 { 0.0 } else { p[IMG_LOW_WIDTH] };
        let b = p[IMG_BALANCE];
        self.target = [low, p[IMG_MID_WIDTH], p[IMG_HIGH_WIDTH], (1.0 - b).min(1.0), (1.0 + b).min(1.0), db_to_gain(p[IMG_OUTPUT])];
        self.listen = p[IMG_LISTEN] as u8;
    }
}

impl Effect for StereoImager {
    fn set_param(&mut self, id: &str, v: f32) {
        if !v.is_finite() {
            return;
        }
        let Some(i) = IMG_IDS.iter().position(|x| *x == id) else {
            return;
        };
        let (lo, hi) = IMG_RANGES[i];
        let mut v = v.clamp(lo, hi);
        if i == IMG_MONO_BASS || i == IMG_LISTEN {
            v = v.round();
        }
        if self.p[i] == v {
            return;
        }
        self.p[i] = v;
        if i == IMG_LOW_FREQ || i == IMG_HIGH_FREQ {
            self.dirty = true;
        } else {
            self.retarget();
        }
    }

    fn process(&mut self, l: &mut [f32], r: &mut [f32], _ctx: &ProcessContext) {
        if self.dirty {
            self.update_crossovers();
        }
        if self.snap {
            self.current = self.target;
            self.snap = false;
        }
        let (target, k, listen) = (self.target, self.smooth, self.listen);
        let mut g = self.current;
        for (a, b) in l.iter_mut().zip(r.iter_mut()) {
            for (c, t) in g.iter_mut().zip(target) {
                let d = t - *c;
                // Lands exactly on the target, so the gains never creep through subnormal numbers.
                *c = if d.abs() < 1e-6 { t } else { *c + d * k };
            }
            let m = (*a + *b) * 0.5;
            let s = (*a - *b) * 0.5;
            let (s_low, s_rest) = self.split_low.split(s);
            let s_low = self.low_ap.process(s_low);
            let (s_mid, s_high) = self.split_high.split(s_rest);
            let m = self.mid_ap[0].process(m);
            let m = self.mid_ap[1].process(m);
            let s = g[0] * s_low + g[1] * s_mid + g[2] * s_high;
            let (yl, yr) = match listen {
                1 => (m, m),
                2 => (s, s),
                _ => ((m + s) * g[3], (m - s) * g[4]),
            };
            *a = yl * g[5];
            *b = yr * g[5];
        }
        self.current = g;
    }

    fn reset(&mut self) {
        self.split_low.reset();
        self.split_high.reset();
        self.low_ap.reset();
        for ap in self.mid_ap.iter_mut() {
            ap.reset();
        }
        self.snap = true;
    }
}

#[cfg(all(test, target_arch = "x86_64"))]
mod tests {
    use super::*;
    use crate::denormal::FlushToZero;

    const SR: f32 = 48000.0;
    const BLOCK: usize = 512;

    /// Every number the comb filters keep between samples.
    fn comb_states(rev: &Reverb) -> impl Iterator<Item = f32> + '_ {
        rev.combs_l.iter().chain(&rev.combs_r).flat_map(|c| c.buf.iter().copied().chain(std::iter::once(c.store)))
    }

    fn allpass_states(rev: &Reverb) -> impl Iterator<Item = f32> + '_ {
        rev.ap_l.iter().chain(&rev.ap_r).flat_map(|a| a.buf.iter().copied())
    }

    /// One block with an impulse, then `silent_blocks` blocks of silence; `each` sees the output and the reverb after
    /// every silent block with its index.
    fn impulse_then_silence(rev: &mut Reverb, silent_blocks: usize, mut each: impl FnMut(usize, &[f32], &[f32], &Reverb)) {
        let ctx = ProcessContext::idle(SR);
        let mut l = vec![0.0f32; BLOCK];
        let mut r = vec![0.0f32; BLOCK];
        l[0] = 1.0;
        r[0] = 1.0;
        rev.process(&mut l, &mut r, &ctx);
        for block in 0..silent_blocks {
            l.iter_mut().chain(r.iter_mut()).for_each(|x| *x = 0.0);
            rev.process(&mut l, &mut r, &ctx);
            each(block, &l, &r, rev);
        }
    }

    #[test]
    fn reverb_tail_reaches_exact_zero_with_flush_to_zero() {
        std::thread::spawn(|| {
            let _ftz = FlushToZero::enable();
            let mut rev = Reverb::new(SR);
            let silent_blocks = 60 * 48000 / BLOCK;
            let last_second = silent_blocks - 94;
            impulse_then_silence(&mut rev, silent_blocks, |block, l, r, _| {
                if block >= last_second {
                    for (i, (a, b)) in l.iter().zip(r).enumerate() {
                        assert!(*a == 0.0 && *b == 0.0, "block {block} sample {i}: {a:e} {b:e}");
                    }
                }
            });
            let comb = comb_states(&rev).find(|x| x.is_subnormal());
            assert_eq!(comb, None, "subnormal comb state");
            let allpass = allpass_states(&rev).find(|x| x.is_subnormal());
            assert_eq!(allpass, None, "subnormal allpass state");
        })
        .join()
        .unwrap();
    }

    #[test]
    fn reverb_tail_goes_subnormal_without_flush_to_zero() {
        std::thread::spawn(|| {
            let mut rev = Reverb::new(SR);
            let mut seen_subnormal = false;
            impulse_then_silence(&mut rev, 30 * 48000 / BLOCK, |_, _, _, rev| {
                seen_subnormal |= comb_states(rev).any(|x| x.is_subnormal());
            });
            assert!(seen_subnormal, "the comb filters never reached subnormal numbers");
        })
        .join()
        .unwrap();
    }

    /// FNV-1a over the bits of every output sample.
    fn fingerprint(l: &[f32], r: &[f32]) -> u64 {
        let mut h: u64 = 0xcbf2_9ce4_8422_2325;
        for x in l.iter().chain(r) {
            for b in x.to_bits().to_le_bytes() {
                h = (h ^ u64::from(b)).wrapping_mul(0x0100_0000_01b3);
            }
        }
        h
    }

    /// One second of loud material for the limiter: a low sine, bursts of a high sine and noise.
    fn limiter_input(sr: f32) -> (Vec<f32>, Vec<f32>) {
        use std::f32::consts::PI;
        let mut noise = crate::util::Noise::new(11);
        let (mut l, mut r) = (Vec::new(), Vec::new());
        for i in 0..sr as usize {
            let t = i as f32 / sr;
            let burst = if (i / 3000) % 2 == 0 { 1.0 } else { 0.2 };
            let x = (2.0 * PI * 110.0 * t).sin() * 0.4 + (2.0 * PI * 3000.0 * t).sin() * 0.3 * burst + noise.next() * 0.1;
            l.push(x);
            r.push(x * 0.8 + noise.next() * 0.05);
        }
        (l, r)
    }

    #[test]
    fn limiter_off_matches_the_previous_output_bit_for_bit() {
        // Output of the limiter before the true-peak mode, with only the parameters it had then.
        const BEFORE: u64 = 0xec70_b6ba_c040_557b;
        let (mut l, mut r) = limiter_input(48000.0);
        let mut lim = Limiter::new(48000.0);
        let ctx = ProcessContext::idle(48000.0);
        let sizes = [512usize, 1, 300, 1024, 77];
        let (mut pos, mut k) = (0, 0);
        while pos < l.len() {
            // parameters move between blocks, as automation moves them
            lim.set_param("gain", [6.0, 9.0, 12.0][k % 3]);
            lim.set_param("ceiling", if k % 2 == 0 { -1.0 } else { -3.0 });
            lim.set_param("release", if k % 4 < 2 { 60.0 } else { 200.0 });
            if k == 40 {
                lim.reset();
            }
            let end = (pos + sizes[k % sizes.len()]).min(l.len());
            lim.process(&mut l[pos..end], &mut r[pos..end], &ctx);
            pos = end;
            k += 1;
        }
        assert!(l.iter().chain(&r).all(|x| x.is_finite()));
        assert_eq!(fingerprint(&l, &r), BEFORE, "the limiter with True Peak Off must sound as before");
    }

    /// A built-in limiter set up from the registry, as the engine sets it up.
    fn limiter_with(sr: f32, gain: f32, ceiling: f32, true_peak: f32) -> Box<dyn Effect> {
        let mut fx = crate::create_effect("limiter", sr).expect("built-in limiter");
        let mut params = std::collections::BTreeMap::new();
        params.insert("gain".to_string(), gain);
        params.insert("ceiling".to_string(), ceiling);
        params.insert("true_peak".to_string(), true_peak);
        crate::configure("limiter", &params, |k, v| fx.set_param(k, v));
        fx
    }

    /// Runs `fx` over the whole signal in blocks of 512.
    fn run_blocks(fx: &mut dyn Effect, l: &mut [f32], r: &mut [f32], sr: f32) {
        let ctx = ProcessContext::idle(sr);
        for (bl, br) in l.chunks_mut(512).zip(r.chunks_mut(512)) {
            fx.process(bl, br, &ctx);
        }
    }

    /// fs/4 shifted by 45 degrees at amplitude 0.5: the samples sit at 0.354, the real peak at 0.5.
    fn quarter_rate_sine(len: usize) -> Vec<f32> {
        use std::f32::consts::PI;
        (0..len).map(|n| 0.5 * (PI * n as f32 / 2.0 + PI / 4.0).sin()).collect()
    }

    /// The three test signals of the true-peak limiter, one second each.
    fn true_peak_signals(sr: f32) -> Vec<(&'static str, Vec<f32>, Vec<f32>)> {
        use std::f32::consts::PI;
        let len = sr as usize;
        let sine = quarter_rate_sine(len);
        let bursts: Vec<f32> = (0..len + 7)
            .map(|n| {
                let amp = if (n / 2000) % 2 == 0 { 1.0 } else { 0.05 };
                0.5 * amp * (2.0 * PI * 0.23 * n as f32).sin()
            })
            .collect();
        let (mut na, mut nb) = (crate::util::Noise::new(3), crate::util::Noise::new(4));
        let noise_l: Vec<f32> = (0..len).map(|_| na.next() * 0.25).collect();
        let noise_r: Vec<f32> = (0..len).map(|_| nb.next() * 0.25).collect();
        vec![
            ("fs/4 at 45 degrees", sine.clone(), sine),
            // the right channel is the left one 7 samples late
            ("bursts", bursts[7..].to_vec(), bursts[..len].to_vec()),
            ("noise", noise_l, noise_r),
        ]
    }

    #[test]
    fn limiter_true_peak_keeps_the_output_under_the_ceiling_between_samples() {
        for sr in [48000.0f32, 44100.0] {
            for (name, mut l, mut r) in true_peak_signals(sr) {
                let mut lim = limiter_with(sr, 12.0, -1.0, 1.0);
                run_blocks(lim.as_mut(), &mut l, &mut r, sr);
                assert!(l.iter().chain(&r).all(|x| x.is_finite()), "{sr} Hz, {name}: not finite");
                let tp = crate::util::true_peak(&l).max(crate::util::true_peak(&r));
                let db = gain_to_db(tp);
                assert!(tp <= db_to_gain(-1.0 + 0.2), "{sr} Hz, {name}: {db:.2} dBTP over the -1 dB ceiling");
                if name.starts_with("fs/4") {
                    assert!(tp >= db_to_gain(-1.5), "{sr} Hz, {name}: {db:.2} dBTP, limited under the ceiling");
                }
            }
        }
    }

    #[test]
    fn limiter_off_lets_peaks_between_samples_through() {
        let sr = 48000.0;
        let mut l = quarter_rate_sine(sr as usize);
        let mut r = l.clone();
        let mut lim = limiter_with(sr, 12.0, -1.0, 0.0);
        run_blocks(lim.as_mut(), &mut l, &mut r, sr);
        let sample_peak = l.iter().chain(&r).fold(0.0f32, |m, x| m.max(x.abs()));
        assert!(sample_peak <= db_to_gain(-1.0) + 1e-6, "sample peak {sample_peak}");
        let tp = crate::util::true_peak(&l).max(crate::util::true_peak(&r));
        assert!(tp > db_to_gain(1.0), "true peak {:.2} dBTP", gain_to_db(tp));
    }

    #[test]
    fn limiter_true_peak_does_not_change_the_latency() {
        let sr = 48000.0;
        let look = (sr * 0.0015) as usize;
        for tp in [0.0, 1.0] {
            let mut lim = limiter_with(sr, 0.0, -1.0, tp);
            let mut l = vec![0.0f32; 512];
            l[0] = 0.1;
            let mut r = l.clone();
            lim.process(&mut l, &mut r, &ProcessContext::idle(sr));
            for (i, (a, b)) in l.iter().zip(&r).enumerate() {
                if i == look {
                    assert!((a - 0.1).abs() < 1e-6 && (b - 0.1).abs() < 1e-6, "true_peak {tp}: {a} {b} at {i}");
                } else {
                    assert!(*a == 0.0 && *b == 0.0, "true_peak {tp}: {a} {b} at {i}");
                }
            }
        }
    }

    #[test]
    fn limiter_switching_true_peak_keeps_the_ceiling() {
        let sr = 48000.0;
        let (_, l0, r0) = true_peak_signals(sr).swap_remove(1);
        let ceiling = db_to_gain(-1.0) + 1e-6;
        let ctx = ProcessContext::idle(sr);
        let mut lim = limiter_with(sr, 12.0, -1.0, 0.0);
        let (mut l, mut r) = (l0.clone(), r0.clone());
        for (k, (bl, br)) in l.chunks_mut(512).zip(r.chunks_mut(512)).enumerate() {
            if k % 10 == 0 {
                lim.set_param("true_peak", if (k / 10) % 2 == 0 { 1.0 } else { 0.0 });
            }
            lim.process(bl, br, &ctx);
        }
        for x in l.iter().chain(&r) {
            assert!(x.is_finite() && x.abs() <= ceiling, "{x} over the ceiling");
        }
        // the engine sets every parameter again on each change: a repeated On is not a new switch
        let mut once = limiter_with(sr, 12.0, -1.0, 0.0);
        let mut twice = limiter_with(sr, 12.0, -1.0, 0.0);
        let (mut l1, mut r1) = (l0.clone(), r0.clone());
        let (mut l2, mut r2) = (l0, r0);
        for (k, ((a, b), (c, d))) in l1.chunks_mut(512).zip(r1.chunks_mut(512)).zip(l2.chunks_mut(512).zip(r2.chunks_mut(512))).enumerate() {
            if k == 5 {
                once.set_param("true_peak", 1.0);
                twice.set_param("true_peak", 1.0);
            }
            if k == 7 {
                twice.set_param("true_peak", 1.0);
            }
            once.process(a, b, &ctx);
            twice.process(c, d, &ctx);
        }
        assert_eq!(fingerprint(&l1, &r1), fingerprint(&l2, &r2));
    }
}

#[cfg(test)]
mod de_esser_tests {
    use super::*;
    use std::collections::BTreeMap;

    const SR: f32 = 48000.0;
    /// Half a second at 48 kHz.
    const LEN: usize = 24000;
    /// The window the tone levels are measured on: 20 periods of 200 Hz and 700 of 7 kHz, so no leakage.
    const WIN: usize = 4800;
    const MINUS_90_DB: f32 = 3.162e-5;

    fn tone(freq: f64, amp: f32, len: usize) -> Vec<f32> {
        (0..len).map(|n| amp * (2.0 * std::f64::consts::PI * freq * n as f64 / SR as f64).sin() as f32).collect()
    }

    fn sum(a: &[f32], b: &[f32]) -> Vec<f32> {
        a.iter().zip(b).map(|(x, y)| x + y).collect()
    }

    /// A de-esser set up the way the engine does it: registry defaults, then `params`.
    fn de_esser(params: &[(&str, f32)]) -> DeEsser {
        let mut d = DeEsser::new(SR);
        let map: BTreeMap<String, f32> = params.iter().map(|(k, v)| (k.to_string(), *v)).collect();
        crate::configure("de_esser", &map, |k, v| d.set_param(k, v));
        d
    }

    /// Copies `input` to both channels and processes it in blocks of 512.
    fn run(d: &mut DeEsser, input: &[f32]) -> (Vec<f32>, Vec<f32>) {
        let mut l = input.to_vec();
        let mut r = input.to_vec();
        let ctx = ProcessContext::idle(SR);
        for (a, b) in l.chunks_mut(512).zip(r.chunks_mut(512)) {
            d.process(a, b, &ctx);
        }
        (l, r)
    }

    /// Amplitude of the `freq` component over the last `WIN` samples (single-bin DFT).
    fn level(x: &[f32], freq: f64) -> f32 {
        let w = &x[x.len() - WIN..];
        let (mut re, mut im) = (0.0f64, 0.0f64);
        for (n, v) in w.iter().enumerate() {
            let ph = 2.0 * std::f64::consts::PI * freq * n as f64 / SR as f64;
            re += *v as f64 * ph.cos();
            im -= *v as f64 * ph.sin();
        }
        (2.0 / WIN as f64 * (re * re + im * im).sqrt()) as f32
    }

    fn db(a: f32, b: f32) -> f32 {
        20.0 * (a / b).log10()
    }

    fn max_diff(a: &[f32], b: &[f32]) -> f32 {
        a.iter().zip(b).map(|(x, y)| (x - y).abs()).fold(0.0, f32::max)
    }

    #[test]
    fn split_turns_down_the_sibilant_band_by_range_and_leaves_the_voice_alone() {
        let input = sum(&tone(200.0, 0.3, LEN), &tone(7000.0, 0.5, LEN));
        let mut d = de_esser(&[]);
        let (l, r) = run(&mut d, &input);
        for out in [&l, &r] {
            let hi = db(level(out, 7000.0), level(&input, 7000.0));
            let lo = db(level(out, 200.0), level(&input, 200.0));
            assert!((-9.0..=-7.0).contains(&hi), "7 kHz changed by {hi} dB");
            assert!(lo.abs() < 0.5, "200 Hz changed by {lo} dB");
        }
        assert!((d.gain_reduction_db + 8.0).abs() < 1e-3, "gain reduction {}", d.gain_reduction_db);
    }

    #[test]
    fn wideband_turns_down_the_whole_signal() {
        let input = sum(&tone(200.0, 0.3, LEN), &tone(7000.0, 0.5, LEN));
        let mut d = de_esser(&[("mode", 1.0)]);
        let (l, r) = run(&mut d, &input);
        for out in [&l, &r] {
            for f in [200.0, 7000.0] {
                let change = db(level(out, f), level(&input, f));
                assert!((-9.0..=-7.0).contains(&change), "{f} Hz changed by {change} dB");
            }
        }
    }

    #[test]
    fn below_threshold_the_output_is_the_input() {
        let input = sum(&tone(200.0, 0.5, LEN), &tone(7000.0, 0.02, LEN));
        for mode in [0.0, 1.0] {
            let mut d = de_esser(&[("mode", mode)]);
            let (l, r) = run(&mut d, &input);
            assert!(max_diff(&l, &input) < MINUS_90_DB, "mode {mode}: left differs by {}", max_diff(&l, &input));
            assert!(max_diff(&r, &input) < MINUS_90_DB, "mode {mode}: right differs by {}", max_diff(&r, &input));
            assert_eq!(d.gain_reduction_db, 0.0);
        }
    }

    #[test]
    fn zero_range_is_transparent_even_above_threshold() {
        let input = sum(&tone(200.0, 0.3, LEN), &tone(7000.0, 0.5, LEN));
        for mode in [0.0, 1.0] {
            let mut d = de_esser(&[("range", 0.0), ("mode", mode)]);
            let (l, r) = run(&mut d, &input);
            assert!(max_diff(&l, &input) < MINUS_90_DB, "mode {mode}: left differs by {}", max_diff(&l, &input));
            assert!(max_diff(&r, &input) < MINUS_90_DB, "mode {mode}: right differs by {}", max_diff(&r, &input));
        }
    }

    #[test]
    fn a_loud_low_tone_does_not_trigger_it() {
        let input = tone(200.0, 0.9, LEN);
        let mut d = de_esser(&[]);
        let (l, r) = run(&mut d, &input);
        assert_eq!(d.gain_reduction_db, 0.0);
        assert!(max_diff(&l, &input) < MINUS_90_DB);
        assert!(max_diff(&r, &input) < MINUS_90_DB);
    }

    #[test]
    fn listen_plays_only_the_detection_band() {
        let input = sum(&tone(200.0, 0.3, LEN), &tone(7000.0, 0.3, LEN));
        let mut d = de_esser(&[("listen", 1.0)]);
        let (l, r) = run(&mut d, &input);
        for out in [&l, &r] {
            assert!(level(out, 7000.0) > 0.7 * 0.3, "7 kHz at {}", level(out, 7000.0));
            assert!(level(out, 200.0) < 0.01 * 0.3, "200 Hz at {}", level(out, 200.0));
        }
    }

    #[test]
    fn silence_and_subnormals_stay_finite() {
        let ctx = ProcessContext::idle(SR);
        // Silence in, silence out.
        let mut d = de_esser(&[]);
        for _ in 0..200 {
            let mut l = [0.0f32; 512];
            let mut r = [0.0f32; 512];
            d.process(&mut l, &mut r, &ctx);
            assert!(l.iter().chain(r.iter()).all(|x| *x == 0.0));
        }
        // Subnormal input, with alternating sign.
        let tiny = [f32::from_bits(1), f32::MIN_POSITIVE * 0.5];
        for mode in [0.0, 1.0] {
            let mut d = de_esser(&[("mode", mode)]);
            for _ in 0..100 {
                let mut l: [f32; 512] = std::array::from_fn(|i| if i % 4 < 2 { tiny[i % 2] } else { -tiny[i % 2] });
                let mut r = l;
                d.process(&mut l, &mut r, &ctx);
                assert!(l.iter().chain(r.iter()).all(|x| x.is_finite() && x.abs() < 1e-30));
                assert!(!d.env.is_subnormal(), "envelope {}", d.env);
            }
        }
        // A loud "s", then five seconds of silence: the envelope settles to zero, not into subnormals.
        let mut d = de_esser(&[]);
        let s = tone(7000.0, 0.9, 512);
        let (mut l, mut r) = (s.clone(), s);
        d.process(&mut l, &mut r, &ctx);
        assert!(d.gain_reduction_db < 0.0);
        for _ in 0..(5 * 48000 / 512) {
            let mut l = [0.0f32; 512];
            let mut r = [0.0f32; 512];
            d.process(&mut l, &mut r, &ctx);
            assert!(l.iter().chain(r.iter()).all(|x| x.is_finite()));
        }
        assert!(d.env == 0.0 || d.env.is_normal(), "envelope {}", d.env);
        assert_eq!(d.gain_reduction_db, 0.0);
    }

    #[test]
    fn reset_clears_the_envelope_and_the_shelf() {
        let mut d = de_esser(&[]);
        let s = tone(7000.0, 0.5, 512);
        let ctx = ProcessContext::idle(SR);
        let mut blocks = 0;
        while d.gain_reduction_db >= -7.0 {
            let (mut l, mut r) = (s.clone(), s.clone());
            d.process(&mut l, &mut r, &ctx);
            blocks += 1;
            assert!(blocks < 100, "the de-esser never kicked in");
        }
        d.reset();
        assert_eq!(d.gain_reduction_db, 0.0);
        let input = sum(&tone(200.0, 0.5, LEN), &tone(7000.0, 0.02, LEN));
        let (l, r) = run(&mut d, &input);
        assert!(max_diff(&l, &input) < MINUS_90_DB, "left differs by {}", max_diff(&l, &input));
        assert!(max_diff(&r, &input) < MINUS_90_DB, "right differs by {}", max_diff(&r, &input));
    }

    #[test]
    fn reacts_within_a_few_milliseconds_and_lets_go_after_the_s() {
        let mut d = de_esser(&[]);
        let ctx = ProcessContext::idle(SR);
        // One block is one millisecond.
        const MS: usize = 48;
        for _ in 0..100 {
            let mut l = [0.0f32; MS];
            let mut r = [0.0f32; MS];
            d.process(&mut l, &mut r, &ctx);
            assert_eq!(d.gain_reduction_db, 0.0);
        }
        let s = tone(7000.0, 0.5, 200 * MS);
        let mut reacted_at = None;
        for (i, chunk) in s.chunks(MS).enumerate() {
            let (mut l, mut r) = (chunk.to_vec(), chunk.to_vec());
            d.process(&mut l, &mut r, &ctx);
            if reacted_at.is_none() && d.gain_reduction_db <= -7.0 {
                reacted_at = Some(i);
            }
        }
        let reacted_at = reacted_at.expect("the de-esser never reached range - 1 dB");
        assert!(reacted_at < 5, "reacted only after {} ms", reacted_at + 1);
        for _ in 0..500 {
            let mut l = [0.0f32; MS];
            let mut r = [0.0f32; MS];
            d.process(&mut l, &mut r, &ctx);
        }
        assert_eq!(d.gain_reduction_db, 0.0);
    }
}

#[cfg(test)]
mod dynamic_eq_tests {
    use super::*;
    use std::collections::BTreeMap;

    const SR: f32 = 48000.0;
    /// Half a second at 48 kHz.
    const LEN: usize = 24000;
    /// The window the tone levels are measured on: whole periods of every test frequency, so no leakage.
    const WIN: usize = 4800;

    fn tone(freq: f64, amp: f32, len: usize) -> Vec<f32> {
        (0..len).map(|n| amp * (2.0 * std::f64::consts::PI * freq * n as f64 / SR as f64).sin() as f32).collect()
    }

    fn sum(a: &[f32], b: &[f32]) -> Vec<f32> {
        a.iter().zip(b).map(|(x, y)| x + y).collect()
    }

    /// A dynamic EQ set up the way the engine does it: registry defaults, then `params`.
    fn deq(params: &[(&str, f32)]) -> DynamicEq {
        let mut d = DynamicEq::new(SR);
        let map: BTreeMap<String, f32> = params.iter().map(|(k, v)| (k.to_string(), *v)).collect();
        crate::configure("dynamic_eq", &map, |k, v| d.set_param(k, v));
        d
    }

    /// Copies `input` to both channels and processes it in blocks of 512.
    fn run(d: &mut DynamicEq, input: &[f32]) -> (Vec<f32>, Vec<f32>) {
        let mut l = input.to_vec();
        let mut r = input.to_vec();
        let ctx = ProcessContext::idle(SR);
        for (a, b) in l.chunks_mut(512).zip(r.chunks_mut(512)) {
            d.process(a, b, &ctx);
        }
        (l, r)
    }

    /// Amplitude of the `freq` component over the last `WIN` samples (single-bin DFT).
    fn level(x: &[f32], freq: f64) -> f32 {
        let w = &x[x.len() - WIN..];
        let (mut re, mut im) = (0.0f64, 0.0f64);
        for (n, v) in w.iter().enumerate() {
            let ph = 2.0 * std::f64::consts::PI * freq * n as f64 / SR as f64;
            re += *v as f64 * ph.cos();
            im -= *v as f64 * ph.sin();
        }
        (2.0 / WIN as f64 * (re * re + im * im).sqrt()) as f32
    }

    fn db(a: f32, b: f32) -> f32 {
        20.0 * (a / b).log10()
    }

    fn max_diff(a: &[f32], b: &[f32]) -> f32 {
        a.iter().zip(b).map(|(x, y)| (x - y).abs()).fold(0.0, f32::max)
    }

    #[test]
    fn defaults_and_ids_match_the_registry() {
        let info = ondina_core::plugins::find("dynamic_eq").expect("dynamic_eq in the registry");
        let ids: Vec<&str> = info.params.iter().map(|p| p.id).collect();
        assert_eq!(ids, DEQ_IDS);
        let defaults: Vec<f32> = info.params.iter().map(|p| p.default).collect();
        assert_eq!(defaults, DEQ_DEFAULTS);
        assert_eq!(DynamicEq::new(SR).p, DEQ_DEFAULTS);
    }

    #[test]
    fn set_param_clamps_like_the_registry() {
        let info = ondina_core::plugins::find("dynamic_eq").unwrap();
        let mut d = DynamicEq::new(SR);
        for (i, p) in info.params.iter().enumerate() {
            d.set_param(p.id, p.min - 1000.0);
            assert_eq!(d.p[i], p.min, "{} below its range", p.id);
            d.set_param(p.id, p.max + 1000.0);
            assert_eq!(d.p[i], p.max, "{} above its range", p.id);
            d.set_param(p.id, f32::NAN);
            d.set_param(p.id, f32::INFINITY);
            assert_eq!(d.p[i], p.max, "{} took a value that is not finite", p.id);
        }
        let before = d.p;
        d.set_param("b5_freq", 1000.0);
        d.set_param("", 1.0);
        assert_eq!(d.p, before);
    }

    #[test]
    fn a_loud_bell_band_is_cut_by_up_to_range() {
        let input = tone(300.0, 0.5, LEN);
        let mut d = deq(&[("b2_threshold", -20.0), ("b2_range", -12.0)]);
        let (l, r) = run(&mut d, &input);
        for out in [&l, &r] {
            let change = db(level(out, 300.0), level(&input, 300.0));
            assert!((-13.0..=-6.0).contains(&change), "300 Hz changed by {change} dB");
        }
        assert!((d.dynamic_gain_db[1] + 12.0).abs() < 0.5, "band 2 moved by {} dB", d.dynamic_gain_db[1]);
        for k in [0, 2, 3] {
            assert_eq!(d.dynamic_gain_db[k], 0.0, "band {} moved", k + 1);
        }
    }

    #[test]
    fn below_threshold_the_band_leaves_the_tone_alone() {
        let input = tone(300.0, 0.01, LEN);
        let mut d = deq(&[("b2_threshold", -20.0), ("b2_range", -12.0)]);
        let (l, r) = run(&mut d, &input);
        for out in [&l, &r] {
            let change = db(level(out, 300.0), level(&input, 300.0));
            assert!(change.abs() < 0.5, "300 Hz changed by {change} dB");
            assert!(max_diff(out, &input) < 1e-4);
        }
        assert_eq!(d.dynamic_gain_db[1], 0.0);
    }

    #[test]
    fn a_positive_range_boosts_a_loud_band() {
        let input = tone(300.0, 0.5, LEN);
        let mut d = deq(&[("b2_threshold", -20.0), ("b2_range", 6.0)]);
        let (l, r) = run(&mut d, &input);
        for out in [&l, &r] {
            let change = db(level(out, 300.0), level(&input, 300.0));
            assert!((5.0..=7.0).contains(&change), "300 Hz changed by {change} dB");
        }
        assert!(d.dynamic_gain_db[1] > 5.0, "band 2 moved by {} dB", d.dynamic_gain_db[1]);
    }

    #[test]
    fn transparent_when_bands_are_off_or_flat() {
        let input = sum(&sum(&tone(80.0, 0.3, LEN), &tone(1000.0, 0.3, LEN)), &tone(8000.0, 0.3, LEN));
        let mut d = deq(&[]);
        let (l, r) = run(&mut d, &input);
        assert!(max_diff(&l, &input) < 1e-4 && max_diff(&r, &input) < 1e-4, "the defaults change the sound");

        let off = [("b1_on", 0.0), ("b2_on", 0.0), ("b3_on", 0.0), ("b4_on", 0.0), ("b2_gain", 6.0), ("b2_range", -12.0), ("b2_threshold", -60.0), ("b4_gain", -6.0)];
        let mut d = deq(&off);
        let (l, r) = run(&mut d, &input);
        assert!(max_diff(&l, &input) < 1e-4 && max_diff(&r, &input) < 1e-4, "bands that are off change the sound");

        let flat: Vec<(&str, f32)> = DEQ_IDS.iter().filter(|id| id.ends_with("_threshold")).map(|id| (*id, -60.0)).collect();
        let mut d = deq(&flat);
        let (l, r) = run(&mut d, &input);
        assert!(max_diff(&l, &input) < 1e-4 && max_diff(&r, &input) < 1e-4, "flat bands change the sound");
        assert_eq!(d.dynamic_gain_db, [0.0; 4]);
    }

    #[test]
    fn static_gain_works_without_dynamics() {
        let input = tone(1000.0, 0.1, LEN);
        let mut d = deq(&[("b3_freq", 1000.0), ("b3_gain", 6.0)]);
        let (l, r) = run(&mut d, &input);
        for out in [&l, &r] {
            let change = db(level(out, 1000.0), level(&input, 1000.0));
            assert!((change - 6.0).abs() < 0.3, "1 kHz changed by {change} dB");
        }
        assert_eq!(d.dynamic_gain_db, [0.0; 4]);
    }

    #[test]
    fn a_low_shelf_reacts_to_lows_only() {
        let shelf = [("b1_freq", 150.0), ("b1_threshold", -20.0), ("b1_range", -10.0)];
        let input = tone(60.0, 0.5, LEN);
        let mut d = deq(&shelf);
        let (l, _) = run(&mut d, &input);
        let change = db(level(&l, 60.0), level(&input, 60.0));
        assert!(change <= -6.0, "a loud 60 Hz changed by {change} dB");
        assert!(d.dynamic_gain_db[0] < -9.0, "band 1 moved by {} dB", d.dynamic_gain_db[0]);

        let input = sum(&tone(60.0, 0.01, LEN), &tone(2000.0, 0.5, LEN));
        let mut d = deq(&shelf);
        let (l, _) = run(&mut d, &input);
        let change = db(level(&l, 60.0), level(&input, 60.0));
        assert!(change.abs() < 0.5, "a quiet 60 Hz under a loud 2 kHz changed by {change} dB");
        assert_eq!(d.dynamic_gain_db[0], 0.0);
    }

    #[test]
    fn extreme_parameters_stay_finite() {
        let ctx = ProcessContext::idle(SR);
        let mut seed = 12345u32;
        let mut noise = move || {
            seed = seed.wrapping_mul(1664525).wrapping_add(1013904223);
            (seed >> 8) as f32 / (1u32 << 24) as f32 - 0.5
        };
        let signal: Vec<f32> = (0..512 * 20).map(|n| (2.0 * std::f32::consts::PI * 1000.0 * n as f32 / SR).sin() + noise()).collect();
        let check = |d: &mut DynamicEq, what: &str| {
            for block in signal.chunks(512) {
                let (mut l, mut r) = (block.to_vec(), block.to_vec());
                d.process(&mut l, &mut r, &ctx);
                assert!(l.iter().chain(r.iter()).all(|x| x.is_finite()), "{what}");
            }
        };
        for gain in [-24.0, 24.0] {
            for range in [-24.0, 24.0] {
                for threshold in [-60.0, 0.0] {
                    for q in [0.1, 10.0] {
                        for freq in [20.0, 20000.0] {
                            for kind in [0.0, 1.0, 2.0] {
                                for (attack, release) in [(0.1, 10.0), (200.0, 2000.0)] {
                                    let mut params = vec![("attack", attack), ("release", release), ("output", 24.0)];
                                    for (i, id) in DEQ_IDS[..DEQ_ATTACK].iter().enumerate() {
                                        let v = match i % DEQ_BAND_PARAMS {
                                            0 => 1.0,
                                            1 => kind,
                                            2 => freq,
                                            3 => gain,
                                            4 => q,
                                            5 => threshold,
                                            _ => range,
                                        };
                                        params.push((*id, v));
                                    }
                                    let mut d = deq(&params);
                                    check(&mut d, &format!("{params:?}"));
                                }
                            }
                        }
                    }
                }
            }
        }
        let mut d = deq(&[]);
        for v in [1e9, -1e9, f32::NAN, f32::INFINITY] {
            for id in DEQ_IDS {
                d.set_param(id, v);
            }
            check(&mut d, &format!("every parameter set to {v}"));
        }
    }

    #[test]
    fn silence_settles_to_zero() {
        let ctx = ProcessContext::idle(SR);
        let working = [("b2_range", -12.0), ("b2_threshold", -60.0)];
        let mut d = deq(&working);
        for _ in 0..200 {
            let mut l = [0.0f32; 512];
            let mut r = [0.0f32; 512];
            d.process(&mut l, &mut r, &ctx);
            assert!(l.iter().chain(r.iter()).all(|x| *x == 0.0));
        }

        let mut d = deq(&working);
        let s = tone(300.0, 0.9, 512);
        let (mut l, mut r) = (s.clone(), s);
        d.process(&mut l, &mut r, &ctx);
        assert!(d.dynamic_gain_db[1] < 0.0);
        for _ in 0..(5 * 48000 / 512) {
            let mut l = [0.0f32; 512];
            let mut r = [0.0f32; 512];
            d.process(&mut l, &mut r, &ctx);
            assert!(l.iter().chain(r.iter()).all(|x| x.is_finite()));
        }
        for band in &d.bands {
            assert!(band.env == 0.0 || band.env.is_normal(), "envelope {}", band.env);
        }
        assert_eq!(d.dynamic_gain_db, [0.0; 4]);

        let tiny = [f32::from_bits(1), f32::MIN_POSITIVE * 0.5];
        let mut d = deq(&working);
        for _ in 0..100 {
            let mut l: [f32; 512] = std::array::from_fn(|i| if i % 4 < 2 { tiny[i % 2] } else { -tiny[i % 2] });
            let mut r = l;
            d.process(&mut l, &mut r, &ctx);
            assert!(l.iter().chain(r.iter()).all(|x| x.is_finite() && x.abs() < 1e-30));
            for band in &d.bands {
                assert!(!band.env.is_subnormal(), "envelope {}", band.env);
            }
        }
    }

    #[test]
    fn no_latency() {
        let mut impulse = vec![0.0f32; 2048];
        impulse[0] = 1.0;
        let mut d = deq(&[]);
        let (l, r) = run(&mut d, &impulse);
        assert_eq!(l, impulse);
        assert_eq!(r, impulse);

        let mut d = deq(&[("b3_freq", 1000.0), ("b3_gain", 6.0)]);
        let (l, _) = run(&mut d, &impulse);
        assert!(l[0] >= 0.5, "the response starts late: first sample {}", l[0]);
        let peak = l.iter().enumerate().fold((0, 0.0f32), |best, (i, x)| if x.abs() > best.1 { (i, x.abs()) } else { best });
        assert_eq!(peak.0, 0, "the response peaks at sample {}", peak.0);
    }

    #[test]
    fn reset_clears_the_bands() {
        let mut d = deq(&[("b2_threshold", -20.0), ("b2_range", -12.0)]);
        let loud = tone(300.0, 0.5, 512);
        let ctx = ProcessContext::idle(SR);
        for _ in 0..100 {
            let (mut l, mut r) = (loud.clone(), loud.clone());
            d.process(&mut l, &mut r, &ctx);
            if d.dynamic_gain_db[1] < -11.0 {
                break;
            }
        }
        assert!(d.dynamic_gain_db[1] < -11.0, "band 2 never reached its range");
        d.reset();
        assert_eq!(d.dynamic_gain_db, [0.0; 4]);
        let input = tone(300.0, 0.01, LEN);
        let (l, r) = run(&mut d, &input);
        assert!(max_diff(&l, &input) < 1e-4 && max_diff(&r, &input) < 1e-4);
    }
}

#[cfg(test)]
mod stereo_imager_tests {
    use super::*;
    use std::collections::BTreeMap;

    const SR: f32 = 48000.0;
    /// One second at 48 kHz: the crossovers have settled long before the window.
    const LEN: usize = 48000;
    /// The last 0.2 s: whole periods of every test frequency (all multiples of 5 Hz), so no leakage.
    const WIN: usize = 9600;
    const BLOCK: usize = 512;

    fn tone(freq: f64, amp: f32, len: usize) -> Vec<f32> {
        (0..len).map(|n| amp * (2.0 * std::f64::consts::PI * freq * n as f64 / SR as f64).sin() as f32).collect()
    }

    fn sum(a: &[f32], b: &[f32]) -> Vec<f32> {
        a.iter().zip(b).map(|(x, y)| x + y).collect()
    }

    fn neg(a: &[f32]) -> Vec<f32> {
        a.iter().map(|x| -x).collect()
    }

    fn mid(l: &[f32], r: &[f32]) -> Vec<f32> {
        l.iter().zip(r).map(|(a, b)| (a + b) * 0.5).collect()
    }

    fn side(l: &[f32], r: &[f32]) -> Vec<f32> {
        l.iter().zip(r).map(|(a, b)| (a - b) * 0.5).collect()
    }

    /// A stereo imager set up the way the engine does it: registry defaults, then `params`.
    fn img(params: &[(&str, f32)]) -> StereoImager {
        let mut d = StereoImager::new(SR);
        let map: BTreeMap<String, f32> = params.iter().map(|(k, v)| (k.to_string(), *v)).collect();
        crate::configure("stereo_imager", &map, |k, v| d.set_param(k, v));
        d
    }

    /// Processes `l` and `r` in blocks of 512.
    fn run(d: &mut StereoImager, l: &[f32], r: &[f32]) -> (Vec<f32>, Vec<f32>) {
        let (mut l, mut r) = (l.to_vec(), r.to_vec());
        let ctx = ProcessContext::idle(SR);
        for (a, b) in l.chunks_mut(BLOCK).zip(r.chunks_mut(BLOCK)) {
            d.process(a, b, &ctx);
        }
        (l, r)
    }

    /// Amplitude of the `freq` component over the last `WIN` samples (single-bin DFT).
    fn level(x: &[f32], freq: f64) -> f32 {
        let w = &x[x.len() - WIN..];
        let (mut re, mut im) = (0.0f64, 0.0f64);
        for (n, v) in w.iter().enumerate() {
            let ph = 2.0 * std::f64::consts::PI * freq * n as f64 / SR as f64;
            re += *v as f64 * ph.cos();
            im -= *v as f64 * ph.sin();
        }
        (2.0 / WIN as f64 * (re * re + im * im).sqrt()) as f32
    }

    fn db(a: f32, b: f32) -> f32 {
        20.0 * (a / b).log10()
    }

    fn max_abs(x: &[f32]) -> f32 {
        x.iter().fold(0.0, |m, v| m.max(v.abs()))
    }

    /// Deterministic pink noise: white noise from an LCG through a three-pole pinking filter.
    fn pink(len: usize, seed: u32) -> Vec<f32> {
        let mut state = seed;
        let (mut b0, mut b1, mut b2) = (0.0f32, 0.0f32, 0.0f32);
        (0..len)
            .map(|_| {
                state = state.wrapping_mul(1_664_525).wrapping_add(1_013_904_223);
                let white = (state >> 8) as f32 / (1u32 << 24) as f32 * 2.0 - 1.0;
                b0 = 0.99765 * b0 + white * 0.099046;
                b1 = 0.963 * b1 + white * 0.296516;
                b2 = 0.57 * b2 + white * 1.052691;
                (b0 + b1 + b2 + white * 0.1848) * 0.05
            })
            .collect()
    }

    /// Energy of `x` in the third-octave band around `fc`, after the first `skip` samples.
    fn band_energy(x: &[f32], fc: f32, skip: usize) -> f64 {
        let mut bp = Biquad::new(BiquadKind::BandPass, fc, 4.32, 0.0, SR);
        x.iter().enumerate().map(|(n, v)| (n, bp.process(*v))).filter(|(n, _)| *n >= skip).map(|(_, y)| f64::from(y) * f64::from(y)).sum()
    }

    #[test]
    fn defaults_and_ranges_match_the_registry() {
        let info = ondina_core::plugins::find("stereo_imager").expect("stereo_imager in the registry");
        let ids: Vec<&str> = info.params.iter().map(|p| p.id).collect();
        assert_eq!(ids, IMG_IDS);
        let defaults: Vec<f32> = info.params.iter().map(|p| p.default).collect();
        assert_eq!(defaults, IMG_DEFAULTS);
        let ranges: Vec<(f32, f32)> = info.params.iter().map(|p| (p.min, p.max)).collect();
        assert_eq!(ranges, IMG_RANGES);
        assert_eq!(StereoImager::new(SR).p, IMG_DEFAULTS);
    }

    #[test]
    fn unity_widths_keep_the_level_flat_on_pink_noise() {
        let l = pink(3 * 48000, 1);
        let r = pink(3 * 48000, 2);
        let (ol, or) = run(&mut img(&[]), &l, &r);
        let skip = 24000;
        let centres = [31.5, 40.0, 50.0, 63.0, 80.0, 100.0, 125.0, 160.0, 200.0, 250.0, 315.0, 400.0, 500.0, 630.0, 800.0, 1000.0, 1250.0, 1600.0, 2000.0, 2500.0, 3150.0, 4000.0, 5000.0, 6300.0, 8000.0, 10000.0, 12500.0, 16000.0];
        for fc in centres {
            for (name, input, out) in [("left", &l, &ol), ("right", &r, &or)] {
                let change = 10.0 * (band_energy(out, fc, skip) / band_energy(input, fc, skip)).log10();
                assert!(change.abs() <= 0.5, "{fc} Hz, {name} channel: {change:.2} dB");
            }
        }
    }

    #[test]
    fn unity_widths_keep_a_hard_left_tone_on_the_left() {
        for f in [30.0, 60.0, 100.0, 150.0, 250.0, 500.0, 1000.0, 2000.0, 4000.0, 8000.0, 12000.0, 16000.0] {
            let l = tone(f, 0.5, LEN);
            let r = vec![0.0; LEN];
            let (ol, or) = run(&mut img(&[]), &l, &r);
            let change = db(level(&ol, f), level(&l, f));
            assert!(change.abs() < 0.1, "{f} Hz: the left channel changed by {change:.3} dB");
            let leak = db(level(&or, f), level(&l, f));
            assert!(level(&or, f) < level(&l, f) * 10f32.powf(-50.0 / 20.0), "{f} Hz: {leak:.1} dB leaked to the right");
        }
    }

    #[test]
    fn mono_bass_folds_the_low_antiphase_and_keeps_the_rest() {
        let set = [("mono_bass", 1.0), ("low_freq", 150.0)];
        let l = tone(60.0, 0.5, LEN);
        let (ol, or) = run(&mut img(&set), &l, &neg(&l));
        for out in [&ol, &or] {
            let change = db(level(out, 60.0), level(&l, 60.0));
            assert!(change <= -30.0, "60 Hz antiphase: {change:.1} dB");
        }
        let l = tone(2000.0, 0.5, LEN);
        let (ol, or) = run(&mut img(&set), &l, &neg(&l));
        for out in [&ol, &or] {
            let change = db(level(out, 2000.0), level(&l, 2000.0));
            assert!(change.abs() <= 1.0, "2 kHz antiphase: {change:.2} dB");
        }
        let l = tone(60.0, 0.5, LEN);
        let (ol, or) = run(&mut img(&set), &l, &l);
        for out in [&ol, &or] {
            let change = db(level(out, 60.0), level(&l, 60.0));
            assert!(change.abs() < 0.1, "60 Hz mono: {change:.3} dB");
        }
    }

    #[test]
    fn listen_plays_only_the_mid_or_only_the_side() {
        let mono = sum(&tone(100.0, 0.4, LEN), &tone(3000.0, 0.3, LEN));
        let (ol, or) = run(&mut img(&[("listen", 2.0)]), &mono, &mono);
        assert!(max_abs(&ol) < 1e-6 && max_abs(&or) < 1e-6, "Side of a mono signal: {} {}", max_abs(&ol), max_abs(&or));
        let (ol, or) = run(&mut img(&[("listen", 1.0)]), &mono, &neg(&mono));
        assert!(max_abs(&ol) < 1e-6 && max_abs(&or) < 1e-6, "Mid of an antiphase signal: {} {}", max_abs(&ol), max_abs(&or));
        let l = tone(1000.0, 0.5, LEN);
        let r = vec![0.0; LEN];
        for listen in [1.0, 2.0] {
            let (ol, or) = run(&mut img(&[("listen", listen)]), &l, &r);
            assert_eq!(ol, or, "listen {listen}: both outputs carry the same signal");
            let change = db(level(&ol, 1000.0), level(&l, 1000.0));
            assert!((change + 6.02).abs() < 0.1, "listen {listen}: {change:.3} dB");
        }
    }

    #[test]
    fn high_width_widens_only_above_high_freq() {
        let s_in = sum(&tone(60.0, 0.3, LEN), &tone(10000.0, 0.3, LEN));
        let m_in = tone(1000.0, 0.3, LEN);
        let l = sum(&m_in, &s_in);
        let r = sum(&m_in, &neg(&s_in));
        let (ol, or) = run(&mut img(&[("high_width", 2.0)]), &l, &r);
        let (so, mo) = (side(&ol, &or), mid(&ol, &or));
        let high = db(level(&so, 10000.0), level(&s_in, 10000.0));
        assert!((5.5..=6.5).contains(&high), "Side at 10 kHz: {high:.2} dB");
        let low = db(level(&so, 60.0), level(&s_in, 60.0));
        assert!(low.abs() < 0.1, "Side at 60 Hz: {low:.3} dB");
        let m = db(level(&mo, 1000.0), level(&m_in, 1000.0));
        assert!(m.abs() < 0.1, "Mid at 1 kHz: {m:.3} dB");
    }

    #[test]
    fn low_and_mid_widths_act_on_their_own_bands() {
        let l = sum(&sum(&tone(60.0, 0.3, LEN), &tone(1000.0, 0.3, LEN)), &tone(10000.0, 0.3, LEN));
        let r = neg(&l);
        let side_change = |params: &[(&str, f32)], f: f64| {
            let (ol, or) = run(&mut img(params), &l, &r);
            db(level(&side(&ol, &or), f), level(&l, f))
        };
        let low = [("low_width", 0.0)];
        let at60 = side_change(&low, 60.0);
        assert!(at60 <= -30.0, "low width 0 at 60 Hz: {at60:.1} dB");
        for f in [1000.0, 10000.0] {
            let change = side_change(&low, f);
            assert!(change.abs() < 0.1, "low width 0 at {f} Hz: {change:.3} dB");
        }
        let mid_half = [("mid_width", 0.5)];
        let at1k = side_change(&mid_half, 1000.0);
        assert!((at1k + 6.0).abs() < 0.5, "mid width 0.5 at 1 kHz: {at1k:.2} dB");
        for f in [60.0, 10000.0] {
            let change = side_change(&mid_half, f);
            assert!(change.abs() < 0.3, "mid width 0.5 at {f} Hz: {change:.3} dB");
        }
    }

    #[test]
    fn width_zero_makes_it_mono() {
        let (l, r) = (pink(LEN, 3), pink(LEN, 4));
        let (ol, or) = run(&mut img(&[("low_width", 0.0), ("mid_width", 0.0), ("high_width", 0.0)]), &l, &r);
        let diff = ol.iter().zip(&or).map(|(a, b)| (a - b).abs()).fold(0.0, f32::max);
        assert!(diff < 1e-6, "left and right differ by {diff}");
    }

    #[test]
    fn balance_turns_the_other_side_down() {
        let m = tone(1000.0, 0.5, LEN);
        let (ol, or) = run(&mut img(&[("balance", 0.5)]), &m, &m);
        let (dl, dr) = (db(level(&ol, 1000.0), level(&m, 1000.0)), db(level(&or, 1000.0), level(&m, 1000.0)));
        assert!((dl + 6.02).abs() < 0.1, "balance 0.5, left: {dl:.3} dB");
        assert!(dr.abs() < 0.1, "balance 0.5, right: {dr:.3} dB");
        let (ol, or) = run(&mut img(&[("balance", -1.0)]), &m, &m);
        assert!(max_abs(&or) < 1e-6, "balance -1, right: {}", max_abs(&or));
        let dl = db(level(&ol, 1000.0), level(&m, 1000.0));
        assert!(dl.abs() < 0.1, "balance -1, left: {dl:.3} dB");
    }

    #[test]
    fn output_gain_scales_both_channels() {
        let (l, r) = (tone(1000.0, 0.5, LEN), tone(500.0, 0.4, LEN));
        let (ol, or) = run(&mut img(&[("output", -6.0)]), &l, &r);
        let (dl, dr) = (db(level(&ol, 1000.0), level(&l, 1000.0)), db(level(&or, 500.0), level(&r, 500.0)));
        assert!((dl + 6.0).abs() < 0.1, "left: {dl:.3} dB");
        assert!((dr + 6.0).abs() < 0.1, "right: {dr:.3} dB");
    }

    #[test]
    fn set_param_clamps_like_the_registry() {
        let info = ondina_core::plugins::find("stereo_imager").unwrap();
        let mut d = img(&[]);
        for (i, p) in info.params.iter().enumerate() {
            assert_eq!(IMG_IDS[i], p.id);
            d.set_param(p.id, p.min - 1000.0);
            assert_eq!(d.p[i], p.min, "{} below the range", p.id);
            d.set_param(p.id, p.max + 1000.0);
            assert_eq!(d.p[i], p.max, "{} above the range", p.id);
            d.set_param(p.id, f32::NAN);
            assert_eq!(d.p[i], p.max, "{}: NaN is ignored", p.id);
            d.set_param(p.id, f32::INFINITY);
            assert_eq!(d.p[i], p.max, "{}: infinity is ignored", p.id);
        }
        let before = d.p;
        d.set_param("no_such_param", 0.5);
        assert_eq!(d.p, before);
        d.set_param("mono_bass", 0.7);
        assert_eq!(d.p[IMG_MONO_BASS], 1.0);
        d.set_param("listen", 1.4);
        assert_eq!(d.p[IMG_LISTEN], 1.0);
    }

    #[test]
    fn parameter_changes_glide() {
        let (mut a, mut b) = (img(&[]), img(&[]));
        let (l, r) = (pink(20 * BLOCK, 5), pink(20 * BLOCK, 6));
        let ctx = ProcessContext::idle(SR);
        let (mut out_a, mut out_b) = (Vec::new(), Vec::new());
        for (n, (cl, cr)) in l.chunks(BLOCK).zip(r.chunks(BLOCK)).enumerate() {
            if n == 10 {
                b.set_param("mono_bass", 1.0);
            }
            let (mut al, mut ar) = (cl.to_vec(), cr.to_vec());
            a.process(&mut al, &mut ar, &ctx);
            let (mut bl, mut br) = (cl.to_vec(), cr.to_vec());
            b.process(&mut bl, &mut br, &ctx);
            out_a.extend(al);
            out_b.extend(bl);
        }
        let diff: Vec<f32> = out_b.iter().zip(&out_a).skip(10 * BLOCK).map(|(x, y)| x - y).collect();
        let first = max_abs(&diff[..8]);
        let last = max_abs(&diff[diff.len() - 2048..]);
        assert!(last > 0.0, "Mono Bass changed nothing");
        assert!(first < 0.1 * last, "the change jumped: {first} at once, {last} at the end");
    }

    #[test]
    fn extreme_parameters_stay_finite() {
        let ctx = ProcessContext::idle(SR);
        let mut state = 7u32;
        let mut noise = move || {
            state = state.wrapping_mul(1_664_525).wrapping_add(1_013_904_223);
            (state >> 8) as f32 / (1u32 << 24) as f32 - 0.5
        };
        let mut blocks = |d: &mut StereoImager, n: usize| {
            for k in 0..n {
                let mut l: Vec<f32> = (0..BLOCK).map(|i| (2.0 * std::f32::consts::PI * 1000.0 * (k * BLOCK + i) as f32 / SR).sin()).collect();
                let mut r: Vec<f32> = (0..BLOCK).map(|_| noise()).collect();
                d.process(&mut l, &mut r, &ctx);
                assert!(l.iter().chain(&r).all(|v| v.is_finite()), "not finite with {:?}", d.p);
            }
        };
        for low_freq in [40.0, 1000.0] {
            for high_freq in [1000.0, 16000.0] {
                for widths in 0..8u32 {
                    for mono_bass in [0.0, 1.0] {
                        for balance in [-1.0, 1.0] {
                            for listen in [0.0, 1.0, 2.0] {
                                let w = |bit: u32| if widths & bit != 0 { 2.0 } else { 0.0 };
                                let mut d = img(&[("low_freq", low_freq), ("high_freq", high_freq), ("low_width", w(1)), ("mid_width", w(2)), ("high_width", w(4)), ("mono_bass", mono_bass), ("balance", balance), ("listen", listen), ("output", 24.0)]);
                                blocks(&mut d, 5);
                            }
                        }
                    }
                }
            }
        }
        let mut d = img(&[]);
        blocks(&mut d, 2);
        for v in [1e9, -1e9, f32::NAN, f32::INFINITY] {
            for id in IMG_IDS {
                d.set_param(id, v);
            }
            blocks(&mut d, 5);
        }
    }

    #[test]
    fn silence_and_subnormals_settle() {
        let mut d = img(&[("high_width", 2.0), ("output", 24.0)]);
        let zeros = vec![0.0; 100 * BLOCK];
        let (ol, or) = run(&mut d, &zeros, &zeros);
        assert!(ol.iter().chain(&or).all(|v| *v == 0.0), "silence in, silence out");
        let tiny: Vec<f32> = (0..100 * BLOCK).map(|n| if n % 2 == 0 { 1e-40 } else { -1e-40 }).collect();
        let (ol, or) = run(&mut img(&[]), &tiny, &tiny);
        assert!(ol.iter().chain(&or).all(|v| v.is_finite() && v.abs() < 1e-30));
        let mut d = img(&[]);
        let noise = pink(201 * BLOCK, 8);
        run(&mut d, &noise[..BLOCK], &noise[..BLOCK]);
        d.set_param("mono_bass", 1.0);
        run(&mut d, &noise[BLOCK..], &noise[BLOCK..]);
        assert!(d.current.iter().all(|g| *g == 0.0 || g.is_normal()), "{:?}", d.current);
        assert_eq!(d.current[0], 0.0);
    }

    #[test]
    fn the_response_starts_at_once() {
        let mut l = vec![0.0; BLOCK];
        l[0] = 1.0;
        let r = vec![0.0; BLOCK];
        let (ol, or) = run(&mut img(&[]), &l, &r);
        assert!(ol[0] >= 0.3, "left starts at {}", ol[0]);
        assert!(or[0].abs() < 1e-3, "right starts at {}", or[0]);
    }

    #[test]
    fn reset_clears_the_filters() {
        let mut d = img(&[("high_width", 2.0)]);
        let (l, r) = (pink(10 * BLOCK, 9), pink(10 * BLOCK, 10));
        run(&mut d, &l, &r);
        d.reset();
        let zeros = vec![0.0; 10 * BLOCK];
        let (ol, or) = run(&mut d, &zeros, &zeros);
        assert!(ol.iter().chain(&or).all(|v| *v == 0.0), "the filters kept ringing after reset");
    }
}

#[cfg(test)]
mod eq_response_tests {
    use super::*;
    use std::collections::BTreeMap;

    const SR: f32 = 48000.0;
    const FREQS: [f32; 10] = [30.0, 60.0, 120.0, 250.0, 500.0, 1000.0, 2000.0, 4000.0, 8000.0, 15000.0];

    /// A Channel EQ set up the way the engine does it: registry defaults, then `params`.
    fn ceq(params: &[(&str, f32)]) -> ChannelEq {
        let mut e = ChannelEq::new(SR);
        let map: BTreeMap<String, f32> = params.iter().map(|(k, v)| (k.to_string(), *v)).collect();
        crate::configure("channel_eq", &map, |k, v| e.set_param(k, v));
        e
    }

    /// A dynamic EQ set up the way the engine does it: registry defaults, then `params`.
    fn deq(params: &[(&str, f32)]) -> DynamicEq {
        let mut d = DynamicEq::new(SR);
        let map: BTreeMap<String, f32> = params.iter().map(|(k, v)| (k.to_string(), *v)).collect();
        crate::configure("dynamic_eq", &map, |k, v| d.set_param(k, v));
        d
    }

    /// Gain in dB the effect gives a steady sine at `freq`: 1.5 s through it in blocks of 512, then the amplitude of
    /// the `freq` component over the last second (whole periods, single-bin DFT).
    fn measured_db(fx: &mut dyn Effect, freq: f32, amp: f32) -> f32 {
        fx.reset();
        let n = 72000;
        let input: Vec<f32> = (0..n).map(|i| amp * (2.0 * std::f64::consts::PI * freq as f64 * i as f64 / SR as f64).sin() as f32).collect();
        let mut l = input.clone();
        let mut r = input;
        let ctx = ProcessContext::idle(SR);
        for (a, b) in l.chunks_mut(512).zip(r.chunks_mut(512)) {
            fx.process(a, b, &ctx);
        }
        let win = &l[n - 48000..];
        let (mut re, mut im) = (0.0f64, 0.0f64);
        for (i, v) in win.iter().enumerate() {
            let ph = 2.0 * std::f64::consts::PI * freq as f64 * i as f64 / SR as f64;
            re += *v as f64 * ph.cos();
            im -= *v as f64 * ph.sin();
        }
        let a = 2.0 / win.len() as f64 * (re * re + im * im).sqrt();
        (20.0 * (a / amp as f64).log10()) as f32
    }

    /// The value of `id` in `map`, or its registry default for `kind`.
    fn get<'a>(map: &'a [(&'a str, f32)], kind: &str) -> impl Fn(&str) -> f32 + 'a {
        let info = ondina_core::plugins::find(kind).expect("kind in the registry");
        move |id: &str| map.iter().find(|(k, _)| *k == id).map(|(_, v)| *v).unwrap_or_else(|| info.params.iter().find(|p| p.id == id).map_or(0.0, |p| p.default))
    }

    #[test]
    fn channel_eq_ids_and_defaults_match_the_registry() {
        let info = ondina_core::plugins::find("channel_eq").expect("channel_eq in the registry");
        let ids: Vec<&str> = info.params.iter().map(|p| p.id).collect();
        assert_eq!(ids, EQ_IDS);
        let defaults: Vec<f32> = info.params.iter().map(|p| p.default).collect();
        assert_eq!(ChannelEq::new(SR).p.to_vec(), defaults);
    }

    #[test]
    fn channel_eq_response_matches_measured_sines() {
        let map: &[(&str, f32)] = &[
            ("hp_on", 1.0), ("hp_freq", 40.0), ("low_gain", 4.0), ("low_freq", 150.0),
            ("mid1_gain", -6.0), ("mid1_freq", 400.0), ("mid1_q", 2.0),
            ("mid2_gain", 5.0), ("mid2_freq", 3000.0), ("mid2_q", 0.7),
            ("high_gain", -3.0), ("high_freq", 9000.0), ("lp_on", 1.0), ("lp_freq", 14000.0), ("output", 1.5),
        ];
        let p = channel_eq_params(get(map, "channel_eq"));
        for f in FREQS {
            let curve = channel_eq_response_db(&p, f, SR);
            let measured = measured_db(&mut ceq(map), f, 0.1);
            assert!((curve - measured).abs() <= 0.5, "{f} Hz: curve {curve:.2} dB, measured {measured:.2} dB");
        }
    }

    #[test]
    fn dynamic_eq_response_matches_measured_sines_with_range_0() {
        let map: &[(&str, f32)] = &[
            ("b1_type", 1.0), ("b1_freq", 120.0), ("b1_gain", 3.0),
            ("b2_type", 0.0), ("b2_freq", 350.0), ("b2_gain", -5.0), ("b2_q", 1.5),
            ("b3_type", 0.0), ("b3_freq", 2500.0), ("b3_gain", 4.0), ("b3_q", 3.0),
            ("b4_type", 2.0), ("b4_freq", 8000.0), ("b4_gain", -4.0), ("output", -1.0),
        ];
        let p = dynamic_eq_params(get(map, "dynamic_eq"));
        for f in FREQS {
            let curve = dynamic_eq_response_db(&p, f, SR, &[0.0; 4]);
            let measured = measured_db(&mut deq(map), f, 0.1);
            assert!((curve - measured).abs() <= 0.5, "{f} Hz: curve {curve:.2} dB, measured {measured:.2} dB");
        }
    }

    #[test]
    fn dynamic_eq_full_range_matches_a_loud_sine() {
        let map: &[(&str, f32)] = &[
            ("b1_on", 0.0), ("b3_on", 0.0), ("b4_on", 0.0),
            ("b2_on", 1.0), ("b2_type", 0.0), ("b2_freq", 1000.0), ("b2_gain", 0.0), ("b2_q", 1.0), ("b2_threshold", -60.0), ("b2_range", -6.0),
        ];
        let p = dynamic_eq_params(get(map, "dynamic_eq"));
        let full = dynamic_eq_response_db(&p, 1000.0, SR, &[0.0, -6.0, 0.0, 0.0]);
        assert!((full + 6.0).abs() <= 0.05, "{full}");
        let measured = measured_db(&mut deq(map), 1000.0, 0.5);
        assert!((full - measured).abs() <= 1.0, "curve {full:.2} dB, measured {measured:.2} dB");
        let flat = dynamic_eq_response_db(&p, 1000.0, SR, &[0.0; 4]);
        assert!(flat.abs() <= 0.01, "{flat}");
    }

    #[test]
    fn eq_responses_are_finite_everywhere() {
        for kind in ["channel_eq", "dynamic_eq"] {
            let info = ondina_core::plugins::find(kind).unwrap();
            let maps: [Vec<(&str, f32)>; 3] = [
                info.params.iter().map(|p| (p.id, p.min)).collect(),
                info.params.iter().map(|p| (p.id, p.max)).collect(),
                info.params.iter().map(|p| (p.id, p.default)).collect(),
            ];
            for map in &maps {
                for sr in [44100.0, 96000.0] {
                    for f in [0.0, 20.0, 1000.0, 20000.0, 30000.0] {
                        if kind == "channel_eq" {
                            let p = channel_eq_params(get(map, kind));
                            assert!(channel_eq_response_db(&p, f, sr).is_finite(), "{kind} {f} Hz at {sr}");
                        } else {
                            let p = dynamic_eq_params(get(map, kind));
                            assert!(dynamic_eq_response_db(&p, f, sr, &[0.0; 4]).is_finite(), "{kind} {f} Hz at {sr}");
                            assert!(dynamic_eq_response_db(&p, f, sr, &[24.0, -24.0, 24.0, -24.0]).is_finite(), "{kind} {f} Hz at {sr}");
                        }
                    }
                }
            }
        }
        let off: &[(&str, f32)] = &[("b2_on", 0.0), ("b2_gain", 6.0), ("b3_gain", 6.0)];
        let filters = dynamic_eq_filters(&dynamic_eq_params(get(off, "dynamic_eq")), SR, &[0.0; 4]);
        assert!(filters[1].is_none() && filters[2].is_some());
        let hp_off: &[(&str, f32)] = &[("hp_on", 0.0), ("mid1_gain", 3.0)];
        let filters = channel_eq_filters(&channel_eq_params(get(hp_off, "channel_eq")), SR);
        assert!(filters[0].is_none() && filters[2].is_some());
        let defaults = channel_eq_filters(&channel_eq_params(get(&[], "channel_eq")), SR);
        assert!(defaults.iter().all(Option::is_none));
    }

    #[test]
    fn dynamic_eq_params_clamp_like_set_param() {
        let map: &[(&str, f32)] = &[("b1_freq", 5.0), ("b1_q", 50.0), ("attack", f32::NAN)];
        let p = dynamic_eq_params(get(map, "dynamic_eq"));
        assert_eq!(p[2], 20.0);
        assert_eq!(p[4], 10.0);
        assert_eq!(p[DEQ_ATTACK], 10.0);
    }
}

#[cfg(test)]
mod gain_reduction_tests {
    use super::*;
    use std::collections::BTreeMap;

    const SR: f32 = 48000.0;
    const BLOCK: usize = 480;

    /// A square wave that alternates +amp and -amp: |x| is constant, so the detectors settle on its level.
    fn square(amp: f32, len: usize) -> Vec<f32> {
        (0..len).map(|n| if n % 2 == 0 { amp } else { -amp }).collect()
    }

    fn tone(freq: f64, amp: f32, len: usize) -> Vec<f32> {
        (0..len).map(|n| amp * (2.0 * std::f64::consts::PI * freq * n as f64 / SR as f64).sin() as f32).collect()
    }

    /// Processes `input` on both channels in blocks of 480 and returns what the effect reports.
    fn feed(fx: &mut dyn Effect, input: &[f32]) -> Option<f32> {
        let mut l = input.to_vec();
        let mut r = input.to_vec();
        let ctx = ProcessContext::idle(SR);
        for (a, b) in l.chunks_mut(BLOCK).zip(r.chunks_mut(BLOCK)) {
            fx.process(a, b, &ctx);
        }
        fx.gain_reduction_db()
    }

    fn run(fx: &mut dyn Effect, amp: f32, secs: f32) -> Option<f32> {
        feed(fx, &square(amp, (SR * secs) as usize))
    }

    /// An effect set up the way the engine does it: registry defaults, then `params`.
    fn effect(kind: &str, params: &[(&str, f32)]) -> Box<dyn Effect> {
        let mut fx = crate::create_effect(kind, SR).expect("effect in the registry");
        let map: BTreeMap<String, f32> = params.iter().map(|(k, v)| (k.to_string(), *v)).collect();
        crate::configure(kind, &map, |k, v| fx.set_param(k, v));
        fx
    }

    fn reduction(v: Option<f32>) -> f32 {
        v.expect("a dynamics effect reports its gain reduction")
    }

    #[test]
    fn compressor_reduction_follows_threshold_and_ratio() {
        let params = [("threshold", -20.0), ("ratio", 4.0), ("knee", 0.0), ("attack", 1.0)];
        let mut c = effect("compressor", &params);
        let g = reduction(run(c.as_mut(), 0.5, 0.5));
        assert!((g + 10.485).abs() < 0.1, "reduction {g} dB");
        let mut c = effect("compressor", &params);
        assert_eq!(run(c.as_mut(), 0.05, 0.5), Some(0.0));
    }

    #[test]
    fn limiter_reduction_reaches_the_ceiling() {
        let params = [("gain", 0.0), ("ceiling", -6.0)];
        let mut l = effect("limiter", &params);
        let g = reduction(run(l.as_mut(), 1.0, 0.5));
        assert!((g + 6.02).abs() < 0.2, "reduction {g} dB");
        let mut l = effect("limiter", &params);
        assert_eq!(run(l.as_mut(), 0.1, 0.5), Some(0.0));
    }

    #[test]
    fn limiter_true_peak_reports_its_reduction() {
        let params = [("gain", 0.0), ("ceiling", -6.0), ("true_peak", 1.0)];
        let mut l = effect("limiter", &params);
        let g = reduction(run(l.as_mut(), 1.0, 0.5));
        assert!((-6.5..=-5.5).contains(&g), "reduction {g} dB");
        l.reset();
        assert_eq!(l.gain_reduction_db(), Some(0.0), "after reset");
        let mut l = effect("limiter", &params);
        assert_eq!(run(l.as_mut(), 0.1, 0.5), Some(0.0));
    }

    #[test]
    fn gate_reduction_reaches_its_floor_below_threshold() {
        let params = [("threshold", -40.0), ("reduction", -60.0)];
        let mut g = effect("noise_gate", &params);
        let closed = reduction(run(g.as_mut(), 0.001, 1.0));
        assert!((closed + 60.0).abs() < 0.5, "closed gate {closed} dB");
        let mut g = effect("noise_gate", &params);
        let open = reduction(run(g.as_mut(), 0.5, 1.0));
        assert!(open > -0.1, "open gate {open} dB");
        let mut g = effect("noise_gate", &params);
        let first = reduction(run(g.as_mut(), 0.0, BLOCK as f32 / SR));
        assert!(first >= -60.0, "a new gate reported {first} dB");
    }

    #[test]
    fn de_esser_and_dynamic_eq_report_what_they_measure() {
        let mut d = DeEsser::new(SR);
        crate::configure("de_esser", &BTreeMap::new(), |k, v| d.set_param(k, v));
        let g = reduction(feed(&mut d, &tone(7000.0, 0.5, 24000)));
        assert_eq!(g, d.gain_reduction_db);
        assert!(g < 0.0, "de-esser reduction {g}");

        let mut cut = effect("dynamic_eq", &[("b2_threshold", -20.0), ("b2_range", -12.0)]);
        let g = reduction(feed(cut.as_mut(), &tone(300.0, 0.5, 24000)));
        assert!(g < 0.0, "a cutting band reported {g}");
        let mut boost = effect("dynamic_eq", &[("b2_threshold", -20.0), ("b2_range", 6.0)]);
        assert_eq!(feed(boost.as_mut(), &tone(300.0, 0.5, 24000)), Some(0.0));
    }

    #[test]
    fn reset_clears_the_reported_reduction() {
        let loud: [(&str, Box<dyn Effect>, Vec<f32>); 5] = [
            ("compressor", effect("compressor", &[("threshold", -30.0)]), square(0.5, 24000)),
            ("limiter", effect("limiter", &[("ceiling", -6.0)]), square(1.0, 24000)),
            ("noise_gate", effect("noise_gate", &[("threshold", -40.0)]), square(0.001, 24000)),
            ("de_esser", effect("de_esser", &[]), tone(7000.0, 0.5, 24000)),
            ("dynamic_eq", effect("dynamic_eq", &[("b2_threshold", -20.0), ("b2_range", -12.0)]), tone(300.0, 0.5, 24000)),
        ];
        for (kind, mut fx, input) in loud {
            let g = reduction(feed(fx.as_mut(), &input));
            assert!(g < -1.0, "{kind} did not reduce ({g} dB)");
            fx.reset();
            assert_eq!(fx.gain_reduction_db(), Some(0.0), "{kind} after reset");
        }
    }
}

#[cfg(test)]
mod transfer_tests {
    use super::*;
    use std::collections::BTreeMap;

    const SR: f32 = 48000.0;
    const BLOCK: usize = 480;

    /// A square wave that alternates +amp and -amp: |x| is constant, so the detectors settle on its level.
    fn square(amp: f32, len: usize) -> Vec<f32> {
        (0..len).map(|n| if n % 2 == 0 { amp } else { -amp }).collect()
    }

    /// An effect set up the way the engine does it: registry defaults, then `params`.
    fn effect(kind: &str, params: &[(&str, f32)]) -> Box<dyn Effect> {
        let mut fx = crate::create_effect(kind, SR).expect("effect in the registry");
        let map: BTreeMap<String, f32> = params.iter().map(|(k, v)| (k.to_string(), *v)).collect();
        crate::configure(kind, &map, |k, v| fx.set_param(k, v));
        fx
    }

    /// Parameter reader: the value in `params`, otherwise the registry default.
    fn get<'a>(kind: &'a str, params: &'a [(&'a str, f32)]) -> impl Fn(&str) -> f32 + 'a {
        move |id: &str| {
            params.iter().find(|(k, _)| *k == id).map(|(_, v)| *v).unwrap_or_else(|| {
                ondina_core::plugins::find(kind).and_then(|i| i.params.iter().find(|p| p.id == id)).map_or(0.0, |p| p.default)
            })
        }
    }

    /// Output level (dB) the effect settles at for a steady square wave of `amp`: the peak of the last block.
    fn settled_out_db(fx: &mut dyn Effect, amp: f32) -> f32 {
        let input = square(amp, (SR * 0.75) as usize);
        let mut l = input.clone();
        let mut r = input;
        let ctx = ProcessContext::idle(SR);
        let mut last = 0.0f32;
        for (a, b) in l.chunks_mut(BLOCK).zip(r.chunks_mut(BLOCK)) {
            fx.process(a, b, &ctx);
            last = a.iter().fold(0.0f32, |m, x| m.max(x.abs()));
        }
        gain_to_db(last)
    }

    /// The drawn curve and the settled processor agree within 0.5 dB at every amplitude of `amps`.
    fn check(kind: &str, params: &[(&str, f32)], amps: &[f32]) {
        for &amp in amps {
            let mut fx = effect(kind, params);
            let measured = settled_out_db(fx.as_mut(), amp);
            let drawn = transfer_db(kind, get(kind, params), gain_to_db(amp)).expect("a dynamics curve");
            assert!((drawn - measured).abs() <= 0.5, "{kind} {params:?} amp {amp}: curve {drawn} dB, processor {measured} dB");
        }
    }

    #[test]
    fn compressor_curve_matches_the_processor() {
        let t = [("attack", 1.0), ("release", 50.0)];
        check("compressor", &[t[0], t[1], ("threshold", -20.0), ("ratio", 4.0), ("knee", 0.0)], &[0.5, 0.05]);
        check("compressor", &[t[0], t[1], ("threshold", -30.0), ("ratio", 8.0), ("knee", 6.0)], &[0.5, 0.0316, 0.01]);
        check("compressor", &[t[0], t[1], ("threshold", -10.0), ("ratio", 2.0), ("knee", 12.0)], &[0.708, 0.316, 0.1]);
        check("compressor", &[t[0], t[1], ("threshold", -24.0), ("ratio", 3.0), ("knee", 0.0), ("makeup", 6.0), ("mix", 0.5)], &[0.5]);
    }

    #[test]
    fn limiter_curve_matches_the_processor() {
        check("limiter", &[("gain", 0.0), ("ceiling", -6.0)], &[1.0, 0.1]);
        check("limiter", &[("gain", 12.0), ("ceiling", -1.0)], &[0.25]);
        check("limiter", &[("gain", 6.0), ("ceiling", -3.0), ("true_peak", 1.0)], &[0.5]);
    }

    #[test]
    fn noise_gate_curve_matches_the_processor() {
        let closed = [("threshold", -40.0), ("reduction", -30.0)];
        check("noise_gate", &closed, &[0.001, 0.1]);
        assert!((transfer_db("noise_gate", get("noise_gate", &closed), gain_to_db(0.001)).unwrap() + 90.0).abs() < 0.01);
        check("noise_gate", &[("threshold", -20.0), ("reduction", -10.0)], &[0.05, 0.5]);
    }

    #[test]
    fn de_esser_curve_matches_the_processor() {
        for mode in [0.0, 1.0] {
            check("de_esser", &[("freq", 4000.0), ("mode", mode), ("threshold", -30.0), ("range", 8.0)], &[0.5, 0.01]);
            check("de_esser", &[("freq", 4000.0), ("mode", mode), ("threshold", -20.0), ("range", 24.0)], &[0.5]);
        }
        let a = [("threshold", -30.0), ("range", 8.0)];
        assert!((transfer_db("de_esser", get("de_esser", &a), gain_to_db(0.5)).unwrap() + 14.0).abs() < 0.1);
        assert!((transfer_db("de_esser", get("de_esser", &a), gain_to_db(0.01)).unwrap() + 40.0).abs() < 0.1);
        let b = [("threshold", -20.0), ("range", 24.0)];
        assert!((transfer_db("de_esser", get("de_esser", &b), gain_to_db(0.5)).unwrap() + 20.0).abs() < 0.1);
    }

    #[test]
    fn transfer_db_knows_only_the_dynamics() {
        for kind in ["reverb", "channel_eq", "vst3"] {
            assert_eq!(transfer_db(kind, get(kind, &[]), -10.0), None, "{kind}");
        }
        for kind in ["compressor", "limiter", "noise_gate", "de_esser"] {
            assert!(transfer_db(kind, get(kind, &[]), -10.0).is_some(), "{kind}");
        }
    }

    #[test]
    fn dynamics_curves_clamp_like_the_processors_and_stay_finite() {
        let c = DynamicsCurve::of("compressor", get("compressor", &[("ratio", 0.5), ("makeup", 3.0)])).unwrap();
        assert!(matches!(c, DynamicsCurve::Compressor { ratio, .. } if ratio == 1.0), "{c:?}");
        for x in [-40.0, -10.0, 0.0] {
            assert!((c.out_db(x) - (x + 3.0)).abs() < 1e-3, "{x}: {}", c.out_db(x));
        }
        let wide = DynamicsCurve::of("de_esser", get("de_esser", &[("range", 40.0), ("threshold", -30.0)])).unwrap();
        let full = DynamicsCurve::of("de_esser", get("de_esser", &[("range", 24.0), ("threshold", -30.0)])).unwrap();
        assert_eq!(wide, full);
        for kind in ["compressor", "limiter", "noise_gate", "de_esser"] {
            let info = ondina_core::plugins::find(kind).unwrap();
            for pick_max in [false, true] {
                let ext: Vec<(&str, f32)> = info.params.iter().map(|p| (p.id, if pick_max { p.max } else { p.min })).collect();
                let c = DynamicsCurve::of(kind, get(kind, &ext)).unwrap();
                for k in -120..=24 {
                    let y = c.out_db(k as f32);
                    assert!(y.is_finite(), "{kind} max {pick_max} at {k} dB: {y}");
                }
                assert!(c.threshold_db().is_finite());
            }
        }
    }

    #[test]
    fn de_esser_band_follows_its_shelf() {
        assert!((de_esser_band_db(6000.0, 8.0, false, 16000.0, SR) + 8.0).abs() <= 0.5);
        assert!(de_esser_band_db(6000.0, 8.0, false, 200.0, SR).abs() <= 0.1);
        assert_eq!(de_esser_band_db(6000.0, 8.0, true, 200.0, SR), -8.0);
        assert_eq!(de_esser_band_db(6000.0, 8.0, true, 16000.0, SR), -8.0);
        assert_eq!(de_esser_band_db(6000.0, 0.0, false, 16000.0, SR), 0.0);
        assert_eq!(de_esser_band_db(6000.0, 0.0, true, 16000.0, SR), 0.0);
    }

    #[test]
    fn threshold_point_of_each_curve() {
        let thr = |kind: &str, p: &[(&str, f32)]| DynamicsCurve::of(kind, get(kind, p)).unwrap().threshold_db();
        assert_eq!(thr("compressor", &[("threshold", -22.0)]), -22.0);
        assert_eq!(thr("limiter", &[("gain", 6.0), ("ceiling", -1.0)]), -7.0);
        assert_eq!(thr("noise_gate", &[("threshold", -45.0)]), -45.0);
        assert_eq!(thr("de_esser", &[("threshold", -33.0)]), -33.0);
    }
}

#[cfg(test)]
mod saturation_tests {
    use super::*;
    use std::collections::BTreeMap;

    const SR: f32 = 48000.0;
    const BLOCK: usize = 480;

    /// An effect set up the way the engine does it: registry defaults, then `params`.
    fn effect(kind: &str, params: &[(&str, f32)]) -> Box<dyn Effect> {
        let mut fx = crate::create_effect(kind, SR).expect("effect in the registry");
        let map: BTreeMap<String, f32> = params.iter().map(|(k, v)| (k.to_string(), *v)).collect();
        crate::configure(kind, &map, |k, v| fx.set_param(k, v));
        fx
    }

    /// The left channel of `input` through `fx`, in blocks of BLOCK.
    fn render(fx: &mut dyn Effect, input: &[f32]) -> Vec<f32> {
        let mut l = input.to_vec();
        let mut r = input.to_vec();
        let ctx = ProcessContext::idle(SR);
        for (a, b) in l.chunks_mut(BLOCK).zip(r.chunks_mut(BLOCK)) {
            fx.process(a, b, &ctx);
        }
        l
    }

    fn sine(freq: f32, amp: f32, len: usize) -> Vec<f32> {
        (0..len).map(|n| amp * (std::f32::consts::TAU * freq * n as f32 / SR).sin()).collect()
    }

    #[test]
    fn saturation_processors_sound_as_before() {
        // The processors as they computed before the shared shapes, copied here so a change of their sound shows up.
        fn shape(mode: u8, x: f32) -> f32 {
            match mode {
                1 => x.clamp(-1.0, 1.0),
                2 => {
                    let y = x.signum() * (1.0 - (-x.abs() * 2.0).exp());
                    (y + 0.2 * y * y).clamp(-1.0, 1.0)
                }
                3 => {
                    if x >= 0.0 { x.tanh() } else { (x * 0.7).tanh() * 1.1 }
                }
                _ => x.tanh(),
            }
        }
        fn overdrive_reference(mode: u8, drive: f32, tone: f32, output_db: f32, mix: f32, input: &[f32]) -> Vec<f32> {
            let output = db_to_gain(output_db);
            let mut lp = OnePole::default();
            let mut dc = DcBlock::default();
            let mut out = Vec::with_capacity(input.len());
            for block in input.chunks(BLOCK) {
                let pre = 1.0 + drive * drive * 60.0;
                let cut = 800.0 * 2f32.powf(tone * 4.0);
                lp.set_freq(cut, SR);
                let comp = 1.0 / (1.0 + drive * 2.0);
                for &v in block {
                    let a = dc.process(lp.process(shape(mode, v * pre)));
                    out.push(v * (1.0 - mix) + a * mix * output * (0.5 + comp));
                }
            }
            out
        }
        fn bitcrusher_reference(bits: f32, down: f32, mix: f32, input: &[f32]) -> Vec<f32> {
            let down = down.max(1.0);
            let (mut count, mut hold) = (0.0f32, 0.0f32);
            let q = 2f32.powf(bits - 1.0);
            input
                .iter()
                .map(|&v| {
                    count += 1.0;
                    if count >= down {
                        count -= down;
                        hold = (v * q).round() / q;
                    }
                    v * (1.0 - mix) + hold * mix
                })
                .collect()
        }
        fn exciter_reference(freq: f32, amount: f32, mix: f32, input: &[f32]) -> Vec<f32> {
            let mut hp1 = Biquad::new(BiquadKind::HighPass, freq, 0.707, 0.0, SR);
            let mut hp2 = Biquad::new(BiquadKind::HighPass, freq, 0.707, 0.0, SR);
            let d = 1.0 + amount * 10.0;
            input
                .iter()
                .map(|&v| {
                    let h = hp2.process((hp1.process(v) * d).tanh());
                    v + h * mix * 0.5
                })
                .collect()
        }

        let input: Vec<f32> = sine(440.0, 0.9, 1500).iter().zip(sine(3000.0, 0.2, 1500)).map(|(a, b)| a + b).collect();
        for mode in 0..=3u8 {
            for (drive, tone, output, mix) in [(0.4, 0.5, -3.0, 1.0), (0.9, 0.8, 6.0, 0.6)] {
                let mut fx = effect("overdrive", &[("mode", mode as f32), ("drive", drive), ("tone", tone), ("output", output), ("mix", mix)]);
                assert_eq!(render(fx.as_mut(), &input), overdrive_reference(mode, drive, tone, output, mix, &input), "overdrive mode {mode} drive {drive}");
            }
        }
        for (bits, down, mix) in [(5.0, 3.0, 0.8), (8.0, 1.0, 1.0)] {
            let mut fx = effect("bitcrusher", &[("bits", bits), ("downsample", down), ("mix", mix)]);
            assert_eq!(render(fx.as_mut(), &input), bitcrusher_reference(bits, down, mix, &input), "bitcrusher {bits} bits");
        }
        for (freq, amount, mix) in [(3000.0, 0.7, 0.4), (1000.0, 0.2, 1.0)] {
            let mut fx = effect("exciter", &[("freq", freq), ("amount", amount), ("mix", mix)]);
            assert_eq!(render(fx.as_mut(), &input), exciter_reference(freq, amount, mix, &input), "exciter {freq} Hz");
        }
    }
}
