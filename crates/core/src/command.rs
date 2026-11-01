//! The complete Ondina API.
//!
//! Every user-facing operation is a [`Command`]. The GUI, undo/redo, the MCP server and the
//! built-in AI assistant all go through this single enum, so anything the app can do can be
//! automated. Each variant is exposed as an MCP tool named after the variant (snake_case);
//! the doc comment becomes the tool description and the fields become the input schema.
//!
//! Conventions: positions/lengths are in beats (quarter notes, beat 0 = start of bar 1);
//! MIDI pitch 60 = C4. track_id 0 addresses the master channel only in the channel commands
//! (set_volume, set_pan, set_mute, set_solo, add_effect, add_vst3, set_surround_pan, get_track);
//! the others, automation included, need a track or bus id.

use std::collections::BTreeMap;

use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

use crate::generate::{ArpMode, BassStyle, CompStyle, DrumStyle};
use crate::model::{AutomationPoint, CcEvent, Id, LyricEvent, MidiRoute, Note, PatternRow, Region, SurroundFormat, TrackKind};
use crate::theory::HarmonyInterval;

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
#[serde(tag = "command", rename_all = "snake_case")]
pub enum Command {
    // ------------------------------------------------------------------ project
    /// Get a compact overview of the whole project: tempo, key, time signature, every track with its id, instrument, plug-ins (with ids), regions (with ids, positions), markers, chord track and transport state. Call this first to learn ids.
    GetProject {},
    /// Get the complete raw project document as JSON (large). Prefer get_project / get_track / get_region.
    GetProjectJson {},
    /// Create a new empty project, replacing the current one. The transport stops (Live Loops cells too) and the playhead goes back to the start. Refused while a recording is in progress: call stop first (stop keeps the take). Refused when the current project has unsaved changes, unless discard_unsaved is true.
    NewProject {
        name: Option<String>,
        /// Tempo in BPM (default 120).
        tempo: Option<f64>,
        /// Time signature like "4/4", "3/4", "6/8".
        time_signature: Option<String>,
        /// Key like "C major", "A minor", "F# dorian".
        key: Option<String>,
        /// true: replace the project even if it has unsaved changes, which are lost (no undo). Default false: from an MCP client or the assistant, a project with unsaved changes is kept and the call fails; save it first with save_project, or ask the user.
        discard_unsaved: Option<bool>,
    },
    /// Open a project file (.ondina JSON). The transport stops and the playhead goes back to the start. Refused while a recording is in progress: call stop first (stop keeps the take). VST3 plug-ins whose path is not a .vst3 bundle on a local drive are not loaded and are listed in blocked_plugins. When an MCP client or the assistant opens the project, VST3 plug-ins outside the standard VST3 folders and the folders the user added in Ondina's VST3 library are not loaded either: they stay in the project and in the saved file, are listed in blocked_plugins, stay silent in playback and bounces, and load only when the user opens the project from the app. Refused when the current project has unsaved changes, unless discard_unsaved is true. The file must be on a local drive (no network shares). Audio files the project lists on a network share are not read either: they stay in the project and are listed in blocked_audio_files.
    OpenProject {
        path: String,
        /// true: replace the project even if it has unsaved changes, which are lost (no undo). Default false: from an MCP client or the assistant, a project with unsaved changes is kept and the call fails; save it first with save_project, or ask the user.
        discard_unsaved: Option<bool>,
    },
    /// Save the project. Without a path saves to the current file (or fails if never saved). The path must be a .ondina file (or an older .aurora one) on a local drive (no network shares); .ondina is added when the path has no extension. An existing file is overwritten only if it is an Ondina project. The previous version is kept next to it as <file>.bak when that name is free or already holds an Ondina project; otherwise the backup is skipped, the file is saved anyway and the result reports backup_error.
    SaveProject { path: Option<String> },
    /// Rename the project.
    SetProjectName { name: String },
    /// Store free-form notes in the project (lyrics, production brief, ideas).
    SetProjectNotes { notes: String },
    /// Set the project tempo (BPM) at the start of the song.
    SetTempo { bpm: f64 },
    /// Add a tempo change at a beat position (step change).
    AddTempoChange { beat: f64, bpm: f64 },
    /// Remove all tempo changes, keeping only the initial tempo.
    ClearTempoChanges {},
    /// Set the time signature.
    SetTimeSignature { numerator: u8, denominator: u8 },
    /// Set the project key, e.g. "C major", "A minor", "Eb mixolydian", "F# minor_pentatonic".
    SetKey { key: String },
    /// Estimate the key of the MIDI parts from their notes (Krumhansl-Kessler key profiles; major and minor keys only). Analyzes one MIDI region, or without region_id every unmuted MIDI region on instrument tracks, skipping Drum Kit tracks and step patterns. Returns the key, a confidence 0..1, low_confidence when there are few notes, and the 3 next candidates: relative keys (C major / A minor) are often close, so check them. apply=true also sets the project key (undoable).
    DetectKey {
        /// MIDI region to analyze; omit to analyze the whole project.
        region_id: Option<Id>,
        /// Set the project key to the result (default false).
        apply: Option<bool>,
    },
    /// Undo the last edit.
    Undo {},
    /// Redo the last undone edit.
    Redo {},

    // ------------------------------------------------------------------ transport
    /// Start playback, optionally from a beat position.
    Play { from_beat: Option<f64> },
    /// Stop playback (and recording).
    Stop {},
    /// Start recording on all record-armed tracks (audio tracks record from the audio input, instrument tracks from MIDI input).
    Record { from_beat: Option<f64> },
    /// Move the playhead. Give either a beat or a 1-based bar number.
    SetPlayhead { beat: Option<f64>, bar: Option<f64> },
    /// Configure the cycle (loop) range.
    SetCycle { enabled: bool, start: Option<f64>, end: Option<f64> },
    /// Enable or disable the metronome click, and optionally set its level and whether it clicks only while recording. The count-in before a recording always clicks, at the same level. Returns the resulting metronome, level_db and only_while_recording.
    SetMetronome {
        enabled: bool,
        /// Click level in dB, from -60 (very quiet) to 0 (full scale); values outside are clamped. A new project starts at about -9.1. Omit to keep the current level.
        level_db: Option<f32>,
        /// true: the click sounds only while recording (and in the count-in), not in plain playback. Omit to keep the current setting.
        only_while_recording: Option<bool>,
    },
    /// Set the number of count-in bars played before recording starts (0-4).
    SetCountIn { bars: u32 },
    /// Get transport state: playing, recording, playhead position (beats, bar.beat.tick, seconds), cycle, metronome, metronome_level_db, metronome_only_while_recording, sample rate, dsp_load (share of the audio block time the last block used) and dsp_overloads (audio blocks since the last Play that took longer than the audio device allows; each one can be heard as a click).
    GetTransport {},
    /// Get what the user has selected in the Ondina window: the selected track, the selected region and up to 256 selected notes of that region (each with its index, pitch, name, start and length in beats from the region start, velocity), plus the playhead and the cycle range. To act on "the notes I selected", pass the note index values as `indices` to quantize_notes, transpose_notes, humanize_notes, set_note_velocities, set_note_lengths, snap_notes_to_key, add_harmony or remove_notes. Indices refer to the region as it is now: read the selection again after editing the region, and compare pitch and start if in doubt. selected_note_count is the real number of selected notes and can be larger than the list. A headless session has no window: track and region are null and notes is empty.
    GetSelection {},
    /// Get what happened in this Ondina session since you last looked, newest last: the edits the user made in the Ondina window (source "gui"), the tools called by MCP clients ("mcp"), the built-in assistant's tool calls ("ai") and scripts ("script"). Each entry has seq, source, command (the tool name), ok, mutating (whether the command changes the project), message (a short summary of the result or the error; it can contain file paths that the user or a client chose), count (how many steps of one continuous gesture, such as a fader drag, the entry stands for) and seconds_ago. Save latest_seq and pass it as `since` next time to get only newer entries; without `since` you get the latest `limit` entries. truncated is true when entries after `since` are missing from the list (the session keeps the last 500, `limit` cut the list, or `since` is larger than latest_seq because Ondina was restarted): read get_project again. revision grows with every change to the project, also changes that leave no entry (knob moves in native plug-in windows); document changes when open_project or new_project replaced the project and session_id changes when Ondina restarts: if either differs from what you saw, ids you remembered may no longer exist. Calls to get_activity are not listed. The selection and the playhead are not listed: read them with get_selection and get_transport.
    GetActivity {
        /// latest_seq from your previous get_activity call: only newer entries are returned.
        since: Option<u64>,
        /// Most entries to return, the newest are kept (default 50, at most 200; 0 returns only the counters).
        limit: Option<usize>,
    },
    /// Silence all sounding notes immediately.
    Panic {},

