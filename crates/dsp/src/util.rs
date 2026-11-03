//! DSP building blocks.

use std::f32::consts::PI;

#[inline]
pub fn db_to_gain(db: f32) -> f32 {
    if db <= -96.0 {
        0.0
    } else {
        10f32.powf(db / 20.0)
    }
}

#[inline]
pub fn gain_to_db(g: f32) -> f32 {
    if g <= 1e-6 {
        -120.0
    } else {
        20.0 * g.log10()
    }
}

#[inline]
pub fn midi_to_freq(note: f32) -> f32 {
    440.0 * 2f32.powf((note - 69.0) / 12.0)
}

/// Equal-power pan law: returns (left, right) gains for pan -1..1.
#[inline]
pub fn pan_gains(pan: f32) -> (f32, f32) {
    let p = (pan.clamp(-1.0, 1.0) + 1.0) * 0.25 * PI;
    (p.cos() * std::f32::consts::SQRT_2, p.sin() * std::f32::consts::SQRT_2)
}

/// Fast xorshift noise in -1..1.
#[derive(Clone)]
pub struct Noise(u32);

impl Noise {
    pub fn new(seed: u32) -> Self {
        Noise(seed.max(1))
    }
    #[inline]
    pub fn next(&mut self) -> f32 {
        let mut x = self.0;
        x ^= x << 13;
        x ^= x >> 17;
        x ^= x << 5;
        self.0 = x;
        (x as f32 / u32::MAX as f32) * 2.0 - 1.0
    }
}

/// Sine lookup table for cheap additive synthesis.
pub struct SineTable {
    table: Vec<f32>,
}

impl SineTable {
    pub fn new() -> Self {
        let n = 4096;
        SineTable { table: (0..=n).map(|i| (i as f32 / n as f32 * 2.0 * PI).sin()).collect() }
    }
    /// phase in 0..1
    #[inline]
    pub fn get(&self, phase: f32) -> f32 {
        let n = (self.table.len() - 1) as f32;
        let p = phase.fract() * n;
        let i = p as usize;
        let f = p - i as f32;
        self.table[i] + (self.table[i + 1] - self.table[i]) * f
    }
}

impl Default for SineTable {
    fn default() -> Self {
        Self::new()
    }
}

#[inline]
fn poly_blep(t: f32, dt: f32) -> f32 {
    if t < dt {
        let t = t / dt;
        t + t - t * t - 1.0
    } else if t > 1.0 - dt {
        let t = (t - 1.0) / dt;
        t * t + t + t + 1.0
    } else {
        0.0
    }
}

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Wave {
    Saw,
    Square,
    Triangle,
    Sine,
    Noise,
}

impl Wave {
    pub fn from_index(i: f32) -> Wave {
        match i.round() as i32 {
            1 => Wave::Square,
            2 => Wave::Triangle,
            3 => Wave::Sine,
            4 => Wave::Noise,
            _ => Wave::Saw,
        }
    }
}

/// Band-limited (PolyBLEP) oscillator.
#[derive(Clone)]
pub struct Osc {
    pub phase: f32,
    tri_state: f32,
}

impl Default for Osc {
    fn default() -> Self {
        Osc { phase: 0.0, tri_state: 0.0 }
    }
}

impl Osc {
    #[inline]
    pub fn next(&mut self, wave: Wave, inc: f32, pw: f32, noise: &mut Noise) -> f32 {
        let t = self.phase;
        let dt = inc.min(0.5);
        let out = match wave {
            Wave::Saw => 2.0 * t - 1.0 - poly_blep(t, dt),
            Wave::Square => {
                let mut v = if t < pw { 1.0 } else { -1.0 };
                v += poly_blep(t, dt);
                v -= poly_blep((t - pw + 1.0).fract(), dt);
                v
            }
            Wave::Triangle => {
                // leaky-integrated band-limited square
                let mut sq = if t < 0.5 { 1.0 } else { -1.0 };
                sq += poly_blep(t, dt);
                sq -= poly_blep((t + 0.5).fract(), dt);
                self.tri_state = dt * 4.0 * sq + (1.0 - dt * 0.05) * self.tri_state;
                self.tri_state
            }
            Wave::Sine => (t * 2.0 * PI).sin(),
            Wave::Noise => noise.next(),
        };
        self.phase += inc;
        if self.phase >= 1.0 {
            self.phase -= 1.0;
        }
        out
    }
}

