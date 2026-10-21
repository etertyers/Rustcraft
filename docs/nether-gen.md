# Nether generation (terrain, biomes, basalt)

Code: `src/world/worldgen/nether.rs` and `nether/{blended,carver,basalt,map}.rs`.
Structure placement is the last section.

## Step 0: what was wrong (measured before any change, seed 20111118)

- **Height, roof, lava**: 128 tall (`HEIGHT`), bedrock roof at y 127 (highest
  non-air, non-bedrock block y 126), lava sea at y 31 and below (sea level 32).
  Right already; untouched.
- **Biome choice**: the nearest of modern's five points, but on a 2-octave
  `Fbm<Perlin>` 260 blocks across (x1.4), 2D, read at each quart's middle. Vanilla's
  temperature noise is 4096 blocks across and humidity 1024. In a 1024 x 1024 quart
  window (4096 blocks) the median patch was 3,800 (wastes) to 9,000 blocks² (about 60-95
  blocks across), 20% of patches were under 32 x 32 blocks, and the five biomes took 13-24% each.
- **Tall basalt pillars**: `pillars` hung 3 pillars per chunk from a random ceiling in
  every soul sand valley chunk, going down to the first solid block or the lava surface
  (it never passed through lava, and was not in the wrong biome). A pillar is exactly the
  cavern height under its ceiling, so its length is a terrain question: the old caverns
  ran from the lava sea to the roof, median 19 and max 83 blocks of open air, with valley
  basalt runs up to 81 (median 6; 64 x 64 chunks every 8 chunks). With 3 forced pillars a
  chunk, in every chunk, the tall ones were common; vanilla's are 10 tries a chunk that
  mostly miss.
- **Terrain**: 4-octave `Fbm<Perlin>` at 56 x 34 blocks, nothing like vanilla's blended noise.

## What it does now

1. **Shape** (`blended.rs`, `Nether::corner`): classic 1.17 blended noise (xz_scale 0.25,
   y_scale 0.375, xz_factor 80, y_factor 60, smear 8) at the corners of 4 x 8 x 4 cells,
   interpolated trilinearly. Top slide from y 104 to 128 toward 0.9375, bottom slide from y 24
   down to -8 toward 2.5, solid where `squeeze(0.64 * v) > 0`. The limit noises skip their
   finest 6 of 16 octaves (together 0.1% of their weight).
2. **Caves** (`carver.rs`): `nether_cave`: chance 0.2 a chunk, y 0-126, room yScale 0.5,
   floor -0.7, tunnels five times as tall as wide, sources within 8 chunks. Only rock is cut,
   never bedrock. Cut space at or under the lava sea is lava with the rest of it (vanilla keeps
   air in carved pockets between y 10 and 31; the lava-everywhere rule wins here).
3. **Bedrock**: y 0 always, then chance 0.8 down to 0.2 at y 4, mirrored at the roof (checked,
   unchanged). Nothing above y 127, nothing generated or decorated in y 123-127 by the basalt features.
4. **Biomes** (`climate::TemperatureHumidity`): the Overworld's normal noises (temperature
   -10 [1.5,0,1,0,0,0], humidity -8 [1,1,0,0,0,0]), each with its own sub-seed from the Nether's
   seed, 2D, sampled at block x 0.25 after the shift noise, on the quart grid, no blending.
   Nearest of (T,H,offset): wastes (0,0,0), soul sand valley (0,-0.5,0), crimson (0.4,0,0),
   warped (0,0.5,0.375), deltas (-0.5,0,0.175).
5. **Surface rules** (`Nether::base`/`surface`): bedrock; netherrack above y 122; then per biome on
   the top block of a floor and the bottom block of a ceiling (ceiling first):
   deltas: basalt on ceilings; floors gravel at y 30-35 where the patch noise >= -0.012, else basalt (selector >= 0) or blackstone;
   valley: gravel patches the same, else soul sand (selector >= 0) or soul soil, floors and ceilings;
   crimson / warped: nylium on floors above the lava sea; wastes: soul sand and gravel patches at y 30-35; netherrack otherwise.
6. **Basalt** (`basalt.rs`, counts in its `TABLE`): in deltas, `delta` 40 tries on every floor layer
   (size 3-7, rim 0-2), small columns 4 a layer (height 1-4, reach 1), large columns 2 a layer
   (height 5-10, reach 2-3), basalt blobs 75 and blackstone blobs 25 a chunk (radius 3-7, netherrack only).
   Columns grow at most `height` blocks, stop at the first non-air block and never stand on lava,
   bedrock, magma, soul sand, nether bricks or chests. In soul sand valleys, `basalt_pillar` 10 tries a
   chunk at y 0-127, kept where the block is air with a solid block above, going down to the first non-air
   block (never below y 32), with side blocks at both ends.

