#!/usr/bin/env bash
# Runs inside a clean distribution container after installing the PakPos
# package (or with SMOKE_APPIMAGE=<file>): the CLI must run, and the app must
# start and draw a window.
set -euo pipefail
if [ -n "${SMOKE_APPIMAGE:-}" ]; then
  # The AppImage: `cli` runs pakpos-cli, no argument starts the app.
  CLI=("$SMOKE_APPIMAGE" cli) APP=("$SMOKE_APPIMAGE")
else
  CLI=(pakpos-cli) APP=(pakpos)
  test -f /usr/share/applications/pakpos.desktop
  test -f /usr/share/icons/hicolor/256x256/apps/pakpos.png
fi
"${CLI[@]}" --version
export HOME=$(mktemp -d) PAKPOS_HOME=$(mktemp -d)/ws PAKPOS_CONFIG=$(mktemp -d)
export PAKPOS_SCENE=settings PAKPOS_SCREENSHOT=${SMOKE_SCREENSHOT:-/tmp/pakpos.png} LIBGL_ALWAYS_SOFTWARE=1
timeout 90 xvfb-run -a -s "-screen 0 1440x900x24" "${APP[@]}"
test -s "$PAKPOS_SCREENSHOT"
echo "app started and drew a window: $PAKPOS_SCREENSHOT"
