//! Ondina studio layer: session/command execution, MCP server and the AI assistant.

pub mod ai;
pub mod local_ai;
pub mod mcp;
pub mod prompts;
pub mod session;
pub mod session_ext;
pub mod settings;

pub use session::{Session, SharedSession, Source};
pub use settings::Settings;
