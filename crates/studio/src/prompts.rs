//! Requests sent by the contextual AI buttons of the GUI. They live here, next to the assistants, so a test can
//! check that the local model is offered every tool these requests need.

use std::fmt::Write;

use ondina_core::model::{Id, PluginInstance, PluginSlot, Project, TrackKind};
use ondina_core::plugins;
use ondina_dsp::effects::SaturationCurve;

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

/// Quick requests of the Stereo Imager panel, as (button label, request). Each one reads the stereo correlation with
/// analyze_mix before and after, so the answer can say what changed in numbers.
pub const STEREO_IMAGER_ACTIONS: &[(&str, &str)] = &[
    (
        "Mono the low end",
        "Make the low end mono with this Stereo Imager: first read the stereo correlation with analyze_mix, then turn Mono Bass on and set Low Freq between 100 and 200 Hz (about 120 Hz on a full mix, up to 200 Hz on a wide pad or synth), leaving the other widths as they are. Change the parameters with set_plugin_params, measure again with analyze_mix, then report the stereo correlation before and after and which parameters you changed.",
    ),
    (
        "Wider without losing mono",
        "Make this sound wider without losing mono compatibility with this Stereo Imager: first read the stereo correlation with analyze_mix, then raise High Width (up to about 1.6) and Mid Width a little (up to about 1.3), and keep the low end narrow with Mono Bass on or Low Width at 1 or less. Width changes only the Side, so the mono sum keeps its level; if the stereo correlation drops below 0.2, back the widths off. Change the parameters with set_plugin_params, measure again with analyze_mix, then report the stereo correlation before and after and which parameters you changed.",
    ),
    (
        "Check mono compatibility",
        "Check the mono compatibility of what this Stereo Imager processes: read the stereo correlation of the master, and of this plug-in's track if it is on a track, with analyze_mix (near 1 is almost mono, below 0.2 the sound loses energy in mono, below 0 parts cancel out). If it is fine, change nothing and say why. If it is too low, lower the widths that are above 1 or turn Mono Bass on with set_plugin_params, then measure again with analyze_mix. Report the stereo correlation values you read and which parameters you changed, or why nothing needed changing.",
    ),
];

/// Overdrive: the requests name Mode, Drive and Output and ask for the level change, as its window draws them.
const OVERDRIVE_ACTIONS: &[(&str, &str)] = &[
    ("Warm it up", "Warm up this sound with this Overdrive: Mode Tube (mode 3) or Soft (mode 0), Drive 0.2 to 0.35, Tone 0.35 to 0.5 so the top stays smooth, Mix 1 on a melodic part (0.4 to 0.6 on drums to keep the attack), and lower Output as Drive goes up so the output peak stays near the input peak (the live meters are in the context; with the transport stopped read them with get_plugin while the song plays). Change the parameters with set_plugin_params, then explain in a few lines which parameters you changed, from what value to what value, and the change in output level you expect in dB."),
    ("Add grit without getting louder", "Add grit to this sound with this Overdrive without making it louder: Mode Hard (mode 1) or Fuzz (mode 2), Drive 0.5 to 0.7, Tone 0.5 to 0.65, then lower Output until the output peak matches the input peak (Drive 0.5 multiplies the input by 16, and a shaped peak reaches at most full scale). Change the parameters with set_plugin_params, then explain in a few lines which parameters you changed, from what value to what value, and the change in output level you expect in dB."),
    ("Parallel crunch", "Use this Overdrive as parallel crunch: Mode Hard (mode 1), Drive 0.6 to 0.8, Mix 0.25 to 0.4 so the clean sound keeps its dynamics and the distortion fills in under it, Output so the level stays about the same. Change the parameters with set_plugin_params, then explain in a few lines which parameters you changed, from what value to what value, and the change in output level you expect in dB."),
];
/// Bitcrusher: Resolution, Downsampling and Mix, with the level change.
const BITCRUSHER_ACTIONS: &[(&str, &str)] = &[
    ("Lo-fi but still clear", "Make this sound lo-fi but still clear with this Bitcrusher: Resolution 10 to 12 bits, Downsampling 2 to 3, Mix 0.4 to 0.6 so the dry signal keeps the words and the attacks readable. Change the parameters with set_plugin_params, then explain in a few lines which parameters you changed, from what value to what value, and the change in output level you expect in dB."),
    ("Retro 8-bit crunch", "Give this sound a retro 8-bit crunch with this Bitcrusher: Resolution 6 to 8 bits, Downsampling 4 to 8, Mix 0.8 to 1; quiet parts fall onto a few steps or to silence, so say if the track has quiet passages that would vanish. Change the parameters with set_plugin_params, then explain in a few lines which parameters you changed, from what value to what value, and the change in output level you expect in dB."),
];
/// Exciter: Frequency, Harmonics and Mix, with the level change.
const EXCITER_ACTIONS: &[(&str, &str)] = &[
    ("More air", "Add air to this sound with this Exciter: Frequency 8 to 10 kHz, Harmonics 0.3 to 0.5, Mix 0.2 to 0.35, so the top gets brighter without the S sounds turning harsh. When the request is about the mix, measure it first with analyze_mix. Change the parameters with set_plugin_params, then explain in a few lines which parameters you changed, from what value to what value, and the change in output level you expect in dB."),
    ("Presence without harshness", "Bring this sound forward with this Exciter: Frequency 3 to 5 kHz, Harmonics 0.2 to 0.35, Mix 0.15 to 0.25; if it gets harsh, raise Frequency before lowering Mix. Change the parameters with set_plugin_params, then explain in a few lines which parameters you changed, from what value to what value, and the change in output level you expect in dB."),
];

/// Plug-in AI bar: "Design a sound" for the plug-in `plugin_id` called `name`.
pub fn design_sound(plugin_id: Id, name: &str) -> String {
    format!("Adjust the parameters of plug-in {plugin_id} ({name}) to get a great sound for this track's role in the song. Explain briefly.")
}

/// Label of the ready-made request that sends design_sound: built-in instruments and the generic set offer it.
pub const DESIGN_A_SOUND: &str = "Design a sound";

/// Ready-made requests of a plug-in's AI bar, as (button label, request). An empty request means design_sound.
/// A plug-in kind without its own row gets the generic set.
pub fn plugin_quick_actions(kind: &str) -> &'static [(&'static str, &'static str)] {
    match kind {
        "analog_synth" => SYNTH_ACTIONS,
        "fm_synth" => FM_SYNTH_ACTIONS,
        "pluck_synth" | "organ" | "sampler" => INSTRUMENT_ACTIONS,
        "drum_synth" => DRUM_ACTIONS,
        "channel_eq" => EQ_ACTIONS,
        "dynamic_eq" => DYNAMIC_EQ_ACTIONS,
        "overdrive" => OVERDRIVE_ACTIONS,
        "bitcrusher" => BITCRUSHER_ACTIONS,
        "exciter" => EXCITER_ACTIONS,
        "compressor" => COMPRESSOR_ACTIONS,
        "limiter" => LIMITER_ACTIONS,
        "de_esser" => DE_ESSER_ACTIONS,
        "noise_gate" => NOISE_GATE_ACTIONS,
        "reverb" => REVERB_ACTIONS,
        "stereo_delay" | "tape_delay" => DELAY_ACTIONS,
        "stereo_imager" => STEREO_IMAGER_ACTIONS,
        _ => GENERIC_ACTIONS,
    }
}

