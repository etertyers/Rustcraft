//! `--bench`: flies a fixed route over generated terrain with vsync off,
//! timing every frame and the main parts of it, then prints a report and
//! quits. Run it the same way before and after a change to compare, or
//! under Instruments to see where the time goes.

use std::time::Duration;

/// The world flown over, unless `--seed` picks another.
pub const SEED: u64 = 20_111_118;
/// How long the flight lasts, unless `--bench <seconds>` says otherwise.
pub const DEFAULT_SECONDS: f32 = 60.0;
/// Blocks a second: creative flying's sprint speed, which loads new chunks
/// as fast as play can.
pub const SPEED: f64 = 10.9;
/// Feet height for the flight, above most hills.
pub const HEIGHT: f64 = 100.0;
/// The first seconds load the world around the start; they're timed
/// separately.
const WARM_UP: f32 = 5.0;

/// Time spent in one frame.
#[derive(Debug, Default, Clone, Copy, PartialEq)]
pub struct FrameTimes {
    /// Since the frame before, in seconds.
    pub frame: f32,
    /// Running game ticks.
    pub game: Duration,
    /// Taking finished chunks from the workers and uploading their meshes.
    pub chunks: Duration,
    /// Building entity meshes and the UI, and drawing.
    pub draw: Duration,
    /// Drawn off screen because the window had no frame to give (it was
    /// hidden or the screen locked).
    pub offscreen: bool,
}

#[derive(Debug)]
pub struct Bench {
    pub seconds: f32,
    /// Overrides the render distance in the options.
    pub distance: Option<i32>,
    elapsed: f32,
    warm_up: Vec<FrameTimes>,
    frames: Vec<FrameTimes>,
}

impl Bench {
    pub fn new(seconds: f32) -> Self {
        Self {
            seconds,
            distance: None,
            elapsed: 0.0,
            warm_up: Vec::new(),
            frames: Vec::new(),
        }
    }

    pub fn record(&mut self, times: FrameTimes) {
        self.elapsed += times.frame;
        if self.elapsed <= WARM_UP {
            self.warm_up.push(times);
        } else {
            self.frames.push(times);
        }
    }

    pub fn done(&self) -> bool {
        self.elapsed >= self.seconds + WARM_UP
    }

    /// Frame rate, slow frames and where the time went, for the warm-up and
    /// the flight.
    pub fn report(&self, render_distance: i32) -> String {
        format!(
            "bench: render distance {render_distance}, {:.0} s at {SPEED} blocks/s\n  warm-up {}\n  flight  {}",
            self.seconds,
            summary(&self.warm_up),
            summary(&self.frames)
        )
    }
}

fn summary(frames: &[FrameTimes]) -> String {
    if frames.is_empty() {
        return "no frames".to_owned();
    }
    let mut sorted: Vec<f32> = frames.iter().map(|f| f.frame).collect();
    sorted.sort_by(f32::total_cmp);
    let total: f32 = sorted.iter().sum();
    let n = frames.len() as f32;
    let percentile = |p: f32| sorted[((n - 1.0) * p).round() as usize];
    let slow = sorted.iter().filter(|&&dt| dt > 1.0 / 30.0).count();
    let offscreen = frames.iter().filter(|f| f.offscreen).count();
    let mean_ms = |part: fn(&FrameTimes) -> Duration| {
        frames.iter().map(|f| part(f).as_secs_f32()).sum::<f32>() / n * 1000.0
    };
    format!(
        "{} frames ({offscreen} off screen), {:.0} fps average, 1% low {:.0} fps, worst {:.1} ms, {slow} over 33 ms; \
         per frame: game {:.2} ms, chunks {:.2} ms, draw {:.2} ms",
        frames.len(),
        n / total,
        1.0 / percentile(0.99),
        sorted[sorted.len() - 1] * 1000.0,
        mean_ms(|f| f.game),
        mean_ms(|f| f.chunks),
        mean_ms(|f| f.draw),
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_report_splits_off_the_warm_up_and_counts_slow_frames() {
        let mut bench = Bench::new(0.5);
        let frame = |seconds: f32| FrameTimes {
            frame: seconds,
            game: Duration::from_millis(1),
            ..FrameTimes::default()
        };
        // 5 s of warm-up at 8 fps, then 64 fps with one 62.5 ms hitch
        // (lengths a float adds up exactly).
        for _ in 0..40 {
            bench.record(frame(0.125));
        }
        assert!(!bench.done());
        for _ in 0..63 {
            bench.record(frame(1.0 / 64.0));
        }
        bench.record(frame(0.0625));
        assert!(bench.done());
        let report = bench.report(9);
        assert!(
            report.contains("warm-up 40 frames (0 off screen), 8 fps"),
            "{report}"
        );
        assert!(
            report.contains("flight  64 frames (0 off screen)"),
            "{report}"
        );
        assert!(report.contains("worst 62.5 ms, 1 over 33 ms"), "{report}");
        assert!(report.contains("game 1.00 ms"), "{report}");
    }
}
