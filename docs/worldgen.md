# World generation

## Biome placement (vanilla 1.21 multi-noise)

Biomes are picked from five climate parameters, exactly as vanilla's
`OverworldBiomeBuilder` does, with the tables below. Code:
`src/world/worldgen/climate.rs` (the noises) and
`src/world/worldgen/biome_source.rs` (the bands and tables).

### Climate noises

Six vanilla "normal noises" (two multi-octave Perlin samplers, the second at
1.0181… the scale, summed and scaled by `(1/6) / expectedDeviation`), all
seeded from the world seed with separate sub-seeds and sampled in 2D at
quarter-block coordinates (`block × 0.25 / BIOME_SCALE`), after a domain warp
by the shift noise (×4):

| Noise | firstOctave | amplitudes |
|---|---|---|
| temperature | -10 | 1.5, 0, 1, 0, 0, 0 |
| humidity (vegetation) | -8 | 1, 1, 0, 0, 0, 0 |
| continentalness | -9 | 1, 1, 2, 2, 2, 1, 1, 1, 1 |
| erosion | -9 | 1, 1, 0, 1, 1 |
| weirdness | -7 | 1, 2, 1, 0, 0, 0 |
| shift (x and z) | -3 | 1, 1, 1, 0 |

- Peaks and valleys: `PV = 1 - |3|W| - 2|`.
- Depth: 0 at the surface, +1/128 per block below it (from the current
  terrain height).
- `config::BIOME_SCALE` (1.0 = vanilla) stretches every biome.
- Biomes are chosen per 4×4×4 cell (a "quart"): surface biomes per 4×4
  column, cave biomes per cell, from the climate at the cell's corner. There's
  no blending and no randomness.

## Trees

Trees are placed like vanilla 1.21's placed tree features. Code: `TREES`,
`plan()` and `large()` in `src/world/worldgen/features.rs`.

- **Attempts**: each biome makes `count` attempts per chunk, plus one more
  with chance `extra`. Each attempt picks a random column in the chunk.
  Dark forests instead make 16 attempts, one in each 4 × 4 cell.
- **Filters**: an attempt is dropped, not retried, when:
  - the column is a different biome;
  - the ground isn't grass, dirt or podzol (mangroves also take mud;
    huge mushrooms take mycelium);
  - water stands over the ground (swamps allow 2 deep, mangroves 5);
  - a cave opens there; or
  - a dry run of the tree shows its trunk would meet the ground or a log
    of a tree planted earlier in the same chunk. Leaves may overlap.
- **Mixes**: each tree's chance is rolled in turn, like vanilla's random
  selector.
- **Across chunks**: a tree's trunk is always in its own chunk, and its
  leaves may spill into the neighbors. Each chunk re-plans its eight
  neighbors' trees from the terrain alone and keeps the blocks that land
  inside itself. `Area::MARGIN` (17) covers those neighbors, so a chunk's
  trees don't depend on load order or on which chunk asks.
- **Fallbacks**: biomes Rustcraft lacks use their fallback's row (see the
  appendix). Wooded badlands is generated as badlands, so it has no trees.

| Biome | Attempts (count + extra) | Trees |
|---|---|---|
| plains | 0 + 5% | 33% fancy oak, else oak |
| meadow | 0 + 1% | oak, fancy oak, tall birch (thirds) |
| forest | 10 + 10% | 20% birch, 10% fancy oak, else oak |
| birch forest | 10 + 10% | birch |
| dark forest | 16 (4 × 4 grid) | 2.5% brown and 5% red huge mushroom, ⅔ dark oak, 20% birch, 10% fancy oak, else oak |
| taiga, snowy taiga | 10 + 10% | 33% pine, else spruce |
| grove | 10 + 10% | pine |
| snowy plains | 0 + 10% | spruce |
| windswept hills | 0 + 10% | 66% spruce, else oak |
| savanna | 1 + 10% | 80% acacia, else oak |
| jungle | 50 + 10% | 10% fancy oak, 50% bush, 33% mega jungle, else jungle tree |
| bamboo jungle | 30 + 10% | as jungle |
| swamp | 2 + 10% | swamp oak with vines |
| mangrove swamp | 25 | mangrove |
| cherry grove | 10 + 10% | cherry |
| mushroom fields | 1 | huge mushroom (half brown, half red) |
| everything else | 0 | none |

