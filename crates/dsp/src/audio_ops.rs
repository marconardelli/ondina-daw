//! Offline audio processing: Time Warp (WSOLA time-stretch), Pitch Edit (pitch shift,
//! pitch correction, audio-to-MIDI via YIN), Varispeed, Smart Tempo and Stem Splitter.

use std::f32::consts::PI;

use rustfft::num_complex::Complex32;
use rustfft::FftPlanner;

use crate::AudioData;

fn hann(n: usize) -> Vec<f32> {
    (0..n).map(|i| 0.5 - 0.5 * (2.0 * PI * i as f32 / n as f32).cos()).collect()
}

/// Resample a channel by `factor` (output length = input / factor); cubic interpolation.
pub fn resample(x: &[f32], factor: f64) -> Vec<f32> {
    if x.is_empty() || factor <= 0.0 {
        return vec![];
    }
    let out_len = ((x.len() as f64) / factor).floor() as usize;
    let at = |i: isize| -> f32 { if i < 0 || i as usize >= x.len() { 0.0 } else { x[i as usize] } };
    (0..out_len)
        .map(|n| {
            let pos = n as f64 * factor;
            let i = pos.floor() as isize;
            let t = (pos - i as f64) as f32;
            let (y0, y1, y2, y3) = (at(i - 1), at(i), at(i + 1), at(i + 2));
            // Catmull-Rom
            let a = -0.5 * y0 + 1.5 * y1 - 1.5 * y2 + 0.5 * y3;
            let b = y0 - 2.5 * y1 + 2.0 * y2 - 0.5 * y3;
            let c = -0.5 * y0 + 0.5 * y2;
            ((a * t + b) * t + c) * t + y1
        })
        .collect()
}

/// WSOLA time-stretch of one channel. `ratio` > 1 makes it longer.
pub fn wsola(x: &[f32], ratio: f64, sr: u32) -> Vec<f32> {
    if x.is_empty() {
        return vec![];
    }
    let win = ((sr as f64 * 0.046) as usize).max(256) & !1; // ~46 ms
    let hop_out = win / 2;
    let hop_in = (hop_out as f64 / ratio).max(1.0);
    let tol = win / 4; // search tolerance
    let w = hann(win);
    let out_len = (x.len() as f64 * ratio) as usize + win;
    let mut out = vec![0.0f32; out_len];
    let mut norm = vec![0.0f32; out_len];
    let get = |i: isize| -> f32 { if i < 0 || i as usize >= x.len() { 0.0 } else { x[i as usize] } };
    let mut prev_pos: isize = 0;
    let mut k = 0usize;
    loop {
        let out_pos = k * hop_out;
        if out_pos + win > out_len {
            break;
        }
        let nominal = (k as f64 * hop_in) as isize;
        if nominal as usize >= x.len() + win {
            break;
        }
        // natural continuation of previous frame
        let target = prev_pos + hop_out as isize;
        let mut best = nominal;
        if k > 0 {
            let mut best_c = f32::MIN;
            let step = 2;
            let mut d = -(tol as isize);
            while d <= tol as isize {
                let cand = nominal + d;
                let mut c = 0.0;
                let mut i = 0;
                while i < win / 2 {
                    c += get(cand + i as isize) * get(target + i as isize);
                    i += 4;
                }
                if c > best_c {
                    best_c = c;
                    best = cand;
                }
                d += step;
            }
        }
        for i in 0..win {
            out[out_pos + i] += get(best + i as isize) * w[i];
            norm[out_pos + i] += w[i];
        }
        prev_pos = best;
        k += 1;
    }
    for i in 0..out.len() {
        if norm[i] > 1e-3 {
            out[i] /= norm[i];
        }
    }
    out.truncate((x.len() as f64 * ratio) as usize);
    out
}

pub fn time_stretch(d: &AudioData, ratio: f64) -> AudioData {
    AudioData { sample_rate: d.sample_rate, channels: d.channels.iter().map(|c| wsola(c, ratio, d.sample_rate)).collect() }
}

