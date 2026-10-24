#!/usr/bin/env bash
# Repackage a NØDE CAD Linux tarball as a snap:
#
#   $DIST/node-cad-<version>-linux-<arch>.snap
#
# Usage: packaging/linux/snap-pack.sh [--destructive-mode] [TARBALL]
#
# TARBALL defaults to $DIST/node-cad-<version>-linux-<arch>.tar.gz from package.sh. The snap is
# built for the host architecture (x86_64 or aarch64), which must match the tarball's. Needs
# snapcraft (`sudo snap install snapcraft --classic`) and, without --destructive-mode, LXD.
# --destructive-mode builds on the host itself, which must be Ubuntu 24.04 (the core24 base),
# as in CI. Install and smoke-test the result with:
#   sudo snap install --dangerous dist/release/node-cad-<version>-linux-<arch>.snap
#   python3 packaging/linux/snap-smoke.py
# Manifest: packaging/linux/snap/snapcraft.yaml.
set -euo pipefail
# shellcheck source=../env.sh
. "$(dirname "${BASH_SOURCE[0]}")/../env.sh"
HERE="$ROOT/packaging/linux"

MODE=(--use-lxd)
TARBALL=""
while [ $# -gt 0 ]; do
  case "$1" in
    --destructive-mode) MODE=(--destructive-mode); shift ;;
    -h | --help) sed -n '2,15p' "$0"; exit 0 ;;
    -*) echo "unknown argument: $1" >&2; exit 2 ;;
    *) TARBALL="$1"; shift ;;
  esac
done

ARCH="$(uname -m)"
case "$ARCH" in
  x86_64) SNAP_ARCH=amd64 ;;
  aarch64 | arm64) ARCH=aarch64; SNAP_ARCH=arm64 ;;
  *) echo "unsupported architecture $ARCH" >&2; exit 2 ;;
esac
BASENAME="node-cad-$VERSION-linux-$ARCH"
TARBALL="${TARBALL:-$DIST/$BASENAME.tar.gz}"
[ -f "$TARBALL" ] || { echo "error: $TARBALL not found (run packaging/linux/package.sh --formats tar first)" >&2; exit 1; }
command -v snapcraft >/dev/null || { echo "error: snapcraft not found (sudo snap install snapcraft --classic)" >&2; exit 1; }
# Snap versions: at most 32 characters of [A-Za-z0-9.+~-].
if ! printf '%s' "$VERSION" | grep -Eq '^[A-Za-z0-9][A-Za-z0-9.+~-]{0,31}$'; then
  echo "error: version $VERSION is not a valid snap version" >&2
  exit 1
fi

echo "==> NODE CAD $VERSION snap for $SNAP_ARCH from $(basename "$TARBALL")"

WORK="$CARGO_TARGET_DIR/snap-pack"
rm -rf "$WORK"
mkdir -p "$WORK/snap" "$WORK/stage"
tar -xzf "$TARBALL" -C "$WORK/stage" --strip-components=1
[ -x "$WORK/stage/bin/node-cad" ] || { echo "error: $TARBALL has no bin/node-cad" >&2; exit 1; }
cp "$HERE/snap/snapcraft.yaml" "$WORK/snap/snapcraft.yaml"
printf '%s\n' "$VERSION" >"$WORK/version.txt"

(cd "$WORK" && snapcraft pack "${MODE[@]}" --build-for="$SNAP_ARCH" --output="$WORK/out.snap")

OUT="$DIST/$BASENAME.snap"
mv "$WORK/out.snap" "$OUT"
echo "wrote $OUT"
ls -lh "$OUT"
