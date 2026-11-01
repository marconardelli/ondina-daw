//! GUI for the extended features: Clip Grid, Score view, surround panner, video window,
//! VST3 browser, Flex/Stem audio tools, comping, MIDI Transform and Environment routing.

use std::sync::mpsc::Receiver;

use ondina_core::command::{Command, NoteFilter, NoteOps};
use ondina_core::model::*;
use eframe::egui::{
    self, pos2, vec2, Align2, Color32, FontId, Rect, RichText, Rounding, Sense, Stroke,
};
use serde_json::Value;

use crate::app::OndinaApp;
use crate::chrome;
use crate::dialog;
use crate::theme;
use crate::widgets::{self, Icon};

#[derive(Default)]
pub struct ExtraState {
    pub show_score: bool,
    score_cache: Option<(u64, Id, Value)>,
    pub show_video: bool,
    video_tex: Option<egui::TextureHandle>,
    video_pending: Option<Receiver<Option<egui::ColorImage>>>,
    video_last: f64,
    pub video_seen: Option<String>,
    pub vst_list: Vec<Value>,
    pub show_transform: bool,
    tf_transpose: i32,
    tf_vel_scale: f32,
    tf_quantize: f64,
    tf_out_of_key_delete: bool,
    tf_reverse: bool,
    tf_invert: bool,
    pub show_routes: bool,
    rt_name: String,
    rt_min: u8,
    rt_max: u8,
    rt_transpose: i32,
    rt_track: Option<Id>,
    stretch: f64,
    semis: f64,
    vari: f64,
    /// Length and drummer choices of the session players in the track menu.
    pub players: crate::session_players::PlayerChoices,
}

impl ExtraState {
    pub fn new() -> Self {
        ExtraState {
            tf_vel_scale: 1.0,
            rt_name: "Split".into(),
            rt_max: 127,
            stretch: 1.0,
            vari: 100.0,
            ..Default::default()
        }
    }
}

// ------------------------------------------------------------------ notation helpers
const LETTER: [i32; 12] = [0, 0, 1, 1, 2, 3, 3, 4, 4, 5, 5, 6];
const SHARP: [bool; 12] = [
    false, true, false, true, false, false, true, false, true, false, true, false,
];

fn parse_name(n: &str) -> Option<u8> {
    ondina_core::theory::parse_note(n)
}

/// Space between the score's sheet and the edges of the editor.
const SCORE_MARGIN: f32 = 12.0;

/// Song bar number of a region's first measure: the score counts measures from the region start, the labels follow
/// the song like the piano roll's ruler (the bar the measure starts in).
fn score_first_bar(region_start: f64, beats_per_bar: f64) -> usize {
    if beats_per_bar <= 0.0 {
        return 1;
    }
    (region_start / beats_per_bar).floor().max(0.0) as usize + 1
}

/// What the Clip Grid says when there is nothing to launch: no track to hold cells, or no scene row yet.
pub fn clip_grid_empty(tracks: usize, scenes: usize) -> Option<(&'static str, &'static str)> {
    if tracks == 0 {
        Some(("No tracks for the Clip Grid", "Add an instrument or audio track: each track gets a column of cells."))
    } else if scenes == 0 {
        Some(("No scenes yet", "Press + Scene to add a row of cells, then click a cell to fill it."))
    } else {
        None
    }
}

/// Diatonic step number (C4 = 28).
fn diatonic(p: u8) -> i32 {
    (p as i32 / 12) * 7 + LETTER[(p % 12) as usize]
}