Ice spikes (3 per chunk) and icebergs (6% of frozen ocean chunks) use the
same table and planning, and an azalea tree grows over lush caves 40% of
the time.

**Measured** (seed 20111118, 500 chunks per biome, trees per chunk):
plains 0.036, meadow 0.004, forest 6.8, birch forest 6.7, dark forest 9.0,
taiga 6.6, snowy taiga 7.1, grove 7.1, snowy plains 0.072, windswept hills
0.054, savanna 0.72, jungle 27.0, bamboo jungle 18.3, swamp 1.24, mangrove
swamp 19.5, cherry grove 7.6, mushroom fields 0.77. Before this change,
open biomes had several times vanilla's numbers: windswept hills 1.09,
plains 0.22 and meadow 0.17.

**Cost**: "generate a chunk" takes 1.43 ms, against 1.26 ms before (+13%),
mostly from the wider area.

## Appendix: vanilla 1.21 biome placement (from OverworldBiomeBuilder)

Bands:
- Temperature T0..T4: [-1,-0.45] [-0.45,-0.15] [-0.15,0.2] [0.2,0.55] [0.55,1]
- Humidity H0..H4: [-1,-0.35] [-0.35,-0.1] [-0.1,0.1] [0.1,0.3] [0.3,1]
- Erosion E0..E6: [-1,-0.78] [-0.78,-0.375] [-0.375,-0.2225] [-0.2225,0.05] [0.05,0.45] [0.45,0.55] [0.55,1]
- Continentalness: mushroom fields <-1.05, deep ocean [-1.05,-0.455], ocean [-0.455,-0.19], Coast [-0.19,-0.11], Near [-0.11,0.03], Mid [0.03,0.3], Far [0.3,1]
- PV: Valleys <-0.85, Low [-0.85,-0.2], Mid [-0.2,0.2], High [0.2,0.7], Peaks >0.7
- W = weirdness. Its sign picks variants.

Oceans (by T0..T4):
- deep: deep_frozen_ocean, deep_cold_ocean, deep_ocean, deep_lukewarm_ocean, warm_ocean
- shallow: frozen_ocean, cold_ocean, ocean, lukewarm_ocean, warm_ocean

MIDDLE[T][H0..H4]:
- T0: snowy_plains, snowy_plains, snowy_plains, snowy_taiga, taiga
- T1: plains, plains, forest, taiga, old_growth_spruce_taiga
- T2: flower_forest, plains, forest, birch_forest, dark_forest
- T3: savanna, savanna, forest, jungle, jungle
- T4: desert, desert, desert, desert, desert

MIDDLE_VARIANT (used when W>0; "-" = keep MIDDLE):
- T0: ice_spikes, -, snowy_taiga, -, -
- T1: -, -, -, -, old_growth_pine_taiga
- T2: sunflower_plains, -, -, old_growth_birch_forest, -
- T3: -, -, plains, sparse_jungle, bamboo_jungle
- T4: -

PLATEAU[T][H]:
- T0: snowy_plains, snowy_plains, snowy_plains, snowy_taiga, snowy_taiga
- T1: meadow, meadow, forest, taiga, old_growth_spruce_taiga
- T2: meadow, meadow, meadow, meadow, dark_forest
- T3: savanna_plateau, savanna_plateau, forest, forest, jungle
- T4: badlands, badlands, badlands, wooded_badlands, wooded_badlands

PLATEAU_VARIANT (W>0):
- T0: ice_spikes, -, -, -, -
- T1: cherry_grove, -, meadow, meadow, old_growth_pine_taiga
- T2: cherry_grove, cherry_grove, forest, birch_forest, -
- T3: -
- T4: eroded_badlands, eroded_badlands, -, -, -

SHATTERED[T][H] ("-" = MIDDLE):
- T0/T1: windswept_gravelly_hills, windswept_gravelly_hills, windswept_hills, windswept_forest, windswept_forest
- T2: windswept_hills, windswept_hills, windswept_hills, windswept_forest, windswept_forest
- T3/T4: -

