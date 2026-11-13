#!/usr/bin/env bash
# Builds the browser version into target/web/ (M71):
#   cargo (wasm32) -> wasm-bindgen -> wasm-opt -O3, plus web/index.html,
#   web/loader.js and assets.zip (the repo's own assets).
# Mojang's files (assets/vanilla), the packs and the mod jars (assets/downloads)
# never go in: the script refuses to copy them and checks the result.
#
#   scripts/build-web.sh [--debug] [--no-opt] [--names]   (--names keeps function names for profiling)
set -euo pipefail
cd "$(dirname "$0")/.."
PROFILE=release; FLAGS=(--release); OPT=1; NAMES=()
for a in "$@"; do case $a in --debug) PROFILE=debug; FLAGS=();; --no-opt) OPT=0;; --names) NAMES=(--keep-debug);; esac; done
OUT=target/web
# Empty the folder rather than remove it, so a running serve-web.sh keeps working.
mkdir -p "$OUT"; find "$OUT" -mindepth 1 -delete; mkdir -p "$OUT/pkg"

cargo rustc --lib "${FLAGS[@]}" --target wasm32-unknown-unknown --crate-type cdylib
WASM=target/wasm32-unknown-unknown/$PROFILE/rustcraft.wasm
wasm-bindgen "$WASM" --target web --out-dir "$OUT/pkg" --out-name rustcraft --no-typescript "${NAMES[@]+"${NAMES[@]}"}"
if [ "$OPT" = 1 ] && [ "$PROFILE" = release ]; then
  wasm-opt -O3 ${NAMES:+-g} --enable-bulk-memory --enable-nontrapping-float-to-int --enable-sign-ext --enable-simd \
    --enable-mutable-globals --enable-reference-types -o "$OUT/pkg/rustcraft_bg.opt.wasm" "$OUT/pkg/rustcraft_bg.wasm"
  mv "$OUT/pkg/rustcraft_bg.opt.wasm" "$OUT/pkg/rustcraft_bg.wasm"
fi
cp web/index.html web/loader.js web/worker.js "$OUT/"

# The repo's own assets, minus Mojang's files and Kenzo's downloads.
python3 -I - "$OUT/assets.zip" <<'PY'
import os, sys, zipfile
out = sys.argv[1]
SKIP = {"vanilla", "downloads"}
n = 0
with zipfile.ZipFile(out, "w", zipfile.ZIP_DEFLATED, compresslevel=6) as z:
    for root, dirs, files in os.walk("assets"):
        rel = os.path.relpath(root, "assets")
        top = rel.split(os.sep)[0]
        dirs[:] = [d for d in dirs if not (rel == "." and d in SKIP)]
        for f in files:
            if f == ".DS_Store":
                continue
            p = os.path.join(root, f)
            z.write(p, os.path.relpath(p, "assets"))
            n += 1
print(f"assets.zip: {n} files")
PY

# Check the output: nothing of Mojang's or Kenzo's in it.
scripts/check-web-output.sh "$OUT"
echo "built $OUT ($(du -sh "$OUT" | cut -f1)); serve it with scripts/serve-web.sh"
