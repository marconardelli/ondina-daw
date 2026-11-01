//! Hosting of third-party VST3 plug-ins (instruments and effects) through `vst3-host`.
//! Parameters are exposed with ids "p<number>" and normalized values 0..1.

use std::path::Path;

use vst3_host::audio::AudioBuffers;
use vst3_host::midi::MidiChannel;
use vst3_host::{Plugin, Vst3Host};

use crate::{Effect, Instrument, ProcessContext};

const MAX_BLOCK: usize = 4096;

/// Absolute path (plug-ins may change the process working directory when loaded).
pub fn absolute(path: &str) -> String {
    let p = Path::new(path);
    let abs = p.canonicalize().unwrap_or_else(|_| std::env::current_dir().map(|d| d.join(p)).unwrap_or(p.to_path_buf()));
    let s = abs.display().to_string();
    s.strip_prefix(r"\\?\").map(|x| x.to_string()).unwrap_or(s)
}

/// A path on a local drive, made absolute without touching the file system. Shared by the
/// plug-in and scan-folder checks: VST3 plug-ins are native code, and a project file or an MCP
/// client must not be able to make Ondina load one from a network share, a device path or a URL.
fn local_absolute(path: &str) -> Result<String, String> {
    if path.trim().is_empty() {
        return Err("the path is empty".into());
    }
    if path.starts_with("\\\\") || path.starts_with("//") {
        return Err(format!("{path} is a network or device path"));
    }
    // the only colon a local path may have is the drive letter's (C:\...); any other one is a
    // URL scheme or an NTFS alternate stream
    let b = path.as_bytes();
    let drive = b.len() >= 2 && b[0].is_ascii_alphabetic() && b[1] == b':';
    if path.matches(':').count() > usize::from(drive) {
        return Err(format!("{path} looks like a URL or an NTFS stream"));
    }
    // lexical only (GetFullPathNameW): canonicalize would open the file first
    let abs = std::path::absolute(path).map_err(|e| format!("{path}: {e}"))?;
    let s = abs.to_string_lossy();
    if !is_drive_path(&s) {
        return Err(format!("{path} is not on a local drive"));
    }
    Ok(s.into_owned())
}

/// `X:\...` or `X:/...` with no other colon.
fn is_drive_path(s: &str) -> bool {
    let b = s.as_bytes();
    b.len() >= 3 && b[0].is_ascii_alphabetic() && b[1] == b':' && (b[2] == b'\\' || b[2] == b'/') && !s[2..].contains(':')
}

/// The last component names a .vst3 bundle (or the .vst3 file inside one).
fn names_vst3(path: &str) -> bool {
    Path::new(path).file_name().and_then(|n| n.to_str()).is_some_and(|n| n.len() > 5 && n.to_ascii_lowercase().ends_with(".vst3"))
}

/// Check a VST3 plug-in path without touching the file system: it must be on a local drive (no
/// network shares, device paths, URLs or NTFS streams) and name a .vst3 bundle or the .vst3
/// file inside one. Returns the absolute path.
pub fn check_plugin_path(path: &str) -> Result<String, String> {
    let abs = local_absolute(path)?;
    if !names_vst3(&abs) {
        return Err(format!("{path} is not a .vst3 plug-in bundle"));
    }
    Ok(abs)
}

/// The second half of the check, on the path Windows resolved (`canonicalize`): a junction or a
/// link on a local drive can lead to a share, and then the resolved path is `\\?\UNC\...`.
pub fn check_resolved(resolved: &str) -> Result<String, String> {
    let s = resolved.strip_prefix(r"\\?\").unwrap_or(resolved);
    if !is_drive_path(s) || !names_vst3(s) {
        return Err(format!("it leads to {resolved}, which is not a .vst3 bundle on a local drive"));
    }
    Ok(s.to_string())
}

/// The path to hand to the VST3 host: checked, resolved, then checked again. Errors start with
/// "plug-in path refused:".
pub fn resolve_plugin_path(path: &str) -> Result<String, String> {
    let refused = |e: String| format!("plug-in path refused: {e}");
    let abs = check_plugin_path(path).map_err(refused)?;
    match Path::new(&abs).canonicalize() {
        Ok(c) => check_resolved(&c.display().to_string()).map_err(|e| refused(format!("{path}: {e}"))),
        // a path that does not exist: the host reports "not found" itself
        Err(_) => Ok(abs),
    }
}

/// Split scan folders into the ones that may be scanned (absolute, on a local drive) and the
/// skipped ones with the reason. Scanning loads every plug-in it finds, so a folder on a share
/// would run code from the network.
pub fn split_scan_paths(extra: &[String]) -> (Vec<String>, Vec<(String, String)>) {
    let mut kept = Vec::new();
    let mut skipped = Vec::new();
    for p in extra {
        match local_absolute(p) {
            Ok(abs) => kept.push(abs),
            Err(e) => skipped.push((p.clone(), e)),
        }
    }
    (kept, skipped)
}

#[derive(Debug, Clone)]
pub struct Vst3Info {
    pub path: String,
    pub name: String,
    pub vendor: String,
    pub version: String,
    pub category: String,
    pub audio_inputs: u32,
    pub audio_outputs: u32,
    pub has_midi_input: bool,
    pub is_instrument: bool,
}

#[derive(Debug, Clone)]
pub struct Vst3ParamInfo {
    pub id: u32,
    pub name: String,
    pub default: f64,
    pub unit: String,
}

type Job = Box<dyn FnOnce() + Send>;

/// All plug-in loading and destruction happens on one dedicated thread. Plug-in frameworks
/// (JUCE, VSTGUI) bind global state to the thread that first initializes them, so loading
/// from different threads (UI, MCP workers, the audio thread) can deadlock.
fn host_thread() -> &'static std::sync::Mutex<std::sync::mpsc::Sender<Job>> {
    static TX: std::sync::OnceLock<std::sync::Mutex<std::sync::mpsc::Sender<Job>>> = std::sync::OnceLock::new();
    TX.get_or_init(|| {
        let (tx, rx) = std::sync::mpsc::channel::<Job>();
        std::thread::Builder::new()
            .name("ondina-vst3-host".into())
            .spawn(move || loop {
                match rx.recv_timeout(std::time::Duration::from_millis(15)) {
                    Ok(job) => job(),
                    Err(std::sync::mpsc::RecvTimeoutError::Timeout) => {}
                    Err(std::sync::mpsc::RecvTimeoutError::Disconnected) => break,
                }
                service_editors();
            })
            .expect("spawn VST3 host thread");
        std::sync::Mutex::new(tx)
    })
}

/// Run `f` on the VST3 host thread and wait for its result.
pub fn on_host<T: Send + 'static>(f: impl FnOnce() -> T + Send + 'static) -> T {
    let (rtx, rrx) = std::sync::mpsc::channel();
    let job: Job = Box::new(move || {
        let _ = rtx.send(f());
    });
    host_thread().lock().unwrap().send(job).expect("VST3 host thread stopped");
    rrx.recv().expect("VST3 host thread panicked")
}

