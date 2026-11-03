//! The delay windows draw the repeats from `ondina_dsp::echoes` and the times from `ondina_core::note_value`: these
//! tests play a short burst through the real Stereo Delay and Tape Delay and check that every drawn repeat is there,
//! at its time and at its level.

use ondina_core::model::PluginInstance;
use ondina_core::note_value::delay_times;
use ondina_dsp::echoes::{stereo_echoes, tape_echoes, Side, Tap};
use ondina_dsp::{create_effect, ProcessContext};

const SR: f32 = 48000.0;
const BLOCK: usize = 512;
/// The burst's middle, in ms from the start.
const HIT_MS: f32 = 10.0;

/// A 1 kHz burst under a 20 ms Hann window, peaking at `amp` on the sample at HIT_MS, then silence.
fn burst(amp: f32, secs: f32) -> Vec<f32> {
    let n = (secs * SR) as usize;
    let len = (0.020 * SR) as usize;
    let centre = (HIT_MS * 0.001 * SR) as usize;
    (0..n)
        .map(|i| {
            if i > len {
                return 0.0;
            }
            let w = 0.5 * (1.0 - (std::f32::consts::TAU * i as f32 / len as f32).cos());
            let t = (i as f32 - centre as f32) / SR;
            amp * w * (std::f32::consts::TAU * 1000.0 * t).cos()
        })
        .collect()
}

/// `input` on both channels through effect `kind` with `params`, in blocks as the engine plays it at 120 BPM.
fn render(kind: &str, params: &[(&str, f32)], input: &[f32]) -> (Vec<f32>, Vec<f32>) {
    let mut fx = create_effect(kind, SR).unwrap();
    for (k, v) in params {
        fx.set_param(k, *v);
    }
    let (mut l, mut r) = (input.to_vec(), input.to_vec());
    let bps = 120.0 / 60.0 / SR as f64;
    let mut beat = 0.0;
    for (bl, br) in l.chunks_mut(BLOCK).zip(r.chunks_mut(BLOCK)) {
        let ctx = ProcessContext { sample_rate: SR, bpm: 120.0, beat, beats_per_sample: bps, playing: true };
        fx.process(bl, br, &ctx);
        beat += bl.len() as f64 * bps;
    }
    for x in l.iter().chain(&r) {
        assert!(x.is_finite(), "{kind}: a sample is not finite");
        assert!(x.abs() <= 1.0 + 1e-6, "{kind}: a sample clips: {x}");
    }
    (l, r)
}

/// The loudest sample within 15 ms of `ms` (+ the hit's place) and where it is.
fn peak(ch: &[f32], ms: f32) -> (usize, f32) {
    let at = |m: f32| ((m * 0.001 * SR) as usize).min(ch.len());
    let (lo, hi) = (at(ms + HIT_MS - 15.0), at(ms + HIT_MS + 15.0));
    (lo..hi).map(|i| (i, ch[i].abs())).fold((lo, 0.0), |best, x| if x.1 > best.1 { x } else { best })
}

fn db(a: f32, b: f32) -> f32 {
    20.0 * (a / b).log10()
}

fn instance(kind: &str, params: &[(&str, f32)]) -> PluginInstance {
    PluginInstance {
        id: 1,
        kind: kind.into(),
        params: params.iter().map(|(k, v)| (k.to_string(), *v)).collect(),
        bypass: false,
        preset: None,
        sample: None,
        vst3_path: None,
        vst3_params: vec![],
        vst3_state: None,
    }
}

fn first(taps: &[Tap], side: Side, n: usize) -> Vec<Tap> {
    taps.iter().copied().filter(|t| t.side == side).take(n).collect()
}

