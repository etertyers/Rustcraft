//! Arrows, following modern `BowItem` and `AbstractArrow`: launched faster
//! the longer the bow was drawn (3 blocks a tick at full draw, with a
//! little scatter and the shooter's own motion), falling and slowing as
//! they fly, hurting the first mob in their path, and sticking in blocks
//! for a minute until picked up.

use glam::DVec3;

use super::mob::Mob;
use crate::player::collision::Aabb;

/// Ticks to fully draw a bow.
const FULL_DRAW: f32 = 20.0;
/// Weakest draw that shoots.
const MIN_POWER: f32 = 0.1;
/// Launch speed at full power, blocks per tick.
const MAX_SPEED: f64 = 3.0;
/// Per tick in flight.
const GRAVITY: f64 = 0.05;
const DRAG: f64 = 0.99;
const WATER_DRAG: f64 = 0.6;
/// Damage per block per tick of speed (the arrow's base damage, 2.0).
const DAMAGE_PER_SPEED: f64 = 2.0;
/// How much an arrow's direction scatters (modern `inaccuracy` 1.0 times
/// 0.0172275, in a triangle distribution).
const SCATTER: f64 = 0.017_227_5;

/// Mobs are hit within this of their hitbox.
const HIT_BORDER: f64 = 0.3;
/// Ticks a stuck arrow stays before vanishing: a minute.
const STUCK_LIFETIME: u32 = 1200;

/// Bow power for a draw held `ticks` ticks, or `None` if
/// too weak to shoot.
pub fn bow_power(ticks: u32) -> Option<f32> {
    let f = ticks as f32 / FULL_DRAW;
    let power = ((f * f + f * 2.0) / 3.0).min(1.0);
    (power >= MIN_POWER).then_some(power)
}

#[derive(Debug, Clone, PartialEq, serde::Serialize, serde::Deserialize)]
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
    /// Shot by a mob: it hits players and not mobs.
    pub hostile: bool,
    /// What the bow was enchanted with.
    pub power: u8,
    pub punch: u8,
    pub flame: bool,
    /// A stray's: slows the player it hits.
    pub slows: bool,
    /// The player who loosed it, whom it never hits (others it does).
    #[serde(default)]
    pub shooter: Option<String>,
}

/// What a flying arrow did this tick.
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum ArrowEvent {
    /// Hit mob `index` for this much.
    HitMob {
        index: usize,
        damage: f32,
        /// Punch pushes the mob back; Flame sets it alight.
        punch: u8,
        flame: bool,
    },
    /// The arrow hit a player for this much.
    HitPlayer { damage: f32 },
}

impl Arrow {
    /// Shot from `eye` along `forward` (a unit vector) with `power` 0–1,
    /// carrying the shooter's `motion`. `random` gives 0–1.
    pub fn shoot(
        eye: DVec3,
        forward: DVec3,
        power: f32,
        motion: DVec3,
        pickup: bool,
        random: &mut impl FnMut() -> f64,
    ) -> Self {
        Self::shoot_inaccurately(eye, forward, power, motion, pickup, 1.0, random)
    }

    /// `shoot` with the scatter multiplied by `inaccuracy` (a player's bow
    /// is 1, a mob's is 14 less 4 a level of difficulty).
    pub fn shoot_inaccurately(
        eye: DVec3,
        forward: DVec3,
        power: f32,
        motion: DVec3,
        pickup: bool,
        inaccuracy: f64,
        random: &mut impl FnMut() -> f64,
    ) -> Self {
        let mut triangle = || SCATTER * inaccuracy * (random() - random());
        let scattered = (forward + DVec3::new(triangle(), triangle(), triangle())).normalize();
        Self {
            position: eye - DVec3::Y * 0.1,
            prev_position: eye - DVec3::Y * 0.1,
            velocity: scattered * MAX_SPEED * power as f64 + motion,
            heading: scattered,
            stuck: false,
            ticks_stuck: 0,
            critical: power >= 1.0,
            pickup,
            hostile: false,
            power: 0,
            punch: 0,
            flame: false,
            slows: false,
            shooter: None,
        }
    }

    /// Gives it the bow's Power, Punch and Flame.
    pub fn enchanted(mut self, power: u8, punch: u8, flame: bool) -> Self {
        self.power = power;
        self.punch = punch;
        self.flame = flame;
        self
    }

