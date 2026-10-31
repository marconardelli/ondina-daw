//! Main application state and frame loop.

use std::collections::{HashMap, HashSet};
use std::path::PathBuf;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;
use std::time::Duration;

use aurora_core::command::Command;
use aurora_core::model::*;
use aurora_core::theory::ScaleKind;
use aurora_dsp::MidiKind;
use aurora_engine::{EngineMsg, Shared};
use aurora_studio::ai::{AiEvent, Assistant};
use aurora_studio::mcp::HttpServerHandle;
use aurora_studio::session::LogEntry;
use aurora_studio::{Settings, SharedSession, Source};
use crossbeam_channel::{Receiver, Sender};
use eframe::egui::{self, Color32, Key, RichText};
use parking_lot::Mutex;
use serde_json::Value;

use crate::theme;
use crate::doc_guard::{self, Answer, CloseStep, DocAction, Step};

#[derive(Clone, Copy, PartialEq, Eq)]
pub enum BottomTab {
    Editor,
    Mixer,
    Live,
    Hidden,
}

#[derive(Clone, Copy, PartialEq, Eq)]
pub enum LeftTab {
    Inspector,
    Library,
}

pub enum ChatItem {
    User(String),
    Assistant(String),
    Tool {
        name: String,
        ok: bool,
        input: String,
        result: String,
    },
    Error(String),
}

pub struct AiState {
    pub assistant: Arc<Mutex<Assistant>>,
    pub local: Arc<Mutex<aurora_studio::local_ai::LocalAssistant>>,
    pub tx: Sender<AiEvent>,
    pub rx: Receiver<AiEvent>,
    pub busy: bool,
    pub input: String,
    pub chat: Vec<ChatItem>,
    pub cancel: Arc<AtomicBool>,
    pub status: String,
}

pub struct View {
    pub project: Arc<Project>,
    pub revision: u64,
    pub undo: Option<String>,
    pub redo: Option<String>,
    pub path: Option<PathBuf>,
    pub dirty: bool,
    pub log: Vec<LogEntry>,
    pub sample_rate: u32,
    pub device: String,
    pub midi_input: Option<String>,
}

pub enum Drag {
    MoveRegion {
        id: Id,
        grab_beat: f64,
        orig_start: f64,
        orig_track: Id,
        cur_start: f64,
        cur_track: Id,
        copy: bool,
    },
    ResizeRegion {
        id: Id,
        orig_len: f64,
        cur_len: f64,
    },
    Cycle {
        anchor: f64,
    },
    Notes {
        region: Id,
        orig: Vec<Note>,
        indices: Vec<usize>,
        grab_beat: f64,
        grab_pitch: i32,
        d_beat: f64,
        d_pitch: i32,
    },
    NoteResize {
        region: Id,
        orig: Vec<Note>,
        indices: Vec<usize>,
        grab_beat: f64,
        d_len: f64,
    },
    Velocity {
        region: Id,
        notes: Vec<Note>,
    },
    /// Rubber-band selection in the piano roll, in beat / pitch coordinates
    /// (pitch is continuous: a note of pitch P spans [P, P+1)).
    Marquee {
        region: Id,
        b0: f64,
        p0: f32,
        b1: f64,
        p1: f32,
        additive: bool,
    },
}

pub struct AuroraApp {
    pub session: SharedSession,
    pub shared: Arc<Shared>,
    pub engine_tx: Sender<EngineMsg>,
    pub settings: Settings,
    pub mcp: Option<HttpServerHandle>,
    pub mcp_error: Option<String>,
    pub view: View,
    pending: Vec<(Command, Option<String>)>,
    pub status: String,
    // arrange
    pub px_per_beat: f32,
    pub scroll_beat: f64,
    pub scroll_y: f32,
    pub track_height: f32,
    pub snap: f64,
    pub selected_track: Option<Id>,
    pub selected_region: Option<Id>,
    pub selected_plugin: Option<Id>,
    pub drag: Option<Drag>,
    pub follow_playhead: bool,
    // panels
    pub bottom: BottomTab,
    pub left: LeftTab,
    pub show_left: bool,
    pub show_assistant: bool,
    // piano roll
    pub pr_px_per_beat: f32,
    pub pr_scroll_beat: f64,
    pub pr_top_pitch: i32,
    pub pr_key_height: f32,
    pub selected_notes: HashSet<usize>,
    pub note_len: f64,
    // misc
    pub meters: HashMap<Id, [f32; 2]>,
    pub waveforms: HashMap<Id, Vec<(f32, f32)>>,
    pub ai: AiState,
    pub musical_typing: bool,
    pub typing_octave: i32,
    typing_held: HashMap<Key, u8>,
    pub show_settings: bool,
    pub show_mcp_info: bool,
    pub show_about: bool,
    pub mix_report: Option<Value>,
    pub rename_buf: String,
    pub vst_filter: String,
    pub api_key_buf: String,
    pub devices_cache: Option<Value>,
    pub midi_inputs_cache: Vec<String>,
    pub bounce_bits: u16,
    pub extra: crate::extra_ui::ExtraState,
    pub last_playhead: f64,
    /// Action waiting for the answer to the "Save changes?" prompt; `Some` while the prompt is open.
    doc_prompt: Option<DocAction>,
    /// Set once the user chose to quit, so our own close request is not cancelled again.
    allow_close: bool,
}

impl AuroraApp {
    pub fn new(
        cc: &eframe::CreationContext<'_>,
        session: SharedSession,
        settings: Settings,
        mcp: Option<HttpServerHandle>,
        mcp_error: Option<String>,
    ) -> Self {
        theme::apply(&cc.egui_ctx);
        cc.egui_ctx.set_pixels_per_point(
            cc.egui_ctx.pixels_per_point() * settings.ui_scale.clamp(0.75, 2.0),
        );
        let (shared, engine_tx, view) = {
            let s = session.lock();
            (
                s.engine.shared.clone(),
                s.engine_sender(),
                Self::make_view(&s),
            )
        };
        let (tx, rx) = crossbeam_channel::unbounded();
        AuroraApp {
            session,
            shared,
            engine_tx,
            api_key_buf: settings.api_key.clone().unwrap_or_default(),
            settings,
            mcp,
            mcp_error,
            view,
            pending: vec![],
            status: String::new(),
            px_per_beat: 28.0,
            scroll_beat: 0.0,
            scroll_y: 0.0,
            track_height: 58.0,
            snap: 1.0,
            selected_track: None,
            selected_region: None,
            selected_plugin: None,
            drag: None,
            follow_playhead: true,
            bottom: BottomTab::Editor,
            left: LeftTab::Inspector,
            show_left: true,
            show_assistant: true,
            pr_px_per_beat: 80.0,
            pr_scroll_beat: 0.0,
            pr_top_pitch: 84,
            pr_key_height: 12.0,
            selected_notes: HashSet::new(),
            note_len: 0.25,
            meters: HashMap::new(),
            waveforms: HashMap::new(),
            ai: AiState {
                assistant: Arc::new(Mutex::new(Assistant::new())),
                local: Arc::new(Mutex::new(Default::default())),
                tx,
                rx,
                busy: false,
                input: String::new(),
                chat: vec![],
                cancel: Arc::new(AtomicBool::new(false)),
                status: String::new(),
            },
            musical_typing: false,
            typing_octave: 4,
            typing_held: HashMap::new(),
            show_settings: false,
            show_mcp_info: false,
            show_about: false,
            mix_report: None,
            rename_buf: String::new(),
            vst_filter: String::new(),
            devices_cache: None,
            midi_inputs_cache: vec![],
            bounce_bits: 24,
            extra: crate::extra_ui::ExtraState::new(),
            last_playhead: 0.0,
            doc_prompt: None,
            allow_close: false,
        }
    }

    fn make_view(s: &aurora_studio::Session) -> View {
        View {
            project: Arc::new(s.project.clone()),
            revision: s.revision,
            undo: s.can_undo().map(|x| x.to_string()),
            redo: s.can_redo().map(|x| x.to_string()),
            path: s.path.clone(),
            dirty: s.dirty,
            log: s.log.iter().rev().take(200).cloned().collect(),
            sample_rate: s.engine.sample_rate(),
            device: s.engine.shared.device_name.lock().clone(),
            midi_input: s.midi_input_name.clone(),
        }
    }

