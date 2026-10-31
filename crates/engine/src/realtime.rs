//! Real-time audio engine: owns the output device thread and communicates with the
//! audio callback through a lock-free channel. If no device is available a "null device"
//! clock keeps the transport running (useful for headless MCP operation).

use std::collections::HashMap;
use std::sync::atomic::{AtomicBool, AtomicU32, AtomicU64, Ordering};
use std::sync::Arc;
use std::thread::JoinHandle;
use std::time::{Duration, Instant};

use ondina_core::model::{Id, Project, MASTER_ID};
use ondina_dsp::MidiKind;
use cpal::traits::{DeviceTrait, HostTrait, StreamTrait};
use cpal::{FromSample, SizedSample};
use crossbeam_channel::{unbounded, Receiver, Sender};
use parking_lot::Mutex;

use crate::pool::AudioPool;
use crate::renderer::{LiveAction, LiveLogEntry, RecordedEvent, Renderer};

pub enum EngineMsg {
    Project(Arc<Project>),
    Pool(Arc<AudioPool>),
    Play { count_in_beats: f64 },
    Stop,
    Locate(f64),
    Live(Id, MidiKind),
    Panic,
    Record(bool),
    LiveLaunch(Id),
    LiveScene(usize),
    LiveStop(Option<Id>),
}

/// Snapshot of the Live Loops engine state.
#[derive(Debug, Clone, Default, serde::Serialize)]
pub struct LiveState {
    /// (track_id, cell_id, start_beat)
    pub playing: Vec<(Id, Id, f64)>,
    /// (at_beat, track_id, cell_id or None for stop)
    pub queued: Vec<(f64, Id, Option<Id>)>,
}

#[derive(Default)]
pub struct Shared {
    pub playing: AtomicBool,
    pub recording: AtomicBool,
    beat_bits: AtomicU64,
    pub sample_rate: AtomicU32,
    load_bits: AtomicU32,
    pub meters: Mutex<HashMap<Id, [f32; 2]>>,
    pub recorded_midi: Mutex<Vec<RecordedEvent>>,
    pub input_buffer: Mutex<Vec<f32>>,
    pub input_channels: AtomicU32,
    pub input_sample_rate: AtomicU32,
    pub device_name: Mutex<String>,
    pub input_device_name: Mutex<String>,
    pub live_state: Mutex<LiveState>,
    pub live_log: Mutex<Vec<LiveLogEntry>>,
}

impl Shared {
    pub fn beat(&self) -> f64 {
        f64::from_bits(self.beat_bits.load(Ordering::Relaxed))
    }
    fn set_beat(&self, b: f64) {
        self.beat_bits.store(b.to_bits(), Ordering::Relaxed);
    }
    /// DSP load as a fraction of the available callback time.
    pub fn load(&self) -> f32 {
        f32::from_bits(self.load_bits.load(Ordering::Relaxed))
    }
}

struct DeviceThread {
    stop: Sender<()>,
    handle: JoinHandle<()>,
}

pub struct Engine {
    tx: Sender<EngineMsg>,
    rx: Receiver<EngineMsg>,
    pub shared: Arc<Shared>,
    project: Arc<Project>,
    pool: Arc<AudioPool>,
    device: Option<DeviceThread>,
    input: Option<DeviceThread>,
    output_name: Option<String>,
    input_name: Option<String>,
    pub audio_enabled: bool,
}

#[derive(Debug, Clone, serde::Serialize)]
pub struct DeviceList {
    pub host: String,
    pub outputs: Vec<String>,
    pub inputs: Vec<String>,
    pub default_output: Option<String>,
    pub default_input: Option<String>,
    pub current_output: String,
    pub current_input: String,
    pub sample_rate: u32,
}

impl Engine {
    /// Create the engine. `audio_enabled=false` forces the null device (headless).
    pub fn new(project: Arc<Project>, pool: Arc<AudioPool>, audio_enabled: bool) -> Engine {
        let (tx, rx) = unbounded();
        let shared = Arc::new(Shared::default());
        shared.sample_rate.store(48000, Ordering::Relaxed);
        let mut e = Engine { tx, rx, shared, project, pool, device: None, input: None, output_name: None, input_name: None, audio_enabled };
        e.start_device();
        e
    }

