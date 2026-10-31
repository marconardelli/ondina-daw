//! The session: owns the project, undo history, audio pool and engine, and executes
//! every [`Command`] — whether it comes from the GUI, the MCP server or the AI assistant.

use std::collections::VecDeque;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::Arc;
use std::time::Instant;

use aurora_core::command::Command;
use aurora_core::model::*;
use aurora_core::ops::{self, Outcome};
use aurora_core::{load_project_str, midi_io, save_project_string};
use aurora_dsp::MidiKind;
use aurora_engine::{load_audio_file, render_offline, write_wav, AudioPool, Engine, OfflineOptions};
use parking_lot::Mutex;
use serde::Serialize;
use serde_json::{json, Value};

pub type SharedSession = Arc<Mutex<Session>>;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum Source {
    Gui,
    Mcp,
    Ai,
    Script,
}

#[derive(Debug, Clone, Serialize)]
pub struct LogEntry {
    pub source: Source,
    pub command: String,
    pub ok: bool,
    pub message: String,
    #[serde(skip)]
    pub at: Instant,
}

const UNDO_LIMIT: usize = 200;

pub struct Session {
    pub project: Project,
    pub path: Option<PathBuf>,
    undo: Vec<(Project, String)>,
    redo: Vec<(Project, String)>,
    pub engine: Engine,
    pub pool: AudioPool,
    pub dirty: bool,
    /// Increments on every project change (GUI caches key off this).
    pub revision: u64,
    pub log: VecDeque<LogEntry>,
    pub selected_track: Option<Id>,
    pub selected_region: Option<Id>,
    midi_target: Arc<AtomicU64>,
    midi_conn: Option<midir::MidiInputConnection<()>>,
    pub midi_input_name: Option<String>,
    pub settings: crate::Settings,
    midi_routes: Arc<parking_lot::RwLock<Vec<MidiRoute>>>,
    record_start: f64,
    record_count_in_sec: f64,
    coalesce: Option<(String, Instant)>,
}

impl Session {
    pub fn new(audio: bool) -> Session {
        let project = Project::new("Untitled");
        let pool = AudioPool::new();
        let engine = Engine::new(Arc::new(project.clone()), Arc::new(pool.clone()), audio);
        Session {
            project,
            path: None,
            undo: vec![],
            redo: vec![],
            engine,
            pool,
            dirty: false,
            revision: 1,
            log: VecDeque::new(),
            selected_track: None,
            selected_region: None,
            midi_target: Arc::new(AtomicU64::new(0)),
            midi_conn: None,
            midi_input_name: None,
            settings: crate::Settings::load(),
            midi_routes: Arc::new(parking_lot::RwLock::new(vec![])),
            record_start: 0.0,
            record_count_in_sec: 0.0,
            coalesce: None,
        }
    }

    pub fn shared(audio: bool) -> SharedSession {
        Arc::new(Mutex::new(Session::new(audio)))
    }

    fn commit(&mut self) {
        *self.midi_routes.write() = self.project.midi_routes.clone();
        self.revision += 1;
        self.dirty = true;
        self.engine.set_project(Arc::new(self.project.clone()));
        self.update_midi_target();
    }

    /// Forget undo/redo and mark the document as saved: used when the whole project was replaced.
    fn reset_history(&mut self) {
        self.undo.clear();
        self.redo.clear();
        self.coalesce = None;
        self.dirty = false;
    }

    fn update_midi_target(&self) {
        let armed = self.project.tracks.iter().find(|t| t.record_armed && t.kind == TrackKind::Instrument).map(|t| t.id);
        let sel = self.selected_track.filter(|id| self.project.track(*id).map(|t| t.kind == TrackKind::Instrument).unwrap_or(false));
        self.midi_target.store(armed.or(sel).unwrap_or(0), Ordering::Relaxed);
    }

    pub fn select_track(&mut self, id: Option<Id>) {
        self.selected_track = id;
        self.update_midi_target();
    }

    fn push_log(&mut self, source: Source, command: String, ok: bool, message: String) {
        self.log.push_back(LogEntry { source, command, ok, message, at: Instant::now() });
        while self.log.len() > 500 {
            self.log.pop_front();
        }
    }

    /// Execute a command. Mutating commands create an undo step; opening or creating a project starts a fresh history.
    pub fn execute(&mut self, cmd: Command, source: Source) -> Result<Value, String> {
        self.coalesce = None;
        self.execute_inner(cmd, source, true)
    }

    /// Execute a mutating command, merging it into the previous undo step if it has the same
    /// `key` and happened recently (fader drags, knob turns).
    pub fn execute_coalesced(&mut self, cmd: Command, key: &str) -> Result<Value, String> {
        let merge = matches!(&self.coalesce, Some((k, t)) if k == key && t.elapsed().as_millis() < 1500);
        let r = self.execute_inner(cmd, Source::Gui, !merge);
        self.coalesce = Some((key.to_string(), Instant::now()));
        r
    }