    pub fn project(&self) -> Arc<Project> {
        self.view.project.clone()
    }

    /// Queue a command (executed at the end of the frame).
    pub fn cmd(&mut self, c: Command) {
        self.pending.push((c, None));
    }

    /// Queue a command that merges into the previous undo step with the same key (continuous drags).
    pub fn cmd_merge(&mut self, c: Command, key: impl Into<String>) {
        self.pending.push((c, Some(key.into())));
    }

    /// Execute immediately and return the result (waits briefly if the AI or MCP holds the session).
    pub fn exec(&mut self, c: Command) -> Option<Value> {
        let Some(mut s) = self.session.try_lock_for(Duration::from_millis(300)) else {
            self.status = "Session busy (AI/MCP running) — try again".into();
            return None;
        };
        let r = s.execute(c, Source::Gui);
        let v = Self::make_view(&s);
        drop(s);
        self.view = v;
        match r {
            Ok(v) => Some(v),
            Err(e) => {
                self.status = format!("⚠ {e}");
                None
            }
        }
    }

    fn sync(&mut self) {
        let Some(mut s) = self.session.try_lock() else {
            return;
        };
        for (c, key) in self.pending.drain(..) {
            let r = match key {
                Some(k) => s.execute_coalesced(c, &k),
                None => s.execute(c, Source::Gui),
            };
            if let Err(e) = r {
                self.status = format!("⚠ {e}");
            }
        }
        s.poll_editor_events();
        if s.selected_track != self.selected_track {
            s.select_track(self.selected_track);
        }
        s.selected_region = self.selected_region;
        if s.revision != self.view.revision
            || s.log.len() != self.view.log.len().min(200)
            || self.view.log.first().map(|l| l.at) != s.log.back().map(|l| l.at)
        {
            self.view = Self::make_view(&s);
        }
        // waveform peaks for new audio files
        let missing: Vec<Id> = self
            .view
            .project
            .audio_files
            .iter()
            .map(|f| f.id)
            .filter(|id| !self.waveforms.contains_key(id))
            .collect();
        let datas: Vec<(Id, Arc<aurora_dsp::AudioData>)> = missing
            .iter()
            .filter_map(|id| s.pool.get(id).map(|d| (*id, d.clone())))
            .collect();
        drop(s);
        for (id, d) in datas {
            let step = 256usize;
            let ch = &d.channels[0];
            let peaks: Vec<(f32, f32)> = ch
                .chunks(step)
                .map(|c| {
                    c.iter()
                        .fold((0.0f32, 0.0f32), |(lo, hi), x| (lo.min(*x), hi.max(*x)))
                })
                .collect();
            self.waveforms.insert(id, peaks);
        }
        // validate selection
        let p = self.view.project.clone();
        if let Some(t) = self.selected_track {
            if p.track(t).is_none() {
                self.selected_track = None;
            }
        }
        if let Some(r) = self.selected_region {
            if p.find_region(r).is_none() {
                self.selected_region = None;
                self.selected_notes.clear();
            }
        }
    }

    pub fn live_note(&self, track: Id, kind: MidiKind) {
        let _ = self.engine_tx.send(EngineMsg::Live(track, kind));
    }

    pub fn position(&self) -> f64 {
        self.shared.beat()
    }

    pub fn playing(&self) -> bool {
        self.shared.playing.load(Ordering::Relaxed)
    }

    pub fn recording(&self) -> bool {
        self.shared.recording.load(Ordering::Relaxed)
    }

    pub fn snap_beat(&self, b: f64) -> f64 {
        if self.snap <= 0.0 {
            b
        } else {
            (b / self.snap).round() * self.snap
        }
    }

    pub fn snap_floor(&self, b: f64) -> f64 {
        if self.snap <= 0.0 {
            b
        } else {
            (b / self.snap).floor() * self.snap
        }
    }

    fn toggle_play(&mut self) {
        if self.playing() {
            self.exec(Command::Stop {});
        } else {
            self.exec(Command::Play { from_beat: None });
        }
    }

    pub fn ask_ai(&mut self, text: String) {
        if self.ai.busy || text.trim().is_empty() {
            return;
        }
        self.show_assistant = true;
        self.ai.chat.push(ChatItem::User(text.clone()));
        self.ai.busy = true;
        self.ai.cancel.store(false, Ordering::Relaxed);
        let p = self.view.project.clone();
        let mut ctx = format!(
            "Playhead: beat {:.2} ({}).",
            self.position(),
            p.bar_beat_string(self.position())
        );
        if let Some(t) = self.selected_track.and_then(|id| p.track(id)) {
            ctx += &format!(" Selected track: '{}' (id {}).", t.name, t.id);
        }
        if let Some((t, r)) = self.selected_region.and_then(|id| p.find_region(id)) {
            ctx += &format!(
                " Selected region: '{}' (id {}) on track {} from beat {} length {}.",
                r.name, r.id, t.id, r.start, r.length
            );
            if let RegionContent::Midi(m) = &r.content {
                let v: Vec<usize> = self.selected_notes.iter().copied().collect();
                // 32 notes keep the context to a few hundred characters; the count says how many more there are.
                ctx += &aurora_core::ops::selected_notes_context(&m.notes, &v, 32);
            }
        }
        let session = self.session.clone();
        let mut settings = self.settings.clone();
        if let Some(s) = self.session.try_lock_for(Duration::from_millis(200)) {
            // model/effort may have been changed through MCP (set_ai_settings)
            settings.model = s.settings.model.clone();
            settings.effort = s.settings.effort.clone();
        }
        let tx = self.ai.tx.clone();
        let cancel = self.ai.cancel.clone();
        let assistant = self.ai.assistant.clone();
        let local = self.ai.local.clone();
        std::thread::spawn(move || {
            if settings.use_local() {
                local
                    .lock()
                    .run_turn(&session, &settings, &text, &ctx, &tx, &cancel);
            } else {
                assistant
                    .lock()
                    .run_turn(&session, &settings, &text, &ctx, &tx, &cancel);
            }
        });
    }

    fn poll_ai(&mut self) {
        while let Ok(ev) = self.ai.rx.try_recv() {
            match ev {
                AiEvent::Text(t) => self.ai.chat.push(ChatItem::Assistant(t)),
                AiEvent::ToolCall {
                    name,
                    input,
                    ok,
                    result,
                } => self.ai.chat.push(ChatItem::Tool {
                    name,
                    ok,
                    input: input.to_string(),
                    result,
                }),
                AiEvent::Status(s) => self.ai.status = s,
                AiEvent::Error(e) => self.ai.chat.push(ChatItem::Error(e)),
                AiEvent::Done => {
                    self.ai.busy = false;
                    self.ai.status.clear();
                }
            }
        }
    }

    fn update_meters(&mut self) {
        let fresh: HashMap<Id, [f32; 2]> = self.shared.meters.lock().clone();
        for v in self.meters.values_mut() {
            v[0] *= 0.82;
            v[1] *= 0.82;
        }
        for (id, p) in fresh {
            let e = self.meters.entry(id).or_insert([0.0; 2]);
            e[0] = e[0].max(p[0]);
            e[1] = e[1].max(p[1]);
        }
    }

    // ---------------------------------------------------------------- file actions

    /// Whether replacing the document now would lose work. Runs the GUI edits queued in this frame first and
    /// ends a recording, so a take in progress becomes part of the project (and of the question) instead of
    /// being dropped with it.
    fn has_unsaved_work(&mut self) -> bool {
        self.sync();
        if self.recording() {
            self.exec(Command::Stop {});
        }
        self.view.dirty || self.recording()
    }

    /// Entry point for every action that replaces the document (menu, shortcuts, recent files).
    pub fn request_doc_action(&mut self, ctx: &egui::Context, action: DocAction) {
        if self.doc_prompt.is_some() {
            return;
        }
        let unsaved = self.has_unsaved_work();
        match doc_guard::on_request(action, unsaved) {
            Step::Proceed(a) => self.run_doc_action(ctx, a),
            Step::Ask(a) => self.open_doc_prompt(ctx, a),
        }
    }

