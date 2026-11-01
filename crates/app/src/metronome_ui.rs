//! The metronome button of the transport bar: its look, its tooltip and the commands of its menu.

use ondina_core::command::Command;
use ondina_core::model::Project;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Look {
    Off,
    /// Clicking now (or in every playback).
    On,
    /// On, but set to click only while recording and not recording now.
    Waiting,
}

pub fn look(p: &Project, recording: bool) -> Look {
    if !p.metronome {
        Look::Off
    } else if p.metronome_only_recording && !recording {
        Look::Waiting
    } else {
        Look::On
    }
}

pub fn tooltip(p: &Project) -> String {
    let mode = if p.metronome_only_recording { " · only while recording" } else { "" };
    format!("Metronome (K) · {:.1} dB{mode} · right-click for level and options", p.metronome_level_db)
}

/// K and a left click: toggle, keeping level and mode.
pub fn toggle(p: &Project) -> Command {
    Command::SetMetronome { enabled: !p.metronome, level_db: None, only_while_recording: None }
}

pub fn set_level(p: &Project, db: f32) -> Command {
    Command::SetMetronome { enabled: p.metronome, level_db: Some(db), only_while_recording: None }
}

pub fn set_only_recording(p: &Project, on: bool) -> Command {
    Command::SetMetronome { enabled: p.metronome, level_db: None, only_while_recording: Some(on) }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn look_shows_when_the_click_waits_for_a_recording() {
        let mut p = Project::new("t");
        assert_eq!(look(&p, false), Look::Off);
        assert_eq!(look(&p, true), Look::Off);
        p.metronome = true;
        assert_eq!(look(&p, false), Look::On);
        assert_eq!(look(&p, true), Look::On);
        p.metronome_only_recording = true;
        assert_eq!(look(&p, false), Look::Waiting);
        assert_eq!(look(&p, true), Look::On);
        p.metronome = false;
        assert_eq!(look(&p, false), Look::Off);
        assert_eq!(look(&p, true), Look::Off);
    }

    #[test]
    fn menu_commands_keep_the_other_settings() {
        let mut p = Project::new("t");
        assert!(matches!(toggle(&p), Command::SetMetronome { enabled: true, level_db: None, only_while_recording: None }));
        p.metronome = true;
        assert!(matches!(toggle(&p), Command::SetMetronome { enabled: false, level_db: None, only_while_recording: None }));
        assert!(matches!(
            set_level(&p, -12.0),
            Command::SetMetronome { enabled: true, level_db: Some(db), only_while_recording: None } if db == -12.0
        ));
        assert!(matches!(
            set_only_recording(&p, true),
            Command::SetMetronome { enabled: true, level_db: None, only_while_recording: Some(true) }
        ));
        p.metronome = false;
        assert!(matches!(set_level(&p, -3.0), Command::SetMetronome { enabled: false, level_db: Some(_), only_while_recording: None }));
        assert!(matches!(
            set_only_recording(&p, false),
            Command::SetMetronome { enabled: false, level_db: None, only_while_recording: Some(false) }
        ));
    }

    #[test]
    fn tooltip_names_level_and_mode() {
        let mut p = Project::new("t");
        let t = tooltip(&p);
        assert!(t.contains("(K)"), "{t}");
        assert!(t.contains("-9.1 dB"), "{t}");
        assert!(!t.contains("only while recording"), "{t}");
        p.metronome_only_recording = true;
        assert!(tooltip(&p).contains("only while recording"));
    }
}