    /// Apply parameter changes made in native plug-in editor windows (one undo step per gesture batch).
    pub fn poll_editor_events(&mut self) -> usize {
        let evs = aurora_dsp::vst3::take_editor_events();
        let states = aurora_dsp::vst3::take_editor_states();
        if evs.is_empty() && states.is_empty() {
            return 0;
        }
        let before = self.project.clone();
        let mut n = 0;
        for st in &states {
            if let Some(p) = self.project.find_plugin_mut(st.plugin_id) {
                p.vst3_state = Some(aurora_core::model::base64_encode(&st.state));
                n += 1;
            }
        }
        for e in &evs {
            if let Some(p) = self.project.find_plugin_mut(e.plugin_id) {
                p.params.insert(e.param.clone(), e.value.clamp(0.0, 1.0));
                n += 1;
            }
        }
        if n > 0 {
            let merge = matches!(&self.coalesce, Some((k, t)) if k == "plugin-editor" && t.elapsed().as_millis() < 1500);
            if !merge {
                self.undo.push((before, "plug-in editor".into()));
            }
            self.coalesce = Some(("plugin-editor".into(), Instant::now()));
            self.redo.clear();
            self.commit();
        }
        n
    }

    fn execute_inner(&mut self, cmd: Command, source: Source, new_undo_step: bool) -> Result<Value, String> {
        self.poll_editor_events();
        // keep open native editors in sync with project-side parameter changes
        match &cmd {
            Command::SetPluginParam { plugin_id, param, value } => aurora_dsp::vst3::editor_set_param(*plugin_id, param.clone(), *value),
            Command::SetPluginParams { plugin_id, params } => {
                for (k, v) in params {
                    aurora_dsp::vst3::editor_set_param(*plugin_id, k.clone(), *v);
                }
            }
            _ => {}
        }
        let name = cmd.name();
        let mutating = cmd.is_mutating();
        // Open and New replace the whole document. An undo step here would bring the old project back
        // with the new file path and audio pool, so they start a fresh history instead.
        let replaces_project = matches!(cmd, Command::OpenProject { .. } | Command::NewProject { .. });
        let before = if mutating { Some(self.project.clone()) } else { None };
        let result = self.dispatch(cmd);
        match &result {
            Ok(v) => {
                if let Some(b) = before {
                    if replaces_project {
                        self.commit();
                        self.reset_history();
                    } else {
                        if new_undo_step {
                            self.undo.push((b, name.clone()));
                            if self.undo.len() > UNDO_LIMIT {
                                self.undo.remove(0);
                            }
                        }
                        self.redo.clear();
                        self.commit();
                    }
                }
                if source != Source::Gui || mutating {
                    let msg = summarize(v);
                    self.push_log(source, name, true, msg);
                }
            }
            Err(e) => {
                if let Some(b) = before {
                    // roll back partial edits
                    self.project = b;
                }
                self.push_log(source, name, false, e.clone());
            }
        }
        result
    }

    pub fn can_undo(&self) -> Option<&str> {
        self.undo.last().map(|x| x.1.as_str())
    }

    pub fn can_redo(&self) -> Option<&str> {
        self.redo.last().map(|x| x.1.as_str())
    }

