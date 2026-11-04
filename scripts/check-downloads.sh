#!/bin/bash
# Prints, for each expected resource pack and mod jar in assets/downloads/,
# "found" (with the file name, which carries the version) or "missing" (with
# a Modrinth link), and exits 1 when anything is missing. Matched by name,
# ignoring case, spaces, dashes, plus signs and versions. Kenzo downloads the
# files by hand; nothing here fetches them.
#
#   scripts/check-downloads.sh [downloads dir]     (default: assets/downloads)
set -u
DIR=${1:-"$(dirname "$0")/../assets/downloads"}
missing=0

# Lower-case letters only: "Bare Bones x Fresh Animations 1.10.4.zip" becomes
# "barebonesxfreshanimationszip".
norm() { printf '%s' "$1" | tr 'A-Z' 'a-z' | tr -cd 'a-z'; }

# find_file <folder> <must have...> -- <must not have...>
find_file() {
  local folder=$1; shift
  local want=() deny=() deny_mode=0
  for word in "$@"; do
    if [ "$word" = "--" ]; then deny_mode=1; elif [ $deny_mode = 1 ]; then deny+=("$word"); else want+=("$word"); fi
  done
  [ -d "$folder" ] || return 1
  local entry name n ok w
  for entry in "$folder"/*; do
    [ -e "$entry" ] || continue
    name=$(basename "$entry")
    n=$(norm "$name"); ok=1
    for w in ${want[@]+"${want[@]}"}; do case $n in *"$w"*) ;; *) ok=0 ;; esac; done
    for w in ${deny[@]+"${deny[@]}"}; do case $n in *"$w"*) ok=0 ;; esac; done
    if [ $ok = 1 ]; then printf '%s' "$name"; return 0; fi
  done
  return 1
}

# report <kind> <title> <folder> <link> <words...>
report() {
  local kind=$1 title=$2 folder=$3 link=$4; shift 4
  local file
  if file=$(find_file "$folder" "$@"); then
    printf 'found    %-8s %-30s %s\n' "$kind" "$title" "$file"
  else
    printf 'missing  %-8s %-30s %s\n' "$kind" "$title" "$link"
    missing=$((missing + 1))
  fi
}

P="$DIR/resourcepacks"; M="$DIR/mods"
pack() { echo "https://modrinth.com/resourcepacks?q=$1"; }
mod() { echo "https://modrinth.com/mods?q=$1"; }
report pack "Bare Bones" "$P" "$(pack Bare+Bones)" barebones -- freshanimations leaves
report pack "Barebones Better Leaves" "$P" "$(pack Barebones+Better+Leaves)" barebones leaves
report pack "Fresh Animations" "$P" "$(pack Fresh+Animations)" freshanimations -- barebones
report pack "Bare Bones x Fresh Animations" "$P" "$(pack Bare+Bones+x+Fresh+Animations)" barebones freshanimations
report pack "Patrix 32x" "$P" "https://modrinth.com/resourcepack/patrix-32x" patrix
report mod "Not Enough Animations" "$M" "$(mod Not+Enough+Animations)" notenoughanimations
report mod "Visuality" "$M" "$(mod Visuality)" visuality
report mod "Cave Dust" "$M" "$(mod Cave+Dust)" cavedust
report mod "Particle Rain" "$M" "$(mod Particle+Rain)" particlerain
report mod "Make Bubbles Pop" "$M" "$(mod Make+Bubbles+Pop)" makebubblespop
report mod "Wakes" "$M" "$(mod Wakes)" wakes
report mod "Presence Footsteps" "$M" "$(mod Presence+Footsteps)" presencefootsteps
report mod "Sound Physics Remastered" "$M" "$(mod Sound+Physics+Remastered)" soundphysicsremastered
report mod "Ambient Sounds" "$M" "$(mod Ambient+Sounds)" ambientsounds

if [ $missing -gt 0 ]; then
  echo "$missing of 14 downloads missing in $DIR" >&2
  exit 1
fi
echo "all 14 downloads found in $DIR"
