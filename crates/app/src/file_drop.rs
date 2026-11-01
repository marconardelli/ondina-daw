//! Files dragged in from Explorer: what each one is, where it lands in the arrangement and how the
//! files of one drop become one undo step. Pure functions, tested without a window.

use std::path::{Path, PathBuf};

use eframe::egui::{self, pos2, vec2, Align2, Color32, FontId, Pos2, Rect, RichText, Rounding, Stroke};
use ondina_core::command::Command;
use ondina_core::model::{Id, Project, TrackKind};
use ondina_studio::Session;

use crate::doc_guard::DocAction;
use crate::theme;

/// Audio formats the decoder reads (the same list as File > Import Audio).
pub const AUDIO_EXTENSIONS: &[&str] = &["wav", "aif", "aiff", "flac", "mp3", "ogg", "m4a", "caf"];
/// Files decoded in one drop. Decoding runs on the GUI thread with the session locked, like File > Import Audio,
/// so a drop of many long files would freeze the window; the rest are counted and reported.
pub const MAX_DROP_FILES: usize = 8;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum FileKind {
    Audio,
    Midi,
    Project,
    Other,
}

/// The arrangement as a drop sees it (screen points).
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct DropGeom {
    /// The whole arrangement area (ruler, lanes, headers and tracks).
    pub rect: Rect,
    pub timeline_x: f32,
    pub tracks_top: f32,
    pub track_h: f32,
    pub scroll_y: f32,
    pub scroll_beat: f64,
    pub px_per_beat: f32,
}

/// Where the files under the pointer would land.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct DropSpot {
    /// Audio track that receives the audio; None makes a new audio track (added below the last one).
    pub track: Option<Id>,
    /// Row the audio lands on: the track's row, or `tracks.len()` for the new track.
    pub row: usize,
    /// Start beat of the first audio file and of the MIDI import.
    pub beat: f64,
    /// The beat comes from the pointer's x (false: from the playhead).
    pub on_timeline: bool,
}

/// Files to import: audio one after the other from `start`, MIDI at `midi_beat`.
#[derive(Debug, Clone, PartialEq)]
pub struct ImportPlan {
    pub audio: Vec<String>,
    pub track: Option<Id>,
    pub start: f64,
    pub midi: Vec<String>,
    pub midi_beat: f64,
    /// File names that are not audio, MIDI or a project.
    pub skipped: Vec<String>,
    /// Files left out by `MAX_DROP_FILES`.
    pub over_limit: usize,
}

#[derive(Debug, Clone, PartialEq)]
pub enum DropPlan {
    /// Open this project through the unsaved-changes prompt.
    Open(String),
    Import(ImportPlan),
    /// Nothing to do; the status bar shows the reason.
    Refuse(String),
}

/// What one drop did, for the status bar and the selection.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct ImportReport {
    pub audio_regions: usize,
    pub midi_files: usize,
    /// Track to select afterwards: the audio track, or the first track of the first MIDI import.
    pub track: Option<Id>,
    /// Last audio region created.
    pub last_region: Option<Id>,
    /// "file name: error" for each file that failed.
    pub errors: Vec<String>,
}

pub fn file_kind(path: &Path) -> FileKind {
    if path.is_dir() {
        return FileKind::Other;
    }
    let ext = path.extension().map(|e| e.to_string_lossy().to_ascii_lowercase()).unwrap_or_default();
    if AUDIO_EXTENSIONS.contains(&ext.as_str()) {
        FileKind::Audio
    } else if ext == "mid" || ext == "midi" {
        FileKind::Midi
    } else if ondina_core::is_project_file(&path.to_string_lossy()) {
        FileKind::Project
    } else {
        FileKind::Other
    }
}

/// Nearest grid line, like the drags in the arrangement (`OndinaApp::snap_beat`).
fn snap_round(b: f64, snap: f64) -> f64 {
    if snap <= 0.0 {
        b
    } else {
        (b / snap).round() * snap
    }
}

/// Grid line at or before `b`, like File > Import Audio at the playhead (`OndinaApp::snap_floor`).
fn snap_floor(b: f64, snap: f64) -> f64 {
    if snap <= 0.0 {
        b
    } else {
        (b / snap).floor() * snap
    }
}