/// Run `f` and restore the process working directory afterwards (some plug-ins change it).
fn keep_cwd<T>(f: impl FnOnce() -> T) -> T {
    let cwd = std::env::current_dir().ok();
    let r = f();
    if let Some(c) = cwd {
        let _ = std::env::set_current_dir(c);
    }
    r
}

/// Scan standard VST3 folders plus `extra` paths.
pub fn scan(extra: &[String]) -> Result<Vec<Vst3Info>, String> {
    let extra = split_scan_paths(extra).0;
    on_host(move || scan_impl(&extra))
}

fn scan_impl(extra: &[String]) -> Result<Vec<Vst3Info>, String> {
    let mut host = Vst3Host::new().map_err(|e| e.to_string())?;
    for p in extra {
        let _ = host.add_scan_path(p);
    }
    let list = keep_cwd(|| host.discover_plugins()).map_err(|e| e.to_string())?;
    Ok(list
        .into_iter()
        .map(|i| Vst3Info {
            is_instrument: i.category.to_lowercase().contains("instrument") || i.category.to_lowercase().contains("synth") || (i.has_midi_input && i.audio_inputs == 0),
            path: i.path.display().to_string(),
            name: i.name,
            vendor: i.vendor,
            version: i.version,
            category: i.category,
            audio_inputs: i.audio_inputs,
            audio_outputs: i.audio_outputs,
            has_midi_input: i.has_midi_input,
        })
        .collect())
}