    fn start_device(&mut self) {
        if let Some(d) = self.device.take() {
            let _ = d.stop.send(());
            let _ = d.handle.join();
        }
        let (stop_tx, stop_rx) = unbounded::<()>();
        let rx = self.rx.clone();
        let shared = self.shared.clone();
        let project = self.project.clone();
        let pool = self.pool.clone();
        let name = self.output_name.clone();
        let use_audio = self.audio_enabled;
        let beat = shared.beat();
        let handle = std::thread::Builder::new()
            .name("ondina-audio".into())
            .spawn(move || {
                if use_audio {
                    match open_output(name, &rx, &shared, &project, &pool, beat) {
                        Ok(stream) => {
                            let _ = stop_rx.recv();
                            drop(stream);
                            return;
                        }
                        Err(e) => {
                            eprintln!("[ondina] audio device unavailable ({e}); using null device");
                        }
                    }
                }
                null_device(&rx, &shared, project, pool, beat, &stop_rx);
            })
            .expect("spawn audio thread");
        self.device = Some(DeviceThread { stop: stop_tx, handle });
    }

    pub fn sender(&self) -> Sender<EngineMsg> {
        self.tx.clone()
    }

    pub fn send(&self, m: EngineMsg) {
        let _ = self.tx.send(m);
    }

    pub fn set_project(&mut self, p: Arc<Project>) {
        self.project = p.clone();
        self.send(EngineMsg::Project(p));
    }

    /// The project snapshot most recently handed to the audio thread.
    pub fn project(&self) -> &Arc<Project> {
        &self.project
    }

    pub fn set_pool(&mut self, pool: Arc<AudioPool>) {
        self.pool = pool.clone();
        self.send(EngineMsg::Pool(pool));
    }

    pub fn play(&self, from: Option<f64>, count_in_beats: f64) {
        if let Some(b) = from {
            self.send(EngineMsg::Locate(b));
            self.shared.set_beat(b);
        }
        self.send(EngineMsg::Play { count_in_beats });
        self.shared.playing.store(true, Ordering::Relaxed);
    }

    pub fn stop(&self) {
        self.send(EngineMsg::Stop);
        self.shared.playing.store(false, Ordering::Relaxed);
    }

    pub fn locate(&self, beat: f64) {
        self.send(EngineMsg::Locate(beat));
        self.shared.set_beat(beat);
    }

    pub fn live(&self, track: Id, kind: MidiKind) {
        self.send(EngineMsg::Live(track, kind));
    }

    pub fn panic(&self) {
        self.send(EngineMsg::Panic);
    }

    pub fn live_launch(&self, cell: Id) {
        self.send(EngineMsg::LiveLaunch(cell));
        self.shared.playing.store(true, Ordering::Relaxed);
    }

    pub fn live_scene(&self, scene: usize) {
        self.send(EngineMsg::LiveScene(scene));
        self.shared.playing.store(true, Ordering::Relaxed);
    }

    pub fn live_stop(&self, track: Option<Id>) {
        self.send(EngineMsg::LiveStop(track));
    }

    pub fn position(&self) -> f64 {
        self.shared.beat()
    }

    pub fn is_playing(&self) -> bool {
        self.shared.playing.load(Ordering::Relaxed)
    }

    pub fn is_recording(&self) -> bool {
        self.shared.recording.load(Ordering::Relaxed)
    }

    pub fn sample_rate(&self) -> u32 {
        self.shared.sample_rate.load(Ordering::Relaxed)
    }

    /// Start recording. `capture_audio` opens the input device.
    pub fn start_recording(&mut self, capture_audio: bool) -> Result<(), String> {
        self.shared.recorded_midi.lock().clear();
        self.shared.input_buffer.lock().clear();
        if capture_audio && self.audio_enabled {
            self.start_input()?;
        }
        self.send(EngineMsg::Record(true));
        self.shared.recording.store(true, Ordering::Relaxed);
        Ok(())
    }

