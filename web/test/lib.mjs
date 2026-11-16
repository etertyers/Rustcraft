// Shared Playwright helpers for the web build tests (M71): Chrome, headed so
// it uses the real GPU, with a profile folder so OPFS survives a reload.
import { chromium } from "playwright";
import { mkdtempSync } from "node:fs";
import { tmpdir } from "node:os";
import { join, resolve, extname } from "node:path";
import { createServer } from "node:http";
import { spawnSync } from "node:child_process";
import { readFile } from "node:fs/promises";

export const ROOT = resolve(new URL("../..", import.meta.url).pathname);
export let URL_BASE = process.env.RUSTCRAFT_URL ?? "http://localhost:8080/";

const MIME = { ".html": "text/html", ".js": "text/javascript", ".wasm": "application/wasm", ".zip": "application/zip", ".json": "application/json" };

// A static server for target/web/ inside this process (same files as
// scripts/serve-web.sh), unless RUSTCRAFT_URL points at another one.
export async function serve() {
  if (process.env.RUSTCRAFT_URL) return () => {};
  const dir = join(ROOT, "target/web");
  const server = createServer(async (req, res) => {
    const path = decodeURIComponent(new URL(req.url, "http://x").pathname);
    try {
      // Local packs for the perf tests (never part of target/web).
      if (path.startsWith("/__packs/") && process.env.RUSTCRAFT_PACKS_DIR) {
        const data = await readFile(join(process.env.RUSTCRAFT_PACKS_DIR, path.slice("/__packs/".length)));
        res.writeHead(200, { "content-type": "application/zip" });
        return res.end(data);
      }
      const file = path === "/" ? "/index.html" : path;
      const data = await readFile(join(dir, file));
      res.writeHead(200, { "content-type": MIME[extname(file)] ?? "application/octet-stream", "cache-control": "no-cache" });
      res.end(data);
    } catch {
      res.writeHead(404); res.end();
    }
  });
  await new Promise((r) => server.listen(Number(process.env.PORT ?? 47831), "127.0.0.1", r));
  URL_BASE = `http://127.0.0.1:${server.address().port}/`;
  return () => server.close();
}

export async function launch(profile, viewport = { width: 1280, height: 720 }, deviceScaleFactor = 1) {
  const dir = profile ?? mkdtempSync(join(tmpdir(), "rustcraft-web-"));
  const context = await chromium.launchPersistentContext(dir, {
    channel: "chrome",
    headless: false,
    viewport,
    ...(viewport ? { deviceScaleFactor } : {}),
    args: ["--enable-unsafe-webgpu", "--enable-features=Vulkan"],
  });
  const page = context.pages()[0] ?? (await context.newPage());
  // Pointer lock needs the browser to be the frontmost app (WrongDocumentError
  // otherwise), so bring this Chrome forward (macOS).
  if (process.platform === "darwin" && !process.env.RUSTCRAFT_NO_FOCUS) {
    await page.bringToFront();
    spawnSync("osascript", ["-e", `tell application "System Events" to set frontmost of (first process whose unix id is ${context.browser()?.process?.()?.pid ?? 0}) to true`], { stdio: "ignore" });
    spawnSync("osascript", ["-e", 'tell application "Google Chrome" to activate'], { stdio: "ignore" });
  }
  // Pointer lock also needs the page's focus controller to be active.
  try {
    const cdp = await context.newCDPSession(page);
    await cdp.send("Emulation.setFocusEmulationEnabled", { enabled: true });
  } catch { /* not fatal */ }
  const logs = [];
  page.on("console", (m) => logs.push(`[${m.type()}] ${m.text()}`));
  page.on("pageerror", (e) => logs.push(`[pageerror] ${e.message}`));
  return { context, page, logs, dir };
}

export async function waitFor(fn, what, ms = 120000) {
  const end = Date.now() + ms;
  for (;;) {
    const v = await fn();
    if (v) return v;
    if (Date.now() > end) throw new Error(`timed out waiting for ${what}`);
    await new Promise((r) => setTimeout(r, 250));
  }
}

// Gets past the setup screen (if shown) and the "Click to start" button.
export async function passSetup(page, vanillaDir = join(ROOT, "assets/vanilla")) {
  await page.goto(URL_BASE);
  const state = await waitFor(async () => {
    if (await page.locator("#setup:not([hidden])").count()) return "setup";
    if (await page.locator("#click").evaluate((e) => e.style.display === "block").catch(() => false)) return "click";
    const err = await page.locator("#error").textContent();
    if (err) throw new Error(`loader error: ${err}`);
    return false;
  }, "the setup screen or the start button");
  if (state === "setup") {
    await page.setInputFiles("#vanilla-folder", vanillaDir);
    await waitFor(() => page.locator("#continue").isEnabled(), "the vanilla layer to be accepted", 600000);
    await page.click("#continue");
  }
  await waitFor(() => page.locator("#click").evaluate((e) => e.style.display === "block"), "the start button");
  return state;
}

export async function startGame(page, logs) {
  await page.click("#go");
  await waitFor(() => logs.some((l) => l.includes("window ") && l.includes("pixels")), "the window", 60000);
}
