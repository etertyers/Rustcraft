//! Chunk storage: a 16×128×16 column split into 16³ sections, plus the
//! block entities (chests, furnaces) in it.

use std::collections::BTreeMap;

use crate::config::{CHUNK_SIZE, WORLD_HEIGHT};

use super::{biome::Biome, block::BlockState, block_entity::BlockEntity};

/// A block's position within its chunk: x and z in 0..16, y in 0..128.
pub type LocalPos = [u8; 3];

/// Edge length of a section (and width of a chunk).
pub const SECTION_SIZE: usize = CHUNK_SIZE;
pub const SECTION_COUNT: usize = WORLD_HEIGHT / SECTION_SIZE;
pub const SECTION_VOLUME: usize = SECTION_SIZE * SECTION_SIZE * SECTION_SIZE;

/// Chunk coordinates: block coordinates divided by 16, rounding down.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub struct ChunkPos {
    pub x: i32,
    pub z: i32,
}

impl ChunkPos {
    pub const fn new(x: i32, z: i32) -> Self {
        Self { x, z }
    }

    /// The chunk containing block column (x, z).
    pub fn of_block(x: i32, z: i32) -> Self {
        let size = CHUNK_SIZE as i32;
        Self::new(x.div_euclid(size), z.div_euclid(size))
    }

    /// World coordinates of the chunk's (0, 0) column.
    pub fn min_block(self) -> (i32, i32) {
        let size = CHUNK_SIZE as i32;
        (self.x * size, self.z * size)
    }

    pub fn offset(self, dx: i32, dz: i32) -> Self {
        Self::new(self.x + dx, self.z + dz)
    }
}

type Section = [BlockState; SECTION_VOLUME];

/// One chunk column. All-air sections take no memory.
#[derive(Clone)]
pub struct Chunk {
    pub pos: ChunkPos,
    sections: [Option<Box<Section>>; SECTION_COUNT],
    /// Biome of each column, indexed `z * 16 + x`.
    biomes: [Biome; CHUNK_SIZE * CHUNK_SIZE],
    block_entities: BTreeMap<LocalPos, BlockEntity>,
}

impl Chunk {
    pub fn new(pos: ChunkPos) -> Self {
        Self {
            pos,
            sections: Default::default(),
            biomes: [Biome::default(); CHUNK_SIZE * CHUNK_SIZE],
            block_entities: BTreeMap::new(),
        }
    }

    pub fn block_entity(&self, at: LocalPos) -> Option<&BlockEntity> {
        self.block_entities.get(&at)
    }

    pub fn block_entity_mut(&mut self, at: LocalPos) -> Option<&mut BlockEntity> {
        self.block_entities.get_mut(&at)
    }

    /// Puts or (with `None`) removes the block entity at `at`, returning the
    /// one there before.
    pub fn set_block_entity(
        &mut self,
        at: LocalPos,
        entity: Option<BlockEntity>,
    ) -> Option<BlockEntity> {
        match entity {
            Some(entity) => self.block_entities.insert(at, entity),
            None => self.block_entities.remove(&at),
        }
    }

    pub fn block_entities(&self) -> impl Iterator<Item = (LocalPos, &BlockEntity)> {
        self.block_entities.iter().map(|(&at, entity)| (at, entity))
    }

    pub fn biome(&self, x: usize, z: usize) -> Biome {
        self.biomes[z * CHUNK_SIZE + x]
    }

    pub fn set_biome(&mut self, x: usize, z: usize, biome: Biome) {
        self.biomes[z * CHUNK_SIZE + x] = biome;
    }

    fn index(x: usize, y: usize, z: usize) -> (usize, usize) {
        let local_y = y % SECTION_SIZE;
        (
            y / SECTION_SIZE,
            (local_y * SECTION_SIZE + z) * SECTION_SIZE + x,
        )
    }

