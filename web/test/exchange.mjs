// M72: world exchange between the browser and the Mac app. Exports the
// world made by persist.mjs from Chrome to a .zip (same folder layout as
// native saves), imports it back (it must get a new name, not overwrite),
// and leaves the zip at /tmp/rustcraft-browser-world.zip so the native test
// can open it:
//   RUSTCRAFT_BROWSER_WORLD_ZIP=/tmp/rustcraft-browser-world.zip \
//     cargo test --lib a_world_exported_from_the_browser_opens_natively -- --ignored
import { launch, waitFor, serve } from "./lib.mjs";
import * as lib from "./lib.mjs";
import { writeFileSync, readFileSync } from "node:fs";

const stop = await serve();
const { context, page, logs, dir } = await launch(process.env.RUSTCRAFT_PROFILE);
let failed = false;
try {
  await page.goto(lib.URL_BASE);
  await waitFor(() => page.locator("#click").evaluate((e) => e.style.display === "block").catch(() => false), "the start button");
  const out = await page.evaluate(() => {
    const bytes = window.rustcraft.export_world("Persist1");
    const imported = window.rustcraft.import_world(bytes);
    return { bytes: Array.from(bytes), imported };
  });
  writeFileSync("/tmp/rustcraft-browser-world.zip", Buffer.from(out.bytes));
  console.log("exported Persist1:", out.bytes.length, "bytes; imported as:", out.imported);
  if (out.imported === "Persist1") throw new Error("the import overwrote the world");
  // A world made by the native game opens in the browser.
  const native = readFileSync("/tmp/rustcraft-native-world.zip");
  const name = await page.evaluate((bytes) => window.rustcraft.import_world(new Uint8Array(bytes)), [...native]);
  console.log("imported the native world as:", name);
  await page.evaluate(() => window.rustcraft.storage_idle());
  logs.length = 0;
  await page.goto(lib.URL_BASE + "?world=" + encodeURIComponent(name));
  await waitFor(() => page.locator("#click").evaluate((e) => e.style.display === "block").catch(() => false), "the start button");
  await page.click("#go");
  await waitFor(() => logs.some((l) => l.includes("in Overworld")), "the native world opening", 90000);
  await new Promise((r) => setTimeout(r, 3000));
  const bad = logs.filter((l) => /^\[(error|pageerror)\]|panicked/.test(l));
  if (bad.length) throw new Error("problems opening the native world:\n" + bad.slice(0, 5).join("\n"));
  console.log("the native world opened in the browser");
} catch (e) {
  console.log("FAILED:", e.message);
  console.log(logs.slice(-15).map((l) => l.slice(0, 300)).join("\n"));
  failed = true;
}
await context.close();
stop();
process.exit(failed ? 1 : 0);
