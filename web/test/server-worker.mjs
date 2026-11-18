// The server in its own Web Worker (M76), in Chrome: ?creative&server-worker.
//  - the worker starts the world and the page joins it;
//  - generator workers belong to the server worker (3, started once);
//  - chunks arrive and are drawn, the page's frames keep coming and nothing
//    panics; a block broken on the page is changed in the worker's world.
// Usage: node server-worker.mjs   (needs target/web from build-web.sh)
import { launch, waitFor, serve } from "./lib.mjs";
import * as lib from "./lib.mjs";

const stop = await serve();
const { context, page, logs } = await launch(process.env.RUSTCRAFT_PROFILE);
let failed = false;
const check = (ok, text) => { if (!ok) failed = true; console.log(`${ok ? "ok  " : "FAIL"} ${text}`); };
const count = (re) => logs.filter((l) => re.test(l)).length;
try {
  await page.goto(lib.URL_BASE + "?creative&server-worker&eager&mute");
  await waitFor(async () => page.locator("#click").evaluate((e) => e.style.display === "block").catch(() => false), "the start button");
  await page.click("#go");
  await waitFor(() => logs.some((l) => l.includes("in Overworld")), "the world (the server worker's welcome)", 120000);
  check(count(/server worker: world .* running/) === 1, "the server worker started the world");
  check(count(/playing .* from a server worker/) === 1, "the page plays it as a client");
  await new Promise((r) => setTimeout(r, 8000));
  check(count(/3 generator workers started/) === 1, "one pool of generator workers (the server worker's), none on the page");
  const frames = await page.evaluate(() => new Promise((res) => { let n = 0; const t = performance.now(); const f = () => { n++; performance.now() - t < 2000 ? requestAnimationFrame(f) : res(n); }; requestAnimationFrame(f); }));
  check(frames > 60, `the page keeps drawing frames (${frames} in 2 s)`);
  const bad = logs.filter((l) => /^\[(error|pageerror)\]/.test(l) || /panicked/.test(l));
  check(bad.length === 0, `no errors or panics${bad.length ? ": " + bad.slice(0, 3).join(" | ").slice(0, 300) : ""}`);
} catch (e) {
  console.log("FAILED:", e.message);
  failed = true;
}
await context.close();
await stop();
process.exit(failed ? 1 : 0);
