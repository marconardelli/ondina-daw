//! Model-level execution for the extended command set: comping, score/MusicXML, lyrics,
//! Clip Grid editing, surround settings, movie settings, MIDI Transform, routing and region paste.

use serde_json::{json, Value};

use crate::command::{Command, NoteFilter, NoteOps};
use crate::model::*;
use crate::ops::{pattern_to_notes, Outcome};
use crate::theory::{estimate_key, Key};

type R = Result<Outcome, String>;

/// Below this many notes the key estimate is a guess, whatever its score.
const KEY_MIN_NOTES: usize = 8;
/// A best-key confidence under this is reported as low.
const KEY_MIN_CONFIDENCE: f32 = 0.6;

fn ok(v: Value) -> R {
    Ok(Outcome::Done(v))
}

pub fn apply(project: &mut Project, cmd: &Command) -> R {
    use Command::*;
    match cmd {
        // ---------------------------------------------------------------- comping
        ListTakes { track_id } => {
            let t = project.track(*track_id).ok_or("track not found")?;
            let mut regs: Vec<&Region> = t.regions.iter().collect();
            regs.sort_by(|a, b| a.start.partial_cmp(&b.start).unwrap());
            let mut groups: Vec<Vec<Value>> = vec![];
            let mut cur_end = f64::MIN;
            for r in regs {
                let v = json!({"region_id": r.id, "name": r.name, "take": r.take, "start": r.start, "end": r.end(), "muted": r.muted});
                if r.start < cur_end - 1e-9 {
                    groups.last_mut().unwrap().push(v);
                } else {
                    groups.push(vec![v]);
                }
                cur_end = cur_end.max(r.end());
            }
            let take_groups: Vec<Value> = groups.into_iter().filter(|g| g.len() > 1 || g[0]["take"].is_number()).map(Value::from).collect();
            ok(json!({"take_groups": take_groups}))
        }
        CompTakes { track_id, segments } => {
            if segments.is_empty() {
                return Err("no segments".into());
            }
            let tempo = project.tempo.clone();
            let mut new_regions = vec![];
            let mut used: Vec<Id> = vec![];
            {
                let t = project.track(*track_id).ok_or("track not found")?;
                for seg in segments {
                    if seg.to_beat <= seg.from_beat {
                        return Err("segment end must be after start".into());
                    }
                    let r = t.regions.iter().find(|r| r.id == seg.region_id).ok_or_else(|| format!("region {} is not on this track", seg.region_id))?;
                    let from = seg.from_beat.max(r.start);
                    let to = seg.to_beat.min(r.end());
                    if to <= from {
                        return Err(format!("segment {}..{} is outside take {}", seg.from_beat, seg.to_beat, r.id));
                    }
                    let mut c = r.clone();
                    c.loop_count = 1;
                    c.muted = false;
                    c.take = None;
                    c.name = format!("Comp ({})", r.name);
                    let rel = from - r.start;
                    match &mut c.content {
                        RegionContent::Audio(a) => {
                            a.offset_sec += tempo.beat_to_sec(from) - tempo.beat_to_sec(r.start);
                            a.fade_in = a.fade_in.min(0.05).max(0.02);
                            a.fade_out = 0.02;
                        }
                        RegionContent::Midi(m) => {
                            m.notes = m.notes.iter().filter(|n| n.start >= rel && n.start < rel + (to - from)).map(|n| Note { start: n.start - rel, length: n.length.min(to - from - (n.start - rel)), ..*n }).collect();
                        }
                        RegionContent::Pattern(_) => return Err("pattern regions cannot be comped".into()),
                    }
                    c.start = from;
                    c.length = to - from;
                    new_regions.push(c);
                    used.push(r.id);
                }
            }
            let mut ids = vec![];
            let all_takes: Vec<Id> = project.track(*track_id).unwrap().regions.iter().filter(|r| used.contains(&r.id) || r.take.is_some()).map(|r| r.id).collect();
            for mut c in new_regions {
                c.id = project.alloc_id();
                ids.push(c.id);
                project.track_mut(*track_id).unwrap().regions.push(c);
            }
            for r in project.track_mut(*track_id).unwrap().regions.iter_mut() {
                if all_takes.contains(&r.id) {
                    r.muted = true;
                }
            }
            ok(json!({"comp_region_ids": ids, "muted_takes": all_takes}))
        }

        // ---------------------------------------------------------------- score
        GetScore { region_id } => {
            let (_, r) = project.find_region(*region_id).ok_or("region not found")?;
            let (notes, lyrics) = region_notes(r);
            let measures = notate(&notes, &lyrics, r.length, project.time_signature);
            ok(json!({"region_id": r.id, "time_signature": format!("{}/{}", project.time_signature.numerator, project.time_signature.denominator), "key": project.key.name(), "clef": clef_for(&notes), "measures": measures}))
        }
        SetLyrics { region_id, text, lyrics } => {
            let r = project.find_region_mut(*region_id).ok_or("region not found")?;
            let RegionContent::Midi(m) = &mut r.content else { return Err("lyrics need a MIDI region".into()) };
            if let Some(l) = lyrics {
                m.lyrics = l.clone();
            } else if let Some(t) = text {
                // melody line: highest note at each onset
                let mut onsets: Vec<f64> = vec![];
                let mut sorted = m.notes.clone();
                sorted.sort_by(|a, b| a.start.partial_cmp(&b.start).unwrap());
                for n in &sorted {
                    if onsets.last().map(|l| (n.start - l).abs() > 1e-6).unwrap_or(true) {
                        onsets.push(n.start);
                    }
                }
                let sylls: Vec<String> = t.split_whitespace().flat_map(|w| {
                    let parts: Vec<&str> = w.split('-').filter(|p| !p.is_empty()).collect();
                    let n = parts.len();
                    parts.into_iter().enumerate().map(move |(i, p)| if i + 1 < n { format!("{p}-") } else { p.to_string() })
                }).collect();
                m.lyrics = onsets.iter().zip(sylls.iter()).map(|(b, s)| LyricEvent { beat: *b, text: s.clone() }).collect();
            } else {
                return Err("give text or lyrics".into());
            }
            ok(json!({"lyric_count": m.lyrics.len()}))
        }

        // ---------------------------------------------------------------- clip grid (grid editing)
        AddScene { name } => {
            let n = project.live.scenes.len();
            if n >= MAX_SCENES {
                return Err(format!("the Clip Grid already has the most scenes it can have ({MAX_SCENES})"));
            }
            project.live.scenes.push(name.clone().unwrap_or_else(|| format!("Scene {}", n + 1)));
            ok(json!({"scene": n}))
        }
        RenameScene { scene, name } => {
            *project.live.scenes.get_mut(*scene).ok_or("scene not found")? = name.clone();
            ok(json!({"ok": true}))
        }
        RemoveScene { scene } => {
            if *scene >= project.live.scenes.len() {
                return Err("scene not found".into());
            }
            project.live.scenes.remove(*scene);
            project.live.cells.retain(|c| c.scene != *scene);
            for c in &mut project.live.cells {
                if c.scene > *scene {
                    c.scene -= 1;
                }
            }
            ok(json!({"ok": true}))
        }
        CreateLiveCell { track_id, scene, length, notes, from_region_id, name } => {
            let tkind = project.track(*track_id).ok_or("track not found")?.kind;
            if tkind == TrackKind::Bus {
                return Err("cells need an instrument or audio track".into());
            }
            if *scene >= MAX_SCENES {
                return Err(format!("scene {scene} is out of range: the Clip Grid has at most {MAX_SCENES} scenes (0..{})", MAX_SCENES - 1));
            }
            while project.live.scenes.len() <= *scene {
                let n = project.live.scenes.len();
                project.live.scenes.push(format!("Scene {}", n + 1));
            }
            let bpb = project.time_signature.beats_per_bar();
            let (content, len, nm) = if let Some(rid) = from_region_id {
                let (_, r) = project.find_region(*rid).ok_or("source region not found")?;
                (r.content.clone(), r.length, r.name.clone())
            } else {
                if tkind != TrackKind::Instrument {
                    return Err("empty/note cells need an instrument track; use from_region_id for audio".into());
                }
                let mut ns = notes.clone().unwrap_or_default();
                ns.sort_by(|a, b| a.start.partial_cmp(&b.start).unwrap());
                (RegionContent::Midi(MidiContent { notes: ns, ..Default::default() }), length.unwrap_or(bpb * 4.0), format!("Cell {}", project.live.cells.len() + 1))
            };
            let id = project.alloc_id();
            project.live.cells.retain(|c| !(c.track_id == *track_id && c.scene == *scene));
            project.live.cells.push(LiveCell { id, track_id: *track_id, scene: *scene, name: name.clone().unwrap_or(nm), length: length.unwrap_or(len).max(0.25), content });
            ok(json!({"cell_id": id}))
        }
        DeleteLiveCell { cell_id } => {
            let before = project.live.cells.len();
            project.live.cells.retain(|c| c.id != *cell_id);
            if before == project.live.cells.len() {
                return Err("cell not found".into());
            }
            ok(json!({"ok": true}))
        }
        SetLiveCellNotes { cell_id, notes } => {
            let c = project.live.cells.iter_mut().find(|c| c.id == *cell_id).ok_or("cell not found")?;
            match &mut c.content {
                RegionContent::Midi(m) => {
                    m.notes = notes.clone();
                    m.notes.sort_by(|a, b| a.start.partial_cmp(&b.start).unwrap());
                }
                _ => return Err("cell is not a MIDI cell".into()),
            }
            ok(json!({"ok": true}))
        }
        SetLiveQuantize { beats } => {
            project.live.quantize = beats.max(0.0);
            ok(json!({"quantize": project.live.quantize}))
        }

        // ---------------------------------------------------------------- surround
        SetSurroundFormat { format } => {
            project.surround = *format;
            ok(json!({"format": format}))
        }
        SetSurroundPan { track_id, azimuth, elevation, spread, object } => {
            let c = project.channel_mut(*track_id).ok_or("track not found")?;
            let mut az = *azimuth % 360.0;
            if az > 180.0 {
                az -= 360.0;
            }
            if az < -180.0 {
                az += 360.0;
            }
            c.surround_pan.azimuth = az;
            if let Some(e) = elevation {
                c.surround_pan.elevation = e.clamp(-30.0, 90.0);
            }
            if let Some(s) = spread {
                c.surround_pan.spread = s.clamp(0.0, 1.0);
            }
            if let Some(o) = object {
                c.surround_pan.object = *o;
            }
            ok(json!({"surround_pan": c.surround_pan}))
        }

        // ---------------------------------------------------------------- video settings
        SetMovieOffset { offset_sec } => {
            project.movie.as_mut().ok_or("no movie loaded")?.offset_sec = *offset_sec;
            ok(json!({"ok": true}))
        }
        RemoveMovie {} => {
            project.movie = None;
            ok(json!({"ok": true}))
        }
        SetFrameRate { fps } => {
            if !(1.0..=120.0).contains(fps) {
                return Err("fps must be between 1 and 120".into());
            }
            project.frame_rate = *fps;
            ok(json!({"frame_rate": fps}))
        }

        // ---------------------------------------------------------------- environment / transform
        TransformNotes { region_id, select, operations } => {
            let key = project.key;
            let r = project.find_region_mut(*region_id).ok_or("region not found")?;
            let len = r.length;
            let RegionContent::Midi(m) = &mut r.content else { return Err("not a MIDI region".into()) };
            let f = select.clone().unwrap_or_default();
            let (changed, deleted) = transform(&mut m.notes, &f, operations, key, len);
            ok(json!({"changed": changed, "deleted": deleted, "note_count": m.notes.len()}))
        }
        SetMidiRoutes { routes } => {
            for r in routes {
                let t = project.track(r.track_id).ok_or_else(|| format!("route '{}': track {} not found", r.name, r.track_id))?;
                if t.kind != TrackKind::Instrument {
                    return Err(format!("route '{}': target must be an instrument track", r.name));
                }
            }
            project.midi_routes = routes.clone();
            ok(json!({"routes": project.midi_routes}))
        }
        GetMidiRoutes {} => ok(json!({"routes": project.midi_routes})),

        // ---------------------------------------------------------------- region paste
        PasteRegion { track_id, start, region } => {
            let copy = pasted_region(project, *track_id, *start, region)?;
            let id = project.alloc_id();
            let start = copy.start;
            project.track_mut(*track_id).ok_or("track not found")?.regions.push(Region { id, ..copy });
            ok(json!({"region_id": id, "track_id": track_id, "start": start}))
        }

        // ---------------------------------------------------------------- key detection
        DetectKey { region_id, apply } => detect_key(project, *region_id, apply.unwrap_or(false)),

        _ => Ok(Outcome::NotHandled),
    }
}