    // ------------------------------------------------------------------ tracks
    /// Add a track. For instrument tracks give a `patch` (see list_patches) or an `instrument` kind + optional `preset`. Returns the new track id.
    AddTrack {
        kind: TrackKind,
        name: Option<String>,
        /// Library patch name (instrument + effects), e.g. "Pop Drum Kit", "Lush Pad", "Lead Vocal".
        patch: Option<String>,
        /// Instrument plug-in kind, e.g. "analog_synth", "drum_synth", "fm_synth", "organ", "pluck_synth", "sampler".
        instrument: Option<String>,
        /// Instrument preset name (see list_presets).
        preset: Option<String>,
        /// Insert position (0-based). Default: end.
        index: Option<usize>,
    },
    /// Delete a track and everything on it. Sends to it from other tracks are removed with their send automation (later send:<i> lanes are renumbered so they stay on the same sends), automation lanes elsewhere that target its plug-ins are removed, and its Live Loops cells and the MIDI input routes to it are deleted, not moved to another track. Returns how many lanes, cells and routes were removed.
    RemoveTrack { track_id: Id },
    /// Duplicate a track with its plug-ins, regions and automation. In the copy, plugin:<id>:<param> lanes point at the copy's own plug-ins; volume, pan, send and surround lanes are copied as they are. Returns the new track id.
    DuplicateTrack { track_id: Id },
    /// Rename a track.
    RenameTrack { track_id: Id, name: String },
    /// Set a track color as hex "#RRGGBB".
    SetTrackColor { track_id: Id, color: String },
    /// Move a track to a new position (0-based).
    MoveTrack { track_id: Id, index: usize },
    /// Set channel fader volume in dB (0 = unity, -inf..+6). track_id 0 = master.
    SetVolume { track_id: Id, db: f32 },
    /// Set channel pan, -1 (left) .. 0 (center) .. 1 (right). track_id 0 = master balance.
    SetPan { track_id: Id, pan: f32 },
    /// Mute or unmute a track.
    SetMute { track_id: Id, mute: bool },
    /// Solo or unsolo a track.
    SetSolo { track_id: Id, solo: bool },
    /// Arm or disarm a track for recording.
    SetRecordArm { track_id: Id, armed: bool },
    /// Route a track output to a bus (bus_id) or back to the master (bus_id omitted).
    SetTrackOutput { track_id: Id, bus_id: Option<Id> },
    /// Add a send from a track to a bus track (e.g. a shared reverb). level_db default -10.
    AddSend { track_id: Id, bus_id: Id, level_db: Option<f32>, pre_fader: Option<bool> },
    /// Change the level of an existing send.
    SetSendLevel { track_id: Id, send_index: usize, level_db: f32 },
    /// Remove a send. Its send:<index> automation lane is removed too, and the lanes of the later sends are renumbered so they keep automating the same sends.
    RemoveSend { track_id: Id, send_index: usize },
    /// Get full details of a track: channel strip, instrument and plug-ins with all parameter values, sends, regions, automation.
    GetTrack { track_id: Id },
    /// Replace a track's instrument and insert effects with a library patch. Automation lanes that targeted the replaced plug-ins (plugin:<id>:<param>, on any track) are removed; volume, pan, send and MIDI effect lanes stay. Returns the track details and removed_automation_lanes.
    ApplyPatch { track_id: Id, patch: String },

    // ------------------------------------------------------------------ plug-ins
    /// List available plug-ins (instruments, effects, MIDI effects) with every parameter: id, range, default, unit, choices. category: "instrument", "effect" or "midi_effect".
    ListPlugins { category: Option<String> },
    /// List factory presets, optionally for one plug-in kind.
    ListPresets { kind: Option<String> },
    /// List library patches (complete channel strip setups: instrument + effect chain) by category.
    ListPatches {},
    /// Set (or replace) the instrument of an instrument track. Automation lanes that targeted the old instrument are removed. Returns the new plugin_id and removed_automation_lanes.
    SetInstrument { track_id: Id, kind: String, preset: Option<String> },
    /// Insert an audio effect on a track's channel strip (track_id 0 = master). index = slot position (default: last). Returns plugin id.
    AddEffect { track_id: Id, kind: String, preset: Option<String>, index: Option<usize> },
    /// Insert a MIDI effect (arpeggiator, transposer, chord_trigger, velocity_processor) before the instrument.
    AddMidiEffect { track_id: Id, kind: String, preset: Option<String> },
    /// Remove a plug-in by id, together with every plugin:<id>:<param> automation lane that targets it.
    RemovePlugin { plugin_id: Id },
    /// Move an insert plug-in to another slot index within its chain.
    MovePlugin { plugin_id: Id, index: usize },
    /// Set one plug-in parameter (see list_plugins for ids and ranges; selector params take the choice index).
    SetPluginParam { plugin_id: Id, param: String, value: f32 },
    /// Set several plug-in parameters at once.
    SetPluginParams { plugin_id: Id, params: BTreeMap<String, f32> },
    /// Load a factory preset into a plug-in.
    LoadPreset { plugin_id: Id, preset: String },
    /// Bypass or enable a plug-in.
    SetPluginBypass { plugin_id: Id, bypass: bool },
    /// Get a plug-in with all its parameter values and their ranges.
    GetPlugin { plugin_id: Id },
    /// Load an audio file into a Sampler instrument. The file must be on a local drive (no network shares).
    SetSamplerSample { plugin_id: Id, path: String },

