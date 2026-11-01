//! Persistent user settings (%APPDATA%\OndinaDAW\settings.json).

use std::path::{Path, PathBuf};

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
    /// Folders the user added in the VST3 library. MCP clients and the assistant may load plug-ins only
    /// from these and the standard VST3 folders; they are never set through a tool.
    #[serde(default)]
    pub vst3_folders: Vec<String>,
    /// Minutes between recovery copies of unsaved work (0 turns autosave off).
    pub autosave_minutes: u32,
    /// Before Save replaces a project file, its previous version is kept next to it as `<file>.bak`
    /// (one copy, replaced at every save).
    pub keep_save_backup: bool,
    /// Sizes of the main window's panels as the person left them (points, before the UI scale). `None` until a
    /// panel is resized, so the window picks a size that fits the screen.
    pub panels: PanelSizes,
    /// True when settings.json exists but could not be read (or could not be set aside after failing to parse):
    /// these are the defaults, and `save_to`/`try_save`/`save` leave the file alone until the person presses
    /// Save settings in the Settings window (`save_replacing`).
    #[serde(skip)]
    pub file_unread: bool,
}

/// Width of the side panels and height of the bottom panel in the main window.
#[derive(Debug, Clone, Copy, Default, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct PanelSizes {
    /// Inspector / Library on the left.
    pub left: Option<f32>,
    /// Assistant on the right.
    pub assistant: Option<f32>,
    /// Editor, Mixer or Clip Grid at the bottom (one height for the three).
    pub bottom: Option<f32>,
}

impl Default for Settings {
    fn default() -> Self {
        Settings { api_key: None, model: crate::ai::DEFAULT_MODEL.into(), effort: "medium".into(), mcp_enabled: true, mcp_port: 3917, audio_output: None, recent_projects: vec![], ui_scale: 1.0, provider: "auto".into(), local_url: "http://127.0.0.1:8080/v1".into(), local_model: "local".into(), vst3_folders: vec![], autosave_minutes: 3, keep_save_backup: true, panels: PanelSizes::default(), file_unread: false }
    }
}

impl Settings {
    pub fn path() -> PathBuf {
        let base = std::env::var_os("APPDATA").map(PathBuf::from).unwrap_or_else(std::env::temp_dir);
        base.join("OndinaDAW").join("settings.json")
    }

    /// Folder of the recovery copies written by autosave (next to settings.json).
    pub fn autosave_dir() -> PathBuf {
        Self::path().parent().map(|d| d.join("Autosave")).unwrap_or_else(|| std::env::temp_dir().join("OndinaDAW").join("Autosave"))
    }

    /// Reads the settings at `path`. A missing file gives the defaults silently. A file that exists but does not
    /// parse is renamed to `<name>.broken`, so the next save does not write over it, and the second value says so.
    /// A file that cannot be read (after a few tries) or set aside gives the defaults with `file_unread` set, so
    /// they are not written over it.
    pub fn load_from(path: &Path) -> (Settings, Option<String>) {
        let name = path.file_name().map(|n| n.to_string_lossy().into_owned()).unwrap_or_else(|| path.display().to_string());
        let unread = || Settings { file_unread: true, ..Default::default() };
        let bytes = match crate::fs_safe::read_retrying(path) {
            Ok(b) => b,
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => return (Settings::default(), None),
            Err(e) => return (unread(), Some(format!("⚠ Settings could not be read ({e}): default settings are in use, and {name} is left as it is until you press Save settings in Settings"))),
        };
        if let Ok(s) = serde_json::from_slice::<Settings>(&bytes) {
            return (s, None);
        }
        let broken = crate::fs_safe::sibling(path, ".broken");
        match std::fs::rename(path, &broken) {
            Ok(()) => (Settings::default(), Some(format!("⚠ Settings could not be read: the file was kept as {} and default settings are in use", broken.display()))),
            Err(e) => (unread(), Some(format!("⚠ Settings could not be read and could not be set aside ({e}): default settings are in use, and {name} is left as it is until you press Save settings in Settings"))),
        }
    }

