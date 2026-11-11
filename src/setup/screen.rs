//! The first-launch screen: what is missing, what it costs to download,
//! one progress bar, and what to do when it can't. It draws with egui's
//! own font, because the Minecraft font is one of the things being fetched.

use super::download::{self, Event, Handle, Policy, Request};
use super::layers;
use super::manifest::{self, Endpoints, Extra};
use super::vanilla::Plan;
use super::{Result, SetupError};
use crossbeam_channel::Receiver;
use std::collections::VecDeque;
use std::path::PathBuf;
use std::sync::Arc;
use std::time::{Duration, Instant};

/// How the screen ended.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Outcome {
    /// The files are in place: go on to the title screen.
    Installed,
    Quit,
}

#[derive(Debug, Clone, PartialEq, Eq)]
enum Phase {
    /// Asking Mojang for the sizes.
    Measuring,
    /// Waiting for the player's choice.
    Ready,
    Working,
    /// One line saying why, and the choices that follow.
    Failed(String),
}

/// Bytes done over time, for MB/s and the time left.
pub(super) struct Progress {
    pub(super) total: u64,
    pub(super) done: i64,
    pub(super) file: String,
    pub(super) stage: Option<String>,
    samples: VecDeque<(Instant, i64)>,
}

impl Progress {
    pub(super) fn new() -> Self {
        Self {
            total: 0,
            done: 0,
            file: String::new(),
            stage: None,
            samples: VecDeque::new(),
        }
    }

    pub(super) fn note(&mut self, now: Instant) {
        self.samples.push_back((now, self.done));
        while self
            .samples
            .front()
            .is_some_and(|(t, _)| now.duration_since(*t) > Duration::from_secs(6))
        {
            self.samples.pop_front();
        }
    }

    /// Bytes per second over the last few seconds.
    pub(super) fn rate(&self) -> f64 {
        match (self.samples.front(), self.samples.back()) {
            (Some((t0, b0)), Some((t1, b1))) if t1 > t0 => {
                (*b1 - *b0).max(0) as f64 / t1.duration_since(*t0).as_secs_f64()
            }
            _ => 0.0,
        }
    }

    pub(super) fn fraction(&self) -> f32 {
        if self.total == 0 {
            0.0
        } else {
            (self.done.max(0) as f64 / self.total as f64).min(1.0) as f32
        }
    }
}

/// Decimal megabytes, like the Finder.
pub fn megabytes(bytes: u64) -> String {
    format!("{:.1} MB", bytes as f64 / 1e6)
}

pub(super) fn duration_text(seconds: f64) -> String {
    let s = seconds.round() as u64;
    if s >= 3600 {
        format!("{} h {} min", s / 3600, s % 3600 / 60)
    } else if s >= 60 {
        format!("{} min {} s", s / 60, s % 60)
    } else {
        format!("{s} s")
    }
}

pub struct SetupScreen {
    endpoints: Endpoints,
    policy: Policy,
    /// The user asset layer the files go to.
    target: PathBuf,
    phase: Phase,
    measuring: Option<Receiver<Result<Plan>>>,
    plan: Option<Arc<Plan>>,
    /// The packs and jars not yet in the downloads folder.
    missing_extras: Vec<Extra>,
    with_packs: bool,
    job: Option<Handle>,
    progress: Progress,
    outcome: Option<Outcome>,
    cancelled: bool,
}

impl SetupScreen {
    /// A screen that will put its files in `target` (the user asset layer).
    pub fn new(
        endpoints: Endpoints,
        policy: Policy,
        target: PathBuf,
        downloads: &std::path::Path,
    ) -> Self {
        let missing_extras = manifest::extras()
            .iter()
            .filter(|e| layers::presence(downloads, e) != layers::Presence::Present)
            .cloned()
            .collect();
        let mut screen = Self {
            endpoints,
            policy,
            target,
            phase: Phase::Measuring,
            measuring: None,
            plan: None,
            missing_extras,
            with_packs: true,
            job: None,
            progress: Progress::new(),
            outcome: None,
            cancelled: false,
        };
        screen.measure();
        screen
    }

    /// How far the running download is, 0 when none runs.
    pub fn progress_fraction(&self) -> f32 {
        if self.phase == Phase::Working {
            self.progress.fraction()
        } else {
            0.0
        }
    }

    pub fn outcome(&self) -> Option<Outcome> {
        self.outcome
    }

    /// Whether the screen wants frames soon (a download is moving).
    pub fn animating(&self) -> bool {
        matches!(self.phase, Phase::Measuring | Phase::Working)
    }

