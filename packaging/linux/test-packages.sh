#!/usr/bin/env bash
# Installs the .deb on Ubuntu 24.04 and the .rpm on Fedora 41, and runs the
# AppImage on Ubuntu, each in a clean container, then starts the app under Xvfb
# (smoke-test.sh). Screenshots go to <dist>/screenshots/.
#   packaging/linux/test-packages.sh <dist-dir>
set -euo pipefail
DIST=$(cd "$1" && pwd)
HERE=$(cd "$(dirname "$0")" && pwd)
mkdir -p "$DIST/screenshots"
run() { docker run --rm -v "$DIST:/dist" -v "$HERE/smoke-test.sh:/smoke.sh:ro" "$@"; }

echo "::group::.deb on Ubuntu 24.04"
run -e SMOKE_SCREENSHOT=/dist/screenshots/deb-ubuntu-24.04.png ubuntu:24.04 bash -c \
  'apt-get update -qq && DEBIAN_FRONTEND=noninteractive apt-get install -y -qq /dist/*.deb xvfb xauth >/dev/null && bash /smoke.sh'
echo "::endgroup::"

echo "::group::.rpm on Fedora 41"
run -e SMOKE_SCREENSHOT=/dist/screenshots/rpm-fedora-41.png fedora:41 bash -c \
  'dnf install -y -q /dist/*.rpm xorg-x11-server-Xvfb xauth >/dev/null && bash /smoke.sh'
echo "::endgroup::"

echo "::group::AppImage on Ubuntu 24.04"
# The AppImage brings PakPos only: give it the usual desktop libraries.
run -e APPIMAGE_EXTRACT_AND_RUN=1 -e SMOKE_SCREENSHOT=/dist/screenshots/appimage-ubuntu-24.04.png ubuntu:24.04 bash -c \
  'apt-get update -qq && DEBIAN_FRONTEND=noninteractive apt-get install -y -qq ca-certificates libegl1 libgl1 libxkbcommon0 libxkbcommon-x11-0 libx11-6 libxcursor1 libxi6 xvfb xauth >/dev/null && SMOKE_APPIMAGE=$(ls /dist/*.AppImage) bash /smoke.sh'
echo "::endgroup::"
