# Textures: what the audit finds and why

`cargo run --release -- --texture-audit [--mode vanilla|barebones]` draws every block, item, mob and
sprite offscreen and flags what looks wrong (see `src/audit/`). This file lists what it still flags
after M61 and the reason. Kenzo's own marks (`texture-marks.ron`, `--texture-marks`) come first
when fixing: none were available when M61 was written.

## Causes fixed in M61

| Cause | Textures affected | Fix |
|---|---|---|
| 26.3 renamed and moved mob textures (`pig_temperate`, `cat_tabby`, `rabbit_brown`, `blaze/blaze`...), so they resolved only through Kenzo's older copies | pig, cow, chicken, blaze, phantom, silverfish, witch, 11 cat coats, 6 rabbit coats | names added to `assets/data/texture_names.ron`; a test checks every mob texture resolves in the vanilla layer alone |
| The canvas grew but the layout didn't (26.3 pig and cow: the old layout in the top half of 64×64; bee: 64×64), while `mobs.ron` declared the old size, so the packs' correct textures were refused and an older wrong-sized one drawn | pig, cow, bee (and mooshroom, which uses the cow's model) | declared sizes now 64×64; vanilla's ghast (exactly 2× the old size) is accepted as a whole multiple |
| A custom entity model with no texture of its own got the mob's texture chosen by the built-in boxes' size, not by the model's `textureSize` | every Fresh Animations mob whose canvas differs from `mobs.ron` (pig, cow...) | `Look::build` picks the texture that fits the `.jem`'s `textureSize` |
| Custom models' parts hung 8 pixels too high: a top-level part's animated translate is an offset from its file translate (the body's `ty` of 16 on a file translate of -8 is the ground, 24), while a child's replaces its own | everything under `body` in Fresh Animations' pig, cow, chicken and likely sheep | `cem/pose.rs`; a test checks a fixture and, with the real pack, that pig, cow and chicken match vanilla's height, length and feet |
| Detector artifacts: zero-thickness boxes (a bee's wing) have faces with no area; the size check compared the top layer's file instead of the one chosen | bee wings, legs | detector skips them; compares the chosen texture |
| Spawn eggs did nothing | all 38 | `src/game/spawn_eggs.rs`: block, spawner, calf, dispenser, custom name, used up in survival only; tests incl. two clients |

## Still flagged (vanilla stack: 73, Barebones: 163)

* **Overlay layers vanilla leaves clear (correct):** `skeleton`, `stray`, `wither_skeleton`, `enderman`
  (box 1 is the hat or head overlay, which these textures leave transparent).
* **Lightning rods' icons are empty (8 items):** the block's model isn't drawn by Rustcraft's block
  mesher (a placeholder block from vanilla's data); not a texture problem. Open.
* **Layouts that changed in 26.x and whose boxes weren't rebuilt (open):** chicken wings, cod and
  salmon fins (the cod's UVs also run past the texture), strider, bee antennae and legs, silverfish,
  `cat#6`, piglin family ears (these are shaped sprites, so some clear pixels are expected; not
  verified). Rebuilding them needs vanilla's 26.x part sizes and pivots, which aren't in the
  downloaded assets; I did not guess them.
* **Barebones only:** `magma_cube` (the 26.3 texture is 64×64 in a new layout, not the old
  one on a bigger canvas), and extra "transparent part" flags on Fresh Animations models whose own
  tiny boxes (eyes, pupils) sample clear pixels by design, and on babies (a baby is drawn as a
  half-size adult).

## Not drawn or not animated (open)

31 textures have a `.png.mcmeta` but aren't in the block atlas, so they don't animate:
* old-name duplicates from Kenzo's 1.12-style folder (`blocks/lava_still`, `blocks/portal`...): the
  real ones are animated under `block/`;
* states Rustcraft doesn't model: lit campfires, soul fire, charged respawn anchor, sculk catalyst
  bloom, sculk sensor tendril, shrieker, stonecutter saw, tall seagrass top;
* sprites drawn by other code: conduit wind, GUI sprites, the vibration particle.

## Rustcraft-only textures vanilla has no file for

None: every texture Rustcraft asks for exists in vanilla 26.3 (M59's asset report has 0 missing).

## Rabbits and the styles (M62)

* The 26.3 rabbit changed layout (696 of 2048 shape pixels differ from the old one), which the built-in
  boxes don't have, so the rabbit (and any mob like it) draws Rustcraft's own old-layout texture where
  vanilla's doesn't fit the model (`Look`'s `own_texture` fallback, logged once per texture).
* Rustcraft's own `assets/textures` never replace a file vanilla has (v1.5). In the Vanilla style 17
  block and item textures are Rustcraft's own because vanilla 26.3 has no file under those names: the
  beds, the chest, end portal and gateway, the dropper front, the piston top, the redstone lamp, and
  the bed, empty map and shield items.
* **Realistic = Patrix 32x alone.** With Fresh Animations under it the audit flags 174 mob parts
  against 153 for Patrix alone: its mob textures clash with Fresh Animations' models on the shulker
  (0 → 10 flags) and the villager (2 → 13). Patrix alone has 153 flags (rabbit 25, bee 18, silverfish
  13, cat 10, salmon 10, horse 8, strider 7...): its mob textures follow 26.x layouts the built-in boxes
  don't have, the same open problem as above, plus its own 225 `optifine/cem` files.
* **Not implemented for Patrix:** its connected textures (26,455 files under `optifine/ctm`, 1,307
  `.properties`), random and natural textures (165 files), emissive overlays and its `_n`/`_s`
  normal and specular maps (21,690 files; PBR stays out of scope). Patrix loads as a plain 32× pack.

## Other known problems

* Kenzo's own `assets/textures` copies still sit above vanilla's files (the "mashup"); M62 separates
  the styles.
* Held-in-hand views of tools aren't in the contact sheets yet.
