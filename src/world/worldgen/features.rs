//! Trees and plants, placed with 1.0.0's shapes and per-biome amounts.
//!
//! Trees can spill leaves into the next chunk, so each chunk replays the
//! trees of its eight neighbors and keeps the blocks that land inside itself.
//! Placement only asks the pure terrain functions, never another chunk, so
//! the result doesn't depend on which chunks happen to be loaded.

use super::{
    rng::Rng,
    terrain::{Column, DIRT, GRASS, SAND, Terrain},
};
use crate::{
    config::{CHUNK_SIZE, SEA_LEVEL, WORLD_HEIGHT},
    world::{
        biome::Biome,
        block::BlockState,
        chunk::{Chunk, ChunkPos},
    },
};

const OAK_LOG: BlockState = BlockState::new(17, 0);
const BIRCH_LOG: BlockState = BlockState::new(17, 2);
const OAK_LEAVES: BlockState = BlockState::new(18, 0);
const BIRCH_LEAVES: BlockState = BlockState::new(18, 2);
const TALL_GRASS: BlockState = BlockState::new(31, 1);
const DEAD_BUSH: BlockState = BlockState::new(32, 0);
const DANDELION: BlockState = BlockState::new(37, 0);
const ROSE: BlockState = BlockState::new(38, 0);
const CACTUS: BlockState = BlockState::new(81, 0);
const REEDS: BlockState = BlockState::new(83, 0);

const TREE_SALT: u64 = 0x7EE;
const PLANT_SALT: u64 = 0x91A;

/// Tree attempts per chunk; forests plant all of them, like 1.0.0's 10.
const TREE_ATTEMPTS: u32 = 10;

pub fn populate(terrain: &Terrain, chunk: &mut Chunk) {
    let center = chunk.pos;
    for dz in -1..=1 {
        for dx in -1..=1 {
            trees(terrain, center.offset(dx, dz), chunk);
        }
    }
    plants(terrain, chunk);
}

/// Writes into `chunk` only, ignoring positions in other chunks.
struct ChunkWriter<'a> {
    chunk: &'a mut Chunk,
    min_x: i32,
    min_z: i32,
}

impl<'a> ChunkWriter<'a> {
    fn new(chunk: &'a mut Chunk) -> Self {
        let (min_x, min_z) = chunk.pos.min_block();
        Self {
            chunk,
            min_x,
            min_z,
        }
    }

    fn local(&self, x: i32, y: i32, z: i32) -> Option<(usize, usize, usize)> {
        let (lx, lz) = (x - self.min_x, z - self.min_z);
        let size = CHUNK_SIZE as i32;
        ((0..size).contains(&lx)
            && (0..size).contains(&lz)
            && (0..WORLD_HEIGHT as i32).contains(&y))
        .then_some((lx as usize, y as usize, lz as usize))
    }

    fn get(&self, x: i32, y: i32, z: i32) -> Option<BlockState> {
        let (lx, ly, lz) = self.local(x, y, z)?;
        Some(self.chunk.get(lx, ly, lz))
    }

    /// Sets the block if it's in this chunk and `replace` accepts what's there.
    fn set_if(
        &mut self,
        x: i32,
        y: i32,
        z: i32,
        block: BlockState,
        replace: fn(BlockState) -> bool,
    ) {
        if let Some((lx, ly, lz)) = self.local(x, y, z)
            && replace(self.chunk.get(lx, ly, lz))
        {
            self.chunk.set(lx, ly, lz, block);
        }
    }
}

/// Air and small plants, which leaves and plants may grow into.
fn is_replaceable(block: BlockState) -> bool {
    block.is_air() || matches!(block.id(), 31 | 37 | 38)
}

/// Logs grow through leaves too.
fn log_replaceable(block: BlockState) -> bool {
    is_replaceable(block) || block.id() == 18
}

fn always(_: BlockState) -> bool {
    true
}

/// A grass column (not undercut by a cave) that something can grow on.
fn grassy(terrain: &Terrain, x: i32, z: i32) -> Option<Column> {
    let column = terrain.column(x, z);
    (column.top == GRASS && !terrain.is_cave(x, column.height, z)).then_some(column)
}

