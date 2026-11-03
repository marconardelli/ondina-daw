//! The drawing of the Sampler window: the sample's waveform with its name, length, loop and root note (or how to load
//! one), a strip of keys to set the root note, and the amp envelope. The waveform comes from the peaks the app already
//! reads for every audio file (OndinaApp::waveforms); the math is plain functions the tests cover, sampler_graph draws
//! them and turns the mouse into edits.

use std::collections::BTreeMap;

use eframe::egui::{self, pos2, vec2, Align2, CursorIcon, Pos2, Rect, Sense, Shape, Stroke, Vec2};
use ondina_core::command::Command;
use ondina_core::model::{AudioFileRef, Id, PluginInstance};
use ondina_core::plugins;
use ondina_core::theory::note_name;

use crate::app::OndinaApp;
use crate::synth_view::{amp_env, env_prefix, ROW_ENV_H};
use crate::theme;
use crate::widgets::{envelope, ParamsEdit};

pub const WAVE_H: f32 = 120.0;
pub const STRIP_H: f32 = 28.0;
/// Wheel points per semitone of the root strip: a notch (40 points) is one semitone.
pub const ROOT_WHEEL_POINTS: f32 = 40.0;
pub const DEFAULT_ROOT: f32 = 60.0;

/// Whether plug-in kind `kind` has the sample drawing.
pub fn has_sampler_view(kind: &str) -> bool {
    kind == "sampler"
}

/// The min and max of the peaks (OndinaApp::waveforms: one (min, max) per 256 frames) that fall in each of `columns`
/// columns. With more columns than peaks each column repeats the peak under it; empty for no peaks or no columns.
pub fn column_peaks(peaks: &[(f32, f32)], columns: usize) -> Vec<(f32, f32)> {
    let n = peaks.len();
    if n == 0 || columns == 0 {
        return Vec::new();
    }
    (0..columns)
        .map(|i| {
            let lo = i * n / columns;
            let hi = ((i + 1) * n / columns).max(lo + 1).min(n);
            peaks[lo..hi].iter().fold((f32::INFINITY, f32::NEG_INFINITY), |(a, b), (mn, mx)| (a.min(*mn), b.max(*mx)))
        })
        .collect()
}

/// The x of the middle of note `note`'s cell (0..=127 across `rect`).
pub fn strip_x(note: f32, rect: Rect) -> f32 {
    rect.left() + (note.clamp(0.0, 127.0) + 0.5) * rect.width() / 128.0
}

/// The note of the cell under `x`, 0..=127.
pub fn strip_note(x: f32, rect: Rect) -> f32 {
    ((x - rect.left()) / rect.width().max(1.0) * 128.0).floor().clamp(0.0, 127.0)
}

/// The root after a wheel turn of `scroll` points added to `acc` (x and y summed): one semitone per
/// ROOT_WHEEL_POINTS, wheel up raising it, clamped to 0..=127. Returns what is left over and the new root.
pub fn root_wheel(acc: f32, root: f32, scroll: Vec2) -> (f32, f32) {
    let s = scroll.x + scroll.y;
    if !s.is_finite() {
        return (acc, root);
    }
    let total = acc + s;
    let steps = (total / ROOT_WHEEL_POINTS).trunc();
    (total - steps * ROOT_WHEEL_POINTS, (root.round() + steps).clamp(0.0, 127.0))
}

/// Undo key of a drag of the root: one step per gesture.
pub fn root_drag_key(pid: Id, gesture: u64) -> String {
    format!("pw{pid}:root#{gesture}")
}

/// Undo key of the wheel over the root strip: turns close together merge.
pub fn root_wheel_key(pid: Id) -> String {
    format!("pw{pid}:root:wheel")
}

/// "Root C4 · Tune +0.00 st".
pub fn root_label(inst: &PluginInstance) -> String {
    let root = inst.param("root_note").round().clamp(0.0, 127.0) as u8;
    format!("Root {} \u{00B7} Tune {:+.2} st", note_name(root), inst.param("tune"))
}

/// "Loop: the whole sample" with Loop On, "One-shot" with it Off.
pub fn loop_label(inst: &PluginInstance) -> &'static str {
    if inst.param("loop") >= 0.5 {
        "Loop: the whole sample"
    } else {
        "One-shot"
    }
}