/// The request a ready-made action sends for the plug-in `plugin_id` called `name`.
pub fn quick_action_request(action: (&str, &str), plugin_id: Id, name: &str) -> String {
    if action.0 == DESIGN_A_SOUND {
        design_sound(plugin_id, name)
    } else {
        action.1.to_string()
    }
}

/// Ready-made requests for a plug-in of `kind` on the master channel (`on_master`) or on a track or bus. Most
/// plug-ins ask the same in both places; the limiter masters the song on the master and tames one part on a track.
pub fn plugin_quick_actions_at(kind: &str, on_master: bool) -> &'static [(&'static str, &'static str)] {
    match kind {
        "limiter" if !on_master => LIMITER_TRACK_ACTIONS,
        _ => plugin_quick_actions(kind),
    }
}

/// The ready-made requests the AI bar of plug-in `plugin_id` offers, following where it sits.
pub fn plugin_bar_actions(project: &Project, plugin_id: Id) -> &'static [(&'static str, &'static str)] {
    let Some(inst) = project.find_plugin(plugin_id) else {
        return GENERIC_ACTIONS;
    };
    let on_master = matches!(project.plugin_slot(plugin_id), Some((None, _)));
    plugin_quick_actions_at(&inst.kind, on_master)
}

const SYNTH_ACTIONS: &[(&str, &str)] = &[
    (DESIGN_A_SOUND, ""),
    ("Give me a bass for this song", "Turn this Analog Synth into a bass that fits this song: read the tempo, key and the other tracks with get_project, then use Saw or Square oscillators with the Sub Osc Level up, a Lowpass Cutoff low enough to leave room for the kick, moderate Resonance, a short Filter Decay with some Filter Env Amount for the attack of each note and a short Amp Release so the notes do not blur. Change the parameters with set_plugin_params, then explain in a few lines which parameters you changed and why."),
    ("A warm pad that opens slowly", "Turn this Analog Synth into a warm pad that opens slowly: two Saw oscillators slightly detuned, a Lowpass Cutoff low at the start with a positive Filter Env Amount and a slow Filter Attack (1.5 to 3 s) so the sound opens over a few seconds, a slow Amp Attack (0.5 to 1.5 s), high Amp Sustain, a long Amp Release (1.5 to 3 s) and a little LFO on the cutoff. Change the parameters with set_plugin_params, then explain in a few lines which parameters you changed and why."),
];
const INSTRUMENT_ACTIONS: &[(&str, &str)] = &[
    (DESIGN_A_SOUND, ""),
    ("Warmer and softer", "Make this instrument warmer and softer: less brightness and attack, a little more body."),
    ("Brighter, more present", "Make this instrument brighter and more present in the mix without making it harsh."),
];
const DRUM_ACTIONS: &[(&str, &str)] = &[
    (DESIGN_A_SOUND, ""),
    ("Punchier kit", "Make this drum kit punchier: a tighter kick with more attack, a crisper snare, shorter hats."),
    ("Softer vintage kit", "Make this drum kit softer and vintage: a rounder kick, darker hats, a little drive."),
];
/// FM Synth: a bass and a pluck, the sounds two operators do best besides bells and electric pianos.
const FM_SYNTH_ACTIONS: &[(&str, &str)] = &[
    (DESIGN_A_SOUND, ""),
    ("Give me a bass for this song", "Turn this FM Synth into a bass that fits this song: read the tempo, key and the other tracks with get_project, then use a Modulator Ratio of 1 or 0.5, a Mod Index that starts high and falls fast (short Index Decay, low Index Sustain) for a bright attack, enough Sustain to hold the note and a short Release. Change the parameters with set_plugin_params, then explain in a few lines which parameters you changed and why."),
    ("A short pluck", "Turn this FM Synth into a short pluck: a fast Attack, a short Decay (0.15 to 0.4 s), Sustain at 0, a short Release and an Index Decay shorter than the Decay, so only the start of each note is bright. Change the parameters with set_plugin_params, then explain in a few lines which parameters you changed and why."),
];
const EQ_ACTIONS: &[(&str, &str)] = &[
    ("Clean up the muddy frequencies", "Clean up the muddy frequencies of this track (usually between 150 and 500 Hz) without making it thin, and add a high-pass if its lowest frequencies are not useful."),
    ("More air", "Add air and presence on top (a high shelf around 8-12 kHz) without making it harsh."),
    ("Remove the low rumble", "Remove the rumble below the useful range of this track with the high-pass (200 Hz at most), keeping its body."),
];
const COMPRESSOR_ACTIONS: &[(&str, &str)] = &[
    ("Even out the level", "Even out the dynamics of this track: catch the loudest peaks with a moderate ratio and set the makeup gain so the level stays about the same."),
    ("More punch", "Make this track punchier: a slower attack lets the transients through, a faster release recovers before the next hit."),
    ("Gentle glue", "Gentle glue compression: low ratio, soft knee, about 2-3 dB of gain reduction on the loudest parts."),
];
const LIMITER_ACTIONS: &[(&str, &str)] = &[
    ("Bring the master to -14 LUFS", "Bring the master to -14 LUFS integrated with the true peak at or under -1 dBTP: measure it with analyze_mix, set this limiter's gain and ceiling with set_plugin_params and turn true_peak On, then measure again and report the numbers."),
    ("Just catch the peaks", "Set this limiter to catch only the highest peaks of the master: ceiling at -1 dB, true_peak On and the least input gain, so the sound does not change."),
];
const LIMITER_TRACK_ACTIONS: &[(&str, &str)] = &[
    ("Catch this track's peaks", "Set this limiter to catch only the highest peaks of this track: measure the track with analyze_mix, then set a ceiling just under its loudest hits and little input gain, so its sound does not change."),
    ("Even out the loudest hits", "Even out the loudest hits of this track with this limiter: raise the input gain by a few dB, keep the ceiling where the track sits now and set a release that brings the loud notes down without pumping. Measure the track with analyze_mix before and after."),
];
const DE_ESSER_ACTIONS: &[(&str, &str)] = &[
    ("Tame the sibilance", "Tame the harsh S and T sounds of this track: find the frequency of the sibilance and set the threshold so only the S sounds are turned down."),
    ("Gentler de-essing", "De-ess more gently: less range and a higher threshold, so the voice keeps its brightness."),
];
const NOISE_GATE_ACTIONS: &[(&str, &str)] = &[
    ("Clean up the pauses", "Set this noise gate so it closes in the pauses and opens on every note: measure the track with analyze_mix, put the Threshold a few dB above the noise and under the quietest notes, a short Attack, a Hold of 20 to 50 ms and a Release that does not cut the tails. Change the parameters with set_plugin_params, then explain in a few lines which parameters you changed and why."),
    ("Tighter drums", "Make these drums tighter with this noise gate: a Threshold just under the hits, a fast Attack, a short Hold and a Release of 30 to 80 ms, and a Reduction of -20 to -40 dB so the bleed drops without vanishing. Change the parameters with set_plugin_params, then explain in a few lines which parameters you changed and why."),
    ("Gentle noise reduction", "Lower the noise in the pauses without closing the gate completely: a Reduction of -10 to -15 dB, a Threshold just above the noise and a slow Release, so the background breathes instead of switching on and off. Change the parameters with set_plugin_params, then explain in a few lines which parameters you changed and why."),
];
const REVERB_ACTIONS: &[(&str, &str)] = &[
    ("Small, close room", "Make this reverb a small, close room: short size, little pre-delay, low mix, so the track stays upfront."),
    ("Big, lush space", "Make this reverb a big, lush space for this track's role: long size, some pre-delay to keep the attack clear, a mix that suits an insert or a send."),
    ("Keep the low end clean", "Keep the reverb out of the low end: raise its low cut so the bass and the kick stay clear."),
];
const DELAY_ACTIONS: &[(&str, &str)] = &[
    ("Echo in time with the song", "Set this delay to echo in time with the song (synced, for example a dotted eighth or a quarter) with a few repeats that sit behind the dry sound."),
    ("Subtle slapback", "Make this delay a subtle slapback: one short repeat (about 80-120 ms), little feedback, low mix."),
    ("Fewer, darker repeats", "Make the repeats fewer and darker so the delay stays behind the dry sound."),
];
const GENERIC_ACTIONS: &[(&str, &str)] = &[
    (DESIGN_A_SOUND, ""),
    ("Make it subtler", "Make this plug-in's effect subtler: keep its character but use less of it."),
    ("Explain the settings", "Explain in a few lines what this plug-in does with its current settings. Do not change anything."),
];