/// ADSR envelope with linear attack and exponential decay/release.
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum EnvStage {
    Idle,
    Attack,
    Decay,
    Sustain,
    Release,
}

#[derive(Clone)]
pub struct Adsr {
    pub stage: EnvStage,
    pub level: f32,
    attack_inc: f32,
    decay_coef: f32,
    release_coef: f32,
    pub sustain: f32,
}

impl Default for Adsr {
    fn default() -> Self {
        Adsr { stage: EnvStage::Idle, level: 0.0, attack_inc: 1.0, decay_coef: 0.001, release_coef: 0.001, sustain: 1.0 }
    }
}

fn exp_coef(time_s: f32, sr: f32) -> f32 {
    // reaches ~99% in `time_s`
    1.0 - (-4.6 / (time_s.max(0.0005) * sr)).exp()
}

impl Adsr {
    pub fn set(&mut self, a: f32, d: f32, s: f32, r: f32, sr: f32) {
        self.attack_inc = 1.0 / (a.max(0.0005) * sr);
        self.decay_coef = exp_coef(d, sr);
        self.release_coef = exp_coef(r, sr);
        self.sustain = s.clamp(0.0, 1.0);
    }
    pub fn trigger(&mut self) {
        self.stage = EnvStage::Attack;
    }
    pub fn release(&mut self) {
        if self.stage != EnvStage::Idle {
            self.stage = EnvStage::Release;
        }
    }
    pub fn kill(&mut self) {
        self.stage = EnvStage::Idle;
        self.level = 0.0;
    }
    pub fn is_active(&self) -> bool {
        self.stage != EnvStage::Idle
    }
    #[inline]
    pub fn next(&mut self) -> f32 {
        match self.stage {
            EnvStage::Idle => {}
            EnvStage::Attack => {
                self.level += self.attack_inc;
                if self.level >= 1.0 {
                    self.level = 1.0;
                    self.stage = EnvStage::Decay;
                }
            }
            EnvStage::Decay => {
                self.level += (self.sustain - self.level) * self.decay_coef;
                if (self.level - self.sustain).abs() < 1e-4 {
                    self.level = self.sustain;
                    self.stage = EnvStage::Sustain;
                }
            }
            EnvStage::Sustain => self.level = self.sustain,
            EnvStage::Release => {
                self.level -= self.level * self.release_coef;
                if self.level < 1e-4 {
                    self.level = 0.0;
                    self.stage = EnvStage::Idle;
                }
            }
        }
        self.level
    }
}

/// Topology-preserving-transform state variable filter (Cytomic).
#[derive(Clone, Default)]
pub struct Svf {
    ic1: f32,
    ic2: f32,
    a1: f32,
    a2: f32,
    a3: f32,
    k: f32,
}

#[derive(Clone, Copy, PartialEq, Eq)]
pub enum FilterMode {
    Low,
    High,
    Band,
}

impl FilterMode {
    pub fn from_index(i: f32) -> Self {
        match i.round() as i32 {
            1 => FilterMode::High,
            2 => FilterMode::Band,
            _ => FilterMode::Low,
        }
    }
}

impl Svf {
    /// `res` 0..1 (1 = near self-oscillation)
    pub fn set(&mut self, cutoff: f32, res: f32, sr: f32) {
        let fc = cutoff.clamp(10.0, sr * 0.49);
        let g = (PI * fc / sr).tan();
        self.k = 2.0 - 1.98 * res.clamp(0.0, 1.0);
        self.a1 = 1.0 / (1.0 + g * (g + self.k));
        self.a2 = g * self.a1;
        self.a3 = g * self.a2;
    }
    #[inline]
    pub fn process(&mut self, v0: f32, mode: FilterMode) -> f32 {
        let v3 = v0 - self.ic2;
        let v1 = self.a1 * self.ic1 + self.a2 * v3;
        let v2 = self.ic2 + self.a2 * self.ic1 + self.a3 * v3;
        self.ic1 = 2.0 * v1 - self.ic1;
        self.ic2 = 2.0 * v2 - self.ic2;
        match mode {
            FilterMode::Low => v2,
            FilterMode::Band => v1,
            FilterMode::High => v0 - self.k * v1 - v2,
        }
    }
    pub fn reset(&mut self) {
        self.ic1 = 0.0;
        self.ic2 = 0.0;
    }
}

