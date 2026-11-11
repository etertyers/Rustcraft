// Rustcraft's browser loader (M71): starts the wasm, shows the asset setup
// screen when Minecraft's files aren't stored yet, then starts the game.
import init, { boot, vanilla_add, vanilla_add_zip, vanilla_finish, pack_add, start, save_now, export_world, import_world, storage_idle } from "./pkg/rustcraft.js";

// For tests and the console: window.rustcraft.save_now() saves the world.
window.rustcraft = { save_now, export_world, import_world, storage_idle };

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

async function main() {
  try {
    await init();
    let haveVanilla = await boot("assets.zip");
    if (!haveVanilla) {
      await setup();
      $("setup").hidden = true;
    }
    status("Starting...");
    // The click that starts the game is also the first gesture audio and
    // the pointer lock need.
    $("click").style.display = "block";
    status("");
    await new Promise((resolve) => { $("go").onclick = resolve; });
    start();
  } catch (e) {
    fail(e);
    status("Rustcraft couldn't start.");
  }
}
main();
