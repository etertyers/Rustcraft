// The built-in files in Chrome (M75), against target/web from a default build:
//  1. first load: no setup screen, straight to "Click to start"; the game
//     reaches its title screen with the Minecraft font, the menu music and
//     Barebones' packs; the background files (sounds, Patrix, the big jar)
//     all arrive;
//  2. reload: nothing but the manifest is fetched, the archives come from OPFS;
//  3. a rebuild with different files (a new stamp in the manifest) fetches
//     again and deletes the old stamp's cache;
//  4. no manifest (a --lite build): the folder-picker setup screen;
//  5. built-in files that fail to load: the setup screen with a one-line reason.
// Usage: node builtin.mjs
import { launch, waitFor, serve, ROOT } from "./lib.mjs";
import * as lib from "./lib.mjs";
import { mkdtempSync, readdirSync } from "node:fs";
import { tmpdir } from "node:os";
import { join } from "node:path";

const stop = await serve();
let failed = false;
const check = (ok, text) => { if (!ok) failed = true; console.log(`${ok ? "ok  " : "FAIL"} ${text}`); };
const ready = (page) => page.locator("#click").evaluate((e) => e.style.display === "block").catch(() => false);
const setupShown = (page) => page.locator("#setup:not([hidden])").count();
const fresh = () => mkdtempSync(join(tmpdir(), "rc-builtin-"));
const first = (dir) => readdirSync(join(ROOT, "assets/vanilla/assets/minecraft", dir)).sort()[0];
const opfsFiles = (page) => page.evaluate(async () => {
  const out = [];
  const walk = async (d, p) => { for await (const [n, h] of d.entries()) { if (h.kind === "directory") await walk(h, p + n + "/"); else out.push(p + n); } };
  await walk(await (await navigator.storage.getDirectory()).getDirectoryHandle("rustcraft-v1"), "");
  return out;
});
const archivesFetched = (requests) => requests.filter((u) => /\/builtin\/(?!manifest\.json)/.test(u)).length;