    fn measure(&mut self) {
        self.phase = Phase::Measuring;
        self.measuring = Some(download::measure(self.endpoints.clone(), self.policy));
    }

    fn start(&mut self) {
        let Some(plan) = self.plan.clone() else {
            return;
        };
        self.cancelled = false;
        self.progress = Progress::new();
        self.job = Some(download::start(Request {
            vanilla: Some(plan),
            extras: if self.with_packs {
                self.missing_extras.clone()
            } else {
                Vec::new()
            },
            target: self.target.clone(),
            endpoints: self.endpoints.clone(),
            policy: self.policy,
        }));
        self.phase = Phase::Working;
    }

    fn pick_folder(&mut self) {
        let Some(picked) = rfd::FileDialog::new()
            .set_title("Choose the folder that has Minecraft's files (assets/minecraft inside)")
            .pick_folder()
        else {
            return;
        };
        self.progress = Progress::new();
        self.job = Some(download::start_import(picked, self.target.clone()));
        self.phase = Phase::Working;
    }

    /// Takes what the worker and Mojang have said since the last frame.
    pub fn update(&mut self) {
        if let Some(rx) = &self.measuring
            && let Ok(result) = rx.try_recv()
        {
            self.measuring = None;
            match result {
                Ok(plan) => {
                    self.plan = Some(Arc::new(plan));
                    self.phase = Phase::Ready;
                    // For tests and demos: `$RUSTCRAFT_SETUP_AUTO=1` presses
                    // "Download" (the packs included) once the sizes are in.
                    if std::env::var_os("RUSTCRAFT_SETUP_AUTO").is_some() {
                        self.start();
                    }
                }
                Err(err) => self.phase = Phase::Failed(err.to_string()),
            }
        }
        let mut finished = None;
        if let Some(job) = &self.job {
            let now = Instant::now();
            for event in job.rx.try_iter() {
                match event {
                    Event::Started { total, .. } => self.progress.total = total,
                    Event::File(name) => {
                        self.progress.file = name;
                        self.progress.stage = None;
                    }
                    Event::Bytes(n) => self.progress.done += n,
                    Event::Stage(text) => self.progress.stage = Some(text),
                    Event::Finished => finished = Some(Ok(())),
                    Event::Failed(err) => finished = Some(Err(err)),
                    Event::Cancelled => finished = Some(Err(SetupError::Cancelled)),
                }
            }
            self.progress.note(now);
        }
        match finished {
            Some(Ok(())) => {
                self.job = None;
                self.outcome = Some(Outcome::Installed);
            }
            Some(Err(SetupError::Cancelled)) => {
                self.job = None;
                self.cancelled = true;
                self.phase = Phase::Ready;
            }
            Some(Err(err)) => {
                self.job = None;
                log::error!("first-launch download failed: {err}");
                self.phase = Phase::Failed(err.to_string());
            }
            None => {}
        }
    }

    fn extras_bytes(&self) -> u64 {
        self.missing_extras.iter().map(|e| e.size).sum()
    }

    /// Draws one frame of the screen.
    pub fn ui(&mut self, ui: &mut egui::Ui) {
        self.update();
        egui::CentralPanel::default()
            .frame(egui::Frame::new().inner_margin(24.0))
            .show(ui, |ui| {
                ui.heading("Welcome to Rustcraft");
                ui.add_space(8.0);
                egui::ScrollArea::vertical().show(ui, |ui| match self.phase.clone() {
                    Phase::Measuring => {
                        ui.add_space(20.0);
                        ui.horizontal(|ui| {
                            ui.spinner();
                            ui.label("Asking Mojang's servers what to download…");
                        });
                    }
                    Phase::Ready => self.ready(ui),
                    Phase::Working => self.working(ui),
                    Phase::Failed(reason) => self.failed(ui, &reason),
                });
            });
    }