/// Checks a project read from a file before anything plays it. What the commands refuse is refused here (a time
/// signature with 0 beats per bar or an odd denominator, a tempo map without points, a pattern with more than
/// `MAX_PATTERN_STEPS` steps, numbers that are not numbers); what has an obvious safe form is put back in range
/// (tempos, the order of tempo points, a cycle of no length, region lengths and loop counts, pattern rows of the
/// wrong length).
pub fn check_loaded_project(p: &mut Project) -> Result<(), String> {
    let ts = p.time_signature;
    if !ts.is_valid() {
        return Err(format!("invalid time signature {}/{}: at least 1 beat per bar, denominator 1, 2, 4, 8 or 16", ts.numerator, ts.denominator));
    }
    p.tempo.repair_loaded()?;
    if !(p.cycle.start.is_finite() && p.cycle.end.is_finite()) {
        return Err("the cycle range is not a number".into());
    }
    p.cycle.start = p.cycle.start.max(0.0);
    if p.cycle.end <= p.cycle.start {
        p.cycle.end = p.cycle.start + ts.beats_per_bar();
    }
    if !p.frame_rate.is_finite() || p.frame_rate <= 0.0 {
        p.frame_rate = 25.0;
    }
    for t in &mut p.tracks {
        for r in &mut t.regions {
            if !(r.start.is_finite() && r.length.is_finite()) {
                return Err(format!("region {} has a position or length that is not a number", r.id));
            }
            r.start = r.start.max(0.0);
            r.length = r.length.max(0.0625);
            r.loop_count = r.loop_count.clamp(1, 1024);
            if let RegionContent::Pattern(pc) = &mut r.content {
                check_loaded_pattern(pc).map_err(|e| format!("region {}: {e}", r.id))?;
            }
        }
    }
    // Automation values go to the mix as they are: keep them in the range the commands keep them in. A lane
    // whose target is no longer valid stays as it is (it plays nothing), and a value that is not a number is
    // dropped (the renderer skips it too).
    let ranges: Vec<Vec<Option<crate::ops::AutomationRange>>> = p
        .tracks
        .iter()
        .map(|t| t.automation.iter().map(|l| crate::ops::validate_target(p, t.id, &l.target).ok()).collect())
        .collect();
    for (t, lane_ranges) in p.tracks.iter_mut().zip(ranges) {
        for (lane, range) in t.automation.iter_mut().zip(lane_ranges) {
            lane.points.retain(|pt| pt.beat.is_finite() && pt.value.is_finite());
            if let Some(range) = range {
                lane.points.iter_mut().for_each(|pt| pt.value = range.clamp(pt.value));
            }
            lane.points.sort_by(|a, b| a.beat.total_cmp(&b.beat));
        }
    }
    // Plug-in parameters go to the DSP and to the plug-in windows as they are: keep them where set_param keeps them,
    // so a hand-edited file cannot hand a window an absurd delay time or a DSP a value that is not a number.
    p.master.inserts.iter_mut().for_each(repair_loaded_params);
    for t in &mut p.tracks {
        t.instrument.iter_mut().chain(t.midi_fx.iter_mut()).chain(t.channel.inserts.iter_mut()).for_each(repair_loaded_params);
    }
    for c in &mut p.live.cells {
        if !c.length.is_finite() {
            return Err(format!("Clip Grid cell {} has a length that is not a number", c.id));
        }
        c.length = c.length.max(0.0625);
        if let RegionContent::Pattern(pc) = &mut c.content {
            check_loaded_pattern(pc).map_err(|e| format!("Clip Grid cell {}: {e}", c.id))?;
        }
    }
    Ok(())
}

