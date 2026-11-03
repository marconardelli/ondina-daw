//! The Drum Synth's window shows, under each pad, only the knobs of that sound (`ondina_core::drum_kit`). These tests
//! play every pad through the real Drum Synth and check that the table tells the truth: each pad sounds, changes with
//! the parameters the table gives it, and with no other one besides Drive and Level.

use ondina_core::drum_kit::{DRUM_PADS, KIT_PARAMS};
use ondina_core::plugins;
use ondina_dsp::{create_instrument, ProcessContext};

const SR: f32 = 48000.0;
const BLOCK: usize = 480;

/// Half a second of pad `note` struck at velocity 100, with `set` applied first.
fn render(note: u8, set: &[(&str, f32)]) -> (Vec<f32>, Vec<f32>) {
    let mut inst = create_instrument("drum_synth", SR).unwrap();
    for (id, v) in set {
        inst.set_param(id, *v);
    }
    inst.note_on(note, 100);
    let ctx = ProcessContext::idle(SR);
    let (mut out_l, mut out_r) = (Vec::new(), Vec::new());
    let (mut l, mut r) = (vec![0.0; BLOCK], vec![0.0; BLOCK]);
    for _ in 0..(SR as usize / 2 / BLOCK) {
        l.iter_mut().for_each(|s| *s = 0.0);
        r.iter_mut().for_each(|s| *s = 0.0);
        inst.render(&mut l, &mut r, &ctx);
        out_l.extend_from_slice(&l);
        out_r.extend_from_slice(&r);
    }
    (out_l, out_r)
}

fn max_diff(a: &(Vec<f32>, Vec<f32>), b: &(Vec<f32>, Vec<f32>)) -> f32 {
    a.0.iter().zip(&b.0).chain(a.1.iter().zip(&b.1)).map(|(x, y)| (x - y).abs()).fold(0.0, f32::max)
}

#[test]
fn every_pad_sounds_and_stays_finite() {
    for pad in &DRUM_PADS {
        let (l, r) = render(pad.note, &[]);
        assert!(l.iter().chain(&r).all(|s| s.is_finite()), "{} is not finite", pad.name);
        let peak = l.iter().chain(&r).map(|s| s.abs()).fold(0.0, f32::max);
        assert!(peak > 1e-3 && peak < 2.0, "{} peaks at {peak}", pad.name);
    }
}

#[test]
fn each_pad_answers_only_to_its_own_parameters() {
    let info = plugins::find("drum_synth").unwrap();
    for pad in &DRUM_PADS {
        let plain = render(pad.note, &[]);
        for p in info.params.iter().filter(|p| !KIT_PARAMS.contains(&p.id)) {
            let v = if p.default == p.max { p.min } else { p.max };
            let changed = render(pad.note, &[(p.id, v)]);
            let diff = max_diff(&plain, &changed);
            if pad.params.contains(&p.id) {
                assert!(diff > 1e-6, "{} does not change with {}", pad.name, p.id);
            } else {
                assert!(changed == plain, "{} changes with {} (by {diff}), which its pad does not show", pad.name, p.id);
            }
        }
    }
}

#[test]
fn drive_and_level_change_every_pad() {
    for pad in &DRUM_PADS {
        let plain = render(pad.note, &[]);
        for set in [("drive", 1.0), ("level", 6.0)] {
            assert!(max_diff(&plain, &render(pad.note, &[set])) > 1e-6, "{} does not change with {}", pad.name, set.0);
        }
    }
}
