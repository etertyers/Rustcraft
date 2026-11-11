#!/bin/sh
# Prints, for each pinned resource pack and mod jar in assets/data/downloads.ron
# (the same list the game downloads on first launch, M74), "found" or
# "missing" (with its Modrinth page), and exits 1 when anything is missing.
# A file counts when its exact pinned name is in assets/downloads/; with
# --verify its sha512 must match too. The game fetches missing ones itself;
# nothing here downloads.
#
#   scripts/check-downloads.sh [--verify] [downloads dir]   (default: assets/downloads)
#   scripts/check-downloads.sh --list                       (kind and file name of each pinned file)
set -eu
HERE=$(dirname "$0")
exec python3 -I - "$HERE/../assets/data/downloads.ron" "$@" <<'PY'
import hashlib, os, re, sys

ron, args = sys.argv[1], sys.argv[2:]
verify = "--verify" in args
args = [a for a in args if a != "--verify"]
here = os.path.dirname(ron)
entries = []
with open(ron, encoding="utf-8") as f:
    for line in f:
        line = line.split("//")[0] if line.lstrip().startswith("//") else line
        if "name:" not in line:
            continue
        fields = dict(re.findall(r'(\w+): "([^"]*)"', line))
        fields["kind"] = re.search(r"kind: (\w+)", line).group(1).lower()
        entries.append(fields)

if args and args[0] == "--list":
    for e in entries:
        print(e["kind"], e["file"])
    sys.exit(0)

directory = args[0] if args else os.path.normpath(os.path.join(here, "..", "downloads"))
folder = {"pack": "resourcepacks", "mod": "mods"}
missing = 0
for e in entries:
    path = os.path.join(directory, folder[e["kind"]], e["file"])
    ok = os.path.isfile(path)
    if ok and verify:
        with open(path, "rb") as f:
            ok = hashlib.sha512(f.read()).hexdigest() == e["sha512"]
    if ok:
        print(f'found    {e["kind"]:<8} {e["name"]:<30} {e["file"]}')
    else:
        print(f'missing  {e["kind"]:<8} {e["name"]:<30} {e["page"]}')
        missing += 1
if missing:
    print(f"{missing} of {len(entries)} downloads missing in {directory}", file=sys.stderr)
    sys.exit(1)
print(f"all {len(entries)} downloads found in {directory}")
PY
