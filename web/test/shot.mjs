// One screenshot for M75: a fresh browser profile opens the default build straight to the title screen.
import { launch, serve, waitFor } from "./lib.mjs";
import * as lib from "./lib.mjs";
import { mkdtempSync } from "node:fs"; import { tmpdir } from "node:os"; import { join } from "node:path";
const stop = await serve();
const { context, page, logs } = await launch(mkdtempSync(join(tmpdir(), "rc-shot-")), { width: 1280, height: 720 });
await page.goto(lib.URL_BASE);
await waitFor(() => page.locator("#click").evaluate((e) => e.style.display === "block").catch(() => false), "start", 120000);
await page.click("#go");
await waitFor(() => logs.some((l) => l.includes("music: playing")), "the menu music", 60000);
await new Promise((r) => setTimeout(r, 2500));
await page.screenshot({ path: process.argv[2] });
console.log(logs.find((l) => l.includes("music: playing")).replace(/^.*music: /, "music: "));
await context.close(); stop();
