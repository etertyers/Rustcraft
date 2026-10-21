//! Thrown splash potions (M26, modern `ThrownPotion`): an arc under
//! gravity until they hit a block or something living, then they burst
//! over everyone near.

use glam::DVec3;

use super::mob::Mob;
use crate::{inventory::stack::ItemStack, player::collision::Aabb};

/// Per tick: gravity and drag (modern 0.05 and 0.99).
const GRAVITY: f64 = 0.05;
const DRAG: f64 = 0.99;
/// Thrown at this speed, aimed this far above the look (modern 0.5 and
/// −20°).
const SPEED: f64 = 0.5;
const LIFT_DEGREES: f64 = 20.0;
/// Half its size.
const HALF: f64 = 0.125;
/// Ticks before it can hit the one who threw it.
const OWNER_GRACE: u32 = 5;

#[derive(Debug, Clone, PartialEq)]
pub struct ThrownPotion {
    pub stack: ItemStack,
    pub position: DVec3,
    prev_position: DVec3,
    pub velocity: DVec3,
    age: u32,
}

impl ThrownPotion {
    /// Thrown from `eye` looking along `yaw` and `pitch` (radians, the
    /// camera's), carrying the thrower's `motion`.
    pub fn throw(stack: ItemStack, eye: DVec3, yaw: f32, pitch: f32, motion: DVec3) -> Self {
        let pitch = f64::from(pitch) + LIFT_DEGREES.to_radians();
        let yaw = f64::from(yaw);
        let dir = DVec3::new(
            yaw.sin() * pitch.cos(),
            pitch.sin(),
            -yaw.cos() * pitch.cos(),
        );
        Self {
            stack,
            position: eye,
            prev_position: eye,
            velocity: dir * SPEED + motion.with_y(0.0),
            age: 0,
        }
    }

    pub fn position_between_ticks(&self, alpha: f64) -> DVec3 {
        self.prev_position.lerp(self.position, alpha)
    }

    /// One tick of flight; `Some(point)` once it breaks there.
    pub fn tick(
        &mut self,
        block_hit: impl Fn(DVec3, DVec3, f64) -> Option<f64>,
        mobs: &[Mob],
        player: &Aabb,
    ) -> Option<DVec3> {
        self.prev_position = self.position;
        self.age += 1;
        let speed = self.velocity.length();
        if speed > 1e-9 {
            let dir = self.velocity / speed;
            let mut reach = block_hit(self.position, dir, speed).unwrap_or(f64::MAX);
            let wall = reach < f64::MAX;
            let mobs_hit = mobs
                .iter()
                .filter(|m| !m.is_dead())
                .filter_map(|m| m.body.aabb().grow(HALF).ray_hit(self.position, dir, speed));
            let players_hit = (self.age > OWNER_GRACE)
                .then(|| player.grow(HALF).ray_hit(self.position, dir, speed))
                .flatten();
            let living = mobs_hit.chain(players_hit).fold(f64::MAX, f64::min);
            if living < reach || wall {
                reach = reach.min(living);
                return Some(self.position + dir * reach);
            }
            self.position += self.velocity;
        }
        self.velocity = self.velocity * DRAG - DVec3::Y * GRAVITY;
        None
    }
}

/// How strongly a splash at `at` reaches a body: 1 at its middle, falling
/// to nothing four blocks out (modern `applySplash`), if it reaches at all.
pub fn splash_strength(at: DVec3, body: &Aabb) -> Option<f64> {
    let around = DVec3::new(4.0, 2.0, 4.0);
    if !Aabb::new(body.min - around, body.max + around).contains(at) {
        return None;
    }
    let middle = (body.min + body.max) / 2.0;
    let d2 = middle.distance_squared(at);
    (d2 < 16.0).then(|| 1.0 - d2.sqrt() / 4.0)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::inventory::items;

    #[test]
    fn a_thrown_potion_arcs_and_breaks_on_the_ground_or_a_body() {
        let stack = ItemStack::new(items::SPLASH_POTION, 1);
        let ground = |from: DVec3, dir: DVec3, reach: f64| {
            Aabb::new(DVec3::new(-50.0, 0.0, -50.0), DVec3::new(50.0, 64.0, 50.0))
                .ray_hit(from, dir, reach)
        };
        let far = Aabb::new(DVec3::splat(1000.0), DVec3::splat(1001.0));
        let mut potion =
            ThrownPotion::throw(stack, DVec3::new(0.0, 65.62, 0.0), 0.0, 0.0, DVec3::ZERO);
        let landed = (0..100)
            .find_map(|_| potion.tick(ground, &[], &far))
            .unwrap();
        assert!((landed.y - 64.0).abs() < 1e-6);
        assert!(
            landed.z < -2.0 && landed.z > -6.0,
            "a few blocks ahead: {landed}"
        );
        // Right on someone: full strength; four blocks off: nothing.
        let body = Aabb::new(DVec3::new(-0.3, 64.0, -0.3), DVec3::new(0.3, 65.8, 0.3));
        assert!(splash_strength(DVec3::new(0.0, 64.9, 0.0), &body).unwrap() > 0.99);
        assert!(splash_strength(DVec3::new(4.5, 64.9, 0.0), &body).is_none());
    }
}
