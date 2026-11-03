//! Note values for times in beats (quarter notes), and the times the built-in delays play at the project's tempo.

use crate::model::PluginInstance;

/// A note value: its length in beats (a beat is a quarter note) and its name.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct NoteValue {
    pub beats: f32,
    pub name: &'static str,
}

/// Straight, dotted and triplet values from 1/32 to a whole note, shortest first. A beat is a quarter note.
pub const NOTE_VALUES: &[NoteValue] = &[
    NoteValue { beats: 1.0 / 12.0, name: "1/32 triplet" },
    NoteValue { beats: 0.125, name: "1/32" },
    NoteValue { beats: 1.0 / 6.0, name: "1/16 triplet" },
    NoteValue { beats: 0.1875, name: "1/32 dotted" },
    NoteValue { beats: 0.25, name: "1/16" },
    NoteValue { beats: 1.0 / 3.0, name: "1/8 triplet" },
    NoteValue { beats: 0.375, name: "1/16 dotted" },
    NoteValue { beats: 0.5, name: "1/8" },
    NoteValue { beats: 2.0 / 3.0, name: "1/4 triplet" },
    NoteValue { beats: 0.75, name: "1/8 dotted" },
    NoteValue { beats: 1.0, name: "1/4" },
    NoteValue { beats: 4.0 / 3.0, name: "1/2 triplet" },
    NoteValue { beats: 1.5, name: "1/4 dotted" },
    NoteValue { beats: 2.0, name: "1/2" },
    NoteValue { beats: 8.0 / 3.0, name: "1/1 triplet" },
    NoteValue { beats: 3.0, name: "1/2 dotted" },
    NoteValue { beats: 4.0, name: "1/1" },
];

/// How far (in beats) a time may be from a note value and still carry its name: 0.333 is a 1/8 triplet.
pub const NOTE_TOLERANCE: f32 = 0.002;

/// Longest delay the built-in delay lines hold (8 s of audio; see StereoDelay::new and TapeDelay::new).
pub const DELAY_MAX_MS: f32 = 8000.0;

/// With Tempo Sync off the Stereo Delay plays Time (unsynced) on the left and this much of it on the right.
pub const FREE_RIGHT_RATIO: f32 = 0.75;

/// The name of the note value nearest to `beats`, if it is within NOTE_TOLERANCE of it.
pub fn note_value_name(beats: f32) -> Option<&'static str> {
    if !beats.is_finite() {
        return None;
    }
    NOTE_VALUES
        .iter()
        .min_by(|a, b| (a.beats - beats).abs().total_cmp(&(b.beats - beats).abs()))
        .filter(|n| (n.beats - beats).abs() <= NOTE_TOLERANCE)
        .map(|n| n.name)
}

/// The note value's name, or the beats themselves ("0.71 beats") for a time off the note values.
pub fn note_value_label(beats: f32) -> String {
    match note_value_name(beats) {
        Some(name) => name.to_string(),
        None => {
            let text = format!("{beats:.2}");
            let text = text.trim_end_matches('0').trim_end_matches('.');
            format!("{text} beats")
        }
    }
}

/// The note value nearest to `beats` (on the beat axis, as the eye sees it) among those within min..=max; `beats`
/// clamped to the range when no note value is in it.
pub fn snap_note_value(beats: f32, min: f32, max: f32) -> f32 {
    NOTE_VALUES
        .iter()
        .filter(|n| (min..=max).contains(&n.beats))
        .min_by(|a, b| (a.beats - beats).abs().total_cmp(&(b.beats - beats).abs()))
        .map_or_else(|| beats.clamp(min, max), |n| n.beats)
}

pub fn beats_to_ms(beats: f64, bpm: f64) -> f64 {
    beats * 60000.0 / bpm.max(1.0)
}

pub fn ms_to_beats(ms: f64, bpm: f64) -> f64 {
    ms * bpm.max(1.0) / 60000.0
}