/// A value that is not a number is dropped, so the parameter reads its default again. A built-in plug-in's known
/// parameter is clamped to its range (and rounded for a selector), a VST3 value to 0..1. Parameters this version
/// does not know are kept, for files saved by a newer Ondina.
fn repair_loaded_params(inst: &mut PluginInstance) {
    inst.params.retain(|_, v| v.is_finite());
    if inst.kind == "vst3" {
        inst.params.values_mut().for_each(|v| *v = v.clamp(0.0, 1.0));
        return;
    }
    let Some(info) = crate::plugins::find(&inst.kind) else {
        return;
    };
    for (id, v) in inst.params.iter_mut() {
        if let Some(p) = info.params.iter().find(|p| p.id == id) {
            *v = v.clamp(p.min, p.max);
            if !p.choices.is_empty() {
                *v = v.round();
            }
        }
    }
}

fn check_loaded_pattern(p: &mut PatternContent) -> Result<(), String> {
    if p.steps > MAX_PATTERN_STEPS {
        return Err(format!("a pattern can have at most {MAX_PATTERN_STEPS} steps"));
    }
    if !p.step_length.is_finite() || !p.swing.is_finite() {
        return Err("pattern step length or swing is not a number".into());
    }
    p.steps = p.steps.max(1);
    p.step_length = p.step_length.max(0.03125);
    p.swing = p.swing.clamp(0.0, 1.0);
    for row in &mut p.rows {
        row.steps.resize(p.steps as usize, 0);
    }
    Ok(())
}

