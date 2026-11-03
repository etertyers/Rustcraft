//! Splashes and waterfall mist (M57, Trailer look only), in the style of
//! the Effective mod: foam and a little mist where falling water lands,
//! at most 32 particles per waterfall and only within 32 blocks, and a
//! splash when something falls fast into water. They're drawn by the
//! client from what it sees, like every look-only effect, and use the
//! block-particle system (white squares).

use std::collections::HashMap;

use glam::{DVec3, IVec3};

use crate::{
    entity::particle::Particle,
    player::collision::Collider,
    world::{block::BlockState, blocks, fluid},
};

/// Waterfalls farther than this make nothing.
pub const RANGE: i32 = 32;
/// The most particles one waterfall has at once.
pub const PER_WATERFALL: usize = 32;
/// Falling faster than this (blocks a tick) into water splashes.
pub const FAST: f64 = 0.4;
/// The most a body of water in view makes at once, across waterfalls.
const MAX_TOTAL: usize = 600;
/// Particles in the splash of something falling in.
const BURST: usize = 14;

/// A falling water block (a source's flow downward: level 8 and up).
pub fn is_falling_water(block: BlockState) -> bool {
    block.is(blocks::WATER) && fluid::level(block) >= 8
}

/// The bottoms of the waterfalls on one row of the world (`z`) within
/// `RANGE` of `center` in x and y: falling water with something that isn't
/// falling water below it. `block` looks a block up.
pub fn landings_on_row(
    block: &impl Fn(i32, i32, i32) -> Option<BlockState>,
    center: IVec3,
    z: i32,
) -> Vec<IVec3> {
    let mut found = Vec::new();
    for x in center.x - RANGE..=center.x + RANGE {
        for y in center.y - RANGE / 2..=center.y + RANGE / 2 {
            if !block(x, y, z).is_some_and(is_falling_water) {
                continue;
            }
            if !block(x, y - 1, z).is_some_and(is_falling_water) {
                found.push(IVec3::new(x, y, z));
            }
        }
    }
    found
}

/// What the client knows of something that can splash into water.
#[derive(Debug, Clone, Copy)]
pub struct Body {
    pub id: u64,
    pub feet: DVec3,
    pub velocity: DVec3,
    pub in_water: bool,
}

#[derive(Default)]
pub struct Splashes {
    particles: Vec<Particle>,
    /// The waterfall each particle came from (`None`: a splash).
    owners: Vec<Option<IVec3>>,
    landings: Vec<IVec3>,
    found: Vec<IVec3>,
    row: i32,
    last_time: Option<u64>,
    /// Whether each body was in water, and how fast it fell, a tick ago.
    before: HashMap<u64, (bool, f64)>,
    random: u64,
}

impl Splashes {
    /// The particles to draw.
    pub fn particles(&self) -> &[Particle] {
        &self.particles
    }

    pub fn clear(&mut self) {
        *self = Self::default();
    }

    fn rand(&mut self) -> f32 {
        // xorshift64*
        self.random = self.random.wrapping_add(0x9E37_79B9_7F4A_7C15);
        let mut x = self.random;
        x ^= x >> 12;
        x ^= x << 25;
        x ^= x >> 27;
        ((x.wrapping_mul(0x2545_F491_4F6C_DD1D) >> 40) as f32) / (1u64 << 24) as f32
    }