/// Load a plug-in just to read its metadata and parameter list.
pub fn probe(path: &str) -> Result<(String, bool, Vec<Vst3ParamInfo>), String> {
    let path = resolve_plugin_path(path)?;
    on_host(move || probe_impl(&path))
}

fn probe_impl(path: &str) -> Result<(String, bool, Vec<Vst3ParamInfo>), String> {
    let mut host = Vst3Host::new().map_err(|e| e.to_string())?;
    let plugin = keep_cwd(|| host.load_plugin(Path::new(path))).map_err(|e| format!("cannot load VST3 '{path}': {e}"))?;
    let info = plugin.info().clone();
    let params = plugin
        .get_parameters()
        .map_err(|e| e.to_string())?
        .into_iter()
        .map(|p| Vst3ParamInfo { id: p.id, name: p.name, default: if p.value.is_finite() { p.value } else { p.default }, unit: p.unit })
        .collect();
    let is_inst = info.category.to_lowercase().contains("instrument") || info.category.to_lowercase().contains("synth") || (info.has_midi_input && info.audio_inputs == 0);
    Ok((info.name, is_inst, params))
}

/// A loaded VST3 plug-in usable as instrument or effect.
pub struct Vst3Processor {
    plugin: std::mem::ManuallyDrop<Plugin>,
    host: std::mem::ManuallyDrop<Vst3Host>,
    buffers: AudioBuffers,
    inputs: usize,
    sr: f32,
    playing: bool,
    bpm: f32,
    pub error: Option<String>,
}

impl Vst3Processor {
    /// Serialize the plug-in's complete state.
    pub fn save_state(&self) -> Result<Vec<u8>, String> {
        self.plugin.save_state().map_err(|e| e.to_string())
    }

    pub fn param_value(&self, id: u32) -> Option<f64> {
        self.plugin.get_parameter(id).ok()
    }

    pub fn load(path: &str, sr: f32, as_instrument: bool) -> Result<Self, String> {
        let path = resolve_plugin_path(path)?;
        on_host(move || Self::load_impl(&path, sr, as_instrument))
    }

    fn load_impl(path: &str, sr: f32, as_instrument: bool) -> Result<Self, String> {
        let mut host = Vst3Host::new().map_err(|e| e.to_string())?;
        let mut plugin = keep_cwd(|| host.load_plugin(Path::new(path))).map_err(|e| format!("cannot load VST3 '{path}': {e}"))?;
        plugin.reconfigure(sr as f64, MAX_BLOCK).map_err(|e| e.to_string())?;
        plugin.start_processing().map_err(|e| e.to_string())?;
        let outs = plugin.output_channel_count().max(2);
        let inputs = if as_instrument { (plugin.info().audio_inputs as usize).min(2) } else { 2 };
        let buffers = AudioBuffers::new(inputs, outs, MAX_BLOCK, sr as f64);
        Ok(Vst3Processor { plugin: std::mem::ManuallyDrop::new(plugin), host: std::mem::ManuallyDrop::new(host), buffers, inputs, sr, playing: false, bpm: 0.0, error: None })
    }

    fn param(&mut self, id: &str, v: f32) {
        if let Some(n) = id.strip_prefix('p').and_then(|x| x.parse::<u32>().ok()) {
            let _ = self.plugin.set_parameter(n, v.clamp(0.0, 1.0) as f64);
        }
    }