impl OndinaApp {
    // ============================================================== Clip Grid
    pub fn live_ui(&mut self, ui: &mut egui::Ui) {
        let p = self.project();
        let ls = self.shared.live_state.lock().clone();
        ui.horizontal(|ui| {
            ui.label(RichText::new("Clip Grid").font(crate::theme::title_font()));
            ui.separator();
            ui.label("Quantize");
            let q = p.live.quantize;
            egui::ComboBox::from_id_salt("liveq").width(70.0).selected_text(match q { x if x == 0.0 => "None".to_string(), x if x == 4.0 => "1 bar".into(), x if x == 1.0 => "1 beat".into(), x => format!("{x} beats") }).show_ui(ui, |ui| {
                for (v, l) in [(0.0, "None"), (1.0, "1 beat"), (2.0, "2 beats"), (4.0, "1 bar"), (8.0, "2 bars")] {
                    if ui.selectable_label(q == v, l).clicked() {
                        self.cmd(Command::SetLiveQuantize { beats: v });
                    }
                }
            });
            if widgets::icon_button(ui, Icon::Plus, "Scene", None).on_hover_text("Add a scene (a row of cells launched together)").clicked() {
                self.cmd(Command::AddScene { name: None });
            }
            if widgets::icon_button(ui, Icon::Stop, "Stop all cells", None).clicked() {
                self.cmd(Command::StopLiveCells { track_id: None });
            }
            if widgets::icon_button(ui, Icon::Record, "Commit performance to timeline", Some(theme::REC)).on_hover_text("Write what the grid played into regular regions").clicked() {
                if let Some(v) = self.exec(Command::CommitLivePerformance {}) {
                    self.status = format!("Committed {} regions", v["created_regions"].as_array().map(|a| a.len()).unwrap_or(0));
                }
            }
            if self.ai_button(ui, "AI: fill the grid") {
                self.ask_ai(ondina_studio::prompts::FILL_GRID.into());
            }
        });
        ui.separator();
        let grid_tracks = p.tracks.iter().filter(|t| t.kind != TrackKind::Bus).count();
        if let Some((title, hint)) = clip_grid_empty(grid_tracks, p.live.scenes.len()) {
            chrome::empty_state(ui, Icon::ClipGrid, title, hint);
            return;
        }
        egui::ScrollArea::both().show(ui, |ui| {
            egui::Grid::new("livegrid").spacing(vec2(4.0, 4.0)).show(ui, |ui| {
                ui.label("");
                for (si, name) in p.live.scenes.iter().enumerate() {
                    let text = widgets::leading_icon_text(ui, RichText::new(name.as_str()).size(theme::SIZE_NUMBER));
                    let resp = ui.add_sized(vec2(110.0, 22.0), egui::Button::new(text));
                    widgets::paint_leading_icon(ui, &resp, Icon::Play, theme::PLAY);
                    let resp = resp.on_hover_text("Launch this scene");
                    if resp.clicked() {
                        self.cmd(Command::LaunchScene { scene: si });
                    }
                    crate::drag_cancel::menu_unless_dragging(&resp, self.drag.is_some(), |ui| {
                        if ui.button("Remove scene").clicked() {
                            self.cmd(Command::RemoveScene { scene: si });
                            ui.close_menu();
                        }
                    });
                }
                ui.end_row();
                for t in p.tracks.iter().filter(|t| t.kind != TrackKind::Bus) {
                    ui.horizontal(|ui| {
                        ui.label(RichText::new(crate::mixer::short(&t.name, 12)).color(theme::rgb(t.color)));
                        if widgets::icon_only_button(ui, Icon::Stop, 18.0, theme::TEXT_2).on_hover_text("Stop this track's cell").clicked() {
                            self.cmd(Command::StopLiveCells { track_id: Some(t.id) });
                        }
                    });
                    for si in 0..p.live.scenes.len() {
                        let cell = p.live.cells.iter().find(|c| c.track_id == t.id && c.scene == si);
                        let (rect, resp) = ui.allocate_exact_size(vec2(110.0, 40.0), Sense::click());
                        match cell {
                            Some(c) => {
                                let playing = ls.playing.iter().any(|x| x.1 == c.id);
                                let queued = ls.queued.iter().any(|x| x.2 == Some(c.id));
                                let base = theme::rgb(t.color);
                                ui.painter().rect_filled(rect, Rounding::same(5.0_f32), if playing { base } else { theme::darker(base, 0.45) });
                                if queued {
                                    let blink = (ui.input(|i| i.time) * 4.0) as i64 % 2 == 0;
                                    if blink {
                                        ui.painter().rect_stroke(rect, Rounding::same(5.0_f32), Stroke::new(2.0_f32, Color32::WHITE));
                                    }
                                }
                                if playing {
                                    // progress ring
                                    if let Some(lp) = ls.playing.iter().find(|x| x.1 == c.id) {
                                        let prog = (((self.position() - lp.2) / c.length).fract().max(0.0)) as f32;
                                        let w = rect.width() * prog;
                                        ui.painter().rect_filled(Rect::from_min_size(pos2(rect.left(), rect.bottom() - 4.0), vec2(w, 4.0)), Rounding::ZERO, Color32::WHITE);
                                    }
                                }
                                ui.painter().text(rect.left_top() + vec2(6.0, 4.0), Align2::LEFT_TOP, crate::mixer::short(&c.name, 14), FontId::proportional(11.0), Color32::BLACK);
                                ui.painter().text(rect.left_bottom() + vec2(6.0, -8.0), Align2::LEFT_BOTTOM, format!("{} bt", c.length), FontId::proportional(9.5), Color32::from_black_alpha(200));
                                let cid = c.id;
                                if resp.clicked() {
                                    self.cmd(Command::LaunchCell { cell_id: cid });
                                }
                                crate::drag_cancel::menu_unless_dragging(&resp, self.drag.is_some(), |ui| {
                                    if ui.button("Delete cell").clicked() {
                                        self.cmd(Command::DeleteLiveCell { cell_id: cid });
                                        ui.close_menu();
                                    }
                                });
                            }
                            None => {
                                ui.painter().rect_stroke(rect, Rounding::same(5.0_f32), Stroke::new(1.0_f32, theme::GRID));
                                ui.painter().text(rect.center(), Align2::CENTER_CENTER, "+", FontId::proportional(16.0), theme::TEXT_DIM);
                                let tid = t.id;
                                let tkind = t.kind;
                                if resp.clicked() {
                                    // copy the selected region of this track, or create an empty bar
                                    let sel = self.selected_region.filter(|r| p.region_track_id(*r) == Some(tid));
                                    if sel.is_some() || tkind == TrackKind::Instrument {
                                        self.cmd(Command::CreateLiveCell { track_id: tid, scene: si, length: if sel.is_some() { None } else { Some(p.time_signature.beats_per_bar()) }, notes: None, from_region_id: sel, name: None });
                                    } else {
                                        self.status = "Select an audio region on this track to put it in a cell".into();
                                    }
                                }
                                resp.on_hover_text("Click: new cell (copies the selected region of this track, or an empty bar)");
                            }
                        }
                    }
                    ui.end_row();
                }
            });
        });
    }