    fn open_doc_prompt(&mut self, ctx: &egui::Context, action: DocAction) {
        // A focused text field (assistant input, rename box) would otherwise keep Enter and Esc.
        ctx.memory_mut(|m| m.stop_text_input());
        self.doc_prompt = Some(action);
    }

    fn run_doc_action(&mut self, ctx: &egui::Context, action: DocAction) {
        match action {
            DocAction::Close => {
                // eframe turns this into a close request on the next frame; allow_close lets it through.
                self.allow_close = true;
                ctx.send_viewport_cmd(egui::ViewportCommand::Close);
            }
            DocAction::New => self.new_project_now(),
            DocAction::Open(None) => self.open_dialog_now(),
            DocAction::Open(Some(path)) => self.open_path_now(path),
        }
    }

    /// Must run inside `update` in the same frame as the close request: eframe only honours a
    /// `CancelClose` sent from that frame.
    fn handle_close_request(&mut self, ctx: &egui::Context) {
        if !ctx.input(|i| i.viewport().close_requested()) {
            return;
        }
        let unsaved = !self.allow_close && self.has_unsaved_work();
        match doc_guard::on_close_requested(self.allow_close, self.doc_prompt.is_some(), unsaved) {
            CloseStep::Allow => {}
            CloseStep::CancelAndAsk => {
                ctx.send_viewport_cmd(egui::ViewportCommand::CancelClose);
                self.open_doc_prompt(ctx, DocAction::Close);
            }
        }
    }

    fn new_project_now(&mut self) {
        self.exec(Command::NewProject {
            name: None,
            tempo: None,
            time_signature: None,
            key: None,
        });
        self.selected_track = None;
        self.selected_region = None;
        self.waveforms.clear();
    }

    fn open_dialog_now(&mut self) {
        if let Some(p) = rfd::FileDialog::new()
            .add_filter("Aurora Project", &["aurora"])
            .pick_file()
        {
            self.open_path_now(p.display().to_string());
        }
    }

    fn open_path_now(&mut self, path: String) {
        if self
            .exec(Command::OpenProject { path: path.clone() })
            .is_some()
        {
            self.settings.add_recent(&path);
            self.settings.save();
            self.waveforms.clear();
            self.status = format!("Opened {path}");
        }
    }

    pub fn save(&mut self, save_as: bool) -> bool {
        let path = if save_as || self.view.path.is_none() {
            let name = format!("{}.aurora", self.view.project.name);
            match rfd::FileDialog::new()
                .add_filter("Aurora Project", &["aurora"])
                .set_file_name(&name)
                .save_file()
            {
                Some(p) => Some(p.display().to_string()),
                None => return false,
            }
        } else {
            None
        };
        match self.exec(Command::SaveProject { path: path.clone() }) {
            Some(v) => {
                if let Some(p) = v["saved"].as_str() {
                    self.settings.add_recent(p);
                    self.settings.save();
                    self.status = format!("Saved {p}");
                }
                true
            }
            None => false,
        }
    }

    pub fn import_audio_dialog(&mut self) {
        if let Some(p) = rfd::FileDialog::new()
            .add_filter(
                "Audio",
                &["wav", "aif", "aiff", "flac", "mp3", "ogg", "m4a", "caf"],
            )
            .pick_file()
        {
            let track = self.selected_track.filter(|t| {
                self.view
                    .project
                    .track(*t)
                    .map(|x| x.kind == TrackKind::Audio)
                    .unwrap_or(false)
            });
            let at = self.snap_floor(self.position());
            if let Some(v) = self.exec(Command::ImportAudio {
                path: p.display().to_string(),
                track_id: track,
                start: Some(at),
            }) {
                self.selected_track = v["track_id"].as_u64();
                self.selected_region = v["region_id"].as_u64();
            }
        }
    }

    pub fn import_midi_dialog(&mut self) {
        if let Some(p) = rfd::FileDialog::new()
            .add_filter("MIDI", &["mid", "midi"])
            .pick_file()
        {
            self.exec(Command::ImportMidi {
                path: p.display().to_string(),
                at_beat: Some(0.0),
            });
        }
    }

    pub fn export_midi_dialog(&mut self) {
        if let Some(p) = rfd::FileDialog::new()
            .add_filter("MIDI", &["mid"])
            .set_file_name(&format!("{}.mid", self.view.project.name))
            .save_file()
        {
            if self
                .exec(Command::ExportMidi {
                    path: p.display().to_string(),
                    track_ids: None,
                })
                .is_some()
            {
                self.status = format!("Exported {}", p.display());
            }
        }
    }

    pub fn bounce_dialog(&mut self) {
        if let Some(p) = rfd::FileDialog::new()
            .add_filter("WAV", &["wav"])
            .set_file_name(&format!("{}.wav", self.view.project.name))
            .save_file()
        {
            let cycle = self.view.project.cycle;
            let (start, end) = if cycle.enabled {
                (Some(cycle.start), Some(cycle.end))
            } else {
                (None, None)
            };
            if let Some(v) = self.exec(Command::Bounce {
                path: p.display().to_string(),
                start,
                end,
                bit_depth: Some(self.bounce_bits),
                normalize: Some(false),
                track_ids: None,
            }) {
                self.status = format!(
                    "Bounced {:.1}s to {}",
                    v["duration_sec"].as_f64().unwrap_or(0.0),
                    p.display()
                );
            }
        }
    }

    pub fn add_track(&mut self, kind: TrackKind, patch: Option<&str>) {
        let idx = self
            .selected_track
            .and_then(|t| self.view.project.track_index(t))
            .map(|i| i + 1);
        if let Some(v) = self.exec(Command::AddTrack {
            kind,
            name: None,
            patch: patch.map(|s| s.to_string()),
            instrument: None,
            preset: None,
            index: idx,
        }) {
            self.selected_track = v["track_id"].as_u64();
            self.selected_region = None;
        }
    }

    // ---------------------------------------------------------------- keyboard

