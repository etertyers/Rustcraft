//! The first-launch screen: what is missing, what it costs to download,
//! one progress bar, and what to do when it can't. It draws with egui's
//! own font, because the Minecraft font is one of the things being fetched.

use super::download::{self, Event, Handle, Policy, Request};
use super::layers::{self, CopySource, Needs};
use super::manifest::{Endpoints, Extra, Kind};
use super::vanilla::Plan;
use super::{Result, SetupError};
use crossbeam_channel::Receiver;
use std::collections::VecDeque;
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
    /// What this start is missing, and the folder that gets filled.
    needs: Needs,
    /// A complete copy elsewhere on this Mac, if one has what is missing.
    copy: Option<CopySource>,
    /// The main button copies (true) or downloads (false).
    use_copy: bool,
    phase: Phase,
    measuring: Option<Receiver<Result<Plan>>>,
    plan: Option<Arc<Plan>>,
    /// Why Mojang couldn't be asked (the copy still works).
    offline: Option<String>,
    /// Start the download as soon as the sizes are in.
    start_when_measured: bool,
    /// Whether the packs go with this run. "Only Minecraft's files" turns it off.
    with_packs: bool,
    /// What to record in the skip marker when the running job succeeds
    /// (`None`: leave it, as for "Use a folder I already have").
    marker_on_success: Option<bool>,
    job: Option<Handle>,
    progress: Progress,
    outcome: Option<Outcome>,
    cancelled: bool,
}

