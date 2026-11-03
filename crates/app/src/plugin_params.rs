//! The generic controls of a plug-in window (knobs and selectors for the built-in plug-ins, normalized knobs for VST3
//! plug-ins) and the one-line summary of the selected plug-in in the Inspector.

use eframe::egui::{self, RichText};
use ondina_core::command::Command;
use ondina_core::model::*;
use ondina_core::plugins::{self, ParamInfo, PluginInfo};

use crate::app::OndinaApp;
use crate::chrome;
use crate::theme;
use crate::widgets::{self, Icon, Knob};

/// Width of one knob cell.
pub const CELL_W: f32 = 76.0;
pub const KNOB_D: f32 = 40.0;
/// Knobs drawn at most for a VST3 (the filter narrows them).
pub const VST3_SHOWN: usize = 120;

/// A group of consecutive parameters shown under one caption.
#[derive(Debug, Clone, PartialEq)]
pub struct ParamSection {
    pub title: Option<String>,
    /// Index in the plug-in's parameters and the label of its cell.
    pub params: Vec<(usize, String)>,
}

/// How a parameter is drawn.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Control {
    Knob { log: bool, bipolar: bool },
    Segmented,
    Menu,
}

/// What a control asks for; the window turns it into a command.
#[derive(Debug, Clone, PartialEq)]
pub enum ParamEdit {
    /// Part of a knob drag: merged into one undo step by `key`.
    Drag { param: String, value: f32, key: String },
    /// A single change (selector, typed value, double-click default).
    Set { param: String, value: f32 },
}

/// The response flags `knob_edit` reads.
#[derive(Debug, Clone, Copy, Default)]
pub struct KnobFlags {
    pub double_clicked: bool,
    pub drag_started: bool,
    pub dragged: bool,
    pub changed: bool,
}

impl KnobFlags {
    pub fn of(r: &egui::Response) -> Self {
        KnobFlags { double_clicked: r.double_clicked(), drag_started: r.drag_started(), dragged: r.dragged(), changed: r.changed() }
    }
}

/// The leading words `a` and `b` share ("Band 1 Freq", "Band 1 Q" -> "Band", "1").
fn common_words<'a>(a: &'a str, b: &str) -> Vec<&'a str> {
    a.split_whitespace().zip(b.split_whitespace()).take_while(|(x, y)| x == y).map(|(x, _)| x).collect()
}

fn starts_with_words(name: &str, words: &[&str]) -> bool {
    let mut it = name.split_whitespace();
    words.iter().all(|w| it.next() == Some(*w))
}

/// The parameters in registry order, grouped where consecutive names share their first words ("Band 1", "Filter",
/// "Low Shelf"); the cell label drops the shared words. Every parameter shows once, in order.
pub fn param_sections(params: &[ParamInfo]) -> Vec<ParamSection> {
    let mut out: Vec<ParamSection> = Vec::new();
    let mut i = 0;
    while i < params.len() {
        let shared = params.get(i + 1).map(|next| common_words(params[i].name, next.name)).unwrap_or_default();
        if shared.is_empty() {
            let cell = (i, params[i].name.to_string());
            match out.last_mut() {
                Some(s) if s.title.is_none() => s.params.push(cell),
                _ => out.push(ParamSection { title: None, params: vec![cell] }),
            }
            i += 1;
            continue;
        }
        let mut cells = Vec::new();
        let mut j = i;
        while j < params.len() && starts_with_words(params[j].name, &shared) {
            let rest: Vec<&str> = params[j].name.split_whitespace().skip(shared.len()).collect();
            let label = if rest.is_empty() { params[j].name.to_string() } else { rest.join(" ") };
            cells.push((j, label));
            j += 1;
        }
        out.push(ParamSection { title: Some(shared.join(" ")), params: cells });
        i = j;
    }
    out
}

/// A selector of a few short choices is a row of segments; anything longer a menu.
pub fn selector_style(choices: &[&str]) -> Control {
    let short = choices.len() <= 4 && choices.iter().all(|c| c.chars().count() <= 10);
    if short && choices.iter().map(|c| c.chars().count()).sum::<usize>() <= 28 {
        Control::Segmented
    } else {
        Control::Menu
    }
}

