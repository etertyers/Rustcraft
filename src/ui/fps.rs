//! Frames-per-second counter.

use std::time::{Duration, Instant};

/// How often the displayed FPS refreshes.
const SAMPLE_PERIOD: Duration = Duration::from_millis(500);

/// Counts frames and reports the average rate over the last sample period.
pub struct FpsCounter {
    frames: u32,
    period_start: Instant,
    fps: f32,
}

impl FpsCounter {
    pub fn new(now: Instant) -> Self {
        Self {
            frames: 0,
            period_start: now,
            fps: 0.0,
        }
    }

    /// Records one frame finishing at `now`.
    pub fn tick(&mut self, now: Instant) {
        self.frames += 1;
        let elapsed = now - self.period_start;
        if elapsed >= SAMPLE_PERIOD {
            self.fps = self.frames as f32 / elapsed.as_secs_f32();
            self.frames = 0;
            self.period_start = now;
        }
    }

    pub fn fps(&self) -> f32 {
        self.fps
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn reports_average_over_each_period() {
        let start = Instant::now();
        let mut counter = FpsCounter::new(start);
        for i in 1..=30 {
            counter.tick(start + Duration::from_millis(i * 1000 / 60));
        }
        assert!((counter.fps() - 60.0).abs() < 0.5, "{}", counter.fps());
    }

    #[test]
    fn shows_zero_until_the_first_period_ends() {
        let start = Instant::now();
        let mut counter = FpsCounter::new(start);
        counter.tick(start + Duration::from_millis(16));
        assert_eq!(counter.fps(), 0.0);
    }
}
