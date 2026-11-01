# The Nether's generation (M24, M25, M49)

## Terrain
3D noise at the corners of 4 × 8 × 4 cells, blended between, with the floor and roof slides; the lava sea at Y 31; biomes from the Nether's own climate noises (see `worldgen/nether.rs`). A column's solid blocks are kept as bits, so every chunk that asks gets the same land.

## Features
Features run in a fixed order for the chunk and its eight neighbors (each replays the others' so a feature crossing a border is whole): blobs, ores, debris, springs, deltas, **basalt columns**, **basalt pillars**, glowstone, fires, vines, vegetation, fungi. A feature only writes the blocks that fall in the chunk being made.

### Basalt (M49)
- **Pillars** (modern `basalt_pillar`) are placed only in the soul sand valleys. Ten tries a chunk at a random column and a uniform height from 10 to 117; a try counts only where its block is open and the one above it isn't (it hangs from a ceiling); basalt goes straight down until the first thing in the way (the lava sea included, so never into it); each of the four sides gets a block with a tenth's chance at each step until it first misses; the foot flares, within 3 blocks, with the chance (10 − |dx|·|dz|) / 10 where there is air over ground. Mostly one block wide, as tall as the cavern.
- **Columns** (modern `small_basalt_columns` and `large_basalt_columns`) only in the basalt deltas: 4 small (height 1–4, reach 0–1) and 2 large (height 5–10, reach 2–3) at random columns of a chunk, a cluster on each floor layer of that column and one on the lava's surface. Every column in the reach finds the floor nearest the center's height and stacks up from it, shorter away from the center, stopping at anything and never on bedrock, magma, soul sand, nether bricks or nether wart; none is taller than 10.

### What was wrong (found by `--nether-map --seed 12345 --chunks 64`)
Before: soul sand valleys got 3 pillars a chunk, every try placed (it picked the ceilings of its column, not a random height), each 32 blocks tall on average (up to 81), and each step put a side block with a sixth's chance on every side whether the one before had missed or not, so the pillars were thick and common. The deltas got 5 column clusters a chunk, at the top floor of one column only. Nothing stood in the wrong biome (0 pillars outside the valleys, 0 clusters outside the deltas).
After: 0.16 pillars a valley chunk (median 26 tall, max 76; at most 10 by test), thin; 12.3 column clusters a delta chunk (4 + 2 on every floor layer), none taller than 10, and none outside their biome.

## Structures
Fortresses, bastions and the Nether's ruined portals (M25) are placed by `structure::nether` on the same placement cells `/locate` walks (`nether::starts_within`).
