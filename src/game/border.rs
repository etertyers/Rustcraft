//! The world border (M45): a square around a center whose size can change
//! over time. Players and mobs are held inside it, and anyone left outside
//! beyond the damage buffer is hurt.

use glam::DVec3;
use serde::{Deserialize, Serialize};

/// Vanilla's default: effectively no border.
pub const DEFAULT_SIZE: f64 = 59_999_968.0;

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct WorldBorder {
    pub center: [f64; 2],
    /// The size when the current change began, the size it is heading for,
    /// when it began (game time) and how many ticks it takes.
    pub from: f64,
    pub to: f64,
    pub start: u64,
    pub ticks: u64,
    /// Health per second for each block outside the buffer.
    pub damage: f64,
    /// Blocks past the border before damage starts.
    pub buffer: f64,
    /// The warning tint begins this many blocks from the border (or when it
    /// is closing in within `warning_time` seconds).
    pub warning_distance: f64,
    pub warning_time: f64,
}

impl Default for WorldBorder {
    fn default() -> Self {
        Self {
            center: [0.0, 0.0],
            from: DEFAULT_SIZE,
            to: DEFAULT_SIZE,
            start: 0,
            ticks: 0,
            damage: 0.2,
            buffer: 5.0,
            warning_distance: 5.0,
            warning_time: 15.0,
        }
    }
}

impl WorldBorder {
    /// The width at game time `now`.
    pub fn size_at(&self, now: u64) -> f64 {
        if self.ticks == 0 || now >= self.start + self.ticks {
            return self.to;
        }
        let t = now.saturating_sub(self.start) as f64 / self.ticks as f64;
        self.from + (self.to - self.from) * t
    }

    /// Starts heading for `size` over `ticks` from `now`.
    pub fn resize(&mut self, now: u64, size: f64, ticks: u64) {
        self.from = self.size_at(now);
        self.to = size;
        self.start = now;
        self.ticks = ticks;
        if ticks == 0 {
            self.from = size;
        }
    }

    pub fn is_moving(&self, now: u64) -> bool {
        self.ticks > 0
            && now < self.start + self.ticks
            && (self.to - self.from).abs() > f64::EPSILON
    }

    /// How far inside the border (x, z) is: negative outside.
    pub fn inside_by(&self, now: u64, x: f64, z: f64) -> f64 {
        let half = self.size_at(now) / 2.0;
        (half - (x - self.center[0]).abs()).min(half - (z - self.center[1]).abs())
    }

    /// `pos` pulled in to just inside the border, with `margin` to spare.
    pub fn clamp(&self, now: u64, pos: DVec3, margin: f64) -> DVec3 {
        let half = (self.size_at(now) / 2.0 - margin).max(0.0);
        DVec3::new(
            pos.x.clamp(self.center[0] - half, self.center[0] + half),
            pos.y,
            pos.z.clamp(self.center[1] - half, self.center[1] + half),
        )
    }

    /// The color of the wall: blue still, red closing in, green growing.
    pub fn color(&self, now: u64) -> [f32; 3] {
        if self.is_moving(now) {
            if self.to < self.from {
                [1.0, 0.2, 0.2]
            } else {
                [0.2, 1.0, 0.3]
            }
        } else {
            [0.2, 0.5, 1.0]
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_size_moves_in_a_straight_line_to_its_target() {
        let mut b = WorldBorder::default();
        b.resize(100, 100.0, 0);
        assert_eq!(b.size_at(100), 100.0);
        b.resize(100, 50.0, 200);
        assert_eq!(b.size_at(100), 100.0);
        assert_eq!(b.size_at(200), 75.0);
        assert_eq!(b.size_at(300), 50.0);
        assert_eq!(b.size_at(10_000), 50.0);
        assert!(b.is_moving(150) && !b.is_moving(300));
        // A change begun part-way starts from where the border is.
        b.resize(200, 150.0, 100);
        assert_eq!(b.size_at(200), 75.0);
        assert_eq!(b.size_at(250), 112.5);
    }

    #[test]
    fn inside_clamp_and_colors() {
        let mut b = WorldBorder {
            center: [10.0, 0.0],
            ..WorldBorder::default()
        };
        b.resize(0, 20.0, 0);
        assert_eq!(b.inside_by(0, 10.0, 0.0), 10.0);
        assert_eq!(b.inside_by(0, 25.0, 0.0), -5.0);
        let p = b.clamp(0, DVec3::new(100.0, 70.0, -100.0), 0.5);
        assert_eq!(p, DVec3::new(19.5, 70.0, -9.5));
        assert_eq!(
            b.clamp(0, DVec3::new(12.0, 5.0, 3.0), 0.5),
            DVec3::new(12.0, 5.0, 3.0)
        );
        assert_eq!(b.color(0), [0.2, 0.5, 1.0]);
        b.resize(0, 10.0, 100);
        assert_eq!(b.color(10), [1.0, 0.2, 0.2]);
        b.resize(0, 40.0, 100);
        assert_eq!(b.color(10), [0.2, 1.0, 0.3]);
    }
}