    // ------------------------------------------------------------------ regions
    /// Create a MIDI region on an instrument track, optionally with notes (note start is relative to the region). Returns region id.
    CreateMidiRegion { track_id: Id, start: f64, length: f64, name: Option<String>, notes: Option<Vec<Note>> },
    /// Create a step-sequencer pattern region. Without rows a default drum kit layout (kick, snare, clap, hats, toms, crash) is used.
    CreatePatternRegion {
        track_id: Id,
        start: f64,
        /// Number of steps (default 16).
        steps: Option<u32>,
        /// Step length in beats (default 0.25 = 1/16).
        step_length: Option<f64>,
        rows: Option<Vec<PatternRow>>,
        name: Option<String>,
    },
    /// Import an audio file (wav, aiff, flac, mp3, ogg, m4a) onto an audio track (creates one if track_id omitted) at a beat position. The file must be on a local drive (no network shares).
    ImportAudio { path: String, track_id: Option<Id>, start: Option<f64> },
    /// Delete a region.
    DeleteRegion { region_id: Id },
    /// Move a region in time and/or to another track.
    MoveRegion { region_id: Id, start: Option<f64>, track_id: Option<Id> },
    /// Change a region's length in beats (MIDI notes beyond the end are kept but not played). A pattern region keeps whole steps and can have at most 65536 steps: a longer length is refused.
    ResizeRegion { region_id: Id, length: f64 },
    /// Split a region at an absolute beat position. Returns the new right-hand region id.
    SplitRegion { region_id: Id, beat: f64 },
    /// Join several MIDI regions on the same track into one.
    JoinRegions { region_ids: Vec<Id> },
    /// Duplicate a region `count` times (default 1), placed back-to-back after it or starting at `start`.
    DuplicateRegion { region_id: Id, start: Option<f64>, count: Option<u32> },
    /// Paste a copy of a whole region onto a track at a beat position. Pass `region` as one entry of `tracks[].regions` from get_project_json, unchanged: notes, controller events, lyrics, transpose and velocity offset (MIDI), steps and swing (pattern), file, offset, gain, fades and reverse (audio), plus name, color, mute and loop count all come along. Its id, start and take are ignored: the copy gets a new id, starts at `start` and is not part of a take set. MIDI and pattern regions need an instrument track; audio regions need an audio track and a file_id listed in the project's audio_files. Returns region_id. One undo step.
    PasteRegion {
        /// Destination track: an instrument track for MIDI and pattern regions, an audio track for audio regions.
        track_id: Id,
        /// Where the copy starts, in beats.
        start: f64,
        /// A region object as returned by get_project_json (an entry of tracks[].regions).
        region: Box<Region>,
    },
    /// Change region properties. Only given fields change.
    SetRegionProperties {
        region_id: Id,
        name: Option<String>,
        muted: Option<bool>,
        /// Number of repetitions (1 = no loop).
        loop_count: Option<u32>,
        /// MIDI: playback transposition in semitones.
        transpose: Option<i32>,
        /// MIDI: velocity offset.
        velocity_offset: Option<i32>,
        /// Audio: gain in dB.
        gain_db: Option<f32>,
        /// Audio: fade in length in beats.
        fade_in: Option<f64>,
        /// Audio: fade out length in beats.
        fade_out: Option<f64>,
        /// Audio: play reversed.
        reverse: Option<bool>,
        /// Hex color "#RRGGBB". An empty string or "none" removes the region's own color, so it shows its
        /// track's color again.
        color: Option<String>,
    },
    /// Get a region with its notes (MIDI), steps (pattern) or audio settings.
    GetRegion { region_id: Id },

    // ------------------------------------------------------------------ notes
    /// Add notes to a MIDI region (start relative to region start, in beats). The region's notes are re-sorted by time afterwards; the result's `indices` lists where the added notes ended up (ascending), ready to pass to transpose_notes, quantize_notes, humanize_notes, set_note_velocities or remove_notes. An added note identical to one already in the region comes right after it.
    AddNotes { region_id: Id, notes: Vec<Note> },
    /// Replace all notes of a MIDI region.
    ReplaceNotes { region_id: Id, notes: Vec<Note> },
    /// Remove notes from a MIDI region. Filters combine; with no filter all notes are removed.
    RemoveNotes { region_id: Id, indices: Option<Vec<usize>>, pitch: Option<u8>, from_beat: Option<f64>, to_beat: Option<f64> },
    /// Transpose notes (all, or the given indices) by semitones.
    TransposeNotes { region_id: Id, semitones: i32, indices: Option<Vec<usize>> },
    /// Quantize notes (all, or the given indices) to a grid in beats (0.25 = 1/16). strength 0..1 (default 1), swing 0..1. Notes are re-sorted by time afterwards; with indices, the result's `indices` lists where those notes ended up. Indices past the last note are ignored.
    QuantizeNotes { region_id: Id, grid: f64, strength: Option<f64>, swing: Option<f64>, quantize_ends: Option<bool>, indices: Option<Vec<usize>> },
    /// Humanize notes (all, or the given indices): random timing deviation (beats, default 0.01) and velocity deviation (default 8). Notes are re-sorted by time afterwards; with indices, the result's `indices` lists where those notes ended up. Indices past the last note are ignored.
    HumanizeNotes { region_id: Id, timing: Option<f64>, velocity: Option<i32>, seed: Option<u64>, indices: Option<Vec<usize>> },
    /// Set velocities: fixed value, or scale factor, for all or given notes.
    SetNoteVelocities { region_id: Id, velocity: Option<u8>, scale: Option<f32>, indices: Option<Vec<usize>> },
    /// Change the lengths of notes in a MIDI region (all, or the given indices). Starts and pitches never change, so the notes keep their order and their indices. mode "legato": each note lasts until the start of the next chosen note that starts later; chosen notes starting less than 1/32 beat after the first note of a group count as one chord and end together; the last group keeps its lengths. "no_overlap": a chosen note is cut where any note of the same pitch starts again in the region. "fixed": every chosen note lasts `length` beats. "scale": every chosen length is multiplied by `factor` (0.5 halves, 2 doubles). No note is made shorter than 1/64 beat, and a lengthened note stops at the end of the region (a note already past it keeps its length). Returns `changed` (how many notes got a new length), `note_count` and, when indices were given, the same `indices`. Indices past the last note are ignored.
    SetNoteLengths {
        region_id: Id,
        /// "legato", "no_overlap", "fixed" or "scale".
        mode: NoteLengthMode,
        /// New length in beats for mode "fixed" (1 = a quarter note, 0.25 = 1/16). Ignored by the other modes.
        length: Option<f64>,
        /// Multiplier for mode "scale" (0.5 = half as long, 2 = twice as long). Ignored by the other modes.
        factor: Option<f64>,
        indices: Option<Vec<usize>>,
    },
    /// Move notes of a region (all, or the given indices) to the nearest pitch in the project key. Indices past the last note are ignored.
    SnapNotesToKey { region_id: Id, indices: Option<Vec<usize>> },
    /// Add a harmony voice to notes of a MIDI region (all, or the given indices): each note gets a copy with the same start and length and 85% of its velocity, `interval` above it (below with below=true) in the project key. In a seven-note key "3rd", "5th" and "6th" count scale steps, so the voice stays in the key (C major, 3rd above: C->E, E->G, B->D); a note outside the key starts from the nearest scale note. In pentatonic, blues and chromatic keys the copy goes to the scale note nearest to the major interval. "octave" is always 12 semitones. A copy identical to a note already there (same pitch and start) is not added (already_present), nor one outside MIDI 0..127 (out_of_range). Notes are re-sorted by time afterwards; the result's `indices` lists where the added notes ended up (ascending), ready to pass to transpose_notes, set_note_velocities or remove_notes. Indices past the last note are ignored.
    AddHarmony {
        region_id: Id,
        /// "3rd", "5th", "6th" or "octave", counted in the project key.
        interval: HarmonyInterval,
        /// Put the voice below the notes (default false: above).
        below: Option<bool>,
        indices: Option<Vec<usize>>,
    },
    /// Insert a voiced chord (symbol like "Cmaj7", "F#m", "G/B") into a MIDI region.
    InsertChord { region_id: Id, symbol: String, start: f64, length: f64, octave: Option<i32>, inversion: Option<u8>, velocity: Option<u8>, with_bass: Option<bool> },
    /// Add MIDI controller events (64 sustain, 1 mod wheel, 7 volume...) to a region.
    AddCc { region_id: Id, events: Vec<CcEvent> },
    /// Replace the events of one MIDI controller in a MIDI region: removes that controller's events with from_beat <= beat < to_beat (from the region start and to its end when omitted, so with neither the whole controller is cleared) and inserts `events`, keeping the events of every other controller. Beats are relative to the region start, values 0..127. Controller numbers as in add_cc: 64 sustain (0 = up, 127 = down), 1 mod wheel, 11 expression, 7 volume, 10 pan, 128 pitch bend (64 = centre). An empty `events` erases the range. Returns `removed` and `added`.
    ReplaceCc {
        region_id: Id,
        /// Controller number 0..127, or 128 for pitch bend.
        controller: u8,
        /// Start of the range to replace (inclusive), in beats relative to the region start.
        from_beat: Option<f64>,
        /// End of the range to replace (exclusive), in beats relative to the region start.
        to_beat: Option<f64>,
        /// The controller's new events; they are stored sorted by beat.
        events: Vec<CcPoint>,
    },

