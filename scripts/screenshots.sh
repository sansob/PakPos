#!/usr/bin/env bash
# Regenerates docs/screenshots/*.png using scripted scenes, a virtual X
# display and the offline mock API (no internet needed).
#
#   ./scripts/screenshots.sh            # all scenes
#   ./scripts/screenshots.sh overview   # one scene
#
# Requires: xvfb-run, python3, a built `target/release/pakpos` (or debug).
set -euo pipefail
cd "$(dirname "$0")/.."

BIN=${BIN:-target/release/pakpos}
[ -x "$BIN" ] || BIN=target/debug/pakpos
OUT=docs/screenshots
PORT=${PORT:-8787}
mkdir -p "$OUT"

python3 scripts/mock_httpbin.py "$PORT" >/dev/null 2>&1 &
MOCK=$!
trap 'kill $MOCK 2>/dev/null || true' EXIT
sleep 1

SCENES=("$@")
[ ${#SCENES[@]} -eq 0 ] && SCENES=(overview tests runner history oauth graphql websocket scripts environments curl snippet settings light)

for scene in "${SCENES[@]}"; do
  HOME_DIR=$(mktemp -d)
  echo "▸ $scene"
  PAKPOS_HOME="$HOME_DIR/workspace" \
  PAKPOS_DEMO_BASE_URL="http://127.0.0.1:$PORT" \
  PAKPOS_SCENE="$scene" \
  PAKPOS_SCREENSHOT="$OUT/$scene.png" \
  LIBGL_ALWAYS_SOFTWARE=1 \
    timeout 90 xvfb-run -a -s "-screen 0 1440x900x24" "$BIN" >/dev/null 2>&1 || echo "  (scene $scene failed)"
  rm -rf "$HOME_DIR"
done
echo "done → $OUT"
