# Contributing to Aurora

Thanks for helping make Aurora better! This guide explains how to set up the project, how the code is organised,
and how to land a change. If anything here is unclear, open an issue — improving this document is a contribution
too.

## Ways to contribute

- **Report a bug** — use the *Bug report* issue template. Include steps to reproduce, what you expected, what
  happened, your Windows version and audio device, and if possible a small `.aurora` project.
- **Propose a feature** — use the *Feature request* template. Describe the musical problem first, then the idea.
- **Pick an issue** — issues labelled `good first issue` are small and well scoped; `help wanted` ones are bigger.
  Comment on the issue before starting so work isn't duplicated.
- **Improve docs and examples** — README, user guides in `docs/`, example projects, doc comments of commands (they
  become the MCP tool descriptions).

## Development setup

1. Install Rust stable with the MSVC toolchain (`rustup default stable-x86_64-pc-windows-msvc`) and the Visual
   Studio Build Tools ("Desktop development with C++").
2. Clone and build:
   ```powershell
   git clone https://github.com/<you>/aurora-daw.git
   cd aurora-daw
   cargo build
   cargo test --workspace
   ```
3. Optional tools (not in the repository) go in `tools\` at the repository root: ffmpeg, llama.cpp + a `.gguf`
   model, ONNX models, test VST3 plug-ins. See the README. Tests that need them skip themselves when they are absent.

Run the app with `cargo run --release -p aurora-app --bin aurora` (add `-- --no-audio` on machines without an
audio device).

## How the code is organised

| Crate | What lives there |
|---|---|
| `crates/core` | `model.rs` (project document), `command.rs` (the `Command` enum = the public API), `ops.rs` / `ops2.rs` (command implementations on the model), `generate.rs` (session players), `theory.rs`, `tempo.rs` |
| `crates/dsp` | instruments (`synth.rs`, `drums.rs`), effects (`effects.rs`, `midifx.rs`), audio operations, spatial audio, VST3 host |
| `crates/engine` | real-time engine (`realtime.rs`), renderer, offline bounce/export, analysis, ADM |
| `crates/studio` | `Session` (command execution, undo/redo), MCP server (`mcp.rs`), AI assistant (`ai.rs`, `local_ai.rs`) |
| `crates/app` | egui GUI (`app.rs`, `arrange.rs`, `editors.rs`, `mixer.rs`, `inspector.rs`, `extra_ui.rs`) and `aurora-mcp` |

The central idea: **GUI, MCP and AI all go through `Session::execute(Command)`**.

## Common tasks

### Add a command (and therefore an MCP tool)

1. Add a variant to `Command` in `crates/core/src/command.rs`. Its doc comment becomes the MCP tool description,
   so write it for an agent that has never seen Aurora: what it does, units, defaults.
2. Implement it in `crates/core/src/ops*.rs` (pure project edits) or `crates/studio/src/session*.rs` (when it needs
   the engine, files or the audio pool). Make it undoable.
3. Add a test in `crates/studio/tests/features.rs` (or next to the code). The test that calls every MCP tool will
   also pick it up.
4. Use it from the GUI with `self.cmd(Command::...)` (queued) or `self.exec(...)` (immediate, returns the result).
5. Regenerate the reference: `cargo run -q -p aurora-app --bin aurora-mcp -- --list-tools > MCP_TOOLS.md`.

### Add an instrument or effect

1. Describe it in the plug-in registry, `registry()` in `crates/core/src/plugins.rs`: kind, name, category and
   parameters (id, range, default, unit). GUI, MCP and the AI read this registry, so the plug-in shows up in the
   inspector and in `list_plugins` automatically.
2. Implement the processor in `crates/dsp` next to the existing ones and create it in `create_instrument`,
   `create_effect` or `create_midi_effect` in `crates/dsp/src/lib.rs`.
3. Keep `process` real-time safe: no allocations, locks, I/O or logging. Pre-allocate in the constructor.
4. Test it numerically on buffers: output level, frequency content where relevant, no NaN, no runaway gain.

### Change the GUI

- The GUI is immediate-mode egui 0.29. **In egui the last widget registered with `ui.interact` is on top and gets
  the click**: register background areas (grids, lanes) *before* the items drawn on them (notes, regions).
- Keep long work off the GUI thread and never hold the session lock for long.
- Put logic that can be tested without a window (hit testing, geometry, selection rules) in plain functions and
  test those.

## Coding conventions

- Rust edition 2021. Follow the style of the surrounding code; **don't reformat whole files** — keep diffs focused
  on your change so they are easy to review.
- No new clippy warnings in the files you touch (`cargo clippy --workspace --all-targets`).
- Don't add `#[allow(...)]` or `#[ignore]` to make a problem go away, and don't delete or rename existing tests.
- Comments explain *why*, not *what*. Code, comments and UI text are in English.
- New model fields need `#[serde(default)]` so older `.aurora` files still open.
- MCP tools are a public API: never remove or rename one; add optional parameters instead of changing existing ones.
- Risky features should be switchable with a setting.

## Commits and pull requests

1. Fork the repository and create a branch from `main` (`feat/piano-roll-zoom`, `fix/reverb-denormals`, …).
2. Make small commits using [Conventional Commits](https://www.conventionalcommits.org/) with the crate as scope:
   `feat(app): ...`, `fix(engine): ...`, `feat(dsp): ...`, `docs: ...`, `test(studio): ...`.
3. Before opening the PR run:
   ```powershell
   cargo test --workspace
   cargo clippy --workspace --all-targets
   ```
4. Open a pull request using the template: what changed, why, how you tested it (including by ear or by hand for
   audio and GUI changes), and screenshots or short recordings for UI changes.
5. A maintainer reviews for correctness, real-time safety, undo/MCP consistency, project compatibility and tests.
   Expect questions — they are part of making the change solid.

Releases, version numbers and `CHANGELOG.md` are handled by the maintainer when changes land, so you don't need to
bump the version in your PR.

## Code of conduct

Be kind, patient and constructive. Assume good intent, critique code rather than people, and help newcomers.
Harassment or discrimination of any kind is not tolerated; the maintainer may remove comments or contributors that
break these rules.

## License

By contributing, you agree that your contributions are licensed under the [MIT License](LICENSE).