pub fn drop_spot(pointer: Option<Pos2>, g: &DropGeom, tracks: &[(Id, TrackKind)], snap: f64, playhead: f64) -> DropSpot {
    let at_playhead = snap_floor(playhead, snap).max(0.0);
    let new_track = tracks.len();
    let Some(p) = pointer.filter(|p| g.rect.contains(*p)) else {
        return DropSpot { track: None, row: new_track, beat: at_playhead, on_timeline: false };
    };
    let (beat, on_timeline) = if p.x >= g.timeline_x && g.px_per_beat > 0.0 {
        (snap_round(g.scroll_beat + ((p.x - g.timeline_x) / g.px_per_beat) as f64, snap).max(0.0), true)
    } else {
        (at_playhead, false)
    };
    let mut spot = DropSpot { track: None, row: new_track, beat, on_timeline };
    if p.y >= g.tracks_top && g.track_h > 0.0 {
        let r = ((p.y - g.tracks_top + g.scroll_y) / g.track_h).floor() as usize;
        if let Some(&(id, TrackKind::Audio)) = tracks.get(r) {
            spot.track = Some(id);
            spot.row = r;
        }
    }
    spot
}

fn file_name(path: &str) -> String {
    Path::new(path).file_name().map(|n| n.to_string_lossy().into_owned()).unwrap_or_else(|| path.to_string())
}

/// The first name, plus " and N more" when there are others.
fn names_summary(names: &[String]) -> String {
    match names {
        [] => String::new(),
        [one] => one.clone(),
        [first, rest @ ..] => format!("{first} and {} more", rest.len()),
    }
}

pub fn plan_drop(paths: &[PathBuf], spot: &DropSpot) -> DropPlan {
    let mut audio = vec![];
    let mut midi = vec![];
    let mut projects = vec![];
    let mut skipped = vec![];
    for p in paths {
        let s = p.display().to_string();
        match file_kind(p) {
            FileKind::Audio => audio.push(s),
            FileKind::Midi => midi.push(s),
            FileKind::Project => projects.push(s),
            FileKind::Other => skipped.push(file_name(&s)),
        }
    }
    if let Some(project) = projects.into_iter().next() {
        return if paths.len() == 1 {
            DropPlan::Open(project)
        } else {
            DropPlan::Refuse("Drop a project on its own to open it: nothing was imported".into())
        };
    }
    if audio.is_empty() && midi.is_empty() {
        if skipped.is_empty() {
            return DropPlan::Refuse("Nothing to import".into());
        }
        return DropPlan::Refuse(format!(
            "Can't import {}: drop audio (wav, aiff, flac, mp3, ogg, m4a, caf), MIDI (.mid) or an Ondina project",
            names_summary(&skipped)
        ));
    }
    let by_name = |a: &String, b: &String| file_name(a).to_lowercase().cmp(&file_name(b).to_lowercase());
    audio.sort_by(by_name);
    midi.sort_by(by_name);
    let total = audio.len() + midi.len();
    audio.truncate(MAX_DROP_FILES);
    midi.truncate(MAX_DROP_FILES - audio.len());
    DropPlan::Import(ImportPlan {
        over_limit: total - audio.len() - midi.len(),
        audio,
        track: spot.track,
        start: spot.beat,
        midi,
        midi_beat: spot.beat,
        skipped,
    })
}

impl ImportPlan {
    /// `after` is the track and end beat of the last audio file of this drop that was imported, if any.
    pub fn audio_command(&self, path: &str, after: Option<(Id, f64)>) -> Command {
        let (track_id, start) = match after {
            Some((t, end)) => (Some(t), end),
            None => (self.track, self.start),
        };
        Command::ImportAudio { path: path.to_string(), track_id, start: Some(start) }
    }

    pub fn midi_command(&self, path: &str) -> Command {
        Command::ImportMidi { path: path.to_string(), at_beat: Some(self.midi_beat) }
    }
}

pub fn next_drop_id() -> u64 {
    use std::sync::atomic::{AtomicU64, Ordering};
    static NEXT: AtomicU64 = AtomicU64::new(1);
    NEXT.fetch_add(1, Ordering::Relaxed)
}

/// Runs the commands of one drop so that everything it imports is one undo step.
pub fn run_import(s: &mut Session, plan: &ImportPlan, drop_id: u64) -> ImportReport {
    let mut r = ImportReport::default();
    // Until a command of the drop succeeds every attempt gets a fresh key: `execute_coalesced` remembers the key
    // of a failed command too, so a fixed key would merge the next file into the step before the drop (the
    // user's last edit). From the first success on the key stays the same and the rest joins that step.
    let mut group: Option<String> = None;
    let mut attempt = 0u32;
    let mut key = |group: &Option<String>| match group {
        Some(k) => k.clone(),
        None => {
            attempt += 1;
            format!("file-drop-{drop_id}-{attempt}")
        }
    };
    let mut after: Option<(Id, f64)> = None;
    for path in &plan.audio {
        let k = key(&group);
        let start = after.map(|(_, end)| end).unwrap_or(plan.start);
        match s.execute_coalesced(plan.audio_command(path, after), &k) {
            Ok(v) => {
                group.get_or_insert(k);
                if let Some(t) = v["track_id"].as_u64() {
                    after = Some((t, start + v["length_beats"].as_f64().unwrap_or(0.0)));
                    r.track = Some(t);
                }
                r.audio_regions += 1;
                r.last_region = v["region_id"].as_u64();
            }
            // `after` stays: the next file goes where the failed one would have gone.
            Err(e) => r.errors.push(format!("{}: {e}", file_name(path))),
        }
    }
    for path in &plan.midi {
        let k = key(&group);
        match s.execute_coalesced(plan.midi_command(path), &k) {
            Ok(v) => {
                group.get_or_insert(k);
                r.midi_files += 1;
                if let Some(t) = v["track_ids"].get(0).and_then(|t| t.as_u64()) {
                    r.track.get_or_insert(t);
                }
            }
            Err(e) => r.errors.push(format!("{}: {e}", file_name(path))),
        }
    }
    r
}

