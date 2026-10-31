---
name: node-cad-name
description: "Use before writing anything a user, a customer or an AI client sees (labels, messages, file headers, executables, packages) or before renaming anything. Says what is called NØDE CAD and what keeps the upstream SolidCraft name so upstream changes still merge."
---

# The product name: NØDE CAD

This fork of SolidCraft ships as **NØDE CAD** (NØDE OS brand).

## What says NØDE CAD
- Everything a user, a customer or an AI client sees: window title, menus, dialogs, every locale, crash and issue reports, headers written into exported files.
- The executables (`node-cad`, `node-cad-cli`), design files (`.nodecad`), the MCP server (`node-cad`, resources `nodecad://…`, the agent chat's bridge `node-cad-app`).
- The macOS bundle (`NØDE CAD.app`, id `fr.node-os.cad`), the Linux app id (`fr.node_os.cad`) and the app icon (`assets/icons/app/shared/source/app_icon.rs`).
- Text written into files, folder names, user agents and driver ids uses ASCII `NODE CAD` / `NODE-CAD`.
- Never put "SolidCraft" back into anything a user or an AI client can see.

## What keeps the SolidCraft name
So upstream changes still merge and nobody's data is lost:
- Cargo package and crate names (`solidcraft`, `solidcraft-cli`, `solidcraft-*`: `cargo run -p solidcraft` still works) and Rust paths.
- `SOLIDCRAFT_*` environment variables, settings, recovery and data folder names, storage and preference keys, keychain service names.
- The format tags written inside files (`solidcraft/N`, `solidcraft-theme/1`, …).
- SolidCraft's copyright in LICENSE-*, NOTICE and THIRD-PARTY-LICENSES.txt.

## Earlier names
They survive only as the compatibility shims listed in `docs/compatibility.md`: `.sc`, `.solidcraft` and `.solvecraft` designs still open; `solidcraft-cli` and `solvecraft-cli` forward to `node-cad-cli`. Do not add new uses of them.
