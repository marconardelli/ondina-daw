# Changelog

All notable changes to Aurora are documented in this file.

The format is based on [Keep a Changelog](https://keepachangelog.com/en/1.1.0/), and this project adheres to
[Semantic Versioning](https://semver.org/spec/v2.0.0.html).

## [Unreleased]

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

[Unreleased]: https://github.com/marconardelli/aurora-daw/compare/v0.1.0...HEAD
[0.1.0]: https://github.com/marconardelli/aurora-daw/releases/tag/v0.1.0
