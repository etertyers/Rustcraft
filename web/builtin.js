// The built-in files (M75): scripts/build-web.sh puts the clone's Minecraft
// files, packs and mod jars in builtin/ next to the page, as archives listed
// in builtin/manifest.json. This module fetches them, keeps each archive in
// OPFS under builtin/<stamp>/ (a rebuild with different files has a new
// stamp, so a stale cache is never used and is deleted), and gives the loader
// the plan. Pure parts are exported for web/test/builtin-unit.mjs.

const ROOT = "rustcraft-v1";
const BASE = "builtin/";

/** Where an archive is cached in OPFS. */
export function cachePath(stamp, file) {
  return `${BASE}${stamp}/${file}`;
}

/** The cached paths that belong to some other build. */
export function stalePaths(listing, stamp) {
  return listing.filter((p) => p.startsWith(BASE) && !p.startsWith(`${BASE}${stamp}/`));
}

/** The layers of a phase, in manifest order. */
export function layersOf(manifest, phase) {
  return manifest.layers.filter((l) => l.phase === phase);
}

/** A manifest the loader can use, or null. */
export function validManifest(m) {
  if (!m || typeof m.stamp !== "string" || !m.stamp || !Array.isArray(m.layers)) return null;
  const ok = m.layers.every((l) => l && typeof l.file === "string" && Number.isFinite(l.size) && ["boot", "background"].includes(l.phase) && ["vanilla", "pack", "mod"].includes(l.kind));
  return ok ? m : null;
}

/** Whether cached bytes are the archive the manifest describes. */
export function cacheMatches(layer, bytes) {
  return !!bytes && bytes.length === layer.size;
}

async function root() {
  return (await navigator.storage.getDirectory()).getDirectoryHandle(ROOT, { create: true });
}

async function dirOf(path, create) {
  let dir = await root();
  const parts = path.split("/").filter(Boolean);
  const name = parts.pop();
  for (const p of parts) dir = await dir.getDirectoryHandle(p, { create });
  return [dir, name];
}

async function readCached(path) {
  try {
    const [dir, name] = await dirOf(path, false);
    const file = await (await dir.getFileHandle(name)).getFile();
    return new Uint8Array(await file.arrayBuffer());
  } catch {
    return undefined;
  }
}

async function writeCached(path, bytes) {
  const [dir, name] = await dirOf(path, true);
  const w = await (await dir.getFileHandle(name, { create: true })).createWritable();
  await w.write(bytes);
  await w.close();
}

async function listBuiltin() {
  const out = [];
  async function walk(dir, prefix) {
    for await (const [name, handle] of dir.entries()) {
      if (handle.kind === "directory") await walk(handle, prefix + name + "/");
      else out.push(prefix + name);
    }
  }
  try {
    const [dir] = await dirOf(`${BASE}x`, false);
    await walk(dir, BASE);
  } catch { /* nothing cached yet */ }
  return out;
}

/** Deletes caches of other builds; returns how many files went. */
export async function dropStale(stamp) {
  let n = 0;
  for (const p of stalePaths(await listBuiltin(), stamp)) {
    try {
      const [dir, name] = await dirOf(p, false);
      await dir.removeEntry(name);
      n++;
    } catch { /* already gone */ }
  }
  return n;
}

/** The manifest next to the page, or null when this is a --lite build. */
export async function fetchManifest(base = "") {
  try {
    const r = await fetch(`${base}${BASE}manifest.json`, { cache: "no-store" });
    if (!r.ok) return null;
    return validManifest(await r.json());
  } catch {
    return null;
  }
}

async function download(url, size, onBytes) {
  const r = await fetch(url, { cache: "no-store" });
  if (!r.ok) throw new Error(`${url}: HTTP ${r.status}`);
  const out = new Uint8Array(size);
  let at = 0;
  const reader = r.body.getReader();
  for (;;) {
    const { done, value } = await reader.read();
    if (done) break;
    if (at + value.length > out.length) throw new Error(`${url}: longer than the ${size} bytes the manifest says`);
    out.set(value, at);
    at += value.length;
    onBytes?.(value.length);
  }
  if (at !== size) throw new Error(`${url}: ${at} bytes, the manifest says ${size}`);
  return out;
}

/**
 * One layer's bytes: from OPFS when this build's copy is there, else fetched
 * and cached. `onBytes(n)` reports fetched bytes; `fromCache` says which.
 */
export async function layerBytes(layer, stamp, { base = "", onBytes } = {}) {
  const path = cachePath(stamp, layer.file);
  const cached = await readCached(path);
  if (cacheMatches(layer, cached)) return { bytes: cached, fromCache: true };
  const bytes = await download(`${base}${BASE}${layer.file}`, layer.size, onBytes);
  try {
    await writeCached(path, bytes);
  } catch (e) {
    console.warn("built-in cache write failed (storage full?):", e?.message ?? e);
  }
  return { bytes, fromCache: false };
}