/// Replays the trees of chunk `source`, writing the parts inside `chunk`.
fn trees(terrain: &Terrain, source: ChunkPos, chunk: &mut Chunk) {
    let mut rng = Rng::for_chunk(terrain.seed(), source.x, source.z, TREE_SALT);
    let (min_x, min_z) = source.min_block();
    let mut writer = ChunkWriter::new(chunk);
    for _ in 0..TREE_ATTEMPTS {
        // One draw per attempt keeps every later attempt's numbers the same
        // whether or not this one grows.
        let mut tree_rng = Rng::new(rng.next_u64());
        let x = min_x + tree_rng.below(CHUNK_SIZE as u32) as i32;
        let z = min_z + tree_rng.below(CHUNK_SIZE as u32) as i32;
        let Some(column) = grassy(terrain, x, z) else {
            continue;
        };
        if column.biome != Biome::Forest {
            continue;
        }
        // 1.0.0 forests: one tree in five is a birch.
        let (log, leaves, height) = if tree_rng.below(5) == 0 {
            (BIRCH_LOG, BIRCH_LEAVES, 5 + tree_rng.below(3) as i32)
        } else {
            (OAK_LOG, OAK_LEAVES, 4 + tree_rng.below(3) as i32)
        };
        let base = column.height + 1;
        if base + height + 1 >= WORLD_HEIGHT as i32 {
            continue;
        }
        tree(&mut writer, &mut tree_rng, x, base, z, height, log, leaves);
    }
}

/// 1.0.0's small tree (`WorldGenTrees`): a trunk with four layers of leaves,
/// two wide ones and two narrow ones, with random corners.
#[allow(clippy::too_many_arguments)]
fn tree(
    writer: &mut ChunkWriter,
    rng: &mut Rng,
    x: i32,
    base: i32,
    z: i32,
    height: i32,
    log: BlockState,
    leaves: BlockState,
) {
    let top = base + height;
    for y in top - 3..=top {
        let layer = y - top;
        let radius = 1 - layer / 2;
        for dx in -radius..=radius {
            for dz in -radius..=radius {
                let corner = dx.abs() == radius && dz.abs() == radius;
                if !corner || (rng.below(2) != 0 && layer != 0) {
                    writer.set_if(x + dx, y, z + dz, leaves, is_replaceable);
                }
            }
        }
    }
    for y in base..top {
        writer.set_if(x, y, z, log, log_replaceable);
    }
    writer.set_if(x, base - 1, z, DIRT, always);
}