/// The control of one parameter: a selector for choices, a knob for the rest.
pub fn param_control(pi: &ParamInfo) -> Control {
    if pi.choices.is_empty() {
        Control::Knob { log: widgets::param_is_log(pi.unit, pi.min, pi.max), bipolar: pi.min < 0.0 && pi.max > 0.0 }
    } else {
        selector_style(pi.choices)
    }
}

/// The control of every parameter of `info`, in order: a selector for choices, a knob for the rest.
pub fn param_controls(info: &PluginInfo) -> Vec<Control> {
    info.params.iter().map(param_control).collect()
}

/// The undo merge key of one knob drag: a new gesture is a new undo step.
pub fn knob_merge_key(pid: Id, param: &str, gesture: u64) -> String {
    format!("pw{pid}:{param}#{gesture}")
}

/// What one frame of a knob asks for. A drag counts a new gesture when it starts, so two drags of the same knob are
/// two undo steps; a double-click (default) or a typed value is a single change.
pub fn knob_edit(pid: Id, param: &str, value: f32, f: KnobFlags, gesture: &mut u64) -> Option<ParamEdit> {
    if f.double_clicked {
        return Some(ParamEdit::Set { param: param.to_string(), value });
    }
    if f.drag_started {
        *gesture += 1;
    }
    if f.changed && f.dragged {
        Some(ParamEdit::Drag { param: param.to_string(), value, key: knob_merge_key(pid, param, *gesture) })
    } else if f.changed {
        Some(ParamEdit::Set { param: param.to_string(), value })
    } else {
        None
    }
}

/// The VST3 parameters whose name contains `filter` (any case), at most `limit`, and how many match in all.
pub fn vst3_visible<'a>(params: &'a [Vst3Param], filter: &str, limit: usize) -> (Vec<&'a Vst3Param>, usize) {
    let f = filter.trim().to_lowercase();
    let matching: Vec<&Vst3Param> = params.iter().filter(|p| f.is_empty() || p.name.to_lowercase().contains(&f)).collect();
    let total = matching.len();
    (matching.into_iter().take(limit).collect(), total)
}

/// Knobs and selectors for every parameter of a built-in plug-in, in sections that wrap with the width.
pub fn builtin_controls(ui: &mut egui::Ui, pid: Id, info: &PluginInfo, inst: &PluginInstance, gesture: &mut u64) -> Vec<ParamEdit> {
    param_knobs(ui, pid, info.params, inst, gesture)
}

/// Knobs and selectors for `params` (a built-in plug-in's parameters or some of them), in sections that wrap with the
/// width.
pub fn param_knobs(ui: &mut egui::Ui, pid: Id, params: &[ParamInfo], inst: &PluginInstance, gesture: &mut u64) -> Vec<ParamEdit> {
    let mut edits = Vec::new();
    for section in param_sections(params) {
        if let Some(title) = &section.title {
            ui.add_space(theme::SP_2);
            chrome::section_label(ui, title);
        }
        ui.horizontal_wrapped(|ui| {
            ui.spacing_mut().item_spacing = egui::vec2(theme::SP_4, theme::SP_4);
            for (idx, label) in &section.params {
                let pi = &params[*idx];
                ui.push_id(pi.id, |ui| {
                    let v = inst.param(pi.id);
                    match param_control(pi) {
                        Control::Knob { log, bipolar } => {
                            let mut x = v;
                            let knob = Knob::new(&mut x, pi.min..=pi.max)
                                .log(log)
                                .bipolar(bipolar)
                                .default_value(pi.default)
                                .unit(pi.unit)
                                .label(label.as_str())
                                .editable(true)
                                .diameter(KNOB_D)
                                .width(CELL_W);
                            let r = ui.add(knob);
                            edits.extend(knob_edit(pid, pi.id, x, KnobFlags::of(&r), gesture));
                        }
                        style => {
                            let last = pi.choices.len().saturating_sub(1);
                            let mut sel = (v.round().max(0.0) as usize).min(last);
                            let before = sel;
                            ui.vertical(|ui| {
                                ui.spacing_mut().item_spacing.y = theme::SP_2;
                                ui.label(theme::label_text(label));
                                if style == Control::Segmented {
                                    widgets::segmented(ui, &mut sel, pi.choices);
                                } else {
                                    let shown = pi.choices.get(sel).copied().unwrap_or("?");
                                    egui::ComboBox::from_id_salt(("pw-choice", pid, pi.id)).selected_text(shown).width(120.0).show_ui(ui, |ui| {
                                        for (ci, c) in pi.choices.iter().enumerate() {
                                            ui.selectable_value(&mut sel, ci, *c);
                                        }
                                    });
                                }
                            });
                            if sel != before {
                                edits.push(ParamEdit::Set { param: pi.id.to_string(), value: sel as f32 });
                            }
                        }
                    }
                });
            }
        });
    }
    edits
}

