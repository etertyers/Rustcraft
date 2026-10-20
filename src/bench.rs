//! `--bench`: flies a fixed route over generated terrain with vsync off,
//! timing every frame and the main parts of it, then prints a report and
//! quits. Run it the same way before and after a change to compare, or
//! under Instruments to see where the time goes.
//!
//! `--perf-test` is the milestone performance gate (CLAUDE.md §7.2): the
//! same flight for 60 seconds at render distance 9, written up in
//! `docs/perf/m<N>.md` with chunk generation and meshing times and memory.

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
/// The window's size: a 1080p screen.
pub const WINDOW_SIZE: (u32, u32) = (1920, 1080);
/// The milestone `--perf-test` writes up (`docs/perf/m<N>.md`).
pub const PERF_MILESTONE: u32 = 24;
/// `--perf-test`'s flight length and render distance.
pub const PERF_SECONDS: f32 = 60.0;
pub const PERF_DISTANCE: i32 = 9;
/// The gate: average and 1% low frame rates a milestone must keep.
pub const MIN_AVERAGE_FPS: f32 = 60.0;
pub const MIN_ONE_PERCENT_LOW_FPS: f32 = 40.0;

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
    /// `--perf-test`: where to write the report.
    pub perf_report: Option<std::path::PathBuf>,
    elapsed: f32,
    warm_up: Vec<FrameTimes>,
    frames: Vec<FrameTimes>,
}

impl Bench {
    pub fn new(seconds: f32) -> Self {
        Self {
            seconds,
            distance: None,
            perf_report: None,
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
            "bench: render distance {render_distance}, {}×{}, {:.0} s at {SPEED} blocks/s\n  warm-up {}\n  flight  {}",
            WINDOW_SIZE.0,
            WINDOW_SIZE.1,
            self.seconds,
            summary(&self.warm_up),
            summary(&self.frames)
        )
    }
}

/// What the chunk workers did during a run, and the memory the game used.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct RunStats {
    /// Chunks generated (or loaded) and their average time in ms.
    pub generated: (u64, f64),
    /// Chunks lit and meshed and their average time in ms.
    pub meshed: (u64, f64),
    /// Resident memory at the end, in MiB.
    pub memory_mib: Option<f64>,
}

/// Flight numbers for the perf report.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Flight {
    pub frames: usize,
    pub average_fps: f32,
    pub one_percent_low_fps: f32,
    pub worst_ms: f32,
    /// Frames over 33 ms and over 50 ms.
    pub over_33ms: usize,
    pub over_50ms: usize,
}

impl Bench {
    /// The flight's numbers (after the warm-up).
    pub fn flight(&self) -> Option<Flight> {
        if self.frames.is_empty() {
            return None;
        }
        let mut sorted: Vec<f32> = self.frames.iter().map(|f| f.frame).collect();
        sorted.sort_by(f32::total_cmp);
        let n = sorted.len();
        let total: f32 = sorted.iter().sum();
        Some(Flight {
            frames: n,
            average_fps: n as f32 / total,
            one_percent_low_fps: 1.0 / sorted[((n - 1) as f32 * 0.99).round() as usize],
            worst_ms: sorted[n - 1] * 1000.0,
            over_33ms: sorted.iter().filter(|&&dt| dt > 1.0 / 30.0).count(),
            over_50ms: sorted.iter().filter(|&&dt| dt > 0.05).count(),
        })
    }

    /// The `--perf-test` write-up, in Markdown.
    pub fn perf_markdown(&self, render_distance: i32, run: RunStats, about: &str) -> String {
        let Some(f) = self.flight() else {
            return "No frames were recorded.\n".to_owned();
        };
        let pass =
            f.average_fps >= MIN_AVERAGE_FPS && f.one_percent_low_fps >= MIN_ONE_PERCENT_LOW_FPS;
        let memory = run
            .memory_mib
            .map_or("unknown".to_owned(), |m| format!("{m:.0} MiB"));
        format!(
            "# M{PERF_MILESTONE} performance\n\n\
             `cargo run --release -- --perf-test`: seed {SEED}, a {:.0} s flight north at \
             {SPEED} blocks/s and feet height {HEIGHT}, render distance {render_distance}, \
             {}×{}, vsync off, after {WARM_UP:.0} s of warm-up.\n\n{about}\n\n\
             | Measure | Value |\n|---|---|\n\
             | Average FPS | {:.0} |\n\
             | 1% low FPS | {:.0} |\n\
             | Worst frame | {:.1} ms |\n\
             | Frames over 33 ms / 50 ms | {} / {} (of {}) |\n\
             | Chunks generated or loaded | {} (avg {:.2} ms each, on worker threads) |\n\
             | Chunks lit and meshed | {} (avg {:.2} ms each, on worker threads) |\n\
             | Memory (resident) | {memory} |\n\n\
             Gate (average ≥ {MIN_AVERAGE_FPS:.0}, 1% low ≥ {MIN_ONE_PERCENT_LOW_FPS:.0}): **{}**\n",
            self.seconds,
            WINDOW_SIZE.0,
            WINDOW_SIZE.1,
            f.average_fps,
            f.one_percent_low_fps,
            f.worst_ms,
            f.over_33ms,
            f.over_50ms,
            f.frames,
            run.generated.0,
            run.generated.1,
            run.meshed.0,
            run.meshed.1,
            if pass { "pass" } else { "FAIL" },
        )
    }
}

/// The machine's model and chip, from `sysctl`.
pub fn machine() -> String {
    let read = |key: &str| {
        std::process::Command::new("sysctl")
            .args(["-n", key])
            .output()
            .ok()
            .and_then(|out| String::from_utf8(out.stdout).ok())
            .map(|s| s.trim().to_owned())
            .unwrap_or_default()
    };
    format!(
        "{} ({})",
        read("hw.model"),
        read("machdep.cpu.brand_string")
    )
}

/// This process's resident memory in MiB, from `ps`.
pub fn resident_memory_mib() -> Option<f64> {
    let output = std::process::Command::new("ps")
        .args(["-o", "rss=", "-p", &std::process::id().to_string()])
        .output()
        .ok()?;
    let kib: f64 = String::from_utf8(output.stdout).ok()?.trim().parse().ok()?;
    Some(kib / 1024.0)
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
    let worst = frames
        .iter()
        .max_by(|a, b| a.frame.total_cmp(&b.frame))
        .expect("not empty");
    let ms = |d: Duration| d.as_secs_f32() * 1000.0;
    format!(
        "{} frames ({offscreen} off screen), {:.0} fps average, 1% low {:.0} fps, worst {:.1} ms \
         (game {:.1}, chunks {:.1}, draw {:.1}), {slow} over 33 ms; \
         per frame: game {:.2} ms, chunks {:.2} ms, draw {:.2} ms",
        frames.len(),
        n / total,
        1.0 / percentile(0.99),
        worst.frame * 1000.0,
        ms(worst.game),
        ms(worst.chunks),
        ms(worst.draw),
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
        assert!(
            report.contains("worst 62.5 ms (game 1.0, chunks 0.0, draw 0.0), 1 over 33 ms"),
            "{report}"
        );
        assert!(report.contains("game 1.00 ms"), "{report}");
    }
}
