//! Requests sent by the contextual AI buttons of the GUI. They live here, next to the assistants, so a test can
//! check that the local model is offered every tool these requests need.

use ondina_core::model::Id;

/// Piano roll: "AI: continue / improve".
pub const CONTINUE_IMPROVE: &str =
    "Improve or complete the selected MIDI region musically (keep its character). Edit the notes directly.";

/// Step sequencer: "AI: write a groove".
pub const WRITE_GROOVE: &str =
    "Program a tight groove in the selected step-sequencer pattern using set_pattern_row (fit the project's style).";

/// Clip Grid: "AI: fill the grid".
pub const FILL_GRID: &str = "Fill the Clip Grid: for each track create cells in 4 scenes (intro, groove, build, drop) with musical variations, using create_live_cell with notes.";

/// Mix report: "Ask the AI to fix the mix".
pub const FIX_MIX: &str = "Analyze the mix and fix the problems you find, then verify with analyze_mix.";

/// Track menu of the arrange window: "Ask AI to write a part for this track".
pub const WRITE_PART: &str = "Write a fitting 8-bar part for the selected track that works with the rest of the project.";

/// Region menu of the arrange window: "Ask AI to vary this region".
pub const VARY_REGION: &str = "Create a musical variation of the selected region on the same track right after it (keep the style, change rhythm or melody slightly).";

/// Plug-in editor: "AI: design a sound" for the plug-in `plugin_id` called `name`.
pub fn design_sound(plugin_id: Id, name: &str) -> String {
    format!("Adjust the parameters of plug-in {plugin_id} ({name}) to get a great sound for this track's role in the song. Explain briefly.")
}