    /// Stop recording and return captured MIDI and (interleaved) audio.
    pub fn stop_recording(&mut self) -> (Vec<RecordedEvent>, Option<(Vec<f32>, u16, u32)>) {
        self.send(EngineMsg::Record(false));
        self.shared.recording.store(false, Ordering::Relaxed);
        if let Some(d) = self.input.take() {
            let _ = d.stop.send(());
            let _ = d.handle.join();
        }
        // allow the callback to flush recorded events
        std::thread::sleep(Duration::from_millis(30));
        let midi = std::mem::take(&mut *self.shared.recorded_midi.lock());
        let audio = std::mem::take(&mut *self.shared.input_buffer.lock());
        let ch = self.shared.input_channels.load(Ordering::Relaxed) as u16;
        let sr = self.shared.input_sample_rate.load(Ordering::Relaxed);
        let audio = if audio.is_empty() || ch == 0 { None } else { Some((audio, ch, sr)) };
        (midi, audio)
    }

    fn start_input(&mut self) -> Result<(), String> {
        if self.input.is_some() {
            return Ok(());
        }
        let (stop_tx, stop_rx) = unbounded::<()>();
        let (ready_tx, ready_rx) = unbounded::<Result<(), String>>();
        let shared = self.shared.clone();
        let name = self.input_name.clone();
        let handle = std::thread::Builder::new()
            .name("ondina-input".into())
            .spawn(move || match open_input(name, &shared) {
                Ok(stream) => {
                    let _ = ready_tx.send(Ok(()));
                    let _ = stop_rx.recv();
                    drop(stream);
                }
                Err(e) => {
                    let _ = ready_tx.send(Err(e));
                }
            })
            .map_err(|e| e.to_string())?;
        match ready_rx.recv_timeout(Duration::from_secs(5)) {
            Ok(Ok(())) => {
                self.input = Some(DeviceThread { stop: stop_tx, handle });
                Ok(())
            }
            Ok(Err(e)) => Err(format!("audio input unavailable: {e}")),
            Err(_) => Err("audio input did not start".into()),
        }
    }

    pub fn list_devices(&self) -> DeviceList {
        let host = cpal::default_host();
        let outputs = host.output_devices().map(|d| d.filter_map(|x| x.name().ok()).collect()).unwrap_or_default();
        let inputs = host.input_devices().map(|d| d.filter_map(|x| x.name().ok()).collect()).unwrap_or_default();
        DeviceList {
            host: host.id().name().to_string(),
            outputs,
            inputs,
            default_output: host.default_output_device().and_then(|d| d.name().ok()),
            default_input: host.default_input_device().and_then(|d| d.name().ok()),
            current_output: self.shared.device_name.lock().clone(),
            current_input: self.input_name.clone().unwrap_or_else(|| "default".into()),
            sample_rate: self.sample_rate(),
        }
    }

    pub fn set_devices(&mut self, output: Option<String>, input: Option<String>) {
        if input.is_some() {
            self.input_name = input;
        }
        if output.is_some() {
            self.output_name = output;
            self.start_device();
        }
    }
}

impl Drop for Engine {
    fn drop(&mut self) {
        if let Some(d) = self.device.take() {
            let _ = d.stop.send(());
            let _ = d.handle.join();
        }
        if let Some(d) = self.input.take() {
            let _ = d.stop.send(());
            let _ = d.handle.join();
        }
    }
}

fn handle_msg(r: &mut Renderer, m: EngineMsg, shared: &Shared) {
    match m {
        EngineMsg::Project(p) => r.set_project(p),
        EngineMsg::Pool(p) => r.set_pool(p),
        EngineMsg::Play { count_in_beats } => {
            r.count_in_beats = count_in_beats;
            r.play();
        }
        EngineMsg::Stop => {
            r.stop();
            shared.playing.store(false, Ordering::Relaxed);
        }
        EngineMsg::Locate(b) => r.locate(b),
        EngineMsg::Live(t, k) => r.live_event(t, k),
        EngineMsg::Panic => r.panic(),
        EngineMsg::Record(on) => r.recording = on,
        EngineMsg::LiveLaunch(c) => r.launch_cell(c),
        EngineMsg::LiveScene(sc) => r.launch_scene(sc),
        EngineMsg::LiveStop(t) => r.stop_live(t),
    }
}