/// "vox.wav · 2.35 s · 44.1 kHz · stereo" (mono, stereo, or "N ch").
pub fn sample_caption(file: &AudioFileRef) -> String {
    let rate = if file.sample_rate % 1000 == 0 { format!("{} kHz", file.sample_rate / 1000) } else { format!("{:.1} kHz", file.sample_rate as f32 / 1000.0) };
    let channels = match file.channels {
        1 => "mono".to_string(),
        2 => "stereo".to_string(),
        n => format!("{n} ch"),
    };
    format!("{} \u{00B7} {:.2} s \u{00B7} {rate} \u{00B7} {channels}", file.name, file.duration_sec)
}

/// Where the parts of the drawing are (tests use it to aim the mouse).
#[cfg(test)]
#[derive(Debug, Clone, Copy)]
pub struct SamplerLayout {
    pub wave: Rect,
    pub strip: Rect,
    pub env: Rect,
    /// The Load sample button, Rect::NOTHING with a sample.
    pub load: Rect,
}

pub struct SamplerOut {
    pub edits: Vec<ParamsEdit>,
    /// "Load sample…" was pressed.
    pub load: bool,
    #[cfg(test)]
    pub layout: SamplerLayout,
}

fn root_edit(root: f32) -> BTreeMap<String, f32> {
    BTreeMap::from([("root_note".to_string(), root)])
}

/// The Sampler's drawings above its knobs: the sample's waveform with its name, length, loop and root note (or how to
/// load one), the root strip and the amp envelope. `file` and `peaks` are the sample's entry in the project and its
/// peaks (None while there is no sample, or before the peaks are read).
pub fn sampler_graph(ui: &mut egui::Ui, inst: &PluginInstance, file: Option<&AudioFileRef>, peaks: Option<&[(f32, f32)]>, gesture: &mut u64) -> SamplerOut {
    let w = ui.available_width();
    let mut edits = Vec::new();
    let mut load = false;
    let wave_color = if inst.bypass { theme::TEXT_DISABLED } else { theme::ACCENT };

    // the waveform: background first, hover only, so the button registered after it takes the clicks
    let (wave, _) = ui.allocate_exact_size(vec2(w, WAVE_H), Sense::hover());
    let mut load_rect = Rect::NOTHING;
    if file.is_none() {
        load_rect = Rect::from_center_size(wave.center() + vec2(0.0, 26.0), vec2(124.0, 24.0));
        load = ui.put(load_rect, egui::Button::new("Load sample\u{2026}")).on_hover_text("Pick an audio file for this Sampler").clicked();
    }
    if ui.is_rect_visible(wave) {
        let painter = ui.painter_at(wave);
        painter.rect(wave, theme::R_MD, theme::INSET, Stroke::new(1.0_f32, theme::BORDER));
        painter.hline(wave.x_range(), wave.center().y, Stroke::new(1.0_f32, theme::HAIRLINE));
        let pad = theme::SP_4;
        match (file, peaks.filter(|p| !p.is_empty())) {
            (Some(f), Some(peaks)) => {
                let half = (WAVE_H / 2.0 - 18.0).max(1.0);
                let cy = wave.center().y;
                let shapes: Vec<Shape> = column_peaks(peaks, wave.width().max(0.0) as usize)
                    .into_iter()
                    .enumerate()
                    .map(|(i, (lo, hi))| {
                        let x = wave.left() + i as f32 + 0.5;
                        let (top, bottom) = (cy - hi.clamp(-1.0, 1.0) * half, cy - lo.clamp(-1.0, 1.0) * half);
                        Shape::line_segment([pos2(x, top), pos2(x, bottom.max(top + 1.0))], Stroke::new(1.0_f32, wave_color))
                    })
                    .collect();
                painter.extend(shapes);
                painter.text(wave.left_top() + vec2(pad, theme::SP_3), Align2::LEFT_TOP, sample_caption(f), theme::label_font(), theme::TEXT_2);
                painter.text(wave.right_top() + vec2(-pad, theme::SP_3), Align2::RIGHT_TOP, loop_label(inst), theme::label_font(), theme::TEXT_2);
                if inst.param("loop") >= 0.5 {
                    // the loop covers the whole sample: a line along the top edge, a notch at each end
                    let y = wave.top() + 1.5;
                    let stroke = Stroke::new(2.0_f32, theme::SOLO);
                    painter.hline(wave.left() + 1.0..=wave.right() - 1.0, y, stroke);
                    painter.vline(wave.left() + 1.0, y..=y + 8.0, stroke);
                    painter.vline(wave.right() - 1.0, y..=y + 8.0, stroke);
                }
                painter.text(wave.right_bottom() + vec2(-pad, -theme::SP_3), Align2::RIGHT_BOTTOM, root_label(inst), theme::number_font(), theme::TEXT);
            }
            (Some(f), None) => {
                painter.text(wave.left_top() + vec2(pad, theme::SP_3), Align2::LEFT_TOP, sample_caption(f), theme::label_font(), theme::TEXT_2);
                painter.text(wave.center(), Align2::CENTER_CENTER, "Waveform not available", theme::label_font(), theme::TEXT_3);
                painter.text(wave.right_bottom() + vec2(-pad, -theme::SP_3), Align2::RIGHT_BOTTOM, root_label(inst), theme::number_font(), theme::TEXT);
            }
            (None, _) => {
                painter.text(wave.center() + vec2(0.0, -26.0), Align2::CENTER_CENTER, "No sample loaded", theme::label_font(), theme::TEXT_2);
                painter.text(wave.center() + vec2(0.0, -8.0), Align2::CENTER_CENTER, "Load an audio file to play it from the keyboard", theme::label_font(), theme::TEXT_3);
            }
        }
    }

    ui.add_space(theme::SP_3);
    let strip = root_strip(ui, inst, w, gesture, &mut edits);
    ui.add_space(theme::SP_3);

    let env_color = if inst.bypass { theme::TEXT_DISABLED } else { theme::ACCENT };
    let env_top = ui.cursor().top();
    edits.extend(envelope(ui, &amp_env(inst, env_color), &env_prefix(inst.id, "amp"), vec2(w, ROW_ENV_H), gesture));
    let _ = (strip, env_top);
    let help = "Click or drag the strip to set the root note (wheel: one semitone) \u{00B7} Double-click: default \u{00B7} Drag the envelope points (Shift: fine)";
    ui.label(egui::RichText::new(help).small().color(theme::TEXT_3));
    SamplerOut {
        edits,
        load,
        #[cfg(test)]
        layout: SamplerLayout { wave, strip, env: Rect::from_min_size(pos2(wave.left(), env_top), vec2(w, ROW_ENV_H)), load: load_rect },
    }
}