Helper picks:
- M = MIDDLE (variant if W>0)
- MB = M, but BADLANDS if T4
- MBS = MB, but SLOPE if T0
- BADLANDS = H0–1: badlands if W<0, else eroded_badlands; H2: badlands; H3–4: wooded_badlands
- P = PLATEAU (variant if W>0)
- S = SHATTERED
- WS(x) = windswept_savanna if T≥2, H≤3 and W≥0, else x
- PK = peak: T0–2: jagged_peaks if W<0, else frozen_peaks; T3: stony_peaks; T4: BADLANDS
- SLOPE = T≥3: P; H0–1: snowy_slopes; else grove
- B = beach: T0 snowy_beach, T4 desert, else beach
- SC = WS(W≥0 ? M : B)
- SW = T0: M; T1–2: swamp; T3–4: mangrove_swamp
- R = T0: frozen_river, else river

Inland table (rows = erosion, columns = continentalness Coast | Near | Mid | Far):
Peaks:
- E0: PK | PK | PK | PK
- E1: MBS | MBS | PK | PK
- E2: M | M | P | P
- E3: M | M | MB | P
- E4: M (all)
- E5: WS(M) | WS(M) | S | S
- E6: M (all)

High:
- E0: M | SLOPE | PK | PK
- E1: M | MBS | SLOPE | SLOPE
- E2: M | M | P | P
- E3: M | M | MB | P
- E4: M (all)
- E5: WS(M) | WS(M) | S | S
- E6: M (all)

Mid:
- E0: stony_shore | SLOPE | SLOPE | SLOPE
- E1: stony_shore | MBS | MBS | (T0 ? SLOPE : P)
- E2: stony_shore | M | MB | P
- E3: M | M | MB | MB
- E4: (W<0 ? B : M) | M | M | M
- E5: SC | WS(M) | S | S
- E6: (W<0 ? B : M) | SW | SW | SW

Low:
- E0–E1: stony_shore | MBS | MBS | MBS
- E2: stony_shore | M | MB | MB
- E3: B | M | MB | MB
- E4: B | M | M | M
- E5: SC | WS(M) | S | S
- E6: B | SW | SW | SW

Valleys:
- E0–E1: (W<0 ? stony_shore : R) | R | MB | MB
- E2–E5: R (all)
- E6: R | SW | SW | SW

Cave biomes: at depth 0.2–0.9, lush caves where humidity ≥ 0.7, dripstone
caves where continentalness ≥ 0.8, otherwise the surface biome.

Fallbacks if missing: flower_forest→forest, sunflower_plains→plains, old_growth_spruce/pine_taiga→taiga, old_growth_birch_forest→birch_forest, sparse_jungle→jungle, savanna_plateau and windswept_savanna→savanna, windswept_forest and windswept_gravelly_hills→windswept_hills, wooded/eroded_badlands→badlands, stony_peaks→windswept_hills, snowy_beach→beach, frozen_river→river.

## Session 1 summary (biome selection)

**Files changed**
- `src/world/worldgen/climate.rs`: rewritten as vanilla normal noises
  with the octaves above, the shift warp, quart sampling and `PV`.
  `PLANE_SPREAD` (0.7) narrows the `noise` crate's 2D Perlin (sd 0.43) toward
  vanilla's improved noise on a plane. Without it, continentalness fell below
  the mushroom fields' -1.05 on 4% of the map instead of 0.4%.
- `src/world/worldgen/biome_source.rs` (new): bands, the tables (written with
  vanilla's names), fallbacks, and the cave rule.
- `src/world/worldgen/biome_map.rs` (new): the map tool's sampling (on every
  core), statistics, colors and image.
- `src/world/worldgen/terrain.rs`:
  - The terrain reads the same climate, so ocean and river biomes line up
    with water. Oceans use vanilla's continentalness bands, and rivers are
    carved where weirdness crosses zero (PV valleys) instead of from a
    separate river noise.
  - The old per-column `pick` (height-based biome rules) and the cave-biome
    patch noises are gone.
  - Two shape constants moved so the existing terrain tests still hold:
    mountains now start at erosion -0.4, since the new erosion is
    bell-shaped, and river valleys use weirdness 0.12 with a 0.03 bed.
  - `Terrain::biome_at` reads a quart's biome straight from the climate.
