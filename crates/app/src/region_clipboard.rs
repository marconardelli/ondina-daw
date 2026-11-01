//! The arrangement's region clipboard: what Ctrl+C / Ctrl+X keep, where Ctrl+V puts the copy and which clipboard,
//! notes or region, a key goes to. Pure functions, so the GUI only wires keys and menu items to them.

use ondina_core::model::{Id, Project, Region, RegionContent, TrackKind};

/// A region copied or cut in the arrangement. Lives in the app until it closes; never saved.
#[derive(Debug, Clone)]
pub struct RegionClip {
    pub region: Region,
    /// The track it came from: where Ctrl+V pastes when no track is selected.
    pub track_id: Id,
    /// `Session::document` when it was copied: an audio file id means nothing in another project.
    pub document: u64,
}

/// Which clipboard a Ctrl+C, Ctrl+X or Ctrl+V goes to.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ClipKind {
    Notes,
    Region,
}

/// The region with its track, as it is now in `project`. `None` when the region is gone.
pub fn copy(project: &Project, region_id: Id, document: u64) -> Option<RegionClip> {
    project.find_region(region_id).map(|(t, r)| RegionClip { region: r.clone(), track_id: t.id, document })
}

/// The copied region as JSON, the object `paste_region` takes. Never empty: the system clipboard has to hold text
/// for Ctrl+V to reach the app.
pub fn clip_text(clip: &RegionClip) -> String {
    serde_json::to_string(&clip.region).unwrap_or_else(|_| "{}".into())
}

/// Where the copy starts: the playhead moved back to the grid line at or before it (`snap` beats, 0 = no grid),
/// like the other "at playhead" actions of the arrangement (`OndinaApp::snap_floor`). Never before 0.
pub fn paste_start(playhead: f64, snap: f64) -> f64 {
    let start = if snap > 0.0 { (playhead / snap).floor() * snap } else { playhead };
    start.max(0.0)
}

/// The track kind a region plays on: audio regions on audio tracks, MIDI and pattern regions on instrument tracks.
pub fn track_kind_for(region: &Region) -> TrackKind {
    match region.content {
        RegionContent::Audio(_) => TrackKind::Audio,
        RegionContent::Midi(_) | RegionContent::Pattern(_) => TrackKind::Instrument,
    }
}

/// The track Ctrl+V pastes on: the selected track, otherwise the one the region came from. An error message when
/// neither exists or the track cannot play the region.
pub fn target_track(project: &Project, clip: &RegionClip, selected_track: Option<Id>) -> Result<Id, String> {
    let track = selected_track.and_then(|id| project.track(id)).or_else(|| project.track(clip.track_id));
    let Some(track) = track else { return Err("select a track to paste the region on".into()) };
    let kind = track_kind_for(&clip.region);
    if track.kind != kind {
        return Err(if kind == TrackKind::Audio { "an audio region needs an audio track" } else { "MIDI and pattern regions need an instrument track" }.into());
    }
    Ok(track.id)
}

/// Track and start of the `PasteRegion` that Ctrl+V runs. An audio region copied in another document is refused:
/// its file id would point at a different file or at none.
pub fn paste_spot(project: &Project, clip: &RegionClip, selected_track: Option<Id>, playhead: f64, snap: f64, document: u64) -> Result<(Id, f64), String> {
    if matches!(clip.region.content, RegionContent::Audio(_)) && clip.document != document {
        return Err("the copied audio region belongs to another project".into());
    }
    Ok((target_track(project, clip, selected_track)?, paste_start(playhead, snap)))
}

/// Who takes Ctrl+C / Ctrl+X: the piano roll when its editing keys act on a MIDI region (`note_keys`) and notes
/// are selected, otherwise the selected region. Nothing during a drag.
pub fn copy_target(note_keys: bool, notes_selected: bool, region_selected: bool, dragging: bool) -> Option<ClipKind> {
    if dragging {
        None
    } else if note_keys && notes_selected {
        Some(ClipKind::Notes)
    } else if region_selected {
        Some(ClipKind::Region)
    } else {
        None
    }
}

