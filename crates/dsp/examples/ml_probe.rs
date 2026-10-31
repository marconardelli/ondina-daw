fn main() {
    let r = hound::WavReader::open("tools/models/audio_example.wav").unwrap();
    let spec = r.spec();
    println!("input: {:?}", spec);
    let samples: Vec<f32> = match spec.sample_format { hound::SampleFormat::Int => r.into_samples::<i32>().map(|s| s.unwrap() as f32 / (1i64 << (spec.bits_per_sample - 1)) as f32).collect(), _ => r.into_samples::<f32>().map(|s| s.unwrap()).collect() };
    let ch = spec.channels as usize;
    let chans: Vec<Vec<f32>> = (0..ch).map(|c| samples.iter().skip(c).step_by(ch).copied().collect()).collect();
    let d = ondina_dsp::AudioData { sample_rate: spec.sample_rate, channels: chans };
    let dir = ondina_dsp::ml_stems::find_model_dir().expect("model");
    let t = std::time::Instant::now();
    let (v, a) = ondina_dsp::ml_stems::spleeter_2stems(&d, &dir).unwrap();
    println!("separated {:.1}s of audio in {:.2}s", d.duration_sec(), t.elapsed().as_secs_f32());
    let e = |x: &[f32]| (x.iter().map(|s| s * s).sum::<f32>() / x.len() as f32).sqrt();
    println!("rms mix {:.4}  vocals {:.4}  accompaniment {:.4}", e(&d.channels[0]), e(&v.channels[0]), e(&a.channels[0]));
    let err: f32 = (0..d.frames()).map(|i| (v.channels[0][i] + a.channels[0][i] - d.channels[0][i]).powi(2)).sum::<f32>() / d.frames() as f32;
    println!("reconstruction rms error {:.6}", err.sqrt());
    for (name, x) in [("mix", &d), ("vocals", &v), ("accomp", &a)] {
        let m = ondina_dsp::audio_ops::mono(x);
        let tr = ondina_dsp::audio_ops::pitch_track(&m, x.sample_rate);
        let voiced = tr.iter().filter(|t| t.1.is_some()).count() as f32 / tr.len() as f32;
        let (l, r) = (&x.channels[0], &x.channels[1]);
        let num: f32 = l.iter().zip(r).map(|(a, b)| a * b).sum();
        let den = (l.iter().map(|a| a * a).sum::<f32>() * r.iter().map(|b| b * b).sum::<f32>()).sqrt();
        println!("{name}: monophonic {:.0}%  L/R correlation {:.2}", voiced * 100.0, num / den);
    }
    // time profile: vocal share of energy per 0.5 s window
    let win = (d.sample_rate / 2) as usize;
    let mut shares = vec![];
    for k in 0..d.frames() / win {
        let ev: f32 = v.channels[0][k * win..(k + 1) * win].iter().map(|x| x * x).sum();
        let ea: f32 = a.channels[0][k * win..(k + 1) * win].iter().map(|x| x * x).sum();
        shares.push(ev / (ev + ea + 1e-9));
    }
    println!("vocal energy share per 0.5s: {}", shares.iter().map(|s| format!("{:.2}", s)).collect::<Vec<_>>().join(" "));
    for (name, x) in [("vocals", &v), ("accomp", &a)] {
        let mut w = hound::WavWriter::create(format!("tools/models/out_{name}.wav"), hound::WavSpec { channels: 2, sample_rate: x.sample_rate, bits_per_sample: 32, sample_format: hound::SampleFormat::Float }).unwrap();
        for i in 0..x.frames() { w.write_sample(x.channels[0][i]).unwrap(); w.write_sample(x.channels[1][i]).unwrap(); }
        w.finalize().unwrap();
    }
}
#[allow(dead_code)]
fn _unused() {}
