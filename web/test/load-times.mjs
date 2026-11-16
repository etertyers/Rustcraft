// Step 0 of M75: how long the title screen takes with each way of loading the
// built-in files, in Chrome on this Mac (localhost). Each row: a fresh profile
// (first load), then the same profile again (reload).
//   node load-times.mjs
import { launch, waitFor, serve, ROOT } from "./lib.mjs";
import * as lib from "./lib.mjs";
import { mkdtempSync, readdirSync, statSync } from "node:fs";
import { tmpdir } from "node:os";
import { join } from "node:path";

const arg = (name, dflt) => { const i = process.argv.indexOf(`--${name}`); return i > 0 ? process.argv[i + 1] : dflt; };
const stop = await serve();
const ready = (page) => page.locator("#click").evaluate((e) => e.style.display === "block").catch(() => false);

async function once(profile, query, { route } = {}) {
  const { context, page, logs } = await launch(profile);
  if (route) await page.route(...route);
  const t0 = Date.now();
  await page.goto(lib.URL_BASE + query);
  await waitFor(() => ready(page), "the start button", 300000);
  const wall = Date.now() - t0;
  const lt = await page.evaluate(() => window.rustcraft.loadTimes);
  const mem = await page.evaluate(() => window.rustcraft.camera ? 0 : 0);
  let bg = null;
  if (!query.includes("eager") && !query.includes("nobackground")) {
    await page.click("#go");
    bg = await waitFor(async () => (await page.evaluate(() => window.rustcraft.loadTimes.background))?.ms ?? false, "the background layers", 600000).catch(() => null);
  }
  await context.close();
  return { wall, lt, bg };
}
const row = (name, r) => console.log(`${name.padEnd(46)} title screen ${String(r.wall).padStart(6)} ms | wasm+assets.zip ${String(Math.round(r.lt.rustBoot ?? 0)).padStart(5)} ms | built-in ${String(Math.round(r.lt.boot?.ms ?? 0)).padStart(6)} ms (fetched ${((r.lt.boot?.fetched ?? 0) / 1e6).toFixed(0)} MB, cache ${((r.lt.boot?.cached ?? 0) / 1e6).toFixed(0)} MB)${r.bg ? ` | background done after ${Math.round(r.bg)} ms` : ""}`);

const fresh = () => mkdtempSync(join(tmpdir(), "rc-load-"));
for (const [name, query] of [["built-in: boot layers, rest in background", "?"], ["built-in: everything before the title screen", "?eager"]]) {
  const p = fresh();
  row(`${name} (first load)`, await once(p, query));
  row(`${name} (reload)`, await once(p, query));
}
// The old way (M71): Minecraft's files picked at the setup screen and stored in
// OPFS as one zip, unzipped into memory at every start; the page has no manifest
// (a --lite build). First the import itself, then a reload.
{
  const noManifest = ["**/builtin/manifest.json", (r) => r.fulfill({ status: 404 })];
  const p = fresh();
  const { context, page } = await launch(p);
  await page.route(...noManifest);
  const t0 = Date.now();
  await page.goto(lib.URL_BASE);
  await waitFor(() => page.locator("#setup:not([hidden])").count(), "the setup screen", 60000);
  await page.setInputFiles("#vanilla-folder", join(ROOT, "assets/vanilla"));
  await waitFor(() => page.locator("#continue").isEnabled(), "the import", 900000);
  console.log(`setup screen: picking the vanilla folder and storing it took ${Date.now() - t0} ms (once per browser)`);
  await context.close();
  row("stored in OPFS, unzipped at every start (reload)", await once(p, "?", { route: noManifest }));
}
// One request per file (the on-demand idea), over localhost.
{
  const files = [];
  const walk = (dir, rel) => { for (const f of readdirSync(dir)) { const p = join(dir, f); const r = rel ? `${rel}/${f}` : f; statSync(p).isDirectory() ? walk(p, r) : files.push(r); } };
  const root = join(ROOT, "assets/vanilla/assets/minecraft");
  for (const d of ["textures", "models", "blockstates", "items", "font", "lang"]) walk(join(root, d), d);
  const { context, page } = await launch(fresh());
  await page.goto(lib.URL_BASE + "?nobackground");
  for (const conc of [6, 24]) {
    const r = await page.evaluate(async ([list, conc]) => {
      const t0 = performance.now(); let bytes = 0, i = 0;
      await Promise.all(Array.from({ length: conc }, async () => { while (i < list.length) { const f = list[i++]; const r = await fetch(`/__vanilla/${f}`, { cache: "no-store" }); bytes += (await r.arrayBuffer()).byteLength; } }));
      return { ms: performance.now() - t0, bytes };
    }, [files, conc]);
    console.log(`one request per file, ${files.length} core files, ${conc} at a time: ${Math.round(r.ms)} ms for ${(r.bytes / 1e6).toFixed(0)} MB (localhost)`);
  }
  await context.close();
}
stop();