    fn dispatch(&mut self, cmd: Command) -> Result<Value, String> {
        use Command::*;
        match cmd {
            Undo {} => {
                let (p, name) = self.undo.pop().ok_or("nothing to undo")?;
                let cur = std::mem::replace(&mut self.project, p);
                self.redo.push((cur, name.clone()));
                self.commit();
                Ok(json!({"undone": name}))
            }
            Redo {} => {
                let (p, name) = self.redo.pop().ok_or("nothing to redo")?;
                let cur = std::mem::replace(&mut self.project, p);
                self.undo.push((cur, name.clone()));
                self.commit();
                Ok(json!({"redone": name}))
            }
            NewProject { .. } => {
                let v = match ops::apply(&mut self.project, &cmd)? {
                    Outcome::Done(v) => v,
                    Outcome::NotHandled => return Err("internal".into()),
                };
                self.path = None;
                self.pool.clear();
                self.engine.set_pool(Arc::new(AudioPool::new()));
                self.selected_track = None;
                self.selected_region = None;
                Ok(v)
            }
            OpenProject { path } => self.open(Path::new(&path)),
            SaveProject { path } => {
                let p = match path {
                    Some(p) => PathBuf::from(p),
                    None => self.path.clone().ok_or("project has never been saved: give a path")?,
                };
                self.save(&p)
            }

            // transport
            Play { from_beat } => {
                if self.engine.is_recording() {
                    return Err("already recording".into());
                }
                self.engine.play(from_beat, 0.0);
                Ok(json!({"playing": true, "from_beat": from_beat.unwrap_or(self.engine.position())}))
            }
            Stop {} => {
                let was_recording = self.engine.is_recording();
                self.engine.stop();
                if was_recording {
                    let created = self.finish_recording()?;
                    return Ok(json!({"stopped": true, "recorded_regions": created}));
                }
                Ok(json!({"stopped": true, "position": self.engine.position()}))
            }
            Record { from_beat } => self.start_recording(from_beat),
            SetPlayhead { beat, bar } => {
                let b = match (beat, bar) {
                    (Some(b), _) => b,
                    (None, Some(bar)) => (bar - 1.0).max(0.0) * self.project.time_signature.beats_per_bar(),
                    _ => return Err("give beat or bar".into()),
                };
                self.engine.locate(b);
                Ok(json!({"beat": b, "position": self.project.bar_beat_string(b)}))
            }
            GetTransport {} => Ok(self.transport_json()),
            Panic {} => {
                self.engine.panic();
                Ok(json!({"ok": true}))
            }

            // IO-level mutating commands
            ImportAudio { path, track_id, start } => {
                let file_id = self.load_audio(Path::new(&path))?;
                let track_id = match track_id {
                    Some(t) => t,
                    None => ops::add_track(&mut self.project, TrackKind::Audio, None, None, None, None, None)?,
                };
                let f = self.project.audio_file(file_id).unwrap().clone();
                let start = start.unwrap_or(0.0);
                let len_beats = self.project.tempo.sec_to_beat(self.project.tempo.beat_to_sec(start) + f.duration_sec) - start;
                let rid = self.project.alloc_id();
                let t = self.project.track_mut(track_id).ok_or("track not found")?;
                if t.kind != TrackKind::Audio {
                    return Err("audio files need an audio track".into());
                }
                if t.regions.is_empty() && t.name.starts_with("Audio ") {
                    t.name = f.name.clone();
                }
                t.regions.push(Region { id: rid, name: f.name.clone(), start, length: len_beats, loop_count: 1, muted: false, color: None, content: RegionContent::Audio(AudioContent { file_id, offset_sec: 0.0, gain_db: 0.0, fade_in: 0.0, fade_out: 0.0, reverse: false }), take: None });
                Ok(json!({"track_id": track_id, "region_id": rid, "file_id": file_id, "duration_sec": f.duration_sec, "length_beats": len_beats}))
            }
            SetSamplerSample { plugin_id, path } => {
                let file_id = self.load_audio(Path::new(&path))?;
                let p = self.project.find_plugin_mut(plugin_id).ok_or("plug-in not found")?;
                if p.kind != "sampler" {
                    return Err("plug-in is not a sampler".into());
                }
                p.sample = Some(file_id);
                Ok(json!({"file_id": file_id}))
            }
            ImportMidi { path, at_beat } => {
                let bytes = std::fs::read(&path).map_err(|e| format!("cannot read {path}: {e}"))?;
                let ids = midi_io::import_midi(&mut self.project, &bytes, at_beat.unwrap_or(0.0))?;
                Ok(json!({"track_ids": ids}))
            }
            ExportMidi { path, track_ids } => {
                let bytes = midi_io::export_midi(&self.project, track_ids.as_deref())?;
                std::fs::write(&path, bytes).map_err(|e| format!("cannot write {path}: {e}"))?;
                Ok(json!({"path": path}))
            }
            Bounce { path, start, end, bit_depth, normalize, track_ids } => {
                let start = start.unwrap_or(0.0);
                let end = end.unwrap_or_else(|| self.project.content_end()).max(start + 1.0);
                let sr = self.engine.sample_rate();
                let mut res = render_offline(&self.project, Arc::new(self.pool.clone()), OfflineOptions { sample_rate: sr, start, end, tail_sec: 2.0, auto_tail_max_sec: 0.0, solo: track_ids, analyze: true, keep_audio: true });
                if normalize.unwrap_or(false) {
                    aurora_engine::offline::normalize(&mut res.left, &mut res.right);
                }
                let bits = match bit_depth.unwrap_or(24) {
                    16 => 16,
                    32 => 32,
                    _ => 24,
                };
                write_wav(Path::new(&path), &res.left, &res.right, sr, bits)?;
                let master = res.analysis.as_ref().and_then(|a| a.get(&MASTER_ID)).cloned();
                Ok(json!({"path": path, "duration_sec": res.left.len() as f64 / sr as f64, "sample_rate": sr, "bit_depth": bits, "master_analysis": master}))
            }
            BounceInPlace { track_id } => {
                let t = self.project.track(track_id).ok_or("track not found")?.clone();
                if t.kind == TrackKind::Bus {
                    return Err("cannot bounce a bus in place".into());
                }
                let end = t.regions.iter().map(|r| r.end()).fold(0.0, f64::max);
                if end <= 0.0 {
                    return Err("track has no regions".into());
                }
                let sr = self.engine.sample_rate();
                let mut solo_proj = self.project.clone();
                // render the track pre-master-chain: bypass master inserts and reset master fader
                solo_proj.master.inserts.clear();
                solo_proj.master.volume_db = 0.0;
                if let Some(tt) = solo_proj.track_mut(track_id) {
                    tt.channel.sends.clear();
                    tt.channel.output = Output::Master;
                    tt.channel.mute = false;
                }
                let res = render_offline(&solo_proj, Arc::new(self.pool.clone()), OfflineOptions { sample_rate: sr, start: 0.0, end, tail_sec: 2.0, auto_tail_max_sec: 0.0, solo: Some(vec![track_id]), analyze: false, keep_audio: true });
                let file = self.media_dir().join(format!("{} (bounce {}).wav", sanitize(&t.name), self.project.next_id));
                write_wav(&file, &res.left, &res.right, sr, 32)?;
                let file_id = self.load_audio(&file)?;
                let dur = self.project.audio_file(file_id).unwrap().duration_sec;
                let insert_at = self.project.track_index(track_id).map(|i| i + 1);
                let new_track = ops::add_track(&mut self.project, TrackKind::Audio, Some(format!("{} (bounced)", t.name)), None, None, None, insert_at)?;
                let rid = self.project.alloc_id();
                let len = self.project.tempo.sec_to_beat(dur);
                let nt = self.project.track_mut(new_track).unwrap();
                nt.channel.volume_db = 0.0;
                nt.regions.push(Region { id: rid, name: t.name.clone(), start: 0.0, length: len, loop_count: 1, muted: false, color: None, content: RegionContent::Audio(AudioContent { file_id, offset_sec: 0.0, gain_db: 0.0, fade_in: 0.0, fade_out: 0.0, reverse: false }), take: None });
                self.project.track_mut(track_id).unwrap().channel.mute = true;
                Ok(json!({"track_id": new_track, "region_id": rid, "file": file.display().to_string()}))
            }
            AnalyzeMix { start, end } => self.analyze(start, end),
            AutoMaster { target_lufs, character } => self.auto_master(target_lufs.unwrap_or(-14.0), character.as_deref()),
            GetMeters {} => {
                let m = self.engine.shared.meters.lock().clone();
                let mut out = serde_json::Map::new();
                for (id, p) in m {
                    let name = if id == MASTER_ID { "master".to_string() } else { self.project.track(id).map(|t| t.name.clone()).unwrap_or_default() };
                    out.insert(id.to_string(), json!({"name": name, "peak_db_l": db(p[0]), "peak_db_r": db(p[1])}));
                }
                Ok(Value::Object(out))
            }
            ListAudioDevices {} => Ok(serde_json::to_value(self.engine.list_devices()).unwrap()),
            SetAudioDevice { output, input } => {
                self.engine.set_devices(output, input);
                std::thread::sleep(std::time::Duration::from_millis(200));
                self.engine.set_project(Arc::new(self.project.clone()));
                self.engine.set_pool(Arc::new(self.pool.clone()));
                Ok(serde_json::to_value(self.engine.list_devices()).unwrap())
            }
            ListMidiInputs {} => {
                let mi = midir::MidiInput::new("aurora-list").map_err(|e| e.to_string())?;
                let names: Vec<String> = mi.ports().iter().filter_map(|p| mi.port_name(p).ok()).collect();
                Ok(json!({"inputs": names, "connected": self.midi_input_name}))
            }
            ConnectMidiInput { name } => self.connect_midi(name),
            PreviewNote { track_id, pitch, velocity, duration } => {
                self.engine.live(track_id, MidiKind::NoteOn { pitch, velocity: velocity.unwrap_or(100) });
                let tx = self.engine_sender();
                let d = duration.unwrap_or(0.5).clamp(0.01, 10.0);
                std::thread::spawn(move || {
                    std::thread::sleep(std::time::Duration::from_secs_f32(d));
                    let _ = tx.send(aurora_engine::EngineMsg::Live(track_id, MidiKind::NoteOff { pitch }));
                });
                Ok(json!({"ok": true}))
            }
            other => {
                if let Some(r) = self.dispatch_ext(&other) {
                    return r;
                }
                match ops::apply(&mut self.project, &other)? {
                    Outcome::Done(v) => Ok(v),
                    Outcome::NotHandled => Err(format!("command '{}' is not implemented", other.name())),
                }
            }
        }
    }

