// Frame times with the server on the page vs in its own Web Worker (M76),
// the same scripted walk in Chrome: a new creative world, W held for 30 s
// (chunks keep loading), requestAnimationFrame deltas on the page.
// Usage: node server-worker-ab.mjs [runs]   -> prints one line per run and mode
import { launch, waitFor, serve } from "./lib.mjs";
import * as lib from "./lib.mjs";

const runs = Number(process.argv[2] ?? 3);
const stop = await serve();
const stats = (d) => {
  const n = d.length, total = d.reduce((a, b) => a + b, 0);
  const missed = d.filter((x) => x > 1000 / 60 * 1.5).length;
  const sorted = [...d].sort((a, b) => b - a);
  const low = sorted.slice(0, Math.max(1, Math.floor(n / 100)));
  return { fps: 1000 / (total / n), missedPct: (100 * missed) / n, low1: 1000 / (low.reduce((a, b) => a + b, 0) / low.length), worst: sorted[0], over50: d.filter((x) => x > 50).length };
};
for (const mode of ["page", "worker"]) {
  for (let i = 1; i <= runs; i++) {
    const { context, page, logs } = await launch(undefined);
    await page.goto(lib.URL_BASE + `?creative&seed=5&eager&mute${mode === "worker" ? "&server-worker" : ""}`);
    await waitFor(async () => page.locator("#click").evaluate((e) => e.style.display === "block").catch(() => false), "the start button");
    await page.click("#go");
    await waitFor(() => logs.some((l) => l.includes("in Overworld") || l.includes("GPU adapter")), "the world", 120000);
    await new Promise((r) => setTimeout(r, 6000));
    await page.locator("#rustcraft").click({ position: { x: 640, y: 360 } });
    await page.keyboard.down("w");
    const d = await page.evaluate(() => new Promise((res) => {
      const out = []; let last = performance.now(); const t0 = last;
      const f = (t) => { out.push(t - last); last = t; t - t0 < 30000 ? requestAnimationFrame(f) : res(out); };
      requestAnimationFrame(f);
    }));
    await page.keyboard.up("w");
    const s = stats(d.slice(1));
    console.log(`${mode} run ${i}: ${s.fps.toFixed(2)} fps, missed ${s.missedPct.toFixed(2)}%, 1% low ${s.low1.toFixed(1)}, worst ${s.worst.toFixed(1)} ms, frames over 50 ms: ${s.over50}`);
    await context.close();
  }
}
await stop();
