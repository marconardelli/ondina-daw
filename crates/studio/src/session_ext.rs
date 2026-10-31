//! Session handlers for the extended command set that need audio data, the engine, files
//! or external tools: Flex Time/Pitch, Stem Splitter, Live Loops transport, surround bounce,
//! movies, VST3 hosting, live MIDI and settings.

use std::path::{Path, PathBuf};
use std::sync::Arc;

use aurora_core::command::Command;
use aurora_core::model::*;
use aurora_core::ops;
use aurora_dsp::{audio_ops, AudioData, MidiKind};
use aurora_engine::{load_audio_file, render_offline, render_surround, write_multichannel_wav, write_wav, OfflineOptions};
use aurora_engine::offline::{AUTO_TAIL_MAX_SEC, BOUNCE_TAIL_SEC};
use serde_json::{json, Value};

use crate::session::Session;

fn ops2_musicxml(p: &Project, ids: Option<&[Id]>) -> Result<String, String> {
    aurora_core::ops2::musicxml(p, ids)
}

/// Locate ffmpeg: next to the executable, in ./tools/ffmpeg/bin, or on PATH.
pub fn ffmpeg_path() -> Option<PathBuf> {
    let mut candidates: Vec<PathBuf> = vec![];
    if let Ok(exe) = std::env::current_exe() {
        for anc in exe.ancestors().take(5) {
            candidates.push(anc.join("ffmpeg.exe"));
            candidates.push(anc.join("tools").join("ffmpeg").join("bin").join("ffmpeg.exe"));
        }
    }
    candidates.push(PathBuf::from("tools").join("ffmpeg").join("bin").join("ffmpeg.exe"));
    if let Some(found) = candidates.into_iter().find(|p| p.exists()) {
        return Some(found);
    }
    let ok = std::process::Command::new("ffmpeg").arg("-version").stdout(std::process::Stdio::null()).stderr(std::process::Stdio::null()).status().map(|s| s.success()).unwrap_or(false);
    ok.then(|| PathBuf::from("ffmpeg"))
}

fn run_ffmpeg(args: &[&str]) -> Result<(), String> {
    let ff = ffmpeg_path().ok_or("ffmpeg not found: install it on PATH or place it in tools\\ffmpeg\\bin")?;
    let out = std::process::Command::new(ff).args(["-hide_banner", "-loglevel", "error", "-y"]).args(args).output().map_err(|e| e.to_string())?;
    if out.status.success() {
        Ok(())
    } else {
        Err(format!("ffmpeg failed: {}", String::from_utf8_lossy(&out.stderr).trim()))
    }
}

