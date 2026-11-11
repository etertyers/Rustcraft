// Plays a world for a while (M71): ?creative or ?world=Name&seed=N. Walks,
// looks around, breaks and places a block, then checks for errors/panics and
// that frames keep coming. Usage: node play.mjs "?creative" [--screenshot f]
import { launch, passSetup, waitFor, serve } from "./lib.mjs";
import * as lib from "./lib.mjs";

const query = process.argv[2] ?? "?creative";
const shot = process.argv.includes("--screenshot") ? process.argv[process.argv.indexOf("--screenshot") + 1] : null;
const stop = await serve();
const { context, page, logs, dir } = await launch(process.env.RUSTCRAFT_PROFILE);
let failed = false;
try {
  // passSetup opens the base URL; add the query to it.
  const base = lib.URL_BASE;
  await page.goto(base + query);
  const state = await waitFor(async () => {
    if (await page.locator("#setup:not([hidden])").count()) return "setup";
    if (await page.locator("#click").evaluate((e) => e.style.display === "block").catch(() => false)) return "click";
    const err = await page.locator("#error").textContent();
    if (err) throw new Error(`loader error: ${err}`);
    return false;
  }, "loader");
  if (state === "setup") throw new Error("setup screen shown: no stored vanilla layer in this profile");
  await page.click("#go");
  await waitFor(() => logs.some((l) => l.includes("GPU adapter")), "gpu", 60000);
  await new Promise((r) => setTimeout(r, 6000));
  const canvas = page.locator("#rustcraft");
  await canvas.click({ position: { x: 640, y: 360 } });
  await page.keyboard.down("w");
  for (let i = 0; i < 20; i++) { await page.mouse.move(640 + i * 10, 360 + (i % 3) * 5); await new Promise((r) => setTimeout(r, 100)); }
  await page.keyboard.up("w");
  await page.mouse.down(); await new Promise((r) => setTimeout(r, 600)); await page.mouse.up();
  if (process.argv.includes("--f12")) { await page.keyboard.press("F12"); await new Promise((r) => setTimeout(r, 1500)); }
  await page.mouse.down({ button: "right" }); await page.mouse.up({ button: "right" });
  await page.keyboard.press("e"); await new Promise((r) => setTimeout(r, 800)); await page.keyboard.press("e");
  await new Promise((r) => setTimeout(r, 3000));
  const bad = logs.filter((l) => /^\[(error|pageerror)\]/.test(l) || /panicked|Invalid|WGSL/.test(l));
  console.log(logs.filter((l) => /chunk|world|sound|server|player/i.test(l)).slice(0, 12).map((l) => l.slice(0, 200)).join("\n"));
  if (bad.length) { console.log("PROBLEMS:\n" + bad.slice(0, 10).map((l) => l.slice(0, 300)).join("\n")); failed = true; }
  if (shot) await page.screenshot({ path: shot });
  const kept = await page.evaluate(async () => {
    const out = [];
    const walk = async (d, p) => { for await (const [n, h] of d.entries()) { if (h.kind === "directory") await walk(h, p + n + "/"); else out.push(p + n); } };
    await walk(await (await navigator.storage.getDirectory()).getDirectoryHandle("rustcraft-v1"), "");
    return out;
  });
  console.log("OPFS files:", kept.join(", "));
} catch (e) {
  console.log("FAILED:", e.message);
  failed = true;
}
(await import("node:fs")).writeFileSync("/tmp/m71-play-logs.txt", logs.join("\n"));
if (process.argv.includes("--reload")) {
  await page.goto(lib.URL_BASE + query);
  await waitFor(() => page.locator("#click").evaluate((e) => e.style.display === "block").catch(() => false), "loader after reload");
  await new Promise((r) => setTimeout(r, 500));
  console.log("after reload:", logs.filter((l) => /restored/.test(l)).map((l) => l.slice(0, 120)).join(" | "));
}
console.log("profile:", dir);
await context.close();
stop();
process.exit(failed ? 1 : 0);
