// The build script's refusal (M71): scripts/check-web-output.sh must fail on a
// folder holding anything from assets/vanilla or assets/downloads, and pass a
// clean one. Usage: node check-output.mjs
import { mkdtempSync, mkdirSync, writeFileSync } from "node:fs";
import { tmpdir } from "node:os";
import { join } from "node:path";
import { spawnSync } from "node:child_process";
import { ROOT } from "./lib.mjs";

const script = join(ROOT, "scripts/check-web-output.sh");
const run = (dir) => spawnSync("bash", [script, dir], { encoding: "utf8" }).status;
const fresh = () => mkdtempSync(join(tmpdir(), "web-out-"));
let bad = 0;
const expect = (name, got, want) => { const ok = got === want; if (!ok) bad++; console.log(`${ok ? "ok  " : "FAIL"} ${name}`); };

const clean = fresh();
mkdirSync(join(clean, "pkg")); writeFileSync(join(clean, "index.html"), "x"); writeFileSync(join(clean, "pkg/a.wasm"), "x");
expect("a clean output passes", run(clean), 0);

for (const [name, path] of [
  ["a vanilla folder", "vanilla/assets/minecraft/x.png"],
  ["a downloads folder", "downloads/resourcepacks/p.zip"],
  ["Minecraft's assets path", "assets/minecraft/sounds.json"],
  ["a mod jar", "mods/x.jar"],
]) {
  const dir = fresh();
  const full = join(dir, path);
  mkdirSync(join(full, ".."), { recursive: true }); writeFileSync(full, "x");
  expect(`${name} is refused`, run(dir) === 0 ? 0 : 1, 1);
}
process.exit(bad ? 1 : 0);
