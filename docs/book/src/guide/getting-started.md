# Getting started

The desktop executable is `node-cad`, the CLI is `node-cad-cli`, and designs are saved as
`.nodecad`. Designs saved with the earlier `.solidcraft` or `.solvecraft` extension still open
(see the [compatibility notes](https://github.com/matthias037/node-cad/blob/main/docs/compatibility.md)).

## Desktop

Download the installer for your system from [node-os.fr/cad](https://node-os.fr/cad#telecharger)
or the [releases page](https://github.com/matthias037/node-cad/releases), or build it yourself with
stable Rust (1.95 or newer):

```sh
git clone https://github.com/matthias037/node-cad && cd node-cad
cargo run --release -p solidcraft -- --sample   # opens with a sample part
```

`node-cad part.step` opens a STEP, IGES, 3MF, STL or OBJ file as a new design, and
`node-cad design.nodecad` opens a saved design. Windows builds cross-compile from Linux with
`cargo xwin build --release --target x86_64-pc-windows-msvc -p solidcraft`.

### macOS

Open the release `.dmg` and drag `NØDE CAD.app` onto Applications.

### Linux AppImage

The first time you open the AppImage it adds a menu entry and icons under `~/.local/share`
(`applications/fr.node_os.cad.desktop`, `icons/hicolor`), so the desktop shows the
right name and icon. It adds nothing if [appimaged](https://github.com/probonopd/go-appimage)
already made an entry for the file, or if a launcher such as Gear Lever or AppImageLauncher
started it (and removes the entry NØDE CAD added before). To
turn it off for good, create an empty file `~/.local/share/solidcraft/no-desktop-integration`, or
start the AppImage with `SOLIDCRAFT_NO_DESKTOP_INTEGRATION=1`.

### Linux Flatpak

Install Flatpak using your distribution's package manager and add Flathub, which provides the
Freedesktop runtime NØDE CAD needs. Then download the release's `.flatpak` file for your
architecture from the releases page and install it:

```sh
flatpak remote-add --user --if-not-exists flathub https://dl.flathub.org/repo/flathub.flatpakrepo
flatpak install --user ./FILE.flatpak      # the file you downloaded
flatpak run fr.node_os.cad
```

Installation needs internet access to fetch any missing runtime. A bundle has no update feed:
to update, install the next release's bundle the same way. Launch with
`flatpak run fr.node_os.cad`, rather than looking for a host `node-cad` binary. Files chosen
through Open/Save use the desktop file-chooser portal; command-line paths outside Documents may
need a specific additional filesystem permission.

### Linux Snap

On Ubuntu and other distributions with snapd, install the release's `.snap` file:

```sh
sudo snap install --dangerous ./FILE.snap   # the file you downloaded
node-cad                # or open NØDE CAD from the app menu
node-cad.cli --version  # the CLI, for scripts and `node-cad.cli mcp`
```

The snap can open designs in your home folder (not hidden folders) and, through the file
chooser, anywhere else. For USB drives and other mounted disks, run
`sudo snap connect node-cad:removable-media` once. Installed from a file, the snap doesn't
update automatically: install the next release's `.snap` the same way.

## Web

The same app runs in the browser: [open NØDE CAD on the web](https://node-os.fr/cad/app/) (`?sample` opens the sample
part, `?webgl` forces WebGL2 where WebGPU is missing or misbehaves; the browser remembers it until
`?webgpu`, and an app installed from Chrome or Edge starts with it too). Designs are saved in the
browser's private file storage, which needs HTTPS or localhost.
Planned worker and tiered-storage integration is
described in [web-runtime.md](https://github.com/matthias037/node-cad/blob/main/docs/web-runtime.md)
(design; not yet wired in the shipped wasm build).

After the first visit the web app works offline: the browser keeps a copy of it. Online, each
launch gets the latest published build (the browser downloads it once when it changes); offline,
when the network doesn't answer within a few seconds, or when a new build can't be downloaded
whole, the kept copy starts (and the download carries on for the next launch).

Browsers that can install web apps add **Install app…** to the File menu: it installs NØDE CAD
as an app with its own window and launcher icon, working offline like the page. In Chrome, Edge
and other Chromium browsers it shows the browser's install prompt, and once NØDE CAD is
installed the item is gone. Safari has no such prompt, so the item shows its steps instead:
Share › Add to Home Screen on iPhone and iPad, File › Add to Dock on a Mac (Safari 17 or later).

To build it yourself:

```sh
rustup target add wasm32-unknown-unknown
cd apps/solidcraft-web && trunk serve --release --no-autoreload
```

Offline use needs `--no-autoreload`: Trunk's live reload changes the page as it serves it, so it
no longer matches the build the browser would keep. Debug builds (`trunk serve`) never keep a copy.

### Self-hosting with Docker

The repository builds the web app into a container image for Linux on x86_64 and ARM64. It
serves the app with nginx on port 8080 and keeps nothing on the server: designs stay in each
visitor's browser. From the repository root, with Docker Compose and
[`packaging/docker/compose.yaml`](https://github.com/matthias037/node-cad/blob/main/packaging/docker/compose.yaml)
(`SOLIDCRAFT_PORT` picks another port):

```sh
docker compose -f packaging/docker/compose.yaml up -d --build   # build the image from your checkout and start it
docker compose -f packaging/docker/compose.yaml down            # stop
```

Or without Compose:

```sh
docker build -f packaging/docker/Dockerfile -t node-cad-web .
docker run -d --name node-cad -p 8080:8080 --restart unless-stopped node-cad-web
```

Then open `http://localhost:8080/`. On another machine the browser only saves designs over HTTPS,
so put a reverse proxy with a certificate in front (Caddy, Traefik, nginx); the app works under
any URL path, such as `https://example.com/cad/`.

### 3D mouse in the browser

The web app reads a 3Dconnexion SpaceMouse or SpaceNavigator through the browser's Gamepad API,
with the same switch and speed in Preferences › Navigation as the desktop app. Edge and Chrome
read the device as it is. A browser shows a gamepad to a page only once it has been moved there,
so if nothing happens at first, push the cap firmly once. 3DxWare also turns the cap into
mouse-wheel zoom in browsers; while the cap moves, the web app ignores the wheel and reads the
cap itself. Before the browser shows the cap to the page, that zoom still gets through, so the
first push after opening the page may zoom a little.

Firefox doesn't list 3D mice ([Mozilla bug 1226575](https://bugzilla.mozilla.org/show_bug.cgi?id=1226575)).
On Windows it does list 3DxWare's virtual joystick, which can carry the cap instead. Save this
as `%APPDATA%\3Dconnexion\3DxWare\Cfg\Firefox.xml`, then restart Firefox (and 3DxWare, if the cap
still only zooms). It changes Firefox only: there 3DxWare sends the cap's six axes to the
joystick instead of the mouse wheel. Delete the file to undo it.

```xml
<?xml version="1.0" encoding="UTF-8"?>
<AppCfg Default="false" xmlns="" CfgFormatVersion="1.3" ThisFileVersion="1.1">
  <AppInfo>
    <Signature>
      <Name>STR_FIREFOX</Name>
      <ExecutableName>FIREFOX.EXE</ExecutableName>
      <Transport>KMJ</Transport>
    </Signature>
    <Options/>
  </AppInfo>
  <CfgProperties>
    <ID>ID_STR_FIREFOX</ID>
    <Name>STR_FIREFOX</Name>
    <InheritsFromID>ID_STR_FIREFOX</InheritsFromID>
  </CfgProperties>
  <Devices>
    <Device>
      <ID>ID_Standard_3D_Mouse</ID>
      <AxisBank>
        <Name>STR_DEFAULT_BANK</Name>
        <ID>Default</ID>
        <Axis><Enabled>true</Enabled><Input><ActionID>HIDMultiAxis_X</ActionID><Min>-512</Min><Max>511</Max><Deadband>0</Deadband></Input><Output><ActionID>HIDJoystick_X</ActionID></Output></Axis>
        <Axis><Enabled>true</Enabled><Input><ActionID>HIDMultiAxis_Y</ActionID><Min>-512</Min><Max>511</Max><Deadband>0</Deadband></Input><Output><ActionID>HIDJoystick_Y</ActionID></Output></Axis>
        <Axis><Enabled>true</Enabled><Input><ActionID>HIDMultiAxis_Z</ActionID><Min>-512</Min><Max>511</Max><Deadband>0</Deadband></Input><Output><ActionID>HIDJoystick_Z</ActionID></Output></Axis>
        <Axis><Enabled>true</Enabled><Input><ActionID>HIDMultiAxis_Rx</ActionID><Min>-512</Min><Max>511</Max><Deadband>0</Deadband></Input><Output><ActionID>HIDJoystick_Rx</ActionID></Output></Axis>
        <Axis><Enabled>true</Enabled><Input><ActionID>HIDMultiAxis_Ry</ActionID><Min>-512</Min><Max>511</Max><Deadband>0</Deadband></Input><Output><ActionID>HIDJoystick_Ry</ActionID></Output></Axis>
        <Axis><Enabled>true</Enabled><Input><ActionID>HIDMultiAxis_Rz</ActionID><Min>-512</Min><Max>511</Max><Deadband>0</Deadband></Input><Output><ActionID>HIDJoystick_Rz</ActionID></Output></Axis>
      </AxisBank>
    </Device>
  </Devices>
</AppCfg>
```

## CLI

`node-cad-cli` runs the same commands without a window:

```sh
cargo run --release -p solidcraft-cli -- commands                       # every command, as JSON
cargo run --release -p solidcraft-cli -- run examples/bracket.json --out bracket.step
cargo run --release -p solidcraft-cli -- eval examples/bracket.json     # volume, area, faces…
cargo run --release -p solidcraft-cli -- snapshot examples/bracket.json --out bracket.png
cargo run --release -p solidcraft-cli -- exec solid.box '{"length": 20, "width": 10, "height": 5}'
cargo run --release -p solidcraft-cli -- run mechanism.json --in arm.nodecad  # with a motion.export_animation step: a GIF
```

A script is a list of commands:

```json
{"commands": [
  {"command": "sketch.create", "params": {"plane": "XY"}},
  {"command": "sketch.rectangle.two_point", "params": {"p0": [0, 0], "p1": [40, 30]}},
  {"command": "sketch.finish", "params": {}},
  {"command": "solid.extrude", "params": {"distance": 20}},
  {"command": "solid.fillet", "params": {"edges": [[0, 0, 10]], "radius": 3}}
]}
```

![The bracket example, rendered by node-cad-cli snapshot](../generated/bracket.png)

`snapshot` looks at the model from the iso view. `--view NAME` takes another standard view (`front`, `back`, `top`, `bottom`, `left`, `right`) or the name of a view saved in the design with `view.save`. `--camera JSON` takes any camera, as the MCP `screenshot` tool does (its conventions are in [the MCP page](mcp.md#camera-and-close-up-recipe)). A script that applies a view itself with `view.apply` sets the camera too. The ground grid lies on Z 0, so it cuts through a part that straddles it: `--no-grid` leaves it out of the picture, and `--no-axes` the origin's axes. The picture is drawn on the GPU with the viewport's own pipelines, so it shows what the window would (a surface analysis such as `inspect.zebra` included); a machine without a GPU uses a software one (Mesa's lavapipe on Linux, WARP on Windows), and with neither (a Mac without usable Metal, a Linux box without Mesa's Vulkan) the CPU rasterizer draws it, without surface analysis colours, and the reply's `renderer` says `cpu` with a warning. `SOLIDCRAFT_GPU=auto|hardware|software|cpu` chooses (`hardware` and `software` fail rather than fall to the CPU), and `node-cad-cli gpu` lists the adapters and the tier a snapshot would use.

```sh
node-cad-cli snapshot part.nodecad --out photo.png --view Photo
node-cad-cli snapshot part.nodecad --out close-up.png --no-grid --no-axes --camera '{"target": [30, -20, 0], "yaw": 0.6, "pitch": 0.25, "distance": 60}'
```

### Seeing what a cut or a join is about to do

A picture answers in a glance what a volume cannot: does the tool body reach through the
material, and is it where you meant it to be? `--body NAME:opacity=0.3` draws that body
see-through, so whatever is inside it shows; `--body NAME:color=#cc3333` gives it a colour.
Either key on its own keeps what the design says about the other, and the option is a shortcut
for `appearance.assign`, the command the Appearance dialog runs — the picture is the one the
window would show with the same appearance assigned.

`--tool NAME` is the common case in one option: that body solid in the tool colour, every
other body see-through around it. `--section z=27` cuts the model at a plane so the inside
shows, as `inspect.section` does: an axis and the world coordinate to cut at, `,flip` to keep
the other side, `XY`, `XZ` or `YZ` for a plane through the origin, or the command's own JSON.

![A pin inside a translucent plate, cut by a section plane](../generated/tool-preview.png)

```sh
# The pin the cut would remove, inside the plate it stops 6 mm short of.
node-cad-cli snapshot plate.nodecad --out preview.png --tool Pin
node-cad-cli snapshot plate.nodecad --out cut.png --tool Pin --section z=12
node-cad-cli snapshot plate.nodecad --out glass.png --body "Plate:opacity=0.25" --body "Pin:color=#1a7f4b"
```

A body drawn see-through is drawn see-through everywhere a picture is drawn, not only here:
an appearance with an opacity below 1 (`Glass - Clear` in the library, or any `opacity` you
assign) now reads as glass in `snapshot`, in MCP screenshots and in a design's thumbnail, as it
always has in the window. A face painted with a see-through look of its own still draws at its
body's opacity.

Windows PowerShell 5.1 drops the double quotes inside an argument it passes to a program: write them as `\"` there.

A command that fails stops the script. A feature can also fail later, when a parameter change
rebuilds it; `run` then exits with status 1, lists the failing features and writes none of
its `--out` and `--save` files, because the bodies lack what those features should have made.
`--allow-errors` writes them anyway.

`run` prints each command's result with `--results` (otherwise the measurement), and `eval`
prints the measurement, as JSON on stdout. `--json-out FILE` writes that JSON to a file
instead. The file is plain UTF-8, which is not what a redirect gives you everywhere: Windows
PowerShell's `>` re-encodes a program's output, as UTF-16 or as UTF-8 with a byte-order mark
depending on its settings, and strict JSON readers such as Python's `json.load` then refuse it.

```sh
cargo run --release -p solidcraft-cli -- run examples/bracket.json --results --json-out results.json
cargo run --release -p solidcraft-cli -- eval examples/bracket.json --json-out measures.json
```

To find the slow steps of a script, `--timings` prints how long each command took instead of
the measurement: `{"timings": [{"index", "command", "ms"}], "total_ms"}`, with `index` counted
from 0. `--profile` adds each command's `profile: {"boolean_ms", "tessellate_ms", "other_ms"}`,
its wall time in the kernel's booleans, in tessellation and in everything else. With
`--results` as well, the results go in the same object as `results`; `--results` alone still
prints a plain array.

```sh
cargo run --release -p solidcraft-cli -- run examples/bracket.json --profile
```

A design's history turns into a script that rebuilds it, which makes a bug that only shows
on one design a one-file reproduction:

```sh
cargo run --release -p solidcraft-cli -- script part.nodecad --out repro.json
cargo run --release -p solidcraft-cli -- eval repro.json               # the same part
```

The script starts with the user parameters (`parameters.add`) and the design's units, sheet
metal and plastic rules and materials, then adds each timeline feature with `timeline.add`,
exactly as the design file holds it: same ids, names and parameter names, so later steps
still find the sketches and bodies they use, and a feature that failed in the design fails
the same way. Edit a step's values to try a change, or delete the steps after a feature to
see the design as it was there. Appearances, folders, named views and configurations are
left out (the CLI notes which), and designs with components or joints are not supported
yet. In the app and over MCP the same script comes from `timeline.export_script`.

Units are millimetres and degrees unless an expression says otherwise (`"1 in"`, `"30 deg"`),
and Z is up.
