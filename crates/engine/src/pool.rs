//! Audio file decoding and the in-memory audio pool.

use std::collections::HashMap;
use std::path::Path;
use std::sync::Arc;

use ondina_core::Id;
use ondina_dsp::AudioData;
use symphonia::core::audio::SampleBuffer;
use symphonia::core::codecs::DecoderOptions;
use symphonia::core::errors::Error as SymError;
use symphonia::core::formats::FormatOptions;
use symphonia::core::io::MediaSourceStream;
use symphonia::core::meta::MetadataOptions;
use symphonia::core::probe::Hint;

/// Decoded audio files referenced by the project, keyed by audio file id.
pub type AudioPool = HashMap<Id, Arc<AudioData>>;

/// Decode any supported audio file (wav, aiff, flac, mp3, ogg/vorbis, m4a/aac, caf) to f32.
pub fn load_audio_file(path: &Path) -> Result<AudioData, String> {
    let file = std::fs::File::open(path).map_err(|e| format!("cannot open {}: {e}", path.display()))?;
    let mss = MediaSourceStream::new(Box::new(file), Default::default());
    let mut hint = Hint::new();
    if let Some(ext) = path.extension().and_then(|e| e.to_str()) {
        hint.with_extension(ext);
    }
    let probed = symphonia::default::get_probe()
        .format(&hint, mss, &FormatOptions::default(), &MetadataOptions::default())
        .map_err(|e| format!("unsupported audio file {}: {e}", path.display()))?;
    let mut format = probed.format;
    let track = format.default_track().ok_or("no audio track in file")?;
    let track_id = track.id;
    // some containers (AAC in MP4) only report the rate once a packet is decoded
    let mut sample_rate = track.codec_params.sample_rate.unwrap_or(0);
    let mut decoder = symphonia::default::get_codecs().make(&track.codec_params, &DecoderOptions::default()).map_err(|e| format!("unsupported codec: {e}"))?;
    let mut channels: Vec<Vec<f32>> = vec![];
    loop {
        let packet = match format.next_packet() {
            Ok(p) => p,
            Err(SymError::IoError(e)) if e.kind() == std::io::ErrorKind::UnexpectedEof => break,
            Err(SymError::ResetRequired) => break,
            Err(e) => return Err(format!("decode error: {e}")),
        };
        if packet.track_id() != track_id {
            continue;
        }
        let decoded = match decoder.decode(&packet) {
            Ok(d) => d,
            Err(SymError::DecodeError(_)) => continue,
            Err(e) => return Err(format!("decode error: {e}")),
        };
        let spec = *decoded.spec();
        if sample_rate == 0 {
            sample_rate = spec.rate;
        }
        let nch = spec.channels.count().max(1);
        if channels.is_empty() {
            channels = vec![vec![]; nch];
        }
        let mut buf = SampleBuffer::<f32>::new(decoded.capacity() as u64, spec);
        buf.copy_interleaved_ref(decoded);
        for frame in buf.samples().chunks(nch) {
            for (c, s) in frame.iter().enumerate() {
                if c < channels.len() {
                    channels[c].push(*s);
                }
            }
        }
    }
    if channels.is_empty() || channels[0].is_empty() || sample_rate == 0 {
        return Err("audio file contains no samples".into());
    }
    // keep at most stereo
    channels.truncate(2);
    Ok(AudioData { sample_rate, channels })
}

/// Write interleaved-free stereo buffers to a WAV file.
pub fn write_wav(path: &Path, left: &[f32], right: &[f32], sample_rate: u32, bit_depth: u16) -> Result<(), String> {
    if let Some(parent) = path.parent() {
        if !parent.as_os_str().is_empty() {
            std::fs::create_dir_all(parent).map_err(|e| e.to_string())?;
        }
    }
    let float = bit_depth == 32;
    let spec = hound::WavSpec {
        channels: 2,
        sample_rate,
        bits_per_sample: if float { 32 } else { bit_depth },
        sample_format: if float { hound::SampleFormat::Float } else { hound::SampleFormat::Int },
    };
    let mut w = hound::WavWriter::create(path, spec).map_err(|e| format!("cannot write {}: {e}", path.display()))?;
    let max = ((1i64 << (bit_depth.clamp(8, 24) - 1)) - 1) as f32;
    for i in 0..left.len() {
        for s in [left[i], right[i]] {
            if float {
                w.write_sample(s).map_err(|e| e.to_string())?;
            } else {
                // TPDF dither for integer formats
                let d = (fast_rand() - fast_rand()) / max;
                let v = ((s + d).clamp(-1.0, 1.0) * max).round() as i32;
                w.write_sample(v).map_err(|e| e.to_string())?;
            }
        }
    }
    w.finalize().map_err(|e| e.to_string())
}

fn fast_rand() -> f32 {
    use std::cell::Cell;
    thread_local!(static S: Cell<u32> = const { Cell::new(0x9E37_79B9) });
    S.with(|s| {
        let mut x = s.get();
        x ^= x << 13;
        x ^= x >> 17;
        x ^= x << 5;
        s.set(x);
        x as f32 / u32::MAX as f32
    })
}
