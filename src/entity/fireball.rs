//! Ghast fireballs (modern `LargeFireball` and `AbstractHurtingProjectile`):
//! pushed along a fixed heading a little harder each tick, with no gravity,
//! until they hit something and burst into a small explosion that starts
//! fires. A player's hit sends one back the way they look. Blazes' small
//! fireballs (M25, modern `SmallFireball`) fly the same way, but only set
//! what they hit alight.

use glam::DVec3;

use super::mob::Mob;
use crate::player::collision::Aabb;

/// Speed added along the heading each tick, and the share of speed kept
/// (modern `accelerationPower` 0.1 and `getInertia` 0.95).
const PUSH: f64 = 0.1;
const INERTIA: f64 = 0.95;
/// Half its size (modern 1 × 1).
const HALF: f64 = 0.5;
/// Damage from a direct hit, before the burst (modern 6).
pub const HIT_DAMAGE: f32 = 6.0;
/// A small fireball's hit, and the burning it gives (modern 5 and 5 s).
pub const SMALL_HIT_DAMAGE: f32 = 5.0;
pub const SMALL_FIRE_TICKS: i32 = 100;
/// Half a small fireball's size (modern 0.3125).
const SMALL_HALF: f64 = 0.156_25;
/// The burst's power (a ghast's `explosionPower` 1).
pub const BURST_POWER: f32 = 1.0;
/// Ticks one flies before burning out.
const LIFETIME: u32 = 600;

#[derive(Debug, Clone, PartialEq, serde::Serialize, serde::Deserialize)]
pub struct Fireball {
    pub position: DVec3,
    prev_position: DVec3,
    pub velocity: DVec3,
    /// Unit vector it's pushed along.
    pub heading: DVec3,
    /// Sent back by the player: it hurts mobs, the ghast included, and
    /// not the player.
    pub returned: bool,
    /// A blaze's: sets alight instead of bursting.
    pub small: bool,
    age: u32,
}

/// What a fireball ran into this tick; each ends it with a burst there.
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum FireballHit {
    Block(DVec3),
    Player(DVec3),
    Mob {
        index: usize,
        at: DVec3,
    },
    /// Flew too long and went out.
    Spent,
}

impl Fireball {
    /// From `from` toward `at`.
    pub fn toward(from: DVec3, at: DVec3) -> Self {
        let heading = (at - from).normalize_or_zero();
        Self {
            position: from,
            prev_position: from,
            velocity: DVec3::ZERO,
            heading,
            returned: false,
            small: false,
            age: 0,
        }
    }

    /// A blaze's small fireball.
    pub fn small(from: DVec3, at: DVec3) -> Self {
        Self {
            small: true,
            ..Self::toward(from, at)
        }
    }

    fn half(&self) -> f64 {
        if self.small { SMALL_HALF } else { HALF }
    }

    pub fn bounds(&self) -> Aabb {
        Aabb::new(
            self.position - DVec3::splat(self.half()),
            self.position + DVec3::splat(self.half()),
        )
    }

    pub fn position_between_ticks(&self, alpha: f64) -> DVec3 {
        self.prev_position.lerp(self.position, alpha)
    }

    /// A player's hit turns it back along where they look (modern
    /// `hurt` on a hurting projectile).
    pub fn send_back(&mut self, look: DVec3) {
        self.heading = look.normalize_or_zero();
        self.velocity = self.heading * self.velocity.length().max(PUSH);
        self.returned = true;
    }

    /// One tick of flight. `block_hit` gives the distance along a ray to
    /// the first solid block; the player is hit unless it was sent back,
    /// mobs (the ghasts too) only once it was.
    pub fn tick(
        &mut self,
        block_hit: impl Fn(DVec3, DVec3, f64) -> Option<f64>,
        mobs: &[Mob],
        player: Option<&Aabb>,
    ) -> Option<FireballHit> {
        self.prev_position = self.position;
        self.age += 1;
        if self.age > LIFETIME {
            return Some(FireballHit::Spent);
        }
        let speed = self.velocity.length();
        if speed > 1e-9 {
            let dir = self.velocity / speed;
            let wall = block_hit(self.position, dir, speed);
            let reach = wall.unwrap_or(speed);
            if !self.returned
                && let Some(player) = player
                && let Some(t) = player.grow(self.half()).ray_hit(self.position, dir, reach)
            {
                return Some(FireballHit::Player(self.position + dir * t));
            }
            if self.returned {
                let hit = mobs
                    .iter()
                    .enumerate()
                    .filter(|(_, mob)| !mob.is_dead())
                    .filter_map(|(i, mob)| {
                        let t =
                            mob.body
                                .aabb()
                                .grow(self.half())
                                .ray_hit(self.position, dir, reach)?;
                        Some((i, t))
                    })
                    .min_by(|a, b| a.1.total_cmp(&b.1));
                if let Some((index, t)) = hit {
                    let at = self.position + dir * t;
                    return Some(FireballHit::Mob { index, at });
                }
            }
            if let Some(t) = wall {
                return Some(FireballHit::Block(self.position + dir * t));
            }
            self.position += self.velocity;
        }
        self.velocity = (self.velocity + self.heading * PUSH) * INERTIA;
        None
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn no_blocks(_: DVec3, _: DVec3, _: f64) -> Option<f64> {
        None
    }

    #[test]
    fn a_fireball_speeds_up_toward_its_target_without_falling() {
        let mut ball = Fireball::toward(DVec3::new(0.0, 80.0, 0.0), DVec3::new(0.0, 80.0, -100.0));
        for _ in 0..100 {
            assert_eq!(ball.tick(no_blocks, &[], None), None);
        }
        assert_eq!(ball.position.y, 80.0, "no gravity");
        // Top speed: 0.1 × 0.95 / 0.05 = 1.9 blocks a tick.
        assert!((ball.velocity.length() - 1.9).abs() < 0.02);
        assert!(ball.position.z < -100.0);
    }

    #[test]
    fn it_bursts_on_a_wall_or_the_player_and_a_hit_sends_it_back() {
        let wall = |from: DVec3, dir: DVec3, reach: f64| {
            Aabb::new(DVec3::new(-5.0, 0.0, -21.0), DVec3::new(5.0, 100.0, -20.0))
                .ray_hit(from, dir, reach)
        };
        let mut ball = Fireball::toward(DVec3::new(0.0, 80.0, 0.0), DVec3::new(0.0, 80.0, -30.0));
        let hit = (0..100).find_map(|_| ball.tick(wall, &[], None));
        assert!(matches!(hit, Some(FireballHit::Block(at)) if (at.z + 20.0).abs() < 1e-6));

        let player = Aabb::new(DVec3::new(-0.3, 80.0, -10.3), DVec3::new(0.3, 81.8, -9.7));
        let mut ball = Fireball::toward(DVec3::new(0.0, 80.5, 0.0), DVec3::new(0.0, 80.5, -10.0));
        let hit = (0..100).find_map(|_| ball.tick(no_blocks, &[], Some(&player)));
        assert!(matches!(hit, Some(FireballHit::Player(_))));

        // Sent back, it flies past the player.
        let mut ball = Fireball::toward(DVec3::new(0.0, 80.5, 0.0), DVec3::new(0.0, 80.5, -10.0));
        for _ in 0..3 {
            ball.tick(no_blocks, &[], None);
        }
        ball.send_back(DVec3::Z);
        assert!(ball.returned);
        let hit = (0..20).find_map(|_| ball.tick(no_blocks, &[], Some(&player)));
        assert_eq!(hit, None);
        assert!(ball.position.z > 0.0);
    }
}