    /// Block at local (x, z) in 0..16 and y in 0..128. Out-of-range y is air.
    pub fn get(&self, x: usize, y: usize, z: usize) -> BlockState {
        debug_assert!(x < CHUNK_SIZE && z < CHUNK_SIZE, "({x}, {z}) outside chunk");
        if y >= WORLD_HEIGHT {
            return BlockState::AIR;
        }
        let (section, index) = Self::index(x, y, z);
        self.sections[section]
            .as_ref()
            .map_or(BlockState::AIR, |s| s[index])
    }

    /// Sets the block at local (x, y, z). Panics if out of range.
    pub fn set(&mut self, x: usize, y: usize, z: usize, block: BlockState) {
        assert!(
            x < CHUNK_SIZE && y < WORLD_HEIGHT && z < CHUNK_SIZE,
            "({x}, {y}, {z}) outside chunk"
        );
        let (section, index) = Self::index(x, y, z);
        match &mut self.sections[section] {
            Some(blocks) => blocks[index] = block,
            None if block.is_air() => {}
            slot @ None => {
                let mut blocks = Box::new([BlockState::AIR; SECTION_VOLUME]);
                blocks[index] = block;
                *slot = Some(blocks);
            }
        }
    }

    /// Whether section `index` (0 = bottom) holds no blocks at all.
    pub fn section_is_empty(&self, index: usize) -> bool {
        self.sections[index].is_none()
    }

    /// Section `index`'s blocks in `y, z, x` order, or `None` if it's all air.
    pub fn section(&self, index: usize) -> Option<&[BlockState]> {
        self.sections[index].as_deref().map(|blocks| &blocks[..])
    }

    /// Replaces section `index` with `blocks` in `y, z, x` order. Panics
    /// unless there are exactly `SECTION_VOLUME` blocks.
    pub fn set_section(&mut self, index: usize, blocks: &[BlockState]) {
        let blocks: &Section = blocks.try_into().expect("a whole section");
        self.sections[index] = Some(Box::new(*blocks));
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn chunk_pos_rounds_toward_negative_infinity() {
        assert_eq!(ChunkPos::of_block(0, 15), ChunkPos::new(0, 0));
        assert_eq!(ChunkPos::of_block(16, -1), ChunkPos::new(1, -1));
        assert_eq!(ChunkPos::of_block(-16, -17), ChunkPos::new(-1, -2));
        assert_eq!(ChunkPos::new(-1, 2).min_block(), (-16, 32));
    }

    #[test]
    fn get_returns_what_set_stored_at_every_corner() {
        let mut chunk = Chunk::new(ChunkPos::new(0, 0));
        let corners = [
            (0, 0, 0),
            (15, 0, 15),
            (0, 127, 0),
            (15, 127, 15),
            (7, 64, 3),
        ];
        for (i, &(x, y, z)) in corners.iter().enumerate() {
            chunk.set(x, y, z, BlockState::new(1, i as u8));
        }
        for (i, &(x, y, z)) in corners.iter().enumerate() {
            assert_eq!(chunk.get(x, y, z), BlockState::new(1, i as u8));
        }
        assert_eq!(chunk.get(1, 0, 0), BlockState::AIR);
    }

    #[test]
    fn air_sections_stay_unallocated() {
        let mut chunk = Chunk::new(ChunkPos::new(0, 0));
        chunk.set(0, 40, 0, BlockState::AIR);
        assert!((0..SECTION_COUNT).all(|s| chunk.section_is_empty(s)));
        chunk.set(0, 40, 0, BlockState::new(1, 0));
        assert!(!chunk.section_is_empty(2));
        assert!(chunk.section_is_empty(1));
    }

    #[test]
    fn biomes_default_to_plains_and_are_per_column() {
        let mut chunk = Chunk::new(ChunkPos::new(0, 0));
        assert_eq!(chunk.biome(5, 9), Biome::Plains);
        chunk.set_biome(5, 9, Biome::Desert);
        assert_eq!(chunk.biome(5, 9), Biome::Desert);
        assert_eq!(chunk.biome(9, 5), Biome::Plains);
    }

    #[test]
    fn above_world_height_is_air() {
        let chunk = Chunk::new(ChunkPos::new(0, 0));
        assert_eq!(chunk.get(0, 128, 0), BlockState::AIR);
    }
}
