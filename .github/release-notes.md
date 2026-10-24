NØDE CAD is open-source parametric 3D CAD in Rust: sketch, constrain, extrude, and change a
parameter to watch the timeline rebuild. [Site and downloads](https://node-os.fr/cad)
· [try it in the browser](https://node-os.fr/cad/app/) ·
[contributing](https://github.com/matthias037/node-cad/blob/main/CONTRIBUTING.md) ·
[issues](https://github.com/matthias037/node-cad/issues). AI agents can model with it over MCP:
`node-cad-cli mcp` ([how](https://github.com/matthias037/node-cad/blob/main/docs/mcp.md)).

**What's new** in NØDE CAD {version} is listed below, and the
[ROADMAP](https://github.com/matthias037/node-cad/blob/main/ROADMAP.md#known-limitations) lists
the known limitations. Designs from every earlier version open in this one.

**New file extension.** Designs are now saved as `.nodecad`. Designs with an earlier extension
still open. Want plain JSON? Use
**File ▸ Save As JSON…** or `--save design.json`. Details:
[compatibility notes](https://github.com/matthias037/node-cad/blob/main/docs/compatibility.md).

**Which file?**
- **Windows:** install the `.msi`, or unzip the `-portable.zip`.
- **macOS 11+** (Apple silicon and Intel): open the `.dmg` and drag `NØDE CAD.app` onto Applications.
  The CLI is in `node-cad-cli-{version}-mac-universal.zip`.
- **Linux** (x86_64 and aarch64, glibc 2.35+): the `.AppImage` (`chmod +x`, then run it), the
  `.deb` or `.rpm`, the `.flatpak` (`flatpak install --user <file>`), the `.snap`
  (`sudo snap install --dangerous <file>`), or the `.tar.gz`.
- **FreeBSD:** unpack the `freebsd-x86_64.tar.gz` with `--strip-components 1 -C /usr/local`.
- **Web:** the web `.zip` is a static site to host yourself.
- `SHA256SUMS.txt`: `sha256sum -c SHA256SUMS.txt --ignore-missing`.

Single-file `.flatpak` bundles include the runtime repository address (Flathub); missing runtimes
still need an internet connection. [Linux installation details](https://github.com/matthias037/node-cad/blob/main/docs/book/src/guide/getting-started.md#linux-flatpak).

The builds aren't signed yet. On macOS, right-click `NØDE CAD.app` in Applications and choose
**Open** the first time (or run `xattr -dr com.apple.quarantine "/Applications/NØDE CAD.app"`).
On Windows, SmartScreen may say "Windows protected your PC": **More info**, then **Run anyway**.
Licences are built in (Help ▸ About ▸ Licences, `node-cad-cli licences`); the packages also
install `THIRD-PARTY-LICENSES.txt`.
