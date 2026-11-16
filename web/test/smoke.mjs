// Smoke test (M71): load the page, pass the setup screen, start, and check the
// game drew a frame with no errors. Usage: node smoke.mjs [--screenshot file]
import { launch, passSetup, startGame, waitFor, serve } from "./lib.mjs";

const stop = await serve();

const shot = process.argv.includes("--screenshot") ? process.argv[process.argv.indexOf("--screenshot") + 1] : null;
const { context, page, logs, dir } = await launch(process.env.RUSTCRAFT_PROFILE);
let failed = false;
try {
  const first = await passSetup(page);
  console.log("setup screen:", first);
  await startGame(page, logs);
  await waitFor(() => logs.some((l) => l.includes("GPU adapter")), "the GPU adapter line", 60000);
  await new Promise((r) => setTimeout(r, 4000));
  const errors = logs.filter((l) => l.startsWith("[error]") || l.startsWith("[pageerror]"));
  console.log(process.env.VERBOSE ? logs.join("\n") : logs.filter((l) => /GPU adapter|window|assets:|loaded|sound/.test(l)).slice(0, 8).join("\n"));
  if (errors.length) { console.log("ERRORS:\n" + errors.slice(0, 10).join("\n")); failed = true; }
  if (shot) await page.screenshot({ path: shot });
} catch (e) {
  console.log("FAILED:", e.message);
  console.log(logs.slice(-25).join("\n"));
  failed = true;
}
import("node:fs").then((fs) => fs.writeFileSync(process.env.LOGFILE ?? "/tmp/m71-logs.txt", logs.join("\n")));
console.log("profile:", dir);
await context.close();
stop();
process.exit(failed ? 1 : 0);