    // ============================================================== Score view
    pub fn score_ui(&mut self, ui: &mut egui::Ui, region: &Region) {
        let rev = self.view.revision;
        let need =
            !matches!(&self.extra.score_cache, Some((r, id, _)) if *r == rev && *id == region.id);
        if need {
            if let Some(v) = self.exec(Command::GetScore {
                region_id: region.id,
            }) {
                self.extra.score_cache = Some((rev, region.id, v));
            }
        }
        let Some((_, _, score)) = self.extra.score_cache.clone() else {
            return;
        };
        ui.horizontal(|ui| {
            ui.label(
                RichText::new(format!(
                    "{}   {}",
                    score["time_signature"].as_str().unwrap_or(""),
                    score["key"].as_str().unwrap_or("")
                ))
                .strong(),
            );
            if ui.button("Export MusicXML…").clicked() {
                if let Some(path) = rfd::FileDialog::new()
                    .add_filter("MusicXML", &["musicxml", "xml"])
                    .set_file_name("score.musicxml")
                    .save_file()
                {
                    self.exec(Command::ExportMusicXml {
                        path: path.display().to_string(),
                        track_ids: None,
                    });
                }
            }
            if ui
                .button("Lyrics…")
                .on_hover_text("Assign the text in the AI box as lyrics")
                .clicked()
            {
                let text = self.ai.input.clone();
                if !text.trim().is_empty() {
                    self.cmd(Command::SetLyrics {
                        region_id: region.id,
                        text: Some(text),
                        lyrics: None,
                    });
                } else {
                    self.status =
                        "Type the lyrics in the assistant input box, then press Lyrics…".into();
                }
            }
        });
        let bass = score["clef"] == "bass";
        let measures = score["measures"].as_array().cloned().unwrap_or_default();
        let staff_gap = 9.0f32;
        let per_beat = 64.0f32;
        let bpb = self.view.project.time_signature.beats_per_bar() as f32;
        let measure_w = per_beat * bpb + 14.0;
        let per_row = ((ui.available_width() - 60.0 - 2.0 * SCORE_MARGIN) / measure_w).floor().max(1.0) as usize;
        let first_bar = score_first_bar(region.start, f64::from(bpb));
        let rows = measures.len().div_ceil(per_row).max(1);
        let row_h = staff_gap * 4.0 + 90.0;
        egui::ScrollArea::vertical().show(ui, |ui| {
            let (outer, _) = ui.allocate_exact_size(
                vec2(ui.available_width(), rows as f32 * row_h + 20.0 + 2.0 * SCORE_MARGIN),
                Sense::hover(),
            );
            // a sheet of paper on the dark ground, with a margin and a soft shadow
            let painter = ui.painter_at(outer);
            painter.rect_filled(outer, Rounding::ZERO, theme::GROUND);
            let rect = outer.shrink(SCORE_MARGIN);
            painter.rect_filled(rect.translate(vec2(0.0, 3.0)).expand(1.0), Rounding::same(theme::R_LG), Color32::from_black_alpha(90));
            painter.rect_filled(rect, Rounding::same(theme::R_SM), Color32::from_rgb(245, 243, 236));
            let ink = Color32::from_rgb(25, 25, 30);
            // bottom line pitch: E4 (treble) or G2 (bass)
            let bottom = if bass { diatonic(43) } else { diatonic(64) };
            for row in 0..rows {
                let top = rect.top() + 30.0 + row as f32 * row_h;
                let left = rect.left() + 50.0;
                let right = left + per_row as f32 * measure_w;
                let line_y = |k: i32| top + (4 - k) as f32 * staff_gap; // k=0 bottom line
                for k in 0..5 {
                    painter.line_segment(
                        [pos2(left - 40.0, line_y(k)), pos2(right, line_y(k))],
                        Stroke::new(1.0_f32, ink),
                    );
                }
                painter.text(
                    pos2(left - 36.0, line_y(2)),
                    Align2::LEFT_CENTER,
                    if bass { "𝄢" } else { "𝄞" },
                    FontId::proportional(staff_gap * if bass { 3.2 } else { 5.0 }),
                    ink,
                );
                for mi in 0..per_row {
                    let idx = row * per_row + mi;
                    let Some(m) = measures.get(idx) else { break };
                    let mx = left + mi as f32 * measure_w;
                    painter.line_segment(
                        [
                            pos2(mx + measure_w, line_y(0)),
                            pos2(mx + measure_w, line_y(4)),
                        ],
                        Stroke::new(1.0_f32, ink),
                    );
                    painter.text(
                        pos2(mx + 2.0, line_y(4) - 12.0),
                        Align2::LEFT_BOTTOM,
                        format!("{}", first_bar + idx),
                        FontId::proportional(9.0),
                        Color32::GRAY,
                    );
                    for it in m["items"].as_array().cloned().unwrap_or_default() {
                        let beat = it["beat"].as_f64().unwrap_or(0.0) as f32;
                        let in_bar = beat - (idx as f32) * bpb;
                        let x = mx + 10.0 + in_bar * per_beat;
                        let value = it["value"].as_str().unwrap_or("quarter");
                        let dotted = value.starts_with("dotted");
                        let ty = value.trim_start_matches("dotted ");
                        if it["rest"].as_bool().unwrap_or(false) {
                            let sym = match ty {
                                "whole" => "𝄻",
                                "half" => "𝄼",
                                "quarter" => "𝄽",
                                "eighth" => "𝄾",
                                _ => "𝄿",
                            };
                            painter.text(
                                pos2(x, line_y(2)),
                                Align2::CENTER_CENTER,
                                sym,
                                FontId::proportional(staff_gap * 3.0),
                                ink,
                            );
                            continue;
                        }
                        let pitches: Vec<u8> = it["pitches"]
                            .as_array()
                            .map(|a| {
                                a.iter()
                                    .filter_map(|n| n.as_str().and_then(parse_name))
                                    .collect()
                            })
                            .unwrap_or_default();
                        let mut ys = vec![];
                        for pch in &pitches {
                            let step = diatonic(*pch) - bottom; // 0 = bottom line, 1 = first space...
                            let y = line_y(0) - step as f32 * staff_gap / 2.0;
                            ys.push(y);
                            // ledger lines
                            let mut s = if step < 0 { -2 } else { 10 };
                            while (step < 0 && s >= step) || (step > 8 && s <= step) {
                                let ly = line_y(0) - s as f32 * staff_gap / 2.0;
                                painter.line_segment(
                                    [pos2(x - 8.0, ly), pos2(x + 8.0, ly)],
                                    Stroke::new(1.0_f32, ink),
                                );
                                s += if step < 0 { -2 } else { 2 };
                            }
                            let hollow = matches!(ty, "whole" | "half");
                            let r = Rect::from_center_size(
                                pos2(x, y),
                                vec2(staff_gap * 1.3, staff_gap * 0.95),
                            );
                            if hollow {
                                painter.add(egui::Shape::ellipse_stroke(
                                    r.center(),
                                    r.size() / 2.0,
                                    Stroke::new(1.6_f32, ink),
                                ));
                            } else {
                                painter.add(egui::Shape::ellipse_filled(
                                    r.center(),
                                    r.size() / 2.0,
                                    ink,
                                ));
                            }
                            if SHARP[(*pch % 12) as usize] {
                                painter.text(
                                    pos2(x - staff_gap * 1.3, y),
                                    Align2::CENTER_CENTER,
                                    "♯",
                                    FontId::proportional(staff_gap * 1.6),
                                    ink,
                                );
                            }
                            if dotted {
                                painter.circle_filled(pos2(x + staff_gap, y), 1.6, ink);
                            }
                        }
                        if ty != "whole" && !ys.is_empty() {
                            let up = ys.iter().sum::<f32>() / ys.len() as f32 > line_y(2);
                            let (sx, y0, y1) = if up {
                                let hi = ys.iter().cloned().fold(f32::MAX, f32::min);
                                let lo = ys.iter().cloned().fold(f32::MIN, f32::max);
                                (x + staff_gap * 0.62, lo, hi - staff_gap * 3.3)
                            } else {
                                let hi = ys.iter().cloned().fold(f32::MAX, f32::min);
                                let lo = ys.iter().cloned().fold(f32::MIN, f32::max);
                                (x - staff_gap * 0.62, hi, lo + staff_gap * 3.3)
                            };
                            painter.line_segment(
                                [pos2(sx, y0), pos2(sx, y1)],
                                Stroke::new(1.2_f32, ink),
                            );
                            let flags = match ty {
                                "eighth" => 1,
                                "16th" => 2,
                                _ => 0,
                            };
                            for f in 0..flags {
                                let fy = y1
                                    + if up {
                                        f as f32 * 5.0
                                    } else {
                                        -(f as f32) * 5.0
                                    };
                                painter.line_segment(
                                    [
                                        pos2(sx, fy),
                                        pos2(sx + 7.0, fy + if up { 8.0 } else { -8.0 }),
                                    ],
                                    Stroke::new(1.5_f32, ink),
                                );
                            }
                        }
                        if it["tie"] == "start" {
                            if let Some(y) = ys.first() {
                                let pts: Vec<egui::Pos2> = (0..=8)
                                    .map(|k| {
                                        let t = k as f32 / 8.0;
                                        pos2(
                                            x + t * per_beat * 0.8,
                                            y + 8.0 + (t * std::f32::consts::PI).sin() * 4.0,
                                        )
                                    })
                                    .collect();
                                painter.add(egui::Shape::line(pts, Stroke::new(1.0_f32, ink)));
                            }
                        }
                        if let Some(l) = it["lyric"].as_str() {
                            painter.text(
                                pos2(x, line_y(0) + 30.0),
                                Align2::CENTER_TOP,
                                l,
                                FontId::proportional(11.0),
                                ink,
                            );
                        }
                    }
                }
            }
        });
    }