    /// A small box round it, for what it presses.
    pub fn bounds(&self) -> Aabb {
        let half = DVec3::splat(0.25);
        Aabb::new(self.position - half, self.position + half)
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
        player: Option<&Aabb>,
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
        let player_hit =
            player.and_then(|player| player.grow(HIT_BORDER).ray_hit(self.position, dir, reach));
        let target = mobs
            .iter()
            .enumerate()
            .filter(|_| !self.hostile)
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
        // A player in the way first, unless a mob is nearer.
        if let Some(t) = player_hit
            && target.is_none_or(|(_, mob)| t <= mob)
        {
            let damage = if self.hostile {
                (speed * DAMAGE_PER_SPEED).ceil() as f32
            } else {
                self.damage_at(speed, random)
            };
            self.ticks_stuck = STUCK_LIFETIME;
            self.stuck = true;
            return Some(ArrowEvent::HitPlayer { damage });
        }
        if let Some((index, _)) = target {
            let damage = self.damage_at(speed, random);
            // 1.0.0 removes an arrow that hits; it doesn't stick in mobs.
            self.ticks_stuck = STUCK_LIFETIME;
            self.stuck = true;
            return Some(ArrowEvent::HitMob {
                index,
                damage,
                punch: self.punch,
                flame: self.flame,
            });
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

    /// What a hit at `speed` does: power adds half a point and half a
    /// point a level to the base damage (modern `baseDamage += power × 0.5
    /// + 0.5`), and a critical shot adds a random extra.
    fn damage_at(&self, speed: f64, random: &mut impl FnMut() -> f32) -> f32 {
        let bonus = if self.power > 0 {
            f64::from(self.power) * 0.5 + 0.5
        } else {
            0.0
        };
        let mut damage = (speed * (DAMAGE_PER_SPEED + bonus)).ceil() as i32;
        if self.critical {
            damage += (random() * (damage / 2 + 2) as f32) as i32;
        }
        damage as f32
    }

    /// Hits something that isn't a mob (the dragon, a crystal): the arrow
    /// is spent, and the damage is what it did.
    pub fn hit_other(&mut self, random: &mut impl FnMut() -> f32) -> f32 {
        let damage = self.damage_at(self.velocity.length(), random);
        self.ticks_stuck = STUCK_LIFETIME;
        self.stuck = true;
        damage
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

    /// A full-power shot with no scatter and a shooter standing still.
    fn shoot(eye: DVec3, forward: DVec3, power: f32) -> Arrow {
        Arrow::shoot(eye, forward, power, DVec3::ZERO, true, &mut || 0.5)
    }

    #[test]
    fn bow_power_follows_the_draw_curve() {
        assert_eq!(bow_power(1), None, "a tap doesn't shoot");
        assert!((bow_power(10).unwrap() - (0.25 + 1.0) / 3.0).abs() < 1e-6);
        assert_eq!(bow_power(20), Some(1.0));
        assert_eq!(bow_power(60), Some(1.0));
    }

    #[test]
    fn a_full_power_arrow_flies_and_falls_then_sticks_in_a_wall() {
        let mut arrow = shoot(DVec3::new(0.0, 65.0, 0.0), DVec3::NEG_Z, 1.0);
        assert!(arrow.critical);
        let mut random = || 0.0;
        arrow.tick(no_blocks, false, &[], None, &mut random);
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
            arrow.tick(wall, false, &[], None, &mut random);
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
    fn arrows_scatter_a_little_and_carry_the_shooters_motion() {
        let eye = DVec3::new(0.0, 65.0, 0.0);
        let mut high = || 0.0;
        let skewed = Arrow::shoot(eye, DVec3::NEG_Z, 1.0, DVec3::ZERO, true, &mut high);
        // Every roll at the extreme bends the aim by at most a couple of degrees.
        assert!(skewed.heading.angle_between(DVec3::NEG_Z) < 0.04);
        assert!((skewed.velocity.length() - 3.0).abs() < 0.01);
        let mut sequence = [0.1, 0.9, 0.6, 0.2, 0.7, 0.3].into_iter().cycle();
        let mut roll = || sequence.next().unwrap();
        let mixed = Arrow::shoot(
            eye,
            DVec3::NEG_Z,
            0.5,
            DVec3::new(0.1, 0.2, -0.3),
            false,
            &mut roll,
        );
        assert!(!mixed.pickup);
        // Half power is 1.5 a tick before the shooter's motion is added.
        assert!(
            (mixed.velocity.z + 1.5 + 0.3).abs() < 0.05,
            "{}",
            mixed.velocity
        );
        assert!(mixed.velocity.y > 0.15 && mixed.velocity.x > 0.05);
    }

    #[test]
    fn arrows_slow_to_six_tenths_in_water() {
        let mut arrow = shoot(DVec3::new(0.0, 65.0, 0.0), DVec3::NEG_Z, 1.0);
        let mut random = || 0.0;
        arrow.tick(no_blocks, true, &[], None, &mut random);
        assert!((arrow.velocity.z + 3.0 * 0.6).abs() < 1e-9);
    }

    #[test]
    fn a_full_power_arrow_hurts_a_mob_for_6_or_more() {
        let mobs = MobRegistry::load(&crate::assets::dir().join("data/mobs.ron")).unwrap();
        let kind = mobs.find("zombie").unwrap();
        let zombie = Mob::new(kind, mobs.get(kind), DVec3::new(0.0, 64.0, -2.0), 0.0);
        let mut arrow = shoot(DVec3::new(0.0, 65.0, 0.0), DVec3::NEG_Z, 1.0);
        let mut random = || 0.0;
        let event = arrow.tick(
            no_blocks,
            false,
            std::slice::from_ref(&zombie),
            None,
            &mut random,
        );
        assert_eq!(
            event,
            Some(ArrowEvent::HitMob {
                index: 0,
                damage: 6.0,
                punch: 0,
                flame: false
            })
        );
    }
}
