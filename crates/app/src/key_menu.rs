//! The "Detect from MIDI" entry of the key menu in the transport bar.

use ondina_core::command::Command;
use serde_json::Value;

pub const DETECT_LABEL: &str = "Detect from MIDI";
pub const DETECT_TOOLTIP: &str =
    "Estimate the key from the MIDI parts (Drum Kit tracks and step patterns are skipped) and set it as the project key";

/// The command of the entry: the whole project, and the result becomes the project key.
pub fn detect_command() -> Command {
    Command::DetectKey { region_id: None, apply: Some(true) }
}

/// What the status bar says after the entry ran, from the result of detect_key.
pub fn status(v: &Value) -> String {
    let key = v["key"].as_str().unwrap_or("?");
    let conf = v["confidence"].as_f64().unwrap_or(0.0);
    let mut s = format!("Key set to {key} (confidence {conf:.2})");
    let close: Vec<&str> = v["alternatives"].as_array().map(|a| a.iter().filter_map(|k| k["key"].as_str()).collect()).unwrap_or_default();
    if !close.is_empty() {
        s.push_str(&format!(" · close: {}", close.join(", ")));
    }
    if v["low_confidence"].as_bool().unwrap_or(false) {
        s.push_str(" · few notes or an unclear key, check by ear");
    }
    s
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn detect_command_reads_the_whole_project_and_applies() {
        let c = detect_command();
        assert!(matches!(c, Command::DetectKey { region_id: None, apply: Some(true) }));
        assert!(c.is_mutating());
    }

    #[test]
    fn status_names_the_key_the_confidence_and_the_close_keys() {
        let v = json!({"key": "A Minor", "confidence": 0.73, "low_confidence": false, "alternatives": [
            {"key": "A Major", "confidence": 0.47},
            {"key": "E Major", "confidence": 0.39},
            {"key": "D Major", "confidence": 0.26},
        ]});
        assert_eq!(status(&v), "Key set to A Minor (confidence 0.73) · close: A Major, E Major, D Major");
    }

    #[test]
    fn status_warns_when_the_estimate_is_weak() {
        let v = json!({"key": "C Major", "confidence": 0.68, "low_confidence": true, "alternatives": [
            {"key": "C Minor", "confidence": 0.68},
        ]});
        assert!(status(&v).ends_with(" · few notes or an unclear key, check by ear"), "{}", status(&v));
        assert_eq!(status(&json!({"key": "C Major", "confidence": 0.9})), "Key set to C Major (confidence 0.90)");
        assert_eq!(status(&json!({})), "Key set to ? (confidence 0.00)");
    }
}
