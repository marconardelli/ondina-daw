## What and why

<!-- What does this change do, and which problem does it solve? Link the issue: Fixes #123 -->

## How it was tested

- [ ] `cargo test --workspace` passes
- [ ] `cargo clippy --workspace --all-targets` shows no new warnings in the touched files
- [ ] Tried in the app (describe: by ear for audio, by hand for GUI)

## Checklist

- [ ] Project changes go through a `Command` (undoable, available over MCP)
- [ ] No MCP tool removed or renamed; `MCP_TOOLS.md` regenerated if tools changed
- [ ] Older `.aurora` projects still open (`#[serde(default)]` on new fields)
- [ ] Nothing allocates, locks or does I/O on the audio thread
- [ ] No unrelated reformatting

## Screenshots / recordings

<!-- For UI changes -->