    // ------------------------------------------------------------------ step sequencer
    /// Set one step of a pattern region: velocity 0 = off.
    SetPatternStep { region_id: Id, row: usize, step: usize, velocity: u8 },
    /// Set a whole pattern row from a string: 'x' accent, 'o' normal, 'g' ghost, '.' off. E.g. "x...x...x...x...". One character per step of the pattern (spaces and '|' are ignored): a string with more or fewer steps is refused.
    SetPatternRow { region_id: Id, row: usize, pattern: String },
    /// Add a row to a pattern region.
    AddPatternRow { region_id: Id, name: String, pitch: u8 },
    /// Change a pattern row in place: rename it, change its MIDI pitch (the drum sound) or mute it (a muted row keeps its steps but does not play). Only the fields passed change; the steps stay as they are.
    UpdatePatternRow {
        region_id: Id,
        /// Row index, from get_region.
        row: usize,
        /// New name (surrounding spaces are trimmed; must not be empty).
        name: Option<String>,
        /// MIDI pitch 0..127 (General MIDI drums: 36 kick, 38 snare, 39 clap, 42 closed hat, 46 open hat, 49 crash, 51 ride).
        pitch: Option<u8>,
        /// true mutes the row, false unmutes it.
        muted: Option<bool>,
    },
    /// Delete a row of a pattern region together with its steps. The rows after it move up one index, so read get_region again before the next edit by row index. Deleting the last row leaves an empty pattern that add_pattern_row can fill again.
    RemovePatternRow { region_id: Id, row: usize },
    /// Set pattern swing 0..1 (applied to every second step). A value outside 0..1 is refused. Returns the swing.
    SetPatternSwing { region_id: Id, swing: f32 },
    /// Convert a pattern region into a regular MIDI region.
    ConvertPatternToMidi { region_id: Id },

    // ------------------------------------------------------------------ session players (generators)
    /// Session drummer: generate a drum groove as a MIDI region on a (drum) instrument track. Returns region_id, seed, start, bars; pass the returned seed to get the same part again. bars is at most 512.
    GenerateDrums { track_id: Id, start: f64, bars: u32, style: DrumStyle, intensity: Option<f32>, fill: Option<bool>, seed: Option<u64> },
    /// Session bassist: generate a bass line following `chords` (one symbol per `beats_per_chord`, default one bar) or the chord track. Returns region_id, seed, start, bars; pass the returned seed to get the same part again. bars is at most 512. beats_per_chord is at least 0.25. The chord track repeats after its last chord, so every bar asked for is filled.
    GenerateBass { track_id: Id, start: f64, bars: u32, style: BassStyle, chords: Option<Vec<String>>, beats_per_chord: Option<f64>, octave: Option<i32>, seed: Option<u64> },
    /// Session keyboard player: generate chord comping following `chords` or the chord track. Returns region_id, seed, start, bars; pass the returned seed to get the same part again. bars is at most 512. beats_per_chord is at least 0.25. The chord track repeats after its last chord, so every bar asked for is filled.
    GenerateChords { track_id: Id, start: f64, bars: u32, style: CompStyle, chords: Option<Vec<String>>, beats_per_chord: Option<f64>, center_pitch: Option<u8>, seed: Option<u64> },
    /// Generate an arpeggio following `chords` or the chord track. rate in beats (0.25 = 1/16). Returns region_id, seed, start, bars; pass the returned seed to get the same part again. bars is at most 512. beats_per_chord is at least 0.25. The chord track repeats after its last chord, so every bar asked for is filled.
    GenerateArpeggio { track_id: Id, start: f64, bars: u32, mode: ArpMode, rate: Option<f64>, octaves: Option<u32>, chords: Option<Vec<String>>, beats_per_chord: Option<f64>, center_pitch: Option<u8>, seed: Option<u64> },
    /// Generate a melody in the project key over `chords` or the chord track. density 0..1. Returns region_id, seed, start, bars; pass the returned seed to get the same part again. bars is at most 512. beats_per_chord is at least 0.25. The chord track repeats after its last chord, so every bar asked for is filled.
    GenerateMelody { track_id: Id, start: f64, bars: u32, density: Option<f32>, octave: Option<i32>, chords: Option<Vec<String>>, beats_per_chord: Option<f64>, seed: Option<u64> },

    // ------------------------------------------------------------------ chord track
    /// Write a chord progression to the chord track. Accepts symbols ("Am7", "F", "G/B") or roman numerals in the project key ("I", "vi", "IV", "V7").
    SetChordProgression { start: f64, chords: Vec<String>, beats_per_chord: Option<f64>, replace: Option<bool> },
    /// Add one chord to the chord track. symbol is a chord symbol ("Am7", "G/B") or a roman numeral in the project key ("IV", "vi"), stored as a chord symbol. Returns chord_id and the stored symbol.
    AddChord { beat: f64, length: f64, symbol: String },
    /// Change a chord on the chord track: symbol (chord symbol or roman numeral in the project key, stored as a chord symbol), beat (start) or length in beats. Omitted fields stay as they are.
    UpdateChord { chord_id: Id, symbol: Option<String>, beat: Option<f64>, length: Option<f64> },
    /// Remove a chord from the chord track.
    RemoveChord { chord_id: Id },
    /// Clear the chord track.
    ClearChords {},

    // ------------------------------------------------------------------ automation
    /// Replace the automation of a parameter on a track or bus (the master has no automation: track_id 0 is refused). target: "volume" (dB, -96..6), "pan" (-1..1), "send:<index>" (dB, -96..6), "surround:azimuth" (degrees, -180..180), "surround:elevation" (degrees, -30..90), "surround:spread" (0..1), or "plugin:<plugin_id>:<param>" for a parameter of a built-in plug-in on the same track (its range is in get_plugin). VST3 parameters cannot be automated: set them with set_plugin_param. Values outside the range are clamped; values that are not finite are refused. Returns the stored points.
    SetAutomation { track_id: Id, target: String, points: Vec<AutomationPoint> },
    /// Add one automation point to a track or bus lane, with the same targets, ranges and limits as set_automation (no master automation). Returns the number of points and the stored value.
    AddAutomationPoint { track_id: Id, target: String, beat: f64, value: f32 },
    /// Remove automation for a target, or all automation of the track if target omitted (tracks and buses; the master has no automation).
    ClearAutomation { track_id: Id, target: Option<String> },

