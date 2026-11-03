# Changelog

All notable changes to Ondina (formerly Aurora) are documented in this file.

The format is based on [Keep a Changelog](https://keepachangelog.com/en/1.1.0/), and this project adheres to
[Semantic Versioning](https://semver.org/spec/v2.0.0.html).

## [Unreleased]

## [0.29.0] - 2026-11-03

### Added

- Drum Synth window: a pad for every sound of the kit, with its General MIDI note and colored by group. Click a pad
  to play it (lower is louder) and select it, right-click to select it silently. Under the pads only the selected
  sound's knobs show, plus Drive and Level for the whole kit, and the pads light up on what the track plays.
- Sampler window: the sample's waveform with its name, length and loop, the root note on a strip of keys (click,
  drag or wheel; double-click for C4), the amp envelope, and the root marked on the keyboard. Without a sample it
  offers **Load sample…**.
- New ready-made AI requests: *Shape the selected sound* (the request carries the selected pad and its
  parameters) and *Tune the sample to the song's key* (it carries the sample and its estimated pitch).

## [0.28.0] - 2026-11-03

### Added

- Saturation windows: Overdrive, Bitcrusher and Exciter draw the transfer curve (input against output level, with
  the area where the level rises or is squashed). The Overdrive shows its other modes faintly, the Bitcrusher draws
  its steps and the Exciter the band above its frequency.
- A dot follows the input level while the song plays, with input and output meters and the level change. Drag up
  and down on the curve for Drive, Harmonics or fewer bits (Shift for fine steps), use the wheel for Output,
  Downsampling or Mix, and double-click for the default. Every gesture is one undo step.
- New ready-made AI requests: *Warm it up* and *Add grit without getting louder* (Overdrive), *Lo-fi but still
  clear* (Bitcrusher) and *More air* (Exciter). The answer gives the values before and after and the level change
  in dB.

## [0.27.0] - 2026-11-03

### Added

- Delay windows: Stereo Delay and Tape Delay draw the repeats of a hit on the project's tempo grid (one, two or
  four bars), each as tall as its level down to -60 dB. The Stereo Delay shows left above and right below.
- Drag the first repeat sideways to change its time: it snaps to note values, dotted and triplet included, and
  Shift gives free values. Drag up or down for the feedback and use the wheel for the mix. **Sync** switches the
  Stereo Delay to times in ms. Every gesture is one undo step.
- The labels give each time as a note value and in ms at the tempo at the playhead, and `get_plugin` returns the
  times as note values too.
- New ready-made AI requests: *Dotted-eighth echo in time*, *Slapback for the vocal*, *Wide ping-pong* and *Dark
  dub echoes*. The request carries the project tempo and the answer names the times as note values.

## [0.26.0] - 2026-11-03

### Added

- Dynamics windows: Compressor, Limiter, Noise Gate and De-Esser draw the transfer curve (input against output
  level) with the threshold point to drag: sideways for the threshold, up and down for the ratio, ceiling,
  reduction or range, the wheel for knee, release, reduction or range, Shift for fine steps and double-click for
  the default.
- A dot on the curve follows the input level while the song plays, next to input, gain reduction and output
  meters with peak hold.
- The De-Esser window also shows the band it turns down, with its frequency to drag.

## [0.25.0] - 2026-11-03

### Added

- Synth windows: the Analog Synth window draws the shapes of both oscillators (click or wheel to change them; the
  square shows its pulse width), the filter curve (drag its point sideways for the cutoff, up and down for the
  resonance) and the amp and filter envelopes with points to drag. The FM Synth window draws the amp and
  modulation index envelopes. Every drag is one undo step.
- A two-octave keyboard at the bottom of every built-in instrument window: a note plays as long as you hold its
  key, the velocity follows where you click, and the octave moves with the buttons or the arrow keys.
- New ready-made AI requests: *Give me a bass for this song* and *A warm pad that opens slowly* (Analog Synth),
  *A short pluck* (FM Synth). The answer names the envelope stages and the filter it changed.

## [0.24.0] - 2026-11-03

### Added