/// Most VST3 parameters a plug-in's AI request lists; the rest are counted and left to get_plugin, so a plug-in with
/// hundreds of parameters does not fill the local model's context.
pub const PLUGIN_CONTEXT_VST3_PARAMS: usize = 64;

/// A number as the model reads it: at most three decimals, no trailing zeros, no "-0".
fn num(v: f32) -> String {
    let r = (f64::from(v) * 1000.0).round() / 1000.0;
    format!("{}", if r.abs() < 1e-9 { 0.0 } else { r })
}

/// "Label (id), Label (id)" or "none".
fn chain_list(plugins: &[PluginInstance]) -> String {
    if plugins.is_empty() {
        return "none".to_string();
    }
    plugins.iter().map(|p| format!("{} ({})", plugins::plugin_label(p), p.id)).collect::<Vec<_>>().join(", ")
}

/// What the AI needs to know about the plug-in `plugin_id` to work on it: what it is, where it sits (track, its kind
/// and the rest of its chain) and every parameter with its value and range. None when the plug-in is gone.
pub fn plugin_context(project: &Project, plugin_id: Id) -> Option<String> {
    let inst = project.find_plugin(plugin_id)?;
    let (track, slot) = project.plugin_slot(plugin_id)?;
    let info = plugins::find(&inst.kind);
    let mut out = String::new();

    let slot_text = match slot {
        PluginSlot::Instrument => "the instrument".to_string(),
        PluginSlot::MidiFx(i) => format!("MIDI effect at index {i}"),
        PluginSlot::Insert(i) => format!("insert at index {i}"),
    };
    let _ = write!(out, "Plug-in {plugin_id}: {} (kind {}), {slot_text} on ", plugins::plugin_label(inst), inst.kind);
    match track {
        Some(t) => {
            let tkind = match t.kind {
                TrackKind::Instrument => "instrument track",
                TrackKind::Audio => "audio track",
                TrackKind::Bus => "bus",
            };
            let _ = write!(out, "track {} \"{}\" ({tkind}).", t.id, t.name);
        }
        None => out.push_str("the master channel."),
    }
    if inst.bypass {
        out.push_str(" It is bypassed.");
    }

    match info {
        Some(i) => {
            let _ = write!(out, "\nAbout it: {}", i.description);
        }
        None if inst.kind == "vst3" => out.push_str("\nAbout it: third-party VST3 plug-in."),
        None => {}
    }

    match track {
        Some(t) => {
            let instrument = match &t.instrument {
                Some(i) => {
                    let preset = i.preset.as_deref().map(|p| format!(" (preset {p})")).unwrap_or_default();
                    format!("{}{preset}", plugins::plugin_label(i))
                }
                None => "none".to_string(),
            };
            let _ = write!(
                out,
                "\nTrack chain: instrument {instrument}; MIDI effects: {}; inserts: {}; volume {} dB.",
                chain_list(&t.midi_fx),
                chain_list(&t.channel.inserts),
                num(t.channel.volume_db)
            );
        }
        None => {
            let _ = write!(out, "\nMaster chain: inserts: {}; volume {} dB.", chain_list(&project.master.inserts), num(project.master.volume_db));
        }
    }

    if let Some(i) = info {
        out.push_str("\nParameters (id \"name\" = value [min..max unit]):");
        for pi in i.params {
            let v = inst.param(pi.id);
            if pi.choices.is_empty() {
                let unit = if pi.unit.is_empty() { String::new() } else { format!(" {}", pi.unit) };
                let _ = write!(out, "\n- {} \"{}\" = {} [{}..{}{unit}]", pi.id, pi.name, num(v), num(pi.min), num(pi.max));
            } else {
                let chosen = pi.choices.get(v.round().max(0.0) as usize).copied().unwrap_or("?");
                let choices = pi.choices.iter().enumerate().map(|(n, c)| format!("{n} {c}")).collect::<Vec<_>>().join(", ");
                let _ = write!(out, "\n- {} \"{}\" = {} ({chosen}) [choices: {choices}]", pi.id, pi.name, num(v));
            }
        }
    } else if inst.kind == "vst3" {
        let n = inst.vst3_params.len();
        if n == 0 {
            out.push_str("\nParameters: none reported yet (the plug-in may not be loaded).");
        } else {
            let _ = write!(out, "\nParameters ({n} in all, values normalized 0..1):");
            for vp in inst.vst3_params.iter().take(PLUGIN_CONTEXT_VST3_PARAMS) {
                let unit = if vp.unit.is_empty() { String::new() } else { format!(", shown in {}", vp.unit) };
                let _ = write!(out, "\n- {} \"{}\" = {} [0..1{unit}]", vp.id, vp.name, num(inst.param(&vp.id)));
            }
            if n > PLUGIN_CONTEXT_VST3_PARAMS {
                let _ = write!(out, "\n- and {} more: read them with get_plugin.", n - PLUGIN_CONTEXT_VST3_PARAMS);
            }
        }
    } else {
        out.push_str("\nParameters (id = value):");
        for (id, v) in &inst.params {
            let _ = write!(out, "\n- {id} = {}", num(*v));
        }
    }
    Some(out)
}

/// What every request from the AI bar of plug-in `plugin_id` (called `label`) asks besides the request itself.
pub fn plugin_bar_rules(plugin_id: Id, label: &str) -> String {
    format!(
        "This request comes from the AI bar of plug-in {plugin_id} ({label}) and is about that plug-in; its settings are listed below, so you do not need get_plugin to read them. If you change parameters, change them only with set_plugin_params (plugin_id {plugin_id}), all in one call when you can. When the request is about how the track or the mix sounds, measure it first with analyze_mix. While the song plays, get_plugin also returns meters: the live input and output peaks and, for a compressor, limiter, gate, de-esser or dynamic EQ, the gain reduction it applies; read them when you need to know how hard the plug-in is working (with the transport stopped they show silence). Then answer in a few lines: which parameters you changed, from what value to what value, and why."
    )
}

/// The context a request from the plug-in's AI bar adds: the rules above and plugin_context. None when the plug-in is gone.
pub fn plugin_bar_context(project: &Project, plugin_id: Id) -> Option<String> {
    let inst = project.find_plugin(plugin_id)?;
    let label = plugins::plugin_label(inst);
    Some(format!("{}{}{}{}{}\n{}", plugin_bar_rules(plugin_id, &label), eq_bar_note(&inst.kind), synth_bar_note(&inst.kind), dynamics_bar_note(&inst.kind), saturation_bar_note(&inst.kind), plugin_context(project, plugin_id)?))
}

