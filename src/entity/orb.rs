//! Experience orbs, as modern Java Edition's `ExperienceOrb`: they fall,
//! float in water, drift toward a player within 8 blocks, merge with
//! orbs of the same size and give their points on touching the player.

use glam::DVec3;

use crate::{
    player::{
        collision::{Aabb, Collider},
        experience,
        physics::{CURRENT_PUSH, sweep},
    },
    world::block::Fluid,
};

/// Edge length of the orb's box.
const SIZE: f64 = 0.5;
const GRAVITY: f64 = 0.03;
const DRAG: f64 = 0.98;
/// Ticks before it disappears: 5 minutes.
const LIFETIME: u32 = 6000;
/// How far away the player pulls it in.
const ATTRACT_RANGE: f64 = 8.0;
/// Orbs this close with the same picture merge into one.
const MERGE_RANGE: f64 = 0.5;

#[derive(Debug, Clone, PartialEq, serde::Serialize, serde::Deserialize)]
pub struct XpOrb {
    pub value: u32,
    bb: Aabb,
    prev_center: DVec3,
    pub velocity: DVec3,
    pub on_ground: bool,
    pub age: u32,
    /// Random phase for the pulsing colour.
    pub phase: f32,
}

impl XpOrb {
    pub fn new(value: u32, center: DVec3, velocity: DVec3, phase: f32) -> Self {
        let half = DVec3::splat(SIZE / 2.0);
        Self {
            value,
            bb: Aabb::new(center - half, center + half),
            prev_center: center,
            velocity,
            on_ground: false,
            age: 0,
            phase,
        }
    }

    pub fn center(&self) -> DVec3 {
        (self.bb.min + self.bb.max) / 2.0
    }

    pub fn bounds(&self) -> Aabb {
        self.bb
    }

    pub fn center_between_ticks(&self, alpha: f64) -> DVec3 {
        self.prev_center.lerp(self.center(), alpha)
    }

    /// Which of the 11 pictures it's drawn with.
    pub fn icon(&self) -> usize {
        experience::orb_icon(self.value)
    }

    pub fn expired(&self) -> bool {
        self.age >= LIFETIME || self.value == 0
    }

    /// Moves it by `by`, as a piston does.
    pub fn shift(&mut self, by: DVec3) {
        self.bb = self.bb.offset(by);
        self.prev_center += by;
    }

    /// Whether `other` is close enough, and the same size, to merge in.
    pub fn can_merge(&self, other: &XpOrb) -> bool {
        self.icon() == other.icon() && self.center().distance(other.center()) <= MERGE_RANGE
    }

    /// One tick: fall (or float), be drawn toward `player` (the point
    /// halfway up the body), slide.
    pub fn tick(&mut self, world: &impl Collider, player: Option<DVec3>) {
        self.prev_center = self.center();
        let region = self.bb.grow(-0.001);
        if world.liquid(&region) == Some(Fluid::Water) {
            self.velocity.x *= 0.99;
            self.velocity.z *= 0.99;
            self.velocity.y = (self.velocity.y + 5.0e-4).min(0.06);
        } else {
            self.velocity.y -= GRAVITY;
        }
        self.velocity += world.current(&region) * CURRENT_PUSH;
        if world.in_lava(&self.bb) {
            // Lava throws it about before it melts away.
            self.velocity = DVec3::new(0.0, 0.2, 0.0) + (self.velocity * 0.5);
        }
        if let Some(target) = player {
            let toward = target - self.center();
            let pull = 1.0 - toward.length() / ATTRACT_RANGE;
            if pull > 0.0 {
                self.velocity += toward.normalize_or_zero() * pull * pull * 0.1;
            }
        }
        let wanted = self.velocity;
        let (moved, bb) = sweep(self.bb, wanted, world, &mut Vec::new());
        self.bb = bb;
        self.on_ground = moved.y != wanted.y && wanted.y < 0.0;
        for axis in 0..3 {
            if moved[axis] != wanted[axis] {
                self.velocity[axis] = 0.0;
            }
        }
        let mut friction = DRAG;
        if self.on_ground {
            let below = (self.center() - DVec3::Y * (SIZE / 2.0 + 0.01)).floor();
            friction =
                world.slipperiness(below.x as i32, below.y as i32, below.z as i32) as f64 * DRAG;
        }
        self.velocity *= DVec3::new(friction, DRAG, friction);
        if self.on_ground {
            self.velocity.y *= -0.9;
        }
        self.age += 1;
    }