    /// One game tick's worth, once per tick of `time`: moves the
    /// particles, looks a row further for waterfalls round `viewer`, and
    /// makes foam where they land and splashes where `bodies` fell in.
    pub fn tick(
        &mut self,
        time: u64,
        viewer: DVec3,
        bodies: &[Body],
        block: &impl Fn(i32, i32, i32) -> Option<BlockState>,
        collider: &impl Collider,
    ) {
        if self.last_time == Some(time) {
            return;
        }
        self.last_time = Some(time);
        // Move what's there.
        for particle in &mut self.particles {
            particle.tick(collider);
        }
        let keep: Vec<bool> = self.particles.iter().map(|p| !p.expired()).collect();
        let mut at = 0;
        self.particles.retain(|_| {
            at += 1;
            keep[at - 1]
        });
        let mut at = 0;
        self.owners.retain(|_| {
            at += 1;
            keep[at - 1]
        });
        // Look for waterfalls, a row of the world a tick, round the viewer.
        let center = viewer.floor().as_ivec3();
        let z = center.z - RANGE + self.row;
        let row = landings_on_row(block, center, z);
        self.found.extend(row);
        self.row += 1;
        if self.row > 2 * RANGE {
            self.row = 0;
            self.landings = std::mem::take(&mut self.found);
        }
        // Foam and mist where they land, within range and under the cap.
        let landings = self.landings.clone();
        for landing in landings {
            let spot = landing.as_dvec3() + DVec3::new(0.5, 0.1, 0.5);
            if (spot - viewer).length() > f64::from(RANGE) {
                continue;
            }
            let mine = self.owners.iter().filter(|o| **o == Some(landing)).count();
            if mine >= PER_WATERFALL || self.particles.len() >= MAX_TOTAL {
                continue;
            }
            let (a, b, c, d) = (self.rand(), self.rand(), self.rand(), self.rand());
            let mut seed = self.rand();
            let mut r = move || {
                seed = (seed * 7.31 + 0.37).fract();
                seed
            };
            if a < 0.7 {
                // Foam thrown up and a little sideways.
                let at = spot + DVec3::new(f64::from(b) - 0.5, 0.0, f64::from(c) - 0.5) * 0.7;
                let up = 0.12 + f64::from(d) * 0.14;
                let side = DVec3::new(f64::from(c) - 0.5, 0.0, f64::from(b) - 0.5) * 0.12;
                self.particles
                    .push(Particle::foam(at, DVec3::new(side.x, up, side.z), &mut r));
            } else {
                // Mist drifting up.
                self.particles
                    .push(Particle::colored(spot + DVec3::Y * 0.3, 0xDDE8F2, &mut r));
            }
            self.owners.push(Some(landing));
        }
        // Something falling fast into water splashes.
        for body in bodies {
            let (was_in, was_vy) = self.before.get(&body.id).copied().unwrap_or((false, 0.0));
            if body.in_water && !was_in && was_vy < -FAST && (body.feet - viewer).length() <= 48.0 {
                self.splash(body.feet, -was_vy);
            }
            self.before
                .insert(body.id, (body.in_water, body.velocity.y));
        }
    }