/// Floor of `Biquad::magnitude_db`: a filter's deep stop band reads as this instead of minus infinity.
pub const MIN_RESPONSE_DB: f32 = -120.0;

/// RBJ biquad.
#[derive(Clone, Copy, Default)]
pub struct Biquad {
    b0: f32,
    b1: f32,
    b2: f32,
    a1: f32,
    a2: f32,
    z1: f32,
    z2: f32,
}

#[derive(Clone, Copy)]
pub enum BiquadKind {
    LowPass,
    HighPass,
    BandPass,
    Peak,
    LowShelf,
    HighShelf,
}

impl Biquad {
    pub fn new(kind: BiquadKind, freq: f32, q: f32, gain_db: f32, sr: f32) -> Self {
        let mut b = Biquad::default();
        b.set(kind, freq, q, gain_db, sr);
        b
    }

    pub fn set(&mut self, kind: BiquadKind, freq: f32, q: f32, gain_db: f32, sr: f32) {
        let f = freq.clamp(5.0, sr * 0.49);
        let w0 = 2.0 * PI * f / sr;
        let (sw, cw) = w0.sin_cos();
        let q = q.max(0.05);
        let alpha = sw / (2.0 * q);
        let a = 10f32.powf(gain_db / 40.0);
        let (b0, b1, b2, a0, a1, a2) = match kind {
            BiquadKind::LowPass => ((1.0 - cw) / 2.0, 1.0 - cw, (1.0 - cw) / 2.0, 1.0 + alpha, -2.0 * cw, 1.0 - alpha),
            BiquadKind::HighPass => ((1.0 + cw) / 2.0, -(1.0 + cw), (1.0 + cw) / 2.0, 1.0 + alpha, -2.0 * cw, 1.0 - alpha),
            BiquadKind::BandPass => (alpha, 0.0, -alpha, 1.0 + alpha, -2.0 * cw, 1.0 - alpha),
            BiquadKind::Peak => (1.0 + alpha * a, -2.0 * cw, 1.0 - alpha * a, 1.0 + alpha / a, -2.0 * cw, 1.0 - alpha / a),
            BiquadKind::LowShelf => {
                let sa = 2.0 * a.sqrt() * alpha;
                (a * ((a + 1.0) - (a - 1.0) * cw + sa), 2.0 * a * ((a - 1.0) - (a + 1.0) * cw), a * ((a + 1.0) - (a - 1.0) * cw - sa), (a + 1.0) + (a - 1.0) * cw + sa, -2.0 * ((a - 1.0) + (a + 1.0) * cw), (a + 1.0) + (a - 1.0) * cw - sa)
            }
            BiquadKind::HighShelf => {
                let sa = 2.0 * a.sqrt() * alpha;
                (a * ((a + 1.0) + (a - 1.0) * cw + sa), -2.0 * a * ((a - 1.0) + (a + 1.0) * cw), a * ((a + 1.0) + (a - 1.0) * cw - sa), (a + 1.0) - (a - 1.0) * cw + sa, 2.0 * ((a - 1.0) - (a + 1.0) * cw), (a + 1.0) - (a - 1.0) * cw - sa)
            }
        };
        self.b0 = b0 / a0;
        self.b1 = b1 / a0;
        self.b2 = b2 / a0;
        self.a1 = a1 / a0;
        self.a2 = a2 / a0;
    }

    #[inline]
    pub fn process(&mut self, x: f32) -> f32 {
        let y = self.b0 * x + self.z1;
        self.z1 = self.b1 * x - self.a1 * y + self.z2;
        self.z2 = self.b2 * x - self.a2 * y;
        y
    }

    pub fn reset(&mut self) {
        self.z1 = 0.0;
        self.z2 = 0.0;
    }

