#!/usr/bin/env bash
# Build and package NØDE CAD for FreeBSD:
#
#   $DIST/node-cad-<version>-freebsd-x86_64.tar.gz   a /usr/local-style tree:
#       node-cad-<version>-freebsd-x86_64/{bin, share/applications, share/icons, share/mime,
#       share/metainfo, share/doc/node-cad}
#
# Install by copying the tree's contents into /usr/local:
#   tar -xzf node-cad-<version>-freebsd-x86_64.tar.gz --strip-components 1 -C /usr/local
#
# Usage: packaging/freebsd/package.sh [--skip-build]
# Needs: bash, cargo, and the packages installed by release.yml's FreeBSD job.
# Adapted from PhotoCraft's packaging (storytold/photocraft, Apache-2.0; see ATTRIBUTION.md).
set -euo pipefail
# shellcheck source=../env.sh
. "$(dirname "${BASH_SOURCE[0]}")/../env.sh"
LINUX="$ROOT/assets/platforms/linux"

SKIP_BUILD=0
while [ $# -gt 0 ]; do
  case "$1" in
    --skip-build) SKIP_BUILD=1; shift ;;
    -h | --help) sed -n '2,13p' "$0"; exit 0 ;;
    *) echo "unknown argument: $1" >&2; exit 2 ;;
  esac
done

# FreeBSD's uname says amd64; release file names use x86_64 like the Linux ones.
case "$(uname -m)" in
  amd64 | x86_64) ARCH=x86_64 ;;
  arm64 | aarch64) ARCH=aarch64 ;;
  *) echo "unsupported architecture $(uname -m)" >&2; exit 2 ;;
esac
BASENAME="node-cad-$VERSION-freebsd-$ARCH"
echo "==> NODE CAD $VERSION for FreeBSD $ARCH"

# The release VM has 12 GB; full parallelism on the biggest crates runs it out of memory.
export CARGO_BUILD_JOBS="${CARGO_BUILD_JOBS:-4}"
if [ "$SKIP_BUILD" = 0 ]; then
  (cd "$ROOT" && cargo build --release --locked -p solidcraft -p solidcraft-cli)
fi
WORK="$CARGO_TARGET_DIR/freebsd-package"
STAGE="$WORK/$BASENAME"
BIN="$CARGO_TARGET_DIR/release"
rm -rf "$WORK"

# FreeBSD's install(1) has no -D: create the directories first.
mkdir -p "$STAGE/bin" "$STAGE/share/applications" "$STAGE/share/mime/packages" \
  "$STAGE/share/metainfo" "$STAGE/share/icons" "$STAGE/share/doc/node-cad"
# solidcraft-cli, solvecraft-cli: the CLI's former names, forwarding to node-cad-cli
# (compatibility shim, docs/compatibility.md).
install -m 755 "$BIN/node-cad" "$BIN/node-cad-cli" "$BIN/solidcraft-cli" "$BIN/solvecraft-cli" "$STAGE/bin/"
strip "$STAGE/bin/node-cad" "$STAGE/bin/node-cad-cli" "$STAGE/bin/solidcraft-cli" "$STAGE/bin/solvecraft-cli" 2>/dev/null || true
# The desktop entry, MIME type, metainfo and icons are the freedesktop files Linux ships.
install -m 644 "$LINUX/$APP_ID.desktop" "$STAGE/share/applications/$APP_ID.desktop"
install -m 644 "$LINUX/$APP_ID.mime.xml" "$STAGE/share/mime/packages/$APP_ID.xml"
sed -e "s/@VERSION@/$VERSION/g" -e "s/@DATE@/$SOLIDCRAFT_BUILD_DATE/g" \
  "$LINUX/$APP_ID.metainfo.xml.in" >"$STAGE/share/metainfo/$APP_ID.metainfo.xml"
make_icons "$STAGE/bin/node-cad-cli" "$WORK/icons"
cp -R "$WORK/icons/hicolor" "$STAGE/share/icons/"
copy_docs "$STAGE/share/doc/node-cad"

for f in bin/node-cad bin/node-cad-cli bin/solidcraft-cli bin/solvecraft-cli "share/applications/$APP_ID.desktop" \
  "share/icons/hicolor/256x256/apps/$APP_ID.png" share/doc/node-cad/LICENSE-MIT \
  share/doc/node-cad/LICENSE-APACHE share/doc/node-cad/THIRD-PARTY-LICENSES.txt; do
  if [ ! -e "$STAGE/$f" ]; then echo "error: $f is missing from the package" >&2; exit 1; fi
done

tar -C "$WORK" -czf "$DIST/$BASENAME.tar.gz" "$BASENAME"
echo "wrote $DIST/$BASENAME.tar.gz"
"$STAGE/bin/node-cad-cli" --version
echo "==> done"