    fn handle_keys(&mut self, ctx: &egui::Context) {
        if ctx.wants_keyboard_input() {
            return;
        }
        let events = ctx.input(|i| i.events.clone());
        let mods = ctx.input(|i| i.modifiers);
        if self.musical_typing && !mods.command {
            let target = self.selected_track.filter(|t| {
                self.view
                    .project
                    .track(*t)
                    .map(|x| x.kind == TrackKind::Instrument)
                    .unwrap_or(false)
            });
            for e in &events {
                if let egui::Event::Key {
                    key,
                    pressed,
                    repeat,
                    ..
                } = e
                {
                    if *repeat {
                        continue;
                    }
                    match key {
                        Key::Z if *pressed => self.typing_octave = (self.typing_octave - 1).max(0),
                        Key::X if *pressed => self.typing_octave = (self.typing_octave + 1).min(8),
                        _ => {}
                    }
                    if let (Some(off), Some(track)) = (typing_offset(*key), target) {
                        let pitch = ((self.typing_octave + 1) * 12 + off).clamp(0, 127) as u8;
                        if *pressed {
                            self.typing_held.insert(*key, pitch);
                            self.live_note(
                                track,
                                MidiKind::NoteOn {
                                    pitch,
                                    velocity: 100,
                                },
                            );
                        } else if let Some(p) = self.typing_held.remove(key) {
                            self.live_note(track, MidiKind::NoteOff { pitch: p });
                        }
                    }
                }
            }
            if ctx.input(|i| i.key_pressed(Key::Space)) {
                self.toggle_play();
            }
            return;
        }
        ctx.input_mut(|i| {
            if i.consume_key(egui::Modifiers::COMMAND, Key::Z) {
                self.pending.push((Command::Undo {}, None));
            }
            if i.consume_key(egui::Modifiers::COMMAND | egui::Modifiers::SHIFT, Key::Z)
                || i.consume_key(egui::Modifiers::COMMAND, Key::Y)
            {
                self.pending.push((Command::Redo {}, None));
            }
        });
        let (space, del, home, r, c, k, d_ctrl, s_ctrl, n_ctrl, o_ctrl, b_ctrl, enter) =
            ctx.input(|i| {
                (
                    i.key_pressed(Key::Space),
                    i.key_pressed(Key::Delete) || i.key_pressed(Key::Backspace),
                    i.key_pressed(Key::Home),
                    i.key_pressed(Key::R) && !i.modifiers.command,
                    i.key_pressed(Key::C) && !i.modifiers.command,
                    i.key_pressed(Key::K) && !i.modifiers.command,
                    i.key_pressed(Key::D) && i.modifiers.command,
                    i.key_pressed(Key::S) && i.modifiers.command,
                    i.key_pressed(Key::N) && i.modifiers.command,
                    i.key_pressed(Key::O) && i.modifiers.command,
                    i.key_pressed(Key::B) && i.modifiers.command,
                    i.key_pressed(Key::Enter),
                )
            });
        if space {
            self.toggle_play();
        }
        if enter {
            self.exec(Command::SetPlayhead {
                beat: Some(0.0),
                bar: None,
            });
        }
        if home {
            self.exec(Command::SetPlayhead {
                beat: Some(0.0),
                bar: None,
            });
        }
        if r {
            if self.recording() {
                self.exec(Command::Stop {});
            } else {
                self.exec(Command::Record { from_beat: None });
            }
        }
        if c {
            let cy = self.view.project.cycle;
            self.cmd(Command::SetCycle {
                enabled: !cy.enabled,
                start: None,
                end: None,
            });
        }
        if k {
            let m = self.view.project.metronome;
            self.cmd(Command::SetMetronome { enabled: !m });
        }
        if del {
            if !self.selected_notes.is_empty() && self.bottom == BottomTab::Editor {
                if let Some(rid) = self.selected_region {
                    let idx: Vec<usize> = self.selected_notes.drain().collect();
                    self.cmd(Command::RemoveNotes {
                        region_id: rid,
                        indices: Some(idx),
                        pitch: None,
                        from_beat: None,
                        to_beat: None,
                    });
                }
            } else if let Some(rid) = self.selected_region.take() {
                self.cmd(Command::DeleteRegion { region_id: rid });
            }
        }
        if d_ctrl {
            if let Some(rid) = self.selected_region {
                if let Some(v) = self.exec(Command::DuplicateRegion {
                    region_id: rid,
                    start: None,
                    count: Some(1),
                }) {
                    self.selected_region = v["region_ids"][0].as_u64();
                }
            }
        }
        if s_ctrl {
            self.save(false);
        }
        if n_ctrl {
            self.request_doc_action(ctx, DocAction::New);
        }
        if o_ctrl {
            self.request_doc_action(ctx, DocAction::Open(None));
        }
        if b_ctrl {
            self.bounce_dialog();
        }
    }

    // ---------------------------------------------------------------- top bars