impl Session {
    /// Returns `Some(result)` when the command is handled here.
    pub(crate) fn dispatch_ext(&mut self, cmd: &Command) -> Option<Result<Value, String>> {
        use Command::*;
        Some(match cmd {
            SendLiveMidi { track_id, events } => self.send_live_midi(*track_id, events),
            TimeStretchRegion { region_id, ratio, target_length } => {
                let len = match self.project.find_region(*region_id) {
                    Some((_, r)) => r.length,
                    None => return Some(Err("region not found".into())),
                };
                let ratio = match (ratio, target_length) {
                    (Some(r), _) => *r,
                    (None, Some(t)) => t / len.max(1e-6),
                    _ => return Some(Err("give ratio or target_length".into())),
                };
                if !(0.1..=10.0).contains(&ratio) {
                    return Some(Err("ratio must be between 0.1 and 10".into()));
                }
                self.process_audio_region(*region_id, "stretch", move |d| audio_ops::time_stretch(d, ratio))
            }
            PitchShiftRegion { region_id, semitones } => {
                let s = semitones.clamp(-24.0, 24.0);
                self.process_audio_region(*region_id, "pitch", move |d| audio_ops::pitch_shift(d, s))
            }
            VarispeedRegion { region_id, percent } => {
                let p = *percent;
                self.process_audio_region(*region_id, "varispeed", move |d| audio_ops::varispeed(d, p))
            }
            PitchCorrectRegion { region_id, strength } => {
                let key = self.project.key;
                let st = strength.unwrap_or(1.0).clamp(0.0, 1.0);
                self.process_audio_region(*region_id, "tuned", move |d| {
                    audio_ops::pitch_correct(
                        d,
                        |m| {
                            let base = m.round() as i32;
                            (base - 6..=base + 6).filter(|p| (0..=127).contains(p) && key.contains(*p as u8)).min_by(|a, b| ((*a as f32 - m).abs()).partial_cmp(&(*b as f32 - m).abs()).unwrap()).map(|p| p as f32).unwrap_or(m)
                        },
                        st,
                    )
                })
            }
            AudioToMidi { region_id, track_id } => self.audio_to_midi(*region_id, *track_id),
            DetectTempo { region_id, apply } => self.detect_tempo(*region_id, apply.unwrap_or(false)),
            NormalizeRegion { region_id, target_db } => self.normalize_region(*region_id, target_db.unwrap_or(-1.0)),
            SplitStems { region_id, engine } => self.split_stems(*region_id, engine.as_deref()),
            ExportMusicXml { path, track_ids } => match ops2_musicxml(&self.project, track_ids.as_deref()) {
                Ok(xml) => match std::fs::write(path, xml) {
                    Ok(()) => Ok(json!({"path": path})),
                    Err(e) => Err(format!("cannot write {path}: {e}")),
                },
                Err(e) => Err(e),
            },
            GetLiveLoops {} => {
                let ls = self.engine.shared.live_state.lock().clone();
                let cells: Vec<Value> = self
                    .project
                    .live
                    .cells
                    .iter()
                    .map(|c| {
                        let kind = match &c.content {
                            RegionContent::Midi(m) => json!({"type": "midi", "note_count": m.notes.len()}),
                            RegionContent::Pattern(_) => json!({"type": "pattern"}),
                            RegionContent::Audio(_) => json!({"type": "audio"}),
                        };
                        json!({"cell_id": c.id, "track_id": c.track_id, "scene": c.scene, "name": c.name, "length": c.length, "content": kind, "playing": ls.playing.iter().any(|p| p.1 == c.id)})
                    })
                    .collect();
                Ok(json!({"scenes": self.project.live.scenes, "quantize": self.project.live.quantize, "cells": cells, "playing": ls.playing.iter().map(|p| json!({"track_id": p.0, "cell_id": p.1, "since_beat": p.2})).collect::<Vec<_>>(), "queued": ls.queued.iter().map(|q| json!({"at_beat": q.0, "track_id": q.1, "cell_id": q.2})).collect::<Vec<_>>()}))
            }
            LaunchCell { cell_id } => {
                if !self.project.live.cells.iter().any(|c| c.id == *cell_id) {
                    return Some(Err("cell not found".into()));
                }
                self.engine.live_launch(*cell_id);
                Ok(json!({"launched": cell_id, "quantize": self.project.live.quantize}))
            }
            LaunchScene { scene } => {
                if *scene >= self.project.live.scenes.len() {
                    return Some(Err("scene not found".into()));
                }
                self.engine.live_scene(*scene);
                Ok(json!({"launched_scene": scene}))
            }
            StopLiveCells { track_id } => {
                self.engine.live_stop(*track_id);
                Ok(json!({"ok": true}))
            }
            CommitLivePerformance {} => self.commit_live(),
            BounceSurround { path, format } => {
                let fmt = format.unwrap_or(self.project.surround);
                let name = match fmt {
                    SurroundFormat::Surround71 => "7.1",
                    SurroundFormat::Immersive714 => "7.1.4",
                    _ => "5.1",
                };
                let end = self.project.content_end();
                if end <= 0.0 {
                    return Some(Err("project is empty".into()));
                }
                let sr = self.engine.sample_rate();
                let (names, chans) = render_surround(&self.project, Arc::new(self.pool.clone()), sr, 0.0, end, name, AUTO_TAIL_MAX_SEC);
                if let Err(e) = write_multichannel_wav(Path::new(path), &chans, sr) {
                    return Some(Err(e));
                }
                let peaks: Vec<Value> = names.iter().zip(&chans).map(|(n, c)| json!({"channel": n, "peak_db": (aurora_dsp::util::gain_to_db(c.iter().fold(0.0f32, |a, b| a.max(b.abs()))) * 10.0).round() / 10.0})).collect();
                Ok(json!({"path": path, "format": name, "channels": names, "duration_sec": chans[0].len() as f64 / sr as f64, "channel_peaks": peaks}))
            }
            BounceBinaural { path } => {
                let end = self.project.content_end();
                if end <= 0.0 {
                    return Some(Err("project is empty".into()));
                }
                let mut p = self.project.clone();
                p.surround = SurroundFormat::Binaural;
                let sr = self.engine.sample_rate();
                let res = render_offline(&p, Arc::new(self.pool.clone()), OfflineOptions { sample_rate: sr, start: 0.0, end, tail_sec: BOUNCE_TAIL_SEC, auto_tail_max_sec: AUTO_TAIL_MAX_SEC, solo: None, analyze: false, keep_audio: true });
                match write_wav(Path::new(path), &res.left, &res.right, sr, 24) {
                    Ok(()) => Ok(json!({"path": path, "duration_sec": res.left.len() as f64 / sr as f64})),
                    Err(e) => Err(e),
                }
            }
            ExportAdm { path, start, end } => self.export_adm(path, *start, *end),
            LoadMovie { path, offset_sec, import_audio } => self.load_movie(path, offset_sec.unwrap_or(0.0), import_audio.unwrap_or(true)),
            ExtractMovieFrame { path, beat } => {
                let Some(m) = self.project.movie.clone() else { return Some(Err("no movie loaded".into())) };
                let b = beat.unwrap_or(self.engine.position());
                let t = (self.project.tempo.beat_to_sec(b) - m.offset_sec).max(0.0);
                let ts = format!("{t:.3}");
                match run_ffmpeg(&["-ss", &ts, "-i", &m.path, "-frames:v", "1", path]) {
                    Ok(()) => Ok(json!({"path": path, "movie_time_sec": t, "timecode": self.project.timecode(b)})),
                    Err(e) => Err(e),
                }
            }
            ExportMovie { path } => {
                let Some(m) = self.project.movie.clone() else { return Some(Err("no movie loaded".into())) };
                let tmp = std::env::temp_dir().join(format!("aurora_mix_{}.wav", std::process::id()));
                let sr = self.engine.sample_rate();
                let start = self.project.tempo.sec_to_beat(m.offset_sec);
                let end = self.project.tempo.sec_to_beat(m.offset_sec + m.duration_sec.max(1.0));
                let mut p = self.project.clone();
                if let Some(aid) = m.audio_file_id {
                    // keep the guide audio out of the export
                    for t in &mut p.tracks {
                        t.regions.retain(|r| !matches!(&r.content, RegionContent::Audio(a) if a.file_id == aid));
                    }
                }
                let res = render_offline(&p, Arc::new(self.pool.clone()), OfflineOptions { sample_rate: sr, start, end, tail_sec: 0.0, auto_tail_max_sec: 0.0, solo: None, analyze: false, keep_audio: true });
                if let Err(e) = write_wav(&tmp, &res.left, &res.right, sr, 24) {
                    return Some(Err(e));
                }
                let r = run_ffmpeg(&["-i", &m.path, "-i", &tmp.display().to_string(), "-map", "0:v:0", "-map", "1:a:0", "-c:v", "copy", "-c:a", "aac", "-b:a", "256k", "-shortest", path]);
                let _ = std::fs::remove_file(&tmp);
                r.map(|_| json!({"path": path}))
            }
            ScanVst3 { extra_paths } => match aurora_dsp::vst3::scan(extra_paths.as_deref().unwrap_or(&[])) {
                Ok(list) => Ok(json!({"plugins": list.iter().map(|p| json!({"path": p.path, "name": p.name, "vendor": p.vendor, "version": p.version, "category": p.category, "instrument": p.is_instrument, "audio_inputs": p.audio_inputs, "audio_outputs": p.audio_outputs})).collect::<Vec<_>>()})),
                Err(e) => Err(e),
            },
            AddVst3 { track_id, path, as_instrument, index } => self.add_vst3(*track_id, path, *as_instrument, *index),
            OpenPluginEditor { plugin_id } => {
                let Some(p) = self.project.find_plugin(*plugin_id) else { return Some(Err("plug-in not found".into())) };
                if p.kind != "vst3" {
                    return Some(Err("built-in plug-ins are edited in the Inspector; native editor windows are for VST3 plug-ins".into()));
                }
                let path = p.vst3_path.clone().unwrap_or_default();
                let params: Vec<(String, f32)> = p.params.iter().map(|(k, v)| (k.clone(), *v)).collect();
                let state = p.vst3_state.as_deref().and_then(aurora_core::model::base64_decode);
                aurora_dsp::vst3::open_editor(*plugin_id, &path, params, state).map(|_| json!({"open": true, "plugin_id": plugin_id}))
            }
            ClosePluginEditor { plugin_id } => {
                aurora_dsp::vst3::close_editor(*plugin_id);
                Ok(json!({"open": false}))
            }
            GetSettings {} => Ok(json!({
                "model": self.settings.model(), "effort": self.settings.effort(), "mcp_enabled": self.settings.mcp_enabled, "mcp_port": self.settings.mcp_port,
                "audio_output": self.settings.audio_output, "api_key_configured": self.settings.api_key().is_some(), "ffmpeg": ffmpeg_path().map(|p| p.display().to_string())
            })),
            SetAiSettings { model, effort } => {
                if let Some(m) = model {
                    self.settings.model = m.clone();
                }
                if let Some(e) = effort {
                    if !matches!(e.as_str(), "low" | "medium" | "high" | "xhigh" | "max") {
                        return Some(Err("effort must be low, medium, high, xhigh or max".into()));
                    }
                    self.settings.effort = e.clone();
                }
                self.settings.save();
                Ok(json!({"model": self.settings.model(), "effort": self.settings.effort()}))
            }
            _ => return None,
        })
    }

