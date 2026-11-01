# Security policy

## Reporting a vulnerability

Please **do not open a public issue** for security problems. Use GitHub's private vulnerability reporting
("Security" tab → "Report a vulnerability") on
[marconardelli/ondina-daw](https://github.com/marconardelli/ondina-daw). You will get an answer as soon as possible;
please allow time for a fix before disclosing publicly.

## Threat model

Ondina is a desktop application. Its most sensitive surface is the **local MCP server**
(`http://127.0.0.1:3917/mcp` and the stdio bridge), which can do everything the GUI can do — including writing
files, loading native VST3 plug-ins and running ffmpeg. The server:

- listens on `127.0.0.1` only;
- rejects requests whose `Origin` header is not local (protection against DNS rebinding from web pages),
  including `Origin: null` (sent by sandboxed iframes and pages opened from files) unless `ONDINA_MCP_TOKEN` is set;
- requires a bearer token when the `ONDINA_MCP_TOKEN` environment variable is set.

Run Ondina with `--no-mcp` if you don't need external agents, and set `ONDINA_MCP_TOKEN` if other software on your
machine could reach the port. A local HTML page opened from a file sends `Origin: null`: to let it call the server, set
`ONDINA_MCP_TOKEN` and send the token as a bearer.

Movie files are read from local drives only: Ondina refuses movie paths that start with `-`, contain a URL scheme or
point to a network share, passes them to ffmpeg with the `file:` prefix and limits ffmpeg to the `file` protocol.
Frames are written as `.png`/`.jpg` and movie exports as `.mp4`, `.mov`, `.mkv` or `.m4v`.

Files written by MCP clients and the assistant are checked too. `save_project`, `export_midi`, `export_music_xml`,
`bounce`, `bounce_surround`, `bounce_binaural` and `export_adm` write only to a local drive and only with the
extension of their file type: `.ondina` (or the older `.aurora`); `.mid` or `.midi`; `.musicxml` or `.xml`; `.wav`. When the path has no
extension, Ondina adds it. Network shares, `\\?\` device paths, URLs, NTFS streams and any other extension are
refused before anything is written. `save_project` never overwrites an existing file that is not an Ondina project,
and neither do the files it writes next to it (the temporary file always has a new name, and the `.bak` copy replaces
only an earlier save); it also refuses to save over a project that was opened from a network share (save it from the app instead). Two limits
remain: a drive letter mapped to a network share counts as a local drive, and a tool can still overwrite an existing
file of its own type (for example a `.wav`). The app's own menus and dialogs are not restricted, so you can still save
or bounce to a NAS from the Save dialog.

VST3 plug-ins are native code, so Ondina loads them only from a `.vst3` bundle on a local drive. `add_vst3`, projects
that reference plug-ins, the native editor windows and the extra folders of `scan_vst3` all go through the same
check: network shares, `\\?\` and `\\.\` device paths, URLs, NTFS streams and files that are not `.vst3` are refused
before anything is loaded, and the path is checked again after Windows resolves links and junctions; a plug-in path
that Windows cannot resolve for any reason other than not existing is refused. An extra scan folder must itself
resolve to a local drive (a drive letter mapped to a share does not), and so must every link or junction inside it,
because the scanner loads every plug-in it finds; a folder that cannot be resolved is skipped. A project that
references a refused plug-in still opens: the plug-in stays in the project but is not loaded (its track is silent
there), and `open_project` lists it in `blocked_plugins`. This applies to the app's own menus too, because the risk
comes from the project file: plug-ins kept on a NAS must be copied to a local folder.

MCP clients and the assistant can load plug-ins only from the standard VST3 folders (`Common Files\VST3` under
both Program Files folders and `%LOCALAPPDATA%\Programs\Common\VST3`) and from the folders you add with **Add
folder…** in the app's VST3 library. The check runs on the path Windows resolved, so a link inside one of those
folders that leads elsewhere is refused. `add_vst3` refuses any other path with "plug-in path refused", and
`scan_vst3` skips any other extra folder and lists it in `skipped_paths`. The list of folders is kept in
`settings.json` and no tool can change it. Plug-ins you pick with **Add from file…** in the app are not limited,
because the choice is yours. The same rule applies to projects they open: a plug-in outside those folders stays in
the project and in the saved file but is not loaded, not in playback, bounces or a native editor window, and
`open_project` lists it in `blocked_plugins`. It loads when you open the project yourself from the app.

## Known issues being worked on

This was found in an automated review of the first release and is tracked for fixing:

- **Native plug-ins referenced by a project you open.** A project you open from the app can still reference a
  `.vst3` bundle in any folder of a local drive, and opening it loads the plug-in without asking; the path check
  may not recognise a drive letter mapped to a network share. A folder you added in the app counts as trusted
  together with every link inside it, as long as the links stay on local drives. Planned: ask before loading a
  plug-in that a project references outside the VST3 folders.

Until these are fixed, only open projects you trust and only connect MCP clients you trust.
