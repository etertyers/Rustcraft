//! Ore, dirt and gravel veins with 1.0.0's counts, sizes and depths.
//!
//! A vein can cross a chunk border, so each chunk replays the veins of its
//! eight neighbors too and keeps only the blocks that land inside itself.

use std::f64::consts::PI;

use super::{
    rng::Rng,
    terrain::{DIRT, GRAVEL, STONE, Terrain},
};
use crate::{
    config::{CHUNK_SIZE, WORLD_HEIGHT},
    world::{
        block::BlockState,
        blocks,
        chunk::{Chunk, ChunkPos},
    },
};

const COAL_ORE: BlockState = BlockState::of(blocks::COAL_ORE);
const IRON_ORE: BlockState = BlockState::of(blocks::IRON_ORE);
const GOLD_ORE: BlockState = BlockState::of(blocks::GOLD_ORE);
const REDSTONE_ORE: BlockState = BlockState::of(blocks::REDSTONE_ORE);
const DIAMOND_ORE: BlockState = BlockState::of(blocks::DIAMOND_ORE);
const LAPIS_ORE: BlockState = BlockState::of(blocks::LAPIS_ORE);

/// How veins of one block are spread through a chunk.
struct VeinKind {
    block: BlockState,
    /// Veins attempted per chunk.
    count: u32,
    /// Blocks per vein (before overlaps and non-stone are skipped).
    size: u32,
    height: Height,
}

enum Height {
    /// Uniform in 0..max.
    Below(u32),
    /// Triangular around `center`, spread ±`spread` (lapis).
    Around { center: u32, spread: u32 },
}

/// 1.0.0's `BiomeDecorator.generateOres`, in its order.
const VEINS: [VeinKind; 8] = [
    VeinKind {
        block: DIRT,
        count: 20,
        size: 32,
        height: Height::Below(128),
    },
    VeinKind {
        block: GRAVEL,
        count: 10,
        size: 32,
        height: Height::Below(128),
    },
    VeinKind {
        block: COAL_ORE,
        count: 20,
        size: 16,
        height: Height::Below(128),
    },
    VeinKind {
        block: IRON_ORE,
        count: 20,
        size: 8,
        height: Height::Below(64),
    },
    VeinKind {
        block: GOLD_ORE,
        count: 2,
        size: 8,
        height: Height::Below(32),
    },
    VeinKind {
        block: REDSTONE_ORE,
        count: 8,
        size: 7,
        height: Height::Below(16),
    },
    VeinKind {
        block: DIAMOND_ORE,
        count: 1,
        size: 7,
        height: Height::Below(16),
    },
    VeinKind {
        block: LAPIS_ORE,
        count: 1,
        size: 6,
        height: Height::Around {
            center: 16,
            spread: 16,
        },
    },
];

const ORE_SALT: u64 = 0x04E5;

/// Adds veins from this chunk and its neighbors to `chunk`, replacing stone only.
pub fn populate(terrain: &Terrain, chunk: &mut Chunk) {
    let center = chunk.pos;
    for dz in -1..=1 {
        for dx in -1..=1 {
            let source = center.offset(dx, dz);
            let mut rng = Rng::for_chunk(terrain.seed(), source.x, source.z, ORE_SALT);
            for kind in &VEINS {
                for _ in 0..kind.count {
                    vein(&mut rng, source, kind, chunk);
                }
            }
        }
    }
}

