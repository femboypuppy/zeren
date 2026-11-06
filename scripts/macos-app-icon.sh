#!/usr/bin/env bash
# The app's icon, into a bundle's Contents/Resources:
#
#   Assets.car   dist/macos/Zeren.icon, which follows the system's light, dark,
#                clear and tinted looks on macOS 26 and later
#   zeren.icns   dist/macos/icon-1024.png, for earlier macOS and the dmg
#
#   scripts/macos-app-icon.sh <Contents/Resources>
#
# Compiling Zeren.icon takes Xcode 26 or later.
set -euo pipefail

ROOT="$(cd "$(dirname "$0")/.." && pwd)"
RESOURCES="$1"

# Command Line Tools have no actool; Xcode may be installed without being the
# selected developer directory.
if ! xcrun --find actool >/dev/null 2>&1; then
  export DEVELOPER_DIR=/Applications/Xcode.app/Contents/Developer
fi

WORK="$(mktemp -d)"
trap 'rm -rf "$WORK"' EXIT

ICONSET="$WORK/zeren.iconset"
mkdir "$ICONSET"
for size in 16 32 128 256 512; do
  sips -z "$size" "$size" "$ROOT/dist/macos/icon-1024.png" --out "$ICONSET/icon_${size}x${size}.png" >/dev/null
  retina=$((size * 2))
  sips -z "$retina" "$retina" "$ROOT/dist/macos/icon-1024.png" --out "$ICONSET/icon_${size}x${size}@2x.png" >/dev/null
done
iconutil -c icns "$ICONSET" -o "$RESOURCES/zeren.icns"

# actool also writes a Zeren.icns of its own, which would land on zeren.icns on
# a case-insensitive disk, so only its catalog is taken.
mkdir "$WORK/car"
xcrun actool "$ROOT/dist/macos/Zeren.icon" \
  --compile "$WORK/car" \
  --platform macosx \
  --minimum-deployment-target 13.0 \
  --app-icon Zeren \
  --output-partial-info-plist "$WORK/partial.plist" >/dev/null
mv "$WORK/car/Assets.car" "$RESOURCES/Assets.car"
