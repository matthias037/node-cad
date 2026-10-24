# Troubleshooting: NØDE CAD won't start

## 0.10.0 closes a few seconds after it starts (Windows)

NØDE CAD 0.10.0 crashes at startup on Windows PCs that draw with Direct3D 12 and the system
shader compiler. That covers older Intel graphics, PCs without a graphics driver (the
"Microsoft Basic Display Adapter"), virtual machines and Remote Desktop sessions. **Install
0.10.1 or later**, which fixes it. Versions before 0.10.0 don't have this problem.

## Start in safe mode

Safe mode starts with default settings (your own settings stay as they are, for the next normal
start), skips the "Recover unsaved design?" offer and draws with the software renderer.

- Windows: run `node-cad.exe --safe-mode` from a terminal in the install folder, or add
  ` --safe-mode` to the end of a shortcut's *Target*.
- macOS: `open -a "NØDE CAD" --args --safe-mode`
- Linux: `node-cad --safe-mode` (Flatpak: `flatpak run fr.node_os.cad --safe-mode`)

Setting `SOLIDCRAFT_SAFE_MODE=1` does the same. From 0.10.2, if two launches in a row stop
before the window opens, the next one starts in safe mode by itself. `SOLIDCRAFT_RENDERER=cpu`
uses only the software renderer and keeps your settings.

## Reset the settings

NØDE CAD keeps its settings and its data in the folders below. All installed versions share
them. No version changes anything outside them: no system settings, drivers or registry
entries. Quit NØDE CAD, then **rename** (don't delete) the folders. NØDE CAD starts with
defaults and makes new ones. Rename them back to restore your settings, or send them to us
with a bug report.

| System | Settings (`app.ron`, `graphics.json`) | Data (`recovery`, `crashes`, `control`, thumbnails) |
|---|---|---|
| Windows | `%APPDATA%\NODE CAD\data` | `%LOCALAPPDATA%\NODE CAD` |
| macOS | `~/Library/Application Support/NODE-CAD` | `~/Library/Application Support/NODE CAD` |
| Linux | `~/.local/share/nodecad` | `~/.local/share/solidcraft`; thumbnails in `$XDG_CACHE_HOME/NODE CAD` or `/tmp/NODE CAD` |
| Linux (Flatpak) | `~/.var/app/fr.node_os.cad/data/nodecad` | `~/.var/app/fr.node_os.cad/data/solidcraft` |

On Windows, paste `%APPDATA%` or `%LOCALAPPDATA%` into the Explorer address bar to open the
folder. Older releases used other folder names next to these; NØDE CAD only reads them, so
you can leave them alone.

- The **`recovery`** folder holds autosaves of designs that were never saved. Rename the data
  folder rather than deleting it, so you can still open those autosaves (`.nodecad` files)
  with File > Open.
- The **`crashes`** folder holds crash reports. Please attach the newest one to a bug report.

From 0.10.2, NØDE CAD also checks the settings files at start. A file it can't read is renamed
to `app.ron.bad-<time>` or `graphics.json.bad-<time>`, and NØDE CAD starts with defaults
instead.

## "No graphics adapter" or nothing happens at all

NØDE CAD needs Direct3D 12 or Vulkan on Windows, Metal on macOS, and Vulkan or OpenGL on
Linux. Without a graphics driver it falls back to the system's software renderer (WARP on
Windows, llvmpipe or lavapipe on Linux), which is slow but works. From 0.10.2, if no window can
be opened at all, NØDE CAD tries the software renderer once and otherwise shows a message
saying what failed (earlier versions closed without a word). Things to try:

- Install the graphics driver from the GPU maker (Intel, AMD, NVIDIA) rather than Windows Update.
- In a virtual machine, turn on 3D acceleration. Over Remote Desktop, try it on the machine itself.
- Run `node-cad --crash-log` to list recent crash reports, and attach the newest one to an
  issue.

## The desktop stutters or freezes while NØDE CAD builds bodies

Some graphics drivers hang on the shader NØDE CAD uses to build bodies on the GPU, and the
system then resets the GPU. NØDE CAD already builds bodies on the CPU on the AMD FirePro
D300/D500/D700 (Mac Pro 2013). On another GPU that does this, start NØDE CAD with
`SOLIDCRAFT_CPU_TESS=1` set to build bodies on the CPU, and open an issue with the GPU's name.

## NØDE CAD closes right away on Windows

A graphics driver can crash the app while it starts, before NØDE CAD can show anything (an
old Intel Vulkan driver, `igvk64.dll`, did). Windows' Event Viewer names the driver under
*Windows Logs › Application* ("Faulting module name").

From 0.11.1, NØDE CAD on Windows starts Direct3D 12 first and loads no other graphics API
when Direct3D 12 has a graphics card; Vulkan and then OpenGL are tried only on a computer
without one. Earlier versions started Vulkan, Direct3D 12 and OpenGL together and preferred
Vulkan. To choose the graphics API yourself, in any version, open a Command Prompt and start
NØDE CAD from it (the install folder is under `C:\Program Files` unless you chose another):

```
set WGPU_BACKEND=dx12
"<install folder>\node-cad.exe"
```

`vulkan` and `gl` are the other choices. `setx WGPU_BACKEND dx12` keeps it for apps started
from the Start menu after you sign in again. From 0.11.1 the setting can also live in
`graphics.json` in the settings folder, as `{"msaa": 4, "backend": "dx12"}` (`auto` is the
order above). Safe mode ignores it and always uses the software renderer.

## NØDE CAD restarted, saying the graphics device was reset

When the graphics driver stops responding, the system resets it (on Windows, "Display driver
stopped responding and has recovered"), and the graphics device NØDE CAD draws with is gone
for good: the window can't draw anything more. NØDE CAD then closes, keeps your unsaved
changes for recovery as it does after a crash, and starts again; the new window says what
happened and offers the changes back. If the device is reset again within ten minutes of that
restart, the next start uses software rendering, which doesn't depend on the graphics driver.
Updating the graphics driver from the GPU maker usually stops the resets.

## A design saved by 0.10 won't open in an older version

Designs saved by a newer NØDE CAD use a newer file format. Older versions refuse them with
"this design was saved by a newer NØDE CAD" and don't change the file. Open them with the
current version.