/// plugin_bar_context plus the insert's live meters when there are any (instruments and MIDI effects have none), so
/// the assistant knows how hard the plug-in is working without asking.
pub fn plugin_bar_context_live(project: &Project, plugin_id: Id, meter: Option<&ondina_engine::PluginMeter>) -> Option<String> {
    let mut text = plugin_bar_context(project, plugin_id)?;
    if let Some(m) = meter {
        text.push('\n');
        text.push_str(&meter_note(m));
        if let Some(inst) = project.find_plugin(plugin_id) {
            text.push_str(&saturation_level_note(inst, m));
        }
    }
    Some(text)
}

/// The live meters of an insert as one sentence: peaks in dBFS rounded to 0.1 dB (-120 is silence) and, for plug-ins
/// that reduce gain, the reduction.
pub fn meter_note(m: &ondina_engine::PluginMeter) -> String {
    let db = |x: f32| num((x * 10.0).round() / 10.0);
    let peak = |g: f32| db(ondina_dsp::util::gain_to_db(g));
    let mut text = format!(" Live meters now (peak dBFS; with the transport stopped they fall to silence): input {}, output {}", peak(m.input_peak), peak(m.output_peak));
    if let Some(g) = m.gain_reduction_db {
        let _ = write!(text, ", gain reduction {} dB", db(g));
    }
    text.push('.');
    text
}

/// What the saturation curve gives at the insert's live input peak, as one sentence, so the assistant knows how much
/// louder or quieter the plug-in makes the sound right now. Empty for other kinds, while bypassed and in silence.
pub fn saturation_level_note(inst: &PluginInstance, m: &ondina_engine::PluginMeter) -> String {
    let in_db = ondina_dsp::util::gain_to_db(m.input_peak);
    let Some(curve) = SaturationCurve::of(&inst.kind, |id| inst.param(id)) else {
        return String::new();
    };
    if inst.bypass || in_db <= -90.0 {
        return String::new();
    }
    let round = |x: f32| (x * 10.0).round() / 10.0;
    let out_db = curve.out_db(in_db);
    let delta = round(out_db - in_db);
    let sign = if delta > 0.0 { "+" } else { "" };
    let which = if inst.kind == "exciter" { "the transfer curve of the band above Frequency" } else { "its transfer curve" };
    format!(" At the live input peak of {} dBFS {which} gives {} dBFS ({sign}{} dB).", num(round(in_db)), num(round(out_db)), num(delta))
}

/// What the AI bar of a dynamics plug-in adds to the rules: its window draws the transfer curve and the gain
/// reduction, so the answer gives the values that move them.
pub fn dynamics_bar_note(kind: &str) -> &'static str {
    match kind {
        "compressor" => " This compressor's window draws its transfer curve with the threshold point, and its input, gain reduction and output meters: in your answer give Threshold, Ratio, Knee and Makeup Gain before and after, and the gain reduction you expect on the loud parts.",
        "limiter" => " This limiter's window draws its transfer curve (the point is where limiting starts: Output Ceiling minus Input Gain) and its input, gain reduction and output meters: in your answer give Input Gain, Output Ceiling and Release before and after, and the gain reduction you expect on the loudest peaks.",
        "noise_gate" => " This noise gate's window draws its transfer curve with the threshold point, and its input, gain reduction and output meters: in your answer give Threshold, Reduction, Hold and Release before and after, and say when the gate will close.",
        "de_esser" => " This de-esser's window draws its transfer curve with the threshold point, the band it turns down around Frequency and its gain reduction meter: in your answer give Frequency, Threshold and Range before and after, and the reduction you expect on the S sounds.",
        _ => "",
    }
}

/// What an equalizer's AI bar adds to the rules: its window draws the bands as points, so the answer names them.
pub fn eq_bar_note(kind: &str) -> &'static str {
    match kind {
        "channel_eq" => " This equalizer's window draws each band as a point on its response curve: in your answer name each band you changed (High Pass, Low Shelf, Mid 1, Mid 2, High Shelf, Low Pass) with its frequency, gain and Q before and after, and say why.",
        "dynamic_eq" => " This dynamic equalizer's window draws each band as a point on its response curve, with the reach of its Range shaded: in your answer name each band you changed (Band 1 to Band 4) with its type, frequency, gain, Q, threshold and range before and after, and say why.",
        _ => "",
    }
}

/// What a synth's AI bar adds to the rules: its window draws the envelopes (and the Analog Synth's oscillators and
/// filter curve), so the answer names what changed in those terms.
pub fn synth_bar_note(kind: &str) -> &'static str {
    match kind {
        "analog_synth" => " This synth's window draws the two oscillator shapes, the filter curve and the amp and filter envelopes: in your answer name each stage you changed (Osc 1 and Osc 2 shapes, Cutoff, Resonance, Filter Env Amount, the Amp and Filter Attack, Decay, Sustain and Release) with its value before and after, and say why.",
        "fm_synth" => " This synth's window draws the amp envelope and the modulation index envelope: in your answer name each stage you changed (Attack, Decay, Sustain, Release, Mod Index, Index Decay, Index Sustain) with its value before and after, and say why.",
        _ => "",
    }
}