- EQ window: Channel EQ and Dynamic EQ draw their response curve with a point per band. Drag a point for frequency
  and gain (Shift for fine steps), use the wheel for Q, double-click for the default and right-click to switch a
  band on or off. The Dynamic EQ shades how far each band can move with its Range.
- Live plug-in meters: input, output and gain reduction of every insert, measured by the engine without slowing
  the audio. MCP clients read them with `get_plugin` (new `meters` field) and `get_meters` (new `plugins` field).
- The plug-in AI bar sends the insert's live meters with the request, so the assistant knows how hard a compressor
  or limiter is working.
- Limiter with a true-peak ceiling, and true peak measured for real (4x oversampled) in `analyze_mix`, bounce and
  the mastering assistant.

### Fixed

- Shorter tool descriptions, so the local model keeps room for the conversation.
- EQ points at the edges of the graph are drawn and grabbed whole.

## [0.23.0] - 2026-11-03

### Added

- Plug-in windows: a click on an insert, instrument or MIDI FX slot, in the mixer or the inspector, opens that
  plug-in in a window of its own. The window is separate and resizable, can go to a second monitor and comes back
  where you left it, also after a restart. A second click brings it to the front, up to eight stay open together,
  and Esc or the X closes it.
- A common header in every plug-in window: the plug-in and its track, Bypass, the factory presets, **Default**
  (every parameter back to its default in one undo step) and the AI bar with its ready-made requests.
- Knobs instead of slider lists: every parameter is a knob, with a log scale for frequencies and long times, the
  value with its unit, Shift for fine steps, double-click for the default and right-click to type a value.
- VST3 plug-ins open their own editor with a compact Ondina window beside it (same header and AI bar, plus
  **Generic controls**). A VST3 without an editor opens Ondina's generic window with a knob per parameter,
  labelled with the unit the plug-in reports, and a filter by name.
- Settings → Display: *Open plug-ins in separate windows* (off keeps them inside the main window) and *Show the AI
  bar beside a plug-in's own window*.

### Changed

- The inspector shows the selected plug-in as one row (name, track, Bypass, Open) instead of its list of
  parameters.

### Fixed

- Ctrl+Shift+Z redoes in the main window too, and Space, Ctrl+Z and Ctrl+Shift+Z work inside plug-in windows.

## [0.22.0] - 2026-11-02

### Added

- Stereo Imager: a new utility effect that widens or narrows the stereo image in three bands (low, mid, high)
  split at two crossover frequencies. Widen pads, choirs and synths without widening the bass, put everything
  below the low crossover in mono with Mono Bass, shift the image with Balance and listen to Mid or Side only to
  find problems. Width changes only the Side, so the mix keeps its level in mono; no latency. It works on tracks,
  buses and the master and comes with three presets: Mono Low End, Wide Synth and Master Width Subtle. MCP clients
  and the assistant add it with `add_effect` (`kind: "stereo_imager"`).
- The Stereo Imager's AI bar offers three ready-made requests: "Mono the low end", "Wider without losing mono"
  and "Check mono compatibility". The assistant reads the stereo correlation before and after, and tells you
  what it changed or why nothing needed changing.
- More AI providers for the assistant: besides Claude and the local model, Settings → Provider now offers OpenAI,
  Google Gemini and Kimi (Moonshot). Each has its own API key and model (defaults `gpt-6-astra`,
  `gemini-3.6-flash` and `kimi-k3.1-pro`); the keys can also come from `OPENAI_API_KEY`, `GEMINI_API_KEY` /
  `GOOGLE_API_KEY` and `MOONSHOT_API_KEY`. These models get all the tools, like Claude, and work with the quick
  actions, the ✨ buttons and the plug-ins' AI bar; one request is still one undo step.

## [0.21.0] - 2026-11-02

### Added

- AI bar on every plug-in: at the bottom of the parameters of any built-in instrument, effect or MIDI effect and of
  any VST3 plug-in, type what you want ("more air on the vocal", "remove the boom below 200 Hz") or pick one of two
  or three ready-made requests for that plug-in. The assistant gets the plug-in, its track, the rest of the chain
  and all the parameter values, changes only that plug-in, and explains in a few lines what it changed and why.
  Everything it did for one request is undone with a single Ctrl+Z. Works with Claude and with the local model.
