//! Primed TNT, following modern Minecraft's `PrimedTnt`: a block that pops
//! up a little, falls with 0.04 gravity and 2% drag, slides to a stop and
//! blows up when its 80-tick fuse runs out.

use glam::DVec3;

use crate::player::collision::{Aabb, Collider};

/// Ticks from lighting to the blast.
pub const FUSE: i32 = 80;
const GRAVITY: f64 = 0.04;
const DRAG: f64 = 0.98;
const GROUND_FRICTION: DVec3 = DVec3::new(0.7, -0.5, 0.7);
/// Width and height of its box.
const SIZE: f64 = 0.98;

#[derive(Debug, Clone, PartialEq)]
pub struct PrimedTnt {
    /// The bottom middle of the box.
    pub position: DVec3,
    pub previous: DVec3,
    pub velocity: DVec3,
    /// Ticks until it explodes.
    pub fuse: i32,
    on_ground: bool,
}

impl PrimedTnt {
    /// TNT lit at `position` (bottom middle), thrown up and a little
    /// sideways along `angle` (radians), as lighting it does.
    pub fn new(position: DVec3, angle: f64, fuse: i32) -> Self {
        Self {
            position,
            previous: position,
            velocity: DVec3::new(-angle.sin() * 0.02, 0.2, -angle.cos() * 0.02),
            fuse,
            on_ground: false,
        }
    }

    pub fn aabb(&self) -> Aabb {
        let half = SIZE / 2.0;
        Aabb::new(
            self.position - DVec3::new(half, 0.0, half),
            self.position + DVec3::new(half, SIZE, half),
        )
    }

    /// Where its blast is centered.
    pub fn center(&self) -> DVec3 {
        self.position + DVec3::Y * (SIZE / 16.0)
    }

    pub fn between_ticks(&self, alpha: f64) -> DVec3 {
        self.previous.lerp(self.position, alpha)
    }

    /// How far its flashing has swollen it, 1 or a little more (modern
    /// `TntRenderer`): it puffs up in the last half second.
    pub fn scale(&self, alpha: f32) -> f32 {
        let left = self.fuse as f32 - alpha + 1.0;
        if left < 10.0 {
            let f = (1.0 - left / 10.0).clamp(0.0, 1.0);
            1.0 + f.powi(4) * 0.3
        } else {
            1.0
        }
    }

    /// Whether it's flashing white now.
    pub fn flashing(&self) -> bool {
        self.fuse / 5 % 2 == 0
    }

    /// One tick of falling and burning down. Returns true when it blows.
    pub fn tick(&mut self, collider: &impl Collider) -> bool {
        self.previous = self.position;
        self.velocity.y -= GRAVITY;
        self.slide(collider);
        self.velocity *= DRAG;
        if self.on_ground {
            self.velocity *= GROUND_FRICTION;
        }
        self.fuse -= 1;
        self.fuse <= 0
    }

    /// Moves by its velocity, one axis at a time (y first), stopping at
    /// blocks.
    fn slide(&mut self, collider: &impl Collider) {
        self.on_ground = false;
        for axis in [1, 0, 2] {
            let wanted = self.velocity[axis];
            if wanted == 0.0 {
                continue;
            }
            let aabb = self.aabb();
            let mut stretch = DVec3::ZERO;
            stretch[axis] = wanted;
            let mut boxes = Vec::new();
            collider.boxes(&aabb.stretch(stretch), &mut boxes);
            let moved = boxes
                .iter()
                .fold(wanted, |d, block| block.clip(&aabb, axis, d));
            self.position[axis] += moved;
            if moved != wanted {
                if axis == 1 && wanted < 0.0 {
                    self.on_ground = true;
                }
                self.velocity[axis] = 0.0;
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{
        player::collision::WorldCollider,
        world::{
            World,
            block::BlockState,
            blocks,
            chunk::{Chunk, ChunkPos},
        },
    };

    #[test]
    fn it_pops_up_falls_settles_and_blows_after_eighty_ticks() {
        let mut world = World::default();
        let mut chunk = Chunk::new(ChunkPos::new(0, 0));
        for x in 0..16 {
            for z in 0..16 {
                chunk.set(x, 63, z, BlockState::of(blocks::STONE));
            }
        }
        world.insert(chunk);
        let collider = WorldCollider {
            world: &world,
            blocks: crate::world::block::registry(),
        };
        let mut tnt = PrimedTnt::new(DVec3::new(8.5, 64.0, 8.5), 0.0, FUSE);
        let mut peak = 0.0f64;
        let mut ticks = 0;
        loop {
            ticks += 1;
            let blew = tnt.tick(&collider);
            peak = peak.max(tnt.position.y);
            if blew {
                break;
            }
        }
        assert_eq!(ticks, 80);
        assert!(peak > 64.2 && peak < 64.5, "popped up {peak}");
        assert!((tnt.position.y - 64.0).abs() < 1e-6, "rests on the stone");
        assert!(tnt.velocity.x.abs() < 1e-3 && tnt.velocity.z.abs() < 1e-3);
    }

    #[test]
    fn it_flashes_every_five_ticks_and_swells_at_the_end() {
        let mut tnt = PrimedTnt::new(DVec3::ZERO, 0.0, 80);
        assert!(tnt.flashing());
        tnt.fuse = 75;
        assert!(!tnt.flashing());
        tnt.fuse = 40;
        assert_eq!(tnt.scale(0.0), 1.0);
        tnt.fuse = 0;
        assert!(tnt.scale(0.0) > 1.1);
    }
}