// 1 and 2: a profile, loaded twice.
const profile = fresh();
{
  const { context, page, logs } = await launch(profile);
  const requests = [];
  page.on("request", (r) => requests.push(r.url()));
  await page.goto(lib.URL_BASE);
  await waitFor(async () => (await ready(page)) || (await setupShown(page)), "the start button or the setup screen", 120000);
  check((await setupShown(page)) === 0 && (await ready(page)), "first load: no folder picker, straight to \"Click to start\"");
  const lt = await page.evaluate(() => window.rustcraft.loadTimes);
  check(lt.boot.fetched > 0 && lt.boot.cached === 0, `first load fetched the boot layers (${(lt.boot.fetched / 1e6).toFixed(0)} MB), none from the cache; title screen ready after ${Math.round(lt.titleReady)} ms`);
  await page.click("#go");
  await waitFor(() => logs.some((l) => l.includes("font:") && l.includes("glyphs")), "the Minecraft font", 60000);
  check(true, "the title screen's Minecraft font loaded (from the built-in vanilla layer)");
  await waitFor(() => logs.some((l) => l.includes("music: playing music/menu/")), "the menu music", 60000).then(
    () => check(true, `the menu music plays (${logs.find((l) => l.includes("music: playing")).replace(/^.*music: /, "music: ")})`),
    () => check(false, "the menu music never started"));
  const packs = logs.find((l) => /resources: .* over the defaults/.test(l));
  check(!!packs && !/resources: no packs/.test(packs) && /Bare Bones/.test(packs), `Barebones is the default style, its packs are applied (${packs?.replace(/^.*resources: /, "").replace(/ over the defaults.*/, "")})`);
  const bg = await waitFor(async () => (await page.evaluate(() => window.rustcraft.loadTimes.background))?.ms ?? false, "the background layers", 300000);
  const present = await page.evaluate((paths) => paths.map((p) => !!window.rustcraft.read_file(p)), [
    `assets/vanilla/assets/minecraft/sounds/block/${first("sounds/block")}`,
    `assets/vanilla/assets/minecraft/sounds/music/game/${first("sounds/music/game")}`,
    "assets/downloads/resourcepacks/Patrix_26.3_32x_basic.zip",
    "assets/downloads/mods/AmbientSounds_FABRIC_v6.3.11_mc26.3.jar",
  ]);
  check(present.every(Boolean), `the background files all arrived in ${Math.round(bg)} ms (a gameplay sound, a music track, Patrix, the big jar: ${present})`);
  const cached = (await opfsFiles(page)).filter((p) => p.startsWith("builtin/"));
  check(cached.length > 15, `the archives are cached in OPFS (${cached.length} files under builtin/<stamp>/)`);
  const bad = logs.filter((l) => /^\[(error|pageerror)\]/.test(l) && !/Failed to load resource/.test(l));
  check(bad.length === 0, `no errors while loading${bad.length ? ": " + bad[0].slice(0, 160) : ""}`);
  await context.close();
}
{
  const { context, page } = await launch(profile);
  const requests = [];
  page.on("request", (r) => requests.push(r.url()));
  await page.goto(lib.URL_BASE);
  await waitFor(() => ready(page), "the start button after a reload", 120000);
  const lt = await page.evaluate(() => window.rustcraft.loadTimes);
  check(lt.boot.fetched === 0 && lt.boot.cached > 0 && archivesFetched(requests) === 0, `reload: no archive fetched, ${(lt.boot.cached / 1e6).toFixed(0)} MB from OPFS; title screen ready after ${Math.round(lt.titleReady)} ms`);
  await context.close();
}
// 3: a rebuild with different files has a new stamp.
{
  const { context, page } = await launch(profile);
  await page.route("**/builtin/manifest.json", async (route) => {
    const r = await route.fetch();
    const m = await r.json();
    m.stamp = "rebuilt" + m.stamp.slice(7);
    await route.fulfill({ response: r, body: JSON.stringify(m), contentType: "application/json" });
  });
  const requests = [];
  page.on("request", (r) => requests.push(r.url()));
  await page.goto(lib.URL_BASE);
  await waitFor(() => ready(page), "the start button with a new stamp", 120000);
  const lt = await page.evaluate(() => window.rustcraft.loadTimes);
  check(lt.boot.fetched > 0 && lt.boot.cached === 0 && archivesFetched(requests) > 0, `a new stamp fetches the archives again (${(lt.boot.fetched / 1e6).toFixed(0)} MB, none from the old cache)`);
  const stamps = new Set((await opfsFiles(page)).filter((p) => p.startsWith("builtin/")).map((p) => p.split("/")[1]));
  check(stamps.size === 1 && [...stamps][0].startsWith("rebuilt"), `the old stamp's cache is deleted (only ${[...stamps]} is left)`);
  await context.close();
}
// 6: Realistic works with the built-in Patrix (everything loaded before the start).
{
  const { context, page, logs } = await launch(profile);
  await page.goto(lib.URL_BASE + "?eager&style=realistic");
  await waitFor(() => ready(page), "the start button (eager)", 180000);
  await page.click("#go");
  const line = await waitFor(() => logs.find((l) => /resources: .* over the defaults/.test(l)), "the packs line", 90000);
  check(/Patrix/.test(line), `?style=realistic applies the built-in Patrix (${line.replace(/^.*resources: /, "").replace(/ over the defaults.*/, "")})`);
  await context.close();
}
// 4: no manifest, a --lite build.
{
  const { context, page } = await launch(fresh());
  await page.route("**/builtin/manifest.json", (r) => r.fulfill({ status: 404 }));
  await page.goto(lib.URL_BASE);
  await waitFor(() => setupShown(page), "the setup screen", 60000);
  check(true, "no built-in manifest (--lite): the folder-picker setup screen");
  await context.close();
}
// 5: a built-in layer that fails to load.
{
  const { context, page } = await launch(fresh());
  await page.route("**/builtin/vanilla-core.zip", (r) => r.abort());
  await page.goto(lib.URL_BASE);
  await waitFor(() => setupShown(page), "the setup screen after a failed load", 60000);
  const text = await page.locator("#error").textContent();
  check(/built-in Minecraft files couldn't be loaded/.test(text) && !text.includes("\n"), `built-in files that fail to load: the setup screen with a one-line reason ("${text.slice(0, 120)}...")`);
  await context.close();
}
stop();
process.exit(failed ? 1 : 0);
