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
}

impl Limiter {
    pub fn new(sr: f32) -> Self {
        let look = (sr * 0.0015) as usize;
        Limiter { sr, gain: 1.0, ceiling: db_to_gain(-1.0), release: 60.0, look, dl: VecDeque::from(vec![0.0; look]), dr: VecDeque::from(vec![0.0; look]), targets: VecDeque::new(), mins: VecDeque::new(), counter: 0, current: 1.0 }
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
            self.dl.push_back(a);
            self.dr.push_back(b);
            let da = self.dl.pop_front().unwrap_or(0.0);
            let db = self.dr.pop_front().unwrap_or(0.0);
            let c = self.ceiling;
            l[i] = (da * self.current).clamp(-c, c);
            r[i] = (db * self.current).clamp(-c, c);
        }
        let _ = &self.targets;
    }
    fn reset(&mut self) {
        self.dl.iter_mut().for_each(|x| *x = 0.0);
        self.dr.iter_mut().for_each(|x| *x = 0.0);
        self.mins.clear();
        self.current = 1.0;
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
}

impl NoiseGate {
    pub fn new(sr: f32) -> Self {
        NoiseGate { sr, threshold: -50.0, reduction: -60.0, attack: 1.0, hold: 20.0, release: 100.0, env: 0.0, gain: 0.0, hold_count: 0 }
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
            l[i] *= self.gain;
            r[i] *= self.gain;
        }
    }
    fn reset(&mut self) {
        self.env = 0.0;
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
