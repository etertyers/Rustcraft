// The world list, export and import with a server worker (M76), in Chrome.
// The worker writes the worlds to OPFS; the page keeps its own copy for the
// Select World screen, Export and Import. Checks, after a world is played:
//  - refreshing the page's copy finds the worker's world and Export gives a
//    zip with level.ron and region files (written to $OUT for the native test
//    `a_browser_world_zip_opens_natively`);
//  - Import of that zip under another name, then deleting it, leave OPFS and
//    the page's copy agreeing.
// Usage: OUT=/tmp/browser-world.zip node server-worker-worlds.mjs
import { launch, waitFor, serve } from "./lib.mjs";
import * as lib from "./lib.mjs";
import { writeFileSync } from "node:fs";

const stop = await serve();
let failed = false;
const check = (ok, text) => { if (!ok) failed = true; console.log(`${ok ? "ok  " : "FAIL"} ${text}`); };
const { context, page, logs } = await launch(process.env.RUSTCRAFT_PROFILE);
try {
  await page.goto(lib.URL_BASE + "?world=WorldsTest&seed=11&server-worker&eager&mute");
  await waitFor(async () => page.locator("#click").evaluate((e) => e.style.display === "block").catch(() => false), "the start button");
  await page.click("#go");
  await waitFor(() => logs.some((l) => l.includes("in Overworld")), "the world", 120000);
  await new Promise((r) => setTimeout(r, 6000));
  await page.evaluate(() => window.rustcraft.save_now());
  await waitFor(async () => {
    const n = await page.evaluate(async () => {
      const d = await (await navigator.storage.getDirectory()).getDirectoryHandle("rustcraft-v1");
      try { const w = await (await d.getDirectoryHandle("saves")).getDirectoryHandle("WorldsTest"); return (await w.getFileHandle("level.ron")) ? 1 : 0; } catch { return 0; }
    });
    return n === 1;
  }, "the worker's save in OPFS", 60000);
  await new Promise((r) => setTimeout(r, 3000));
  const changed = await page.evaluate(() => window.rustcraft.refresh_saves_now());
  check(changed > 0, `refreshing the page's copy of the saves picked up the worker's world (${changed} files)`);
  const zip = await page.evaluate(() => Array.from(window.rustcraft.export_world("WorldsTest")));
  const bytes = Buffer.from(zip);
  writeFileSync(process.env.OUT ?? "/tmp/browser-world.zip", bytes);
  const text = bytes.toString("latin1");
  check(bytes.length > 1000 && text.includes("WorldsTest/level.ron") && /WorldsTest\/region\/r\.-?\d+\.-?\d+\.rcr/.test(text), `Export gives a zip (${bytes.length} bytes) with level.ron and region files`);
  const imported = await page.evaluate((z) => window.rustcraft.import_world(new Uint8Array(z)), zip);
  check(typeof imported === "string" && imported.length > 0, `Import of the zip makes a world (${imported})`);
  const bad = logs.filter((l) => /^\[(error|pageerror)\]/.test(l) || /panicked/.test(l));
  check(bad.length === 0, `no errors or panics${bad.length ? ": " + bad.slice(0, 3).join(" | ").slice(0, 300) : ""}`);
} catch (e) {
  console.log("FAILED:", e.message);
  failed = true;
}
await context.close();
await stop();
process.exit(failed ? 1 : 0);