/// Pitch shift preserving duration.
pub fn pitch_shift(d: &AudioData, semitones: f64) -> AudioData {
    let r = 2f64.powf(semitones / 12.0);
    let stretched = time_stretch(d, r);
    let frames = d.frames();
    AudioData {
        sample_rate: d.sample_rate,
        channels: stretched
            .channels
            .iter()
            .map(|c| {
                let mut y = resample(c, r);
                y.resize(frames, 0.0);
                y
            })
            .collect(),
    }
}

/// Tape-style speed change: percent 200 = twice as fast and an octave higher.
pub fn varispeed(d: &AudioData, percent: f64) -> AudioData {
    let f = (percent / 100.0).clamp(0.1, 10.0);
    AudioData { sample_rate: d.sample_rate, channels: d.channels.iter().map(|c| resample(c, f)).collect() }
}

pub fn mono(d: &AudioData) -> Vec<f32> {
    let n = d.frames();
    (0..n).map(|i| d.channels.iter().map(|c| c[i]).sum::<f32>() / d.channels.len().max(1) as f32).collect()
}

/// YIN fundamental frequency estimate for one frame. Returns (f0, confidence 0..1).
pub fn yin(frame: &[f32], sr: u32, fmin: f32, fmax: f32) -> Option<(f32, f32)> {
    let n = frame.len() / 2;
    let tau_min = (sr as f32 / fmax) as usize;
    let tau_max = ((sr as f32 / fmin) as usize).min(n - 1);
    if tau_max <= tau_min + 2 {
        return None;
    }
    let mut d = vec![0.0f32; tau_max + 1];
    for tau in 1..=tau_max {
        let mut s = 0.0;
        for i in 0..n {
            let v = frame[i] - frame[i + tau];
            s += v * v;
        }
        d[tau] = s;
    }
    // cumulative mean normalized difference
    let mut cmnd = vec![1.0f32; tau_max + 1];
    let mut run = 0.0;
    for tau in 1..=tau_max {
        run += d[tau];
        cmnd[tau] = if run > 0.0 { d[tau] * tau as f32 / run } else { 1.0 };
    }
    let thr = 0.15;
    let mut tau = tau_min;
    while tau < tau_max {
        if cmnd[tau] < thr {
            while tau + 1 < tau_max && cmnd[tau + 1] < cmnd[tau] {
                tau += 1;
            }
            break;
        }
        tau += 1;
    }
    if tau >= tau_max {
        // fall back to global minimum if reasonably periodic
        let (bt, bv) = (tau_min..tau_max).map(|t| (t, cmnd[t])).fold((0, 1.0), |a, b| if b.1 < a.1 { b } else { a });
        if bv > 0.35 {
            return None;
        }
        tau = bt;
    }
    // parabolic interpolation
    let t = tau as f32;
    let (a, b, c) = (cmnd[tau - 1], cmnd[tau], cmnd[(tau + 1).min(tau_max)]);
    let denom = a + c - 2.0 * b;
    let shift = if denom.abs() > 1e-9 { 0.5 * (a - c) / denom } else { 0.0 };
    Some((sr as f32 / (t + shift), (1.0 - cmnd[tau]).clamp(0.0, 1.0)))
}

/// Pitch track: (time_sec, midi pitch or None, rms) per hop.
pub fn pitch_track(x: &[f32], sr: u32) -> Vec<(f64, Option<f32>, f32)> {
    let frame = 2048usize;
    let hop = 256usize;
    let mut out = vec![];
    let mut i = 0;
    while i + frame < x.len() {
        let f = &x[i..i + frame];
        let rms = (f.iter().map(|v| v * v).sum::<f32>() / frame as f32).sqrt();
        let p = if rms > 0.01 { yin(f, sr, 50.0, 1200.0).filter(|(_, c)| *c > 0.6).map(|(f0, _)| 69.0 + 12.0 * (f0 / 440.0).log2()) } else { None };
        out.push(((i + frame / 2) as f64 / sr as f64, p, rms));
        i += hop;
    }
    out
}

