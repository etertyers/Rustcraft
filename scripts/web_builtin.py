#!/usr/bin/env python3
"""Builds the web version's built-in files (M75) from this clone's own assets.

    scripts/web_builtin.py --assets assets --out target/web/builtin

Reads <assets>/vanilla/assets/minecraft (the game's files only: never
<assets>/vanilla/data or server/), <assets>/downloads/resourcepacks and
<assets>/downloads/mods, and writes into --out:

  manifest.json          the layers, their sizes and a stamp of the content
  vanilla-core.zip       everything the title screen needs (textures, models,
                         blockstates, items, font, lang, sounds.json, the menu
                         music and the UI clicks)
  vanilla-sounds-N.zip   the other sounds, in chunks, most needed first
  packs/<file>           each resource pack as it is
  mods/<file>            each mod jar as it is (the extras read their sounds
                         and particle textures; see docs/web.md)

Layers have a phase: "boot" ones (the vanilla core, the packs and jars under
20 MB) are loaded before the title screen,
"background" ones while the game runs, in manifest order. The zips are
deterministic (sorted, fixed dates), so the stamp changes only when a file
does, and the browser's cache (keyed by the stamp) stays valid across
rebuilds of the same files.

Exit 3 with a one-line reason when the vanilla layer is incomplete; exit 4
when a pack or jar named by assets/data/downloads.ron is missing (unless
downloads/.skip-packs says to build without them).
"""
import argparse
import hashlib
import json
import os
import re
import sys
import zipfile
from pathlib import Path

ZIP_DATE = (1980, 1, 1, 0, 0, 0)
# Sounds needed before the title screen is useful: the menu music and clicks.
BOOT_SOUND_PREFIXES = ("sounds/music/menu/", "sounds/random/", "sounds/ui/")
# Sounds that are heavy and rarely needed first go last.
LATE_SOUND_PREFIXES = ("sounds/records/", "sounds/music/")
CHUNK_BYTES = 48 << 20
# Mod jars whose size makes them background-last.
BIG_JAR_BYTES = 20 << 20
BIG_PACK_BYTES = 20 << 20
VANILLA_DIRS = ("textures", "models", "blockstates", "items")


class Incomplete(Exception):
    def __init__(self, code, text):
        super().__init__(text)
        self.code = code


def pinned(downloads_ron):
    """(kind, file) of every entry of assets/data/downloads.ron."""
    out = []
    for line in Path(downloads_ron).read_text(encoding="utf-8").splitlines():
        if "name:" not in line or line.lstrip().startswith("//"):
            continue
        fields = dict(re.findall(r'(\w+): "([^"]*)"', line))
        kind = re.search(r"kind: (\w+)", line)
        if kind and "file" in fields:
            out.append((kind.group(1).lower(), fields["file"]))
    return out


def vanilla_files(assets):
    root = Path(assets) / "vanilla" / "assets" / "minecraft"
    if not root.is_dir():
        raise Incomplete(3, f"{root} is missing: run scripts/fetch-vanilla-assets.sh")
    for d in VANILLA_DIRS:
        if not (root / d).is_dir() or not any((root / d).rglob("*.*")):
            raise Incomplete(3, f"assets/vanilla has no {d}/ files: run scripts/fetch-vanilla-assets.sh")
    for f in ("sounds.json", "lang/en_us.json", "font/default.json"):
        if not (root / f).is_file():
            raise Incomplete(3, f"assets/vanilla is missing {f}: run scripts/fetch-vanilla-assets.sh")
    files = []
    for path in sorted(root.rglob("*")):
        if path.is_file() and path.name != ".DS_Store":
            files.append((f"assets/minecraft/{path.relative_to(root).as_posix()}", path))
    return files


def write_zip(out, entries, deflate):
    method = zipfile.ZIP_DEFLATED if deflate else zipfile.ZIP_STORED
    with zipfile.ZipFile(out, "w", method, compresslevel=6 if deflate else None) as z:
        for name, path in entries:
            info = zipfile.ZipInfo(name, ZIP_DATE)
            info.compress_type = method
            info.external_attr = 0o644 << 16
            z.writestr(info, Path(path).read_bytes())


def sha(path):
    h = hashlib.sha256()
    with open(path, "rb") as f:
        for block in iter(lambda: f.read(1 << 20), b""):
            h.update(block)
    return h.hexdigest()


def chunks(entries):
    """Entries split into runs of at most CHUNK_BYTES, in order."""
    run, size = [], 0
    for e in entries:
        n = e[1].stat().st_size
        if run and size + n > CHUNK_BYTES:
            yield run
            run, size = [], 0
        run.append(e)
        size += n
    if run:
        yield run


