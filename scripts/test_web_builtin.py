#!/usr/bin/env python3
"""Tests for the web build's built-in files (M75): python3 scripts/test_web_builtin.py

Builds fake clones in a temp folder (tiny files, the real rules of
scripts/check-vanilla.sh and check-downloads.sh) and checks the layer builder
and scripts/build-web.sh --assets-only: a default build holds the vanilla
layer and the packs, --lite holds none of them, an incomplete clone with a
downloader that fails stops with the downloader's reason.
"""
import json
import os
import re
import shutil
import stat
import subprocess
import sys
import tempfile
import unittest
import zipfile
from pathlib import Path

HERE = Path(__file__).resolve().parent
ROOT = HERE.parent
sys.path.insert(0, str(HERE))
import web_builtin  # noqa: E402

DOWNLOADS_RON = ROOT / "assets" / "data" / "downloads.ron"


def fake_clone(base, packs=True, sounds=True):
    """A clone's assets folder with a (tiny) complete vanilla layer."""
    assets = Path(base) / "assets"
    mc = assets / "vanilla" / "assets" / "minecraft"
    write = lambda rel, data=b"x": (mc / rel).parent.mkdir(parents=True, exist_ok=True) or (mc / rel).write_bytes(data)
    write("textures/block/stone.png")
    write("textures/font/ascii.png")
    write("models/block/stone.json", b"{}")
    write("blockstates/stone.json", b"{}")
    write("items/stick.json", b"{}")
    write("lang/en_us.json", b"{}")
    write("sounds.json", json.dumps({"menu": {"sounds": ["music/menu/menu1"]}, "step": {"sounds": ["block/stone/step1", "mob/pig/say1"]}}).encode())
    write("font/default.json", json.dumps({"providers": [{"type": "bitmap", "file": "minecraft:font/ascii.png"}]}).encode())
    for name in ("music/menu/menu1", "block/stone/step1", "mob/pig/say1"):
        write(f"sounds/{name}.ogg", b"o" * 100)
    # What a real layer also has, and must stay out:
    (assets / "vanilla" / "data" / "minecraft" / "worldgen").mkdir(parents=True)
    (assets / "vanilla" / "data" / "minecraft" / "worldgen" / "x.json").write_text("{}")
    (assets / "vanilla" / "server").mkdir()
    (assets / "vanilla" / "server" / "server.jar").write_bytes(b"jar")
    (assets / "data").mkdir()
    shutil.copy(DOWNLOADS_RON, assets / "data" / "downloads.ron")
    (assets / "downloads").mkdir()
    if packs:
        for line in DOWNLOADS_RON.read_text().splitlines():
            if "name:" in line and not line.lstrip().startswith("//"):
                kind = re.search(r"kind: (\w+)", line).group(1).lower()
                file = re.search(r'file: "([^"]*)"', line).group(1)
                d = assets / "downloads" / ("resourcepacks" if kind == "pack" else "mods")
                d.mkdir(exist_ok=True)
                (d / file).write_bytes(b"p" * 50)
    else:
        (assets / "downloads" / ".skip-packs").write_text("")
    return assets


def names(zip_path):
    with zipfile.ZipFile(zip_path) as z:
        return z.namelist()


class Builder(unittest.TestCase):
    def setUp(self):
        self.tmp = tempfile.TemporaryDirectory()
        self.addCleanup(self.tmp.cleanup)

    def test_the_layers_hold_the_game_files_the_packs_and_the_jars_but_nothing_else(self):
        assets = fake_clone(self.tmp.name)
        out = Path(self.tmp.name) / "out"
        m = web_builtin.build(assets, out)
        core = names(out / "vanilla-core.zip")
        self.assertIn("assets/minecraft/textures/block/stone.png", core)
        self.assertIn("assets/minecraft/sounds.json", core)
        self.assertIn("assets/minecraft/sounds/music/menu/menu1.ogg", core, "the menu music is in the core")
        self.assertNotIn("assets/minecraft/sounds/block/stone/step1.ogg", core)
        every = [n for f in out.glob("vanilla-*.zip") for n in names(f)]
        self.assertTrue(every and all(n.startswith("assets/minecraft/") for n in every), "no data/ or server/")
        self.assertIn("assets/minecraft/sounds/block/stone/step1.ogg", every)
        self.assertEqual(sum(l["kind"] == "pack" for l in m["layers"]), 5)
        self.assertEqual(sum(l["kind"] == "mod" for l in m["layers"]), 9)
        self.assertEqual(m["defaultStyle"], "barebones")
        boot = [l["kind"] for l in m["layers"] if l["phase"] == "boot"]
        self.assertEqual(boot[0], "vanilla", "the vanilla core comes first")
        self.assertTrue((out / "manifest.json").is_file())

    def test_the_stamp_follows_the_content_not_the_time(self):
        assets = fake_clone(self.tmp.name)
        a = web_builtin.build(assets, Path(self.tmp.name) / "a")["stamp"]
        b = web_builtin.build(assets, Path(self.tmp.name) / "b")["stamp"]
        self.assertEqual(a, b, "the same files give the same stamp, so a rebuild keeps the browser's cache")
        (assets / "vanilla" / "assets" / "minecraft" / "textures" / "block" / "stone.png").write_bytes(b"changed")
        c = web_builtin.build(assets, Path(self.tmp.name) / "c")["stamp"]
        self.assertNotEqual(a, c, "a changed file invalidates it")

    def test_skip_packs_builds_without_packs_and_jars_and_defaults_to_vanilla(self):
        assets = fake_clone(self.tmp.name, packs=False)
        m = web_builtin.build(assets, Path(self.tmp.name) / "out")
        self.assertEqual([l["kind"] for l in m["layers"] if l["kind"] != "vanilla"], [])
        self.assertEqual(m["defaultStyle"], "vanilla")

    def test_an_incomplete_vanilla_layer_or_a_missing_pack_is_refused_with_a_reason(self):
        assets = fake_clone(self.tmp.name)
        shutil.rmtree(assets / "vanilla" / "assets" / "minecraft" / "models")
        with self.assertRaises(web_builtin.Incomplete) as e:
            web_builtin.build(assets, Path(self.tmp.name) / "o1")
        self.assertEqual(e.exception.code, 3)
        self.assertIn("models/", str(e.exception))
        assets = fake_clone(self.tmp.name + "/second")
        next((assets / "downloads" / "resourcepacks").iterdir()).unlink()
        with self.assertRaises(web_builtin.Incomplete) as e:
            web_builtin.build(assets, Path(self.tmp.name) / "o2")
        self.assertEqual(e.exception.code, 4)


