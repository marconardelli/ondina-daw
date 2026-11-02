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

/// Quick requests of the Dynamic EQ panel, as (button label, request). analyze_mix measures only low, mid and high, so
/// the requests say where to look and leave the exact frequency to the ear.
pub const DYNAMIC_EQ_ACTIONS: &[(&str, &str)] = &[
    (
        "Clean up the muddy frequencies",
        "Clean up the muddy frequencies of this track with this Dynamic EQ: use a Bell band between 200 and 500 Hz with a negative Range (-4 to -8 dB) and a Threshold just under the loud passages, so it cuts only when the mud builds up. analyze_mix measures only low, mid and high, so choose the exact frequency by ear. Change the bands with set_plugin_params, then explain in a few lines which bands you changed and why.",
    ),
    (
        "Tame harsh resonances",
        "Tame the harsh resonances of this track with this Dynamic EQ: use one or two narrow Bell bands (Q 3 to 6) between 2 and 5 kHz with a negative Range (-4 to -10 dB), a fast Attack and a Threshold just under the peaks, so they cut only while a resonance rings. analyze_mix measures only low, mid and high, so choose the exact frequency by ear. Change the bands with set_plugin_params, then explain in a few lines which bands you changed and why.",
    ),
    (
        "More air on the vocal",
        "Give this vocal more air with this Dynamic EQ: use a High Shelf band from 10 to 12 kHz with 2 to 4 dB of static Gain and a negative Range with the Threshold under the loudest S sounds, so the boost backs off when they get harsh. analyze_mix measures only low, mid and high, so choose the exact frequency by ear. Change the bands with set_plugin_params, then explain in a few lines which bands you changed and why.",
    ),
];

/// Plug-in editor: "AI: design a sound" for the plug-in `plugin_id` called `name`.
pub fn design_sound(plugin_id: Id, name: &str) -> String {
    format!("Adjust the parameters of plug-in {plugin_id} ({name}) to get a great sound for this track's role in the song. Explain briefly.")
}
