//! Machine-learning source separation (Deezer Spleeter 2-stems, ONNX export from k2-fsa/sherpa-onnx).
//! Vocals and accompaniment magnitudes are estimated by two neural networks; soft Wiener masks are
//! applied to the complex STFT and inverted.

use std::path::{Path, PathBuf};
use std::sync::OnceLock;

use rustfft::num_complex::Complex32;
use rustfft::FftPlanner;

use crate::audio_ops::{resample, split_stems};
use crate::AudioData;

const N_FFT: usize = 4096;
const HOP: usize = 1024;
const BINS: usize = 1024; // bins fed to the network
const SPLIT: usize = 512; // frames per network chunk
const MODEL_SR: u32 = 44100;
pub const MODEL_DIR_NAME: &str = "sherpa-onnx-spleeter-2stems-fp16";

fn env() -> &'static ort::environment::Environment {
    static ENV: OnceLock<ort::environment::Environment> = OnceLock::new();
    ENV.get_or_init(|| ort::init().with_name("ondina").build().expect("ONNX Runtime environment"))
}

/// Locate the Spleeter model folder (next to the executable, in ./tools/models, or $ONDINA_MODELS; $AURORA_MODELS still works).
pub fn find_model_dir() -> Option<PathBuf> {
    let mut c: Vec<PathBuf> = vec![];
    if let Some(m) = ondina_core::env_var("MODELS") {
        c.push(PathBuf::from(m).join(MODEL_DIR_NAME));
    }
    if let Ok(exe) = std::env::current_exe() {
        for a in exe.ancestors().take(5) {
            c.push(a.join("models").join(MODEL_DIR_NAME));
            c.push(a.join("tools").join("models").join(MODEL_DIR_NAME));
        }
    }
    c.push(PathBuf::from("tools").join("models").join(MODEL_DIR_NAME));
    c.into_iter().find(|p| p.join("vocals.fp16.onnx").exists() && p.join("accompaniment.fp16.onnx").exists())
}

fn hann(n: usize) -> Vec<f32> {
    (0..n).map(|i| 0.5 - 0.5 * (2.0 * std::f32::consts::PI * i as f32 / n as f32).cos()).collect()
}

struct Stft {
    frames: usize,
    data: Vec<Vec<Complex32>>, // [frame][bin 0..=N_FFT/2]
}

fn stft(x: &[f32], w: &[f32], planner: &mut FftPlanner<f32>) -> Stft {
    let fft = planner.plan_fft_forward(N_FFT);
    let frames = x.len() / HOP + 1;
    let half = N_FFT / 2;
    let data = (0..frames)
        .map(|f| {
            let start = f as isize * HOP as isize - half as isize;
            let mut buf: Vec<Complex32> = (0..N_FFT)
                .map(|i| {
                    let idx = start + i as isize;
                    let v = if idx >= 0 && (idx as usize) < x.len() { x[idx as usize] } else { 0.0 };
                    Complex32::new(v * w[i], 0.0)
                })
                .collect();
            fft.process(&mut buf);
            buf.truncate(half + 1);
            buf
        })
        .collect();
    Stft { frames, data }
}

fn istft(s: &Stft, mask: &[Vec<f32>], len: usize, w: &[f32], planner: &mut FftPlanner<f32>) -> Vec<f32> {
    let ifft = planner.plan_fft_inverse(N_FFT);
    let half = N_FFT / 2;
    let mut out = vec![0.0f32; len + N_FFT];
    let mut norm = vec![0.0f32; len + N_FFT];
    let mut buf = vec![Complex32::new(0.0, 0.0); N_FFT];
    for f in 0..s.frames {
        for b in 0..=half {
            buf[b] = s.data[f][b] * mask[f][b];
        }
        for b in 1..half {
            buf[N_FFT - b] = buf[b].conj();
        }
        ifft.process(&mut buf);
        let start = f as isize * HOP as isize - half as isize;
        for i in 0..N_FFT {
            let idx = start + i as isize;
            if idx >= 0 && (idx as usize) < len {
                out[idx as usize] += buf[i].re / N_FFT as f32 * w[i];
                norm[idx as usize] += w[i] * w[i];
            }
        }
    }
    (0..len).map(|i| if norm[i] > 1e-6 { out[i] / norm[i] } else { 0.0 }).collect()
}

