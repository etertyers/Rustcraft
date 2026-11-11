// M72: a world saves and reopens in Chrome. Plays a survival world, saves,
// notes a hash of every saved file in OPFS, reloads the page, checks the same
// files are restored byte for byte, and that the world opens again.
// Usage: RUSTCRAFT_PROFILE=<profile with vanilla> node persist.mjs
import { launch, waitFor, serve } from "./lib.mjs";
import * as lib from "./lib.mjs";

const stop = await serve();
const { context, page, logs, dir } = await launch(process.env.RUSTCRAFT_PROFILE);
const query = "?world=Persist1&seed=7";
const opfsHashes = () => page.evaluate(async () => {
  const out = {};
  const walk = async (d, p) => {
    for await (const [n, h] of d.entries()) {
      if (h.kind === "directory") await walk(h, p + n + "/");
      else if (p.startsWith("saves/")) {
        const buf = await (await h.getFile()).arrayBuffer();
        const sum = await crypto.subtle.digest("SHA-256", buf);
        out[p + n] = [buf.byteLength, [...new Uint8Array(sum)].slice(0, 6).map((b) => b.toString(16).padStart(2, "0")).join("")];
      }
    }
  };
  await walk(await (await navigator.storage.getDirectory()).getDirectoryHandle("rustcraft-v1"), "");
  return out;
});
const ready = () => page.locator("#click").evaluate((e) => e.style.display === "block").catch(() => false);
let failed = false;
try {
  await page.goto(lib.URL_BASE + query);
  await waitFor(ready, "the start button");
  await page.click("#go");
  await waitFor(() => logs.some((l) => l.includes("in Overworld")), "the world", 90000);
  await new Promise((r) => setTimeout(r, 6000));
  await page.keyboard.down("w"); await new Promise((r) => setTimeout(r, 800)); await page.keyboard.up("w");
  await page.mouse.down(); await new Promise((r) => setTimeout(r, 500)); await page.mouse.up();
  await page.evaluate(() => window.rustcraft.save_now());
  await page.evaluate(() => window.rustcraft.storage_idle());
  await new Promise((r) => setTimeout(r, 1000));
  const before = await opfsHashes();
  const names = Object.keys(before);
  console.log("saved files:", names.length, names.slice(0, 4).join(", "));
  if (!names.some((n) => n.endsWith("level.ron"))) throw new Error("no level.ron saved");
  // Reload: everything comes back from OPFS.
  logs.length = 0;
  await page.goto(lib.URL_BASE + query);
  await waitFor(ready, "the start button after reload");
  const restored = logs.filter((l) => /restored saves\//.test(l)).length;
  const after = await opfsHashes();
  const same = names.every((n) => JSON.stringify(before[n]) === JSON.stringify(after[n]));
  console.log("restored files:", restored, "- identical in OPFS:", same);
  if (!same || restored !== names.length) throw new Error("the saved files changed across the reload");
  await page.click("#go");
  await waitFor(() => logs.some((l) => l.includes("in Overworld")), "the saved world opening", 90000);
  const bad = logs.filter((l) => /^\[(error|pageerror)\]|panicked/.test(l));
  if (bad.length) throw new Error("problems after reopening:\n" + bad.slice(0, 5).join("\n"));
  console.log("the world reopened from its save");
} catch (e) {
  console.log("FAILED:", e.message);
  failed = true;
}
console.log("profile:", dir);
await context.close();
stop();
process.exit(failed ? 1 : 0);
