//! Block-breaking particles, following 1.0.0's `EntityFX` and
//! `EntityDiggingFX`: little squares cut from a block's texture that fly
//! out, fall and slide to a stop. Breaking a block bursts 4×4×4 of them
//! (`addBlockDestroyEffects`); mining chips one off the hit face each tick
//! (`addBlockHitEffects`).

use glam::{DVec3, IVec3};

use crate::{
    player::collision::{Aabb, Collider},
    world::{Face, block::BlockState},
};

/// 1.0.0 draws at most this many particles at once.
pub const MAX_PARTICLES: usize = 4000;
/// Per tick, as 1.0.0's digging particles (gravity 0.04 × the block's
/// particle gravity, 1 for every block in scope).
const GRAVITY: f64 = 0.04;
const DRAG: f64 = 0.98;
const GROUND_FRICTION: f64 = 0.7;
/// Half the width of the box a particle collides with (1.0.0's 0.2 size).
const HALF_SIZE: f64 = 0.1;

#[derive(Debug, Clone, PartialEq)]
pub struct Particle {
    pub position: DVec3,
    prev_position: DVec3,
    velocity: DVec3,
    age: u32,
    lifetime: u32,
    /// Half its drawn width in blocks.
    pub half_width: f32,
    /// The block and face whose texture it shows.
    pub block: BlockState,
    pub face: Face,
    /// Where in the texture its quarter-size window sits, 0–0.75 each way.
    pub window: [f32; 2],
}

impl Particle {
    /// 1.0.0's EntityFX constructor: `motion` plus a random push, scaled to
    /// a random speed, then nudged upward.
    fn new(
        position: DVec3,
        motion: DVec3,
        block: BlockState,
        face: Face,
        random: &mut impl FnMut() -> f32,
    ) -> Self {
        let mut r = || random() as f64;
        let push = DVec3::new(r() * 2.0 - 1.0, r() * 2.0 - 1.0, r() * 2.0 - 1.0) * 0.4;
        let mut velocity = motion + push;
        let speed = (r() + r() + 1.0) * 0.15;
        let length = velocity.length().max(1e-6);
        velocity = velocity / length * speed * 0.4;
        velocity.y += 0.1;
        // 1.0.0's scale, (0.5–1) × 2 halved for digging; drawn 0.1 blocks
        // either side of the middle per unit.
        let scale = (r() * 0.5 + 0.5) as f32;
        Self {
            position,
            prev_position: position,
            velocity,
            age: 0,
            lifetime: (4.0 / (r() * 0.9 + 0.1)) as u32,
            half_width: 0.1 * scale,
            block,
            face,
            window: [(r() * 3.0 / 4.0) as f32, (r() * 3.0 / 4.0) as f32],
        }
    }

    /// The burst from breaking `block` at `cell`: 4×4×4 particles flying
    /// out from the middle, showing its bottom texture.
    pub fn burst(cell: IVec3, block: BlockState, random: &mut impl FnMut() -> f32) -> Vec<Self> {
        let mut particles = Vec::with_capacity(64);
        for i in 0..4 {
            for j in 0..4 {
                for k in 0..4 {
                    let offset = DVec3::new(i as f64, j as f64, k as f64) / 4.0 + 0.125;
                    let position = cell.as_dvec3() + offset;
                    let motion = offset - 0.5;
                    particles.push(Self::new(position, motion, block, Face::Bottom, random));
                }
            }
        }
        particles
    }