class BuildScript(unittest.TestCase):
    def setUp(self):
        self.tmp = tempfile.TemporaryDirectory()
        self.addCleanup(self.tmp.cleanup)

    def run_build(self, assets, *flags, downloader=None):
        env = dict(os.environ, RUSTCRAFT_ASSETS=str(assets), RUSTCRAFT_WEB_OUT=str(Path(self.tmp.name) / "web"))
        if downloader:
            env["RUSTCRAFT_DOWNLOADER"] = downloader
        return subprocess.run([str(ROOT / "scripts" / "build-web.sh"), "--assets-only", *flags], capture_output=True, text=True, cwd=ROOT, env=env)

    def test_a_default_build_has_the_vanilla_layer_and_the_packs(self):
        assets = fake_clone(self.tmp.name)
        r = self.run_build(assets)
        self.assertEqual(r.returncode, 0, r.stdout + r.stderr)
        out = Path(self.tmp.name) / "web" / "builtin"
        self.assertTrue((out / "vanilla-core.zip").is_file())
        self.assertTrue(any((out / "packs").iterdir()))
        self.assertIn("built-in files", r.stdout + r.stderr)

    def test_lite_has_none_of_them(self):
        assets = fake_clone(self.tmp.name)
        r = self.run_build(assets, "--lite")
        self.assertEqual(r.returncode, 0, r.stdout + r.stderr)
        self.assertFalse((Path(self.tmp.name) / "web" / "builtin").exists())

    def test_an_incomplete_clone_with_a_failing_downloader_stops_with_its_reason(self):
        assets = fake_clone(self.tmp.name)
        shutil.rmtree(assets / "vanilla" / "assets" / "minecraft" / "textures")
        fail = Path(self.tmp.name) / "fail.sh"
        fail.write_text("#!/bin/sh\necho 'download-only: starting'\necho 'Error: Modrinth is not reachable (test)' >&2\nexit 1\n")
        fail.chmod(fail.stat().st_mode | stat.S_IEXEC)
        r = self.run_build(assets, downloader=str(fail))
        self.assertEqual(r.returncode, 1)
        self.assertIn("couldn't fetch Minecraft's files: Error: Modrinth is not reachable (test)", r.stderr)
        self.assertFalse((Path(self.tmp.name) / "web" / "builtin").exists(), "nothing half-built is left")

    def test_the_output_check_refuses_a_stray_pack_in_the_built_in_folder(self):
        assets = fake_clone(self.tmp.name)
        self.assertEqual(self.run_build(assets).returncode, 0)
        out = Path(self.tmp.name) / "web"
        (out / "builtin" / "packs" / "NotPinned.zip").write_bytes(b"x")
        r = subprocess.run([str(ROOT / "scripts" / "check-web-output.sh"), str(out)], capture_output=True, text=True)
        self.assertEqual(r.returncode, 1)
        self.assertIn("NotPinned.zip", r.stderr)

    def test_a_downloader_that_fills_the_clone_lets_the_build_continue(self):
        assets = fake_clone(self.tmp.name)
        mc = assets / "vanilla" / "assets" / "minecraft"
        backup = Path(self.tmp.name) / "textures-backup"
        shutil.move(mc / "textures", backup)
        fix = Path(self.tmp.name) / "fix.sh"
        fix.write_text(f"#!/bin/sh\ncp -R '{backup}' '{mc}/textures'\n")
        fix.chmod(fix.stat().st_mode | stat.S_IEXEC)
        r = self.run_build(assets, downloader=str(fix))
        self.assertEqual(r.returncode, 0, r.stdout + r.stderr)
        self.assertIn("fetching them", r.stdout)
        self.assertTrue((Path(self.tmp.name) / "web" / "builtin" / "vanilla-core.zip").is_file())


if __name__ == "__main__":
    unittest.main()