    fn send_live_midi(&mut self, track_id: Id, events: &[aurora_core::command::LiveMidiEvent]) -> Result<Value, String> {
        let t = self.project.track(track_id).ok_or("track not found")?;
        if t.kind != TrackKind::Instrument {
            return Err("live MIDI needs an instrument track".into());
        }
        let total: u64 = events.iter().filter_map(|e| e.delay_ms).sum();
        if total > 30_000 {
            return Err("total delay must be at most 30 s".into());
        }
        // schedule on a helper thread so the session is not blocked
        let tx = self.engine_sender();
        let evs: Vec<(u64, MidiKind)> = events
            .iter()
            .map(|e| {
                let k = match e.kind.as_str() {
                    "note_on" => Ok(MidiKind::NoteOn { pitch: e.number.min(127), velocity: e.value.unwrap_or(100).clamp(1, 127) }),
                    "note_off" => Ok(MidiKind::NoteOff { pitch: e.number.min(127) }),
                    "cc" => Ok(MidiKind::Cc { controller: e.number.min(127), value: e.value.unwrap_or(0).min(127) }),
                    other => Err(format!("unknown event type '{other}'")),
                }?;
                Ok((e.delay_ms.unwrap_or(0), k))
            })
            .collect::<Result<_, String>>()?;
        let n = evs.len();
        std::thread::spawn(move || {
            for (d, k) in evs {
                if d > 0 {
                    std::thread::sleep(std::time::Duration::from_millis(d));
                }
                let _ = tx.send(aurora_engine::EngineMsg::Live(track_id, k));
            }
        });
        Ok(json!({"scheduled_events": n, "duration_ms": total}))
    }

