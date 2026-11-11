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
#
#   scripts/check-bundle.sh --lite [Rustcraft.app] [assets dir]   (M74)
#   The lite app (bundle-macos.sh --lite) must hold none of Mojang's files,
#   packs or mod jars (no vanilla/ or downloads/ folder, no .ogg, no .jar, no
#   pack named in assets/data/downloads.ron), the other asset folders must match,
#   and a copy started from / with an empty home folder must show the
#   first-launch screen (checked by its log line), without touching the network.
set -eu
cd "$(dirname "$0")/.."
LITE=0
if [ "${1:-}" = "--lite" ]; then LITE=1; shift; fi
if [ $LITE = 1 ]; then DEFAULT_APP=target/release/bundle-lite/Rustcraft.app; else DEFAULT_APP=target/release/bundle/Rustcraft.app; fi
APP=${1:-$DEFAULT_APP}
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

if [ $LITE = 1 ]; then FOLDERS="textures data structures skins"; else FOLDERS="vanilla downloads textures data structures skins"; fi
for folder in $FOLDERS; do
  want=$(count "$SRC/$folder")
  have=$(count "$RES/$folder")
  if [ "$want" != "$have" ]; then
    echo "error: $folder: $want files in $SRC, $have in the app" >&2
    failed=1
  else
    echo "ok       $folder: $have files"
  fi
done
if [ $LITE = 0 ] && [ ! -d "$RES/vanilla/assets/minecraft" ]; then
  echo "error: the app has no vanilla layer (assets/vanilla)" >&2
  failed=1
fi
if [ $LITE = 1 ]; then
  # Nothing of Mojang's, the packs or the jars anywhere in the app.
  for folder in vanilla downloads; do
    if [ -e "$RES/$folder" ]; then
      echo "error: the lite app has an assets/$folder folder" >&2
      failed=1
    fi
  done
  stray=$(find "$APP" \( -name '*.ogg' -o -name '*.jar' -o -name 'sounds.json' -o -name 'client.jar*' \) | head -5)
  names=$(sh scripts/check-downloads.sh --list | sed 's/^[a-z]* //')
  while IFS= read -r name; do
    [ -n "$name" ] || continue
    hit=$(find "$APP" -name "$name" | head -1)
    [ -z "$hit" ] || stray="$stray
$hit"
  done <<LISTEND
$names
LISTEND
  if [ -n "$stray" ]; then
    echo "error: the lite app holds files that must be downloaded instead:" >&2
    echo "$stray" >&2
    failed=1
  else
    echo "ok       no Mojang file, pack or mod jar inside"
  fi
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
if [ $LITE = 1 ]; then
  # Started from / with an empty home folder and no network (the setup server
  # is an unused local port): it must open the first-launch screen.
  HOME_DIR="$TMP/home"; mkdir -p "$HOME_DIR"
  LOG="$TMP/lite.log"
  # exec: the background job is the app itself, so kill reaches it.
  (cd / && exec env -u RUSTCRAFT_ASSETS -u RUSTCRAFT_NO_DOWNLOAD HOME="$HOME_DIR" RUST_LOG=info \
     RUSTCRAFT_SETUP_BASE=http://127.0.0.1:9 "$TMP/Rustcraft.app/Contents/MacOS/rustcraft" >"$LOG" 2>&1) &
  PID=$!
  seen=0
  for _ in $(seq 1 60); do
    if grep -q "first-launch screen: window opened" "$LOG" 2>/dev/null; then seen=1; break; fi
    kill -0 "$PID" 2>/dev/null || break
    sleep 0.5
  done
  kill "$PID" 2>/dev/null || true
  wait "$PID" 2>/dev/null || true
  if [ $seen = 1 ]; then
    echo "ok       the lite app shows the first-launch screen"
  else
    echo "error: the lite app, started from / with an empty home, did not show the first-launch screen; its log:" >&2
    tail -15 "$LOG" >&2
    failed=1
  fi
elif ! (cd / && env -u RUSTCRAFT_ASSETS "$TMP/Rustcraft.app/Contents/MacOS/rustcraft" --asset-report --exit) ; then
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
