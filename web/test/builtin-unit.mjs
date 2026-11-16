// Unit tests for web/builtin.js's pure parts (M75): node web/test/builtin-unit.mjs
import assert from "node:assert/strict";
import { cachePath, stalePaths, layersOf, validManifest, cacheMatches } from "../builtin.js";

const m = { stamp: "abc123", layers: [
  { name: "core", kind: "vanilla", phase: "boot", file: "vanilla-core.zip", size: 10, files: 3 },
  { name: "pack", kind: "pack", phase: "boot", file: "packs/p.zip", size: 5, files: 1 },
  { name: "sounds", kind: "vanilla", phase: "background", file: "vanilla-sounds-1.zip", size: 20, files: 9 },
] };
let n = 0;
const t = (name, fn) => { fn(); n++; console.log("ok  ", name); };

t("an archive is cached under the build's stamp", () => {
  assert.equal(cachePath("abc123", "packs/p.zip"), "builtin/abc123/packs/p.zip");
});
t("only another build's files are stale", () => {
  const listing = ["builtin/old1/vanilla-core.zip", "builtin/abc123/vanilla-core.zip", "builtin/old2/packs/p.zip", "options.ron", "saves/W/level.ron"];
  assert.deepEqual(stalePaths(listing, "abc123"), ["builtin/old1/vanilla-core.zip", "builtin/old2/packs/p.zip"]);
});
t("a stamp that merely starts like another is still another build", () => {
  assert.deepEqual(stalePaths(["builtin/abc1234/x.zip"], "abc123"), ["builtin/abc1234/x.zip"]);
});
t("layers come out by phase in manifest order", () => {
  assert.deepEqual(layersOf(m, "boot").map((l) => l.name), ["core", "pack"]);
  assert.deepEqual(layersOf(m, "background").map((l) => l.name), ["sounds"]);
});
t("a good manifest is accepted, broken ones are not", () => {
  assert.equal(validManifest(m), m);
  assert.equal(validManifest(null), null);
  assert.equal(validManifest({ stamp: "", layers: [] }), null);
  assert.equal(validManifest({ stamp: "x" }), null);
  assert.equal(validManifest({ stamp: "x", layers: [{ file: "a", size: 1, phase: "later", kind: "pack" }] }), null);
  assert.equal(validManifest({ stamp: "x", layers: [{ file: "a", size: "1", phase: "boot", kind: "pack" }] }), null);
});
t("cached bytes count only when they are the size the manifest says", () => {
  const layer = m.layers[0];
  assert.equal(cacheMatches(layer, new Uint8Array(10)), true);
  assert.equal(cacheMatches(layer, new Uint8Array(9)), false, "a half-written cache is refetched");
  assert.equal(cacheMatches(layer, undefined), false);
});
console.log(`${n} passed`);
