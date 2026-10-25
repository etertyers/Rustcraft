//! Chunk storage: a 16×384×16 column (Y −64 to 319) split into 16³
//! sections, plus the block entities (chests, furnaces) in it. Blocks are
//! addressed by their world Y; section 0 starts at `MIN_Y`.

use std::collections::BTreeMap;

use crate::config::{CHUNK_SIZE, MAX_Y, MIN_Y, WORLD_HEIGHT};

use super::{biome::Biome, block::BlockState, block_entity::BlockEntity};

/// A block's position within its chunk: x and z in 0..16, and its world Y.
pub type LocalPos = [i16; 3];

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

/// Cave biome cells per chunk: 4×4 across, one per 4 blocks of height.
pub const CAVE_CELLS: usize = 4 * 4 * (WORLD_HEIGHT / 4);
/// A cave biome cell with no cave biome.
pub const NO_CAVE: u8 = u8::MAX;

/// Index of the cave biome cell holding a block at world `y`, `(y, z, x)`
/// order.
fn cave_cell(x: usize, y: i32, z: usize) -> usize {
    (((y - MIN_Y) as usize / 4) * 4 + z / 4) * 4 + x / 4
}

/// The section holding world `y` (0 at `MIN_Y`).
pub fn section_of(y: i32) -> usize {
    ((y - MIN_Y) as usize) / SECTION_SIZE
}

/// The world Y at the bottom of section `index`.
pub fn section_bottom(index: usize) -> i32 {
    MIN_Y + (index * SECTION_SIZE) as i32
}

/// Whether world `y` is inside a chunk.
pub fn in_height(y: i32) -> bool {
    (MIN_Y..MAX_Y).contains(&y)
}

/// One chunk column. All-air sections take no memory.
#[derive(Clone)]
pub struct Chunk {
    pub pos: ChunkPos,
    sections: [Option<Box<Section>>; SECTION_COUNT],
    /// Biome of each column, indexed `z * 16 + x`.
    biomes: [Biome; CHUNK_SIZE * CHUNK_SIZE],
    /// Cave biomes (lush and dripstone caves) underground, in 4×4×4 cells
    /// as modern Minecraft stores biomes; `NO_CAVE` cells take the
    /// column's biome. `None` when the chunk has no cave biome at all.
    cave_biomes: Option<Box<[u8; CAVE_CELLS]>>,
    block_entities: BTreeMap<LocalPos, BlockEntity>,
    /// Blocks whose state hangs on their neighbors (fence connections,
    /// stair corners) and must be worked out again once the chunks around
    /// are loaded: those upgraded from v1.0, which didn't store it.
    pub post_process: Vec<LocalPos>,
    /// Loaded from a save made before M34 (Y 0 to 255): the Overworld
    /// fills in the new depth below it (`Terrain::deepen`).
    pub shallow: bool,
}

impl Chunk {
    pub fn new(pos: ChunkPos) -> Self {
        Self {
            pos,
            sections: Default::default(),
            biomes: [Biome::default(); CHUNK_SIZE * CHUNK_SIZE],
            cave_biomes: None,
            block_entities: BTreeMap::new(),
            post_process: Vec::new(),
            shallow: false,
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

    /// The biome at a block: a cave biome if one fills its cell, otherwise
    /// the column's.
    pub fn biome_at(&self, x: usize, y: i32, z: usize) -> Biome {
        self.cave_biomes
            .as_ref()
            .and_then(|cells| Biome::from_id(cells[cave_cell(x, y, z)]))
            .unwrap_or_else(|| self.biome(x, z))
    }

    /// Puts a cave biome (or, with `None`, the column's) in the 4×4×4 cell
    /// holding a block.
    pub fn set_cave_biome(&mut self, x: usize, y: i32, z: usize, biome: Option<Biome>) {
        let id = biome.map_or(NO_CAVE, Biome::id);
        if id == NO_CAVE && self.cave_biomes.is_none() {
            return;
        }
        let cells = self
            .cave_biomes
            .get_or_insert_with(|| Box::new([NO_CAVE; CAVE_CELLS]));
        cells[cave_cell(x, y, z)] = id;
    }

    /// The cave biome cells, `None` if there are none.
    pub fn cave_biomes(&self) -> Option<&[u8; CAVE_CELLS]> {
        self.cave_biomes.as_deref()
    }

    fn index(x: usize, y: i32, z: usize) -> (usize, usize) {
        let up = (y - MIN_Y) as usize;
        let local_y = up % SECTION_SIZE;
        (
            up / SECTION_SIZE,
            (local_y * SECTION_SIZE + z) * SECTION_SIZE + x,
        )
    }

    /// Block at local (x, z) in 0..16 and world `y`. Out-of-range y is air.
    pub fn get(&self, x: usize, y: i32, z: usize) -> BlockState {
        debug_assert!(x < CHUNK_SIZE && z < CHUNK_SIZE, "({x}, {z}) outside chunk");
        if !in_height(y) {
            return BlockState::AIR;
        }
        let (section, index) = Self::index(x, y, z);
        self.sections[section]
            .as_ref()
            .map_or(BlockState::AIR, |s| s[index])
    }

    /// Sets the block at local (x, z) and world `y`. Panics if out of range.
    pub fn set(&mut self, x: usize, y: i32, z: usize, block: BlockState) {
        assert!(
            x < CHUNK_SIZE && in_height(y) && z < CHUNK_SIZE,
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

    /// One past the highest section holding any blocks (0 if it's all air).
    pub fn top_section(&self) -> usize {
        self.sections
            .iter()
            .rposition(Option::is_some)
            .map_or(0, |index| index + 1)
    }

    /// One above the top of the highest section holding any block, as a
    /// world Y: no block in the chunk is at or above this height.
    pub fn height_bound(&self) -> i32 {
        (0..SECTION_COUNT)
            .rev()
            .find(|&index| !self.section_is_empty(index))
            .map_or(MIN_Y, |index| section_bottom(index + 1))
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
    use crate::world::blocks;

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
            (0, 255, 0),
            (15, 255, 15),
            (7, 64, 3),
        ];
        let marker = |i: usize| {
            BlockState::of(
                [
                    blocks::STONE,
                    blocks::DIRT,
                    blocks::SAND,
                    blocks::BRICKS,
                    blocks::GLASS,
                ][i],
            )
        };
        for (i, &(x, y, z)) in corners.iter().enumerate() {
            chunk.set(x, y, z, marker(i));
        }
        for (i, &(x, y, z)) in corners.iter().enumerate() {
            assert_eq!(chunk.get(x, y, z), marker(i));
        }
        assert_eq!(chunk.get(1, 0, 0), BlockState::AIR);
    }

    #[test]
    fn air_sections_stay_unallocated() {
        let mut chunk = Chunk::new(ChunkPos::new(0, 0));
        chunk.set(0, 40, 0, BlockState::AIR);
        assert!((0..SECTION_COUNT).all(|s| chunk.section_is_empty(s)));
        chunk.set(0, 40, 0, BlockState::of(blocks::STONE));
        assert!(!chunk.section_is_empty(section_of(40)));
        assert!(chunk.section_is_empty(section_of(40) - 1));
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
        assert_eq!(chunk.get(0, 256, 0), BlockState::AIR);
    }
}
