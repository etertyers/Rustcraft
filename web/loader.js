// Rustcraft's browser loader (M71, M75): starts the wasm, loads the build's
// own Minecraft files, packs and jars when it has them (builtin/, see
// builtin.js) and goes straight to "Click to start"; a --lite build, or built-in
// files that fail to load, get the asset setup screen instead. The rest of the
// built-in files (the sounds, the big pack) arrive while the game runs.
import init, { boot, vanilla_add, vanilla_add_zip, vanilla_finish, vanilla_check, pack_add, builtin_pack_add, builtin_mod_add, bg_zip_start, bg_zip_step, start, save_now, export_world, import_world, storage_idle, read_file, camera, surface } from "./pkg/rustcraft.js";
import { fetchManifest, layersOf, dropStale, layerBytes } from "./builtin.js";

// For tests and the console: window.rustcraft.save_now() saves the world.
window.rustcraft = { save_now, export_world, import_world, storage_idle, read_file, pack_add, camera, surface, loadTimes: {} };

const $ = (id) => document.getElementById(id);
const status = (t) => { $("status").textContent = t; };
const fail = (e) => { $("error").textContent = String(e?.message ?? e); console.error(e); };

async function readAll(file) {
  return new Uint8Array(await file.arrayBuffer());
}

async function addVanilla(files) {
  let kept = 0;
  for (const [i, file] of files.entries()) {
    const path = file.webkitRelativePath || file.name;
    if (!path.includes("assets/minecraft/") && !path.includes("assets\\minecraft\\")) continue;
    if (vanilla_add(path, await readAll(file))) kept++;
    if (i % 200 === 0) status(`Reading Minecraft's files... ${i}/${files.length}`);
  }
  return kept;
}

async function setup() {
  $("setup").hidden = false;
  status("Minecraft's files are needed once.");
  let have = false;
  const done = async (kept) => {
    try {
      const summary = await vanilla_finish();
      have = true;
      $("error").textContent = "";
      status(summary);
      $("continue").disabled = false;
    } catch (e) {
      status(`Read ${kept} files.`);
      fail(e);
    }
  };
  $("vanilla-folder").onchange = async (e) => {
    try { await done(await addVanilla([...e.target.files])); } catch (err) { fail(err); }
  };
  $("vanilla-zip").onchange = async (e) => {
    try {
      let kept = 0;
      for (const f of e.target.files) kept += vanilla_add_zip(await readAll(f));
      await done(kept);
    } catch (err) { fail(err); }
  };
  $("packs").onchange = async (e) => {
    try {
      for (const f of e.target.files) pack_add(f.name, await readAll(f));
      $("have").textContent = `${e.target.files.length} pack file(s) added`;
    } catch (err) { fail(err); }
  };
  await new Promise((resolve) => { $("continue").onclick = resolve; });
  return have;
}

const tick = () => new Promise((r) => setTimeout(r, 4));
const mb = (n) => `${(n / 1e6).toFixed(0)} MB`;

function deliver(layer, bytes) {
  const name = layer.file.split("/").pop();
  if (layer.kind === "vanilla") vanilla_add_zip(bytes);
  else if (layer.kind === "pack") builtin_pack_add(name, bytes);
  else builtin_mod_add(name, bytes);
}

// The layers needed before the title screen: Minecraft's core files, the
// default style's packs and the small jars.
async function loadBoot(manifest) {
  const t0 = performance.now();
  // ?eager loads the background layers before the title screen too: only for
  // measuring what that would cost (docs/web.md).
  const layers = location.search.includes("eager") ? manifest.layers : layersOf(manifest, "boot");
  const total = layers.reduce((s, l) => s + l.size, 0);
  let done = 0, fetched = 0, cached = 0;
  await dropStale(manifest.stamp);
  for (const layer of layers) {
    const shown = () => status(`Loading Minecraft's files... ${Math.round(((done + 0) / total) * 100)}% (${mb(total)})`);
    shown();
    const r = await layerBytes(layer, manifest.stamp, { onBytes: (n) => { done += n; fetched += n; shown(); } });
    if (r.fromCache) { done += layer.size; cached += layer.size; shown(); }
    deliver(layer, r.bytes);
  }
  const summary = vanilla_check(location.search.includes("eager"));
  window.rustcraft.loadTimes.boot = { ms: performance.now() - t0, fetched, cached, layers: layers.length, summary };
  console.info(`built-in files ready in ${(performance.now() - t0).toFixed(0)} ms (${mb(fetched)} fetched, ${mb(cached)} from the cache)`);
}

// The rest, while the game runs: sounds, the big pack, the big jar. Unpacked
// a few milliseconds at a time so no frame stalls.
async function loadBackground(manifest) {
  if (location.search.includes("nobackground") || location.search.includes("eager")) return;
  const layers = layersOf(manifest, "background");
  const total = layers.reduce((s, l) => s + l.size, 0);
  const el = $("bg");
  let done = 0;
  const t0 = performance.now();
  const show = () => { if (el) { el.hidden = false; el.textContent = `Loading sounds and packs... ${Math.round((done / total) * 100)}%`; } };
  show();
  for (const layer of layers) {
    try {
      const r = await layerBytes(layer, manifest.stamp, { onBytes: (n) => { done += n; show(); } });
      if (r.fromCache) { done += layer.size; show(); }
      await tick();
      if (layer.kind === "vanilla") {
        bg_zip_start(r.bytes);
        while (!bg_zip_step(3)) await tick();
      } else {
        deliver(layer, r.bytes);
      }
    } catch (e) {
      console.warn(`built-in ${layer.file} didn't load:`, e?.message ?? e);
      done += layer.size;
    }
    await tick();
  }
  window.rustcraft.loadTimes.background = { ms: performance.now() - t0, layers: layers.length };
  console.info(`built-in background files loaded in ${(performance.now() - t0).toFixed(0)} ms`);
  if (el) el.hidden = true;
}

async function main() {
  try {
    if (!window.isSecureContext) {
      // OPFS, WebGPU and pointer lock options need https or localhost.
      throw new Error("This page needs a secure address: https://, or http://localhost. Start scripts/serve-web.sh --https and open its https URL (accept the certificate warning once).");
    }
    const t0 = performance.now();
    await init();
    const manifest = await fetchManifest();
    const tBoot = performance.now();
    let haveVanilla = await boot("assets.zip", !!manifest);
    window.rustcraft.loadTimes.rustBoot = performance.now() - tBoot;
    let builtIn = false;
    if (manifest) {
      try {
        await loadBoot(manifest);
        builtIn = true;
        haveVanilla = true;
      } catch (e) {
        // One line of reason, then the setup screen.
        const reason = String(e?.message ?? e).split("\n")[0];
        console.error("built-in files failed:", e);
        $("error").textContent = `The built-in Minecraft files couldn't be loaded (${reason}). You can pick them yourself:`;
        haveVanilla = false;
      }
    }
    if (!haveVanilla) {
      await setup();
      $("setup").hidden = true;
    }
    window.rustcraft.loadTimes.titleReady = performance.now() - t0;
    status("Starting...");
    // The click that starts the game is also the first gesture audio and
    // the pointer lock need.
    $("click").style.display = "block";
    status("");
    await new Promise((resolve) => { $("go").onclick = resolve; });
    start();
    if (builtIn) loadBackground(manifest);
  } catch (e) {
    fail(e);
    status("Rustcraft couldn't start.");
  }
}
main();