    // ------------------------------------------------------------------ markers & arrangement
    /// Add a marker. Give a length to create an arrangement marker (section like "Verse", "Chorus").
    AddMarker { beat: f64, name: String, length: Option<f64> },
    /// Remove a marker.
    RemoveMarker { marker_id: Id },
    /// Change marker name, position or length.
    UpdateMarker { marker_id: Id, name: Option<String>, beat: Option<f64>, length: Option<f64> },
    /// Copy every region (and chords) in a time section to another position, e.g. repeat a chorus. insert=true shifts later material to make room.
    CopySection { from_beat: f64, length: f64, to_beat: f64, insert: Option<bool> },
    /// Delete everything in a time section; ripple=true pulls later material back.
    DeleteSection { from_beat: f64, length: f64, ripple: Option<bool> },
    /// Insert empty time at a position, pushing later material.
    InsertTime { at_beat: f64, length: f64 },

    // ------------------------------------------------------------------ import / export
    /// Bounce (render) the mix to an audio file (.wav). Defaults: whole song, 24 bit. track_ids renders only those tracks (soloed). After the end the render goes on until reverb and delay tails fade below about -90 dBFS (at least 2 s, at most 20 s: a sound that never fades, like a note held past the end or a noisy plug-in, stops at 20 s). tail_sec sets an exact tail instead: 0 cuts exactly at end, e.g. to export a loop. normalize=true scales the peak to -1 dBFS. normalize_lufs brings the integrated loudness to a target with one gain for the whole mix (no limiter), keeping the peak under true_peak_ceiling_db: the result's loudness reports gain_db, lufs_after, true_peak_after and target_reached (false when the ceiling stopped the gain: the mix needs a limiter, e.g. auto_master, to get that loud). The peak is the analyze_mix estimate; 16 and 24 bit dither can add about one LSB. The path must be a .wav file on a local drive (no network shares); .wav is added when the path has no extension. A render covers at most 60 minutes of the song (plus the tail): a longer range is refused.
    Bounce {
        path: String,
        start: Option<f64>,
        end: Option<f64>,
        bit_depth: Option<u16>,
        /// true: scale the peak to -1 dBFS. Ignored when normalize_lufs is set.
        normalize: Option<bool>,
        track_ids: Option<Vec<Id>>,
        /// Exact tail after end, in seconds (0 to 60). Omit it for the automatic tail; 0 cuts exactly at end.
        tail_sec: Option<f64>,
        /// Loudness target in LUFS, -30 to -6 (-14 streaming, -16, -23 broadcast).
        normalize_lufs: Option<f32>,
        /// Peak ceiling for normalize_lufs in dBTP, -9 to 0 (default -1).
        true_peak_ceiling_db: Option<f32>,
    },
    /// Render a track to audio and place it on a new audio track (bounce in place). The source track is muted. Reverb and delay tails are kept: the render goes on until they fade (at least 2 s, at most 20 s after the last region). A render covers at most 60 minutes of the song (plus the tail): a longer range is refused.
    BounceInPlace { track_id: Id },
    /// Export MIDI regions to a Standard MIDI File. The path must be a .mid or .midi file on a local drive (no network shares); .mid is added when the path has no extension.
    ExportMidi { path: String, track_ids: Option<Vec<Id>> },
    /// Import a Standard MIDI File, creating instrument tracks (channel 10 becomes a drum track). The file must be on a local drive (no network shares).
    ImportMidi { path: String, at_beat: Option<f64> },

    // ------------------------------------------------------------------ analysis / mastering
    /// Render offline and analyze: integrated loudness (LUFS), true-peak estimate, RMS, crest factor, stereo correlation, low/mid/high spectral balance for the master and each track. Use it to "listen" to the mix. A render covers at most 60 minutes of the song (plus the tail): a longer range is refused.
    AnalyzeMix { start: Option<f64>, end: Option<f64> },
    /// Get live peak meters (dBFS) for every track and the master.
    GetMeters {},
    /// Mastering assistant: analyze the mix and build a master chain (EQ, glue compressor, limiter) targeting a loudness (default -14 LUFS).
    AutoMaster { target_lufs: Option<f32>, character: Option<String> },

    // ------------------------------------------------------------------ devices
    /// List audio output/input devices and the current one.
    ListAudioDevices {},
    /// Select audio devices by name.
    SetAudioDevice { output: Option<String>, input: Option<String> },
    /// List MIDI input ports.
    ListMidiInputs {},
    /// Connect a MIDI input port (by name substring) to play the selected/armed instrument track.
    ConnectMidiInput { name: Option<String> },
    /// Play a note on a track's instrument for auditioning (duration in seconds, default 0.5).
    PreviewNote { track_id: Id, pitch: u8, velocity: Option<u8>, duration: Option<f32> },
    /// Send real-time MIDI events to a track's instrument (play it live from an agent). Events run immediately in order; use `delay_ms` for timing.
    SendLiveMidi { track_id: Id, events: Vec<LiveMidiEvent> },

    // ------------------------------------------------------------------ flex time / pitch (audio editing)
    /// Flex Time: time-stretch an audio region without changing pitch (WSOLA). Give a ratio (2.0 = twice as long) or a target length in beats. Renders a new audio file.
    TimeStretchRegion { region_id: Id, ratio: Option<f64>, target_length: Option<f64> },
    /// Flex Pitch: transpose an audio region without changing its duration.
    PitchShiftRegion { region_id: Id, semitones: f64 },
    /// Varispeed: change speed and pitch together like tape (percent: 100 = unchanged, 200 = double speed and one octave up).
    VarispeedRegion { region_id: Id, percent: f64 },
    /// Pitch correction: detect the pitch of a monophonic audio region (vocal, bass, lead) and pull every note toward the nearest note of the project key. strength 0..1 (default 1).
    PitchCorrectRegion { region_id: Id, strength: Option<f32> },
    /// Flex Pitch to MIDI: transcribe a monophonic audio region to a MIDI region (pitch tracking). Creates an instrument track unless track_id is given.
    AudioToMidi { region_id: Id, track_id: Option<Id> },
    /// Smart Tempo: estimate the tempo (BPM) of an audio region; apply=true sets the project tempo.
    DetectTempo { region_id: Id, apply: Option<bool> },
    /// Normalize an audio region gain so its peak reaches the target (default -1 dBFS).
    NormalizeRegion { region_id: Id, target_db: Option<f32> },
    /// Stem Splitter: separate an audio region into vocals, drums, bass and other stems on four new audio tracks. engine "ml" (default when the model is installed) uses a neural network (Spleeter) for vocals/accompaniment; "spectral" uses DSP only.
    SplitStems { region_id: Id, engine: Option<String> },

    // ------------------------------------------------------------------ comping
    /// List recorded takes (overlapping regions) on a track, with their take numbers.
    ListTakes { track_id: Id },
    /// Build a comp: for each segment pick the take region to use between from_beat and to_beat. Creates comp regions and mutes the original takes.
    CompTakes { track_id: Id, segments: Vec<CompSegment> },

