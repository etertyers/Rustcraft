//! Blocks, chunks and the loaded world.

pub mod block;
pub mod chunk;
pub mod mesh;
pub mod worldgen;

use std::{collections::HashMap, sync::Arc};

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