    // ============================================================== surround panner
    pub fn surround_inspector(&mut self, ui: &mut egui::Ui, p: &Project, track: Option<&Track>) {
        egui::CollapsingHeader::new("Surround / Spatial Audio").default_open(p.surround != SurroundFormat::Stereo).show(ui, |ui| {
            egui::ComboBox::from_id_salt("sfmt").selected_text(format!("{:?}", p.surround)).show_ui(ui, |ui| {
                for f in [SurroundFormat::Stereo, SurroundFormat::Binaural, SurroundFormat::Surround51, SurroundFormat::Surround71, SurroundFormat::Immersive714] {
                    if ui.selectable_label(p.surround == f, format!("{f:?}")).clicked() {
                        self.cmd(Command::SetSurroundFormat { format: f });
                    }
                }
            });
            if p.surround == SurroundFormat::Stereo {
                ui.label(RichText::new("Choose a surround or binaural format to position tracks in 3D. Monitoring is binaural on headphones.").small().color(theme::TEXT_DIM));
                return;
            }
            let Some(t) = track else { return };
            let sp = t.channel.surround_pan;
            let size = 150.0;
            let (rect, resp) = ui.allocate_exact_size(vec2(size, size), Sense::click_and_drag());
            let c = rect.center();
            let painter = ui.painter_at(rect);
            painter.circle_filled(c, size / 2.0 - 2.0, theme::BG);
            painter.circle_stroke(c, size / 2.0 - 2.0, Stroke::new(1.0_f32, theme::GRID_BAR));
            painter.circle_stroke(c, size / 4.0, Stroke::new(0.5_f32, theme::GRID));
            // speakers
            let lay_name = match p.surround { SurroundFormat::Surround71 => "7.1", SurroundFormat::Immersive714 => "7.1.4", _ => "5.1" };
            for (name, az, el) in ondina_dsp::spatial::layout(lay_name) {
                if el < -900.0 {
                    continue;
                }
                let r = if el > 0.0 { size * 0.25 } else { size * 0.45 };
                let a = az.to_radians();
                let pos = c + vec2(a.sin() * r, -a.cos() * r);
                painter.circle_filled(pos, 4.0, if el > 0.0 { Color32::from_rgb(120, 140, 230) } else { theme::TEXT_DIM });
                painter.text(pos + vec2(0.0, -9.0), Align2::CENTER_BOTTOM, name, FontId::proportional(8.5), theme::TEXT_DIM);
            }
            let radius = size * 0.45 * (1.0 - sp.elevation / 90.0 * 0.45);
            let a = sp.azimuth.to_radians();
            let src = c + vec2(a.sin() * radius, -a.cos() * radius);
            painter.circle_filled(src, 6.0 + sp.spread * 6.0, theme::rgb(t.color));
            painter.text(c + vec2(0.0, size / 2.0 - 10.0), Align2::CENTER_CENTER, "listener", FontId::proportional(8.0), theme::TEXT_DIM);
            if let Some(pp) = resp.interact_pointer_pos() {
                if resp.dragged() || resp.clicked() {
                    let d = pp - c;
                    let az = d.x.atan2(-d.y).to_degrees();
                    self.cmd_merge(Command::SetSurroundPan { track_id: t.id, azimuth: az, elevation: None, spread: None, object: None }, format!("spn{}", t.id));
                }
            }
            let mut el = sp.elevation;
            if ui.add(egui::Slider::new(&mut el, 0.0..=90.0).text("Elevation°")).changed() {
                self.cmd_merge(Command::SetSurroundPan { track_id: t.id, azimuth: sp.azimuth, elevation: Some(el), spread: None, object: None }, format!("spe{}", t.id));
            }
            let mut spr = sp.spread;
            if ui.add(egui::Slider::new(&mut spr, 0.0..=1.0).text("Spread")).changed() {
                self.cmd_merge(Command::SetSurroundPan { track_id: t.id, azimuth: sp.azimuth, elevation: None, spread: Some(spr), object: None }, format!("sps{}", t.id));
            }
            ui.label(RichText::new(format!("Azimuth {:.0}°", sp.azimuth)).small());
            let mut obj = sp.object;
            if ui.checkbox(&mut obj, "Audio object (ADM / object-based)").on_hover_text("Objects are exported as separate ADM objects with their trajectory; others go to the 7.1.2 bed. Automate surround:azimuth / surround:elevation to move them.").changed() {
                self.cmd(Command::SetSurroundPan { track_id: t.id, azimuth: sp.azimuth, elevation: None, spread: None, object: Some(obj) });
            }
        });
    }

