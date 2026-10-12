#!/usr/bin/env bash
# Regenerates every app icon from the SVG sources in src-tauri/icons/source/:
#   desktop.svg     macOS-grid master -> `tauri icon` (png, icns, ico, Windows tiles)
#   foreground.svg  Android adaptive foreground (mark inside the safe circle, transparent)
#   legacy.svg      Android pre-26 launcher (rounded square)
#   round.svg       Android round launcher
# Android icons go to src-tauri/icons/android and into the committed Gradle project
# (src-tauri/gen/android), which is what the APK is built from. Needs rsvg-convert and magick.
set -euo pipefail
cd "$(dirname "$0")/.."
SRC=src-tauri/icons/source
TMP=$(mktemp -d "${TMPDIR:-$PWD/.icons-tmp}.XXXX" 2>/dev/null || mktemp -d)
trap 'rm -rf "$TMP"' EXIT
BG='#1C212B'

rsvg-convert -w 1024 -h 1024 "$SRC/desktop.svg" -o "$TMP/desktop.png"
pnpm -s tauri icon "$TMP/desktop.png" -o "$TMP/out" >/dev/null
find "$TMP/out" -maxdepth 1 -type f -exec cp {} src-tauri/icons/ \;

declare -A LEGACY=([mdpi]=48 [hdpi]=72 [xhdpi]=96 [xxhdpi]=144 [xxxhdpi]=192)
declare -A FORE=([mdpi]=108 [hdpi]=162 [xhdpi]=216 [xxhdpi]=324 [xxxhdpi]=432)
for res in src-tauri/icons/android src-tauri/gen/android/app/src/main/res; do
  for d in "${!LEGACY[@]}"; do
    mkdir -p "$res/mipmap-$d"
    rsvg-convert -w "${LEGACY[$d]}" -h "${LEGACY[$d]}" "$SRC/legacy.svg" -o "$res/mipmap-$d/ic_launcher.png"
    rsvg-convert -w "${LEGACY[$d]}" -h "${LEGACY[$d]}" "$SRC/round.svg" -o "$res/mipmap-$d/ic_launcher_round.png"
    rsvg-convert -w "${FORE[$d]}" -h "${FORE[$d]}" "$SRC/foreground.svg" -o "$res/mipmap-$d/ic_launcher_foreground.png"
  done
  mkdir -p "$res/mipmap-anydpi-v26" "$res/values"
  for name in ic_launcher ic_launcher_round; do
    cat > "$res/mipmap-anydpi-v26/$name.xml" <<XML
<?xml version="1.0" encoding="utf-8"?>
<adaptive-icon xmlns:android="http://schemas.android.com/apk/res/android">
  <background android:drawable="@color/ic_launcher_background"/>
  <foreground android:drawable="@mipmap/ic_launcher_foreground"/>
  <monochrome android:drawable="@mipmap/ic_launcher_foreground"/>
</adaptive-icon>
XML
  done
  cat > "$res/values/ic_launcher_background.xml" <<XML
<?xml version="1.0" encoding="utf-8"?>
<resources>
  <color name="ic_launcher_background">$BG</color>
</resources>
XML
done
echo "icons regenerated"