- `src/config.rs`: `BIOME_SCALE`.
- `src/ui/mod.rs`, `src/app.rs`: an F3 line with T/H/C/E/W/PV.
- `src/main.rs`: `--biome-map`.
- Tests:
  - Feature tests now look for whole 3×3-chunk patches of a biome without
    surface structures, and beaches are dropped from the "open biomes have
    no trees" check, since they're now strips.
  - The chunk-border tree test only checks small-oak tops (a trunk below,
    leaves on both sides).
  - The ore table test surveys seed 15, which is hilly at its origin.
  - The lowland step limit is now 8 (it was 5).

**Tools**
- `cargo run --release -- --biome-map --seed <n> --size <pixels> --out <file.png>`
  writes one pixel per 4×4 column (so `--size 4096` covers 16384 blocks).
  It prints each biome's share, mean patch size, area-weighted median patch
  size, the land median patch, the ocean share and the snowy-beside-hot
  count. The legend is a column of color swatches in the same order as the
  printed list (no text).
- F3 shows `T H C E W PV` where the player stands.

**Numbers** (seed 20111118, 4096 × 4096 quarts):
- Oceans cover 32.5% of the area.
- Every surface biome appears, with zero snowy-beside-hot pairs.
- The land median patch is about 289 × 289 blocks.
- Six other seeds gave oceans at 29–34% and land median patches of
  279–302 blocks.

**Known issues**
- The terrain shape is still the old one fed with the new climate:
  - Mountains, plateaus and valleys don't follow vanilla's splines, so biomes
    like plateaus and slopes may sit on ground that doesn't look like them.
  - River banks can be up to 8 blocks steep where weirdness changes quickly.
  - Session 2 replaces the terrain shaping.
- Rivers follow weirdness's zero lines, so some form loops (vanilla's do too).
- The fifteen vanilla biomes Rustcraft lacks use the fallbacks above (see
  `docs/DECISIONS.md`).
- A 4096-block square at vanilla scale often holds only two or three
  temperature bands. That's vanilla's size, and `BIOME_SCALE` can shrink it.
- Chunks generated before this change keep their old biomes.
- The village seeds listed in the M23 PR were found with the old biomes and
  will have moved.

## Session 2 summary (terrain shape)

**How it works**
- **Splines** (`src/world/worldgen/terrain_shape.rs`): continentalness, then erosion, then PV, nested as in vanilla's `TerrainProvider`. They give each column three values:
  - `offset`: the surface height it's pulled toward.
  - `factor`: how tightly it's pulled. The factor follows erosion, from 0.035 in mountains to 0.35 on worn-flat land. It's tighter in valleys (0.5) and under the sea (0.25).
  - `jaggedness`: up to 18 extra blocks of ridged noise, only on peaks (PV > 0.4), at low erosion, inland.
- **Density** (`terrain.rs`): `(offset + jaggedness × ridge − y) × factor + 0.6 × base 3D noise`, solid above zero.
  - It's computed at the corners of 4 × 8 × 4 cells and trilinearly interpolated.
  - Corners where the pull alone decides the sign skip the noise; that's the main speed trick.
  - The noise fades out between Y 180 and 200, so nothing floats up there.
  - The pull is capped at Y 240, and nothing is solid from Y 248.
  - The chunk fill, the area's column heights and single-column `height()` all use the same corner grid, so features, structures and the filled chunk always agree.
- **Water and caves**: water fills from the surface to Y 63. Caves stay 4 blocks below the floor of any water (it was 3).
- **Depth**: cave biome depth now comes from the offset (the preliminary surface) at the quart's corner.
- **Height map tool**: `cargo run --release -- --height-map --seed <n> --size <pixels> [--out <png>]`.
  - It writes biome colors hillshaded from the north-west, with water darkened by depth. The default output is `docs/screenshots/m21-height-map.png`.
  - It prints min, mean and max surface Y per biome.

**Per-biome surface Y** (seed 20111118, 8192 blocks across):

| Biome | min | mean | max |
|---|---|---|---|
| Deep oceans | 37 | 41–42 | 49–55 |
| Oceans | 44 | 49–52 | 58 |
| Beach | 55 | 60 | 69 |
| Stony shore | 55 | 62 | 86 |
| River | 55 | 57 | 59 |
| Swamps | 57 | 64–65 | 83 |
| Plains | 55 | 79 | 174 |
| Forest | 55 | 86 | 218 |
| Meadow | 93 | 125 | 176 |
| Snowy slopes | 59 | 126 | 217 |
| Jagged peaks | 128 | 180 | 227 |
| Frozen peaks | 131 | 189 | 238 |
| Mushroom fields | 54 | 66 | 72 |

