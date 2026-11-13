// What does Chrome report for a canvas under device pixel ratio 2 (emulated)?
import { launch } from "./lib.mjs";
const { context, page } = await launch(undefined, { width: 800, height: 600 }, 2);
await page.goto("about:blank");
const r = await page.evaluate(() => new Promise((resolve) => {
  const c = document.createElement("canvas");
  Object.assign(c.style, { position: "fixed", inset: 0, width: "100vw", height: "100vh" });
  document.body.append(c);
  new ResizeObserver((entries) => {
    const e = entries[0];
    resolve({ dpr: devicePixelRatio, css: [e.contentRect.width, e.contentRect.height], contentBox: [e.contentBoxSize[0].inlineSize, e.contentBoxSize[0].blockSize], devicePixelContentBox: e.devicePixelContentBoxSize?.[0] ? [e.devicePixelContentBoxSize[0].inlineSize, e.devicePixelContentBoxSize[0].blockSize] : null });
  }).observe(c, { box: "device-pixel-content-box" });
}));
console.log(JSON.stringify(r));
await context.close();
