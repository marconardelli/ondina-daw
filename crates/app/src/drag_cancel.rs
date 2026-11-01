//! How a mouse drag ends: Esc drops it, and only the release of the button that holds it ends it.
//! egui already stops its own drag on Esc and starts no new one until the button is pressed again;
//! this drops the app's `Drag` on the same frame.

use eframe::egui;
use ondina_core::command::Command;

use crate::app::{Drag, OndinaApp};
use crate::arrange::cycle_gesture_key;

/// What to send when `drag` is cancelled, with its undo merge key. Most drags change the project only on
/// release, so there is nothing to send. Cycle drags send `SetCycle` while they move: they get the cycle back
/// as it was when the press began, under the gesture's key, so it merges into the gesture's single undo step.
pub fn cancel_command(drag: &Drag) -> Option<(Command, String)> {
    match drag {
        Drag::Cycle { orig, gesture, sent, .. } => sent.then(|| restore(orig.enabled, orig.start, orig.end, *gesture)),
        Drag::CycleEdit { start, end, enabled, gesture, sent, .. } => {
            sent.map(|_| restore(*enabled, *start, *end, *gesture))
        }
        Drag::MoveRegion { .. }
        | Drag::ResizeRegion { .. }
        | Drag::RegionFade { .. }
        | Drag::MoveMarker { .. }
        | Drag::ResizeMarker { .. }
        | Drag::MoveChord { .. }
        | Drag::ResizeChord { .. }
        | Drag::MoveTrack { .. }
        | Drag::Notes { .. }
        | Drag::NoteResize { .. }
        | Drag::Velocity { .. }
        | Drag::Marquee { .. }
        | Drag::CcStroke { .. }
        | Drag::AutoPoint { .. }
        | Drag::StepPaint { .. } => None,
    }
}

fn restore(enabled: bool, start: f64, end: f64, gesture: u64) -> (Command, String) {
    (Command::SetCycle { enabled, start: Some(start), end: Some(end) }, cycle_gesture_key(gesture))
}

/// Drops the drag in progress and returns what its cancel sends. The release that follows finds no drag.
pub fn take_cancelled(drag: &mut Option<Drag>) -> Option<(Command, String)> {
    drag.take().and_then(|d| cancel_command(&d))
}

/// Removes every Esc press of this frame (key repeats too, whatever the modifiers: a box selection is dragged
/// with Ctrl or Shift held) and tells whether there was one.
pub fn take_escape(events: &mut Vec<egui::Event>) -> bool {
    crate::editors::take_event(events, |e| matches!(e, egui::Event::Key { key: egui::Key::Escape, pressed: true, .. }))
}

/// The button that holds `drag`: the controller lane erases with the right button, every other drag is a
/// left-button drag.
pub fn held_button(drag: &Drag) -> egui::PointerButton {
    match drag {
        Drag::CcStroke { stroke, .. } if stroke.erase => egui::PointerButton::Secondary,
        _ => egui::PointerButton::Primary,
    }
}

/// Whether this frame's pointer events end `drag`: the button that holds it came up, or a release left no
/// button down. The second case ends the drags egui starts with the right or middle button too (most editors
/// start a drag on `drag_started()`, whatever the button). A right or middle click while the left button is
/// still down leaves a left drag running.
pub fn drag_finished(pointer: &egui::PointerState, drag: Option<&Drag>) -> bool {
    drag.is_some_and(|d| pointer.button_released(held_button(d)) || (pointer.any_released() && !pointer.any_down()))
}

/// Whether a context menu may be shown: not while a drag is in progress, unless it was already open.
pub fn menu_may_open(dragging: bool, open: bool) -> bool {
    !dragging || open
}

/// `Response::context_menu` that a right click during a drag does not open: egui gives a right click made with
/// the left button still down to the widget under the pointer.
pub fn menu_unless_dragging(resp: &egui::Response, dragging: bool, add_contents: impl FnOnce(&mut egui::Ui)) {
    if menu_may_open(dragging, resp.context_menu_opened()) {
        resp.context_menu(add_contents);
    }
}

