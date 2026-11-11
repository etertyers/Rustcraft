// Records a Chrome trace of one web perf flight and lists the long tasks and
// GC pauses (M73 hot-spot hunting). Usage: node trace.mjs [--extra "&mute"] --profile <dir>
import { launch, waitFor, serve } from "./lib.mjs";
import * as lib from "./lib.mjs";
import { writeFileSync } from "node:fs";

const arg = (name, dflt) => { const i = process.argv.indexOf(`--${name}`); return i > 0 ? process.argv[i + 1] : dflt; };
const extra = arg("extra", "");
const stop = await serve();
const { context, page, logs } = await launch(arg("profile", process.env.RUSTCRAFT_PROFILE), { width: 1920, height: 1080 });
const cdp = await context.newCDPSession(page);
const events = [];
cdp.on("Tracing.dataCollected", (d) => events.push(...d.value));
await page.goto(lib.URL_BASE + `?perf-test&label=trace&row=web-vanilla&style=vanilla&shaders=off${extra}`);
await waitFor(() => page.locator("#click").evaluate((e) => e.style.display === "block").catch(() => false), "start");
await cdp.send("Tracing.start", { categories: "devtools.timeline,v8,v8.wasm,v8.wasm.detailed,disabled-by-default-v8.gc,disabled-by-default-v8.cpu_profiler,toplevel", transferMode: "ReportEvents" });
await page.click("#go");
await waitFor(async () => (await page.evaluate(() => !!window.rustcraft.read_file("docs/perf/trace.json"))), "the flight", 300000);
const done = new Promise((r) => cdp.once("Tracing.tracingComplete", r));
await cdp.send("Tracing.end");
await done;
// CPU profile samples: which functions ran inside each long frame.
const nodes = new Map();
const samples = [];
for (const e of events.filter((e) => e.name === "ProfileChunk")) {
  const p = e.args?.data?.cpuProfile;
  if (!p) continue;
  for (const n of p.nodes ?? []) nodes.set(`${e.id}:${n.id}`, n.callFrame.functionName || `(anon ${n.callFrame.url?.split("/").pop()}:${n.callFrame.lineNumber})`);
  const deltas = e.args.data.timeDeltas ?? [];
  let t = (samples.at(-1)?.t ?? 0);
  (p.samples ?? []).forEach((id, i) => { t += deltas[i] ?? 0; samples.push({ t, name: nodes.get(`${e.id}:${id}`) ?? "?", chunk: e.id }); });
}
for (const fr of events.filter((e) => e.ph === "X" && e.name === "FireAnimationFrame" && e.dur > 40000 && e.dur < 200000)) {
  const prof = events.find((e) => e.name === "Profile");
  const start = prof?.args?.data?.startTime ?? 0;
  const inside = samples.filter((x) => start + x.t >= fr.ts && start + x.t <= fr.ts + fr.dur);
  const by = {};
  for (const x of inside) by[x.name] = (by[x.name] ?? 0) + 1;
  console.log(`CPU samples in the ${(fr.dur / 1000).toFixed(0)} ms frame: ${inside.length}`);
  for (const [k, v] of Object.entries(by).sort((a, b) => b[1] - a[1]).slice(0, 12)) console.log(`    ${v}  ${k}`);
}
const long = events.filter((e) => e.ph === "X" && e.dur > 25000).sort((a, b) => b.dur - a.dur);
const gc = events.filter((e) => e.ph === "X" && /GC/.test(e.name) && e.dur > 5000).sort((a, b) => b.dur - a.dur);
console.log("events:", events.length);
// What happened inside the longest animation frame callbacks on the main thread.
const frames = events.filter((e) => e.ph === "X" && e.name === "FireAnimationFrame" && e.dur > 40000 && e.dur < 200000);
for (const fr of frames) {
  const inside = events.filter((e) => e.ph === "X" && e.pid === fr.pid && e.tid === fr.tid && e.ts >= fr.ts && e.ts + e.dur <= fr.ts + fr.dur && e !== fr && e.dur > 1000);
  const by = {};
  for (const e of inside) { by[e.name] = by[e.name] ?? { n: 0, ms: 0 }; by[e.name].n++; by[e.name].ms += e.dur / 1000; }
  console.log(`frame ${(fr.dur / 1000).toFixed(1)} ms at +${((fr.ts - events[0].ts) / 1e6).toFixed(1)} s, inside:`);
  for (const [k, v] of Object.entries(by).sort((a, b) => b[1].ms - a[1].ms).slice(0, 8)) console.log(`    ${v.ms.toFixed(1)} ms x${v.n}  ${k}`);
}
console.log("long events (>25 ms):");
for (const e of long.slice(0, 25)) console.log(`  ${(e.dur / 1000).toFixed(1)} ms  ${e.name}  [${e.cat}] pid ${e.pid} tid ${e.tid} ${JSON.stringify(e.args ?? {}).slice(0, 120)}`);
console.log("GC pauses (>5 ms):");
for (const e of gc.slice(0, 10)) console.log(`  ${(e.dur / 1000).toFixed(1)} ms  ${e.name}`);
writeFileSync("/tmp/rustcraft-trace-summary.txt", JSON.stringify({ long: long.slice(0, 50), gc: gc.slice(0, 20) }, null, 1));
await context.close();
stop();