    /// Whether the player's box `player` is touching it.
    pub fn touches(&self, player: &Aabb) -> bool {
        self.bb.intersects(player)
    }
}

/// Adds `orb` to `orbs`, joining one of the same size close by.
pub fn spawn(orbs: &mut Vec<XpOrb>, orb: XpOrb) {
    match orbs.iter_mut().find(|other| other.can_merge(&orb)) {
        Some(other) => {
            other.value += orb.value;
            other.age = other.age.min(orb.age);
        }
        None => orbs.push(orb),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{
        inventory::stack::Items,
        player::collision::WorldCollider,
        world::{World, chunk::ChunkPos, worldgen::flat::flat_chunk},
    };

    fn flat() -> (World, Items) {
        let mut world = World::default();
        for z in -1..=1 {
            for x in -1..=1 {
                world.insert(flat_chunk(ChunkPos::new(x, z)));
            }
        }
        (world, Items::new())
    }

    fn run(world: &World, items: &Items, orb: &mut XpOrb, player: Option<DVec3>, ticks: u32) {
        let collider = WorldCollider {
            world,
            blocks: items.blocks,
        };
        for _ in 0..ticks {
            orb.tick(&collider, player);
        }
    }

    #[test]
    fn an_orb_falls_and_rests_on_the_ground() {
        let (world, items) = flat();
        let mut orb = XpOrb::new(5, DVec3::new(0.5, 70.0, 0.5), DVec3::ZERO, 0.0);
        run(&world, &items, &mut orb, None, 80);
        // The ground is the top of y 63; the box is 0.5 tall.
        assert!(
            (orb.bounds().min.y - 64.0).abs() < 0.05,
            "{:?}",
            orb.bounds()
        );
        assert!(orb.velocity.length() < 0.1);
    }

    #[test]
    fn a_player_within_eight_blocks_pulls_it_in() {
        let (world, items) = flat();
        let mut orb = XpOrb::new(5, DVec3::new(5.5, 64.25, 0.5), DVec3::ZERO, 0.0);
        let player = DVec3::new(0.5, 64.9, 0.5);
        let start = orb.center().distance(player);
        run(&world, &items, &mut orb, Some(player), 30);
        assert!(orb.center().distance(player) < start - 0.5);
        // The pull grows as it nears, so it arrives in a few seconds.
        run(&world, &items, &mut orb, Some(player), 120);
        assert!(orb.center().distance(player) < 1.0, "{}", orb.center());
        // Outside the range nothing happens.
        let mut far = XpOrb::new(5, DVec3::new(20.5, 64.25, 0.5), DVec3::ZERO, 0.0);
        run(&world, &items, &mut far, Some(player), 30);
        assert!((far.center().x - 20.5).abs() < 0.01);
    }

    #[test]
    fn orbs_of_one_size_nearby_merge() {
        let mut orbs = Vec::new();
        let at = DVec3::new(0.0, 64.0, 0.0);
        spawn(&mut orbs, XpOrb::new(3, at, DVec3::ZERO, 0.0));
        spawn(
            &mut orbs,
            XpOrb::new(4, at + DVec3::X * 0.3, DVec3::ZERO, 0.0),
        );
        assert_eq!(orbs.len(), 1);
        assert_eq!(orbs[0].value, 7);
        spawn(&mut orbs, XpOrb::new(1, at, DVec3::ZERO, 0.0));
        assert_eq!(orbs.len(), 2, "a different size stays apart");
        spawn(
            &mut orbs,
            XpOrb::new(3, at + DVec3::X * 3.0, DVec3::ZERO, 0.0),
        );
        assert_eq!(orbs.len(), 3, "too far to merge");
    }

    #[test]
    fn it_expires_after_five_minutes() {
        let (world, items) = flat();
        let mut orb = XpOrb::new(1, DVec3::new(0.5, 64.25, 0.5), DVec3::ZERO, 0.0);
        run(&world, &items, &mut orb, None, LIFETIME - 1);
        assert!(!orb.expired());
        run(&world, &items, &mut orb, None, 1);
        assert!(orb.expired());
    }
}