    /// Extract the audio covered by one iteration of an audio region.
    fn region_audio(&self, region_id: Id) -> Result<(Id, Region, AudioData), String> {
        let (t, r) = self.project.find_region(region_id).ok_or("region not found")?;
        let RegionContent::Audio(a) = &r.content else { return Err("not an audio region".into()) };
        let data = self.pool.get(&a.file_id).ok_or("audio data not loaded")?;
        let secs = self.project.tempo.beat_to_sec(r.start + r.length) - self.project.tempo.beat_to_sec(r.start);
        let sr = data.sample_rate as f64;
        let s0 = (a.offset_sec * sr) as usize;
        let s1 = ((a.offset_sec + secs) * sr) as usize;
        let channels = data.channels.iter().map(|c| c[s0.min(c.len())..s1.min(c.len())].to_vec()).collect();
        Ok((t.id, r.clone(), AudioData { sample_rate: data.sample_rate, channels }))
    }

    fn save_audio(&mut self, d: &AudioData, name: &str) -> Result<Id, String> {
        let file = self.media_dir().join(format!("{} {}.wav", name.replace(['\\', '/', ':', '*', '?', '"', '<', '>', '|'], "_"), self.project.next_id));
        let l = &d.channels[0];
        let r = if d.channels.len() > 1 { &d.channels[1] } else { &d.channels[0] };
        write_wav(&file, l, r, d.sample_rate, 32)?;
        self.load_audio(&file)
    }

