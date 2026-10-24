# Compatibility with earlier names

NØDE CAD is forked from SolidCraft, which was first called SolveCraft. Everything a user, a
customer or an AI client sees uses NØDE CAD's names: the `node-cad` and `node-cad-cli`
programs, `.nodecad` designs, the `node-cad` MCP server and its `nodecad://` resources, the
`fr.node_os.cad` Linux app ID and the `NØDE CAD.app` bundle (`fr.node-os.cad`).

Some internal names keep the SolidCraft spelling, so that upstream changes still merge and
existing designs, settings and setups keep working. They are not shims and stay as they are:

- the Cargo packages and crates (`solidcraft`, `solidcraft-cli`, `solidcraft-*`) and the
  repository folders `apps/solidcraft*`;
- the `"format": "solidcraft/N"` tag inside designs and the other markers written into files
  (`solidcraft-container/1`, `solidcraft-theme/1`, `solidcraft-shortcuts/1`,
  `solidcraft-crash/1`, `solidcraft-library/1`, …);
- the `SOLIDCRAFT_*` environment variables;
- settings and data folder names, preference and keychain keys (section 6).

Older names a user may still meet survive **only** in the shims below (plus their tests, this
file, and changelog / release notes). When you add or remove a shim, update this file in the
same change.

## 1. `.solidcraft` and `.solvecraft` design files

New designs are saved as `.nodecad`. Designs with SolidCraft's `.solidcraft` extension, or the
older `.solvecraft`, still open everywhere a design opens:

| Path in | Where |
|---|---|
| Extension → design format | `crates/io/src/lib.rs` (`LEGACY_DESIGN_EXTS`, `DESIGN_EXTS`, format detection) |
| Open dialog filters (desktop) | `apps/solidcraft/src/main.rs` (`services`), from the format registry in `crates/io/src/formats.rs` |
| Drag-drop, recent files, `doc.open`, MCP `open`, CLI `eval`/`run` | go through the same format detection in `solidcraft_io` |
| Linked and extracted components | `crates/engine/src/cmd/component_link.rs`, `component_extract.rs`: any design extension |
| Autosaves | `crates/engine/src/recovery.rs`: an autosave kept under a legacy extension is still listed and recovered |
| Browser file pickers | `apps/solidcraft-web/src/files.rs` (`accept`, from the format registry) and `apps/solidcraft-web/src/disk.rs` (`open_options`) |
| Crash reports | `crates/engine/src/crash.rs`: reports hold `design.nodecad`; `design.solidcraft` in a report from an earlier release is still replayed |
| Windows | `assets/platforms/windows/solidcraft.wxs`: the design ProgId registers `.nodecad` and keeps the legacy extensions |
| macOS | `assets/platforms/macos/Info.plist.in`: the design document type lists `nodecad` and the legacy extensions |
| Linux / FreeBSD / Flatpak | `assets/platforms/linux/fr.node_os.cad.mime.xml`: the `*.nodecad` glob and the legacy globs |

Recent-files entries that point at a legacy file keep working: they are plain paths and open
through the same detection. The browser app also lists designs it stored under the old
extensions, and keyboard-shortcut files exported as `solvecraft-shortcuts/1` still import
(`crates/ui-egui/src/toolbar_custom.rs`). Tests: `crates/io` (`Format::from_name`,
`is_design_path`, `paths_resolve_to_the_canonical_rows`),
`legacy_extension_and_format_tag_still_open` (CLI, `run_results.rs`) and
`legacy_design_files_and_resource_uris` (MCP `open`).

## 2. `solidcraft-cli` and `solvecraft-cli` aliases

Two small alias executables ship next to `node-cad-cli` in every package
(`apps/solidcraft-cli/src/alias.rs`, built as `alias_solidcraft.rs` and `alias_solvecraft.rs`): `solidcraft-cli`, SolidCraft's name for the CLI, and
`solvecraft-cli`, the name before it. Each prints a one-line deprecation note to stderr and
runs the `node-cad-cli` next to it (else the one on `PATH`) with the same arguments,
stdin/stdout and exit code, so scripts and MCP client configs that call an old name keep
working. Test: `apps/solidcraft-cli/tests/legacy_alias.rs` (forwards `--version` and the exit
status; the note goes to stderr only, so stdout stays clean for MCP).

On macOS, `NØDE CAD.app/Contents/MacOS` holds `solidcraft-cli` next to `node-cad-cli` too, for MCP
setups that start the CLI inside the bundle (`packaging/macos/package.sh`).

There is no alias for the desktop binary: launchers, file associations and Start-menu entries
installed by the packages point at `node-cad`.

## 3. MCP

`serverInfo.name` is `node-cad`. The name a client shows is chosen by the user when the server
is added, so an existing setup such as

```sh
claude mcp add solidcraft -- solidcraft-cli mcp
```

keeps working (through the alias above). New setups use:

```sh
claude mcp add node-cad -- node-cad-cli mcp
```