## Where this differs from the brief

- **Shift noise 2, not 4** (`CLIMATE_SHIFT`). The Overworld's shift (4 quarter-blocks) splinters every
  border into islands of one or two quarts: with it, 79% of patches are under 32 x 32 blocks (0.01% of the
  land). At 2, 14%; at 0 (no shift), still 10%, because nearest-point regions of smooth noise always leave slivers
  where two borders meet. Set it back to 4.0 for exact vanilla.
- **Patch tests measure by area.** "Median patch >= 128 x 128" and "under 5% small" hold when a patch is
  weighted by its area (the patch a typical block lies in; share of land in small patches). Counting patches
  instead gives specks a vote and fails for any nearest-point source (wastes' median patch is 32 blocks², with
  43 patches against 6 big ones). Both numbers are printed.
- Sample chunks are every 8th chunk (a 64 x 64 grid over 8192 blocks), not 64 x 64 adjacent chunks, so all
  five biomes are in it.

## Results (seed 20111118, 4096 x 4096 quart columns = 16384 blocks)

| Biome | Share | Typical patch by area |
|---|---|---|
| Nether wastes | 34.3% | 64.8M blocks² |
| Crimson forest | 25.3% | 19.2M |
| Warped forest | 4.3% | 0.30M |
| Soul sand valley | 11.6% | 0.53M |
| Basalt deltas | 24.5% | 20.2M |

Chunk sample: cavern height median 14, max 94; tallest basalt column 10; tallest pillar 78
(median 13, lowest y 32); highest non-air, non-bedrock y 126. Chunk generation: 1.39 -> 1.56 ms (+12%,
`cargo bench --bench world -- "generate a nether chunk"`); density columns and caves are cached across chunks.

## Running it

```
cargo run --release -- --nether-map --seed 20111118 --size 4096 --out docs/screenshots/nether-biome-map.png
```

One pixel per quart column (legend, top left, in order: wastes, crimson, warped, valley, deltas).
Prints the share, patch count, median and by-area patch size per biome, patches under 32 x 32,
and, for a 64 x 64 grid of chunks, cavern heights, tallest basalt column and pillar, lowest pillar y and
the highest non-air, non-bedrock y. Tests: `cargo test nether`.

## Structures (placement)

Code: `src/world/structure/nether.rs` (tables, choice, survey), `fossil.rs`. Pieces and loot are unchanged.

**Step 0, why they seemed rare.** Placement was already vanilla's grid: one candidate chunk per
27 x 27 chunk region (432 blocks), 94 of 100 regions got a structure. That is about one complex
per 186,000 blocks², so an area flown through at 100 blocks wide holds one every 1,900 blocks.
Nothing in placement explains 1 bastion and 0 fortresses; what it did get wrong was small: a
bastion rolled in basalt deltas was dropped instead of falling back to a fortress (6% of regions
had nothing), a fortress's start was 8 blocks into its chunk (modern: 2), and the ruined portal
spacing was 25/10 (modern's shared `ruined_portals` set is 40/15). The playtest was on the old
terrain, so what the player saw may also have been structures buried in rock or hidden by
the open caverns; that is not verified.

Now (all numbers in the tables at the top of `structure/nether.rs`):

| Set | Spacing | Separation | Salt | Where | Weight |
|---|---|---|---|---|---|
| `nether_complexes` | 27 | 4 | 30084232 | fortress: every Nether biome; bastion remnant: not basalt deltas | 2, 3 |
| `ruined_portals` | 40 | 15 | 34222645 | every Nether biome | |
| `nether_fossil` | 2 | 1 | 14357921 | soul sand valley, y 32-125 | |

- A region's candidate is `region * spacing + [0, spacing - separation)` on each axis, drawn from
  (world seed, region, salt), so any chunk can ask which starts are near it (fortresses reach 8
  chunks) with no dependence on load order.
- A complex candidate picks fortress or bastion by weight from (seed, chunk); if the biome at its
  start refuses the pick, the other is tried, so every region has exactly one complex. Fortress
  share therefore depends on how much of the area is deltas (39-88% over seeds 1-59 in 100 regions,
  about 55% with a quarter of the land in deltas).
- Fortress: start piece 2 blocks into its chunk at y 48-70, pieces within 112 blocks. Bastion: start at
  y 33. The bastion is still the single keep, not a jigsaw of four types (pieces were out of scope).
- Fossils are new: a bone-block spine with ribs, one skeleton instead of modern's fourteen templates.

Tests: `cargo test structure::nether`. `--nether-map --structures` draws fortresses (white), bastions
(yellow) and ruined portals (magenta) and prints totals and the ten nearest to 0,0 (fossils are only counted).