    fn ready(&mut self, ui: &mut egui::Ui) {
        let Some(plan) = self.plan.clone() else {
            return;
        };
        ui.label(
            "Rustcraft draws the world with Minecraft's own textures, sounds and font. \
             They aren't included with the game: it downloads them once, from Mojang's \
             servers, onto this Mac.",
        );
        ui.add_space(8.0);
        if self.cancelled {
            ui.colored_label(
                ui.visuals().warn_fg_color,
                "Stopped. Download again to carry on where it left off.",
            );
            ui.add_space(4.0);
        }
        ui.strong("Missing: Minecraft's files (required)");
        let sounds = plan.sound_bytes();
        ui.label(format!(
            "  Textures, models and text (client {}): {}",
            plan.version,
            megabytes(plan.client.size + plan.object_bytes() - sounds)
        ));
        ui.label(format!("  Sounds and music: {}", megabytes(sounds)));
        ui.add_space(10.0);
        ui.checkbox(
            &mut self.with_packs,
            "Also get the Barebones and Realistic style packs",
        );
        if !self.missing_extras.is_empty() {
            ui.label(format!(
                "  {} packs and mods from Modrinth, {}",
                self.missing_extras.len(),
                megabytes(self.extras_bytes())
            ));
            egui::CollapsingHeader::new("What these are").show(ui, |ui| {
                for extra in &self.missing_extras {
                    ui.horizontal_wrapped(|ui| {
                        ui.label(format!(
                            "{} by {} — {} —",
                            extra.name,
                            extra.author,
                            license_text(&extra.license)
                        ));
                        ui.hyperlink_to("Modrinth", &extra.page);
                        ui.label(megabytes(extra.size));
                    });
                }
            });
        }
        ui.add_space(14.0);
        let total = plan.download_bytes()
            + if self.with_packs {
                self.extras_bytes()
            } else {
                0
            };
        ui.horizontal(|ui| {
            if ui
                .button(format!("Download Minecraft's files ({})", megabytes(total)))
                .clicked()
            {
                self.start();
            }
            if ui.button("Use a folder I already have…").clicked() {
                self.pick_folder();
            }
            if ui.button("Quit").clicked() {
                self.outcome = Some(Outcome::Quit);
            }
        });
    }

    fn working(&mut self, ui: &mut egui::Ui) {
        let p = &self.progress;
        ui.add_space(10.0);
        ui.add(
            egui::ProgressBar::new(p.fraction())
                .show_percentage()
                .desired_height(22.0),
        );
        ui.add_space(6.0);
        let rate = p.rate();
        let left = p.total.saturating_sub(p.done.max(0) as u64);
        let mut line = format!(
            "{} of {}",
            megabytes(p.done.max(0) as u64),
            megabytes(p.total)
        );
        if rate > 1000.0 {
            line += &format!(
                ", {:.1} MB/s, about {} left",
                rate / 1e6,
                duration_text(left as f64 / rate)
            );
        }
        ui.label(line);
        match (&p.stage, p.file.is_empty()) {
            (Some(stage), _) => ui.label(stage.as_str()),
            (None, false) => ui.label(format!("Now: {}", p.file)),
            _ => ui.label(""),
        };
        ui.add_space(12.0);
        if ui.button("Cancel").clicked()
            && let Some(job) = &self.job
        {
            job.cancel();
        }
    }

    fn failed(&mut self, ui: &mut egui::Ui, reason: &str) {
        ui.add_space(10.0);
        ui.colored_label(ui.visuals().error_fg_color, reason);
        ui.add_space(6.0);
        ui.label("Nothing is lost: Retry picks up where it stopped.");
        ui.add_space(12.0);
        ui.horizontal(|ui| {
            if ui.button("Retry").clicked() {
                if self.plan.is_some() {
                    self.start();
                } else {
                    self.measure();
                }
            }
            if ui.button("Use a folder I already have…").clicked() {
                self.pick_folder();
            }
            if ui.button("Quit").clicked() {
                self.outcome = Some(Outcome::Quit);
            }
        });
    }
}

/// A licence id from Modrinth in words.
pub fn license_text(id: &str) -> String {
    match id {
        "LicenseRef-All-Rights-Reserved" => "All rights reserved".into(),
        "LicenseRef-Custom-License" => "custom licence".into(),
        "LicenseRef-See-Terms-of-Use-in-Description" => "see its terms of use".into(),
        other => other.trim_start_matches("LicenseRef-").replace('-', " "),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn sizes_and_times_read_like_the_finder() {
        assert_eq!(megabytes(423_900_000), "423.9 MB");
        assert_eq!(duration_text(75.0), "1 min 15 s");
        assert_eq!(duration_text(9.0), "9 s");
    }

    #[test]
    fn rate_comes_from_the_last_seconds() {
        let mut p = Progress::new();
        let t0 = Instant::now();
        p.total = 100_000_000;
        p.done = 0;
        p.note(t0);
        p.done = 20_000_000;
        p.note(t0 + Duration::from_secs(2));
        assert!((p.rate() - 10_000_000.0).abs() < 1.0);
        assert!((p.fraction() - 0.2).abs() < 1e-6);
    }

    #[test]
    fn licences_are_readable() {
        assert_eq!(license_text("MIT"), "MIT");
        assert_eq!(
            license_text("LicenseRef-All-Rights-Reserved"),
            "All rights reserved"
        );
        assert_eq!(
            license_text("LicenseRef-Polyform-Shield-1.0"),
            "Polyform Shield 1.0"
        );
    }
}