fn publish(r: &mut Renderer, shared: &Shared) {
    shared.set_beat(r.beat);
    shared.playing.store(r.playing, Ordering::Relaxed);
    if let Some(mut m) = shared.meters.try_lock() {
        // latest block peaks with a short release so slow readers (MCP) still see activity
        for (id, p) in r.track_peaks() {
            let e = m.entry(id).or_insert([0.0; 2]);
            e[0] = p[0].max(e[0] * 0.97);
            e[1] = p[1].max(e[1] * 0.97);
        }
        let e = m.entry(MASTER_ID).or_insert([0.0; 2]);
        e[0] = r.master_peak[0].max(e[0] * 0.97);
        e[1] = r.master_peak[1].max(e[1] * 0.97);
    }
    if let Some(mut ls) = shared.live_state.try_lock() {
        let (playing, queued) = r.live_state();
        ls.playing = playing;
        ls.queued = queued
            .into_iter()
            .map(|(at, a)| match a {
                LiveAction::Launch { cell_id, track_id } => (at, track_id, Some(cell_id)),
                LiveAction::Stop { track_id } => (at, track_id, None),
            })
            .collect();
    }
    if !r.live_log.is_empty() {
        if let Some(mut log) = shared.live_log.try_lock() {
            log.append(&mut r.live_log);
        }
    }
    if !r.recorded.is_empty() {
        if let Some(mut rec) = shared.recorded_midi.try_lock() {
            rec.append(&mut r.recorded);
        }
    }
}

fn open_output(name: Option<String>, rx: &Receiver<EngineMsg>, shared: &Arc<Shared>, project: &Arc<Project>, pool: &Arc<AudioPool>, beat: f64) -> Result<cpal::Stream, String> {
    let host = cpal::default_host();
    let device = match &name {
        Some(n) => host.output_devices().map_err(|e| e.to_string())?.find(|d| d.name().map(|x| x.contains(n.as_str())).unwrap_or(false)).ok_or_else(|| format!("output device '{n}' not found"))?,
        None => host.default_output_device().ok_or("no default output device")?,
    };
    let config = device.default_output_config().map_err(|e| e.to_string())?;
    let sr = config.sample_rate().0;
    shared.sample_rate.store(sr, Ordering::Relaxed);
    *shared.device_name.lock() = device.name().unwrap_or_default();
    let mut renderer = Renderer::new(sr as f32, project.clone(), pool.clone());
    renderer.beat = beat;
    let stream_config: cpal::StreamConfig = config.clone().into();
    let stream = match config.sample_format() {
        cpal::SampleFormat::F32 => build_stream::<f32>(&device, &stream_config, renderer, rx.clone(), shared.clone()),
        cpal::SampleFormat::I16 => build_stream::<i16>(&device, &stream_config, renderer, rx.clone(), shared.clone()),
        cpal::SampleFormat::U16 => build_stream::<u16>(&device, &stream_config, renderer, rx.clone(), shared.clone()),
        cpal::SampleFormat::I32 => build_stream::<i32>(&device, &stream_config, renderer, rx.clone(), shared.clone()),
        f => Err(format!("unsupported sample format {f:?}")),
    }?;
    stream.play().map_err(|e| e.to_string())?;
    Ok(stream)
}