/// What a saturation plug-in's AI bar adds to the rules: its window draws the transfer curve and the input and
/// output meters, so the answer gives the values that move the curve and the level change it expects.
pub fn saturation_bar_note(kind: &str) -> &'static str {
    match kind {
        "overdrive" => " This overdrive's window draws its transfer curve (input against output level) for the current Mode, with the live input level moving on it, and its input and output meters. Drive multiplies the input by 1 + 60 x Drive squared before the shaper (Drive 0.5 = +24 dB, 1 = +36 dB); a shaped peak reaches at most full scale, then 0.5 + 1 / (1 + 2 x Drive) and Output set the level. In your answer give Mode, Drive and Output before and after, and how many dB louder or quieter the output should get.",
        "bitcrusher" => " This bitcrusher's window draws its stepped transfer curve (input against output level) and its input and output meters. Resolution sets the steps (8 bits = 256 levels: quiet parts fall onto a few steps or to silence), Downsampling holds each value for that many samples, Mix blends in the dry signal. In your answer give Resolution, Downsampling and Mix before and after, and the change in output level you expect in dB.",
        "exciter" => " This exciter's window draws the transfer curve of the band above Frequency and its input and output meters. Harmonics drives that band into a soft shaper (1 + 10 x Harmonics times), Mix sets how much of the shaped band is added to the untouched dry sound. In your answer give Frequency, Harmonics and Mix before and after, and the change in output level you expect in dB.",
        _ => "",
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{Session, Source};
    use ondina_core::command::Command;
    use ondina_core::model::{PluginInstance, Vst3Param, MASTER_ID};
    use ondina_core::plugins::PluginCategory;
    use serde_json::{json, Value};

    fn run(s: &mut Session, name: &str, args: Value) -> Value {
        let cmd = Command::from_tool_call(name, &args).unwrap_or_else(|e| panic!("{name}: {e}"));
        s.execute(cmd, Source::Mcp).unwrap_or_else(|e| panic!("{name} {args}: {e}"))
    }

    fn id(v: &Value, key: &str) -> Id {
        v[key].as_u64().unwrap()
    }

    #[test]
    fn quick_actions_cover_every_builtin_plugin() {
        let kinds: Vec<&str> = plugins::registry().iter().map(|i| i.kind).chain(["vst3", "unknown_kind"]).collect();
        for kind in kinds {
            // the same rules for the plain list and for the lists on the master and on a track
            for (place, actions) in [("any", plugin_quick_actions(kind)), ("master", plugin_quick_actions_at(kind, true)), ("track", plugin_quick_actions_at(kind, false))] {
                assert!((2..=3).contains(&actions.len()), "{kind} ({place}): {} actions", actions.len());
                for (i, (label, request)) in actions.iter().enumerate() {
                    assert!(!label.is_empty(), "{kind} ({place})");
                    assert!(actions[..i].iter().all(|a| a.0 != *label), "{kind} ({place}): '{label}' twice");
                    assert_eq!(request.is_empty(), *label == DESIGN_A_SOUND, "{kind} ({place}): '{label}'");
                }
                if plugins::find(kind).is_some_and(|i| i.category == PluginCategory::Instrument) {
                    assert!(actions.iter().any(|a| a.0 == DESIGN_A_SOUND), "{kind} ({place})");
                }
            }
        }
        assert!(plugin_quick_actions("channel_eq").iter().any(|a| a.0 == "Clean up the muddy frequencies"));
        assert!(plugin_quick_actions("limiter").iter().any(|a| a.0 == "Bring the master to -14 LUFS"));
        for kind in ["compressor", "de_esser", "reverb", "stereo_delay"] {
            assert_ne!(plugin_quick_actions(kind), GENERIC_ACTIONS, "{kind}");
        }
        assert_eq!(plugin_quick_actions("vst3"), GENERIC_ACTIONS);
        assert_eq!(plugin_quick_actions("unknown_kind"), GENERIC_ACTIONS);
    }

    #[test]
    fn dynamics_plugins_have_their_own_quick_actions() {
        for kind in ["compressor", "limiter", "noise_gate", "de_esser"] {
            for on_master in [true, false] {
                let actions = plugin_quick_actions_at(kind, on_master);
                assert!(actions.len() >= 2, "{kind} (master {on_master}): {} actions", actions.len());
                assert_ne!(actions, GENERIC_ACTIONS, "{kind} (master {on_master})");
            }
        }
        let gate = plugin_quick_actions("noise_gate");
        assert_eq!(gate.iter().map(|a| a.0).collect::<Vec<_>>(), ["Clean up the pauses", "Tighter drums", "Gentle noise reduction"]);
        for (label, request) in gate {
            assert!(request.contains("set_plugin_params"), "{label}");
            assert!(request.contains("explain in a few lines which parameters you changed and why"), "{label}");
        }
    }

    #[test]
    fn dynamics_bar_note_names_the_curve_parameters() {
        let wanted: [(&str, &[&str]); 4] = [
            ("compressor", &["Threshold", "Ratio", "Knee"]),
            ("limiter", &["Input Gain", "Output Ceiling"]),
            ("noise_gate", &["Threshold", "Reduction"]),
            ("de_esser", &["Frequency", "Threshold", "Range"]),
        ];
        let tools: Vec<String> = ondina_core::command::tool_specs().into_iter().map(|t| t.name).collect();
        for (kind, names) in wanted {
            let note = dynamics_bar_note(kind);
            assert!(note.contains("transfer curve"), "{kind}: {note}");
            for name in names {
                assert!(note.contains(name), "{kind}: missing {name:?} in {note}");
            }
            for word in note.split(|c: char| !(c.is_ascii_alphanumeric() || c == '_')) {
                assert!(!tools.iter().any(|t| t == word), "{word} names a tool");
            }
        }
        assert_eq!(dynamics_bar_note("channel_eq"), "");
        assert_eq!(dynamics_bar_note("reverb"), "");
        let mut s = Session::new(false);
        let t = id(&run(&mut s, "add_track", json!({"kind": "audio", "name": "Drums"})), "track_id");
        let comp = id(&run(&mut s, "add_effect", json!({"track_id": t, "kind": "compressor"})), "plugin_id");
        let text = plugin_bar_context(&s.project, comp).unwrap();
        assert!(text.contains(dynamics_bar_note("compressor")), "{text}");
    }

    #[test]
    fn design_a_sound_sends_the_design_request() {
        assert_eq!(quick_action_request((DESIGN_A_SOUND, ""), 7, "Analog Synth"), design_sound(7, "Analog Synth"));
        let other = EQ_ACTIONS[1];
        assert_eq!(quick_action_request(other, 7, "Channel EQ"), other.1);
    }

    #[test]
    fn synth_quick_actions_shape_the_sound() {
        let labels = |kind| plugin_quick_actions(kind).iter().map(|a| a.0).collect::<Vec<_>>();
        assert_eq!(labels("analog_synth"), [DESIGN_A_SOUND, "Give me a bass for this song", "A warm pad that opens slowly"]);
        assert_eq!(labels("fm_synth"), [DESIGN_A_SOUND, "Give me a bass for this song", "A short pluck"]);
        assert_ne!(plugin_quick_actions("analog_synth"), plugin_quick_actions("fm_synth"));
        // parameter names each request cites, as the registry names them (or without their "Amp"/"Filter" prefix)
        let cited: [(&str, &[&str]); 2] = [
            ("analog_synth", &["Sub Osc Level", "Cutoff", "Resonance", "Filter Decay", "Filter Env Amount", "Amp Release", "Filter Attack", "Amp Attack", "Amp Sustain"]),
            ("fm_synth", &["Modulator Ratio", "Mod Index", "Index Decay", "Index Sustain", "Sustain", "Release", "Attack", "Decay"]),
        ];
        for (kind, names) in cited {
            let info = plugins::find(kind).unwrap();
            let known = |n: &str| info.params.iter().any(|p| p.name == n || p.name == format!("Amp {n}") || p.name == format!("Filter {n}"));
            for n in names {
                assert!(known(n), "{kind}: '{n}' is not a parameter name");
            }
            for (label, request) in plugin_quick_actions(kind).iter().filter(|a| !a.1.is_empty()) {
                assert!(request.contains("set_plugin_params"), "{kind} '{label}'");
                assert!(request.contains("explain in a few lines which parameters you changed and why"), "{kind} '{label}'");
                let named = names.iter().filter(|n| request.contains(*n)).count();
                assert!(named >= 2, "{kind} '{label}' names {named} parameters");
            }
        }
    }

    #[test]
    fn synth_bar_context_asks_to_name_the_stages() {
        let mut s = Session::new(false);
        let a = id(&run(&mut s, "add_track", json!({"kind": "instrument", "name": "Lead", "instrument": "analog_synth"})), "track_id");
        let f = id(&run(&mut s, "add_track", json!({"kind": "instrument", "name": "Keys", "instrument": "fm_synth"})), "track_id");
        let pid = |t: Id| s.project.track(t).unwrap().instrument.as_ref().unwrap().id;
        let (ap, fp) = (pid(a), pid(f));
        for (p, kind, label) in [(ap, "analog_synth", "Analog Synth"), (fp, "fm_synth", "FM Synth")] {
            let text = plugin_bar_context(&s.project, p).unwrap();
            assert!(text.starts_with(&plugin_bar_rules(p, label)), "{text}");
            assert!(!synth_bar_note(kind).is_empty());
            assert!(text.contains(synth_bar_note(kind)), "{text}");
            assert!(text.contains(&plugin_context(&s.project, p).unwrap()));
        }
        let at = plugin_bar_context(&s.project, ap).unwrap();
        for want in ["Cutoff", "Osc 1", "Filter Env Amount", "say why"] {
            assert!(at.contains(want), "missing {want:?}");
        }
        assert!(plugin_bar_context(&s.project, fp).unwrap().contains("Index Decay"));
        assert_eq!(synth_bar_note("compressor"), "");
        assert_eq!(synth_bar_note("organ"), "");
        let tools: Vec<String> = ondina_core::command::tool_specs().into_iter().map(|t| t.name).collect();
        for kind in ["analog_synth", "fm_synth"] {
            for word in synth_bar_note(kind).split(|c: char| !(c.is_ascii_alphanumeric() || c == '_')) {
                assert!(!tools.iter().any(|t| t == word), "{word} names a tool");
            }
        }
    }

    #[test]
    fn plugin_context_lists_the_track_and_every_parameter() {
        let mut s = Session::new(false);
        let t = id(&run(&mut s, "add_track", json!({"kind": "audio", "name": "Vocals"})), "track_id");
        let c = id(&run(&mut s, "add_effect", json!({"track_id": t, "kind": "compressor"})), "plugin_id");
        let eq = id(&run(&mut s, "add_effect", json!({"track_id": t, "kind": "channel_eq"})), "plugin_id");
        run(&mut s, "set_plugin_param", json!({"plugin_id": eq, "param": "hp_freq", "value": 120}));

        let text = plugin_context(&s.project, eq).unwrap();
        let head = format!("Plug-in {eq}: Channel EQ (kind channel_eq), insert at index 1 on track {t} \"Vocals\" (audio track).\n");
        assert!(text.starts_with(&head), "{text}");
        for want in [
            "About it: Six-band parametric equalizer".to_string(),
            format!("inserts: Compressor ({c}), Channel EQ ({eq})"),
            "Parameters (id \"name\" = value [min..max unit]):".to_string(),
            "- hp_freq \"HP Freq\" = 120 [20..1000 Hz]".to_string(),
            "- hp_on \"High Pass\" = 0 (Off) [choices: 0 Off, 1 On]".to_string(),
            "- mid1_q \"Mid 1 Q\" = 1 [0.1..10]".to_string(),
        ] {
            assert!(text.contains(&want), "missing {want:?} in:\n{text}");
        }
        for pi in plugins::find("channel_eq").unwrap().params {
            assert!(text.contains(&format!("- {} \"{}\" = ", pi.id, pi.name)), "{}", pi.id);
        }
        assert!(!text.ends_with('\n'));
        assert!(plugin_context(&s.project, 9999).is_none());
    }

    #[test]
    fn plugin_context_names_the_instrument_midi_and_master_slots() {
        let mut s = Session::new(false);
        let t = id(&run(&mut s, "add_track", json!({"kind": "instrument", "name": "Lead", "instrument": "analog_synth"})), "track_id");
        let synth = s.project.track(t).unwrap().instrument.as_ref().unwrap().id;
        let arp = id(&run(&mut s, "add_midi_effect", json!({"track_id": t, "kind": "arpeggiator"})), "plugin_id");
        let l = id(&run(&mut s, "add_effect", json!({"track_id": MASTER_ID, "kind": "limiter"})), "plugin_id");

        let text = plugin_context(&s.project, synth).unwrap();
        assert!(text.contains(&format!("the instrument on track {t} \"Lead\" (instrument track).")), "{text}");
        assert!(text.contains("- osc1_wave \"Osc 1 Wave\" = 0 (Saw)"), "{text}");
        assert!(text.contains("- level \"Level\" = -8 [-40..6 dB]"), "{text}");
        assert!(text.contains(&format!("MIDI effects: Arpeggiator ({arp})")), "{text}");
        assert!(!text.contains("It is bypassed."));

        let text = plugin_context(&s.project, arp).unwrap();
        assert!(text.contains(&format!("MIDI effect at index 0 on track {t} \"Lead\" (instrument track).")), "{text}");

        run(&mut s, "set_plugin_bypass", json!({"plugin_id": l, "bypass": true}));
        let text = plugin_context(&s.project, l).unwrap();
        let head = format!("Plug-in {l}: Limiter (kind limiter), insert at index 0 on the master channel. It is bypassed.");
        assert!(text.starts_with(&head), "{text}");
        assert!(text.contains(&format!("Master chain: inserts: Limiter ({l})")), "{text}");
    }

    #[test]
    fn plugin_context_lists_64_vst3_parameters_and_counts_the_rest() {
        let mut s = Session::new(false);
        let t = id(&run(&mut s, "add_track", json!({"kind": "audio", "name": "Keys"})), "track_id");
        let pid = s.project.alloc_id();
        let vst3_params = (0..100).map(|i| Vst3Param { id: format!("p{i}"), name: format!("Param {i}"), default: 0.5, unit: String::new() }).collect();
        let path = Some(r"C:\Private\Test Synth.vst3".to_string());
        let inst = PluginInstance { id: pid, kind: "vst3".into(), params: Default::default(), bypass: false, preset: Some("Test Synth".into()), sample: None, vst3_path: path, vst3_params, vst3_state: None };
        s.project.track_mut(t).unwrap().channel.inserts.push(inst);

        let text = plugin_context(&s.project, pid).unwrap();
        assert!(text.contains("Test Synth (kind vst3)"), "{text}");
        assert!(text.contains("About it: third-party VST3 plug-in."), "{text}");
        assert!(text.contains("Parameters (100 in all, values normalized 0..1):"), "{text}");
        assert!(text.contains("- p63 \"Param 63\" = 0.5 [0..1]"), "{text}");
        assert!(!text.contains("- p64 \"Param 64\""), "{text}");
        assert!(text.contains("- and 36 more: read them with get_plugin."), "{text}");
        // the plug-in's path stays on this computer
        assert!(!text.contains("Private"), "{text}");

        s.project.track_mut(t).unwrap().channel.inserts[0].vst3_params.clear();
        let text = plugin_context(&s.project, pid).unwrap();
        assert!(text.contains("Parameters: none reported yet"), "{text}");
    }

    #[test]
    fn plugin_bar_rules_ask_for_a_short_explanation() {
        let mut s = Session::new(false);
        let t = id(&run(&mut s, "add_track", json!({"kind": "audio", "name": "Vocals"})), "track_id");
        let eq = id(&run(&mut s, "add_effect", json!({"track_id": t, "kind": "channel_eq"})), "plugin_id");
        let text = plugin_bar_context(&s.project, eq).unwrap();
        assert!(text.starts_with(&plugin_bar_rules(eq, "Channel EQ")), "{text}");
        assert!(text.contains(&plugin_context(&s.project, eq).unwrap()));
        let wanted = ["set_plugin_params".to_string(), "analyze_mix".into(), "in a few lines".into(), "which parameters you changed".into(), "and why".into(), format!("plugin_id {eq}")];
        for want in wanted {
            assert!(text.contains(&want), "missing {want:?}");
        }
        assert!(plugin_bar_context(&s.project, 9999).is_none());
    }

    #[test]
    fn eq_bar_context_asks_to_name_the_bands() {
        let mut s = Session::new(false);
        let t = id(&run(&mut s, "add_track", json!({"kind": "audio", "name": "Vocals"})), "track_id");
        let ceq = id(&run(&mut s, "add_effect", json!({"track_id": t, "kind": "channel_eq"})), "plugin_id");
        let deq = id(&run(&mut s, "add_effect", json!({"track_id": t, "kind": "dynamic_eq"})), "plugin_id");
        let comp = id(&run(&mut s, "add_effect", json!({"track_id": t, "kind": "compressor"})), "plugin_id");
        for (pid, kind, label) in [(ceq, "channel_eq", "Channel EQ"), (deq, "dynamic_eq", "Dynamic EQ")] {
            let text = plugin_bar_context(&s.project, pid).unwrap();
            assert!(text.starts_with(&plugin_bar_rules(pid, label)), "{text}");
            assert!(text.contains(eq_bar_note(kind)), "{text}");
            assert!(text.contains(&plugin_context(&s.project, pid).unwrap()));
        }
        let c = plugin_bar_context(&s.project, ceq).unwrap();
        for want in ["Mid 1", "frequency, gain and Q", "say why"] {
            assert!(c.contains(want), "missing {want:?}");
        }
        let d = plugin_bar_context(&s.project, deq).unwrap();
        for want in ["Band 1 to Band 4", "range"] {
            assert!(d.contains(want), "missing {want:?}");
        }
        assert_eq!(eq_bar_note("compressor"), "");
        let rules = plugin_bar_rules(comp, &plugins::plugin_label(s.project.find_plugin(comp).unwrap()));
        assert_eq!(plugin_bar_context(&s.project, comp).unwrap(), format!("{rules}{}\n{}", dynamics_bar_note("compressor"), plugin_context(&s.project, comp).unwrap()));
        let tools: Vec<String> = ondina_core::command::tool_specs().into_iter().map(|t| t.name).collect();
        for kind in ["channel_eq", "dynamic_eq"] {
            for word in eq_bar_note(kind).split(|c: char| !(c.is_ascii_alphanumeric() || c == '_')) {
                assert!(!tools.iter().any(|t| t == word), "{word} names a tool");
            }
        }
    }

    #[test]
    fn plugin_bar_rules_mention_the_live_meters() {
        let text = plugin_bar_rules(7, "Compressor");
        for want in ["get_plugin also returns meters", "gain reduction", "transport stopped"] {
            assert!(text.contains(want), "missing {want:?}: {text}");
        }
    }

    #[test]
    fn limiter_quick_actions_follow_the_slot() {
        let mut s = Session::new(false);
        let l = id(&run(&mut s, "add_effect", json!({"track_id": MASTER_ID, "kind": "limiter"})), "plugin_id");
        let t = id(&run(&mut s, "add_track", json!({"kind": "audio", "name": "Drums"})), "track_id");
        let t_lim = id(&run(&mut s, "add_effect", json!({"track_id": t, "kind": "limiter"})), "plugin_id");
        let comp = id(&run(&mut s, "add_effect", json!({"track_id": t, "kind": "compressor"})), "plugin_id");
        let b = id(&run(&mut s, "add_track", json!({"kind": "bus"})), "track_id");
        let b_lim = id(&run(&mut s, "add_effect", json!({"track_id": b, "kind": "limiter"})), "plugin_id");

        let labels = |pid: Id| -> Vec<&'static str> { plugin_bar_actions(&s.project, pid).iter().map(|a| a.0).collect() };
        let master = labels(l);
        assert!(master.contains(&"Bring the master to -14 LUFS") && master.contains(&"Just catch the peaks"), "{master:?}");
        for pid in [t_lim, b_lim] {
            let track = labels(pid);
            assert!(track.contains(&"Catch this track's peaks") && track.contains(&"Even out the loudest hits"), "{track:?}");
            assert!(!track.contains(&"Bring the master to -14 LUFS"), "{track:?}");
        }
        assert_eq!(plugin_bar_actions(&s.project, comp), plugin_quick_actions("compressor"));
        assert_eq!(plugin_quick_actions_at("limiter", true), plugin_quick_actions("limiter"));
        assert_eq!(plugin_bar_actions(&s.project, 9999), GENERIC_ACTIONS);
    }

    #[test]
    fn limiter_context_lists_every_parameter_and_the_rules() {
        let mut s = Session::new(false);
        let l = id(&run(&mut s, "add_effect", json!({"track_id": MASTER_ID, "kind": "limiter"})), "plugin_id");
        let text = plugin_bar_context(&s.project, l).unwrap();
        for want in [
            "on the master channel",
            "- gain \"Input Gain\" = 0 [0..24 dB]",
            "- ceiling \"Output Ceiling\" = -1 [-12..0 dB]",
            "- release \"Release\" = 60 [1..1000 ms]",
            "- true_peak \"True Peak\" = 0 (Off) [choices: 0 Off, 1 On]",
            "About it: Look-ahead brickwall limiter",
            "set_plugin_params",
            "which parameters you changed",
            "from what value to what value",
            "and why",
        ] {
            assert!(text.contains(want), "missing {want:?} in:
{text}");
        }
    }

    #[test]
    fn numbers_read_short() {
        assert_eq!(num(80.0), "80");
        assert_eq!(num(0.005), "0.005");
        assert_eq!(num(-0.0001), "0");
        assert_eq!(num(1.23456), "1.235");
    }

    #[test]
    fn bar_context_carries_the_live_meters() {
        use ondina_engine::PluginMeter;
        let mut s = Session::new(false);
        let t = id(&run(&mut s, "add_track", json!({"kind": "audio", "name": "Drums"})), "track_id");
        let comp = id(&run(&mut s, "add_effect", json!({"track_id": t, "kind": "compressor"})), "plugin_id");
        let m = PluginMeter { input_peak: 0.5, output_peak: 0.25, gain_reduction_db: Some(-3.04) };
        let base = plugin_bar_context(&s.project, comp).unwrap();
        let live = plugin_bar_context_live(&s.project, comp, Some(&m)).unwrap();
        assert!(live.starts_with(&base), "{live}");
        for want in ["Live meters now", "input -6", "output -12", "gain reduction -3 dB"] {
            assert!(live.contains(want), "missing {want:?} in {live}");
        }
        assert_eq!(plugin_bar_context_live(&s.project, comp, None).as_ref(), Some(&base));
        let eq = PluginMeter { input_peak: 0.5, output_peak: 0.5, gain_reduction_db: None };
        let no_gr = plugin_bar_context_live(&s.project, comp, Some(&eq)).unwrap();
        assert!(no_gr.starts_with(&base) && no_gr.contains("input -6, output -6."), "{no_gr}");
        assert!(!meter_note(&eq).contains("gain reduction"));
        let silent = PluginMeter { input_peak: 0.0, output_peak: 0.0, gain_reduction_db: Some(0.0) };
        assert!(meter_note(&silent).contains("input -120, output -120, gain reduction 0 dB"), "{}", meter_note(&silent));
        assert_eq!(plugin_bar_context_live(&s.project, 9999, Some(&m)), None);
        let tools: Vec<String> = ondina_core::command::tool_specs().into_iter().map(|t| t.name).collect();
        for word in meter_note(&m).split(|c: char| !(c.is_ascii_alphanumeric() || c == '_')) {
            assert!(!tools.iter().any(|t| t == word), "{word} names a tool");
        }
    }

    #[test]
    fn synth_bar_context_live_carries_the_whole_plugin() {
        use ondina_engine::PluginMeter;
        let mut s = Session::new(false);
        for (kind, label) in [("analog_synth", "Analog Synth"), ("fm_synth", "FM Synth")] {
            let t = id(&run(&mut s, "add_track", json!({"kind": "instrument", "name": "Lead", "instrument": kind})), "track_id");
            let p = s.project.track(t).unwrap().instrument.as_ref().unwrap().id;
            // instruments have no live meter in the engine: the bar sends the context without one
            let text = plugin_bar_context_live(&s.project, p, None).unwrap();
            assert_eq!(Some(&text), plugin_bar_context(&s.project, p).as_ref());
            let head = [format!("AI bar of plug-in {p} ({label})"), format!("(kind {kind})"), "the instrument".into(), format!("track {t} \"Lead\" (instrument track)"), "which parameters you changed".into(), "say why".into()];
            for want in head {
                assert!(text.contains(&want), "{kind}: missing {want:?}");
            }
            assert!(text.contains(synth_bar_note(kind)), "{kind}");
            for pi in plugins::find(kind).unwrap().params {
                assert!(text.contains(&format!("
- {} \"{}\" = ", pi.id, pi.name)), "{kind}: {} missing", pi.id);
            }
            assert!(text.contains("[20..20000 Hz]") || kind == "fm_synth", "{kind}: ranges with units");
            // were the engine to meter it, the meters would follow the synth note
            let m = PluginMeter { input_peak: 0.5, output_peak: 0.5, gain_reduction_db: None };
            let live = plugin_bar_context_live(&s.project, p, Some(&m)).unwrap();
            assert!(live.starts_with(&text) && live.ends_with(&meter_note(&m)), "{live}");
        }
    }

    const SATURATION_KINDS: [&str; 3] = ["overdrive", "bitcrusher", "exciter"];

    #[test]
    fn saturation_plugins_have_their_own_quick_actions() {
        let labels: [(&str, &[&str]); 3] = [
            ("overdrive", &["Warm it up", "Add grit without getting louder", "Parallel crunch"]),
            ("bitcrusher", &["Lo-fi but still clear", "Retro 8-bit crunch"]),
            ("exciter", &["More air", "Presence without harshness"]),
        ];
        for (kind, want) in labels {
            for on_master in [true, false] {
                let actions = plugin_quick_actions_at(kind, on_master);
                assert_ne!(actions, GENERIC_ACTIONS, "{kind} (master {on_master})");
                assert!((2..=3).contains(&actions.len()), "{kind}: {} actions", actions.len());
                assert_eq!(actions.iter().map(|a| a.0).collect::<Vec<_>>(), want, "{kind}");
                for (label, request) in actions {
                    assert!(request.contains("set_plugin_params"), "{kind}: {label}");
                    assert!(request.contains("the change in output level you expect in dB"), "{kind}: {label}");
                }
            }
        }
    }

    #[test]
    fn saturation_bar_note_names_the_curve_parameters_and_the_level_change() {
        let wanted: [(&str, &[&str]); 3] = [
            ("overdrive", &["Mode", "Drive", "Output"]),
            ("bitcrusher", &["Resolution", "Downsampling", "Mix"]),
            ("exciter", &["Frequency", "Harmonics", "Mix"]),
        ];
        let tools: Vec<String> = ondina_core::command::tool_specs().into_iter().map(|t| t.name).collect();
        for (kind, names) in wanted {
            let note = saturation_bar_note(kind);
            for want in ["transfer curve", "before and after", "dB"] {
                assert!(note.contains(want), "{kind}: missing {want:?} in {note}");
            }
            for name in names {
                assert!(note.contains(name), "{kind}: missing {name:?} in {note}");
            }
            for word in note.split(|c: char| !(c.is_ascii_alphanumeric() || c == '_')) {
                assert!(!tools.iter().any(|t| t == word), "{word} names a tool");
            }
        }
        assert_eq!(saturation_bar_note("compressor"), "");
        assert_eq!(saturation_bar_note("reverb"), "");
        let mut s = Session::new(false);
        let t = id(&run(&mut s, "add_track", json!({"kind": "audio", "name": "Gtr"})), "track_id");
        let od = id(&run(&mut s, "add_effect", json!({"track_id": t, "kind": "overdrive"})), "plugin_id");
        let text = plugin_bar_context(&s.project, od).unwrap();
        assert!(text.contains(saturation_bar_note("overdrive")), "{text}");
    }

    #[test]
    fn saturation_level_note_reads_the_curve_at_the_live_input() {
        use ondina_dsp::util::gain_to_db;
        use ondina_engine::PluginMeter;
        let rounded = |x: f32| num((x * 10.0).round() / 10.0);
        let mut s = Session::new(false);
        let t = id(&run(&mut s, "add_track", json!({"kind": "audio", "name": "Gtr"})), "track_id");
        let od = id(&run(&mut s, "add_effect", json!({"track_id": t, "kind": "overdrive"})), "plugin_id");
        let ex = id(&run(&mut s, "add_effect", json!({"track_id": t, "kind": "exciter"})), "plugin_id");
        let comp = id(&run(&mut s, "add_effect", json!({"track_id": t, "kind": "compressor"})), "plugin_id");
        let m = PluginMeter { input_peak: 0.25, output_peak: 0.3, gain_reduction_db: None };
        let inst = s.project.find_plugin(od).unwrap().clone();
        let note = saturation_level_note(&inst, &m);
        let curve = SaturationCurve::of("overdrive", |id| inst.param(id)).unwrap();
        let out = rounded(curve.out_db(gain_to_db(0.25)));
        assert!(note.contains("-12") && note.contains(&format!("gives {out} dBFS")) && note.contains("transfer curve"), "{note}");
        assert!(note.contains("(+"), "the overdrive raises -12 dB: {note}");
        let live = plugin_bar_context_live(&s.project, od, Some(&m)).unwrap();
        assert!(live.ends_with(&format!("{}{note}", meter_note(&m))), "{live}");
        // silence, Bypass and a plug-in without a saturation curve add nothing
        assert_eq!(saturation_level_note(&inst, &PluginMeter { input_peak: 0.0, ..m }), "");
        let exciter = saturation_level_note(s.project.find_plugin(ex).unwrap(), &m);
        assert!(exciter.contains("band above Frequency"), "{exciter}");
        assert_eq!(saturation_level_note(s.project.find_plugin(comp).unwrap(), &m), "");
        assert!(plugin_bar_context_live(&s.project, comp, Some(&m)).unwrap().ends_with(&meter_note(&m)));
        run(&mut s, "set_plugin_bypass", json!({"plugin_id": od, "bypass": true}));
        assert_eq!(saturation_level_note(s.project.find_plugin(od).unwrap(), &m), "");
        let tools: Vec<String> = ondina_core::command::tool_specs().into_iter().map(|t| t.name).collect();
        for word in note.split(|c: char| !(c.is_ascii_alphanumeric() || c == '_')) {
            assert!(!tools.iter().any(|t| t == word), "{word} names a tool");
        }
    }

    #[test]
    fn saturation_requests_use_only_local_tools() {
        let mut texts: Vec<&str> = SATURATION_KINDS.iter().flat_map(|k| plugin_quick_actions(k).iter().map(|a| a.1)).collect();
        texts.extend(SATURATION_KINDS.iter().map(|k| saturation_bar_note(k)));
        for text in texts {
            for word in text.split(|c: char| !(c.is_ascii_alphanumeric() || c == '_')) {
                if crate::mcp::tools().iter().any(|t| t.name == word) {
                    assert!(crate::local_ai::LOCAL_TOOLS.contains(&word), "{word}, named in {text:?}, is not offered to the local model");
                }
            }
        }
        for tool in ["set_plugin_params", "get_plugin", "analyze_mix"] {
            assert!(crate::local_ai::LOCAL_TOOLS.contains(&tool), "{tool}");
        }
    }
}