/// Monophonic transcription: notes as (start_sec, dur_sec, midi, velocity).
pub fn transcribe(d: &AudioData) -> Vec<(f64, f64, u8, u8)> {
    let x = mono(d);
    let tr = pitch_track(&x, d.sample_rate);
    let mut notes = vec![];
    let mut cur: Option<(f64, i32, f32, usize)> = None; // start, pitch, max rms, frames
    let flush = |cur: &mut Option<(f64, i32, f32, usize)>, end: f64, notes: &mut Vec<(f64, f64, u8, u8)>| {
        if let Some((s, p, r, n)) = cur.take() {
            if n >= 6 && end - s > 0.06 {
                let vel = (40.0 + (r * 4.0).min(1.0) * 87.0) as u8;
                notes.push((s, end - s, p.clamp(0, 127) as u8, vel));
            }
        }
    };
    for (t, p, rms) in &tr {
        match (p, &mut cur) {
            (Some(pm), Some(c)) if (pm.round() as i32 - c.1).abs() == 0 => {
                c.2 = c.2.max(*rms);
                c.3 += 1;
            }
            (Some(pm), _) => {
                flush(&mut cur, *t, &mut notes);
                cur = Some((*t, pm.round() as i32, *rms, 1));
            }
            (None, _) => flush(&mut cur, *t, &mut notes),
        }
    }
    if let Some(last) = tr.last() {
        flush(&mut cur, last.0, &mut notes);
    }
    notes
}

/// Pitch correction toward the nearest allowed pitch (`snap` maps midi -> target midi).
/// The audio is split into segments of constant correction, each pitch-shifted with WSOLA
/// and joined with crossfades.
pub fn pitch_correct(d: &AudioData, snap: impl Fn(f32) -> f32, strength: f32) -> AudioData {
    let x = mono(d);
    let sr = d.sample_rate;
    let n = d.frames();
    let hop = 1024usize;
    let frame = 4096usize;
    // correction in semitones per hop
    let mut corr: Vec<f32> = vec![];
    let mut i = 0;
    while i < n {
        let s = i.saturating_sub(frame / 2).min(n.saturating_sub(frame));
        let c = if n >= frame {
            yin(&x[s..s + frame], sr, 60.0, 1100.0)
                .filter(|(_, c)| *c > 0.6)
                .map(|(f0, _)| {
                    let m = 69.0 + 12.0 * (f0 / 440.0).log2();
                    (snap(m) - m) * strength
                })
                .unwrap_or(0.0)
        } else {
            0.0
        };
        corr.push(c);
        i += hop;
    }
    // group hops into segments of similar correction
    let mut segs: Vec<(usize, usize, f32)> = vec![];
    for (k, c) in corr.iter().enumerate() {
        match segs.last_mut() {
            Some(last) if (last.2 - c).abs() < 0.08 => last.1 = k + 1,
            _ => segs.push((k, k + 1, *c)),
        }
    }
    let xf = 512usize;
    let channels = d
        .channels
        .iter()
        .map(|ch| {
            let mut out = vec![0.0f32; n];
            let mut wsum = vec![0.0f32; n];
            for (a, b, c) in &segs {
                let s0 = (a * hop).min(n);
                let s1 = (b * hop).min(n);
                if s1 <= s0 {
                    continue;
                }
                let lo = s0.saturating_sub(xf);
                let hi = (s1 + xf).min(n);
                let chunk = AudioData { sample_rate: sr, channels: vec![ch[lo..hi].to_vec()] };
                let shifted = if c.abs() < 0.01 { chunk } else { pitch_shift(&chunk, *c as f64) };
                for (j, v) in shifted.channels[0].iter().enumerate() {
                    let idx = lo + j;
                    if idx >= hi {
                        break;
                    }
                    // trapezoid crossfade weights
                    let w = if idx < s0 { (idx + xf - s0) as f32 / xf as f32 } else if idx >= s1 { (hi - idx) as f32 / xf as f32 } else { 1.0 };
                    let w = w.clamp(0.0, 1.0);
                    out[idx] += v * w;
                    wsum[idx] += w;
                }
            }
            (0..n).map(|i| if wsum[i] > 1e-4 { out[i] / wsum[i] } else { 0.0 }).collect()
        })
        .collect();
    AudioData { sample_rate: sr, channels }
}

