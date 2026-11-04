#!/usr/bin/env bash
# Downloads vanilla Minecraft assets (branch 26.3 of InventivetalentDev/minecraft-assets)
# into assets/vanilla/assets/minecraft/: textures, models, blockstates, items,
# lang/en_us.json, sounds.json, sounds and the font definitions (font/*.json). They are the lowest fallback layer
# (below Kenzo's assets and resource packs) and are git-ignored; never commit them.
# Re-runnable: an existing checkout is kept and only completed.
set -euo pipefail

REPO=https://github.com/InventivetalentDev/minecraft-assets
BRANCH=26.3
DEST="$(cd "$(dirname "$0")/.." && pwd)/assets/vanilla"

if [ ! -d "$DEST/.git" ]; then
  mkdir -p "$DEST"
  git clone --depth 1 --filter=blob:none --sparse -b "$BRANCH" "$REPO" "$DEST"
fi
git -C "$DEST" sparse-checkout set --no-cone \
  /assets/minecraft/textures/ /assets/minecraft/models/ \
  /assets/minecraft/blockstates/ /assets/minecraft/items/ \
  /assets/minecraft/sounds/ /assets/minecraft/sounds.json \
  /assets/minecraft/lang/en_us.json \
  /assets/minecraft/font/*.json /assets/minecraft/font/include/
git -C "$DEST" checkout -q
echo "vanilla assets ready in $DEST/assets/minecraft"

# World-gen data (M53): vanilla 1.21.11's density functions, noises, the
# overworld noise settings and the carvers (branch 1.21.11-data of
# misode/mcmeta), into assets/vanilla/data/minecraft/worldgen/. Only the tests
# and the reference evaluator read it; the game never needs it, and it is
# git-ignored with the rest of assets/vanilla. Re-runnable.
DATA_REPO=https://github.com/misode/mcmeta
DATA_BRANCH=1.21.11-data
DATA_DEST="$DEST/data/minecraft/worldgen"
if [ ! -f "$DATA_DEST/noise_settings/overworld.json" ]; then
  TMP="$(mktemp -d)"
  trap 'rm -rf "$TMP"' EXIT
  git clone --depth 1 --filter=blob:none --sparse -b "$DATA_BRANCH" "$DATA_REPO" "$TMP/mcmeta"
  git -C "$TMP/mcmeta" sparse-checkout set --no-cone \
    /data/minecraft/worldgen/density_function/ /data/minecraft/worldgen/noise/ \
    /data/minecraft/worldgen/noise_settings/overworld.json \
    /data/minecraft/worldgen/configured_carver/
  git -C "$TMP/mcmeta" checkout -q
  mkdir -p "$DATA_DEST"
  cp -R "$TMP/mcmeta/data/minecraft/worldgen/." "$DATA_DEST/"
fi
echo "vanilla world-gen data ready in $DATA_DEST"
