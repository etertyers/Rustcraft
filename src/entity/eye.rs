//! A thrown eye of ender (modern `EyeOfEnder`): it floats toward the
//! nearest stronghold, rising as it goes, and after four seconds either
//! drops as an item or breaks (one time in five).

use glam::DVec3;

/// Ticks it flies before it drops or breaks.
pub const LIFE: u32 = 80;
/// How far toward the stronghold one throw goes at most, and how high
/// above the thrower it climbs when the stronghold is farther than that.
const REACH: f64 = 12.0;
const CLIMB: f64 = 8.0;
/// Horizontal speed in blocks a tick: a tenth of the way left to go,
/// within these.
const SPEED: (f64, f64) = (0.05, 0.25);
/// How quickly the vertical speed eases toward a block a tick up or down.
const LIFT_EASE: f64 = 0.015;

#[derive(Debug, Clone, PartialEq)]
pub struct EyeOfEnder {
    pub position: DVec3,
    prev_position: DVec3,
    pub velocity: DVec3,
    target: DVec3,
    pub age: u32,
    /// Whether it drops an item when it lands (four in five do).
    pub survives: bool,
}

/// What became of an eye at the end of its flight.
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum Landing {
    Dropped(DVec3),
    Broke(DVec3),
}

impl EyeOfEnder {
    /// Thrown from `from`, to float toward the block column at `toward`.
    pub fn throw(from: DVec3, toward: (f64, f64), survives: bool) -> Self {
        let (dx, dz) = (toward.0 - from.x, toward.1 - from.z);
        let distance = dx.hypot(dz);
        let target = if distance > REACH {
            DVec3::new(
                from.x + dx / distance * REACH,
                from.y + CLIMB,
                from.z + dz / distance * REACH,
            )
        } else {
            DVec3::new(toward.0, from.y, toward.1)
        };
        Self {
            position: from,
            prev_position: from,
            velocity: DVec3::ZERO,
            target,
            age: 0,
            survives,
        }
    }

    pub fn position_between_ticks(&self, alpha: f64) -> DVec3 {
        self.prev_position.lerp(self.position, alpha)
    }

    /// One tick; `Some` once the flight is over.
    pub fn tick(&mut self) -> Option<Landing> {
        self.prev_position = self.position;
        self.position += self.velocity;
        let (dx, dz) = (
            self.target.x - self.position.x,
            self.target.z - self.position.z,
        );
        let left = dx.hypot(dz);
        let heading = dz.atan2(dx);
        let mut speed = (left * 0.1).clamp(SPEED.0, SPEED.1);
        let mut rise = self.velocity.y;
        if left < 1.0 {
            speed *= 0.8;
            rise *= 0.8;
        }
        let goal = if self.position.y < self.target.y {
            1.0
        } else {
            -1.0
        };
        self.velocity = DVec3::new(
            heading.cos() * speed,
            rise + (goal - rise) * LIFT_EASE,
            heading.sin() * speed,
        );
        self.age += 1;
        (self.age > LIFE).then_some(if self.survives {
            Landing::Dropped(self.position)
        } else {
            Landing::Broke(self.position)
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn fly(mut eye: EyeOfEnder) -> (EyeOfEnder, Landing, f64) {
        let mut top: f64 = 0.0;
        loop {
            top = top.max(eye.position.y);
            if let Some(landing) = eye.tick() {
                return (eye, landing, top);
            }
        }
    }

    #[test]
    fn a_far_stronghold_sends_it_twelve_blocks_that_way_and_up() {
        let from = DVec3::new(0.0, 70.0, 0.0);
        let (eye, landing, top) = fly(EyeOfEnder::throw(from, (3000.0, 0.0), true));
        let Landing::Dropped(at) = landing else {
            panic!("{landing:?}")
        };
        assert_eq!(at, eye.position);
        // Along +x, near the twelve blocks it aims at, and well above.
        assert!(at.x > 9.0 && at.x < 13.0, "{at}");
        assert!(at.z.abs() < 0.01);
        assert!(top > from.y + 4.0, "rose {}", top - from.y);
        assert_eq!(eye.age, LIFE + 1);
    }

    #[test]
    fn a_near_one_stops_over_it_and_never_overshoots() {
        let from = DVec3::new(0.0, 70.0, 0.0);
        let (eye, _, _) = fly(EyeOfEnder::throw(from, (0.0, -5.0), false));
        assert!((eye.position.z + 5.0).abs() < 1.0, "{}", eye.position);
        assert!(eye.position.x.abs() < 0.01);
    }

    #[test]
    fn it_breaks_instead_of_dropping_when_it_doesnt_survive() {
        let (_, landing, _) = fly(EyeOfEnder::throw(DVec3::ZERO, (50.0, 50.0), false));
        assert!(matches!(landing, Landing::Broke(_)));
    }
}
