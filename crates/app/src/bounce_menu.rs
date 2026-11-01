//! The bounce options of the File menu: how loud the bounced file is, and the bounce command they build.

use ondina_core::command::Command;
use serde_json::Value;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Loudness {
    /// The mix as it is.
    Off,
    /// Peak at -1 dBFS (the `normalize` of the bounce).
    Peak,
    Lufs14,
    Lufs16,
    Lufs23,
}

impl Loudness {
    pub const ALL: [Loudness; 5] = [Loudness::Off, Loudness::Peak, Loudness::Lufs14, Loudness::Lufs16, Loudness::Lufs23];

    pub fn label(self) -> &'static str {
        match self {
            Loudness::Off => "Off",
            Loudness::Peak => "Peak -1 dB",
            Loudness::Lufs14 => "-14 LUFS (streaming)",
            Loudness::Lufs16 => "-16 LUFS",
            Loudness::Lufs23 => "-23 LUFS (broadcast)",
        }
    }

    fn target(self) -> Option<f32> {
        match self {
            Loudness::Lufs14 => Some(-14.0),
            Loudness::Lufs16 => Some(-16.0),
            Loudness::Lufs23 => Some(-23.0),
            Loudness::Off | Loudness::Peak => None,
        }
    }
}

/// The bounce of File > Bounce to WAV and Ctrl+B: the cycle range when the cycle is on, the chosen bit depth and loudness.
pub fn command(path: String, cycle: Option<(f64, f64)>, bit_depth: u16, loudness: Loudness) -> Command {
    Command::Bounce {
        path,
        start: cycle.map(|c| c.0),
        end: cycle.map(|c| c.1),
        bit_depth: Some(bit_depth),
        normalize: Some(loudness == Loudness::Peak),
        track_ids: None,
        tail_sec: None,
        normalize_lufs: loudness.target(),
        true_peak_ceiling_db: None,
    }
}

/// The status bar line after a bounce: length, file, and what the loudness choice did.
pub fn status(result: &Value, path: &str, loudness: Loudness) -> String {
    let mut s = format!("Bounced {:.1}s to {path}", result["duration_sec"].as_f64().unwrap_or(0.0));
    if loudness == Loudness::Peak {
        s.push_str(" · peak at -1 dBFS");
    }
    let l = &result["loudness"];
    if l.is_object() {
        let num = |k: &str| l[k].as_f64().unwrap_or(0.0);
        if l["target_reached"].as_bool().unwrap_or(false) {
            s.push_str(&format!(" · {:.1} LUFS (gain {:+.1} dB)", num("lufs_after"), num("gain_db")));
        } else {
            s.push_str(&format!(
                " · {:.1} LUFS, short of {:.0} LUFS: the {:.0} dBTP peak ceiling held the gain at {:+.1} dB (Mastering Assistant adds a limiter)",
                num("lufs_after"),
                num("target_lufs"),
                num("true_peak_ceiling_db"),
                num("gain_db")
            ));
        }
    }
    for w in result["warnings"].as_array().into_iter().flatten() {
        if let Some(w) = w.as_str() {
            s.push_str(&format!(" · ⚠ {w}"));
        }
    }
    s
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn each_loudness_choice_builds_its_bounce() {
        let expected = [
            (Loudness::Off, false, None),
            (Loudness::Peak, true, None),
            (Loudness::Lufs14, false, Some(-14.0)),
            (Loudness::Lufs16, false, Some(-16.0)),
            (Loudness::Lufs23, false, Some(-23.0)),
        ];
        for (choice, peak, lufs) in expected {
            match command("x.wav".into(), None, 24, choice) {
                Command::Bounce { path, start, end, bit_depth, normalize, track_ids, tail_sec, normalize_lufs, true_peak_ceiling_db } => {
                    assert_eq!(path, "x.wav");
                    assert_eq!((start, end), (None, None));
                    assert_eq!(bit_depth, Some(24));
                    assert_eq!(normalize, Some(peak), "{choice:?}");
                    assert_eq!(normalize_lufs, lufs, "{choice:?}");
                    assert_eq!(true_peak_ceiling_db, None);
                    assert_eq!(track_ids, None);
                    assert_eq!(tail_sec, None);
                }
                other => panic!("{other:?}"),
            }
        }
        match command("x.wav".into(), Some((8.0, 24.0)), 16, Loudness::Lufs14) {
            Command::Bounce { start, end, bit_depth, .. } => {
                assert_eq!((start, end), (Some(8.0), Some(24.0)));
                assert_eq!(bit_depth, Some(16));
            }
            other => panic!("{other:?}"),
        }
    }

    #[test]
    fn labels_are_distinct() {
        assert_eq!(Loudness::ALL[0], Loudness::Off);
        let labels: std::collections::BTreeSet<&str> = Loudness::ALL.iter().map(|l| l.label()).collect();
        assert_eq!(labels.len(), Loudness::ALL.len());
    }

    #[test]
    fn status_says_what_the_loudness_did() {
        let plain = json!({"duration_sec": 12.34});
        assert_eq!(status(&plain, "a.wav", Loudness::Off), "Bounced 12.3s to a.wav");
        assert!(status(&plain, "a.wav", Loudness::Peak).contains("-1 dBFS"));
        let reached = json!({"duration_sec": 12.34, "loudness": {"target_lufs": -14.0, "lufs_after": -14.0, "gain_db": -4.1, "true_peak_ceiling_db": -1.0, "target_reached": true}});
        let s = status(&reached, "a.wav", Loudness::Lufs14);
        assert!(s.contains("-14.0 LUFS") && s.contains("-4.1 dB"), "{s}");
        let short = json!({"duration_sec": 12.34, "loudness": {"target_lufs": -6, "lufs_after": -13.3, "gain_db": 16.6, "true_peak_ceiling_db": -1, "target_reached": false}});
        let s = status(&short, "a.wav", Loudness::Lufs14);
        assert!(s.contains("short of -6 LUFS") && s.contains("Mastering Assistant") && s.contains("+16.6 dB"), "{s}");
        let silent = json!({"duration_sec": 1.0, "warnings": ["the mix is silent: normalize_lufs left it as it is"]});
        assert!(status(&silent, "a.wav", Loudness::Lufs14).contains("silent"));
    }
}