    fn menu_bar(&mut self, ui: &mut egui::Ui) {
        egui::menu::bar(ui, |ui| {
            ui.menu_button("File", |ui| {
                let ctx = ui.ctx().clone();
                if ui.button("New Project      Ctrl+N").clicked() {
                    ui.close_menu();
                    self.request_doc_action(&ctx, DocAction::New);
                }
                if ui.button("Open…            Ctrl+O").clicked() {
                    ui.close_menu();
                    self.request_doc_action(&ctx, DocAction::Open(None));
                }
                ui.menu_button("Open Recent", |ui| {
                    for p in self.settings.recent_projects.clone() {
                        if ui.button(&p).clicked() {
                            ui.close_menu();
                            self.request_doc_action(&ctx, DocAction::Open(Some(p)));
                        }
                    }
                });
                if ui.button("Save             Ctrl+S").clicked() {
                    ui.close_menu();
                    self.save(false);
                }
                if ui.button("Save As…").clicked() {
                    ui.close_menu();
                    self.save(true);
                }
                ui.separator();
                if ui.button("Import Audio File…").clicked() {
                    ui.close_menu();
                    self.import_audio_dialog();
                }
                if ui.button("Import MIDI File…").clicked() {
                    ui.close_menu();
                    self.import_midi_dialog();
                }
                if ui.button("Export MIDI File…").clicked() {
                    ui.close_menu();
                    self.export_midi_dialog();
                }
                if ui.button("Export Score as MusicXML…").clicked() {
                    ui.close_menu();
                    if let Some(p) = rfd::FileDialog::new()
                        .add_filter("MusicXML", &["musicxml"])
                        .set_file_name("score.musicxml")
                        .save_file()
                    {
                        self.exec(Command::ExportMusicXml {
                            path: p.display().to_string(),
                            track_ids: None,
                        });
                    }
                }
                ui.separator();
                if ui.button("Load Movie…").clicked() {
                    ui.close_menu();
                    self.load_movie_dialog();
                }
                if ui.button("Export Movie with Soundtrack…").clicked() {
                    ui.close_menu();
                    if let Some(p) = rfd::FileDialog::new()
                        .add_filter("MP4", &["mp4"])
                        .set_file_name("scored.mp4")
                        .save_file()
                    {
                        self.exec(Command::ExportMovie {
                            path: p.display().to_string(),
                        });
                    }
                }
                if ui.button("Bounce Surround (5.1/7.1/7.1.4)…").clicked() {
                    ui.close_menu();
                    if let Some(p) = rfd::FileDialog::new()
                        .add_filter("WAV", &["wav"])
                        .set_file_name("surround.wav")
                        .save_file()
                    {
                        self.exec(Command::BounceSurround {
                            path: p.display().to_string(),
                            format: None,
                        });
                    }
                }
                if ui
                    .button("Export ADM BWF (object-based / Atmos master)…")
                    .clicked()
                {
                    ui.close_menu();
                    if let Some(p) = rfd::FileDialog::new()
                        .add_filter("ADM BWF", &["wav"])
                        .set_file_name("master.adm.wav")
                        .save_file()
                    {
                        if let Some(v) = self.exec(Command::ExportAdm {
                            path: p.display().to_string(),
                            start: None,
                            end: None,
                        }) {
                            self.status = format!(
                                "ADM exported: 7.1.2 bed + {} objects",
                                v["objects"].as_array().map(|a| a.len()).unwrap_or(0)
                            );
                        }
                    }
                }
                if ui.button("Bounce Binaural (headphones)…").clicked() {
                    ui.close_menu();
                    if let Some(p) = rfd::FileDialog::new()
                        .add_filter("WAV", &["wav"])
                        .set_file_name("binaural.wav")
                        .save_file()
                    {
                        self.exec(Command::BounceBinaural {
                            path: p.display().to_string(),
                        });
                    }
                }
                ui.separator();
                ui.horizontal(|ui| {
                    ui.label("Bounce bit depth");
                    ui.selectable_value(&mut self.bounce_bits, 16, "16");
                    ui.selectable_value(&mut self.bounce_bits, 24, "24");
                    ui.selectable_value(&mut self.bounce_bits, 32, "32f");
                });
                if ui.button("Bounce to WAV…   Ctrl+B").clicked() {
                    ui.close_menu();
                    self.bounce_dialog();
                }
                ui.separator();
                if ui.button("Settings…").clicked() {
                    self.show_settings = true;
                    ui.close_menu();
                }
            });
            ui.menu_button("Edit", |ui| {
                let undo = self
                    .view
                    .undo
                    .clone()
                    .map(|u| format!("Undo {u}"))
                    .unwrap_or("Undo".into());
                if ui
                    .add_enabled(
                        self.view.undo.is_some(),
                        egui::Button::new(format!("{undo}   Ctrl+Z")),
                    )
                    .clicked()
                {
                    self.cmd(Command::Undo {});
                    ui.close_menu();
                }
                let redo = self
                    .view
                    .redo
                    .clone()
                    .map(|u| format!("Redo {u}"))
                    .unwrap_or("Redo".into());
                if ui
                    .add_enabled(
                        self.view.redo.is_some(),
                        egui::Button::new(format!("{redo}   Ctrl+Y")),
                    )
                    .clicked()
                {
                    self.cmd(Command::Redo {});
                    ui.close_menu();
                }
                ui.separator();
                if ui
                    .add_enabled(
                        self.selected_region.is_some(),
                        egui::Button::new("Split Region at Playhead"),
                    )
                    .clicked()
                {
                    if let Some(r) = self.selected_region {
                        let b = self.position();
                        self.cmd(Command::SplitRegion {
                            region_id: r,
                            beat: b,
                        });
                    }
                    ui.close_menu();
                }
                if ui
                    .add_enabled(
                        self.selected_region.is_some(),
                        egui::Button::new("Duplicate Region   Ctrl+D"),
                    )
                    .clicked()
                {
                    if let Some(r) = self.selected_region {
                        self.cmd(Command::DuplicateRegion {
                            region_id: r,
                            start: None,
                            count: Some(1),
                        });
                    }
                    ui.close_menu();
                }
                if ui
                    .add_enabled(
                        self.selected_region.is_some(),
                        egui::Button::new("Delete Region   Del"),
                    )
                    .clicked()
                {
                    if let Some(r) = self.selected_region.take() {
                        self.cmd(Command::DeleteRegion { region_id: r });
                    }
                    ui.close_menu();
                }
                ui.separator();
                if ui.button("Insert 1 Bar at Playhead").clicked() {
                    let bpb = self.view.project.time_signature.beats_per_bar();
                    let at = self.snap_floor(self.position());
                    self.cmd(Command::InsertTime {
                        at_beat: at,
                        length: bpb,
                    });
                    ui.close_menu();
                }
                if ui.button("Delete Cycle Section").clicked() {
                    let cy = self.view.project.cycle;
                    self.cmd(Command::DeleteSection {
                        from_beat: cy.start,
                        length: cy.end - cy.start,
                        ripple: Some(true),
                    });
                    ui.close_menu();
                }
                if ui.button("MIDI Transform…").clicked() {
                    self.extra.show_transform = true;
                    ui.close_menu();
                }
                if ui.button("Repeat Cycle Section").clicked() {
                    let cy = self.view.project.cycle;
                    self.cmd(Command::CopySection {
                        from_beat: cy.start,
                        length: cy.end - cy.start,
                        to_beat: cy.end,
                        insert: Some(true),
                    });
                    ui.close_menu();
                }
            });
            ui.menu_button("Track", |ui| {
                if ui.button("New Software Instrument Track").clicked() {
                    self.add_track(TrackKind::Instrument, None);
                    ui.close_menu();
                }
                if ui.button("New Drum Track").clicked() {
                    self.add_track(TrackKind::Instrument, Some("Pop Drum Kit"));
                    ui.close_menu();
                }
                if ui.button("New Audio Track").clicked() {
                    self.add_track(TrackKind::Audio, None);
                    ui.close_menu();
                }
                if ui.button("New Aux Bus").clicked() {
                    self.add_track(TrackKind::Bus, None);
                    ui.close_menu();
                }
                if ui.button("MIDI Environment (splits/layers)…").clicked() {
                    self.extra.show_routes = true;
                    ui.close_menu();
                }
                ui.separator();
                let has = self.selected_track.is_some();
                if ui
                    .add_enabled(has, egui::Button::new("Duplicate Track"))
                    .clicked()
                {
                    if let Some(t) = self.selected_track {
                        self.cmd(Command::DuplicateTrack { track_id: t });
                    }
                    ui.close_menu();
                }
                if ui
                    .add_enabled(has, egui::Button::new("Bounce in Place"))
                    .clicked()
                {
                    if let Some(t) = self.selected_track {
                        self.exec(Command::BounceInPlace { track_id: t });
                    }
                    ui.close_menu();
                }
                if ui
                    .add_enabled(has, egui::Button::new("Delete Track"))
                    .clicked()
                {
                    if let Some(t) = self.selected_track.take() {
                        self.cmd(Command::RemoveTrack { track_id: t });
                    }
                    ui.close_menu();
                }
            });
            ui.menu_button("Mix", |ui| {
                if ui
                    .button("Analyze Mix (loudness, spectrum, advice)")
                    .clicked()
                {
                    ui.close_menu();
                    if let Some(v) = self.exec(Command::AnalyzeMix {
                        start: None,
                        end: None,
                    }) {
                        self.mix_report = Some(v);
                    }
                }
                if ui.button("Mastering Assistant (-14 LUFS)").clicked() {
                    ui.close_menu();
                    if let Some(v) = self.exec(Command::AutoMaster {
                        target_lufs: Some(-14.0),
                        character: None,
                    }) {
                        self.status = format!(
                            "Mastered: {:.1} LUFS (limiter +{:.1} dB)",
                            v["measured_lufs"].as_f64().unwrap_or(0.0),
                            v["limiter_gain_db"].as_f64().unwrap_or(0.0)
                        );
                    }
                }
                if ui.button("Panic (all notes off)").clicked() {
                    self.cmd(Command::Panic {});
                    ui.close_menu();
                }
            });
            ui.menu_button("View", |ui| {
                ui.checkbox(&mut self.show_left, "Inspector / Library");
                ui.checkbox(&mut self.show_assistant, "AI Assistant");
                ui.radio_value(
                    &mut self.bottom,
                    BottomTab::Editor,
                    "Editor (Piano Roll / Step)",
                );
                ui.radio_value(&mut self.bottom, BottomTab::Mixer, "Mixer");
                ui.radio_value(&mut self.bottom, BottomTab::Live, "Live Loops grid");
                ui.radio_value(&mut self.bottom, BottomTab::Hidden, "Hide bottom panel");
                ui.checkbox(&mut self.extra.show_video, "Movie window");
                ui.checkbox(&mut self.extra.show_score, "Score view in editor");
                ui.checkbox(&mut self.follow_playhead, "Follow playhead");
                ui.checkbox(
                    &mut self.musical_typing,
                    "Musical Typing (A–K keys play notes)",
                );
            });
            ui.menu_button("AI", |ui| {
                if ui.button("Open Assistant").clicked() {
                    self.show_assistant = true;
                    ui.close_menu();
                }
                if ui.button("MCP Server Info…").clicked() {
                    self.show_mcp_info = true;
                    ui.close_menu();
                }
                if ui.button("AI Settings…").clicked() {
                    self.show_settings = true;
                    ui.close_menu();
                }
            });
            ui.menu_button("Help", |ui| {
                if ui.button("Keyboard Shortcuts & About").clicked() {
                    self.show_about = true;
                    ui.close_menu();
                }
            });
        });
    }