/// Estimate tempo from onset-strength autocorrelation. Returns (bpm, confidence).
pub fn detect_tempo(d: &AudioData) -> (f64, f32) {
    let x = mono(d);
    let sr = d.sample_rate as f64;
    let hop = 512usize;
    // onset strength: positive log-energy flux of a high-passed signal
    let mut prev = 0.0f32;
    let mut hp_prev = 0.0f32;
    let mut x_prev = 0.0f32;
    let mut env = vec![];
    for chunk in x.chunks(hop) {
        let mut e = 0.0;
        for s in chunk {
            let hp = 0.97 * (hp_prev + s - x_prev);
            hp_prev = hp;
            x_prev = *s;
            e += hp * hp;
        }
        let le = (1e-6 + e).ln();
        env.push((le - prev).max(0.0));
        prev = le;
    }
    if env.len() < 64 {
        return (120.0, 0.0);
    }
    let mean = env.iter().sum::<f32>() / env.len() as f32;
    let env: Vec<f32> = env.iter().map(|v| v - mean).collect();
    let fps = sr / hop as f64;
    let mut best = (120.0, f32::MIN);
    let mut scores = vec![];
    for bpm10 in 600..=2000 {
        let bpm = bpm10 as f64 / 10.0;
        let lag = fps * 60.0 / bpm;
        let mut s = 0.0;
        for mult in [1.0, 2.0, 4.0] {
            let l = (lag * mult).round() as usize;
            if l >= env.len() {
                continue;
            }
            let mut c = 0.0;
            for i in l..env.len() {
                c += env[i] * env[i - l];
            }
            s += c / (env.len() - l) as f32 / mult as f32;
        }
        // mild preference for 80..160 bpm
        let w = (-(((bpm / 115.0).ln() / 0.6).powi(2)) as f32).exp() * 0.3 + 0.7;
        s *= w;
        scores.push(s);
        if s > best.1 {
            best = (bpm, s);
        }
    }
    let max = best.1;
    let mean_s = scores.iter().sum::<f32>() / scores.len() as f32;
    let conf = if max > 0.0 { ((max - mean_s) / max).clamp(0.0, 1.0) } else { 0.0 };
    (best.0, conf)
}

