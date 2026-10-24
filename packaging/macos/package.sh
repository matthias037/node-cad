#!/usr/bin/env bash
# Build and package NØDE CAD for macOS:
#
#   $DIST/NODE-CAD-<version>-mac-<arch>.dmg           NØDE CAD.app (with node-cad-cli inside) on a
#                                                      drag-to-Applications DMG
#   $DIST/node-cad-cli-<version>-mac-<arch>.zip       the headless CLI (and its solidcraft-cli and
#                                                      solvecraft-cli compatibility aliases) with
#                                                      the licence files
#
# Usage: packaging/macos/package.sh [--arch universal|aarch64|x86_64] [--skip-build]
#
# The app is ad-hoc signed (not notarized): the first launch needs right-click > Open.
# Adapted from PhotoCraft's packaging (storytold/photocraft, Apache-2.0; see ATTRIBUTION.md).
set -euo pipefail
# shellcheck source=../env.sh
. "$(dirname "${BASH_SOURCE[0]}")/../env.sh"
# NØDE CAD's own reverse-DNS id, beside NØDE OS's fr.node-os.desktop.
MACOS_BUNDLE_ID=fr.node-os.cad

ARCH=universal
SKIP_BUILD=0
while [ $# -gt 0 ]; do
  case "$1" in
    --arch) ARCH="$2"; shift 2 ;;
    --skip-build) SKIP_BUILD=1; shift ;;
    -h | --help) sed -n '2,11p' "$0"; exit 0 ;;
    *) echo "unknown argument: $1" >&2; exit 2 ;;
  esac
done
case "$ARCH" in
  universal) TARGETS=(aarch64-apple-darwin x86_64-apple-darwin) ;;
  aarch64) TARGETS=(aarch64-apple-darwin) ;;
  x86_64) TARGETS=(x86_64-apple-darwin) ;;
  *) echo "unknown --arch $ARCH" >&2; exit 2 ;;
esac

# Keep in sync with LSMinimumSystemVersion in Info.plist.in.
export MACOSX_DEPLOYMENT_TARGET=11.0
SHORT_VERSION="${VERSION%%-*}"
WORK="$CARGO_TARGET_DIR/macos-package"
APP="$WORK/NØDE CAD.app"
DMG="$DIST/NODE-CAD-$VERSION-mac-$ARCH.dmg"
CLI_ZIP="$DIST/node-cad-cli-$VERSION-mac-$ARCH.zip"

echo "==> NØDE CAD $VERSION for macOS ($ARCH)"
if [ "$SKIP_BUILD" = 0 ]; then
  args=()
  for t in "${TARGETS[@]}"; do args+=(--target "$t"); done
  (cd "$ROOT" && cargo build --release --locked -p solidcraft -p solidcraft-cli "${args[@]}")
fi

rm -rf "$WORK"
mkdir -p "$WORK/bin"
for bin in node-cad node-cad-cli solidcraft-cli solvecraft-cli; do
  inputs=()
  for t in "${TARGETS[@]}"; do inputs+=("$CARGO_TARGET_DIR/$t/release/$bin"); done
  lipo -create -output "$WORK/bin/$bin" "${inputs[@]}"
  lipo -info "$WORK/bin/$bin"
done

# ---- NØDE CAD.app ------------------------------------------------------------------------------
echo "==> assembling $APP"
mkdir -p "$APP/Contents/MacOS" "$APP/Contents/Resources"
cp "$WORK/bin/node-cad" "$APP/Contents/MacOS/NODE-CAD"
# node-cad-cli next to the app, as every other package has it: the agent chat starts it as the
# agent's MCP server (`node-cad-cli mcp --connect`) and looks for it beside the running program.
cp "$WORK/bin/node-cad-cli" "$APP/Contents/MacOS/node-cad-cli"
# Compatibility shim (docs/compatibility.md): MCP client setups made with earlier releases start
# Contents/MacOS/solidcraft-cli; the alias forwards to node-cad-cli beside it.
cp "$WORK/bin/solidcraft-cli" "$APP/Contents/MacOS/solidcraft-cli"
# The icon is drawn in code (assets/icons/app/shared/source/app_icon.rs); iconutil packs the sizes.
ICONSET="$WORK/NODE-CAD.iconset"
mkdir -p "$ICONSET"
for s in 16 32 128 256 512; do
  "$WORK/bin/node-cad-cli" icon --size "$s" --out "$ICONSET/icon_${s}x${s}.png"
  "$WORK/bin/node-cad-cli" icon --size "$((s * 2))" --out "$ICONSET/icon_${s}x${s}@2x.png"