/// A generic VST3 knob's caption: the parameter's name and, when the plug-in reports one, its unit ("Gain (dB)").
pub fn vst3_knob_label(name: &str, unit: &str) -> String {
    match unit.trim() {
        "" => name.to_string(),
        u => format!("{name} ({u})"),
    }
}

/// The tooltip line of a generic VST3 knob with a unit: the knob shows the normalized value, not the plug-in's.
pub fn vst3_knob_note(unit: &str) -> Option<String> {
    match unit.trim() {
        "" => None,
        u => Some(format!("Normalized 0..1 \u{00B7} the plug-in shows it in {u}")),
    }
}

/// Normalized knobs (0..1) for the parameters a VST3 reported, with a filter by name.
pub fn vst3_controls(ui: &mut egui::Ui, pid: Id, inst: &PluginInstance, filter: &mut String, gesture: &mut u64) -> Vec<ParamEdit> {
    let mut edits = Vec::new();
    if inst.vst3_params.is_empty() {
        ui.label(RichText::new("This plug-in reported no parameters").color(theme::TEXT_3));
        return edits;
    }
    ui.horizontal(|ui| {
        widgets::icon(ui, Icon::Search, 13.0, theme::TEXT_3);
        ui.add(egui::TextEdit::singleline(filter).hint_text("Filter parameters").desired_width(220.0));
    });
    let (shown, total) = vst3_visible(&inst.vst3_params, filter, VST3_SHOWN);
    let count = if shown.len() < total {
        format!("Showing {} of {total} — type to filter", shown.len())
    } else {
        format!("{} of {} parameters", shown.len(), inst.vst3_params.len())
    };
    ui.label(RichText::new(count).small().color(theme::TEXT_DIM));
    ui.horizontal_wrapped(|ui| {
        ui.spacing_mut().item_spacing = egui::vec2(theme::SP_4, theme::SP_4);
        for vp in shown {
            ui.push_id(&vp.id, |ui| {
                let mut x = inst.param(&vp.id);
                let knob = Knob::new(&mut x, 0.0..=1.0)
                    .default_value(vp.default.clamp(0.0, 1.0))
                    .unit("")
                    .label(vst3_knob_label(&vp.name, &vp.unit))
                    .editable(true)
                    .diameter(KNOB_D)
                    .width(CELL_W);
                let knob = match vst3_knob_note(&vp.unit) {
                    Some(note) => knob.note(note),
                    None => knob,
                };
                let r = ui.add(knob);
                edits.extend(knob_edit(pid, &vp.id, x, KnobFlags::of(&r), gesture));
            });
        }
    });
    edits
}