/// Plants stay inside their own chunk, so only this chunk's are placed.
fn plants(terrain: &Terrain, chunk: &mut Chunk) {
    let pos = chunk.pos;
    let mut rng = Rng::for_chunk(terrain.seed(), pos.x, pos.z, PLANT_SALT);
    let (min_x, min_z) = pos.min_block();
    let mut writer = ChunkWriter::new(chunk);
    let pick = |rng: &mut Rng| {
        (
            min_x + rng.below(CHUNK_SIZE as u32) as i32,
            min_z + rng.below(CHUNK_SIZE as u32) as i32,
        )
    };

    // Tall grass: thick on plains, sparse in forests.
    for _ in 0..32 {
        let (x, z) = pick(&mut rng);
        let roll = rng.next_f64();
        if let Some(column) = grassy(terrain, x, z) {
            let chance = match column.biome {
                Biome::Plains => 0.9,
                Biome::Forest => 0.3,
                _ => 0.0,
            };
            if roll < chance {
                writer.set_if(x, column.height + 1, z, TALL_GRASS, |b| b.is_air());
            }
        }
    }

    // Flowers: dandelions, and one patch in four is roses.
    for _ in 0..4 {
        let (cx, cz) = pick(&mut rng);
        let flower = if rng.below(4) == 0 { ROSE } else { DANDELION };
        let roll = rng.next_f64();
        let chance = match terrain.column(cx, cz).biome {
            Biome::Plains => 0.8,
            Biome::Forest => 0.4,
            _ => 0.0,
        };
        for _ in 0..8 {
            let x = (cx + rng.below(7) as i32 - 3).clamp(min_x, min_x + 15);
            let z = (cz + rng.below(7) as i32 - 3).clamp(min_z, min_z + 15);
            if roll < chance
                && let Some(column) = grassy(terrain, x, z)
            {
                writer.set_if(x, column.height + 1, z, flower, is_replaceable);
            }
        }
    }

    // Desert: dead bushes and cactus on open sand.
    for _ in 0..4 {
        let (x, z) = pick(&mut rng);
        let column = terrain.column(x, z);
        if column.biome == Biome::Desert && on_open_sand(terrain, column, x, z) {
            writer.set_if(x, column.height + 1, z, DEAD_BUSH, |b| b.is_air());
        }
    }
    for _ in 0..10 {
        let (x, z) = pick(&mut rng);
        let spread = rng.below(3) + 1;
        let tall = 1 + rng.below(spread) as i32;
        let column = terrain.column(x, z);
        let clear_sides = [(1, 0), (-1, 0), (0, 1), (0, -1)].iter().all(|&(dx, dz)| {
            terrain.column(x + dx, z + dz).height <= column.height
                && (1..=tall).all(|dy| {
                    writer
                        .get(x + dx, column.height + dy, z + dz)
                        .is_none_or(|b| b.is_air())
                })
        });
        if column.biome == Biome::Desert && on_open_sand(terrain, column, x, z) && clear_sides {
            for dy in 1..=tall {
                writer.set_if(x, column.height + dy, z, CACTUS, |b| b.is_air());
            }
        }
    }

    // Sugar cane on shores, next to water.
    for _ in 0..20 {
        let (x, z) = pick(&mut rng);
        let spread = rng.below(3) + 1;
        let tall = 2 + rng.below(spread) as i32;
        let column = terrain.column(x, z);
        let shore = column.height == SEA_LEVEL
            && matches!(column.top, GRASS | DIRT | SAND)
            && !terrain.is_cave(x, column.height, z);
        let by_water = [(1, 0), (-1, 0), (0, 1), (0, -1)]
            .iter()
            .any(|&(dx, dz)| terrain.column(x + dx, z + dz).height < SEA_LEVEL);
        if shore && by_water {
            for dy in 1..=tall {
                writer.set_if(x, column.height + dy, z, REEDS, |b| b.is_air());
            }
        }
    }
}