/// "375 ms": whole milliseconds.
pub fn ms_text(ms: f64) -> String {
    format!("{} ms", ms.round())
}

/// "1/8 dotted · 375 ms": what a time in beats is at `bpm`.
pub fn time_label(beats: f32, bpm: f64) -> String {
    format!("{} \u{00B7} {}", note_value_label(beats), ms_text(beats_to_ms(beats as f64, bpm)))
}

/// The times a delay echoes at, in ms.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct DelayTimes {
    pub left_ms: f32,
    /// None for the Tape Delay, which has one repeat for both sides.
    pub right_ms: Option<f32>,
    pub synced: bool,
}

/// The times plug-in `inst` echoes at, in ms at `bpm`, as StereoDelay::process and TapeDelay::process compute them
/// (capped at DELAY_MAX_MS). None for a plug-in that is not one of the two delays.
pub fn delay_times(inst: &PluginInstance, bpm: f64) -> Option<DelayTimes> {
    let ms = |beats: f32| (beats_to_ms(beats as f64, bpm) as f32).min(DELAY_MAX_MS);
    match inst.kind.as_str() {
        "stereo_delay" if inst.param("sync") >= 0.5 => Some(DelayTimes { left_ms: ms(inst.param("time_l")), right_ms: Some(ms(inst.param("time_r"))), synced: true }),
        "stereo_delay" => {
            let t = inst.param("time_ms");
            Some(DelayTimes { left_ms: t.min(DELAY_MAX_MS), right_ms: Some((t * FREE_RIGHT_RATIO).min(DELAY_MAX_MS)), synced: false })
        }
        "tape_delay" => Some(DelayTimes { left_ms: ms(inst.param("time")), right_ms: None, synced: true }),
        _ => None,
    }
}

/// "120", "118.5": a tempo to one decimal, without trailing zeros.
pub fn bpm_text(bpm: f64) -> String {
    format!("{}", (bpm * 10.0).round() / 10.0)
}

