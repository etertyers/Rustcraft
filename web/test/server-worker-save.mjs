// Saves through the server worker (M76), in Chrome: ?world=SaveTest&seed=7&server-worker.
//  - the worker writes the world to OPFS (synchronous access handles): level.ron,
//    region files, playerdata/;
//  - after a reload the worker reads them back and opens the same world
//    (the same level.ron bytes, no new world created).
// Usage: node server-worker-save.mjs   (needs target/web from build-web.sh)
import { launch, waitFor, serve } from "./lib.mjs";
import * as lib from "./lib.mjs";
import { mkdtempSync } from "node:fs";
import { tmpdir } from "node:os";
import { join } from "node:path";

const stop = await serve();
const profile = mkdtempSync(join(tmpdir(), "rc-sw-save-"));
let failed = false;
const check = (ok, text) => { if (!ok) failed = true; console.log(`${ok ? "ok  " : "FAIL"} ${text}`); };
const query = "?world=SaveTest&seed=7&server-worker&eager&mute";
const opfs = (page) => page.evaluate(async () => {
  const out = {};
  const walk = async (d, p) => { for await (const [n, h] of d.entries()) { if (h.kind === "directory") await walk(h, p + n + "/"); else if (p.startsWith("saves/")) out[p + n] = (await (await h.getFile()).arrayBuffer()).byteLength; } };
  try { await walk(await (await navigator.storage.getDirectory()).getDirectoryHandle("rustcraft-v1"), ""); } catch {}
  return out;
});
const level = (page) => page.evaluate(async () => {
  try {
    const d = await (await navigator.storage.getDirectory()).getDirectoryHandle("rustcraft-v1");
    const f = await (await (await (await d.getDirectoryHandle("saves")).getDirectoryHandle("SaveTest")).getFileHandle("level.ron")).getFile();
    return await f.text();
  } catch { return null; }
});
async function run(label) {
  const { context, page, logs } = await launch(profile);
  await page.goto(lib.URL_BASE + query);
  await waitFor(async () => page.locator("#click").evaluate((e) => e.style.display === "block").catch(() => false), "the start button");
  await page.click("#go");
  await waitFor(() => logs.some((l) => l.includes("in Overworld")), `${label}: the world`, 120000);
  await new Promise((r) => setTimeout(r, 6000));
  return { context, page, logs };
}
try {
  const a = await run("first run");
  await a.page.evaluate(() => window.rustcraft.save_now());
  const files = await waitFor(async () => {
    const f = await opfs(a.page);
    return Object.keys(f).some((k) => k.endsWith("level.ron")) && Object.keys(f).some((k) => /region|\.rgn|\.mca/.test(k)) ? f : false;
  }, "the worker's save in OPFS", 60000);
  check(true, `the worker wrote the world to OPFS: ${Object.keys(files).length} files (${Object.keys(files).slice(0, 4).join(", ")}...)`);
  const before = await level(a.page);
  check(/seed:\s*7/.test(before ?? ""), `level.ron has the seed (${(before ?? "").replace(/\s+/g, " ").slice(0, 80)})`);
  check(a.logs.some((l) => /server worker: 0 saved files read/.test(l)), "first run started from nothing (no saved files)");
  await a.context.close();
  const b = await run("after reload");
  const read = b.logs.find((l) => /server worker: \d+ saved files read/.test(l));
  check(!!read && !/: 0 saved/.test(read), `the worker read the world back (${read?.replace(/^.*server worker: /, "")})`);
  await new Promise((r) => setTimeout(r, 2000));
  const after = await level(b.page);
  check(after === before, "the same world opened (level.ron unchanged, no new world)");
  const bad = [...a.logs, ...b.logs].filter((l) => /^\[(error|pageerror)\]/.test(l) || /panicked/.test(l));
  check(bad.length === 0, `no errors or panics${bad.length ? ": " + bad.slice(0, 3).join(" | ").slice(0, 300) : ""}`);
  await b.context.close();
} catch (e) {
  console.log("FAILED:", e.message);
  failed = true;
}
await stop();
process.exit(failed ? 1 : 0);
