# Ondina DAW

**A native digital audio workstation for Windows, written in Rust — with a real-time audio engine, a built-in AI
assistant and a complete MCP API, so that every single operation is available to humans and AI agents alike.**

[![License: MIT](https://img.shields.io/badge/license-MIT-blue.svg)](LICENSE)
![Platform: Windows 10/11](https://img.shields.io/badge/platform-Windows%2010%2F11-0078d4)
![Rust](https://img.shields.io/badge/rust-stable-orange)
![MCP tools](https://img.shields.io/badge/MCP%20tools-163-8a5cf6)
[![Sponsor](https://img.shields.io/badge/sponsor-%E2%9D%A4-db61a2?logo=githubsponsors&logoColor=white)](https://github.com/sponsors/marconardelli)
[![Ko-fi](https://img.shields.io/badge/Ko--fi-support-ff5e5b?logo=kofi&logoColor=white)](https://ko-fi.com/marconardelli)

Ondina covers the full workflow of a modern DAW — compose, record, arrange, mix, master and export.

> **Status: early (0.x).** Ondina is usable and broad, but young. Expect rough edges, especially in the editors.
> Bug reports and contributions are very welcome — see [Contributing](#contributing).

---

## Table of contents

- [Why Ondina](#why-ondina)
- [Features](#features)
- [Quick start](#quick-start)
- [Using Ondina](#using-ondina)
- [AI: three ways in](#ai-three-ways-in)
- [Controlling Ondina over MCP](#controlling-ondina-over-mcp)
- [Architecture](#architecture)
- [Project layout](#project-layout)
- [Building, testing and quality gates](#building-testing-and-quality-gates)
- [Versioning and releases](#versioning-and-releases)
- [Roadmap and known limitations](#roadmap-and-known-limitations)
- [Contributing](#contributing)
- [Support Ondina](#support-ondina)
- [Security](#security)
- [License](#license)

---

## Why Ondina

- **One API for everything.** Every action — a click in the GUI, undo/redo, an MCP call, the AI assistant — is a
  serializable `Command` executed by the same `Session`. Add a command once and it is instantly available to the
  GUI, to MCP clients and to the AI, with documentation generated from its doc comment.
- **Real-time audio in Rust.** The audio thread must deliver a buffer every 1–10 ms without ever stalling. Rust
  gives C/C++ performance with no garbage collector (no unpredictable pauses) and compiler-checked memory and
  thread safety — essential when GUI, audio engine, MCP server and AI agent all work on the same project at once.
- **AI-native, not AI-dependent.** Ondina works fully offline. The AI assistant can run on a local model, Claude is
  optional, and any external agent can drive the whole DAW through MCP without an API key.

## Features

Every row is available both in the GUI and as MCP tools (full list in [`MCP_TOOLS.md`](MCP_TOOLS.md)).

| Area | What Ondina does | Main MCP tools |
|---|---|---|
| Projects, tempo, meter, key | `.ondina` files, tempo changes, count-in, notes and lyrics | `new_project` `save_project` `set_tempo` `add_tempo_change` `set_key` |
| Transport, cycle, metronome | sample-accurate transport, cycle (drag, resize and toggle it on the ruler), click (level and "only while recording" from a right-click on the metronome button), count-in | `play` `stop` `set_cycle` `set_metronome` `set_count_in` |
| Tracks and library | instrument, audio and aux bus tracks, 23 patches | `add_track` `apply_patch` `list_patches` |
| Audio and MIDI recording | WASAPI inputs, MIDI ports, Musical Typing | `record` `set_record_arm` `connect_midi_input` `send_live_midi` |
| Comping | cycle recording creates one take per pass; comp by segments | `list_takes` `comp_takes` |
| Arrangement | drag/copy/resize/split/join/loop regions, copy / cut / paste regions at the playhead (Ctrl+C / X / V, also onto another track), fades, chord track (type chords or roman numerals in place, drag to move and resize), section markers, region rename and color | `move_region` `paste_region` `split_region` `copy_section` `set_chord_progression` `update_chord` |
| Piano roll and step sequencer | note editing, box selection, copy / cut / paste and arrow-key moves, Alt+drag copies, Alt+←/→ change the length, quantize / humanize / snap to key on the selection, harmony voice in the key (3rd, 5th, 6th, octave, above or below), Length menu (legato, remove overlaps, fixed, ×½, ×2) and Velocity menu (set or scale) on the selection, velocity lane, controller lane (sustain, mod wheel, expression, volume, pan, pitch bend) | `add_notes` `quantize_notes` `add_harmony` `set_note_lengths` `replace_cc` `set_pattern_row` |
| MIDI transform and routing | conditional transforms; split/layer of MIDI input | `transform_notes` `set_midi_routes` |
| Session players | drums (13 styles), bass, chords, arpeggios, melodies | `generate_drums` `generate_bass` `generate_chords` … |
| Time and pitch | WSOLA time-stretch, pitch shift, varispeed, key-aware pitch correction, audio→MIDI (YIN) | `time_stretch_region` `pitch_shift_region` `varispeed_region` `pitch_correct_region` `audio_to_midi` |
| Tempo detection | BPM estimation from audio | `detect_tempo` |
| Stem splitter | neural network (Spleeter, ONNX Runtime) for vocals/accompaniment plus spectral drums/bass/other; DSP-only fallback | `split_stems` |
| Mixer, sends, automation | channel strips with meters, routing, volume/pan/send/parameter automation | `set_volume` `add_send` `set_automation` |
| Live loops | cell × scene grid, quantized launching, record the performance to the timeline | `create_live_cell` `launch_cell` `launch_scene` `commit_live_performance` |
| Score | notation view (staff, values, ties, lyrics), MusicXML export | `get_score` `set_lyrics` `export_music_xml` |
| Surround and spatial audio | 5.1, 7.1, 7.1.4, 3D VBAP panner, automated trajectories, binaural monitoring and bounce | `set_surround_format` `set_surround_pan` `bounce_surround` `bounce_binaural` |
| Object-based mixing | tracks as audio objects, ADM BWF export (ITU-R BS.2076 + BS.2088): 7.1.2 bed + objects | `set_surround_pan` `export_adm` |
| Video | linked movie (local file) with offset, movie audio, SMPTE timecode, video window, export with soundtrack (.mp4, .mov, .mkv, .m4v) | `load_movie` `set_frame_rate` `extract_movie_frame` `export_movie` |
| Built-in instruments | Analog Synth, FM Synth, Drum Synth, Sampler, Tonewheel Organ, Plucked String | `set_instrument` `set_plugin_params` |
| Built-in effects | 16 audio effects (EQ, dynamics, delay, reverb, modulation, distortion, filters, utility) + 4 MIDI effects | `add_effect` `add_midi_effect` |
| VST3 plug-ins | scan and host third-party VST3 instruments and effects (`.vst3` bundles on local drives only; MCP clients and the assistant load them, also from projects they open, only from the standard VST3 folders and the folders added in the library), native editor windows, automatable parameters, state saved in the project | `scan_vst3` `add_vst3` `open_plugin_editor` `set_plugin_param` |
| Mastering and loudness | integrated LUFS, true peak, LRA, spectrum, correlation, advice; automatic mastering | `analyze_mix` `auto_master` |
| Bounce and export | WAV 16/24/32-float with dither, reverb and delay tails rendered until they fade (or an exact length with tail_sec), MIDI, bounce in place, multichannel surround | `bounce` `export_midi` `bounce_in_place` |

## Quick start

### Requirements

- Windows 10 or 11 (x64)
- [Rust](https://rustup.rs/) stable with the MSVC toolchain (Visual Studio Build Tools, "Desktop development with C++")
- Optional runtime tools, looked up in `tools\` next to the working directory (not part of the repository):
  - `tools\ffmpeg\bin\ffmpeg.exe` — video frames and movie export (or ffmpeg on your `PATH`)
  - `tools\llm\llama\llama-server.exe` + a `.gguf` model in `tools\llm\` — offline AI assistant (llama.cpp)
  - `tools\models\` — Spleeter ONNX model (~38 MB) for the neural stem splitter
  - `tools\vst3\` — extra VST3 plug-ins to scan

### Download a release

Prebuilt Windows binaries (`ondina.exe` and `ondina-mcp.exe`) are attached to each
[GitHub release](https://github.com/marconardelli/ondina-daw/releases).

### Build from source

```powershell
git clone https://github.com/marconardelli/ondina-daw.git
cd ondina-daw
cargo build --release
.\target\release\ondina.exe                                # GUI + MCP server on http://127.0.0.1:3917/mcp
.\target\release\ondina.exe examples\demo-song.aurora      # open an example project
```

Command-line options: `--no-audio` (null audio device), `--mcp-port N`, `--no-mcp`.

The `examples\` folder contains ready-made projects (`demo-song.aurora`, `feature-demo.aurora`, `lofi-beat.aurora`)
and rendered audio. They were saved before the project was renamed, so they still use the old `.aurora`
extension: Ondina opens both.

## Using Ondina

- **Arrange window**: double-click an instrument lane to create a MIDI region; drag to move, Alt+drag to copy, drag
  the edge to resize. Ctrl+C / Ctrl+X copy or cut the selected region and Ctrl+V pastes it at the playhead (back to the snap grid)
  on the selected track, or on the track it came from when none is selected (also Edit > Copy Region, Cut
  Region, Paste Region); the copy becomes the selection and each cut or paste is one undo step. With notes
  selected in the piano roll these keys act on the notes; the last copy, notes or region, is what Ctrl+V pastes.
  On an audio region, drag the small squares on its top edge to set the fade in and fade out
  (Alt+drag ignores the grid, double-click removes the fade); the fade curves are drawn over the waveform. The ruler has a cycle strip on top: drag the cycle to move it, drag an edge to resize it, click
  it to turn cycle on or off and drag on the empty strip to draw a new one (Shift+drag anywhere on the ruler still
  draws one); click or drag the lower half, with the bar numbers, to move the playhead. Drag a track header up or
  down to reorder the tracks (a line shows where it lands) and double-click a track's name to rename it in place. In the marker lane click a section to jump to it,
  right-click it to cycle, repeat or delete it, drag it to move it, drag its right edge to resize it and
  double-click it to rename it; double-click the empty lane to add a 4-bar section. Right-click a region and choose
  Rename… to type a new name over it (Enter or a click elsewhere confirms, Esc cancels), or Color to pick one of 12
  colors; Track color gives it back the color of its track. The wheel scrolls the tracks,
  Shift+wheel (or a horizontal wheel) scrolls time, Ctrl+wheel zooms time around the pointer and Ctrl+Shift+wheel
  makes the tracks taller or shorter around the one under the pointer; View > Zoom In, Zoom Out, Zoom to Fit, Taller
  Tracks and Shorter Tracks do the same from the menu. On short tracks the M/S/R buttons move next to the name and
  the volume slider is left to the mixer. Press A (or View > Show Automation) to show automation over the regions:
  each track shows one lane (pick Volume, Pan, a send or another lane it already has in the selector that replaces
  the header's volume slider, or in the header's right-click menu Automation lane); click below a region's name bar
  to add a point (on the grid, Alt ignores it), drag a point to move it, double-click or right-click it to delete
  it; an empty lane shows the channel value as a dashed line. Each edit is one undo step and is the same
  `set_automation` agents use. Esc during a drag (a region, a fade, a marker, a chord, the cycle, a track header or
  an automation point) cancels it: everything goes back to where it was.
- **Piano roll**: click on empty space to add a note (length from the toolbar) and select it (Ctrl/Shift+click keeps the selection and adds the new note); click a note to select it,
  Ctrl/Shift+click to add to the selection; drag on empty space to box-select (stacked notes included); drag a note
  to move it (the moved notes stay selected), Alt+drag to copy it (the copies become the selection, one undo step),
  drag its right edge to resize; Esc during a drag (notes, note edges, box selection, velocities, a
  controller stroke) cancels it; Delete/Backspace removes the selection; Ctrl+C/Ctrl+X/Ctrl+V copy, cut
  and paste the selected notes (pasted at the playhead when it is inside the region, otherwise at their original
  position; the copied notes also go to the system clipboard as JSON); Ctrl+A selects every note; ←/→ move the
  selection by the snap step (1/16 with snap off), ↑/↓ by a semitone, Shift+↑/↓ by an octave; Alt+←/→ shorten or lengthen the selection by the snap step (never below one step); double-click or
  right-click deletes a single note; drag in the velocity lane to set velocities: every note the stroke crosses gets the value along the stroke, and a stroke that changes nothing leaves no undo step; below it the controller lane shows one controller of the region (pick it in the selector on the left, which also shows how many events each one has): the sustain pedal as bars where it is down, the others as a stepped line; drag to draw one event per snap step (1/32 with snap off), right-drag to erase, click to set or erase a single step; the curve after the stroke goes back to what it was, each stroke is one undo step and the CC button closes the lane (the built-in instruments respond only to sustain, on the Analog Synth and FM Synth; VST3 instruments get every controller); the selection stays on the same notes after Undo/Redo and assistant or MCP edits; the view opens on the region's notes (double-click the region to frame them again), the wheel scrolls pitch at the same speed both ways, Shift+wheel scrolls time, Ctrl+wheel zooms time around the pointer, Ctrl+Shift+wheel zooms pitch, and the playhead follows the repeats of a looped region.
- **Score**: switch the editor to 𝄞 Score to see the selected region as notation, export MusicXML, assign lyrics.
- **Mixer and inspector**: volume, pan, sends, inserts, plug-in parameters and native VST3 windows; third-party
  plug-ins show their own name, a double-click on a VST3 slot opens its window, and plug-ins that are not loaded are
  struck through with the reason on hover; the inspector's Region section renames and recolors the selected region
  and sets its loop count and velocity offset.
- **Unsaved changes**: when the project has unsaved changes (the • in the title bar), closing the window, New,
  Open or a recent project first asks *Save*, *Don't Save* or *Cancel*. Esc cancels, Enter saves. A recording in
  progress is stopped and kept before the question.
- **Autosave and recovery**: every 3 minutes (Settings > Autosave; 0 turns it off) a copy of unsaved work is
  written in the background to %APPDATA%\OndinaDAW\Autosave. The copy is removed when you save, start a new project
  or open another, and when Ondina closes normally. If Ondina did not close normally, the next start lists the copies
  it left: Recover opens one as unsaved work (Save asks where to save it, proposing the original file), Discard
  deletes it. Audio files are not copied, and a recording still in progress is not in the copy.
- **Safe saving**: Save writes the project to a new temporary file next to it (never over a file that is already
  there), forces it to disk and only then puts it in place, so a full disk, a crash or a power cut during the save
  never leaves a broken project. The previous version is kept as `<name>.ondina.bak` (one copy, replaced at every
  save; Settings > Autosave > *Keep a backup of the previous save*); a file with that name that is not an Ondina
  project (or a folder) is left alone: the project is saved and the status bar says the copy was not written. To go
  back to it, copy it and rename the copy to end in `.ondina`. Settings are written the same way; a settings file
  that cannot be parsed is kept as `settings.json.broken`, and one that Windows does not let Ondina read at startup
  (an antivirus or a cloud sync holding it) is left as it is and not written over until you press *Save settings*;
  the status bar says so in both cases.
- **Drag and drop**: drag audio files (wav, aiff, flac, mp3, ogg, m4a, caf) from Explorer onto an audio track to
  place them at that point (snapped to the grid); below the last track or on an instrument track they go on a new
  audio track. While you drag, a line and the file name show where they land. Several files go one after the
  other, up to 8 per drop, and one Ctrl+Z removes the whole drop. A MIDI file is imported at the beat under the
  pointer; a project file opens (asking to save unsaved changes first). Dropped outside the arrangement, files go
  to the playhead.
- **DSP meter**: the transport bar shows the DSP load of the last audio block and how many blocks since the last
  Play took longer than the audio device allows (red when there are any; each one can be heard as a click).
  `get_transport` reports the same numbers as `dsp_load` and `dsp_overloads`. Ondina flushes denormal numbers to
  zero while it renders, live and in bounces, so reverb and filter tails that fade out do not raise the load.
- **Metronome**: K or a click on the metronome button turns the click on and off. Right-click the button to set
  the click level (-60 to 0 dB, -9.1 dB by default) and to make it click only while recording; the count-in always
  clicks. The button is dark blue while the click waits for a recording. `set_metronome` takes the same
  settings (`level_db`, `only_while_recording`) and `get_transport` reports them.

### Keyboard shortcuts

| Key | Action | Key | Action |
|---|---|---|---|
| Space | play / stop | Ctrl+Z / Ctrl+Y | undo / redo |
| R | record | Delete | delete selection |
| C | toggle cycle | Ctrl+D | duplicate region |
| K | metronome | Ctrl+S / O / N | save / open / new |
| Home | go to start | Ctrl+B | bounce |
| Ctrl+wheel | zoom time around the pointer | Shift+wheel | scroll time |
| Ctrl+Shift+wheel | track height (piano roll: key height) | | |
| Ctrl+C / X / V | copy / cut / paste the selected notes (piano roll), otherwise the selected region (pasted at the playhead) | Ctrl+A | select all notes (piano roll) |
| ← / → | move selected notes by the snap step | ↑ / ↓ (Shift: octave) | transpose selected notes (piano roll) |
| `,` / `.` | playhead to previous / next bar | Shift+, / Shift+. | playhead to previous / next marker |
| Ctrl+T | split selected region at playhead | Esc (while dragging) | cancel the drag |
| Alt+← / → | shorten / lengthen selected notes | Alt+drag (piano roll) | copy the dragged notes |
| ↑ / ↓ | select previous / next track | M / S | mute / solo selected track |
| Shift+M | add a marker at the playhead | A | show / hide automation |

Musical Typing: `A W S E D F T G Y H U J K` play notes, `Z` / `X` change octave.

`,` and `.` from the middle of a bar go to the start of that bar; while playing, less than half a beat past a bar
line or a marker, `,` goes to the one before. ↑ / ↓ move the selected notes while notes are selected in the piano
roll, otherwise they select tracks. These keys, A, M, S, Shift+M and Ctrl+T are off while you type in a text field,
while dragging and while Musical Typing is on (there A, S and M play notes).

## AI: three ways in

All three use the same command API, and every AI edit is undoable: everything the built-in assistant does for one
request is a single undo step, and **↶ Undo AI edit** reverts it only when the last change came from the assistant
(MCP clients get one undo step per tool call).

1. **MCP (no key needed)** — any MCP-capable agent (Claude Code, Claude Desktop, other clients) drives Ondina with
   all 163 tools. See below.
2. **Offline assistant** — the ✨ AI Assistant panel uses a **local language model** through an OpenAI-compatible
   server (llama.cpp, Ollama, LM Studio…). Ondina starts the bundled llama.cpp server automatically when it finds a
   `.gguf` model in `tools\llm` and stops it when Ondina closes. No account, nothing leaves your machine; small
   models get a curated subset of 46 tools, enough for the ✨ buttons of the piano roll, step sequencer, plug-in
   editors and Live Loops.
3. **Claude (optional)** — with an Anthropic API key (Settings ⚙ or `ANTHROPIC_API_KEY`) the panel uses Claude with
   all tools.

Choose the provider in **Settings ⚙ → Provider** (*Auto* uses Claude when a key is configured, the local model
otherwise). The assistant receives the current selection (track, region, notes, playhead) as context, offers quick
actions, and can "listen" to the mix through `analyze_mix`. MCP clients read the same selection with
`get_selection`. With `get_activity` they see what happened since they last looked: the edits made in the window, the
assistant's and other clients' tool calls, and whether the project was replaced or Ondina restarted. Contextual AI
buttons live in the piano roll, step
sequencer, plug-in editors and region menus. The local model edits just the selected notes by passing their indices:
it can quantize, humanize, transpose them, snap them to the project key or add a harmony voice. The line under the
panel title shows which engine answers and how many tools it gets.

## Controlling Ondina over MCP

| Mode | How |
|---|---|
| Running app (Streamable HTTP) | `http://127.0.0.1:3917/mcp` |
| Claude Code | `claude mcp add --transport http ondina http://127.0.0.1:3917/mcp` |
| stdio clients (Claude Desktop…) → running app | `ondina-mcp.exe` |
| Headless session, no GUI | `ondina-mcp.exe --headless [--audio] [--project file.ondina]` |
| List all tools as Markdown | `ondina-mcp.exe --list-tools` |

Example `claude_desktop_config.json`:

```json
{ "mcpServers": { "ondina": { "command": "C:\\path\\to\\ondina-mcp.exe" } } }
```

The server exposes **tools** (163, annotated read-only/destructive), **resources** (`ondina://guide`,
`ondina://project`, `ondina://project/document`, `ondina://plugins`, `ondina://patches`) and **prompts**
(`compose_song`, `mix_review`, `arrange_sections`). It listens on `127.0.0.1` only, rejects non-local `Origin`
headers (DNS-rebinding protection), including `Origin: null` unless a token is set, and requires a bearer token when
`ONDINA_MCP_TOKEN` is set.

API conventions: positions are in **beats** (quarter notes, beat 0 = bar 1), note positions are relative to the
region start, MIDI pitch 60 = C4, `track_id` 0 = master.

## Architecture

```
 GUI ─┐
 MCP ─┼─► Session::execute(Command) ─► Project (+ undo) ─► Arc snapshot ─► Engine (audio thread)
 AI  ─┘
```

| Crate | Responsibility |
|---|---|
| `ondina-core` | project model, the `Command` enum (= the API), music theory, generators, MIDI I/O |
| `ondina-dsp` | instruments (6), effects (16), MIDI effects (4), flex/stem/tempo audio operations, spatial audio, VST3 host |
| `ondina-engine` | renderer, real-time `cpal`/WASAPI engine, live loops, recording, stereo/surround bounce, LUFS analysis, ADM |
| `ondina-studio` | `Session` (executes commands, undo/redo), MCP server (HTTP + stdio + bridge), AI assistant (Claude and local) |
| `ondina-app` | egui GUI: `ondina.exe` (desktop) and `ondina-mcp.exe` (console, for stdio MCP clients) |

Design rules that keep Ondina healthy (please respect them in contributions):

- **Every project change goes through a `Command`**, so it is undoable and reachable from GUI, MCP and AI. The GUI
  never mutates the `Project` directly.
- **The audio thread is real-time**: no allocations, blocking locks, I/O or logging in the audio callback or in
  `process` functions. The engine receives the project as an immutable `Arc` snapshot.
- **The GUI never blocks**: network, AI, analysis, bounce and file work run off the GUI thread and can be cancelled.
- **Existing projects keep opening**: new model fields get `#[serde(default)]`.
- **MCP tools are a public API**: they are never removed or renamed, and existing parameters keep their meaning.

## Project layout

```
crates/
  core/      src/model.rs, command.rs, ops.rs, ops2.rs, generate.rs, theory.rs, tempo.rs, midi_io.rs, plugins.rs
  dsp/       src/synth.rs, drums.rs, effects.rs, midifx.rs, audio_ops.rs, ml_stems.rs, spatial.rs, vst3.rs
  engine/    src/realtime.rs, renderer.rs, offline.rs, analysis.rs, adm.rs, pool.rs
  studio/    src/session.rs, session_ext.rs, mcp.rs, ai.rs, local_ai.rs, settings.rs; tests/features.rs
  app/       src/app.rs, arrange.rs, editors.rs, mixer.rs, inspector.rs, extra_ui.rs, assistant.rs, autosave.rs; src/bin/ondina-mcp.rs
examples/    example projects and rendered audio
MCP_TOOLS.md generated reference of every MCP tool
CHANGELOG.md release notes
```

## Building, testing and quality gates

```powershell
cargo build --release                 # optimized build (thin LTO)
cargo test --workspace                # unit + end-to-end tests
cargo clippy --workspace --all-targets
.\target\release\ondina-mcp.exe --list-tools > MCP_TOOLS.md   # regenerate the tool reference
```

The test suite covers DSP, the engine, the session, MCP over HTTP, the AI agent against a mock server and
end-to-end features — including a test that **calls every MCP tool** and fails if any is not implemented. Tests that
need optional tools (ffmpeg, test VST3 plug-ins, the local model) skip themselves when those are missing.

A change is ready to merge when:

- `cargo test --workspace` passes;
- it adds **no new clippy warnings** in the files it touches (the codebase is being cleaned up gradually, so new
  code must be warning-free);
- no MCP tool disappears, and `MCP_TOOLS.md` is regenerated if tools changed;
- no test is removed, renamed or disabled, and no `#[allow(...)]` / `#[ignore]` is added to silence a problem.

Ondina has also been verified by hand on real hardware: third-party VST3 plug-ins (Dexed, Airwindows Consolidated)
loaded, played and reopened; MIDI input through a virtual loopMIDI port; recording from a real microphone; ADM files
rendered with the EBU reference renderer (`ear`, ITU-R BS.2127); the neural stem splitter on a real song; native
plug-in windows driven over MCP; video linking and export.

## Versioning and releases

Ondina follows [Semantic Versioning](https://semver.org/). The version lives in `[workspace.package]` in
`Cargo.toml`, every release is tagged `vX.Y.Z`, described in [`CHANGELOG.md`](CHANGELOG.md) (Keep a Changelog
format) and published as a GitHub release with Windows binaries. While the version is `0.x`, minor versions add
features and may still change behaviour; patch versions fix and improve.

## Roadmap and known limitations

- The neural stem splitter separates vocals and accompaniment; drums and bass are then extracted with DSP from the
  accompaniment (not with a 4-stem network).
- ADM BWF export follows the open ITU standards and is validated with the EBU renderer; import into the proprietary
  Dolby Atmos Renderer has not been tested.
- Native VST3 windows use a dedicated "proxy" instance; parameters and full state are synchronized with the playing
  instance about once per second and when the window closes.
- MIDI input was tested through the Windows MIDI subsystem with a virtual port, not yet with a physical keyboard.
- A 3B local model follows direct requests well but is much less capable than Claude on long creative tasks; with a
  local model on CPU the first answer can take minutes.
- The score view is display-only for now: edit notes in the piano roll.

Ideas and larger plans are tracked as [GitHub issues](https://github.com/marconardelli/ondina-daw/issues) — look for
the `good first issue` and `help wanted` labels.

## Contributing

Contributions of every size are welcome: bug reports, new instruments and effects, editor improvements, MCP tools,
documentation, tests, performance work.

1. Read [`CONTRIBUTING.md`](CONTRIBUTING.md): setup, coding conventions, how to add a command / MCP tool, an
   instrument or an effect, and how pull requests are reviewed.
2. Open an issue first for anything bigger than a small fix, so we can agree on the approach.
3. Fork, create a branch, make a focused change with tests, and open a pull request.

By contributing you agree that your contributions are licensed under the MIT License.

## Support Ondina

Ondina is free and open source, developed in spare time. If it is useful to you and you want to help it grow, you can
support its development:

- [**GitHub Sponsors**](https://github.com/sponsors/marconardelli) — one-time or monthly sponsorship.
- [**Ko-fi**](https://ko-fi.com/marconardelli) — buy me a coffee, no account needed.

Every contribution goes into development time, test hardware and plug-ins. Starring the repository, reporting bugs and
telling other musicians about Ondina help just as much.

## Security

Ondina runs a local MCP server and can load native plug-ins, so please read [`SECURITY.md`](SECURITY.md) and report
vulnerabilities privately as described there, not in public issues.

## License

[MIT](LICENSE) © 2026 Marco Nardelli.

VST is a trademark of Steinberg Media Technologies GmbH.
