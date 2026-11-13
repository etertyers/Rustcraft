// The web perf test (M73): `?perf-test` in the page flies the same scripted
// path as the native `--perf-test` (60 s after a 5 s warm-up, render distance
// 9) and writes the same fields; this runs it in Chrome (headed, real GPU)
// `--runs` times, each in a fresh page, and writes the medians, like
// `perf::combine`, to docs/perf/<label>.json and .md.
//   node perf.mjs --label m73-web [--runs 3] [--row web-vanilla] [--style vanilla]
//                 [--shaders off|low|medium|high] [--extra "&distance=9"]
//                 [--profile <dir with vanilla stored>] [--packs <dir of pack .zips>]
import { launch, waitFor, serve, ROOT } from "./lib.mjs";
import * as lib from "./lib.mjs";
import { writeFileSync, readdirSync } from "node:fs";
import { join } from "node:path";

const arg = (name, dflt) => { const i = process.argv.indexOf(`--${name}`); return i > 0 ? process.argv[i + 1] : dflt; };
const label = arg("label", "m73-web");
const runs = Number(arg("runs", 3));
const row = arg("row", "web-vanilla");
const style = arg("style", "vanilla");
const shaders = arg("shaders", "off");
const extra = arg("extra", "");
const profile = arg("profile", process.env.RUSTCRAFT_PROFILE);
const packs = arg("packs", null);
if (packs) process.env.RUSTCRAFT_PACKS_DIR = packs;

const median = (v) => { const s = [...v].sort((a, b) => a - b); const n = s.length; return n % 2 ? s[(n - 1) / 2] : (s[n / 2 - 1] + s[n / 2]) / 2; };
const spread = (v) => { const m = median(v); return m === 0 ? 0 : (100 * (Math.max(...v) - Math.min(...v))) / Math.abs(m); };

const stop = await serve();
const results = [];
let browserVersion = "";
for (let i = 1; i <= runs; i++) {
  const name = `${label}-run${i}`;
  const { context, page, logs } = await launch(profile, { width: 1920, height: 1080 });
  try {
    browserVersion = context.browser()?.version() ?? browserVersion;
    if (packs) {
      // Packs the setup screen would take, added for this run (never committed).
      await page.goto(lib.URL_BASE);
      await page.waitForSelector("#click, #setup:not([hidden])", { timeout: 120000 }).catch(() => {});
    }
    const shaderQuery = ["low", "medium", "high"].includes(shaders) ? `shaders=on&quality=${shaders}` : `shaders=${shaders}`;
    const query = `?perf-test&label=${name}&row=${row}&style=${style}&${shaderQuery}${extra}`;
    await page.goto(lib.URL_BASE + query);
    await waitFor(() => page.locator("#click").evaluate((e) => e.style.display === "block").catch(() => false), "the start button");
    if (packs) {
      // pack_add before start: the loader's pack input is hidden in the setup panel only, so call the export.
      for (const f of readdirSync(packs).filter((f) => f.endsWith(".zip"))) {
        await page.evaluate(async (n) => {
          const r = await fetch("/__packs/" + encodeURIComponent(n));
          window.rustcraft.pack_add(n, new Uint8Array(await r.arrayBuffer()));
        }, f);
      }
    }
    await page.click("#go");
    const file = `docs/perf/${name}.json`;
    const raw = await waitFor(async () => {
      const bytes = await page.evaluate((p) => { const b = window.rustcraft.read_file(p); return b ? Array.from(b) : null; }, file);
      return bytes && Buffer.from(bytes).toString();
    }, `${name}'s results`, 300000);
    results.push(JSON.parse(raw));
    (await import("node:fs")).writeFileSync(`/tmp/${name}-logs.txt`, logs.join("\n"));
    console.log(logs.filter((l) => /flight|ended at|display:|generator workers|WARN.*(shader|pipeline)/i.test(l)).map((l) => l.slice(0, 330)).join("\n"));
    console.log(`run ${i}: ${results.at(-1).measures.average_fps.toFixed(1)} fps avg, 1% low ${results.at(-1).measures.one_percent_low_fps.toFixed(1)}, worst ${results.at(-1).measures.worst_frame_ms.toFixed(1)} ms, over 33/50 ms: ${results.at(-1).measures.frames_over_33ms}/${results.at(-1).measures.frames_over_50ms}`);
  } catch (e) {
    console.log(`run ${i} FAILED: ${e.message}`);
    (await import("node:fs")).writeFileSync(`/tmp/${name}-fail-logs.txt`, logs.join("\n"));
    console.log(logs.filter((l) => /error|panick|Invalid/i.test(l)).slice(0, 5).join("\n"));
  }
  await context.close();
}
stop();
if (!results.length) process.exit(1);

const keys = Object.keys(results[0].measures);
const measures = {}, spread_pct = {};
for (const k of keys) {
  const v = results.map((r) => r.measures[k]).filter((x) => x !== undefined);
  measures[k] = median(v);
  spread_pct[k] = spread(v);
}
const combined = {
  ...results[0],
  label, row,
  machine: "web",
  machine_detail: `Chrome ${browserVersion} on the Mac mini/MacBook the sessions run on (${results[0].machine_detail.slice(0, 120)})`,
  runs: results.length, measures, spread_pct,
  per_run: results.map((r) => r.measures),
  worst_of_runs_ms: Math.max(...results.map((r) => r.measures.worst_frame_ms)),
};
const dir = join(ROOT, "docs/perf");
writeFileSync(join(dir, `${label}.json`), JSON.stringify(combined, null, 2) + "\n");
let md = `# ${label} performance\n\n${results.length} flights of ${combined.seconds} s (seed ${combined.seed}, render distance ${combined.render_distance}) in ${combined.machine_detail}; median and spread (max − min as % of the median). Row \`${row}\`, style \`${style}\`, shaders \`${shaders}\`${extra ? `, extra \`${extra}\`` : ""}.\n\n| Measure | Median | Spread |\n|---|---|---|\n`;
for (const k of keys) md += `| ${k} | ${measures[k].toFixed(2)} | ${spread_pct[k].toFixed(1)}% |\n`;
md += `| worst frame over all runs (ms) | ${combined.worst_of_runs_ms.toFixed(2)} | |\n`;
writeFileSync(join(dir, `${label}.md`), md);
console.log(`wrote docs/perf/${label}.json and .md`);
