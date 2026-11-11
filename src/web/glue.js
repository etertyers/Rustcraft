// Browser glue for the wasm build (M71): OPFS, the page's query string,
// pointer lock and the keyboard. Kept small; everything with logic is Rust.

let rootPromise = null;
function root() {
  rootPromise ??= navigator.storage.getDirectory().then((r) =>
    r.getDirectoryHandle("rustcraft-v1", { create: true }));
  return rootPromise;
}

async function dirOf(path, create) {
  let dir = await root();
  const parts = path.split("/").filter(Boolean);
  const name = parts.pop();
  for (const p of parts) dir = await dir.getDirectoryHandle(p, { create });
  return [dir, name];
}

// Writes to one file are queued, so two in flight never fight for the lock.
const chains = new Map();
function queued(path, job) {
  const next = (chains.get(path) ?? Promise.resolve()).then(job, job);
  chains.set(path, next.catch(() => {}));
  return next;
}

export async function opfsWrite(path, bytes) {
  return queued(path, async () => {
    const [dir, name] = await dirOf(path, true);
    const file = await dir.getFileHandle(name, { create: true });
    const w = await file.createWritable();
    await w.write(bytes);
    await w.close();
  });
}

// Resolves once every queued write and delete has finished.
export function opfsIdle() {
  return Promise.all([...chains.values()]);
}

export async function opfsRead(path) {
  try {
    const [dir, name] = await dirOf(path, false);
    const file = await (await dir.getFileHandle(name)).getFile();
    return new Uint8Array(await file.arrayBuffer());
  } catch {
    return undefined;
  }
}

export async function opfsDelete(path) {
  return queued(path, async () => {
    try {
      const [dir, name] = await dirOf(path, false);
      await dir.removeEntry(name, { recursive: true });
    } catch { /* already gone */ }
  });
}

// Every file path under the root, e.g. "options.ron", "assets/downloads/x.zip".
export async function opfsList() {
  const out = [];
  async function walk(dir, prefix) {
    for await (const [name, handle] of dir.entries()) {
      if (handle.kind === "directory") await walk(handle, prefix + name + "/");
      else out.push(prefix + name);
    }
  }
  await walk(await root(), "");
  return out;
}

export function queryString() {
  return location.search;
}

export function setStatus(text) {
  const el = document.getElementById("status");
  if (el) el.textContent = text;
}

export function hideOverlay() {
  document.getElementById("overlay")?.remove();
}

export async function storageEstimate() {
  const e = (await navigator.storage?.estimate?.()) ?? {};
  return [e.usage ?? 0, e.quota ?? 0];
}

// Calls back when the pointer lock ends (Esc, tab switch), so the game can
// open its pause menu as Minecraft does when the window loses focus.
export function onPointerLockLost(cb) {
  document.addEventListener("pointerlockchange", () => {
    if (!document.pointerLockElement) cb();
  });
}

// The canvas asks for the lock with unadjustedMovement where supported.
export function installPointerLock(canvas) {
  const request = canvas.requestPointerLock.bind(canvas);
  canvas.requestPointerLock = (options) => {
    // Unadjusted movement where supported; otherwise the plain lock. A
    // refusal (no focus yet, the user pressed Esc) is not an error.
    const quiet = (p) => { p?.catch?.(() => {}); return p; };
    try {
      const p = request({ unadjustedMovement: true, ...options });
      p?.catch?.(() => { quiet(request(options)); });
      return p;
    } catch {
      try { return quiet(request(options)); } catch { /* ignored */ }
    }
  };
  canvas.addEventListener("contextmenu", (e) => e.preventDefault());
}

// In fullscreen on Chromium, lets Escape and the F-keys reach the game.
export function installKeyboardLock() {
  document.addEventListener("fullscreenchange", () => {
    if (!navigator.keyboard?.lock) return;
    if (document.fullscreenElement) {
      navigator.keyboard.lock(["Escape", "F1", "F2", "F3", "F4", "F5", "F11", "F12"]).catch(() => {});
    } else {
      navigator.keyboard.unlock();
    }
  });
}

// True once the player has clicked or pressed a key (audio may start).
let gesture = false;
export function listenForGesture() {
  const on = () => { gesture = true; };
  addEventListener("pointerdown", on, { once: true, capture: true });
  addEventListener("keydown", on, { once: true, capture: true });
}
export function hadGesture() {
  return gesture;
}

export function hardwareConcurrency() {
  return navigator.hardwareConcurrency ?? 4;
}

// A file the player saves (an exported world).
export function downloadBytes(filename, bytes) {
  const url = URL.createObjectURL(new Blob([bytes]));
  const a = Object.assign(document.createElement("a"), { href: url, download: filename });
  document.body.append(a);
  a.click();
  a.remove();
  setTimeout(() => URL.revokeObjectURL(url), 10000);
}

// Asks for one file; calls back with its bytes. Needs a click to be allowed.
export function pickFile(accept, cb) {
  const input = Object.assign(document.createElement("input"), { type: "file", accept });
  input.onchange = async () => {
    const file = input.files?.[0];
    if (file) cb(new Uint8Array(await file.arrayBuffer()));
  };
  input.click();
}

// Calls back when the tab is hidden or closing: the game saves then.
export function onLeavingPage(cb) {
  document.addEventListener("visibilitychange", () => { if (document.hidden) cb(); });
  addEventListener("pagehide", () => cb());
}