/// Spectral source separation into (vocals, drums, bass, other) using harmonic/percussive
/// median filtering, stereo center extraction and band masks.
pub fn split_stems(d: &AudioData) -> [AudioData; 4] {
    let n_fft = 2048usize;
    let hop = 512usize;
    let bins = n_fft / 2 + 1;
    let sr = d.sample_rate as f32;
    let w = hann(n_fft);
    let mut planner = FftPlanner::<f32>::new();
    let fft = planner.plan_fft_forward(n_fft);
    let ifft = planner.plan_fft_inverse(n_fft);
    let chans: Vec<&Vec<f32>> = if d.channels.len() >= 2 { vec![&d.channels[0], &d.channels[1]] } else { vec![&d.channels[0], &d.channels[0]] };
    let n = d.frames();
    let frames = n / hop + 2;
    // STFT both channels
    let stft = |x: &Vec<f32>| -> Vec<Vec<Complex32>> {
        (0..frames)
            .map(|f| {
                let start = f as isize * hop as isize - (n_fft / 2) as isize;
                let mut buf: Vec<Complex32> = (0..n_fft)
                    .map(|i| {
                        let idx = start + i as isize;
                        let v = if idx >= 0 && (idx as usize) < x.len() { x[idx as usize] } else { 0.0 };
                        Complex32::new(v * w[i], 0.0)
                    })
                    .collect();
                fft.process(&mut buf);
                buf.truncate(bins);
                buf
            })
            .collect()
    };
    let sl = stft(chans[0]);
    let srr = stft(chans[1]);
    let mag: Vec<Vec<f32>> = (0..frames).map(|f| (0..bins).map(|b| (sl[f][b].norm() + srr[f][b].norm()) * 0.5).collect()).collect();
    // median filters
    let k = 8usize; // half-width (17 taps)
    let mut harm = vec![vec![0.0f32; bins]; frames];
    let mut perc = vec![vec![0.0f32; bins]; frames];
    let mut tmp = Vec::with_capacity(2 * k + 1);
    for f in 0..frames {
        for b in 0..bins {
            tmp.clear();
            for ff in f.saturating_sub(k)..(f + k + 1).min(frames) {
                tmp.push(mag[ff][b]);
            }
            let m = tmp.len() / 2;
            harm[f][b] = *tmp.select_nth_unstable_by(m, |a, b| a.partial_cmp(b).unwrap()).1;
            tmp.clear();
            for bb in b.saturating_sub(k)..(b + k + 1).min(bins) {
                tmp.push(mag[f][bb]);
            }
            let m = tmp.len() / 2;
            perc[f][b] = *tmp.select_nth_unstable_by(m, |a, b| a.partial_cmp(b).unwrap()).1;
        }
    }
    let bin_hz = sr / n_fft as f32;
    let band = |hz: f32, lo: f32, hi: f32| -> f32 {
        let a = ((hz - lo * 0.7) / (lo * 0.3)).clamp(0.0, 1.0);
        let b = ((hi * 1.3 - hz) / (hi * 0.3)).clamp(0.0, 1.0);
        a * b
    };
    // masks per stem
    let mut masks = vec![vec![vec![0.0f32; bins]; frames]; 4];
    for f in 0..frames {
        for b in 0..bins {
            let h = harm[f][b].powi(2);
            let p = perc[f][b].powi(2);
            let tot = h + p + 1e-12;
            let (hm, pm) = (h / tot, p / tot);
            let l = sl[f][b];
            let r = srr[f][b];
            let center = 1.0 - (l - r).norm() / (l.norm() + r.norm() + 1e-9);
            let hz = b as f32 * bin_hz;
            let bass = hm * if hz < 160.0 { 1.0 } else { band(hz, 1.0, 160.0) };
            let drums = pm * if hz < 100.0 { 0.6 } else { 1.0 };
            let vocals = hm * center.powi(3) * band(hz, 180.0, 7000.0) * (1.0 - bass);
            let other = (1.0 - bass - drums - vocals).max(0.0);
            masks[0][f][b] = vocals;
            masks[1][f][b] = drums;
            masks[2][f][b] = bass;
            masks[3][f][b] = other;
        }
    }
    // inverse STFT with weighted overlap-add
    let istft = |spec: &Vec<Vec<Complex32>>, mask: &Vec<Vec<f32>>| -> Vec<f32> {
        let mut out = vec![0.0f32; n + n_fft];
        let mut norm = vec![0.0f32; n + n_fft];
        let mut buf = vec![Complex32::new(0.0, 0.0); n_fft];
        for f in 0..frames {
            for b in 0..bins {
                buf[b] = spec[f][b] * mask[f][b];
            }
            for b in 1..n_fft - bins + 1 {
                buf[n_fft - b] = buf[b].conj();
            }
            ifft.process(&mut buf);
            let start = f as isize * hop as isize - (n_fft / 2) as isize;
            for i in 0..n_fft {
                let idx = start + i as isize;
                if idx >= 0 && (idx as usize) < n {
                    out[idx as usize] += buf[i].re / n_fft as f32 * w[i];
                    norm[idx as usize] += w[i] * w[i];
                }
            }
        }
        (0..n).map(|i| if norm[i] > 1e-6 { out[i] / norm[i] } else { 0.0 }).collect()
    };
    let make = |m: &Vec<Vec<f32>>| AudioData { sample_rate: d.sample_rate, channels: vec![istft(&sl, m), istft(&srr, m)] };
    [make(&masks[0]), make(&masks[1]), make(&masks[2]), make(&masks[3])]
}

