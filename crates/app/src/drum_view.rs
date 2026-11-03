//! The drawing of the Drum Synth window: a pad for every sound of the kit on the General MIDI drum map
//! (ondina_core::drum_kit), played like the keys of the keyboard, lit while the track plays it, and under the pads the
//! knobs of the selected sound and of the whole kit. The math is plain functions the tests cover, drum_pads draws the
//! pads and turns the mouse into live notes.

use std::time::Duration;

use eframe::egui::{self, pos2, vec2, Align2, Color32, Pos2, Rect, Sense, Stroke};
use ondina_core::drum_kit::{self, DRUM_PADS};
use ondina_core::model::{Id, PluginInstance, Project, RegionContent, Track, MASTER_ID};
use ondina_core::plugins::{ParamInfo, PluginInfo};
use ondina_core::theory::note_name;
use ondina_dsp::MidiKind;

use crate::app::OndinaApp;
use crate::chrome;
use crate::plugin_params::{param_knobs, ParamEdit};
use crate::synth_view::InstrumentKeys;
use crate::theme;
use crate::widgets::{self, KeyboardState};

pub const PAD_H: f32 = 54.0;
pub const PAD_GAP: f32 = theme::SP_3;
/// The pads do not grow past this width: on a wide window they stay at the left.
pub const GRID_MAX_W: f32 = 520.0;
/// How long a pad stays lit after the track strikes it.
pub const FLASH_SECS: f64 = 0.12;
/// Repaint rate while the song plays, so the lit pads follow it in a separate window too.
pub const PADS_REPAINT: Duration = Duration::from_millis(33);

/// Whether plug-in kind `kind` has the pads.
pub fn has_drum_view(kind: &str) -> bool {
    kind == "drum_synth"
}

/// Height of the grid: four rows of pads.
pub fn grid_height() -> f32 {
    4.0 * PAD_H + 3.0 * PAD_GAP
}

/// The rectangle of every pad of DRUM_PADS (same order) inside `grid`: pad 0 at the bottom left, four per row.
pub fn pad_rects(grid: Rect) -> [Rect; 16] {
    let w = grid.width().min(GRID_MAX_W);
    let pw = (w - 3.0 * PAD_GAP) / 4.0;
    std::array::from_fn(|i| {
        let (col, row) = ((i % 4) as f32, (3 - i / 4) as f32);
        Rect::from_min_size(grid.min + vec2(col * (pw + PAD_GAP), row * (PAD_H + PAD_GAP)), vec2(pw, PAD_H))
    })
}

/// The pad under `pos` and the velocity of a press there (widgets::key_velocity: 32 at the top edge, 127 at the
/// bottom). None in the gaps and outside the grid.
pub fn pad_at(pos: Pos2, grid: Rect) -> Option<(u8, u8)> {
    let rects = pad_rects(grid);
    let i = rects.iter().position(|r| r.contains(pos))?;
    let r = rects[i];
    Some((DRUM_PADS[i].note, widgets::key_velocity((pos.y - r.top()) / r.height())))
}

/// FLASH_SECS in beats at `bpm`.
pub fn flash_beats(bpm: f64) -> f64 {
    FLASH_SECS * bpm.max(1.0) / 60.0
}

/// The pitches the regions of `track` sound at `beat`: unmuted MIDI and pattern regions (ops::pattern_to_notes), the
/// repeat the beat falls in, the region's transposition applied, notes starting past the region's length dropped. A
/// note counts from its start for its length (cut at the region's length) or `flash` beats, whichever is longer.
/// Sorted, without repeats.
pub fn struck_pitches(track: &Track, beat: f64, flash: f64) -> Vec<u8> {
    let mut out = Vec::new();
    for r in &track.regions {
        if r.muted || r.length <= 0.0 || beat < r.start || beat >= r.end() {
            continue;
        }
        let (notes, transpose) = match &r.content {
            RegionContent::Midi(m) => (m.notes.clone(), m.transpose),
            RegionContent::Pattern(p) => (ondina_core::ops::pattern_to_notes(p), 0),
            RegionContent::Audio(_) => continue,
        };
        let k = ((beat - r.start) / r.length).floor();
        let local = beat - r.start - k * r.length;
        for n in notes.iter().filter(|n| n.start < r.length) {
            let lasts = n.length.min(r.length - n.start).max(flash);
            if n.start <= local && local < n.start + lasts {
                out.push((n.pitch as i32 + transpose).clamp(0, 127) as u8);
            }
        }
    }
    out.sort_unstable();
    out.dedup();
    out
}