    pub fn engine_sender(&self) -> crossbeam_channel::Sender<aurora_engine::EngineMsg> {
        self.engine.sender()
    }

    pub fn transport_json(&self) -> Value {
        let b = self.engine.position();
        json!({
            "playing": self.engine.is_playing(),
            "recording": self.engine.is_recording(),
            "beat": (b * 1000.0).round() / 1000.0,
            "position": self.project.bar_beat_string(b),
            "seconds": (self.project.tempo.beat_to_sec(b) * 1000.0).round() / 1000.0,
            "bpm": self.project.tempo.bpm_at(b),
            "cycle": {"enabled": self.project.cycle.enabled, "start": self.project.cycle.start, "end": self.project.cycle.end},
            "metronome": self.project.metronome,
            "dsp_load": (self.engine.shared.load() * 100.0).round() / 100.0,
            "sample_rate": self.engine.sample_rate(),
        })
    }

    /// Directory for recordings and bounces.
    pub fn media_dir(&self) -> PathBuf {
        let base = match &self.path {
            Some(p) => p.parent().map(|d| d.to_path_buf()).unwrap_or_default().join(format!("{} Media", p.file_stem().and_then(|s| s.to_str()).unwrap_or("Project"))),
            None => default_documents_dir().join("Aurora").join("Recordings"),
        };
        let _ = std::fs::create_dir_all(&base);
        base
    }

    /// Decode an audio file into the pool and register it in the project.
    pub fn load_audio(&mut self, path: &Path) -> Result<Id, String> {
        let canonical = path.canonicalize().unwrap_or_else(|_| path.to_path_buf());
        let path_str = aurora_dsp::vst3::absolute(&canonical.display().to_string());
        if let Some(f) = self.project.audio_files.iter().find(|f| f.path == path_str) {
            if self.pool.contains_key(&f.id) {
                return Ok(f.id);
            }
        }
        let data = load_audio_file(&canonical)?;
        let id = self.project.alloc_id();
        self.project.audio_files.push(AudioFileRef {
            id,
            path: path_str,
            name: canonical.file_stem().and_then(|s| s.to_str()).unwrap_or("audio").to_string(),
            sample_rate: data.sample_rate,
            channels: data.channels.len() as u16,
            duration_sec: data.duration_sec(),
        });
        self.pool.insert(id, Arc::new(data));
        self.engine.set_pool(Arc::new(self.pool.clone()));
        Ok(id)
    }

