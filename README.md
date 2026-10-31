# Aurora DAW

**A native digital audio workstation for Windows, written in Rust — with a real-time audio engine, a built-in AI
assistant and a complete MCP API, so that every single operation is available to humans and AI agents alike.**

[![License: MIT](https://img.shields.io/badge/license-MIT-blue.svg)](LICENSE)
![Platform: Windows 10/11](https://img.shields.io/badge/platform-Windows%2010%2F11-0078d4)
![Rust](https://img.shields.io/badge/rust-stable-orange)
![MCP tools](https://img.shields.io/badge/MCP%20tools-156-8a5cf6)

Aurora covers the full workflow of a modern DAW — compose, record, arrange, mix, master and export.

> **Status: early (0.x).** Aurora is usable and broad, but young. Expect rough edges, especially in the editors.
> Bug reports and contributions are very welcome — see [Contributing](#contributing).

---

## Table of contents

- [Why Aurora](#why-aurora)
- [Features](#features)
- [Quick start](#quick-start)
- [Using Aurora](#using-aurora)
- [AI: three ways in](#ai-three-ways-in)
- [Controlling Aurora over MCP](#controlling-aurora-over-mcp)
- [Architecture](#architecture)
- [Project layout](#project-layout)
- [Building, testing and quality gates](#building-testing-and-quality-gates)
- [Versioning and releases](#versioning-and-releases)
- [Roadmap and known limitations](#roadmap-and-known-limitations)
- [Contributing](#contributing)
- [Security](#security)
- [License](#license)

---

## Why Aurora

- **One API for everything.** Every action — a click in the GUI, undo/redo, an MCP call, the AI assistant — is a
  serializable `Command` executed by the same `Session`. Add a command once and it is instantly available to the
  GUI, to MCP clients and to the AI, with documentation generated from its doc comment.
- **Real-time audio in Rust.** The audio thread must deliver a buffer every 1–10 ms without ever stalling. Rust
  gives C/C++ performance with no garbage collector (no unpredictable pauses) and compiler-checked memory and
  thread safety — essential when GUI, audio engine, MCP server and AI agent all work on the same project at once.
- **AI-native, not AI-dependent.** Aurora works fully offline. The AI assistant can run on a local model, Claude is
  optional, and any external agent can drive the whole DAW through MCP without an API key.

## Features

Every row is available both in the GUI and as MCP tools (full list in [`MCP_TOOLS.md`](MCP_TOOLS.md)).

| Area | What Aurora does | Main MCP tools |
|---|---|---|
| Projects, tempo, meter, key | `.aurora` files, tempo changes, count-in, notes and lyrics | `new_project` `save_project` `set_tempo` `add_tempo_change` `set_key` |
| Transport, cycle, metronome | sample-accurate transport, cycle, click, count-in | `play` `stop` `set_cycle` `set_metronome` `set_count_in` |
| Tracks and library | instrument, audio and aux bus tracks, 23 patches | `add_track` `apply_patch` `list_patches` |
| Audio and MIDI recording | WASAPI inputs, MIDI ports, Musical Typing | `record` `set_record_arm` `connect_midi_input` `send_live_midi` |
| Comping | cycle recording creates one take per pass; comp by segments | `list_takes` `comp_takes` |
| Arrangement | drag/copy/resize/split/join/loop regions, fades, chord track, section markers | `move_region` `split_region` `copy_section` `set_chord_progression` |
| Piano roll and step sequencer | note editing, box selection, quantize, humanize, velocity lane | `add_notes` `quantize_notes` `set_pattern_row` |
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
| Video | linked movie with offset, movie audio, SMPTE timecode, video window, export with soundtrack | `load_movie` `set_frame_rate` `extract_movie_frame` `export_movie` |
| Built-in instruments | Analog Synth, FM Synth, Drum Synth, Sampler, Tonewheel Organ, Plucked String | `set_instrument` `set_plugin_params` |
| Built-in effects | 16 audio effects (EQ, dynamics, delay, reverb, modulation, distortion, filters, utility) + 4 MIDI effects | `add_effect` `add_midi_effect` |
| VST3 plug-ins | scan and host third-party VST3 instruments and effects, native editor windows, automatable parameters, state saved in the project | `scan_vst3` `add_vst3` `open_plugin_editor` `set_plugin_param` |
| Mastering and loudness | integrated LUFS, true peak, LRA, spectrum, correlation, advice; automatic mastering | `analyze_mix` `auto_master` |
| Bounce and export | WAV 16/24/32-float with dither, MIDI, bounce in place, multichannel surround | `bounce` `export_midi` `bounce_in_place` |

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

Prebuilt Windows binaries (`aurora.exe` and `aurora-mcp.exe`) are attached to each
[GitHub release](https://github.com/marconardelli/aurora-daw/releases).

### Build from source

```powershell
git clone https://github.com/marconardelli/aurora-daw.git
cd aurora-daw
cargo build --release
.\target\release\aurora.exe                                # GUI + MCP server on http://127.0.0.1:3917/mcp
.\target\release\aurora.exe examples\demo-song.aurora      # open an example project
```

Command-line options: `--no-audio` (null audio device), `--mcp-port N`, `--no-mcp`.

The `examples\` folder contains ready-made projects (`demo-song.aurora`, `feature-demo.aurora`, `lofi-beat.aurora`)
and rendered audio.

## Using Aurora

- **Arrange window**: double-click an instrument lane to create a MIDI region; drag to move, Alt+drag to copy, drag
  the edge to resize; Shift+drag on the ruler to set the cycle.
- **Piano roll**: click on empty space to add a note (length from the toolbar); click a note to select it,
  Ctrl/Shift+click to add to the selection; drag on empty space to box-select (stacked notes included); drag a note
  to move it, drag its right edge to resize; Delete/Backspace removes the selection; double-click or right-click
  deletes a single note; drag in the velocity lane to set velocities.
- **Score**: switch the editor to 𝄞 Score to see the selected region as notation, export MusicXML, assign lyrics.
- **Mixer and inspector**: volume, pan, sends, inserts, plug-in parameters and native VST3 windows.

### Keyboard shortcuts

| Key | Action | Key | Action |
|---|---|---|---|
| Space | play / stop | Ctrl+Z / Ctrl+Y | undo / redo |
| R | record | Delete | delete selection |
| C | toggle cycle | Ctrl+D | duplicate region |
| K | metronome | Ctrl+S / O / N | save / open / new |
| Home | go to start | Ctrl+B | bounce |
| Ctrl+wheel | zoom | Shift+wheel | scroll horizontally |

Musical Typing: `A W S E D F T G Y H U J K` play notes, `Z` / `X` change octave.

## AI: three ways in

All three use the same command API, and every AI edit is undoable.

1. **MCP (no key needed)** — any MCP-capable agent (Claude Code, Claude Desktop, other clients) drives Aurora with
   all 156 tools. See below.
2. **Offline assistant** — the ✨ AI Assistant panel uses a **local language model** through an OpenAI-compatible
   server (llama.cpp, Ollama, LM Studio…). Aurora starts the bundled llama.cpp server automatically when it finds a
   `.gguf` model in `tools\llm`. No account, nothing leaves your machine; small models get a curated subset of 31
   tools.
3. **Claude (optional)** — with an Anthropic API key (Settings ⚙ or `ANTHROPIC_API_KEY`) the panel uses Claude with
   all tools.

Choose the provider in **Settings ⚙ → Provider** (*Auto* uses Claude when a key is configured, the local model
otherwise). The assistant receives the current selection (track, region, notes, playhead) as context, offers quick
actions, and can "listen" to the mix through `analyze_mix`. Contextual AI buttons live in the piano roll, step
sequencer, plug-in editors and region menus.

## Controlling Aurora over MCP

| Mode | How |
|---|---|
| Running app (Streamable HTTP) | `http://127.0.0.1:3917/mcp` |
| Claude Code | `claude mcp add --transport http aurora http://127.0.0.1:3917/mcp` |
| stdio clients (Claude Desktop…) → running app | `aurora-mcp.exe` |
| Headless session, no GUI | `aurora-mcp.exe --headless [--audio] [--project file.aurora]` |
| List all tools as Markdown | `aurora-mcp.exe --list-tools` |

Example `claude_desktop_config.json`:

```json
{ "mcpServers": { "aurora": { "command": "C:\\path\\to\\aurora-mcp.exe" } } }
```

The server exposes **tools** (156, annotated read-only/destructive), **resources** (`aurora://guide`,
`aurora://project`, `aurora://project/document`, `aurora://plugins`, `aurora://patches`) and **prompts**
(`compose_song`, `mix_review`, `arrange_sections`). It listens on `127.0.0.1` only, rejects non-local `Origin`
headers (DNS-rebinding protection) and requires a bearer token when `AURORA_MCP_TOKEN` is set.

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
| `aurora-core` | project model, the `Command` enum (= the API), music theory, generators, MIDI I/O |
| `aurora-dsp` | instruments (6), effects (16), MIDI effects (4), flex/stem/tempo audio operations, spatial audio, VST3 host |
| `aurora-engine` | renderer, real-time `cpal`/WASAPI engine, live loops, recording, stereo/surround bounce, LUFS analysis, ADM |
| `aurora-studio` | `Session` (executes commands, undo/redo), MCP server (HTTP + stdio + bridge), AI assistant (Claude and local) |
| `aurora-app` | egui GUI: `aurora.exe` (desktop) and `aurora-mcp.exe` (console, for stdio MCP clients) |

Design rules that keep Aurora healthy (please respect them in contributions):

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
  app/       src/app.rs, arrange.rs, editors.rs, mixer.rs, inspector.rs, extra_ui.rs, assistant.rs; src/bin/aurora-mcp.rs
examples/    example projects and rendered audio
MCP_TOOLS.md generated reference of every MCP tool
CHANGELOG.md release notes
```

## Building, testing and quality gates

```powershell
cargo build --release                 # optimized build (thin LTO)
cargo test --workspace                # unit + end-to-end tests
cargo clippy --workspace --all-targets
.\target\release\aurora-mcp.exe --list-tools > MCP_TOOLS.md   # regenerate the tool reference
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

Aurora has also been verified by hand on real hardware: third-party VST3 plug-ins (Dexed, Airwindows Consolidated)
loaded, played and reopened; MIDI input through a virtual loopMIDI port; recording from a real microphone; ADM files
rendered with the EBU reference renderer (`ear`, ITU-R BS.2127); the neural stem splitter on a real song; native
plug-in windows driven over MCP; video linking and export.

## Versioning and releases

Aurora follows [Semantic Versioning](https://semver.org/). The version lives in `[workspace.package]` in
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

Ideas and larger plans are tracked as [GitHub issues](https://github.com/marconardelli/aurora-daw/issues) — look for
the `good first issue` and `help wanted` labels.

## Contributing

Contributions of every size are welcome: bug reports, new instruments and effects, editor improvements, MCP tools,
documentation, tests, performance work.

1. Read [`CONTRIBUTING.md`](CONTRIBUTING.md): setup, coding conventions, how to add a command / MCP tool, an
   instrument or an effect, and how pull requests are reviewed.
2. Open an issue first for anything bigger than a small fix, so we can agree on the approach.
3. Fork, create a branch, make a focused change with tests, and open a pull request.

By contributing you agree that your contributions are licensed under the MIT License.

## Security

Aurora runs a local MCP server and can load native plug-ins, so please read [`SECURITY.md`](SECURITY.md) and report
vulnerabilities privately as described there, not in public issues.

## License

[MIT](LICENSE) © 2026 Marco Nardelli.

VST is a trademark of Steinberg Media Technologies GmbH.
