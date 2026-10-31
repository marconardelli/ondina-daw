# Changelog

All notable changes to Ondina (formerly Aurora) are documented in this file.

The format is based on [Keep a Changelog](https://keepachangelog.com/en/1.1.0/), and this project adheres to
[Semantic Versioning](https://semver.org/spec/v2.0.0.html).

## [Unreleased]

### Changed

- The project is now called **Ondina** (the name Aurora was already taken). The executables are `ondina.exe` and
  `ondina-mcp.exe`, the crates are `ondina-*`, new projects are saved as `.ondina` and the MCP resources use
  `ondina://`. MCP tool names do not change.
- Compatibility with the old name: `.aurora` projects still open (and appear in the Open dialog), the
  `AURORA_MCP_TOKEN` and `AURORA_MODELS` variables still work when the `ONDINA_*` ones are not set, settings are read
  from `%APPDATA%\AuroraDAW` until the first save writes `%APPDATA%\OndinaDAW`, and `aurora://` resource URIs are
  still served. MCP clients and the assistant can save over `.aurora` projects as well as `.ondina` ones.

## [0.2.2] - 2026-10-31

### Changed

- Saving, bouncing and exporting from the assistant or an MCP client now writes only to local drives, with the
  right extension for each file type (`.aurora`, `.wav`, `.mid`/`.midi`, `.musicxml`/`.xml`); a missing extension
  is added for you. Network paths and other extensions are refused, and the assistant never overwrites a file that
  is not an Aurora project. The app's own Save and Bounce dialogs can still use any folder, including a NAS.
- Piano roll: the selected notes stay selected after Undo and Redo, MIDI Transform and edits made by the assistant
  or an MCP client. Notes that no longer exist drop out of the selection.

## [0.2.1] - 2026-10-31

### Changed

- Movies for scoring to picture are read from local drives only: web addresses, network shares and odd paths are
  refused before ffmpeg runs, and ffmpeg is allowed to open local files only. Frames are saved as PNG or JPEG and
  movie exports as MP4, MOV, MKV or M4V.

### Fixed

- Arrange marker lane: clicking a marker jumps to it again, and right-click offers cycle, repeat and delete.
- Markers can be dragged to move them, sections can be resized from their right edge (snapped to the grid), and
  double-clicking a marker renames it. Each change is a single undo step.

## [0.2.0] - 2026-10-31

### Added

- Aurora now asks *Save*, *Don't Save* or *Cancel* before closing the window, starting a new project, opening
  another one or picking a recent file when there are unsaved changes. Esc cancels, Enter saves, and a recording
  in progress is stopped and kept before the question.

### Changed

- Bounces keep reverb and delay tails: after the end of the song (or the cycle), Aurora keeps rendering until the
  sound fades out, from 2 up to 20 seconds, instead of always adding a fixed 2 seconds. This also applies to
  Bounce in Place, surround and binaural bounces.
- The `bounce` MCP tool accepts an optional `tail_sec` to set an exact tail length; `tail_sec: 0` exports a loop
  of exact length with no tail.

## [0.1.2] - 2026-10-31

### Fixed

- Arrange window: dragging the right edge of a region resizes it again, as a single undo step.

### Security

- The MCP server now refuses requests with `Origin: null` (sent by sandboxed iframes and pages opened from files)
  unless `AURORA_MCP_TOKEN` is set, so a web page can no longer drive Aurora through the local server.

## [0.1.1] - 2026-10-31

### Changed

- Piano roll: Quantize, Humanize and Snap to key now work on the selected notes only (all notes when nothing is
  selected), and the same notes stay selected afterwards.
- The matching MCP tools (`quantize_notes`, `humanize_notes`, `snap_notes_to_key`) accept an optional list of note
  indices.
- The AI assistant now knows which notes are selected in the piano roll, so you can ask it to work on just those.

### Fixed

- Undo right after opening a project or starting a new one no longer brings back the previous project.
- A project that has just been opened is no longer marked as modified.

## [0.1.0] - 2026-10-31

First public release.

### Added

- Native Windows DAW in Rust: real-time WASAPI engine, instrument, audio and aux bus tracks, 23 patches.
- Composition: piano roll, step sequencer, score view with MusicXML export, chord track, session players for drums
  (13 styles), bass, chords, arpeggios and melodies.
- Recording: audio and MIDI inputs, cycle recording with takes and comping, Musical Typing.
- Editing: region move/copy/resize/split/join/loop, fades, section markers, MIDI transform and routing.
- Audio operations: time-stretch, pitch shift, varispeed, pitch correction, audio→MIDI, tempo detection, neural
  stem splitter.
- Mixing: mixer with meters, sends and buses, automation, 6 instruments, 16 audio effects, 4 MIDI effects, VST3
  hosting with native editor windows.
- Live loops grid with quantized launching and performance recording.
- Surround (5.1, 7.1, 7.1.4), 3D panner, binaural bounce, ADM BWF export.
- Video linking, timecode and movie export with soundtrack.
- Loudness analysis and automatic mastering; WAV, MIDI and surround bounce.
- AI assistant with a local model (llama.cpp) or Claude; 156 MCP tools over HTTP and stdio, plus a headless mode.
- Piano roll box selection: drag on empty space to select every note in the rectangle, including stacked notes.

### Fixed

- Piano roll: clicking an existing note now selects it instead of adding a duplicate note on top of it;
  double-click / right-click delete and resizing from the right edge work again.
- Piano roll: selected notes are clearly highlighted, also during playback.

[Unreleased]: https://github.com/marconardelli/ondina-daw/compare/v0.2.2...HEAD
[0.2.2]: https://github.com/marconardelli/ondina-daw/compare/v0.2.1...v0.2.2
[0.2.1]: https://github.com/marconardelli/ondina-daw/compare/v0.2.0...v0.2.1
[0.2.0]: https://github.com/marconardelli/ondina-daw/compare/v0.1.2...v0.2.0
[0.1.2]: https://github.com/marconardelli/ondina-daw/compare/v0.1.1...v0.1.2
[0.1.1]: https://github.com/marconardelli/ondina-daw/compare/v0.1.0...v0.1.1
[0.1.0]: https://github.com/marconardelli/ondina-daw/releases/tag/v0.1.0
