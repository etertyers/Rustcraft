#!/bin/sh
# Checks a built Rustcraft.app against the folders it was built from: the
# number of files in each asset folder must match, and the app's own binary,
# started from / (as the Finder does), must report no silent sound event, no
# missing texture and a Minecraft font (--asset-report --exit).
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
if ! (cd / && "$BIN" --asset-report --exit) ; then
  echo "error: the app's own asset report found problems (above)" >&2
  failed=1
fi
if [ $failed = 1 ]; then
  echo "bundle check FAILED" >&2
  exit 1
fi
echo "bundle check passed"