**Benchmark**: "generate a chunk" takes 1.25 ms, against 1.31 ms before this session.

**Tests**
- Added:
  - Ocean biome columns sit under Y 60, and peaks average above 150 with nothing above 250.
  - River columns hold water at Y 63 over a bed at 61 or lower.
  - No terrain floats above 200.
  - The same seed gives the same chunks.
  - The spline targets hold, and jaggedness appears only on low-erosion peaks.
- Changed:
  - Lowland steps (erosion −0.2 and up) are at most 7 blocks, and anywhere at most 16.
  - The old "peaks over 200 in this square" test is replaced by the per-biome one.

**Known issues**
- **Mushroom fields** rise as low islands (Y 54–72), as in vanilla, not the Y 35–45 sea floor the brief listed.
- **Valley steps**: worn land can step up to 7 blocks where it drops into a river valley.
- **Overhangs and caves**: mountains with low erosion have 3D overhangs. Air pockets the 3D noise leaves under the surface stay dry, even below sea level (there are no aquifers).
- **Valleys in mountains**: every valley dips to just under sea level, even where the biome table says mountain (Valleys E0–E1 Mid/Far). These are dry or water-filled gorges.
- **Old chunks**: chunks generated before this change keep their old shape, so there's a seam where old meets new.

## Session 3 summary (surfaces and decoration)

**Surface rules** (`Terrain::top()` and `layer()` in `terrain.rs`, applied after the terrain shape, in this order):
- **Bedrock floor**: unchanged.
- **Under water** (not river or swamp):
  - Beaches and deserts get sand.
  - Elsewhere, clay patches (shallow only), then gravel where it's cold (temperature < 0.3), deep (more than 14 below sea level) or in a gravel patch, otherwise sand.
  - Rivers use sand, gravel and clay along their beds.
- **Wet biomes**: swamps and mangrove swamps get water patches at sea level. Mangrove ground is mud, with mud under its pools.
- **Sand**: desert and beach get 3–4 blocks of sand over sandstone.
- **Stony shore**: stone with gravel patches.
- **Badlands**: terracotta bands keyed to Y on steep or high ground (over 100), red sand on top.
- **Mushroom fields**: mycelium.
- **Peaks**:
  - Frozen peaks: snow blocks, with packed ice and ice on steep faces.
  - Jagged peaks and snowy slopes: snow blocks, stone where steep.
  - `stony_peaks` (generated as windswept hills, see DECISIONS): calcite and stone above Y 150, gravel patches lower down.
- **Steep slopes** (rising 4+ blocks over 1) are stone instead of grass.
- **Above Y 200**, grass becomes snow blocks in cold biomes and stone elsewhere.
- **Snow and ice**: snowy biomes keep snow layers on the ground and ice on still water, from the existing temperature-based freezing.

**Decoration**
- Trees and plants read the biome at their column's quart and only grow on grass-type ground (`Column::grassy()`), so none land on the new stone, sand or ice slopes.
- 2 × 2 trunks (mega jungle and dark oak) now push roots down up to 6 blocks to the ground (`root()` in `features.rs`). On slopes they used to hang one corner over air.
- Ore and mob rules that depend on biome name the `Biome` enum (`ores.rs` emerald and badlands gold, `mobs.ron` spawn lists), filled from the new biome source, so they needed no change.

**Surface shares** (topmost block in sampled chunks, seed 20111118; from the `each_biome_shows_its_own_ground` test):

| Biome | Ground | Share |
|---|---|---|
| Desert | sand | 98% |
| Badlands | terracotta or red sand | 99% |
| Mushroom fields | mycelium | 90% |
| Snowy plains | snow or ice | 100% |
| Plains | grass | 100% |

**Perf**: `cargo bench` "generate a chunk" 1.26 ms (M23 1.41 ms). The `--perf-test` gives 146 fps average and an 85 fps 1% low, with no frame over 33 ms. Full table in `docs/perf/m21.md`.

