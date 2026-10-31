//! Surround / immersive panning (VBAP over standard speaker layouts) and a real-time
//! binaural panner (ITD + ILD + head-shadow filtering) for headphone spatial audio.

use crate::util::{Biquad, BiquadKind, DelayLine, OnePole};

/// Speaker (azimuth deg, elevation deg); LFE is marked with elevation -999.
pub fn layout(name: &str) -> Vec<(&'static str, f32, f32)> {
    let mut v = vec![("L", -30.0, 0.0), ("R", 30.0, 0.0), ("C", 0.0, 0.0), ("LFE", 0.0, -999.0)];
    match name {
        "5.1" => v.extend([("Ls", -110.0, 0.0), ("Rs", 110.0, 0.0)]),
        "7.1" => v.extend([("Ls", -90.0, 0.0), ("Rs", 90.0, 0.0), ("Lrs", -150.0, 0.0), ("Rrs", 150.0, 0.0)]),
        "7.1.2" => v.extend([("Lss", -90.0, 0.0), ("Rss", 90.0, 0.0), ("Lrs", -135.0, 0.0), ("Rrs", 135.0, 0.0), ("Ltm", -90.0, 45.0), ("Rtm", 90.0, 45.0)]),
        "7.1.4" => v.extend([("Ls", -90.0, 0.0), ("Rs", 90.0, 0.0), ("Lrs", -150.0, 0.0), ("Rrs", 150.0, 0.0), ("Ltf", -45.0, 45.0), ("Rtf", 45.0, 45.0), ("Ltr", -135.0, 45.0), ("Rtr", 135.0, 45.0)]),
        _ => {}
    }
    v
}

fn wrap(a: f32) -> f32 {
    let mut x = a % 360.0;
    if x > 180.0 {
        x -= 360.0;
    }
    if x < -180.0 {
        x += 360.0;
    }
    x
}

/// Pairwise amplitude panning within one ring of speakers.
fn ring_gains(az: f32, ring: &[(usize, f32)], out: &mut [f32], scale: f32) {
    if ring.is_empty() {
        return;
    }
    if ring.len() == 1 {
        out[ring[0].0] += scale;
        return;
    }
    let mut sorted: Vec<(usize, f32)> = ring.to_vec();
    sorted.sort_by(|a, b| a.1.partial_cmp(&b.1).unwrap());
    let a = wrap(az);
    for k in 0..sorted.len() {
        let (i0, a0) = sorted[k];
        let (i1, mut a1) = sorted[(k + 1) % sorted.len()];
        if a1 <= a0 {
            a1 += 360.0;
        }
        let mut aa = a;
        if aa < a0 {
            aa += 360.0;
        }
        if aa >= a0 && aa <= a1 {
            let t = (aa - a0) / (a1 - a0).max(1e-3);
            let th = t * std::f32::consts::FRAC_PI_2;
            out[i0] += th.cos() * scale;
            out[i1] += th.sin() * scale;
            return;
        }
    }
}

/// Gains per speaker of `layout` for a source at azimuth/elevation with spread 0..1.
pub fn vbap_gains(lay: &[(&str, f32, f32)], azimuth: f32, elevation: f32, spread: f32) -> Vec<f32> {
    let mut g = vec![0.0f32; lay.len()];
    let ear: Vec<(usize, f32)> = lay.iter().enumerate().filter(|(_, s)| s.2 == 0.0).map(|(i, s)| (i, s.1)).collect();
    let top: Vec<(usize, f32)> = lay.iter().enumerate().filter(|(_, s)| s.2 > 0.0).map(|(i, s)| (i, s.1)).collect();
    let el = elevation.clamp(0.0, 90.0) / 90.0;
    let (ear_w, top_w) = if top.is_empty() { (1.0, 0.0) } else { ((1.0 - el).sqrt(), el.sqrt()) };
    ring_gains(azimuth, &ear, &mut g, ear_w);
    ring_gains(azimuth, &top, &mut g, top_w);
    if spread > 0.0 {
        let n = (ear.len() + top.len()).max(1) as f32;
        for (i, s) in lay.iter().enumerate() {
            if s.2 > -900.0 {
                g[i] = g[i] * (1.0 - spread) + spread / n.sqrt();
            }
        }
    }
    // power normalize (excluding LFE)
    let p: f32 = g.iter().map(|x| x * x).sum::<f32>().sqrt().max(1e-6);
    g.iter_mut().for_each(|x| *x /= p);
    g
}

/// Real-time binaural panner for one stereo source.
pub struct Binaural {
    sr: f32,
    dl: [DelayLine; 2],
    shadow: [OnePole; 2],
    rear: [Biquad; 2],
    height: [Biquad; 2],
    cur: Option<(f32, f32, f32)>,
    params: [(f32, f32, f32, f32); 2], // per input channel: itd_l, itd_r, gain_l, gain_r
    cut: [(f32, f32); 2],
}