def build(assets, out):
    assets, out = Path(assets), Path(out)
    files = vanilla_files(assets)
    downloads = assets / "downloads"
    skip = (downloads / ".skip-packs").exists()
    ron = assets / "data" / "downloads.ron"
    wanted = pinned(ron) if ron.is_file() else []
    packs, mods = [], []
    if not skip:
        for kind, name in wanted:
            path = downloads / ("resourcepacks" if kind == "pack" else "mods") / name
            if not path.is_file():
                raise Incomplete(4, f"{path} is missing (pinned in assets/data/downloads.ron): run cargo run --release -- --download-only")
            (packs if kind == "pack" else mods).append(path)
    out.mkdir(parents=True, exist_ok=True)
    for old in list(out.rglob("*")):
        if old.is_file():
            old.unlink()
    layers = []

    def add(name, kind, phase, file, count):
        path = out / file
        layers.append({"name": name, "kind": kind, "phase": phase, "file": file,
                       "size": path.stat().st_size, "files": count, "sha256": sha(path)})

    # boot: the vanilla core
    is_sound = lambda n: n.startswith("assets/minecraft/sounds/") and n.endswith(".ogg")
    rel = lambda n: n[len("assets/minecraft/"):]
    boot_sound = lambda n: rel(n).startswith(BOOT_SOUND_PREFIXES)
    core = [e for e in files if not is_sound(e[0]) or boot_sound(e[0])]
    write_zip(out / "vanilla-core.zip", core, deflate=True)
    add("Minecraft's files", "vanilla", "boot", "vanilla-core.zip", len(core))
    # boot: the small packs (the default style, Barebones, needs them)
    (out / "packs").mkdir(exist_ok=True)
    big_packs = []
    for p in packs:
        if p.stat().st_size >= BIG_PACK_BYTES:
            big_packs.append(p)
            continue
        (out / "packs" / p.name).write_bytes(p.read_bytes())
        add(p.name, "pack", "boot", f"packs/{p.name}", 1)
    # boot: the small mod jars (the extras read particle sprites from them when
    # the atlas is built, so they must be there before the game starts)
    (out / "mods").mkdir(exist_ok=True)
    big_mods = []
    for m in mods:
        if m.stat().st_size >= BIG_JAR_BYTES:
            big_mods.append(m)
            continue
        (out / "mods" / m.name).write_bytes(m.read_bytes())
        add(m.name, "mod", "boot", f"mods/{m.name}", 1)
    # background, in order: the gameplay sounds, big packs, the records and
    # the music, then the big jars (their sounds are looked up when played)
    sounds = [e for e in files if is_sound(e[0]) and not boot_sound(e[0])]
    gameplay = [e for e in sounds if not rel(e[0]).startswith(LATE_SOUND_PREFIXES)]
    late = [e for e in sounds if rel(e[0]).startswith(LATE_SOUND_PREFIXES)]
    n = 0
    for run in chunks(gameplay):
        n += 1
        write_zip(out / f"vanilla-sounds-{n}.zip", run, deflate=False)
        add(f"Sounds {n}", "vanilla", "background", f"vanilla-sounds-{n}.zip", len(run))
    for p in big_packs:
        (out / "packs" / p.name).write_bytes(p.read_bytes())
        add(p.name, "pack", "background", f"packs/{p.name}", 1)
    for run in chunks(late):
        n += 1
        write_zip(out / f"vanilla-sounds-{n}.zip", run, deflate=False)
        add(f"Sounds {n}", "vanilla", "background", f"vanilla-sounds-{n}.zip", len(run))
    for m in big_mods:
        (out / "mods" / m.name).write_bytes(m.read_bytes())
        add(m.name, "mod", "background", f"mods/{m.name}", 1)
    stamp = hashlib.sha256("".join(f"{l['file']}:{l['sha256']};" for l in layers).encode()).hexdigest()[:16]
    barebones = any("Bare Bones" in p.name or "Bare%20Bones" in p.name for p in packs)
    manifest = {"stamp": stamp, "defaultStyle": "barebones" if barebones else "vanilla", "layers": layers}
    (out / "manifest.json").write_text(json.dumps(manifest, indent=2) + "\n")
    return manifest


def main():
    ap = argparse.ArgumentParser()
    ap.add_argument("--assets", default="assets")
    ap.add_argument("--out", required=True)
    args = ap.parse_args()
    try:
        m = build(args.assets, args.out)
    except Incomplete as err:
        print(f"error: {err}", file=sys.stderr)
        sys.exit(err.code)
    total = sum(l["size"] for l in m["layers"])
    boot = sum(l["size"] for l in m["layers"] if l["phase"] == "boot")
    print(f"built-in files: {len(m['layers'])} layers, {total / 1e6:.0f} MB ({boot / 1e6:.0f} MB before the title screen), stamp {m['stamp']}")


if __name__ == "__main__":
    main()