    fn sync_transport(&mut self, ctx: &ProcessContext) {
        if (ctx.bpm - self.bpm).abs() > 1e-3 {
            self.bpm = ctx.bpm;
            let _ = self.plugin.set_tempo(ctx.bpm as f64);
        }
        if ctx.playing != self.playing {
            self.playing = ctx.playing;
            let _ = self.plugin.set_playing(ctx.playing);
        }
    }

    /// Process `l`/`r` (as input when effect) producing output; `add` = mix into buffers.
    fn run(&mut self, l: &mut [f32], r: &mut [f32], ctx: &ProcessContext, add: bool) {
        let _ = self.sr;
        self.sync_transport(ctx);
        let mut done = 0;
        while done < l.len() {
            let n = (l.len() - done).min(MAX_BLOCK);
            for ch in self.buffers.inputs.iter_mut().chain(self.buffers.outputs.iter_mut()) {
                ch.resize(n, 0.0);
                ch.fill(0.0);
            }
            self.buffers.block_size = n;
            if self.inputs >= 1 {
                self.buffers.inputs[0][..n].copy_from_slice(&l[done..done + n]);
            }
            if self.inputs >= 2 {
                self.buffers.inputs[1][..n].copy_from_slice(&r[done..done + n]);
            }
            if let Err(e) = self.plugin.process_audio(&mut self.buffers) {
                self.error = Some(e.to_string());
                return;
            }
            let ol = &self.buffers.outputs[0];
            let or = if self.buffers.outputs.len() > 1 { &self.buffers.outputs[1] } else { &self.buffers.outputs[0] };
            for i in 0..n {
                if add {
                    l[done + i] += ol[i];
                    r[done + i] += or[i];
                } else {
                    l[done + i] = ol[i];
                    r[done + i] = or[i];
                }
            }
            done += n;
        }
    }
}

impl Drop for Vst3Processor {
    fn drop(&mut self) {
        // destroy on the host thread: plug-in first, then the host owning its module
        let mut plugin = unsafe { std::mem::ManuallyDrop::take(&mut self.plugin) };
        let host = unsafe { std::mem::ManuallyDrop::take(&mut self.host) };
        let job: Job = Box::new(move || {
            let _ = plugin.stop_processing();
            drop(plugin);
            drop(host);
        });
        if let Ok(tx) = host_thread().lock() {
            let _ = tx.send(job);
        }
    }
}

impl Instrument for Vst3Processor {
    fn set_param(&mut self, id: &str, value: f32) {
        self.param(id, value);
    }
    fn note_on(&mut self, pitch: u8, velocity: u8) {
        let _ = self.plugin.send_midi_note(pitch, velocity, MidiChannel::Ch1);
    }
    fn note_off(&mut self, pitch: u8) {
        let _ = self.plugin.send_midi_note_off(pitch, MidiChannel::Ch1);
    }
    fn control_change(&mut self, cc: u8, value: u8) {
        let _ = self.plugin.send_midi_cc(cc, value, MidiChannel::Ch1);
    }
    fn all_notes_off(&mut self) {
        let _ = self.plugin.midi_panic();
    }
    fn load_state(&mut self, data: &[u8]) {
        if let Err(e) = self.plugin.load_state(data) {
            self.error = Some(e.to_string());
        }
    }
    fn render(&mut self, left: &mut [f32], right: &mut [f32], ctx: &ProcessContext) {
        self.run(left, right, ctx, true);
    }
}

/// Effect wrapper (distinct type so both trait objects can coexist).
pub struct Vst3Effect(pub Vst3Processor);

impl Effect for Vst3Effect {
    fn set_param(&mut self, id: &str, value: f32) {
        self.0.param(id, value);
    }
    fn process(&mut self, left: &mut [f32], right: &mut [f32], ctx: &ProcessContext) {
        self.0.run(left, right, ctx, false);
    }
    fn reset(&mut self) {
        let _ = self.0.plugin.midi_panic();
    }
    fn load_state(&mut self, data: &[u8]) {
        if let Err(e) = self.0.plugin.load_state(data) {
            self.0.error = Some(e.to_string());
        }
    }
}