    // ============================================================== VST3 browser
    /// Scan the VST3 folders (the standard ones, the user's and tools\vst3) and refresh the library list.
    fn scan_vst3_library(&mut self) {
        let extra = vec!["tools/vst3".to_string()];
        if let Some(v) = self.exec(Command::ScanVst3 {
            extra_paths: Some(extra),
        }) {
            self.extra.vst_list =
                v["plugins"].as_array().cloned().unwrap_or_default();
        }
    }

    /// Store a new list of VST3 folders in both copies of the settings and on disk. False (nothing
    /// changed) when the session is busy.
    fn store_vst3_folders(&mut self, folders: Vec<String>) -> bool {
        let Some(mut s) = self.session.try_lock_for(std::time::Duration::from_millis(300)) else {
            self.status = "Session busy (AI/MCP running) — try again".into();
            return false;
        };
        s.settings.vst3_folders = folders.clone();
        drop(s);
        self.settings.vst3_folders = folders;
        self.settings.save();
        true
    }

    pub fn vst_library(&mut self, ui: &mut egui::Ui) {
        egui::CollapsingHeader::new("Third-party VST3")
            .default_open(true)
            .show(ui, |ui| {
                ui.horizontal(|ui| {
                    if ui.button("Scan VST3 plug-ins").clicked() {
                        self.scan_vst3_library();
                    }
                    if ui.button("Add from file…").clicked() {
                        if let Some(dir) = rfd::FileDialog::new().pick_folder() {
                            if let Some(t) = self.selected_track {
                                self.exec(Command::AddVst3 {
                                    track_id: t,
                                    path: dir.display().to_string(),
                                    as_instrument: None,
                                    index: None,
                                });
                            }
                        }
                    }
                    if ui
                        .button("Add folder…")
                        .on_hover_text("Add a folder of VST3 plug-ins. MCP clients and the assistant may load plug-ins only from the standard VST3 folders and the folders added here.")
                        .clicked()
                    {
                        if let Some(dir) = rfd::FileDialog::new().pick_folder() {
                            match ondina_dsp::vst3::check_user_folder(&dir.display().to_string()) {
                                Ok(f) => {
                                    let mut list = self.settings.clone();
                                    if !list.add_vst3_folder(&f) {
                                        self.status = format!("{f} is already in the VST3 folders");
                                    } else if self.store_vst3_folders(list.vst3_folders) {
                                        self.status = format!("VST3 folder added: {f}");
                                        self.scan_vst3_library();
                                    }
                                }
                                Err(e) => self.status = format!("Folder not added: {e}"),
                            }
                        }
                    }
                });
                if !self.settings.vst3_folders.is_empty() {
                    ui.label(RichText::new("Plug-in folders").small().color(theme::TEXT_DIM));
                    let mut remove: Option<String> = None;
                    for f in self.settings.vst3_folders.clone() {
                        ui.horizontal(|ui| {
                            ui.label(RichText::new(&f).small());
                            if widgets::icon_only_button(ui, Icon::Close, 16.0, theme::TEXT_3).on_hover_text("Remove this folder").clicked() {
                                remove = Some(f.clone());
                            }
                        });
                    }
                    if let Some(f) = remove {
                        let mut list = self.settings.clone();
                        list.remove_vst3_folder(&f);
                        if self.store_vst3_folders(list.vst3_folders) {
                            self.status = format!("VST3 folder removed: {f}");
                            self.scan_vst3_library();
                        }
                    }
                }
                for pl in self.extra.vst_list.clone() {
                    ui.horizontal(|ui| {
                        let inst = pl["instrument"].as_bool().unwrap_or(false);
                        widgets::icon(ui, if inst { Icon::Piano } else { Icon::Effect }, 14.0, theme::TEXT_2);
                        ui.label(format!(
                            "{} · {}",
                            pl["name"].as_str().unwrap_or(""),
                            pl["vendor"].as_str().unwrap_or("")
                        ));
                        let path = pl["path"].as_str().unwrap_or("").to_string();
                        if inst {
                            if widgets::icon_button(ui, Icon::Plus, "Track", None).on_hover_text("New instrument track with this plug-in").clicked() {
                                if let Some(v) = self.exec(Command::AddTrack {
                                    kind: TrackKind::Instrument,
                                    name: pl["name"].as_str().map(|s| s.to_string()),
                                    patch: None,
                                    instrument: None,
                                    preset: None,
                                    index: None,
                                }) {
                                    if let Some(tid) = v["track_id"].as_u64() {
                                        self.exec(Command::AddVst3 {
                                            track_id: tid,
                                            path: path.clone(),
                                            as_instrument: Some(true),
                                            index: None,
                                        });
                                        self.selected_track = Some(tid);
                                    }
                                }
                            }
                        } else if ui
                            .add(egui::Button::new("Insert"))
                            .on_hover_text("Insert on the selected track (or master)")
                            .clicked()
                        {
                            let t = self.selected_track.unwrap_or(MASTER_ID);
                            self.exec(Command::AddVst3 {
                                track_id: t,
                                path: path.clone(),
                                as_instrument: Some(false),
                                index: None,
                            });
                        }
                    });
                }
            });
    }

