//! What the dragon throws and leaves (M27): fireballs that burst into
//! clouds of dragon's breath, and the clouds, which hurt whatever stands in
//! them (modern `DragonFireball` and `AreaEffectCloud`).

use glam::DVec3;

use super::mob::Mob;
use crate::player::collision::Aabb;

/// A fireball's half size, and how long it flies before it fizzles.
const HALF: f64 = 0.5;
const LIFE: u32 = 400;
/// A cloud from a fireball: radius, how long it lasts; the cloud the
/// dragon breathes while it sits.
pub const FIREBALL_CLOUD: (f32, u32) = (3.0, 600);
pub const BREATH_CLOUD: (f32, u32) = (5.0, 200);
/// Every cloud grows toward this radius over its life.
const FULL_RADIUS: f32 = 7.0;
/// Ticks between the harm a cloud does, and how much (modern's instant
/// damage II every 20 ticks).
pub const PULSE: u32 = 20;
pub const HARM: f32 = 12.0;
/// How high a cloud reaches above the spot it was laid, and below it.
const REACH_UP: f64 = 1.5;
const REACH_DOWN: f64 = 0.5;

#[derive(Debug, Clone, PartialEq)]
pub struct DragonFireball {
    pub position: DVec3,
    prev_position: DVec3,
    pub velocity: DVec3,
    age: u32,
}

impl DragonFireball {
    pub fn new(from: DVec3, velocity: DVec3) -> Self {
        Self {
            position: from,
            prev_position: from,
            velocity,
            age: 0,
        }
    }

    pub fn position_between_ticks(&self, alpha: f64) -> DVec3 {
        self.prev_position.lerp(self.position, alpha)
    }

    /// One tick of flight; `Some(point)` where it bursts: on a block, the
    /// player, a mob, or at the end of its life.
    pub fn tick(
        &mut self,
        block_hit: impl Fn(DVec3, DVec3, f64) -> Option<f64>,
        mobs: &[Mob],
        player: Option<&Aabb>,
    ) -> Option<DVec3> {
        self.prev_position = self.position;
        self.age += 1;
        let speed = self.velocity.length();
        if speed > 1e-9 {
            let dir = self.velocity / speed;
            let wall = block_hit(self.position, dir, speed);
            let living = mobs
                .iter()
                .filter(|m| !m.is_dead())
                .filter_map(|m| m.body.aabb().grow(HALF).ray_hit(self.position, dir, speed))
                .chain(player.and_then(|p| p.grow(HALF).ray_hit(self.position, dir, speed)))
                .fold(f64::MAX, f64::min);
            let reach = wall.unwrap_or(f64::MAX).min(living);
            if reach < f64::MAX {
                return Some(self.position + dir * reach);
            }
            self.position += self.velocity;
        }
        (self.age > LIFE).then_some(self.position)
    }
}

/// A cloud of dragon's breath lying on the ground.
#[derive(Debug, Clone, PartialEq)]
pub struct BreathCloud {
    pub center: DVec3,
    pub age: u32,
    radius: f32,
    grow: f32,
    lifetime: u32,
}

impl BreathCloud {
    pub fn new(center: DVec3, (radius, lifetime): (f32, u32)) -> Self {
        Self {
            center,
            age: 0,
            radius,
            grow: (FULL_RADIUS - radius) / lifetime as f32,
            lifetime,
        }
    }

    pub fn radius(&self) -> f32 {
        self.radius
    }

    pub fn expired(&self) -> bool {
        self.age >= self.lifetime
    }

    pub fn tick(&mut self) {
        self.age += 1;
        self.radius += self.grow;
    }

    /// Whether this tick is one it harms what it holds.
    pub fn pulses(&self) -> bool {
        self.age > 0 && self.age.is_multiple_of(PULSE)
    }

    /// Whether any of `body` is in it: over a disc `radius` wide, from half
    /// a block under to a block and a half over where it lies.
    pub fn holds(&self, body: &Aabb) -> bool {
        if body.max.y < self.center.y - REACH_DOWN || body.min.y > self.center.y + REACH_UP {
            return false;
        }
        let nearest = DVec3::new(
            self.center.x.clamp(body.min.x, body.max.x),
            0.0,
            self.center.z.clamp(body.min.z, body.max.z),
        );
        let d = (nearest.x - self.center.x).hypot(nearest.z - self.center.z);
        d <= f64::from(self.radius)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn nothing(_: DVec3, _: DVec3, _: f64) -> Option<f64> {
        None
    }

    #[test]
    fn a_fireball_flies_straight_and_bursts_on_a_wall() {
        let mut ball = DragonFireball::new(DVec3::new(0.0, 80.0, 0.0), DVec3::new(0.0, 0.0, -1.0));
        let wall = |from: DVec3, dir: DVec3, reach: f64| {
            Aabb::new(DVec3::new(-5.0, 70.0, -30.0), DVec3::new(5.0, 90.0, -20.0))
                .ray_hit(from, dir, reach)
        };
        let burst = (0..100).find_map(|_| ball.tick(wall, &[], None)).unwrap();
        assert!((burst.z + 20.0).abs() < 1e-6, "{burst}");
        assert_eq!(burst.y, 80.0);
    }

    #[test]
    fn a_fireball_bursts_on_the_player_and_fizzles_in_the_end() {
        let body = Aabb::new(DVec3::new(-0.3, 78.0, -10.3), DVec3::new(0.3, 79.8, -9.7));
        let mut ball = DragonFireball::new(DVec3::new(0.0, 79.0, 0.0), DVec3::new(0.0, 0.0, -0.9));
        let burst = (0..100)
            .find_map(|_| ball.tick(nothing, &[], Some(&body)))
            .unwrap();
        assert!(burst.z < -9.0 && burst.z > -10.5, "{burst}");
        let mut lost = DragonFireball::new(DVec3::ZERO, DVec3::new(0.0, 0.0, 0.0));
        assert!((0..500).any(|_| lost.tick(nothing, &[], None).is_some()));
    }

    #[test]
    fn a_cloud_grows_toward_seven_blocks_and_fades_after_its_time() {
        let mut cloud = BreathCloud::new(DVec3::ZERO, FIREBALL_CLOUD);
        assert_eq!(cloud.radius(), 3.0);
        for _ in 0..FIREBALL_CLOUD.1 {
            assert!(!cloud.expired());
            cloud.tick();
        }
        assert!(cloud.expired());
        assert!((cloud.radius() - 7.0).abs() < 0.01);
        let breath = BreathCloud::new(DVec3::ZERO, BREATH_CLOUD);
        assert_eq!(breath.radius(), 5.0);
    }

    #[test]
    fn a_cloud_holds_what_stands_in_its_disc_and_pulses_each_second() {
        let cloud = BreathCloud::new(DVec3::new(10.0, 64.0, 10.0), FIREBALL_CLOUD);
        let at = |x: f64, y: f64| {
            Aabb::new(
                DVec3::new(x - 0.3, y, 10.0 - 0.3),
                DVec3::new(x + 0.3, y + 1.8, 10.3),
            )
        };
        assert!(cloud.holds(&at(10.0, 64.0)));
        assert!(cloud.holds(&at(12.5, 64.0)));
        assert!(!cloud.holds(&at(14.0, 64.0)), "a block outside");
        assert!(!cloud.holds(&at(10.0, 70.0)), "flying over");
        assert!(cloud.holds(&at(10.0, 62.5)), "standing a little low");
        let mut c = cloud;
        let pulses = (0..60)
            .filter(|_| {
                c.tick();
                c.pulses()
            })
            .count();
        assert_eq!(pulses, 3);
    }
}
