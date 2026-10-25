//! Flat test world: bedrock, stone, three layers of dirt and grass at sea level.

use crate::{
    config::{CHUNK_SIZE, SEA_LEVEL},
    world::{
        block::BlockState,
        blocks,
        chunk::{Chunk, ChunkPos},
    },
};

const BEDROCK: BlockState = BlockState::of(blocks::BEDROCK);
const STONE: BlockState = BlockState::of(blocks::STONE);
const DIRT: BlockState = BlockState::of(blocks::DIRT);
const GRASS: BlockState = BlockState::of(blocks::GRASS_BLOCK);

pub fn flat_chunk(pos: ChunkPos) -> Chunk {
    let mut chunk = Chunk::new(pos);
    let top = SEA_LEVEL;
    for z in 0..CHUNK_SIZE {
        for x in 0..CHUNK_SIZE {
            chunk.set(x, 0, z, BEDROCK);
            for y in 1..top - 3 {
                chunk.set(x, y, z, STONE);
            }
            for y in top - 3..top {
                chunk.set(x, y, z, DIRT);
            }
            chunk.set(x, top, z, GRASS);
        }
    }
    chunk
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn layers_are_in_order_with_grass_at_sea_level() {
        let chunk = flat_chunk(ChunkPos::new(-2, 5));
        assert_eq!(chunk.get(3, 0, 3), BEDROCK);
        assert_eq!(chunk.get(3, 1, 3), STONE);
        assert_eq!(chunk.get(3, 62, 3), DIRT);
        assert_eq!(chunk.get(3, 63, 3), GRASS);
        assert!(chunk.get(3, 64, 3).is_air());
    }
}