    // ============================================================== audio tools (Flex / Stems)
    pub fn audio_tools(&mut self, ui: &mut egui::Ui, region: &Region) {
        let rid = region.id;
        ui.horizontal_wrapped(|ui| {
            ui.label(RichText::new("Flex").strong());
            ui.add(
                egui::DragValue::new(&mut self.extra.stretch)
                    .range(0.25..=4.0)
                    .speed(0.01)
                    .prefix("Time ×"),
            );
            if ui.button("Stretch").clicked() {
                self.exec(Command::TimeStretchRegion {
                    region_id: rid,
                    ratio: Some(self.extra.stretch),
                    target_length: None,
                });
            }
            ui.add(
                egui::DragValue::new(&mut self.extra.semis)
                    .range(-24.0..=24.0)
                    .speed(0.1)
                    .suffix(" st"),
            );
            if ui.button("Pitch").clicked() {
                self.exec(Command::PitchShiftRegion {
                    region_id: rid,
                    semitones: self.extra.semis,
                });
            }
            ui.add(
                egui::DragValue::new(&mut self.extra.vari)
                    .range(25.0..=400.0)
                    .suffix(" %"),
            );
            if ui.button("Varispeed").clicked() {
                self.exec(Command::VarispeedRegion {
                    region_id: rid,
                    percent: self.extra.vari,
                });
            }
            if ui.button("Tune to key").clicked() {
                self.exec(Command::PitchCorrectRegion {
                    region_id: rid,
                    strength: Some(1.0),
                });
            }
            if ui.button("→ MIDI").clicked() {
                self.exec(Command::AudioToMidi {
                    region_id: rid,
                    track_id: None,
                });
            }
            if ui.button("Detect tempo").clicked() {
                if let Some(v) = self.exec(Command::DetectTempo {
                    region_id: rid,
                    apply: Some(false),
                }) {
                    self.status =
                        format!("Detected {} bpm (confidence {})", v["bpm"], v["confidence"]);
                }
            }
            if ui.button("Normalize").clicked() {
                self.exec(Command::NormalizeRegion {
                    region_id: rid,
                    target_db: Some(-1.0),
                });
            }
            if widgets::icon_button(ui, Icon::Scissors, "Stem Splitter", None).clicked() {
                if let Some(v) = self.exec(Command::SplitStems {
                    region_id: rid,
                    engine: None,
                }) {
                    self.status = format!(
                        "Created {} stem tracks",
                        v["stems"].as_array().map(|a| a.len()).unwrap_or(0)
                    );
                }
            }
        });
    }

