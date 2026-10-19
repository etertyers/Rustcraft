//! Combat effects: the stars of a critical hit and the arc of a sword's
//! sweep, little camera-facing sprites that live a few ticks.

use glam::DVec3;

/// Stars in a critical hit's burst (vanilla's tracking emitter makes
/// 16 a tick for 3 ticks).
const CRIT_COUNT: usize = 48;
/// A star's drag each tick, and how fast it flies out.
const CRIT_DRAG: f64 = 0.7;
const CRIT_SPEED: f64 = 0.55;
/// Frames in a sweep and how long each lasts.
pub const SWEEP_FRAMES: usize = 4;

#[derive(Debug, Clone, Copy, PartialEq)]
pub enum EffectKind {
    Crit,
    /// The arc of a sweep, turned toward `yaw`.
    Sweep {
        yaw: f32,
    },
}

#[derive(Debug, Clone, PartialEq)]
pub struct Effect {
    pub kind: EffectKind,
    pub position: DVec3,
    prev_position: DVec3,
    velocity: DVec3,
    age: u32,
    lifetime: u32,
    /// Half the sprite's width in blocks.
    pub half_width: f32,
}

impl Effect {
    /// A burst of critical-hit stars around `at`.
    pub fn crit(at: DVec3, random: &mut impl FnMut() -> f32) -> Vec<Self> {
        let mut stars = Vec::with_capacity(CRIT_COUNT);
        while stars.len() < CRIT_COUNT {
            let direction = DVec3::new(
                random() as f64 * 2.0 - 1.0,
                random() as f64 * 2.0 - 1.0,
                random() as f64 * 2.0 - 1.0,
            );
            if direction.length_squared() > 1.0 {
                continue;
            }
            let start = at + direction * 0.25;
            stars.push(Self {
                kind: EffectKind::Crit,
                position: start,
                prev_position: start,
                velocity: (direction + DVec3::Y * 0.2) * CRIT_SPEED,
                age: 0,
                lifetime: (6.0 / (random() * 0.8 + 0.6)) as u32 + 2,
                half_width: 0.05 + random() * 0.04,
            });
        }
        stars
    }

    /// The sweep's arc at `at`, facing `yaw`.
    pub fn sweep(at: DVec3, yaw: f32) -> Vec<Self> {
        vec![Self {
            kind: EffectKind::Sweep { yaw },
            position: at,
            prev_position: at,
            velocity: DVec3::ZERO,
            age: 0,
            lifetime: SWEEP_FRAMES as u32,
            half_width: 0.5,
        }]
    }

    /// Which frame of a sweep it shows, 0 to `SWEEP_FRAMES − 1`.
    pub fn frame(&self) -> usize {
        ((self.age as usize * SWEEP_FRAMES) / self.lifetime as usize).min(SWEEP_FRAMES - 1)
    }

    pub fn position_between_ticks(&self, alpha: f64) -> DVec3 {
        self.prev_position.lerp(self.position, alpha)
    }

    pub fn expired(&self) -> bool {
        self.age >= self.lifetime
    }

    pub fn tick(&mut self) {
        self.prev_position = self.position;
        self.age += 1;
        self.position += self.velocity;
        if self.kind == EffectKind::Crit {
            self.velocity *= CRIT_DRAG;
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_crit_bursts_into_stars_that_fly_out_slow_down_and_vanish() {
        let mut state = 7u32;
        let mut random = move || {
            state = state.wrapping_mul(1_664_525).wrapping_add(1_013_904_223);
            (state >> 8) as f32 / (1u32 << 24) as f32
        };
        let at = DVec3::new(5.0, 65.0, 5.0);
        let mut stars = Effect::crit(at, &mut random);
        assert_eq!(stars.len(), CRIT_COUNT);
        assert!(stars.iter().all(|s| s.position.distance(at) <= 0.25 + 1e-9));
        let mut farthest: f64 = 0.0;
        for _ in 0..30 {
            stars.iter_mut().for_each(Effect::tick);
            stars.retain(|s| !s.expired());
            farthest = stars
                .iter()
                .map(|s| s.position.distance(at))
                .fold(farthest, f64::max);
        }
        // They spread about a block and are gone within a second.
        assert!(farthest > 0.5 && farthest < 3.0, "{farthest}");
        assert!(stars.is_empty());
    }

    #[test]
    fn a_sweep_plays_its_frames_in_four_ticks() {
        let mut sweep = Effect::sweep(DVec3::ZERO, 0.0).pop().unwrap();
        let mut frames = Vec::new();
        while !sweep.expired() {
            frames.push(sweep.frame());
            sweep.tick();
        }
        assert_eq!(frames, [0, 1, 2, 3]);
        assert!(sweep.expired());
    }
}