    /// A splash at `at` from falling at `speed` blocks a tick.
    pub fn splash(&mut self, at: DVec3, speed: f64) {
        let count = (BURST as f64 * (speed / FAST).min(2.5)) as usize;
        for _ in 0..count {
            if self.particles.len() >= MAX_TOTAL {
                break;
            }
            let (a, b, c) = (self.rand(), self.rand(), self.rand());
            let angle = f64::from(a) * std::f64::consts::TAU;
            let push = 0.04 + f64::from(b) * 0.10;
            let velocity = DVec3::new(
                angle.cos() * push,
                0.18 + f64::from(c) * 0.22,
                angle.sin() * push,
            );
            let mut seed = self.rand();
            let mut r = move || {
                seed = (seed * 7.31 + 0.37).fract();
                seed
            };
            self.particles
                .push(Particle::foam(at + DVec3::Y * 0.1, velocity, &mut r));
            self.owners.push(None);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::player::collision::Aabb;
    use crate::world::block::Fluid;

    /// A column of falling water over a pool, in a map of blocks.
    fn waterfall() -> HashMap<(i32, i32, i32), BlockState> {
        let mut world = HashMap::new();
        for y in 70..=75 {
            world.insert((5, y, 5), fluid::at_level(Fluid::Water, 8));
        }
        world.insert((5, 69, 5), fluid::at_level(Fluid::Water, 0));
        world
    }

    struct Open;
    impl Collider for Open {
        fn boxes(&self, _: &Aabb, _: &mut Vec<Aabb>) {}
        fn slipperiness(&self, _: i32, _: i32, _: i32) -> f32 {
            0.6
        }
    }

    fn lookup(
        world: &HashMap<(i32, i32, i32), BlockState>,
    ) -> impl Fn(i32, i32, i32) -> Option<BlockState> + '_ {
        |x, y, z| Some(*world.get(&(x, y, z)).unwrap_or(&BlockState::AIR))
    }

    #[test]
    fn the_bottom_of_a_waterfall_is_found_and_nothing_else() {
        let world = waterfall();
        let found = landings_on_row(&lookup(&world), IVec3::new(5, 70, 5), 5);
        assert_eq!(found, [IVec3::new(5, 70, 5)]);
        // Other rows and still water have none.
        assert!(landings_on_row(&lookup(&world), IVec3::new(5, 70, 5), 6).is_empty());
        let mut calm = HashMap::new();
        calm.insert((5, 70, 5), fluid::at_level(Fluid::Water, 0));
        assert!(landings_on_row(&lookup(&calm), IVec3::new(5, 70, 5), 5).is_empty());
    }

    #[test]
    fn a_waterfall_never_has_more_than_32_particles_and_only_near_the_viewer() {
        let world = waterfall();
        let block = lookup(&world);
        let mut splashes = Splashes::default();
        let near = DVec3::new(5.5, 72.0, 8.5);
        for t in 0..400 {
            splashes.tick(t, near, &[], &block, &Open);
            let mine = splashes
                .owners
                .iter()
                .filter(|o| **o == Some(IVec3::new(5, 70, 5)))
                .count();
            assert!(mine <= PER_WATERFALL, "{mine} at tick {t}");
        }
        assert!(!splashes.particles().is_empty(), "the fall makes foam");
        // Far away (more than 32 blocks): none, even once it is known.
        let mut far = Splashes::default();
        for t in 0..400 {
            far.tick(t, near + DVec3::new(40.0, 0.0, 0.0), &[], &block, &Open);
        }
        assert!(far.particles().is_empty());
    }

    #[test]
    fn something_falling_fast_into_water_splashes_and_a_slow_one_doesnt() {
        let world = HashMap::new();
        let block = lookup(&world);
        let viewer = DVec3::new(0.0, 64.0, 0.0);
        let body = |t: u64, y: f64, vy: f64, water: bool| {
            (
                t,
                [Body {
                    id: 7,
                    feet: DVec3::new(1.0, y, 1.0),
                    velocity: DVec3::new(0.0, vy, 0.0),
                    in_water: water,
                }],
            )
        };
        let mut fast = Splashes::default();
        for (t, bodies) in [body(0, 70.0, -0.8, false), body(1, 63.0, -0.2, true)] {
            fast.tick(t, viewer, &bodies, &block, &Open);
        }
        assert!(
            fast.particles().len() >= BURST,
            "{}",
            fast.particles().len()
        );
        let mut slow = Splashes::default();
        for (t, bodies) in [body(0, 70.0, -0.1, false), body(1, 63.0, -0.05, true)] {
            slow.tick(t, viewer, &bodies, &block, &Open);
        }
        assert!(slow.particles().is_empty());
        // Already in the water a tick ago: not an entry.
        let mut wading = Splashes::default();
        for (t, bodies) in [body(0, 63.0, -0.9, true), body(1, 62.0, -0.9, true)] {
            wading.tick(t, viewer, &bodies, &block, &Open);
        }
        assert!(wading.particles().is_empty());
    }

    #[test]
    fn the_foam_falls_and_fades() {
        let mut splashes = Splashes::default();
        splashes.splash(DVec3::new(0.0, 64.0, 0.0), 0.8);
        let world = HashMap::new();
        let block = lookup(&world);
        for t in 0..60 {
            splashes.tick(t, DVec3::ZERO, &[], &block, &Open);
        }
        assert!(splashes.particles().is_empty());
    }
}
