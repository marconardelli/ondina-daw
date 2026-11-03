//! Ondina DAW core: project model, the command API, music theory and generators.

pub mod command;
pub mod drum_kit;
pub mod generate;
pub mod midi_io;
pub mod model;
pub mod note_value;
pub mod ops;
pub mod ops2;
pub mod plugins;
pub mod tempo;
pub mod theory;

pub use command::{tool_specs, Command, ToolSpec};
pub use model::*;

pub const APP_NAME: &str = "Ondina DAW";
pub const PROJECT_EXTENSION: &str = "ondina";
/// Extension of projects saved before the rename to Ondina; they still open.
pub const LEGACY_PROJECT_EXTENSION: &str = "aurora";

/// True for a project file name (`.ondina`, or the older `.aurora`).
pub fn is_project_file(name: &str) -> bool {
    let lower = name.to_ascii_lowercase();
    lower.ends_with(&format!(".{PROJECT_EXTENSION}")) || lower.ends_with(&format!(".{LEGACY_PROJECT_EXTENSION}"))
}

/// Read `ONDINA_<suffix>`, falling back to the pre-rename `AURORA_<suffix>`.
pub fn env_var(suffix: &str) -> Option<String> {
    std::env::var(format!("ONDINA_{suffix}")).or_else(|_| std::env::var(format!("AURORA_{suffix}"))).ok()
}

/// Serialize a project to the JSON project file format.
pub fn save_project_string(p: &Project) -> Result<String, String> {
    let doc = serde_json::json!({ "format": "ondina-project", "version": 1, "project": p });
    serde_json::to_string_pretty(&doc).map_err(|e| e.to_string())
}

/// Parse a project file. The file is checked like a command's input before anything plays it: a time signature,
/// tempo map or pattern the commands would refuse is refused here too (see `ops2::check_loaded_project`), and
/// values with an obvious safe form (a tempo out of range, a cycle of no length, an automation value out of its
/// range) are put back in range.
pub fn load_project_str(s: &str) -> Result<Project, String> {
    let v: serde_json::Value = serde_json::from_str(s).map_err(|e| format!("invalid project file: {e}"))?;
    let p = v.get("project").cloned().unwrap_or(v);
    let mut project: Project = serde_json::from_value(p).map_err(|e| format!("invalid project file: {e}"))?;
    ops2::check_loaded_project(&mut project).map_err(|e| format!("invalid project file: {e}"))?;
    Ok(project)
}
