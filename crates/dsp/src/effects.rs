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

impl ChannelEq {
    pub fn new(sr: f32) -> Self {
        ChannelEq { sr, p: [0.0, 80.0, 0.0, 120.0, 0.0, 400.0, 1.0, 0.0, 2500.0, 1.0, 0.0, 8000.0, 0.0, 16000.0, 0.0], bands_l: [Biquad::default(); 6], bands_r: [Biquad::default(); 6], dirty: true }
    }
    fn update(&mut self) {
        let p = &self.p;
        let sr = self.sr;
        let specs = [
            (BiquadKind::HighPass, p[1], 0.707, 0.0),
            (BiquadKind::LowShelf, p[3], 0.707, p[2]),
            (BiquadKind::Peak, p[5], p[6], p[4]),
            (BiquadKind::Peak, p[8], p[9], p[7]),
            (BiquadKind::HighShelf, p[11], 0.707, p[10]),
            (BiquadKind::LowPass, p[13], 0.707, 0.0),
        ];
        for (i, (k, f, q, g)) in specs.iter().enumerate() {
            self.bands_l[i].set(*k, *f, *q, *g, sr);
            self.bands_r[i].set(*k, *f, *q, *g, sr);
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
        let active = [self.p[0] >= 0.5, self.p[2] != 0.0, self.p[4] != 0.0, self.p[7] != 0.0, self.p[10] != 0.0, self.p[12] >= 0.5];
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
        let (t, r, k) = (self.threshold, self.ratio, self.knee);
        let over = x_db - t;
        if k > 0.0 && over.abs() <= k / 2.0 {
            x_db + (1.0 / r - 1.0) * (over + k / 2.0).powi(2) / (2.0 * k)
        } else if over > 0.0 {
            t + over / r
        } else {
            x_db
        }
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

pub struct Limiter {
    sr: f32,
    gain: f32,
    ceiling: f32,
    release: f32,
    look: usize,
    dl: VecDeque<f32>,
    dr: VecDeque<f32>,
    targets: VecDeque<f32>,
    mins: VecDeque<(usize, f32)>,
    counter: usize,
    current: f32,
    /// Deepest reduction of the last block in dB (<= 0).
    gain_reduction_db: f32,
}

impl Limiter {
    pub fn new(sr: f32) -> Self {
        let look = (sr * 0.0015) as usize;
        Limiter { sr, gain: 1.0, ceiling: db_to_gain(-1.0), release: 60.0, look, dl: VecDeque::from(vec![0.0; look]), dr: VecDeque::from(vec![0.0; look]), targets: VecDeque::new(), mins: VecDeque::new(), counter: 0, current: 1.0, gain_reduction_db: 0.0 }
    }
}

impl Effect for Limiter {
    fn set_param(&mut self, id: &str, v: f32) {
        match id {
            "gain" => self.gain = db_to_gain(v),
            "ceiling" => self.ceiling = db_to_gain(v),
            "release" => self.release = v,
            _ => {}
        }
    }
    fn process(&mut self, l: &mut [f32], r: &mut [f32], _ctx: &ProcessContext) {
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
            self.dl.push_back(a);
            self.dr.push_back(b);
            let da = self.dl.pop_front().unwrap_or(0.0);
            let db = self.dr.pop_front().unwrap_or(0.0);
            let c = self.ceiling;
            l[i] = (da * self.current).clamp(-c, c);
            r[i] = (db * self.current).clamp(-c, c);
        }
        let _ = &self.targets;
        self.gain_reduction_db = gain_to_db(min_gain).min(0.0);
    }
    fn reset(&mut self) {
        self.dl.iter_mut().for_each(|x| *x = 0.0);
        self.dr.iter_mut().for_each(|x| *x = 0.0);
        self.mins.clear();
        self.current = 1.0;
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
        match self.kind {
            1 => (BiquadKind::LowShelf, DEQ_SHELF_Q),
            2 => (BiquadKind::HighShelf, DEQ_SHELF_Q),
            _ => (BiquadKind::Peak, self.q),
        }
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