    fn open(&mut self, path: &Path) -> Result<Value, String> {
        let text = std::fs::read_to_string(path).map_err(|e| format!("cannot read {}: {e}", path.display()))?;
        let project = load_project_str(&text)?;
        let mut pool = AudioPool::new();
        let mut missing = vec![];
        for f in &project.audio_files {
            match load_audio_file(Path::new(&f.path)) {
                Ok(d) => {
                    pool.insert(f.id, Arc::new(d));
                }
                Err(_) => missing.push(f.path.clone()),
            }
        }
        self.engine.stop();
        self.project = project;
        self.pool = pool;
        self.engine.set_pool(Arc::new(self.pool.clone()));
        self.engine.locate(0.0);
        self.path = Some(path.to_path_buf());
        self.selected_track = None;
        self.selected_region = None;
        Ok(json!({"opened": path.display().to_string(), "missing_audio_files": missing, "summary": ops::project_summary(&self.project)}))
    }

    fn save(&mut self, path: &Path) -> Result<Value, String> {
        let mut path = path.to_path_buf();
        if path.extension().is_none() {
            path.set_extension(aurora_core::PROJECT_EXTENSION);
        }
        if let Some(parent) = path.parent() {
            if !parent.as_os_str().is_empty() {
                std::fs::create_dir_all(parent).map_err(|e| e.to_string())?;
            }
        }
        let s = save_project_string(&self.project)?;
        std::fs::write(&path, s).map_err(|e| format!("cannot write {}: {e}", path.display()))?;
        self.path = Some(path.clone());
        self.dirty = false;
        self.revision += 1;
        Ok(json!({"saved": path.display().to_string()}))
    }

    fn start_recording(&mut self, from_beat: Option<f64>) -> Result<Value, String> {
        let armed: Vec<&Track> = self.project.tracks.iter().filter(|t| t.record_armed).collect();
        if armed.is_empty() {
            return Err("no track is record-armed (use set_record_arm)".into());
        }
        let capture_audio = armed.iter().any(|t| t.kind == TrackKind::Audio);
        let start = from_beat.unwrap_or(self.engine.position());
        let count_in = self.project.count_in_bars as f64 * self.project.time_signature.beats_per_bar();
        self.record_start = start;
        self.record_count_in_sec = count_in * 60.0 / self.project.tempo.bpm_at(start);
        self.engine.start_recording(capture_audio)?;
        self.engine.play(Some(start), count_in);
        Ok(json!({"recording": true, "from_beat": start, "count_in_beats": count_in, "armed_tracks": armed.iter().map(|t| t.id).collect::<Vec<_>>()}))
    }

    fn finish_recording(&mut self) -> Result<Value, String> {
        let (midi, audio) = self.engine.stop_recording();
        let before = self.project.clone();
        let mut created = vec![];
        let bpb = self.project.time_signature.beats_per_bar();
        // MIDI
        let armed_inst: Vec<Id> = self.project.tracks.iter().filter(|t| t.record_armed && t.kind == TrackKind::Instrument).map(|t| t.id).collect();
        for tid in armed_inst {
          // split into takes where the cycle wrapped (beat jumps backwards)
          let evs: Vec<&aurora_engine::RecordedEvent> = midi.iter().filter(|e| e.track_id == tid).collect();
          let mut passes: Vec<Vec<&aurora_engine::RecordedEvent>> = vec![vec![]];
          let mut prev = f64::MIN;
          for e in evs {
              if e.beat < prev - 0.5 {
                  passes.push(vec![]);
              }
              prev = e.beat;
              passes.last_mut().unwrap().push(e);
          }
          let multi = passes.iter().filter(|p| p.iter().any(|e| matches!(e.kind, MidiKind::NoteOn { .. }))).count() > 1;
          let n_passes = passes.len();
          for (take_idx, pass) in passes.into_iter().enumerate() {
            let mut open: std::collections::HashMap<u8, (f64, u8)> = Default::default();
            let mut notes = vec![];
            let mut cc = vec![];
            let mut last = 0.0f64;
            for e in pass {
                last = last.max(e.beat);
                match e.kind {
                    MidiKind::NoteOn { pitch, velocity } => {
                        open.insert(pitch, (e.beat, velocity));
                    }
                    MidiKind::NoteOff { pitch } => {
                        if let Some((s, v)) = open.remove(&pitch) {
                            notes.push(Note { pitch, velocity: v, start: s, length: (e.beat - s).max(0.02) });
                        }
                    }
                    MidiKind::Cc { controller, value } => cc.push(CcEvent { beat: e.beat, controller, value }),
                }
            }
            for (p, (s, v)) in open {
                notes.push(Note { pitch: p, velocity: v, start: s, length: (last - s).max(0.25) });
            }
            if notes.is_empty() {
                continue;
            }
            let first = notes.iter().map(|n| n.start).fold(f64::MAX, f64::min);
            let end = notes.iter().map(|n| n.start + n.length).fold(0.0, f64::max);
            let rstart = (first / bpb).floor() * bpb;
            let rend = ((end / bpb).ceil() * bpb).max(rstart + bpb);
            for n in &mut notes {
                n.start -= rstart;
            }
            for c in &mut cc {
                c.beat -= rstart;
            }
            let rid = self.project.alloc_id();
            let t = self.project.track_mut(tid).unwrap();
            let name = if multi { format!("{} take {}", t.name, take_idx + 1) } else { format!("{} rec", t.name) };
            let muted = multi && take_idx + 1 < n_passes;
            t.regions.push(Region { id: rid, name, start: rstart, length: rend - rstart, loop_count: 1, muted, color: None, content: RegionContent::Midi(MidiContent { notes, cc, ..Default::default() }), take: if multi { Some(take_idx as u32 + 1) } else { None } });
            created.push(rid);
          }
        }
        // Audio
        if let Some((buf, ch, sr)) = audio {
            let ch = ch.max(1) as usize;
            let skip = (self.record_count_in_sec * sr as f64) as usize * ch;
            let data = if buf.len() > skip { &buf[skip..] } else { &[][..] };
            if !data.is_empty() {
                let frames = data.len() / ch;
                let mut l = Vec::with_capacity(frames);
                let mut r = Vec::with_capacity(frames);
                for f in data.chunks(ch) {
                    l.push(f[0]);
                    r.push(if ch > 1 { f[1] } else { f[0] });
                }
                let file = self.media_dir().join(format!("Recording {}.wav", self.project.next_id));
                write_wav(&file, &l, &r, sr, 32)?;
                let file_id = self.load_audio(&file)?;
                let dur = frames as f64 / sr as f64;
                let start = self.record_start;
                let len = self.project.tempo.sec_to_beat(self.project.tempo.beat_to_sec(start) + dur) - start;
                let armed_audio: Vec<Id> = self.project.tracks.iter().filter(|t| t.record_armed && t.kind == TrackKind::Audio).map(|t| t.id).collect();
                // cycle recording: one take per pass, all pointing into the same file
                let cy = self.project.cycle;
                let tempo = self.project.tempo.clone();
                let mut passes: Vec<(f64, f64, f64)> = vec![]; // (start beat, length beats, file offset sec)
                if cy.enabled && start < cy.end && start + len > cy.end + 0.05 {
                    let first_len = cy.end - start;
                    passes.push((start, first_len, 0.0));
                    let mut off = tempo.beat_to_sec(cy.end) - tempo.beat_to_sec(start);
                    let cyc_sec = tempo.beat_to_sec(cy.end) - tempo.beat_to_sec(cy.start);
                    while off < dur - 0.05 {
                        let remain = dur - off;
                        let l = if remain >= cyc_sec { cy.end - cy.start } else { tempo.sec_to_beat(tempo.beat_to_sec(cy.start) + remain) - cy.start };
                        passes.push((cy.start, l, off));
                        off += cyc_sec;
                    }
                } else {
                    passes.push((start, len, 0.0));
                }
                let multi = passes.len() > 1;
                for tid in armed_audio {
                    for (k, (ps, pl, po)) in passes.iter().enumerate() {
                        let rid = self.project.alloc_id();
                        let name = if multi { format!("Take {}", k + 1) } else { "Recording".into() };
                        self.project.track_mut(tid).unwrap().regions.push(Region { id: rid, name, start: *ps, length: *pl, loop_count: 1, muted: multi && k + 1 < passes.len(), color: None, content: RegionContent::Audio(AudioContent { file_id, offset_sec: *po, gain_db: 0.0, fade_in: 0.0, fade_out: 0.0, reverse: false }), take: if multi { Some(k as u32 + 1) } else { None } });
                        created.push(rid);
                    }
                }
            }
        }
        if !created.is_empty() {
            self.undo.push((before, "record".into()));
            self.redo.clear();
            self.commit();
        }
        Ok(json!(created))
    }

