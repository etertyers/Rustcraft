//! A shulker's bullet (M28, modern `ShulkerBullet`): a slow homing spark
//! that follows its target, hurts a little, and lifts whoever it hits.

use glam::DVec3;

use crate::player::collision::Aabb;

/// Blocks a tick, and how quickly it turns toward its target.
const SPEED: f64 = 0.15;
const TURN: f64 = 0.1;
/// Half its size, ticks it flies before it fizzles, damage, and the
/// levitation it gives (ticks).
const HALF: f64 = 0.15;
const LIFE: u32 = 200;
pub const DAMAGE: f32 = 4.0;
pub const LEVITATION_TICKS: u32 = 200;

#[derive(Debug, Clone, PartialEq, serde::Serialize, serde::Deserialize)]
pub struct ShulkerBullet {
    pub position: DVec3,
    prev_position: DVec3,
    velocity: DVec3,
    age: u32,
}

/// What a bullet did this tick.
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum BulletEvent {
    HitPlayer,
    HitBlock,
    Fizzled,
}

impl ShulkerBullet {
    pub fn new(from: DVec3, toward: DVec3) -> Self {
        let dir = (toward - from).normalize_or_zero();
        Self {
            position: from,
            prev_position: from,
            velocity: dir * SPEED,
            age: 0,
        }
    }

    pub fn position_between_ticks(&self, alpha: f64) -> DVec3 {
        self.prev_position.lerp(self.position, alpha)
    }

    /// One tick, bending toward `target` (the player's middle, if there is
    /// one to follow).
    pub fn tick(
        &mut self,
        target: Option<DVec3>,
        player: Option<&Aabb>,
        block_hit: impl Fn(DVec3, DVec3, f64) -> Option<f64>,
    ) -> Option<BulletEvent> {
        self.prev_position = self.position;
        self.age += 1;
        if let Some(target) = target {
            let want = (target - self.position).normalize_or_zero() * SPEED;
            self.velocity += (want - self.velocity) * TURN;
            let speed = self.velocity.length();
            if speed > 1e-9 {
                self.velocity *= SPEED / speed;
            }
        }
        let speed = self.velocity.length();
        if speed > 1e-9 {
            let dir = self.velocity / speed;
            if let Some(player) = player
                && player
                    .grow(HALF)
                    .ray_hit(self.position, dir, speed)
                    .is_some()
            {
                return Some(BulletEvent::HitPlayer);
            }
            if block_hit(self.position, dir, speed).is_some() {
                return Some(BulletEvent::HitBlock);
            }
            self.position += self.velocity;
        }
        (self.age > LIFE).then_some(BulletEvent::Fizzled)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn nothing(_: DVec3, _: DVec3, _: f64) -> Option<f64> {
        None
    }

    #[test]
    fn a_bullet_creeps_toward_and_follows_a_moving_target_and_hits_it() {
        let mut bullet =
            ShulkerBullet::new(DVec3::new(0.0, 65.0, 0.0), DVec3::new(10.0, 65.0, 0.0));
        let mut target = DVec3::new(10.0, 65.0, 0.0);
        let mut hit = None;
        for tick in 0..400 {
            // The target walks off sideways: the bullet bends after it.
            target.z += 0.05;
            let body = Aabb::new(
                target - DVec3::new(0.3, 1.0, 0.3),
                target + DVec3::new(0.3, 0.8, 0.3),
            );
            if let Some(event) = bullet.tick(Some(target), Some(&body), nothing) {
                hit = Some((tick, event));
                break;
            }
        }
        let (tick, event) = hit.unwrap();
        assert_eq!(event, BulletEvent::HitPlayer);
        // 15 blocks at 0.15 a tick takes about a hundred ticks.
        assert!((50..200).contains(&tick), "{tick}");
        assert!(bullet.position.z > 1.0, "it followed: {}", bullet.position);
    }

    #[test]
    fn it_bursts_on_a_wall_and_fizzles_without_a_target() {
        let wall = |from: DVec3, dir: DVec3, reach: f64| {
            Aabb::new(DVec3::new(4.0, 60.0, -5.0), DVec3::new(5.0, 70.0, 5.0))
                .ray_hit(from, dir, reach)
        };
        let mut bullet =
            ShulkerBullet::new(DVec3::new(0.0, 65.0, 0.0), DVec3::new(10.0, 65.0, 0.0));
        let event = (0..300).find_map(|_| bullet.tick(None, None, wall));
        assert_eq!(event, Some(BulletEvent::HitBlock));
        let mut lost = ShulkerBullet::new(DVec3::ZERO, DVec3::Y);
        let event = (0..300).find_map(|_| lost.tick(None, None, nothing));
        assert_eq!(event, Some(BulletEvent::Fizzled));
    }
}
