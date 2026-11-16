// The build script's refusal (M71, M75): scripts/check-web-output.sh must fail on
// anything of Mojang's, a pack or a jar outside builtin/; --lite builds must have
// no builtin/ at all and default builds must have one (the layer content itself is
// tested in scripts/test_web_builtin.py). Usage: node check-output.mjs
import { mkdtempSync, mkdirSync, writeFileSync } from "node:fs";
import { tmpdir } from "node:os";
import { join } from "node:path";
import { spawnSync } from "node:child_process";
import { ROOT } from "./lib.mjs";

const script = join(ROOT, "scripts/check-web-output.sh");
const run = (dir, ...flags) => spawnSync("bash", [script, ...flags, dir], { encoding: "utf8" }).status;
const fresh = () => mkdtempSync(join(tmpdir(), "web-out-"));
let bad = 0;
const expect = (name, got, want) => { const ok = got === want; if (!ok) bad++; console.log(`${ok ? "ok  " : "FAIL"} ${name}`); };

const clean = fresh();
mkdirSync(join(clean, "pkg")); writeFileSync(join(clean, "index.html"), "x"); writeFileSync(join(clean, "pkg/a.wasm"), "x");
expect("a clean lite output passes", run(clean, "--lite"), 0);
expect("a default output without builtin/ is refused", run(clean) === 0 ? 0 : 1, 1);
{
  const withBuiltin = fresh();
  mkdirSync(join(withBuiltin, "builtin")); writeFileSync(join(withBuiltin, "builtin/manifest.json"), "{\"stamp\":\"x\",\"layers\":[]}");
  expect("a lite output with a builtin/ folder is refused", run(withBuiltin, "--lite") === 0 ? 0 : 1, 1);
  expect("a default output with a manifest passes", run(withBuiltin), 0);
  writeFileSync(join(withBuiltin, "builtin/stray.zip"), "x");
  expect("a file in builtin/ the manifest doesn't name is refused", run(withBuiltin) === 0 ? 0 : 1, 1);
}

for (const [name, path] of [
  ["a vanilla folder", "vanilla/assets/minecraft/x.png"],
  ["a downloads folder", "downloads/resourcepacks/p.zip"],
  ["Minecraft's assets path", "assets/minecraft/sounds.json"],
  ["a mod jar", "mods/x.jar"],
]) {
  const dir = fresh();
  const full = join(dir, path);
  mkdirSync(join(full, ".."), { recursive: true }); writeFileSync(full, "x");
  expect(`${name} is refused (lite)`, run(dir, "--lite") === 0 ? 0 : 1, 1);
  mkdirSync(join(dir, "builtin")); writeFileSync(join(dir, "builtin/manifest.json"), "{\"stamp\":\"x\",\"layers\":[]}");
  expect(`${name} is refused (default)`, run(dir) === 0 ? 0 : 1, 1);
}
process.exit(bad ? 1 : 0);