    fn analyze(&mut self, start: Option<f64>, end: Option<f64>) -> Result<Value, String> {
        let start = start.unwrap_or(0.0);
        let end = end.unwrap_or_else(|| self.project.content_end());
        if end <= start {
            return Err("project is empty: nothing to analyze".into());
        }
        let res = render_offline(&self.project, Arc::new(self.pool.clone()), OfflineOptions { sample_rate: 44100, start, end, tail_sec: 1.0, auto_tail_max_sec: 0.0, solo: None, analyze: true, keep_audio: false });
        let a = res.analysis.unwrap_or_default();
        let master = a.get(&MASTER_ID).cloned();
        let mut tracks = vec![];
        for t in &self.project.tracks {
            if let Some(r) = a.get(&t.id) {
                tracks.push(json!({"track_id": t.id, "name": t.name, "analysis": r}));
            }
        }
        let mut advice: Vec<String> = vec![];
        if let Some(m) = &master {
            if m.silent {
                advice.push("The mix is silent: check mutes, regions and instruments.".into());
            } else {
                if m.true_peak_dbtp > -0.5 {
                    advice.push(format!("True peak {:.1} dBTP is too hot; add a limiter on the master with ceiling -1 dB or lower the master fader.", m.true_peak_dbtp));
                }
                if m.integrated_lufs < -20.0 {
                    advice.push(format!("Integrated loudness {:.1} LUFS is quiet for streaming (-14 typical); consider auto_master.", m.integrated_lufs));
                } else if m.integrated_lufs > -8.0 {
                    advice.push(format!("Integrated loudness {:.1} LUFS is very loud; dynamics may be squashed.", m.integrated_lufs));
                }
                let [lo, mid, hi] = m.spectral_balance_pct;
                if lo > 60.0 {
                    advice.push(format!("Low end is dominant ({lo:.0}% of energy below 250 Hz): high-pass non-bass tracks or lower bass/kick."));
                }
                if hi > 25.0 {
                    advice.push(format!("High frequencies are strong ({hi:.0}% above 4 kHz): the mix may sound harsh."));
                }
                if mid < 20.0 {
                    advice.push("Midrange is thin: melodic parts may lack presence.".into());
                }
                if m.stereo_correlation > 0.97 {
                    advice.push("The mix is almost mono: pan tracks or add stereo effects.".into());
                }
                if m.stereo_correlation < 0.1 {
                    advice.push("Low stereo correlation: possible phase problems in mono playback.".into());
                }
                if m.crest_factor_db < 6.0 {
                    advice.push("Low crest factor: the mix is heavily compressed.".into());
                }
            }
        }
        for t in &tracks {
            let r: aurora_engine::AnalysisReport = serde_json::from_value(t["analysis"].clone()).unwrap_or_else(|_| unreachable_report());
            if r.peak_dbfs > -0.1 && !r.silent {
                advice.push(format!("Track '{}' clips (peak {:.1} dBFS): lower its fader.", t["name"].as_str().unwrap_or(""), r.peak_dbfs));
            }
        }
        Ok(json!({"range_beats": [start, end], "master": master, "tracks": tracks, "advice": advice}))
    }

