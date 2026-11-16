#!/usr/bin/env bash
# Checks a web build (target/web/):
#  - everywhere but builtin/: nothing of Mojang's, no packs, no jars (the
#    repo's own assets.zip included): that is what makes the page safe to
#    host without the files;
#  - builtin/ (M75, the clone's own files for a default build): absent with
#    --lite; otherwise a manifest, every layer it names with the size it
#    says, vanilla zips holding only assets/minecraft/ (never data/ or
#    server/), packs and jars only the ones assets/data/downloads.ron pins,
#    and nothing else.
#
#   scripts/check-web-output.sh [--lite] [target/web]
set -euo pipefail
HERE=$(dirname "$0")
LITE=0; OUT=target/web
for a in "$@"; do case $a in --lite) LITE=1;; *) OUT=$a;; esac; done
python3 -I - "$OUT" "$LITE" "$HERE/../assets/data/downloads.ron" <<'PY'
import json, os, re, sys, zipfile

out, lite, ron = sys.argv[1], sys.argv[2] == "1", sys.argv[3]
bad = []

def suspicious(name):
    n = name.replace("\\", "/")
    return (n.startswith(("vanilla/", "downloads/")) or "/vanilla/" in n or "/downloads/" in n
            or "assets/minecraft/" in n or n.endswith((".jar", ".ogg"))
            or n in ("vanilla", "downloads"))

for root, dirs, files in os.walk(out):
    rel_root = os.path.relpath(root, out)
    if rel_root == "builtin" or rel_root.startswith("builtin" + os.sep):
        continue
    dirs[:] = [d for d in dirs if not (rel_root == "." and d == "builtin")]
    for f in files + dirs:
        rel = os.path.relpath(os.path.join(root, f), out)
        if suspicious(rel):
            bad.append(rel)
    for f in files:
        if f.endswith(".zip"):
            with zipfile.ZipFile(os.path.join(root, f)) as z:
                bad += [f"{f}:{n}" for n in z.namelist() if suspicious(n)]

builtin = os.path.join(out, "builtin")
if lite:
    if os.path.exists(builtin):
        bad.append("builtin/ (a --lite build has no built-in files)")
elif os.path.isdir(builtin):
    pinned = {"pack": set(), "mod": set()}
    for line in open(ron, encoding="utf-8"):
        if "name:" in line and not line.lstrip().startswith("//"):
            kind = re.search(r"kind: (\w+)", line)
            file = re.search(r'file: "([^"]*)"', line)
            if kind and file:
                pinned[kind.group(1).lower()].add(file.group(1))
    try:
        manifest = json.load(open(os.path.join(builtin, "manifest.json")))
    except (OSError, ValueError) as e:
        bad.append(f"builtin/manifest.json ({e})")
        manifest = {"layers": []}
    named = {"manifest.json"}
    for layer in manifest["layers"]:
        f = layer["file"]
        named.add(f)
        path = os.path.join(builtin, f)
        if not os.path.isfile(path):
            bad.append(f"builtin/{f} (named by the manifest, missing)")
            continue
        if os.path.getsize(path) != layer["size"]:
            bad.append(f"builtin/{f} (size differs from the manifest)")
        if layer["kind"] == "vanilla":
            with zipfile.ZipFile(path) as z:
                bad += [f"builtin/{f}:{n}" for n in z.namelist() if not n.startswith("assets/minecraft/")]
        else:
            base = os.path.basename(f)
            if base not in pinned[layer["kind"]]:
                bad.append(f"builtin/{f} (not pinned in assets/data/downloads.ron)")
    for root, _, files in os.walk(builtin):
        for f in files:
            rel = os.path.relpath(os.path.join(root, f), builtin)
            if rel not in named and f != ".DS_Store":
                bad.append(f"builtin/{rel} (not in the manifest)")
else:
    bad.append("builtin/ is missing (a default build has it; --lite is the one without)")

if bad:
    print("error: the web build holds something it shouldn't:", file=sys.stderr)
    for b in bad[:20]:
        print("  ", b, file=sys.stderr)
    sys.exit(1)
print("web output clean: " + ("lite, no built-in files" if lite else "built-in files are the game's own plus the pinned packs and jars"))
PY