    /// The settings and, when the file could not be used, the line for the status bar.
    pub fn load_reporting() -> (Settings, Option<String>) {
        // Before the rename the settings lived in %APPDATA%\AuroraDAW: they are read from there until the first save writes the new file.
        let legacy = Self::path().parent().and_then(|d| d.parent()).map(|b| b.join("AuroraDAW").join("settings.json"));
        let path = Some(Self::path()).filter(|p| p.exists()).or(legacy).unwrap_or_else(Self::path);
        Self::load_from(&path)
    }

    pub fn load() -> Settings {
        Self::load_reporting().0
    }

    /// Writes the settings to `path` through `fs_safe::write_replace`: a failure leaves the previous file as it was.
    /// Refused while `file_unread` is set, so defaults never replace settings that could not be read.
    pub fn save_to(&self, path: &Path) -> Result<(), String> {
        if self.file_unread {
            return Err(format!("{} could not be read when Ondina started and is left as it is: press Save settings in Settings to replace it", path.display()));
        }
        self.write_to(path)
    }

    /// The Settings window's Save: writes even when the file could not be read at startup, because the person chose
    /// to replace it with what the window shows. The flag is cleared only when the write succeeds.
    pub fn save_replacing_to(&mut self, path: &Path) -> Result<(), String> {
        self.write_to(path)?;
        self.file_unread = false;
        Ok(())
    }

    /// `save_replacing_to` on the usual file.
    pub fn save_replacing(&mut self) -> Result<(), String> {
        self.save_replacing_to(&Self::path())
    }

    fn write_to(&self, path: &Path) -> Result<(), String> {
        if let Some(d) = path.parent() {
            std::fs::create_dir_all(d).map_err(|e| format!("cannot create {}: {e}", d.display()))?;
        }
        let text = serde_json::to_string_pretty(self).map_err(|e| e.to_string())?;
        crate::fs_safe::write_replace(path, text.as_bytes())
    }

    /// `save_to` on the usual file.
    pub fn try_save(&self) -> Result<(), String> {
        self.save_to(&Self::path())
    }