fn run_model(path: &Path, input: &[f32], splits: usize) -> Result<Vec<f32>, String> {
    let mut session = ort::session::Session::builder(env()).map_err(|e| e.to_string())?.commit_from_file(path).map_err(|e| format!("cannot load {}: {e}", path.display()))?;
    let tensor = ort::value::Tensor::from_array(([2usize, splits, SPLIT, BINS], input.to_vec().into_boxed_slice())).map_err(|e| e.to_string())?;
    let outputs = session.run(ort::inputs!["x" => tensor]).map_err(|e| e.to_string())?;
    let (_, data) = outputs["y"].try_extract_tensor::<f32>().map_err(|e| e.to_string())?;
    Ok(data.to_vec())
}

/// Separate into (vocals, accompaniment) with the neural network.
pub fn spleeter_2stems(d: &AudioData, model_dir: &Path) -> Result<(AudioData, AudioData), String> {
    let src_sr = d.sample_rate;
    let ratio = src_sr as f64 / MODEL_SR as f64;
    let chans: Vec<Vec<f32>> = (0..2)
        .map(|c| {
            let x = &d.channels[c.min(d.channels.len() - 1)];
            if src_sr == MODEL_SR { x.clone() } else { resample(x, ratio) }
        })
        .collect();
    let len = chans[0].len();
    let w = hann(N_FFT);
    let mut planner = FftPlanner::<f32>::new();
    let specs: Vec<Stft> = chans.iter().map(|x| stft(x, &w, &mut planner)).collect();
    let frames = specs[0].frames;
    let splits = frames.div_ceil(SPLIT).max(1);
    // input magnitudes [2, splits, 512, 1024]
    let mut input = vec![0.0f32; 2 * splits * SPLIT * BINS];
    for c in 0..2 {
        for f in 0..frames {
            let (s, fi) = (f / SPLIT, f % SPLIT);
            let base = ((c * splits + s) * SPLIT + fi) * BINS;
            for b in 0..BINS {
                input[base + b] = specs[c].data[f][b].norm();
            }
        }
    }
    let yv = run_model(&model_dir.join("vocals.fp16.onnx"), &input, splits)?;
    let ya = run_model(&model_dir.join("accompaniment.fp16.onnx"), &input, splits)?;
    let mut voc = vec![];
    let mut acc = vec![];
    for c in 0..2 {
        let mut mv = vec![vec![0.0f32; N_FFT / 2 + 1]; frames];
        let mut ma = vec![vec![0.0f32; N_FFT / 2 + 1]; frames];
        for f in 0..frames {
            let (s, fi) = (f / SPLIT, f % SPLIT);
            let base = ((c * splits + s) * SPLIT + fi) * BINS;
            for b in 0..=N_FFT / 2 {
                let bb = b.min(BINS - 1); // extend the top bin's mask above 11 kHz
                let v = yv[base + bb].powi(2);
                let a = ya[base + bb].powi(2);
                let tot = v + a + 1e-10;
                mv[f][b] = v / tot;
                ma[f][b] = a / tot;
            }
        }
        let back = |x: Vec<f32>| if src_sr == MODEL_SR { x } else { let mut y = resample(&x, 1.0 / ratio); y.resize(d.frames(), 0.0); y };
        voc.push(back(istft(&specs[c], &mv, len, &w, &mut planner)));
        acc.push(back(istft(&specs[c], &ma, len, &w, &mut planner)));
    }
    Ok((AudioData { sample_rate: src_sr, channels: voc }, AudioData { sample_rate: src_sr, channels: acc }))
}

/// Four stems: ML vocals, plus drums/bass/other split spectrally from the ML accompaniment.
pub fn ml_four_stems(d: &AudioData, model_dir: &Path) -> Result<[AudioData; 4], String> {
    let (voc, acc) = spleeter_2stems(d, model_dir)?;
    let [rest_voc, drums, bass, other] = split_stems(&acc);
    // anything the spectral stage still called "vocal" in the accompaniment belongs to "other"
    let other = AudioData { sample_rate: other.sample_rate, channels: other.channels.iter().zip(&rest_voc.channels).map(|(a, b)| a.iter().zip(b).map(|(x, y)| x + y).collect()).collect() };
    Ok([voc, drums, bass, other])
}