    // ------------------------------------------------------------------ score
    /// Get a region as music notation: measures with notes (pitch names, note values, dots, ties, rests) and lyrics.
    GetScore { region_id: Id },
    /// Export the score of instrument tracks as MusicXML (opens in MuseScore, Sibelius, Finale, Dorico). The path must be a .musicxml or .xml file on a local drive (no network shares); .musicxml is added when the path has no extension.
    ExportMusicXml { path: String, track_ids: Option<Vec<Id>> },
    /// Attach lyrics to a MIDI region: syllables in `text` (split by spaces; use a hyphen inside words like sun-shine) are assigned to the melody notes in order. Or give explicit `lyrics`.
    SetLyrics { region_id: Id, text: Option<String>, lyrics: Option<Vec<LyricEvent>> },

    // ------------------------------------------------------------------ live loops
    /// Get the Live Loops grid: scenes, cells (per track/scene) and which cells are playing or queued.
    GetLiveLoops {},
    /// Add a scene (row) to the Live Loops grid. The grid has at most 256 scenes.
    AddScene { name: Option<String> },
    /// Rename a scene.
    RenameScene { scene: usize, name: String },
    /// Remove a scene and its cells.
    RemoveScene { scene: usize },
    /// Create a Live Loops cell on a track and scene: empty MIDI (length in beats), with notes, or copied from an existing region. scene is 0..255 (the grid has at most 256 scenes); missing scenes up to it are added.
    CreateLiveCell { track_id: Id, scene: usize, length: Option<f64>, notes: Option<Vec<Note>>, from_region_id: Option<Id>, name: Option<String> },
    /// Delete a Live Loops cell.
    DeleteLiveCell { cell_id: Id },
    /// Replace the notes of a MIDI Live Loops cell.
    SetLiveCellNotes { cell_id: Id, notes: Vec<Note> },
    /// Launch a cell (quantized to the live quantize grid). Starts playback if stopped.
    LaunchCell { cell_id: Id },
    /// Launch every cell of a scene.
    LaunchScene { scene: usize },
    /// Stop live cells on a track (or all tracks if omitted) and return to the timeline.
    StopLiveCells { track_id: Option<Id> },
    /// Set live launch quantization in beats (0 = immediate, 4 = one bar in 4/4).
    SetLiveQuantize { beats: f64 },
    /// Record what the Live Loops performance played so far into regular timeline regions.
    CommitLivePerformance {},

    // ------------------------------------------------------------------ surround & spatial audio
    /// Set the mix output format: stereo, binaural (headphone spatial audio, monitored live), 5.1, 7.1 or 7.1.4 (immersive with height speakers).
    SetSurroundFormat { format: SurroundFormat },
    /// Position a track in 3D space for surround/binaural mixing (azimuth degrees: 0 front, 90 right, -90 left, 180 rear). object=true makes it an audio object (object-based mixing, exported as an ADM object) instead of part of the bed. Animate positions with automation targets "surround:azimuth", "surround:elevation", "surround:spread".
    SetSurroundPan { track_id: Id, azimuth: f32, elevation: Option<f32>, spread: Option<f32>, object: Option<bool> },
    /// Bounce a multichannel surround WAV (channel order L R C LFE Ls Rs, plus Lrs Rrs for 7.1, plus Ltf Rtf Ltr Rtr for 7.1.4). Reverb and delay tails are kept (at least 2 s, at most 20 s after the end). The path must be a .wav file on a local drive (no network shares); .wav is added when the path has no extension. A render covers at most 60 minutes of the song (plus the tail): a longer range is refused.
    BounceSurround { path: String, format: Option<SurroundFormat> },
    /// Bounce a binaural (headphone spatial audio) stereo WAV from the 3D positions. Reverb and delay tails are kept (at least 2 s, at most 20 s after the end). The path must be a .wav file on a local drive (no network shares); .wav is added when the path has no extension. A render covers at most 60 minutes of the song (plus the tail): a longer range is refused.
    BounceBinaural { path: String },
    /// Export an ADM BWF master (ITU-R BS.2076 metadata in a BW64 file, 48 kHz / 24-bit), the interchange format used for Dolby Atmos deliveries: a 7.1.2 channel bed plus one audio object per object track, with time-varying positions from the surround automation. The path must be a .wav file on a local drive (no network shares); .wav is added when the path has no extension. A render covers at most 60 minutes of the song (plus the tail): a longer range is refused.
    ExportAdm { path: String, start: Option<f64>, end: Option<f64> },

    // ------------------------------------------------------------------ video
    /// Link a movie (mp4/mov/m4v) on a local drive for scoring to picture; URLs, network paths and paths starting with '-' are refused. Its audio is imported onto an audio track unless import_audio=false.
    LoadMovie { path: String, offset_sec: Option<f64>, import_audio: Option<bool> },
    /// Change the movie start offset (song seconds).
    SetMovieOffset { offset_sec: f64 },
    /// Unlink the movie.
    RemoveMovie {},
    /// Set the SMPTE frame rate for timecode (23.976, 24, 25, 29.97, 30, 50, 60).
    SetFrameRate { fps: f64 },
    /// Extract the movie frame at a song position as an image (path ending in .png, .jpg or .jpeg; requires ffmpeg on PATH), so an agent can look at the picture.
    ExtractMovieFrame { path: String, beat: Option<f64> },
    /// Export the movie with the bounced mix as soundtrack (path ending in .mp4, .mov, .mkv or .m4v; requires ffmpeg on PATH). A render covers at most 60 minutes of the song (plus the tail): a longer range is refused.
    ExportMovie { path: String },

    // ------------------------------------------------------------------ environment / MIDI transform
    /// MIDI Transform: select notes of a region by conditions and apply operations (transpose, velocity, length, timing, invert, reverse, delete, fixed pitch).
    TransformNotes { region_id: Id, select: Option<NoteFilter>, operations: NoteOps },
    /// Environment routing for live MIDI input: keyboard splits/layers with channel and note-range filters, transpose and velocity scaling, each to a track. Empty list = default (input goes to the armed/selected track).
    SetMidiRoutes { routes: Vec<MidiRoute> },
    /// Get the MIDI input routing.
    GetMidiRoutes {},

    // ------------------------------------------------------------------ third-party plug-ins (VST3)
    /// Scan the standard VST3 folders, the VST3 folders the user added in Ondina and extra_paths for third-party plug-ins. extra_paths must be folders on a local drive: network shares, device paths and URLs are skipped and listed in skipped_paths. From MCP clients and the assistant an extra folder must also be inside a standard VST3 folder or a folder the user added in Ondina's VST3 library; other folders are skipped and listed in skipped_paths too. scanned_folders lists the extra folders that were scanned.
    ScanVst3 { extra_paths: Option<Vec<String>> },
    /// Load a VST3 plug-in (path to a .vst3 bundle on a local drive) as instrument or insert effect on a track (track_id 0 = master). Network shares, device paths, URLs and files that are not .vst3 are refused. From MCP clients and the assistant the bundle must also be inside a standard VST3 folder (Common Files\VST3, %LOCALAPPDATA%\Programs\Common\VST3) or a folder the user added in Ondina's VST3 library (Add folder…): if it is refused, ask the user to add its folder. Its parameters are then addressable as "p<id>" (normalized 0..1) with set_plugin_param; they cannot be automated (set_automation accepts built-in plug-ins only).
    AddVst3 { track_id: Id, path: String, as_instrument: Option<bool>, index: Option<usize> },