// ====================================================================== native plug-in editors
//
// Editors run on a dedicated "proxy" instance owned by the host thread (which also pumps the
// window messages). Parameter gestures in the editor are reported as `EditorEvent`s and applied
// to the project, which updates the playing instance; project-side changes are forwarded back.

/// A parameter change made in a native plug-in editor window.
#[derive(Debug, Clone)]
pub struct EditorEvent {
    pub plugin_id: u64,
    pub param: String,
    pub value: f32,
}

/// A new complete state captured from an editor window (program/bank changes etc.).
#[derive(Debug, Clone)]
pub struct EditorState {
    pub plugin_id: u64,
    pub state: Vec<u8>,
}

fn editor_states_store() -> &'static std::sync::Mutex<Vec<EditorState>> {
    static E: std::sync::OnceLock<std::sync::Mutex<Vec<EditorState>>> = std::sync::OnceLock::new();
    E.get_or_init(Default::default)
}

/// Drain complete-state snapshots captured from editor windows.
pub fn take_editor_states() -> Vec<EditorState> {
    std::mem::take(&mut *editor_states_store().lock().unwrap())
}

fn state_hash(b: &[u8]) -> u64 {
    use std::hash::{Hash, Hasher};
    let mut h = std::collections::hash_map::DefaultHasher::new();
    b.hash(&mut h);
    h.finish()
}

struct EditorSlot {
    window: vst3_host::PluginWindow,
    plugin: std::sync::Arc<std::sync::Mutex<Plugin>>,
    host: Vst3Host,
    /// Last known parameter values, to detect changes the plug-in does not report as gestures.
    cache: std::collections::HashMap<u32, f64>,
    ticks: u32,
    state_hash: u64,
}

thread_local! {
    static EDITORS: std::cell::RefCell<std::collections::HashMap<u64, EditorSlot>> = std::cell::RefCell::new(std::collections::HashMap::new());
}

fn editor_events_store() -> &'static std::sync::Mutex<Vec<EditorEvent>> {
    static E: std::sync::OnceLock<std::sync::Mutex<Vec<EditorEvent>>> = std::sync::OnceLock::new();
    E.get_or_init(Default::default)
}

fn open_ids_store() -> &'static std::sync::Mutex<Vec<u64>> {
    static E: std::sync::OnceLock<std::sync::Mutex<Vec<u64>>> = std::sync::OnceLock::new();
    E.get_or_init(Default::default)
}

fn destroy_with_id(id: u64, slot: EditorSlot) {
    if let Ok(p) = slot.plugin.lock() {
        if let Ok(st) = p.save_state() {
            if state_hash(&st) != slot.state_hash {
                editor_states_store().lock().unwrap().push(EditorState { plugin_id: id, state: st });
            }
        }
    }
    destroy(slot);
}

fn destroy(slot: EditorSlot) {
    let EditorSlot { mut window, plugin, host, .. } = slot;
    window.close();
    drop(window);
    drop(plugin);
    drop(host);
}

/// Dispatch all pending Win32 messages of the host thread (editor windows live here).
fn pump_messages() {
    #[cfg(windows)]
    unsafe {
        use windows_sys::Win32::UI::WindowsAndMessaging::{DispatchMessageW, PeekMessageW, TranslateMessage, MSG, PM_REMOVE};
        let mut msg: MSG = std::mem::zeroed();
        let mut n = 0;
        while n < 500 && PeekMessageW(&mut msg, std::ptr::null_mut(), 0, 0, PM_REMOVE) != 0 {
            TranslateMessage(&msg);
            DispatchMessageW(&msg);
            n += 1;
        }
    }
}