impl OndinaApp {
    /// Esc with a drag in progress cancels it. Without a drag Esc is left to the rename fields and the
    /// unsaved-changes prompt, so it is consumed only here.
    pub(crate) fn cancel_drag_on_escape(&mut self, ctx: &egui::Context) {
        if self.drag.is_none() || !ctx.input_mut(|i| take_escape(&mut i.events)) {
            return;
        }
        if let Some((c, key)) = take_cancelled(&mut self.drag) {
            self.cmd_merge(c, key);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::{
        cancel_command, drag_finished, held_button, menu_may_open, menu_unless_dragging, take_cancelled, take_escape,
    };
    use crate::app::Drag;
    use crate::arrange::{cycle_gesture_key, CycleZone};
    use crate::automation_lane::{lane_spec, PointDrag};
    use crate::cc_lane::CcStroke;
    use crate::note_drag::VelocityStroke;
    use crate::region_edit::FadeHandle;
    use crate::step_paint::StepStroke;
    use eframe::egui::{self, pos2, vec2, Event, Key, Modifiers, PointerButton, Pos2, Rect, Sense};
    use ondina_core::command::Command;
    use ondina_core::model::{AutomationPoint, Cycle, Note, RegionContent, TrackKind};
    use ondina_studio::{Session, Source};
    use serde_json::{json, Value};

    fn run(s: &mut Session, name: &str, args: Value) -> Value {
        let c = Command::from_tool_call(name, &args).unwrap();
        s.execute(c, Source::Gui).unwrap_or_else(|e| panic!("{name} {args} failed: {e}"))
    }

    fn notes() -> Vec<Note> {
        vec![
            Note { pitch: 60, velocity: 100, start: 0.0, length: 1.0 },
            Note { pitch: 64, velocity: 90, start: 1.0, length: 0.5 },
        ]
    }

    fn is_set_cycle(r: &Option<(Command, String)>, enabled: bool, start: f64, end: f64, key: &str) -> bool {
        matches!(
            r,
            Some((Command::SetCycle { enabled: e, start: Some(s), end: Some(t) }, k))
                if *e == enabled && *s == start && *t == end && k == key
        )
    }

    fn key(code: Key, pressed: bool, repeat: bool, modifiers: Modifiers) -> Event {
        Event::Key { key: code, physical_key: None, pressed, repeat, modifiers }
    }

    fn moved(p: Pos2) -> Event {
        Event::PointerMoved(p)
    }

    fn button(p: Pos2, b: PointerButton, pressed: bool) -> Event {
        Event::PointerButton { pos: p, button: b, pressed, modifiers: Modifiers::NONE }
    }

    fn raw(k: usize, events: Vec<Event>) -> egui::RawInput {
        egui::RawInput {
            screen_rect: Some(Rect::from_min_size(Pos2::ZERO, vec2(800.0, 600.0))),
            time: Some(k as f64 * 0.016),
            events,
            ..Default::default()
        }
    }

    /// One frame per entry; what `drag_finished` says for `drag` in each.
    fn finished_per_frame(frames: Vec<Vec<Event>>, drag: Option<&Drag>) -> Vec<bool> {
        let ctx = egui::Context::default();
        let mut out = Vec::new();
        for (k, events) in frames.into_iter().enumerate() {
            let _ = ctx.run(raw(k, events), |ctx| out.push(ctx.input(|i| drag_finished(&i.pointer, drag))));
        }
        out
    }

    const A: Pos2 = pos2(200.0, 130.0);
    const B: Pos2 = pos2(500.0, 130.0);
    const C: Pos2 = pos2(520.0, 200.0);

    /// A left drag from A to C with a right click and a middle click at B on the way.
    fn left_drag_with_other_clicks() -> Vec<Vec<Event>> {
        vec![
            vec![moved(A)],
            vec![button(A, PointerButton::Primary, true)],
            vec![moved(B)],
            vec![button(B, PointerButton::Secondary, true)],
            vec![button(B, PointerButton::Secondary, false)],
            vec![button(B, PointerButton::Middle, true)],
            vec![button(B, PointerButton::Middle, false)],
            vec![moved(C)],
            vec![button(C, PointerButton::Primary, false)],
        ]
    }

    fn move_region() -> Drag {
        Drag::MoveRegion { id: 3, grab_beat: 2.0, orig_start: 0.0, orig_track: 1, cur_start: 4.0, cur_track: 2, copy: false }
    }

    #[test]
    fn held_button_is_the_left_one_except_for_a_cc_erase_stroke() {
        let erase = Drag::CcStroke { region: 3, stroke: CcStroke::new(64, true, 0.25, 4.0) };
        let draw = Drag::CcStroke { region: 3, stroke: CcStroke::new(64, false, 0.25, 4.0) };
        let step = Drag::StepPaint { region: 3, stroke: StepStroke::begin(0, 0, 0) };
        assert_eq!(held_button(&erase), PointerButton::Secondary);
        assert_eq!(held_button(&draw), PointerButton::Primary);
        assert_eq!(held_button(&move_region()), PointerButton::Primary);
        assert_eq!(held_button(&step), PointerButton::Primary);
    }

    #[test]
    fn a_right_or_middle_click_does_not_end_a_left_drag() {
        let drags = vec![
            move_region(),
            Drag::Notes { region: 3, orig: notes(), indices: vec![0, 1], grab_beat: 0.5, grab_pitch: 60, d_beat: 1.0, d_pitch: 2 },
            Drag::StepPaint { region: 3, stroke: StepStroke::begin(0, 0, 0) },
            Drag::CcStroke { region: 3, stroke: CcStroke::new(64, false, 0.25, 4.0) },
        ];
        let mut want = vec![false; 8];
        want.push(true);
        for d in &drags {
            assert_eq!(finished_per_frame(left_drag_with_other_clicks(), Some(d)), want);
        }
    }

    #[test]
    fn the_left_release_ends_the_drag_even_with_the_right_still_down() {
        let d = move_region();
        let start = || {
            vec![
                vec![moved(A)],
                vec![button(A, PointerButton::Primary, true)],
                vec![moved(B)],
                vec![button(B, PointerButton::Secondary, true)],
            ]
        };
        let mut frames = start();
        frames.push(vec![button(B, PointerButton::Primary, false)]);
        assert_eq!(finished_per_frame(frames, Some(&d)), vec![false, false, false, false, true]);
        let mut frames = start();
        frames.push(vec![button(B, PointerButton::Secondary, false), button(B, PointerButton::Primary, false)]);
        assert_eq!(finished_per_frame(frames, Some(&d)), vec![false, false, false, false, true]);
    }

    #[test]
    fn a_cc_erase_stroke_ends_on_the_right_release_not_on_a_left_click() {
        let d = Drag::CcStroke { region: 3, stroke: CcStroke::new(64, true, 0.25, 4.0) };
        let frames = vec![
            vec![moved(A)],
            vec![button(A, PointerButton::Secondary, true)],
            vec![moved(B)],
            vec![button(B, PointerButton::Primary, true)],
            vec![button(B, PointerButton::Primary, false)],
            vec![button(B, PointerButton::Secondary, false)],
        ];
        assert_eq!(finished_per_frame(frames, Some(&d)), vec![false, false, false, false, false, true]);
    }

    #[test]
    fn a_drag_egui_started_with_another_button_ends_when_no_button_is_left_down() {
        let d = move_region();
        for b in [PointerButton::Secondary, PointerButton::Middle] {
            let frames = vec![vec![moved(A)], vec![button(A, b, true)], vec![moved(B)], vec![button(B, b, false)]];
            assert_eq!(finished_per_frame(frames, Some(&d)), vec![false, false, false, true], "{b:?}");
        }
    }

    #[test]
    fn without_a_drag_nothing_ends() {
        assert!(finished_per_frame(left_drag_with_other_clicks(), None).iter().all(|f| !f));
    }

    #[test]
    fn a_menu_opens_only_without_a_drag_or_when_already_open() {
        assert!(menu_may_open(false, false));
        assert!(menu_may_open(false, true));
        assert!(menu_may_open(true, true));
        assert!(!menu_may_open(true, false));
    }

    /// A left drag from one widget with a right click on another on the way: whether egui gave the target a
    /// right click, and whether a context menu is open right after `menu_unless_dragging` in the last frame.
    fn right_click_during_drag(dragging: bool) -> (bool, bool) {
        let frames = vec![
            vec![moved(A)],
            vec![button(A, PointerButton::Primary, true)],
            vec![moved(B)],
            vec![button(B, PointerButton::Secondary, true)],
            vec![button(B, PointerButton::Secondary, false)],
        ];
        let last = frames.len() - 1;
        let ctx = egui::Context::default();
        let (mut clicked, mut open) = (false, false);
        for (k, events) in frames.into_iter().enumerate() {
            let _ = ctx.run(raw(k, events), |ctx| {
                egui::CentralPanel::default().show(ctx, |ui| {
                    let src = Rect::from_min_max(pos2(100.0, 100.0), pos2(300.0, 160.0));
                    let _ = ui.interact(src, egui::Id::new("src"), Sense::click_and_drag());
                    let rect = Rect::from_min_max(pos2(400.0, 100.0), pos2(600.0, 160.0));
                    let target = ui.interact(rect, egui::Id::new("target"), Sense::click_and_drag());
                    clicked |= target.secondary_clicked();
                    menu_unless_dragging(&target, dragging, |ui| {
                        ui.label("menu");
                    });
                    if k == last {
                        open = ui.ctx().is_context_menu_open();
                    }
                });
            });
        }
        (clicked, open)
    }

    #[test]
    fn a_right_click_during_a_drag_opens_no_context_menu() {
        assert_eq!(right_click_during_drag(false), (true, true));
        assert_eq!(right_click_during_drag(true), (true, false));
    }

    #[test]
    fn drags_that_commit_on_release_send_nothing_when_cancelled() {
        let mut stroke = CcStroke::new(64, false, 0.25, 4.0);
        stroke.add(1.0, 100);
        let drags = vec![
            Drag::MoveRegion { id: 3, grab_beat: 2.0, orig_start: 0.0, orig_track: 1, cur_start: 4.0, cur_track: 2, copy: false },
            Drag::MoveRegion { id: 3, grab_beat: 2.0, orig_start: 0.0, orig_track: 1, cur_start: 4.0, cur_track: 1, copy: true },
            Drag::ResizeRegion { id: 3, orig_len: 4.0, cur_len: 6.0 },
            Drag::RegionFade { id: 3, handle: FadeHandle::In, grab_beat: 0.0, orig: 0.0, cur: 1.5 },
            Drag::MoveMarker { id: 5, grab_beat: 8.0, orig_beat: 8.0, cur_beat: 12.0 },
            Drag::ResizeMarker { id: 5, start: 8.0, orig_len: 4.0, cur_len: 8.0 },
            Drag::MoveChord { id: 6, grab_beat: 0.0, orig_beat: 0.0, cur_beat: 2.0 },
            Drag::ResizeChord { id: 6, start: 0.0, orig_len: 4.0, cur_len: 2.0, next_start: Some(4.0) },
            Drag::MoveTrack { id: 2, gap: 0 },
            Drag::Notes { region: 3, orig: notes(), indices: vec![0, 1], grab_beat: 0.5, grab_pitch: 60, d_beat: 1.0, d_pitch: 2 },
            Drag::NoteResize { region: 3, orig: notes(), indices: vec![1], grab_beat: 1.5, d_len: 0.5 },
            Drag::Velocity { region: 3, stroke: VelocityStroke::new(notes()) },
            Drag::Marquee { region: 3, b0: 0.0, p0: 60.0, b1: 2.0, p1: 66.0, additive: true },
            Drag::CcStroke { region: 3, stroke },
        ];
        for d in &drags {
            assert!(cancel_command(d).is_none());
        }
    }

    #[test]
    fn cancelled_cycle_drags_restore_the_cycle_they_started_from() {
        let drawn = Drag::Cycle { anchor: 4.0, gesture: 7, orig: Cycle { enabled: false, start: 8.0, end: 16.0 }, sent: true };
        assert!(is_set_cycle(&cancel_command(&drawn), false, 8.0, 16.0, &cycle_gesture_key(7)));
        let edited = Drag::CycleEdit {
            zone: CycleZone::End,
            grab_beat: 16.0,
            start: 8.0,
            end: 16.0,
            enabled: true,
            gesture: 9,
            sent: Some((8.0, 20.0)),
            sent_at: 1.0,
        };
        assert!(is_set_cycle(&cancel_command(&edited), true, 8.0, 16.0, &cycle_gesture_key(9)));
    }

    #[test]
    fn cycle_drags_that_sent_nothing_send_nothing() {
        let drawn = Drag::Cycle { anchor: 4.0, gesture: 7, orig: Cycle { enabled: true, start: 0.0, end: 16.0 }, sent: false };
        assert!(cancel_command(&drawn).is_none());
        let edited = Drag::CycleEdit {
            zone: CycleZone::Body,
            grab_beat: 10.0,
            start: 8.0,
            end: 16.0,
            enabled: true,
            gesture: 9,
            sent: None,
            sent_at: 0.0,
        };
        assert!(cancel_command(&edited).is_none());
    }

    #[test]
    fn a_cancelled_drag_is_gone_before_the_release() {
        let mut d = Some(Drag::MoveRegion { id: 3, grab_beat: 2.0, orig_start: 0.0, orig_track: 1, cur_start: 4.0, cur_track: 1, copy: false });
        assert!(take_cancelled(&mut d).is_none());
        assert!(d.is_none());
        let mut d = Some(Drag::CycleEdit {
            zone: CycleZone::Body,
            grab_beat: 10.0,
            start: 8.0,
            end: 16.0,
            enabled: true,
            gesture: 4,
            sent: Some((9.0, 17.0)),
            sent_at: 0.5,
        });
        assert!(is_set_cycle(&take_cancelled(&mut d), true, 8.0, 16.0, &cycle_gesture_key(4)));
        assert!(d.is_none());
        // the release that follows finds nothing to send
        assert!(take_cancelled(&mut d).is_none());
        assert!(d.is_none());
    }

    #[test]
    fn escape_is_taken_with_any_modifiers_and_other_keys_stay() {
        let mut events = vec![
            key(Key::Escape, true, false, Modifiers::CTRL),
            key(Key::A, true, false, Modifiers::NONE),
            key(Key::Escape, true, true, Modifiers::NONE),
            key(Key::Escape, false, false, Modifiers::NONE),
            Event::Copy,
        ];
        assert!(take_escape(&mut events));
        assert_eq!(
            events,
            vec![key(Key::A, true, false, Modifiers::NONE), key(Key::Escape, false, false, Modifiers::NONE), Event::Copy]
        );
        assert!(!take_escape(&mut events));
        assert_eq!(events.len(), 3);
        assert!(!take_escape(&mut Vec::new()));
    }

    #[test]
    fn cancelling_a_cycle_move_leaves_the_cycle_and_one_step_that_changes_nothing() {
        let mut s = Session::new(false);
        run(&mut s, "set_cycle", json!({"enabled": true, "start": 8.0, "end": 16.0}));
        let before = s.project.cycle;
        let gesture = 1001;
        let key = cycle_gesture_key(gesture);
        let mut drag = Some(Drag::CycleEdit {
            zone: CycleZone::Body,
            grab_beat: 10.0,
            start: 8.0,
            end: 16.0,
            enabled: true,
            gesture,
            sent: None,
            sent_at: 0.0,
        });
        for span in [(9.0, 17.0), (11.0, 19.0), (12.0, 20.0)] {
            let c = Command::SetCycle { enabled: true, start: Some(span.0), end: Some(span.1) };
            s.execute_coalesced(c, &key).unwrap();
            if let Some(Drag::CycleEdit { sent, .. }) = &mut drag {
                *sent = Some(span);
            }
        }
        assert_eq!(s.project.cycle, Cycle { enabled: true, start: 12.0, end: 20.0 });

        let (c, k) = take_cancelled(&mut drag).unwrap();
        s.execute_coalesced(c, &k).unwrap();
        assert_eq!(s.project.cycle, before);
        assert!(drag.is_none());

        // the gesture left one step, and that step changes nothing
        run(&mut s, "undo", json!({}));
        assert_eq!(s.project.cycle, before);
        run(&mut s, "undo", json!({}));
        assert_eq!(s.project.cycle, Cycle { enabled: false, start: 0.0, end: 16.0 });
    }

    #[test]
    fn cancelling_a_drawn_cycle_brings_back_the_old_one() {
        let mut s = Session::new(false);
        run(&mut s, "set_cycle", json!({"enabled": false, "start": 8.0, "end": 16.0}));
        let before = s.project.cycle;
        let gesture = 1002;
        let key = cycle_gesture_key(gesture);
        let mut drag = Some(Drag::Cycle { anchor: 24.0, gesture, orig: before, sent: false });
        for end in [28.0, 32.0] {
            let c = Command::SetCycle { enabled: true, start: Some(24.0), end: Some(end) };
            s.execute_coalesced(c, &key).unwrap();
            if let Some(Drag::Cycle { sent, .. }) = &mut drag {
                *sent = true;
            }
        }
        assert_eq!(s.project.cycle, Cycle { enabled: true, start: 24.0, end: 32.0 });

        let (c, k) = take_cancelled(&mut drag).unwrap();
        s.execute_coalesced(c, &k).unwrap();
        assert_eq!(s.project.cycle, before);
        assert!(!s.project.cycle.enabled);
        assert!(drag.is_none());

        run(&mut s, "undo", json!({}));
        assert_eq!(s.project.cycle, before);
    }

    #[test]
    fn cancelling_an_automation_drag_sends_nothing_and_leaves_the_lane() {
        let mut s = Session::new(false);
        let add = Command::AddTrack { kind: TrackKind::Audio, name: None, patch: None, instrument: None, preset: None, index: None };
        let tid = s.execute(add, Source::Gui).unwrap()["track_id"].as_u64().unwrap();
        let points = vec![AutomationPoint { beat: 0.0, value: -6.0 }, AutomationPoint { beat: 4.0, value: 0.0 }];
        s.execute(Command::SetAutomation { track_id: tid, target: "volume".into(), points: points.clone() }, Source::Gui).unwrap();
        let undo_before = s.can_undo().map(String::from);
        let lane = |s: &Session| s.project.track(tid).unwrap().automation[0].points.clone();

        let spec = lane_spec(&s.project, s.project.track(tid).unwrap(), "volume").unwrap();
        let mut drag = PointDrag::existing(spec, points.clone(), 1, 4.0, 50.0);
        drag.update(6.0, 30.0, 60.0, 20.0, 0.25);
        assert_ne!(drag.cur, drag.orig);
        let d = Drag::AutoPoint { track: tid, drag };
        assert!(cancel_command(&d).is_none());
        let mut d = Some(d);
        assert!(take_cancelled(&mut d).is_none());
        assert!(d.is_none());
        assert_eq!(lane(&s), points);
        assert_eq!(s.can_undo().map(String::from), undo_before);
    }

    #[test]
    fn a_cancelled_step_paint_sends_nothing_and_leaves_the_pattern() {
        let mut s = Session::new(false);
        let tid = run(&mut s, "add_track", json!({"kind": "instrument", "patch": "Pop Drum Kit"}))["track_id"].as_u64().unwrap();
        let rid = run(&mut s, "create_pattern_region", json!({"track_id": tid, "start": 0, "steps": 16}))["region_id"]
            .as_u64()
            .unwrap();
        run(&mut s, "set_pattern_step", json!({"region_id": rid, "row": 0, "step": 2, "velocity": 120}));
        let pattern = |s: &Session| match &s.project.find_region(rid).unwrap().1.content {
            RegionContent::Pattern(p) => p.rows.iter().map(|r| r.steps.clone()).collect::<Vec<_>>(),
            _ => panic!("not a pattern"),
        };
        let before = pattern(&s);
        let undo_before = s.can_undo().map(String::from);

        let mut stroke = StepStroke::begin(0, 0, 0);
        stroke.reach(0, 3);
        let d = Drag::StepPaint { region: rid, stroke };
        assert!(cancel_command(&d).is_none());
        let mut d = Some(d);
        assert!(take_cancelled(&mut d).is_none());
        assert!(d.is_none());
        assert_eq!(pattern(&s), before);
        assert_eq!(s.can_undo().map(String::from), undo_before);
    }
}
