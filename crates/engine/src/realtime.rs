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
use ondina_dsp::denormal::FlushToZero;
use cpal::traits::{DeviceTrait, HostTrait, StreamTrait};
use cpal::{FromSample, SizedSample};
use crossbeam_channel::{unbounded, Receiver, Sender};
use parking_lot::Mutex;

use crate::pool::AudioPool;
use crate::renderer::{LiveAction, LiveLogEntry, RecordedEvent, Renderer};

pub enum EngineMsg {
    Project(Arc<Project>),
    Pool(Arc<AudioPool>),
    Play { count_in_beats: f64, generation: u64 },
    Stop { generation: u64 },
    Locate { beat: f64, generation: u64 },
    Live(Id, MidiKind),
    Panic,
    Record(bool),
    LiveLaunch { cell: Id, generation: u64 },
    LiveScene { scene: usize, generation: u64 },
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

/// A transport change asked by the control thread. Until the audio thread publishes a block rendered after it,
/// readers see the asked value instead of the published one, so a block that was already being rendered cannot
/// show the old state again.
///
/// Why it holds with atomics only: `publish` stores the published values first and the applied generation after
/// them (Release), so a reader that sees `applied >= asked` (Acquire) also sees a published value that contains the
/// command. Otherwise the reader takes `value`, stored before the Release increment of `asked` it has just seen.
/// `publish` never has to compare generations, so there is no check-then-act window.
#[derive(Default)]
struct Request {
    asked: AtomicU64,
    applied: AtomicU64,
    value: AtomicU64,
}

impl Request {
    /// Store the asked value, then the new generation (Release: whoever sees the generation sees the value).
    fn ask(&self, value: u64) -> u64 {
        self.value.store(value, Ordering::Relaxed);
        self.asked.fetch_add(1, Ordering::Release) + 1
    }

    /// The asked value while the audio thread has not yet published the generation that applies it.
    fn pending(&self) -> Option<u64> {
        let asked = self.asked.load(Ordering::Acquire);
        if self.applied.load(Ordering::Acquire) >= asked {
            None
        } else {
            Some(self.value.load(Ordering::Relaxed))
        }
    }

    /// Called by `publish` after the published values are stored (Release orders them before the generation).
    fn mark_applied(&self, generation: u64) {
        self.applied.store(generation, Ordering::Release);
    }

