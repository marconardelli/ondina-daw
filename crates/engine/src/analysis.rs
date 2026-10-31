//! Streaming loudness / spectrum analyzer (ITU-R BS.1770 style K-weighting with gating).

use aurora_dsp::util::{gain_to_db, Biquad, BiquadKind};
use serde::{Deserialize, Serialize};

pub struct Analyzer {
    sr: f32,
    k1: [Biquad; 2],
    k2: [Biquad; 2],
    low: [Biquad; 2],
    high: [Biquad; 2],
    sub_len: usize,
    sub_acc: f64,
    sub_count: usize,
    subs: Vec<f64>,
    peak: f32,
    sum_sq: f64,
    n: u64,
    low_e: f64,
    high_e: f64,
    sum_lr: f64,
    sum_ll: f64,
    sum_rr: f64,
    prev: [f32; 2],
    true_peak: f32,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AnalysisReport {
    pub integrated_lufs: f32,
    pub short_term_max_lufs: f32,
    pub loudness_range_lu: f32,
    pub peak_dbfs: f32,
    pub true_peak_dbtp: f32,
    pub rms_dbfs: f32,
    pub crest_factor_db: f32,
    pub stereo_correlation: f32,
    /// Share of energy below 250 Hz / 250 Hz–4 kHz / above 4 kHz, in percent.
    pub spectral_balance_pct: [f32; 3],
    pub clipping: bool,
    pub silent: bool,
}

impl Analyzer {
    pub fn new(sr: f32) -> Self {
        let mk = |k, f, q, g| Biquad::new(k, f, q, g, sr);
        Analyzer {
            sr,
            k1: [mk(BiquadKind::HighShelf, 1681.0, 0.707, 4.0); 2],
            k2: [mk(BiquadKind::HighPass, 38.0, 0.5, 0.0); 2],
            low: [mk(BiquadKind::LowPass, 250.0, 0.707, 0.0); 2],
            high: [mk(BiquadKind::HighPass, 4000.0, 0.707, 0.0); 2],
            sub_len: (sr * 0.1) as usize,
            sub_acc: 0.0,
            sub_count: 0,
            subs: vec![],
            peak: 0.0,
            sum_sq: 0.0,
            n: 0,
            low_e: 0.0,
            high_e: 0.0,
            sum_lr: 0.0,
            sum_ll: 0.0,
            sum_rr: 0.0,
            prev: [0.0; 2],
            true_peak: 0.0,
        }
    }

    pub fn push(&mut self, l: &[f32], r: &[f32]) {
        for i in 0..l.len() {
            let x = [l[i], r[i]];
            let mut k = 0.0f64;
            for c in 0..2 {
                let a = x[c].abs();
                self.peak = self.peak.max(a);
                // 4x linear-interpolated inter-sample peak estimate
                for j in 1..4 {
                    let t = j as f32 / 4.0;
                    let v = (self.prev[c] + (x[c] - self.prev[c]) * t).abs();
                    self.true_peak = self.true_peak.max(v);
                }
                self.true_peak = self.true_peak.max(a);
                self.prev[c] = x[c];
                let kw = self.k2[c].process(self.k1[c].process(x[c])) as f64;
                k += kw * kw;
                let lo = self.low[c].process(x[c]) as f64;
                let hi = self.high[c].process(x[c]) as f64;
                self.low_e += lo * lo;
                self.high_e += hi * hi;
                self.sum_sq += (x[c] as f64).powi(2);
            }
            self.sum_lr += x[0] as f64 * x[1] as f64;
            self.sum_ll += (x[0] as f64).powi(2);
            self.sum_rr += (x[1] as f64).powi(2);
            self.n += 1;
            self.sub_acc += k;
            self.sub_count += 1;
            if self.sub_count >= self.sub_len {
                self.subs.push(self.sub_acc / self.sub_count as f64);
                self.sub_acc = 0.0;
                self.sub_count = 0;
            }
        }
    }

    fn lufs(ms: f64) -> f32 {
        if ms <= 1e-12 {
            -120.0
        } else {
            (-0.691 + 10.0 * ms.log10()) as f32
        }
    }

    pub fn report(&self) -> AnalysisReport {
        let _ = self.sr;
        // 400 ms momentary blocks (4 sub-blocks, 75% overlap)
        let blocks: Vec<f64> = if self.subs.len() >= 4 { self.subs.windows(4).map(|w| w.iter().sum::<f64>() / 4.0).collect() } else if !self.subs.is_empty() { vec![self.subs.iter().sum::<f64>() / self.subs.len() as f64] } else { vec![] };
        let abs_gated: Vec<f64> = blocks.iter().copied().filter(|b| Self::lufs(*b) > -70.0).collect();
        let integrated = if abs_gated.is_empty() {
            -120.0
        } else {
            let mean = abs_gated.iter().sum::<f64>() / abs_gated.len() as f64;
            let rel = Self::lufs(mean) - 10.0;
            let g: Vec<f64> = abs_gated.iter().copied().filter(|b| Self::lufs(*b) > rel).collect();
            if g.is_empty() { Self::lufs(mean) } else { Self::lufs(g.iter().sum::<f64>() / g.len() as f64) }
        };
        let st: Vec<f32> = if self.subs.len() >= 30 { self.subs.windows(30).map(|w| Self::lufs(w.iter().sum::<f64>() / 30.0)).collect() } else { vec![integrated] };
        let st_max = st.iter().copied().fold(-120.0, f32::max);
        let mut st_gated: Vec<f32> = st.iter().copied().filter(|x| *x > -70.0 && *x > integrated - 20.0).collect();
        st_gated.sort_by(|a, b| a.partial_cmp(b).unwrap());
        let lra = if st_gated.len() > 2 {
            let lo = st_gated[(st_gated.len() as f32 * 0.1) as usize];
            let hi = st_gated[((st_gated.len() as f32 * 0.95) as usize).min(st_gated.len() - 1)];
            hi - lo
        } else {
            0.0
        };
        let rms = if self.n > 0 { (self.sum_sq / (self.n as f64 * 2.0)).sqrt() as f32 } else { 0.0 };
        let total = self.sum_sq.max(1e-12);
        let low = (self.low_e / total * 100.0) as f32;
        let high = (self.high_e / total * 100.0) as f32;
        let mid = (100.0 - low - high).max(0.0);
        let corr = if self.sum_ll > 1e-12 && self.sum_rr > 1e-12 { (self.sum_lr / (self.sum_ll * self.sum_rr).sqrt()) as f32 } else { 1.0 };
        AnalysisReport {
            integrated_lufs: round1(integrated),
            short_term_max_lufs: round1(st_max),
            loudness_range_lu: round1(lra),
            peak_dbfs: round1(gain_to_db(self.peak)),
            true_peak_dbtp: round1(gain_to_db(self.true_peak)),
            rms_dbfs: round1(gain_to_db(rms)),
            crest_factor_db: round1(gain_to_db(self.peak) - gain_to_db(rms)),
            stereo_correlation: (corr * 100.0).round() / 100.0,
            spectral_balance_pct: [low.round(), mid.round(), high.round()],
            clipping: self.peak >= 0.999,
            silent: self.peak < 1e-5,
        }
    }
}

fn round1(x: f32) -> f32 {
    (x * 10.0).round() / 10.0
}