    fn process_audio_region(&mut self, region_id: Id, label: &str, f: impl Fn(&AudioData) -> AudioData) -> Result<Value, String> {
        let (_, r, data) = self.region_audio(region_id)?;
        if data.frames() < 16 {
            return Err("region contains no audio".into());
        }
        let out = f(&data);
        let file_id = self.save_audio(&out, &format!("{} ({label})", r.name))?;
        let start_sec = self.project.tempo.beat_to_sec(r.start);
        let new_len = self.project.tempo.sec_to_beat(start_sec + out.duration_sec()) - r.start;
        let reg = self.project.find_region_mut(region_id).unwrap();
        if let RegionContent::Audio(a) = &mut reg.content {
            a.file_id = file_id;
            a.offset_sec = 0.0;
        }
        reg.length = new_len.max(0.01);
        Ok(json!({"region_id": region_id, "file_id": file_id, "length_beats": reg.length, "duration_sec": out.duration_sec()}))
    }

    fn audio_to_midi(&mut self, region_id: Id, track_id: Option<Id>) -> Result<Value, String> {
        let (_, r, data) = self.region_audio(region_id)?;
        let notes = audio_ops::transcribe(&data);
        if notes.is_empty() {
            return Err("no pitched notes detected (use a monophonic, dry recording)".into());
        }
        let tempo = self.project.tempo.clone();
        let s0 = tempo.beat_to_sec(r.start);
        let midi: Vec<Note> = notes
            .iter()
            .map(|(s, d, p, v)| {
                let b0 = tempo.sec_to_beat(s0 + s) - r.start;
                let b1 = tempo.sec_to_beat(s0 + s + d) - r.start;
                Note { pitch: *p, velocity: *v, start: b0.max(0.0), length: (b1 - b0).max(0.05) }
            })
            .collect();
        let tid = match track_id {
            Some(t) => t,
            None => ops::add_track(&mut self.project, TrackKind::Instrument, Some(format!("{} (MIDI)", r.name)), None, Some("analog_synth"), Some("Soft Sine"), None)?,
        };
        let rid = self.project.alloc_id();
        let t = self.project.track_mut(tid).ok_or("track not found")?;
        if t.kind != TrackKind::Instrument {
            return Err("target must be an instrument track".into());
        }
        let n = midi.len();
        t.regions.push(Region { take: None, id: rid, name: format!("{} (MIDI)", r.name), start: r.start, length: r.length, loop_count: 1, muted: false, color: None, content: RegionContent::Midi(MidiContent { notes: midi, ..Default::default() }) });
        Ok(json!({"track_id": tid, "region_id": rid, "note_count": n}))
    }

    fn detect_tempo(&mut self, region_id: Id, apply: bool) -> Result<Value, String> {
        let (_, _, data) = self.region_audio(region_id)?;
        if data.duration_sec() < 3.0 {
            return Err("need at least 3 seconds of audio".into());
        }
        let (bpm, conf) = audio_ops::detect_tempo(&data);
        let bpm = (bpm * 10.0).round() / 10.0;
        if apply {
            self.project.tempo.set_initial_bpm(bpm);
        }
        Ok(json!({"bpm": bpm, "confidence": (conf * 100.0).round() / 100.0, "applied": apply}))
    }