    fn applied(&self) -> u64 {
        self.applied.load(Ordering::Acquire)
    }
}

/// The newest transport generations the renderer has applied; `publish` hands them to `Shared` after the values.
#[derive(Default, Clone, Copy)]
struct Applied {
    locate: u64,
    run: u64,
}

impl Applied {
    /// A renderer that replaces another (device change) starts from what the old one last published.
    fn from_shared(shared: &Shared) -> Applied {
        Applied { locate: shared.locate.applied(), run: shared.run.applied() }
    }
}

#[derive(Default)]
pub struct Shared {
    playing: AtomicBool,
    locate: Request,
    run: Request,
    pub recording: AtomicBool,
    beat_bits: AtomicU64,
    pub sample_rate: AtomicU32,
    load_bits: AtomicU32,
    overloads: AtomicU32,
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
    /// The state the last transport command asked for until the audio thread has applied it, then the published one.
    pub fn beat(&self) -> f64 {
        match self.locate.pending() {
            Some(bits) => f64::from_bits(bits),
            None => f64::from_bits(self.beat_bits.load(Ordering::Relaxed)),
        }
    }
    /// The state the last transport command asked for until the audio thread has applied it, then the published one.
    pub fn is_playing(&self) -> bool {
        match self.run.pending() {
            Some(v) => v != 0,
            None => self.playing.load(Ordering::Relaxed),
        }
    }
    fn set_beat(&self, b: f64) {
        self.beat_bits.store(b.to_bits(), Ordering::Relaxed);
    }
    /// DSP load as a fraction of the available callback time.
    pub fn load(&self) -> f32 {
        f32::from_bits(self.load_bits.load(Ordering::Relaxed))
    }
    /// Audio blocks since the last Play that took longer than the time the device gives them (each can be heard
    /// as a click or a dropout).
    pub fn overloads(&self) -> u32 {
        self.overloads.load(Ordering::Relaxed)
    }
    /// Store the DSP load of the block just rendered and count it when it ran out of time. Real-time safe: only
    /// atomics.
    pub fn record_block_load(&self, load: f32) {
        self.load_bits.store(load.to_bits(), Ordering::Relaxed);
        if load > OVERLOAD_LOAD {
            self.overloads.fetch_add(1, Ordering::Relaxed);
        }
    }
}

/// A block whose DSP load goes over this share of its time budget missed the device deadline.
const OVERLOAD_LOAD: f32 = 1.0;

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
        self.shared.overloads.store(0, Ordering::Relaxed);
        if let Some(b) = from {
            self.locate(b);
        }
        let generation = self.shared.run.ask(1);
        self.send(EngineMsg::Play { count_in_beats, generation });
    }

    pub fn stop(&self) {
        let generation = self.shared.run.ask(0);
        self.send(EngineMsg::Stop { generation });
    }

    pub fn locate(&self, beat: f64) {
        // the same clamp as `Renderer::locate`, so the asked and the published beat agree
        let beat = beat.max(0.0);
        let generation = self.shared.locate.ask(beat.to_bits());
        self.send(EngineMsg::Locate { beat, generation });
    }

    pub fn live(&self, track: Id, kind: MidiKind) {
        self.send(EngineMsg::Live(track, kind));
    }

    pub fn panic(&self) {
        self.send(EngineMsg::Panic);
    }

    pub fn live_launch(&self, cell: Id) {
        let generation = self.shared.run.ask(1);
        self.send(EngineMsg::LiveLaunch { cell, generation });
    }

    pub fn live_scene(&self, scene: usize) {
        let generation = self.shared.run.ask(1);
        self.send(EngineMsg::LiveScene { scene, generation });
    }

    pub fn live_stop(&self, track: Option<Id>) {
        self.send(EngineMsg::LiveStop(track));
    }

    pub fn position(&self) -> f64 {
        self.shared.beat()
    }

    pub fn is_playing(&self) -> bool {
        self.shared.is_playing()
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

/// Apply a message to the renderer. Transport generations are only noted here: `publish` marks them applied after
/// it has stored the values of a block rendered with them.
fn handle_msg(r: &mut Renderer, m: EngineMsg, applied: &mut Applied) {
    match m {
        EngineMsg::Project(p) => r.set_project(p),
        EngineMsg::Pool(p) => r.set_pool(p),
        EngineMsg::Play { count_in_beats, generation } => {
            r.count_in_beats = count_in_beats;
            r.play();
            applied.run = applied.run.max(generation);
        }
        EngineMsg::Stop { generation } => {
            r.stop();
            applied.run = applied.run.max(generation);
        }
        EngineMsg::Locate { beat, generation } => {
            r.locate(beat);
            applied.locate = applied.locate.max(generation);
        }
        EngineMsg::Live(t, k) => r.live_event(t, k),
        EngineMsg::Panic => r.panic(),
        EngineMsg::Record(on) => r.recording = on,
        EngineMsg::LiveLaunch { cell, generation } => {
            r.launch_cell(cell);
            applied.run = applied.run.max(generation);
        }
        EngineMsg::LiveScene { scene, generation } => {
            r.launch_scene(scene);
            applied.run = applied.run.max(generation);
        }
        EngineMsg::LiveStop(t) => r.stop_live(t),
    }
}

fn publish(r: &mut Renderer, shared: &Shared, applied: &Applied) {
    // values first, then the generations they contain (see `Request`)
    shared.set_beat(r.beat);
    shared.playing.store(r.playing, Ordering::Relaxed);
    shared.locate.mark_applied(applied.locate);
    shared.run.mark_applied(applied.run);
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
    let mut applied = Applied::from_shared(&shared);
    device
        .build_output_stream(
            config,
            move |data: &mut [T], _: &cpal::OutputCallbackInfo| {
                let _ftz = FlushToZero::enable();
                let t0 = Instant::now();
                while let Ok(m) = rx.try_recv() {
                    handle_msg(&mut renderer, m, &mut applied);
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
                publish(&mut renderer, &shared, &applied);
                let budget = frames as f32 / sr;
                let load = t0.elapsed().as_secs_f32() / budget.max(1e-6);
                shared.record_block_load(load);
            },
            |e| eprintln!("[ondina] stream error: {e}"),
            None,
        )
        .map_err(|e| e.to_string())
}

fn null_device(rx: &Receiver<EngineMsg>, shared: &Arc<Shared>, project: Arc<Project>, pool: Arc<AudioPool>, beat: f64, stop: &Receiver<()>) {
    let _ftz = FlushToZero::enable();
    let sr = 48000u32;
    shared.sample_rate.store(sr, Ordering::Relaxed);
    *shared.device_name.lock() = "Null device (no audio output)".into();
    let mut r = Renderer::new(sr as f32, project, pool);
    r.beat = beat;
    let mut applied = Applied::from_shared(shared);
    let frames = 480;
    let mut l = vec![0.0; frames];
    let mut rr = vec![0.0; frames];
    let period = Duration::from_secs_f64(frames as f64 / sr as f64);
    let mut next = Instant::now();
    loop {
        while let Ok(m) = rx.try_recv() {
            handle_msg(&mut r, m, &mut applied);
        }
        r.render(&mut l, &mut rr);
        publish(&mut r, shared, &applied);
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn block_load_counts_only_blocks_over_budget() {
        let s = Shared::default();
        s.record_block_load(0.5);
        assert_eq!(s.overloads(), 0);
        assert_eq!(s.load(), 0.5);
        s.record_block_load(1.0);
        assert_eq!(s.overloads(), 0);
        s.record_block_load(1.01);
        assert_eq!(s.overloads(), 1);
        s.record_block_load(3.0);
        assert_eq!(s.overloads(), 2);
        s.record_block_load(f32::NAN);
        assert_eq!(s.overloads(), 2);
    }

    #[test]
    fn play_resets_the_overload_count() {
        let e = Engine::new(Arc::new(Project::new("t")), Arc::new(AudioPool::new()), false);
        e.shared.record_block_load(2.0);
        assert_eq!(e.shared.overloads(), 1);
        e.play(None, 0.0);
        assert_eq!(e.shared.overloads(), 0);
        e.stop();
    }

    #[test]
    fn a_block_rendered_before_a_transport_command_does_not_hide_it() {
        let shared = Shared::default();
        let mut r = Renderer::new(48000.0, Arc::new(Project::new("t")), Arc::new(AudioPool::new()));
        let mut applied = Applied::from_shared(&shared);
        // the audio thread has drained its messages and is rendering at beat 7 while playing...
        r.beat = 7.0;
        r.playing = true;
        // ...when the control thread moves the playhead and stops
        let locate_gen = shared.locate.ask(3.0f64.to_bits());
        let run_gen = shared.run.ask(0);
        publish(&mut r, &shared, &applied);
        assert_eq!(shared.beat(), 3.0);
        assert!(!shared.is_playing());
        // next block: both commands are applied and published
        handle_msg(&mut r, EngineMsg::Locate { beat: 3.0, generation: locate_gen }, &mut applied);
        handle_msg(&mut r, EngineMsg::Stop { generation: run_gen }, &mut applied);
        publish(&mut r, &shared, &applied);
        assert_eq!(shared.beat(), 3.0);
        assert!(!shared.is_playing());
        // from now on readers see what the audio thread publishes
        r.beat = 5.0;
        r.playing = true;
        publish(&mut r, &shared, &applied);
        assert_eq!(shared.beat(), 5.0);
        assert!(shared.is_playing());
    }

    #[test]
    fn an_older_generation_never_lowers_the_applied_one() {
        let mut r = Renderer::new(48000.0, Arc::new(Project::new("t")), Arc::new(AudioPool::new()));
        let mut applied = Applied { locate: 5, run: 0 };
        handle_msg(&mut r, EngineMsg::Locate { beat: 1.0, generation: 3 }, &mut applied);
        assert_eq!(applied.locate, 5);
        assert_eq!(r.beat, 1.0);
    }

    /// Busy-wait instead of sleeping: on Windows a short sleep lasts a whole timer tick.
    fn spin(d: Duration) {
        let t = Instant::now();
        while t.elapsed() < d {
            std::hint::spin_loop();
        }
    }

    #[test]
    fn locate_play_and_stop_are_visible_at_once_on_the_null_device() {
        let e = Engine::new(Arc::new(Project::new("t")), Arc::new(AudioPool::new()), false);
        for i in 0..200 {
            let x = (i % 13) as f64 * 0.75 + 0.5;
            e.locate(x);
            assert_eq!(e.position(), x, "locate {i}");
            spin(Duration::from_micros(250));
        }
        for i in 0..200 {
            e.play(None, 0.0);
            assert!(e.is_playing(), "play {i}");
            spin(Duration::from_micros(250));
            e.stop();
            assert!(!e.is_playing(), "stop {i}");
            spin(Duration::from_micros(250));
        }
        e.play(Some(16.0), 0.0);
        let p = e.position();
        assert!((16.0..17.0).contains(&p), "{p}");
        e.stop();
    }
}

