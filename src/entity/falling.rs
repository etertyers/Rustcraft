//! Falling blocks: sand, gravel and the like with nothing under them,
//! following modern Minecraft's `FallingBlockEntity`. One falls straight
//! down, speeding up by 0.04 a tick with 2% drag, and lands as a block in
//! the cell it comes to rest in, or drops as an item if that cell can't
//! take it (a torch, a slab) or nothing would hold it up there.

use glam::{DVec3, IVec3};

use crate::{
    player::collision::{Aabb, Collider},
    world::block::BlockState,
};

/// Speed gained downward each tick.
const GRAVITY: f64 = 0.04;
/// Speed kept each tick.
const DRAG: f64 = 0.98;
/// Width and height of the falling box.
const SIZE: f64 = 0.98;
/// Ticks after which a block that never lands is dropped (modern 600 at
/// the bottom of the world, 100 lower down).
const MAX_TICKS: u32 = 600;

#[derive(Debug, Clone, PartialEq, serde::Serialize, serde::Deserialize)]
pub struct FallingBlock {
    pub state: BlockState,
    /// The bottom middle of the box.
    pub position: DVec3,
    pub previous: DVec3,
    pub velocity: DVec3,
    pub ticks: u32,
    /// Height it started falling from.
    origin: f64,
}

/// What a falling block did this tick.
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum Fall {
    Falling,
    /// It came to rest with its bottom in this cell.
    Landed(IVec3),
    /// It fell out of the world or for too long.
    Lost,
}

impl FallingBlock {
    /// A block starting to fall from cell `at`.
    pub fn new(state: BlockState, at: IVec3) -> Self {
        let position = at.as_dvec3() + DVec3::new(0.5, 0.0, 0.5);
        Self {
            state,
            position,
            previous: position,
            velocity: DVec3::ZERO,
            ticks: 0,
            origin: position.y,
        }
    }

    /// How far it has dropped since it started.
    pub fn fallen(&self) -> f64 {
        self.origin - self.position.y
    }

    pub fn aabb(&self) -> Aabb {
        let half = SIZE / 2.0;
        Aabb::new(
            self.position - DVec3::new(half, 0.0, half),
            self.position + DVec3::new(half, SIZE, half),
        )
    }

    /// The bottom middle, `alpha` of the way from the last tick to this one.
    pub fn between_ticks(&self, alpha: f64) -> DVec3 {
        self.previous.lerp(self.position, alpha)
    }

    /// One tick of falling against `collider`.
    pub fn tick(&mut self, collider: &impl Collider) -> Fall {
        self.previous = self.position;
        self.ticks += 1;
        self.velocity.y -= GRAVITY;
        let wanted = self.velocity.y;
        let aabb = self.aabb();
        let mut boxes = Vec::new();
        collider.boxes(&aabb.stretch(DVec3::new(0.0, wanted, 0.0)), &mut boxes);
        let dy = boxes.iter().fold(wanted, |dy, b| b.clip(&aabb, 1, dy));
        self.position.y += dy;
        self.velocity *= DRAG;
        if dy != wanted && wanted < 0.0 {
            return Fall::Landed(self.position.floor().as_ivec3());
        }
        if self.position.y < -64.0 || self.ticks > MAX_TICKS {
            return Fall::Lost;
        }
        Fall::Falling
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{
        player::collision::WorldCollider,
        world::{
            World, blocks,
            chunk::{Chunk, ChunkPos},
        },
    };

    #[test]
    fn falls_faster_and_faster_until_it_lands_on_the_ground() {
        let mut world = World::default();
        let mut chunk = Chunk::new(ChunkPos::new(0, 0));
        chunk.set(2, 63, 2, BlockState::of(blocks::STONE));
        world.insert(chunk);
        let collider = WorldCollider {
            world: &world,
            blocks: crate::world::block::registry(),
        };
        let mut sand = FallingBlock::new(BlockState::of(blocks::SAND), IVec3::new(2, 70, 2));
        let mut ticks = 0;
        let landed = loop {
            ticks += 1;
            match sand.tick(&collider) {
                Fall::Falling => {}
                other => break other,
            }
        };
        assert_eq!(landed, Fall::Landed(IVec3::new(2, 64, 2)));
        assert_eq!(sand.position.y, 64.0);
        // Six blocks at 0.04 a tick², less drag: about 17 ticks.
        assert!((15..=20).contains(&ticks), "{ticks} ticks");
    }
}
