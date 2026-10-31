//! Probe the bundled test VST3 plug-ins: load, list params, render audio.
fn main() {
    let list = aurora_dsp::vst3::scan(&["tools/vst3".to_string()]).unwrap();
    for p in &list {
        println!("found: {} | {} | inst={} | {}", p.name, p.vendor, p.is_instrument, p.path);
    }
    use aurora_dsp::{Effect, Instrument, ProcessContext};
    let ctx = ProcessContext { sample_rate: 48000.0, bpm: 120.0, beat: 0.0, beats_per_sample: 2.0 / 48000.0, playing: true };
    let (name, inst, params) = aurora_dsp::vst3::probe("tools/vst3/Dexed.vst3").unwrap();
    println!("Dexed probe: {name} inst={inst} params={}", params.len());
    let mut d = aurora_dsp::vst3::Vst3Processor::load("tools/vst3/Dexed.vst3", 48000.0, true).unwrap();
    d.note_on(60, 110);
    let mut e = 0.0;
    for _ in 0..50 {
        let mut l = vec![0.0; 512];
        let mut r = vec![0.0; 512];
        d.render(&mut l, &mut r, &ctx);
        e += l.iter().map(|x| x * x).sum::<f32>();
    }
    println!("Dexed energy after note: {e:.3} err={:?}", d.error);
    let (aname, _, ap) = aurora_dsp::vst3::probe("tools/vst3/Airwindows Consolidated.vst3").unwrap();
    println!("Airwindows probe: {aname} params={}", ap.len());
    let mut fx = aurora_dsp::vst3::Vst3Effect(aurora_dsp::vst3::Vst3Processor::load("tools/vst3/Airwindows Consolidated.vst3", 48000.0, false).unwrap());
    for (i, p) in ap.iter().take(4).enumerate() {
        println!("  param {i}: p{} {} default {}", p.id, p.name, p.default);
    }
    let mut total = 0.0;
    for _ in 0..40 {
        let mut l: Vec<f32> = (0..512).map(|i| (i as f32 * 0.05).sin() * 0.5).collect();
        let mut r = l.clone();
        fx.process(&mut l, &mut r, &ctx);
        total += l.iter().map(|x| x * x).sum::<f32>();
    }
    println!("Airwindows out energy over 40 blocks: {total:.3} err={:?}", fx.0.error);
}
