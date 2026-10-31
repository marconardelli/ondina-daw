//! Standard MIDI File import/export.

use midly::num::{u15, u24, u28, u4, u7};
use midly::{Format, Header, MetaMessage, MidiMessage, Smf, Timing, TrackEvent, TrackEventKind};

use crate::model::*;
use crate::ops;
use crate::theory::Chord;

const PPQ: u16 = 960;

/// Export MIDI and pattern regions of the given tracks (all instrument tracks if None) as SMF type 1.
pub fn export_midi(project: &Project, track_ids: Option<&[Id]>) -> Result<Vec<u8>, String> {
    let mut tracks: Vec<Vec<TrackEvent<'static>>> = vec![];
    // conductor track
    let mut cond: Vec<(u64, TrackEventKind<'static>)> = vec![];
    for tp in &project.tempo.points {
        let us = (60_000_000.0 / tp.bpm).round() as u32;
        cond.push(((tp.beat * PPQ as f64).round() as u64, TrackEventKind::Meta(MetaMessage::Tempo(u24::new(us)))));
    }
    let ts = project.time_signature;
    let denom_pow = (ts.denominator as f32).log2() as u8;
    cond.push((0, TrackEventKind::Meta(MetaMessage::TimeSignature(ts.numerator, denom_pow, 24, 8))));
    tracks.push(to_delta(cond));

    let mut channel = 0u8;
    for t in &project.tracks {
        if t.kind != TrackKind::Instrument {
            continue;
        }
        if let Some(ids) = track_ids {
            if !ids.contains(&t.id) {
                continue;
            }
        }
        let is_drum = t.instrument.as_ref().map(|i| i.kind == "drum_synth").unwrap_or(false);
        let ch = if is_drum { 9 } else {
            let c = channel % 16;
            channel += 1;
            if c == 9 { channel += 1; 10 } else { c }
        };
        let mut evs: Vec<(u64, TrackEventKind<'static>)> = vec![];
        let name: &'static [u8] = Box::leak(t.name.clone().into_bytes().into_boxed_slice());
        evs.push((0, TrackEventKind::Meta(MetaMessage::TrackName(name))));
        for r in &t.regions {
            if r.muted {
                continue;
            }
            let (notes, transpose, vel_off): (Vec<Note>, i32, i32) = match &r.content {
                RegionContent::Midi(m) => (m.notes.clone(), m.transpose, m.velocity_offset),
                RegionContent::Pattern(p) => (ops::pattern_to_notes(p), 0, 0),
                _ => continue,
            };
            for k in 0..r.loop_count.max(1) {
                let base = r.start + k as f64 * r.length;
                for n in notes.iter().filter(|n| n.start < r.length) {
                    let pitch = (n.pitch as i32 + transpose).clamp(0, 127) as u8;
                    let vel = (n.velocity as i32 + vel_off).clamp(1, 127) as u8;
                    let len = n.length.min(r.length - n.start);
                    let on = ((base + n.start) * PPQ as f64).round() as u64;
                    let off = ((base + n.start + len) * PPQ as f64).round() as u64;
                    evs.push((on, TrackEventKind::Midi { channel: u4::new(ch), message: MidiMessage::NoteOn { key: u7::new(pitch), vel: u7::new(vel) } }));
                    evs.push((off.max(on + 1), TrackEventKind::Midi { channel: u4::new(ch), message: MidiMessage::NoteOff { key: u7::new(pitch), vel: u7::new(0) } }));
                }
                if let RegionContent::Midi(m) = &r.content {
                    for c in m.cc.iter().filter(|c| c.beat < r.length) {
                        let at = ((base + c.beat) * PPQ as f64).round() as u64;
                        evs.push((at, TrackEventKind::Midi { channel: u4::new(ch), message: MidiMessage::Controller { controller: u7::new(c.controller.min(127)), value: u7::new(c.value.min(127)) } }));
                    }
                }
            }
        }
        // note-offs before note-ons at the same tick
        evs.sort_by_key(|(t, e)| (*t, matches!(e, TrackEventKind::Midi { message: MidiMessage::NoteOn { .. }, .. }) as u8));
        tracks.push(to_delta(evs));
    }
    let smf = Smf { header: Header::new(Format::Parallel, Timing::Metrical(u15::new(PPQ))), tracks };
    let mut out = vec![];
    smf.write_std(&mut out).map_err(|e| e.to_string())?;
    Ok(out)
}

fn to_delta(mut evs: Vec<(u64, TrackEventKind<'static>)>) -> Vec<TrackEvent<'static>> {
    evs.sort_by_key(|e| e.0);
    let mut out = vec![];
    let mut last = 0u64;
    for (t, kind) in evs {
        out.push(TrackEvent { delta: u28::new((t - last) as u32), kind });
        last = t;
    }
    out.push(TrackEvent { delta: u28::new(0), kind: TrackEventKind::Meta(MetaMessage::EndOfTrack) });
    out
}

/// Import an SMF into the project. Returns created track ids.
pub fn import_midi(project: &mut Project, bytes: &[u8], at_beat: f64) -> Result<Vec<Id>, String> {
    let smf = Smf::parse(bytes).map_err(|e| format!("invalid MIDI file: {e}"))?;
    let ppq = match smf.header.timing {
        Timing::Metrical(t) => t.as_int() as f64,
        Timing::Timecode(..) => return Err("SMPTE-timed MIDI files are not supported".into()),
    };
    let empty_project = project.tracks.is_empty();
    // (channel -> notes) grouped per SMF track
    let mut created = vec![];
    let mut first_tempo_set = false;
    for track in &smf.tracks {
        let mut tick: u64 = 0;
        let mut name = String::new();
        let mut open: std::collections::HashMap<(u8, u8), (u64, u8)> = Default::default();
        let mut per_channel: std::collections::BTreeMap<u8, Vec<Note>> = Default::default();
        for ev in track {
            tick += ev.delta.as_int() as u64;
            match ev.kind {
                TrackEventKind::Meta(MetaMessage::TrackName(n)) => name = String::from_utf8_lossy(n).to_string(),
                TrackEventKind::Meta(MetaMessage::Tempo(us)) if empty_project => {
                    let bpm = 60_000_000.0 / us.as_int() as f64;
                    let beat = tick as f64 / ppq;
                    if !first_tempo_set && beat == 0.0 {
                        project.tempo.set_initial_bpm(bpm);
                        first_tempo_set = true;
                    } else {
                        project.tempo.add_change(beat + at_beat, bpm);
                    }
                }
                TrackEventKind::Meta(MetaMessage::TimeSignature(n, d, _, _)) if empty_project && tick == 0 => {
                    project.time_signature = TimeSignature { numerator: n, denominator: 1 << d };
                }
                TrackEventKind::Midi { channel, message } => {
                    let ch = channel.as_int();
                    match message {
                        MidiMessage::NoteOn { key, vel } if vel.as_int() > 0 => {
                            open.insert((ch, key.as_int()), (tick, vel.as_int()));
                        }
                        MidiMessage::NoteOn { key, .. } | MidiMessage::NoteOff { key, .. } => {
                            if let Some((start, vel)) = open.remove(&(ch, key.as_int())) {
                                per_channel.entry(ch).or_default().push(Note {
                                    pitch: key.as_int(),
                                    velocity: vel,
                                    start: start as f64 / ppq,
                                    length: ((tick - start) as f64 / ppq).max(0.01),
                                });
                            }
                        }
                        _ => {}
                    }
                }
                _ => {}
            }
        }
        for (ch, mut notes) in per_channel {
            if notes.is_empty() {
                continue;
            }
            notes.sort_by(|a, b| a.start.partial_cmp(&b.start).unwrap());
            let first = notes.first().unwrap().start;
            let last = notes.iter().map(|n| n.start + n.length).fold(0.0, f64::max);
            let bpb = project.time_signature.beats_per_bar();
            let region_start = (first / bpb).floor() * bpb;
            let region_end = (last / bpb).ceil() * bpb;
            for n in &mut notes {
                n.start -= region_start;
            }
            let is_drum = ch == 9;
            let tname = if name.is_empty() { if is_drum { "Drums".to_string() } else { format!("MIDI Ch {}", ch + 1) } } else { name.clone() };
            let id = if is_drum {
                ops::add_track(project, TrackKind::Instrument, Some(tname.clone()), Some("Pop Drum Kit"), None, None, None)?
            } else {
                ops::add_track(project, TrackKind::Instrument, Some(tname.clone()), None, Some("analog_synth"), Some("Pluck Keys"), None)?
            };
            let rid = project.alloc_id();
            project.track_mut(id).unwrap().regions.push(Region {
                id: rid,
                name: tname,
                start: region_start + at_beat,
                length: (region_end - region_start).max(bpb),
                loop_count: 1,
                muted: false,
                color: None,
                content: RegionContent::Midi(MidiContent { notes, ..Default::default() }), take: None });
            created.push(id);
        }
    }
    let _ = Chord::parse("C");
    Ok(created)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::command::Command;

    #[test]
    fn midi_roundtrip() {
        let mut p = Project::new("x");
        let id = ops::add_track(&mut p, TrackKind::Instrument, None, None, None, None, None).unwrap();
        let _ = ops::apply(&mut p, &Command::CreateMidiRegion { track_id: id, start: 4.0, length: 4.0, name: None, notes: Some(vec![Note { pitch: 60, velocity: 100, start: 0.0, length: 1.0 }, Note { pitch: 64, velocity: 90, start: 1.0, length: 1.0 }]) });
        let bytes = export_midi(&p, None).unwrap();
        let mut q = Project::new("y");
        let created = import_midi(&mut q, &bytes, 0.0).unwrap();
        assert_eq!(created.len(), 1);
        let r = &q.tracks[0].regions[0];
        assert_eq!(r.start, 4.0);
        if let RegionContent::Midi(m) = &r.content {
            assert_eq!(m.notes.len(), 2);
        } else {
            panic!()
        }
    }
}