fn service_editors() {
    if open_ids_store().lock().map(|v| v.is_empty()).unwrap_or(true) && EDITORS.with(|e| e.borrow().is_empty()) {
        return;
    }
    pump_messages();
    EDITORS.with(|eds| {
        let mut eds = eds.borrow_mut();
        let mut closed = vec![];
        for (id, slot) in eds.iter_mut() {
            let _ = slot.window.service_platform_events();
            slot.ticks += 1;
            if let Ok(mut p) = slot.plugin.try_lock() {
                for e in p.take_parameter_edits() {
                    if let Some(v) = e.value {
                        slot.cache.insert(e.id, v);
                        editor_events_store().lock().unwrap().push(EditorEvent { plugin_id: *id, param: format!("p{}", e.id), value: v as f32 });
                    }
                }
                // every ~1 s capture the complete state (programs, banks...) when it changed
                if slot.ticks % 60 == 0 {
                    if let Ok(st) = p.save_state() {
                        let h = state_hash(&st);
                        if h != slot.state_hash {
                            slot.state_hash = h;
                            editor_states_store().lock().unwrap().push(EditorState { plugin_id: *id, state: st });
                        }
                    }
                }
                // every ~150 ms diff all parameter values against the cache
                if slot.ticks % 10 == 0 {
                    if let Ok(params) = p.get_parameters() {
                        for prm in params {
                            let old = slot.cache.insert(prm.id, prm.value);
                            if let Some(o) = old {
                                if (o - prm.value).abs() > 1e-6 {
                                    editor_events_store().lock().unwrap().push(EditorEvent { plugin_id: *id, param: format!("p{}", prm.id), value: prm.value as f32 });
                                }
                            }
                        }
                    }
                }
            }
            if slot.window.closed_by_user() {
                closed.push(*id);
            }
        }
        for id in closed {
            if let Some(slot) = eds.remove(&id) {
                destroy_with_id(id, slot);
            }
        }
        *open_ids_store().lock().unwrap() = eds.keys().copied().collect();
    });
}

/// Open the native editor of a VST3 plug-in, initialized with the given parameter values.
pub fn open_editor(plugin_id: u64, path: &str, params: Vec<(String, f32)>, state: Option<Vec<u8>>) -> Result<(), String> {
    let path = resolve_plugin_path(path)?;
    on_host(move || {
        if EDITORS.with(|e| e.borrow().contains_key(&plugin_id)) {
            return Ok(());
        }
        let mut host = Vst3Host::new().map_err(|e| e.to_string())?;
        let mut plugin = keep_cwd(|| host.load_plugin(Path::new(&path))).map_err(|e| format!("cannot load VST3 {path}: {e}"))?;
        if !plugin.has_editor() {
            return Err("this plug-in has no editor window".into());
        }
        if let Some(st) = &state {
            let _ = plugin.load_state(st);
        }
        for (k, v) in &params {
            if let Some(n) = k.strip_prefix('p').and_then(|x| x.parse::<u32>().ok()) {
                let _ = plugin.set_parameter(n, *v as f64);
            }
        }
        let plugin = std::sync::Arc::new(std::sync::Mutex::new(plugin));
        let mut window = vst3_host::PluginWindow::new(plugin.clone());
        keep_cwd(|| window.open()).map_err(|e| format!("cannot open editor: {e}"))?;
        let cache = plugin.lock().map(|p| p.get_parameters().map(|v| v.into_iter().map(|x| (x.id, x.value)).collect()).unwrap_or_default()).unwrap_or_default();
        let state_hash = plugin.lock().ok().and_then(|p| p.save_state().ok()).map(|s| state_hash(&s)).unwrap_or(0);
        EDITORS.with(|e| e.borrow_mut().insert(plugin_id, EditorSlot { window, plugin, host, cache, ticks: 0, state_hash }));
        open_ids_store().lock().unwrap().push(plugin_id);
        Ok(())
    })
}

pub fn close_editor(plugin_id: u64) {
    on_host(move || {
        let slot = EDITORS.with(|e| e.borrow_mut().remove(&plugin_id));
        if let Some(slot) = slot {
            destroy_with_id(plugin_id, slot);
        }
        open_ids_store().lock().unwrap().retain(|x| *x != plugin_id);
    })
}