/// The strip of 128 keys under the waveform: a click or a drag sets the root note, the wheel moves it by semitones,
/// a double-click puts it back to its default. Returns where it is.
fn root_strip(ui: &mut egui::Ui, inst: &PluginInstance, w: f32, gesture: &mut u64, edits: &mut Vec<ParamsEdit>) -> Rect {
    let (strip, _) = ui.allocate_exact_size(vec2(w, STRIP_H), Sense::hover());
    let resp = ui.interact(strip, ui.id().with("root-strip"), Sense::click_and_drag()).on_hover_cursor(CursorIcon::PointingHand);
    let root = inst.param("root_note").round().clamp(0.0, 127.0);
    let default = plugins::find(&inst.kind).and_then(|i| i.params.iter().find(|p| p.id == "root_note")).map_or(DEFAULT_ROOT, |p| p.default);
    let pointer = resp.interact_pointer_pos().or(resp.hover_pos());
    let under = pointer.map(|p| strip_note(p.x, strip));
    if resp.double_clicked() {
        if default != root {
            edits.push(ParamsEdit::Set { params: root_edit(default) });
        }
    } else if resp.clicked() {
        if let Some(n) = under.filter(|n| *n != root) {
            edits.push(ParamsEdit::Set { params: root_edit(n) });
        }
    }
    if resp.drag_started() {
        *gesture += 1;
    }
    if resp.dragged() {
        if let Some(n) = under.filter(|n| *n != root) {
            edits.push(ParamsEdit::Drag { params: root_edit(n), key: root_drag_key(inst.id, *gesture) });
        }
    }
    let acc_id = egui::Id::new(("root-wheel", inst.id));
    if resp.contains_pointer() {
        let s = ui.input(|i| i.raw_scroll_delta);
        if s != Vec2::ZERO {
            let acc = ui.data(|d| d.get_temp::<f32>(acc_id)).unwrap_or(0.0);
            let (left, next) = root_wheel(acc, root, s);
            ui.data_mut(|d| d.insert_temp(acc_id, left));
            if next != root {
                edits.push(ParamsEdit::Drag { params: root_edit(next), key: root_wheel_key(inst.id) });
            }
            ui.input_mut(|i| {
                i.raw_scroll_delta = Vec2::ZERO;
                i.smooth_scroll_delta = Vec2::ZERO;
            });
        }
    }

    if ui.is_rect_visible(strip) {
        let painter = ui.painter_at(strip.expand(1.0));
        painter.rect(strip, theme::R_SM, theme::CONTROL, Stroke::new(1.0_f32, theme::BORDER));
        let cell = strip.width() / 128.0;
        let hover = resp.hover_pos().map(|p| strip_note(p.x, strip));
        for n in 0..128u8 {
            let x0 = strip.left() + n as f32 * cell;
            let r = Rect::from_min_max(pos2(x0, strip.top() + 1.0), pos2(x0 + cell, strip.bottom() - 1.0));
            let black = matches!(n % 12, 1 | 3 | 6 | 8 | 10);
            let fill = if n as f32 == root {
                theme::ACCENT
            } else if hover == Some(n as f32) {
                theme::HOVER
            } else if black {
                theme::GROUND
            } else {
                continue;
            };
            painter.rect_filled(r, 0.0, fill);
        }
        for n in (0..128u8).step_by(12) {
            let x = strip.left() + n as f32 * cell;
            painter.vline(x, strip.y_range(), Stroke::new(1.0_f32, theme::BORDER_HOVER));
            painter.text(pos2(x + theme::SP_1, strip.bottom() - theme::SP_1), Align2::LEFT_BOTTOM, note_name(n), theme::label_font(), theme::TEXT_3);
        }
        let x = strip_x(root, strip);
        let tip = strip.top() + 6.0;
        painter.add(Shape::convex_polygon(vec![pos2(x - 4.0, strip.top()), pos2(x + 4.0, strip.top()), pos2(x, tip)], theme::ON_ACCENT, Stroke::NONE));
    }
    strip
}

