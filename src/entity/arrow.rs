//! Arrows, following 1.0.0's `ItemBow` and `EntityArrow`: launched faster
//! the longer the bow was drawn, falling and slowing as they fly, hurting
//! the first mob in their path, and sticking in blocks until picked up.

use glam::DVec3;

use super::mob::Mob;
use crate::player::collision::Aabb;

/// Ticks to fully draw a bow.
const FULL_DRAW: f32 = 20.0;
/// Weakest draw that shoots.
const MIN_POWER: f32 = 0.1;
/// Launch speed at full power, blocks per tick (1.0.0: power × 2 × 1.5).
const MAX_SPEED: f64 = 3.0;
/// Per tick in flight.
const GRAVITY: f64 = 0.05;
const DRAG: f64 = 0.99;
const WATER_DRAG: f64 = 0.8;
/// Damage per block per tick of speed (1.0.0's `damage` 2.0).
const DAMAGE_PER_SPEED: f64 = 2.0;
/// Mobs are hit within this of their hitbox (1.0.0 grows it by 0.3).
const HIT_BORDER: f64 = 0.3;
/// Ticks a stuck arrow stays before vanishing (1.0.0: a minute).
const STUCK_LIFETIME: u32 = 1200;

/// Bow power for a draw held `ticks` ticks (1.0.0's curve), or `None` if
/// too weak to shoot.
pub fn bow_power(ticks: u32) -> Option<f32> {
    let f = ticks as f32 / FULL_DRAW;
    let power = ((f * f + f * 2.0) / 3.0).min(1.0);
    (power >= MIN_POWER).then_some(power)
}

#[derive(Debug, Clone, PartialEq)]
pub struct Arrow {
    pub position: DVec3,
    prev_position: DVec3,
    pub velocity: DVec3,
    /// Direction it points (kept once stuck).
    pub heading: DVec3,
    /// Stuck in a block.
    pub stuck: bool,
    ticks_stuck: u32,
    /// A full-power shot, which hits harder.
    pub critical: bool,
    /// Whether it can be picked back up (not from creative).
    pub pickup: bool,
}

/// What a flying arrow did this tick.
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum ArrowEvent {
    /// Hit mob `index` for this much.
    HitMob { index: usize, damage: f32 },
}

impl Arrow {
    /// Shot from `eye` along `forward` (a unit vector) with `power` 0–1.
    pub fn shoot(eye: DVec3, forward: DVec3, power: f32, pickup: bool) -> Self {
        Self {
            position: eye - DVec3::Y * 0.1,
            prev_position: eye - DVec3::Y * 0.1,
            velocity: forward * MAX_SPEED * power as f64,
            heading: forward,
            stuck: false,
            ticks_stuck: 0,
            critical: power >= 1.0,
            pickup,
        }
    }

    pub fn position_between_ticks(&self, alpha: f64) -> DVec3 {
        self.prev_position.lerp(self.position, alpha)
    }

    pub fn expired(&self) -> bool {
        self.ticks_stuck >= STUCK_LIFETIME
    }

