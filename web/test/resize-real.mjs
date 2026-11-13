// Like resize.mjs, but with no Playwright emulation: the real window on the
// real display (its real devicePixelRatio), resized with CDP's
// Browser.setWindowBounds.
import { launch, waitFor, serve } from "./lib.mjs";
import * as lib from "./lib.mjs";
const arg = (name, dflt) => { const i = process.argv.indexOf(`--${name}`); return i > 0 ? process.argv[i + 1] : dflt; };
const stop = await serve();
const { context, page, logs } = await launch(arg("profile", process.env.RUSTCRAFT_PROFILE), null);
await page.goto(lib.URL_BASE + "?creative&mute");
await waitFor(() => page.locator("#click").evaluate((e) => e.style.display === "block").catch(() => false), "start");
await page.click("#go");
await waitFor(() => logs.some((l) => l.includes("in Overworld")), "the world", 90000);
await new Promise((r) => setTimeout(r, 3000));
const cdp = await context.newCDPSession(page);
const { windowId } = await cdp.send("Browser.getWindowForTarget");
const read = () => page.evaluate(() => {
  const c = document.getElementById("rustcraft"); const r = c.getBoundingClientRect();
  return { viewport: [innerWidth, innerHeight], dpr: devicePixelRatio, css: [Math.round(r.width), Math.round(r.height)], buffer: [c.width, c.height], surface: window.rustcraft.surface(), fullscreen: !!document.fullscreenElement };
});
const settle = () => page.evaluate(() => new Promise((r) => setTimeout(() => requestAnimationFrame(() => requestAnimationFrame(r)), 600)));
let failed = false;
const check = async (label) => {
  await settle();
  const s = await read();
  const want = [Math.round(s.viewport[0] * s.dpr), Math.round(s.viewport[1] * s.dpr)];
  const ok = s.css[0] === s.viewport[0] && s.css[1] === s.viewport[1] && s.buffer[0] === want[0] && s.buffer[1] === want[1] && s.surface[0] === want[0] && s.surface[1] === want[1];
  if (!ok) failed = true;
  console.log(`${ok ? "ok  " : "FAIL"} ${label}: dpr ${s.dpr}, viewport ${s.viewport}, canvas css ${s.css}, buffer ${s.buffer}, surface ${s.surface} (want ${want})`);
};
await check("start");
for (const [w, h] of [[1000, 700], [1300, 800], [900, 650]]) {
  await cdp.send("Browser.setWindowBounds", { windowId, bounds: { windowState: "normal", width: w, height: h } });
  await check(`window ${w}x${h}`);
}
// Fullscreen is the same kind of viewport change (the ResizeObserver path
// above); Chrome under automation won't enter it, so it is a manual check.
await context.close();
stop();
process.exit(failed ? 1 : 0);
