//! Blocks, chunks and the loaded world.

pub mod biome;
pub mod blend;
pub mod block;
pub mod block_entity;
pub mod blocks;
pub mod chorus;
pub mod chunk;
pub mod connect;
pub mod daytime;
pub mod dimension;
pub mod end_portal;
pub mod explosion;
pub mod fast_hash;
pub mod fire;
pub mod fluid;
pub mod harvest;
pub mod legacy;
pub mod light;
pub mod manager;
pub mod mesh;
pub mod piston;
pub mod portal;
pub mod props;
pub mod rails;
pub mod redstone;
pub mod save;
pub mod shape;
pub mod structure;
pub mod worldgen;

use std::sync::Arc;

use crate::config;
use block::BlockState;
use chunk::{Chunk, ChunkPos};
use fast_hash::FastMap;
use mesh::ChunkArea;

/// The loaded chunks.
#[derive(Default)]
pub struct World {
    chunks: FastMap<ChunkPos, Arc<Chunk>>,
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

    /// The biome at a block, counting cave biomes underground.
    pub fn biome_at_block(&self, x: i32, y: i32, z: i32) -> Option<biome::Biome> {
        let chunk = self.get(ChunkPos::of_block(x, z))?;
        let size = config::CHUNK_SIZE as i32;
        let y = y.clamp(0, config::WORLD_HEIGHT as i32 - 1) as usize;
        Some(chunk.biome_at(x.rem_euclid(size) as usize, y, z.rem_euclid(size) as usize))
    }

    pub fn get(&self, pos: ChunkPos) -> Option<&Arc<Chunk>> {
        self.chunks.get(&pos)
    }

    pub fn len(&self) -> usize {
        self.chunks.len()
    }

    pub fn is_empty(&self) -> bool {
        self.chunks.is_empty()
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

    /// The face looking the other way.
    pub fn opposite(self) -> Face {
        match self {
            Face::Top => Face::Bottom,
            Face::Bottom => Face::Top,
            Face::North => Face::South,
            Face::South => Face::North,
            Face::East => Face::West,
            Face::West => Face::East,
        }
    }

    /// A quarter turn clockwise seen from above (north → east); top and
    /// bottom stay put.
    pub fn rotate_cw(self) -> Face {
        match self {
            Face::North => Face::East,
            Face::East => Face::South,
            Face::South => Face::West,
            Face::West => Face::North,
            vertical => vertical,
        }
    }

    /// A quarter turn counterclockwise seen from above (north → west).
    pub fn rotate_ccw(self) -> Face {
        self.rotate_cw().opposite()
    }

    /// The horizontal direction a player looking along `yaw` (radians, 0
    /// north, turning east) faces.
    pub fn from_yaw(yaw: f32) -> Face {
        let quarter = (yaw / std::f32::consts::FRAC_PI_2).round() as i32;
        [Face::North, Face::East, Face::South, Face::West][quarter.rem_euclid(4) as usize]
    }

    pub fn is_horizontal(self) -> bool {
        !matches!(self, Face::Top | Face::Bottom)
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
    use crate::world::blocks;

    #[test]
    fn block_lookup_handles_negative_coordinates_and_unloaded_chunks() {
        let mut world = World::default();
        let mut chunk = Chunk::new(ChunkPos::new(-1, 0));
        chunk.set(15, 64, 3, BlockState::of(blocks::STONE));
        world.insert(chunk);
        assert_eq!(world.block(-1, 64, 3), Some(BlockState::of(blocks::STONE)));
        assert_eq!(world.block(-1, 65, 3), Some(BlockState::AIR));
        assert_eq!(world.block(-1, 500, 3), Some(BlockState::AIR));
        assert_eq!(world.block(0, 64, 3), None);
    }

    #[test]
    fn get_mut_copies_a_shared_chunk() {
        let mut world = World::default();
        world.insert(Chunk::new(ChunkPos::new(0, 0)));
        let shared = world.get(ChunkPos::new(0, 0)).unwrap().clone();
        world.get_mut(ChunkPos::new(0, 0)).unwrap().set(
            1,
            2,
            3,
            BlockState::of(blocks::COBBLESTONE),
        );
        assert!(shared.get(1, 2, 3).is_air());
        assert_eq!(
            world.block(1, 2, 3),
            Some(BlockState::of(blocks::COBBLESTONE))
        );
    }
}
