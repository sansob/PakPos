#!/usr/bin/env bash
# Builds the .deb, .rpm and AppImage from target/release into <out-dir>.
#   packaging/linux/build-packages.sh <version> <x86_64|aarch64> <out-dir>
# Needs cargo-deb, cargo-generate-rpm, desktop-file-validate and appimagetool
# (APPIMAGETOOL=/path/to/appimagetool).
set -euo pipefail
VERSION=$1 ARCH=$2 OUT=$3
cd "$(dirname "$0")/../.."
mkdir -p "$OUT"
cargo deb -p pakpos --no-build --no-strip -o "$OUT/"
# RPM versions cannot contain "-": 0.3.0-rc.2 -> 0.3.0~rc.2 (sorts before 0.3.0).
cargo generate-rpm -p crates/pakpos-app -o "$OUT/" -s "version = \"${VERSION//-/\~}\""
packaging/linux/make-appimage.sh "$VERSION" "$ARCH" "$OUT"