    fn normalize_region(&mut self, region_id: Id, target_db: f32) -> Result<Value, String> {
        let (_, _, data) = self.region_audio(region_id)?;
        let peak = data.channels.iter().flat_map(|c| c.iter()).fold(0.0f32, |a, b| a.max(b.abs()));
        if peak < 1e-6 {
            return Err("region is silent".into());
        }
        let gain = target_db - aurora_dsp::util::gain_to_db(peak);
        if let Some(r) = self.project.find_region_mut(region_id) {
            if let RegionContent::Audio(a) = &mut r.content {
                a.gain_db = gain.clamp(-60.0, 24.0);
            }
        }
        Ok(json!({"gain_db": gain}))
    }

    fn split_stems(&mut self, region_id: Id, engine: Option<&str>) -> Result<Value, String> {
        let (src_track, r, data) = self.region_audio(region_id)?;
        if data.duration_sec() < 0.5 {
            return Err("region too short".into());
        }
        let model = aurora_dsp::ml_stems::find_model_dir();
        let use_ml = match engine {
            Some("spectral") => false,
            Some("ml") => {
                if model.is_none() {
                    return Err("ML stem model not installed (tools/models/sherpa-onnx-spleeter-2stems-fp16)".into());
                }
                true
            }
            Some(other) => return Err(format!("unknown engine '{other}' (use ml or spectral)")),
            None => model.is_some(),
        };
        let stems = if use_ml { aurora_dsp::ml_stems::ml_four_stems(&data, model.as_ref().unwrap())? } else { audio_ops::split_stems(&data) };
        let names = ["Vocals", "Drums", "Bass", "Other"];
        let mut out = vec![];
        let mut insert_at = self.project.track_index(src_track).map(|i| i + 1);
        for (k, stem) in stems.iter().enumerate() {
            let file_id = self.save_audio(stem, &format!("{} - {}", r.name, names[k]))?;
            let tid = ops::add_track(&mut self.project, TrackKind::Audio, Some(format!("{} - {}", r.name, names[k])), None, None, None, insert_at)?;
            insert_at = insert_at.map(|i| i + 1);
            let rid = self.project.alloc_id();
            self.project.track_mut(tid).unwrap().regions.push(Region { take: None, id: rid, name: names[k].into(), start: r.start, length: r.length, loop_count: 1, muted: false, color: None, content: RegionContent::Audio(AudioContent { file_id, offset_sec: 0.0, gain_db: 0.0, fade_in: 0.0, fade_out: 0.0, reverse: false }) });
            let e = stem.channels[0].iter().map(|v| v * v).sum::<f32>() / stem.frames().max(1) as f32;
            out.push(json!({"stem": names[k], "track_id": tid, "region_id": rid, "rms_db": (aurora_dsp::util::gain_to_db(e.sqrt()) * 10.0).round() / 10.0}));
        }
        if let Some(reg) = self.project.find_region_mut(region_id) {
            reg.muted = true;
        }
        Ok(json!({"stems": out, "engine": if use_ml { "ml (spleeter neural network + spectral drum/bass split)" } else { "spectral" }, "source_region_muted": true}))
    }