    /// Gain in dB of the filter at `freq` Hz for sample rate `sr`: the magnitude of its transfer function on the unit
    /// circle, never below `MIN_RESPONSE_DB`. For drawing a response; not meant for the audio thread.
    pub fn magnitude_db(&self, freq: f32, sr: f32) -> f32 {
        if sr <= 0.0 || !sr.is_finite() {
            return 0.0;
        }
        let sr = sr as f64;
        let f = (freq as f64).clamp(0.0, sr / 2.0);
        let w = 2.0 * std::f64::consts::PI * f / sr;
        let (b0, b1, b2, a1, a2) = (self.b0 as f64, self.b1 as f64, self.b2 as f64, self.a1 as f64, self.a2 as f64);
        let (s1, c1) = w.sin_cos();
        let (s2, c2) = (2.0 * w).sin_cos();
        let nr = b0 + b1 * c1 + b2 * c2;
        let ni = -(b1 * s1 + b2 * s2);
        let dr = 1.0 + a1 * c1 + a2 * c2;
        let di = -(a1 * s1 + a2 * s2);
        let num = nr * nr + ni * ni;
        let den = (dr * dr + di * di).max(1e-300);
        let db = 10.0 * (num / den).max(1e-30).log10();
        (db as f32).max(MIN_RESPONSE_DB)
    }
}

/// One-pole low-pass, also used as parameter smoother.
#[derive(Clone, Copy, Default)]
pub struct OnePole {
    pub z: f32,
    pub a: f32,
}

impl OnePole {
    pub fn set_freq(&mut self, freq: f32, sr: f32) {
        self.a = 1.0 - (-2.0 * PI * freq.clamp(1.0, sr * 0.49) / sr).exp();
    }
    pub fn set_time(&mut self, seconds: f32, sr: f32) {
        self.a = 1.0 - (-1.0 / (seconds.max(1e-5) * sr)).exp();
    }
    #[inline]
    pub fn process(&mut self, x: f32) -> f32 {
        self.z += (x - self.z) * self.a;
        self.z
    }
}

/// Fractional delay line.
#[derive(Clone)]
pub struct DelayLine {
    buf: Vec<f32>,
    pos: usize,
}

impl DelayLine {
    pub fn new(max_samples: usize) -> Self {
        DelayLine { buf: vec![0.0; max_samples.max(4)], pos: 0 }
    }
    pub fn len(&self) -> usize {
        self.buf.len()
    }
    pub fn is_empty(&self) -> bool {
        self.buf.is_empty()
    }
    #[inline]
    pub fn write(&mut self, x: f32) {
        self.buf[self.pos] = x;
        self.pos += 1;
        if self.pos >= self.buf.len() {
            self.pos = 0;
        }
    }
    /// Read `delay` samples behind the last write (fractional, linear interpolation).
    #[inline]
    pub fn read(&self, delay: f32) -> f32 {
        let n = self.buf.len();
        let d = delay.clamp(1.0, (n - 2) as f32);
        let rp = self.pos as f32 - d;
        let rp = if rp < 0.0 { rp + n as f32 } else { rp };
        let i = rp as usize;
        let f = rp - i as f32;
        let a = self.buf[i % n];
        let b = self.buf[(i + 1) % n];
        a + (b - a) * f
    }
    pub fn clear(&mut self) {
        self.buf.iter_mut().for_each(|x| *x = 0.0);
    }
}

/// DC blocking filter.
#[derive(Clone, Copy, Default)]
pub struct DcBlock {
    x1: f32,
    y1: f32,
}

impl DcBlock {
    #[inline]
    pub fn process(&mut self, x: f32) -> f32 {
        let y = x - self.x1 + 0.995 * self.y1;
        self.x1 = x;
        self.y1 = y;
        y
    }
}

/// True-peak meter in the manner of ITU-R BS.1770: 4x oversampling with a 48-tap windowed-sinc interpolator, 12 taps
/// per phase. Fixed state and no allocation after `new`, so the limiter can run it on the audio thread.
#[derive(Clone, Copy, Debug)]
pub struct TruePeak {
    /// Phases 1, 2 and 3 (a quarter, half and three quarters of a sample); phase 0 is the sample itself.
    coef: [[f32; TruePeak::TAPS]; 3],
    /// The last TAPS input samples, newest first.
    hist: [f32; TruePeak::TAPS],
    /// No interpolated point can exceed this times the largest sample in `hist` (largest sum of |coef| of a phase,
    /// with a hair of room for f32 rounding).
    bound: f32,
}

impl TruePeak {
    const TAPS: usize = 12;
    /// How many samples ago the segment that `push` measures starts: the detector runs this late.
    pub const DELAY: usize = 6;

