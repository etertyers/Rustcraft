// CPU profile of the first click in a world (M73 follow-up): which wasm
// functions run in the stall. Build with `scripts/build-web.sh --names` first.
//   node profile-click.mjs [--extra "&mute"] --profile <dir>
import { launch, waitFor, serve } from "./lib.mjs";
import * as lib from "./lib.mjs";
const arg = (n, d) => { const i = process.argv.indexOf(`--${n}`); return i > 0 ? process.argv[i + 1] : d; };
const stop = await serve();
const { context, page, logs } = await launch(arg("profile", process.env.RUSTCRAFT_PROFILE), { width: 1440, height: 900 });
await page.goto(lib.URL_BASE + "?creative" + arg("extra", ""));
await waitFor(() => page.locator("#click").evaluate((e) => e.style.display === "block").catch(() => false), "start");
await page.click("#go");
await waitFor(() => logs.some((l) => l.includes("in Overworld")), "the world", 90000);
await new Promise((r) => setTimeout(r, 8000));
const cdp = await context.newCDPSession(page);
await cdp.send("Profiler.enable");
await cdp.send("Profiler.setSamplingInterval", { interval: 200 });
await cdp.send("Profiler.start");
await page.mouse.click(400, 300);
await new Promise((r) => setTimeout(r, 1500));
const { profile } = await cdp.send("Profiler.stop");
const byId = new Map(profile.nodes.map((n) => [n.id, n]));
const parent = new Map();
for (const n of profile.nodes) for (const c of n.children ?? []) parent.set(c, n.id);
const self = {}, total = {};
const dts = profile.timeDeltas;
profile.samples.forEach((id, i) => {
  const dt = (dts[i] ?? 0) / 1000;
  const name = byId.get(id).callFrame.functionName || "(anon)";
  self[name] = (self[name] ?? 0) + dt;
  const seen = new Set();
  for (let p = id; p; p = parent.get(p)) {
    const nm = byId.get(p).callFrame.functionName || "(anon)";
    if (!seen.has(nm)) { seen.add(nm); total[nm] = (total[nm] ?? 0) + dt; }
  }
});
const top = (o, n) => Object.entries(o).sort((a, b) => b[1] - a[1]).slice(0, n).map(([k, v]) => `${v.toFixed(0).padStart(5)} ms  ${k}`).join("\n");
console.log("-- inclusive (calls and everything under them)\n" + top(total, 28));
console.log("-- self time\n" + top(self, 12));
await context.close();
stop();