fn count(n: usize, one: &str, many: &str) -> String {
    format!("{n} {}", if n == 1 { one } else { many })
}

pub fn report_status(plan: &ImportPlan, r: &ImportReport) -> String {
    let mut imported = vec![];
    if r.audio_regions > 0 {
        imported.push(count(r.audio_regions, "audio file", "audio files"));
    }
    if r.midi_files > 0 {
        imported.push(count(r.midi_files, "MIDI file", "MIDI files"));
    }
    let mut notes = vec![];
    if !plan.skipped.is_empty() {
        notes.push(format!("skipped {} (not audio, MIDI or a project)", names_summary(&plan.skipped)));
    }
    if plan.over_limit > 0 {
        notes.push(format!(
            "{} not imported (at most {MAX_DROP_FILES} per drop)",
            count(plan.over_limit, "more file", "more files")
        ));
    }
    let error = r.errors.first().map(|e| match r.errors.len() {
        1 => format!("⚠ {e}"),
        n => format!("⚠ {e} (+{} more)", n - 1),
    });
    let mut parts = vec![];
    if imported.is_empty() {
        parts.extend(error);
        parts.extend(notes);
    } else {
        imported[0] = format!("Imported {}", imported[0]);
        parts.extend(imported);
        parts.extend(notes);
        parts.extend(error);
    }
    if parts.is_empty() {
        "Nothing imported".into()
    } else {
        parts.join(" · ")
    }
}

/// The text next to the pointer while files are dragged over the window.
pub fn hover_label(plan: &DropPlan, track_name: Option<&str>, at: &str) -> String {
    match plan {
        DropPlan::Open(path) => format!("{} → open project", file_name(path)),
        DropPlan::Refuse(m) => m.clone(),
        DropPlan::Import(p) => {
            let mut s = p.audio.first().or(p.midi.first()).map(|f| file_name(f)).unwrap_or_default();
            let others = (p.audio.len() + p.midi.len()).saturating_sub(1);
            if others > 0 {
                s += &format!(" + {others} more");
            }
            if p.audio.is_empty() {
                s += &format!(" → MIDI import at {at}");
            } else {
                s += &format!(" → {} at {at}", track_name.unwrap_or("new audio track"));
                if !p.midi.is_empty() {
                    s += &format!(" · MIDI at {at}");
                }
            }
            if p.over_limit > 0 {
                s += &format!(" (first {MAX_DROP_FILES} only)");
            }
            if !p.skipped.is_empty() {
                s += &format!(" · {} skipped", p.skipped.len());
            }
            s
        }
    }
}

/// Physical screen pixels (what Windows reports for the cursor) to points inside the window.
pub fn screen_px_to_points(x: i32, y: i32, pixels_per_point: f32, inner_min: Pos2) -> Pos2 {
    Pos2::new(x as f32 / pixels_per_point - inner_min.x, y as f32 / pixels_per_point - inner_min.y)
}

/// The pointer while files are dragged over the window. During an OLE drag Windows sends the window no mouse
/// moves and winit ignores the point it gets with the drag events, so egui's pointer stays where it was before
/// the drag: ask Windows for the cursor instead (physical pixels, the process is per-monitor DPI aware).
pub fn drag_pointer(ctx: &egui::Context) -> Option<Pos2> {
    #[cfg(windows)]
    {
        use windows_sys::Win32::Foundation::POINT;
        use windows_sys::Win32::UI::WindowsAndMessaging::GetCursorPos;
        let mut pt = POINT { x: 0, y: 0 };
        // SAFETY: the only pointer passed is to a local POINT that outlives the call.
        let ok = unsafe { GetCursorPos(&mut pt) } != 0;
        let (ppp, inner) = ctx.input(|i| (i.pixels_per_point, i.viewport().inner_rect));
        if let (true, Some(inner)) = (ok, inner) {
            return Some(screen_px_to_points(pt.x, pt.y, ppp, inner.min));
        }
    }
    ctx.input(|i| i.pointer.latest_pos())
}