    fn commit_live(&mut self) -> Result<Value, String> {
        let mut log = std::mem::take(&mut *self.engine.shared.live_log.lock());
        let now = self.engine.position();
        let playing = self.engine.shared.live_state.lock().playing.clone();
        for (t, c, s) in playing {
            log.push(aurora_engine::LiveLogEntry { track_id: t, cell_id: c, start: s, end: now.max(s) });
        }
        if log.is_empty() {
            return Err("nothing has been played in Live Loops yet".into());
        }
        let mut created = vec![];
        for e in log {
            let Some(cell) = self.project.live.cells.iter().find(|c| c.id == e.cell_id).cloned() else { continue };
            let dur = e.end - e.start;
            if dur < 0.01 {
                continue;
            }
            let full = (dur / cell.length).floor() as u32;
            let rest = dur - full as f64 * cell.length;
            if full > 0 {
                let id = self.project.alloc_id();
                if let Some(t) = self.project.track_mut(e.track_id) {
                    t.regions.push(Region { take: None, id, name: cell.name.clone(), start: e.start, length: cell.length, loop_count: full, muted: false, color: None, content: cell.content.clone() });
                    created.push(id);
                }
            }
            if rest > 0.05 {
                let id = self.project.alloc_id();
                if let Some(t) = self.project.track_mut(e.track_id) {
                    t.regions.push(Region { take: None, id, name: cell.name.clone(), start: e.start + full as f64 * cell.length, length: rest, loop_count: 1, muted: false, color: None, content: cell.content.clone() });
                    created.push(id);
                }
            }
        }
        Ok(json!({"created_regions": created}))
    }

    fn export_adm(&mut self, path: &str, start: Option<f64>, end: Option<f64>) -> Result<Value, String> {
        let start = start.unwrap_or(0.0);
        let end = end.unwrap_or_else(|| self.project.content_end());
        if end <= start {
            return Err("project is empty".into());
        }
        let sr = 48000; // ADM/Atmos masters are 48 kHz
        let (bed_audio, objs) = aurora_engine::render_objects(&self.project, Arc::new(self.pool.clone()), sr, start, end);
        let defs = aurora_engine::adm::bed_712();
        let bed: Vec<aurora_engine::adm::BedChannel> = defs.iter().zip(bed_audio).map(|((label, spk, az, el), audio)| aurora_engine::adm::BedChannel { label, speaker: spk, azimuth: *az, elevation: *el, audio }).collect();
        let duration = bed[0].audio.len() as f64 / sr as f64;
        let t0 = self.project.tempo.beat_to_sec(start);
        let mut objects = vec![];
        for (tid, audio) in objs {
            let t = self.project.track(tid).unwrap();
            let sp = t.channel.surround_pan;
            let lane = |name: &str| t.automation.iter().find(|l| l.target == name && l.enabled && !l.points.is_empty()).cloned();
            let (la, le, ls) = (lane("surround:azimuth"), lane("surround:elevation"), lane("surround:spread"));
            let animated = la.is_some() || le.is_some() || ls.is_some();
            let step = if animated { 0.1 } else { duration };
            let mut blocks = vec![];
            let mut ts = 0.0;
            while ts < duration - 1e-9 {
                let d = step.min(duration - ts);
                let beat = self.project.tempo.sec_to_beat(t0 + ts);
                blocks.push(aurora_engine::adm::ObjectBlock {
                    start_sec: ts,
                    duration_sec: d,
                    azimuth: la.as_ref().and_then(|l| l.value_at(beat)).unwrap_or(sp.azimuth),
                    elevation: le.as_ref().and_then(|l| l.value_at(beat)).unwrap_or(sp.elevation).clamp(-90.0, 90.0),
                    width: ls.as_ref().and_then(|l| l.value_at(beat)).unwrap_or(sp.spread).clamp(0.0, 1.0),
                });
                ts += d;
            }
            objects.push(aurora_engine::adm::AdmObject { name: t.name.clone(), audio, blocks });
        }
        aurora_engine::adm::write_adm_bwf(Path::new(path), &self.project.name, sr, &bed, &objects)?;
        Ok(json!({"path": path, "sample_rate": sr, "bit_depth": 24, "bed": "7.1.2", "bed_channels": bed.len(), "objects": objects.iter().map(|o| json!({"name": o.name, "position_blocks": o.blocks.len()})).collect::<Vec<_>>(), "duration_sec": duration, "standard": "ITU-R BS.2076 (ADM) + BS.2088 chna in RIFF/WAVE"}))
    }

