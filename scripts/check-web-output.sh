#!/usr/bin/env bash
# Fails if the web build holds anything from assets/vanilla or assets/downloads.
#   scripts/check-web-output.sh [target/web]
set -euo pipefail
OUT=${1:-target/web}
python3 -I - "$OUT" <<'PY'
import os, sys, zipfile
out = sys.argv[1]
bad = []
def suspicious(name):
    n = name.replace("\\", "/")
    return (n.startswith(("vanilla/", "downloads/")) or "/vanilla/" in n or "/downloads/" in n
            or "assets/minecraft/" in n or n.endswith((".jar", ".ogg"))
            or n in ("vanilla", "downloads"))
for root, dirs, files in os.walk(out):
    for f in files + dirs:
        rel = os.path.relpath(os.path.join(root, f), out)
        if suspicious(rel):
            bad.append(rel)
    for f in files:
        if f.endswith(".zip"):
            with zipfile.ZipFile(os.path.join(root, f)) as z:
                bad += [f"{f}:{n}" for n in z.namelist() if suspicious(n)]
if bad:
    print("error: the web build must not contain Mojang's files, packs or mod jars:", file=sys.stderr)
    for b in bad[:20]:
        print("  ", b, file=sys.stderr)
    sys.exit(1)
print("web output clean: no vanilla layer, packs or mod jars")
PY
