//! The sounds of the Drum Synth's kit as its window shows them: a pad per sound on the General MIDI drum map, the
//! parameters that shape each one, and which group of sounds owns each parameter. The GUI draws the pads from it,
//! the assistant's context names the selected pad from it, and the tests check it against the registry and the sound.

/// A pad of the Drum Synth's window.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct DrumPad {
    /// General MIDI drum note.
    pub note: u8,
    pub name: &'static str,
    /// Name of the DrumGroup it belongs to.
    pub group: &'static str,
    /// The Drum Synth parameters that change this pad's sound, besides Drive and Level.
    pub params: &'static [&'static str],
}

/// A group of sounds and the parameters it owns: every Drum Synth parameter has one owner, or belongs to the Kit.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct DrumGroup {
    pub name: &'static str,
    pub params: &'static [&'static str],
}

/// Parameters that shape the whole kit.
pub const KIT_PARAMS: &[&str] = &["drive", "level"];

/// The pad a window selects when it opens.
pub const DEFAULT_PAD: u8 = 36;

pub const DRUM_GROUPS: [DrumGroup; 7] = [
    DrumGroup { name: "Kick", params: &["kick_tune", "kick_decay", "kick_punch"] },
    DrumGroup { name: "Snare", params: &["snare_tune", "snare_snappy", "snare_decay"] },
    DrumGroup { name: "Clap", params: &["clap_decay"] },
    DrumGroup { name: "Hats", params: &["hat_tone", "hat_decay", "open_hat_decay"] },
    DrumGroup { name: "Toms", params: &["tom_tune"] },
    DrumGroup { name: "Cymbals", params: &["cymbal_decay"] },
    DrumGroup { name: "Percussion", params: &[] },
];

/// Pad 1 (bottom left) to pad 16 (top right), four per row. The metallic tone `hat_tone` shapes the hi-hats, the
/// crash and the ride alike; the Hats own it.
pub const DRUM_PADS: [DrumPad; 16] = [
    DrumPad { note: 36, name: "Kick", group: "Kick", params: &["kick_tune", "kick_decay", "kick_punch"] },
    DrumPad { note: 37, name: "Rim", group: "Percussion", params: &[] },
    DrumPad { note: 38, name: "Snare", group: "Snare", params: &["snare_tune", "snare_snappy", "snare_decay"] },
    DrumPad { note: 39, name: "Clap", group: "Clap", params: &["clap_decay"] },
    DrumPad { note: 42, name: "Closed Hat", group: "Hats", params: &["hat_tone", "hat_decay"] },
    DrumPad { note: 44, name: "Pedal Hat", group: "Hats", params: &["hat_tone", "hat_decay"] },
    DrumPad { note: 46, name: "Open Hat", group: "Hats", params: &["hat_tone", "open_hat_decay"] },
    DrumPad { note: 54, name: "Tambourine", group: "Percussion", params: &[] },
    DrumPad { note: 41, name: "Floor Tom", group: "Toms", params: &["tom_tune"] },
    DrumPad { note: 45, name: "Low Tom", group: "Toms", params: &["tom_tune"] },
    DrumPad { note: 47, name: "Mid Tom", group: "Toms", params: &["tom_tune"] },
    DrumPad { note: 50, name: "High Tom", group: "Toms", params: &["tom_tune"] },
    DrumPad { note: 49, name: "Crash", group: "Cymbals", params: &["hat_tone", "cymbal_decay"] },
    DrumPad { note: 51, name: "Ride", group: "Cymbals", params: &["hat_tone", "cymbal_decay"] },
    DrumPad { note: 56, name: "Cowbell", group: "Percussion", params: &[] },
    DrumPad { note: 75, name: "Claves", group: "Percussion", params: &[] },
];

/// The pad of `note`, if the window has one.
pub fn pad(note: u8) -> Option<&'static DrumPad> {
    DRUM_PADS.iter().find(|p| p.note == note)
}

/// The group that owns parameter `id` (None for the Kit's and for unknown ones).
pub fn owner(id: &str) -> Option<&'static DrumGroup> {
    DRUM_GROUPS.iter().find(|g| g.params.contains(&id))
}

/// Whether parameter `id` shows among the Kit's knobs: Drive, Level, and any parameter no group owns (a parameter
/// added to the Drum Synth later is never hidden).
pub fn is_kit_param(id: &str) -> bool {
    KIT_PARAMS.contains(&id) || owner(id).is_none()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::plugins;

    fn registry() -> &'static plugins::PluginInfo {
        plugins::find("drum_synth").unwrap()
    }

    fn in_registry(id: &str) -> bool {
        registry().params.iter().any(|p| p.id == id)
    }

    #[test]
    fn every_drum_synth_parameter_has_exactly_one_owner() {
        for p in registry().params {
            let owners = DRUM_GROUPS.iter().filter(|g| g.params.contains(&p.id)).count() + KIT_PARAMS.contains(&p.id) as usize;
            assert_eq!(owners, 1, "{} has {owners} owners", p.id);
        }
        for g in &DRUM_GROUPS {
            for (i, id) in g.params.iter().enumerate() {
                assert!(in_registry(id), "{id} of {} is not a Drum Synth parameter", g.name);
                assert!(!g.params[i + 1..].contains(id), "{id} twice in {}", g.name);
            }
        }
        for id in KIT_PARAMS {
            assert!(in_registry(id), "{id} of the Kit is not a Drum Synth parameter");
        }
    }

    #[test]
    fn every_pad_parameter_belongs_to_the_drum_synth() {
        for pad in &DRUM_PADS {
            assert!(DRUM_GROUPS.iter().any(|g| g.name == pad.group), "{} has no group {}", pad.name, pad.group);
            for id in pad.params {
                assert!(in_registry(id), "{id} of {} is not a Drum Synth parameter", pad.name);
                assert!(owner(id).is_some(), "{id} of {} has no owner", pad.name);
                assert!(!KIT_PARAMS.contains(id), "{id} of {} is a Kit parameter", pad.name);
            }
        }
        for g in &DRUM_GROUPS {
            for id in g.params {
                assert!(DRUM_PADS.iter().any(|p| p.params.contains(id)), "{id} of {} is on no pad", g.name);
            }
        }
    }

    #[test]
    fn pads_are_sixteen_distinct_general_midi_notes() {
        for (i, a) in DRUM_PADS.iter().enumerate() {
            assert!((35..=81).contains(&a.note), "{} is not a General MIDI drum note", a.note);
            for b in &DRUM_PADS[i + 1..] {
                assert_ne!(a.note, b.note);
                assert_ne!(a.name, b.name);
            }
        }
        assert_eq!(pad(DEFAULT_PAD).unwrap().name, "Kick");
        assert!(pad(60).is_none());
    }

    #[test]
    fn a_parameter_without_a_group_shows_in_the_kit() {
        assert!(is_kit_param("drive"));
        assert!(is_kit_param("level"));
        assert!(is_kit_param("a_future_param"));
        assert!(!is_kit_param("kick_tune"));
        assert!(!is_kit_param("hat_tone"));
        assert_eq!(owner("hat_tone").unwrap().name, "Hats");
    }
}