impl OndinaApp {
    /// The selected plug-in in the Inspector: one row with its name and track, Bypass, Open (its window) and a close
    /// button that clears the selection. The plug-in is adjusted in its window.
    pub(crate) fn plugin_summary(&mut self, ui: &mut egui::Ui, p: &Project, pid: Id) {
        let Some(inst) = p.find_plugin(pid).cloned() else {
            return;
        };
        let track = match p.plugin_owner(pid) {
            Some(MASTER_ID) => "Master".to_string(),
            Some(t) => p.track(t).map(|t| t.name.clone()).unwrap_or_default(),
            None => String::new(),
        };
        let row = egui::Frame::none()
            .fill(theme::RAISED)
            .stroke(egui::Stroke::new(1.0_f32, theme::BORDER))
            .rounding(theme::R_XL)
            .inner_margin(theme::SP_3)
            .show(ui, |ui| {
                ui.set_width(ui.available_width());
                ui.horizontal(|ui| {
                    ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                        if widgets::icon_only_button(ui, Icon::Close, 18.0, theme::TEXT_3).on_hover_text("Deselect the plug-in").clicked() {
                            self.selected_plugin = None;
                        }
                        if ui.button("Open").on_hover_text("Open the plug-in's window").clicked() {
                            self.open_plugin(pid);
                        }
                        if ui.add(egui::Button::new("Bypass").selected(inst.bypass)).on_hover_text("Bypass the plug-in").clicked() {
                            self.cmd(Command::SetPluginBypass { plugin_id: pid, bypass: !inst.bypass });
                        }
                        ui.with_layout(egui::Layout::left_to_right(egui::Align::Center), |ui| {
                            let name = RichText::new(plugins::plugin_label(&inst)).font(egui::FontId::new(12.0, theme::semibold())).color(theme::TEXT);
                            ui.add(egui::Label::new(name).truncate());
                            if !track.is_empty() {
                                ui.add(egui::Label::new(RichText::new(format!("· {track}")).color(theme::TEXT_2)).truncate());
                            }
                        });
                    });
                });
            });
        row.response.on_hover_text("Click Open or the plug-in's slot to adjust it in its window");
    }

    /// Turns what the window's controls asked for into commands: a drag merges into one undo step per gesture.
    pub(crate) fn apply_param_edits(&mut self, pid: Id, edits: Vec<ParamEdit>) {
        for e in edits {
            match e {
                ParamEdit::Drag { param, value, key } => self.cmd_merge(Command::SetPluginParam { plugin_id: pid, param, value }, key),
                ParamEdit::Set { param, value } => self.cmd(Command::SetPluginParam { plugin_id: pid, param, value }),
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn titles_and_labels(kind: &str) -> Vec<(Option<String>, Vec<String>)> {
        param_sections(plugins::find(kind).unwrap().params).into_iter().map(|s| (s.title, s.params.into_iter().map(|(_, l)| l).collect())).collect()
    }

    fn strs(v: &[&str]) -> Vec<String> {
        v.iter().map(|s| s.to_string()).collect()
    }

    #[test]
    fn sections_group_bands_and_stages() {
        let deq = titles_and_labels("dynamic_eq");
        assert_eq!(deq.len(), 5);
        for (b, s) in deq.iter().take(4).enumerate() {
            let band = format!("Band {}", b + 1);
            assert_eq!(s.0.as_deref(), Some(band.as_str()));
            assert_eq!(s.1, strs(&[&band, "Type", "Freq", "Gain", "Q", "Threshold", "Range"]));
        }
        assert_eq!(deq[4], (None, strs(&["Attack", "Release", "Output Gain"])));

        let ceq = titles_and_labels("channel_eq");
        let titles: Vec<Option<&str>> = ceq.iter().map(|s| s.0.as_deref()).collect();
        assert_eq!(titles, [None, Some("Low Shelf"), Some("Mid 1"), Some("Mid 2"), Some("High Shelf"), None]);
        assert_eq!(ceq[0].1, strs(&["High Pass", "HP Freq"]));
        assert_eq!(ceq[1].1, strs(&["Gain", "Freq"]));
        assert_eq!(ceq[5].1, strs(&["Low Pass", "LP Freq", "Output Gain"]));

        let synth = titles_and_labels("analog_synth");
        let find = |t: &str| synth.iter().find(|s| s.0.as_deref() == Some(t)).unwrap_or_else(|| panic!("no {t}")).1.clone();
        assert_eq!(find("Osc").len(), 5);
        assert_eq!(find("Filter"), strs(&["Attack", "Decay", "Sustain", "Release"]));
        assert_eq!(find("Amp").len(), 4);
        assert_eq!(find("LFO").len(), 3);
        assert_eq!(titles_and_labels("gain")[1], (Some("Invert".into()), strs(&["Left", "Right"])));

        for info in plugins::registry() {
            let idx: Vec<usize> = param_sections(info.params).into_iter().flat_map(|s| s.params.into_iter().map(|(i, _)| i)).collect();
            assert_eq!(idx, (0..info.params.len()).collect::<Vec<_>>(), "{}", info.kind);
        }
    }

    #[test]
    fn every_registry_parameter_gets_a_control() {
        for info in plugins::registry() {
            let controls = param_controls(info);
            assert_eq!(controls.len(), info.params.len(), "{}", info.kind);
            for (pi, c) in info.params.iter().zip(&controls) {
                match c {
                    Control::Knob { log, .. } => {
                        assert!(pi.choices.is_empty(), "{}: {} has choices", info.kind, pi.id);
                        assert_eq!(*log, widgets::param_is_log(pi.unit, pi.min, pi.max), "{}: {}", info.kind, pi.id);
                    }
                    Control::Segmented | Control::Menu => assert!(!pi.choices.is_empty(), "{}: {}", info.kind, pi.id),
                }
            }
        }
    }

    #[test]
    fn selectors_are_segmented_only_when_short() {
        for c in [&["Off", "On"][..], &["Lowpass", "Highpass", "Bandpass"], &["Bell", "Low Shelf", "High Shelf"], &["Stereo", "Mid", "Side"], &["Soft", "Hard", "Fuzz", "Tube"]] {
            assert_eq!(selector_style(c), Control::Segmented, "{c:?}");
        }
        assert_eq!(selector_style(&["Saw", "Square", "Triangle", "Sine", "Noise"]), Control::Menu);
        assert_eq!(selector_style(&["Quarter", "Eighth", "Sixteenth", "Thirty-second"]), Control::Menu);
        for info in plugins::registry() {
            for pi in info.params.iter().filter(|p| p.id == "rate" || p.id == "stages" || p.id == "chord") {
                if pi.choices.len() > 4 {
                    assert_eq!(selector_style(pi.choices), Control::Menu, "{}: {}", info.kind, pi.id);
                }
            }
        }
    }

    #[test]
    fn each_drag_gets_its_own_merge_key() {
        let mut g = 0;
        let drag = |started| KnobFlags { drag_started: started, dragged: true, changed: true, ..Default::default() };
        let key = |e: Option<ParamEdit>| match e {
            Some(ParamEdit::Drag { key, .. }) => key,
            other => panic!("{other:?}"),
        };
        assert_eq!(key(knob_edit(7, "threshold", -20.0, drag(true), &mut g)), "pw7:threshold#1");
        assert_eq!(key(knob_edit(7, "threshold", -22.0, drag(false), &mut g)), "pw7:threshold#1");
        assert_eq!(knob_edit(7, "threshold", -22.0, KnobFlags::default(), &mut g), None);
        assert_eq!(key(knob_edit(7, "threshold", -25.0, drag(true), &mut g)), "pw7:threshold#2");
        let typed = KnobFlags { changed: true, ..Default::default() };
        assert_eq!(knob_edit(7, "threshold", -6.0, typed, &mut g), Some(ParamEdit::Set { param: "threshold".into(), value: -6.0 }));
        let double = KnobFlags { double_clicked: true, changed: true, ..Default::default() };
        assert_eq!(knob_edit(7, "threshold", -18.0, double, &mut g), Some(ParamEdit::Set { param: "threshold".into(), value: -18.0 }));
        assert_eq!(knob_merge_key(7, "threshold", 2), "pw7:threshold#2");
    }

    fn instance(kind: &str) -> PluginInstance {
        PluginInstance { id: 3, kind: kind.into(), params: Default::default(), bypass: false, preset: None, sample: None, vst3_path: None, vst3_params: vec![], vst3_state: None }
    }

    /// Two frames of `draw` in a headless context; everything the controls asked for.
    fn draw_headless(mut draw: impl FnMut(&mut egui::Ui) -> Vec<ParamEdit>) -> Vec<ParamEdit> {
        let ctx = egui::Context::default();
        theme::apply(&ctx);
        let mut all = Vec::new();
        for k in 0..2 {
            let raw = egui::RawInput {
                screen_rect: Some(egui::Rect::from_min_size(egui::Pos2::ZERO, egui::vec2(640.0, 480.0))),
                time: Some(k as f64 * 0.016),
                ..Default::default()
            };
            let _ = ctx.run(raw, |ctx| {
                egui::CentralPanel::default().show(ctx, |ui| {
                    egui::ScrollArea::vertical().show(ui, |ui| all.extend(draw(ui)));
                });
            });
        }
        all
    }

    #[test]
    fn every_builtin_plugin_draws_without_edits() {
        for info in plugins::registry() {
            let inst = instance(info.kind);
            let mut g = 0;
            let edits = draw_headless(|ui| builtin_controls(ui, 3, info, &inst, &mut g));
            assert!(edits.is_empty(), "{}: {edits:?}", info.kind);
            assert_eq!(g, 0);
        }
    }

    #[test]
    fn param_knobs_draw_only_the_parameters_given() {
        let info = plugins::find("drum_synth").unwrap();
        let subset: Vec<ParamInfo> = info.params.iter().filter(|p| p.id.starts_with("snare_")).copied().collect();
        assert_eq!(subset.len(), 3);
        let inst = instance("drum_synth");
        let mut g = 0;
        assert!(draw_headless(|ui| param_knobs(ui, 3, &subset, &inst, &mut g)).is_empty());
        assert_eq!(g, 0);
        for kind in ["drum_synth", "analog_synth"] {
            let info = plugins::find(kind).unwrap();
            assert_eq!(param_controls(info), info.params.iter().map(param_control).collect::<Vec<_>>(), "{kind}");
        }
    }

    #[test]
    fn vst3_filter_and_limit() {
        let mut params = Vec::new();
        for i in 1..=150 {
            params.push(Vst3Param { id: format!("p{i}"), name: format!("Gain {i}"), default: 0.5, unit: String::new() });
        }
        for i in 1..=150 {
            params.push(Vst3Param { id: format!("p{}", 150 + i), name: format!("Cut {i}"), default: 0.5, unit: String::new() });
        }
        let count = |f: &str| {
            let (shown, total) = vst3_visible(&params, f, VST3_SHOWN);
            (shown.len(), total)
        };
        assert_eq!(count(""), (120, 300));
        assert_eq!(count("cut"), (120, 150));
        let (shown, total) = vst3_visible(&params, "CUT 15", VST3_SHOWN);
        assert_eq!(total, 2);
        assert_eq!(shown.iter().map(|p| p.name.as_str()).collect::<Vec<_>>(), ["Cut 15", "Cut 150"]);
        assert_eq!(count("zzz"), (0, 0));

        let mut inst = instance("vst3");
        inst.vst3_params = params;
        let mut filter = String::new();
        let mut g = 0;
        assert!(draw_headless(|ui| vst3_controls(ui, 3, &inst, &mut filter, &mut g)).is_empty());
    }

    #[test]
    fn vst3_knobs_name_the_reported_unit() {
        assert_eq!(vst3_knob_label("Gain", "dB"), "Gain (dB)");
        assert_eq!(vst3_knob_label("Mix", ""), "Mix");
        assert_eq!(vst3_knob_label("Mix", "  "), "Mix");
        assert_eq!(vst3_knob_note("dB").as_deref(), Some("Normalized 0..1 \u{00B7} the plug-in shows it in dB"));
        assert_eq!(vst3_knob_note(""), None);
    }
}
