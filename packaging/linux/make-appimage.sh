#!/usr/bin/env bash
# Builds PakPos-<version>-<arch>.AppImage from target/release.
#   packaging/linux/make-appimage.sh <version> <x86_64|aarch64> <out-dir>
# Needs appimagetool on PATH (or APPIMAGETOOL=/path/to/appimagetool).
# The app uses the system's OpenGL/EGL, xkbcommon and X11/Wayland libraries,
# like every other desktop app; only PakPos itself is inside the image.
set -euo pipefail
VERSION=$1 ARCH=$2 OUT=$3
ROOT=$(cd "$(dirname "$0")/../.." && pwd)
TOOL=${APPIMAGETOOL:-appimagetool}
APPDIR=$(mktemp -d)/PakPos.AppDir

mkdir -p "$APPDIR/usr/bin" "$APPDIR/usr/share/applications" \
  "$APPDIR/usr/share/icons/hicolor/256x256/apps" "$APPDIR/usr/share/licenses/pakpos/fonts" "$OUT"
install -m 755 "$ROOT/target/release/pakpos" "$ROOT/target/release/pakpos-cli" "$APPDIR/usr/bin/"
install -m 644 "$ROOT/packaging/linux/pakpos.desktop" "$APPDIR/usr/share/applications/pakpos.desktop"
install -m 644 "$ROOT/assets/icon-256.png" "$APPDIR/usr/share/icons/hicolor/256x256/apps/pakpos.png"
install -m 644 "$ROOT"/LICENSE-MIT "$ROOT"/LICENSE-APACHE "$APPDIR/usr/share/licenses/pakpos/"
install -m 644 "$ROOT"/assets/fonts/OFL-*.txt "$APPDIR/usr/share/licenses/pakpos/fonts/"
# appimagetool wants the desktop file and icon at the top level too.
cp "$ROOT/packaging/linux/pakpos.desktop" "$APPDIR/pakpos.desktop"
cp "$ROOT/assets/icon-256.png" "$APPDIR/pakpos.png"
ln -s pakpos.png "$APPDIR/.DirIcon"

# `PakPos.AppImage` starts the app; `PakPos.AppImage cli <args>` runs pakpos-cli.
cat > "$APPDIR/AppRun" <<'RUN'
#!/bin/sh
HERE=$(dirname "$(readlink -f "$0")")
if [ "${1:-}" = "cli" ]; then
  shift
  exec "$HERE/usr/bin/pakpos-cli" "$@"
fi
exec "$HERE/usr/bin/pakpos" "$@"
RUN
chmod 755 "$APPDIR/AppRun"

# No FUSE needed to build (CI runners and containers often lack it).
APPIMAGE_EXTRACT_AND_RUN=1 ARCH=$ARCH "$TOOL" ${APPIMAGETOOL_ARGS:-} --no-appstream "$APPDIR" "$OUT/PakPos-$VERSION-$ARCH.AppImage"
echo "built $OUT/PakPos-$VERSION-$ARCH.AppImage"