    fn transport_bar(&mut self, ui: &mut egui::Ui) {
        let p = self.project();
        ui.horizontal(|ui| {
            ui.spacing_mut().item_spacing.x = 4.0;
            let big = |t: &str| RichText::new(t).size(16.0);
            if ui
                .button(big("⏮"))
                .on_hover_text("Go to start (Home)")
                .clicked()
            {
                self.exec(Command::SetPlayhead {
                    beat: Some(0.0),
                    bar: None,
                });
            }
            if ui.button(big("⏪")).on_hover_text("Back one bar").clicked() {
                let b = (self.position() - p.time_signature.beats_per_bar()).max(0.0);
                self.exec(Command::SetPlayhead {
                    beat: Some(b),
                    bar: None,
                });
            }
            if ui
                .button(big("⏩"))
                .on_hover_text("Forward one bar")
                .clicked()
            {
                let b = self.position() + p.time_signature.beats_per_bar();
                self.exec(Command::SetPlayhead {
                    beat: Some(b),
                    bar: None,
                });
            }
            let playing = self.playing();
            if ui
                .add(
                    egui::Button::new(big(if playing { "⏹" } else { "▶" })).fill(if playing {
                        Color32::from_rgb(40, 110, 50)
                    } else {
                        theme::LANE
                    }),
                )
                .on_hover_text("Play / Stop (Space)")
                .clicked()
            {
                self.toggle_play();
            }
            let rec = self.recording();
            if ui
                .add(
                    egui::Button::new(big("⏺").color(if rec {
                        Color32::WHITE
                    } else {
                        theme::REC
                    }))
                    .fill(if rec { theme::REC } else { theme::LANE }),
                )
                .on_hover_text("Record (R)")
                .clicked()
            {
                if rec {
                    self.exec(Command::Stop {});
                } else {
                    self.exec(Command::Record { from_beat: None });
                }
            }
            let cyc = p.cycle.enabled;
            if ui
                .add(egui::Button::new(big("🔁")).fill(if cyc {
                    Color32::from_rgb(150, 115, 20)
                } else {
                    theme::LANE
                }))
                .on_hover_text("Cycle (C)")
                .clicked()
            {
                self.cmd(Command::SetCycle {
                    enabled: !cyc,
                    start: None,
                    end: None,
                });
            }
            let met = p.metronome;
            if ui
                .add(egui::Button::new(big("🎵")).fill(if met {
                    Color32::from_rgb(60, 90, 150)
                } else {
                    theme::LANE
                }))
                .on_hover_text("Metronome (K)")
                .clicked()
            {
                self.cmd(Command::SetMetronome { enabled: !met });
            }
            ui.separator();
            // LCD
            let pos = self.position();
            egui::Frame::none()
                .fill(Color32::from_rgb(18, 20, 24))
                .rounding(4.0_f32)
                .inner_margin(egui::Margin::symmetric(10.0, 3.0))
                .show(ui, |ui| {
                    ui.horizontal(|ui| {
                        ui.label(
                            RichText::new(p.bar_beat_string(pos))
                                .monospace()
                                .size(18.0)
                                .color(theme::ACCENT),
                        );
                        if p.movie.is_some() {
                            ui.label(
                                RichText::new(p.timecode(pos))
                                    .monospace()
                                    .size(13.0)
                                    .color(Color32::from_rgb(120, 200, 255)),
                            );
                        }
                        let secs = p.tempo.beat_to_sec(pos);
                        ui.label(
                            RichText::new(format!(
                                "{:02}:{:05.2}",
                                (secs / 60.0) as u32,
                                secs % 60.0
                            ))
                            .monospace()
                            .size(13.0)
                            .color(theme::TEXT_DIM),
                        );
                        ui.separator();
                        let mut bpm = p.tempo.initial_bpm();
                        if ui
                            .add(
                                egui::DragValue::new(&mut bpm)
                                    .range(20.0..=300.0)
                                    .speed(0.2)
                                    .suffix(" bpm")
                                    .max_decimals(1),
                            )
                            .changed()
                        {
                            self.cmd_merge(Command::SetTempo { bpm }, "tempo");
                        }
                        let ts = p.time_signature;
                        egui::ComboBox::from_id_salt("timesig")
                            .width(56.0)
                            .selected_text(format!("{}/{}", ts.numerator, ts.denominator))
                            .show_ui(ui, |ui| {
                                for (n, d) in
                                    [(4u8, 4u8), (3, 4), (2, 4), (5, 4), (6, 8), (7, 8), (12, 8)]
                                {
                                    if ui
                                        .selectable_label(
                                            ts.numerator == n && ts.denominator == d,
                                            format!("{n}/{d}"),
                                        )
                                        .clicked()
                                    {
                                        self.cmd(Command::SetTimeSignature {
                                            numerator: n,
                                            denominator: d,
                                        });
                                    }
                                }
                            });
                        egui::ComboBox::from_id_salt("key")
                            .width(130.0)
                            .selected_text(p.key.name())
                            .show_ui(ui, |ui| {
                                for root in 0..12u8 {
                                    for scale in [
                                        ScaleKind::Major,
                                        ScaleKind::Minor,
                                        ScaleKind::Dorian,
                                        ScaleKind::Mixolydian,
                                    ] {
                                        let k = aurora_core::theory::Key { root, scale };
                                        if ui.selectable_label(p.key == k, k.name()).clicked() {
                                            self.cmd(Command::SetKey { key: k.name() });
                                        }
                                    }
                                }
                            });
                    });
                });
            ui.separator();
            ui.label("Snap");
            egui::ComboBox::from_id_salt("snap")
                .width(64.0)
                .selected_text(snap_label(self.snap))
                .show_ui(ui, |ui| {
                    for s in [0.0, 4.0, 1.0, 0.5, 0.25, 0.125] {
                        ui.selectable_value(&mut self.snap, s, snap_label(s));
                    }
                });
            ui.separator();
            let load = self.shared.load();
            ui.label(
                RichText::new(format!("DSP {:>3.0}%", load * 100.0))
                    .small()
                    .color(if load > 0.8 {
                        theme::REC
                    } else {
                        theme::TEXT_DIM
                    }),
            );
            if self.musical_typing {
                ui.label(
                    RichText::new(format!("🎹 Typing C{}", self.typing_octave))
                        .color(theme::ACCENT),
                );
            }
            ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                if ui
                    .selectable_label(
                        self.show_assistant,
                        RichText::new("✨ AI Assistant").color(theme::AI),
                    )
                    .clicked()
                {
                    self.show_assistant = !self.show_assistant;
                }
                if ui
                    .selectable_label(self.bottom == BottomTab::Live, "🔲 Live Loops")
                    .clicked()
                {
                    self.bottom = if self.bottom == BottomTab::Live {
                        BottomTab::Hidden
                    } else {
                        BottomTab::Live
                    };
                }
                if ui
                    .selectable_label(self.bottom == BottomTab::Mixer, "🎚 Mixer")
                    .clicked()
                {
                    self.bottom = if self.bottom == BottomTab::Mixer {
                        BottomTab::Hidden
                    } else {
                        BottomTab::Mixer
                    };
                }
                if ui
                    .selectable_label(self.bottom == BottomTab::Editor, "🎹 Editor")
                    .clicked()
                {
                    self.bottom = if self.bottom == BottomTab::Editor {
                        BottomTab::Hidden
                    } else {
                        BottomTab::Editor
                    };
                }
                if ui.selectable_label(self.show_left, "ℹ Inspector").clicked() {
                    self.show_left = !self.show_left;
                }
            });
        });
    }

    fn status_bar(&mut self, ui: &mut egui::Ui) {
        ui.horizontal(|ui| {
            let mcp = match (&self.mcp, &self.mcp_error) {
                (Some(h), _) => RichText::new(format!("● MCP {}", h.address))
                    .color(Color32::from_rgb(100, 200, 120)),
                (None, Some(e)) => RichText::new(format!("○ MCP off: {e}")).color(theme::REC),
                _ => RichText::new("○ MCP disabled").color(theme::TEXT_DIM),
            };
            if ui
                .add(egui::Label::new(mcp.small()).sense(egui::Sense::click()))
                .on_hover_text("Click for MCP setup instructions")
                .clicked()
            {
                self.show_mcp_info = true;
            }
            ui.separator();
            ui.label(
                RichText::new(format!(
                    "🔊 {} · {} Hz",
                    self.view.device, self.view.sample_rate
                ))
                .small()
                .color(theme::TEXT_DIM),
            );
            if let Some(m) = &self.view.midi_input {
                ui.label(
                    RichText::new(format!("🎹 {m}"))
                        .small()
                        .color(theme::TEXT_DIM),
                );
            }
            ui.separator();
            if let Some(last) = self.view.log.iter().find(|l| l.source != Source::Gui) {
                let (tag, col) = match last.source {
                    Source::Ai => ("AI", theme::AI),
                    Source::Mcp => ("MCP", Color32::from_rgb(100, 200, 120)),
                    _ => ("", theme::TEXT_DIM),
                };
                ui.label(
                    RichText::new(format!(
                        "{tag}: {} {}",
                        last.command,
                        if last.ok { "✓" } else { "✗" }
                    ))
                    .small()
                    .color(col),
                );
                ui.separator();
            }
            ui.label(RichText::new(&self.status).small());
            ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                let name = self
                    .view
                    .path
                    .as_ref()
                    .map(|p| p.display().to_string())
                    .unwrap_or_else(|| "unsaved project".into());
                ui.label(
                    RichText::new(format!(
                        "{}{}",
                        name,
                        if self.view.dirty { " •" } else { "" }
                    ))
                    .small()
                    .color(theme::TEXT_DIM),
                );
            });
        });
    }

    fn windows(&mut self, ctx: &egui::Context) {
        let mut open = self.show_settings;
        egui::Window::new("Settings").open(&mut open).resizable(false).show(ctx, |ui| {
            ui.heading("AI assistant");
            ui.horizontal(|ui| {
                ui.label("Anthropic API key");
                ui.add(egui::TextEdit::singleline(&mut self.api_key_buf).password(true).desired_width(260.0));
            });
            if std::env::var("ANTHROPIC_API_KEY").map(|k| !k.is_empty()).unwrap_or(false) {
                ui.label(RichText::new("Using ANTHROPIC_API_KEY from the environment.").small().color(theme::TEXT_DIM));
            }
            ui.horizontal(|ui| {
                ui.label("Provider");
                for (v, l) in [("auto", "Auto"), ("local", "Local (offline)"), ("anthropic", "Claude API")] {
                    ui.selectable_value(&mut self.settings.provider, v.to_string(), l);
                }
            });
            ui.horizontal(|ui| {
                ui.label("Local endpoint");
                ui.text_edit_singleline(&mut self.settings.local_url);
            });
            ui.label(RichText::new(match aurora_studio::local_ai::bundled_llm() { Some((_, m)) => format!("Bundled local model: {}", m.file_name().and_then(|f| f.to_str()).unwrap_or("")), None => "No bundled local model (tools/llm); any OpenAI-compatible server works.".into() }).small().color(theme::TEXT_DIM));
            ui.horizontal(|ui| {
                ui.label("Claude model");
                ui.text_edit_singleline(&mut self.settings.model);
            });
            ui.horizontal(|ui| {
                ui.label("Effort");
                for e in ["low", "medium", "high", "xhigh", "max"] {
                    ui.selectable_value(&mut self.settings.effort, e.to_string(), e);
                }
            });
            ui.separator();
            ui.heading("MCP server");
            ui.checkbox(&mut self.settings.mcp_enabled, "Enable HTTP MCP endpoint (restart required)");
            ui.horizontal(|ui| {
                ui.label("Port");
                ui.add(egui::DragValue::new(&mut self.settings.mcp_port).range(1024..=65535));
            });
            ui.separator();
            ui.heading("Audio & MIDI");
            if self.devices_cache.is_none() {
                self.devices_cache = self.exec(Command::ListAudioDevices {});
                if let Some(v) = self.exec(Command::ListMidiInputs {}) {
                    self.midi_inputs_cache = v["inputs"].as_array().map(|a| a.iter().filter_map(|x| x.as_str().map(|s| s.to_string())).collect()).unwrap_or_default();
                }
            }
            if let Some(d) = self.devices_cache.clone() {
                ui.label(format!("Host: {}  ·  current output: {}", d["host"].as_str().unwrap_or(""), d["current_output"].as_str().unwrap_or("")));
                egui::ComboBox::from_label("Output device").selected_text(self.settings.audio_output.clone().unwrap_or("Default".into())).show_ui(ui, |ui| {
                    for o in d["outputs"].as_array().cloned().unwrap_or_default() {
                        let name = o.as_str().unwrap_or("").to_string();
                        if ui.selectable_label(self.settings.audio_output.as_deref() == Some(&name), &name).clicked() {
                            self.settings.audio_output = Some(name.clone());
                            self.exec(Command::SetAudioDevice { output: Some(name), input: None });
                            self.devices_cache = None;
                        }
                    }
                });
            }
            ui.horizontal(|ui| {
                ui.label("MIDI input");
                for name in self.midi_inputs_cache.clone() {
                    if ui.button(&name).clicked() {
                        self.exec(Command::ConnectMidiInput { name: Some(name) });
                    }
                }
                if self.midi_inputs_cache.is_empty() {
                    ui.label(RichText::new("none found").color(theme::TEXT_DIM));
                }
            });
            ui.horizontal(|ui| {
                ui.label("UI scale (restart)");
                ui.add(egui::Slider::new(&mut self.settings.ui_scale, 0.75..=2.0));
            });
            ui.separator();
            if ui.button("Save settings").clicked() {
                self.settings.api_key = if self.api_key_buf.trim().is_empty() { None } else { Some(self.api_key_buf.trim().to_string()) };
                self.settings.save();
                if let Some(mut s) = self.session.try_lock_for(Duration::from_millis(300)) {
                    s.settings = self.settings.clone();
                }
                self.status = "Settings saved".into();
            }
        });
        self.show_settings = open;

        let mut open = self.show_mcp_info;
        egui::Window::new("MCP server").open(&mut open).default_width(560.0).show(ctx, |ui| {
            let addr = self.mcp.as_ref().map(|h| h.address.clone()).unwrap_or_else(|| format!("http://127.0.0.1:{}/mcp", self.settings.mcp_port));
            ui.label(format!("Aurora exposes its complete API ({} tools) over the Model Context Protocol.", aurora_studio::mcp::tools().len()));
            ui.add_space(4.0);
            ui.label(RichText::new("Streamable HTTP endpoint (controls this running app):").strong());
            ui.code(&addr);
            ui.add_space(4.0);
            ui.label(RichText::new("Claude Code:").strong());
            ui.code(format!("claude mcp add --transport http aurora {addr}"));
            ui.add_space(4.0);
            ui.label(RichText::new("Stdio clients (e.g. Claude Desktop) — bridge to this app:").strong());
            let exe = std::env::current_exe().ok().and_then(|p| p.parent().map(|d| d.join("aurora-mcp.exe").display().to_string())).unwrap_or("aurora-mcp.exe".into());
            ui.code(format!("{{\n  \"mcpServers\": {{\n    \"aurora\": {{ \"command\": \"{}\" }}\n  }}\n}}", exe.replace('\\', "\\\\")));
            ui.label(RichText::new("Add --headless to run a private session without the GUI. Set AURORA_MCP_TOKEN to require a bearer token.").small().color(theme::TEXT_DIM));
            ui.separator();
            ui.label(RichText::new("Recent external activity").strong());
            egui::ScrollArea::vertical().max_height(180.0).show(ui, |ui| {
                for l in self.view.log.iter().filter(|l| l.source != Source::Gui).take(60) {
                    let col = if l.source == Source::Ai { theme::AI } else { Color32::from_rgb(100, 200, 120) };
                    ui.label(RichText::new(format!("[{:?}] {} {} — {}", l.source, l.command, if l.ok { "✓" } else { "✗" }, l.message)).small().color(col));
                }
            });
        });
        self.show_mcp_info = open;

        let mut open = self.show_about;
        egui::Window::new("Aurora DAW").open(&mut open).show(ctx, |ui| {
            ui.heading("Aurora DAW");
            ui.label("A native Windows digital audio workstation written in Rust, with an AI assistant and a full MCP API.");
            ui.separator();
            egui::Grid::new("keys").striped(true).show(ui, |ui| {
                for (k, d) in [
                    ("Space", "Play / Stop"),
                    ("R", "Record"),
                    ("C", "Cycle on/off"),
                    ("K", "Metronome"),
                    ("Home / Enter", "Go to start"),
                    ("Ctrl+Z / Ctrl+Y", "Undo / Redo"),
                    ("Del", "Delete region or selected notes"),
                    ("Ctrl+D", "Duplicate region"),
                    ("Ctrl+S / Ctrl+O / Ctrl+N", "Save / Open / New"),
                    ("Ctrl+B", "Bounce"),
                    ("Double-click track lane", "New MIDI region"),
                    ("Alt+drag region", "Copy region"),
                    ("Ctrl+wheel", "Zoom"),
                    ("Shift+drag ruler", "Set cycle range"),
                    ("Musical Typing: A W S E D F T G Y H U J K", "Play notes, Z/X octave"),
                ] {
                    ui.label(RichText::new(k).monospace());
                    ui.label(d);
                    ui.end_row();
                }
            });
        });
        self.show_about = open;

        if let Some(rep) = self.mix_report.clone() {
            let mut open = true;
            egui::Window::new("Mix Analysis").open(&mut open).default_width(520.0).show(ctx, |ui| {
                let m = &rep["master"];
                egui::Grid::new("mixrep").striped(true).show(ui, |ui| {
                    for (label, key, unit) in [("Integrated loudness", "integrated_lufs", "LUFS"), ("Short-term max", "short_term_max_lufs", "LUFS"), ("Loudness range", "loudness_range_lu", "LU"), ("True peak", "true_peak_dbtp", "dBTP"), ("RMS", "rms_dbfs", "dBFS"), ("Crest factor", "crest_factor_db", "dB"), ("Stereo correlation", "stereo_correlation", "")] {
                        ui.label(label);
                        ui.label(RichText::new(format!("{} {}", m[key], unit)).monospace());
                        ui.end_row();
                    }
                    ui.label("Spectral balance low/mid/high");
                    ui.label(RichText::new(format!("{} / {} / {} %", m["spectral_balance_pct"][0], m["spectral_balance_pct"][1], m["spectral_balance_pct"][2])).monospace());
                    ui.end_row();
                });
                ui.separator();
                for a in rep["advice"].as_array().cloned().unwrap_or_default() {
                    ui.label(format!("• {}", a.as_str().unwrap_or("")));
                }
                ui.separator();
                egui::ScrollArea::vertical().max_height(200.0).show(ui, |ui| {
                    egui::Grid::new("trk").striped(true).show(ui, |ui| {
                        ui.label(RichText::new("Track").strong());
                        ui.label(RichText::new("Peak").strong());
                        ui.label(RichText::new("LUFS").strong());
                        ui.label(RichText::new("Low/Mid/High %").strong());
                        ui.end_row();
                        for t in rep["tracks"].as_array().cloned().unwrap_or_default() {
                            let a = &t["analysis"];
                            ui.label(t["name"].as_str().unwrap_or(""));
                            ui.label(format!("{}", a["peak_dbfs"]));
                            ui.label(format!("{}", a["integrated_lufs"]));
                            ui.label(format!("{}/{}/{}", a["spectral_balance_pct"][0], a["spectral_balance_pct"][1], a["spectral_balance_pct"][2]));
                            ui.end_row();
                        }
                    });
                });
                if ui.button(RichText::new("✨ Ask the AI to fix the mix").color(theme::AI)).clicked() {
                    self.ask_ai("Analyze the mix and fix the problems you find, then verify with analyze_mix.".into());
                }
            });
            if !open {
                self.mix_report = None;
            }
        }
    }

    fn doc_prompt_ui(&mut self, ctx: &egui::Context) {
        let Some(action) = self.doc_prompt.clone() else {
            return;
        };
        let screen = ctx.screen_rect();
        egui::Area::new(egui::Id::new("doc_prompt_backdrop"))
            .order(egui::Order::Foreground)
            .fixed_pos(screen.min)
            .show(ctx, |ui| {
                ui.painter().rect_filled(screen, 0.0, Color32::from_black_alpha(150));
                ui.allocate_rect(screen, egui::Sense::click_and_drag());
            });
        let question = doc_guard::prompt_question(&self.view.project.name, &action);
        let mut answer = None;
        let shown = egui::Window::new("Unsaved Changes")
            .id(egui::Id::new("doc_prompt"))
            .order(egui::Order::Foreground)
            .anchor(egui::Align2::CENTER_CENTER, [0.0, 0.0])
            .collapsible(false)
            .resizable(false)
            .movable(false)
            .show(ctx, |ui| {
                ui.label(RichText::new(question).strong());
                ui.label(
                    RichText::new("Your changes will be lost if you don't save them.")
                        .small()
                        .color(theme::TEXT_DIM),
                );
                ui.add_space(8.0);
                ui.horizontal(|ui| {
                    let save = egui::Button::new(RichText::new("Save").color(Color32::BLACK)).fill(theme::ACCENT);
                    if ui.add(save).clicked() {
                        answer = Some(Answer::Save);
                    }
                    if ui.button("Don't Save").clicked() {
                        answer = Some(Answer::DontSave);
                    }
                    if ui.button("Cancel").clicked() {
                        answer = Some(Answer::Cancel);
                    }
                });
            });
        if let Some(r) = shown {
            ctx.move_to_top(r.response.layer_id);
        }
        ctx.input_mut(|i| {
            if i.consume_key(egui::Modifiers::NONE, Key::Escape) {
                answer.get_or_insert(Answer::Cancel);
            }
            if i.consume_key(egui::Modifiers::NONE, Key::Enter) {
                answer.get_or_insert(Answer::Save);
            }
        });
        let Some(answer) = answer else {
            return;
        };
        self.doc_prompt = None;
        let saved = answer == Answer::Save && self.save(false);
        if let Some(next) = doc_guard::on_answer(action, answer, saved) {
            self.run_doc_action(ctx, next);
        }
    }
}

