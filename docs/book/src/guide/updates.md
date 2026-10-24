# Updates and privacy

NØDE CAD does not look for new releases by itself yet: it has no release feed of its own, so
release builds make no update request at all and send nothing about you or your designs. The
**Update** pill in the title bar and **Check now** in **Preferences ▸ General ▸ Updates** stay
inactive until that feed exists.

## Getting a new version

New versions are published on [node-os.fr/cad](https://node-os.fr/cad#telecharger) and on the
[releases page](https://github.com/matthias037/node-cad/releases). Install a new version over
the old one the way you first installed it; designs, settings and autosaves are kept.

| Installed as | How to update |
|---|---|
| AppImage | Replace the old AppImage with the new one |
| Flatpak bundle | `flatpak install --user ./FILE.flatpak` with the new bundle |
| Snap from a downloaded file | `sudo snap install --dangerous ./FILE.snap` with the new file |
| `.deb` | `sudo apt install ./FILE.deb` |
| `.rpm` | `sudo dnf install ./FILE.rpm` |
| `.tar.gz` | Unpack over the old copy |
| Windows installer (`.msi`) | Run the new installer: it upgrades the installed copy in place |
| Windows portable `.zip` | Unzip over the old copy (`node-cad.exe` and `node-cad-cli.exe`) |
| macOS | Drag the new `NØDE CAD.app` from the `.dmg` onto Applications, replacing the old copy |
| Browser app | Nothing to do: each launch gets the latest published build |

## Checking a download

When a release carries a `SHA256SUMS.txt` with the SHA-256 of every package, check a download
by hand: compare its hash (`sha256sum FILE` on Linux, `shasum -a 256 FILE` on macOS,
`Get-FileHash FILE` in PowerShell) with the line for that file.

## Turning checks off for good

Setting the environment variable `SOLIDCRAFT_UPDATE_CHECK=0` turns all update checks off, for
example on managed machines, so a later release that does check still asks nothing. Builds you
compile yourself, the Flatpak and the browser app never check.