    pub fn new() -> Self {
        let half = Self::DELAY as f64;
        let mut coef = [[0.0f32; Self::TAPS]; 3];
        let mut bound = 1.0f32;
        for (p, row) in coef.iter_mut().enumerate() {
            let frac = (p + 1) as f64 / 4.0;
            let mut c = [0.0f64; Self::TAPS];
            for (k, ck) in c.iter_mut().enumerate() {
                // distance in samples between hist[k] and the point interpolated DELAY - frac samples ago
                let d = k as f64 - half + frac;
                let x = std::f64::consts::PI * d;
                let sinc = if d.abs() < 1e-12 { 1.0 } else { x.sin() / x };
                let window = if d.abs() < half { 0.5 * (1.0 + (x / half).cos()) } else { 0.0 };
                *ck = sinc * window;
            }
            let sum: f64 = c.iter().sum();
            for (dst, src) in row.iter_mut().zip(c) {
                *dst = (src / sum) as f32;
            }
            bound = bound.max(row.iter().map(|x| x.abs()).sum::<f32>() * 1.001);
        }
        TruePeak { coef, hist: [0.0; Self::TAPS], bound }
    }

    /// Feeds one sample. Returns the largest absolute value of the segment that starts DELAY samples ago: that sample
    /// and the three points interpolated after it.
    #[inline]
    pub fn push(&mut self, x: f32) -> f32 {
        self.hist.copy_within(0..Self::TAPS - 1, 1);
        self.hist[0] = x;
        self.segment_peak()
    }

    /// `push` for a running maximum: when no point of the segment can go above `floor` it skips the interpolation and
    /// returns the sample alone. The running maximum comes out the same as with `push`.
    #[inline]
    pub fn push_above(&mut self, x: f32, floor: f32) -> f32 {
        self.hist.copy_within(0..Self::TAPS - 1, 1);
        self.hist[0] = x;
        let largest = self.hist.iter().fold(0.0f32, |m, h| m.max(h.abs()));
        if largest * self.bound <= floor {
            self.hist[Self::DELAY].abs()
        } else {
            self.segment_peak()
        }
    }

    fn segment_peak(&self) -> f32 {
        let mut m = self.hist[Self::DELAY].abs();
        for row in &self.coef {
            let y: f32 = row.iter().zip(&self.hist).map(|(c, h)| c * h).sum();
            m = m.max(y.abs());
        }
        m
    }

    pub fn reset(&mut self) {
        self.hist = [0.0; Self::TAPS];
    }
}

impl Default for TruePeak {
    fn default() -> Self {
        Self::new()
    }
}

