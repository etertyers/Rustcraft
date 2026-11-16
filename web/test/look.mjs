// Mouse look probe (M73 follow-up): clicks the canvas to lock the pointer, moves
// the mouse, and reports the delays between the click, the lock, the DOM
// mousemove events and the camera's yaw changing, plus frame gaps, long
// tasks and console warnings. Usage: node look.mjs [--profile dir] [--screenshot f]
import { launch, waitFor, serve } from "./lib.mjs";
import * as lib from "./lib.mjs";

const arg = (name, dflt) => { const i = process.argv.indexOf(`--${name}`); return i > 0 ? process.argv[i + 1] : dflt; };
const stop = await serve();
const { context, page, logs } = await launch(arg("profile", process.env.RUSTCRAFT_PROFILE), { width: 1440, height: 900 });
await page.goto(lib.URL_BASE + "?creative" + arg("extra", ""));
await waitFor(() => page.locator("#click").evaluate((e) => e.style.display === "block").catch(() => false), "start");
await page.click("#go");
await waitFor(() => logs.some((l) => l.includes("in Overworld")), "the world", 90000);
await new Promise((r) => setTimeout(r, 8000)); // let the first chunks arrive
await page.evaluate(() => {
  window.__mm = []; window.__cam = []; window.__lt = []; window.__lock = [];
  document.addEventListener("mousemove", (e) => window.__mm.push([performance.now(), e.movementX, e.movementY, !!document.pointerLockElement]), true);
  document.addEventListener("pointerlockchange", () => window.__lock.push([performance.now(), !!document.pointerLockElement]));
  document.addEventListener("pointerlockerror", () => window.__lock.push([performance.now(), "error"]));
  new PerformanceObserver((l) => l.getEntries().forEach((e) => window.__lt.push([e.startTime, e.duration]))).observe({ entryTypes: ["longtask"] });
  const loop = () => { const c = window.rustcraft.camera(); window.__cam.push([performance.now(), c[0], c[1], c[2]]); requestAnimationFrame(loop); };
  requestAnimationFrame(loop);
});
await page.bringToFront();
await page.evaluate(() => window.focus());
const warnsBefore = logs.filter((l) => l.startsWith("[warning]")).length;
const tClick = await page.evaluate(() => performance.now());
await page.mouse.move(400, 300); // somewhere off-center, as a player's pointer would be
await page.mouse.click(400, 300);
await new Promise((r) => setTimeout(r, 400));
const tMove = await page.evaluate(() => performance.now());
// Look around: slow, then fast.
for (let i = 0; i < 40; i++) { await page.mouse.move(400 + i * 3, 300 + (i % 5)); await new Promise((r) => setTimeout(r, 16)); }
for (let i = 0; i < 40; i++) { await page.mouse.move(520 + i * 25, 300 - (i % 7)); await new Promise((r) => setTimeout(r, 16)); }
await new Promise((r) => setTimeout(r, 500));
const d = await page.evaluate(() => ({ mm: window.__mm, cam: window.__cam, lt: window.__lt, lock: window.__lock }));
const t0 = tClick;
const rel = (t) => (t - t0).toFixed(0);
const firstMove = d.mm.find((m) => m[1] !== 0 || m[2] !== 0);
const yaw0 = d.cam.find((c) => c[0] > tMove)?.[1];
const firstYaw = d.cam.find((c) => c[0] > tMove && Math.abs(c[1] - yaw0) > 1e-4);
const gaps = d.cam.slice(1).map((c, i) => [c[0], c[0] - d.cam[i][0]]).filter((g) => g[0] > tClick).sort((a, b) => b[1] - a[1]).slice(0, 5);
console.log("document focused:", await page.evaluate(() => document.hasFocus()), "| lock element now:", await page.evaluate(() => document.pointerLockElement?.id ?? null));
console.log("pointer lock events (ms after the click):", JSON.stringify(d.lock.map((l) => [rel(l[0]), l[1]])));
console.log(`mousemove events: ${d.mm.length}, with movement: ${d.mm.filter((m) => m[1] || m[2]).length}, while locked: ${d.mm.filter((m) => m[3]).length}`);
console.log("first mousemove with movement:", firstMove ? `${rel(firstMove[0])} ms after the click (movement ${firstMove[1]},${firstMove[2]}, locked ${firstMove[3]})` : "none");
console.log("first movement after the settle time moved the camera at:", firstYaw ? `${rel(firstYaw[0])} ms (frame ${firstYaw[3]})` : "never");
console.log("longest frame gaps (ms after click, gap ms):", JSON.stringify(gaps.map((g) => [rel(g[0]), Math.round(g[1])])));
console.log("long tasks after the click:", JSON.stringify(d.lt.filter((l) => l[0] > tClick).map((l) => [rel(l[0]), Math.round(l[1])])));
console.log("console warnings during the probe:", logs.filter((l) => l.startsWith("[warning]")).length - warnsBefore);
console.log("sample warnings:", logs.filter((l) => l.startsWith("[warning]")).slice(warnsBefore, warnsBefore + 3).map((l) => l.slice(0, 160)).join(" | "));
console.log("yaw range during probe:", d.cam.length ? `${Math.min(...d.cam.map((c) => c[1])).toFixed(3)}..${Math.max(...d.cam.map((c) => c[1])).toFixed(3)}` : "no frames");
(await import("node:fs")).writeFileSync("/tmp/look-probe.json", JSON.stringify(d));
await context.close();
stop();
