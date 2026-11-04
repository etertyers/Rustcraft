#!/bin/sh
# Checks that the vanilla layer (scripts/fetch-vanilla-assets.sh) is complete
# enough to build an app with sound, the Minecraft font and every texture:
# textures, models, blockstates, items, lang/en_us.json, sounds.json, every
# .ogg file sounds.json names, font/default.json and every font page it names.
# Exits 1 and says what is missing; prints the counts when all is there.
#
#   scripts/check-vanilla.sh [assets dir]      (default: assets)
set -eu
ASSETS=${1:-"$(dirname "$0")/../assets"}
exec python3 -I - "$ASSETS" <<'PY'
import json, os, sys

assets = sys.argv[1]
mc = os.path.join(assets, "vanilla", "assets", "minecraft")
problems = []

def need_dir(name, minimum=1):
    path = os.path.join(mc, name)
    count = sum(len(files) for _, _, files in os.walk(path)) if os.path.isdir(path) else 0
    if count < minimum:
        problems.append(f"assets/vanilla has no {name}/ files")
    return count

def need_file(name):
    if not os.path.isfile(os.path.join(mc, name)):
        problems.append(f"assets/vanilla is missing {name}")
        return False
    return True

counts = {d: need_dir(d) for d in ("textures", "models", "blockstates", "items")}
need_file("lang/en_us.json")

# Every sound file sounds.json names.
named = set()
if need_file("sounds.json"):
    with open(os.path.join(mc, "sounds.json"), encoding="utf-8") as f:
        events = json.load(f)
    for event in events.values():
        for entry in event.get("sounds", []):
            if isinstance(entry, str):
                name, kind = entry, "sound"
            else:
                name, kind = entry.get("name", ""), entry.get("type", "sound")
            if kind == "event" or ":" in name and not name.startswith("minecraft:"):
                continue
            named.add(name.removeprefix("minecraft:"))
missing_sounds = sorted(n for n in named if not os.path.isfile(os.path.join(mc, "sounds", n + ".ogg")))
if missing_sounds:
    problems.append(
        f"assets/vanilla is missing {len(missing_sounds)} sound files (of {len(named)} named, "
        f"e.g. {missing_sounds[0]}.ogg): run scripts/fetch-vanilla-assets.sh")

# The font: default.json, the providers it references, and every bitmap page.
pages = set()
def collect(path, depth=0):
    full = os.path.join(mc, path)
    if not os.path.isfile(full):
        problems.append(f"assets/vanilla is missing {path}: run scripts/fetch-vanilla-assets.sh")
        return
    with open(full, encoding="utf-8") as f:
        data = json.load(f)
    for provider in data.get("providers", []):
        kind = provider.get("type")
        if kind == "reference" and depth < 4:
            collect("font/" + provider["id"].removeprefix("minecraft:") + ".json", depth + 1)
        elif kind == "bitmap":
            pages.add("textures/" + provider["file"].removeprefix("minecraft:"))
collect("font/default.json")
missing_pages = sorted(p for p in pages if not os.path.isfile(os.path.join(mc, p)))
if missing_pages:
    problems.append(f"assets/vanilla is missing {len(missing_pages)} font pages (e.g. {missing_pages[0]})")

if problems:
    for p in problems:
        print("error:", p, file=sys.stderr)
    sys.exit(1)
print(f"vanilla layer complete: {counts['textures']} textures, {counts['models']} models, "
      f"{counts['blockstates']} blockstates, {counts['items']} items, {len(named)} sound files, "
      f"{len(pages)} font pages")
PY
