# Downloads

Every platform and package. Installers and the web app are on
**[node-os.fr/cad](https://node-os.fr/cad#telecharger)**; every release, with its checksums and
all formats, is on the [releases page](https://github.com/matthias037/node-cad/releases).

- **Windows:** download and run the `.msi` installer, or unzip the portable `.zip`, which runs
  without installing.
- **macOS:** open the disk image (`.dmg`) and drag NØDE CAD into Applications
  (Apple silicon and Intel).
- **Linux:** download the AppImage, allow it to run as a program and open it. Or install the
  `.deb` (Ubuntu, Debian, Mint) or `.rpm` (Fedora, openSUSE), the `.flatpak` bundle or the
  `.snap` file (see [Getting started](book/src/guide/getting-started.md)).
- **Web:** [try it in your browser](https://node-os.fr/cad/app/), no install.

## Code signing

Windows releases will be code signed through the SignPath Foundation: free code signing provided by
[SignPath.io](https://about.signpath.io/), certificate by
[SignPath Foundation](https://signpath.org/). This is being set up; until it's live, the note
below applies. macOS releases are signed with an Apple Developer ID and notarized by Apple (the app
and the disk image carry the notarization ticket), so they open normally.

The Windows downloads aren't signed yet. If SmartScreen warns you, choose **More info**, then
**Run anyway**.

Install details and building from source are in
[Getting started](book/src/guide/getting-started.md).
