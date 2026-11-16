#!/usr/bin/env bash
# Builds the browser version into target/web/:
#   cargo (wasm32) -> wasm-bindgen -> wasm-opt -O3, plus web/index.html,
#   web/loader.js, web/builtin.js, web/worker.js and assets.zip (the repo's own
#   assets).
#
# By default (M75) it also puts THIS CLONE's Minecraft files (assets/vanilla's
# game files, never its data/ or server/), resource packs and mod jars
# (assets/downloads) in target/web/builtin/ as versioned archives, so the page
# opens straight to the title screen with no folder picker. target/ is
# git-ignored: those files are never committed. If the clone's files are
# incomplete it fetches them first with the native downloader and no window
# (cargo run --release -- --download-only) and stops with its one-line reason
# when that fails. --lite builds without them and keeps the folder-picker
# setup screen. assets.zip never holds them.
#
#   scripts/build-web.sh [--lite] [--debug] [--no-opt] [--names]
#   scripts/build-web.sh --assets-only    only target/web/builtin (tests; no wasm build)
#   RUSTCRAFT_ASSETS=<dir>                use that assets folder instead of ./assets
#   RUSTCRAFT_DOWNLOADER="<cmd>"          the downloader command (default: cargo run --release -- --download-only)
set -euo pipefail
cd "$(dirname "$0")/.."
PROFILE=release; FLAGS=(--release); OPT=1; NAMES=(); LITE=0; ASSETS_ONLY=0
for a in "$@"; do case $a in --debug) PROFILE=debug; FLAGS=();; --no-opt) OPT=0;; --names) NAMES=(--keep-debug);; --lite) LITE=1;; --assets-only) ASSETS_ONLY=1;; esac; done
OUT=${RUSTCRAFT_WEB_OUT:-target/web}
ASSETS=${RUSTCRAFT_ASSETS:-assets}
# Empty the folder rather than remove it, so a running serve-web.sh keeps working.
mkdir -p "$OUT"; find "$OUT" -mindepth 1 -delete; mkdir -p "$OUT/pkg"

# Are this clone's files complete (the rules of check-vanilla.sh and
# check-downloads.sh; .skip-packs means "Minecraft's files only")?
files_complete() {
  scripts/check-vanilla.sh "$ASSETS" >/dev/null 2>&1 || return 1
  [ -f "$ASSETS/downloads/.skip-packs" ] || scripts/check-downloads.sh "$ASSETS/downloads" >/dev/null 2>&1 || return 1
}

# Fills the clone with the native downloader when something is missing.
ensure_files() {
  files_complete && return 0
  echo "build-web: this clone's Minecraft files or packs are incomplete; fetching them (no window)..."
  local log; log=$(mktemp)
  if ! sh -c "${RUSTCRAFT_DOWNLOADER:-cargo run --release -- --download-only}" >"$log" 2>&1; then
    local reason
    reason=$(grep -E '^(Error|error)' "$log" | tail -1 || true)
    [ -n "$reason" ] || reason=$(grep -v '^[[:space:]]*$' "$log" | tail -1 || true)
    echo "error: couldn't fetch Minecraft's files: ${reason:-the downloader failed}" >&2
    echo "       (scripts/build-web.sh --lite builds without them and asks for the files in the browser)" >&2
    exit 1
  fi
  if ! files_complete; then
    local why
    why=$(scripts/check-vanilla.sh "$ASSETS" 2>&1 | grep -E '^error' | head -1 || true)
    echo "error: ${why:-some packs or jars of this clone are still missing} after the download (scripts/check-vanilla.sh, scripts/check-downloads.sh)" >&2
    exit 1
  fi
}

if [ "$LITE" = 0 ]; then
  ensure_files
  python3 scripts/web_builtin.py --assets "$ASSETS" --out "$OUT/builtin" || exit $?
fi
if [ "$ASSETS_ONLY" = 1 ]; then
  scripts/check-web-output.sh $([ "$LITE" = 1 ] && echo --lite) "$OUT"
  exit 0
fi

cargo rustc --lib "${FLAGS[@]}" --target wasm32-unknown-unknown --crate-type cdylib
WASM=target/wasm32-unknown-unknown/$PROFILE/rustcraft.wasm
wasm-bindgen "$WASM" --target web --out-dir "$OUT/pkg" --out-name rustcraft --no-typescript "${NAMES[@]+"${NAMES[@]}"}"
if [ "$OPT" = 1 ] && [ "$PROFILE" = release ]; then
  wasm-opt -O3 ${NAMES:+-g} --enable-bulk-memory --enable-nontrapping-float-to-int --enable-sign-ext --enable-simd \
    --enable-mutable-globals --enable-reference-types -o "$OUT/pkg/rustcraft_bg.opt.wasm" "$OUT/pkg/rustcraft_bg.wasm"
  mv "$OUT/pkg/rustcraft_bg.opt.wasm" "$OUT/pkg/rustcraft_bg.wasm"
fi
cp web/index.html web/loader.js web/builtin.js web/worker.js "$OUT/"

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

# Check the output: only what this build means to hold (see check-web-output.sh).
scripts/check-web-output.sh $([ "$LITE" = 1 ] && echo --lite) "$OUT"
NOTE=""; [ "$LITE" = 1 ] && NOTE=", lite: no built-in Minecraft files"
SIZE=$(du -sh "$OUT" | cut -f1)
echo "built $OUT (${SIZE}${NOTE}); serve it with scripts/serve-web.sh"