/// Forward a parameter change from the project to an open editor.
pub fn editor_set_param(plugin_id: u64, param: String, value: f32) {
    if !open_editors().contains(&plugin_id) {
        return;
    }
    let job: Job = Box::new(move || {
        EDITORS.with(|e| {
            if let Some(slot) = e.borrow_mut().get_mut(&plugin_id) {
                if let (Some(n), Ok(mut p)) = (param.strip_prefix('p').and_then(|x| x.parse::<u32>().ok()), slot.plugin.lock()) {
                    let _ = p.set_parameter(n, value as f64);
                    slot.cache.insert(n, value as f64);
                }
            }
        });
    });
    if let Ok(tx) = host_thread().lock() {
        let _ = tx.send(job);
    }
}

pub fn open_editors() -> Vec<u64> {
    open_ids_store().lock().map(|v| v.clone()).unwrap_or_default()
}

/// Drain parameter changes made in editor windows.
pub fn take_editor_events() -> Vec<EditorEvent> {
    std::mem::take(&mut *editor_events_store().lock().unwrap())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn check_plugin_path_refuses_network_device_url_and_non_vst3_paths() {
        for bad in [
            "\\\\server\\share\\a.vst3",
            "//server/a.vst3",
            "\\\\?\\C:\\a.vst3",
            "\\\\.\\C:\\a.vst3",
            "/\\server\\share\\a.vst3",
            "http://x/a.vst3",
            "C:\\a.vst3:evil",
            "C:\\plugins\\a.dll",
            "C:\\x.vst3\\payload.dll",
            "",
            "   ",
        ] {
            assert!(check_plugin_path(bad).is_err(), "{bad:?} was accepted");
        }
    }

    #[test]
    fn check_plugin_path_accepts_local_vst3_bundles() {
        let bundle = "C:\\Program Files\\Common Files\\VST3\\X.vst3";
        assert_eq!(check_plugin_path(bundle).unwrap(), bundle);
        let inner = "C:\\Program Files\\Common Files\\VST3\\X.vst3\\Contents\\x86_64-win\\X.vst3";
        assert_eq!(check_plugin_path(inner).unwrap(), inner);
        assert_eq!(check_plugin_path("C:/VST3/X.VST3").unwrap(), "C:\\VST3\\X.VST3");
    }

    #[test]
    fn check_resolved_refuses_shares_and_non_vst3_targets() {
        for bad in ["\\\\?\\UNC\\server\\share\\a.vst3", "\\\\server\\share\\a.vst3", "\\\\?\\Volume{0}\\a.vst3", "\\\\?\\C:\\real\\payload.dll"] {
            let e = check_resolved(bad).unwrap_err();
            assert!(e.contains(bad), "{e}");
        }
    }

    #[test]
    fn check_resolved_strips_the_device_prefix() {
        assert_eq!(check_resolved("\\\\?\\C:\\VST3\\X.vst3").unwrap(), "C:\\VST3\\X.vst3");
    }

    #[test]
    fn split_scan_paths_skips_network_folders() {
        let extra: Vec<String> = ["tools/vst3", "\\\\server\\vst", "C:\\VST", "http://x/vst"].iter().map(|s| s.to_string()).collect();
        let (kept, skipped) = split_scan_paths(&extra);
        assert_eq!(kept.len(), 2, "{kept:?}");
        assert!(Path::new(&kept[0]).is_absolute(), "{kept:?}");
        assert_eq!(Path::new(&kept[0]), std::path::absolute("tools/vst3").unwrap());
        assert_eq!(kept[1], "C:\\VST");
        assert_eq!(skipped.len(), 2, "{skipped:?}");
        assert_eq!(skipped[0].0, "\\\\server\\vst");
        assert_eq!(skipped[1].0, "http://x/vst");
        assert!(skipped.iter().all(|(_, r)| !r.is_empty()));
    }
}