    // ============================================================== windows
    pub fn extra_windows(&mut self, ctx: &egui::Context) {
        // open the movie window automatically when a (new) movie gets linked, e.g. via MCP
        let movie_path = self.view.project.movie.as_ref().map(|m| m.path.clone());
        if movie_path.is_some() && movie_path != self.extra.video_seen {
            self.extra.show_video = true;
            self.extra.video_last = -1.0;
            self.extra.video_tex = None;
        }
        self.extra.video_seen = movie_path;
        // ---- video
        if self.extra.show_video {
            let mut open = true;
            let p = self.project();
            dialog::window("Movie")
                .open(&mut open)
                .default_size([360.0, 240.0])
                .show(ctx, |ui| {
                    let Some(m) = p.movie.clone() else {
                        ui.label("No movie loaded.");
                        if ui.button("Load Movie…").clicked() {
                            self.load_movie_dialog();
                        }
                        return;
                    };
                    // a project made elsewhere may carry a URL or an option as movie path: no ffmpeg for it
                    let refused = ondina_studio::session_ext::ffmpeg_input(&m.path).err();
                    ui.label(
                        RichText::new(format!(
                            "{}  ·  TC {}",
                            std::path::Path::new(&m.path)
                                .file_name()
                                .and_then(|f| f.to_str())
                                .unwrap_or(""),
                            p.timecode(self.position())
                        ))
                        .monospace(),
                    );
                    // request a new frame when the playhead moved
                    let t = (p.tempo.beat_to_sec(self.position()) - m.offset_sec)
                        .clamp(0.0, m.duration_sec.max(0.0));
                    let interval = if self.playing() { 0.25 } else { 0.04 };
                    if refused.is_none()
                        && self.extra.video_pending.is_none()
                        && (t - self.extra.video_last).abs() > interval
                    {
                        self.extra.video_last = t;
                        let (tx, rx) = std::sync::mpsc::channel();
                        let path = m.path.clone();
                        std::thread::spawn(move || {
                            let _ = tx.send(grab_frame(&path, t));
                        });
                        self.extra.video_pending = Some(rx);
                    }
                    if let Some(rx) = &self.extra.video_pending {
                        if let Ok(img) = rx.try_recv() {
                            if let Some(img) = img {
                                self.extra.video_tex = Some(ctx.load_texture(
                                    "movie",
                                    img,
                                    egui::TextureOptions::LINEAR,
                                ));
                            }
                            self.extra.video_pending = None;
                        }
                    }
                    if let Some(e) = &refused {
                        ui.label(
                            RichText::new(format!(
                                "Movie path refused: {e}. Load the movie again from a local file."
                            ))
                            .color(theme::REC),
                        );
                    } else if let Some(tex) = &self.extra.video_tex {
                        let w = ui.available_width();
                        ui.image((tex.id(), vec2(w, w * 9.0 / 16.0)));
                    } else {
                        ui.label(
                            RichText::new("Frames need ffmpeg (tools\\ffmpeg or PATH).")
                                .color(theme::TEXT_DIM),
                        );
                    }
                    ui.horizontal(|ui| {
                        let mut off = m.offset_sec;
                        if ui
                            .add(
                                egui::DragValue::new(&mut off)
                                    .speed(0.04)
                                    .prefix("Offset ")
                                    .suffix(" s"),
                            )
                            .changed()
                        {
                            self.cmd_merge(Command::SetMovieOffset { offset_sec: off }, "movieoff");
                        }
                        let mut fps = p.frame_rate;
                        egui::ComboBox::from_id_salt("fps")
                            .width(70.0)
                            .selected_text(format!("{fps} fps"))
                            .show_ui(ui, |ui| {
                                for f in [23.976, 24.0, 25.0, 29.97, 30.0, 50.0, 60.0] {
                                    if ui.selectable_value(&mut fps, f, format!("{f}")).clicked() {
                                        self.cmd(Command::SetFrameRate { fps: f });
                                    }
                                }
                            });
                        if ui.button("Export Movie…").clicked() {
                            if let Some(path) = rfd::FileDialog::new()
                                .add_filter("MP4", &["mp4"])
                                .set_file_name("scored.mp4")
                                .save_file()
                            {
                                self.exec(Command::ExportMovie {
                                    path: path.display().to_string(),
                                });
                            }
                        }
                    });
                });
            self.extra.show_video = open;
        }
        // ---- MIDI transform
        if self.extra.show_transform {
            let mut open = true;
            dialog::window("MIDI Transform")
                .open(&mut open)
                .show(ctx, |ui| {
                    let Some(rid) = self.selected_region else {
                        ui.label("Select a MIDI region.");
                        return;
                    };
                    let only_sel = !self.selected_notes.is_empty();
                    ui.label(if only_sel {
                        format!("Acts on {} selected notes", self.selected_notes.len())
                    } else {
                        "Acts on all notes of the region".into()
                    });
                    ui.add(
                        egui::Slider::new(&mut self.extra.tf_transpose, -24..=24).text("Transpose"),
                    );
                    ui.add(
                        egui::Slider::new(&mut self.extra.tf_vel_scale, 0.1..=2.0)
                            .text("Velocity ×"),
                    );
                    ui.add(
                        egui::Slider::new(&mut self.extra.tf_quantize, 0.0..=1.0)
                            .text("Quantize grid (beats, 0 = off)"),
                    );
                    ui.checkbox(&mut self.extra.tf_reverse, "Reverse (retrograde)");
                    ui.checkbox(&mut self.extra.tf_invert, "Invert around first note");
                    ui.checkbox(
                        &mut self.extra.tf_out_of_key_delete,
                        "Delete notes outside the key",
                    );
                    if ui.button("Apply").clicked() {
                        let sel: Option<Vec<usize>> = if only_sel {
                            Some(self.selected_notes.iter().copied().collect())
                        } else {
                            None
                        };
                        if self.extra.tf_out_of_key_delete {
                            self.cmd(Command::TransformNotes {
                                region_id: rid,
                                select: Some(NoteFilter {
                                    out_of_key: Some(true),
                                    ..Default::default()
                                }),
                                operations: NoteOps {
                                    delete: Some(true),
                                    ..Default::default()
                                },
                            });
                        }
                        let first_pitch = self.view.project.find_region(rid).and_then(|(_, r)| {
                            if let RegionContent::Midi(m) = &r.content {
                                m.notes.first().map(|n| n.pitch)
                            } else {
                                None
                            }
                        });
                        self.cmd(Command::TransformNotes {
                            region_id: rid,
                            select: Some(NoteFilter {
                                indices: sel,
                                ..Default::default()
                            }),
                            operations: NoteOps {
                                transpose: (self.extra.tf_transpose != 0)
                                    .then_some(self.extra.tf_transpose),
                                velocity_scale: ((self.extra.tf_vel_scale - 1.0).abs() > 1e-3)
                                    .then_some(self.extra.tf_vel_scale),
                                quantize: (self.extra.tf_quantize > 0.0)
                                    .then_some(self.extra.tf_quantize),
                                reverse: self.extra.tf_reverse.then_some(true),
                                invert_around: if self.extra.tf_invert {
                                    first_pitch
                                } else {
                                    None
                                },
                                ..Default::default()
                            },
                        });
                        self.selected_notes.clear();
                    }
                });
            self.extra.show_transform = open;
        }
        // ---- environment routing
        if self.extra.show_routes {
            let mut open = true;
            let p = self.project();
            dialog::window("MIDI Environment").open(&mut open).show(ctx, |ui| {
                ui.label(RichText::new("Keyboard splits and layers: each route sends a note range of the MIDI input to a track.").small().color(theme::TEXT_DIM));
                let mut routes = p.midi_routes.clone();
                let mut changed = false;
                let mut remove = None;
                for (i, r) in routes.iter().enumerate() {
                    ui.horizontal(|ui| {
                        let tname = p.track(r.track_id).map(|t| t.name.clone()).unwrap_or("?".into());
                        ui.label(format!("{}: notes {}–{}  transpose {:+}  → {}", r.name, r.note_min.unwrap_or(0), r.note_max.unwrap_or(127), r.transpose, tname));
                        if widgets::icon_only_button(ui, Icon::Close, 16.0, theme::TEXT_3).on_hover_text("Remove this route").clicked() {
                            remove = Some(i);
                        }
                    });
                }
                if let Some(i) = remove {
                    routes.remove(i);
                    changed = true;
                }
                ui.separator();
                ui.horizontal(|ui| {
                    ui.text_edit_singleline(&mut self.extra.rt_name);
                    ui.add(egui::DragValue::new(&mut self.extra.rt_min).range(0..=127).prefix("from "));
                    ui.add(egui::DragValue::new(&mut self.extra.rt_max).range(0..=127).prefix("to "));
                    ui.add(egui::DragValue::new(&mut self.extra.rt_transpose).range(-48..=48).prefix("tr "));
                });
                ui.horizontal(|ui| {
                    egui::ComboBox::from_id_salt("rtt").selected_text(self.extra.rt_track.and_then(|t| p.track(t)).map(|t| t.name.clone()).unwrap_or("Track…".into())).show_ui(ui, |ui| {
                        for t in p.tracks.iter().filter(|t| t.kind == TrackKind::Instrument) {
                            ui.selectable_value(&mut self.extra.rt_track, Some(t.id), &t.name);
                        }
                    });
                    if ui.button("Add route").clicked() {
                        if let Some(t) = self.extra.rt_track {
                            routes.push(MidiRoute { name: self.extra.rt_name.clone(), channel: None, note_min: Some(self.extra.rt_min), note_max: Some(self.extra.rt_max), transpose: self.extra.rt_transpose, velocity_scale: None, track_id: t });
                            changed = true;
                        }
                    }
                });
                if changed {
                    self.cmd(Command::SetMidiRoutes { routes });
                }
            });
            self.extra.show_routes = open;
        }
    }

