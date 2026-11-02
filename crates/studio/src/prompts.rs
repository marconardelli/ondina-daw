//! Requests sent by the contextual AI buttons of the GUI. They live here, next to the assistants, so a test can
//! check that the local model is offered every tool these requests need.

use std::fmt::Write;

use ondina_core::model::{Id, PluginInstance, PluginSlot, Project, TrackKind};
use ondina_core::plugins;

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
        "analog_synth" | "fm_synth" => SYNTH_ACTIONS,
        "pluck_synth" | "organ" | "sampler" => INSTRUMENT_ACTIONS,
        "drum_synth" => DRUM_ACTIONS,
        "channel_eq" => EQ_ACTIONS,
        "dynamic_eq" => DYNAMIC_EQ_ACTIONS,
        "compressor" => COMPRESSOR_ACTIONS,
        "limiter" => LIMITER_ACTIONS,
        "de_esser" => DE_ESSER_ACTIONS,
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

const SYNTH_ACTIONS: &[(&str, &str)] = &[
    (DESIGN_A_SOUND, ""),
    ("A bass for this song", "Turn this synth into a bass sound that fits this song: solid low end, controlled resonance, short release."),
    ("A warm pad that opens slowly", "Turn this synth into a warm pad that opens slowly: slow attack, a filter that opens over a few seconds, long release."),
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
    ("Bring the master to -14 LUFS", "Bring the master to -14 LUFS integrated with the true peak at or under -1 dB: measure it with analyze_mix, set this limiter's gain and ceiling with set_plugin_params, then measure again and report the numbers."),
    ("Just catch the peaks", "Set this limiter to catch only the highest peaks: ceiling at -1 dB and the least input gain, so the sound does not change."),
];
const DE_ESSER_ACTIONS: &[(&str, &str)] = &[
    ("Tame the sibilance", "Tame the harsh S and T sounds of this track: find the frequency of the sibilance and set the threshold so only the S sounds are turned down."),
    ("Gentler de-essing", "De-ess more gently: less range and a higher threshold, so the voice keeps its brightness."),
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
    let label = plugins::plugin_label(project.find_plugin(plugin_id)?);
    Some(format!("{}\n{}", plugin_bar_rules(plugin_id, &label), plugin_context(project, plugin_id)?))
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
            let actions = plugin_quick_actions(kind);
            assert!((2..=3).contains(&actions.len()), "{kind}: {} actions", actions.len());
            for (i, (label, request)) in actions.iter().enumerate() {
                assert!(!label.is_empty(), "{kind}");
                assert!(actions[..i].iter().all(|a| a.0 != *label), "{kind}: '{label}' twice");
                assert_eq!(request.is_empty(), *label == DESIGN_A_SOUND, "{kind}: '{label}'");
            }
            if plugins::find(kind).is_some_and(|i| i.category == PluginCategory::Instrument) {
                assert!(actions.iter().any(|a| a.0 == DESIGN_A_SOUND), "{kind}");
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
    fn design_a_sound_sends_the_design_request() {
        assert_eq!(quick_action_request((DESIGN_A_SOUND, ""), 7, "Analog Synth"), design_sound(7, "Analog Synth"));
        let other = EQ_ACTIONS[1];
        assert_eq!(quick_action_request(other, 7, "Channel EQ"), other.1);
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
    fn plugin_bar_rules_mention_the_live_meters() {
        let text = plugin_bar_rules(7, "Compressor");
        for want in ["get_plugin also returns meters", "gain reduction", "transport stopped"] {
            assert!(text.contains(want), "missing {want:?}: {text}");
        }
    }

    #[test]
    fn numbers_read_short() {
        assert_eq!(num(80.0), "80");
        assert_eq!(num(0.005), "0.005");
        assert_eq!(num(-0.0001), "0");
        assert_eq!(num(1.23456), "1.235");
    }
}
