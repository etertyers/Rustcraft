// The canvas follows the browser window (M73 follow-up). Resizes the viewport
// twice (and once with a device pixel ratio of 2) and compares, after each:
// the canvas's CSS box, its drawing buffer (width/height attributes), the
// wgpu surface the game reports, and the viewport. Usage: node resize.mjs [--profile dir]
import { launch, waitFor, serve } from "./lib.mjs";
import * as lib from "./lib.mjs";

const arg = (name, dflt) => { const i = process.argv.indexOf(`--${name}`); return i > 0 ? process.argv[i + 1] : dflt; };
const stop = await serve();
let failed = false;
// What the browser says the canvas is in device pixels (CSS size x ratio on a
// real display; Playwright's DPR emulation doesn't scale it, so it is the
// reference rather than width * dpr).
const read = (page) => page.evaluate(async () => {
  const c = document.getElementById("rustcraft");
  const r = c.getBoundingClientRect();
  const device = await new Promise((done) => {
    const o = new ResizeObserver((e) => { const b = e[0].devicePixelContentBoxSize?.[0]; o.disconnect(); done(b ? [b.inlineSize, b.blockSize] : null); });
    o.observe(c, { box: "device-pixel-content-box" });
  });
  return { device,
    viewport: [innerWidth, innerHeight], dpr: devicePixelRatio,
    css: [Math.round(r.width), Math.round(r.height)], inlineStyle: c.getAttribute("style"),
    buffer: [c.width, c.height], surface: window.rustcraft.surface(),
    scrollbars: [document.documentElement.scrollWidth > innerWidth, document.documentElement.scrollHeight > innerHeight],
  };
});
const settle = (page) => page.evaluate(() => new Promise((r) => setTimeout(() => requestAnimationFrame(() => requestAnimationFrame(r)), 400)));
for (const dpr of [1, 2]) {
  const { context, page, logs } = await launch(arg("profile", process.env.RUSTCRAFT_PROFILE), { width: 1280, height: 720 }, dpr);
  await page.goto(lib.URL_BASE + "?creative&mute");
  await waitFor(() => page.locator("#click").evaluate((e) => e.style.display === "block").catch(() => false), "start");
  await page.click("#go");
  await waitFor(() => logs.some((l) => l.includes("in Overworld")), "the world", 90000);
  await new Promise((r) => setTimeout(r, 3000));
  for (const [w, h] of [[1280, 720], [900, 600], [1500, 800], [700, 500]]) {
    await page.setViewportSize({ width: w, height: h });
    await settle(page);
    const s = await read(page);
    const want = s.device ?? [Math.round(w * dpr), Math.round(h * dpr)];
    const ok = s.css[0] === w && s.css[1] === h && s.buffer[0] === want[0] && s.buffer[1] === want[1]
      && s.surface[0] === want[0] && s.surface[1] === want[1] && !s.scrollbars[0] && !s.scrollbars[1];
    if (!ok) failed = true;
    console.log(`${ok ? "ok  " : "FAIL"} dpr ${dpr} viewport ${w}x${h}: canvas css ${s.css}, buffer ${s.buffer}, surface ${s.surface} (browser says ${want}), scrollbars ${s.scrollbars}${s.inlineStyle ? `, inline style "${s.inlineStyle}"` : ""}`);
  }
  console.log("page dpr:", await page.evaluate(() => devicePixelRatio), "| winit/game:", logs.filter((l) => /scale factor/.test(l)).map((l) => l.replace(/^\[log\] /, "").slice(0, 150)).join(" || "));
  await context.close();
}
stop();
process.exit(failed ? 1 : 0);