/// Some(PADS_REPAINT) while the song plays and the window is visible.
pub fn pads_repaint(visible: bool, playing: bool) -> Option<Duration> {
    (visible && playing).then_some(PADS_REPAINT)
}

/// The color of a group of sounds, from theme.rs: Kick MONITOR, Snare SOLO, Clap REC, Hats PLAY, Toms MUTE,
/// Cymbals AI, anything else TEXT_3.
pub fn group_color(group: &str) -> Color32 {
    match group {
        "Kick" => theme::MONITOR,
        "Snare" => theme::SOLO,
        "Clap" => theme::REC,
        "Hats" => theme::PLAY,
        "Toms" => theme::MUTE,
        "Cymbals" => theme::AI,
        _ => theme::TEXT_3,
    }
}

/// The knobs a pad shows: its own parameters (in the pad's order) and the Kit's (registry order, see
/// drum_kit::is_kit_param), as registry entries. An unknown note shows the Kick's.
pub fn pad_knobs(info: &PluginInfo, note: u8) -> (Vec<ParamInfo>, Vec<ParamInfo>) {
    let pad = drum_kit::pad(note).unwrap_or(&DRUM_PADS[0]);
    let own = pad.params.iter().filter_map(|id| info.params.iter().find(|p| p.id == *id).copied()).collect();
    let kit = info.params.iter().filter(|p| drum_kit::is_kit_param(p.id)).copied().collect();
    (own, kit)
}

pub struct DrumOut {
    /// Live notes to send: the pads behave like the keys of the keyboard.
    pub notes: Vec<MidiKind>,
    /// The pad to select: the one just pressed, or right-clicked.
    pub select: Option<u8>,
    /// The grid (tests use it to find the pads).
    #[cfg(test)]
    pub grid: Rect,
}

/// The pad grid of the Drum Synth window and its help line. `keys` holds the pad being pressed (KeyboardState: the
/// same press, slide and release rules as the keyboard), `selected` is outlined, `lit` are the pitches the track
/// plays now. `active`: the window has the focus; without it a held pad is released and nothing plays.
pub fn drum_pads(ui: &mut egui::Ui, inst: &PluginInstance, keys: &mut KeyboardState, selected: u8, lit: &[u8], active: bool) -> DrumOut {
    let (grid, _) = ui.allocate_exact_size(vec2(ui.available_width(), grid_height()), Sense::hover());
    // one widget for the whole grid, like the keys of the keyboard: no pad covers another
    let resp = ui.interact(grid, ui.id().with("drum-pads"), Sense::click_and_drag());
    let pointer = ui.input(|i| i.pointer.interact_pos());
    // the right button selects without playing
    let down = resp.is_pointer_button_down_on() && ui.input(|i| i.pointer.primary_down());
    let under = pointer.and_then(|p| pad_at(p, grid));
    let notes = keys.update(under, down && active);
    let mut select = notes.iter().find_map(|n| match n {
        MidiKind::NoteOn { pitch, .. } => Some(*pitch),
        _ => None,
    });
    if resp.secondary_clicked() {
        if let Some((note, _)) = pointer.and_then(|p| pad_at(p, grid)) {
            select = Some(note);
        }
    }

    if ui.is_rect_visible(grid) {
        let painter = ui.painter_at(grid.expand(2.0));
        let hover = resp.hover_pos().and_then(|p| pad_at(p, grid)).map(|h| h.0);
        let held = keys.held.map(|h| h.0);
        for (pad, r) in DRUM_PADS.iter().zip(pad_rects(grid)) {
            let c = if inst.bypass { theme::TEXT_DISABLED } else { group_color(pad.group) };
            let is_held = held == Some(pad.note);
            let fill = if is_held {
                theme::ACCENT
            } else {
                let t = if lit.contains(&pad.note) {
                    0.65
                } else if hover == Some(pad.note) {
                    0.30
                } else {
                    0.16
                };
                widgets::mix(theme::RAISED, c, t)
            };
            let stroke = if pad.note == selected { Stroke::new(2.0_f32, theme::ACCENT) } else { Stroke::new(1.0_f32, theme::BORDER) };
            painter.rect(r, theme::R_MD, fill, stroke);
            let name_color = if is_held { theme::ON_ACCENT } else { theme::TEXT };
            painter.text(r.left_top() + vec2(theme::SP_4, theme::SP_3), Align2::LEFT_TOP, pad.name, theme::label_font(), name_color);
            let sub = format!("{} \u{00B7} {}", pad.note, note_name(pad.note));
            let sub_color = if is_held { theme::ON_ACCENT } else { theme::TEXT_3 };
            painter.text(pos2(r.left() + theme::SP_4, r.bottom() - theme::SP_3), Align2::LEFT_BOTTOM, sub, theme::label_font(), sub_color);
        }
    }
    let name = drum_kit::pad(selected).map_or("Kick", |p| p.name);
    let help = format!("{name} selected \u{00B7} Click a pad to play it (lower is louder) \u{00B7} Right-click: select without playing \u{00B7} Lit pads follow the track");
    ui.label(egui::RichText::new(help).small().color(theme::TEXT_3));
    DrumOut {
        notes,
        select,
        #[cfg(test)]
        grid,
    }
}