fn on_open_sand(terrain: &Terrain, column: Column, x: i32, z: i32) -> bool {
    column.top == SAND && column.height >= SEA_LEVEL && !terrain.is_cave(x, column.height, z)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::HashMap;

    /// Generates a square of chunks and counts each block above sea level by
    /// the biome of its column.
    fn census(
        terrain: &Terrain,
        radius: i32,
        center: ChunkPos,
    ) -> (
        HashMap<ChunkPos, Chunk>,
        HashMap<(Biome, BlockState), usize>,
    ) {
        let mut chunks = HashMap::new();
        let mut counts = HashMap::new();
        for dz in -radius..=radius {
            for dx in -radius..=radius {
                let pos = center.offset(dx, dz);
                let chunk = terrain.generate(pos);
                for y in SEA_LEVEL as usize..WORLD_HEIGHT {
                    for i in 0..256 {
                        let (x, z) = (i % 16, i / 16);
                        *counts
                            .entry((chunk.biome(x, z), chunk.get(x, y, z)))
                            .or_insert(0) += 1;
                    }
                }
                chunks.insert(pos, chunk);
            }
        }
        (chunks, counts)
    }

    fn count(
        counts: &HashMap<(Biome, BlockState), usize>,
        biome: Biome,
        block: BlockState,
    ) -> usize {
        counts.get(&(biome, block)).copied().unwrap_or(0)
    }

    /// Finds a chunk whose center column is `biome`.
    fn find_biome(terrain: &Terrain, biome: Biome) -> ChunkPos {
        (0..400)
            .flat_map(|r| [(r, 0), (0, r), (-r, 0), (0, -r), (r, r), (-r, -r)])
            .map(|(x, z)| ChunkPos::new(x * 3, z * 3))
            .find(|pos| {
                let (x, z) = pos.min_block();
                (-1..=1).all(|dx| {
                    (-1..=1)
                        .all(|dz| terrain.column(x + 8 + dx * 16, z + 8 + dz * 16).biome == biome)
                })
            })
            .unwrap_or_else(|| panic!("no {biome:?} found"))
    }

    #[test]
    fn forests_have_trees_and_plains_have_grass_and_flowers() {
        let terrain = Terrain::new(11);
        let (_, counts) = census(&terrain, 1, find_biome(&terrain, Biome::Forest));
        assert!(count(&counts, Biome::Forest, OAK_LOG) > 20, "{counts:?}");
        assert!(count(&counts, Biome::Forest, OAK_LEAVES) > 0);
        let (_, counts) = census(&terrain, 2, find_biome(&terrain, Biome::Plains));
        assert!(count(&counts, Biome::Plains, TALL_GRASS) > 50);
        assert!(count(&counts, Biome::Plains, DANDELION) + count(&counts, Biome::Plains, ROSE) > 0);
        for biome in [Biome::Plains, Biome::Desert, Biome::Ocean] {
            assert_eq!(
                count(&counts, biome, OAK_LOG),
                0,
                "1.0.0 {biome:?} has no trees"
            );
        }
    }

    #[test]
    fn birch_trees_grow_somewhere_in_forests() {
        let terrain = Terrain::new(11);
        let (_, counts) = census(&terrain, 3, find_biome(&terrain, Biome::Forest));
        assert!(count(&counts, Biome::Forest, BIRCH_LOG) > 0);
        assert!(count(&counts, Biome::Forest, BIRCH_LEAVES) > 0);
    }

    #[test]
    fn deserts_have_cactus_and_dead_bushes() {
        let terrain = Terrain::new(12);
        let (chunks, counts) = census(&terrain, 2, find_biome(&terrain, Biome::Desert));
        assert!(count(&counts, Biome::Desert, CACTUS) > 0, "{counts:?}");
        assert!(count(&counts, Biome::Desert, DEAD_BUSH) > 0);
        assert_eq!(count(&counts, Biome::Desert, TALL_GRASS), 0);
        // Every cactus stands on sand or cactus.
        for chunk in chunks.values() {
            for y in 1..WORLD_HEIGHT {
                for i in 0..256 {
                    if chunk.get(i % 16, y, i / 16) == CACTUS {
                        let below = chunk.get(i % 16, y - 1, i / 16);
                        assert!(below == SAND || below == CACTUS, "cactus on {below:?}");
                    }
                }
            }
        }
    }

    #[test]
    fn sugar_cane_grows_next_to_water() {
        let terrain = Terrain::new(13);
        let mut found = false;
        'search: for cz in -12..12 {
            for cx in -12..12 {
                let chunk = terrain.generate(ChunkPos::new(cx, cz));
                let (min_x, min_z) = chunk.pos.min_block();
                for i in 0..256 {
                    let (x, z) = (i % 16, i / 16);
                    if chunk.get(x, SEA_LEVEL as usize + 1, z) == REEDS {
                        let (wx, wz) = (min_x + x as i32, min_z + z as i32);
                        let wet = [(1, 0), (-1, 0), (0, 1), (0, -1)]
                            .iter()
                            .any(|&(dx, dz)| terrain.column(wx + dx, wz + dz).height < SEA_LEVEL);
                        assert!(wet, "sugar cane away from water at {wx},{wz}");
                        found = true;
                        break 'search;
                    }
                }
            }
        }
        assert!(found, "no sugar cane in 576 chunks");
    }

    #[test]
    fn trees_on_a_chunk_border_keep_their_leaves_in_the_next_chunk() {
        let terrain = Terrain::new(11);
        let center = find_biome(&terrain, Biome::Forest);
        let (chunks, _) = census(&terrain, 2, center);
        let mut checked = 0;
        for (&pos, chunk) in &chunks {
            let Some(east) = chunks.get(&pos.offset(1, 0)) else {
                continue;
            };
            for z in 0..16 {
                for y in SEA_LEVEL as usize..WORLD_HEIGHT - 1 {
                    // The top log of a trunk on the east edge of the chunk.
                    let log = chunk.get(15, y, z);
                    if log.id() != 17 || chunk.get(15, y + 1, z).id() != 18 {
                        continue;
                    }
                    // Leaves one block east of the top log are never a random
                    // corner, so they must be in the neighbor chunk.
                    let beside = east.get(0, y, z);
                    assert!(
                        matches!(beside.id(), 17 | 18),
                        "{beside:?} beside a tree top"
                    );
                    checked += 1;
                }
            }
        }
        assert!(checked > 0, "no border trees found to check");
    }
}
