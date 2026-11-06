//! Distant Terrain (M69): terrain drawn far past the render distance as
//! simplified level-of-detail (LOD) meshes, after the Distant Horizons mod
//! (rebuilt natively, from its behavior).
//!
//! - `data`: the samples, slices and tiles, and their compressed bytes.
//! - `colors`: block colors for LOD, taken from the atlas.
//! - `sample`: filling tiles, from real chunks and from the fast path that
//!   runs the terrain noise at low resolution for unexplored land.
//!
//! LOD never changes the world: it only reads chunks and the terrain.

pub mod colors;
pub mod data;
pub mod sample;
pub mod store;

use crate::config::CHUNK_SIZE;

/// Samples along one side of a tile. A tile at level L covers
/// `TILE_SAMPLES << L` blocks a side.
pub const TILE_SAMPLES: usize = 32;
/// The coarsest level: 64 blocks per sample.
pub const MAX_LEVEL: u8 = 6;
/// Vertical slices per sample at most (ground, fluid, canopy, overhang).
pub const MAX_SLICES: usize = 4;
/// Distant Terrain's smallest and largest distances, in chunks.
pub const MIN_DISTANCE: i32 = 32;
pub const MAX_DISTANCE: i32 = 512;
/// The default distance in the Barebones and Realistic styles.
pub const DEFAULT_DISTANCE: i32 = 128;
/// Disk cap per world, in bytes (oldest-used removed first).
pub const DEFAULT_DISK_CAP: u64 = 1 << 30;
/// Memory cap for the LOD data kept in RAM, in bytes.
pub const MEMORY_CAP: u64 = 1 << 30;

/// The quality setting: slices per sample and the finest level.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, serde::Serialize, serde::Deserialize)]
pub enum Quality {
    Low,
    #[default]
    Medium,
    High,
}

impl Quality {
    pub const ALL: [Quality; 3] = [Quality::Low, Quality::Medium, Quality::High];

    /// Slices kept per sample.
    pub fn slices(self) -> usize {
        match self {
            Quality::Low => 2,
            Quality::Medium => 3,
            Quality::High => MAX_SLICES,
        }
    }

    /// The finest level drawn (2 = 4 blocks per sample).
    pub fn finest_level(self) -> u8 {
        match self {
            Quality::Low => 2,
            Quality::Medium => 1,
            Quality::High => 0,
        }
    }

    pub fn name(self) -> &'static str {
        match self {
            Quality::Low => "Low",
            Quality::Medium => "Medium",
            Quality::High => "High",
        }
    }
}

/// The level used for a tile `distance` blocks from the viewer: the finest
/// level next to the loaded area, one coarser each time the distance
/// doubles past `near` blocks.
pub fn level_for(distance: f32, near: f32, quality: Quality) -> u8 {
    let ratio = (distance / near.max(1.0)).max(1.0);
    let extra = ratio.log2().floor() as i32;
    (i32::from(quality.finest_level()) + extra).clamp(0, i32::from(MAX_LEVEL)) as u8
}

/// Blocks along one side of a chunk.
pub const CHUNK_BLOCKS: i32 = CHUNK_SIZE as i32;

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn levels_grow_with_distance_and_never_pass_the_coarsest() {
        let q = Quality::Medium;
        assert_eq!(level_for(10.0, 144.0, q), 1);
        assert_eq!(level_for(300.0, 144.0, q), 2);
        assert_eq!(level_for(1e9, 144.0, q), MAX_LEVEL);
        assert_eq!(level_for(10.0, 144.0, Quality::High), 0);
    }
}
