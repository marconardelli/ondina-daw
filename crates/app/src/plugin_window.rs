//! Plug-in windows: every plug-in opens in a window of its own (a separate egui viewport, or a floating window
//! inside the main one when separate windows are off), with a common header and its controls.
//!
//! What decides which windows are open, where they go and when they close is plain logic ([`PluginWindows`] and the
//! free functions) that the tests cover; the app part only reads `self.view` and queues commands.

use std::collections::HashMap;

use eframe::egui::{self, Pos2, RichText, Vec2};
use ondina_core::command::Command;
use ondina_core::model::*;
use ondina_core::plugins::{self, PluginCategory, PluginInfo};
use ondina_studio::{Source, WindowRect};

use crate::app::OndinaApp;
use crate::plugin_params::{builtin_controls, vst3_controls};
use crate::{chrome, theme};

/// Most plug-in windows open at once: past this the least recently used one closes.
pub const MAX_PLUGIN_WINDOWS: usize = 8;
/// Least inner size of a plug-in window.
pub const MIN_SIZE: [f32; 2] = [360.0, 240.0];

#[derive(Debug, Clone)]
pub struct OpenWindow {
    pub id: Id,
    /// Where it opened (decided at its first frame); the builder keeps it, so the window can be moved freely.
    pub initial: Option<WindowRect>,
    /// Why the native editor of a VST3 is not used (not loaded, no editor, could not open), shown in the header.
    pub native_note: Option<String>,
}

/// What a click on a slot did to the windows.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Opened {
    /// A new window; `closed` is the one that made room for it.
    New { closed: Option<Id> },
    /// The window was already open: it comes to the front.
    Focused,
}

/// The open plug-in windows and what they remember during this app session.
#[derive(Default)]
pub struct PluginWindows {
    /// Least recently focused first.
    open: Vec<OpenWindow>,
    /// Where each window was in this app session, by plug-in id.
    rects: HashMap<Id, WindowRect>,
    /// Windows to bring to the front this frame.
    focus: Vec<Id>,
    /// `View::document` the windows belong to.
    document: u64,
    /// Knob drag counter: every drag is its own undo step (see `knob_edit`).
    pub gesture: u64,
    /// The VST3 parameter filter of each window.
    pub filters: HashMap<Id, String>,
}

impl PluginWindows {
    pub fn new(document: u64) -> Self {
        PluginWindows { document, ..Default::default() }
    }

    /// Opens the window of `id`, or brings it to the front when it is open (updating its note when one is given).
    pub fn open(&mut self, id: Id, note: Option<String>) -> Opened {
        if let Some(i) = self.open.iter().position(|w| w.id == id) {
            let mut w = self.open.remove(i);
            if note.is_some() {
                w.native_note = note;
            }
            self.open.push(w);
            self.focus.push(id);
            return Opened::Focused;
        }
        let closed = if self.open.len() >= MAX_PLUGIN_WINDOWS {
            let old = self.open.remove(0).id;
            self.filters.remove(&old);
            Some(old)
        } else {
            None
        };
        self.open.push(OpenWindow { id, initial: None, native_note: note });
        Opened::New { closed }
    }

    pub fn close(&mut self, id: Id) {
        self.open.retain(|w| w.id != id);
        self.filters.remove(&id);
    }

    pub fn is_open(&self, id: Id) -> bool {
        self.open.iter().any(|w| w.id == id)
    }

    pub fn ids(&self) -> Vec<Id> {
        self.open.iter().map(|w| w.id).collect()
    }

    pub fn window(&self, id: Id) -> Option<&OpenWindow> {
        self.open.iter().find(|w| w.id == id)
    }

    /// The window with the focus goes last: the least recently used one closes first.
    pub fn touch(&mut self, id: Id) {
        if let Some(i) = self.open.iter().position(|w| w.id == id) {
            if i + 1 != self.open.len() {
                let w = self.open.remove(i);
                self.open.push(w);
            }
        }
    }

    pub fn take_focus(&mut self) -> Vec<Id> {
        std::mem::take(&mut self.focus)
    }

    /// Closes the windows whose plug-in is gone (removed, its addition undone, the instrument replaced).
    pub fn prune(&mut self, project: &Project) -> Vec<Id> {
        let gone: Vec<Id> = self.open.iter().map(|w| w.id).filter(|id| project.find_plugin(*id).is_none()).collect();
        for id in &gone {
            self.close(*id);
        }
        gone
    }