/// The copy `paste_region` adds, without its id: `region` checked against the destination track and the project,
/// its numbers kept in the ranges the other region commands keep. JSON numbers are always finite; the checks cover
/// a region built in code.
fn pasted_region(project: &Project, track_id: Id, start: f64, region: &Region) -> Result<Region, String> {
    let kind = project.track(track_id).ok_or("track not found")?.kind;
    if !start.is_finite() {
        return Err("start must be a finite beat position".into());
    }
    if !region.length.is_finite() || region.length <= 0.0 {
        return Err("region length must be more than 0 beats".into());
    }
    let instrument_only = "MIDI and pattern regions need an instrument track";
    let mut r = region.clone();
    r.start = start.max(0.0);
    r.length = r.length.max(0.0625);
    r.loop_count = r.loop_count.clamp(1, 1024);
    r.take = None;
    match &mut r.content {
        RegionContent::Audio(a) => {
            if kind != TrackKind::Audio {
                return Err("an audio region needs an audio track".into());
            }
            if !project.audio_files.iter().any(|f| f.id == a.file_id) {
                return Err(format!("audio file {} is not in this project: import it first with import_audio", a.file_id));
            }
            if !(a.offset_sec.is_finite() && a.fade_in.is_finite() && a.fade_out.is_finite()) {
                return Err("audio offset and fades must be finite numbers".into());
            }
            a.offset_sec = a.offset_sec.max(0.0);
            a.gain_db = a.gain_db.clamp(-60.0, 24.0);
            a.fade_in = a.fade_in.max(0.0);
            a.fade_out = a.fade_out.max(0.0);
        }
        RegionContent::Midi(m) => {
            if kind != TrackKind::Instrument {
                return Err(instrument_only.into());
            }
            let finite = m.notes.iter().all(|n| n.start.is_finite() && n.length.is_finite())
                && m.cc.iter().all(|e| e.beat.is_finite())
                && m.lyrics.iter().all(|l| l.beat.is_finite());
            if !finite {
                return Err("note, controller and lyric positions must be finite numbers".into());
            }
            crate::ops::sanitize_notes(&mut m.notes);
            for e in &mut m.cc {
                e.beat = e.beat.max(0.0);
                e.value = e.value.min(127);
                e.controller = e.controller.min(128);
            }
            m.cc.sort_by(|a, b| a.beat.total_cmp(&b.beat));
            m.transpose = m.transpose.clamp(-48, 48);
            m.velocity_offset = m.velocity_offset.clamp(-127, 127);
        }
        RegionContent::Pattern(p) => {
            if kind != TrackKind::Instrument {
                return Err(instrument_only.into());
            }
            if !p.step_length.is_finite() {
                return Err("pattern step length must be a finite number".into());
            }
            // the same bound as resize_region: a pattern made in Ondina always fits
            if p.steps > MAX_PATTERN_STEPS {
                return Err(format!("a pattern can have at most {MAX_PATTERN_STEPS} steps"));
            }
            p.steps = p.steps.max(1);
            p.step_length = p.step_length.max(0.03125);
            p.swing = p.swing.clamp(0.0, 1.0);
            for row in &mut p.rows {
                row.pitch = row.pitch.min(127);
                row.steps.resize(p.steps as usize, 0);
                row.steps.iter_mut().for_each(|s| *s = (*s).min(127));
            }
            r.length = f64::from(p.steps) * p.step_length;
        }
    }
    Ok(r)
}

/// Apply an Environment routing table to one live MIDI note: returns (track, pitch, velocity) targets.
pub fn route_note(routes: &[MidiRoute], channel: u8, pitch: u8, velocity: u8) -> Vec<(Id, u8, u8)> {
    routes
        .iter()
        .filter(|r| r.channel.map(|c| c == channel).unwrap_or(true))
        .filter(|r| r.note_min.map(|m| pitch >= m).unwrap_or(true) && r.note_max.map(|m| pitch <= m).unwrap_or(true))
        .map(|r| {
            let p = (pitch as i32 + r.transpose).clamp(0, 127) as u8;
            let v = ((velocity as f32) * r.velocity_scale.unwrap_or(1.0)).round().clamp(1.0, 127.0) as u8;
            (r.track_id, p, if velocity == 0 { 0 } else { v })
        })
        .collect()
}

fn transform(notes: &mut Vec<Note>, f: &NoteFilter, o: &NoteOps, key: Key, region_len: f64) -> (usize, usize) {
    let sel: Vec<bool> = notes
        .iter()
        .enumerate()
        .map(|(i, n)| {
            f.indices.as_ref().map(|ix| ix.contains(&i)).unwrap_or(true)
                && f.pitch_min.map(|v| n.pitch >= v).unwrap_or(true)
                && f.pitch_max.map(|v| n.pitch <= v).unwrap_or(true)
                && f.velocity_min.map(|v| n.velocity >= v).unwrap_or(true)
                && f.velocity_max.map(|v| n.velocity <= v).unwrap_or(true)
                && f.start_min.map(|v| n.start >= v).unwrap_or(true)
                && f.start_max.map(|v| n.start <= v).unwrap_or(true)
                && f.length_min.map(|v| n.length >= v).unwrap_or(true)
                && f.length_max.map(|v| n.length <= v).unwrap_or(true)
                && f.out_of_key.map(|v| !v || !key.contains(n.pitch)).unwrap_or(true)
        })
        .collect();
    let count = sel.iter().filter(|x| **x).count();
    if o.delete == Some(true) {
        let mut i = 0;
        notes.retain(|_| {
            let keep = !sel[i];
            i += 1;
            keep
        });
        return (0, count);
    }
    let (min_s, max_e) = notes.iter().zip(&sel).filter(|(_, s)| **s).fold((f64::MAX, f64::MIN), |(a, b), (n, _)| (a.min(n.start), b.max(n.start + n.length)));
    for (n, s) in notes.iter_mut().zip(&sel) {
        if !*s {
            continue;
        }
        let mut p = n.pitch as i32;
        if let Some(c) = o.invert_around {
            p = 2 * c as i32 - p;
        }
        if let Some(t) = o.transpose {
            p += t;
        }
        if let Some(fp) = o.fixed_pitch {
            p = fp as i32;
        }
        n.pitch = p.clamp(0, 127) as u8;
        let mut v = n.velocity as f32;
        if let Some(sc) = o.velocity_scale {
            v *= sc;
        }
        if let Some(a) = o.velocity_add {
            v += a as f32;
        }
        if let Some(fv) = o.velocity_fixed {
            v = fv as f32;
        }
        n.velocity = v.round().clamp(1.0, 127.0) as u8;
        if o.reverse == Some(true) {
            n.start = min_s + (max_e - (n.start + n.length));
        }
        if let Some(ts) = o.time_scale {
            n.start *= ts;
            n.length *= ts;
        }
        if let Some(off) = o.time_offset {
            n.start += off;
        }
        if let Some(q) = o.quantize.filter(|q| *q > 0.0) {
            n.start = (n.start / q).round() * q;
        }
        if let Some(ls) = o.length_scale {
            n.length *= ls;
        }
        if let Some(lf) = o.length_fixed {
            n.length = lf;
        }
        n.start = n.start.clamp(0.0, region_len.max(n.start));
        n.length = n.length.max(0.01);
    }
    notes.sort_by(|a, b| a.start.partial_cmp(&b.start).unwrap());
    (count, 0)
}

fn is_drum_kit(t: &Track) -> bool {
    t.instrument.as_ref().is_some_and(|i| i.kind == "drum_synth")
}

/// The notes of a MIDI region as they sound, for the key estimate: region transposition applied, notes past the
/// region end dropped and tails cut there, lengths multiplied by the loop count.
fn sounding_notes(r: &Region, m: &MidiContent, out: &mut Vec<(u8, f64, u8)>) {
    for n in m.notes.iter().filter(|n| n.start < r.length) {
        let len = n.length.min(r.length - n.start);
        if len > 0.0 {
            out.push(((n.pitch as i32 + m.transpose).clamp(0, 127) as u8, len * r.loop_count.max(1) as f64, n.velocity));
        }
    }
}