impl OndinaApp {
    /// "Load sample…": an audio file picked in the system dialog goes into Sampler `pid` (SetSamplerSample).
    pub(crate) fn load_sampler_sample(&mut self, pid: Id) {
        if let Some(f) = rfd::FileDialog::new().add_filter("Audio", &["wav", "aif", "aiff", "flac", "mp3", "ogg"]).pick_file() {
            self.exec(Command::SetSamplerSample { plugin_id: pid, path: f.display().to_string() });
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use eframe::egui::{Event, Modifiers, MouseWheelUnit, PointerButton};

    fn instance(kind: &str) -> PluginInstance {
        PluginInstance { id: 3, kind: kind.into(), params: Default::default(), bypass: false, preset: None, sample: None, vst3_path: None, vst3_params: vec![], vst3_state: None }
    }

    fn with(mut i: PluginInstance, kv: &[(&str, f32)]) -> PluginInstance {
        for (k, v) in kv {
            i.params.insert(k.to_string(), *v);
        }
        i
    }

    fn file() -> AudioFileRef {
        AudioFileRef { id: 900, path: "pad.wav".into(), name: "pad.wav".into(), sample_rate: 44100, channels: 2, duration_sec: 2.5 }
    }

    fn peaks() -> Vec<(f32, f32)> {
        (0..400).map(|i| (i as f32 * 0.05).sin().abs()).map(|a| (-a, a)).collect()
    }

    fn press(pos: Pos2, button: PointerButton, pressed: bool) -> Event {
        Event::PointerButton { pos, button, pressed, modifiers: Modifiers::NONE }
    }

    struct Run {
        layout: SamplerLayout,
        edits: Vec<ParamsEdit>,
        load: bool,
        height: f32,
    }

    /// Frames of the drawing in a headless 700x600 context, one per entry of `frames`.
    fn run_frames(inst: &PluginInstance, file: Option<&AudioFileRef>, peaks: Option<&[(f32, f32)]>, gesture: &mut u64, frames: &[Vec<Event>]) -> Run {
        let ctx = egui::Context::default();
        theme::apply(&ctx);
        let mut run = Run { layout: SamplerLayout { wave: Rect::NOTHING, strip: Rect::NOTHING, env: Rect::NOTHING, load: Rect::NOTHING }, edits: Vec::new(), load: false, height: 0.0 };
        for (k, events) in frames.iter().enumerate() {
            let raw = egui::RawInput {
                screen_rect: Some(Rect::from_min_size(Pos2::ZERO, vec2(700.0, 600.0))),
                time: Some(k as f64 / 60.0),
                events: events.clone(),
                ..Default::default()
            };
            let _ = ctx.run(raw, |ctx| {
                egui::CentralPanel::default().show(ctx, |ui| {
                    let top = ui.cursor().top();
                    let out = sampler_graph(ui, inst, file, peaks, gesture);
                    run.height = ui.min_rect().bottom() - top;
                    run.layout = out.layout;
                    run.edits.extend(out.edits);
                    run.load |= out.load;
                });
            });
        }
        run
    }

    fn layout(inst: &PluginInstance) -> SamplerLayout {
        run_frames(inst, None, None, &mut 0, &[vec![]]).layout
    }

    fn cell(strip: Rect, note: f32) -> Pos2 {
        pos2(strip_x(note, strip), strip.center().y)
    }

    #[test]
    fn column_peaks_keep_the_extremes_of_each_column() {
        let p: Vec<(f32, f32)> = (0..8).map(|i| (-(i as f32), i as f32 * 2.0)).collect();
        assert_eq!(column_peaks(&p, 4), [(-1.0, 2.0), (-3.0, 6.0), (-5.0, 10.0), (-7.0, 14.0)]);
        let p = [(-0.1, 0.1), (-0.2, 0.2), (-0.3, 0.3)];
        assert_eq!(column_peaks(&p, 6), [p[0], p[0], p[1], p[1], p[2], p[2]]);
        assert!(column_peaks(&[], 4).is_empty());
        assert!(column_peaks(&p, 0).is_empty());
    }

    #[test]
    fn strip_maps_notes_and_back() {
        let r = Rect::from_min_size(pos2(10.0, 0.0), vec2(640.0, STRIP_H));
        for n in [0.0, 60.0, 127.0] {
            assert_eq!(strip_note(strip_x(n, r), r), n);
        }
        assert_eq!(strip_note(r.left() - 30.0, r), 0.0);
        assert_eq!(strip_note(r.right() + 30.0, r), 127.0);
    }

    #[test]
    fn root_wheel_moves_by_semitones() {
        assert_eq!(root_wheel(0.0, 60.0, vec2(0.0, 40.0)), (0.0, 61.0));
        let (acc, root) = root_wheel(0.0, 60.0, vec2(0.0, 20.0));
        assert_eq!(root, 60.0);
        assert_eq!(root_wheel(acc, root, vec2(0.0, 20.0)), (0.0, 61.0));
        assert_eq!(root_wheel(0.0, 0.0, vec2(0.0, -40.0)).1, 0.0);
        assert_eq!(root_wheel(0.0, 127.0, vec2(0.0, 80.0)).1, 127.0);
        assert_eq!(root_wheel(5.0, 60.0, vec2(f32::NAN, 0.0)), (5.0, 60.0));
    }

    #[test]
    fn labels_read_root_loop_and_sample() {
        let inst = instance("sampler");
        assert_eq!(root_label(&inst), "Root C4 \u{00B7} Tune +0.00 st");
        assert_eq!(root_label(&with(instance("sampler"), &[("root_note", 57.0), ("tune", -0.3)])), "Root A3 \u{00B7} Tune -0.30 st");
        assert_eq!(loop_label(&inst), "One-shot");
        assert_eq!(loop_label(&with(instance("sampler"), &[("loop", 1.0)])), "Loop: the whole sample");
        let f = AudioFileRef { id: 1, path: "vox.wav".into(), name: "vox.wav".into(), sample_rate: 44100, channels: 2, duration_sec: 2.346 };
        assert_eq!(sample_caption(&f), "vox.wav \u{00B7} 2.35 s \u{00B7} 44.1 kHz \u{00B7} stereo");
        let f = AudioFileRef { sample_rate: 48000, channels: 1, ..f };
        assert_eq!(sample_caption(&f), "vox.wav \u{00B7} 2.35 s \u{00B7} 48 kHz \u{00B7} mono");
    }

    #[test]
    fn sampler_graph_draws_with_and_without_a_sample_without_edits() {
        let f = file();
        let p = peaks();
        let mut bypassed = with(instance("sampler"), &[("loop", 1.0)]);
        bypassed.bypass = true;
        for inst in [instance("sampler"), with(instance("sampler"), &[("loop", 1.0)]), bypassed] {
            for (file, peaks) in [(None, None), (Some(&f), Some(p.as_slice())), (Some(&f), None)] {
                let mut g = 0;
                let run = run_frames(&inst, file, peaks, &mut g, &[vec![], vec![]]);
                assert!(run.edits.is_empty(), "{:?}", run.edits);
                assert_eq!(g, 0);
                assert!(!run.load);
                assert!(run.height > 100.0, "{}", run.height);
                assert!(run.layout.wave.is_positive() && run.layout.strip.is_positive() && run.layout.env.is_positive());
                assert_eq!(run.layout.load.is_positive(), file.is_none());
            }
        }
    }

    #[test]
    fn dragging_on_the_root_strip_is_one_gesture() {
        let inst = instance("sampler");
        let strip = layout(&inst).strip;
        let p = cell(strip, 60.0);
        let step = vec2((strip_x(64.0, strip) - p.x) / 3.0, 0.0);
        let mut frames = vec![vec![], vec![Event::PointerMoved(p)], vec![press(p, PointerButton::Primary, true)]];
        for k in 1..=3 {
            frames.push(vec![Event::PointerMoved(p + step * k as f32)]);
        }
        frames.push(vec![press(p + step * 3.0, PointerButton::Primary, false)]);
        let mut g = 0;
        let run = run_frames(&inst, None, None, &mut g, &frames);
        assert!(!run.edits.is_empty());
        for e in &run.edits {
            assert!(matches!(e, ParamsEdit::Drag { key, .. } if *key == root_drag_key(3, 1)), "{:?}", run.edits);
        }
        let Some(ParamsEdit::Drag { params, .. }) = run.edits.last() else { unreachable!() };
        assert_eq!(params["root_note"], 64.0);
        assert_eq!(g, 1);
    }

    #[test]
    fn wheel_over_the_strip_moves_the_root_by_semitones() {
        let inst = instance("sampler");
        let strip = layout(&inst).strip;
        let wheel = Event::MouseWheel { unit: MouseWheelUnit::Line, delta: vec2(0.0, 1.0), modifiers: Modifiers::NONE };
        let mut g = 0;
        let run = run_frames(&inst, None, None, &mut g, &[vec![], vec![Event::PointerMoved(cell(strip, 30.0))], vec![wheel]]);
        assert_eq!(run.edits, [ParamsEdit::Drag { params: root_edit(61.0), key: root_wheel_key(3) }]);
        assert_eq!(g, 0);
    }

    #[test]
    fn double_click_on_the_strip_sets_the_default_root() {
        let inst = with(instance("sampler"), &[("root_note", 48.0)]);
        let strip = layout(&inst).strip;
        let p = cell(strip, 70.0);
        let frames = [vec![], vec![Event::PointerMoved(p)], vec![press(p, PointerButton::Primary, true)], vec![press(p, PointerButton::Primary, false)], vec![press(p, PointerButton::Primary, true)], vec![press(p, PointerButton::Primary, false)]];
        let run = run_frames(&inst, None, None, &mut 0, &frames);
        assert_eq!(run.edits.last(), Some(&ParamsEdit::Set { params: root_edit(60.0) }), "{:?}", run.edits);
    }

    #[test]
    fn without_a_sample_the_load_button_asks_for_a_file() {
        let inst = instance("sampler");
        let b = layout(&inst).load.center();
        let run = run_frames(&inst, None, None, &mut 0, &[vec![], vec![Event::PointerMoved(b)], vec![press(b, PointerButton::Primary, true)], vec![press(b, PointerButton::Primary, false)]]);
        assert!(run.load);
        assert!(run.edits.is_empty(), "{:?}", run.edits);
    }

    #[test]
    fn the_sampler_view_reuses_the_common_header() {
        let src = include_str!("sampler_view.rs");
        assert!(!src.contains(concat!("plugin_ai_bar", "(")));
        assert!(!src.contains(concat!("plugin_header", "(")));
    }
}