    /// Another document (opened, new, recovered): its plug-in ids are other plug-ins, so every window closes.
    pub fn follow_document(&mut self, document: u64) {
        if document != self.document {
            self.document = document;
            self.open.clear();
            self.rects.clear();
            self.filters.clear();
            self.focus.clear();
        }
    }

    pub fn remember(&mut self, id: Id, r: WindowRect) {
        self.rects.insert(id, r);
    }

    pub fn rect(&self, id: Id) -> Option<WindowRect> {
        self.rects.get(&id).copied()
    }

    pub fn initial(&self, id: Id) -> Option<WindowRect> {
        self.window(id).and_then(|w| w.initial)
    }

    pub fn set_initial(&mut self, id: Id, r: WindowRect) {
        if let Some(w) = self.open.iter_mut().find(|w| w.id == id) {
            w.initial = Some(r);
        }
    }
}

/// The viewport of the window of plug-in `id`.
pub fn viewport_id(id: Id) -> egui::ViewportId {
    egui::ViewportId::from_hash_of(("plugin-window", id))
}

/// "Compressor — Vox", "Limiter — Master"; None when the plug-in is gone.
pub fn window_title(project: &Project, id: Id) -> Option<String> {
    let inst = project.find_plugin(id)?;
    let owner = match project.plugin_owner(id)? {
        MASTER_ID => "Master".to_string(),
        t => project.track(t).map(|t| t.name.clone()).unwrap_or_default(),
    };
    Some(format!("{} \u{2014} {owner}", plugins::plugin_label(inst)))
}

/// Under which name the settings keep a window's place: the built-in kind, or the VST3 bundle.
pub fn settings_key(inst: &PluginInstance) -> String {
    if inst.kind == "vst3" {
        format!("vst3:{}", inst.vst3_path.as_deref().unwrap_or_default())
    } else {
        inst.kind.clone()
    }
}

/// The size a window opens at the first time: instruments have more to show than effects.
pub fn default_size(inst: &PluginInstance) -> Vec2 {
    if crate::eq_view::has_eq_view(&inst.kind) {
        return egui::vec2(680.0, 640.0);
    }
    match plugins::find(&inst.kind).map(|i| i.category) {
        Some(PluginCategory::Instrument) => egui::vec2(680.0, 600.0),
        Some(_) => egui::vec2(560.0, 440.0),
        None => egui::vec2(640.0, 560.0),
    }
}

/// Whether a remembered rectangle can be used: finite, a sensible size, and not far away from the main window (a
/// monitor that is no longer there).
pub fn plausible(r: WindowRect, main: Option<egui::Rect>) -> bool {
    let finite = [r.x, r.y, r.w, r.h].iter().all(|v| v.is_finite());
    let sized = (200.0..=8000.0).contains(&r.w) && (200.0..=8000.0).contains(&r.h);
    let near = main.is_none_or(|m| {
        let c = egui::pos2(r.x + r.w / 2.0, r.y + r.h / 2.0);
        c.distance(m.center()) <= 10000.0
    });
    finite && sized && near
}

/// Where a window opens and how big: where it was in this app session, else where the settings remember it, else
/// the default size cascaded from the main window's corner (`cascade` is how many windows are open before it).
pub fn initial_rect(session: Option<WindowRect>, saved: Option<WindowRect>, main: Option<egui::Rect>, cascade: usize, size: Vec2) -> (Option<Pos2>, Vec2) {
    if let Some(r) = session.filter(|r| plausible(*r, main)).or(saved.filter(|r| plausible(*r, main))) {
        return (Some(egui::pos2(r.x, r.y)), egui::vec2(r.w, r.h));
    }
    let k = (cascade % 8) as f32;
    (main.map(|m| m.min + egui::vec2(120.0 + 28.0 * k, 90.0 + 28.0 * k)), size)
}

/// Esc closes the focused plug-in window, unless it is cancelling the typing of a value or closing a menu.
pub fn esc_closes(esc: bool, typing: bool, popup: bool) -> bool {
    esc && !typing && !popup
}

/// What a click on a plug-in slot opens.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SlotClick {
    /// The VST3's own editor window.
    Native,
    /// Ondina's plug-in window.
    Window,
}

/// A loaded VST3 opens its own editor; everything else Ondina's window.
pub fn slot_click(kind: &str, unloaded: bool) -> SlotClick {
    if kind == "vst3" && !unloaded {
        SlotClick::Native
    } else {
        SlotClick::Window
    }
}