/// True peak of a buffer (linear): the largest of its samples and of the points TruePeak interpolates between them.
/// The last DELAY samples count with their own value, as the analyzer counts them.
pub fn true_peak(samples: &[f32]) -> f32 {
    let mut tp = TruePeak::new();
    let mut m = 0.0f32;
    for &x in samples {
        m = m.max(tp.push_above(x, m)).max(x.abs());
    }
    m
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn biquad_magnitude_matches_known_points() {
        let sr = 48000.0;
        let peak = Biquad::new(BiquadKind::Peak, 1000.0, 1.0, 6.0, sr);
        assert!((peak.magnitude_db(1000.0, sr) - 6.0).abs() <= 0.05, "{}", peak.magnitude_db(1000.0, sr));
        assert!(peak.magnitude_db(20.0, sr).abs() <= 0.1);
        assert!(peak.magnitude_db(20000.0, sr).abs() <= 0.1);
        let lp = Biquad::new(BiquadKind::LowPass, 1000.0, 0.707, 0.0, sr);
        assert!((lp.magnitude_db(1000.0, sr) + 3.01).abs() <= 0.05, "{}", lp.magnitude_db(1000.0, sr));
        assert!(lp.magnitude_db(100.0, sr).abs() <= 0.05);
        assert!(lp.magnitude_db(10000.0, sr) < -35.0);
        let hs = Biquad::new(BiquadKind::HighShelf, 2000.0, 0.707, 6.0, sr);
        assert!((hs.magnitude_db(18000.0, sr) - 6.0).abs() <= 0.2, "{}", hs.magnitude_db(18000.0, sr));
        assert!(hs.magnitude_db(50.0, sr).abs() <= 0.1);
        let hp = Biquad::new(BiquadKind::HighPass, 1000.0, 0.707, 0.0, sr);
        let low = hp.magnitude_db(5.0, sr);
        assert!((MIN_RESPONSE_DB..-60.0).contains(&low), "{low}");
    }

    #[test]
    fn biquad_magnitude_is_finite_everywhere() {
        let kinds = [BiquadKind::LowPass, BiquadKind::HighPass, BiquadKind::BandPass, BiquadKind::Peak, BiquadKind::LowShelf, BiquadKind::HighShelf];
        for kind in kinds {
            for sr in [44100.0, 48000.0, 96000.0] {
                let b = Biquad::new(kind, 1000.0, 0.707, 6.0, sr);
                for f in [0.0, 1.0, 20.0, 1000.0, 23999.0, 24000.0, 30000.0, -5.0] {
                    let db = b.magnitude_db(f, sr);
                    assert!(db.is_finite() && db >= MIN_RESPONSE_DB, "{f} Hz at {sr}: {db}");
                }
            }
        }
        let b = Biquad::new(BiquadKind::Peak, 1000.0, 1.0, 6.0, 48000.0);
        assert_eq!(b.magnitude_db(1000.0, 0.0), 0.0);
    }

    #[test]
    fn adsr_cycle() {
        let mut e = Adsr::default();
        e.set(0.01, 0.05, 0.5, 0.05, 1000.0);
        e.trigger();
        for _ in 0..200 {
            e.next();
        }
        assert!((e.level - 0.5).abs() < 0.01);
        e.release();
        for _ in 0..500 {
            e.next();
        }
        assert!(!e.is_active());
    }

    #[test]
    fn biquad_lowpass_attenuates() {
        let sr = 48000.0;
        let mut b = Biquad::new(BiquadKind::LowPass, 500.0, 0.707, 0.0, sr);
        let mut peak: f32 = 0.0;
        for i in 0..4800 {
            let x = (i as f32 * 2.0 * PI * 8000.0 / sr).sin();
            let y = b.process(x);
            if i > 1000 {
                peak = peak.max(y.abs());
            }
        }
        assert!(peak < 0.05);
    }

    #[test]
    fn true_peak_finds_the_peak_between_samples() {
        use std::f32::consts::PI;
        // fs/4 shifted by 45 degrees: every sample sits at 0.707 of the real peak
        let x: Vec<f32> = (0..4000).map(|n| (PI * n as f32 / 2.0 + PI / 4.0).sin()).collect();
        let mut tp = TruePeak::new();
        let mut m = 0.0f32;
        for (n, &s) in x.iter().enumerate() {
            let p = tp.push(s);
            if n >= 100 {
                m = m.max(p);
            }
        }
        assert!(m >= db_to_gain(-0.2) && m <= db_to_gain(0.1), "push read {m}");
        let whole = true_peak(&x);
        assert!(whole >= db_to_gain(-0.2), "true_peak read {whole}");
    }

    #[test]
    fn true_peak_does_not_overread_a_low_sine() {
        use std::f32::consts::PI;
        let x: Vec<f32> = (0..44100).map(|n| 0.5 * (2.0 * PI * 997.0 * n as f32 / 44100.0).sin()).collect();
        let sample_peak = x.iter().fold(0.0f32, |m, s| m.max(s.abs()));
        let tp = true_peak(&x);
        assert!(tp <= 0.5 * db_to_gain(0.1), "true peak {tp}");
        assert!(tp >= sample_peak, "true peak {tp} under the sample peak {sample_peak}");
    }

    #[test]
    fn true_peak_phase_zero_is_the_sample() {
        let mut tp = TruePeak::new();
        let mut last = tp.push(0.5);
        for _ in 0..TruePeak::DELAY {
            last = tp.push(0.0);
        }
        assert_eq!(last, 0.5);
        assert_eq!(true_peak(&[0.0, -0.8, 0.0]), 0.8);
    }

    #[test]
    fn push_above_keeps_the_running_maximum_of_push() {
        let mut noise = Noise::new(5);
        let n = 20000;
        let x: Vec<f32> = (0..n)
            .map(|i| {
                let ramp = if i < n / 2 { i as f32 / (n / 2) as f32 } else { 1.0 - 0.9 * (i - n / 2) as f32 / (n / 2) as f32 };
                noise.next() * ramp
            })
            .collect();
        let (mut full, mut skip) = (TruePeak::new(), TruePeak::new());
        let (mut a, mut b) = (0.0f32, 0.0f32);
        for &s in &x {
            a = a.max(full.push(s));
            b = b.max(skip.push_above(s, b));
        }
        assert_eq!(a, b);
    }
}