impl Binaural {
    pub fn new(sr: f32) -> Self {
        let mk = || DelayLine::new((sr * 0.002) as usize + 8);
        Binaural { sr, dl: [mk(), mk()], shadow: [OnePole::default(), OnePole::default()], rear: [Biquad::default(); 2], height: [Biquad::default(); 2], cur: None, params: [(0.0, 0.0, 1.0, 1.0); 2], cut: [(20000.0, 20000.0); 2] }
    }

    fn ear_params(az_deg: f32) -> (f32, f32, f32, f32, f32, f32) {
        let az = az_deg.to_radians();
        let s = az.sin(); // + right
        let itd = 0.00066 * s.abs(); // seconds
        let near = 1.0 + 0.25 * s.abs();
        let far = 1.0 - 0.55 * s.abs();
        let far_cut = 20000.0 - 17000.0 * s.abs();
        if s >= 0.0 {
            // source on the right: left ear is far
            (itd, 0.0, far, near, far_cut, 20000.0)
        } else {
            (0.0, itd, near, far, 20000.0, far_cut)
        }
    }

    fn update(&mut self, az: f32, el: f32, spread: f32) {
        if self.cur == Some((az, el, spread)) {
            return;
        }
        self.cur = Some((az, el, spread));
        // the two input channels are placed at az -/+ width
        let width = 15.0 + spread * 60.0;
        for (c, a) in [(0usize, az - width), (1, az + width)] {
            let (il, ir, gl, gr, cl, cr) = Self::ear_params(a);
            self.params[c] = (il, ir, gl, gr);
            self.cut[c] = (cl, cr);
        }
        let back = wrap(az).abs() > 90.0;
        for k in 0..2 {
            self.rear[k].set(BiquadKind::HighShelf, 4000.0, 0.7, if back { -4.0 } else { 0.0 }, self.sr);
            self.height[k].set(BiquadKind::Peak, 7000.0, 1.0, el.clamp(0.0, 90.0) / 90.0 * 5.0, self.sr);
        }
    }

    /// Process a stereo source in place into binaural stereo.
    pub fn process(&mut self, l: &mut [f32], r: &mut [f32], azimuth: f32, elevation: f32, spread: f32) {
        self.update(azimuth, elevation, spread);
        let sr = self.sr;
        for i in 0..l.len() {
            let ins = [l[i], r[i]];
            let mut ear = [0.0f32; 2];
            for c in 0..2 {
                self.dl[c].write(ins[c]);
                let (itd_l, itd_r, gl, gr) = self.params[c];
                let (cl, cr) = self.cut[c];
                let vl = if itd_l > 0.0 { self.dl[c].read(1.0 + itd_l * sr) } else { ins[c] };
                let vr = if itd_r > 0.0 { self.dl[c].read(1.0 + itd_r * sr) } else { ins[c] };
                // simple per-ear shadow: blend toward a low-passed version
                let shade_l = 1.0 - (cl / 20000.0);
                let shade_r = 1.0 - (cr / 20000.0);
                ear[0] += vl * gl * (1.0 - shade_l * 0.5);
                ear[1] += vr * gr * (1.0 - shade_r * 0.5);
            }
            self.shadow[0].set_freq(self.cut[0].0.min(self.cut[1].0), sr);
            self.shadow[1].set_freq(self.cut[0].1.min(self.cut[1].1), sr);
            let el = self.shadow[0].process(ear[0]) * 0.5 + ear[0] * 0.5;
            let er = self.shadow[1].process(ear[1]) * 0.5 + ear[1] * 0.5;
            l[i] = self.height[0].process(self.rear[0].process(el)) * 0.7;
            r[i] = self.height[1].process(self.rear[1].process(er)) * 0.7;
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn vbap_points_to_the_right_speakers() {
        let lay = layout("5.1");
        let g = vbap_gains(&lay, 30.0, 0.0, 0.0);
        assert!(g[1] > 0.99, "{g:?}"); // R
        let g = vbap_gains(&lay, 180.0, 0.0, 0.0);
        assert!(g[4] > 0.6 && g[5] > 0.6, "rear between Ls/Rs: {g:?}");
        let top = layout("7.1.4");
        let g = vbap_gains(&top, 45.0, 60.0, 0.0);
        assert!(g[9] > g[1], "height speaker should dominate: {g:?}");
    }

    #[test]
    fn binaural_right_source_is_louder_right() {
        let mut b = Binaural::new(48000.0);
        let mut l: Vec<f32> = (0..4800).map(|i| (i as f32 * 0.05).sin()).collect();
        let mut r = l.clone();
        b.process(&mut l, &mut r, 90.0, 0.0, 0.0);
        let e = |x: &[f32]| x.iter().map(|v| v * v).sum::<f32>();
        assert!(e(&r) > e(&l) * 1.5);
    }
}