/// `detect_key`: estimate the key of one MIDI region, or of every unmuted MIDI region on the instrument tracks that
/// are not Drum Kits, and with `apply` make it the project key.
fn detect_key(project: &mut Project, region_id: Option<Id>, apply: bool) -> R {
    let mut notes: Vec<(u8, f64, u8)> = vec![];
    let mut regions = 0usize;
    match region_id {
        Some(id) => {
            let (t, r) = project.tracks.iter().find_map(|t| t.regions.iter().find(|r| r.id == id).map(|r| (t, r))).ok_or("region not found")?;
            match &r.content {
                RegionContent::Audio(_) => return Err(format!("region {id} is an audio region: detect_key reads MIDI notes (use audio_to_midi first)")),
                RegionContent::Pattern(_) => return Err(format!("region {id} is a step pattern: drum patterns have no key")),
                RegionContent::Midi(_) if is_drum_kit(t) => return Err(format!("region {id} is on a Drum Kit track: drum notes have no key")),
                RegionContent::Midi(m) => {
                    sounding_notes(r, m, &mut notes);
                    regions = 1;
                }
            }
        }
        None => {
            for t in project.tracks.iter().filter(|t| t.kind == TrackKind::Instrument && !is_drum_kit(t)) {
                for r in t.regions.iter().filter(|r| !r.muted) {
                    if let RegionContent::Midi(m) = &r.content {
                        sounding_notes(r, m, &mut notes);
                        regions += 1;
                    }
                }
            }
        }
    }
    if notes.is_empty() {
        return Err("no MIDI notes to analyze: add or import a MIDI part on an instrument track (Drum Kit tracks and step patterns are skipped)".into());
    }
    let ranked = estimate_key(&notes);
    if ranked.is_empty() {
        return Err("these notes do not point to a key: all 12 pitch classes are equally present".into());
    }
    let (best, conf) = ranked[0];
    let low = notes.len() < KEY_MIN_NOTES || conf < KEY_MIN_CONFIDENCE;
    let round = |c: f32| (c as f64 * 100.0).round() / 100.0;
    let alternatives: Vec<Value> = ranked[1..4].iter().map(|(k, c)| json!({"key": k.name(), "confidence": round(*c)})).collect();
    if apply {
        project.key = best;
    }
    ok(json!({"key": best.name(), "confidence": round(conf), "low_confidence": low, "alternatives": alternatives, "note_count": notes.len(), "regions": regions, "applied": apply}))
}

fn region_notes(r: &Region) -> (Vec<Note>, Vec<LyricEvent>) {
    match &r.content {
        RegionContent::Midi(m) => (m.notes.iter().map(|n| Note { pitch: (n.pitch as i32 + m.transpose).clamp(0, 127) as u8, ..*n }).collect(), m.lyrics.clone()),
        RegionContent::Pattern(p) => (pattern_to_notes(p), vec![]),
        RegionContent::Audio(_) => (vec![], vec![]),
    }
}

const STEPS: [&str; 12] = ["C", "C", "D", "D", "E", "F", "F", "G", "G", "A", "A", "B"];
const ALTER: [i32; 12] = [0, 1, 0, 1, 0, 0, 1, 0, 1, 0, 1, 0];

/// Note values in sixteenths -> (type name, dotted).
const VALUES: [(u32, &str, bool); 8] = [(16, "whole", false), (12, "half", true), (8, "half", false), (6, "quarter", true), (4, "quarter", false), (3, "eighth", true), (2, "eighth", false), (1, "16th", false)];

fn split_duration(mut d: u32) -> Vec<(u32, &'static str, bool)> {
    let mut out = vec![];
    while d > 0 {
        let v = VALUES.iter().find(|v| v.0 <= d).copied().unwrap_or((1, "16th", false));
        out.push(v);
        d -= v.0;
    }
    out
}

fn clef_for(notes: &[Note]) -> &'static str {
    if notes.is_empty() {
        return "treble";
    }
    let avg = notes.iter().map(|n| n.pitch as f32).sum::<f32>() / notes.len() as f32;
    if avg < 55.0 { "bass" } else { "treble" }
}

#[derive(Clone)]
struct Event {
    start: u32,
    dur: u32,
    pitches: Vec<u8>,
    lyric: Option<String>,
}

/// Quantize to sixteenths and build a single voice (chords for simultaneous onsets, rests for gaps).
fn voice_events(notes: &[Note], lyrics: &[LyricEvent], length_beats: f64) -> (Vec<Event>, u32) {
    let total = ((length_beats * 4.0).round() as u32).max(1);
    let mut map: std::collections::BTreeMap<u32, (u32, Vec<u8>)> = Default::default();
    for n in notes {
        let s = (n.start * 4.0).round() as u32;
        if s >= total {
            continue;
        }
        let d = ((n.length * 4.0).round() as u32).max(1);
        let e = map.entry(s).or_insert((d, vec![]));
        e.0 = e.0.min(d);
        if !e.1.contains(&n.pitch) {
            e.1.push(n.pitch);
        }
    }
    let starts: Vec<u32> = map.keys().copied().collect();
    let mut events = vec![];
    let mut t = 0;
    for (i, s) in starts.iter().enumerate() {
        if *s > t {
            events.push(Event { start: t, dur: s - t, pitches: vec![], lyric: None });
        }
        let next = starts.get(i + 1).copied().unwrap_or(total);
        let (d, mut ps) = map[s].clone();
        ps.sort();
        let dur = d.min(next - s).min(total - s).max(1);
        let lyric = lyrics.iter().find(|l| ((l.beat * 4.0).round() as u32) == *s).map(|l| l.text.clone());
        events.push(Event { start: *s, dur, pitches: ps, lyric });
        t = s + dur;
    }
    if t < total {
        events.push(Event { start: t, dur: total - t, pitches: vec![], lyric: None });
    }
    (events, total)
}