/// The main window's shortcuts that also work in a plug-in window (each window gets its own keys).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum WindowKey {
    PlayStop,
    Undo,
    Redo,
}

/// Space, Ctrl+Z, Ctrl+Shift+Z and Ctrl+Y, consumed. Call it only while nothing is being typed.
pub fn window_shortcut(i: &mut egui::InputState) -> Option<WindowKey> {
    use egui::{Key, Modifiers};
    // Ctrl+Shift+Z first: a Ctrl+Z pattern also matches it
    if i.consume_key(Modifiers::COMMAND | Modifiers::SHIFT, Key::Z) || i.consume_key(Modifiers::COMMAND, Key::Y) {
        Some(WindowKey::Redo)
    } else if i.consume_key(Modifiers::COMMAND, Key::Z) {
        Some(WindowKey::Undo)
    } else if i.consume_key(Modifiers::NONE, Key::Space) {
        Some(WindowKey::PlayStop)
    } else {
        None
    }
}

/// What a window reported this frame.
#[derive(Debug, Clone, Copy, PartialEq, Default)]
pub struct WindowOutcome {
    /// Closed with its X or with Esc.
    pub close: bool,
    /// Where it is now (separate windows only).
    pub rect: Option<WindowRect>,
    /// It has the keyboard focus.
    pub focused: bool,
}

/// Title, position and size of a window as it opens.
#[derive(Debug, Clone, Copy)]
pub struct WindowSpec<'a> {
    pub title: &'a str,
    pub pos: Option<Pos2>,
    pub size: Vec2,
}

/// A plug-in window inside the main window: closed with its X.
fn embedded_window(ctx: &egui::Context, id: Id, spec: &WindowSpec, add: &mut dyn FnMut(&mut egui::Ui)) -> WindowOutcome {
    let mut open = true;
    let mut w = egui::Window::new(spec.title)
        .id(egui::Id::new(("plugin-window", id)))
        .open(&mut open)
        .resizable(true)
        .default_size(spec.size)
        .min_size(MIN_SIZE);
    if let Some(p) = spec.pos {
        w = w.default_pos(p);
    }
    w.show(ctx, |ui| add(ui));
    WindowOutcome { close: !open, rect: None, focused: false }
}

/// Shows the window of plug-in `id` for one frame, drawing its contents with `add`. With `separate` it is a viewport
/// of its own (when the backend has them); otherwise it floats in the main window.
pub fn show_window(ctx: &egui::Context, separate: bool, id: Id, spec: WindowSpec, mut add: impl FnMut(&mut egui::Ui)) -> WindowOutcome {
    if !separate {
        return embedded_window(ctx, id, &spec, &mut add);
    }
    // position and size are the ones it opened with: egui sends a command only when the builder changes, so the
    // window stays where the person moves it
    let mut builder = egui::ViewportBuilder::default()
        .with_title(spec.title)
        .with_inner_size(spec.size)
        .with_min_inner_size(MIN_SIZE)
        .with_resizable(true);
    if let Some(p) = spec.pos {
        builder = builder.with_position(p);
    }
    ctx.show_viewport_immediate(viewport_id(id), builder, |vctx, class| {
        if class == egui::ViewportClass::Embedded {
            return embedded_window(vctx, id, &spec, &mut add);
        }
        // egui drops the focus of a field on Esc before anything is drawn: a field that had it last frame means the
        // Esc cancels the typing
        let typing_key = egui::Id::new(("plugin-window-typing", id));
        let typed_last_frame = vctx.data(|d| d.get_temp::<bool>(typing_key)).unwrap_or(false);
        let typing = typed_last_frame || vctx.wants_keyboard_input();
        let popup = vctx.memory(|m| m.any_popup_open());
        let esc = vctx.input(|i| i.key_pressed(egui::Key::Escape));
        let close_requested = vctx.input(|i| i.viewport().close_requested());
        egui::CentralPanel::default()
            .frame(egui::Frame::none().fill(theme::PANEL_BG).inner_margin(theme::SP_5))
            .show(vctx, |ui| add(ui));
        let typing_now = vctx.wants_keyboard_input();
        vctx.data_mut(|d| d.insert_temp(typing_key, typing_now));
        let (rect, focused) = vctx.input(|i| {
            let vp = i.viewport();
            let rect = match (vp.outer_rect, vp.inner_rect) {
                (Some(o), Some(inner)) => Some(WindowRect { x: o.min.x, y: o.min.y, w: inner.width(), h: inner.height() }),
                _ => None,
            };
            (rect, vp.focused == Some(true))
        });
        WindowOutcome { close: close_requested || esc_closes(esc, typing, popup), rect, focused }
    })
}