impl SetupScreen {
    /// A screen for `needs`; `copy` is a folder on this Mac that has it all.
    pub fn new(
        endpoints: Endpoints,
        policy: Policy,
        needs: Needs,
        copy: Option<CopySource>,
    ) -> Self {
        let mut screen = Self {
            endpoints,
            policy,
            use_copy: copy.is_some(),
            copy,
            phase: Phase::Ready,
            measuring: None,
            plan: None,
            offline: None,
            start_when_measured: false,
            with_packs: true,
            marker_on_success: None,
            job: None,
            progress: Progress::new(),
            outcome: None,
            cancelled: false,
            needs,
        };
        // Only Mojang's sizes need asking, and only when its files are missing.
        if screen.needs.vanilla {
            screen.measure();
        }
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

    /// The packs this run fetches.
    fn extras(&self) -> Vec<Extra> {
        if self.with_packs {
            self.needs.extras.clone()
        } else {
            Vec::new()
        }
    }

    fn extras_bytes(&self) -> u64 {
        self.extras().iter().map(|e| e.size).sum()
    }

    /// What the main action does: copy, else download.
    fn act(&mut self) {
        if !self.needs.vanilla && !self.with_packs {
            // Vanilla is here and the player wants no packs: nothing to fetch.
            self.finish_ok(Some(true));
        } else if self.use_copy && self.copy.is_some() {
            self.start_copy();
        } else {
            self.start();
        }
    }

    fn finish_ok(&mut self, marker: Option<bool>) {
        if let Some(skip) = marker
            && !self.needs.extras.is_empty()
        {
            layers::set_skip_packs(&self.needs.target, skip);
        }
        self.job = None;
        self.outcome = Some(Outcome::Installed);
    }

    fn start_copy(&mut self) {
        let Some(mut source) = self.copy.clone() else {
            return;
        };
        source.extras = self.extras();
        source.vanilla = self.needs.vanilla;
        self.cancelled = false;
        self.progress = Progress::new();
        self.marker_on_success = Some(!self.with_packs);
        self.job = Some(download::start_copy(source, self.needs.target.clone()));
        self.phase = Phase::Working;
    }

    fn start(&mut self) {
        let plan = if self.needs.vanilla {
            let Some(plan) = self.plan.clone() else {
                // Mojang's sizes aren't in yet (or failed): ask, then go.
                self.start_when_measured = true;
                self.measure();
                return;
            };
            Some(plan)
        } else {
            None
        };
        self.cancelled = false;
        self.progress = Progress::new();
        self.marker_on_success = Some(!self.with_packs);
        self.job = Some(download::start(Request {
            vanilla: plan,
            extras: self.extras(),
            target: self.needs.target.clone(),
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
        self.marker_on_success = None;
        self.job = Some(download::start_import(picked, self.needs.target.clone()));
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
                    self.offline = None;
                    self.phase = Phase::Ready;
                    // For tests and demos: `$RUSTCRAFT_SETUP_AUTO=1` presses
                    // the main button once the sizes are in.
                    if self.start_when_measured
                        || std::env::var_os("RUSTCRAFT_SETUP_AUTO").is_some()
                    {
                        self.start_when_measured = false;
                        self.act();
                    }
                }
                // A copy needs no network: offer it, with the reason.
                Err(err) if self.use_copy && self.copy.is_some() => {
                    self.offline = Some(err.to_string());
                    self.phase = Phase::Ready;
                }
                Err(err) => {
                    self.start_when_measured = false;
                    self.phase = Phase::Failed(err.to_string());
                }
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
            Some(Ok(())) => self.finish_ok(self.marker_on_success),
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

    /// The bytes a download would fetch, when known.
    fn download_bytes(&self) -> Option<u64> {
        let vanilla = if self.needs.vanilla {
            self.plan.as_ref()?.download_bytes()
        } else {
            0
        };
        Some(vanilla + self.extras_bytes())
    }

    fn ready(&mut self, ui: &mut egui::Ui) {
        ui.label(
            "Rustcraft draws the world with Minecraft's own textures, sounds and font, and its \
             looks come from a few resource packs and mods. They aren't part of the game's \
             code, so this copy of Rustcraft gets them once.",
        );
        ui.add_space(6.0);
        let mut missing = Vec::new();
        if self.needs.vanilla {
            missing.push("Minecraft's files".to_owned());
        }
        if !self.needs.extras.is_empty() {
            let packs = self
                .needs
                .extras
                .iter()
                .filter(|e| e.kind == Kind::Pack)
                .count();
            let mods = self.needs.extras.len() - packs;
            missing.push(format!("{packs} packs and {mods} mods"));
        }
        ui.label(format!("Missing here: {}.", missing.join(" and ")));
        if self.cancelled {
            ui.colored_label(
                ui.visuals().warn_fg_color,
                "Stopped. Pressing the button again carries on where it left off.",
            );
        }
        ui.add_space(14.0);
        let width = ui.available_width().min(520.0);
        let big = |text: String| {
            egui::Button::new(egui::RichText::new(text).size(20.0))
                .min_size(egui::vec2(width, 56.0))
        };
        match (&self.copy, self.use_copy) {
            (Some(copy), true) => {
                let label = format!(
                    "Copy from this Mac ({}, {})",
                    megabytes(copy.bytes),
                    home_relative(&copy.assets)
                );
                if ui.add(big(label)).clicked() {
                    self.act();
                }
                ui.add_space(4.0);
                let text = match self.download_bytes() {
                    Some(n) => format!("Download instead ({})", megabytes(n)),
                    None => "Download instead".to_owned(),
                };
                if ui.small_button(text).clicked() {
                    self.use_copy = false;
                }
                if let Some(err) = &self.offline {
                    ui.colored_label(ui.visuals().warn_fg_color, format!("Download: {err}"));
                }
            }
            _ => {
                let label = match self.download_bytes() {
                    Some(n) => format!("Download game files ({})", megabytes(n)),
                    None => "Download game files".to_owned(),
                };
                if ui.add(big(label)).clicked() {
                    self.act();
                }
                if self.copy.is_some() && ui.small_button("Copy from this Mac instead").clicked() {
                    self.use_copy = true;
                }
            }
        }
        ui.add_space(6.0);
        if !self.needs.extras.is_empty() && ui.link("Only Minecraft's files").clicked() {
            self.with_packs = false;
            self.act();
        }
        ui.add_space(14.0);
        ui.horizontal(|ui| {
            if ui.button("Use a folder I already have…").clicked() {
                self.pick_folder();
            }
            if ui.button("Quit").clicked() {
                self.outcome = Some(Outcome::Quit);
            }
        });
        if !self.needs.extras.is_empty() {
            ui.add_space(10.0);
            egui::CollapsingHeader::new("What the packs and mods are").show(ui, |ui| {
                for extra in &self.needs.extras {
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
                self.act();
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

/// `path` with the home folder written as `~`.
fn home_relative(path: &std::path::Path) -> String {
    if let Some(home) = crate::platform::env::var_os("HOME")
        && let Ok(rest) = path.strip_prefix(&home)
    {
        return format!("~/{}", rest.display());
    }
    path.display().to_string()
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