fn snap_label(s: f64) -> &'static str {
    match s {
        x if x == 0.0 => "Off",
        x if x == 4.0 => "Bar",
        x if x == 1.0 => "1/4",
        x if x == 0.5 => "1/8",
        x if x == 0.25 => "1/16",
        _ => "1/32",
    }
}

fn typing_offset(k: Key) -> Option<i32> {
    Some(match k {
        Key::A => 0,
        Key::W => 1,
        Key::S => 2,
        Key::E => 3,
        Key::D => 4,
        Key::F => 5,
        Key::T => 6,
        Key::G => 7,
        Key::Y => 8,
        Key::H => 9,
        Key::U => 10,
        Key::J => 11,
        Key::K => 12,
        Key::O => 13,
        Key::L => 14,
        _ => return None,
    })
}

impl eframe::App for AuroraApp {
    fn update(&mut self, ctx: &egui::Context, _frame: &mut eframe::Frame) {
        self.sync();
        self.poll_ai();
        self.update_meters();
        self.handle_close_request(ctx);
        if self.doc_prompt.is_none() {
            self.handle_keys(ctx);
        }

        let title = format!(
            "Aurora DAW — {}{}",
            self.view.project.name,
            if self.view.dirty { " •" } else { "" }
        );
        ctx.send_viewport_cmd(egui::ViewportCommand::Title(title));

        egui::TopBottomPanel::top("menu").show(ctx, |ui| self.menu_bar(ui));
        egui::TopBottomPanel::top("transport")
            .frame(
                egui::Frame::none()
                    .fill(theme::PANEL)
                    .inner_margin(egui::Margin::symmetric(8.0, 5.0)),
            )
            .show(ctx, |ui| self.transport_bar(ui));
        egui::TopBottomPanel::bottom("status")
            .frame(
                egui::Frame::none()
                    .fill(theme::BG)
                    .inner_margin(egui::Margin::symmetric(8.0, 2.0)),
            )
            .show(ctx, |ui| self.status_bar(ui));
        if self.show_assistant {
            egui::SidePanel::right("assistant")
                .default_width(370.0)
                .min_width(280.0)
                .show(ctx, |ui| self.assistant_ui(ui));
        }
        if self.show_left {
            egui::SidePanel::left("left")
                .default_width(270.0)
                .min_width(220.0)
                .show(ctx, |ui| self.left_ui(ui));
        }
        match self.bottom {
            BottomTab::Editor => {
                egui::TopBottomPanel::bottom("editor")
                    .resizable(true)
                    .default_height(300.0)
                    .min_height(160.0)
                    .show(ctx, |ui| self.editor_ui(ui));
            }
            BottomTab::Mixer => {
                egui::TopBottomPanel::bottom("mixer")
                    .resizable(true)
                    .default_height(330.0)
                    .min_height(240.0)
                    .show(ctx, |ui| self.mixer_ui(ui));
            }
            BottomTab::Live => {
                egui::TopBottomPanel::bottom("live")
                    .resizable(true)
                    .default_height(300.0)
                    .min_height(180.0)
                    .show(ctx, |ui| self.live_ui(ui));
            }
            BottomTab::Hidden => {}
        }
        egui::CentralPanel::default()
            .frame(egui::Frame::none().fill(theme::BG))
            .show(ctx, |ui| self.arrange_ui(ui));
        self.windows(ctx);
        self.extra_windows(ctx);
        self.doc_prompt_ui(ctx);
        self.sync();
        ctx.request_repaint_after(Duration::from_millis(if self.playing() || self.ai.busy {
            16
        } else {
            50
        }));
    }

    fn on_exit(&mut self, _gl: Option<&eframe::glow::Context>) {
        self.settings.save();
        if let Some(h) = &self.mcp {
            h.shutdown();
        }
    }
}
