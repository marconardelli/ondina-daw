//! Persistent user settings (%APPDATA%\OndinaDAW\settings.json).

use std::path::PathBuf;

use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(default)]
pub struct Settings {
    /// Anthropic API key (the ANTHROPIC_API_KEY environment variable takes precedence).
    pub api_key: Option<String>,
    pub model: String,
    /// low | medium | high | xhigh | max
    pub effort: String,
    pub mcp_enabled: bool,
    pub mcp_port: u16,
    pub audio_output: Option<String>,
    pub recent_projects: Vec<String>,
    pub ui_scale: f32,
    /// "anthropic" (Claude API) or "local" (offline OpenAI-compatible model, e.g. bundled llama.cpp).
    pub provider: String,
    pub local_url: String,
    pub local_model: String,
}

impl Default for Settings {
    fn default() -> Self {
        Settings { api_key: None, model: crate::ai::DEFAULT_MODEL.into(), effort: "medium".into(), mcp_enabled: true, mcp_port: 3917, audio_output: None, recent_projects: vec![], ui_scale: 1.0, provider: "auto".into(), local_url: "http://127.0.0.1:8080/v1".into(), local_model: "local".into() }
    }
}

impl Settings {
    pub fn path() -> PathBuf {
        let base = std::env::var_os("APPDATA").map(PathBuf::from).unwrap_or_else(std::env::temp_dir);
        base.join("OndinaDAW").join("settings.json")
    }

    pub fn load() -> Settings {
        // Before the rename the settings lived in %APPDATA%\AuroraDAW: they are read from there until the first save writes the new file.
        let legacy = Self::path().parent().and_then(|d| d.parent()).map(|b| b.join("AuroraDAW").join("settings.json"));
        let path = Some(Self::path()).filter(|p| p.exists()).or(legacy).unwrap_or_else(Self::path);
        std::fs::read_to_string(path).ok().and_then(|s| serde_json::from_str(&s).ok()).unwrap_or_default()
    }

    pub fn save(&self) {
        let p = Self::path();
        if let Some(d) = p.parent() {
            let _ = std::fs::create_dir_all(d);
        }
        let _ = std::fs::write(p, serde_json::to_string_pretty(self).unwrap_or_default());
    }

    pub fn api_key(&self) -> Option<String> {
        std::env::var("ANTHROPIC_API_KEY").ok().filter(|k| !k.trim().is_empty()).or_else(|| self.api_key.clone().filter(|k| !k.trim().is_empty()))
    }

    pub fn model(&self) -> String {
        if self.model.trim().is_empty() { crate::ai::DEFAULT_MODEL.into() } else { self.model.clone() }
    }

    /// Effective provider: "auto" uses Claude when a key is configured, otherwise the local model.
    pub fn use_local(&self) -> bool {
        match self.provider.as_str() {
            "local" => true,
            "anthropic" => false,
            _ => self.api_key().is_none(),
        }
    }

    pub fn effort(&self) -> String {
        match self.effort.as_str() {
            "low" | "medium" | "high" | "xhigh" | "max" => self.effort.clone(),
            _ => "medium".into(),
        }
    }

    pub fn add_recent(&mut self, path: &str) {
        self.recent_projects.retain(|p| p != path);
        self.recent_projects.insert(0, path.to_string());
        self.recent_projects.truncate(10);
    }
}