    /// One tick: fly, sticking at the first solid block (`block_hit` gives
    /// the distance along a ray to one) or hitting the first living mob in
    /// the way. `random` rolls the critical bonus.
    pub fn tick(
        &mut self,
        block_hit: impl Fn(DVec3, DVec3, f64) -> Option<f64>,
        in_water: bool,
        mobs: &[Mob],
        random: &mut impl FnMut() -> f32,
    ) -> Option<ArrowEvent> {
        self.prev_position = self.position;
        if self.stuck {
            self.ticks_stuck += 1;
            return None;
        }
        let speed = self.velocity.length();
        if speed < 1e-9 {
            return None;
        }
        let dir = self.velocity / speed;
        self.heading = dir;
        let wall = block_hit(self.position, dir, speed);
        let reach = wall.unwrap_or(speed);
        let target = mobs
            .iter()
            .enumerate()
            .filter(|(_, mob)| !mob.is_dead())
            .filter_map(|(i, mob)| {
                let t = mob
                    .body
                    .aabb()
                    .grow(HIT_BORDER)
                    .ray_hit(self.position, dir, reach)?;
                Some((i, t))
            })
            .min_by(|a, b| a.1.total_cmp(&b.1));
        if let Some((index, _)) = target {
            let mut damage = (speed * DAMAGE_PER_SPEED).ceil() as i32;
            if self.critical {
                damage += (random() * (damage / 2 + 2) as f32) as i32;
            }
            let damage = damage as f32;
            // 1.0.0 removes an arrow that hits; it doesn't stick in mobs.
            self.ticks_stuck = STUCK_LIFETIME;
            self.stuck = true;
            return Some(ArrowEvent::HitMob { index, damage });
        }
        if let Some(t) = wall {
            self.position += dir * t;
            self.velocity = DVec3::ZERO;
            self.stuck = true;
            return None;
        }
        self.position += self.velocity;
        let drag = if in_water { WATER_DRAG } else { DRAG };
        self.velocity *= drag;
        self.velocity.y -= GRAVITY;
        None
    }

    /// Whether the player's box reaches a stuck arrow they may pick up.
    pub fn can_pick_up(&self, player: &Aabb) -> bool {
        self.pickup && self.stuck && !self.expired() && player.grow(1.0).contains(self.position)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::entity::mobs::MobRegistry;

    fn no_blocks(_: DVec3, _: DVec3, _: f64) -> Option<f64> {
        None
    }

    #[test]
    fn bow_power_follows_1_0_0() {
        assert_eq!(bow_power(1), None, "a tap doesn't shoot");
        assert!((bow_power(10).unwrap() - (0.25 + 1.0) / 3.0).abs() < 1e-6);
        assert_eq!(bow_power(20), Some(1.0));
        assert_eq!(bow_power(60), Some(1.0));
    }

    #[test]
    fn a_full_power_arrow_flies_and_falls_then_sticks_in_a_wall() {
        let mut arrow = Arrow::shoot(DVec3::new(0.0, 65.0, 0.0), DVec3::NEG_Z, 1.0, true);
        assert!(arrow.critical);
        let mut random = || 0.0;
        arrow.tick(no_blocks, false, &[], &mut random);
        assert!(
            (arrow.position.z + 3.0).abs() < 1e-9,
            "3 blocks the first tick"
        );
        assert!(arrow.velocity.y < 0.0, "starts falling");
        // A wall 10 blocks away.
        let wall = |from: DVec3, dir: DVec3, reach: f64| {
            Aabb::new(DVec3::new(-5.0, 0.0, -11.0), DVec3::new(5.0, 100.0, -10.0))
                .ray_hit(from, dir, reach)
        };
        for _ in 0..10 {
            arrow.tick(wall, false, &[], &mut random);
        }
        assert!(arrow.stuck);
        assert!(
            (arrow.position.z + 10.0).abs() < 1e-9,
            "stuck at the wall face: {}",
            arrow.position
        );
        let player = crate::player::physics::sized_hitbox(
            arrow.position - DVec3::new(0.0, 1.0, -0.5),
            0.6,
            1.8,
        );
        assert!(arrow.can_pick_up(&player));
    }

    #[test]
    fn a_full_power_arrow_hurts_a_mob_for_6_or_more() {
        let mobs = MobRegistry::load(&crate::assets::dir().join("data/mobs.ron")).unwrap();
        let kind = mobs.find("zombie").unwrap();
        let zombie = Mob::new(kind, mobs.get(kind), DVec3::new(0.0, 64.0, -2.0), 0.0);
        let mut arrow = Arrow::shoot(DVec3::new(0.0, 65.0, 0.0), DVec3::NEG_Z, 1.0, true);
        let mut random = || 0.0;
        let event = arrow.tick(no_blocks, false, std::slice::from_ref(&zombie), &mut random);
        assert_eq!(
            event,
            Some(ArrowEvent::HitMob {
                index: 0,
                damage: 6.0
            })
        );
    }
}
