// Mouse look (M73 follow-up). After the click that engages the camera:
//  1. the click causes no long main-thread task (it used to: audio start,
//     200-600 ms on an M1);
//  2. a mouse move turns the camera by exactly movement x 0.0025 rad
//     (config::MOUSE_SENSITIVITY) within two frames;
//  3. every event counts: a burst of small moves adds up to their sum.
// A real pointer lock can't be granted to Chrome under automation
// (WrongDocumentError), but the game gets the same MouseMotion events.
import { launch, waitFor, serve } from "./lib.mjs";
import * as lib from "./lib.mjs";
const arg = (name, dflt) => { const i = process.argv.indexOf(`--${name}`); return i > 0 ? process.argv[i + 1] : dflt; };
const SENS = 0.0025;
const TAU = Math.PI * 2;
const stop = await serve();
const { context, page, logs } = await launch(arg("profile", process.env.RUSTCRAFT_PROFILE), { width: 1280, height: 720 });
let failed = false;
const check = (ok, text) => { if (!ok) failed = true; console.log(`${ok ? "ok  " : "FAIL"} ${text}`); };
await page.goto(lib.URL_BASE + "?creative");
await waitFor(() => page.locator("#click").evaluate((e) => e.style.display === "block").catch(() => false), "start");
await page.click("#go");
await waitFor(() => logs.some((l) => l.includes("in Overworld")), "the world", 90000);
await new Promise((r) => setTimeout(r, 8000));
await page.evaluate(() => {
  window.__lt = [];
  window.__mx = 0;
  new PerformanceObserver((l) => l.getEntries().forEach((e) => window.__lt.push([e.startTime, e.duration]))).observe({ entryTypes: ["longtask"] });
  document.addEventListener("mousemove", (e) => { window.__mx += e.movementX; }, true);
});
const frames = (n) => page.evaluate((n) => new Promise((r) => { let i = 0; const f = () => (++i >= n ? r() : requestAnimationFrame(f)); requestAnimationFrame(f); }), n);
const yaw = async () => (await page.evaluate(() => window.rustcraft.camera()))[0];
// 1. The click that engages the camera.
const t0 = await page.evaluate(() => performance.now());
await page.mouse.move(640, 360);
await page.mouse.click(640, 360);
await frames(30);
const longTasks = (await page.evaluate(() => window.__lt)).filter((l) => l[0] > t0);
check(longTasks.length === 0, `the click causes no long task (${longTasks.map((l) => Math.round(l[1]) + " ms").join(", ") || "none"})`);
// 2. One move: the camera turns on the next frames.
await page.evaluate(() => { window.__mx = 0; });
const y0 = await yaw();
await page.mouse.move(640 + 120, 360);
await frames(2);
const y1 = await yaw();
const mx1 = await page.evaluate(() => window.__mx);
const turned = ((y1 - y0 + TAU * 1.5) % TAU) - TAU / 2;
check(mx1 !== 0 && Math.abs(turned - mx1 * SENS) < 1e-3, `one move of ${mx1} px turned the camera ${turned.toFixed(4)} rad within 2 frames (expected ${(mx1 * SENS).toFixed(4)})`);
// 3. A burst of small moves: nothing is dropped.
await page.evaluate(() => { window.__mx = 0; });
const y2 = await yaw();
for (let i = 1; i <= 40; i++) await page.mouse.move(760 + i * 3, 360);
await frames(3);
const y3 = await yaw();
const mx3 = await page.evaluate(() => window.__mx);
const turned3 = ((y3 - y2 + TAU * 1.5) % TAU) - TAU / 2;
check(mx3 !== 0 && Math.abs(turned3 - mx3 * SENS) < 2e-3, `40 small moves (${mx3} px in all) turned the camera ${turned3.toFixed(4)} rad (expected ${(mx3 * SENS).toFixed(4)})`);
await context.close();
stop();
process.exit(failed ? 1 : 0);