    /// A chip off `face` of `block` at `cell` while it's mined: just
    /// outside the face, slow and small.
    pub fn chip(
        cell: IVec3,
        face: Face,
        block: BlockState,
        random: &mut impl FnMut() -> f32,
    ) -> Self {
        let normal = DVec3::from(face.normal().map(f64::from));
        let mut spot = DVec3::new(random() as f64, random() as f64, random() as f64) * 0.8 + 0.1;
        // On the face, 0.1 outside it.
        for axis in 0..3 {
            if normal[axis] != 0.0 {
                spot[axis] = if normal[axis] > 0.0 { 1.1 } else { -0.1 };
            }
        }
        let mut particle = Self::new(cell.as_dvec3() + spot, DVec3::ZERO, block, face, random);
        // 1.0.0's multiplyVelocity(0.2) and multipleParticleScaleBy(0.6).
        particle.velocity.x *= 0.2;
        particle.velocity.z *= 0.2;
        particle.velocity.y = (particle.velocity.y - 0.1) * 0.2 + 0.1;
        particle.half_width *= 0.6;
        particle
    }

    pub fn position_between_ticks(&self, alpha: f64) -> DVec3 {
        self.prev_position.lerp(self.position, alpha)
    }

    pub fn expired(&self) -> bool {
        self.age >= self.lifetime
    }

    /// One tick: fall, move (stopping against blocks), slow down.
    pub fn tick(&mut self, world: &impl Collider) {
        self.prev_position = self.position;
        self.age += 1;
        self.velocity.y -= GRAVITY;
        let mut on_ground = false;
        let mut boxes = Vec::new();
        for axis in 0..3 {
            let mut moved = self.position;
            moved[axis] += self.velocity[axis];
            let near = Aabb::new(moved - HALF_SIZE, moved + HALF_SIZE);
            boxes.clear();
            world.boxes(&near, &mut boxes);
            if boxes.iter().any(|b| b.intersects(&near)) {
                if axis == 1 && self.velocity.y < 0.0 {
                    on_ground = true;
                }
                self.velocity[axis] = 0.0;
            } else {
                self.position = moved;
            }
        }
        self.velocity *= DRAG;
        if on_ground {
            self.velocity.x *= GROUND_FRICTION;
            self.velocity.z *= GROUND_FRICTION;
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::world::blocks;

    /// Solid ground below y 64.
    struct Floor;

    impl Collider for Floor {
        fn boxes(&self, region: &Aabb, out: &mut Vec<Aabb>) {
            if region.min.y < 64.0 {
                out.push(Aabb::new(
                    DVec3::new(-100.0, 0.0, -100.0),
                    DVec3::new(100.0, 64.0, 100.0),
                ));
            }
        }
        fn slipperiness(&self, _: i32, _: i32, _: i32) -> f32 {
            0.6
        }
        fn liquid(&self, _: &Aabb) -> Option<crate::world::block::Fluid> {
            None
        }
    }

    #[test]
    fn a_broken_block_bursts_into_64_pieces_that_land_and_fade() {
        let mut seed = 1u32;
        let mut random = || {
            seed = seed.wrapping_mul(1_103_515_245).wrapping_add(12345);
            (seed >> 8) as f32 / (1u32 << 24) as f32
        };
        let stone = BlockState::of(blocks::STONE);
        let mut particles = Particle::burst(IVec3::new(0, 64, 0), stone, &mut random);
        assert_eq!(particles.len(), 64);
        assert!(particles.iter().all(|p| p.face == Face::Bottom));
        // The top layer flies upward to begin with.
        assert!(particles.iter().any(|p| p.velocity.y > 0.0));
        for _ in 0..200 {
            for particle in &mut particles {
                particle.tick(&Floor);
            }
        }
        for particle in &particles {
            assert!(
                particle.position.y >= 64.0 + HALF_SIZE - 1e-9,
                "{}",
                particle.position
            );
            assert!(particle.expired(), "lived {} ticks", particle.age);
        }
    }

    #[test]
    fn a_chip_starts_just_off_the_mined_face() {
        let mut random = || 0.5;
        let chip = Particle::chip(
            IVec3::new(3, 64, 3),
            Face::East,
            BlockState::of(blocks::STONE),
            &mut random,
        );
        assert!((chip.position.x - 4.1).abs() < 1e-9);
        assert!(chip.position.y > 64.0 && chip.position.y < 65.0);
        assert!(chip.half_width < 0.05);
    }
}