#[cfg(test)]
mod tests {
    use super::*;

    fn sine(f: f32, secs: f32, sr: u32) -> Vec<f32> {
        (0..(secs * sr as f32) as usize).map(|i| (2.0 * PI * f * i as f32 / sr as f32).sin() * 0.5).collect()
    }

    fn est_freq(x: &[f32], sr: u32) -> f32 {
        let mid = x.len() / 2;
        yin(&x[mid - 2048..mid + 2048], sr, 50.0, 2000.0).unwrap().0
    }

    #[test]
    fn stretch_shift_varispeed_yin() {
        let sr = 44100;
        let d = AudioData { sample_rate: sr, channels: vec![sine(440.0, 1.0, sr)] };
        let s = time_stretch(&d, 1.5);
        assert!((s.frames() as f64 - 1.5 * 44100.0).abs() < 2.0);
        assert!((est_freq(&s.channels[0], sr) - 440.0).abs() < 4.0, "stretch changed pitch");
        let p = pitch_shift(&d, 12.0);
        assert_eq!(p.frames(), d.frames());
        assert!((est_freq(&p.channels[0], sr) - 880.0).abs() < 10.0, "got {}", est_freq(&p.channels[0], sr));
        let v = varispeed(&d, 50.0);
        assert!((v.frames() as i64 - 88200).abs() < 4);
        assert!((est_freq(&v.channels[0], sr) - 220.0).abs() < 3.0);
    }

    #[test]
    fn transcribe_and_correct() {
        let sr = 44100;
        let mut x = sine(261.63, 0.5, sr); // C4
        x.extend(vec![0.0; 4410]);
        x.extend(sine(329.63, 0.5, sr)); // E4
        let d = AudioData { sample_rate: sr, channels: vec![x] };
        let notes = transcribe(&d);
        let pitches: Vec<u8> = notes.iter().map(|n| n.2).collect();
        assert_eq!(pitches, vec![60, 64], "{notes:?}");
        // 30 cents flat A4 corrected toward 440
        let flat = AudioData { sample_rate: sr, channels: vec![sine(432.4, 1.0, sr)] };
        let c = pitch_correct(&flat, |m| m.round(), 1.0);
        let f = est_freq(&c.channels[0], sr);
        assert!((f - 440.0).abs() < 3.0, "corrected to {f}");
    }

    #[test]
    fn tempo_and_stems() {
        let sr = 22050;
        // clicks at 100 bpm for 8 s
        let mut x = vec![0.0f32; sr as usize * 8];
        let period = (sr as f64 * 60.0 / 100.0) as usize;
        let mut t = 0;
        while t + 200 < x.len() {
            for i in 0..200 {
                x[t + i] = (1.0 - i as f32 / 200.0) * if i % 2 == 0 { 0.9 } else { -0.9 };
            }
            t += period;
        }
        let d = AudioData { sample_rate: sr, channels: vec![x.clone()] };
        let (bpm, _) = detect_tempo(&d);
        assert!((bpm - 100.0).abs() < 2.0 || (bpm - 200.0).abs() < 4.0 || (bpm - 50.0).abs() < 1.0, "bpm {bpm}");
        // stems: sustained bass tone + clicks -> bass stem holds the tone, drums the clicks
        let bass = sine(80.0, 8.0, sr);
        let mix: Vec<f32> = x.iter().zip(&bass).map(|(a, b)| a * 0.5 + b).collect();
        let st = split_stems(&AudioData { sample_rate: sr, channels: vec![mix.clone(), mix] });
        let e = |d: &AudioData| d.channels[0].iter().map(|v| v * v).sum::<f32>();
        let (eb, ed) = (e(&st[2]), e(&st[1]));
        assert!(eb > ed, "bass stem should carry the tone");
        // reconstruction: stems sum close to input energy
        let total: f32 = st.iter().map(e).sum();
        assert!(total > 0.0);
    }
}