/// One sentence about the times of a delay, for the assistant and for get_plugin. None for other kinds.
pub fn timing_summary(inst: &PluginInstance, bpm: f64, tempo_changes: bool) -> Option<String> {
    let t = delay_times(inst, bpm)?;
    let at = |beats: f32, ms: f32| format!("{} ({})", note_value_label(beats), ms_text(ms as f64));
    let mut s = match (inst.kind.as_str(), t.right_ms) {
        ("stereo_delay", Some(right)) if t.synced => {
            format!("Synced to the tempo: left {}, right {} at {} BPM.", at(inst.param("time_l"), t.left_ms), at(inst.param("time_r"), right), bpm_text(bpm))
        }
        ("stereo_delay", Some(right)) => format!(
            "Tempo Sync is off: left {}, right {} (always 3/4 of the left); Left Time and Right Time are not used until Tempo Sync is on.",
            ms_text(t.left_ms as f64),
            ms_text(right as f64)
        ),
        _ => format!("Synced to the tempo: {} at {} BPM.", at(inst.param("time"), t.left_ms), bpm_text(bpm)),
    };
    if tempo_changes {
        s.push_str(" The tempo changes along the song and the delay follows it; these ms are at the first tempo.");
    }
    Some(s)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn instance(kind: &str, kv: &[(&str, f32)]) -> PluginInstance {
        PluginInstance {
            id: 1,
            kind: kind.into(),
            params: kv.iter().map(|(k, v)| (k.to_string(), *v)).collect(),
            bypass: false,
            preset: None,
            sample: None,
            vst3_path: None,
            vst3_params: vec![],
            vst3_state: None,
        }
    }

    fn near(a: f32, b: f32) -> bool {
        (a - b).abs() < 1e-4
    }

    #[test]
    fn snap_and_names_follow_the_note_values() {
        assert_eq!(note_value_name(0.75), Some("1/8 dotted"));
        assert_eq!(note_value_name(0.333), Some("1/8 triplet"));
        assert_eq!(note_value_name(0.5), Some("1/8"));
        assert_eq!(note_value_name(4.0), Some("1/1"));
        assert_eq!(note_value_name(0.71), None);
        assert_eq!(note_value_name(f32::NAN), None);
        assert_eq!(snap_note_value(0.74, 0.0625, 4.0), 0.75);
        assert!(near(snap_note_value(0.35, 0.0625, 4.0), 1.0 / 3.0));
        assert!(near(snap_note_value(0.01, 0.0625, 4.0), 1.0 / 12.0));
        assert_eq!(snap_note_value(9.0, 0.0625, 4.0), 4.0);
        assert_eq!(snap_note_value(9.0, 5.0, 6.0), 6.0);
        assert!(NOTE_VALUES.windows(2).all(|w| w[0].beats < w[1].beats));
        assert!(NOTE_VALUES.iter().all(|n| (0.0625..=4.0).contains(&n.beats)));
    }

    #[test]
    fn labels_read_note_values_and_ms() {
        assert_eq!(time_label(0.75, 120.0), "1/8 dotted \u{00B7} 375 ms");
        assert_eq!(time_label(0.5, 90.0), "1/8 \u{00B7} 333 ms");
        assert_eq!(note_value_label(0.71), "0.71 beats");
        assert_eq!(note_value_label(0.7), "0.7 beats");
        assert_eq!(note_value_label(0.333), "1/8 triplet");
        assert_eq!(beats_to_ms(0.75, 120.0), 375.0);
        assert_eq!(ms_to_beats(375.0, 120.0), 0.75);
        assert_eq!(ms_text(262.5), "263 ms");
        assert_eq!(bpm_text(120.0), "120");
        assert_eq!(bpm_text(118.54), "118.5");
    }

    #[test]
    fn delay_times_follow_the_delays() {
        let d = delay_times(&instance("stereo_delay", &[]), 120.0).unwrap();
        assert_eq!(d, DelayTimes { left_ms: 375.0, right_ms: Some(250.0), synced: true });
        let d = delay_times(&instance("stereo_delay", &[("sync", 0.0), ("time_ms", 200.0)]), 120.0).unwrap();
        assert_eq!(d, DelayTimes { left_ms: 200.0, right_ms: Some(150.0), synced: false });
        let d = delay_times(&instance("tape_delay", &[]), 120.0).unwrap();
        assert_eq!(d, DelayTimes { left_ms: 250.0, right_ms: None, synced: true });
        let d = delay_times(&instance("stereo_delay", &[("time_l", 4.0)]), 20.0).unwrap();
        assert_eq!(d.left_ms, DELAY_MAX_MS);
        assert_eq!(delay_times(&instance("compressor", &[]), 120.0), None);
    }

    #[test]
    fn timing_summary_names_the_times() {
        assert_eq!(timing_summary(&instance("stereo_delay", &[]), 120.0, false).unwrap(), "Synced to the tempo: left 1/8 dotted (375 ms), right 1/8 (250 ms) at 120 BPM.");
        assert_eq!(
            timing_summary(&instance("stereo_delay", &[("sync", 0.0)]), 120.0, false).unwrap(),
            "Tempo Sync is off: left 350 ms, right 263 ms (always 3/4 of the left); Left Time and Right Time are not used until Tempo Sync is on."
        );
        assert_eq!(timing_summary(&instance("tape_delay", &[]), 120.0, false).unwrap(), "Synced to the tempo: 1/8 (250 ms) at 120 BPM.");
        assert_eq!(timing_summary(&instance("tape_delay", &[("time", 0.71)]), 118.5, false).unwrap(), "Synced to the tempo: 0.71 beats (359 ms) at 118.5 BPM.");
        let changes = timing_summary(&instance("tape_delay", &[]), 120.0, true).unwrap();
        assert!(changes.ends_with(" The tempo changes along the song and the delay follows it; these ms are at the first tempo."), "{changes}");
        assert_eq!(timing_summary(&instance("reverb", &[]), 120.0, false), None);
    }
}
