# Changelog

All notable changes to Ondina (formerly Aurora) are documented in this file.

The format is based on [Keep a Changelog](https://keepachangelog.com/en/1.1.0/), and this project adheres to
[Semantic Versioning](https://semver.org/spec/v2.0.0.html).

## [Unreleased]

## [0.10.1] - 2026-10-31

### Changed

- Press Esc while dragging to cancel the drag: everything goes back to where it was, with no need for Ctrl+Z. It
  works in the Arrange window (regions, fades, markers, chords, the cycle, track headers) and in the piano roll
  (notes, note edges, box selection, velocities, controller strokes).
- The transport bar shows, next to the DSP load, how many audio blocks since the last Play took longer than the
  audio device allows. The number turns red as soon as there is one, since each can be heard as a click.
  `get_transport` reports it as `dsp_overloads`.

### Fixed

- No more CPU spikes while notes, reverb tails and filters fade out: the tiny values left at the end of a sound
  are flushed to zero during playback and in bounces. The sound does not change.

## [0.10.0] - 2026-10-31

### Added

- Fade handles on audio regions in the Arrange window: hover over a region (or select it) and drag the small
  squares on its top edge to set the fade-in and fade-out. The fade curve is drawn over the waveform, on every
  repetition of a looped region.
- Fade dragging follows Snap (at most one beat); hold Alt to drag freely. Double-click a handle to remove that
  fade, and undo any drag with a single Ctrl+Z.
- The VST3 library has **Add folder…**: the folders you add are saved in the settings and scanned, and the ✕ next
  to a folder removes it.

### Changed

- MCP clients and the assistant can load VST3 plug-ins only from the standard VST3 folders and from the folders
  you added in the library. Other paths are refused with "plug-in path refused", and `scan_vst3` skips other extra
  folders and lists them in `skipped_paths`. No tool can change the folder list. **Add from file…** in the app
  is not limited, because the choice is yours.

### Fixed

- The **Fade in** and **Fade out** sliders in the audio editor now show fades longer than 8 beats instead of
  cutting them off.

## [0.9.0] - 2026-10-31

### Added

- Arrange keyboard shortcuts: `,` and `.` move the playhead to the previous or next bar, Shift+`,` and Shift+`.` to
  the previous or next marker (or to the start or end of the song).
- Ctrl+T splits the selected region at the playhead.
- From the middle of a bar, `,` goes to the start of that bar; while playing, just past a bar line or a marker it
  goes to the one before. These keys are off while you type in a text field and while Musical Typing is on.

### Changed

- **New** now stops the transport (Live Loops cells too) and puts the playhead back at the start, like **Open**.
- The assistant and MCP clients can no longer open or create a project while a recording is running: they are
  asked to stop first, so the take stays in the song it was recorded for.

### Fixed

- Opening or creating a project during a recording no longer leaves the recording running on the new project: the
  recording is stopped and the take is not written into the wrong song.

### Security

- `scan_vst3` extra folders are checked after Windows resolves them: a folder on a drive letter mapped to a network
  share, or one containing a link or junction that leads to a share, is skipped instead of scanned (scanning loads
  every plug-in it finds). A folder that cannot be resolved is skipped too.
- A plug-in path that Windows cannot resolve (for example access denied or a broken link) is now refused; only a path
  that does not exist is still passed on, so the host can report it as not found.

## [0.8.1] - 2026-10-31

### Changed

- VST3 plug-ins are loaded only from a `.vst3` bundle on a local drive. The same check applies to **Add from
  file…**, the assistant, MCP clients, the extra scan folders and the projects you open: plug-ins on a network
  share, or files that are not `.vst3`, are refused with a message.
- Opening a project that uses a refused plug-in still works: the plug-in stays in place but is silent, and the
  status bar says how many plug-ins were not loaded and how many audio files are missing. If you keep plug-ins on
  a NAS, copy them to a local folder and add them again.

### Fixed

- Chord track: **Edit chord** from the right-click menu now opens the text field to change the chord.
- A rename field left open (marker, track, region, region name in the inspector, chord) now closes without
  applying anything when you create or open another project, also when the assistant or an MCP client does it.
- Track headers: right-click only opens the menu. Holding or dragging with the right or middle button no longer
  moves the track; moving a track is done by dragging with the left button.
- Step sequencer: during playback the white outline follows the playing step in every repeat of a looped region.

## [0.8.0] - 2026-10-31

### Added

- Piano roll: a controller lane below the velocity lane shows one MIDI controller of the region at a time. Pick
  Sustain, Mod wheel, Expression, Volume, Pan, Pitch bend or any other controller already in the region from the
  selector on the left, which also shows how many events each one has.
- Piano roll: the sustain pedal is drawn as bars where it is down, the other controllers as a stepped line.
- Piano roll: drag in the controller lane to draw one event per snap step, right-drag to erase, click to set or
  erase a single step. The rest of the curve stays as it was, and each stroke is a single undo step.
- Piano roll: the CC button closes the controller lane to leave more room for the notes. The built-in Analog Synth
  and FM Synth respond to the sustain pedal; VST3 instruments get every controller.
- MCP: new `replace_cc` tool replaces the events of one controller in a beat range, so the assistant can edit
  sustain, mod wheel and other curves as well as add them.

### Changed

- Regions: right-click a region and choose **Rename…** to type a new name over it (Enter or a click elsewhere
  confirms, Esc cancels), or **Color** to pick one of 12 colors. **Track color** gives it back the color of its
  track and keeps following it.
- Inspector: the Region section renames and recolors the selected region, with the same colors and a **Custom**
  picker for any color. Every rename or color change is a single undo step.

## [0.7.0] - 2026-10-31

### Added

- Piano roll: Ctrl+C, Ctrl+X and Ctrl+V copy, cut and paste the selected notes. Pasted notes land at the playhead
  when it is inside the region, otherwise at the position they were copied from, and stay selected. Each paste is
  a single undo step.
- Piano roll: copied notes also go to the system clipboard as text, so you can paste them into the assistant chat.
- Piano roll: Ctrl+A selects every note.
- Piano roll: the arrow keys move the selected notes: left and right by the snap step, up and down by a semitone,
  Shift+up and down by an octave. Chords keep their shape at the edges of the region and of the MIDI range.

### Changed

- Ruler: the top strip is now a cycle strip. Drag the cycle to move it, drag either edge to resize it, click it to
  turn cycle on or off, and drag on the empty strip to draw a new one. The lower half, with the bar numbers, moves
  the playhead. Shift+drag anywhere on the ruler still draws a cycle, and every gesture is a single undo step.

## [0.6.0] - 2026-10-31

### Added

- Chord track: double-click an empty spot to type a chord in place, as a symbol (`Am7`, `F#m`, `G/B`) or as a
  Roman numeral in the project key (`I`, `vi`, `IV`, `V7`). A numeral becomes the matching chord: `IV` in C major
  is `F`.
- Chord track: double-click a chord (or right-click and **Edit chord**) to change it, drag it to move it, drag its
  right edge to change its length, and right-click **Remove chord** to delete it. Every change is a single undo
  step.
- Chord track: the label on the left shows the project key, and an invalid chord keeps the field open with a red
  outline so you can retype it.
- Session players (Bassist, Keys, Arpeggio, Melody) follow the chord track when they get no progression of their
  own, and Roman numerals saved on the chord track are read in the current project key.
- MCP: new `update_chord` tool to change a chord's symbol, start or length; `add_chord` also accepts Roman
  numerals.

### Changed

- Local AI model: the ✨ buttons now work with the offline assistant. It gets the tools their requests need
  (continue or improve notes in the piano roll, write a groove in the step sequencer, design a sound in the plug-in
  editors, fill the Live Loops grid, the mix report and the arrange window menus).
- Local AI model: when notes are selected in the piano roll, the assistant edits only those notes.

## [0.5.1] - 2026-10-31

### Changed

- Assistant: everything the built-in assistant does for one request is now a single undo step. One Ctrl+Z, or
  **Undo AI edit**, puts the project back as it was before the request, and the Edit menu shows which request you
  are undoing.
- Assistant: **Undo AI edit** is active only when the last change came from the assistant. If you move a note or a
  fader after the answer, the button turns off and Ctrl+Z undoes your own change first.
- Arrange window: Ctrl+Shift+wheel makes the tracks taller or shorter around the track under the pointer. The View
  menu has Zoom In, Zoom Out, Zoom to Fit, Taller Tracks and Shorter Tracks.
- Arrange window: on short tracks the M/S/R buttons move next to the track name and the volume is set from the
  mixer.

### Fixed

- Arrange window: Ctrl+wheel zooms time around the pointer again, and Shift+wheel (or a horizontal wheel) scrolls
  time again.

## [0.5.0] - 2026-10-31

### Added

- MCP: new `get_selection` tool. An external agent (Claude Desktop, Claude Code and other MCP clients) can now see
  what you selected in the Ondina window: the track, the region, the selected notes, the playhead and the cycle
  range. Ask it to "quantize the notes I selected" and it works on exactly those notes.

### Changed

- Piano roll: when you open a region the view jumps to its notes (to C4 if the region is empty). Double-click the
  region in the arrange window to frame it again.
- Piano roll: the mouse wheel scrolls pitch at the same speed up and down; Shift+wheel scrolls time.
- Piano roll: Ctrl+wheel zooms time around the point under the pointer, and the new Ctrl+Shift+wheel zooms pitch
  (key height) around the note under the pointer.
- Piano roll: during playback the playhead follows the repeats of a looped region, and with Follow playhead on the
  view follows it too.

## [0.4.1] - 2026-10-31

### Changed

- Offline assistant: the Stop button now interrupts the request in progress right away. Changes already made stay
  and can be undone with Ctrl+Z.
- Offline assistant: in long conversations the oldest requests are forgotten so the chat always fits in the model's
  context. New chat starts over.

### Fixed

- Duplicate track now copies the track's automation too (volume, pan, sends and plug-in parameters). In the copy,
  plug-in automation moves the copy's own plug-ins, not the original track's.
- Changing a track's instrument or applying a patch removes the automation of the plug-ins that were replaced,
  instead of leaving lanes pointing at plug-ins that no longer exist. Volume, pan and sends stay, and one Ctrl+Z
  brings everything back.
- The bundled llama.cpp server started by Ondina is now closed when Ondina exits. A server you started yourself
  (Ollama, LM Studio, your own llama-server) is left running.

## [0.4.0] - 2026-10-31

### Added

- Arrange window: drag a track header up or down to reorder the tracks. A line shows where the track will land, the
  view scrolls when you reach the top or bottom edge, and Esc cancels. The mixer follows the same order.
- Arrange window: double-click a track's name to rename it in place. Enter or a click outside confirms, Esc cancels.
- Moving or renaming a track is undone with a single Ctrl+Z.

### Fixed

- Deleting a track also removes the sends that fed it and their automation, the automation of its plug-ins on other
  tracks, its Live Loops cells and the MIDI input routes to it, instead of leaving them attached to another track.
- Deleting a send or a plug-in also removes its automation lane, and the automation of the remaining sends stays on
  the right send. Undo brings everything back.

## [0.3.0] - 2026-10-31

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

[Unreleased]: https://github.com/marconardelli/ondina-daw/compare/v0.10.1...HEAD
[0.10.1]: https://github.com/marconardelli/ondina-daw/compare/v0.10.0...v0.10.1
[0.10.0]: https://github.com/marconardelli/ondina-daw/compare/v0.9.0...v0.10.0
[0.9.0]: https://github.com/marconardelli/ondina-daw/compare/v0.8.1...v0.9.0
[0.8.1]: https://github.com/marconardelli/ondina-daw/compare/v0.8.0...v0.8.1
[0.8.0]: https://github.com/marconardelli/ondina-daw/compare/v0.7.0...v0.8.0
[0.7.0]: https://github.com/marconardelli/ondina-daw/compare/v0.6.0...v0.7.0
[0.6.0]: https://github.com/marconardelli/ondina-daw/compare/v0.5.1...v0.6.0
[0.5.1]: https://github.com/marconardelli/ondina-daw/compare/v0.5.0...v0.5.1
[0.5.0]: https://github.com/marconardelli/ondina-daw/compare/v0.4.1...v0.5.0
[0.4.1]: https://github.com/marconardelli/ondina-daw/compare/v0.4.0...v0.4.1
[0.4.0]: https://github.com/marconardelli/ondina-daw/compare/v0.3.0...v0.4.0
[0.3.0]: https://github.com/marconardelli/ondina-daw/compare/v0.2.2...v0.3.0
[0.2.2]: https://github.com/marconardelli/ondina-daw/compare/v0.2.1...v0.2.2
[0.2.1]: https://github.com/marconardelli/ondina-daw/compare/v0.2.0...v0.2.1
[0.2.0]: https://github.com/marconardelli/ondina-daw/compare/v0.1.2...v0.2.0
[0.1.2]: https://github.com/marconardelli/ondina-daw/compare/v0.1.1...v0.1.2
[0.1.1]: https://github.com/marconardelli/ondina-daw/compare/v0.1.0...v0.1.1
[0.1.0]: https://github.com/marconardelli/ondina-daw/releases/tag/v0.1.0