- Dynamic EQ: a new effect with four bands (Bell, Low Shelf, High Shelf) that cut or boost only while their
  frequency range goes over the threshold, up to the chosen range, with attack, release and output gain. It tames
  a booming bass or a vocal resonance only when it shows up, adds no latency and comes with three presets: Tame
  Resonance, Vocal Mud Control and Bass Boom Control. MCP clients and the assistant add it with `add_effect`
  (`kind: "dynamic_eq"`), and its AI bar offers ready-made requests for mud, harsh resonances and vocal air.

### Changed

- The AI bar replaces the "AI: design a sound" button of the instrument editors; *Design a sound* is now one of its
  ready-made requests.
- `get_plugin` also returns the plug-in's description, the default of each parameter, and the track, slot and
  position in the chain where the plug-in sits.

## [0.20.1] - 2026-11-02

### Fixed

- When the audio output stops working on its own (headphones unplugged or disabled), playback stops where it was
  and sound moves to the same device if it comes back, else to the default output, else to the null device. The
  status bar says what happened, and `get_transport` reports `output_device` and `device_notice`. Before, the
  transport stayed in Play with the playhead frozen and no sound.
- `load_movie`, `extract_movie_frame` and `export_movie` called from MCP clients or the assistant check the movie
  path where Windows resolves it, as every other file they make Ondina read: a path that leads to a network share
  is refused.

## [0.20.0] - 2026-11-01

A new interface for the whole app: same features and the same MCP tools, laid out and drawn like a professional
studio.

### Added

- New visual theme: graphite surfaces in layers with one water-blue accent for selection and focus, separate colors
  for play, record, solo, mute, monitoring and the assistant, and buttons that look different when hovered and
  when switched on. The app bundles the Inter and JetBrains Mono fonts (SIL Open Font License) and draws its own
  vector icons, so no more empty squares or emoji.
- Control bar: Library and Inspector buttons, transport drawn with the new icons (Play green while playing, Record
  red, Cycle amber), and a display with fixed-width fields for position, time, tempo, time signature and key, so
  nothing moves during playback. The tempo field shows the tempo in force at the playhead, including tempo
  changes. On a narrow window the bar drops lower-priority items one step at a time instead of overlapping.
- One control chooses the bottom panel: Editor, Mixer, Clip Grid or Hide.
- Resizable panels: the side panel, the assistant and the bottom panel are resized by dragging a splitter with a
  grip and closed from their header or the control bar. Sizes and which panels are open are kept for the next
  start; the Mixer keeps its own height, tall enough for whole strips.
- Inspector channel strip: track card (color, name, type, instrument, options), instrument, MIDI effects, audio
  effects (bypassed ones struck through), sends, output, pan knob, fader with dB scale, stereo meter and Mute /
  Solo. Presets, plug-in parameters, region, surround, project and master settings are in the menus and the
  sections below.
- Assistant panel with a status pill (Claude, local model or offline). With no model connected it explains how to
  connect one, and the quick actions, Send and every other AI button in the app are disabled with the reason on
  hover.
- Mixer header with a filter (All / Tracks / Buses), the loudness and true peak of the last analysis and an
  "Analyze mix" button. Strips all have the same size, with input, effects and sends, pan with its label, fader
  with a dB scale, stereo meter with peak hold and clip light (click to clear), value and peak boxes and a
  numbered footer; on a short panel they drop effect and send rows so the fader always stays in view. The Master
  is pinned on the right with its loudness box.
- Bounce dialog (File > Bounce to WAV, Ctrl+B) with range, bit depth and loudness target.
- Library patch search, and empty states with a hint for an empty project, the editor with no region, the Clip
  Grid and a search with no match.

### Changed

- Track headers: color bar, number, name and type, larger Mute / Solo / Record buttons in their own colors, a slim
  volume slider with its dB value (double-click for 0 dB) and a quiet stereo meter; the selected track stands out.
- Arrange window: bar numbers in monospace, amber cycle band, markers and chords as rounded chips, regions with
  rounded corners, a border in the track color and a white ring when selected; drum and step-sequencer regions
  show their hits on separate rows, audio waveforms are filled, the playhead has a triangle head.
