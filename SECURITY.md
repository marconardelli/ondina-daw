# Security policy

## Reporting a vulnerability

Please **do not open a public issue** for security problems. Use GitHub's private vulnerability reporting
("Security" tab → "Report a vulnerability") on
[marconardelli/aurora-daw](https://github.com/marconardelli/aurora-daw). You will get an answer as soon as possible;
please allow time for a fix before disclosing publicly.

## Threat model

Aurora is a desktop application. Its most sensitive surface is the **local MCP server**
(`http://127.0.0.1:3917/mcp` and the stdio bridge), which can do everything the GUI can do — including writing
files, loading native VST3 plug-ins and running ffmpeg. The server:

- listens on `127.0.0.1` only;
- rejects requests whose `Origin` header is not local (protection against DNS rebinding from web pages),
  including `Origin: null` (sent by sandboxed iframes and pages opened from files) unless `AURORA_MCP_TOKEN` is set;
- requires a bearer token when the `AURORA_MCP_TOKEN` environment variable is set.

Run Aurora with `--no-mcp` if you don't need external agents, and set `AURORA_MCP_TOKEN` if other software on your
machine could reach the port. A local HTML page opened from a file sends `Origin: null`: to let it call the server, set
`AURORA_MCP_TOKEN` and send the token as a bearer.

Movie files are read from local drives only: Aurora refuses movie paths that start with `-`, contain a URL scheme or
point to a network share, passes them to ffmpeg with the `file:` prefix and limits ffmpeg to the `file` protocol.
Frames are written as `.png`/`.jpg` and movie exports as `.mp4`, `.mov`, `.mkv` or `.m4v`.

Files written by MCP clients and the assistant are checked too. `save_project`, `export_midi`, `export_music_xml`,
`bounce`, `bounce_surround`, `bounce_binaural` and `export_adm` write only to a local drive and only with the
extension of their file type: `.aurora`; `.mid` or `.midi`; `.musicxml` or `.xml`; `.wav`. When the path has no
extension, Aurora adds it. Network shares, `\\?\` device paths, URLs, NTFS streams and any other extension are
refused before anything is written. `save_project` never overwrites an existing file that is not an Aurora project,
and it refuses to save over a project that was opened from a network share (save it from the app instead). Two limits
remain: a drive letter mapped to a network share counts as a local drive, and a tool can still overwrite an existing
file of its own type (for example a `.wav`). The app's own menus and dialogs are not restricted, so you can still save
or bounce to a NAS from the Save dialog.

## Known issues being worked on

This was found in an automated review of the first release and is tracked for fixing:

- **Native plug-in loading from any path.** `add_vst3` (and projects that reference VST3 plug-ins) can load a plug-in
  from an arbitrary path, including network paths. A VST3 plug-in is native code, so a malicious project file or an
  untrusted MCP client could run code. Planned fix: only load plug-ins found by the scanner in standard or
  user-configured folders, reject UNC/network paths, and ask the user before loading a plug-in requested by MCP/AI or
  referenced by an opened project.

Until these are fixed, only open projects you trust and only connect MCP clients you trust.
