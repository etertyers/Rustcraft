//! Blocks, chunks and the loaded world.

pub mod biome;
pub mod block;
pub mod chunk;
pub mod daytime;
pub mod harvest;
pub mod light;
pub mod manager;
pub mod mesh;
pub mod save;
pub mod worldgen;

use std::{collections::HashMap, sync::Arc};

use crate::config;
use block::BlockState;
use chunk::{Chunk, ChunkPos};
use mesh::ChunkArea;

/// The loaded chunks.
#[derive(Default)]
pub struct World {
    chunks: HashMap<ChunkPos, Arc<Chunk>>,
}

impl World {
    pub fn insert(&mut self, chunk: Chunk) {
        self.chunks.insert(chunk.pos, Arc::new(chunk));
    }

    /// Re-adds a chunk kept from an earlier unload.
    pub fn insert_shared(&mut self, chunk: Arc<Chunk>) {
        self.chunks.insert(chunk.pos, chunk);
    }

    pub fn remove(&mut self, pos: ChunkPos) -> Option<Arc<Chunk>> {
        self.chunks.remove(&pos)
    }

    /// A loaded chunk for editing; copies it first if a worker still holds it.
    pub fn get_mut(&mut self, pos: ChunkPos) -> Option<&mut Chunk> {
        self.chunks.get_mut(&pos).map(Arc::make_mut)
    }

    /// Block at world (x, y, z): `None` if its chunk isn't loaded, air above
    /// or below the world.
    pub fn block(&self, x: i32, y: i32, z: i32) -> Option<BlockState> {
        let chunk = self.get(ChunkPos::of_block(x, z))?;
        if !(0..config::WORLD_HEIGHT as i32).contains(&y) {
            return Some(BlockState::AIR);
        }
        let size = config::CHUNK_SIZE as i32;
        Some(chunk.get(
            x.rem_euclid(size) as usize,
            y as usize,
            z.rem_euclid(size) as usize,
        ))
    }

    /// Biome of the column at world (x, z), if its chunk is loaded.
    pub fn biome_at(&self, x: i32, z: i32) -> Option<biome::Biome> {
        let chunk = self.get(ChunkPos::of_block(x, z))?;
        let size = config::CHUNK_SIZE as i32;
        Some(chunk.biome(x.rem_euclid(size) as usize, z.rem_euclid(size) as usize))
    }

    pub fn get(&self, pos: ChunkPos) -> Option<&Arc<Chunk>> {
        self.chunks.get(&pos)
    }

    pub fn len(&self) -> usize {
        self.chunks.len()
    }

    pub fn positions(&self) -> impl Iterator<Item = ChunkPos> + '_ {
        self.chunks.keys().copied()
    }

    /// The chunk at `pos` with its loaded neighbors, ready for meshing.
    pub fn area(&self, pos: ChunkPos) -> Option<ChunkArea> {
        let center = self.get(pos)?.clone();
        Some(ChunkArea::new(center, |p| self.get(p).cloned()))
    }
}

/// A block face, named by the direction it faces.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum Face {
    Top,
    Bottom,
    North,
    South,
    East,
    West,
}

impl Face {
    pub const ALL: [Face; 6] = [
        Face::Top,
        Face::Bottom,
        Face::North,
        Face::South,
        Face::East,
        Face::West,
    ];

    /// Position in `Face::ALL`, for per-face arrays.
    pub fn index(self) -> usize {
        self as usize
    }

    /// Unit offset toward the neighbor this face looks at.
    pub fn normal(self) -> [i32; 3] {
        match self {
            Face::Top => [0, 1, 0],
            Face::Bottom => [0, -1, 0],
            Face::North => [0, 0, -1],
            Face::South => [0, 0, 1],
            Face::East => [1, 0, 0],
            Face::West => [-1, 0, 0],
        }
    }

    /// Minecraft's fixed directional shading: top 1.0, bottom 0.5, Z faces 0.8, X faces 0.6.
    pub fn shade(self) -> f32 {
        match self {
            Face::Top => 1.0,
            Face::Bottom => 0.5,
            Face::North | Face::South => 0.8,
            Face::East | Face::West => 0.6,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn block_lookup_handles_negative_coordinates_and_unloaded_chunks() {
        let mut world = World::default();
        let mut chunk = Chunk::new(ChunkPos::new(-1, 0));
        chunk.set(15, 64, 3, BlockState::new(1, 0));
        world.insert(chunk);
        assert_eq!(world.block(-1, 64, 3), Some(BlockState::new(1, 0)));
        assert_eq!(world.block(-1, 65, 3), Some(BlockState::AIR));
        assert_eq!(world.block(-1, 500, 3), Some(BlockState::AIR));
        assert_eq!(world.block(0, 64, 3), None);
    }

    #[test]
    fn get_mut_copies_a_shared_chunk() {
        let mut world = World::default();
        world.insert(Chunk::new(ChunkPos::new(0, 0)));
        let shared = world.get(ChunkPos::new(0, 0)).unwrap().clone();
        world
            .get_mut(ChunkPos::new(0, 0))
            .unwrap()
            .set(1, 2, 3, BlockState::new(4, 0));
        assert!(shared.get(1, 2, 3).is_air());
        assert_eq!(world.block(1, 2, 3), Some(BlockState::new(4, 0)));
    }
}
