#!/usr/bin/env bash
# Downloads vanilla Minecraft assets (branch 26.3 of InventivetalentDev/minecraft-assets)
# into assets/vanilla/assets/minecraft/: textures, models, blockstates, items,
# lang/en_us.json, sounds.json and sounds. They are the lowest fallback layer
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
  /assets/minecraft/lang/en_us.json
git -C "$DEST" checkout -q
echo "vanilla assets ready in $DEST/assets/minecraft"
