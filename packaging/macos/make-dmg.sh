#!/usr/bin/env bash
# Builds PakPos.app (universal binary) and a .dmg.
# Usage: packaging/macos/make-dmg.sh <version> <out-dir>
# Expects target/{aarch64,x86_64}-apple-darwin/release/{pakpos,pakpos-cli}.
set -euo pipefail
VERSION="$1"
OUT="$2"
ROOT="$(cd "$(dirname "$0")/../.." && pwd)"
STAGE="$(mktemp -d)"
APP="$STAGE/PakPos.app"

mkdir -p "$APP/Contents/MacOS" "$APP/Contents/Resources" "$OUT"
for bin in pakpos pakpos-cli; do
  lipo -create \
    "$ROOT/target/aarch64-apple-darwin/release/$bin" \
    "$ROOT/target/x86_64-apple-darwin/release/$bin" \
    -output "$APP/Contents/MacOS/$bin"
done
sed "s/__VERSION__/$VERSION/g" "$ROOT/packaging/macos/Info.plist" > "$APP/Contents/Info.plist"
cp "$ROOT/assets/icon.icns" "$APP/Contents/Resources/icon.icns"

# Ad-hoc signature so Apple Silicon runs it; users still see the
# "unidentified developer" prompt until the project has a Developer ID.
codesign --force --deep --sign - "$APP" || true

ln -s /Applications "$STAGE/Applications"
hdiutil create -volname "PakPos $VERSION" -srcfolder "$STAGE" -ov -format UDZO \
  "$OUT/PakPos-$VERSION-macos-universal.dmg"

# Plain CLI archive for Homebrew / CI use.
tar -C "$APP/Contents/MacOS" -czf "$OUT/pakpos-cli-$VERSION-macos-universal.tar.gz" pakpos-cli
