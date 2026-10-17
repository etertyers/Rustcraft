//! Blocks and (from M2) chunks.

pub mod block;

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
