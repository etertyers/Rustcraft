#!/bin/sh
# Checks a built Rustcraft.app against the folders it was built from: the
# number of files in each asset folder must match; the binary must not contain
# this repo's absolute path (a path baked in at build time makes the app work
# only on the Mac that built it); and a copy of the app, started from / (as
# the Finder does) in a temp folder while this repo's assets/ is renamed away,
# must report no silent sound event, no missing texture and a Minecraft font
# (--asset-report --exit).
#
#   scripts/check-bundle.sh [Rustcraft.app] [assets dir]
#   (default: target/release/bundle/Rustcraft.app and assets)
set -eu
cd "$(dirname "$0")/.."
APP=${1:-target/release/bundle/Rustcraft.app}
SRC=${2:-assets}
[ -d "$APP" ] || { echo "error: $APP doesn't exist" >&2; exit 1; }
APP=$(cd "$APP" && pwd)
RES="$APP/Contents/Resources/assets"
failed=0

# Files in a folder, ignoring what bundle-macos.sh leaves out.
count() {
  [ -d "$1" ] || { echo 0; return; }
  find "$1" -type f ! -name .DS_Store ! -name '*.svg' ! -path '*/.git/*' | wc -l | tr -d ' '
}

for folder in vanilla downloads textures data structures skins; do
  want=$(count "$SRC/$folder")
  have=$(count "$RES/$folder")
  if [ "$want" != "$have" ]; then
    echo "error: $folder: $want files in $SRC, $have in the app" >&2
    failed=1
  else
    echo "ok       $folder: $have files"
  fi
done
if [ ! -d "$RES/vanilla/assets/minecraft" ]; then
  echo "error: the app has no vanilla layer (assets/vanilla)" >&2
  failed=1
fi

BIN="$APP/Contents/MacOS/rustcraft"
if [ ! -x "$BIN" ]; then
  echo "error: $BIN is missing" >&2
  exit 1
fi
# 1. No build-machine path in the binary.
REPO=$(pwd -P)
if strings -a "$BIN" | grep -qF "$REPO"; then
  echo "error: the binary contains this repo's path ($REPO); some loading uses a path baked in at build time:" >&2
  strings -a "$BIN" | grep -F "$REPO" | sort -u | head -5 >&2
  failed=1
else
  echo "ok       no build-machine path in the binary"
fi

# 2. Run a copy with the repo's assets/ unreachable.
TMP=$(mktemp -d "${TMPDIR:-/tmp}/rustcraft-bundle-check.XXXXXX")
HIDDEN="$REPO/assets.hidden-by-check-bundle"
restore() {
  if [ -d "$HIDDEN" ] && [ ! -e "$REPO/assets" ]; then mv "$HIDDEN" "$REPO/assets"; fi
  rm -rf "$TMP"
}
trap restore EXIT INT TERM
cp -cR "$APP" "$TMP/Rustcraft.app" 2>/dev/null || cp -R "$APP" "$TMP/Rustcraft.app"
if [ -d "$REPO/assets" ]; then mv "$REPO/assets" "$HIDDEN"; fi
if ! (cd / && env -u RUSTCRAFT_ASSETS "$TMP/Rustcraft.app/Contents/MacOS/rustcraft" --asset-report --exit) ; then
  echo "error: the copied app's asset report found problems with the repo unreachable (above)" >&2
  failed=1
fi
restore
trap - EXIT INT TERM
if [ $failed = 1 ]; then
  echo "bundle check FAILED" >&2
  exit 1
fi
echo "bundle check passed"