    fn auto_master(&mut self, target: f32, character: Option<&str>) -> Result<Value, String> {
        // remove previous auto chain
        self.project.master.inserts.retain(|p| p.preset.as_deref().map(|s| !s.starts_with("Auto ")).unwrap_or(true));
        let first = self.analyze(None, None)?;
        let m: aurora_engine::AnalysisReport = serde_json::from_value(first["master"].clone()).map_err(|_| "analysis failed")?;
        if m.silent {
            return Err("the mix is silent".into());
        }
        let warm = matches!(character, Some("warm"));
        let bright = matches!(character, Some("bright") | Some("punchy"));
        let [lo, _mid, hi] = m.spectral_balance_pct;
        let mut eq = ops::new_plugin(&mut self.project, "channel_eq", Some("Master Polish"))?;
        let low_adj = if lo > 55.0 { -1.5 } else if lo < 30.0 { 1.5 } else { 0.5 } + if warm { 1.0 } else { 0.0 };
        let high_adj = if hi > 20.0 { -1.0 } else if hi < 8.0 { 2.0 } else { 1.0 } + if bright { 1.0 } else if warm { -0.5 } else { 0.0 };
        eq.params.insert("low_gain".into(), low_adj);
        eq.params.insert("high_gain".into(), high_adj);
        eq.preset = Some("Auto EQ".into());
        let mut comp = ops::new_plugin(&mut self.project, "compressor", Some("Master Bus Glue"))?;
        comp.preset = Some("Auto Glue".into());
        let mut lim = ops::new_plugin(&mut self.project, "limiter", None)?;
        lim.params.insert("ceiling".into(), -1.0);
        lim.params.insert("gain".into(), 0.0);
        lim.preset = Some("Auto Limiter".into());
        let lim_id = lim.id;
        self.project.master.inserts.push(eq);
        self.project.master.inserts.push(comp);
        self.project.master.inserts.push(lim);
        // two refinement passes on limiter gain
        let mut measured = 0.0;
        for _ in 0..3 {
            let a = self.analyze(None, None)?;
            let r: aurora_engine::AnalysisReport = serde_json::from_value(a["master"].clone()).map_err(|_| "analysis failed")?;
            measured = r.integrated_lufs;
            let delta = target - r.integrated_lufs;
            if delta.abs() < 0.5 {
                break;
            }
            let p = self.project.find_plugin_mut(lim_id).unwrap();
            let g = (p.param("gain") + delta).clamp(0.0, 24.0);
            p.params.insert("gain".into(), g);
        }
        let gain = self.project.find_plugin(lim_id).map(|p| p.param("gain")).unwrap_or(0.0);
        Ok(json!({"target_lufs": target, "measured_lufs": measured, "limiter_gain_db": gain, "chain": ["channel_eq (Auto EQ)", "compressor (Auto Glue)", "limiter (Auto Limiter)"], "before": m}))
    }

    fn connect_midi(&mut self, name: Option<String>) -> Result<Value, String> {
        self.midi_conn = None;
        let mi = midir::MidiInput::new("aurora").map_err(|e| e.to_string())?;
        let ports = mi.ports();
        let port = match &name {
            Some(n) => ports.iter().find(|p| mi.port_name(p).map(|x| x.to_lowercase().contains(&n.to_lowercase())).unwrap_or(false)),
            None => ports.first(),
        }
        .ok_or("no matching MIDI input port")?
        .clone();
        let pname = mi.port_name(&port).unwrap_or_default();
        let tx = self.engine_sender();
        let target = self.midi_target.clone();
        let routes = self.midi_routes.clone();
        let conn = mi
            .connect(
                &port,
                "aurora-in",
                move |_t, msg, _| {
                    if msg.is_empty() {
                        return;
                    }
                    let channel = (msg[0] & 0x0F) + 1;
                    // Environment routing: splits/layers
                    {
                        let rs = routes.read();
                        if !rs.is_empty() {
                            let status = msg[0] & 0xF0;
                            match (status, msg.get(1), msg.get(2)) {
                                (0x90, Some(&p), Some(&v)) | (0x80, Some(&p), Some(&v)) => {
                                    let on = status == 0x90 && v > 0;
                                    for (t, pp, vv) in aurora_core::ops2::route_note(&rs, channel, p, if on { v } else { 0 }) {
                                        let k = if on { MidiKind::NoteOn { pitch: pp, velocity: vv } } else { MidiKind::NoteOff { pitch: pp } };
                                        let _ = tx.send(aurora_engine::EngineMsg::Live(t, k));
                                    }
                                }
                                (0xB0, Some(&c), Some(&v)) => {
                                    let mut sent = std::collections::HashSet::new();
                                    for r in rs.iter().filter(|r| r.channel.map(|ch| ch == channel).unwrap_or(true)) {
                                        if sent.insert(r.track_id) {
                                            let _ = tx.send(aurora_engine::EngineMsg::Live(r.track_id, MidiKind::Cc { controller: c, value: v }));
                                        }
                                    }
                                }
                                _ => {}
                            }
                            return;
                        }
                    }
                    let track = target.load(Ordering::Relaxed);
                    if track == 0 {
                        return;
                    }
                    let status = msg[0] & 0xF0;
                    let kind = match (status, msg.get(1), msg.get(2)) {
                        (0x90, Some(&p), Some(&v)) if v > 0 => MidiKind::NoteOn { pitch: p, velocity: v },
                        (0x90, Some(&p), _) | (0x80, Some(&p), _) => MidiKind::NoteOff { pitch: p },
                        (0xB0, Some(&c), Some(&v)) => MidiKind::Cc { controller: c, value: v },
                        _ => return,
                    };
                    let _ = tx.send(aurora_engine::EngineMsg::Live(track, kind));
                },
                (),
            )
            .map_err(|e| e.to_string())?;
        self.midi_conn = Some(conn);
        self.midi_input_name = Some(pname.clone());
        self.update_midi_target();
        Ok(json!({"connected": pname}))
    }
}