impl crate::app::OndinaApp {
    /// Feedback while files are dragged over the window, and the import when they are dropped.
    pub(crate) fn file_drop_ui(&mut self, ui: &mut egui::Ui, g: DropGeom, p: &Project) {
        let (hovered, dropped) = ui.input(|i| (i.raw.hovered_files.clone(), i.raw.dropped_files.clone()));
        if hovered.is_empty() && dropped.is_empty() {
            return;
        }
        let tracks: Vec<(Id, TrackKind)> = p.tracks.iter().map(|t| (t.id, t.kind)).collect();
        let pointer = drag_pointer(ui.ctx());
        let spot = drop_spot(pointer, &g, &tracks, self.snap, self.position());
        if !dropped.is_empty() {
            let paths: Vec<PathBuf> = dropped.iter().filter_map(|f| f.path.clone()).collect();
            let ctx = ui.ctx().clone();
            self.drop_files(&ctx, &paths, &spot);
            return;
        }
        if self.doc_prompt_open() {
            return;
        }
        let paths: Vec<PathBuf> = hovered.iter().filter_map(|f| f.path.clone()).collect();
        let plan = plan_drop(&paths, &spot);
        draw_drop_hint(ui, &g, p, &spot, &plan, pointer);
        // No events arrive while dragging: keep drawing so the line follows the cursor.
        ui.ctx().request_repaint();
    }

    fn drop_files(&mut self, ctx: &egui::Context, paths: &[PathBuf], spot: &DropSpot) {
        if self.doc_prompt_open() {
            self.status = "Answer the Save question first: the dropped files were not used".into();
            return;
        }
        match plan_drop(paths, spot) {
            // The single entry point asks to save unsaved work and stops a recording first.
            DropPlan::Open(path) => self.request_doc_action(ctx, DocAction::Open(Some(path))),
            DropPlan::Refuse(m) => self.status = m,
            DropPlan::Import(plan) => {
                let id = next_drop_id();
                if let Some(r) = self.exec_with(|s| run_import(s, &plan, id)) {
                    if r.track.is_some() {
                        self.selected_track = r.track;
                        self.selected_region = r.last_region;
                    }
                    self.status = report_status(&plan, &r);
                }
            }
        }
    }
}