/// One vein, following 1.0.0's `WorldGenMinable`: a line of overlapping
/// ellipsoids whose radius swells in the middle.
fn vein(rng: &mut Rng, source: ChunkPos, kind: &VeinKind, chunk: &mut Chunk) {
    let (min_x, min_z) = source.min_block();
    let x = (min_x + rng.below(CHUNK_SIZE as u32) as i32) as f64;
    let y = match kind.height {
        Height::Below(max) => rng.below(max),
        Height::Around { center, spread } => {
            rng.below(spread) + rng.below(spread) + center - spread
        }
    } as f64;
    let z = (min_z + rng.below(CHUNK_SIZE as u32) as i32) as f64;

    let size = kind.size as f64;
    let angle = rng.next_f64() * PI;
    let (sin, cos) = angle.sin_cos();
    let (x0, x1) = (x + sin * size / 8.0, x - sin * size / 8.0);
    let (z0, z1) = (z + cos * size / 8.0, z - cos * size / 8.0);
    let y0 = y + rng.below(3) as f64 - 2.0;
    let y1 = y + rng.below(3) as f64 - 2.0;

    let (chunk_x, chunk_z) = chunk.pos.min_block();
    for step in 0..=kind.size {
        let t = step as f64 / size;
        let (cx, cy, cz) = (x0 + (x1 - x0) * t, y0 + (y1 - y0) * t, z0 + (z1 - z0) * t);
        let spread = rng.next_f64() * size / 16.0;
        let radius = ((step as f64 * PI / size).sin() + 1.0) * spread + 1.0;
        let half = radius / 2.0;
        for bx in (cx - half).floor() as i32..=(cx + half).floor() as i32 {
            let lx = bx - chunk_x;
            if !(0..CHUNK_SIZE as i32).contains(&lx) {
                continue;
            }
            for by in (cy - half).floor() as i32..=(cy + half).floor() as i32 {
                if !(0..WORLD_HEIGHT as i32).contains(&by) {
                    continue;
                }
                for bz in (cz - half).floor() as i32..=(cz + half).floor() as i32 {
                    let lz = bz - chunk_z;
                    if !(0..CHUNK_SIZE as i32).contains(&lz) {
                        continue;
                    }
                    let d = |b: i32, c: f64| (b as f64 + 0.5 - c) / half;
                    let (dx, dy, dz) = (d(bx, cx), d(by, cy), d(bz, cz));
                    if dx * dx + dy * dy + dz * dz < 1.0
                        && chunk.get(lx as usize, by as usize, lz as usize) == STONE
                    {
                        chunk.set(lx as usize, by as usize, lz as usize, kind.block);
                    }
                }
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Highest y each ore reaches and how many blocks of it there are, over
    /// a patch of chunks.
    fn survey(seed: u64) -> std::collections::HashMap<BlockState, (usize, usize)> {
        let terrain = Terrain::new(seed);
        let mut found = std::collections::HashMap::new();
        for cz in -3..3 {
            for cx in -3..3 {
                let chunk = terrain.generate(ChunkPos::new(cx, cz));
                for y in 0..WORLD_HEIGHT {
                    for z in 0..16 {
                        for x in 0..16 {
                            let block = chunk.get(x, y, z);
                            let entry = found.entry(block).or_insert((0, 0));
                            entry.0 = entry.0.max(y);
                            entry.1 += 1;
                        }
                    }
                }
            }
        }
        found
    }

    #[test]
    fn ores_appear_only_at_their_1_0_0_depths() {
        let found = survey(21);
        // A vein can bulge a couple of blocks past its center's range.
        for (ore, limit) in [
            (DIAMOND_ORE, 19),
            (REDSTONE_ORE, 19),
            (GOLD_ORE, 35),
            (IRON_ORE, 67),
            (LAPIS_ORE, 35),
        ] {
            if let Some(&(top, _)) = found.get(&ore) {
                assert!(top <= limit, "{ore:?} found at y {top}");
            }
        }
        for ore in [COAL_ORE, IRON_ORE, REDSTONE_ORE] {
            assert!(found.contains_key(&ore), "no {ore:?} in 36 chunks");
        }
        // Coal is the most common ore, diamond the rarest.
        let count = |ore| found.get(&ore).map_or(0, |e| e.1);
        assert!(count(COAL_ORE) > count(IRON_ORE));
        assert!(count(IRON_ORE) > count(DIAMOND_ORE));
    }

    #[test]
    fn veins_continue_across_chunk_borders() {
        // Generating a chunk alone or as part of a patch gives the same blocks,
        // so a vein from a neighbor shows up whether or not the neighbor is loaded.
        let terrain = Terrain::new(4);
        let alone = terrain.generate(ChunkPos::new(1, 1));
        let _neighbor = terrain.generate(ChunkPos::new(0, 1));
        let again = terrain.generate(ChunkPos::new(1, 1));
        for y in 0..WORLD_HEIGHT {
            for z in 0..16 {
                for x in 0..16 {
                    assert_eq!(alone.get(x, y, z), again.get(x, y, z));
                }
            }
        }
        // And some ore in chunk (1, 1) came from a neighbor's vein.
        let mut from_neighbors = Chunk::new(ChunkPos::new(1, 1));
        terrain.fill(&mut from_neighbors);
        let own_only = {
            let mut c = from_neighbors.clone();
            let mut rng = Rng::for_chunk(terrain.seed(), 1, 1, ORE_SALT);
            for kind in &VEINS {
                for _ in 0..kind.count {
                    vein(&mut rng, ChunkPos::new(1, 1), kind, &mut c);
                }
            }
            c
        };
        populate(&terrain, &mut from_neighbors);
        let differs = (0..WORLD_HEIGHT).any(|y| {
            (0..256)
                .any(|i| from_neighbors.get(i % 16, y, i / 16) != own_only.get(i % 16, y, i / 16))
        });
        assert!(differs, "no neighbor vein reached into the chunk");
    }
}