    fn load_movie(&mut self, path: &str, offset_sec: f64, import_audio: bool) -> Result<Value, String> {
        let p = Path::new(path);
        if !p.exists() {
            return Err(format!("movie not found: {path}"));
        }
        let mut duration = 0.0;
        let mut audio_file_id = None;
        let mut region = None;
        // decode natively, or let ffmpeg extract the soundtrack for codecs symphonia lacks
        let decoded = load_audio_file(p).or_else(|e| {
            if ffmpeg_path().is_none() {
                return Err(e);
            }
            let tmp = std::env::temp_dir().join(format!("aurora_movie_{}.wav", std::process::id()));
            run_ffmpeg(&["-i", path, "-vn", "-acodec", "pcm_f32le", &tmp.display().to_string()])?;
            let d = load_audio_file(&tmp);
            let _ = std::fs::remove_file(&tmp);
            d
        });
        match decoded {
            Ok(d) => {
                duration = d.duration_sec();
                if import_audio {
                    let file_id = self.save_audio(&d, &format!("{} (movie audio)", p.file_stem().and_then(|s| s.to_str()).unwrap_or("movie")))?;
                    audio_file_id = Some(file_id);
                    let tid = ops::add_track(&mut self.project, TrackKind::Audio, Some("Movie Audio".into()), None, None, None, Some(0))?;
                    let start = self.project.tempo.sec_to_beat(offset_sec);
                    let len = self.project.tempo.sec_to_beat(offset_sec + duration) - start;
                    let rid = self.project.alloc_id();
                    self.project.track_mut(tid).unwrap().regions.push(Region { take: None, id: rid, name: "Movie Audio".into(), start, length: len, loop_count: 1, muted: false, color: None, content: RegionContent::Audio(AudioContent { file_id, offset_sec: 0.0, gain_db: 0.0, fade_in: 0.0, fade_out: 0.0, reverse: false }) });
                    region = Some((tid, rid));
                }
            }
            Err(e) if import_audio => return Err(format!("cannot read the movie's audio track: {e}")),
            Err(_) => {}
        }
        let abs = aurora_dsp::vst3::absolute(path);
        self.project.movie = Some(MovieRef { path: abs.clone(), offset_sec, duration_sec: duration, audio_file_id });
        Ok(json!({"movie": abs, "duration_sec": duration, "offset_sec": offset_sec, "audio_track": region.map(|r| json!({"track_id": r.0, "region_id": r.1})), "frame_rate": self.project.frame_rate, "ffmpeg_available": ffmpeg_path().is_some()}))
    }

    fn add_vst3(&mut self, track_id: Id, path: &str, as_instrument: Option<bool>, index: Option<usize>) -> Result<Value, String> {
        let abs = aurora_dsp::vst3::absolute(path);
        let path = abs.as_str();
        let (name, is_inst, params) = aurora_dsp::vst3::probe(path)?;
        let as_inst = as_instrument.unwrap_or(is_inst);
        let inst = PluginInstance {
            id: self.project.alloc_id(),
            kind: "vst3".into(),
            params: Default::default(),
            bypass: false,
            preset: Some(name.clone()),
            sample: None,
            vst3_path: Some(path.to_string()),
            vst3_state: None,
            vst3_params: params.iter().map(|p| Vst3Param { id: format!("p{}", p.id), name: p.name.clone(), default: if p.default.is_finite() { p.default.clamp(0.0, 1.0) as f32 } else { 0.0 }, unit: p.unit.clone() }).collect(),
        };
        let pid = inst.id;
        if as_inst {
            let t = self.project.track_mut(track_id).ok_or("track not found")?;
            if t.kind != TrackKind::Instrument {
                return Err("instruments need an instrument track".into());
            }
            t.instrument = Some(inst);
        } else {
            let c = self.project.channel_mut(track_id).ok_or("track not found")?;
            let i = index.unwrap_or(c.inserts.len()).min(c.inserts.len());
            c.inserts.insert(i, inst);
        }
        Ok(json!({"plugin_id": pid, "name": name, "as_instrument": as_inst, "parameter_count": params.len(), "parameters": params.iter().take(40).map(|p| json!({"id": format!("p{}", p.id), "name": p.name, "default": p.default})).collect::<Vec<_>>()}))
    }
}
