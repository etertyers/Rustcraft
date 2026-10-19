//! Debug "block gallery": every block and listed variant on a grass floor.
//!
//! Blocks are laid out in rows of 16 IDs along +X, three blocks apart. Each
//! block's variants (one state for each, like a ripe crop or a lit furnace)
//! run along +Z behind it.

use std::collections::HashMap;

use crate::{
    config::{CHUNK_SIZE, SEA_LEVEL},
    world::{
        block::{BlockRegistry, BlockState},
        blocks,
        chunk::{Chunk, ChunkPos},
    },
};

const IDS_PER_ROW: usize = 16;
const COLUMN_SPACING: i32 = 3;
/// Leaves room for a block and its variants plus a gap between rows.
const ROW_SPACING: i32 = 12;
const FLOOR: BlockState = BlockState::of(blocks::GRASS_BLOCK);
const FLOOR_Y: i32 = SEA_LEVEL;

/// One block placed in the gallery.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Exhibit {
    pub block: BlockState,
    pub pos: [i32; 3],
}

/// Where each block goes: every block except air in its default state, then
/// for each variant the first state that shows it.
pub fn exhibits(registry: &BlockRegistry) -> Vec<Exhibit> {
    let mut exhibits = Vec::new();
    for (index, def) in registry
        .iter()
        .filter(|def| def.id != blocks::AIR)
        .enumerate()
    {
        let x = (index % IDS_PER_ROW) as i32 * COLUMN_SPACING;
        let row_z = (index / IDS_PER_ROW) as i32 * ROW_SPACING;
        let mut shown = vec![BlockState::of(def.id)];
        for variant in 0..def.variants.len() {
            let first = registry
                .states()
                .filter(|state| state.id() == def.id)
                .find(|&state| def.shows_variant(state, variant));
            if let Some(state) = first.filter(|state| !shown.contains(state)) {
                shown.push(state);
            }
        }
        for (z, block) in shown.into_iter().enumerate() {
            debug_assert!(
                (z as i32) < ROW_SPACING - 1,
                "{} has too many variants",
                def.name
            );
            exhibits.push(Exhibit {
                block,
                pos: [x, FLOOR_Y + 1, row_z + z as i32],
            });
        }
    }
    exhibits
}

/// Builds the gallery's chunks, with a grass floor one chunk wider than the exhibits.
pub fn gallery_chunks(registry: &BlockRegistry) -> Vec<Chunk> {
    let exhibits = exhibits(registry);
    let max_x = exhibits.iter().map(|e| e.pos[0]).max().unwrap_or(0);
    let max_z = exhibits.iter().map(|e| e.pos[2]).max().unwrap_or(0);
    let low = ChunkPos::of_block(-1, -1).offset(-1, -1);
    let high = ChunkPos::of_block(max_x, max_z).offset(1, 1);

    let mut chunks = HashMap::new();
    for cz in low.z..=high.z {
        for cx in low.x..=high.x {
            let pos = ChunkPos::new(cx, cz);
            let mut chunk = Chunk::new(pos);
            for z in 0..CHUNK_SIZE {
                for x in 0..CHUNK_SIZE {
                    chunk.set(x, FLOOR_Y as usize, z, FLOOR);
                }
            }
            chunks.insert(pos, chunk);
        }
    }
    let size = CHUNK_SIZE as i32;
    for exhibit in exhibits {
        let [x, y, z] = exhibit.pos;
        let chunk = chunks
            .get_mut(&ChunkPos::of_block(x, z))
            .expect("floor covers every exhibit");
        chunk.set(
            x.rem_euclid(size) as usize,
            y as usize,
            z.rem_euclid(size) as usize,
            exhibit.block,
        );
    }
    chunks.into_values().collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::world::blocks;
    use std::collections::{BTreeSet, HashSet};

    fn registry() -> &'static BlockRegistry {
        crate::world::block::registry()
    }

    #[test]
    fn every_block_and_variant_is_shown_once() {
        let registry = registry();
        let exhibits = exhibits(registry);
        let ids: BTreeSet<_> = exhibits.iter().map(|e| e.block.id()).collect();
        assert_eq!(ids.len(), registry.iter().count() - 1, "all but air");
        let wheat = exhibits
            .iter()
            .filter(|e| e.block.is(blocks::WHEAT))
            .count();
        assert_eq!(wheat, 8, "every growth stage");
        let positions: HashSet<_> = exhibits.iter().map(|e| e.pos).collect();
        assert_eq!(positions.len(), exhibits.len(), "two exhibits share a spot");
    }

    #[test]
    fn chunks_contain_every_exhibit() {
        let registry = registry();
        let chunks: HashMap<ChunkPos, Chunk> = gallery_chunks(registry)
            .into_iter()
            .map(|c| (c.pos, c))
            .collect();
        for exhibit in exhibits(registry) {
            let [x, y, z] = exhibit.pos;
            let chunk = &chunks[&ChunkPos::of_block(x, z)];
            let block = chunk.get(
                x.rem_euclid(16) as usize,
                y as usize,
                z.rem_euclid(16) as usize,
            );
            assert_eq!(block, exhibit.block);
        }
    }
}