/// Split events at bar lines and into notatable values with ties.
fn measures_of(events: &[Event], ts: TimeSignature) -> Vec<Vec<(Event, &'static str, bool, bool, bool)>> {
    let bar = ((ts.beats_per_bar() * 4.0).round() as u32).max(1);
    let mut measures: Vec<Vec<(Event, &'static str, bool, bool, bool)>> = vec![];
    for e in events {
        let mut s = e.start;
        let end = e.start + e.dur;
        let mut first = true;
        while s < end {
            let m = (s / bar) as usize;
            let bar_end = (m as u32 + 1) * bar;
            let chunk = (end - s).min(bar_end - s);
            let parts = split_duration(chunk);
            for (pi, (d, ty, dot)) in parts.iter().enumerate() {
                while measures.len() <= m {
                    measures.push(vec![]);
                }
                let is_last_piece = s + d >= end;
                let tie_start = !e.pitches.is_empty() && !is_last_piece;
                let tie_stop = !e.pitches.is_empty() && !first;
                let mut ev = e.clone();
                ev.start = s;
                ev.dur = *d;
                if !first {
                    ev.lyric = None;
                }
                measures[m].push((ev, ty, *dot, tie_start, tie_stop));
                s += d;
                first = false;
                let _ = pi;
            }
        }
    }
    measures
}

fn notate(notes: &[Note], lyrics: &[LyricEvent], length: f64, ts: TimeSignature) -> Vec<Value> {
    let (events, _) = voice_events(notes, lyrics, length);
    measures_of(&events, ts)
        .into_iter()
        .enumerate()
        .map(|(i, m)| {
            let items: Vec<Value> = m
                .iter()
                .map(|(e, ty, dot, ts_, tp)| {
                    let names: Vec<String> = e.pitches.iter().map(|p| crate::theory::note_name(*p)).collect();
                    let mut v = json!({"beat": e.start as f64 / 4.0, "value": if *dot { format!("dotted {ty}") } else { ty.to_string() }});
                    if names.is_empty() {
                        v["rest"] = json!(true);
                    } else {
                        v["pitches"] = json!(names);
                    }
                    if *ts_ {
                        v["tie"] = json!("start");
                    } else if *tp {
                        v["tie"] = json!("stop");
                    }
                    if let Some(l) = &e.lyric {
                        v["lyric"] = json!(l);
                    }
                    v
                })
                .collect();
            json!({"measure": i + 1, "items": items})
        })
        .collect()
}

fn key_fifths(k: Key) -> i32 {
    // fifths of the relative major
    let major_root = match k.scale {
        crate::theory::ScaleKind::Minor | crate::theory::ScaleKind::HarmonicMinor | crate::theory::ScaleKind::MelodicMinor | crate::theory::ScaleKind::MinorPentatonic | crate::theory::ScaleKind::Blues => (k.root + 3) % 12,
        crate::theory::ScaleKind::Dorian => (k.root + 10) % 12,
        crate::theory::ScaleKind::Mixolydian => (k.root + 5) % 12,
        crate::theory::ScaleKind::Lydian => (k.root + 7) % 12,
        crate::theory::ScaleKind::Phrygian => (k.root + 8) % 12,
        crate::theory::ScaleKind::Locrian => (k.root + 1) % 12,
        _ => k.root,
    };
    [0, -5, 2, -3, 4, -1, 6, 1, -4, 3, -2, 5][major_root as usize]
}

fn xml_escape(s: &str) -> String {
    s.replace('&', "&amp;").replace('<', "&lt;").replace('>', "&gt;").replace('"', "&quot;")
}

/// Build a MusicXML (partwise 4.0) document for instrument tracks.
pub fn musicxml(project: &Project, track_ids: Option<&[Id]>) -> Result<String, String> {
    let tracks: Vec<&Track> = project.tracks.iter().filter(|t| t.kind == TrackKind::Instrument && track_ids.map(|ids| ids.contains(&t.id)).unwrap_or(true)).collect();
    if tracks.is_empty() {
        return Err("no instrument tracks to export".into());
    }
    let ts = project.time_signature;
    let end = project.content_end().max(ts.beats_per_bar());
    let mut x = String::new();
    x += "<?xml version=\"1.0\" encoding=\"UTF-8\"?>\n<!DOCTYPE score-partwise PUBLIC \"-//Recordare//DTD MusicXML 4.0 Partwise//EN\" \"http://www.musicxml.org/dtds/partwise.dtd\">\n<score-partwise version=\"4.0\">\n";
    x += &format!("  <work><work-title>{}</work-title></work>\n  <identification><encoding><software>Ondina DAW</software></encoding></identification>\n  <part-list>\n", xml_escape(&project.name));
    for (i, t) in tracks.iter().enumerate() {
        x += &format!("    <score-part id=\"P{}\"><part-name>{}</part-name></score-part>\n", i + 1, xml_escape(&t.name));
    }
    x += "  </part-list>\n";
    for (i, t) in tracks.iter().enumerate() {
        // flatten all regions of the track into one timeline of notes
        let mut notes = vec![];
        let mut lyrics = vec![];
        for r in t.regions.iter().filter(|r| !r.muted) {
            let (ns, ls) = region_notes(r);
            for k in 0..r.loop_count.max(1) {
                let off = r.start + k as f64 * r.length;
                notes.extend(ns.iter().filter(|n| n.start < r.length).map(|n| Note { start: n.start + off, length: n.length.min(r.length - n.start), ..*n }));
                lyrics.extend(ls.iter().map(|l| LyricEvent { beat: l.beat + off, text: l.text.clone() }));
            }
        }
        let is_drum = t.instrument.as_ref().map(|p| p.kind == "drum_synth").unwrap_or(false);
        let clef = if is_drum { "percussion" } else { clef_for(&notes) };
        let (events, _) = voice_events(&notes, &lyrics, end);
        let measures = measures_of(&events, ts);
        x += &format!("  <part id=\"P{}\">\n", i + 1);
        for (mi, m) in measures.iter().enumerate() {
            x += &format!("    <measure number=\"{}\">\n", mi + 1);
            if mi == 0 {
                let (sign, line) = match clef {
                    "bass" => ("F", 4),
                    "percussion" => ("percussion", 2),
                    _ => ("G", 2),
                };
                x += &format!(
                    "      <attributes><divisions>4</divisions><key><fifths>{}</fifths></key><time><beats>{}</beats><beat-type>{}</beat-type></time><clef><sign>{}</sign><line>{}</line></clef></attributes>\n",
                    key_fifths(project.key), ts.numerator, ts.denominator, sign, line
                );
                x += &format!("      <direction placement=\"above\"><direction-type><metronome><beat-unit>quarter</beat-unit><per-minute>{}</per-minute></metronome></direction-type><sound tempo=\"{}\"/></direction>\n", project.tempo.initial_bpm().round(), project.tempo.initial_bpm());
            }
            for (e, ty, dot, tie_start, tie_stop) in m {
                if e.pitches.is_empty() {
                    x += &format!("      <note><rest/><duration>{}</duration><type>{}</type>{}</note>\n", e.dur, ty, if *dot { "<dot/>" } else { "" });
                    continue;
                }
                for (pi, p) in e.pitches.iter().enumerate() {
                    let pc = (*p % 12) as usize;
                    let oct = *p as i32 / 12 - 1;
                    let alter = if ALTER[pc] != 0 { format!("<alter>{}</alter>", ALTER[pc]) } else { String::new() };
                    let ties = format!("{}{}", if *tie_stop { "<tie type=\"stop\"/>" } else { "" }, if *tie_start { "<tie type=\"start\"/>" } else { "" });
                    let tied = format!("{}{}", if *tie_stop { "<tied type=\"stop\"/>" } else { "" }, if *tie_start { "<tied type=\"start\"/>" } else { "" });
                    let notations = if tied.is_empty() { String::new() } else { format!("<notations>{tied}</notations>") };
                    let lyric = match (&e.lyric, pi) {
                        (Some(l), 0) => {
                            let syl = if l.ends_with('-') { "begin" } else { "single" };
                            format!("<lyric number=\"1\"><syllabic>{}</syllabic><text>{}</text></lyric>", syl, xml_escape(l.trim_end_matches('-')))
                        }
                        _ => String::new(),
                    };
                    x += &format!(
                        "      <note>{}<pitch><step>{}</step>{}<octave>{}</octave></pitch><duration>{}</duration>{}<type>{}</type>{}{}{}</note>\n",
                        if pi > 0 { "<chord/>" } else { "" },
                        STEPS[pc], alter, oct, e.dur, ties, ty, if *dot { "<dot/>" } else { "" }, notations, lyric
                    );
                }
            }
            x += "    </measure>\n";
        }
        x += "  </part>\n";
    }
    x += "</score-partwise>\n";
    Ok(x)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::ops;
    use crate::theory::ScaleKind;

    fn run(p: &mut Project, c: Command) -> Value {
        match ops::apply(p, &c).unwrap() {
            Outcome::Done(v) => v,
            Outcome::NotHandled => panic!("not handled"),
        }
    }

    #[test]
    fn score_lyrics_musicxml_transform_comp_live_routes() {
        let mut p = Project::new("s");
        let t = ops::add_track(&mut p, TrackKind::Instrument, Some("Lead".into()), None, None, None, None).unwrap();
        let notes = vec![
            Note { pitch: 60, velocity: 90, start: 0.0, length: 1.0 },
            Note { pitch: 62, velocity: 90, start: 1.0, length: 0.5 },
            Note { pitch: 64, velocity: 90, start: 2.0, length: 3.0 },
            Note { pitch: 61, velocity: 40, start: 5.0, length: 1.0 },
        ];
        let rid = run(&mut p, Command::CreateMidiRegion { track_id: t, start: 0.0, length: 8.0, name: None, notes: Some(notes) })["region_id"].as_u64().unwrap();
        run(&mut p, Command::SetLyrics { region_id: rid, text: Some("hel-lo bright world".into()), lyrics: None });
        let sc = run(&mut p, Command::GetScore { region_id: rid });
        let m1 = &sc["measures"][0]["items"];
        assert_eq!(m1[0]["value"], "quarter");
        assert_eq!(m1[0]["lyric"], "hel-");
        // the dotted-half spanning the barline must be tied
        let s = sc.to_string();
        assert!(s.contains("\"tie\":\"start\"") && s.contains("\"tie\":\"stop\""), "{s}");
        let xml = musicxml(&p, None).unwrap();
        assert!(xml.contains("<score-partwise") && xml.contains("<step>E</step>") && xml.contains("<tie type=\"start\"/>") && xml.contains("<text>hel</text>"));
        // MIDI transform: delete out-of-key, double velocity of the rest
        let r = run(&mut p, Command::TransformNotes { region_id: rid, select: Some(NoteFilter { out_of_key: Some(true), ..Default::default() }), operations: NoteOps { delete: Some(true), ..Default::default() } });
        assert_eq!(r["deleted"], 1);
        run(&mut p, Command::TransformNotes { region_id: rid, select: None, operations: NoteOps { velocity_scale: Some(2.0), invert_around: Some(60), ..Default::default() } });
        if let RegionContent::Midi(m) = &p.find_region(rid).unwrap().1.content {
            assert_eq!(m.notes[1].pitch, 58);
            assert_eq!(m.notes[0].velocity, 127);
        }
        // comping: two overlapping takes
        let a = run(&mut p, Command::CreateMidiRegion { track_id: t, start: 16.0, length: 8.0, name: Some("Take 1".into()), notes: Some(vec![Note { pitch: 60, velocity: 80, start: 0.0, length: 1.0 }, Note { pitch: 67, velocity: 80, start: 5.0, length: 1.0 }]) })["region_id"].as_u64().unwrap();
        let b = run(&mut p, Command::CreateMidiRegion { track_id: t, start: 16.0, length: 8.0, name: Some("Take 2".into()), notes: Some(vec![Note { pitch: 72, velocity: 80, start: 0.0, length: 1.0 }, Note { pitch: 79, velocity: 80, start: 5.0, length: 1.0 }]) })["region_id"].as_u64().unwrap();
        let takes = run(&mut p, Command::ListTakes { track_id: t });
        assert!(takes["take_groups"].as_array().unwrap().iter().any(|g| g.as_array().unwrap().len() == 2));
        let comp = run(&mut p, Command::CompTakes { track_id: t, segments: vec![crate::command::CompSegment { region_id: a, from_beat: 16.0, to_beat: 20.0 }, crate::command::CompSegment { region_id: b, from_beat: 20.0, to_beat: 24.0 }] });
        assert_eq!(comp["comp_region_ids"].as_array().unwrap().len(), 2);
        let tr = p.track(t).unwrap();
        assert!(tr.regions.iter().filter(|r| r.id == a || r.id == b).all(|r| r.muted));
        // clip grid
        let c = run(&mut p, Command::CreateLiveCell { track_id: t, scene: 5, length: Some(4.0), notes: None, from_region_id: Some(rid), name: None })["cell_id"].as_u64().unwrap();
        assert!(p.live.scenes.len() >= 6);
        run(&mut p, Command::SetLiveCellNotes { cell_id: c, notes: vec![Note { pitch: 50, velocity: 100, start: 0.0, length: 1.0 }] });
        run(&mut p, Command::RemoveScene { scene: 5 });
        assert!(p.live.cells.is_empty());
        // routing: split keyboard
        run(&mut p, Command::SetMidiRoutes { routes: vec![MidiRoute { name: "low".into(), channel: None, note_min: None, note_max: Some(59), transpose: -12, velocity_scale: None, track_id: t }] });
        assert_eq!(route_note(&p.midi_routes, 1, 48, 100), vec![(t, 36, 100)]);
        assert!(route_note(&p.midi_routes, 1, 72, 100).is_empty());
    }

    fn notes_of(pitches_and_lengths: &[(u8, f64)]) -> Vec<Note> {
        let mut start = 0.0;
        pitches_and_lengths
            .iter()
            .map(|&(pitch, length)| {
                let n = Note { pitch, velocity: 100, start, length };
                start += length;
                n
            })
            .collect()
    }

    fn a_minor_melody() -> Vec<Note> {
        notes_of(&[(69, 1.0), (72, 1.0), (71, 1.0), (69, 1.0), (76, 1.0), (74, 1.0), (72, 1.0), (71, 1.0), (68, 1.0), (69, 1.0), (71, 1.0), (72, 1.0), (71, 1.0), (68, 1.0), (69, 2.0)])
    }

    fn drum_notes() -> Vec<Note> {
        let mut notes = vec![];
        for i in 0..16 {
            notes.push(Note { pitch: 42, velocity: 100, start: i as f64, length: 0.5 });
        }
        for i in 0..4 {
            let bar = i as f64 * 4.0;
            notes.push(Note { pitch: 37, velocity: 100, start: bar + 3.5, length: 0.5 });
            notes.push(Note { pitch: 36, velocity: 100, start: bar, length: 0.5 });
            notes.push(Note { pitch: 38, velocity: 100, start: bar + 2.0, length: 0.5 });
        }
        notes.push(Note { pitch: 49, velocity: 100, start: 0.0, length: 2.0 });
        notes.push(Note { pitch: 49, velocity: 100, start: 8.0, length: 2.0 });
        notes
    }

    fn midi_region(p: &mut Project, track_id: Id, start: f64, notes: Vec<Note>) -> Id {
        run(p, Command::CreateMidiRegion { track_id, start, length: 16.0, name: None, notes: Some(notes) })["region_id"].as_u64().unwrap()
    }

    fn detect_err(p: &mut Project, region_id: Option<Id>) -> String {
        ops::apply(p, &Command::DetectKey { region_id, apply: None }).err().expect("detect_key should fail")
    }

    #[test]
    fn detect_key_reads_midi_and_skips_drums_patterns_and_muted_regions() {
        let mut p = Project::new("k");
        let lead = ops::add_track(&mut p, TrackKind::Instrument, Some("Lead".into()), None, None, None, None).unwrap();
        midi_region(&mut p, lead, 0.0, a_minor_melody());
        let drums = ops::add_track(&mut p, TrackKind::Instrument, Some("Drums".into()), None, Some("drum_synth"), None, None).unwrap();
        let drum_region = midi_region(&mut p, drums, 0.0, drum_notes());
        let sharps: Vec<Note> = (0..16).map(|i| Note { pitch: if i % 2 == 0 { 66 } else { 61 }, velocity: 100, start: i as f64, length: 1.0 }).collect();
        let muted = midi_region(&mut p, lead, 16.0, sharps);
        run(&mut p, Command::from_tool_call("set_region_properties", &json!({"region_id": muted, "muted": true})).unwrap());
        let pattern = run(&mut p, Command::from_tool_call("create_pattern_region", &json!({"track_id": lead, "start": 32})).unwrap())["region_id"].as_u64().unwrap();

        let r = run(&mut p, Command::DetectKey { region_id: None, apply: None });
        assert_eq!(r["key"], "A Minor", "{r}");
        assert_eq!(r["applied"], false);
        assert_eq!(r["regions"], 1);
        assert_eq!(r["note_count"], 15);
        assert_eq!(r["alternatives"].as_array().unwrap().len(), 3);
        assert_eq!(r["low_confidence"], false);
        assert_eq!(p.key, Key { root: 0, scale: ScaleKind::Major });

        assert!(detect_err(&mut p, Some(drum_region)).contains("Drum Kit"));
        assert!(detect_err(&mut p, Some(pattern)).contains("step pattern"));
        assert_eq!(detect_err(&mut p, Some(9999)), "region not found");

        let r = run(&mut p, Command::DetectKey { region_id: Some(muted), apply: None });
        assert_ne!(r["key"], "A Minor", "{r}");
    }

    #[test]
    fn detect_key_applies_transpose_and_flags_few_notes() {
        let mut p = Project::new("k");
        let t = ops::add_track(&mut p, TrackKind::Instrument, Some("Keys".into()), None, None, None, None).unwrap();
        let mut scale = notes_of(&[(60, 1.0), (62, 1.0), (64, 1.0), (65, 1.0), (67, 1.0), (69, 1.0), (71, 1.0), (72, 1.0)]);
        scale.extend([60, 64, 67].iter().map(|&pitch| Note { pitch, velocity: 100, start: 8.0, length: 4.0 }));
        let rid = midi_region(&mut p, t, 0.0, scale);
        run(&mut p, Command::from_tool_call("set_region_properties", &json!({"region_id": rid, "transpose": 7})).unwrap());
        let r = run(&mut p, Command::DetectKey { region_id: Some(rid), apply: None });
        assert_eq!(r["key"], "G Major", "{r}");

        let mut p = Project::new("k");
        let lead = ops::add_track(&mut p, TrackKind::Instrument, Some("Lead".into()), None, None, None, None).unwrap();
        midi_region(&mut p, lead, 0.0, a_minor_melody());
        let r = run(&mut p, Command::DetectKey { region_id: None, apply: Some(true) });
        assert_eq!(r["applied"], true);
        assert_eq!(p.key, Key { root: 9, scale: ScaleKind::Minor });

        let mut p = Project::new("k");
        let drums = ops::add_track(&mut p, TrackKind::Instrument, Some("Drums".into()), None, Some("drum_synth"), None, None).unwrap();
        midi_region(&mut p, drums, 0.0, drum_notes());
        assert!(detect_err(&mut p, None).contains("no MIDI notes"));
        let keys = ops::add_track(&mut p, TrackKind::Instrument, Some("Keys".into()), None, None, None, None).unwrap();
        midi_region(&mut p, keys, 0.0, notes_of(&[(60, 1.0), (64, 1.0), (67, 1.0)]));
        let r = run(&mut p, Command::DetectKey { region_id: None, apply: None });
        assert_eq!(r["low_confidence"], true);
        assert_eq!(r["note_count"], 3);
    }
}