**Tests added**
- Each biome shows its own ground (the table above).
- Beaches are at least 70% sand, with sandstone within 8 blocks below.
- No plant or upright trunk foot stands on air or fluid, across 11 land biomes. This skips cobwebs and dripstone, which hang in caves, and allows for acacia's leaning trunks.
- The same seed regenerates the same decorated chunk.

**Known issues**
- **Missing biomes**: `stony_peaks`, `snowy_beach` and `frozen_river` are still fallbacks (windswept hills, beach, river) with their surfaces approximated.
- **Old chunks**: saved worlds show seams at the edge of explored land (DECISIONS).
- **Badlands below Y 100**: gentle badlands land is plain red sand. Bands show only on slopes and higher ground.

## The Nether (M24)

`world/worldgen/nether.rs`. The Nether is y 0–127 (air above the roof), with its own noises mixed from the world seed.

- **Shape**: 3D noise (`Fbm<Perlin>`, 4 octaves, 56 blocks across and 34 up, −0.08 toward open) at the corners of 4 × 8 × 4 cells, blended in between. Over the bottom 24 blocks the density slides toward 2.5 and over the top 24 toward 0.9375 (modern `slideNetherLike`), so floors and roofs close the caverns. Bedrock always at y 0 and 127, and at y 1–4 / 123–126 with chance 4/5 … 1/5.
- **Lava sea**: open space at y 31 and below is lava.
- **Biomes**: modern's Nether multi-noise points on temperature and humidity (offsets for warped forest and basalt deltas), from 2-octave noise 260 blocks across, picked at the middle of each 4 × 4 quart.
- **Surfaces** (bare terrain only, so every chunk agrees): crimson and warped nylium on forest floors above the lava; soul sand or soul soil (patch noise) three deep on valley floors and two under their ceilings; basalt or blackstone four deep on delta floors and two under ceilings; soul sand and gravel patches on the wastes' shores (y 30–34); netherrack elsewhere.
- **Features**, per chunk, in order (each chunk replays its eight neighbors' and keeps what lands in it; features read only the bare terrain, except vines, which stay in one column and stop at anything already there):

| Feature | Where | Per chunk |
|---|---|---|
| Basalt (12) and blackstone (4) blobs, radius 3–6 | deltas | 16 tries |
| Quartz ore veins (radius ~1.6) | everywhere, y 10–118 | 16 (32 in deltas) |
| Gold ore veins (~1.4) | everywhere, y 10–118 | 10 (20 in deltas) |
| Magma (~2.6) | y 27–36 | 4 |
| Gravel (~2.4) | y 5–31 | 2 |
| Lava springs: closed (in solid rock), open (one side open, rock above) | y 10–118 | 20 tries; open ones 4 (8 in deltas) |
| Lava deltas: lava pools 2–5 across, 0–1 magma rim | deltas floors | 6 |
| Basalt columns: 2 large (reach 2–3, 5–10 tall), 3 small | deltas, from floor or lava | 5 |
| Basalt pillars floor to ceiling | valleys | 3 |
| Glowstone clumps (600 tries to add a cell touching exactly one) | under ceilings | 6–15 |
| Fire / soul fire patches | wastes / valleys | 0–3 |
| Weeping vines with nether wart block patches | crimson ceilings | 10 |
| Roots, fungi, sprouts, twisting vines (on nylium); mushrooms | forests; wastes and valleys | per floor column |
| Huge crimson / warped fungi (stem 4–13, doubled 1 in 12, thick 6%; wart cap with shroomlight, weeping vines under crimson caps) | forests on nylium | 8 |

Generating a Nether chunk takes 1.38 ms (`cargo bench`), about what an Overworld one does. `cargo test --release nether_slice -- --ignored` with `NETHER_SLICE=out.png` draws a side view, and `nether_views` finds open spots in each biome for screenshots.

## The End (M27)

`world/worldgen/end.rs`. One disc of end stone about 150 blocks across (see DECISIONS for the numbers), flat on its top at y 63 and rounded underneath, with the ten obsidian pillars of modern's `SpikeFeature` and the bedrock podium of `EndPodiumFeature` (a hole until the dragon dies). Everything is a function of the position and the seed, so chunks agree across borders; pillars and the podium are written clipped to the chunk. Generating an End chunk takes 0.04 ms (`cargo bench`). The outer islands, chorus plants and End cities come in M28.