    pub fn save(&self) {
        let _ = self.try_save();
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

    /// Add a VST3 folder (already checked and resolved by the caller). False when it is already in the
    /// list, compared without regard to case as Windows does.
    pub fn add_vst3_folder(&mut self, folder: &str) -> bool {
        let key = folder.to_lowercase();
        if self.vst3_folders.iter().any(|f| f.to_lowercase() == key) {
            return false;
        }
        self.vst3_folders.push(folder.to_string());
        true
    }

    /// Remove a VST3 folder, compared without regard to case.
    pub fn remove_vst3_folder(&mut self, folder: &str) {
        let key = folder.to_lowercase();
        self.vst3_folders.retain(|f| f.to_lowercase() != key);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn settings_without_vst3_folders_load_with_an_empty_list() {
        let s: Settings = serde_json::from_str(r#"{"model":"x","mcp_port":4000}"#).unwrap();
        assert!(s.vst3_folders.is_empty());
        assert_eq!(s.mcp_port, 4000);
        let mut s = s;
        s.vst3_folders.push("C:\\Plugins".into());
        let back: Settings = serde_json::from_str(&serde_json::to_string(&s).unwrap()).unwrap();
        assert_eq!(back.vst3_folders, vec!["C:\\Plugins".to_string()]);
    }

    #[test]
    fn settings_without_panel_sizes_load_with_none_and_keep_them_on_save() {
        let s: Settings = serde_json::from_str(r#"{"model":"x"}"#).unwrap();
        assert_eq!(s.panels, PanelSizes::default());
        let mut s = s;
        s.panels = PanelSizes { left: Some(250.0), assistant: None, bottom: Some(320.5) };
        let back: Settings = serde_json::from_str(&serde_json::to_string(&s).unwrap()).unwrap();
        assert_eq!(back.panels, s.panels);
        let partial: Settings = serde_json::from_str(r#"{"panels":{"left":300}}"#).unwrap();
        assert_eq!(partial.panels, PanelSizes { left: Some(300.0), assistant: None, bottom: None });
    }

    #[test]
    fn settings_without_autosave_minutes_default_to_three() {
        let s: Settings = serde_json::from_str(r#"{"model":"x"}"#).unwrap();
        assert_eq!(s.autosave_minutes, 3);
        let off: Settings = serde_json::from_str(r#"{"autosave_minutes":0}"#).unwrap();
        assert_eq!(off.autosave_minutes, 0);
        let mut s = s;
        s.autosave_minutes = 7;
        let back: Settings = serde_json::from_str(&serde_json::to_string(&s).unwrap()).unwrap();
        assert_eq!(back.autosave_minutes, 7);
    }

    #[test]
    fn autosave_dir_is_next_to_the_settings_file() {
        let dir = Settings::autosave_dir();
        assert_eq!(dir.parent(), Settings::path().parent());
        assert_eq!(dir.file_name().unwrap(), "Autosave");
    }

    #[test]
    fn add_vst3_folder_skips_duplicates_ignoring_case() {
        let mut s = Settings::default();
        assert!(s.add_vst3_folder("C:\\Plugins"));
        assert!(!s.add_vst3_folder("c:\\plugins"));
        assert_eq!(s.vst3_folders.len(), 1);
    }

    #[test]
    fn remove_vst3_folder_ignores_case() {
        let mut s = Settings::default();
        s.add_vst3_folder("C:\\Plugins");
        s.remove_vst3_folder("c:\\PLUGINS");
        assert!(s.vst3_folders.is_empty());
    }

    fn test_dir(name: &str) -> PathBuf {
        let d = std::env::temp_dir().join("ondina_settings_tests").join(name);
        let _ = std::fs::remove_dir_all(&d);
        std::fs::create_dir_all(&d).unwrap();
        d
    }

    fn names(dir: &Path) -> Vec<String> {
        let mut v: Vec<String> = std::fs::read_dir(dir).unwrap().map(|e| e.unwrap().file_name().to_string_lossy().into_owned()).collect();
        v.sort();
        v
    }

    #[test]
    fn settings_without_keep_save_backup_default_to_on() {
        let s: Settings = serde_json::from_str(r#"{"model":"x"}"#).unwrap();
        assert!(s.keep_save_backup);
        let off: Settings = serde_json::from_str(r#"{"keep_save_backup":false}"#).unwrap();
        assert!(!off.keep_save_backup);
        let back: Settings = serde_json::from_str(&serde_json::to_string(&off).unwrap()).unwrap();
        assert!(!back.keep_save_backup);
    }

    #[test]
    fn save_to_writes_a_file_load_from_reads_back() {
        let dir = test_dir("round_trip");
        let p = dir.join("settings.json");
        let s = Settings { mcp_port: 4100, vst3_folders: vec!["C:\\Plugins".into()], ..Default::default() };
        s.save_to(&p).unwrap();
        let (back, notice) = Settings::load_from(&p);
        assert_eq!(notice, None);
        assert_eq!(back.mcp_port, 4100);
        assert_eq!(back.vst3_folders, vec!["C:\\Plugins".to_string()]);
        assert_eq!(names(&dir), vec!["settings.json"]);
    }

    #[test]
    fn missing_settings_file_gives_defaults_without_a_notice() {
        let dir = test_dir("missing");
        let (s, notice) = Settings::load_from(&dir.join("settings.json"));
        assert_eq!(s.mcp_port, 3917);
        assert_eq!(notice, None);
    }

    #[test]
    fn unreadable_settings_are_kept_as_broken() {
        let dir = test_dir("broken");
        let p = dir.join("settings.json");
        let broken = dir.join("settings.json.broken");
        let truncated = r#"{"api_key": "sk-ant-"#;
        std::fs::write(&p, truncated).unwrap();
        let (s, notice) = Settings::load_from(&p);
        assert_eq!(s.mcp_port, 3917);
        let n = notice.expect("a notice for the status bar");
        assert!(n.contains("settings.json.broken"), "{n}");
        assert!(!p.exists());
        assert_eq!(std::fs::read_to_string(&broken).unwrap(), truncated);
        Settings::default().save_to(&p).unwrap();
        assert_eq!(std::fs::read_to_string(&broken).unwrap(), truncated);
        let (_, again) = Settings::load_from(&p);
        assert_eq!(again, None);
    }

    #[test]
    fn empty_settings_file_is_kept_as_broken_too() {
        let dir = test_dir("empty");
        let p = dir.join("settings.json");
        std::fs::write(&p, b"").unwrap();
        let (s, notice) = Settings::load_from(&p);
        assert_eq!(s.mcp_port, 3917);
        assert!(notice.is_some());
        assert!(!p.exists());
        assert!(dir.join("settings.json.broken").is_file());
    }

    #[cfg(windows)]
    #[test]
    fn settings_held_by_another_program_are_not_saved_over() {
        use std::os::windows::fs::OpenOptionsExt;
        let dir = test_dir("held");
        let p = dir.join("settings.json");
        Settings { mcp_port: 4200, api_key: Some("sk-keep".into()), ..Default::default() }.save_to(&p).unwrap();
        let before = std::fs::read(&p).unwrap();
        let handle = std::fs::OpenOptions::new().read(true).share_mode(0).open(&p).unwrap();
        let (s, notice) = Settings::load_from(&p);
        drop(handle);
        assert!(s.file_unread);
        assert_eq!(s.mcp_port, 3917);
        let n = notice.expect("a notice for the status bar");
        assert!(n.contains("Save settings"), "{n}");
        assert!(s.save_to(&p).is_err());
        assert_eq!(std::fs::read(&p).unwrap(), before);
        assert_eq!(names(&dir), vec!["settings.json"]);
        let mut s = s;
        s.save_replacing_to(&p).unwrap();
        assert!(!s.file_unread);
        let (back, again) = Settings::load_from(&p);
        assert_eq!(back.mcp_port, 3917);
        assert_eq!(again, None);
    }

    #[cfg(windows)]
    #[test]
    fn settings_released_quickly_are_read_on_a_retry() {
        use std::os::windows::fs::OpenOptionsExt;
        let dir = test_dir("released");
        let p = dir.join("settings.json");
        Settings { mcp_port: 4300, ..Default::default() }.save_to(&p).unwrap();
        let (held_tx, held_rx) = std::sync::mpsc::channel();
        let locked = p.clone();
        let holder = std::thread::spawn(move || {
            let handle = std::fs::OpenOptions::new().read(true).share_mode(0).open(&locked).unwrap();
            held_tx.send(()).unwrap();
            std::thread::sleep(std::time::Duration::from_millis(60));
            drop(handle);
        });
        held_rx.recv().unwrap();
        let (s, notice) = Settings::load_from(&p);
        holder.join().unwrap();
        assert_eq!(notice, None);
        assert_eq!(s.mcp_port, 4300);
        assert!(!s.file_unread);
    }

    #[cfg(windows)]
    #[test]
    fn unparsable_settings_that_cannot_be_set_aside_are_not_saved_over() {
        use std::os::windows::fs::OpenOptionsExt;
        let dir = test_dir("cannot_set_aside");
        let p = dir.join("settings.json");
        let truncated = r#"{"api_key": "sk-ant-"#;
        std::fs::write(&p, truncated).unwrap();
        let handle = std::fs::OpenOptions::new().read(true).share_mode(1).open(&p).unwrap();
        let (s, notice) = Settings::load_from(&p);
        let n = notice.expect("a notice for the status bar");
        assert!(n.contains("could not be set aside"), "{n}");
        assert!(n.contains("Save settings"), "{n}");
        assert!(s.file_unread);
        assert!(s.save_to(&p).is_err());
        drop(handle);
        assert!(s.save_to(&p).is_err());
        assert_eq!(std::fs::read_to_string(&p).unwrap(), truncated);
        assert!(!dir.join("settings.json.broken").exists());
    }

    #[test]
    fn fresh_settings_save_as_before() {
        let dir = test_dir("fresh");
        let p = dir.join("settings.json");
        let s = Settings::default();
        assert!(!s.file_unread);
        s.save_to(&p).unwrap();
        assert!(p.is_file());
    }
}