/// The knobs under the pads: the selected pad's own parameters (or a line saying it has none), then a "Kit" section
/// with Drive, Level and any parameter no group owns. Same knobs, keys and undo merging as builtin_controls.
pub fn drum_knobs(ui: &mut egui::Ui, pid: Id, info: &PluginInfo, inst: &PluginInstance, selected: u8, gesture: &mut u64) -> Vec<ParamEdit> {
    let (own, kit) = pad_knobs(info, selected);
    let mut edits = Vec::new();
    if own.is_empty() {
        let name = drum_kit::pad(selected).map_or("Kick", |p| p.name);
        let line = format!("{name} has no controls of its own: Drive and Level shape the whole kit");
        ui.label(egui::RichText::new(line).small().color(theme::TEXT_3));
    } else {
        edits.extend(param_knobs(ui, pid, &own, inst, gesture));
    }
    ui.add_space(theme::SP_2);
    chrome::section_label(ui, "Kit");
    edits.extend(param_knobs(ui, pid, &kit, inst, gesture));
    edits
}

impl OndinaApp {
    /// The pads of Drum Synth `inst`'s window: they play live notes on its track like the keyboard (the held pad
    /// lives in PluginWindows::keyboards, so closing the window releases it), select the pad whose knobs show, and
    /// light up on what the track plays.
    pub(crate) fn drum_window(&mut self, ui: &mut egui::Ui, p: &Project, inst: &PluginInstance) {
        let pid = inst.id;
        let selected = self.plugin_windows.drum_pad(pid);
        let active = ui.ctx().input(|i| i.viewport().focused) != Some(false);
        let Some(track) = p.plugin_owner(pid).filter(|t| *t != MASTER_ID) else {
            // an instrument always sits on a track; without one the pads only select
            let out = drum_pads(ui, inst, &mut KeyboardState::default(), selected, &[], active);
            if let Some(n) = out.select {
                self.plugin_windows.drum_pads.insert(pid, n);
            }
            return;
        };
        let lit = match p.track(track) {
            Some(t) if self.playing() => {
                let beat = self.position();
                struck_pitches(t, beat, flash_beats(p.tempo.bpm_at(beat)))
            }
            _ => Vec::new(),
        };
        let entry = self.plugin_windows.keyboards.entry(pid).or_insert(InstrumentKeys { track, keys: KeyboardState::default() });
        // a NoteOff always goes where its NoteOn went
        if entry.keys.held.is_none() {
            entry.track = track;
        }
        let out = drum_pads(ui, inst, &mut entry.keys, selected, &lit, active);
        let to = entry.track;
        if let Some(n) = out.select {
            self.plugin_windows.drum_pads.insert(pid, n);
        }
        for kind in out.notes {
            self.live_note(to, kind);
        }
        let visible = ui.ctx().input(|i| i.viewport().minimized) != Some(true);
        if let Some(d) = pads_repaint(visible, self.playing()) {
            ui.ctx().request_repaint_after(d);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use eframe::egui::{Event, Modifiers, PointerButton};
    use ondina_core::model::{Channel, MidiContent, Note, PatternContent, PatternRow, Region, TrackKind};
    use ondina_core::plugins;

    fn instance(kind: &str) -> PluginInstance {
        PluginInstance { id: 3, kind: kind.into(), params: Default::default(), bypass: false, preset: None, sample: None, vst3_path: None, vst3_params: vec![], vst3_state: None }
    }

    fn press(pos: Pos2, button: PointerButton, pressed: bool) -> Event {
        Event::PointerButton { pos, button, pressed, modifiers: Modifiers::NONE }
    }

    fn rect_of(grid: Rect, note: u8) -> Rect {
        pad_rects(grid)[DRUM_PADS.iter().position(|p| p.note == note).unwrap()]
    }

    struct Run {
        grid: Rect,
        height: f32,
        notes: Vec<MidiKind>,
        selects: Vec<Option<u8>>,
    }

    /// Frames of the pads in a headless 700x500 context, one per entry (the window's focus and the events of that
    /// frame).
    fn pads_frames(inst: &PluginInstance, keys: &mut KeyboardState, lit: &[u8], frames: &[(bool, Vec<Event>)]) -> Run {
        let ctx = egui::Context::default();
        theme::apply(&ctx);
        let mut run = Run { grid: Rect::NOTHING, height: 0.0, notes: Vec::new(), selects: Vec::new() };
        for (k, (active, events)) in frames.iter().enumerate() {
            let raw = egui::RawInput {
                screen_rect: Some(Rect::from_min_size(Pos2::ZERO, vec2(700.0, 500.0))),
                time: Some(k as f64 / 60.0),
                events: events.clone(),
                ..Default::default()
            };
            let _ = ctx.run(raw, |ctx| {
                egui::CentralPanel::default().show(ctx, |ui| {
                    let top = ui.cursor().top();
                    let out = drum_pads(ui, inst, keys, drum_kit::DEFAULT_PAD, lit, *active);
                    run.height = ui.min_rect().bottom() - top;
                    run.grid = out.grid;
                    run.notes.extend(out.notes);
                    run.selects.push(out.select);
                });
            });
        }
        run
    }

    fn grid() -> Rect {
        pads_frames(&instance("drum_synth"), &mut KeyboardState::default(), &[], &[(true, vec![])]).grid
    }

    #[test]
    fn pads_cover_the_grid_in_kit_order() {
        let grid = Rect::from_min_size(pos2(10.0, 20.0), vec2(500.0, grid_height()));
        let rects = pad_rects(grid);
        for (i, a) in rects.iter().enumerate() {
            assert!(grid.expand(0.01).contains_rect(*a), "pad {i} {a:?}");
            for b in &rects[i + 1..] {
                assert!(!a.intersects(*b) || a.intersect(*b).area() < 1e-3, "{a:?} {b:?}");
            }
        }
        assert_eq!((rects[0].left(), rects[0].bottom()), (grid.left(), grid.bottom()));
        assert_eq!(DRUM_PADS[3].name, "Clap");
        assert!((rects[3].right() - grid.right()).abs() < 1e-3 && (rects[3].bottom() - grid.bottom()).abs() < 1e-3);
        assert_eq!(DRUM_PADS[15].name, "Claves");
        assert!((rects[15].right() - grid.right()).abs() < 1e-3 && (rects[15].top() - grid.top()).abs() < 1e-3);
        let wide = Rect::from_min_size(Pos2::ZERO, vec2(1200.0, grid_height()));
        let used = pad_rects(wide).iter().map(|r| r.right()).fold(0.0, f32::max);
        assert!(used <= GRID_MAX_W + 1e-3, "{used}");
    }

    #[test]
    fn pad_at_finds_the_pad_and_its_velocity() {
        let grid = Rect::from_min_size(pos2(10.0, 20.0), vec2(500.0, grid_height()));
        let snare = rect_of(grid, 38);
        assert!(matches!(pad_at(snare.center(), grid), Some((38, _))));
        let (_, top) = pad_at(pos2(snare.center().x, snare.top() + 0.5), grid).unwrap();
        assert!((32..=34).contains(&top), "{top}");
        let (_, bottom) = pad_at(pos2(snare.center().x, snare.bottom() - 0.5), grid).unwrap();
        assert!((126..=127).contains(&bottom), "{bottom}");
        assert_eq!(pad_at(pos2(snare.right() + PAD_GAP / 2.0, snare.center().y), grid), None);
        assert_eq!(pad_at(pos2(grid.right() + 5.0, snare.center().y), grid), None);
        assert_eq!(pad_at(pos2(snare.center().x, grid.bottom() + 5.0), grid), None);
    }

    #[test]
    fn pressing_a_pad_plays_its_note_until_release() {
        let p = rect_of(grid(), 38).center();
        let mut keys = KeyboardState::default();
        let frames = [(true, vec![Event::PointerMoved(p)]), (true, vec![press(p, PointerButton::Primary, true)]), (true, vec![press(p, PointerButton::Primary, false)])];
        let run = pads_frames(&instance("drum_synth"), &mut keys, &[], &frames);
        assert!(matches!(run.notes.as_slice(), [MidiKind::NoteOn { pitch: 38, .. }, MidiKind::NoteOff { pitch: 38 }]), "{:?}", run.notes);
        assert_eq!(run.selects, [None, Some(38), None]);
        assert_eq!(keys.held, None);
    }

    #[test]
    fn leaving_the_pads_with_the_button_held_releases_the_note() {
        let g = grid();
        let kick = rect_of(g, 36).center();
        let outside = pos2(g.center().x, g.bottom() + 40.0);
        let mut keys = KeyboardState::default();
        let frames = [(true, vec![Event::PointerMoved(kick)]), (true, vec![press(kick, PointerButton::Primary, true)]), (true, vec![Event::PointerMoved(outside)]), (true, vec![Event::PointerMoved(outside + vec2(5.0, 0.0))])];
        let run = pads_frames(&instance("drum_synth"), &mut keys, &[], &frames);
        assert!(matches!(run.notes.as_slice(), [MidiKind::NoteOn { pitch: 36, .. }, MidiKind::NoteOff { pitch: 36 }]), "{:?}", run.notes);
        assert_eq!(keys.held, None);

        let snare = rect_of(g, 38).center();
        let mut keys = KeyboardState::default();
        let frames = [(true, vec![Event::PointerMoved(kick)]), (true, vec![press(kick, PointerButton::Primary, true)]), (true, vec![Event::PointerMoved(snare)])];
        let run = pads_frames(&instance("drum_synth"), &mut keys, &[], &frames);
        assert!(matches!(run.notes.as_slice(), [MidiKind::NoteOn { pitch: 36, .. }, MidiKind::NoteOff { pitch: 36 }, MidiKind::NoteOn { pitch: 38, .. }]), "{:?}", run.notes);
        assert_eq!(run.selects.last(), Some(&Some(38)));
    }

    #[test]
    fn losing_focus_with_a_pad_held_sends_one_note_off_and_no_new_note_on() {
        let p = rect_of(grid(), 38).center();
        let mut keys = KeyboardState::default();
        // the button goes down with focus, then the window loses it while the button is still held and the pointer
        // keeps moving over the pad: no release arrives
        let frames = [
            (true, vec![Event::PointerMoved(p)]),
            (true, vec![press(p, PointerButton::Primary, true)]),
            (false, vec![Event::PointerMoved(p + vec2(1.0, 0.0))]),
            (false, vec![Event::PointerMoved(p)]),
            (false, vec![Event::PointerMoved(p + vec2(0.0, -1.0))]),
        ];
        let run = pads_frames(&instance("drum_synth"), &mut keys, &[], &frames);
        assert!(matches!(run.notes.as_slice(), [MidiKind::NoteOn { pitch: 38, .. }, MidiKind::NoteOff { pitch: 38 }]), "{:?}", run.notes);
        assert_eq!(keys.held, None);
    }

    #[test]
    fn right_click_selects_a_pad_without_playing() {
        let p = rect_of(grid(), 49).center();
        let mut keys = KeyboardState::default();
        let frames = [(true, vec![Event::PointerMoved(p)]), (true, vec![press(p, PointerButton::Secondary, true)]), (true, vec![press(p, PointerButton::Secondary, false)]), (true, vec![])];
        let run = pads_frames(&instance("drum_synth"), &mut keys, &[], &frames);
        assert!(run.notes.is_empty(), "{:?}", run.notes);
        assert!(run.selects.contains(&Some(49)), "{:?}", run.selects);
        assert!(run.selects.iter().all(|s| s.is_none() || *s == Some(49)));
    }

    #[test]
    fn drum_pads_draw_without_notes() {
        let mut bypassed = instance("drum_synth");
        bypassed.bypass = true;
        for inst in [instance("drum_synth"), bypassed] {
            let run = pads_frames(&inst, &mut KeyboardState::default(), &[36, 42], &[(true, vec![]), (true, vec![])]);
            assert!(run.notes.is_empty());
            assert!(run.selects.iter().all(|s| s.is_none()));
            assert!(run.height >= grid_height() && grid_height() > 100.0, "{}", run.height);
        }
    }

    fn track(regions: Vec<Region>) -> Track {
        Track {
            id: 1,
            name: "Drums".into(),
            kind: TrackKind::Instrument,
            color: [0, 0, 0],
            channel: Channel::default(),
            instrument: None,
            midi_fx: vec![],
            regions,
            automation: vec![],
            record_armed: false,
            input_channel: 0,
            height: 0.0,
        }
    }

    fn region(start: f64, length: f64, loop_count: u32, content: RegionContent) -> Region {
        Region { id: 2, name: "r".into(), start, length, loop_count, muted: false, color: None, content, take: None }
    }

    fn midi(notes: &[(u8, f64, f64)], transpose: i32) -> RegionContent {
        let notes = notes.iter().map(|(pitch, start, length)| Note { pitch: *pitch, velocity: 100, start: *start, length: *length }).collect();
        RegionContent::Midi(MidiContent { notes, transpose, ..Default::default() })
    }

    #[test]
    fn struck_pitches_follow_the_regions() {
        let notes = [(36, 0.0, 0.25), (38, 1.0, 0.25)];
        let t = track(vec![region(4.0, 4.0, 2, midi(&notes, 0))]);
        assert_eq!(struck_pitches(&t, 4.0, 0.1), [36]);
        assert_eq!(struck_pitches(&t, 5.1, 0.24), [38]);
        assert_eq!(struck_pitches(&t, 9.0, 0.1), [38]);
        assert!(struck_pitches(&t, 3.9, 0.1).is_empty());
        assert!(struck_pitches(&t, 12.0, 0.1).is_empty());
        let mut muted = t.clone();
        muted.regions[0].muted = true;
        assert!(struck_pitches(&muted, 4.0, 0.1).is_empty());
        let up = track(vec![region(4.0, 4.0, 2, midi(&notes, 2))]);
        assert_eq!(struck_pitches(&up, 4.0, 0.1), [38]);
        let pattern = PatternContent { steps: 16, step_length: 0.25, rows: vec![PatternRow { name: "Closed Hat".into(), pitch: 42, steps: vec![100, 0, 0, 0], muted: false }], swing: 0.0 };
        let p = track(vec![region(0.0, 4.0, 1, RegionContent::Pattern(pattern))]);
        assert_eq!(struck_pitches(&p, 0.0, 0.1), [42]);
    }

    #[test]
    fn pads_repaint_only_while_playing_and_visible() {
        assert_eq!(pads_repaint(true, true), Some(PADS_REPAINT));
        assert_eq!(pads_repaint(true, false), None);
        assert_eq!(pads_repaint(false, true), None);
        assert_eq!(pads_repaint(false, false), None);
        assert!((flash_beats(120.0) - 0.24).abs() < 1e-9);
    }

    fn ids(v: &[ParamInfo]) -> Vec<&'static str> {
        v.iter().map(|p| p.id).collect()
    }

    #[test]
    fn pad_knobs_show_the_pad_and_the_kit() {
        let info = plugins::find("drum_synth").unwrap();
        let (own, kit) = pad_knobs(info, 38);
        assert_eq!(ids(&own), ["snare_tune", "snare_snappy", "snare_decay"]);
        assert_eq!(ids(&kit), ["drive", "level"]);
        assert!(pad_knobs(info, 56).0.is_empty());
        assert_eq!(ids(&pad_knobs(info, 49).0), ["hat_tone", "cymbal_decay"]);
        assert_eq!(ids(&pad_knobs(info, 60).0), ids(&pad_knobs(info, 36).0));
        for p in info.params {
            let shown = ids(&kit).contains(&p.id) || DRUM_PADS.iter().any(|pad| ids(&pad_knobs(info, pad.note).0).contains(&p.id));
            assert!(shown, "{} shows under no pad", p.id);
        }
    }

    #[test]
    fn every_group_has_its_own_color() {
        let colors: Vec<Color32> = drum_kit::DRUM_GROUPS.iter().map(|g| group_color(g.name)).collect();
        for (i, a) in colors.iter().enumerate() {
            assert_ne!(*a, theme::ACCENT);
            assert!(!colors[i + 1..].contains(a), "{}", drum_kit::DRUM_GROUPS[i].name);
        }
    }

    #[test]
    fn the_drum_view_reuses_the_common_header() {
        let src = include_str!("drum_view.rs");
        assert!(!src.contains(concat!("plugin_ai_bar", "(")));
        assert!(!src.contains(concat!("plugin_header", "(")));
    }

    #[test]
    fn the_drum_window_keeps_its_held_pad_in_the_window_keyboards() {
        let src = include_str!("drum_view.rs");
        let body = &src[src.find(concat!("fn drum_window", "(")).unwrap()..src.find(concat!("mod tests", " {")).unwrap()];
        assert!(body.contains(concat!("plugin_windows.keyboards.entry", "(")));
        assert!(body.contains(concat!("live_note", "(")));
    }
}