- New tracks take the twelve colors of the new palette in turn (existing projects keep their colors).
- Piano roll: tools on one line with a "⋯" menu for the rest, a keyboard with clear black keys and every C
  labelled, rows shaded by key with a line at each C, song bar numbers, notes in the track color, velocity stems.
  The controller lane starts closed and has its own header, so about 18 rows show in the default panel; a region
  opened in a small panel shows all its notes.
- Step sequencer: visible off cells, groups of four, distinct ghost / normal / accent steps, the playing step
  highlighted, and per-row M, sound and menu buttons.
- The Score is a framed sheet on the dark background and numbers bars from the song.
- Settings apply and save as you change them; the "Save settings" button is gone.
- Dialogs, menus and tooltips share one look: dimmed backdrop under modal dialogs, a filled button for the main
  action, shortcuts right-aligned in menus, tooltips after a short pause.
- "Live Loops" is now called "Clip Grid", "Flex Time" "Time Warp" and "Flex Pitch" "Pitch Edit" in the app, the
  guides and the MCP tool descriptions. MCP tool names, parameters and project files are unchanged.

### Fixed

- Clicking a track header on another track drops the selected region, so the Inspector never shows a region of a
  different track.
- A track scrolled half out of view no longer swallows clicks on the ruler, the marker lane and the chord lane.
- Opening the Editor while the Clip Grid is shown works again.
- The Mix Analysis window shows rounded numbers with units ("-17.8 LUFS").
- Meter peak markers no longer rest on the floor of a silent meter after playback stops.

## [0.19.2] - 2026-11-01

### Changed

- `new_project` and `open_project` called by an MCP client or the assistant no longer replace a project with unsaved
  changes: they return an error unless the new optional `discard_unsaved` parameter is `true`. The window keeps
  asking Save / Don't Save / Cancel as before.
- The session bassist, keyboard player, arpeggiator and melody generator repeat the chord track after its last chord,
  so every bar asked for gets notes (an 8-bar part over a 4-bar progression plays it twice), as the drummer already
  did.