Switch before the aliases are removed. Resources are listed as `nodecad://design` and
`nodecad://commands`; `resources/read` still answers the `solidcraft://` and `solvecraft://`
URIs earlier releases listed (`crates/mcp/src/server.rs`, test
`legacy_design_files_and_resource_uris`). The in-app agent chat hands its agent a bridged
server named `node-cad-app`, so a server the user added as `node-cad` doesn't take its place;
its permission questions treat tools of a server named `node-cad…` or `solidcraft…` as the app's
own (`crates/ui-egui/src/agent_chat/mod.rs`).

## 3b. The browser API

The browser app's script API is `window.nodecad`; the same object is also `window.solidcraft`,
the name earlier releases used, so scripts written for them keep working
(`apps/solidcraft-web/src/api.rs`, docs/browser-api.md).

## 4. The `solvecraft/N` format tag

Designs record their format as `"format": "solidcraft/N"`; that tag is internal and unchanged.
Files saved by builds up to 0.5 say `"format": "solvecraft/N"`; `crates/doc/src/format.rs`
(`version_of`) reads both prefixes and upgrades them as usual (the file is re-tagged
`solidcraft/N` when saved).

Not backward-compatible: builds up to 0.5 don't know the new tag and refuse files saved by
later builds. Test: `version_of` accepts both prefixes; legacy-tagged designs load and are
re-tagged on save.

## 5. `SOLVECRAFT_*` environment variables

Every runtime variable is read through one helper, `solidcraft_geom::env` (`var`, `var_os`):
it reads `SOLIDCRAFT_<NAME>` and falls back to `SOLVECRAFT_<NAME>` when the new one is unset.
The shell entry points do the same: `packaging/linux/AppRun`
(`SOLIDCRAFT_NO_DESKTOP_INTEGRATION`) and `packaging/env.sh` (`SOLIDCRAFT_VERSION`,
`SOLIDCRAFT_BUILD_DATE`). Tests: the helper's unit tests (new wins, old is the fallback,
neither → unset).

## 6. Settings, preferences, autosave and recovery folders

| | Linux | macOS | Windows |
|---|---|---|---|
| Preferences, graphics settings (eframe storage) | `~/.local/share/nodecad` | `~/Library/Application Support/NODE-CAD` | `%APPDATA%\NODE CAD\data` |
| Data: recovery (autosave), crashes, control tokens | `~/.local/share/solidcraft` | `~/Library/Application Support/NODE CAD` | `%LOCALAPPDATA%\NODE CAD` |

Folder names are written in ASCII. On first launch, if the settings or recovery folder doesn't
exist but the one of releases before 0.5 (`solvecraft` / `SolveCraft`) does, it is **copied**
to the new location; the old folder is never deleted or changed. The window and UI state that
eframe kept under `ai.storyteller.solvecraft` is copied the same way. Autosaves copied from the
old folder (`<id>.solvecraft`) are listed and recovered like new ones. The preferences key in
eframe's storage is `solidcraft.prefs`; `solvecraft.prefs` is read when it is missing (desktop
and browser, `solidcraft_ui_egui::prefs::stored`). The browser app also reads files and the
autosave it kept under the old names in local storage and OPFS
(`apps/solidcraft-web/src/files.rs`). A 3DxWare profile the driver made for an old executable
name is read until it makes a new one (`apps/solidcraft/src/space_mouse.rs`).
In the Flatpak sandbox these folders live under `~/.var/app/<app id>/data`. The thumbnail cache
is not migrated; it is rebuilt. The copy helper is `solidcraft_io::adopt_legacy_dir`, used by
the desktop app (`adopt_legacy_settings`) and `solidcraft_engine::recovery::default_dir`.
Tests: `legacy_dir` (copy once, never overwrite, old folder intact),
`first_launch_adopts_the_previous_release_settings`,
`autosave_from_a_legacy_folder_is_listed_and_loads`, `recovery_dirs_and_their_legacy_names`,
`stored_prefs_fall_back_to_the_legacy_key`.

## 7. Linux app ID

NØDE CAD's Linux app ID is `fr.node_os.cad`: the Flatpak app ID, the `.desktop`, metainfo and
MIME file names, the icon name (`assets/icons/app/linux/exports/hicolor/*/apps/fr.node_os.cad.png`)
and the window's Wayland `app_id` / X11 class (`crates/io/src/legacy_dir.rs`, `APP_ID`).
SolidCraft used `ai.storyteller.solidcraft`, and releases up to 0.7 `ai.storyteller.solvecraft`.
In the Flatpak sandbox, `solidcraft_io::legacy_flatpak_path` (`LEGACY_APP_ID`) lets the first
launch copy settings and autosaves from the old app's data folder when it is readable; the old
folder is not changed. Test: `flatpak_twin_maps_our_sandbox_data_to_the_old_app_id`.

## 8. Draft design containers

The #303 draft used `solvecraft-container/1`. `crates/io/src/design.rs` accepts that
marker alongside `solidcraft-container/1`, and writes only the latter. Optional
old previews are discarded; required document/blob validation is unchanged. The
`draft_container_marker_remains_readable` regression covers this shim.
