//! TODO
use super::Piece;
use crate::world::{
    chunk::ChunkPos,
    worldgen::{rng::Rng, terrain::Terrain},
};

pub const CHANCE: f64 = 0.0;

pub fn generate(_terrain: &Terrain, _start: ChunkPos, _rng: &mut Rng) -> Option<Vec<Piece>> {
    None
}

pub fn positions(_seed: u64) -> Vec<ChunkPos> {
    Vec::new()
}