/// The view a plug-in window shows under the header: the EQ graph above the knobs for the equalizers. The next slices add dynamics and synth.
enum PluginView {
    Generic(&'static PluginInfo),
    Eq(&'static PluginInfo),
    Vst3,
    Unknown,
}

impl OndinaApp {
    /// A click on a plug-in's slot: a loaded VST3 opens its own editor, everything else (and a VST3 whose editor
    /// cannot open) Ondina's plug-in window; an open window comes to the front.
    pub(crate) fn open_plugin(&mut self, pid: Id) {
        self.selected_plugin = Some(pid);
        let Some(inst) = self.view.project.find_plugin(pid) else {
            return;
        };
        let unloaded = self.view.unloaded_vst3.get(&pid).cloned();
        let note = match slot_click(&inst.kind, unloaded.is_some()) {
            SlotClick::Native => match self.exec_with(|s| s.execute(Command::OpenPluginEditor { plugin_id: pid }, Source::Gui)) {
                None | Some(Ok(_)) => return,
                Some(Err(e)) => {
                    self.status = format!("\u{26A0} {e}: showing Ondina's controls");
                    Some(e)
                }
            },
            SlotClick::Window => unloaded.map(|r| format!("Not loaded: {r}")),
        };
        self.open_window(pid, note);
    }

    /// "Generic controls" of a VST3: Ondina's window even when the plug-in has its own.
    pub(crate) fn open_generic_window(&mut self, pid: Id) {
        self.selected_plugin = Some(pid);
        self.open_window(pid, None);
    }

    fn open_window(&mut self, pid: Id, note: Option<String>) {
        if let Opened::New { closed: Some(_) } = self.plugin_windows.open(pid, note) {
            self.status = format!("Closed the oldest plug-in window ({MAX_PLUGIN_WINDOWS} open at most)");
        }
    }

    /// Draws every open plug-in window; called once per frame from the main window, after its panels.
    pub(crate) fn plugin_windows_ui(&mut self, ctx: &egui::Context) {
        self.plugin_windows.follow_document(self.view.document);
        let p = self.project();
        self.plugin_windows.prune(&p);
        let main = ctx.input(|i| i.viewport().outer_rect);
        let separate = self.settings.plugin_windows_separate;
        for (k, id) in self.plugin_windows.ids().into_iter().enumerate() {
            let (Some(title), Some(inst)) = (window_title(&p, id), p.find_plugin(id)) else {
                continue;
            };
            let key = settings_key(inst);
            let initial = match self.plugin_windows.initial(id) {
                Some(r) => r,
                None => {
                    let saved = self.settings.plugin_windows.get(&key).copied();
                    let (pos, size) = initial_rect(self.plugin_windows.rect(id), saved, main, k, default_size(inst));
                    let pos = pos.unwrap_or(Pos2::new(f32::NAN, f32::NAN));
                    let r = WindowRect { x: pos.x, y: pos.y, w: size.x, h: size.y };
                    self.plugin_windows.set_initial(id, r);
                    r
                }
            };
            let pos = (initial.x.is_finite() && initial.y.is_finite()).then(|| egui::pos2(initial.x, initial.y));
            let spec = WindowSpec { title: &title, pos, size: egui::vec2(initial.w, initial.h) };
            let out = show_window(ctx, separate, id, spec, |ui| self.plugin_window_body(ui, &p, id));
            if out.close {
                self.plugin_windows.close(id);
                continue;
            }
            if out.focused {
                self.plugin_windows.touch(id);
            }
            if let Some(r) = out.rect {
                self.plugin_windows.remember(id, r);
                self.settings.remember_plugin_window(&key, r);
            }
        }
        for id in self.plugin_windows.take_focus() {
            if separate {
                ctx.send_viewport_cmd_to(viewport_id(id), egui::ViewportCommand::Minimized(false));
                ctx.send_viewport_cmd_to(viewport_id(id), egui::ViewportCommand::Focus);
            } else {
                ctx.move_to_top(egui::LayerId::new(egui::Order::Middle, egui::Id::new(("plugin-window", id))));
            }
        }
    }

    /// Header, a line and the plug-in's controls.
    fn plugin_window_body(&mut self, ui: &mut egui::Ui, p: &Project, id: Id) {
        let Some(inst) = p.find_plugin(id).cloned() else {
            return;
        };
        // two windows of the same kind draw the same widgets: their ids (and the text being typed) must differ
        ui.push_id(("plugin-window", id), |ui| {
            // a window inside the main one shares its keys, which the main window already handled this frame
            let own_keys = ui.ctx().viewport_id() != egui::ViewportId::ROOT;
            if own_keys && !ui.ctx().wants_keyboard_input() {
                match ui.input_mut(window_shortcut) {
                    Some(WindowKey::PlayStop) => self.toggle_play(),
                    Some(WindowKey::Undo) => self.cmd(Command::Undo {}),
                    Some(WindowKey::Redo) => self.cmd(Command::Redo {}),
                    None => {}
                }
            }
            self.plugin_header(ui, p, &inst);
            ui.add_space(theme::SP_3);
            let (rect, _) = ui.allocate_exact_size(egui::vec2(ui.available_width(), 1.0), egui::Sense::hover());
            ui.painter().hline(rect.x_range(), rect.center().y, egui::Stroke::new(1.0_f32, theme::CONTROL));
            ui.add_space(theme::SP_3);
            let view = match plugins::find(&inst.kind) {
                Some(info) if crate::eq_view::has_eq_view(info.kind) => PluginView::Eq(info),
                Some(info) => PluginView::Generic(info),
                None if inst.kind == "vst3" => PluginView::Vst3,
                None => PluginView::Unknown,
            };
            if let PluginView::Eq(_) = view {
                // the graph stays above the scrolling knobs, so the wheel over a point never scrolls them
                let sr = crate::eq_view::curve_sample_rate(self.view.sample_rate);
                let height = crate::eq_view::graph_height(ui.available_height());
                let out = crate::eq_view::eq_graph(ui, &inst, sr, height, &mut self.plugin_windows.gesture);
                self.apply_eq_edits(id, out.edits);
                ui.add_space(theme::SP_3);
            }
            let edits = egui::ScrollArea::vertical()
                .auto_shrink([false, false])
                .show(ui, |ui| match view {
                    PluginView::Generic(info) | PluginView::Eq(info) => builtin_controls(ui, id, info, &inst, &mut self.plugin_windows.gesture),
                    PluginView::Vst3 => {
                        let mut filter = self.plugin_windows.filters.get(&id).cloned().unwrap_or_default();
                        let edits = vst3_controls(ui, id, &inst, &mut filter, &mut self.plugin_windows.gesture);
                        self.plugin_windows.filters.insert(id, filter);
                        edits
                    }
                    PluginView::Unknown => {
                        let text = format!("Unknown plug-in kind '{}': saved by a newer version of Ondina", inst.kind);
                        ui.label(RichText::new(text).color(theme::TEXT_3));
                        Vec::new()
                    }
                })
                .inner;
            self.apply_param_edits(id, edits);
        });
    }

    /// The header every plug-in window shares: track color, name and track, Bypass, presets, Default, the VST3's own
    /// window, and the AI bar.
    fn plugin_header(&mut self, ui: &mut egui::Ui, p: &Project, inst: &PluginInstance) {
        let pid = inst.id;
        let info = plugins::find(&inst.kind);
        let owner = p.plugin_owner(pid);
        let track = owner.filter(|o| *o != MASTER_ID).and_then(|t| p.track(t));
        let track_name = match owner {
            Some(MASTER_ID) => "Master".to_string(),
            _ => track.map(|t| t.name.clone()).unwrap_or_default(),
        };
        let unloaded = self.view.unloaded_vst3.get(&pid).cloned();
        let defaults = plugins::default_params(inst);
        ui.horizontal(|ui| {
            ui.spacing_mut().item_spacing.x = theme::SP_3;
            if let Some(t) = track {
                chrome::color_dot(ui, theme::rgb(t.color), 10.0);
            }
            let name = ui.label(RichText::new(plugins::plugin_label(inst)).font(theme::title_font()).color(theme::TEXT));
            if let Some(info) = info {
                name.on_hover_text(info.description);
            }
            ui.label(RichText::new(format!("\u{2014} {track_name}")).color(theme::TEXT_2));
            if inst.bypass {
                chrome::status_pill(ui, "Bypassed", theme::TEXT_3, theme::RAISED, theme::TEXT_2);
            }
            ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                if ui.add(egui::Button::new("Bypass").selected(inst.bypass)).on_hover_text("Bypass the plug-in").clicked() {
                    self.cmd(Command::SetPluginBypass { plugin_id: pid, bypass: !inst.bypass });
                }
                let presets: Vec<_> = plugins::presets_for(&inst.kind).collect();
                if !presets.is_empty() {
                    egui::ComboBox::from_id_salt(("pw-preset", pid))
                        .selected_text(inst.preset.clone().unwrap_or("Preset\u{2026}".into()))
                        .show_ui(ui, |ui| {
                            for pr in presets {
                                if ui.selectable_label(inst.preset.as_deref() == Some(pr.name), pr.name).clicked() {
                                    self.cmd(Command::LoadPreset { plugin_id: pid, preset: pr.name.to_string() });
                                }
                            }
                        });
                }
                let default = ui
                    .add_enabled(!defaults.is_empty(), egui::Button::new("Default"))
                    .on_hover_text("Set every parameter back to its default (one undo step)");
                if default.clicked() {
                    self.cmd(Command::SetPluginParams { plugin_id: pid, params: defaults.clone() });
                }
                if inst.kind == "vst3" {
                    let open = ondina_dsp::vst3::open_editors().contains(&pid);
                    let clicked = match &unloaded {
                        Some(reason) if !open => ui.add_enabled(false, egui::Button::new("Native window")).on_disabled_hover_text(reason).clicked(),
                        _ => ui
                            .button(if open { "Close native window" } else { "Native window" })
                            .on_hover_text("The plug-in's own window")
                            .clicked(),
                    };
                    if clicked {
                        if open {
                            self.cmd(Command::ClosePluginEditor { plugin_id: pid });
                        } else {
                            self.exec(Command::OpenPluginEditor { plugin_id: pid });
                        }
                    }
                }
            });
        });
        if inst.kind == "vst3" {
            ui.label(RichText::new(inst.vst3_path.clone().unwrap_or_default()).small().color(theme::TEXT_DIM));
        }
        let note = self.plugin_windows.window(pid).and_then(|w| w.native_note.clone()).or(unloaded.map(|r| format!("Not loaded: {r}")));
        if let Some(note) = note {
            ui.label(RichText::new(note).small().color(theme::REC));
        }
        self.plugin_ai_bar(ui, p, pid);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use ondina_studio::Session;
    use serde_json::json;

    fn run(s: &mut Session, name: &str, args: serde_json::Value) -> serde_json::Value {
        let cmd = Command::from_tool_call(name, &args).unwrap();
        s.execute(cmd, Source::Gui).unwrap_or_else(|e| panic!("{name}: {e}"))
    }

    fn id_of(v: &serde_json::Value, key: &str) -> Id {
        v[key].as_u64().unwrap()
    }

    #[test]
    fn opening_twice_focuses_instead_of_duplicating() {
        let mut w = PluginWindows::new(1);
        assert_eq!(w.open(5, None), Opened::New { closed: None });
        assert_eq!(w.open(5, None), Opened::Focused);
        assert_eq!(w.ids(), [5]);
        assert_eq!(w.take_focus(), [5]);
        assert!(w.take_focus().is_empty());
    }

    #[test]
    fn three_windows_stay_open_together() {
        let mut w = PluginWindows::new(1);
        for id in [1, 2, 3] {
            w.open(id, None);
        }
        assert_eq!(w.ids(), [1, 2, 3]);
        assert!([1, 2, 3].iter().all(|id| w.is_open(*id)));
        w.open(1, None);
        assert_eq!(w.ids(), [2, 3, 1]);
        w.touch(3);
        assert_eq!(w.ids(), [2, 1, 3]);
    }

    #[test]
    fn the_oldest_window_closes_past_the_limit() {
        let mut w = PluginWindows::new(1);
        for id in 1..=MAX_PLUGIN_WINDOWS as Id {
            assert_eq!(w.open(id, None), Opened::New { closed: None });
        }
        assert_eq!(w.open(100, None), Opened::New { closed: Some(1) });
        assert_eq!(w.ids().len(), MAX_PLUGIN_WINDOWS);
        assert!(!w.is_open(1));
    }

    #[test]
    fn windows_close_when_their_plugin_is_removed_or_undone() {
        let mut s = Session::new(false);
        let t = id_of(&run(&mut s, "add_track", json!({"kind": "audio", "name": "Vox"})), "track_id");
        let a = id_of(&run(&mut s, "add_effect", json!({"track_id": t, "kind": "compressor"})), "plugin_id");
        let b = id_of(&run(&mut s, "add_effect", json!({"track_id": t, "kind": "reverb"})), "plugin_id");
        let mut w = PluginWindows::new(s.document);
        w.open(a, None);
        w.open(b, None);
        w.filters.insert(a, "x".into());
        assert!(w.prune(&s.project).is_empty());
        run(&mut s, "remove_plugin", json!({"plugin_id": a}));
        assert_eq!(w.prune(&s.project), [a]);
        assert_eq!(w.ids(), [b]);
        assert!(!w.filters.contains_key(&a));
        let c = id_of(&run(&mut s, "add_effect", json!({"track_id": t, "kind": "chorus"})), "plugin_id");
        w.open(c, None);
        run(&mut s, "undo", json!({}));
        assert_eq!(w.prune(&s.project), [c]);
        assert_eq!(w.ids(), [b]);
    }

    #[test]
    fn a_new_document_closes_every_window() {
        let mut w = PluginWindows::new(1);
        w.open(4, None);
        w.open(5, None);
        w.remember(4, WindowRect { x: 1.0, y: 2.0, w: 400.0, h: 300.0 });
        w.follow_document(1);
        assert_eq!(w.ids(), [4, 5]);
        w.follow_document(2);
        assert!(w.ids().is_empty());
        assert_eq!(w.rect(4), None);
    }

    #[test]
    fn window_title_names_plugin_and_track() {
        let mut s = Session::new(false);
        let t = id_of(&run(&mut s, "add_track", json!({"kind": "audio", "name": "Vox"})), "track_id");
        let fx = id_of(&run(&mut s, "add_effect", json!({"track_id": t, "kind": "compressor"})), "plugin_id");
        let lim = id_of(&run(&mut s, "add_effect", json!({"track_id": MASTER_ID, "kind": "limiter"})), "plugin_id");
        let keys = id_of(&run(&mut s, "add_track", json!({"kind": "instrument", "name": "Keys", "instrument": "fm_synth"})), "track_id");
        let inst = s.project.track(keys).unwrap().instrument.as_ref().unwrap().id;
        assert_eq!(window_title(&s.project, fx).as_deref(), Some("Compressor \u{2014} Vox"));
        assert_eq!(window_title(&s.project, lim).as_deref(), Some("Limiter \u{2014} Master"));
        let name = plugins::find("fm_synth").unwrap().name;
        assert_eq!(window_title(&s.project, inst), Some(format!("{name} \u{2014} Keys")));
        assert_eq!(window_title(&s.project, 999_999), None);
    }

    fn instance(kind: &str, path: Option<&str>) -> PluginInstance {
        PluginInstance { id: 1, kind: kind.into(), params: Default::default(), bypass: false, preset: None, sample: None, vst3_path: path.map(Into::into), vst3_params: vec![], vst3_state: None }
    }

    #[test]
    fn settings_keys_tell_vst3_bundles_apart() {
        assert_eq!(settings_key(&instance("compressor", None)), "compressor");
        let a = settings_key(&instance("vst3", Some(r"C:\VST3\A.vst3")));
        let b = settings_key(&instance("vst3", Some(r"C:\VST3\B.vst3")));
        assert_ne!(a, b);
        assert!(a.starts_with("vst3:"));
        assert_eq!(default_size(&instance("analog_synth", None)), egui::vec2(680.0, 600.0));
        assert_eq!(default_size(&instance("compressor", None)), egui::vec2(560.0, 440.0));
        assert_eq!(default_size(&instance("vst3", None)), egui::vec2(640.0, 560.0));
    }

    #[test]
    fn initial_rect_prefers_session_then_settings_then_cascade() {
        let main = Some(egui::Rect::from_min_size(egui::pos2(100.0, 50.0), egui::vec2(1600.0, 900.0)));
        let size = egui::vec2(560.0, 440.0);
        let session = WindowRect { x: 300.0, y: 200.0, w: 500.0, h: 400.0 };
        let saved = WindowRect { x: 400.0, y: 250.0, w: 600.0, h: 450.0 };
        assert_eq!(initial_rect(Some(session), Some(saved), main, 0, size), (Some(egui::pos2(300.0, 200.0)), egui::vec2(500.0, 400.0)));
        assert_eq!(initial_rect(None, Some(saved), main, 0, size), (Some(egui::pos2(400.0, 250.0)), egui::vec2(600.0, 450.0)));
        let no_size = WindowRect { w: 0.0, ..saved };
        let far = WindowRect { x: 50000.0, ..saved };
        assert_eq!(initial_rect(None, Some(no_size), main, 0, size), (Some(egui::pos2(220.0, 140.0)), size));
        assert_eq!(initial_rect(None, Some(far), main, 1, size), (Some(egui::pos2(248.0, 168.0)), size));
        assert_eq!(initial_rect(None, None, None, 0, size), (None, size));
        let nan = WindowRect { x: f32::NAN, ..saved };
        assert!(!plausible(nan, None));
    }

    #[test]
    fn escape_closes_only_when_not_typing() {
        assert!(esc_closes(true, false, false));
        assert!(!esc_closes(true, true, false));
        assert!(!esc_closes(true, false, true));
        assert!(!esc_closes(false, false, false));
    }

    #[test]
    fn vst3_click_opens_native_only_when_loaded() {
        assert_eq!(slot_click("vst3", false), SlotClick::Native);
        assert_eq!(slot_click("vst3", true), SlotClick::Window);
        assert_eq!(slot_click("compressor", false), SlotClick::Window);
    }

    #[test]
    fn viewport_ids_differ_per_plugin() {
        assert_ne!(viewport_id(1), viewport_id(2));
        assert_ne!(viewport_id(1), egui::ViewportId::ROOT);
    }

    #[test]
    fn window_shortcuts_match_the_main_window() {
        use egui::{Event, Key, Modifiers};
        let ctx = egui::Context::default();
        let press = |key, modifiers| Event::Key { key, physical_key: None, pressed: true, repeat: false, modifiers };
        let cases = [
            (press(Key::Space, Modifiers::NONE), Some(WindowKey::PlayStop)),
            (press(Key::Z, Modifiers::COMMAND), Some(WindowKey::Undo)),
            (press(Key::Z, Modifiers::COMMAND | Modifiers::SHIFT), Some(WindowKey::Redo)),
            (press(Key::Y, Modifiers::COMMAND), Some(WindowKey::Redo)),
            (press(Key::A, Modifiers::NONE), None),
        ];
        for (k, (event, want)) in cases.into_iter().enumerate() {
            let raw = egui::RawInput { time: Some(k as f64 * 0.016), events: vec![event.clone()], ..Default::default() };
            let mut got = None;
            let _ = ctx.run(raw, |ctx| got = ctx.input_mut(window_shortcut));
            assert_eq!(got, want, "{event:?}");
        }
    }

    #[test]
    fn windows_fall_back_inside_the_main_window() {
        let ctx = egui::Context::default();
        for separate in [true, false] {
            let mut calls = 0;
            for k in 0..3 {
                let raw = egui::RawInput {
                    screen_rect: Some(egui::Rect::from_min_size(egui::Pos2::ZERO, egui::vec2(1200.0, 800.0))),
                    time: Some(k as f64 * 0.016),
                    ..Default::default()
                };
                let mut out = WindowOutcome::default();
                let _ = ctx.run(raw, |ctx| {
                    let spec = WindowSpec { title: "Compressor \u{2014} Vox", pos: None, size: egui::vec2(560.0, 440.0) };
                    out = show_window(ctx, separate, 7, spec, |ui| {
                        calls += 1;
                        ui.label("knobs");
                    });
                });
                assert!(!out.close);
                assert_eq!(out.rect, None);
            }
            assert_eq!(calls, 3, "separate = {separate}");
        }
    }

    #[test]
    fn the_ai_bar_is_drawn_only_by_the_plugin_window() {
        let call = concat!("plugin_ai_bar", "(");
        for (name, src) in [
            ("plugin_params.rs", include_str!("plugin_params.rs")),
            ("inspector.rs", include_str!("inspector.rs")),
            ("inspector_strip.rs", include_str!("inspector_strip.rs")),
            ("mixer.rs", include_str!("mixer.rs")),
        ] {
            assert!(!src.contains(call), "{name} draws the AI bar");
        }
        assert_eq!(include_str!("plugin_window.rs").matches(concat!("self.plugin_ai_bar", "(")).count(), 1);
    }
}