/// Who takes Ctrl+V: the piano roll when its editing keys act and it holds copied notes, otherwise the region
/// clipboard. A copy of one kind empties the other, so the most recent copy decides. Nothing during a drag.
pub fn paste_target(note_keys: bool, note_clip: bool, region_clip: bool, dragging: bool) -> Option<ClipKind> {
    if dragging {
        None
    } else if note_keys && note_clip {
        Some(ClipKind::Notes)
    } else if region_clip {
        Some(ClipKind::Region)
    } else {
        None
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use ondina_core::command::Command;
    use ondina_core::model::{AudioContent, MidiContent, Note};
    use ondina_studio::{Session, Source};
    use serde_json::{json, Value};

    fn run(s: &mut Session, name: &str, args: Value, src: Source) -> Value {
        let c = Command::from_tool_call(name, &args).unwrap();
        s.execute(c, src).unwrap_or_else(|e| panic!("{name} {args} failed: {e}"))
    }

    fn add_track(s: &mut Session, kind: &str) -> Id {
        run(s, "add_track", json!({"kind": kind}), Source::Gui)["track_id"].as_u64().unwrap()
    }

    fn midi_region(s: &mut Session, track_id: Id, start: f64) -> Id {
        let notes = json!([{"pitch": 60, "velocity": 100, "start": 0.0, "length": 1.0}, {"pitch": 67, "velocity": 90, "start": 2.0, "length": 1.5}]);
        run(s, "create_midi_region", json!({"track_id": track_id, "start": start, "length": 8, "notes": notes}), Source::Gui)["region_id"].as_u64().unwrap()
    }

    fn region(content: RegionContent) -> Region {
        Region { id: 1, name: "r".into(), start: 0.0, length: 4.0, loop_count: 1, muted: false, color: None, content, take: None }
    }

    fn midi_clip(track_id: Id, document: u64) -> RegionClip {
        let notes = vec![Note { pitch: 60, velocity: 100, start: 0.0, length: 1.0 }];
        RegionClip { region: region(RegionContent::Midi(MidiContent { notes, ..MidiContent::default() })), track_id, document }
    }

    fn audio_clip(track_id: Id, document: u64) -> RegionClip {
        let content = RegionContent::Audio(AudioContent { file_id: 1, offset_sec: 0.0, gain_db: 0.0, fade_in: 0.0, fade_out: 0.0, reverse: false });
        RegionClip { region: region(content), track_id, document }
    }

    fn without_id_start(r: &Region) -> Value {
        let mut v = serde_json::to_value(r).unwrap();
        let o = v.as_object_mut().unwrap();
        o.remove("id");
        o.remove("start");
        v
    }

    #[test]
    fn copy_keeps_the_region_its_track_and_the_document() {
        let mut s = Session::new(false);
        let t = add_track(&mut s, "instrument");
        let rid = midi_region(&mut s, t, 4.0);
        let c = copy(&s.project, rid, 7).unwrap();
        let orig = s.project.find_region(rid).unwrap().1;
        assert_eq!(serde_json::to_value(&c.region).unwrap(), serde_json::to_value(orig).unwrap());
        assert_eq!(c.track_id, t);
        assert_eq!(c.document, 7);
        assert!(copy(&s.project, 999_999, 7).is_none());
    }

    #[test]
    fn clip_text_is_the_region_as_json() {
        let c = midi_clip(3, 0);
        let text = clip_text(&c);
        assert!(!text.is_empty());
        let back: Region = serde_json::from_str(&text).unwrap();
        assert_eq!(back.id, c.region.id);
        assert_eq!(back.name, c.region.name);
        assert_eq!(serde_json::to_value(&back.content).unwrap(), serde_json::to_value(&c.region.content).unwrap());
    }

    #[test]
    fn paste_start_snaps_back_to_the_grid() {
        assert_eq!(paste_start(9.7, 1.0), 9.0);
        assert_eq!(paste_start(9.7, 4.0), 8.0);
        assert_eq!(paste_start(1.3, 0.25), 1.25);
        assert_eq!(paste_start(9.7, 0.0), 9.7);
        assert_eq!(paste_start(0.0, 1.0), 0.0);
        assert_eq!(paste_start(-0.5, 0.0), 0.0);
    }

    #[test]
    fn paste_goes_to_the_selected_track_then_to_the_origin() {
        let mut s = Session::new(false);
        let origin = add_track(&mut s, "instrument");
        let other = add_track(&mut s, "instrument");
        let c = midi_clip(origin, s.document);
        assert_eq!(target_track(&s.project, &c, Some(other)), Ok(other));
        assert_eq!(target_track(&s.project, &c, None), Ok(origin));
        assert_eq!(target_track(&s.project, &c, Some(999_999)), Ok(origin));
        run(&mut s, "remove_track", json!({"track_id": origin}), Source::Gui);
        let e = target_track(&s.project, &c, None).unwrap_err();
        assert!(e.contains("select a track"), "{e}");
    }

    #[test]
    fn paste_refuses_a_track_of_the_wrong_kind() {
        let mut s = Session::new(false);
        let inst = add_track(&mut s, "instrument");
        let audio = add_track(&mut s, "audio");
        let bus = add_track(&mut s, "bus");
        let e = target_track(&s.project, &midi_clip(inst, 0), Some(audio)).unwrap_err();
        assert!(e.contains("instrument track"), "{e}");
        let e = target_track(&s.project, &audio_clip(audio, 0), Some(inst)).unwrap_err();
        assert!(e.contains("audio track"), "{e}");
        let pattern = RegionContent::Pattern(ondina_core::model::PatternContent { steps: 16, step_length: 0.25, rows: vec![], swing: 0.0 });
        let pc = RegionClip { region: region(pattern), track_id: inst, document: 0 };
        assert_eq!(track_kind_for(&pc.region), TrackKind::Instrument);
        assert!(target_track(&s.project, &pc, Some(bus)).is_err());
        assert_eq!(track_kind_for(&audio_clip(audio, 0).region), TrackKind::Audio);
    }

    #[test]
    fn audio_from_another_document_is_refused_midi_is_not() {
        let mut s = Session::new(false);
        let inst = add_track(&mut s, "instrument");
        let audio = add_track(&mut s, "audio");
        let doc = s.document;
        let e = paste_spot(&s.project, &audio_clip(audio, doc + 1), None, 4.0, 1.0, doc).unwrap_err();
        assert!(e.contains("another project"), "{e}");
        assert_eq!(paste_spot(&s.project, &audio_clip(audio, doc), None, 4.5, 1.0, doc), Ok((audio, 4.0)));
        assert_eq!(paste_spot(&s.project, &midi_clip(inst, doc + 1), None, 4.5, 0.0, doc), Ok((inst, 4.5)));
    }

    #[test]
    fn notes_win_copy_while_the_piano_roll_has_a_selection() {
        assert_eq!(copy_target(true, true, true, false), Some(ClipKind::Notes));
        assert_eq!(copy_target(true, false, true, false), Some(ClipKind::Region));
        assert_eq!(copy_target(false, true, true, false), Some(ClipKind::Region));
        assert_eq!(copy_target(false, false, false, false), None);
        assert_eq!(copy_target(true, true, true, true), None);
        assert_eq!(copy_target(false, false, true, true), None);
    }

    #[test]
    fn the_latest_copy_decides_paste() {
        assert_eq!(paste_target(true, true, false, false), Some(ClipKind::Notes));
        assert_eq!(paste_target(true, false, true, false), Some(ClipKind::Region));
        assert_eq!(paste_target(false, true, false, false), None);
        assert_eq!(paste_target(false, false, true, false), Some(ClipKind::Region));
        assert_eq!(paste_target(true, false, false, false), None);
        assert_eq!(paste_target(true, false, true, true), None);
    }

    #[test]
    fn cut_then_paste_at_the_playhead_is_two_undo_steps() {
        let mut s = Session::new(false);
        let t = add_track(&mut s, "instrument");
        let rid = midi_region(&mut s, t, 4.0);
        let before = serde_json::to_value(s.project.find_region(rid).unwrap().1).unwrap();
        let clip = copy(&s.project, rid, s.document).unwrap();
        s.execute(Command::DeleteRegion { region_id: rid }, Source::Gui).unwrap();
        assert!(s.project.find_region(rid).is_none());

        assert_eq!(paste_spot(&s.project, &clip, None, 20.3, 1.0, s.document), Ok((t, 20.0)));
        let v = s.execute(Command::PasteRegion { track_id: t, start: 20.0, region: Box::new(clip.region.clone()) }, Source::Gui).unwrap();
        let new = v["region_id"].as_u64().unwrap();
        assert_ne!(new, rid);
        let (track, pasted) = s.project.find_region(new).unwrap();
        assert_eq!(track.id, t);
        assert_eq!(pasted.start, 20.0);
        assert_eq!(without_id_start(pasted), without_id_start(&clip.region));

        run(&mut s, "undo", json!({}), Source::Gui);
        assert!(s.project.find_region(new).is_none());
        assert!(s.project.find_region(rid).is_none());
        run(&mut s, "undo", json!({}), Source::Gui);
        let (track, back) = s.project.find_region(rid).unwrap();
        assert_eq!(track.id, t);
        assert_eq!(back.start, 4.0);
        assert_eq!(serde_json::to_value(back).unwrap(), before);
    }
}
