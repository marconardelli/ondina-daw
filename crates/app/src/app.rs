//! Main application state and frame loop.

use std::collections::{HashMap, HashSet};
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;
use std::time::Duration;

use ondina_core::command::Command;
use ondina_core::model::*;
use ondina_dsp::MidiKind;
use ondina_engine::{EngineMsg, Shared};
use ondina_studio::ai::{AiEvent, Assistant};
use ondina_studio::mcp::HttpServerHandle;
use ondina_studio::session::LogEntry;
use ondina_studio::{Settings, SharedSession, Source};
use crossbeam_channel::{Receiver, Sender};
use eframe::egui::{self, Key, RichText};
use parking_lot::Mutex;
use serde_json::Value;

use crate::bounce_menu;
use crate::dialog;
use crate::theme;
use crate::doc_guard::{self, Answer, CloseStep, DocAction, Step};
use crate::editors::take_event;
use crate::metronome_ui;
use crate::note_clipboard as nc;
use crate::note_selection;
use crate::region_clipboard as rc;
use crate::transport_keys as tk;
use crate::widgets;

/// Tooltip of the overload counter in the transport bar.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum BottomTab {
    Editor,
    Mixer,
    Live,
    Hidden,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
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
    pub local: Arc<Mutex<ondina_studio::local_ai::LocalAssistant>>,
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
    /// `Session::document` when the view was taken.
    pub document: u64,
    pub undo: Option<String>,
    /// The step Undo would revert was made by the built-in assistant.
    pub undo_ai: bool,
    pub redo: Option<String>,
    pub path: Option<PathBuf>,
    pub dirty: bool,
    pub log: Vec<LogEntry>,
    pub sample_rate: u32,
    pub device: String,
    pub midi_input: Option<String>,
    /// VST3 plug-ins of the document that the engine does not load, by id, with the reason (`Session::unloaded_vst3`).
    pub unloaded_vst3: HashMap<Id, String>,
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
    /// Dragging a fade handle of an audio region. `orig` is the fade when the drag began and `cur` what the
    /// release sends, both in beats; `grab_beat` is where the press began.
    RegionFade {
        id: Id,
        handle: crate::region_edit::FadeHandle,
        grab_beat: f64,
        orig: f64,
        cur: f64,
    },
    /// Drawing a new cycle on the ruler from `anchor`; `gesture` keys its single undo step. `orig` is the cycle
    /// when the press began and `sent` tells whether the drag has sent a range yet (Esc puts `orig` back).
    Cycle {
        anchor: f64,
        gesture: u64,
        orig: Cycle,
        sent: bool,
    },
    /// Moving the cycle (zone Body) or one of its edges on the ruler's cycle strip. `start`, `end` and `enabled` are
    /// the cycle when the drag began; `sent` is the last range sent and `sent_at` when (egui time, seconds). Esc
    /// puts the cycle back as it began only if `sent` is set: otherwise nothing has changed.
    CycleEdit {
        zone: crate::arrange::CycleZone,
        grab_beat: f64,
        start: f64,
        end: f64,
        enabled: bool,
        gesture: u64,
        sent: Option<(f64, f64)>,
        sent_at: f64,
    },
    /// Moving a marker in the marker lane; only `cur_beat` changes until release.
    MoveMarker {
        id: Id,
        grab_beat: f64,
        orig_beat: f64,
        cur_beat: f64,
    },
    /// Dragging the right edge of a section marker.
    ResizeMarker {
        id: Id,
        start: f64,
        orig_len: f64,
        cur_len: f64,
    },
    /// Moving a chord in the chord lane; only `cur_beat` changes until release.
    MoveChord {
        id: Id,
        grab_beat: f64,
        orig_beat: f64,
        cur_beat: f64,
    },
    /// Dragging the right edge of a chord; `next_start` is where the next chord begins, if any.
    ResizeChord {
        id: Id,
        start: f64,
        orig_len: f64,
        cur_len: f64,
        next_start: Option<f64>,
    },
    /// Dragging a track header to reorder tracks; `gap` is where it would drop
    /// (0 = above the first track, the track count = below the last).
    MoveTrack {
        id: Id,
        gap: usize,
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
    /// A mouse stroke in the piano roll's velocity lane; `stroke` holds the region's notes as they were and as drawn.
    Velocity {
        region: Id,
        stroke: crate::note_drag::VelocityStroke,
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
    /// A mouse stroke in the piano roll's controller lane: drawing (left button) or erasing (right button).
    CcStroke {
        region: Id,
        stroke: crate::cc_lane::CcStroke,
    },
    /// Dragging a point in the arrangement's automation view (a new one when the press began on empty space).
    AutoPoint {
        track: Id,
        drag: crate::automation_lane::PointDrag,
    },
    /// A paint stroke in the step sequencer; nothing is sent until the release.
    StepPaint {
        region: Id,
        stroke: crate::step_paint::StepStroke,
    },
}

pub struct OndinaApp {
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
    /// A View-menu step for the arrangement, applied by `arrange_ui` where the view's size is known.
    pub arrange_step: Option<crate::arrange_view::ViewStep>,
    /// A track to bring into view, applied by `arrange_ui` where the view's height is known.
    pub reveal_track: Option<Id>,
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
    /// Fraction of a row left over from smooth wheel scrolling, carried to the next frame.
    pub pr_pitch_carry: f32,
    /// Which region the piano roll last framed on its notes.
    pub pr_framing: crate::piano_roll_view::ViewFraming,
    pub selected_notes: HashSet<usize>,
    /// Region the note selection was made in; when `selected_region` changes the indices mean nothing.
    notes_region: Option<Id>,
    pub note_len: f64,
    /// Value of the number field in the piano roll's Velocity menu.
    pub pr_velocity: u8,
    /// Grid of the piano roll's Q button; None follows the snap.
    pub pr_quantize: Option<f64>,
    /// How far the piano roll's transpose buttons have moved the current selection.
    pub pr_transpose_run: crate::pr_toolbar::TransposeRun,
    /// Notes copied or cut in the piano roll (Ctrl+C / Ctrl+X); lives for the whole app session.
    pub note_clipboard: Option<crate::note_clipboard::NoteClip>,
    /// Region copied or cut in the arrangement (Ctrl+C / Ctrl+X); lives for the whole app session.
    pub region_clipboard: Option<crate::region_clipboard::RegionClip>,
    /// What the piano roll's controller lane shows (open or closed, which controller).
    pub cc_lane: crate::cc_lane::CcLaneState,
    /// Whether the arrangement shows automation over the regions, and which lane each track shows.
    pub automation: crate::automation_lane::AutomationView,
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
    /// The Bounce dialog is open.
    pub show_bounce: bool,
    /// The settings as last written by the Settings window (`None` while it is closed).
    pub settings_saved: Option<String>,
    pub mix_report: Option<Value>,
    /// Master loudness of the last mix analysis, shown in the mixer.
    pub mix_loudness: Option<crate::mixer::MixLoudness>,
    pub rename_buf: String,
    /// Name and chord fields open in place; closed when the project is replaced.
    pub edits: crate::arrange::InPlaceEdits,
    pub vst_filter: String,
    /// Search text of the Library's patches.
    pub library_filter: String,
    pub api_key_buf: String,
    pub devices_cache: Option<Value>,
    pub midi_inputs_cache: Vec<String>,
    pub bounce_bits: u16,
    /// Loudness of File > Bounce to WAV; kept for the session like bounce_bits.
    pub bounce_loudness: bounce_menu::Loudness,
    pub extra: crate::extra_ui::ExtraState,
    pub last_playhead: f64,
    /// Action waiting for the answer to the "Save changes?" prompt; `Some` while the prompt is open.
    doc_prompt: Option<DocAction>,
    /// Set once the user chose to quit, so our own close request is not cancelled again.
    allow_close: bool,
    /// Background autosave: decides each frame, writes on its own thread.
    autosave: crate::autosave::Autosave,
    /// Copies left by a run that did not close cleanly; the recovery window is open while not empty.
    recovery: Vec<crate::autosave::Leftover>,
    /// Document opened from an autosave copy and where it originally lived (proposed by the next Save As).
    recovered: Option<(u64, PathBuf)>,
    /// When the document was last saved, for the status bar.
    pub(crate) save_clock: crate::status_bar::SaveClock,
}

impl OndinaApp {
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
        let (autosave, recovery) = crate::autosave::Autosave::start(Settings::autosave_dir(), view.document);
        let (tx, rx) = crossbeam_channel::unbounded();
        // the panels open as they were left
        let layout = settings.panels.clone();
        OndinaApp {
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
            arrange_step: None,
            reveal_track: None,
            snap: 1.0,
            selected_track: None,
            selected_region: None,
            selected_plugin: None,
            drag: None,
            follow_playhead: true,
            bottom: crate::panel_layout::bottom_from_key(layout.bottom_panel.as_deref()),
            left: LeftTab::Inspector,
            show_left: layout.left_open.unwrap_or(true),
            show_assistant: layout.assistant_open.unwrap_or(true),
            pr_px_per_beat: 80.0,
            pr_scroll_beat: 0.0,
            pr_top_pitch: 84,
            pr_key_height: crate::piano_roll_view::DEFAULT_KEY_H,
            pr_pitch_carry: 0.0,
            pr_framing: Default::default(),
            selected_notes: HashSet::new(),
            notes_region: None,
            note_len: 0.25,
            pr_velocity: 100,
            pr_quantize: None,
            pr_transpose_run: Default::default(),
            note_clipboard: None,
            region_clipboard: None,
            cc_lane: Default::default(),
            automation: Default::default(),
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
            show_bounce: false,
            settings_saved: None,
            mix_report: None,
            mix_loudness: None,
            rename_buf: String::new(),
            edits: Default::default(),
            vst_filter: String::new(),
            library_filter: String::new(),
            devices_cache: None,
            midi_inputs_cache: vec![],
            bounce_bits: 24,
            bounce_loudness: bounce_menu::Loudness::Off,
            extra: crate::extra_ui::ExtraState::new(),
            last_playhead: 0.0,
            doc_prompt: None,
            allow_close: false,
            autosave,
            recovery,
            recovered: None,
            save_clock: Default::default(),
        }
    }

    fn make_view(s: &ondina_studio::Session) -> View {
        View {
            project: Arc::new(s.project.clone()),
            revision: s.revision,
            document: s.document,
            undo: s.can_undo().map(|x| x.to_string()),
            undo_ai: s.last_undo_is_ai(),
            redo: s.can_redo().map(|x| x.to_string()),
            path: s.path.clone(),
            dirty: s.dirty,
            log: s.log.iter().rev().take(200).cloned().collect(),
            sample_rate: s.engine.sample_rate(),
            device: s.engine.shared.device_name.lock().clone(),
            midi_input: s.midi_input_name.clone(),
            unloaded_vst3: s.unloaded_vst3(),
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
    pub fn exec(&mut self, mut c: Command) -> Option<Value> {
        let Some(mut s) = self.session.try_lock_for(Duration::from_millis(300)) else {
            self.status = "Session busy (AI/MCP running) — try again".into();
            return None;
        };
        if s.revision != self.view.revision {
            // The assistant, MCP or a script changed the project since this command was built from the view.
            note_selection::reindex_command(&mut c, &self.view.project, &s.project);
        }
        let r = s.execute(c, Source::Gui);
        let v = Self::make_view(&s);
        drop(s);
        note_selection::follow(&mut self.selected_notes, self.selected_region, &self.view.project, &v.project);
        self.edits.follow_document(self.view.document, v.document);
        self.view = v;
        match r {
            Ok(v) => Some(v),
            Err(e) => {
                self.status = format!("⚠ {e}");
                None
            }
        }
    }

    /// Runs `f` with the session locked, like `exec`, and refreshes the view once afterwards: for a GUI action made of
    /// several commands that need each other's results (the files of one drop).
    pub fn exec_with<R>(&mut self, f: impl FnOnce(&mut ondina_studio::Session) -> R) -> Option<R> {
        let Some(mut s) = self.session.try_lock_for(Duration::from_millis(300)) else {
            self.status = "Session busy (AI/MCP running) — try again".into();
            return None;
        };
        let r = f(&mut s);
        let v = Self::make_view(&s);
        drop(s);
        note_selection::follow(&mut self.selected_notes, self.selected_region, &self.view.project, &v.project);
        self.edits.follow_document(self.view.document, v.document);
        self.view = v;
        Some(r)
    }

    fn sync(&mut self) {
        let Some(mut s) = self.session.try_lock() else {
            return;
        };
        if s.revision != self.view.revision {
            // Commands queued this frame address notes by index in the project the GUI showed.
            for (c, _) in self.pending.iter_mut() {
                note_selection::reindex_command(c, &self.view.project, &s.project);
            }
        }
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
            let fresh = Self::make_view(&s);
            note_selection::follow(&mut self.selected_notes, self.selected_region, &self.view.project, &fresh.project);
            self.edits.follow_document(self.view.document, fresh.document);
            self.view = fresh;
        }
        if let Some(v) = note_selection::session_notes_update(&s.selected_notes, &self.selected_notes, self.notes_region == self.selected_region) {
            s.selected_notes = v;
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
        let datas: Vec<(Id, Arc<ondina_dsp::AudioData>)> = missing
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
        if self.selected_region != self.notes_region {
            self.selected_notes.clear();
            self.notes_region = self.selected_region;
        }
    }

    pub fn live_note(&self, track: Id, kind: MidiKind) {
        let _ = self.engine_tx.send(EngineMsg::Live(track, kind));
    }

    pub fn position(&self) -> f64 {
        self.shared.beat()
    }

    pub fn playing(&self) -> bool {
        self.shared.is_playing()
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

    pub(crate) fn toggle_play(&mut self) {
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
        // every AI button is disabled without a model; this keeps a stray request from failing in the background
        if self.ai_engine() == crate::assistant::AiEngine::Offline {
            self.status = crate::assistant::NO_MODEL_HINT.into();
            return;
        }
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
                ctx += &ondina_core::ops::selected_notes_context(&m.notes, &v, 32);
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

    /// Whether the "Save changes?" prompt or the recovery window is waiting for an answer.
    pub(crate) fn doc_prompt_open(&self) -> bool {
        self.doc_prompt.is_some() || !self.recovery.is_empty()
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
            DocAction::Recover(copy) => self.recover_now(copy),
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
            discard_unsaved: None,
        });
        self.selected_track = None;
        self.selected_region = None;
        self.pr_framing.reset();
        self.waveforms.clear();
    }

    fn open_dialog_now(&mut self) {
        if let Some(p) = rfd::FileDialog::new()
            .add_filter("Ondina Project", &["ondina", "aurora"])
            .pick_file()
        {
            self.open_path_now(p.display().to_string());
        }
    }

    fn open_path_now(&mut self, path: String) {
        if let Some(v) = self.exec(Command::OpenProject { path: path.clone(), discard_unsaved: None }) {
            self.settings.add_recent(&path);
            self.settings.save();
            self.waveforms.clear();
            self.pr_framing.reset();
            self.status = doc_guard::open_status(&path, &v);
        }
    }

    /// Opens an autosave copy as unsaved work; the copy stays on disk until this instance has written its own.
    fn recover_now(&mut self, copy: String) {
        let copy = PathBuf::from(copy);
        let meta = crate::autosave::read_meta(&copy);
        match self.exec_with(|s| s.open_recovered(&copy)) {
            Some(Ok(v)) => {
                self.waveforms.clear();
                self.pr_framing.reset();
                let name = self.view.project.name.clone();
                self.recovered = meta.and_then(|m| m.original).map(|o| (self.view.document, PathBuf::from(o)));
                self.autosave.adopt(self.view.document, self.view.revision, &copy);
                self.status = crate::autosave::recovered_status(&name, &v);
            }
            Some(Err(e)) => self.status = format!("⚠ Can't recover {}: {e}", copy.display()),
            None => {}
        }
    }

    /// The original file of the recovered document, while that document is still the one open.
    fn recovered_original(&self) -> Option<&Path> {
        self.recovered.as_ref().filter(|(doc, _)| *doc == self.view.document).map(|(_, p)| p.as_path())
    }

    fn autosave_tick(&mut self, ctx: &egui::Context) {
        let recovered = self.recovered_original().map(Path::to_path_buf);
        self.autosave.tick(crate::autosave::Snapshot {
            now: ctx.input(|i| i.time),
            minutes: self.settings.autosave_minutes,
            dirty: self.view.dirty,
            document: self.view.document,
            revision: self.view.revision,
            project: &self.view.project,
            path: self.view.path.as_deref(),
            recovered: recovered.as_deref(),
        });
        if let Some(msg) = self.autosave.poll() {
            self.status = msg;
        }
    }

    pub fn save(&mut self, save_as: bool) -> bool {
        let path = if save_as || self.view.path.is_none() {
            let (folder, name) = crate::autosave::save_dialog_default(&self.view.project.name, self.recovered_original());
            let mut dialog = rfd::FileDialog::new()
                .add_filter("Ondina Project", &["ondina", "aurora"])
                .set_file_name(&name);
            if let Some(d) = folder {
                dialog = dialog.set_directory(d);
            }
            match dialog.save_file() {
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
                    self.status = match v["backup_error"].as_str() {
                        Some(e) => format!("⚠ Saved {p}, but the copy of the previous version was not written: {e}"),
                        None => format!("Saved {p}"),
                    };
                }
                self.recovered = None;
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
            let range = cycle.enabled.then_some((cycle.start, cycle.end));
            let loudness = self.bounce_loudness;
            let path = p.display().to_string();
            if let Some(v) = self.exec(bounce_menu::command(
                path.clone(),
                range,
                self.bounce_bits,
                loudness,
            )) {
                self.status = bounce_menu::status(&v, &path, loudness);
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
        let note_arrows = self.piano_roll_keys(ctx);
        self.transport_keys(ctx, note_arrows);
        self.region_clipboard_keys(ctx);
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
            let c = metronome_ui::toggle(&self.view.project);
            self.cmd(c);
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
            self.show_bounce = true;
        }
    }

    /// `,` `.` move the playhead by a bar, with Shift to the previous or next marker; Ctrl+T splits the selected
    /// region at the playhead. Up/Down select the previous or next track unless the piano roll is moving the
    /// selected notes (`note_arrows`), M mutes and S solos the selected track, Shift+M adds a marker at the playhead.
    /// A shows or hides the automation view. Runs from `handle_keys` after the piano roll keys.
    fn transport_keys(&mut self, ctx: &egui::Context, note_arrows: bool) {
        let (text_focus, dragging) = (ctx.wants_keyboard_input(), self.drag.is_some());
        if !tk::keys_on(text_focus, self.musical_typing, dragging) {
            return;
        }
        let musical_typing = self.musical_typing;
        let (jump, split, track, auto_toggle) = ctx.input_mut(|i| {
            let (mut jump, mut split, mut track, mut auto_toggle) = (None, false, None, false);
            i.events.retain(|e| {
                let egui::Event::Key { key, physical_key, pressed: true, repeat, modifiers } = e else {
                    return true;
                };
                if !auto_toggle
                    && crate::automation_lane::view_toggle(text_focus, musical_typing, dragging, *key, *repeat, *modifiers)
                {
                    auto_toggle = true;
                    return false;
                }
                if jump.is_none() {
                    if let Some(j) = tk::jump_from_key(*key, *physical_key, *modifiers) {
                        jump = Some(j);
                        return false;
                    }
                }
                if !split && tk::split_key(*key, *repeat, *modifiers) {
                    split = true;
                    return false;
                }
                if track.is_none() {
                    if let Some(k) = tk::track_key(*key, *repeat, *modifiers, !note_arrows) {
                        track = Some(k);
                        return false;
                    }
                }
                true
            });
            (jump, split, track, auto_toggle)
        });
        if auto_toggle {
            self.automation.on = !self.automation.on;
            self.status = if self.automation.on { "Automation view on" } else { "Automation view off" }.into();
        }
        if let Some(j) = jump {
            let pos = self.position();
            let to = tk::jump_target(&self.view.project, pos, j, self.playing());
            if (to - pos).abs() > tk::EPS {
                self.exec(Command::SetPlayhead { beat: Some(to), bar: None });
            }
        }
        if split {
            match tk::split_command(&self.view.project, self.selected_region, self.position()) {
                Ok(c) => {
                    let at = self.view.project.bar_beat_string(self.position());
                    if self.exec(c).is_some() {
                        self.status = format!("Split region at {at}");
                    }
                }
                Err(msg) => self.status = msg.into(),
            }
        }
        if let Some(k) = track {
            self.run_track_key(k);
        }
    }

    /// A track key, from the keyboard or the Track and Edit menus. Selecting a track is GUI state, like a click on
    /// its header; mute, solo and the marker are queued commands, one undo step each.
    pub(crate) fn run_track_key(&mut self, k: tk::TrackKey) {
        match k {
            tk::TrackKey::Step(d) => {
                let ids: Vec<Id> = self.view.project.tracks.iter().map(|t| t.id).collect();
                if let Some(id) = tk::track_step(&ids, self.selected_track, d) {
                    self.selected_track = Some(id);
                    self.reveal_track = Some(id);
                    if let Some(t) = self.view.project.track(id) {
                        self.status = format!("Selected {}", t.name);
                    }
                }
            }
            tk::TrackKey::Edit(e) => {
                match tk::track_command(&self.view.project, self.selected_track, self.position(), e) {
                    Ok((c, msg)) => {
                        self.cmd(c);
                        self.status = msg;
                    }
                    Err(msg) => self.status = msg,
                }
            }
        }
    }

    /// Ctrl+C / Ctrl+X / Ctrl+V on regions, after the piano roll had its turn (`copy_target` / `paste_target` say who
    /// owns a key). Musical Typing does not turn them off: Ctrl keys play no notes.
    fn region_clipboard_keys(&mut self, ctx: &egui::Context) {
        let dragging = self.drag.is_some();
        let midi_open = self
            .selected_region
            .and_then(|r| self.view.project.find_region(r))
            .is_some_and(|(_, r)| matches!(r.content, RegionContent::Midi(_)));
        let note_keys = midi_open
            && nc::keys_on(self.bottom == BottomTab::Editor, self.extra.show_score, ctx.wants_keyboard_input(), dragging);
        let copy_to = rc::copy_target(note_keys, !self.selected_notes.is_empty(), self.selected_region.is_some(), dragging);
        let paste_to = rc::paste_target(note_keys, self.note_clipboard.is_some(), self.region_clipboard.is_some(), dragging);
        let (cut, copy, paste) = ctx.input_mut(|i| {
            let region = copy_to == Some(rc::ClipKind::Region);
            let cut = region && take_event(&mut i.events, |e| matches!(e, egui::Event::Cut));
            let copy = region && take_event(&mut i.events, |e| matches!(e, egui::Event::Copy));
            let paste = paste_to == Some(rc::ClipKind::Region) && take_event(&mut i.events, |e| matches!(e, egui::Event::Paste(_)));
            (cut, copy, paste)
        });
        if cut || copy {
            self.copy_region(ctx, cut);
        } else if paste {
            self.paste_region();
        }
    }

    /// Edit > Copy Region / Cut Region, Ctrl+C / Ctrl+X: the selected region goes to the region clipboard, and the
    /// note clipboard empties so Ctrl+V pastes what was copied last. It also goes to the system clipboard as JSON:
    /// egui reports Ctrl+V only while that holds text. Cut deletes the region, one undo step.
    fn copy_region(&mut self, ctx: &egui::Context, cut: bool) {
        let Some(rid) = self.selected_region else { return };
        let Some(clip) = rc::copy(&self.view.project, rid, self.view.document) else { return };
        let name = clip.region.name.clone();
        ctx.copy_text(rc::clip_text(&clip));
        self.region_clipboard = Some(clip);
        self.note_clipboard = None;
        if cut {
            self.selected_region = None;
            self.cmd(Command::DeleteRegion { region_id: rid });
            self.status = format!("Cut region {name}");
        } else {
            self.status = format!("Copied region {name}");
        }
    }

    /// Edit > Paste Region, Ctrl+V: the copied region at the playhead (back to the snap grid) on the selected track,
    /// or on the track it came from. The copy becomes the selection.
    fn paste_region(&mut self) {
        let Some(clip) = self.region_clipboard.clone() else { return };
        let spot = rc::paste_spot(&self.view.project, &clip, self.selected_track, self.position(), self.snap, self.view.document);
        let (track_id, start) = match spot {
            Ok(s) => s,
            Err(e) => {
                self.status = format!("Can't paste: {e}");
                return;
            }
        };
        let name = clip.region.name.clone();
        if let Some(v) = self.exec(Command::PasteRegion { track_id, start, region: Box::new(clip.region) }) {
            self.selected_region = v["region_id"].as_u64();
            self.selected_track = Some(track_id);
            self.reveal_track = Some(track_id);
            let track = self.view.project.track(track_id).map(|t| t.name.clone()).unwrap_or_default();
            let at = self.view.project.bar_beat_string(start);
            self.status = format!("Pasted {name} on {track} at {at}");
        }
    }

    // ---------------------------------------------------------------- top bars

    fn menu_bar(&mut self, ui: &mut egui::Ui) {
        let bar = ui.max_rect();
        egui::menu::bar(ui, |ui| {
            let (logo, _) = ui.allocate_exact_size(egui::vec2(24.0, 16.0), egui::Sense::hover());
            widgets::paint_logo(ui.painter(), egui::Rect::from_min_size(logo.min, egui::vec2(16.0, 16.0)));
            ui.menu_button("File", |ui| {
                let ctx = ui.ctx().clone();
                if ui.add(egui::Button::new("New Project").shortcut_text("Ctrl+N")).clicked() {
                    ui.close_menu();
                    self.request_doc_action(&ctx, DocAction::New);
                }
                if ui.add(egui::Button::new("Open…").shortcut_text("Ctrl+O")).clicked() {
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
                if ui.add(egui::Button::new("Save").shortcut_text("Ctrl+S")).clicked() {
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
                if ui.button("Bounce Surround…").on_hover_text("A 5.1, 7.1 or 7.1.4 WAV in the project's surround format").clicked() {
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
                    .button("Export ADM BWF…")
                    .on_hover_text("An object-based master (ADM BWF) for Atmos delivery: 7.1.2 bed plus objects")
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
                if ui.button("Bounce Binaural…").on_hover_text("A stereo mix of the surround scene for headphones").clicked() {
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
                if ui.add(egui::Button::new("Bounce to WAV\u{2026}").shortcut_text("Ctrl+B")).clicked() {
                    ui.close_menu();
                    self.show_bounce = true;
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
                        egui::Button::new(undo).shortcut_text("Ctrl+Z"),
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
                        egui::Button::new(redo).shortcut_text("Ctrl+Y"),
                    )
                    .clicked()
                {
                    self.cmd(Command::Redo {});
                    ui.close_menu();
                }
                ui.separator();
                let edit_ctx = ui.ctx().clone();
                if ui
                    .add_enabled(
                        self.selected_region.is_some(),
                        egui::Button::new("Copy Region").shortcut_text("Ctrl+C"),
                    )
                    .clicked()
                {
                    self.copy_region(&edit_ctx, false);
                    ui.close_menu();
                }
                if ui
                    .add_enabled(
                        self.selected_region.is_some(),
                        egui::Button::new("Cut Region").shortcut_text("Ctrl+X"),
                    )
                    .clicked()
                {
                    self.copy_region(&edit_ctx, true);
                    ui.close_menu();
                }
                if ui
                    .add_enabled(
                        self.region_clipboard.is_some(),
                        egui::Button::new("Paste Region").shortcut_text("Ctrl+V"),
                    )
                    .clicked()
                {
                    self.paste_region();
                    ui.close_menu();
                }
                ui.separator();
                if ui
                    .add_enabled(
                        self.selected_region.is_some(),
                        egui::Button::new("Split Region at Playhead").shortcut_text("Ctrl+T"),
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
                if ui.add(egui::Button::new("Add Marker at Playhead").shortcut_text("Shift+M")).clicked() {
                    self.run_track_key(tk::TrackKey::Edit(tk::TrackEdit::Marker));
                    ui.close_menu();
                }
                if ui
                    .add_enabled(
                        self.selected_region.is_some(),
                        egui::Button::new("Duplicate Region").shortcut_text("Ctrl+D"),
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
                        self.can_repeat_notes(),
                        egui::Button::new("Repeat Selected Notes").shortcut_text("Ctrl+D"),
                    )
                    .clicked()
                {
                    self.repeat_selected_notes();
                    ui.close_menu();
                }
                if ui
                    .add_enabled(
                        self.selected_region.is_some(),
                        egui::Button::new("Delete Region").shortcut_text("Del"),
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
                if ui.button("MIDI Environment…").on_hover_text("Keyboard splits and layers: route note ranges of the MIDI input to tracks").clicked() {
                    self.extra.show_routes = true;
                    ui.close_menu();
                }
                ui.separator();
                let has = self.selected_track.is_some();
                let any = !self.view.project.tracks.is_empty();
                let (muted, soloed) = self
                    .selected_track
                    .and_then(|t| self.view.project.track(t))
                    .map_or((false, false), |t| (t.channel.mute, t.channel.solo));
                if ui
                    .add_enabled(any, egui::Button::new("Select Previous Track").shortcut_text("Up"))
                    .clicked()
                {
                    self.run_track_key(tk::TrackKey::Step(tk::Dir::Back));
                    ui.close_menu();
                }
                if ui
                    .add_enabled(any, egui::Button::new("Select Next Track").shortcut_text("Down"))
                    .clicked()
                {
                    self.run_track_key(tk::TrackKey::Step(tk::Dir::Forward));
                    ui.close_menu();
                }
                let mute = if muted { "Unmute Track" } else { "Mute Track" };
                if ui.add_enabled(has, egui::Button::new(mute).shortcut_text("M")).clicked() {
                    self.run_track_key(tk::TrackKey::Edit(tk::TrackEdit::Mute));
                    ui.close_menu();
                }
                let solo = if soloed { "Unsolo Track" } else { "Solo Track" };
                if ui.add_enabled(has, egui::Button::new(solo).shortcut_text("S")).clicked() {
                    self.run_track_key(tk::TrackKey::Edit(tk::TrackEdit::Solo));
                    ui.close_menu();
                }
                ui.separator();
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
                    .button("Analyze Mix").on_hover_text("Loudness, spectrum and advice for the mix")
                    .clicked()
                {
                    ui.close_menu();
                    self.analyze_mix();
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
                ui.radio_value(&mut self.bottom, BottomTab::Live, "Clip Grid");
                ui.radio_value(&mut self.bottom, BottomTab::Hidden, "Hide bottom panel");
                ui.checkbox(&mut self.extra.show_video, "Movie window");
                ui.checkbox(&mut self.extra.show_score, "Score view in editor");
                ui.checkbox(&mut self.follow_playhead, "Follow playhead");
                ui.checkbox(&mut self.automation.on, "Show Automation")
                    .on_hover_text("A: show each track's volume, pan or other automation over its regions");
                ui.separator();
                use crate::arrange_view::{can_step, ViewStep};
                let wheel_zoom = "Ctrl+wheel over the tracks zooms around the pointer";
                let wheel_height = "Ctrl+Shift+wheel over the tracks";
                for (step, label, tip) in [
                    (ViewStep::ZoomIn, "Zoom In", wheel_zoom),
                    (ViewStep::ZoomOut, "Zoom Out", wheel_zoom),
                    (ViewStep::Fit, "Zoom to Fit", "Show the whole song in the visible width"),
                    (ViewStep::Taller, "Taller Tracks", wheel_height),
                    (ViewStep::Shorter, "Shorter Tracks", wheel_height),
                ] {
                    if ui
                        .add_enabled(can_step(step, self.arrange_view()), egui::Button::new(label))
                        .on_hover_text(tip)
                        .clicked()
                    {
                        self.arrange_step = Some(step);
                        ui.close_menu();
                    }
                }
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
            // the document's name in the middle of the bar, with its save state
            let mut job = egui::text::LayoutJob::default();
            let fmt = |color| egui::TextFormat::simple(theme::body_font(), color);
            job.append(&self.view.project.name, 0.0, fmt(theme::TEXT_2));
            if self.view.dirty {
                job.append(" \u{b7} modified", 0.0, fmt(theme::TEXT_3));
            }
            let galley = ui.fonts(|f| f.layout_job(job));
            let at = egui::pos2(bar.center().x - galley.size().x / 2.0, bar.center().y - galley.size().y / 2.0);
            if at.x > ui.cursor().left() + theme::SP_6 {
                ui.painter().galley(at, galley, theme::TEXT_2);
            }
        });
    }

    fn windows(&mut self, ctx: &egui::Context) {
        self.settings_window(ctx);
        self.bounce_window(ctx);

        let mut open = self.show_mcp_info;
        dialog::window("MCP Server").open(&mut open).default_width(560.0).show(ctx, |ui| {
            let addr = self.mcp.as_ref().map(|h| h.address.clone()).unwrap_or_else(|| format!("http://127.0.0.1:{}/mcp", self.settings.mcp_port));
            ui.label(format!("Ondina exposes its complete API ({} tools) over the Model Context Protocol.", ondina_studio::mcp::tools().len()));
            ui.add_space(4.0);
            ui.label(RichText::new("Streamable HTTP endpoint (controls this running app):").strong());
            ui.code(&addr);
            ui.add_space(4.0);
            ui.label(RichText::new("Claude Code:").strong());
            ui.code(format!("claude mcp add --transport http ondina {addr}"));
            ui.add_space(4.0);
            ui.label(RichText::new("Stdio clients (e.g. Claude Desktop) — bridge to this app:").strong());
            let exe = std::env::current_exe().ok().and_then(|p| p.parent().map(|d| d.join("ondina-mcp.exe").display().to_string())).unwrap_or("ondina-mcp.exe".into());
            ui.code(format!("{{\n  \"mcpServers\": {{\n    \"ondina\": {{ \"command\": \"{}\" }}\n  }}\n}}", exe.replace('\\', "\\\\")));
            ui.label(RichText::new("Add --headless to run a private session without the GUI. Set ONDINA_MCP_TOKEN to require a bearer token.").small().color(theme::TEXT_DIM));
            ui.separator();
            ui.label(RichText::new("Recent external activity").strong());
            egui::ScrollArea::vertical().max_height(180.0).show(ui, |ui| {
                for l in self.view.log.iter().filter(|l| l.source != Source::Gui).take(60) {
                    let col = if l.source == Source::Ai { theme::AI } else { theme::PLAY };
                    ui.label(RichText::new(format!("[{:?}] {} {} — {}", l.source, l.command, if l.ok { "ok" } else { "failed" }, l.message)).small().color(col));
                }
            });
        });
        self.show_mcp_info = open;

        let mut open = self.show_about;
        dialog::window("Keyboard Shortcuts & About").open(&mut open).show(ctx, |ui| {
            ui.heading("Ondina DAW");
            ui.label("A native Windows digital audio workstation written in Rust, with an AI assistant and a full MCP API.");
            ui.separator();
            egui::Grid::new("keys").striped(true).show(ui, |ui| {
                for (k, d) in [
                    ("Space", "Play / Stop"),
                    ("R", "Record"),
                    ("C", "Cycle on/off"),
                    ("K", "Metronome"),
                    ("Home / Enter", "Go to start"),
                    (", / .", "Playhead to previous / next bar"),
                    ("Shift+, / Shift+.", "Playhead to previous / next marker"),
                    ("Ctrl+Z / Ctrl+Y", "Undo / Redo"),
                    ("Del", "Delete region or selected notes"),
                    ("Ctrl+D", "Duplicate region (piano roll: repeat selected notes)"),
                    ("Ctrl+T", "Split selected region at playhead"),
                    ("Ctrl+S / Ctrl+O / Ctrl+N", "Save / Open / New"),
                    ("Ctrl+B", "Bounce"),
                    ("Double-click track lane", "New MIDI region"),
                    ("Alt+drag region", "Copy region"),
                    ("Wheel", "Scroll tracks (piano roll: pitch)"),
                    ("Shift+wheel", "Scroll time"),
                    ("Ctrl+wheel", "Zoom time around the pointer"),
                    ("Ctrl+Shift+wheel", "Track height (piano roll: key height)"),
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

        self.mix_analysis_ui(ctx);
    }

    fn doc_prompt_ui(&mut self, ctx: &egui::Context) {
        let Some(action) = self.doc_prompt.clone() else {
            return;
        };
        dialog::backdrop(ctx, "doc_prompt_backdrop");
        let question = doc_guard::prompt_question(&self.view.project.name, &action);
        let mut answer = None;
        let shown = dialog::modal("Unsaved Changes", "doc_prompt")
            .show(ctx, |ui| {
                ui.label(RichText::new(question).strong());
                dialog::hint(ui, "Your changes will be lost if you don't save them.");
                ui.add_space(theme::SP_5);
                ui.horizontal(|ui| {
                    if dialog::primary_button(ui, "Save").clicked() {
                        answer = Some(Answer::Save);
                    }
                    if dialog::secondary_button(ui, "Don't Save").clicked() {
                        answer = Some(Answer::DontSave);
                    }
                    if dialog::secondary_button(ui, "Cancel").clicked() {
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

    /// At startup, when an earlier run did not close cleanly: offers the copies it left back.
    fn recovery_ui(&mut self, ctx: &egui::Context) {
        enum Choice {
            Recover(usize),
            Discard(usize),
            DiscardAll,
            Later,
        }
        if self.recovery.is_empty() {
            return;
        }
        dialog::backdrop(ctx, "recovery_backdrop");
        let now = crate::autosave::unix_secs();
        let mut choice = None;
        let shown = dialog::modal("Recover Unsaved Work", "recovery_prompt")
            .show(ctx, |ui| {
                ui.label(RichText::new("Ondina did not close normally. These copies of unsaved work were found:").strong());
                ui.add_space(6.0);
                egui::ScrollArea::vertical().max_height(8.0 * 44.0).show(ui, |ui| {
                    for (i, l) in self.recovery.iter().enumerate() {
                        ui.horizontal(|ui| {
                            ui.vertical(|ui| {
                                ui.label(RichText::new(&l.meta.name).strong());
                                let from = l.meta.original.as_ref().map_or_else(|| "never saved".to_string(), |o| format!("from {o}"));
                                let when = crate::autosave::age_label(now, l.meta.saved_at);
                                ui.label(RichText::new(format!("{from} · {when}")).small().color(theme::TEXT_DIM));
                            });
                            ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                                if dialog::secondary_button(ui, "Discard").clicked() {
                                    choice = Some(Choice::Discard(i));
                                }
                                if dialog::primary_button(ui, "Recover").clicked() {
                                    choice = Some(Choice::Recover(i));
                                }
                            });
                        });
                        ui.separator();
                    }
                });
                ui.label(
                    RichText::new("Audio files stay where they were; a recording that was still in progress is not in the copy.")
                        .small()
                        .color(theme::TEXT_DIM),
                );
                ui.add_space(8.0);
                ui.horizontal(|ui| {
                    if dialog::secondary_button(ui, "Discard All").clicked() {
                        choice = Some(Choice::DiscardAll);
                    }
                    if dialog::secondary_button(ui, "Decide Later").clicked() {
                        choice = Some(Choice::Later);
                    }
                });
            });
        if let Some(r) = shown {
            ctx.move_to_top(r.response.layer_id);
        }
        if ctx.input_mut(|i| i.consume_key(egui::Modifiers::NONE, Key::Escape)) {
            choice.get_or_insert(Choice::Later);
        }
        match choice {
            Some(Choice::Recover(i)) => {
                // The other copies stay on disk and are offered again at the next start.
                let copy = self.recovery[i].copy.display().to_string();
                self.recovery.clear();
                self.request_doc_action(ctx, DocAction::Recover(copy));
            }
            Some(Choice::Discard(i)) => {
                let l = self.recovery.remove(i);
                self.autosave.discard(&l.stem);
            }
            Some(Choice::DiscardAll) => {
                for l in self.recovery.drain(..) {
                    self.autosave.discard(&l.stem);
                }
            }
            Some(Choice::Later) => self.recovery.clear(),
            None => {}
        }
    }
}

pub(crate) fn snap_label(s: f64) -> &'static str {
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

impl eframe::App for OndinaApp {
    fn update(&mut self, ctx: &egui::Context, _frame: &mut eframe::Frame) {
        self.sync();
        self.poll_ai();
        self.update_meters();
        self.handle_close_request(ctx);
        self.cancel_drag_on_escape(ctx);
        if !self.doc_prompt_open() {
            self.handle_keys(ctx);
        }

        let title = format!(
            "Ondina DAW — {}{}",
            self.view.project.name,
            if self.view.dirty { " •" } else { "" }
        );
        ctx.send_viewport_cmd(egui::ViewportCommand::Title(title));

        egui::TopBottomPanel::top("menu")
            .exact_height(28.0)
            .frame(egui::Frame::none().fill(theme::CHROME).inner_margin(egui::Margin::symmetric(10.0, 0.0)))
            .show(ctx, |ui| self.menu_bar(ui));
        egui::TopBottomPanel::top("transport")
            .exact_height(crate::control_bar::BAR_HEIGHT)
            .frame(
                egui::Frame::none()
                    .fill(theme::PANEL)
                    .inner_margin(egui::Margin::symmetric(theme::SP_5, theme::SP_2)),
            )
            .show(ctx, |ui| self.control_bar(ui));
        egui::TopBottomPanel::bottom("status")
            .exact_height(24.0)
            .frame(egui::Frame::none().fill(theme::CHROME).inner_margin(egui::Margin::symmetric(theme::SP_5, 0.0)))
            .show(ctx, |ui| self.status_bar(ui));
        self.show_panels(ctx);
        egui::CentralPanel::default()
            .frame(egui::Frame::none().fill(theme::BG))
            .show(ctx, |ui| self.arrange_ui(ui));
        self.windows(ctx);
        self.extra_windows(ctx);
        self.recovery_ui(ctx);
        self.doc_prompt_ui(ctx);
        self.sync();
        self.autosave_tick(ctx);
        ctx.request_repaint_after(Duration::from_millis(if self.playing() || self.ai.busy {
            16
        } else {
            50
        }));
    }

    fn on_exit(&mut self, _gl: Option<&eframe::glow::Context>) {
        self.autosave.shutdown();
        self.settings.save();
        if let Some(h) = &self.mcp {
            h.shutdown();
        }
        // The bundled model server keeps gigabytes in memory: it goes away with the app.
        ondina_studio::local_ai::shutdown_server();
    }
}
