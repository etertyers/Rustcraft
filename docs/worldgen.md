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