    pub fn load_movie_dialog(&mut self) {
        if let Some(p) = rfd::FileDialog::new()
            .add_filter("Movie", &["mp4", "mov", "m4v", "mkv", "avi"])
            .pick_file()
        {
            if self
                .exec(Command::LoadMovie {
                    path: p.display().to_string(),
                    offset_sec: Some(0.0),
                    import_audio: Some(true),
                })
                .is_some()
            {
                self.extra.show_video = true;
            }
        }
    }

    pub fn takes_menu(&mut self, ui: &mut egui::Ui, region: &Region) {
        if region.take.is_some() {
            if ui
                .button(format!(
                    "Comp: use take {} in the cycle range",
                    region.take.unwrap_or(0)
                ))
                .clicked()
            {
                let cy = self.view.project.cycle;
                if let Some(tid) = self.view.project.region_track_id(region.id) {
                    self.cmd(Command::CompTakes {
                        track_id: tid,
                        segments: vec![ondina_core::command::CompSegment {
                            region_id: region.id,
                            from_beat: cy.start.max(region.start),
                            to_beat: cy.end.min(region.end()),
                        }],
                    });
                }
                ui.close_menu();
            }
        }
    }
}

/// Decode one movie frame at `t` seconds via ffmpeg into an egui image (320x180 RGB).
fn grab_frame(path: &str, t: f64) -> Option<egui::ColorImage> {
    let (w, h) = (320usize, 180usize);
    let px = ondina_studio::session_ext::movie_frame_rgb(path, t, w, h).ok()?;
    Some(egui::ColorImage::from_rgb([w, h], &px))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn score_bars_are_numbered_from_the_song() {
        assert_eq!(score_first_bar(0.0, 4.0), 1);
        assert_eq!(score_first_bar(16.0, 4.0), 5);
        assert_eq!(score_first_bar(18.0, 4.0), 5, "a region starting mid-bar: the bar it starts in");
        assert_eq!(score_first_bar(9.0, 3.0), 4, "3/4");
        assert_eq!(score_first_bar(8.0, 0.0), 1);
    }
}