fn build_stream<T>(device: &cpal::Device, config: &cpal::StreamConfig, mut renderer: Renderer, rx: Receiver<EngineMsg>, shared: Arc<Shared>) -> Result<cpal::Stream, String>
where
    T: SizedSample + FromSample<f32>,
{
    let channels = config.channels as usize;
    let sr = config.sample_rate.0 as f32;
    let mut bl = vec![0.0f32; 8192];
    let mut br = vec![0.0f32; 8192];
    device
        .build_output_stream(
            config,
            move |data: &mut [T], _: &cpal::OutputCallbackInfo| {
                let t0 = Instant::now();
                while let Ok(m) = rx.try_recv() {
                    handle_msg(&mut renderer, m, &shared);
                }
                let frames = data.len() / channels.max(1);
                if bl.len() < frames {
                    bl.resize(frames, 0.0);
                    br.resize(frames, 0.0);
                }
                renderer.render(&mut bl[..frames], &mut br[..frames]);
                for (i, frame) in data.chunks_mut(channels).enumerate() {
                    for (c, s) in frame.iter_mut().enumerate() {
                        let v = match (channels, c) {
                            (1, _) => (bl[i] + br[i]) * 0.5,
                            (_, 0) => bl[i],
                            (_, 1) => br[i],
                            _ => 0.0,
                        };
                        *s = T::from_sample(v.clamp(-1.0, 1.0));
                    }
                }
                publish(&mut renderer, &shared);
                let budget = frames as f32 / sr;
                let load = t0.elapsed().as_secs_f32() / budget.max(1e-6);
                shared.load_bits.store(load.to_bits(), Ordering::Relaxed);
            },
            |e| eprintln!("[ondina] stream error: {e}"),
            None,
        )
        .map_err(|e| e.to_string())
}

fn null_device(rx: &Receiver<EngineMsg>, shared: &Arc<Shared>, project: Arc<Project>, pool: Arc<AudioPool>, beat: f64, stop: &Receiver<()>) {
    let sr = 48000u32;
    shared.sample_rate.store(sr, Ordering::Relaxed);
    *shared.device_name.lock() = "Null device (no audio output)".into();
    let mut r = Renderer::new(sr as f32, project, pool);
    r.beat = beat;
    let frames = 480;
    let mut l = vec![0.0; frames];
    let mut rr = vec![0.0; frames];
    let period = Duration::from_secs_f64(frames as f64 / sr as f64);
    let mut next = Instant::now();
    loop {
        while let Ok(m) = rx.try_recv() {
            handle_msg(&mut r, m, shared);
        }
        r.render(&mut l, &mut rr);
        publish(&mut r, shared);
        next += period;
        let now = Instant::now();
        let wait = if next > now { next - now } else {
            next = now;
            Duration::from_millis(0)
        };
        if stop.recv_timeout(wait).is_ok() {
            return;
        }
    }
}

fn open_input(name: Option<String>, shared: &Arc<Shared>) -> Result<cpal::Stream, String> {
    let host = cpal::default_host();
    let device = match &name {
        Some(n) => host.input_devices().map_err(|e| e.to_string())?.find(|d| d.name().map(|x| x.contains(n.as_str())).unwrap_or(false)).ok_or_else(|| format!("input device '{n}' not found"))?,
        None => host.default_input_device().ok_or("no default input device")?,
    };
    let config = device.default_input_config().map_err(|e| e.to_string())?;
    shared.input_channels.store(config.channels() as u32, Ordering::Relaxed);
    shared.input_sample_rate.store(config.sample_rate().0, Ordering::Relaxed);
    *shared.input_device_name.lock() = device.name().unwrap_or_default();
    let sc: cpal::StreamConfig = config.clone().into();
    let sh = shared.clone();
    let err = |e| eprintln!("[ondina] input error: {e}");
    let stream = match config.sample_format() {
        cpal::SampleFormat::F32 => device.build_input_stream(&sc, move |d: &[f32], _: &_| sh.input_buffer.lock().extend_from_slice(d), err, None),
        cpal::SampleFormat::I16 => device.build_input_stream(&sc, move |d: &[i16], _: &_| sh.input_buffer.lock().extend(d.iter().map(|x| *x as f32 / 32768.0)), err, None),
        cpal::SampleFormat::I32 => device.build_input_stream(&sc, move |d: &[i32], _: &_| sh.input_buffer.lock().extend(d.iter().map(|x| *x as f32 / 2147483648.0)), err, None),
        f => return Err(format!("unsupported input format {f:?}")),
    }
    .map_err(|e| e.to_string())?;
    stream.play().map_err(|e| e.to_string())?;
    Ok(stream)
}