    /// Open the native editor window of a VST3 plug-in. Knob moves in the window are applied to the project (undoable) and to the playing instance. Refused for a plug-in that open_project listed in blocked_plugins.
    OpenPluginEditor { plugin_id: Id },
    /// Close a plug-in editor window.
    ClosePluginEditor { plugin_id: Id },

    // ------------------------------------------------------------------ settings
    /// Get application settings (AI model, effort, MCP port, audio device).
    GetSettings {},
    /// Change the AI assistant model and/or effort (low, medium, high, xhigh, max).
    SetAiSettings { model: Option<String>, effort: Option<String> },
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
pub struct LiveMidiEvent {
    /// note_on, note_off or cc
    #[serde(rename = "type")]
    pub kind: String,
    /// Note number (note events) or controller number (cc).
    pub number: u8,
    /// Velocity (note_on) or controller value (cc).
    pub value: Option<u8>,
    /// Wait this many milliseconds before sending the event.
    pub delay_ms: Option<u64>,
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
pub struct CompSegment {
    pub region_id: Id,
    pub from_beat: f64,
    pub to_beat: f64,
}

/// One controller event of `replace_cc` (the controller is given once for all events).
#[derive(Debug, Clone, Copy, Serialize, Deserialize, JsonSchema, PartialEq)]
pub struct CcPoint {
    /// Position in beats relative to the region start (0 or more).
    pub beat: f64,
    /// Value 0..127.
    pub value: u8,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize, JsonSchema)]
pub struct NoteFilter {
    pub pitch_min: Option<u8>,
    pub pitch_max: Option<u8>,
    pub velocity_min: Option<u8>,
    pub velocity_max: Option<u8>,
    /// Start position range (beats relative to the region).
    pub start_min: Option<f64>,
    pub start_max: Option<f64>,
    pub length_min: Option<f64>,
    pub length_max: Option<f64>,
    /// Only notes outside the project key.
    pub out_of_key: Option<bool>,
    /// Only notes at these indices.
    pub indices: Option<Vec<usize>>,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize, JsonSchema)]
pub struct NoteOps {
    pub transpose: Option<i32>,
    pub fixed_pitch: Option<u8>,
    /// Mirror pitches around this note.
    pub invert_around: Option<u8>,
    pub velocity_fixed: Option<u8>,
    pub velocity_scale: Option<f32>,
    pub velocity_add: Option<i32>,
    pub length_fixed: Option<f64>,
    pub length_scale: Option<f64>,
    pub time_offset: Option<f64>,
    /// Scale positions relative to the region start (2.0 = half speed).
    pub time_scale: Option<f64>,
    /// Reverse the selected notes in time.
    pub reverse: Option<bool>,
    /// Quantize starts to this grid (beats).
    pub quantize: Option<f64>,
    pub delete: Option<bool>,
}

/// How set_note_lengths changes the notes.
#[derive(Debug, Clone, Copy, Serialize, Deserialize, JsonSchema, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum NoteLengthMode {
    Legato,
    NoOverlap,
    Fixed,
    Scale,
}

impl Command {
    /// Snake-case name (also the MCP tool name).
    pub fn name(&self) -> String {
        serde_json::to_value(self)
            .ok()
            .and_then(|v| v.get("command").and_then(|c| c.as_str()).map(|s| s.to_string()))
            .unwrap_or_default()
    }

    /// Whether the command modifies the project document (and therefore creates an undo step).
    pub fn is_mutating(&self) -> bool {
        use Command::*;
        !matches!(
            self,
            GetProject {}
                | GetProjectJson {}
                | SaveProject { .. }
                | Undo {}
                | Redo {}
                | Play { .. }
                | Stop {}
                | Record { .. }
                | SetPlayhead { .. }
                | GetTransport {}
                | GetSelection {}
                | GetActivity { .. }
                | Panic {}
                | GetTrack { .. }
                | ListPlugins { .. }
                | ListPresets { .. }
                | ListPatches {}
                | GetPlugin { .. }
                | GetRegion { .. }
                | Bounce { .. }
                | ExportMidi { .. }
                | AnalyzeMix { .. }
                | GetMeters {}
                | ListAudioDevices {}
                | SetAudioDevice { .. }
                | ListMidiInputs {}
                | ConnectMidiInput { .. }
                | PreviewNote { .. }
                | SendLiveMidi { .. }
                | DetectTempo { apply: None, .. }
                | DetectTempo { apply: Some(false), .. }
                | DetectKey { apply: None, .. }
                | DetectKey { apply: Some(false), .. }
                | ListTakes { .. }
                | GetScore { .. }
                | ExportMusicXml { .. }
                | GetLiveLoops {}
                | LaunchCell { .. }
                | LaunchScene { .. }
                | StopLiveCells { .. }
                | BounceSurround { .. }
                | BounceBinaural { .. }
                | ExportAdm { .. }
                | ExtractMovieFrame { .. }
                | ExportMovie { .. }
                | GetMidiRoutes {}
                | ScanVst3 { .. }
                | OpenPluginEditor { .. }
                | ClosePluginEditor { .. }
                | GetSettings {}
                | SetAiSettings { .. }
        )
    }

    /// Whether the command only reads: it changes nothing in the project, on disk, in the settings, on the audio
    /// and MIDI devices or in what is playing. MCP publishes this as `readOnlyHint`, which clients may use to run
    /// a tool without asking. Not the same as `!is_mutating()`: saving, bouncing, undo, transport and settings
    /// create no undo step but do change things. An explicit list, so a new command is never read-only by
    /// accident.
    pub fn is_read_only(&self) -> bool {
        use Command::*;
        matches!(
            self,
            GetProject {}
                | GetProjectJson {}
                | GetTransport {}
                | GetSelection {}
                | GetActivity { .. }
                | GetTrack { .. }
                | ListPlugins { .. }
                | ListPresets { .. }
                | ListPatches {}
                | GetPlugin { .. }
                | GetRegion { .. }
                | AnalyzeMix { .. }
                | GetMeters {}
                | ListAudioDevices {}
                | ListMidiInputs {}
                | DetectTempo { apply: None, .. }
                | DetectTempo { apply: Some(false), .. }
                | DetectKey { apply: None, .. }
                | DetectKey { apply: Some(false), .. }
                | ListTakes { .. }
                | GetScore { .. }
                | GetLiveLoops {}
                | GetMidiRoutes {}
                | GetSettings {}
        )
    }

    /// Build a command from an MCP tool name and its JSON arguments.
    pub fn from_tool_call(name: &str, args: &serde_json::Value) -> Result<Command, String> {
        let mut obj = match args {
            serde_json::Value::Object(m) => m.clone(),
            serde_json::Value::Null => serde_json::Map::new(),
            _ => return Err("arguments must be an object".into()),
        };
        obj.insert("command".into(), serde_json::Value::String(name.to_string()));
        serde_json::from_value(serde_json::Value::Object(obj)).map_err(|e| format!("invalid arguments for {name}: {e}"))
    }
}

/// A tool description generated from the [`Command`] schema.
#[derive(Debug, Clone, Serialize)]
pub struct ToolSpec {
    pub name: String,
    pub description: String,
    pub input_schema: serde_json::Value,
    /// The tool only reads (see `Command::is_read_only`); published as MCP `readOnlyHint`.
    pub read_only: bool,
}

