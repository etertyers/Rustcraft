# M67: exposed dirt and openings, Rustcraft before and after, next to vanilla

Seed 12345, the 1,000 × 1,000 blocks round 0, 0 (63 × 63 chunks for Rustcraft, 64 × 64 for Mojang's 1.21.11 server, which generated chunks −32 to 31 with `forceload`). All counts are per 1,000 × 1,000 blocks. Seeds don't give the same land, so these are statistics, not blocks. Rustcraft's rows come from `--grass-test`, vanilla's from `cargo run --release --example vanilla_dirt -- assets/vanilla/server/world/region`. Exposed dirt is a `dirt` block with nothing above it but air, leaves and plants.

| Row | Rustcraft before | Rustcraft after | Vanilla 1.21.11 |
|---|---|---|---|
| Exposed dirt, a noise cave or entrance cut the grass | 82,343 blocks, 4,182 patches (largest 2,901) | 0 | not separable |
| Exposed dirt, a carver cut it | 3,491 blocks, 345 patches (largest 113) | 659 blocks, 189 patches (largest 47) | not separable |
| Exposed dirt, a surface rule put dirt on top | 0 | 0 | not separable |
| **Exposed dirt, the finished chunks** | **84,950 blocks, 4,535 patches (largest 2,921)** | **1,291 blocks, 419 patches (largest 47)** | **1,780 blocks, 243 patches (largest 78)** |
| Biome ground (coarse dirt, podzol, rooted dirt), not a bug | 36,042 blocks, 649 patches | 41,034 blocks, 553 patches | 48,492 blocks, 70 patches (largest 16,835) |
| M53 openings (the density caves and carvers) | 1,142.6; 6.957% of land columns open | 1,142.6; 6.957% (unchanged: the density and carvers are the same) | not measurable from saved chunks |
| Pits (top 5+ blocks under all four columns 5 away) | 1,729 columns, 262 groups (the same: tops don't depend on the grass) | 1,729 columns, 262 groups; 0.173% of columns | 1,155 columns, 244 groups; 0.115% of columns |

Reading it: the exposed dirt from cave cuts went from 85,000 blocks to 1,300, under vanilla's 1,780 (27% under, not within 20%: the rest of vanilla's dirt comes from places this count can't separate, such as ore-feature dirt blobs, village paths and tree bases). Nothing in the code aims at the number (root §8). Pits (the stand-in for openings; its definition is mine, chosen because it needs only the finished blocks) are 262 groups against vanilla's 244 per km².

## Step 0, in words

- Rustcraft's order before: the noise step, with the surface layers hung from the heightmap inside the same loop; then the worm carvers; then ores, underground features, structures, trees and plants.
- Its surface rules found the floor from the column top (the heightmap), not from each air gap (`stone_depth`); there was no `above_preliminary_surface`; the carvers carved bottom up and had no top-material check, so every cave cut left dirt.