done
iconutil -c icns -o "$APP/Contents/Resources/NODE-CAD.icns" "$ICONSET"
mkdir -p "$APP/Contents/Resources/Licenses"
copy_docs "$APP/Contents/Resources/Licenses"
sed -e "s/@VERSION@/$VERSION/g" -e "s/@SHORT_VERSION@/$SHORT_VERSION/g" "$ROOT/assets/platforms/macos/Info.plist.in" >"$APP/Contents/Info.plist"
plutil -lint "$APP/Contents/Info.plist"
printf 'APPL????' >"$APP/Contents/PkgInfo"
# Ad-hoc signature over the whole bundle (Apple silicon refuses unsigned code). Nested code is
# signed first: the bundle signature refuses an unsigned helper in Contents/MacOS.
codesign --force --sign - --timestamp=none --identifier "$MACOS_BUNDLE_ID-cli" "$APP/Contents/MacOS/node-cad-cli"
codesign --force --sign - --timestamp=none --identifier "$MACOS_BUNDLE_ID-cli-alias" "$APP/Contents/MacOS/solidcraft-cli"
codesign --force --sign - --timestamp=none "$APP"
codesign --verify --strict --deep --verbose=2 "$APP"
test -x "$APP/Contents/MacOS/node-cad-cli" || { echo "node-cad-cli is missing from $APP" >&2; exit 1; }

# ---- DMG ---------------------------------------------------------------------------------------
echo "==> building $DMG"
STAGE="$WORK/dmg"
mkdir -p "$STAGE"
ditto "$APP" "$STAGE/NØDE CAD.app"
ln -s /Applications "$STAGE/Applications"
cp "$ROOT/assets/platforms/macos/README.txt" "$STAGE/README.txt"
rm -f "$DMG" "$WORK/raw.dmg"
# makehybrid + convert builds the image without attaching a device, unlike `create -srcfolder`,
# which is flaky on CI runners ("Resource busy").
hdiutil makehybrid -hfs -hfs-volume-name "NØDE CAD" -hfs-openfolder "$STAGE" -o "$WORK/raw.dmg" "$STAGE"
hdiutil convert "$WORK/raw.dmg" -format UDZO -imagekey zlib-level=9 -o "$DMG"
rm -f "$WORK/raw.dmg"

# Check the image as a user gets it: mount it and look inside.
MNT="$(mktemp -d)"
hdiutil attach -readonly -nobrowse -mountpoint "$MNT" "$DMG"
ls -la "$MNT" "$MNT/NØDE CAD.app/Contents" "$MNT/NØDE CAD.app/Contents/MacOS" "$MNT/NØDE CAD.app/Contents/Resources"
test -L "$MNT/Applications" && test "$(readlink "$MNT/Applications")" = /Applications
plutil -lint "$MNT/NØDE CAD.app/Contents/Info.plist"
# Check the names Finder/LaunchServices actually use, and the bundle identity.
for key in CFBundleName CFBundleDisplayName; do
  test "$(/usr/libexec/PlistBuddy -c "Print :$key" "$MNT/NØDE CAD.app/Contents/Info.plist")" = "NØDE CAD"
done
for key in CFBundleExecutable CFBundleIconFile; do
  test "$(/usr/libexec/PlistBuddy -c "Print :$key" "$MNT/NØDE CAD.app/Contents/Info.plist")" = NODE-CAD
done
test "$(/usr/libexec/PlistBuddy -c 'Print :CFBundleIdentifier' "$MNT/NØDE CAD.app/Contents/Info.plist")" = "$MACOS_BUNDLE_ID"
test -s "$MNT/NØDE CAD.app/Contents/Resources/NODE-CAD.icns"
cmp "$ROOT/assets/platforms/macos/README.txt" "$MNT/README.txt"
case "$ARCH" in universal) WANT=(arm64 x86_64) ;; aarch64) WANT=(arm64) ;; *) WANT=(x86_64) ;; esac
for arch in "${WANT[@]}"; do
  lipo "$MNT/NØDE CAD.app/Contents/MacOS/NODE-CAD" -verify_arch "$arch"
done
test -s "$MNT/NØDE CAD.app/Contents/Resources/Licenses/THIRD-PARTY-LICENSES.txt"
# Not --strict: the mounted HFS volume adds Finder info, which strict verification rejects.
codesign --verify --deep --verbose=2 "$MNT/NØDE CAD.app"
"$MNT/NØDE CAD.app/Contents/MacOS/NODE-CAD" --version
hdiutil detach "$MNT"

# ---- CLI ---------------------------------------------------------------------------------------
echo "==> building $CLI_ZIP"
CLI_DIR="$WORK/node-cad-cli-$VERSION-mac-$ARCH"
mkdir -p "$CLI_DIR"
cp "$WORK/bin/node-cad-cli" "$CLI_DIR/"
codesign --force --sign - --timestamp=none --identifier "$MACOS_BUNDLE_ID-cli" "$CLI_DIR/node-cad-cli"
# Compatibility shim (docs/compatibility.md): the CLI's former names forward to node-cad-cli.
for alias in solidcraft-cli solvecraft-cli; do
  cp "$WORK/bin/$alias" "$CLI_DIR/"
  codesign --force --sign - --timestamp=none --identifier "$MACOS_BUNDLE_ID-cli-alias" "$CLI_DIR/$alias"
done
copy_docs "$CLI_DIR"
rm -f "$CLI_ZIP"
ditto -c -k --keepParent "$CLI_DIR" "$CLI_ZIP"

"$WORK/bin/node-cad-cli" --version
echo "==> done"
ls -lh "$DMG" "$CLI_ZIP"