/// Generate one tool spec per [`Command`] variant from the JSON schema, with `$ref`s inlined.
pub fn tool_specs() -> Vec<ToolSpec> {
    let root = serde_json::to_value(schemars::schema_for!(Command)).expect("schema");
    let defs = root.get("definitions").cloned().unwrap_or(serde_json::json!({}));
    let variants = root.get("oneOf").and_then(|v| v.as_array()).cloned().unwrap_or_default();
    let mut out = vec![];
    for v in variants {
        let mut v = inline_refs(&v, &defs, 0);
        let name = v
            .pointer("/properties/command/enum/0")
            .and_then(|x| x.as_str())
            .unwrap_or_default()
            .to_string();
        if name.is_empty() {
            continue;
        }
        let description = v.get("description").and_then(|d| d.as_str()).unwrap_or("").to_string();
        let obj = v.as_object_mut().unwrap();
        obj.remove("description");
        if let Some(props) = obj.get_mut("properties").and_then(|p| p.as_object_mut()) {
            props.remove("command");
        }
        if let Some(req) = obj.get_mut("required").and_then(|r| r.as_array_mut()) {
            req.retain(|x| x != "command");
        }
        obj.entry("properties").or_insert(serde_json::json!({}));
        obj.insert("type".into(), serde_json::json!("object"));
        let read_only = Command::from_tool_call(&name, &minimal_args(&v)).map(|c| c.is_read_only()).unwrap_or(false);
        out.push(ToolSpec { name, description, input_schema: v, read_only });
    }
    out
}

/// Fill required fields with placeholder values, just to classify the command.
fn minimal_args(schema: &serde_json::Value) -> serde_json::Value {
    let mut m = serde_json::Map::new();
    if let (Some(req), Some(props)) = (schema.get("required").and_then(|r| r.as_array()), schema.get("properties")) {
        for r in req {
            let key = r.as_str().unwrap_or_default();
            let p = props.get(key).cloned().unwrap_or_default();
            m.insert(key.to_string(), placeholder(&p));
        }
    }
    serde_json::Value::Object(m)
}

fn placeholder(p: &serde_json::Value) -> serde_json::Value {
    if let Some(e) = p.get("enum").and_then(|e| e.as_array()).and_then(|a| a.first()) {
        return e.clone();
    }
    if let Some(one) = p.get("oneOf").or(p.get("anyOf")).and_then(|a| a.as_array()).and_then(|a| a.first()) {
        return placeholder(one);
    }
    let t = match p.get("type") {
        Some(serde_json::Value::String(s)) => s.clone(),
        Some(serde_json::Value::Array(a)) => a.first().and_then(|x| x.as_str()).unwrap_or("null").to_string(),
        _ => "null".into(),
    };
    match t.as_str() {
        "integer" | "number" => serde_json::json!(0),
        "string" => serde_json::json!(""),
        "boolean" => serde_json::json!(false),
        "array" => serde_json::json!([]),
        "object" => serde_json::json!({}),
        _ => serde_json::Value::Null,
    }
}

fn inline_refs(v: &serde_json::Value, defs: &serde_json::Value, depth: usize) -> serde_json::Value {
    use serde_json::Value;
    if depth > 20 {
        return v.clone();
    }
    match v {
        Value::Object(m) => {
            if let Some(Value::String(r)) = m.get("$ref") {
                let key = r.trim_start_matches("#/definitions/");
                if let Some(d) = defs.get(key) {
                    let mut resolved = inline_refs(d, defs, depth + 1);
                    // keep sibling keys such as description
                    if let Value::Object(rm) = &mut resolved {
                        for (k, val) in m {
                            if k != "$ref" {
                                rm.insert(k.clone(), inline_refs(val, defs, depth + 1));
                            }
                        }
                    }
                    return resolved;
                }
            }
            let mut out = serde_json::Map::new();
            for (k, val) in m {
                if k == "definitions" || k == "$schema" || k == "title" {
                    continue;
                }
                out.insert(k.clone(), inline_refs(val, defs, depth + 1));
            }
            // schemars renders Option<T> of a $ref as allOf:[{$ref}] — flatten single allOf
            if let Some(Value::Array(all)) = out.get("allOf").cloned() {
                if all.len() == 1 {
                    out.remove("allOf");
                    if let Value::Object(inner) = &all[0] {
                        for (k, val) in inner {
                            out.entry(k.clone()).or_insert(val.clone());
                        }
                    }
                }
            }
            Value::Object(out)
        }
        Value::Array(a) => Value::Array(a.iter().map(|x| inline_refs(x, defs, depth + 1)).collect()),
        _ => v.clone(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn specs_generated() {
        let specs = tool_specs();
        assert!(specs.len() > 80, "got {}", specs.len());
        let add = specs.iter().find(|s| s.name == "add_track").unwrap();
        assert!(add.description.contains("Add a track"));
        assert!(add.input_schema["properties"]["kind"].is_object());
        let s = serde_json::to_string(&specs).unwrap();
        assert!(!s.contains("$ref"), "refs must be inlined");
        let gp = specs.iter().find(|s| s.name == "get_project").unwrap();
        assert!(gp.read_only);
        assert!(!add.read_only);
        let c = Command::from_tool_call("set_volume", &serde_json::json!({"track_id": 1, "db": -3.0})).unwrap();
        assert_eq!(c.name(), "set_volume");
    }

    /// MCP clients may run read-only tools without asking (readOnlyHint). Only the tools that just read are
    /// marked: anything that changes the project, files on disk, settings, devices or what the user hears is
    /// not. The list is explicit so that a new tool is not read-only by accident.
    #[test]
    fn only_tools_that_just_read_are_marked_read_only() {
        const READ_ONLY: &[&str] = &[
            "analyze_mix",
            "detect_key",
            "detect_tempo",
            "get_activity",
            "get_live_loops",
            "get_meters",
            "get_midi_routes",
            "get_plugin",
            "get_project",
            "get_project_json",
            "get_region",
            "get_score",
            "get_selection",
            "get_settings",
            "get_track",
            "get_transport",
            "list_audio_devices",
            "list_midi_inputs",
            "list_patches",
            "list_plugins",
            "list_presets",
            "list_takes",
        ];
        let specs = tool_specs();
        let mut marked: Vec<&str> = specs.iter().filter(|s| s.read_only).map(|s| s.name.as_str()).collect();
        marked.sort_unstable();
        assert_eq!(marked, READ_ONLY);
        for name in ["save_project", "bounce", "export_midi", "export_music_xml", "bounce_surround", "bounce_binaural", "export_adm", "export_movie", "extract_movie_frame", "undo", "redo", "scan_vst3", "set_ai_settings", "set_audio_device", "record", "stop", "play"] {
            assert!(specs.iter().any(|s| s.name == name && !s.read_only), "{name} must not be read-only");
        }
        // a read-only tool never creates an undo step either
        for s in specs.iter().filter(|s| s.read_only) {
            let c = Command::from_tool_call(&s.name, &minimal_args(&s.input_schema)).unwrap();
            assert!(!c.is_mutating(), "{}", s.name);
        }
    }

    #[test]
    fn detect_key_is_read_only_unless_applied() {
        assert!(!Command::DetectKey { region_id: None, apply: None }.is_mutating());
        assert!(!Command::DetectKey { region_id: Some(1), apply: Some(false) }.is_mutating());
        assert!(Command::DetectKey { region_id: None, apply: Some(true) }.is_mutating());
        assert!(tool_specs().iter().any(|s| s.name == "detect_key" && s.read_only));
        let c = Command::from_tool_call("detect_key", &serde_json::json!({})).unwrap();
        assert_eq!(c.name(), "detect_key");
    }
}