fn unreachable_report() -> aurora_engine::AnalysisReport {
    aurora_engine::Analyzer::new(44100.0).report()
}

fn db(x: f32) -> f32 {
    (aurora_dsp::util::gain_to_db(x) * 10.0).round() / 10.0
}

fn sanitize(s: &str) -> String {
    s.chars().map(|c| if c.is_alphanumeric() || c == ' ' || c == '-' || c == '_' { c } else { '_' }).collect()
}

fn summarize(v: &Value) -> String {
    let s = v.to_string();
    if s.len() > 160 {
        format!("{}…", &s[..s.char_indices().nth(160).map(|x| x.0).unwrap_or(s.len())])
    } else {
        s
    }
}

pub fn default_documents_dir() -> PathBuf {
    std::env::var_os("USERPROFILE").or_else(|| std::env::var_os("HOME")).map(|h| PathBuf::from(h).join("Documents")).unwrap_or_else(std::env::temp_dir)
}

#[cfg(test)]
mod tests {
    use super::*;
    use aurora_core::generate::{BassStyle, DrumStyle};

    #[test]
    fn session_workflow_with_undo_bounce_analyze_save() {
        let mut s = Session::new(false);
        let d = s.execute(Command::AddTrack { kind: TrackKind::Instrument, name: None, patch: Some("Pop Drum Kit".into()), instrument: None, preset: None, index: None }, Source::Script).unwrap();
        let did = d["track_id"].as_u64().unwrap();
        s.execute(Command::SetChordProgression { start: 0.0, chords: vec!["Am".into(), "F".into(), "C".into(), "G".into()], beats_per_chord: None, replace: None }, Source::Script).unwrap();
        s.execute(Command::GenerateDrums { track_id: did, start: 0.0, bars: 4, style: DrumStyle::Pop, intensity: None, fill: None, seed: Some(1) }, Source::Script).unwrap();
        let b = s.execute(Command::AddTrack { kind: TrackKind::Instrument, name: None, patch: Some("Classic Synth Bass".into()), instrument: None, preset: None, index: None }, Source::Script).unwrap();
        s.execute(Command::GenerateBass { track_id: b["track_id"].as_u64().unwrap(), start: 0.0, bars: 4, style: BassStyle::Octaves, chords: None, beats_per_chord: None, octave: None, seed: Some(2) }, Source::Script).unwrap();
        assert_eq!(s.project.tracks.len(), 2);
        s.execute(Command::Undo {}, Source::Script).unwrap();
        assert!(s.project.tracks[1].regions.is_empty());
        s.execute(Command::Redo {}, Source::Script).unwrap();
        assert_eq!(s.project.tracks[1].regions.len(), 1);
        let a = s.execute(Command::AnalyzeMix { start: None, end: None }, Source::Script).unwrap();
        assert!(a["master"]["integrated_lufs"].as_f64().unwrap() > -60.0, "{a}");
        let dir = std::env::temp_dir().join("aurora_test");
        let wav = dir.join("mix.wav");
        let r = s.execute(Command::Bounce { path: wav.display().to_string(), start: None, end: None, bit_depth: Some(16), normalize: Some(true), track_ids: None }, Source::Script).unwrap();
        assert!(r["duration_sec"].as_f64().unwrap() > 7.0);
        let m = s.execute(Command::AutoMaster { target_lufs: Some(-14.0), character: None }, Source::Script).unwrap();
        assert!((m["measured_lufs"].as_f64().unwrap() + 14.0).abs() < 2.5, "{m}");
        let proj = dir.join("song.aurora");
        s.execute(Command::SaveProject { path: Some(proj.display().to_string()) }, Source::Script).unwrap();
        let mut s2 = Session::new(false);
        s2.execute(Command::OpenProject { path: proj.display().to_string() }, Source::Script).unwrap();
        assert_eq!(s2.project.tracks.len(), 2);
        // import the bounce as audio
        let imp = s2.execute(Command::ImportAudio { path: wav.display().to_string(), track_id: None, start: Some(0.0) }, Source::Script).unwrap();
        assert!(imp["length_beats"].as_f64().unwrap() > 15.0);
        let bip = s2.execute(Command::BounceInPlace { track_id: did }, Source::Script).unwrap();
        assert!(bip["track_id"].as_u64().is_some());
    }
}