#[test]
fn stereo_delay_echoes_land_where_the_window_draws_them() {
    let params = [("sync", 1.0), ("time_l", 0.75), ("time_r", 0.5), ("feedback", 0.5), ("crossfeed", 0.0), ("low_cut", 20.0), ("high_cut", 20000.0), ("mix", 1.0)];
    let t = delay_times(&instance("stereo_delay", &params), 120.0).unwrap();
    assert_eq!((t.left_ms, t.right_ms), (375.0, Some(250.0)));
    let taps = stereo_echoes(t.left_ms, t.right_ms.unwrap(), 0.5, 0.0, 2000.0);
    let (l, r) = render("stereo_delay", &params, &burst(0.5, 1.5));
    for (side, ch) in [(Side::Left, &l), (Side::Right, &r)] {
        let three = first(&taps, side, 3);
        assert_eq!(three.len(), 3);
        for tap in three {
            let (i, p) = peak(ch, tap.ms);
            let want = ((tap.ms + HIT_MS) * 48.0) as i64;
            assert!((i as i64 - want).abs() <= 48, "{side:?} at {} ms: peak at sample {i}, want {want}", tap.ms);
            // the open cuts take less than 0.01 dB at 1 kHz
            let off = db(p, 0.5 * tap.gain);
            assert!(off.abs() <= 0.3, "{side:?} at {} ms: {off} dB off", tap.ms);
        }
    }
}

#[test]
fn free_stereo_delay_puts_the_right_side_at_three_quarters() {
    let params = [("sync", 0.0), ("time_ms", 200.0), ("feedback", 0.0), ("low_cut", 20.0), ("high_cut", 20000.0), ("mix", 1.0)];
    let t = delay_times(&instance("stereo_delay", &params), 120.0).unwrap();
    assert_eq!((t.left_ms, t.right_ms), (200.0, Some(150.0)));
    let (l, r) = render("stereo_delay", &params, &burst(0.5, 0.5));
    let loudest = |ch: &[f32]| (0..ch.len()).max_by(|a, b| ch[*a].abs().total_cmp(&ch[*b].abs())).unwrap();
    assert!((loudest(&l) as i64 - 210 * 48).abs() <= 48, "left peak at {}", loudest(&l));
    assert!((loudest(&r) as i64 - 160 * 48).abs() <= 48, "right peak at {}", loudest(&r));
}

#[test]
fn ping_pong_crosses_the_sides_where_the_model_says() {
    let params = [("sync", 1.0), ("time_l", 0.75), ("time_r", 0.5), ("feedback", 0.5), ("crossfeed", 1.0), ("low_cut", 20.0), ("high_cut", 20000.0), ("mix", 1.0)];
    let taps = stereo_echoes(375.0, 250.0, 0.5, 1.0, 1500.0);
    let (l, r) = render("stereo_delay", &params, &burst(0.5, 1.6));
    for (side, ch) in [(Side::Left, &l), (Side::Right, &r)] {
        for tap in first(&taps, side, 3) {
            let off = db(peak(ch, tap.ms).1, 0.5 * tap.gain);
            assert!(off.abs() <= 0.5, "{side:?} at {} ms: {off} dB off", tap.ms);
        }
    }
    // with full ping-pong nothing comes back on its own side: no left repeat at 750 ms, no right one at 500 ms
    for (side, ch, ms) in [(Side::Left, &l, 750.0), (Side::Right, &r, 500.0)] {
        assert!(!taps.iter().any(|t| t.side == side && (t.ms - ms).abs() < 30.0), "the model draws one at {ms}");
        let level = db(peak(ch, ms).1.max(1e-9), 0.5);
        assert!(level < -40.0, "{side:?} at {ms} ms: {level} dB");
    }
}

#[test]
fn tape_delay_echoes_fade_by_its_saturating_feedback() {
    let params = [("time", 0.5), ("feedback", 0.5), ("saturation", 0.0), ("wow", 0.0), ("tone", 1.0), ("mix", 1.0)];
    let t = delay_times(&instance("tape_delay", &params), 120.0).unwrap();
    assert_eq!(t.left_ms, 250.0);
    let taps = tape_echoes(t.left_ms, 0.5, 0.0, 1000.0);
    // the line takes (l + r) / 2: a full-level burst on both channels is a full-level repeat
    let (l, r) = render("tape_delay", &params, &burst(1.0, 1.0));
    for tap in taps.iter().take(3) {
        for ch in [&l, &r] {
            let (i, p) = peak(ch, tap.ms);
            let want = ((tap.ms + HIT_MS) * 48.0) as i64;
            assert!((i as i64 - want).abs() <= 48, "at {} ms: peak at sample {i}, want {want}", tap.ms);
            // Tone 1 (9.8 kHz) takes about 0.05 dB a trip at 1 kHz
            let off = db(p, tap.gain);
            assert!(off.abs() <= 0.3, "at {} ms: {off} dB off", tap.ms);
        }
    }
}