- MCP clients and the assistant can no longer read files on network shares (`\\server\share`, `//server`,
  `\\?\UNC\`) with `import_audio`, `set_sampler_sample`, `import_midi` and `open_project`; audio files a project lists
  on a share are reported in `blocked_audio_files` instead of being read. Opening and importing from the app is not
  restricted.
- Limits on command input, with a clear error instead of a crash or a hang: a render (`bounce`, `analyze_mix`,
  `bounce_in_place`, surround, binaural, ADM and movie export) covers at most 60 minutes of the song; a pattern has at
  most 65536 steps; the Live Loops grid has at most 256 scenes; a generator writes at most 512 bars, starts at a beat
  from 0 to 1000000 and follows chords of at least 1/4 beat.
- `set_pattern_swing` refuses a value outside 0..1 and returns the swing; `set_pattern_row` refuses a string whose
  number of steps differs from the pattern, instead of correcting the input without saying so.

### Fixed

- A time signature of 0 beats per bar (from `new_project`, a MIDI file or a project file) and a project file with an
  empty tempo map no longer stop the audio until Ondina is restarted. Project files are checked when they are opened:
  invalid time signatures, tempo maps and patterns are refused, and tempos, cycle ranges and automation values out of
  range are put back in range.
- Automation values are kept in the range of their target (for example -96..6 dB for volume): a volume point of
  +1000 dB no longer turns into noise or silence that stays in the bus reverbs, and values that are not numbers are
  refused.
- MCP tool annotations: only tools that just read are marked read-only. `save_project`, `bounce` and the exports,
  `undo`/`redo`, `scan_vst3`, `set_ai_settings`, the transport and the device tools were marked read-only, so a client
  could run them without asking.
- Tool descriptions no longer promise master automation on `track_id 0` or automation of VST3 parameters, which
  Ondina does not support.

## [0.19.1] - 2026-11-01

### Changed

- The sustain pedal now works on the Tonewheel Organ, Plucked String and Sampler too, not only on the Analog Synth
  and FM Synth: notes keep ringing while the pedal is down. A note played again with the pedal down restarts on the
  same voice, and a key still held when the pedal comes up keeps sounding until you release it. The Drum Synth
  ignores the pedal, and the controller lane says so.

### Fixed

- A right or middle click while dragging regions, notes or steps no longer drops the drag early: the arrange window,
  the piano roll and the step sequencer finish the move only when you let go of the left button.

## [0.19.0] - 2026-11-01

### Added

- Piano roll: **Ctrl+D** with notes selected repeats them right after themselves instead of duplicating the whole
  region. The copies start where the selection ends (rounded up to the snap grid, at most a beat; exact with snap
  off), notes that would fall past the region end are dropped, and the copies become the new selection, so
  pressing Ctrl+D again repeats the phrase again. Each repeat is a single Ctrl+Z. Also in **Edit > Repeat Selected
  Notes**. Without selected notes, Ctrl+D still duplicates the region.

### Changed

- Session players from the track menu (right-click an empty spot on an instrument track) now fill the cycle when
  it is on and the playhead is inside it; otherwise they start at the bar under the playhead and last the
  **Length** chosen at the top of the menu (1, 2, 4, 8 or 16 bars). The menu entries show the length that will be
  used.
- Session Drummer: choose the **Intensity** (Soft, Medium, Hard) and add a **Fill at the end** of parts of two bars
  or more. Length, intensity and fill are remembered while Ondina is open.
- After generating a part, the status bar shows the part, its length and the seed, for example
  "Drums (Pop), 8 bars, seed 1234". Each generation is a single Ctrl+Z.
- The `generate_drums`, `generate_bass`, `generate_chords`, `generate_arpeggio` and `generate_melody` tools return
  the seed they used, with the start and length in bars: the same seed with the same settings gives the same part
  again, a different seed gives a variation.

## [0.18.0] - 2026-11-01

### Added

- Step sequencer: each row now has an **M** button to mute it. A muted row stays visible, greyed out, keeps its
  steps and can still be edited, but it does not play and is left out of the region thumbnail and the MIDI export.
- Step sequencer: the row-name menu gains **Mute** / **Unmute**, **Rename...** (type the new name in place, Enter
  confirms, Esc cancels), **Sound** (pick one of the 25 Drum Kit sounds: name and note change together, steps and
  mute stay) and **Delete row** (the rows below move up; an empty pattern can be filled again with **+ Row**).
  Every change is a single Ctrl+Z.
- The assistant and MCP clients can do the same with the new `update_pattern_row` (name, sound, mute) and
  `remove_pattern_row` tools.

### Changed

- Bounce to a loudness target: **File > Bounce loudness** chooses Off, Peak -1 dB, -14 LUFS (streaming), -16 LUFS
  or -23 LUFS (broadcast). With a LUFS target Ondina measures the mix and applies one gain to the whole song, with
  no limiter, keeping peaks under -1 dBTP. If the mix is too dynamic to get there, the status bar says so: use the
  Mastering Assistant to add a limiter.
- The `bounce` tool takes `normalize_lufs` (-30 to -6) and `true_peak_ceiling_db` (-9 to 0, default -1) and
  reports the gain applied, the loudness and true peak after, and whether the target was reached.

## [0.17.0] - 2026-11-01

### Added

- **Detect from MIDI**, the first entry of the key menu in the transport bar, finds the key of your MIDI parts and
  sets it as the project key. It looks at the unmuted MIDI regions on instrument tracks (Drum Kit tracks and step
  patterns are left out), and long, loud notes count more than short, soft ones. The status bar shows the key, how
  confident the guess is and the closest alternatives: relative keys like C major and A minor share the same notes,
  so pick the other one from the menu if the part sounds minor. Only major and minor keys are proposed, and Ctrl+Z
  brings back the previous key.
- The assistant and MCP clients can do the same with the new `detect_key` tool, on one region or the whole
  project; it changes the project key only when asked to (`apply: true`).

### Changed

- Step sequencer: paint steps by dragging across the grid, even across several rows. A stroke turns cells on if the
  first one was off and off if it was on, cells you skip with a fast move are filled in, Esc cancels the stroke
  while the button is still held, and the whole stroke is a single Ctrl+Z.
- Step sequencer: right-click a row name for a menu with **Preview**, **Clear row**, **Every step**,
  **Every 2nd step**, **Every 4th step** and **Offbeats**.
- Step sequencer: **+ Row** under the grid adds a drum sound, chosen from the 25 Drum Kit sounds.

## [0.16.0] - 2026-11-01

### Added

- Piano roll: new **Length** menu for the selected notes (or the whole region when nothing is selected).
  *Legato* stretches each note to the start of the next one (notes played together, even a slightly loose chord,
  end together), *Remove overlaps* cuts a note where the same pitch starts again, *Fixed* gives every note the
  current *Note length*, and *Halve* / *Double* shorten or lengthen them. Starts and pitches never move, and each
  change is a single Ctrl+Z.
- Piano roll: new **Velocity** menu for the selected notes: set them all to 64, 80, 100 or 127 (or type a value
  from 1 to 127), or scale them to 90% or 110%.
- The assistant (with Claude) and MCP clients can change note lengths with the new `set_note_lengths` tool, for
  example "make the bass legato".
- New **De-Esser** effect (Dynamics group) that turns down harsh S and T sounds on vocals only while they happen.
  Set the Frequency where the sibilance starts, the Threshold and the maximum reduction (Range); *Split* mode turns
  down only the high band, *Wideband* the whole signal, and *Listen* lets you hear the band being detected. Below
  the threshold the sound passes through unchanged, with no added latency. Two presets: *Vocal De-Ess* and
  *Soft De-Ess*.
- Add it from the mixer insert slots or the inspector; the assistant and MCP clients add it with `add_effect`
  (`kind: "de_esser"`), or just ask the assistant to "de-ess the vocal".

## [0.15.2] - 2026-11-01

### Changed

- The metronome click now has its own level: right-click the metronome button in the transport bar to set it
  (-60 to 0 dB, -9.1 dB by default) or reset it. The level changes right away, even during playback, and a whole
  slider drag is undone with a single Ctrl+Z.
- New *Only while recording* option in the same menu: the click plays only while you record, not during normal
  playback. The count-in always clicks, at the same level. The button turns dark blue while the click is waiting
  for a recording.
- The assistant and MCP clients can set the click level and the recording-only mode with `set_metronome`;
  `get_transport` and `get_project` report them. Projects saved before this version click at the usual level.
- Saving never writes over a file that is not Ondina's: the temporary file used during a save always has a new,
  unique name, and stale temporary files from an interrupted save are cleaned up at a later save.
- A file or folder called `<name>.ondina.bak` that is not an Ondina project is left alone: the project is still
  saved and the status bar says the backup copy was not written.
- If Windows does not let Ondina read `settings.json` at startup (for example an antivirus or a cloud sync is
  holding it), Ondina starts with the defaults but no longer writes over your settings file until you press
  *Save settings*; the Settings window points this out.

## [0.15.1] - 2026-11-01

### Changed

- Saving can no longer leave you with a broken project: Ondina writes the project to a temporary file, makes sure it
  is fully on disk and only then puts it in place. A full disk, a crash or a power cut during the save keeps the
  project you had, and the status bar says why the save failed.
- The previous version of the project is kept next to it as `<name>.ondina.bak` (one copy, replaced at every save,
  also when the assistant or an MCP client saves). To go back to it, copy it and rename the copy to end in
  `.ondina`. You can turn the copy off in Settings > Autosave, *Keep a backup of the previous save*.
- Settings are saved the same safe way. A settings file that cannot be read at startup is kept as
  `settings.json.broken` (so your API key, VST3 folders and recent files can be recovered from it), Ondina starts
  with the defaults and the status bar tells you.
- Mixer and inspector show third-party plug-ins by their own name instead of "?"; hover a slot to read a name that
  is too long to fit.
- In the mixer, double-click a VST3 insert or VST3 instrument to open its window (or right-click an insert, Open
  plug-in window). A single click still selects it.
- Plug-ins that are not loaded are struck through and greyed out in the mixer and the inspector; hover them to see
  why.

## [0.15.0] - 2026-11-01

### Added

- Piano roll: the new Harmony menu adds a second voice to the selected notes (or to the whole region when nothing is
  selected): a 3rd, 5th or 6th above, a 3rd or 6th below, or an octave above or below. Thirds, fifths and sixths
  follow the project key, so the harmony stays in key (in C major the 3rd above C is E, above B it is D); octaves
  are always 12 semitones.
- The harmony notes start and end with the originals, play a little softer (85% velocity), never double a note that
  is already there and become the new selection, so you can move them with the arrow keys or delete them. One
  Ctrl+Z removes the whole harmony.
- The AI assistant (including the offline model) and MCP clients can add a harmony voice too, for example "add a
  third above the selected notes".
- Automation in the arrange window: press A (or View > Show Automation) to see each track's automation drawn over
  its regions. Choose the lane (Volume, Pan, a send or a plug-in parameter it already uses) from the selector in the
  track header or from the header's right-click menu, Automation lane.
- Click below a region's name bar to add a point (snapped to the grid, hold Alt to place it freely), drag a point to
  move it while a box shows its value, and double-click or right-click a point to delete it. Esc during a drag puts
  the point back, and every edit is a single undo step.
- An empty lane shows the channel's current value as a dashed line. Regions can still be selected, moved, resized
  and faded from their name bar while automation is shown, and what the assistant writes appears in the same lanes
  and can be fixed with the mouse.

## [0.14.0] - 2026-11-01

### Added

- Autosave and crash recovery: while you work, Ondina writes a copy of unsaved changes every 3 minutes in the
  background, without stopping playback or recording. The copies go to %APPDATA%\OndinaDAW\Autosave and are removed
  when you save, open or start another project, or close Ondina normally.
- If Ondina did not close normally (a plug-in or audio driver crash, or it was ended from Task Manager), the next
  start shows Recover Unsaved Work with the copies it found: Recover opens one as unsaved work and the first Save
  proposes the original file, Discard and Discard All delete copies, Decide Later keeps them for the next start.
- Settings > Autosave sets the minutes between copies (0 to 60); 0 turns autosave off. Audio files are not copied,
  and a recording still in progress is not part of the copy.

## [0.13.0] - 2026-11-01

### Added

- Arrange window: copy, cut and paste whole regions. Select a region and press Ctrl+C or Ctrl+X, then Ctrl+V pastes
  a copy at the playhead (snapped back to the grid) on the selected track, or on the track it came from when no
  track is selected. The same actions are in the Edit menu (Copy Region, Cut Region, Paste Region). The copy keeps
  everything: notes, controllers, lyrics, pattern steps, audio gain, fades and reverse, name, color and loops. Each
  cut or paste is one undo step, and MIDI or pattern regions can be pasted into another open project.
- For MCP clients and the assistant, a new `paste_region` tool places a copy of a region on any track.

### Changed

- Offline assistant: the local model can now humanize the selected notes, snap them to the project key and add
  harmony, so requests like "humanize the selected notes" work without Claude. It gets 45 tools instead of 43.
- The line under the assistant panel title shows which engine answers and how many tools it gets.
- The local model is warned that Quantize reorders notes, so follow-up edits on the same selection hit the right
  notes.

## [0.12.0] - 2026-10-31

### Added

- Drag and drop from Explorer: drop audio files (wav, aiff, flac, mp3, ogg, m4a, caf) on an audio track and they
  land at that point, snapped to the grid; below the last track or on an instrument or bus track they go on a new
  audio track. While you drag, a line and a label show the file name, the track and where it will land. Several
  files go one after the other, up to 8 per drop, and one Ctrl+Z removes the whole drop. A MIDI file is imported
  at the beat under the pointer, and a project file opens (asking to save unsaved changes first). Files dropped
  outside the arrangement go to the playhead.

### Changed

- Piano roll: a note drawn with a click is selected right away, so the arrow keys, Alt+Left/Right, Delete and
  Ctrl+C/X act on it at once. With Ctrl or Shift the new note is added to the selection, and a click past the end
  of the region clears it.
- For MCP clients and the assistant, `add_notes` now returns the indices of the added notes, ready to pass to
  transpose, quantize, humanize, velocity or remove tools.

## [0.11.0] - 2026-10-31

### Added

- Track keys: Up and Down select the track above or below and scroll it into view, M mutes the selected track and
  S solos it (press again to undo), and Shift+M adds a marker at the playhead, even during playback. Each one
  undoes with a single Ctrl+Z, and the same actions are in the Track and Edit menus. With notes selected in the
  piano roll the arrows still move the notes; with Musical Typing on, S and M play notes as before.

### Changed

- The Bassist, Keys and Melody session players follow the project time signature: their rhythms start again at
  every bar. 3/4 and 6/8 get rhythms of their own, and in 6/8 the melody accents the first and fourth eighth note.
  In other meters such as 5/4 or 7/8 the 4/4 rhythm repeats every four beats and stops at the bar line. In 4/4,
  with the same seed, the parts are identical to previous versions.

## [0.10.2] - 2026-10-31

### Changed

- When an MCP client or the assistant opens a project, VST3 plug-ins stored outside the standard VST3 folders and
  the folders added in the library are no longer loaded. They stay in the project and in the saved file, but they
  do not play, are skipped in bounces and their editor window does not open; `open_project` lists them in
  `blocked_plugins`. They load as usual when you open the project yourself from the app.

### Fixed

- The transport reports the right position and state immediately after you move the playhead, press Play or
  press Stop: it no longer jumps back to the old position for a moment.

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

[Unreleased]: https://github.com/marconardelli/ondina-daw/compare/v0.29.0...HEAD
[0.29.0]: https://github.com/marconardelli/ondina-daw/compare/v0.28.0...v0.29.0
[0.28.0]: https://github.com/marconardelli/ondina-daw/compare/v0.27.0...v0.28.0
[0.27.0]: https://github.com/marconardelli/ondina-daw/compare/v0.26.0...v0.27.0
[0.26.0]: https://github.com/marconardelli/ondina-daw/compare/v0.25.0...v0.26.0
[0.25.0]: https://github.com/marconardelli/ondina-daw/compare/v0.24.0...v0.25.0
[0.24.0]: https://github.com/marconardelli/ondina-daw/compare/v0.23.0...v0.24.0
[0.23.0]: https://github.com/marconardelli/ondina-daw/compare/v0.22.0...v0.23.0
[0.22.0]: https://github.com/marconardelli/ondina-daw/compare/v0.21.0...v0.22.0
[0.21.0]: https://github.com/marconardelli/ondina-daw/compare/v0.20.1...v0.21.0
[0.20.1]: https://github.com/marconardelli/ondina-daw/compare/v0.20.0...v0.20.1
[0.20.0]: https://github.com/marconardelli/ondina-daw/compare/v0.19.2...v0.20.0
[0.19.2]: https://github.com/marconardelli/ondina-daw/compare/v0.19.1...v0.19.2
[0.19.1]: https://github.com/marconardelli/ondina-daw/compare/v0.19.0...v0.19.1
[0.19.0]: https://github.com/marconardelli/ondina-daw/compare/v0.18.0...v0.19.0
[0.18.0]: https://github.com/marconardelli/ondina-daw/compare/v0.17.0...v0.18.0
[0.17.0]: https://github.com/marconardelli/ondina-daw/compare/v0.16.0...v0.17.0
[0.16.0]: https://github.com/marconardelli/ondina-daw/compare/v0.15.2...v0.16.0
[0.15.2]: https://github.com/marconardelli/ondina-daw/compare/v0.15.1...v0.15.2
[0.15.1]: https://github.com/marconardelli/ondina-daw/compare/v0.15.0...v0.15.1
[0.15.0]: https://github.com/marconardelli/ondina-daw/compare/v0.14.0...v0.15.0
[0.14.0]: https://github.com/marconardelli/ondina-daw/compare/v0.13.0...v0.14.0
[0.13.0]: https://github.com/marconardelli/ondina-daw/compare/v0.12.0...v0.13.0
[0.12.0]: https://github.com/marconardelli/ondina-daw/compare/v0.11.0...v0.12.0
[0.11.0]: https://github.com/marconardelli/ondina-daw/compare/v0.10.2...v0.11.0
[0.10.2]: https://github.com/marconardelli/ondina-daw/compare/v0.10.1...v0.10.2
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