/// Where the files would land: the target row, the line at the drop beat and a label near the pointer. Paints
/// only, so the arrangement's clicks are not affected.
fn draw_drop_hint(ui: &egui::Ui, g: &DropGeom, p: &Project, spot: &DropSpot, plan: &DropPlan, pointer: Option<Pos2>) {
    let tracks_area = Rect::from_min_max(pos2(g.rect.left(), g.tracks_top), g.rect.max);
    let painter = ui.painter_at(tracks_area);
    let line_x = g.timeline_x + ((spot.beat - g.scroll_beat) as f32) * g.px_per_beat;
    let line = |y0: f32, y1: f32| {
        if spot.on_timeline && line_x >= g.timeline_x {
            painter.line_segment([pos2(line_x, y0), pos2(line_x, y1)], Stroke::new(2.0_f32, theme::ACCENT));
        }
    };
    match plan {
        DropPlan::Import(ip) if !ip.audio.is_empty() => {
            let y = g.tracks_top + spot.row as f32 * g.track_h - g.scroll_y;
            let row = Rect::from_min_max(pos2(g.rect.left(), y), pos2(g.rect.right(), y + g.track_h));
            painter.rect_filled(row, Rounding::ZERO, theme::ACCENT.gamma_multiply(0.18));
            if spot.track.is_none() {
                painter.rect_stroke(row, Rounding::ZERO, Stroke::new(1.0_f32, theme::ACCENT));
                painter.text(
                    pos2(g.rect.left() + 10.0, y + g.track_h / 2.0),
                    Align2::LEFT_CENTER,
                    "New audio track",
                    FontId::proportional(11.0),
                    theme::TEXT_DIM,
                );
            }
            line(y, y + g.track_h);
        }
        DropPlan::Import(_) => line(g.tracks_top, g.rect.bottom()),
        DropPlan::Open(_) => {
            ui.painter_at(g.rect).rect_stroke(g.rect.shrink(1.0), Rounding::ZERO, Stroke::new(2.0_f32, theme::ACCENT));
        }
        DropPlan::Refuse(_) => {}
    }
    let track_name = spot.track.and_then(|id| p.track(id)).map(|t| t.name.as_str());
    let text = hover_label(plan, track_name, &p.bar_beat_string(spot.beat));
    let color = if matches!(plan, DropPlan::Refuse(_)) { theme::REC } else { Color32::WHITE };
    let at = match pointer.filter(|p| ui.ctx().screen_rect().contains(*p)) {
        Some(p) => p + vec2(14.0, 14.0),
        None => pos2(g.rect.center().x - 120.0, g.rect.top() + 8.0),
    };
    egui::Area::new(egui::Id::new("file_drop_hint"))
        .order(egui::Order::Tooltip)
        .interactable(false)
        .fixed_pos(at)
        .show(ui.ctx(), |ui| {
            egui::Frame::popup(ui.style()).fill(theme::PANEL).show(ui, |ui| {
                ui.label(RichText::new(text).color(color));
            });
        });
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::doc_guard::{self, DocAction, Step};
    use eframe::egui::pos2;
    use ondina_studio::Source;
    use serde_json::{json, Value};

    fn geom() -> DropGeom {
        DropGeom {
            rect: Rect::from_min_max(pos2(0.0, 40.0), pos2(1200.0, 700.0)),
            timeline_x: 210.0,
            tracks_top: 100.0,
            track_h: 58.0,
            scroll_y: 0.0,
            scroll_beat: 0.0,
            px_per_beat: 20.0,
        }
    }

    fn tracks() -> Vec<(Id, TrackKind)> {
        vec![(1, TrackKind::Instrument), (2, TrackKind::Instrument), (3, TrackKind::Audio)]
    }

    fn x_of(beat: f64) -> f32 {
        210.0 + beat as f32 * 20.0
    }

    fn y_of(row: usize) -> f32 {
        100.0 + row as f32 * 58.0 + 29.0
    }

    /// 2 s of audio: 4 beats at the 120 bpm of a new project.
    fn wav(name: &str) -> PathBuf {
        let dir = std::env::temp_dir().join("ondina_app_tests");
        std::fs::create_dir_all(&dir).unwrap();
        let wav = dir.join(name);
        ondina_engine::write_wav(&wav, &vec![0.1; 88200], &vec![0.1; 88200], 44100, 16).unwrap();
        wav
    }

    fn run(s: &mut Session, name: &str, args: Value) -> Value {
        let c = Command::from_tool_call(name, &args).unwrap();
        s.execute(c, Source::Gui).unwrap_or_else(|e| panic!("{name} {args} failed: {e}"))
    }

    fn paths(names: &[&str]) -> Vec<PathBuf> {
        names.iter().map(PathBuf::from).collect()
    }

    fn spot_at(beat: f64, track: Option<Id>) -> DropSpot {
        DropSpot { track, row: 3, beat, on_timeline: true }
    }

    fn import(plan: DropPlan) -> ImportPlan {
        match plan {
            DropPlan::Import(p) => p,
            other => panic!("expected an import, got {other:?}"),
        }
    }

    fn audio_target(c: &Command) -> (Option<Id>, Option<f64>) {
        match c {
            Command::ImportAudio { track_id, start, .. } => (*track_id, *start),
            _ => panic!("expected ImportAudio"),
        }
    }

    fn plan_of(audio: Vec<String>, midi: Vec<String>, track: Option<Id>, start: f64) -> ImportPlan {
        ImportPlan { audio, track, start, midi, midi_beat: start, skipped: vec![], over_limit: 0 }
    }

    fn audio_regions(s: &Session) -> Vec<(Id, Id, f64)> {
        let mut out = vec![];
        for t in &s.project.tracks {
            for r in &t.regions {
                out.push((t.id, r.id, r.start));
            }
        }
        out
    }

    #[test]
    fn file_kind_by_extension() {
        for f in ["A.WAV", "b.flac", "c.mp3", "d.ogg", "e.aif", "f.aiff", "g.m4a", "h.caf"] {
            assert_eq!(file_kind(Path::new(f)), FileKind::Audio, "{f}");
        }
        for f in ["x.mid", "y.MIDI"] {
            assert_eq!(file_kind(Path::new(f)), FileKind::Midi, "{f}");
        }
        for f in ["s.ondina", "old.AURORA"] {
            assert_eq!(file_kind(Path::new(f)), FileKind::Project, "{f}");
        }
        for f in ["notes.txt", "C:/loops"] {
            assert_eq!(file_kind(Path::new(f)), FileKind::Other, "{f}");
        }
    }

    #[test]
    fn drop_on_an_audio_lane_imports_there_at_the_snapped_beat() {
        let spot = drop_spot(Some(pos2(x_of(8.3), y_of(2))), &geom(), &tracks(), 1.0, 0.0);
        assert_eq!(spot, DropSpot { track: Some(3), row: 2, beat: 8.0, on_timeline: true });
        let plan = import(plan_drop(&paths(&["C:/s/kick.wav"]), &spot));
        match plan.audio_command(&plan.audio[0], None) {
            Command::ImportAudio { path, track_id, start } => {
                assert_eq!(path, PathBuf::from("C:/s/kick.wav").display().to_string());
                assert_eq!(track_id, Some(3));
                assert_eq!(start, Some(8.0));
            }
            _ => panic!("expected ImportAudio"),
        }
        let spot = drop_spot(Some(pos2(x_of(8.6), y_of(2))), &geom(), &tracks(), 1.0, 0.0);
        assert_eq!(spot.beat, 9.0);
        let spot = drop_spot(Some(pos2(x_of(8.3), y_of(2))), &geom(), &tracks(), 0.0, 0.0);
        assert!((spot.beat - 8.3).abs() < 1e-6, "{}", spot.beat);
        assert_eq!(spot.track, Some(3));
    }

    #[test]
    fn drop_below_the_tracks_or_on_an_instrument_creates_a_new_track() {
        for row in [3, 0] {
            let spot = drop_spot(Some(pos2(x_of(8.3), y_of(row))), &geom(), &tracks(), 1.0, 0.0);
            assert_eq!(spot, DropSpot { track: None, row: 3, beat: 8.0, on_timeline: true }, "row {row}");
            let plan = import(plan_drop(&paths(&["C:/s/kick.wav"]), &spot));
            assert_eq!(audio_target(&plan.audio_command(&plan.audio[0], None)), (None, Some(8.0)));
        }
        let g = DropGeom { scroll_y: 58.0, ..geom() };
        let spot = drop_spot(Some(pos2(x_of(8.3), y_of(2))), &g, &tracks(), 1.0, 0.0);
        assert_eq!((spot.track, spot.row), (None, 3));
    }

    #[test]
    fn drop_outside_the_timeline_uses_the_playhead() {
        let g = geom();
        for (p, track) in [(None, None), (Some(pos2(x_of(4.0), 750.0)), None), (Some(pos2(100.0, y_of(2))), Some(3))] {
            let spot = drop_spot(p, &g, &tracks(), 1.0, 6.7);
            assert_eq!(spot.beat, 6.0, "{p:?}");
            assert!(!spot.on_timeline, "{p:?}");
            assert_eq!(spot.track, track, "{p:?}");
        }
        let spot = drop_spot(Some(pos2(x_of(4.0), 60.0)), &g, &tracks(), 1.0, 6.7);
        assert_eq!(spot, DropSpot { track: None, row: 3, beat: 4.0, on_timeline: true });
    }

    #[test]
    fn midi_project_and_unknown_files() {
        let spot = spot_at(8.0, None);
        let plan = import(plan_drop(&paths(&["C:/m/song.mid"]), &spot));
        assert_eq!(plan.midi.len(), 1);
        assert!(plan.audio.is_empty());
        match plan.midi_command(&plan.midi[0]) {
            Command::ImportMidi { path, at_beat } => {
                assert!(path.ends_with("song.mid"), "{path}");
                assert_eq!(at_beat, Some(8.0));
            }
            _ => panic!("expected ImportMidi"),
        }
        match plan_drop(&paths(&["C:/p/song.ondina"]), &spot) {
            DropPlan::Open(p) => assert!(p.ends_with("song.ondina"), "{p}"),
            other => panic!("expected Open, got {other:?}"),
        }
        match plan_drop(&paths(&["C:/d/notes.txt"]), &spot) {
            DropPlan::Refuse(m) => assert!(m.contains("notes.txt"), "{m}"),
            other => panic!("expected Refuse, got {other:?}"),
        }
        let plan = import(plan_drop(&paths(&["C:/d/notes.txt", "C:/d/a.wav"]), &spot));
        assert_eq!(plan.skipped, vec!["notes.txt".to_string()]);
        assert_eq!(plan.audio.len(), 1);
    }

    #[test]
    fn a_project_must_be_dropped_on_its_own() {
        let spot = spot_at(0.0, None);
        for files in [["C:/p/a.ondina", "C:/p/b.wav"], ["C:/p/a.ondina", "C:/p/b.aurora"]] {
            assert!(matches!(plan_drop(&paths(&files), &spot), DropPlan::Refuse(_)), "{files:?}");
        }
    }

    #[test]
    fn dropped_project_asks_before_replacing_unsaved_work() {
        let DropPlan::Open(path) = plan_drop(&paths(&["C:/p/song.ondina"]), &spot_at(0.0, None)) else {
            panic!("expected Open");
        };
        let action = DocAction::Open(Some(path));
        assert_eq!(doc_guard::on_request(action.clone(), true), Step::Ask(action.clone()));
        assert_eq!(doc_guard::on_request(action.clone(), false), Step::Proceed(action));
    }

    #[test]
    fn files_are_sorted_by_name_and_capped() {
        let spot = spot_at(0.0, None);
        let plan = import(plan_drop(&paths(&["C:/x/b.wav", "C:/x/A.wav", "C:/x/c.mid"]), &spot));
        let names: Vec<&str> = plan.audio.iter().map(|p| Path::new(p).file_name().unwrap().to_str().unwrap()).collect();
        assert_eq!(names, ["A.wav", "b.wav"]);
        assert_eq!(plan.midi.len(), 1);
        assert_eq!(plan.over_limit, 0);
        let mut many: Vec<String> = (0..10).map(|i| format!("C:/x/{i:02}.wav")).collect();
        many.push("C:/x/a.mid".into());
        many.push("C:/x/b.mid".into());
        let many: Vec<PathBuf> = many.iter().map(PathBuf::from).collect();
        let plan = import(plan_drop(&many, &spot));
        assert_eq!((plan.audio.len(), plan.midi.len(), plan.over_limit), (8, 0, 4));
    }

    #[test]
    fn audio_command_follows_the_previous_file() {
        let plan = plan_of(vec!["a.wav".into()], vec![], None, 4.0);
        assert_eq!(audio_target(&plan.audio_command("b.wav", Some((7, 12.0)))), (Some(7), Some(12.0)));
        assert_eq!(audio_target(&plan.audio_command("a.wav", None)), (None, Some(4.0)));
    }

    #[test]
    fn two_dropped_files_are_one_undo_step() {
        let mut s = Session::new(false);
        let files = vec![wav("drop_two_a.wav").display().to_string(), wav("drop_two_b.wav").display().to_string()];
        let plan = plan_of(files, vec![], None, 4.0);
        let r = run_import(&mut s, &plan, next_drop_id());
        assert_eq!(r.audio_regions, 2);
        assert!(r.errors.is_empty(), "{:?}", r.errors);
        assert_eq!(s.project.tracks.len(), 1);
        let regions = audio_regions(&s);
        assert_eq!(regions.len(), 2);
        assert!((regions[0].2 - 4.0).abs() < 1e-6, "{regions:?}");
        assert!((regions[1].2 - 8.0).abs() < 1e-6, "{regions:?}");
        assert_eq!(r.track, Some(s.project.tracks[0].id));
        assert_eq!(r.last_region, Some(regions[1].1));
        run(&mut s, "undo", json!({}));
        assert!(s.project.tracks.is_empty());
        assert_eq!(s.can_undo(), None);
    }

    #[test]
    fn a_failed_first_file_never_merges_into_the_previous_step() {
        let mut s = Session::new(false);
        run(&mut s, "add_track", json!({"kind": "instrument", "name": "Keys"}));
        let files = vec!["C:/missing/none.wav".to_string(), wav("drop_fail_b.wav").display().to_string()];
        let plan = plan_of(files, vec![], None, 2.0);
        let r = run_import(&mut s, &plan, next_drop_id());
        assert_eq!(r.audio_regions, 1);
        assert_eq!(r.errors.len(), 1);
        assert!(r.errors[0].contains("none.wav"), "{:?}", r.errors);
        let regions = audio_regions(&s);
        assert_eq!(regions.len(), 1);
        assert!((regions[0].2 - 2.0).abs() < 1e-6, "{regions:?}");
        run(&mut s, "undo", json!({}));
        let names: Vec<&str> = s.project.tracks.iter().map(|t| t.name.as_str()).collect();
        assert_eq!(names, ["Keys"]);
        run(&mut s, "undo", json!({}));
        assert!(s.project.tracks.is_empty());
    }

    #[test]
    fn two_drops_are_two_undo_steps() {
        let mut s = Session::new(false);
        let first = plan_of(vec![wav("drop_twice_a.wav").display().to_string()], vec![], None, 0.0);
        let r1 = run_import(&mut s, &first, next_drop_id());
        assert_eq!(r1.audio_regions, 1);
        let second = plan_of(vec![wav("drop_twice_b.wav").display().to_string()], vec![], r1.track, 8.0);
        let r2 = run_import(&mut s, &second, next_drop_id());
        assert_eq!(r2.audio_regions, 1);
        assert_eq!(r2.track, r1.track);
        assert_eq!(audio_regions(&s).len(), 2);
        run(&mut s, "undo", json!({}));
        let regions = audio_regions(&s);
        assert_eq!(regions.len(), 1);
        assert_eq!(Some(regions[0].1), r1.last_region);
    }

    #[test]
    fn audio_and_midi_of_one_drop_are_one_undo_step() {
        let dir = std::env::temp_dir().join("ondina_app_tests");
        std::fs::create_dir_all(&dir).unwrap();
        let mid = dir.join("drop_song.mid").display().to_string();
        let mut src = Session::new(false);
        let tid = run(&mut src, "add_track", json!({"kind": "instrument", "name": "Keys"}))["track_id"].as_u64().unwrap();
        let notes = json!([{"pitch": 60, "velocity": 100, "start": 0.0, "length": 1.0},
                           {"pitch": 64, "velocity": 100, "start": 1.0, "length": 1.0}]);
        run(&mut src, "create_midi_region", json!({"track_id": tid, "start": 0, "length": 4, "notes": notes}));
        run(&mut src, "export_midi", json!({"path": mid}));

        let mut s = Session::new(false);
        let mut plan = plan_of(vec![wav("drop_mixed.wav").display().to_string()], vec![mid], None, 0.0);
        plan.midi_beat = 8.0;
        let r = run_import(&mut s, &plan, next_drop_id());
        assert!(r.errors.is_empty(), "{:?}", r.errors);
        assert_eq!((r.audio_regions, r.midi_files), (1, 1));
        assert!(s.project.tracks.len() >= 2);
        run(&mut s, "undo", json!({}));
        assert!(s.project.tracks.is_empty());
    }

    #[test]
    fn status_line_says_what_happened() {
        let plan = plan_of(vec!["a.wav".into(), "b.wav".into()], vec!["s.mid".into()], None, 0.0);
        let r = ImportReport { audio_regions: 2, midi_files: 1, ..Default::default() };
        assert_eq!(report_status(&plan, &r), "Imported 2 audio files · 1 MIDI file");

        let mut plan = plan_of(vec!["a.wav".into()], vec![], None, 0.0);
        plan.skipped = vec!["notes.txt".into()];
        plan.over_limit = 3;
        let r = ImportReport {
            audio_regions: 1,
            errors: vec!["b.wav: bad".into(), "c.wav: bad".into()],
            ..Default::default()
        };
        assert_eq!(
            report_status(&plan, &r),
            "Imported 1 audio file · skipped notes.txt (not audio, MIDI or a project) · \
             3 more files not imported (at most 8 per drop) · ⚠ b.wav: bad (+1 more)"
        );

        let plan = plan_of(vec!["a.wav".into()], vec![], None, 0.0);
        let r = ImportReport { errors: vec!["a.wav: cannot read".into()], ..Default::default() };
        assert!(report_status(&plan, &r).starts_with("⚠ "), "{}", report_status(&plan, &r));
    }

    #[test]
    fn hover_label_names_file_and_destination() {
        let one = DropPlan::Import(plan_of(vec!["C:/s/kick.wav".into()], vec![], Some(3), 8.0));
        assert_eq!(hover_label(&one, Some("Drums Loop"), "3.1.000"), "kick.wav → Drums Loop at 3.1.000");
        let three = DropPlan::Import(plan_of(vec!["C:/s/a.wav".into(), "C:/s/b.wav".into(), "C:/s/c.wav".into()], vec![], None, 0.0));
        assert_eq!(hover_label(&three, None, "1.1.000"), "a.wav + 2 more → new audio track at 1.1.000");
        let midi = DropPlan::Import(plan_of(vec![], vec!["C:/m/song.mid".into()], None, 4.0));
        assert_eq!(hover_label(&midi, None, "2.1.000"), "song.mid → MIDI import at 2.1.000");
        let open = DropPlan::Open("C:/p/song.ondina".into());
        assert_eq!(hover_label(&open, None, "1.1.000"), "song.ondina → open project");
        let no = DropPlan::Refuse("Can't import notes.txt".into());
        assert_eq!(hover_label(&no, None, "1.1.000"), "Can't import notes.txt");
        let mut mixed = plan_of(vec!["C:/s/a.wav".into()], vec!["C:/m/b.mid".into()], None, 0.0);
        mixed.over_limit = 2;
        mixed.skipped = vec!["x.txt".into()];
        assert_eq!(
            hover_label(&DropPlan::Import(mixed), None, "1.1.000"),
            "a.wav + 1 more → new audio track at 1.1.000 · MIDI at 1.1.000 (first 8 only) · 1 skipped"
        );
    }

    #[test]
    fn cursor_pixels_become_window_points() {
        assert_eq!(screen_px_to_points(1500, 900, 1.5, pos2(100.0, 50.0)), pos2(900.0, 550.0));
        assert_eq!(screen_px_to_points(300, 200, 1.0, pos2(0.0, 0.0)), pos2(300.0, 200.0));
    }
}
