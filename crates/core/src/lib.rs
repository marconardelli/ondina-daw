//! Aurora DAW core: project model, the command API, music theory and generators.

pub mod command;
pub mod generate;
pub mod midi_io;
pub mod model;
pub mod ops;
pub mod ops2;
pub mod plugins;
pub mod tempo;
pub mod theory;

pub use command::{tool_specs, Command, ToolSpec};
pub use model::*;

pub const APP_NAME: &str = "Aurora DAW";
pub const PROJECT_EXTENSION: &str = "aurora";

/// Serialize a project to the JSON project file format.
pub fn save_project_string(p: &Project) -> Result<String, String> {
    let doc = serde_json::json!({ "format": "aurora-project", "version": 1, "project": p });
    serde_json::to_string_pretty(&doc).map_err(|e| e.to_string())
}

/// Parse a project file.
pub fn load_project_str(s: &str) -> Result<Project, String> {
    let v: serde_json::Value = serde_json::from_str(s).map_err(|e| format!("invalid project file: {e}"))?;
    let p = v.get("project").cloned().unwrap_or(v);
    serde_json::from_value(p).map_err(|e| format!("invalid project file: {e}"))
}
